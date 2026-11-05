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
claude mcp add lamina -- <path>/target/release/lamina mcp
```

## Licence

MIT OR Apache-2.0.
