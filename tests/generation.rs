use std::collections::{HashSet, VecDeque};

use urbanitas::export::json;
use urbanitas::geometry::{EPS, Side};
use urbanitas::model::{Floor, OpeningKind, RoomKind};
use urbanitas::{
    ArchitectureStyle, Building, BuildingParams, BuildingType, Garden, ParamError, Size, generate,
};

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

/// Every combination of style, type, garden, floor count and room count, over a few seeds.
fn sweep() -> impl Iterator<Item = BuildingParams> {
    ArchitectureStyle::ALL.into_iter().flat_map(|style| {
        TYPES.into_iter().flat_map(move |building_type| {
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
                            style,
                        })
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
    assert_eq!(checked, 2 * 4 * 4 * 4 * 7 * 6);
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
