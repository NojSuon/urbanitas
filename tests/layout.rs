use urbanitas::export::{obj, svg};
use urbanitas::geometry::Rect;
use urbanitas::mesh::build::{Scene, View, WallMode};
use urbanitas::mesh::{Material, MeshSet};
use urbanitas::model::{FeatureKind, OpeningKind, Roof};
use urbanitas::parts::*;
use urbanitas::style::{Palette, StyleProfile};
use urbanitas::{ArchitectureStyle, BuildingParams, BuildingType, Garden, generate};

fn florentine_villa(seed: u64) -> BuildingParams {
    BuildingParams {
        seed,
        floors: 3,
        building_type: BuildingType::Villa,
        garden: Garden::Surrounding,
        style: ArchitectureStyle::Florentine,
        ..BuildingParams::default()
    }
}

fn assert_valid_mesh(set: &MeshSet, what: &str) {
    assert!(!set.is_empty(), "{what} built no geometry");
    for (material, mesh) in &set.meshes {
        let n = mesh.positions.len();
        assert_eq!(
            mesh.normals.len(),
            n,
            "{what} {material:?}: one normal per vertex"
        );
        assert_eq!(mesh.uvs.len(), n, "{what} {material:?}: one UV per vertex");
        assert_eq!(mesh.indices.len() % 3, 0);
        assert!(
            mesh.indices.iter().all(|&i| (i as usize) < n),
            "{what} {material:?}: index out of range"
        );
        for [x, y, z] in &mesh.normals {
            let len = (x * x + y * y + z * z).sqrt();
            assert!(
                (len - 1.0).abs() < 1e-3,
                "{what} {material:?}: normal of length {len}"
            );
        }
        for p in mesh
            .positions
            .iter()
            .flatten()
            .chain(mesh.uvs.iter().flatten())
        {
            assert!(p.is_finite(), "{what} {material:?}: non-finite value");
        }
    }
}

fn layout_with(kind: PartKind) -> Layout {
    let style = StyleProfile::for_style(ArchitectureStyle::Florentine);
    let palette = Palette::choose(
        ArchitectureStyle::Florentine,
        &mut urbanitas::rng::Rng::new(1),
    );
    let mut layout = Layout::new("test", style, palette);
    layout.add(0, kind);
    layout
}

#[test]
fn every_part_kind_builds_a_valid_mesh() {
    let wall = Wall {
        a: [0.0, 0.0],
        b: [6.0, 0.0],
        height: 3.6,
        thickness: 0.45,
        exterior: true,
        base: 0.0,
        finish: WallFinish::Building,
    };
    let mut kinds = vec![
        PartKind::Wall(wall),
        PartKind::Floor(FloorPart {
            rect: Rect::new(0.0, 0.0, 4.0, 3.0),
            room: None,
        }),
        PartKind::Stairs(StairsPart {
            rect: Rect::new(0.0, 0.0, 2.4, 4.8),
            rise: 3.6,
            heading: Heading::East,
        }),
        PartKind::Balcony(BalconyPart {
            at: [3.0, 0.0],
            width: 2.4,
            depth: 0.9,
            facing: [0.0, -1.0],
        }),
    ];
    for roof in [
        Roof::Flat {
            parapet_height: 0.9,
            balustrade: false,
        },
        Roof::Flat {
            parapet_height: 1.0,
            balustrade: true,
        },
        Roof::Hip {
            pitch_deg: 18.0,
            overhang: 1.0,
            ridge_height: 2.0,
        },
    ] {
        kinds.push(PartKind::Roof(RoofPart {
            rect: Rect::new(0.0, 0.0, 10.0, 8.0),
            roof,
            open: [false, true, false, false],
        }));
    }
    for kind in [
        OpeningKind::Window,
        OpeningKind::Door,
        OpeningKind::BalconyDoor,
        OpeningKind::Storefront,
        OpeningKind::Gate,
    ] {
        kinds.push(PartKind::Opening(OpeningPart {
            kind,
            at: [3.0, 0.0],
            width: 1.2,
            height: 2.4,
            sill: 0.2,
            arched: true,
            facing: [0.0, -1.0],
        }));
    }
    for kind in AreaKind::ALL {
        kinds.push(PartKind::Area(AreaPart {
            kind,
            rect: Rect::new(0.0, 0.0, 6.0, 3.0),
            height: kind.default_height(),
        }));
    }
    for kind in PropKind::ALL {
        kinds.push(PartKind::Prop(PropPart {
            kind,
            at: [1.0, 1.0],
            yaw: 0.3,
            scale: 1.0,
            height: kind.default_height(),
        }));
    }
    for finish in [WallFinish::Garden, WallFinish::DryStone, WallFinish::Fence] {
        kinds.push(PartKind::Wall(Wall {
            a: [0.0, 0.0],
            b: [8.0, 0.0],
            height: finish.default_height(),
            thickness: finish.default_thickness(),
            exterior: false,
            base: 0.0,
            finish,
        }));
    }
    for kind in kinds {
        let what = format!("{kind:?}");
        let layout = layout_with(kind);
        let set = Scene::new(&layout).build_all(&View::default());
        assert_valid_mesh(&set, &what);
    }
}

#[test]
fn generated_buildings_mesh_cleanly_in_every_view() {
    for style in ArchitectureStyle::ALL {
        for seed in 0..4 {
            let layout = generate(&BuildingParams {
                style,
                ..florentine_villa(seed)
            })
            .unwrap()
            .to_layout();
            let scene = Scene::new(&layout);
            for walls in [WallMode::Up, WallMode::Cutaway, WallMode::Down] {
                let view = View {
                    walls,
                    look: Some([0.6, 0.8]),
                };
                assert_valid_mesh(
                    &scene.build_all(&view),
                    &format!("{style:?} seed {seed} {walls:?}"),
                );
            }
            let full = scene.build_all(&View::default()).triangle_count();
            let down = scene
                .build_level(
                    0,
                    &View {
                        walls: WallMode::Down,
                        look: None,
                    },
                )
                .triangle_count();
            assert!(down < full, "walls-down should draw less than everything");
        }
    }
}

#[test]
fn layout_keeps_every_room_opening_and_tree() {
    let building = generate(&florentine_villa(3)).unwrap();
    let layout = building.to_layout();
    let floors_in_layout = layout
        .parts
        .iter()
        .filter(|p| matches!(p.kind, PartKind::Floor(_)))
        .count();
    let stair_wells_above_ground = building.floors.len() - 1;
    let rooms: usize = building.floors.iter().map(|f| f.rooms.len()).sum();
    // Rooms along the facades are split round the walls, so there can be more slabs than rooms.
    assert!(floors_in_layout >= rooms - stair_wells_above_ground);

    let openings: usize = building.floors.iter().map(|f| f.openings.len()).sum();
    let doors: usize = building
        .floors
        .iter()
        .map(|f| f.walls.iter().filter(|w| w.door.is_some()).count())
        .sum();
    let layout_openings = layout
        .parts
        .iter()
        .filter(|p| matches!(p.kind, PartKind::Opening(_)))
        .count();
    assert_eq!(layout_openings, openings + doors);

    let garden = building.garden.as_ref().unwrap();
    let trees = garden.features.iter().filter(|f| f.kind.is_tree()).count();
    let layout_trees = layout
        .parts
        .iter()
        .filter(|p| matches!(&p.kind, PartKind::Prop(prop) if matches!(prop.kind, PropKind::OliveTree | PropKind::CypressTree | PropKind::LemonTree)))
        .count();
    assert_eq!(layout_trees, trees);
    assert!(
        garden
            .features
            .iter()
            .any(|f| f.kind == FeatureKind::Loggia),
        "Florentine villas get a loggia"
    );

    assert_eq!(layout.levels() as usize, building.floors.len());
    // Every opening sits on a wall.
    for part in &layout.parts {
        if let PartKind::Opening(o) = &part.kind {
            assert!(
                layout.host_wall(part.level, o.at).is_some(),
                "opening {} has no wall",
                part.id
            );
        }
    }
}

#[test]
fn layout_round_trips_through_json() {
    let layout = generate(&florentine_villa(5)).unwrap().to_layout();
    let back = Layout::from_json(&layout.to_json()).unwrap();
    assert_eq!(back, layout);
}

#[test]
fn a_window_cuts_a_hole_in_its_wall() {
    let wall = Wall {
        a: [0.0, 0.0],
        b: [6.0, 0.0],
        height: 3.6,
        thickness: 0.45,
        exterior: true,
        base: 0.0,
        finish: WallFinish::Building,
    };
    let mut layout = layout_with(PartKind::Wall(wall));
    let solid = Scene::new(&layout)
        .build_part(&layout.parts[0], &View::default())
        .triangle_count();
    layout.add(
        0,
        PartKind::Opening(OpeningPart {
            kind: OpeningKind::Window,
            at: [3.0, 0.0],
            width: 1.2,
            height: 1.8,
            sill: 1.0,
            arched: false,
            facing: [0.0, -1.0],
        }),
    );
    let scene = Scene::new(&layout);
    let wall_part = &layout.parts[0];
    let holed = scene.build_part(wall_part, &View::default());
    assert!(
        holed.triangle_count() > solid,
        "the wall is split around the window"
    );
    // No wall geometry crosses the middle of the hole.
    let crosses = holed.meshes.values().any(|mesh| {
        mesh.indices.chunks(3).any(|tri| {
            let ps: Vec<[f32; 3]> = tri.iter().map(|&i| mesh.positions[i as usize]).collect();
            let min = |k: usize| ps.iter().map(|p| p[k]).fold(f32::MAX, f32::min);
            let max = |k: usize| ps.iter().map(|p| p[k]).fold(f32::MIN, f32::max);
            min(0) < 3.0 && max(0) > 3.0 && min(1) < 1.9 && max(1) > 1.9
        })
    });
    assert!(!crosses, "no face spans the window opening");
}

#[test]
fn hand_added_parts_survive_and_move() {
    let mut layout = generate(&florentine_villa(2)).unwrap().to_layout();
    let count = layout.parts.len();
    let id = layout.add(
        0,
        PartKind::Prop(PropPart {
            kind: PropKind::Fountain,
            at: [0.0, -4.0],
            yaw: 0.0,
            scale: 1.0,
            height: 1.5,
        }),
    );
    assert_eq!(layout.parts.len(), count + 1);
    layout.get_mut(id).unwrap().translate([1.0, 2.0]);
    match &layout.get(id).unwrap().kind {
        PartKind::Prop(p) => assert_eq!(p.at, [1.0, -2.0]),
        _ => unreachable!(),
    }
    let removed = layout.remove(id).unwrap();
    assert_eq!(layout.parts.len(), count);
    layout.restore(removed);
    assert!(layout.get(id).is_some());
}

#[test]
fn obj_export_has_normals_uvs_and_textures() {
    let layout = generate(&florentine_villa(9)).unwrap().to_layout();
    let export = obj::to_obj(&layout, "villa.mtl", "villa");
    let count = |prefix: &str| export.obj.lines().filter(|l| l.starts_with(prefix)).count();
    let (v, vt, vn) = (count("v "), count("vt "), count("vn "));
    assert!(v > 1000);
    assert_eq!(v, vt);
    assert_eq!(v, vn);
    for line in export.obj.lines().filter(|l| l.starts_with("f ")) {
        for corner in line[2..].split(' ') {
            let ids: Vec<usize> = corner.split('/').map(|i| i.parse().unwrap()).collect();
            assert_eq!(ids.len(), 3);
            assert!(ids.iter().all(|&i| i >= 1 && i <= v), "bad face: {line}");
        }
    }
    for material in export.obj.lines().filter_map(|l| l.strip_prefix("usemtl ")) {
        assert!(export.mtl.contains(&format!("newmtl {material}\n")));
    }
    for material in [
        Material::Plaster,
        Material::Rustication,
        Material::RoofTile,
        Material::Glass,
        Material::Shutter,
    ] {
        assert!(
            export.skins.iter().any(|s| s.material == material),
            "Florentine villa uses {material:?}"
        );
        assert!(
            export
                .mtl
                .contains(&format!("map_Kd villa_{}.png", material.name()))
        );
    }
}

#[test]
fn obj_export_writes_textures() {
    let dir = std::env::temp_dir().join(format!("urbanitas-obj-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let layout = generate(&BuildingParams::default()).unwrap().to_layout();
    let written = obj::write_obj(&layout, &dir.join("house.obj"), 32).unwrap();
    assert!(written.iter().any(|p| p.ends_with("house.mtl")));
    let pngs: Vec<_> = written
        .iter()
        .filter(|p| p.extension().is_some_and(|e| e == "png"))
        .collect();
    assert!(pngs.len() >= 5);
    for png in pngs {
        let bytes = std::fs::read(png).unwrap();
        assert_eq!(&bytes[1..4], b"PNG");
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn svg_plan_per_level_from_layout() {
    let building = generate(&BuildingParams {
        floors: 3,
        ..florentine_villa(11)
    })
    .unwrap();
    let layout = building.to_layout();
    let plans = svg::floor_plans(&layout);
    assert_eq!(plans.len(), 3);
    for (plan, floor) in plans.iter().zip(&building.floors) {
        assert!(plan.starts_with("<svg") && plan.trim_end().ends_with("</svg>"));
        assert!(plan.contains(&floor.name()));
        for room in floor
            .rooms
            .iter()
            .filter(|r| floor.level == 0 || r.kind != urbanitas::model::RoomKind::Stairs)
        {
            assert!(
                plan.contains(&format!(">{}</text>", room.kind.label())),
                "{} on {}",
                room.kind.label(),
                floor.name()
            );
        }
    }
}

#[test]
fn walls_meet_in_l_and_courtyard_plans() {
    for shape in [
        urbanitas::Shape::L,
        urbanitas::Shape::Courtyard,
        urbanitas::Shape::U,
    ] {
        let p = BuildingParams {
            size: urbanitas::Size {
                width: 22.0,
                depth: 20.0,
            },
            rooms: 6,
            shape,
            ..florentine_villa(4)
        };
        let layout = generate(&p).unwrap().to_layout();
        let scene = Scene::new(&layout);
        assert_valid_mesh(&scene.build_all(&View::default()), &format!("{shape:?}"));
        // Facade wall ends always land on another facade wall (corners close).
        for part in &layout.parts {
            let PartKind::Wall(w) = &part.kind else {
                continue;
            };
            if !w.exterior {
                continue;
            }
            for end in [w.a, w.b] {
                let joined = layout.parts.iter().any(|o| {
                    o.id != part.id
                        && o.level == part.level
                        && matches!(&o.kind, PartKind::Wall(x) if x.exterior && (dist(x.a, end) < 1e-6 || dist(x.b, end) < 1e-6))
                });
                assert!(
                    joined,
                    "{shape:?}: facade end {end:?} on level {} meets nothing",
                    part.level
                );
            }
        }
    }
}

#[test]
fn terrain_and_sites_mesh_cleanly() {
    for context in [
        urbanitas::Context::Urban,
        urbanitas::Context::Suburban,
        urbanitas::Context::Rural,
    ] {
        for style in ArchitectureStyle::ALL {
            let p = urbanitas::SiteParams {
                context,
                building: BuildingParams {
                    style,
                    seed: 3,
                    ..BuildingParams::default()
                },
                buildings: 4,
                relief: 3.0,
            };
            let layout = urbanitas::generate_site(&p).unwrap();
            let scene = Scene::new(&layout);
            let terrain = scene.build_terrain();
            assert_valid_mesh(&terrain, &format!("{context:?} terrain"));
            assert_valid_mesh(
                &scene.build_all(&View::default()),
                &format!("{context:?} {style:?}"),
            );
            for level in 0..=layout.parts.iter().map(|p| p.level).max().unwrap() {
                for (_, set) in scene.build_level_groups(level, &View::default()) {
                    for mesh in set.meshes.values() {
                        assert_eq!(mesh.indices.len() % 3, 0);
                    }
                }
            }
        }
    }
}
