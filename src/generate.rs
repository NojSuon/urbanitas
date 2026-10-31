//! Turns [`BuildingParams`] into a [`Building`].
//!
//! The pipeline, per building:
//! 1. pick a colour palette and a roof for the style,
//! 2. on multi-storey buildings, reserve a stair core (and a hall in front of
//!    it) on the west side, stacked identically on every floor,
//! 3. split the rest of each floor into rooms with binary space partitioning
//!    and give the biggest rooms to the most important uses,
//! 4. join the rooms with interior doors along a spanning tree, so every room
//!    is reachable without walking through a bathroom,
//! 5. punch the front door, windows, storefronts and balconies into the facade,
//! 6. lay out the garden around the entrances.

use std::f64::consts::PI;

use crate::geometry::{EPS, Rect, Side};
use crate::model::*;
use crate::params::{
    BuildingParams, BuildingType, Garden, MIN_HALL_DEPTH, ParamError, STAIR_LENGTH, STAIR_WIDTH,
};
use crate::rng::Rng;
use crate::style::{OpeningSpec, Palette, StyleProfile};

/// Room splits snap to this grid, in metres, so plans read cleanly.
const GRID: f64 = 0.05;
/// Clear wall kept between an opening and the corner of its room.
const CORNER_CLEARANCE: f64 = 0.4;
/// Spacing between candidate tree positions along the edge of the lot.
const TREE_SPACING: f64 = 3.2;

pub fn generate(params: &BuildingParams) -> Result<Building, ParamError> {
    params.validate()?;
    let mut rng = Rng::new(params.seed);
    let style = StyleProfile::for_style(params.style);
    let palette = Palette::choose(params.style, &mut rng);
    let footprint = Rect::new(0.0, 0.0, params.size.width, params.size.depth);
    let roof = choose_roof(params, &style, &mut rng);
    let core = circulation_core(params, &footprint);
    let floors: Vec<Floor> = (0..params.floors)
        .map(|level| layout_floor(params, &style, &footprint, core.as_ref(), level, &mut rng))
        .collect();
    let garden = plan_garden(params, &footprint, &floors[0], &mut rng);
    Ok(Building {
        name: format!("urbanitas_{}_{}", params.building_type.label(), params.seed),
        params: params.clone(),
        style,
        palette,
        footprint,
        floors,
        roof,
        garden,
    })
}

fn choose_roof(params: &BuildingParams, style: &StyleProfile, rng: &mut Rng) -> Roof {
    let flat = match params.building_type {
        BuildingType::Apartment | BuildingType::Shop => style.flat_roof_chance > 0.0,
        BuildingType::Villa => false,
        BuildingType::House => rng.chance(style.flat_roof_chance),
    };
    if flat {
        Roof::Flat {
            parapet_height: style.parapet_height,
        }
    } else {
        let overhang = style.eave_overhang;
        let short = params.size.width.min(params.size.depth) + 2.0 * overhang;
        Roof::Hip {
            pitch_deg: style.roof_pitch_deg,
            overhang,
            ridge_height: short / 2.0 * (style.roof_pitch_deg * PI / 180.0).tan(),
        }
    }
}

/// The stair well and hall shared by all floors, and what is left for rooms.
struct Core {
    stairs: Rect,
    hall: Option<Rect>,
    rest: Rect,
}

fn circulation_core(params: &BuildingParams, footprint: &Rect) -> Option<Core> {
    if params.floors < 2 {
        return None;
    }
    let depth = footprint.d;
    let (stairs, hall) = if depth - STAIR_LENGTH >= MIN_HALL_DEPTH {
        (
            Rect::new(
                footprint.x,
                footprint.z1() - STAIR_LENGTH,
                STAIR_WIDTH,
                STAIR_LENGTH,
            ),
            Some(Rect::new(
                footprint.x,
                footprint.z,
                STAIR_WIDTH,
                depth - STAIR_LENGTH,
            )),
        )
    } else {
        (
            Rect::new(footprint.x, footprint.z, STAIR_WIDTH, depth),
            None,
        )
    };
    Some(Core {
        stairs,
        hall,
        rest: Rect::new(
            footprint.x + STAIR_WIDTH,
            footprint.z,
            footprint.w - STAIR_WIDTH,
            depth,
        ),
    })
}

fn layout_floor(
    params: &BuildingParams,
    style: &StyleProfile,
    footprint: &Rect,
    core: Option<&Core>,
    level: u32,
    rng: &mut Rng,
) -> Floor {
    let mut rooms = Vec::new();
    let usable = match core {
        Some(core) => {
            if let Some(hall) = core.hall {
                rooms.push(Room::new(rooms.len(), RoomKind::Hall, hall));
            }
            rooms.push(Room::new(rooms.len(), RoomKind::Stairs, core.stairs));
            core.rest
        }
        None => *footprint,
    };

    let mut kinds = room_program(
        params.building_type,
        level,
        params.floors,
        params.rooms as usize,
    );
    shuffle(&mut kinds, rng);
    let weights: Vec<f64> = kinds.iter().map(|k| k.area_weight()).collect();
    let rects = split(usable, &weights, rng);
    for (kind, rect) in kinds.into_iter().zip(rects) {
        rooms.push(Room::new(rooms.len(), kind, rect));
    }

    let walls = connect_rooms(&rooms, footprint, style);
    let mut floor = Floor {
        level,
        elevation: level as f64 * style.floor_height,
        height: style.floor_height,
        rooms,
        walls,
        openings: Vec::new(),
        balconies: Vec::new(),
    };
    place_openings(&mut floor, params, style, footprint, rng);
    floor
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
    let (a, b) = if rect.w >= rect.d {
        let cut = snap_within(rect.x + rect.w * ratio, rect.x, rect.x1());
        (
            Rect::new(rect.x, rect.z, cut - rect.x, rect.d),
            Rect::new(cut, rect.z, rect.x1() - cut, rect.d),
        )
    } else {
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

/// The room the front door opens into: the hall, or the most important room on the street.
fn entrance_room(rooms: &[Room], footprint: &Rect) -> usize {
    if let Some(hall) = rooms.iter().find(|r| r.kind == RoomKind::Hall) {
        return hall.id;
    }
    let street_span = |r: &Room| {
        r.rect
            .span_on_side(Side::South, footprint)
            .map_or(0.0, |(a, b)| b - a)
    };
    rooms
        .iter()
        .filter(|r| r.kind != RoomKind::Stairs && street_span(r) >= 2.6)
        .min_by(|a, b| {
            a.kind
                .size_rank()
                .cmp(&b.kind.size_rank())
                .then(b.area.total_cmp(&a.area))
        })
        .or_else(|| {
            rooms
                .iter()
                .max_by(|a, b| street_span(a).total_cmp(&street_span(b)))
        })
        .map_or(0, |r| r.id)
}

/// Builds every partition and puts doors on a spanning tree grown from the entrance.
fn connect_rooms(rooms: &[Room], footprint: &Rect, style: &StyleProfile) -> Vec<InteriorWall> {
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
    let mut connected = vec![false; rooms.len()];
    connected[entrance_room(rooms, footprint)] = true;
    // First try full-width doors; then accept narrower ones for awkward leftovers.
    for min_len in [spec.width + 0.2, 0.7] {
        loop {
            let mut best: Option<(usize, f64)> = None;
            for (index, wall) in walls.iter().enumerate() {
                let [a, b] = wall.rooms;
                let clear = wall.segment.inside_walls(footprint, style.wall_thickness);
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
            let clear = wall.segment.inside_walls(footprint, style.wall_thickness);
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

fn place_openings(
    floor: &mut Floor,
    params: &BuildingParams,
    style: &StyleProfile,
    footprint: &Rect,
    rng: &mut Rng,
) {
    let ground = floor.level == 0;
    let mut entrances = Vec::new();
    if ground {
        let main = entrance_room(&floor.rooms, footprint);
        entrances.push(main);
        if params.building_type == BuildingType::Shop {
            let shop = floor.rooms.iter().find(|r| {
                r.kind == RoomKind::Retail && r.rect.span_on_side(Side::South, footprint).is_some()
            });
            if let Some(shop) = shop.filter(|s| s.id != main) {
                entrances.push(shop.id);
            }
        }
    }
    let balcony_chance = if params.building_type == BuildingType::Apartment {
        0.85
    } else {
        style.balcony_chance
    };
    let t = style.wall_thickness;

    for index in 0..floor.rooms.len() {
        let room = floor.rooms[index].clone();
        let mut has_balcony = false;
        for side in Side::ALL {
            let Some((mut a, mut b)) = room.rect.span_on_side(side, footprint) else {
                continue;
            };
            // Stay clear of the thick walls at the building's corners.
            let (side_start, side_end) = match side {
                Side::South | Side::North => (footprint.x, footprint.x1()),
                Side::West | Side::East => (footprint.z, footprint.z1()),
            };
            if (a - side_start).abs() < EPS {
                a += t;
            }
            if (side_end - b).abs() < EPS {
                b -= t;
            }

            let primary = if ground && side == Side::South && entrances.contains(&index) {
                Some((OpeningKind::Door, style.entrance))
            } else if !ground
                && !has_balcony
                && matches!(side, Side::South | Side::North)
                && room.kind.allows_balcony()
                && b - a >= style.balcony_door.width + 1.4
                && rng.chance(balcony_chance)
            {
                has_balcony = true;
                Some((OpeningKind::BalconyDoor, style.balcony_door))
            } else {
                None
            };

            let mut free = vec![(a, b)];
            if let Some((kind, spec)) = primary {
                if b - a >= spec.width + 0.6 {
                    let center = (a + b) / 2.0;
                    floor
                        .openings
                        .push(opening(kind, side, center, spec, index));
                    if kind == OpeningKind::BalconyDoor {
                        floor.balconies.push(Balcony {
                            side,
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

            let (kind, spec) = if ground && room.kind == RoomKind::Retail && side == Side::South {
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
                        .push(opening(kind, side, center, spec, index));
                }
            }
        }
    }
}

fn opening(kind: OpeningKind, side: Side, center: f64, spec: OpeningSpec, room: usize) -> Opening {
    Opening {
        kind,
        side,
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

fn plan_garden(
    params: &BuildingParams,
    footprint: &Rect,
    ground: &Floor,
    rng: &mut Rng,
) -> Option<GardenPlan> {
    let (south, north, sides) = match params.garden {
        Garden::None => return None,
        Garden::Front => (6.0, 1.5, 1.5),
        Garden::Back => (2.0, 8.0, 1.5),
        Garden::Surrounding => (6.0, 8.0, 5.0),
    };
    let style = StyleProfile::for_style(params.style);
    let lot = footprint.expand(sides, south, sides, north);
    let mut features = Vec::new();
    fn add(features: &mut Vec<GardenFeature>, kind: FeatureKind, rect: Rect, height: f64) -> Rect {
        features.push(GardenFeature { kind, rect, height });
        rect
    }

    let doors: Vec<&Opening> = ground
        .openings
        .iter()
        .filter(|o| o.kind == OpeningKind::Door && o.side == Side::South)
        .collect();
    for door in &doors {
        let path = Rect::new(door.center - 0.7, lot.z, 1.4, footprint.z - lot.z);
        add(&mut features, FeatureKind::Path, path, 0.04);
        for dir in [-1.0, 1.0] {
            let c = door.center + dir * (door.width / 2.0 + 0.6);
            add(
                &mut features,
                FeatureKind::Pot,
                Rect::new(c - 0.25, footprint.z - 0.8, 0.5, 0.5),
                0.6,
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
    clear.extend(features.iter().map(|f| f.rect.grow(0.8)));
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
}
