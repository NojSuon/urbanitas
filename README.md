# Urbanitas

A procedural architecture generator and builder written in Rust, by **Noj Suon**.

Urbanitas works at three scales:
- **Landscapes.** Generate terrain: flat, hills, mountains or a valley. Add a sea with a beach or cliffs, a lake, or a river.
- **Towns.** Click where the piazza goes. Streets grow outwards along the contours and fill with houses that face them. Each house stands on its own terraced pad.
- **Buildings.** Every house is generated from a few parameters (floors, size, type, rooms, garden, plan shape and style), down to its rooms, archways, windows, balconies, roof and garden.

Everything is made of **modular parts**. You can edit any house piece by piece in the app, Sims-style, save it as a reusable **design**, and let towns build with your designs.

- **The app** (`urbanitas-app`) shows it all in 3D under a Florentine-styled HUD.
- **The command line** (`urbanitas`) generates and exports:
  - **OBJ + MTL + PNG textures**: a 3D model for Blender, Godot, Unity, Unreal or any OBJ viewer
  - **SVG**: an architectural floor plan for every floor
  - **JSON**: a project file the app can open

The same parameters and seed always produce the same result.

## Styles

- **Florentine**: Renaissance Tuscan.
  - Ochre, sienna or cream plaster over a rusticated stone ground floor (*bugnato*)
  - Arched windows in pietra serena frames, with keystones and green shutters
  - A moulded cornice under deep tiled eaves
  - In the garden: arcaded loggias, fountains, cypresses and olive trees
- **Palermitan**: the palazzi of Palermo.
  - Tall floors, a piano nobile of balconies with curved *petto d'oca* railings
  - Stone quoins and pediments
  - Flat roofs with balustrades and *altane*
  - Arched *androne* passages into courtyards with lemon trees
- **Mediterranean**: whitewashed or ochre walls, round-arched openings, painted shutters, wrought-iron balconies, and a hipped terracotta roof or a flat roof terrace.

Plans can be a block, L, T, U, a courtyard (with an arcaded portico) or stepped.

Every surface uses a procedural texture generated in Rust, so there are no image assets and every colour palette gets matching textures. The textures include plaster, rusticated stone, roof tiles, cotto, shutters, basalt cobbles, paving slabs, dry-stone walling, grass, sand, rock and water.

## The app

```bash
cargo run --release -p urbanitas-app
```

The app opens on a hilly coast with a town already grown. The **icon rail** on the left opens panels that slide in over the view:

| Panel | What it does |
| --- | --- |
| **Landscape** | Terrain (flat, hills, mountains, valley), water (sea with a beach, cliffs or both; lake; river), map size and seed. It regenerates as you change it. Site presets: one building, a street of palazzi, villas, a farm. |
| **Town** | Style, number of houses, density, where houses come from (generated, your designs, or mixed) and the seed. **Place the piazza** shows a ghost piazza under the pointer (green where it fits). Click and the town grows round it. **Regrow** reshuffles; **Clear town** leaves the bare land. |
| **Build** | The parts catalogue (Buildings, Walls, Openings, Structure, Roofs, Garden, Streets) with pictures and hover cards. **Street with houses** draws a street that fills with houses on both sides. |
| **Designs** | Save the selected house as a design. Place copies from the library (R turns them). **Update all copies** pushes changes made to one copy into the design and every other copy. |
| **Colours** | Palette swatches and a colour picker for the selected house, or for the town. |
| **Settings** | Quality, the minimap, key reference. |

**Terrain bubbles.** A column of round buttons floats beside the panels: Raise, Lower, Flatten, Smooth, Sand and Water level. Hold the left button and paint. Dabs follow the cursor, and each stroke is one undo step. Houses, piazzas and terraces are left untouched. A popover sets the size and strength. Click the lit bubble to put the brush away.

**Houses are modules.** Shift+click a house to select it, and its card floats beside it:
- rename it, turn it, delete it
- recolour it, or save it as a design
- **Open house** edits its parts on their own, hiding the rest of the town

Toasts confirm what happened. The minimap in the corner shows the ground, water, streets and houses; click it to look somewhere else.

| Action | How |
| --- | --- |
| Draw walls, floors, roofs, lawns, pools, streets… | Click and drag |
| Place windows, doors, archways, balconies, gates | Point at a wall and click |
| Place trees, props, buildings, design copies | Click |
| Snap / free placement | 0.5 m grid; **hold Alt** for free placement and 15° turns |
| Turn | R (90°), Q / E (15° with Alt) |
| Select, move, delete | Click (Shift: whole house), drag, Delete |
| Undo / redo | Ctrl+Z / Ctrl+Y |
| Change level | PgUp / PgDn |
| See inside | Walls *Up*, *Cutaway* or *Down* |
| Camera | Right-drag orbit, middle-drag pan, scroll zoom |

**File.** *New plot*, *Save* and *Open…* use `./projects/`. Designs live in `./designs/` as `*.design.json`. *Export* writes OBJ, SVG or JSON to `./exports/<name>/` and includes your edits.

**Runs on low-end PCs.** *Quality: Low* turns off shadows and anti-aliasing, halves texture sizes and uses a coarser terrain mesh. The scene is drawn as a few merged meshes per floor, house and material. Only the floors you edit are rebuilt, and terrain strokes redraw only the ground. The status bar shows FPS, triangles and rebuild time.

App flags choose the starting scene and help with scripted screenshots:

```bash
cargo run --release -p urbanitas-app -- --landscape mountains --water lake --town 30 --seed 7
cargo run --release -p urbanitas-app -- --context urban --style palermitan --quality low
cargo run --release -p urbanitas-app -- --demo piazza --screenshot shot.png
```

## Command line

```bash
cargo run --release -- --style florentine --type villa --floors 2 --rooms 5 --width 16 --depth 12 --garden surrounding --seed 7
cargo run --release -- --landscape hills --water beach-and-cliffs --town 30 --seed 5
```

The first writes one building, the second a whole town on a landscape, into `output/`:
- an `.obj`, its `.mtl` and one PNG texture per material
- one `.svg` plan per floor
- a `.urbanitas.json` project file the app can open (copy it into `projects/`)

| Flag | Default | Meaning |
| --- | --- | --- |
| `--floors` | `2` | Number of floors (1–12) |
| `--width` / `--depth` | `12` / `10` | Footprint, in metres (5–60) |
| `--type` | `house` | `house`, `villa`, `apartment` or `shop` |
| `--rooms` | `4` | Rooms per floor, not counting the hall and stairs |
| `--garden` | `front` | `none`, `front`, `back` or `surrounding` |
| `--style` | `mediterranean` | `mediterranean`, `florentine` or `palermitan` |
| `--shape` | `auto` | `auto`, `block`, `l`, `t`, `u`, `courtyard` or `stepped` |
| `--context` | `single` | `single`, `urban`, `suburban` or `rural` site |
| `--buildings`, `--relief` | `6`, `0` | Buildings in a site, and its hills in metres |
| `--landscape` | | `flat`, `hills`, `mountains` or `valley`: grow a town on a landscape instead |
| `--water` | `beach` | `none`, `beach`, `cliffs`, `beach-and-cliffs`, `lake` or `river` |
| `--map-size` | `320` | Side of the landscape, in metres |
| `--town` | `24` | Houses in the town (0 for bare land) |
| `--seed` | random | Seed for the random generator |
| `--format` | `obj,svg,json` | Comma-separated output formats |
| `--texture-size` | `512` | PNG texture size for the OBJ |
| `--out`, `-o` | `output` | Output directory |
| `--name` | generated | Base name for the output files |
| `--params` | | Read the building parameters from a JSON file instead |

## How it works

1. **Landscape** (`landscape.rs`). Fractal noise makes the relief; ridged noise makes mountains. The sea takes the south edge: a sandy beach slopes into it, or the land drops off a cliff, and the coast can alternate between the two. A lake is carved into the lowest ground in the middle and ringed by a bank. A river meanders from north to south, always running downhill, in a channel with banks. Ground kinds (grass, earth, sand, rock, scree) come from height, slope and water.
2. **Town** (`town.rs`). The piazza is levelled where you click. Streets leave it breadth first in short straight runs, turning towards the gentlest climb and stopping at water, cliffs and the map edge. Each street is lined with houses as it's laid; every house is turned to face its street and stands on a levelled pad with a stone plinth. Pads keep their level a grid step past their edges, and where pads meet the ground takes the lowest, so it never pokes through a floor. House character depends on the plot:
   - tall attached palazzi near the piazza
   - houses with gardens further out
   - villas among olives on the edge
   - low beach houses by the sand, and villas on cliff tops by the sea
3. **Building** (`generate.rs`). Floors are split into rooms by weighted binary space partitioning, with a stair core and hall on every floor. Rooms are linked by archways along a spanning tree, so every room is reachable and none only through a bathroom. Then come the facade's doors, windows and balconies, the roof and the garden.
4. **Parts** (`parts.rs`). Everything becomes a `Layout`: a flat list of modular parts, grouped into houses (modules) with their own colours, plot and orientation. A layout is also the project file.
5. **Designs** (`design.rs`). A design is a house's parts relative to its plot, facing south. Copies are placed turned and moved and remember their design, so `update_copies` can rebuild them all.
6. **Mesh and texture.** `mesh::build::Scene` turns parts into geometry: walls meet at corner posts, cut holes for their openings, and gain the style's details. `texture::generate` paints a tileable texture with a mip chain for each material. The OBJ exporter and the app share the meshes.

The model uses metres, with `y` up, `x` running west → east and `z` running south → north. Houses face south (−z) until they are turned.

## Using it as a library

```rust
use urbanitas::{grow_town, landscape_layout, suggest_piazza, LandscapeParams, Relief, TownParams, WaterBody};
use urbanitas::export::obj;

let land = LandscapeParams { relief: Relief::Hills, water: WaterBody::Beach, size: 320.0, seed: 5 };
let mut layout = landscape_layout(&land, urbanitas::ArchitectureStyle::Florentine);
let plaza = suggest_piazza(&layout).unwrap();
let report = grow_town(&mut layout, plaza, &TownParams::default(), &[])?;
obj::write_obj(&layout, "town.obj".as_ref(), 512)?;
```

## Development

```bash
cargo test --workspace
```

The tests cover:
- **Buildings.** A sweep of generated buildings over every style, type, garden, shape and size checks that rooms tile each floor, every room is reachable, openings fit and never overlap, and stairs line up.
- **Landscapes.** They're deterministic. The sea floods the south edge (cliffs are steep, beaches sandy), the lake is enclosed, and the river runs downhill below its banks.
- **Towns** on every landscape:
  - the piazza is level, and streets stay dry
  - houses don't overlap each other or the streets, and stand on their pads
  - the seaside gets beach and cliff houses, and a drawn street fills with houses
- **Designs.** They survive a JSON round trip. A copy matches its design at every turn, updates reach every copy and nothing else, and towns arrange saved designs.
- **Meshes and exports.** Every part kind and every water surface builds a valid mesh. The OBJ, MTL, PNG and SVG exports are well formed.

New styles go in `src/style.rs`: add a variant to `ArchitectureStyle`, then its `StyleProfile` and palettes.

## License

MIT © 2026 Noj Suon
