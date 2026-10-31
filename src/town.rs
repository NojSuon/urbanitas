//! Towns grown on a landscape around a piazza.
//!
//! [`grow_town`] levels a piazza where you click, with a fountain, benches, lamps and a
//! belvedere tower on its uphill side. Streets leave it in every direction that isn't
//! blocked and wander outwards in short straight runs, turning towards the gentlest
//! climb so they follow the contours; they branch as they go and stop at water, cliffs
//! and the edge of the map. Houses then line both sides of every street, each turned
//! to face it, standing on its own levelled pad (a stone terrace where the ground falls
//! away). Their character changes with distance from the piazza: tall attached palazzi
//! in the centre, houses with gardens further out, villas among olive trees on the
//! edge, and low beach houses by the sand or villas perched on cliff tops by the sea.
//!
//! [`fill_street`] lines a street drawn by hand with houses in the same way.

use std::f64::consts::{FRAC_PI_2, PI};
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::design::Design;
use crate::generate::generate;
use crate::geometry::Rect;
use crate::landscape::{Water, WaterKind};
use crate::params::{ArchitectureStyle, BuildingParams, BuildingType, Garden, Shape, Size};
use crate::parts::*;
use crate::rng::Rng;
use crate::terrain::{Terrain, smoothstep};

/// Where a town's houses come from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesignMix {
    /// Every house newly generated.
    #[default]
    Generated,
    /// Only your saved designs (plots none of them fit stay empty).
    Designs,
    /// Saved designs and generated houses, half and half.
    Mixed,
}

impl DesignMix {
    pub const ALL: [DesignMix; 3] = [DesignMix::Generated, DesignMix::Designs, DesignMix::Mixed];

    pub fn label(self) -> &'static str {
        match self {
            DesignMix::Generated => "Generated houses",
            DesignMix::Designs => "My designs",
            DesignMix::Mixed => "Mixed",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TownParams {
    pub style: ArchitectureStyle,
    /// How many houses to aim for.
    pub houses: u32,
    /// 0–1: how tightly houses pack and how often streets branch.
    pub density: f64,
    pub seed: u64,
    /// Where the houses come from.
    pub mix: DesignMix,
}

impl Default for TownParams {
    fn default() -> Self {
        Self {
            style: ArchitectureStyle::Florentine,
            houses: 24,
            density: 0.6,
            seed: 1,
            mix: DesignMix::Generated,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TownReport {
    pub houses: usize,
    pub streets: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TownError {
    /// The piazza would stand in water.
    InWater,
    /// The piazza would hang over the edge of the map.
    OffMap,
}

impl fmt::Display for TownError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TownError::InWater => write!(f, "the piazza can't stand in the water"),
            TownError::OffMap => write!(f, "the piazza must be inside the map"),
        }
    }
}

impl std::error::Error for TownError {}

/// The kinds of house a plot can take.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HouseKind {
    /// Tall attached palazzi round the piazza.
    Palazzo,
    /// Town houses with small gardens.
    House,
    /// Villas with grounds, on the edge of town.
    Villa,
    /// Low villas by the beach, with palms.
    Beach,
    /// Villas on the cliff top, looking out to sea.
    Cliff,
}

/// Compass directions as unit steps on the ground plane (south is −z).
const DIRS: [P2; 4] = [[0.0, -1.0], [1.0, 0.0], [0.0, 1.0], [-1.0, 0.0]];

fn left(d: usize) -> usize {
    (d + 3) % 4
}

fn right(d: usize) -> usize {
    (d + 1) % 4
}

/// A street segment between two points on one axis.
#[derive(Clone, Copy, Debug)]
struct Segment {
    a: P2,
    b: P2,
    width: f64,
}

impl Segment {
    fn horizontal(&self) -> bool {
        (self.a[1] - self.b[1]).abs() < 1e-9
    }

    /// The paved area, covering both joints.
    fn rect(&self) -> Rect {
        let (x0, x1) = (self.a[0].min(self.b[0]), self.a[0].max(self.b[0]));
        let (z0, z1) = (self.a[1].min(self.b[1]), self.a[1].max(self.b[1]));
        Rect::new(x0, z0, x1 - x0, z1 - z0).grow(self.width / 2.0)
    }
}

fn rect_between(a: P2, b: P2, half: f64) -> Rect {
    let (x0, x1) = (a[0].min(b[0]), a[0].max(b[0]));
    let (z0, z1) = (a[1].min(b[1]), a[1].max(b[1]));
    Rect::new(x0, z0, x1 - x0, z1 - z0).grow(half)
}

fn dist(a: P2, b: P2) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}

/// Whether a square stays clear of the water, checked on a 5 × 5 grid of points. The grid is
/// finer than the narrowest wet band (a river plus its margin), so no water slips between.
fn dry_square(water: Option<&Water>, t: &Terrain, r: &Rect) -> bool {
    (0..25).all(|k| {
        let p = [
            r.x + r.w * (k % 5) as f64 / 4.0,
            r.z + r.d * (k / 5) as f64 / 4.0,
        ];
        !wet(water, t, p, 2.0)
    })
}

/// Whether ground at `p` is too close to the water to build on.
fn wet(water: Option<&Water>, t: &Terrain, p: P2, margin: f64) -> bool {
    water.is_some_and(|w| {
        w.surface_near(p, margin)
            .is_some_and(|s| t.height_at(p) < s + 0.6)
    })
}

struct Town<'a> {
    layout: &'a mut Layout,
    /// Saved designs the town may use.
    designs: &'a [Design],
    params: TownParams,
    rng: Rng,
    plaza: P2,
    /// Ground already spoken for: the piazza, streets and house lots.
    taken: Vec<Rect>,
    /// Ground kept free of houses (but not of streets).
    reserved: Vec<Rect>,
    streets: Vec<Segment>,
    /// House pads to level: lot, height.
    pads: Vec<(Rect, f64)>,
    /// Trees and lamps to plant once the ground is final.
    props: Vec<(PropKind, P2, f64)>,
    houses: usize,
}

impl<'a> Town<'a> {
    fn new(layout: &'a mut Layout, plaza: P2, params: &TownParams, designs: &'a [Design]) -> Self {
        if layout.terrain.is_none() {
            layout.terrain = Some(Terrain::flat(
                Rect::new(plaza[0] - 120.0, plaza[1] - 120.0, 240.0, 240.0),
                2.0,
            ));
        }
        // Everything already standing is in the way.
        let mut taken = Vec::new();
        let mut groups: Vec<(u32, Rect)> = Vec::new();
        for part in layout.parts.iter().filter(|p| p.level == 0) {
            let r = part.footprint();
            match part.group {
                Some(g) => match groups.iter_mut().find(|(k, _)| *k == g) {
                    Some((_, b)) => *b = b.union(&r),
                    None => groups.push((g, r)),
                },
                None => {
                    if let PartKind::Area(_) | PartKind::Wall(_) = part.kind {
                        taken.push(r)
                    }
                }
            }
        }
        taken.extend(groups.into_iter().map(|(_, r)| r));
        Self {
            layout,
            designs,
            params: *params,
            rng: Rng::new(params.seed ^ 0x70_57),
            plaza,
            taken,
            reserved: Vec::new(),
            streets: Vec::new(),
            pads: Vec::new(),
            props: Vec::new(),
            houses: 0,
        }
    }

    fn terrain(&self) -> &Terrain {
        self.layout
            .terrain
            .as_ref()
            .expect("towns always have terrain")
    }

    fn water(&self) -> Option<&Water> {
        self.layout.water.as_ref()
    }

    fn free(&self, r: &Rect) -> bool {
        let map = self.terrain().rect.grow(-4.0);
        map.contains(r)
            && !self
                .taken
                .iter()
                .chain(&self.reserved)
                .any(|t| t.intersects(r))
    }

    fn area(&mut self, kind: AreaKind, rect: Rect, ground: f64, height: f64) {
        self.layout.add_in(
            0,
            ground,
            None,
            PartKind::Area(AreaPart { kind, rect, height }),
        );
    }

    /// Levels the piazza and furnishes it. Returns its rectangle.
    fn piazza(&mut self) -> Result<Rect, TownError> {
        let side = (20.0 + self.params.houses as f64 * 0.3).clamp(22.0, 34.0);
        let r = Rect::new(
            self.plaza[0] - side / 2.0,
            self.plaza[1] - side / 2.0,
            side,
            side,
        );
        if !self.terrain().rect.grow(-8.0).contains(&r) {
            return Err(TownError::OffMap);
        }
        let water = self.layout.water.as_ref();
        let t = self.layout.terrain.as_ref().unwrap();
        if !dry_square(water, t, &r) {
            return Err(TownError::InWater);
        }
        let level = t.height_at(self.plaza).max(water.map_or(f64::MIN, |w| {
            w.surface_near(self.plaza, 50.0).unwrap_or(f64::MIN) + 1.0
        }));
        // Which side climbs highest: the tower goes there.
        let sides = [[0.0, -1.0], [1.0, 0.0], [0.0, 1.0], [-1.0, 0.0]]
            .map(|d: P2| t.height_at([self.plaza[0] + d[0] * side, self.plaza[1] + d[1] * side]));
        let uphill = (0..4)
            .max_by(|&a, &b| sides[a].total_cmp(&sides[b]))
            .unwrap_or(2);
        self.layout
            .terrain
            .as_mut()
            .unwrap()
            .flatten_pad(&r.grow(0.5), level, 10.0);
        self.area(AreaKind::Piazza, r, level, 0.05);
        self.layout.add_in(
            0,
            level,
            None,
            PartKind::Prop(PropPart {
                kind: PropKind::Fountain,
                at: self.plaza,
                yaw: 0.0,
                scale: 1.0,
                height: PropKind::Fountain.default_height(),
            }),
        );
        let inset = 2.0;
        for (x, z) in [
            (r.x + inset, r.z + inset),
            (r.x1() - inset, r.z + inset),
            (r.x + inset, r.z1() - inset),
            (r.x1() - inset, r.z1() - inset),
        ] {
            self.props.push((PropKind::Lamp, [x, z], 1.0));
        }
        for k in 0..4 {
            let d = DIRS[k];
            let at = [self.plaza[0] + d[0] * 5.5, self.plaza[1] + d[1] * 5.5];
            self.layout.add_in(
                0,
                level,
                None,
                PartKind::Prop(PropPart {
                    kind: PropKind::Bench,
                    at,
                    yaw: [0.0, -FRAC_PI_2, PI, FRAC_PI_2][k],
                    scale: 1.0,
                    height: PropKind::Bench.default_height(),
                }),
            );
        }
        // The belvedere stands in the uphill corner to the right of that side.
        let tower = 4.5;
        let d = DIRS[uphill];
        let r2 = DIRS[right(uphill)];
        let cx = self.plaza[0]
            + d[0] * (side / 2.0 - tower / 2.0 - 0.5)
            + r2[0] * (side / 2.0 - tower / 2.0 - 0.5);
        let cz = self.plaza[1]
            + d[1] * (side / 2.0 - tower / 2.0 - 0.5)
            + r2[1] * (side / 2.0 - tower / 2.0 - 0.5);
        let height = 13.0 + self.rng.range(0.0, 4.0);
        self.area(
            AreaKind::Tower,
            Rect::new(cx - tower / 2.0, cz - tower / 2.0, tower, tower),
            level,
            height,
        );
        // Houses keep back from the piazza, leaving a ring of open ground round it.
        self.taken.push(r);
        self.reserved.push(r.grow(5.0));
        // A pad of its own, so the houses round it don't tilt it.
        self.pads.push((r, level));
        Ok(r)
    }

    /// Tries to run a street `length` metres from `from` in direction `dir`.
    /// Returns how steep it climbs, or `None` if it can't go that way.
    fn probe(&self, from: P2, dir: usize, length: f64, width: f64) -> Option<f64> {
        let d = DIRS[dir];
        let to = [from[0] + d[0] * length, from[1] + d[1] * length];
        let t = self.terrain();
        if !t.rect.grow(-10.0).contains(&rect_between(to, to, width)) {
            return None;
        }
        // Keep clear of other streets beside it, but let it join the one it leaves.
        // Starts clear of the street it leaves (at most 6 m wide).
        let skip = width / 2.0 + 3.1;
        let start = [from[0] + d[0] * skip, from[1] + d[1] * skip];
        let clear = rect_between(start, to, width / 2.0);
        let side = if d[0] == 0.0 {
            clear.expand(10.0, 0.0, 10.0, 0.0)
        } else {
            clear.expand(0.0, 10.0, 0.0, 10.0)
        };
        if self.taken.iter().any(|r| r.intersects(&clear))
            || self
                .streets
                .iter()
                .any(|s| s.rect().intersects(&side) && !s.rect().contains_point(from))
        {
            return None;
        }
        let steps = 8;
        let mut last = t.height_at(from);
        let mut steepest: f64 = 0.0;
        for k in 1..=steps {
            let f = k as f64 / steps as f64;
            let p = [from[0] + d[0] * length * f, from[1] + d[1] * length * f];
            if wet(self.water(), t, p, width) {
                return None;
            }
            let h = t.height_at(p);
            steepest = steepest.max((h - last).abs() / (length / steps as f64));
            last = h;
        }
        // Too steep for a street: about 30°.
        if steepest > 0.6 {
            return None;
        }
        let climb = (t.height_at(to) - t.height_at(from)).abs() / length;
        (climb <= 0.3).then_some(climb + steepest * 0.3)
    }

    fn lay_street(&mut self, seg: Segment) {
        let r = seg.rect();
        let ground = self.layout.ground_at([r.center().0, r.center().1]);
        self.area(AreaKind::Street, r, ground, 0.05);
        self.streets.push(seg);
        self.taken.push(r);
    }

    /// Grows streets outwards from the piazza, breadth first, lining each with houses as it
    /// goes, until the town has its houses (or nowhere left to go).
    fn grow_streets(&mut self, piazza: Rect) {
        let wanted = self.params.houses as usize;
        let most = wanted.max(4) * 2;
        let branching = 0.2 + 0.3 * self.params.density.clamp(0.0, 1.0);
        let half = piazza.w / 2.0;
        let mut frontier: std::collections::VecDeque<(P2, usize, f64)> =
            std::collections::VecDeque::new();
        let mut arms: Vec<usize> = (0..4).collect();
        // A random first direction, so towns differ even on the same spot.
        let spin = self.rng.below(4);
        arms.rotate_left(spin);
        for dir in arms {
            let d = DIRS[dir];
            frontier.push_back((
                [self.plaza[0] + d[0] * half, self.plaza[1] + d[1] * half],
                dir,
                6.0,
            ));
        }
        while let Some((from, dir, width)) = frontier.pop_front() {
            if self.houses >= wanted || self.streets.len() >= most {
                break;
            }
            let length = self.rng.range(22.0, 34.0);
            // Straight on if it can, otherwise turning, and a shorter run where the full one won't go.
            let mut best: Option<(f64, usize, f64)> = None;
            for (choice, penalty) in [(dir, 0.0), (left(dir), 0.05), (right(dir), 0.05)] {
                for (run, short) in [(length, 0.0), (length * 0.55, 0.08)] {
                    if let Some(cost) = self.probe(from, choice, run, width) {
                        let cost = cost + penalty + short + self.rng.range(0.0, 0.04);
                        if best.is_none_or(|(c, _, _)| cost < c) {
                            best = Some((cost, choice, run));
                        }
                        break;
                    }
                }
            }
            let Some((_, go, length)) = best else {
                continue;
            };
            let d = DIRS[go];
            let to = [from[0] + d[0] * length, from[1] + d[1] * length];
            let seg = Segment {
                a: from,
                b: to,
                width,
            };
            self.lay_street(seg);
            for side in [1.0, -1.0] {
                self.line_side(seg, side);
            }
            let width = 5.0;
            frontier.push_back((to, go, width));
            if self.rng.chance(branching) {
                frontier.push_back((
                    to,
                    if self.rng.chance(0.5) {
                        left(go)
                    } else {
                        right(go)
                    },
                    width,
                ));
            }
        }
    }

    /// What sort of house suits a plot centred near `p`.
    fn kind_at(&self, p: P2) -> HouseKind {
        let scale = (self.params.houses as f64 / 24.0).sqrt().clamp(0.6, 2.0);
        let r = dist(p, self.plaza);
        if let Some(w) = self.water().filter(|w| w.kind == WaterKind::Sea) {
            let h = self.terrain().height_at(p);
            let shore = w.distance(p);
            if shore < 45.0 && h > 8.0 {
                return HouseKind::Cliff;
            }
            if shore < 40.0 && h < 7.0 {
                return HouseKind::Beach;
            }
        }
        if r < 42.0 * scale {
            HouseKind::Palazzo
        } else if r < 95.0 * scale {
            HouseKind::House
        } else {
            HouseKind::Villa
        }
    }

    /// Parameters for a house of `kind`, plus the gap to leave beside it.
    fn house_params(&mut self, kind: HouseKind) -> (BuildingParams, f64) {
        let rng = &mut self.rng;
        let tight = 1.0 - 0.5 * self.params.density.clamp(0.0, 1.0);
        let (building_type, floors, garden, width, depth, gap) = match kind {
            HouseKind::Palazzo => (
                if rng.chance(0.4) {
                    BuildingType::Shop
                } else {
                    BuildingType::Apartment
                },
                3 + rng.below(if self.params.style == ArchitectureStyle::Palermitan {
                    3
                } else {
                    2
                }) as u32,
                Garden::None,
                rng.range(9.0, 13.0),
                rng.range(11.0, 15.0),
                0.0,
            ),
            HouseKind::House => (
                BuildingType::House,
                2 + rng.below(2) as u32,
                *rng.pick(&[Garden::Back, Garden::Front, Garden::None]),
                rng.range(9.0, 13.0),
                rng.range(10.0, 13.0),
                rng.range(1.5, 3.0) * tight,
            ),
            HouseKind::Villa => (
                if rng.chance(0.7) {
                    BuildingType::Villa
                } else {
                    BuildingType::House
                },
                1 + rng.below(2) as u32,
                Garden::Surrounding,
                rng.range(12.0, 16.0),
                rng.range(11.0, 15.0),
                rng.range(4.0, 7.0) * tight,
            ),
            HouseKind::Beach => (
                BuildingType::Villa,
                1 + rng.below(2) as u32,
                Garden::Front,
                rng.range(11.0, 15.0),
                rng.range(10.0, 13.0),
                rng.range(3.0, 5.0) * tight,
            ),
            HouseKind::Cliff => (
                BuildingType::Villa,
                2,
                Garden::Back,
                rng.range(12.0, 15.0),
                rng.range(11.0, 14.0),
                rng.range(4.0, 6.0) * tight,
            ),
        };
        let rooms = ((width * depth / 26.0).round() as u32).clamp(2, 8);
        let params = BuildingParams {
            seed: rng.next_u64(),
            floors,
            size: Size { width, depth },
            building_type,
            rooms,
            garden,
            style: self.params.style,
            shape: Shape::Auto,
        };
        (params, gap)
    }

    /// Lines one side of a street with houses. `side` is +1 for the north or east side.
    fn line_side(&mut self, seg: Segment, side: f64) {
        let horizontal = seg.horizontal();
        let (lo, hi) = if horizontal {
            (seg.a[0].min(seg.b[0]), seg.a[0].max(seg.b[0]))
        } else {
            (seg.a[1].min(seg.b[1]), seg.a[1].max(seg.b[1]))
        };
        let axis = if horizontal { seg.a[1] } else { seg.a[0] };
        // South of a street a house turns to face north, and so on round the compass.
        let quarters = match (horizontal, side > 0.0) {
            (true, true) => 0,
            (false, true) => 1,
            (true, false) => 2,
            (false, false) => 3,
        };
        let mut cursor = lo + seg.width / 2.0;
        let mut misses = 0;
        let end = hi - 4.0;
        while cursor < end - 5.0 && self.houses < self.params.houses as usize && misses < 6 {
            let probe = if horizontal {
                [cursor + 6.0, axis + side * 10.0]
            } else {
                [axis + side * 10.0, cursor + 6.0]
            };
            let kind = self.kind_at(probe);
            let (params, gap) = self.house_params(kind);
            let Some((design, link)) = self.choose_design(&params, end - cursor) else {
                cursor += 3.0;
                misses += 1;
                continue;
            };
            match self.place_house(
                &design, link, kind, quarters, axis, cursor, seg.width, gap, end,
            ) {
                Some(used) => {
                    cursor += used + gap;
                    misses = 0;
                }
                None => {
                    cursor += 3.0;
                    misses += 1;
                }
            }
        }
    }

    /// The house for a plot with `room` metres of street left: a saved design that fits
    /// (as the design mix allows), or a newly generated house. The flag says whether it's a
    /// saved design, whose copies stay linked to it.
    fn choose_design(&mut self, params: &BuildingParams, room: f64) -> Option<(Design, bool)> {
        let saved = match self.params.mix {
            DesignMix::Generated => false,
            DesignMix::Designs => true,
            DesignMix::Mixed => self.rng.chance(0.5),
        };
        if saved && !self.designs.is_empty() {
            let fits: Vec<&Design> = self
                .designs
                .iter()
                .filter(|d| d.lot.w * 0.6 <= room)
                .collect();
            if !fits.is_empty() {
                return Some(((*self.rng.pick(&fits)).clone(), true));
            }
            if self.params.mix == DesignMix::Designs {
                return None;
            }
        }
        generate(params)
            .ok()
            .map(|b| (Design::from_building(&b), false))
    }

    /// Places one house at `cursor` along a street, if it fits. Returns its frontage.
    #[allow(clippy::too_many_arguments)]
    fn place_house(
        &mut self,
        design: &Design,
        link: bool,
        kind: HouseKind,
        quarters: usize,
        axis: f64,
        cursor: f64,
        street_width: f64,
        gap: f64,
        end: f64,
    ) -> Option<f64> {
        // Frontage along the street and depth away from it.
        let (front, deep) = (design.lot.w, design.lot.d);
        // A house may overhang the end of its street a little.
        if cursor + front * 0.6 > end {
            return None;
        }
        let margin = (gap / 2.0).min(0.8);
        let setback = 0.4 + margin;
        let near = street_width / 2.0 + setback;
        let target = match quarters {
            0 => Rect::new(cursor, axis + near, front, deep),
            2 => Rect::new(cursor, axis - near - deep, front, deep),
            1 => Rect::new(axis + near, cursor, deep, front),
            _ => Rect::new(axis - near - deep, cursor, deep, front),
        };
        if !self.free(&target.grow(margin)) {
            return None;
        }
        let t = self.terrain();
        let samples = [
            [target.x, target.z],
            [target.x1(), target.z],
            [target.x, target.z1()],
            [target.x1(), target.z1()],
            [target.center().0, target.center().1],
        ];
        if samples.iter().any(|&p| wet(self.water(), t, p, 2.0)) {
            return None;
        }
        let (low, high) = t.range_in(&target);
        if high - low > 7.0 {
            return None;
        }
        let level = (low + high) / 2.0;
        let goal = target.center();
        let group = design.place(self.layout, [goal.0, goal.1], quarters as u8, level, link);
        // A stone plinth under the whole lot, just below its garden: where the ground
        // falls away it shows as a terrace wall.
        self.layout.add_in(
            0,
            level,
            Some(group),
            PartKind::Area(AreaPart {
                kind: AreaKind::Platform,
                rect: target.grow(margin.min(0.6)),
                height: -0.03,
            }),
        );
        self.pads.push((target, level));
        self.taken.push(target.grow(margin));
        self.houses += 1;
        // Trees round villas: palms by the beach, pines on the cliffs, olives and cypresses inland.
        let behind = match quarters {
            0 => [0.0, 1.0],
            1 => [1.0, 0.0],
            2 => [0.0, -1.0],
            _ => [-1.0, 0.0],
        };
        let (cx, cz) = target.center();
        let reach = deep / 2.0 + 3.5;
        let across = [behind[1], -behind[0]];
        let trees: &[(PropKind, f64)] = match kind {
            HouseKind::Villa => &[
                (PropKind::OliveTree, -0.35),
                (PropKind::OliveTree, 0.0),
                (PropKind::CypressTree, 0.35),
            ],
            HouseKind::Beach => &[(PropKind::Palm, -0.4), (PropKind::Palm, 0.4)],
            HouseKind::Cliff => &[(PropKind::StonePine, 0.0)],
            _ => &[],
        };
        for &(tree, f) in trees {
            let at = [
                cx + behind[0] * reach + across[0] * front * f,
                cz + behind[1] * reach + across[1] * front * f,
            ];
            let spot = Rect::new(at[0] - 1.0, at[1] - 1.0, 2.0, 2.0);
            if self.free(&spot) && !wet(self.water(), self.terrain(), at, 1.0) {
                let scale = self.rng.range(0.8, 1.2);
                self.props.push((tree, at, scale));
                self.taken.push(spot);
            }
        }
        Some(front)
    }

    fn line_streets(&mut self, from: usize) {
        for k in from..self.streets.len() {
            let seg = self.streets[k];
            for side in [1.0, -1.0] {
                self.line_side(seg, side);
            }
        }
    }

    /// Levels every house pad, blending the ground between them, then plants trees and lamps.
    fn finish(mut self) -> TownReport {
        let pads = std::mem::take(&mut self.pads);
        if let Some(t) = self.layout.terrain.as_mut() {
            level_pads(t, &pads, 3.0);
        }
        for (kind, at, scale) in std::mem::take(&mut self.props) {
            let ground = self.layout.ground_at(at);
            let yaw = self.rng.range(0.0, std::f64::consts::TAU);
            self.layout.add_in(
                0,
                ground,
                None,
                PartKind::Prop(PropPart {
                    kind,
                    at,
                    yaw,
                    scale,
                    height: kind.default_height() * scale.sqrt(),
                }),
            );
        }
        TownReport {
            houses: self.houses,
            streets: self.streets.len(),
        }
    }
}

/// Sets the ground under each pad to its height, and blends the ground around the pads
/// back to what it was over `falloff` metres, following the nearest pad. Pads keep their
/// height for one grid step beyond their edges, so the ground between grid points is level
/// too; where pads meet, the ground takes the lowest, so it never rises through a floor.
pub fn level_pads(t: &mut Terrain, pads: &[(Rect, f64)], falloff: f64) {
    if pads.is_empty() {
        return;
    }
    let (sx, sz) = t.spacing();
    let reach = sx.max(sz) * 1.05;
    for j in 0..=t.rows {
        for i in 0..=t.cols {
            let p = t.point(i, j);
            let mut nearest = (f64::MAX, 0.0);
            let mut lowest = f64::MAX;
            for (r, level) in pads {
                let dx = (r.x - p[0]).max(p[0] - r.x1()).max(0.0);
                let dz = (r.z - p[1]).max(p[1] - r.z1()).max(0.0);
                let d = (dx * dx + dz * dz).sqrt();
                if d < nearest.0 {
                    nearest = (d, *level);
                }
                if d <= reach {
                    lowest = lowest.min(*level);
                }
            }
            let (d, level) = nearest;
            let level = if lowest < f64::MAX { lowest } else { level };
            if d >= reach + falloff {
                continue;
            }
            let k = 1.0 - smoothstep(((d - reach) / falloff).max(0.0));
            let h = t.get(i, j);
            t.set(i, j, h + (level - h) * k);
        }
    }
}

/// A good spot for a piazza: the gentlest dry ground near the middle of the map.
pub fn suggest_piazza(layout: &Layout) -> Option<P2> {
    let t = layout.terrain.as_ref()?;
    let (cx, cz) = t.rect.center();
    let reach = (t.rect.w.min(t.rect.d) * 0.3).min(90.0);
    let mut best: Option<(P2, f64)> = None;
    for k in 0..160 {
        // A spread of points over the middle of the map, the same every time.
        let p = [
            cx + (((k * 37) % 61) as f64 / 30.0 - 1.0) * reach,
            cz + (((k * 23) % 47) as f64 / 23.0 - 1.0) * reach,
        ];
        let around: Vec<P2> = (0..9)
            .map(|a| {
                [
                    p[0] + (a % 3) as f64 * 20.0 - 20.0,
                    p[1] + (a / 3) as f64 * 20.0 - 20.0,
                ]
            })
            .collect();
        // Room for the largest piazza and the ring round it, all dry.
        let square = Rect::new(p[0] - 24.0, p[1] - 24.0, 48.0, 48.0);
        if !dry_square(layout.water.as_ref(), t, &square) {
            continue;
        }
        let (lo, hi) = around
            .iter()
            .map(|&q| t.height_at(q))
            .fold((f64::MAX, f64::MIN), |(a, b), h| (a.min(h), b.max(h)));
        // Gentle ground, and not too far from the middle.
        let cost = hi - lo + 0.02 * ((p[0] - cx).abs() + (p[1] - cz).abs());
        if best.is_none_or(|(_, c)| cost < c) {
            best = Some((p, cost));
        }
    }
    best.map(|(p, _)| p)
}

/// Grows a town around a piazza at `plaza` on the layout's landscape, using `designs` as
/// the design mix allows.
pub fn grow_town(
    layout: &mut Layout,
    plaza: P2,
    params: &TownParams,
    designs: &[Design],
) -> Result<TownReport, TownError> {
    let mut town = Town::new(layout, plaza, params, designs);
    let piazza = town.piazza()?;
    town.grow_streets(piazza);
    Ok(town.finish())
}

/// Lays a street from `a` towards `b` (straightened onto the nearer axis) and lines it with
/// houses. Returns how many houses were built.
pub fn fill_street(
    layout: &mut Layout,
    a: P2,
    b: P2,
    params: &TownParams,
    designs: &[Design],
) -> TownReport {
    let b = if (b[0] - a[0]).abs() >= (b[1] - a[1]).abs() {
        [b[0], a[1]]
    } else {
        [a[0], b[1]]
    };
    // Houses take their character from the nearest piazza, if there is one.
    let plaza = layout
        .parts
        .iter()
        .find_map(|p| match &p.kind {
            PartKind::Area(area) if area.kind == AreaKind::Piazza => {
                Some([area.rect.center().0, area.rect.center().1])
            }
            _ => None,
        })
        .unwrap_or([(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0]);
    let mut town = Town::new(layout, plaza, params, designs);
    let seg = Segment { a, b, width: 5.0 };
    town.lay_street(seg);
    town.line_streets(0);
    town.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::landscape::{LandscapeParams, Relief, WaterBody, generate_landscape};
    use crate::style::{Palette, StyleProfile};

    fn landscape(relief: Relief, water: WaterBody) -> Layout {
        let (terrain, water) = generate_landscape(&LandscapeParams {
            relief,
            water,
            size: 320.0,
            seed: 7,
        });
        let style = ArchitectureStyle::Florentine;
        let mut layout = Layout::new(
            "town",
            StyleProfile::for_style(style),
            Palette::choose(style, &mut Rng::new(1)),
        );
        layout.terrain = Some(terrain);
        layout.water = water;
        layout
    }

    fn dry_spot(layout: &Layout) -> P2 {
        suggest_piazza(layout).expect("somewhere dry")
    }

    /// Each house's ground-floor outline, with the height it stands at.
    fn houses_at(layout: &Layout) -> Vec<(Rect, f64)> {
        let mut out: Vec<(u32, Rect, f64)> = Vec::new();
        for p in layout.parts.iter().filter(|p| p.level == 0) {
            if let (Some(g), PartKind::Floor(f)) = (p.group, &p.kind) {
                match out.iter_mut().find(|(k, _, _)| *k == g) {
                    Some((_, r, _)) => *r = r.union(&f.rect),
                    None => out.push((g, f.rect, p.ground)),
                }
            }
        }
        out.into_iter().map(|(_, r, h)| (r, h)).collect()
    }

    fn houses(layout: &Layout) -> Vec<Rect> {
        houses_at(layout).into_iter().map(|(r, _)| r).collect()
    }

    #[test]
    fn towns_grow_on_every_landscape() {
        for relief in Relief::ALL {
            for water in WaterBody::ALL {
                let mut layout = landscape(relief, water);
                let plaza = dry_spot(&layout);
                let report = grow_town(
                    &mut layout,
                    plaza,
                    &TownParams {
                        houses: 20,
                        ..TownParams::default()
                    },
                    &[],
                )
                .unwrap_or_else(|e| panic!("{relief:?} {water:?}: {e}"));
                assert!(
                    report.streets >= 2,
                    "{relief:?} {water:?}: streets ({report:?})"
                );
                assert!(
                    report.houses >= 6,
                    "{relief:?} {water:?}: houses ({report:?})"
                );
                let t = layout.terrain.as_ref().unwrap();
                // The piazza is level.
                let piazza = layout
                    .parts
                    .iter()
                    .find_map(|p| match &p.kind {
                        PartKind::Area(a) if a.kind == AreaKind::Piazza => Some((a.rect, p.ground)),
                        _ => None,
                    })
                    .unwrap();
                let (lo, hi) = t.range_in(&piazza.0);
                assert!(
                    hi - lo < 0.2,
                    "{relief:?} {water:?}: piazza level ({lo}..{hi})"
                );
                // Houses don't overlap each other or the streets, and stand on their pads.
                let (homes, levels): (Vec<Rect>, Vec<f64>) = houses_at(&layout).into_iter().unzip();
                let streets: Vec<Rect> = layout
                    .parts
                    .iter()
                    .filter_map(|p| match &p.kind {
                        PartKind::Area(a) if a.kind == AreaKind::Street => Some(a.rect),
                        _ => None,
                    })
                    .collect();
                for (i, a) in homes.iter().enumerate() {
                    for b in &homes[i + 1..] {
                        assert!(!a.intersects(b), "{relief:?} {water:?}: houses overlap");
                    }
                    for s in &streets {
                        assert!(
                            !a.intersects(s),
                            "{relief:?} {water:?}: a house stands in the street"
                        );
                    }
                    let level = levels[i];
                    let (_, hi) = t.range_in(a);
                    assert!(
                        hi < level + 0.05,
                        "{relief:?} {water:?}: the ground pokes through a floor ({hi} > {level})"
                    );
                    let middle = t.height_at([a.center().0, a.center().1]);
                    assert!(
                        (middle - level).abs() < 0.05,
                        "{relief:?} {water:?}: a house stands on its pad ({middle} vs {level})"
                    );
                    let c = [a.center().0, a.center().1];
                    assert!(
                        !wet(layout.water.as_ref(), t, c, 0.0),
                        "{relief:?} {water:?}: a house in the water"
                    );
                }
                // Streets stay dry.
                for s in &streets {
                    let c = [s.center().0, s.center().1];
                    if let Some(w) = &layout.water {
                        assert!(
                            !w.is_wet(c, t.height_at(c)),
                            "{relief:?} {water:?}: a street in the water"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn towns_are_deterministic_and_vary_by_seed() {
        let make = |seed| {
            let mut layout = landscape(Relief::Hills, WaterBody::Beach);
            let plaza = dry_spot(&layout);
            grow_town(
                &mut layout,
                plaza,
                &TownParams {
                    seed,
                    ..TownParams::default()
                },
                &[],
            )
            .unwrap();
            layout
        };
        assert_eq!(make(3), make(3));
        assert_ne!(make(3), make(4));
    }

    #[test]
    fn the_seaside_gets_beach_and_cliff_houses() {
        let mut beach = 0;
        let mut cliff = 0;
        for (water, seed) in [
            (WaterBody::Beach, 1),
            (WaterBody::Cliffs, 2),
            (WaterBody::BeachAndCliffs, 3),
        ] {
            let mut layout = landscape(Relief::Flat, water);
            // Close to the shore.
            let w = layout.water.clone().unwrap();
            let mut plaza = [0.0, -40.0];
            while w.distance(plaza) < 35.0 {
                plaza[1] += 4.0;
            }
            let town = Town::new(
                &mut layout,
                plaza,
                &TownParams {
                    seed,
                    ..TownParams::default()
                },
                &[],
            );
            for x in (-60..=60).step_by(10) {
                let p = [x as f64, plaza[1] - 10.0];
                match town.kind_at(p) {
                    HouseKind::Beach => beach += 1,
                    HouseKind::Cliff => cliff += 1,
                    _ => {}
                }
            }
            drop(town);
            grow_town(
                &mut layout,
                plaza,
                &TownParams {
                    seed,
                    ..TownParams::default()
                },
                &[],
            )
            .unwrap();
        }
        assert!(beach > 0, "beach houses by the sand");
        assert!(cliff > 0, "villas on the cliffs");
    }

    #[test]
    fn towns_arrange_saved_designs() {
        let b = generate(&BuildingParams {
            garden: Garden::Back,
            seed: 4,
            ..BuildingParams::default()
        })
        .unwrap();
        let mut d = Design::from_building(&b);
        d.id = "casa".into();
        d.name = "Casa".into();
        let mut layout = landscape(Relief::Flat, WaterBody::None);
        let plaza = dry_spot(&layout);
        let params = TownParams {
            houses: 12,
            mix: DesignMix::Designs,
            ..TownParams::default()
        };
        let report = grow_town(&mut layout, plaza, &params, std::slice::from_ref(&d)).unwrap();
        assert!(report.houses >= 6, "{report:?}");
        let copies = layout
            .modules
            .iter()
            .filter(|m| m.design.as_deref() == Some("casa"))
            .count();
        assert_eq!(copies, report.houses, "every house is a copy of the design");
        // Each copy, saved again, is the design (turned to face its street).
        for g in 0..layout.modules.len() as u32 {
            if layout.module(g).is_some_and(|m| m.is_copy()) {
                let again = Design::capture(&layout, g, "casa", "Casa").unwrap();
                assert_eq!(
                    again.parts.len(),
                    d.parts.len(),
                    "the design (its plinth stays with the site)"
                );
                assert!(
                    (again.lot.w - d.lot.w).abs() < 1e-6 && (again.lot.d - d.lot.d).abs() < 1e-6
                );
            }
        }
        // Mixed towns use both.
        let mut layout = landscape(Relief::Flat, WaterBody::None);
        let params = TownParams {
            houses: 24,
            mix: DesignMix::Mixed,
            ..TownParams::default()
        };
        grow_town(&mut layout, plaza, &params, std::slice::from_ref(&d)).unwrap();
        let copies = layout.modules.iter().filter(|m| m.is_copy()).count();
        assert!(
            copies > 0 && copies < layout.modules.len(),
            "{copies} of {}",
            layout.modules.len()
        );
    }

    #[test]
    fn piazzas_refuse_water() {
        let mut layout = landscape(Relief::Flat, WaterBody::Beach);
        let t = layout.terrain.as_ref().unwrap();
        let sea = [0.0, t.rect.z + 40.0];
        assert_eq!(
            grow_town(&mut layout, sea, &TownParams::default(), &[]),
            Err(TownError::InWater)
        );
    }

    #[test]
    fn a_drawn_street_fills_with_houses() {
        let mut layout = landscape(Relief::Flat, WaterBody::None);
        let report = fill_street(
            &mut layout,
            [-60.0, 20.0],
            [60.0, 24.0],
            &TownParams::default(),
            &[],
        );
        assert_eq!(report.streets, 1);
        assert!(report.houses >= 6, "{report:?}");
        assert!(
            houses(&layout)
                .iter()
                .all(|r| r.z >= 22.0 || r.z1() <= 18.0),
            "houses keep off the street"
        );
    }
}
