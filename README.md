# Urbanitas

A procedural building generator written in Rust, by **Noj Suon**.

You describe a building with a few parameters: floors, size, type, rooms, garden and architectural style. Urbanitas generates a complete building from them, with floor plans, interior walls and doors, windows and balconies, a roof and a landscaped garden. It exports the result as:

- **OBJ + MTL**: a textured-by-colour 3D model for Blender, Godot, Unity, Unreal or any OBJ viewer
- **SVG**: an architectural floor plan for every floor
- **JSON**: the full building model, for use in your own tools

The same parameters and seed always produce the same building.

The first supported style is **Mediterranean**: thick whitewashed or ochre walls, round-arched windows and doors, painted shutters, iron balconies, and either a hipped terracotta roof or a flat roof terrace, set among cypresses, olive and lemon trees, pergolas and pools.

## Quick start

```bash
cargo run --release -- --seed 42
```

This writes `output/urbanitas_house_42.obj`, a matching `.mtl` file, one `.svg` plan per floor and a `.json` model.

To export only the 3D model:

```bash
cargo run --release -- --type villa --floors 2 --rooms 5 --width 16 --depth 12 --garden surrounding --format obj
```

## Parameters

| Flag | Default | Meaning |
| --- | --- | --- |
| `--floors` | `2` | Number of floors (1–12) |
| `--width` | `12` | Footprint width, west → east, in metres (5–60) |
| `--depth` | `10` | Footprint depth, front → back, in metres (5–60) |
| `--type` | `house` | `house`, `villa`, `apartment` or `shop` |
| `--rooms` | `4` | Rooms per floor, not counting the hall and stairs (1–24) |
| `--garden` | `front` | `none`, `front`, `back` or `surrounding` |
| `--style` | `mediterranean` | Architectural style |
| `--seed` | random | Seed for the random generator |
| `--format` | `obj,svg,json` | Comma-separated output formats |
| `--out`, `-o` | `output` | Output directory |
| `--name` | `urbanitas_<type>_<seed>` | Base name for the output files |
| `--params` | | Read every building parameter from a JSON file instead |

Each room needs at least 12 m² of floor area. Multi-storey buildings reserve a 2.6 m stair core on their west side, so they must be at least 5.6 m wide.

A parameters file uses the same names. Any field you leave out takes its default:

```json
{
  "seed": 7,
  "floors": 3,
  "size": { "width": 14.0, "depth": 11.0 },
  "building_type": "apartment",
  "rooms": 4,
  "garden": "back",
  "style": "mediterranean"
}
```

### Building types

- **house**: living room, kitchen, dining room and bathroom downstairs, bedrooms upstairs. A single-storey house gets bedrooms on the ground floor.
- **villa**: a larger programme with a lounge and guest rooms. It always gets a hipped tile roof and, when the back garden is deep enough, a pool.
- **apartment**: the same flat on every floor, a flat roof terrace and frequent balconies.
- **shop**: a shop with arched storefronts, storage and an office on the ground floor, and a flat above it with its own entrance.

## How it works

1. **Style.** The style profile sets proportions (floor height, wall thickness), opening sizes and shapes, and a colour palette drawn from the seed.
2. **Circulation.** Buildings with more than one floor get a stair well stacked on every floor, with a hall in front of it that takes the front door.
3. **Rooms.** Binary space partitioning splits the rest of each floor. Each split is weighted by the rooms on either side, so a living room ends up about three times the size of a bathroom.
4. **Doors.** Interior doors follow a spanning tree grown from the entrance. Every room is reachable, the hall always opens onto the stairs, and you never have to walk through a bathroom to reach another room.
5. **Facade.** The front door, arched windows with shutters, small windows for bathrooms and stairs, storefronts and balconies are placed along every outside wall, clear of corners and of each other.
6. **Garden.** The garden adds a path and potted plants at each entrance, a terrace, pergola and pool at the back when there is room, and trees around the edge of the lot.

The model uses metres, with `y` up, `x` running west → east and `z` running from the street front (south) to the back (north). Every part of the OBJ is a named group (`floor0_walls`, `floor1_glass`, `roof_tiles`, `garden_olive_12`, …), so you can select or hide parts after import.

## Using it as a library

```rust
use urbanitas::{generate, BuildingParams, BuildingType, Garden, Size};
use urbanitas::export::{obj, svg};

let params = BuildingParams {
    seed: 42,
    floors: 3,
    size: Size { width: 14.0, depth: 11.0 },
    building_type: BuildingType::Apartment,
    rooms: 4,
    garden: Garden::Back,
    ..Default::default()
};
let building = generate(&params)?;
obj::write_obj(&building, "apartment.obj".as_ref())?;
let plans: Vec<String> = svg::floor_plans(&building);
```

## Development

```bash
cargo test
```

The tests include a sweep of 2,688 generated buildings, covering every type, garden, floor count and room count. They check that rooms tile each floor exactly, every room is reachable, openings never overlap, stairs line up across floors and the exports are well formed.

New styles go in `src/style.rs`: add a variant to `ArchitectureStyle`, then its `StyleProfile` and palettes.

## License

MIT © 2026 Noj Suon
