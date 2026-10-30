use std::collections::{HashSet, VecDeque};

use urbanitas::export::{json, obj, svg};
use urbanitas::geometry::{EPS, Side};
use urbanitas::model::{Floor, OpeningKind, RoomKind};
use urbanitas::{Building, BuildingParams, BuildingType, Garden, ParamError, Size, generate};

const TYPES: [BuildingType; 4] = [
    BuildingType::House,
    BuildingType::Villa,
    BuildingType::Apartment,
    BuildingType::Shop,
];
const GARDENS: [Garden; 4] = [
    Garden::None,
    Garden::Front,
    Garden::Back,
    Garden::Surrounding,
];

fn params(seed: u64) -> BuildingParams {
    BuildingParams {
        seed,
        ..BuildingParams::default()
    }
}

/// Every combination of type, garden, floor count and room count, over a few seeds.
fn sweep() -> impl Iterator<Item = BuildingParams> {
    TYPES.into_iter().flat_map(|building_type| {
        GARDENS.into_iter().flat_map(move |garden| {
            (1..=4).flat_map(move |floors| {
                (1..=7).flat_map(move |rooms| {
                    (0..6).map(move |seed| BuildingParams {
                        seed,
                        floors,
                        size: Size {
                            width: 14.0,
                            depth: 11.0,
                        },
                        building_type,
                        rooms,
                        garden,
                        ..BuildingParams::default()
                    })
                })
            })
        })
    })
}

fn assert_rooms_tile_footprint(b: &Building, floor: &Floor) {
    let fp = b.footprint;
    let area: f64 = floor.rooms.iter().map(|r| r.rect.area()).sum();
    assert!(
        (area - fp.area()).abs() < 1e-6,
        "rooms cover {area} of {} m²",
        fp.area()
    );
    for (i, a) in floor.rooms.iter().enumerate() {
        assert!(fp.contains(&a.rect), "room {i} leaves the footprint");
        assert!(
            a.rect.min_side() > 1.2,
            "room {i} is a sliver: {:?}",
            a.rect
        );
        for other in &floor.rooms[i + 1..] {
            assert!(
                !a.rect.intersects(&other.rect),
                "rooms overlap: {:?} {:?}",
                a.rect,
                other.rect
            );
        }
    }
}

fn assert_all_rooms_reachable(floor: &Floor) {
    let mut seen = HashSet::from([0usize]);
    let mut queue = VecDeque::from([0usize]);
    while let Some(room) = queue.pop_front() {
        for wall in floor
            .walls
            .iter()
            .filter(|w| w.door.is_some() && w.rooms.contains(&room))
        {
            let next = if wall.rooms[0] == room {
                wall.rooms[1]
            } else {
                wall.rooms[0]
            };
            if seen.insert(next) {
                queue.push_back(next);
            }
        }
    }
    assert_eq!(
        seen.len(),
        floor.rooms.len(),
        "{} has unreachable rooms",
        floor.name()
    );
}

fn assert_doors_clear_of_outer_walls(b: &Building, floor: &Floor) {
    for wall in &floor.walls {
        let Some(door) = wall.door else { continue };
        let clear = wall
            .segment
            .inside_walls(&b.footprint, b.style.wall_thickness);
        assert!(
            door.center - door.width / 2.0 >= clear.start - EPS
                && door.center + door.width / 2.0 <= clear.end + EPS,
            "door runs into an exterior wall: {door:?} on {clear:?}"
        );
    }
}

fn assert_openings_fit(b: &Building, floor: &Floor) {
    for side in Side::ALL {
        let mut spans: Vec<(f64, f64)> = floor
            .openings
            .iter()
            .filter(|o| o.side == side)
            .map(|o| (o.start(), o.end()))
            .collect();
        spans.sort_by(|a, b| a.0.total_cmp(&b.0));
        for pair in spans.windows(2) {
            assert!(
                pair[0].1 <= pair[1].0 + EPS,
                "openings overlap on {side:?}: {pair:?}"
            );
        }
    }
    for o in &floor.openings {
        let room = &floor.rooms[o.room];
        let (a, b_) = room
            .rect
            .span_on_side(o.side, &b.footprint)
            .expect("opening on an outside wall");
        assert!(
            o.start() >= a - EPS && o.end() <= b_ + EPS,
            "opening leaves its room"
        );
        assert!(
            o.sill + o.height < floor.height,
            "opening taller than the floor"
        );
    }
}

#[test]
fn default_building_generates() {
    let b = generate(&BuildingParams::default()).unwrap();
    assert_eq!(b.floors.len(), 2);
    // 4 rooms per floor plus the hall and stairs.
    assert!(b.floors.iter().all(|f| f.rooms.len() == 6));
    assert!(b.garden.is_some());
    assert!(b.height() > 2.0 * b.style.floor_height);
}

#[test]
fn same_seed_same_building() {
    let a = json::to_json(&generate(&params(42)).unwrap());
    let b = json::to_json(&generate(&params(42)).unwrap());
    assert_eq!(a, b);
    let c = json::to_json(&generate(&params(43)).unwrap());
    assert_ne!(a, c);
}

#[test]
fn sweep_holds_invariants() {
    let mut checked = 0;
    for p in sweep() {
        let b = generate(&p).unwrap_or_else(|e| panic!("{p:?} failed: {e}"));
        assert_eq!(b.floors.len(), p.floors as usize);
        let circulation = if p.floors > 1 { 2 } else { 0 };
        for floor in &b.floors {
            assert_eq!(floor.rooms.len(), p.rooms as usize + circulation, "{p:?}");
            assert_rooms_tile_footprint(&b, floor);
            assert_all_rooms_reachable(floor);
            assert_doors_clear_of_outer_walls(&b, floor);
            assert_openings_fit(&b, floor);
        }
        let ground = &b.floors[0];
        assert!(
            ground
                .openings
                .iter()
                .any(|o| o.kind == OpeningKind::Door && o.side == Side::South),
            "{p:?} has no front door"
        );
        if p.floors > 1 {
            let stairs = b.stairs().expect("stairs");
            for floor in &b.floors {
                assert_eq!(floor.find(RoomKind::Stairs).map(|r| r.rect), Some(stairs));
            }
        } else {
            assert!(b.stairs().is_none());
        }
        assert_eq!(b.garden.is_some(), p.garden != Garden::None);
        if let Some(garden) = &b.garden {
            assert!(garden.lot.contains(&b.footprint));
            for f in &garden.features {
                assert!(garden.lot.contains(&f.rect), "{:?} outside the lot", f.kind);
                if f.kind.is_tree() {
                    assert!(!f.rect.intersects(&b.footprint), "tree inside the house");
                }
            }
        }
        checked += 1;
    }
    assert_eq!(checked, 4 * 4 * 4 * 7 * 6);
}

#[test]
fn rooms_get_sensible_uses() {
    let b = generate(&BuildingParams {
        rooms: 5,
        ..params(7)
    })
    .unwrap();
    let ground = &b.floors[0];
    let living = ground
        .find(RoomKind::Living)
        .expect("living room downstairs");
    let bath = ground
        .find(RoomKind::Bathroom)
        .expect("bathroom downstairs");
    assert!(living.area > bath.area);
    assert!(b.floors[1].find(RoomKind::Bedroom).is_some());

    let shop = generate(&BuildingParams {
        building_type: BuildingType::Shop,
        ..params(3)
    })
    .unwrap();
    let ground = &shop.floors[0];
    assert!(ground.find(RoomKind::Retail).is_some());
}

#[test]
fn invalid_parameters_are_rejected() {
    let reject = |p: BuildingParams| generate(&p).unwrap_err();
    assert!(matches!(
        reject(BuildingParams {
            floors: 0,
            ..params(1)
        }),
        ParamError::Floors(0)
    ));
    assert!(matches!(
        reject(BuildingParams {
            rooms: 0,
            ..params(1)
        }),
        ParamError::Rooms(0)
    ));
    assert!(matches!(
        reject(BuildingParams {
            size: Size {
                width: 3.0,
                depth: 10.0
            },
            ..params(1)
        }),
        ParamError::Size {
            dimension: "width",
            ..
        }
    ));
    assert!(matches!(
        reject(BuildingParams {
            size: Size {
                width: 12.0,
                depth: f64::NAN
            },
            ..params(1)
        }),
        ParamError::Size {
            dimension: "depth",
            ..
        }
    ));
    assert!(matches!(
        reject(BuildingParams {
            rooms: 20,
            ..params(1)
        }),
        ParamError::TooCrowded { .. }
    ));
    assert!(matches!(
        reject(BuildingParams {
            size: Size {
                width: 5.0,
                depth: 10.0
            },
            ..params(1)
        }),
        ParamError::NoRoomForStairs { .. }
    ));
}

#[test]
fn obj_export_is_well_formed() {
    for p in [
        params(1),
        BuildingParams {
            building_type: BuildingType::Villa,
            garden: Garden::Surrounding,
            floors: 3,
            ..params(9)
        },
    ] {
        let b = generate(&p).unwrap();
        let export = obj::to_obj(&b, "test.mtl");
        let vertices = export.obj.lines().filter(|l| l.starts_with("v ")).count();
        assert!(vertices > 100);
        let mut faces = 0;
        for line in export.obj.lines().filter(|l| l.starts_with("f ")) {
            let indices: Vec<usize> = line[2..].split(' ').map(|i| i.parse().unwrap()).collect();
            assert!(indices.len() >= 3);
            assert!(
                indices.iter().all(|&i| i >= 1 && i <= vertices),
                "bad face: {line}"
            );
            faces += 1;
        }
        assert!(faces > 100);
        assert!(export.obj.contains("mtllib test.mtl"));
        for material in export.obj.lines().filter_map(|l| l.strip_prefix("usemtl ")) {
            assert!(
                export.mtl.contains(&format!("newmtl {material}\n")),
                "missing material {material}"
            );
        }
        for group in [
            "floor0_walls",
            "floor0_glass",
            "roof_slab",
            "floor0_partitions",
        ] {
            assert!(
                export.obj.contains(&format!("g {group}\n")),
                "missing group {group}"
            );
        }
    }
}

#[test]
fn obj_export_writes_both_files() {
    let dir = std::env::temp_dir().join(format!("urbanitas-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("house.obj");
    obj::write_obj(&generate(&params(5)).unwrap(), &path).unwrap();
    assert!(
        std::fs::read_to_string(&path)
            .unwrap()
            .contains("mtllib house.mtl")
    );
    assert!(
        std::fs::read_to_string(dir.join("house.mtl"))
            .unwrap()
            .contains("newmtl wall")
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn svg_plan_per_floor() {
    let b = generate(&BuildingParams {
        floors: 3,
        ..params(11)
    })
    .unwrap();
    let plans = svg::floor_plans(&b);
    assert_eq!(plans.len(), 3);
    for (plan, floor) in plans.iter().zip(&b.floors) {
        assert!(plan.starts_with("<svg") && plan.trim_end().ends_with("</svg>"));
        assert!(plan.contains(&floor.name()));
        for room in &floor.rooms {
            assert!(plan.contains(&format!(">{}</text>", room.kind.label())));
        }
    }
}

#[test]
fn params_round_trip_through_json() {
    let p = BuildingParams {
        building_type: BuildingType::Apartment,
        floors: 4,
        rooms: 3,
        ..params(99)
    };
    let text = serde_json::to_string(&p).unwrap();
    assert_eq!(serde_json::from_str::<BuildingParams>(&text).unwrap(), p);
    // Missing fields fall back to defaults.
    let partial: BuildingParams =
        serde_json::from_str(r#"{"floors": 1, "building_type": "villa"}"#).unwrap();
    assert_eq!(partial.floors, 1);
    assert_eq!(partial.building_type, BuildingType::Villa);
    assert_eq!(partial.rooms, BuildingParams::default().rooms);
}
