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

## Tools

`lamina-step` has two developer examples: `corpus <dir>` loads every STEP file in
a folder and compares it with the OBJ of the same name, and `lamina-render`'s
`meshview <model> <out.png>` renders a single model.

## Licence

MIT OR Apache-2.0.
