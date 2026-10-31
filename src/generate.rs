//! Turns [`BuildingParams`] into a [`Building`].
//!
//! The pipeline, per building:
//! 1. pick a colour palette and a plan shape; build the footprint from wings
//!    (a street range plus back wings, or four ranges around a courtyard),
//! 2. decide which wings each floor covers; a top floor may step back, leaving
//!    a roof terrace on the floor below,
//! 3. reserve a stair core (and a hall in front of it) at the west end of the
//!    street range, stacked on every floor; courtyard palazzi also get an
//!    androne, a passage from the street into the courtyard,
//! 4. split each wing's remaining space into rooms with binary space
//!    partitioning, giving the biggest rooms to the most important uses,
//! 5. join the rooms with archways along a spanning tree, so every room is
//!    reachable without walking through a bathroom,
//! 6. punch doors, windows, storefronts and balconies into every piece of the
//!    outline, the courtyard's included,
//! 7. roof each wing, and lay out the courtyard and garden.

use crate::geometry::{EPS, Edge, Rect, clip_to_outline, outline};
use crate::model::*;
use crate::params::{
    BuildingParams, BuildingType, Garden, MIN_HALL_DEPTH, MIN_ROOM_AREA, ParamError, STAIR_LENGTH,
    STAIR_WIDTH, Shape,
};
use crate::rng::Rng;
use crate::style::{OpeningSpec, Palette, StyleProfile};

/// Room splits snap to this grid, in metres, so plans read cleanly.
const GRID: f64 = 0.05;
/// Clear wall kept between an opening and the corner of its room.
const CORNER_CLEARANCE: f64 = 0.4;
/// Spacing between candidate tree positions along the edge of the lot.
const TREE_SPACING: f64 = 3.2;
/// Narrowest a wing may be.
const MIN_WING: f64 = 4.5;
/// Width of the passage from the street into a courtyard.
const ANDRONE_WIDTH: f64 = 3.2;
/// Depth of the arcaded walk around a courtyard.
pub const PORTICO_DEPTH: f64 = 2.2;

pub fn generate(params: &BuildingParams) -> Result<Building, ParamError> {
    params.validate()?;
    let mut rng = Rng::new(params.seed);
    let style = StyleProfile::for_style(params.style);
    let palette = Palette::choose(params.style, &mut rng);
    let (shape, wings, courtyard) = massing(params, &style, &mut rng);
    let footprint = Rect::new(0.0, 0.0, params.size.width, params.size.depth);
    let level_wings = floor_wings(params, &style, &wings, &mut rng);
    let core = circulation_core(params, &wings[0]);
    let androne = courtyard.map(|_| {
        let front = wings[0];
        let x = snap_within(front.center().0 - ANDRONE_WIDTH / 2.0, front.x, front.x1());
        Rect::new(x, front.z, ANDRONE_WIDTH, front.d)
    });
    let floors: Vec<Floor> = (0..params.floors)
        .map(|level| {
            layout_floor(
                params,
                &style,
                &level_wings,
                core.as_ref(),
                androne,
                level,
                &mut rng,
            )
        })
        .collect();
    let roofs = plan_roofs(params, &style, &level_wings, &mut rng);
    let garden = plan_garden(
        params, &style, &footprint, shape, &wings, courtyard, &floors, &roofs, &mut rng,
    );
    Ok(Building {
        name: format!("urbanitas_{}_{}", params.building_type.label(), params.seed),
        params: params.clone(),
        style,
        palette,
        shape,
        footprint,
        wings,
        courtyard,
        floors,
        roofs,
        garden,
    })
}

fn choose_shape(params: &BuildingParams, style: &StyleProfile, rng: &mut Rng) -> Shape {
    if params.shape != Shape::Auto {
        return params.shape;
    }
    let big = params.size.width >= 16.0 && params.size.depth >= 16.0;
    let grand = matches!(
        params.building_type,
        BuildingType::Villa | BuildingType::Apartment
    );
    if big && grand && rng.chance(style.courtyard_chance) {
        return Shape::Courtyard;
    }
    let options: &[Shape] = match params.building_type {
        BuildingType::House => &[Shape::Block, Shape::L, Shape::L, Shape::Stepped, Shape::T],
        BuildingType::Villa => &[Shape::L, Shape::U, Shape::T, Shape::U, Shape::Stepped],
        BuildingType::Apartment => &[Shape::Block, Shape::U, Shape::L, Shape::T],
        BuildingType::Shop => &[Shape::Block, Shape::L, Shape::Stepped],
    };
    *rng.pick(options)
}

/// The footprint as wings, the street range first, plus the courtyard if there is one.
/// Falls back to a plain block when the chosen shape does not fit.
fn massing(
    params: &BuildingParams,
    style: &StyleProfile,
    rng: &mut Rng,
) -> (Shape, Vec<Rect>, Option<Rect>) {
    let (w, d) = (params.size.width, params.size.depth);
    let block = (Shape::Block, vec![Rect::new(0.0, 0.0, w, d)], None);
    let shape = choose_shape(params, style, rng);
    let half = |v: f64| (v * 2.0).round() / 2.0;
    let (wings, courtyard) = match shape {
        Shape::Auto | Shape::Block => return block,
        Shape::L => {
            let d1 = half(d * rng.range(0.5, 0.62));
            let w2 = half(w * rng.range(0.38, 0.55));
            let x = if rng.chance(0.5) { 0.0 } else { w - w2 };
            (
                vec![Rect::new(0.0, 0.0, w, d1), Rect::new(x, d1, w2, d - d1)],
                None,
            )
        }
        Shape::T => {
            let d1 = half(d * rng.range(0.5, 0.6));
            let w2 = half(w * rng.range(0.35, 0.5));
            let x = half((w - w2) / 2.0);
            (
                vec![Rect::new(0.0, 0.0, w, d1), Rect::new(x, d1, w2, d - d1)],
                None,
            )
        }
        Shape::U => {
            let d1 = half(d * rng.range(0.45, 0.55));
            let w2 = half(w * rng.range(0.28, 0.35));
            (
                vec![
                    Rect::new(0.0, 0.0, w, d1),
                    Rect::new(0.0, d1, w2, d - d1),
                    Rect::new(w - w2, d1, w2, d - d1),
                ],
                None,
            )
        }
        Shape::Courtyard => {
            let dr = half((d * 0.3).clamp(5.0, 7.0));
            let wr = half((w * 0.28).clamp(5.0, 7.0));
            let court = Rect::new(wr, dr, w - 2.0 * wr, d - 2.0 * dr);
            (
                vec![
                    Rect::new(0.0, 0.0, w, dr),
                    Rect::new(0.0, d - dr, w, dr),
                    Rect::new(0.0, dr, wr, d - 2.0 * dr),
                    Rect::new(w - wr, dr, wr, d - 2.0 * dr),
                ],
                Some(court),
            )
        }
        Shape::Stepped => {
            let d1 = half(d * rng.range(0.5, 0.62));
            let w2 = half(w * rng.range(0.55, 0.75));
            let x = half(rng.range(0.0, w - w2));
            (
                vec![Rect::new(0.0, 0.0, w, d1), Rect::new(x, d1, w2, d - d1)],
                None,
            )
        }
    };

    let narrow = wings.iter().any(|r| r.w < MIN_WING || r.d < MIN_WING);
    let court_ok = courtyard.is_none_or(|c| c.w >= 4.0 && c.d >= 4.0);
    let u_ok = shape != Shape::U || wings[2].x - wings[1].x1() >= 3.5;
    let core = if params.floors > 1 {
        STAIR_WIDTH * wings[0].d
    } else {
        0.0
    };
    let passage = if courtyard.is_some() {
        ANDRONE_WIDTH * wings[0].d
    } else {
        0.0
    };
    let usable: f64 = wings.iter().map(Rect::area).sum::<f64>() - core - passage;
    let roomy = usable / params.rooms as f64 >= MIN_ROOM_AREA * 0.9;
    if narrow || !court_ok || !u_ok || !roomy {
        return block;
    }
    (shape, wings, courtyard)
}

/// Which wings each floor covers. The top floor may drop a back wing, whose roof
/// becomes a terrace; the street range (with the stairs) always continues.
fn floor_wings(
    params: &BuildingParams,
    style: &StyleProfile,
    wings: &[Rect],
    rng: &mut Rng,
) -> Vec<Vec<Rect>> {
    let mut levels = vec![wings.to_vec(); params.floors as usize];
    if params.floors >= 2 && wings.len() >= 2 && rng.chance(style.terrace_chance) {
        let drop = 1 + rng.below(wings.len() - 1);
        levels.last_mut().expect("at least two floors").remove(drop);
    }
    levels
}

/// The stair well and hall shared by all floors, at the west end of the street range.
struct Core {
    stairs: Rect,
    hall: Option<Rect>,
}

fn circulation_core(params: &BuildingParams, front: &Rect) -> Option<Core> {
    if params.floors < 2 {
        return None;
    }
    let length = STAIR_LENGTH.min(front.d);
    if front.d - length >= MIN_HALL_DEPTH {
        Some(Core {
            stairs: Rect::new(front.x, front.z1() - length, STAIR_WIDTH, length),
            hall: Some(Rect::new(front.x, front.z, STAIR_WIDTH, front.d - length)),
        })
    } else {
        Some(Core {
            stairs: Rect::new(front.x, front.z, STAIR_WIDTH, front.d),
            hall: None,
        })
    }
}

fn layout_floor(
    params: &BuildingParams,
    style: &StyleProfile,
    level_wings: &[Vec<Rect>],
    core: Option<&Core>,
    androne: Option<Rect>,
    level: u32,
    rng: &mut Rng,
) -> Floor {
    let wings = level_wings[level as usize].clone();
    let edges = outline(&wings);
    let androne = androne.filter(|_| level == 0);
    let mut rooms = Vec::new();
    let mut free = Vec::new();
    for (i, wing) in wings.iter().enumerate() {
        if i > 0 {
            free.push(*wing);
            continue;
        }
        let mut rest = *wing;
        if let Some(core) = core {
            if let Some(hall) = core.hall {
                rooms.push(Room::new(rooms.len(), RoomKind::Hall, hall));
            }
            rooms.push(Room::new(rooms.len(), RoomKind::Stairs, core.stairs));
            rest = Rect::new(wing.x + STAIR_WIDTH, wing.z, wing.w - STAIR_WIDTH, wing.d);
        }
        match androne {
            Some(a) => {
                rooms.push(Room::new(rooms.len(), RoomKind::Hall, a));
                free.push(Rect::new(rest.x, rest.z, a.x - rest.x, rest.d));
                free.push(Rect::new(a.x1(), rest.z, rest.x1() - a.x1(), rest.d));
            }
            None => free.push(rest),
        }
    }
    free.retain(|r| r.w > 1.5 && r.d > 1.5);

    let area: f64 = free.iter().map(Rect::area).sum();
    let fits = ((area / (MIN_ROOM_AREA * 0.8)).floor() as usize).max(1);
    let room_for: usize = free.iter().map(capacity).sum();
    let count = (params.rooms as usize)
        .min(fits)
        .min(room_for)
        .max(free.len());
    let mut kinds = room_program(params.building_type, level, params.floors, count);
    shuffle(&mut kinds, rng);
    let mut next = 0;
    for (rect, n) in free.iter().zip(share(count, &free)) {
        let mine = &kinds[next..next + n];
        next += n;
        let weights: Vec<f64> = mine.iter().map(|k| k.area_weight()).collect();
        for (kind, piece) in mine.iter().zip(split(*rect, &weights, rng)) {
            rooms.push(Room::new(rooms.len(), *kind, piece));
        }
    }

    let entrance = entrance_room(&rooms, &edges, androne);
    let walls = connect_rooms(&rooms, &edges, style, entrance);
    let mut floor = Floor {
        level,
        elevation: level as f64 * style.floor_height,
        height: style.floor_height,
        wings,
        outline: edges,
        rooms,
        walls,
        openings: Vec::new(),
        balconies: Vec::new(),
    };
    let below = level
        .checked_sub(1)
        .map(|l| level_wings[l as usize].as_slice());
    place_openings(&mut floor, params, style, entrance, androne, below, rng);
    floor
}

/// Narrowest side a room is split down to, when there is space.
const MIN_ROOM_SIDE: f64 = 2.2;

/// How many rooms fit in `rect` without any being narrower than [`MIN_ROOM_SIDE`].
fn capacity(rect: &Rect) -> usize {
    let across = (rect.w / MIN_ROOM_SIDE).floor().max(1.0) as usize;
    let along = (rect.d / MIN_ROOM_SIDE).floor().max(1.0) as usize;
    (across * along).clamp(1, 12)
}

/// How many of `count` rooms each rectangle gets: at least one each, the rest by area,
/// never more than fit.
fn share(count: usize, rects: &[Rect]) -> Vec<usize> {
    let mut shares = vec![1usize; rects.len()];
    for _ in rects.len()..count {
        let best = (0..rects.len())
            .filter(|&i| shares[i] < capacity(&rects[i]))
            .max_by(|&a, &b| {
                (rects[a].area() / shares[a] as f64)
                    .total_cmp(&(rects[b].area() / shares[b] as f64))
            });
        match best {
            Some(i) => shares[i] += 1,
            None => break,
        }
    }
    shares
}

fn shuffle<T>(items: &mut [T], rng: &mut Rng) {
    for i in (1..items.len()).rev() {
        items.swap(i, rng.below(i + 1));
    }
}

/// Binary space partitioning into one rectangle per weight, in the same order,
/// each with roughly its weight's share of the area.
fn split(rect: Rect, weights: &[f64], rng: &mut Rng) -> Vec<Rect> {
    if weights.len() <= 1 {
        return vec![rect];
    }
    let half = weights.len() / 2;
    let mid = if rng.chance(0.5) {
        half
    } else {
        weights.len() - half
    };
    let (first, second) = weights.split_at(mid);
    let share = first.iter().sum::<f64>() / weights.iter().sum::<f64>();
    let ratio = (share * rng.range(0.94, 1.06)).clamp(0.15, 0.85);
    // Keep at least a room's width per room on each side of the cut, when there is space.
    let keep = |len: f64, ratio: f64| {
        let (lo, hi) = (
            MIN_ROOM_SIDE * first.len() as f64,
            len - MIN_ROOM_SIDE * second.len() as f64,
        );
        if lo <= hi {
            (len * ratio).clamp(lo, hi) / len
        } else {
            first.len() as f64 / weights.len() as f64
        }
    };
    let (a, b) = if rect.w >= rect.d {
        let ratio = keep(rect.w, ratio);
        let cut = snap_within(rect.x + rect.w * ratio, rect.x, rect.x1());
        (
            Rect::new(rect.x, rect.z, cut - rect.x, rect.d),
            Rect::new(cut, rect.z, rect.x1() - cut, rect.d),
        )
    } else {
        let ratio = keep(rect.d, ratio);
        let cut = snap_within(rect.z + rect.d * ratio, rect.z, rect.z1());
        (
            Rect::new(rect.x, rect.z, rect.w, cut - rect.z),
            Rect::new(rect.x, cut, rect.w, rect.z1() - cut),
        )
    };
    let mut out = split(a, first, rng);
    out.extend(split(b, second, rng));
    out
}

/// Snaps `value` to the grid unless that would push it onto `lo` or `hi`.
fn snap_within(value: f64, lo: f64, hi: f64) -> f64 {
    let snapped = (value / GRID).round() * GRID;
    if snapped > lo + EPS && snapped < hi - EPS {
        snapped
    } else {
        value
    }
}

/// What the rooms on one floor are used for, largest first.
fn room_program(building: BuildingType, level: u32, floors: u32, count: usize) -> Vec<RoomKind> {
    use RoomKind::*;
    let ground = level == 0;
    let (base, filler): (&[RoomKind], RoomKind) = match (building, ground) {
        (BuildingType::House, true) if floors == 1 => (
            &[Living, Kitchen, Bathroom, Bedroom, Dining, Bedroom, Study][..],
            Bedroom,
        ),
        (BuildingType::House, true) => (&[Living, Kitchen, Bathroom, Dining, Study][..], Study),
        (BuildingType::House, false) => (
            &[Bedroom, Bathroom, Bedroom, Bedroom, Study, Bathroom][..],
            Bedroom,
        ),
        (BuildingType::Villa, true) if floors == 1 => (
            &[
                Living, Kitchen, Dining, Bedroom, Bathroom, Bedroom, Lounge, Bathroom, Study,
            ][..],
            Bedroom,
        ),
        (BuildingType::Villa, true) => (
            &[Living, Kitchen, Dining, Bathroom, Lounge, Study, Bedroom][..],
            Lounge,
        ),
        (BuildingType::Villa, false) => (
            &[Bedroom, Bathroom, Bedroom, Bathroom, Bedroom, Lounge][..],
            Bedroom,
        ),
        (BuildingType::Apartment, _) => (
            &[Living, Kitchen, Bedroom, Bathroom, Bedroom, Study, Bedroom][..],
            Bedroom,
        ),
        (BuildingType::Shop, true) => (&[Retail, Storage, Bathroom, Office, Storage][..], Office),
        (BuildingType::Shop, false) => {
            (&[Living, Kitchen, Bedroom, Bathroom, Bedroom][..], Bedroom)
        }
    };
    let mut kinds: Vec<RoomKind> = (0..count)
        .map(|i| base.get(i).copied().unwrap_or(filler))
        .collect();
    kinds.sort_by_key(|k| k.size_rank());
    kinds
}

/// How much of `rect` faces the street.
fn street_span(rect: &Rect, edges: &[Edge]) -> f64 {
    edges
        .iter()
        .filter(|e| e.is_street())
        .filter_map(|e| e.span(rect))
        .map(|(a, b)| b - a)
        .sum()
}

/// The room the front door opens into: the androne, the hall, or the best room on the street.
fn entrance_room(rooms: &[Room], edges: &[Edge], androne: Option<Rect>) -> usize {
    if let Some(a) = androne.and_then(|a| rooms.iter().find(|r| r.rect == a)) {
        return a.id;
    }
    if let Some(hall) = rooms.iter().find(|r| r.kind == RoomKind::Hall) {
        return hall.id;
    }
    rooms
        .iter()
        .filter(|r| r.kind != RoomKind::Stairs && street_span(&r.rect, edges) >= 2.6)
        .min_by(|a, b| {
            a.kind
                .size_rank()
                .cmp(&b.kind.size_rank())
                .then(b.area.total_cmp(&a.area))
        })
        .or_else(|| {
            rooms
                .iter()
                .max_by(|a, b| street_span(&a.rect, edges).total_cmp(&street_span(&b.rect, edges)))
        })
        .map_or(0, |r| r.id)
}

/// Builds every partition and puts archways on a spanning tree grown from the entrance.
fn connect_rooms(
    rooms: &[Room],
    edges: &[Edge],
    style: &StyleProfile,
    root: usize,
) -> Vec<InteriorWall> {
    let mut walls = Vec::new();
    for i in 0..rooms.len() {
        for j in i + 1..rooms.len() {
            if let Some(segment) = rooms[i].rect.shared_wall(&rooms[j].rect) {
                if segment.len() > 0.05 {
                    walls.push(InteriorWall {
                        segment,
                        rooms: [i, j],
                        door: None,
                    });
                }
            }
        }
    }

    let spec = style.interior_door;
    let t = style.wall_thickness;
    let mut connected = vec![false; rooms.len()];
    connected[root] = true;
    // First try full-width doors; then accept narrower ones for awkward leftovers.
    for min_len in [spec.width + 0.2, 0.7] {
        loop {
            let mut best: Option<(usize, f64)> = None;
            for (index, wall) in walls.iter().enumerate() {
                let [a, b] = wall.rooms;
                let clear = clip_to_outline(&wall.segment, edges, t);
                if connected[a] == connected[b] || clear.len() < min_len {
                    continue;
                }
                let from = if connected[a] { a } else { b };
                let mut score = clear.len();
                if rooms[a].kind.is_circulation() && rooms[b].kind.is_circulation() {
                    score += 200.0;
                }
                if rooms[from].kind.is_dead_end() {
                    score -= 100.0;
                }
                if best.is_none_or(|(_, s)| score > s) {
                    best = Some((index, score));
                }
            }
            let Some((index, _)) = best else { break };
            let wall = &mut walls[index];
            let clear = clip_to_outline(&wall.segment, edges, t);
            wall.door = Some(Door {
                center: clear.mid(),
                width: spec.width.min(clear.len() - 0.2),
                height: spec.height,
                arched: spec.arched,
            });
            connected[wall.rooms[0]] = true;
            connected[wall.rooms[1]] = true;
        }
    }
    walls
}

/// Whether the ground just outside `edge` (between `a` and `b`) is the roof of a lower wing.
fn faces_terrace(edge: &Edge, a: f64, b: f64, below: &[Rect]) -> bool {
    let p = edge.point((a + b) / 2.0);
    let o = edge.outward();
    let q = [p[0] + o[0] * 0.6, p[1] + o[1] * 0.6];
    below
        .iter()
        .any(|r| q[0] > r.x && q[0] < r.x1() && q[1] > r.z && q[1] < r.z1())
}

#[allow(clippy::too_many_arguments)]
fn place_openings(
    floor: &mut Floor,
    params: &BuildingParams,
    style: &StyleProfile,
    entrance: usize,
    androne: Option<Rect>,
    below: Option<&[Rect]>,
    rng: &mut Rng,
) {
    let ground = floor.level == 0;
    let mut entrances = Vec::new();
    if ground {
        entrances.push(entrance);
        if params.building_type == BuildingType::Shop {
            let shop = floor
                .rooms
                .iter()
                .find(|r| r.kind == RoomKind::Retail && street_span(&r.rect, &floor.outline) > 0.0);
            if let Some(shop) = shop.filter(|s| s.id != entrance) {
                entrances.push(shop.id);
            }
        }
    }
    let balcony_chance = if params.building_type == BuildingType::Apartment {
        0.85
    } else {
        style.balcony_chance
    };
    let nobile = style.nobile_balconies && floor.level == 1;
    let t = style.wall_thickness;
    let edges = floor.outline.clone();
    let passage = OpeningSpec {
        width: (ANDRONE_WIDTH - 0.8).min(2.4),
        height: (style.floor_height - 0.5).min(3.4),
        sill: 0.0,
        arched: true,
    };

    for index in 0..floor.rooms.len() {
        let room = floor.rooms[index].clone();
        let is_androne = androne.is_some_and(|a| a == room.rect);
        let mut has_balcony = false;
        for edge in &edges {
            let Some((mut a, mut b)) = edge.span(&room.rect) else {
                continue;
            };
            // Stay clear of the thick walls at the outline's corners.
            if (a - edge.seg.start).abs() < EPS {
                a += t;
            }
            if (edge.seg.end - b).abs() < EPS {
                b -= t;
            }
            if b - a < 0.5 {
                continue;
            }
            let onto_terrace = below.is_some_and(|wings| faces_terrace(edge, a, b, wings));

            let primary = if ground && edge.is_street() && entrances.contains(&index) {
                let spec = if is_androne {
                    OpeningSpec {
                        arched: true,
                        ..passage
                    }
                } else {
                    style.entrance
                };
                Some((OpeningKind::Door, spec, false))
            } else if is_androne && !edge.is_street() {
                Some((OpeningKind::Archway, passage, false))
            } else if onto_terrace && !has_balcony {
                has_balcony = true;
                Some((OpeningKind::BalconyDoor, style.balcony_door, false))
            } else if !ground
                && !nobile
                && !has_balcony
                && edge.seg.axis == crate::geometry::Axis::X
                && room.kind.allows_balcony()
                && b - a >= style.balcony_door.width + 1.4
                && rng.chance(balcony_chance)
            {
                has_balcony = true;
                Some((OpeningKind::BalconyDoor, style.balcony_door, true))
            } else {
                None
            };

            let mut free = vec![(a, b)];
            if let Some((kind, spec, with_balcony)) = primary {
                if b - a >= spec.width + 0.6 {
                    let center = (a + b) / 2.0;
                    floor
                        .openings
                        .push(opening(kind, *edge, center, spec, index));
                    if with_balcony {
                        floor.balconies.push(Balcony {
                            edge: *edge,
                            center,
                            width: (b - a - 0.4).min(3.2),
                            depth: style.balcony_depth,
                            room: index,
                        });
                    }
                    // `spread` keeps the other half of the gap.
                    let half = spec.width / 2.0 + style.window_gap / 2.0;
                    free = vec![(a, center - half), (center + half, b)];
                }
            }
            if is_androne {
                continue;
            }

            // The piano nobile of a Palermo palazzo: a balcony door on its own balcony
            // at every street bay.
            if nobile && edge.is_street() && !room.kind.small_windows() {
                let spec = style.balcony_door;
                let bay = spec.width + 0.7;
                for (lo, hi) in free {
                    for center in spread(lo, hi, bay, style.window_gap) {
                        floor.openings.push(opening(
                            OpeningKind::BalconyDoor,
                            *edge,
                            center,
                            spec,
                            index,
                        ));
                        floor.balconies.push(Balcony {
                            edge: *edge,
                            center,
                            width: bay,
                            depth: style.balcony_depth,
                            room: index,
                        });
                    }
                }
                continue;
            }

            let (kind, spec) = if ground && room.kind == RoomKind::Retail && edge.is_street() {
                (OpeningKind::Storefront, style.storefront)
            } else if room.kind.small_windows() {
                (OpeningKind::Window, style.small_window)
            } else {
                (OpeningKind::Window, style.window)
            };
            for (lo, hi) in free {
                for center in spread(lo, hi, spec.width, style.window_gap) {
                    floor
                        .openings
                        .push(opening(kind, *edge, center, spec, index));
                }
            }
        }
    }
}

fn opening(kind: OpeningKind, edge: Edge, center: f64, spec: OpeningSpec, room: usize) -> Opening {
    Opening {
        kind,
        edge,
        center,
        width: spec.width,
        height: spec.height,
        sill: spec.sill,
        arched: spec.arched,
        room,
    }
}

/// Centres for as many `width`-wide openings as fit in `lo..hi`, evenly spaced
/// and at least `gap` apart. Each end keeps half a gap clear, so openings in
/// neighbouring stretches of wall also end up a full gap apart (room for shutters
/// and stone frames).
fn spread(lo: f64, hi: f64, width: f64, gap: f64) -> Vec<f64> {
    let margin = CORNER_CLEARANCE.max(gap / 2.0);
    let usable = hi - lo - 2.0 * margin;
    if usable < width {
        return Vec::new();
    }
    let count = ((usable + gap) / (width + gap)).floor().max(1.0) as usize;
    if count == 1 {
        return vec![(lo + hi) / 2.0];
    }
    let first = lo + margin + width / 2.0;
    let step = (usable - width) / (count - 1) as f64;
    (0..count).map(|i| first + step * i as f64).collect()
}

/// A roof over every wing that the floor above does not continue: the main roof on
/// top, flat terraces where a floor steps back.
fn plan_roofs(
    params: &BuildingParams,
    style: &StyleProfile,
    level_wings: &[Vec<Rect>],
    rng: &mut Rng,
) -> Vec<RoofSpec> {
    let floors = params.floors;
    let flat_top = match params.building_type {
        BuildingType::Apartment | BuildingType::Shop => style.flat_roof_chance > 0.0,
        BuildingType::Villa => style.flat_roof_chance >= 1.0,
        BuildingType::House => rng.chance(style.flat_roof_chance),
    };
    let terrace = Roof::Flat {
        parapet_height: style.parapet_height,
        balustrade: style.roof_balustrade,
    };
    let mut roofs = Vec::new();
    for (level, wings) in level_wings.iter().enumerate() {
        let above = level_wings.get(level + 1);
        for wing in wings {
            if above.is_some_and(|a| a.contains(wing)) {
                continue;
            }
            let roof_level = level as u32 + 1;
            let roof = if roof_level < floors || flat_top {
                terrace
            } else {
                Roof::hip(wing, style.roof_pitch_deg, style.eave_overhang)
            };
            roofs.push(RoofSpec {
                rect: *wing,
                level: roof_level,
                roof,
                open: [false; 4],
            });
        }
    }
    // No parapet where a flat roof meets another one, or a wall of the floor above.
    let snapshot = roofs.clone();
    for roof in &mut roofs {
        if !matches!(roof.roof, Roof::Flat { .. }) {
            continue;
        }
        let r = roof.rect;
        // Thin strips just across each side, kept off the corners.
        let sides = [
            Rect::new(r.x + 0.2, r.z - 0.05, r.w - 0.4, 0.1),
            Rect::new(r.x + 0.2, r.z1() - 0.05, r.w - 0.4, 0.1),
            Rect::new(r.x - 0.05, r.z + 0.2, 0.1, r.d - 0.4),
            Rect::new(r.x1() - 0.05, r.z + 0.2, 0.1, r.d - 0.4),
        ];
        let above = level_wings.get(roof.level as usize);
        for (k, probe) in sides.iter().enumerate() {
            let meets_roof = snapshot.iter().any(|o| {
                o.level == roof.level
                    && o.rect != r
                    && matches!(o.roof, Roof::Flat { .. })
                    && o.rect.grow(0.01).intersects(probe)
            });
            let meets_wall =
                above.is_some_and(|wings| wings.iter().any(|w| w.grow(0.01).intersects(probe)));
            roof.open[k] = meets_roof || meets_wall;
        }
    }
    roofs
}

#[allow(clippy::too_many_arguments)]
fn plan_garden(
    params: &BuildingParams,
    style: &StyleProfile,
    footprint: &Rect,
    shape: Shape,
    wings: &[Rect],
    courtyard: Option<Rect>,
    floors: &[Floor],
    roofs: &[RoofSpec],
    rng: &mut Rng,
) -> Option<GardenPlan> {
    let mut features = Vec::new();
    fn add(features: &mut Vec<GardenFeature>, kind: FeatureKind, rect: Rect, height: f64) -> Rect {
        features.push(GardenFeature {
            kind,
            rect,
            height,
            level: 0,
        });
        rect
    }

    // The courtyard of a palazzo: paving, a portico, a well and citrus trees.
    if let Some(court) = courtyard {
        add(
            &mut features,
            FeatureKind::Courtyard,
            court,
            style.floor_height,
        );
        let (cx, cz) = court.center();
        add(
            &mut features,
            FeatureKind::Well,
            Rect::new(cx - 0.7, cz - 0.7, 1.4, 1.4),
            2.2,
        );
        let inner = court.grow(-(PORTICO_DEPTH + 0.9));
        if inner.w > 1.5 && inner.d > 1.5 {
            for (x, z) in [
                (inner.x, inner.z),
                (inner.x1(), inner.z),
                (inner.x, inner.z1()),
                (inner.x1(), inner.z1()),
            ] {
                add(
                    &mut features,
                    FeatureKind::LemonTree,
                    Rect::new(x - 0.7, z - 0.7, 1.4, 1.4),
                    2.6,
                );
            }
        }
    }

    // A rooftop loggia on the main roof.
    let top = floors.len() as u32;
    if let Some(roof) = roofs
        .iter()
        .find(|r| r.level == top && matches!(r.roof, Roof::Hip { .. }))
    {
        if rng.chance(style.altana_chance) && roof.rect.w >= 8.0 && roof.rect.d >= 5.0 {
            let r = roof.rect;
            let x = if rng.chance(0.5) {
                r.x + 0.6
            } else {
                r.x1() - 4.6
            };
            features.push(GardenFeature {
                kind: FeatureKind::Altana,
                rect: Rect::new(x, r.z + r.d / 2.0 - 1.7, 4.0, 3.4),
                height: style.floor_height,
                level: top,
            });
        }
    }

    let (south, north, sides) = match params.garden {
        Garden::None => {
            return (!features.is_empty()).then_some(GardenPlan {
                kind: Garden::None,
                lot: *footprint,
                features,
            });
        }
        Garden::Front => (6.0, 1.5, 1.5),
        Garden::Back => (2.0, 8.0, 1.5),
        Garden::Surrounding => (6.0, 8.0, 5.0),
    };
    let lot = footprint.expand(sides, south, sides, north);

    let doors: Vec<&Opening> = floors[0]
        .openings
        .iter()
        .filter(|o| o.kind == OpeningKind::Door && o.edge.is_street())
        .collect();
    for door in &doors {
        let z = door.edge.seg.coord;
        add(
            &mut features,
            FeatureKind::Path,
            Rect::new(door.center - 0.7, lot.z, 1.4, z - lot.z),
            0.04,
        );
        for dir in [-1.0, 1.0] {
            let c = door.center + dir * (door.width / 2.0 + 0.6);
            add(
                &mut features,
                FeatureKind::Pot,
                Rect::new(c - 0.25, z - 0.8, 0.5, 0.5),
                0.6,
            );
        }
    }

    // The open court of a U: a parterre around a fountain.
    if shape == Shape::U {
        let court = Rect::new(
            wings[1].x1(),
            wings[1].z,
            wings[2].x - wings[1].x1(),
            wings[1].d,
        )
        .grow(-0.6);
        if court.w >= 3.0 && court.d >= 3.0 {
            add(&mut features, FeatureKind::Parterre, court, 0.6);
            let (cx, cz) = court.center();
            add(
                &mut features,
                FeatureKind::Fountain,
                Rect::new(cx - 0.9, cz - 0.9, 1.8, 1.8),
                1.4,
            );
        }
    }

    let terrace = (north >= 4.0).then(|| {
        add(
            &mut features,
            FeatureKind::Terrace,
            Rect::new(footprint.x, footprint.z1(), footprint.w, 2.5),
            0.05,
        )
    });

    let wants_pool = params.building_type == BuildingType::Villa || rng.chance(0.25);
    if wants_pool && north >= 7.5 && footprint.w >= 5.0 {
        let width = (footprint.w - 2.0).min(9.0);
        let pool = Rect::new(
            footprint.x + (footprint.w - width) / 2.0,
            footprint.z1() + 3.0,
            width,
            3.6,
        );
        add(&mut features, FeatureKind::Pool, pool, 1.4);
    }

    if let (Some(terrace), true) = (terrace, style.loggia) {
        let width = (footprint.w * 0.6).clamp(4.0, 9.0).min(footprint.w);
        let loggia = Rect::new(
            footprint.x + (footprint.w - width) / 2.0,
            terrace.z,
            width,
            2.6,
        );
        add(
            &mut features,
            FeatureKind::Loggia,
            loggia,
            style.floor_height,
        );
    } else if let Some(terrace) = terrace {
        if rng.chance(0.6) {
            let width = (footprint.w * 0.45).clamp(2.5, 5.0);
            let x = if rng.chance(0.5) {
                footprint.x + 0.3
            } else {
                footprint.x1() - width - 0.3
            };
            add(
                &mut features,
                FeatureKind::Pergola,
                Rect::new(x, terrace.z + 0.1, width, terrace.d - 0.2),
                2.6,
            );
        }
    }

    if let (Some(door), true) = (doors.first(), style.fountain && south >= 6.0) {
        // On the roomier side of the front path.
        let x = if door.center - footprint.x > footprint.x1() - door.center {
            (footprint.x + door.center) / 2.0
        } else {
            (door.center + footprint.x1()) / 2.0
        };
        let fountain = Rect::new(x - 1.0, footprint.z - 4.4, 2.0, 2.0);
        if lot.contains(&fountain)
            && !features
                .iter()
                .any(|f| f.rect.grow(0.3).intersects(&fountain))
        {
            add(&mut features, FeatureKind::Fountain, fountain, 1.5);
        }
    }

    // Trees along the edge of the lot, clear of the house, paths and terrace.
    let mut clear: Vec<Rect> = vec![footprint.grow(1.6)];
    clear.extend(
        features
            .iter()
            .filter(|f| f.level == 0)
            .map(|f| f.rect.grow(0.8)),
    );
    let inner = lot.grow(-1.2);
    let corners = [
        (inner.x, inner.z),
        (inner.x1(), inner.z),
        (inner.x1(), inner.z1()),
        (inner.x, inner.z1()),
    ];
    for i in 0..4 {
        let (p, q) = (corners[i], corners[(i + 1) % 4]);
        let length = ((q.0 - p.0).powi(2) + (q.1 - p.1).powi(2)).sqrt();
        let steps = (length / TREE_SPACING).floor().max(1.0) as usize;
        for step in 0..steps {
            let t = step as f64 / steps as f64;
            let (x, z) = (p.0 + (q.0 - p.0) * t, p.1 + (q.1 - p.1) * t);
            let kind = *rng.pick(&[
                FeatureKind::OliveTree,
                FeatureKind::CypressTree,
                FeatureKind::CypressTree,
                FeatureKind::LemonTree,
            ]);
            let (size, height) = match kind {
                FeatureKind::OliveTree => (2.6, 4.0),
                FeatureKind::CypressTree => (1.0, 7.0),
                _ => (1.8, 3.0),
            };
            let canopy = Rect::new(x - size / 2.0, z - size / 2.0, size, size);
            let skip = rng.chance(0.2);
            if skip || !lot.contains(&canopy) || clear.iter().any(|c| c.intersects(&canopy)) {
                continue;
            }
            clear.push(canopy.grow(0.3));
            add(&mut features, kind, canopy, height);
        }
    }

    Some(GardenPlan {
        kind: params.garden,
        lot,
        features,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_tiles_the_rectangle() {
        let mut rng = Rng::new(3);
        let rect = Rect::new(2.6, 0.0, 9.4, 10.0);
        for count in 1..12 {
            let parts = split(rect, &vec![1.0; count], &mut rng);
            assert_eq!(parts.len(), count);
            let area: f64 = parts.iter().map(Rect::area).sum();
            assert!((area - rect.area()).abs() < 1e-6);
        }
    }

    #[test]
    fn spread_respects_gap() {
        let centers = spread(0.0, 10.0, 1.0, 1.4);
        assert!(!centers.is_empty());
        for pair in centers.windows(2) {
            assert!(pair[1] - pair[0] >= 1.0 + 1.4 - 1e-9);
        }
        assert!(spread(0.0, 1.0, 1.0, 1.4).is_empty());
    }

    #[test]
    fn biggest_room_is_living() {
        let kinds = room_program(BuildingType::House, 0, 2, 4);
        assert_eq!(kinds[0], RoomKind::Living);
        assert_eq!(*kinds.last().unwrap(), RoomKind::Bathroom);
    }

    #[test]
    fn shares_cover_every_rectangle() {
        let rects = [
            Rect::new(0.0, 0.0, 10.0, 5.0),
            Rect::new(0.0, 5.0, 3.0, 3.0),
        ];
        assert_eq!(share(5, &rects), vec![4, 1]);
        assert_eq!(share(30, &rects), vec![8, 1], "capped by what fits");
        assert_eq!(share(2, &rects), vec![1, 1]);
    }

    #[test]
    fn every_shape_fits_a_palazzo_plot() {
        for shape in Shape::ALL {
            let params = BuildingParams {
                size: crate::params::Size {
                    width: 22.0,
                    depth: 20.0,
                },
                rooms: 6,
                shape,
                ..BuildingParams::default()
            };
            let (got, wings, court) = massing(
                &params,
                &StyleProfile::for_style(params.style),
                &mut Rng::new(1),
            );
            if shape != Shape::Auto {
                assert_eq!(got, shape);
            }
            assert_eq!(court.is_some(), got == Shape::Courtyard);
            for (i, a) in wings.iter().enumerate() {
                for b in &wings[i + 1..] {
                    assert!(!a.intersects(b), "{shape:?} wings overlap");
                }
            }
        }
    }
}
