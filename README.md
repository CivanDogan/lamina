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
| `lamina-core` | Project operations, one function per tool |
| `lamina-mcp` | The `lamina` binary: MCP server and CLI |
| `lamina-viewer` | Live 2D/3D viewer |

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

The viewer watches the folder and redraws whenever files change. Keys: `1` 2D,
`2` 3D, `F` fit. 2D: scroll to zoom, drag to pan, click to pin a part or net.
3D: drag to orbit, right-drag to pan. Notes typed in the side panel are saved to
`notes.json` for the agent.

## Tools

`lamina-step` has two developer examples: `corpus <dir>` loads every STEP file in
a folder and compares it with the OBJ of the same name, and `lamina-render`'s
`meshview <model> <out.png>` renders a single model.

## Licence

MIT OR Apache-2.0.
