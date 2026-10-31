use std::collections::{HashSet, VecDeque};

use urbanitas::export::json;
use urbanitas::geometry::{EPS, Rect, clip_to_outline};
use urbanitas::model::{Floor, OpeningKind, RoomKind};
use urbanitas::{
    ArchitectureStyle, Building, BuildingParams, BuildingType, Garden, ParamError, Shape, Size,
    generate,
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
                            shape: Shape::Auto,
                        })
                    })
                })
            })
        })
    })
}

fn assert_rooms_tile_floor(floor: &Floor) {
    let wings: f64 = floor.wings.iter().map(Rect::area).sum();
    let area: f64 = floor.rooms.iter().map(|r| r.rect.area()).sum();
    assert!(
        (area - wings).abs() < 1e-6,
        "rooms cover {area} of {wings} m²"
    );
    for (i, a) in floor.rooms.iter().enumerate() {
        assert!(
            floor.wings.iter().any(|w| w.contains(&a.rect)),
            "room {i} leaves its wing: {:?}",
            a.rect
        );
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
    for (i, a) in floor.wings.iter().enumerate() {
        for b in &floor.wings[i + 1..] {
            assert!(!a.intersects(b), "wings overlap");
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
        let clear = clip_to_outline(&wall.segment, &floor.outline, b.style.wall_thickness);
        assert!(
            door.center - door.width / 2.0 >= clear.start - EPS
                && door.center + door.width / 2.0 <= clear.end + EPS,
            "door runs into an exterior wall: {door:?} on {clear:?}"
        );
    }
}

fn assert_outline_closed(floor: &Floor) {
    // Every corner of the outline is shared by exactly two edges.
    let mut corners: Vec<(i64, i64)> = Vec::new();
    for e in &floor.outline {
        for along in [e.seg.start, e.seg.end] {
            let p = e.point(along);
            corners.push((
                (p[0] * 1000.0).round() as i64,
                (p[1] * 1000.0).round() as i64,
            ));
        }
    }
    corners.sort();
    for chunk in corners.chunk_by(|a, b| a == b) {
        assert_eq!(
            chunk.len() % 2,
            0,
            "{} outline is open at {:?}",
            floor.name(),
            chunk[0]
        );
    }
}

fn assert_openings_fit(floor: &Floor) {
    for (i, a) in floor.openings.iter().enumerate() {
        for b in &floor.openings[i + 1..] {
            if a.edge == b.edge {
                assert!(
                    a.end() <= b.start() + EPS || b.end() <= a.start() + EPS,
                    "openings overlap: {a:?} {b:?}"
                );
            }
        }
    }
    for o in &floor.openings {
        assert!(floor.outline.contains(&o.edge), "opening off the outline");
        let room = &floor.rooms[o.room];
        let (a, b_) = o
            .edge
            .span(&room.rect)
            .expect("opening on its room's outside wall");
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

fn check(b: &Building, p: &BuildingParams) {
    assert_eq!(b.floors.len(), p.floors as usize);
    for floor in &b.floors {
        let living = floor
            .rooms
            .iter()
            .filter(|r| !r.kind.is_circulation())
            .count();
        assert!(
            living >= 1 && living <= p.rooms as usize + floor.wings.len() + 1,
            "{p:?}: {living} rooms"
        );
        assert_rooms_tile_floor(floor);
        assert_outline_closed(floor);
        assert_all_rooms_reachable(floor);
        assert_doors_clear_of_outer_walls(b, floor);
        assert_openings_fit(floor);
    }
    let ground = &b.floors[0];
    assert!(
        ground
            .openings
            .iter()
            .any(|o| o.kind == OpeningKind::Door && o.edge.is_street()),
        "{p:?} has no front door"
    );
    if let Some(court) = b.courtyard {
        // A way into the courtyard from the androne.
        assert!(
            ground
                .openings
                .iter()
                .any(|o| o.kind == OpeningKind::Archway
                    && o.edge.out > 0.0
                    && (o.edge.seg.coord - court.z).abs() < EPS),
            "{p:?}: courtyard has no way in"
        );
    }
    if p.floors > 1 {
        let stairs = b.stairs().expect("stairs");
        for floor in &b.floors {
            assert_eq!(floor.find(RoomKind::Stairs).map(|r| r.rect), Some(stairs));
        }
    } else {
        assert!(b.stairs().is_none());
    }
    if p.garden != Garden::None {
        let garden = b.garden.as_ref().expect("a garden");
        assert!(garden.lot.contains(&b.footprint));
        for f in garden.features.iter().filter(|f| f.level == 0) {
            assert!(garden.lot.contains(&f.rect), "{:?} outside the lot", f.kind);
            if f.kind.is_tree() && b.courtyard.is_none_or(|c| !c.contains(&f.rect)) {
                assert!(
                    b.wings.iter().all(|w| !f.rect.intersects(w)),
                    "tree inside the house"
                );
            }
        }
    }
}

#[test]
fn default_building_generates() {
    let b = generate(&BuildingParams::default()).unwrap();
    assert_eq!(b.floors.len(), 2);
    // Rooms on every floor plus the hall and stairs.
    assert!(b.floors.iter().all(|f| f.rooms.len() >= 3));
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
        check(&b, &p);
        checked += 1;
    }
    assert_eq!(checked, 3 * 4 * 4 * 4 * 7 * 6);
}

#[test]
fn every_shape_holds_invariants() {
    let mut shapes_seen = HashSet::new();
    for style in ArchitectureStyle::ALL {
        for shape in Shape::ALL {
            for building_type in TYPES {
                for floors in 1..=4 {
                    for seed in 0..4 {
                        let p = BuildingParams {
                            seed,
                            floors,
                            size: Size {
                                width: 22.0,
                                depth: 20.0,
                            },
                            building_type,
                            rooms: 6,
                            garden: Garden::Surrounding,
                            style,
                            shape,
                        };
                        let b = generate(&p).unwrap_or_else(|e| panic!("{p:?} failed: {e}"));
                        check(&b, &p);
                        shapes_seen.insert(b.shape);
                    }
                }
            }
        }
    }
    for shape in [
        Shape::Block,
        Shape::L,
        Shape::T,
        Shape::U,
        Shape::Courtyard,
        Shape::Stepped,
    ] {
        assert!(shapes_seen.contains(&shape), "{shape:?} never built");
    }
}

#[test]
fn plans_vary() {
    // Auto shapes give different footprints across seeds.
    let mut outlines = HashSet::new();
    for seed in 0..20 {
        let p = BuildingParams {
            seed,
            size: Size {
                width: 18.0,
                depth: 16.0,
            },
            building_type: BuildingType::Villa,
            rooms: 5,
            ..BuildingParams::default()
        };
        let b = generate(&p).unwrap();
        outlines.insert(b.floors[0].outline.len());
    }
    assert!(outlines.len() >= 2, "every villa had the same outline");
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
