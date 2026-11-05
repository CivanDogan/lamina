# Lamina design

Status: draft, 2026-11-05.

## 1. Intent

Lamina is a PCB layout program written in Rust whose primary user is a
coding agent (Claude Code, Codex or any MCP client), with a human
watching and commenting. It depends on no other EDA tool. It must work
with no window open, and when the viewer is open it must show every
change the agent makes the moment it happens, in 2D and 3D.

The existing tools either bolt an assistant onto a GUI built for a mouse
(Flux, EasyEDA), expose a GUI tool's file format through a thin MCP
wrapper (the KiCad MCP servers), or invent a language the agent has to
learn each session (atopile). None give the agent a clean board model,
tools whose results it can verify, and a fast way to see what it did.

## 2. Goals, non-goals, anti-goals

Goals

- An agent can take a board from parts list and netlist to fab-ready
  Gerbers through MCP tools alone, and verify each step.
- The project is plain files in git. Diffs are readable.
- A native viewer shows the live board in 2D and 3D, redrawing on every
  change, and runs or not without affecting the agent.
- No bundled parts library. Parts, footprints and 3D models are pulled
  on demand and cached in the project.
- Fast and light: a dense 4-layer board renders at 60 fps, memory stays
  low, the PC does not freeze.
- First real output: the MeraTag collar board (70 x 40 mm, 4 layers,
  about 15 parts) ordered from JLCPCB with assembly.

Non-goals (not in version one)

- Schematic editor or schematic drawing. The netlist comes from a text
  description or a file.
- Human drag-and-drop editing in the viewer. Version one is watch and
  comment. Editing is a later phase.
- Simulation (SPICE, signal integrity, thermal).
- Importing designs from Altium, Eagle, KiCad or EasyEDA.
- Multi-user or concurrent agents on one project.

Anti-goals (would make it a failure even if it works)

- Any dependency on KiCad, its libraries or its CLI.
- A daemon the agent must find running. If a window or server has to be
  up for the agent to work, the design has failed.
- A new language. The agent speaks JSON and tool calls, nothing to learn.
- A footprint that is wrong because it was guessed. Every footprint
  traces to a datasheet drawing or an IPC-7351 formula with the inputs
  recorded.

## 3. Constraints

- Rust throughout. One Cargo workspace.
- MCP over stdio. One agent process per session, launched by the client.
- Windows 11 first (the development machine), macOS second. Linux should
  work but is not tested in version one.
- No AI attribution anywhere in the repository (commits, docs, code).
- Keep system load low during builds and runs: no parallel heavy jobs.
- The crate name `lamina` on crates.io is taken (a compiler backend).
  Published crates, if ever, use `lamina-core`, `lamina-mcp`,
  `lamina-viewer`, all free as of 2026-11-05. The binaries are still
  called `lamina` and `lamina-viewer`.

## 4. Approaches considered

1. One native app with everything inside, including the MCP server.
   Simplest, but the agent can only work while the window is open and a
   viewer crash kills the session. Rejected: violates an anti-goal.
2. Core library, headless engine speaking MCP over stdio, separate
   viewer process watching the project files. Agent and viewer share
   nothing but the files on disk. Chosen.
3. Same as 2, but a long-running localhost HTTP daemon instead of
   stdio, so several clients share one in-memory engine. Rejected for
   version one: the state already lives in files, a daemon runs stale
   code after every rebuild during development, needs lifecycle
   management and a localhost token, and the user runs one agent at a
   time. HTTP transport can be added later without changing the core.

Transport note: stdio and localhost HTTP are both far faster than any
DRC or render, so speed did not decide this.

## 5. Decisions

### 5.1 Workspace layout

- `lamina-core` (library): data model, units, geometry, footprint
  generator, parts fetching and cache, DRC, renderer to raster,
  exporters, project load/save. Everything testable without a window.
- `lamina-mcp` (binary `lamina`): stdio MCP server over `rmcp`, plus the
  same operations as CLI subcommands (`lamina check`, `lamina render`,
  `lamina export`) so humans and CI can use it too.
- `lamina-viewer` (binary): egui + wgpu window. Watches the project
  folder with `notify`, reloads, redraws 2D and 3D. Read-only in
  version one. Links `lamina-core` for parsing and tessellation; no
  protocol between engine and viewer, the files are the protocol.

### 5.2 Project on disk

A project is a folder, in git:

```
board.json        outline, stackup, layers, design rules, fab preset
parts.json        refdes -> part (MPN, LCSC id, footprint id, value, 3D ref)
nets.json         net name -> list of (refdes, pad)
placement.json    refdes -> (x, y, rotation, side)
routing.json      traces, vias, zones (pours), keep-outs, text, graphics
cache/footprints/ generated or datasheet-derived footprints, one JSON each,
                  with provenance (formula + inputs, or datasheet page)
cache/parts/      distributor data as fetched
cache/models/     STEP as fetched, plus tessellated mesh (binary) per model
cache/render/     PNG snapshots the agent asked for
```

All geometry is in millimetres as f64, origin at the board outline's
bounding-box lower-left, Y up, rotation counter-clockwise in degrees.
Every file has a `schema` version field. The JSON schema for each file
is published in `lamina-core` so an agent can read it with one tool call.

Saving is atomic (write temp, rename) so the viewer never reads a half
file. A `.lamina.lock` file in the project folder stops a second engine
from opening the same project; this is a safety net, not a feature.

### 5.3 MCP tool surface

Small, strictly typed, each returns a result the agent can check.
Sketch, final names to the builder:

- project: `open`, `status`, `schema`
- board: `set_outline`, `set_stackup`, `set_rules` (or `use_fab_preset`)
- parts: `search_part` (LCSC and manufacturer), `add_part`,
  `remove_part`, `footprint_from_package` (IPC-7351),
  `footprint_from_pads` (datasheet numbers typed in), `fetch_model`
- nets: `set_nets` (whole netlist), `connect`, `disconnect`
- placement: `place`, `move`, `rotate`, `flip`, `align`, `distribute`
- routing: `route_trace` (explicit points), `add_via`, `add_zone`,
  `add_keepout`, `autoroute` (see 5.7), `ripup`
- checks: `drc`, `connectivity` (unrouted nets, shorts), `measure`
- views: `render` (PNG of a layer set or 3D view, returned as image),
  `describe` (text summary of the board or a region)
- output: `export_gerbers`, `export_drill`, `export_bom`, `export_pnp`,
  `export_step`

Rules for every tool: validate input before touching the project,
return what changed and any rule now violated, never silently succeed.
`drc` and `connectivity` are cheap enough to run after every mutation and
their result is included in the mutation's reply.

### 5.4 Parts and footprints

No library ships with Lamina. Sources, in order of preference:

1. LCSC / JLCPCB public API: part data, stock, price, package name, and
   the EasyEDA footprint and 3D model JSON when present. This matters
   because JLCPCB assembly only stocks LCSC parts.
2. Manufacturer: STEP model and datasheet land pattern. The agent reads
   the datasheet and calls `footprint_from_pads` with the numbers; the
   footprint stores the datasheet name and page.
3. IPC-7351 generator for standard packages (chip 0402 to 2512, SOT,
   SOIC, QFN, LQFP, BGA, LGA grids): package dimensions in, pads,
   paste, mask, silkscreen and courtyard out, at a chosen density level.
   The footprint stores the formula inputs.
4. Distributor APIs (Mouser, Digi-Key, Nexar) for parameters and pricing
   when LCSC does not have the part. Keys live in the user's config, not
   the project.

Not used: SnapEDA, Ultra Librarian, SamacSys (redistribution forbidden),
KiCad libraries.

3D models: STEP is the stored source. On first use it is tessellated
with the `truck` crate into a binary mesh cached beside it. The viewer
and the 3D renderer load meshes, never STEP. A part with no model gets a
grey box from its courtyard and the package height.

### 5.5 Geometry and DRC

Polygon work (clearance, pours, union, offset) with `geo` and
`i_overlay`. Copper pours are computed geometry stored as outlines plus
the fill result, recomputed on demand. DRC checks in version one:
clearance per layer and net class, trace width, via drill and annular
ring, hole-to-hole and hole-to-copper, courtyard overlap, copper in
keep-outs, copper to board edge, mask and paste sanity, unconnected
pads, shorts. Rules come from a fab preset (JLCPCB 2-layer and 4-layer
in version one) and can be overridden per board and per net class.

### 5.6 Rendering

Two renderers share the same scene-building code in `lamina-core`:

- Raster to PNG for the agent: any layer set, any region, with optional
  overlays (DRC markers, net highlight, ratsnest). This is how the agent
  sees its work from day one.
- The viewer draws the same scene with wgpu, 2D layers with per-layer
  toggles and 3D with the board stack and part meshes. Hot reload: the
  file watcher fires, the changed file is reparsed, only the affected
  GPU buffers are rebuilt.

### 5.7 Routing

Version one has two paths. The agent routes by hand with `route_trace`,
giving points and a layer, with the engine snapping to pads and
refusing anything that violates DRC. And a built-in grid router (A* or
Lee style, two to four layers, respects rules and keep-outs) for the
easy nets. Freerouting as an external subprocess is allowed as an
option behind a flag but is not required, since the user does not want
Java or other EDA tooling on the machine. A real native topological
router is a later project.

### 5.8 Exports

Written natively in Rust: Gerber X2 (RS-274X with attributes), Excellon
drill with plated and non-plated files and slots, JLCPCB-format BOM and
pick-and-place CSV, a STEP of the bare board outline (full assembly STEP
is later). Tested by round-tripping through an independent Gerber
viewer and by one real JLCPCB order.

### 5.9 Order of work

1. Core model, project files, schema, load and save, `lamina` CLI.
2. IPC-7351 footprint generator and `footprint_from_pads`; LCSC fetch;
   STEP fetch and tessellation.
3. Raster renderer and `render` tool. From here the agent can see.
4. Placement tools, DRC, connectivity.
5. Hand routing tools, zones, keep-outs, grid router.
6. Exporters. Order a test coupon board from JLCPCB to validate output.
7. Viewer: 2D with hot reload, then 3D.
8. MeraTag collar board end to end, ordered with assembly.

MCP wraps each core operation as soon as it exists, so the agent can use
Lamina to build Lamina's own test boards from step 3 onward.

### 5.10 First target

MeraTag collar: 70 x 40 mm, 1.6 mm FR-4, 4 layers proposed, about 15
parts including a 47-pad LGA radio module with antenna keep-outs, an
LGA-14 IMU, 0402 to 1206 passives, a 2 x 5 through-hole header, a
soldered AA cell on plated slots and NPTH cable-tie slots, test pads and
fiducials. Two parts are not at LCSC and come from manufacturer
drawings. Spec: `Desktop\collar_tag\hardware\PCB-SPEC.md`. Everything in
that spec must be expressible in Lamina's model: slots, NPTH, keep-outs
on all layers, per-net widths, mask and paste overrides, fiducials.

## 6. Left to the builder

- Exact JSON schemas, within the file split above.
- Tool names and argument shapes, within the sketch above.
- Choice of tessellation tolerance, mesh format, PNG resolution defaults.
- Grid router algorithm details and limits.
- How the viewer picks which project to watch (argument, last opened).
- Test strategy, but every geometry and export function needs tests, and
  Gerber output needs a golden-file comparison.

## 7. Open

- Licence for the repository. Proposed: MIT or Apache-2.0, as the user
  prefers. Not decided.
- Whether to publish on crates.io at all.
