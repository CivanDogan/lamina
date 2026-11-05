# Lamina

PCB layout for coding agents. A headless engine speaks MCP over stdio, the
board lives as plain JSON files in git, and a separate viewer redraws in 2D
and 3D whenever those files change.

Status: early development. See `docs/specs/` for the design.

## Layout

| Crate | Purpose |
|---|---|
| `lamina-model` | Data types, JSON schema, project load and save |
| `lamina-geom` | Polygon operations and spatial index |
| `lamina-footprint` | IPC-7351 footprint generator, datasheet footprints |
| `lamina-parts` | LCSC / JLCPCB / EasyEDA part search and import |
| `lamina-step` | STEP reader and tessellator, OBJ loader, mesh cache |
| `lamina-drc` | Design rules, fab presets, DRC, connectivity |
| `lamina-route` | Interactive routing, zones, grid router |
| `lamina-export` | Gerber X2, Excellon, BOM, CPL, board STEP |
| `lamina-scene` | Draw list shared by the renderers |
| `lamina-render` | PNG rendering, 2D and 3D |
| `lamina-build` | Jsonnet design sources, workspaces |
| `lamina-kicad` | KiCad footprint and board files in and out |
| `lamina-core` | Project operations, one function per tool |
| `lamina-mcp` | The `lamina` binary: MCP server and CLI |
| `lamina-viewer` | Live 2D/3D viewer and editor |

`vendor/jrsonnet-stdlib` is the Jsonnet standard library with YAML and
wasm-bindgen removed, because their pinned versions conflict with egui and wgpu.

## Build

```
cargo build --release
```

## Use with an MCP client

```
claude mcp add --scope user lamina -- <path>/target/release/lamina mcp
```

Codex: add `[mcp_servers.lamina]` with `command = "<path>/target/release/lamina"` and `args = ["mcp"]` to `~/.codex/config.toml`.

A typical session: `open` (create: true), `use_fab_preset`, `set_outline`,
`import_lcsc` for each part, `set_nets`, `place_many`, `autoroute`, `add_zone`
for ground, `check`, `render`, `export`. Read the user's comments with `notes`.

## Viewer

```
lamina-viewer <project folder>
```

The viewer watches the folder and redraws whenever files change, and it edits
the board the way KiCad does: click to select, drag to move, box-select,
`R` rotate, `F` flip, `L` lock, `M` move, `Del` delete, arrow keys nudge,
`X` route (with `V` for a via and layer change, `/` to flip the bend),
`V` via, `Z` pour, `K` keep-out, `O` outline, `N` grid, `Ctrl+Z` / `Ctrl+Y`
undo and redo. Right-drag or `Space`+drag pans, scroll zooms, `1` / `2`
switch 2D and 3D. The inspector edits the selected item's values. Edits go
through the same engine as the tools, so an agent can work on the board at
the same time; both sides re-read the files before each change. Notes typed
in the side panel are saved to `notes.json` for the agent.

## Modular boards

A board can be generated from `design.jsonnet` instead of hand-written JSON.
Jsonnet is JSON plus variables, functions and `import`, so parts, nets and
the board outline can come from reusable blocks:

```jsonnet
local lamina = import 'lamina.libsonnet';
local servo = import 'servo.libsonnet';
lamina.merge([
  { board: { name: 'demo', outline: lamina.rect(64, 52, 3), rules: { preset: 'jlcpcb-2layer' } } },
  servo.channel('J2', 'R8', { pwm: 'PWM0', out: 'SERVO1', vservo: 'VSERVO', gnd: 'GND' }),
  servo.channel('J3', 'R9', { pwm: 'PWM1', out: 'SERVO2', vservo: 'VSERVO', gnd: 'GND' }),
])
```

`lamina build <board>` writes `board.json`, `parts.json` and `nets.json`; the
MCP server and the viewer rebuild on their own when the source or an imported
file changes, and refuse direct edits to the generated files. Placement and
routing stay plain JSON and are edited as usual. `lamina.libsonnet` is built
in: `merge` joins fragments (parts must be unique, nets with the same name
join their pads), plus `net`, `pads`, `rect`, `hole` and `corner_holes`.

Several boards share blocks through a workspace: `lamina workspace <dir>`
writes `lamina.json` with `boards` and `lib` folders. Each lib folder is an
import path and may hold shared `footprints/` and `models/`.
`lamina new <dir> --modular` starts a board from a source template.

## KiCad files

Lamina reads and writes KiCad's formats without running KiCad:

- `import_kicad_footprint` takes `Library:Name` (for example
  `Package_TO_SOT_SMD:SOT-23`), a footprint name from a local library, or a
  `.kicad_mod` file. Local libraries come from `kicad_footprints` in
  `lamina.json`, `KICAD*_FOOTPRINT_DIR` or a standard install; otherwise the
  footprint and its STEP model are fetched from the official KiCad libraries.
  `search_kicad_footprints` searches the local ones.
- `import_kicad_board` / `lamina import-kicad <file> <dir>` read a
  `.kicad_pcb` (KiCad 6 to 10): outline, footprints, parts, nets, placement,
  tracks, vias, zones, keep-outs and texts.
- `export_kicad` / `lamina export-kicad <dir>` write a `.kicad_pcb` that KiCad
  8 or newer opens; KiCad refills the zones.

## EasyEDA boards

`import_easyeda_board` / `lamina import-easyeda <file.json> <dir>` read a board
saved from the standard EasyEDA editor with File → Export → EasyEDA: outline,
parts with their LCSC numbers, nets, placement, tracks, vias, copper areas,
holes and texts. 3D models are fetched from EasyEDA like LCSC imports. Solid
copper regions (teardrops, hand-drawn copper) are kept as drawings. Rules are
relaxed to what the board contains; apply a fab preset afterwards.

## Gerbers from any CAD program

`import_gerbers` / `lamina import-gerbers <folder or zip> <dir>` read Gerber
(RS-274X with X2 attributes, aperture macros, arcs, regions, step and repeat)
and Excellon drill files, whatever program wrote them. Layers are recognised
by their X2 file function or by common names (KiCad, Altium/Protel, EasyEDA,
Eagle). Fab files have no parts or nets, so the board comes in as artwork:
outline, holes, and copper and silkscreen as drawings, for viewing, rendering
and checking against the original.

## STEP for mechanical CAD

`export_step` / `lamina export-step <dir>` write the assembled board as an
AP214 STEP file: the board as a solid (outline, cut-outs, drilled holes;
`vias` adds via holes) and every part placed as an assembly component.
Part STEP files are copied in whole, so their exact surfaces, colours and
names come through; OBJ-only models go in as faceted surfaces. The file opens
in Fusion, FreeCAD, SolidWorks and other CAD programs for enclosure work.

## External autorouters

`export_specctra` / `lamina export-dsn <dir>` write a Specctra DSN of the
board, and `import_specctra_session` / `lamina import-ses <file.ses> <dir>`
read the session back. With [Freerouting](https://github.com/freerouting/freerouting):

```
lamina export-dsn boards/servo
java -jar freerouting.jar -de boards/servo/output/esp32-servo.dsn -do boards/servo/output/esp32-servo.ses
lamina import-ses boards/servo/output/esp32-servo.ses boards/servo
```

Existing tracks are exported as protected wiring unless `reroute_all`.

## Tools

`lamina-export` has `outline <file.gbr> [tolerance]`, which prints the closed
loops found in a Gerber outline layer. `lamina-step` has two developer examples: `corpus <dir>` loads every STEP file in
a folder and compares it with the OBJ of the same name, and `lamina-render`'s
`meshview <model> <out.png>` renders a single model.

## Licence

MIT OR Apache-2.0.
