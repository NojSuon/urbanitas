//! Converts a generated [`Building`] into a modular [`Layout`].

use crate::geometry::{Axis, Rect, Side};
use crate::model::{Building, FeatureKind, OpeningKind, RoomKind};
use crate::parts::*;

impl Building {
    /// See [`to_layout`].
    pub fn to_layout(&self) -> Layout {
        to_layout(self)
    }
}

/// Every room, wall, opening, balcony, roof and garden feature of `building`, as parts.
pub fn to_layout(building: &Building) -> Layout {
    let b = building;
    let style = &b.style;
    let t = style.wall_thickness;
    let slab = style.slab_thickness;
    let fh = style.floor_height;
    let fp = b.footprint;
    // Exterior walls run on this centre line; floors reach it, so their edges hide inside the walls.
    let centre = fp.grow(-t / 2.0);
    let inside = fp.grow(-t);
    let levels = b.floors.len() as u32;

    let mut layout = Layout::new(b.name.clone(), style.clone(), b.palette.clone());
    layout.description = format!(
        "{} {} · seed {} · {:.1} × {:.1} m · {} floor(s) · {} roof · {}",
        b.params.style.label(),
        b.params.building_type.label(),
        b.params.seed,
        fp.w,
        fp.d,
        levels,
        b.roof.label(),
        b.palette.name
    );

    for floor in &b.floors {
        let level = floor.level;
        let top = level + 1 == levels;
        // The top floor's facade also wraps the edge of the roof slab.
        let facade_height = fh + if top { slab } else { 0.0 };
        for side in Side::ALL {
            let (a, b_) = side_line(side, &centre);
            layout.add(
                level,
                PartKind::Wall(Wall {
                    a,
                    b: b_,
                    height: facade_height,
                    thickness: t,
                    exterior: true,
                }),
            );
        }

        for room in &floor.rooms {
            if level > 0 && room.kind == RoomKind::Stairs {
                continue; // the stair well is open from below
            }
            if let Some(rect) = room.rect.intersection(&centre) {
                layout.add(
                    level,
                    PartKind::Floor(FloorPart {
                        rect,
                        room: Some(room.kind),
                    }),
                );
            }
        }

        if !top {
            if let Some(rect) = floor
                .find(RoomKind::Stairs)
                .and_then(|s| s.rect.intersection(&inside))
            {
                let rect = rect.expand(0.0, 0.0, -style.partition_thickness / 2.0, 0.0);
                layout.add(
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
            let seg = wall.segment.inside_walls(&fp, t);
            if seg.len() < 0.05 {
                continue;
            }
            let a = on_axis(seg.axis, seg.coord, seg.start);
            let b_ = on_axis(seg.axis, seg.coord, seg.end);
            layout.add(
                level,
                PartKind::Wall(Wall {
                    a,
                    b: b_,
                    height: fh,
                    thickness: style.partition_thickness,
                    exterior: false,
                }),
            );
            if let Some(door) = wall.door {
                let facing = match seg.axis {
                    Axis::X => [0.0, 1.0],
                    Axis::Z => [1.0, 0.0],
                };
                layout.add(
                    level,
                    PartKind::Opening(OpeningPart {
                        kind: OpeningKind::Archway,
                        at: on_axis(seg.axis, seg.coord, door.center),
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
            layout.add(
                level,
                PartKind::Opening(OpeningPart {
                    kind: o.kind,
                    at: side_point(o.side, &centre, o.center),
                    width: o.width,
                    height: o.height,
                    sill: o.sill + slab,
                    arched: o.arched,
                    facing: outward(o.side),
                }),
            );
        }

        for balcony in &floor.balconies {
            layout.add(
                level,
                PartKind::Balcony(BalconyPart {
                    at: side_point(balcony.side, &centre, balcony.center),
                    width: balcony.width,
                    depth: balcony.depth,
                    facing: outward(balcony.side),
                }),
            );
        }
    }

    layout.add(
        levels,
        PartKind::Roof(RoofPart {
            rect: fp,
            roof: b.roof,
        }),
    );

    match &b.garden {
        Some(garden) => {
            layout.add(
                0,
                PartKind::Area(AreaPart {
                    kind: AreaKind::Lawn,
                    rect: garden.lot,
                    height: 0.1,
                }),
            );
            for f in &garden.features {
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
                    FeatureKind::Loggia => area(AreaKind::Loggia),
                    FeatureKind::OliveTree => prop(PropKind::OliveTree),
                    FeatureKind::CypressTree => prop(PropKind::CypressTree),
                    FeatureKind::LemonTree => prop(PropKind::LemonTree),
                    FeatureKind::Pot => prop(PropKind::Pot),
                    FeatureKind::Fountain => prop(PropKind::Fountain),
                };
                layout.add(0, kind);
            }
        }
        None => {
            layout.add(
                0,
                PartKind::Area(AreaPart {
                    kind: AreaKind::Terrace,
                    rect: fp.grow(1.0),
                    height: 0.02,
                }),
            );
        }
    }
    layout
}

/// The two ends of the centre line of the facade on `side`.
fn side_line(side: Side, c: &Rect) -> (P2, P2) {
    match side {
        Side::South => ([c.x, c.z], [c.x1(), c.z]),
        Side::North => ([c.x, c.z1()], [c.x1(), c.z1()]),
        Side::West => ([c.x, c.z], [c.x, c.z1()]),
        Side::East => ([c.x1(), c.z], [c.x1(), c.z1()]),
    }
}

/// The point `along` the facade on `side`, on its centre line.
fn side_point(side: Side, c: &Rect, along: f64) -> P2 {
    match side {
        Side::South => [along, c.z],
        Side::North => [along, c.z1()],
        Side::West => [c.x, along],
        Side::East => [c.x1(), along],
    }
}

fn outward(side: Side) -> P2 {
    match side {
        Side::South => [0.0, -1.0],
        Side::North => [0.0, 1.0],
        Side::West => [-1.0, 0.0],
        Side::East => [1.0, 0.0],
    }
}

fn on_axis(axis: Axis, coord: f64, along: f64) -> P2 {
    match axis {
        Axis::X => [along, coord],
        Axis::Z => [coord, along],
    }
}
