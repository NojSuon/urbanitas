//! Converts a generated [`Building`] into modular [`Layout`] parts.

use crate::geometry::{Axis, Edge, Rect, clip_to_outline, corner_neighbour};
use crate::model::{Building, FeatureKind, OpeningKind, RoomKind};
use crate::parts::*;

impl Building {
    /// See [`to_layout`].
    pub fn to_layout(&self) -> Layout {
        to_layout(self)
    }

    pub fn description(&self) -> String {
        format!(
            "{} {} · {} plan · seed {} · {:.1} × {:.1} m · {} floor(s) · {} roof · {}",
            self.params.style.label(),
            self.params.building_type.label(),
            self.shape.label(),
            self.params.seed,
            self.footprint.w,
            self.footprint.d,
            self.floors.len(),
            self.roof_label(),
            self.palette.name
        )
    }
}

/// Every room, wall, opening, balcony, roof and garden feature of `building`, as parts.
pub fn to_layout(building: &Building) -> Layout {
    let mut layout = Layout::new(
        building.name.clone(),
        building.style.clone(),
        building.palette.clone(),
    );
    layout.description = building.description();
    append_building(&mut layout, building, [0.0, 0.0], 0.0, None);
    layout
}

/// Adds every part of `b` to `layout`, moved by `offset`, standing at `ground`, as building `group`.
pub fn append_building(
    layout: &mut Layout,
    b: &Building,
    offset: P2,
    ground: f64,
    group: Option<u32>,
) {
    let first = layout.parts.len();
    let style = &b.style;
    let t = style.wall_thickness;
    let half = t / 2.0;
    let slab = style.slab_thickness;
    let fh = style.floor_height;
    let mut add = |level: u32, kind: PartKind| {
        layout.add_in(level, ground, group, kind);
    };

    for floor in &b.floors {
        let level = floor.level;
        let edges = &floor.outline;
        // Facades stack without overlapping: each covers the slab above it.
        let (base, height) = if level == 0 {
            (0.0, fh + slab)
        } else {
            (slab, fh)
        };
        for edge in edges {
            let c = edge.seg.coord - edge.out * half;
            let start = corner_neighbour(edges, edge, edge.seg.start)
                .map_or(edge.seg.start, |n| edge.seg.start - n.out * half);
            let end = corner_neighbour(edges, edge, edge.seg.end)
                .map_or(edge.seg.end, |n| edge.seg.end - n.out * half);
            let (a, b_) = match edge.seg.axis {
                Axis::X => ([start, c], [end, c]),
                Axis::Z => ([c, start], [c, end]),
            };
            add(
                level,
                PartKind::Wall(Wall {
                    a,
                    b: b_,
                    height,
                    thickness: t,
                    exterior: true,
                    base,
                    finish: WallFinish::Building,
                }),
            );
        }

        for room in &floor.rooms {
            if level > 0 && room.kind == RoomKind::Stairs {
                continue; // the stair well is open from below
            }
            for rect in under_walls(&room.rect, edges, half) {
                add(
                    level,
                    PartKind::Floor(FloorPart {
                        rect,
                        room: Some(room.kind),
                    }),
                );
            }
        }

        if (level as usize) + 1 < b.floors.len() {
            if let Some(stairs) = floor.find(RoomKind::Stairs) {
                let rect = inset(&stairs.rect, edges, t, style.partition_thickness);
                add(
                    level,
                    PartKind::Stairs(StairsPart {
                        rect,
                        rise: fh,
                        heading: Heading::North,
                    }),
                );
            }
        }

        for wall in &floor.walls {
            let seg = clip_to_outline(&wall.segment, edges, t);
            if seg.len() < 0.05 {
                continue;
            }
            let at = |along: f64| match seg.axis {
                Axis::X => [along, seg.coord],
                Axis::Z => [seg.coord, along],
            };
            add(
                level,
                PartKind::Wall(Wall {
                    a: at(seg.start),
                    b: at(seg.end),
                    height: fh,
                    thickness: style.partition_thickness,
                    exterior: false,
                    base: 0.0,
                    finish: WallFinish::Building,
                }),
            );
            if let Some(door) = wall.door {
                let facing = match seg.axis {
                    Axis::X => [0.0, 1.0],
                    Axis::Z => [1.0, 0.0],
                };
                add(
                    level,
                    PartKind::Opening(OpeningPart {
                        kind: OpeningKind::Archway,
                        at: at(door.center),
                        width: door.width,
                        height: door.height,
                        sill: slab,
                        arched: door.arched,
                        facing,
                    }),
                );
            }
        }

        for o in &floor.openings {
            add(
                level,
                PartKind::Opening(OpeningPart {
                    kind: o.kind,
                    at: on_centre_line(&o.edge, o.center, half),
                    width: o.width,
                    height: o.height,
                    sill: o.sill + slab,
                    arched: o.arched,
                    facing: o.edge.outward(),
                }),
            );
        }

        for balcony in &floor.balconies {
            add(
                level,
                PartKind::Balcony(BalconyPart {
                    at: on_centre_line(&balcony.edge, balcony.center, half),
                    width: balcony.width,
                    depth: balcony.depth,
                    facing: balcony.edge.outward(),
                }),
            );
        }
    }

    for roof in &b.roofs {
        add(
            roof.level,
            PartKind::Roof(RoofPart {
                rect: roof.rect,
                roof: roof.roof,
                open: roof.open,
            }),
        );
    }

    let plot = b
        .garden
        .as_ref()
        .filter(|g| g.kind != crate::params::Garden::None);
    match plot {
        Some(garden) => add(
            0,
            PartKind::Area(AreaPart {
                kind: AreaKind::Lawn,
                rect: garden.lot,
                height: 0.1,
            }),
        ),
        None => add(
            0,
            PartKind::Area(AreaPart {
                kind: AreaKind::Terrace,
                rect: b.footprint.grow(1.0),
                height: 0.02,
            }),
        ),
    }
    for f in b.garden.iter().flat_map(|g| &g.features) {
        let area = |kind| {
            PartKind::Area(AreaPart {
                kind,
                rect: f.rect,
                height: f.height,
            })
        };
        let prop = |kind: PropKind| {
            let (x, z) = f.rect.center();
            PartKind::Prop(PropPart {
                kind,
                at: [x, z],
                yaw: 0.0,
                scale: f.rect.w / kind.base_size(),
                height: f.height,
            })
        };
        let kind = match f.kind {
            FeatureKind::Path => area(AreaKind::Path),
            FeatureKind::Terrace => area(AreaKind::Terrace),
            FeatureKind::Pool => area(AreaKind::Pool),
            FeatureKind::Pergola => area(AreaKind::Pergola),
            FeatureKind::Loggia | FeatureKind::Altana => area(AreaKind::Loggia),
            FeatureKind::Courtyard => area(AreaKind::Courtyard),
            FeatureKind::Parterre => area(AreaKind::Parterre),
            FeatureKind::OliveTree => prop(PropKind::OliveTree),
            FeatureKind::CypressTree => prop(PropKind::CypressTree),
            FeatureKind::LemonTree => prop(PropKind::LemonTree),
            FeatureKind::Pot => prop(PropKind::Pot),
            FeatureKind::Fountain => prop(PropKind::Fountain),
            FeatureKind::Well => prop(PropKind::Well),
        };
        add(f.level, kind);
    }

    if offset != [0.0, 0.0] {
        for part in &mut layout.parts[first..] {
            part.translate(offset);
        }
    }
}

/// The point `along` an outline edge, on the centre line of its wall.
fn on_centre_line(edge: &Edge, along: f64, half: f64) -> P2 {
    let p = edge.point(along);
    let o = edge.outward();
    [p[0] - o[0] * half, p[1] - o[1] * half]
}

/// `rect` without the strips under the facade walls (`half` thick) along the outline,
/// so floor slabs stop inside the walls instead of showing on the facade.
fn under_walls(rect: &Rect, edges: &[Edge], half: f64) -> Vec<Rect> {
    let mut pieces = vec![*rect];
    for edge in edges {
        let Some((a, b)) = edge.span(rect) else {
            continue;
        };
        let c = edge.seg.coord;
        let band = match (edge.seg.axis, edge.out > 0.0) {
            (Axis::X, false) => Rect::new(a, c, b - a, half),
            (Axis::X, true) => Rect::new(a, c - half, b - a, half),
            (Axis::Z, false) => Rect::new(c, a, half, b - a),
            (Axis::Z, true) => Rect::new(c - half, a, half, b - a),
        };
        pieces = pieces
            .into_iter()
            .flat_map(|p| match p.intersection(&band) {
                Some(cut) => p.subtract(&cut),
                None => vec![p],
            })
            .collect();
    }
    pieces
}

/// `rect` shrunk by `wall` on sides that lie on the outline and by half a partition elsewhere.
fn inset(rect: &Rect, edges: &[Edge], wall: f64, partition: f64) -> Rect {
    let on = |axis: Axis, out: f64| {
        edges
            .iter()
            .any(|e| e.seg.axis == axis && e.out == out && e.span(rect).is_some())
    };
    let s = |outline: bool| if outline { wall } else { partition / 2.0 };
    rect.expand(
        -s(on(Axis::Z, -1.0)),
        -s(on(Axis::X, -1.0)),
        -s(on(Axis::Z, 1.0)),
        -s(on(Axis::X, 1.0)),
    )
}
