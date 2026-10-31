# Urbanitas

A procedural building generator and builder written in Rust, by **Noj Suon**.

You describe a building with a few parameters: floors, size, type, rooms, garden and architectural style. Urbanitas generates a complete building from them, with floor plans, interior walls and archways, windows, doors and balconies, a roof and a landscaped garden. The building is made of **modular parts**, so you can then edit it piece by piece in the app, Sims-style, or build one from scratch.

- **The app** (`urbanitas-app`) shows the building in 3D. You change parameters in a Florentine-styled HUD and watch it regenerate live, or switch to build mode to place, move, rotate and delete parts.
- **The command line** (`urbanitas`) generates buildings and exports:
  - **OBJ + MTL + PNG textures**: a 3D model for Blender, Godot, Unity, Unreal or any OBJ viewer
  - **SVG**: an architectural floor plan for every floor
  - **JSON**: a project file the app can open

The same parameters and seed always produce the same building.

## Styles

- **Florentine**: Renaissance Tuscan.
  - Ochre, sienna or cream plaster over a rusticated stone ground floor (*bugnato*)
  - Stone string courses at each floor line
  - Arched windows in pietra serena frames, with keystones and green shutters
  - A moulded stone cornice under deep tiled eaves with exposed rafters
  - Turned stone balustrades
  - In the garden: arcaded loggias, fountains, cypresses and olive trees
- **Mediterranean**: whitewashed or ochre walls, round-arched windows and doors, painted shutters, wrought-iron balconies, and either a hipped terracotta roof or a flat roof terrace, with pergolas and pools.

Every surface is textured with procedural textures generated in Rust, so there are no image assets to ship and every colour palette gets matching textures. The textures are plaster, rusticated stone, coppi roof tiles, cotto floors, louvred shutters, plank doors, gravel, grass, water and foliage. The geometry is low-poly but detailed:
- true round arches with curved reveals
- stone archivolts and keystones
- extruded cornice profiles
- lathe-turned columns, balusters, pots and fountains
- faceted tree canopies

## The app

```bash
cargo run --release -p urbanitas-app
```

**Generate mode.** Sliders and dropdowns for style, building type, floors, width, depth, rooms per floor, garden and seed. Every change regenerates the building.

**Build mode** (*Edit part by part*):

| Action | How |
| --- | --- |
| Pick a part | Catalogue: Walls, Openings, Structure, Roofs, Garden |
| Draw walls, floors, roofs, lawns, pools… | Click and drag |
| Place windows, doors, archways and balconies | Point at a wall and click; the wall cuts the opening |
| Place trees, pots, fountains, columns, stairs… | Click |
| Snap / free placement | Snaps to a 0.5 m grid; **hold Alt** for free placement and 15° turns |
| Rotate | R (90°), Q / E (15° with Alt) |
| Select, move, delete | Click, drag, Delete |
| Undo / redo | Ctrl+Z / Ctrl+Y |
| Change level | PgUp / PgDn, or the level arrows |
| See inside | Walls *Up*, *Cutaway* (walls facing the camera drop to waist height) or *Down* |
| Camera | Right-drag orbit, middle-drag pan, scroll zoom |

The inspector edits the selected part: wall height and thickness, opening size and sill, prop scale and height, roof overhang, and so on. Placements show a green ghost when they fit and a red one when they don't.

**File.** *New plot* starts from an empty lawn. *Save* and *Open…* use `./projects/`. *Export* writes OBJ, SVG or JSON to `./exports/<name>/` and includes your edits.

**Runs on low-end PCs.** Set *Quality* to *Low* to turn off shadows and anti-aliasing and use smaller textures. The building is drawn as a few merged meshes per floor and material. Only the floors you edit are rebuilt. Textures are shared between buildings and have mipmaps. The status bar shows FPS, the triangle count and how long the last rebuild took.

App flags set the starting building and help with scripted screenshots:

```bash
cargo run --release -p urbanitas-app -- --style florentine --type villa --floors 3 --seed 7 --quality low
cargo run --release -p urbanitas-app -- --demo build --screenshot shot.png
```

## Command line

```bash
cargo run --release -- --style florentine --type villa --floors 2 --rooms 5 --width 16 --depth 12 --garden surrounding --seed 7
```

This writes into `output/`:
- `urbanitas_villa_7.obj`, its `.mtl` and one PNG texture per material
- one `.svg` plan per floor
- `urbanitas_villa_7.urbanitas.json`, which the app can open: copy it into `projects/`

| Flag | Default | Meaning |
| --- | --- | --- |
| `--floors` | `2` | Number of floors (1–12) |
| `--width` | `12` | Footprint width, west → east, in metres (5–60) |
| `--depth` | `10` | Footprint depth, front → back, in metres (5–60) |
| `--type` | `house` | `house`, `villa`, `apartment` or `shop` |
| `--rooms` | `4` | Rooms per floor, not counting the hall and stairs (1–24) |
| `--garden` | `front` | `none`, `front`, `back` or `surrounding` |
| `--style` | `mediterranean` | `mediterranean` or `florentine` |
| `--seed` | random | Seed for the random generator |
| `--format` | `obj,svg,json` | Comma-separated output formats |
| `--texture-size` | `512` | PNG texture size for the OBJ |
| `--out`, `-o` | `output` | Output directory |
| `--name` | `urbanitas_<type>_<seed>` | Base name for the output files |
| `--params` | | Read every building parameter from a JSON file instead |

Each room needs at least 12 m² of floor area. Multi-storey buildings reserve a 2.6 m stair core on their west side, so they must be at least 5.6 m wide. A parameters file uses the same names as the flags, and any field you leave out takes its default.

### Building types

- **house**: living room, kitchen, dining room and bathroom downstairs, bedrooms upstairs.
- **villa**: a larger programme with a lounge and guest rooms, a hipped tile roof and, when the back garden is deep enough, a pool.
- **apartment**: the same flat on every floor, with frequent balconies.
- **shop**: a shop with arched storefronts on the ground floor and a flat above it with its own entrance.

## How it works

1. **Generate.** The generator splits each floor into rooms with weighted binary space partitioning, stacking a stair core and hall on every floor. It links the rooms with archways along a spanning tree (every room reachable, none through a bathroom), places the facade's doors, windows and balconies, chooses a roof and lays out the garden.
2. **Parts.** The building becomes a `Layout`, a flat list of modular parts: walls, openings mounted on walls, floors, stairs, roofs, garden areas and props. Hand-built and generated buildings are the same data, and a layout is also the project file.
3. **Mesh.** `mesh::build::Scene` turns parts into geometry:
   - walls meet at corner posts and stop at the faces of walls they run into
   - each wall cuts holes for the openings mounted on it
   - the style adds its details
   - the OBJ exporter and the app use the same meshes.
4. **Texture.** `texture::generate` paints a tileable texture with a mip chain for each material, tinted with the building's palette.

The model uses metres, with `y` up, `x` running west → east and `z` running from the street front (south) to the back (north).

## Using it as a library

```rust
use urbanitas::{generate, BuildingParams, ArchitectureStyle};
use urbanitas::export::{obj, svg};
use urbanitas::parts::{PartKind, PropKind, PropPart};

let building = generate(&BuildingParams { style: ArchitectureStyle::Florentine, ..Default::default() })?;
let mut layout = building.to_layout();
layout.add(0, PartKind::Prop(PropPart { kind: PropKind::Fountain, at: [6.0, -4.0], yaw: 0.0, scale: 1.0, height: 1.5 }));
obj::write_obj(&layout, "villa.obj".as_ref(), 512)?;
let plans: Vec<String> = svg::floor_plans(&layout);
```

## Development

```bash
cargo test --workspace
```

The tests include a sweep of 5,376 generated buildings, covering every style, type, garden, floor count and room count. They check that:
- rooms tile each floor exactly
- every room is reachable
- openings never overlap
- stairs line up across floors

They also check the meshes and exports:
- every part kind builds a valid mesh, with in-range indices, unit normals and one UV per vertex
- windows cut holes in their walls
- layouts survive a JSON round trip
- the OBJ, MTL, PNG and SVG exports are well formed

New styles go in `src/style.rs`: add a variant to `ArchitectureStyle`, then its `StyleProfile` and palettes.

## License

MIT © 2026 Noj Suon
