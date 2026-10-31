//! Whole landscapes to build towns on: the lie of the land (flat, hills, mountains or a
//! valley) and its water (a sea with a beach or cliffs, a lake or a river).
//!
//! The map is a square centred on the origin. The sea always lies to the south (−z), the
//! way generated houses face, so seafront houses look out over it. Ground kinds (grass,
//! sand, rock, scree) are worked out from height, slope and water by [`ground_map`].

use std::f64::consts::TAU;

use clap::ValueEnum;
use serde::{Deserialize, Serialize};

use crate::geometry::Rect;
use crate::mesh::{Material, MeshSet, V3, v3};
use crate::parts::P2;
use crate::rng::Rng;
use crate::terrain::{Terrain, hills, smooth_noise, smoothstep};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum Relief {
    /// Nearly level ground.
    Flat,
    /// Rolling hills.
    #[default]
    Hills,
    /// Ridged peaks of 40–80 m, rising away from the coast.
    Mountains,
    /// A broad valley between two ridges.
    Valley,
}

impl Relief {
    pub const ALL: [Relief; 4] = [
        Relief::Flat,
        Relief::Hills,
        Relief::Mountains,
        Relief::Valley,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Relief::Flat => "Flat",
            Relief::Hills => "Hills",
            Relief::Mountains => "Mountains",
            Relief::Valley => "Valley",
        }
    }
}

/// Which body of water the landscape has.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum WaterBody {
    None,
    /// The sea, with a sandy beach.
    #[default]
    Beach,
    /// The sea, below cliffs.
    Cliffs,
    /// The sea, with a coast that alternates between beaches and cliffs.
    BeachAndCliffs,
    Lake,
    River,
}

impl WaterBody {
    pub const ALL: [WaterBody; 6] = [
        WaterBody::None,
        WaterBody::Beach,
        WaterBody::Cliffs,
        WaterBody::BeachAndCliffs,
        WaterBody::Lake,
        WaterBody::River,
    ];

    pub fn label(self) -> &'static str {
        match self {
            WaterBody::None => "None",
            WaterBody::Beach => "Sea, beach",
            WaterBody::Cliffs => "Sea, cliffs",
            WaterBody::BeachAndCliffs => "Sea, beach + cliffs",
            WaterBody::Lake => "Lake",
            WaterBody::River => "River",
        }
    }

    pub fn is_sea(self) -> bool {
        matches!(
            self,
            WaterBody::Beach | WaterBody::Cliffs | WaterBody::BeachAndCliffs
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct LandscapeParams {
    pub relief: Relief,
    pub water: WaterBody,
    /// Side of the square map, in metres.
    pub size: f64,
    pub seed: u64,
}

impl Default for LandscapeParams {
    fn default() -> Self {
        Self {
            relief: Relief::Hills,
            water: WaterBody::Beach,
            size: 320.0,
            seed: 1,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WaterKind {
    Sea,
    Lake,
    River,
}

/// A body of water lying on the terrain.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Water {
    pub kind: WaterKind,
    /// Surface height of a sea or lake.
    pub level: f64,
    /// The sea's surface, or the bounds of a lake or river.
    pub area: Rect,
    /// The sea's shoreline (west to east), a lake's shore (a closed loop), or a river's
    /// centre line from upstream down.
    #[serde(default)]
    pub path: Vec<P2>,
    /// A river's surface height at each point of `path`.
    #[serde(default)]
    pub surface: Vec<f64>,
    /// A river's width.
    #[serde(default)]
    pub width: f64,
}

/// Distance from `p` to the segment `a`–`b`, and how far along it the nearest point lies (0–1).
fn to_segment(p: P2, a: P2, b: P2) -> (f64, f64) {
    let (dx, dz) = (b[0] - a[0], b[1] - a[1]);
    let len2 = dx * dx + dz * dz;
    let t = if len2 < 1e-12 {
        0.0
    } else {
        (((p[0] - a[0]) * dx + (p[1] - a[1]) * dz) / len2).clamp(0.0, 1.0)
    };
    let (qx, qz) = (a[0] + dx * t, a[1] + dz * t);
    (((p[0] - qx).powi(2) + (p[1] - qz).powi(2)).sqrt(), t)
}

fn inside_loop(p: P2, ring: &[P2]) -> bool {
    let mut inside = false;
    let mut j = ring.len() - 1;
    for i in 0..ring.len() {
        let (a, b) = (ring[i], ring[j]);
        if (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0]
        {
            inside = !inside;
        }
        j = i;
    }
    inside
}

impl Water {
    /// A river's distance from `p` to its centre line, with the surface height there.
    fn river_nearest(&self, p: P2) -> (f64, f64) {
        let mut best = (f64::MAX, 0.0);
        for k in 1..self.path.len() {
            let (d, t) = to_segment(p, self.path[k - 1], self.path[k]);
            if d < best.0 {
                best = (
                    d,
                    self.surface[k - 1] + (self.surface[k] - self.surface[k - 1]) * t,
                );
            }
        }
        best
    }

    /// Distance from `p` to the water's edge: 0 on or over the water.
    pub fn distance(&self, p: P2) -> f64 {
        match self.kind {
            WaterKind::Sea => {
                // The shoreline runs west to east; everything south of it is sea.
                let shore = &self.path;
                if shore.len() < 2 {
                    return (p[1] - self.area.z1()).max(0.0);
                }
                let k = shore
                    .partition_point(|q| q[0] < p[0])
                    .clamp(1, shore.len() - 1);
                let (a, b) = (shore[k - 1], shore[k]);
                let t = ((p[0] - a[0]) / (b[0] - a[0])).clamp(0.0, 1.0);
                if p[1] <= a[1] + (b[1] - a[1]) * t {
                    return 0.0;
                }
                shore
                    .windows(2)
                    .map(|w| to_segment(p, w[0], w[1]).0)
                    .fold(f64::MAX, f64::min)
            }
            WaterKind::Lake => {
                if inside_loop(p, &self.path) {
                    return 0.0;
                }
                let n = self.path.len();
                (0..n)
                    .map(|k| to_segment(p, self.path[k], self.path[(k + 1) % n]).0)
                    .fold(f64::MAX, f64::min)
            }
            WaterKind::River => (self.river_nearest(p).0 - self.width / 2.0).max(0.0),
        }
    }

    /// The water's surface height within `margin` metres of `p`, if there is water there.
    pub fn surface_near(&self, p: P2, margin: f64) -> Option<f64> {
        match self.kind {
            WaterKind::Sea => Some(self.level),
            WaterKind::Lake => (self.distance(p) <= margin).then_some(self.level),
            WaterKind::River => {
                let (d, s) = self.river_nearest(p);
                (d <= self.width / 2.0 + margin).then_some(s)
            }
        }
    }

    /// Whether ground at height `ground` at `p` lies under the water.
    pub fn is_wet(&self, p: P2, ground: f64) -> bool {
        self.surface_near(p, 0.0).is_some_and(|s| ground < s)
    }

    /// Raises (or with a negative `rise`, lowers) the water's surface.
    pub fn shift(&mut self, rise: f64) {
        self.level += rise;
        for s in &mut self.surface {
            *s += rise;
        }
    }

    /// The water surface, in [`Material::Water`].
    pub fn mesh(&self) -> MeshSet {
        let mut set = MeshSet::default();
        match self.kind {
            WaterKind::Sea => {
                let a = self.area;
                let y = self.level;
                set.polygon(
                    Material::Water,
                    &[
                        v3(a.x, y, a.z),
                        v3(a.x1(), y, a.z),
                        v3(a.x1(), y, a.z1()),
                        v3(a.x, y, a.z1()),
                    ],
                    V3::UP,
                );
            }
            WaterKind::Lake => {
                let n = self.path.len() as f64;
                let c = self
                    .path
                    .iter()
                    .fold([0.0, 0.0], |c, p| [c[0] + p[0] / n, c[1] + p[1] / n]);
                let y = self.level;
                for k in 0..self.path.len() {
                    let (a, b) = (self.path[k], self.path[(k + 1) % self.path.len()]);
                    set.polygon(
                        Material::Water,
                        &[v3(c[0], y, c[1]), v3(a[0], y, a[1]), v3(b[0], y, b[1])],
                        V3::UP,
                    );
                }
            }
            WaterKind::River => {
                let half = self.width / 2.0 + 0.3;
                let edge = |k: usize| {
                    let (a, b) = (
                        self.path[k.saturating_sub(1)],
                        self.path[(k + 1).min(self.path.len() - 1)],
                    );
                    let (dx, dz) = (b[0] - a[0], b[1] - a[1]);
                    let len = (dx * dx + dz * dz).sqrt().max(1e-9);
                    let (nx, nz) = (-dz / len * half, dx / len * half);
                    let (p, y) = (self.path[k], self.surface[k]);
                    (v3(p[0] + nx, y, p[1] + nz), v3(p[0] - nx, y, p[1] - nz))
                };
                for k in 1..self.path.len() {
                    let (l0, r0) = edge(k - 1);
                    let (l1, r1) = edge(k);
                    set.polygon(Material::Water, &[l0, r0, r1], V3::UP);
                    set.polygon(Material::Water, &[l0, r1, l1], V3::UP);
                }
            }
        }
        set
    }
}

/// What the ground is made of at a point of the terrain.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Ground {
    Grass,
    /// Dry earth on steeper slopes.
    Earth,
    Sand,
    /// Cliffs and crags.
    Rock,
    /// Pale loose stone on high peaks.
    Scree,
}

impl Ground {
    pub fn material(self) -> Material {
        match self {
            Ground::Grass => Material::Lawn,
            Ground::Earth => Material::Earth,
            Ground::Sand => Material::Sand,
            Ground::Rock => Material::Rock,
            Ground::Scree => Material::Gravel,
        }
    }

    /// A colour for the ground (sRGB), for engines that tint one shared ground texture.
    pub fn colour(self) -> [f32; 3] {
        match self {
            Ground::Grass => [0.36, 0.47, 0.2],
            Ground::Earth => [0.62, 0.52, 0.36],
            Ground::Sand => [0.88, 0.8, 0.6],
            Ground::Rock => [0.5, 0.47, 0.42],
            Ground::Scree => [0.74, 0.72, 0.66],
        }
    }
}

/// The ground kind at every grid point of `t`, row-major like its heights.
pub fn ground_map(t: &Terrain, water: Option<&Water>) -> Vec<Ground> {
    let (lo, hi) = t.heights.iter().fold((f64::MAX, f64::MIN), |(lo, hi), &h| {
        (lo.min(h as f64), hi.max(h as f64))
    });
    // Only real mountains get scree, on their steeper tops (not on a flat cliff-top plateau).
    let scree_above = if hi - lo > 35.0 {
        hi - (hi - lo) * 0.22
    } else {
        f64::MAX
    };
    let (sx, sz) = t.spacing();
    let mut out = Vec::with_capacity(t.heights.len());
    for j in 0..=t.rows {
        for i in 0..=t.cols {
            let h = t.get(i, j);
            let dx = (t.get((i + 1).min(t.cols), j) - t.get(i.saturating_sub(1), j)) / (2.0 * sx);
            let dz = (t.get(i, (j + 1).min(t.rows)) - t.get(i, j.saturating_sub(1))) / (2.0 * sz);
            let grade = (dx * dx + dz * dz).sqrt();
            let p = t.point(i, j);
            let shore = water
                .and_then(|w| {
                    w.surface_near(
                        p,
                        if w.kind == WaterKind::River {
                            w.width * 0.3
                        } else {
                            3.0
                        },
                    )
                })
                .is_some_and(|s| h < s + 1.6 && grade < 0.45);
            out.push(if t.sand_at(i, j) > 0.5 || (shore && grade < 0.45) {
                Ground::Sand
            } else if grade > 1.1 {
                Ground::Rock
            } else if h > scree_above && grade > 0.25 {
                Ground::Scree
            } else if grade > 0.55 {
                Ground::Earth
            } else {
                Ground::Grass
            });
        }
    }
    out
}

/// Ridged noise in `[0, 1]`: sharp crests where smooth noise crosses its middle.
fn ridges(x: f64, z: f64, scale: f64, seed: u64) -> f64 {
    let (mut sum, mut amp, mut norm, mut freq) = (0.0, 1.0, 0.0, 1.0 / scale);
    for octave in 0..5 {
        let n = smooth_noise(x * freq, z * freq, seed + 31 + octave);
        let ridge = 1.0 - (2.0 * n - 1.0).abs();
        sum += ridge * ridge * amp;
        norm += amp;
        amp *= 0.5;
        freq *= 2.0;
    }
    sum / norm
}

fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

/// How far a valley's floor swings east and west along its length.
fn valley_meander(z: f64, size: f64, seed: u64) -> f64 {
    size * 0.08 * (z / size * TAU * 0.8 + seed as f64 * 0.37).sin()
}

fn relief_height(relief: Relief, p: P2, rect: &Rect, seed: u64) -> f64 {
    let size = rect.w;
    let (x, z) = (p[0], p[1]);
    let north = (z - rect.z) / rect.d;
    match relief {
        Relief::Flat => 1.2 * hills(x, z, size / 3.0, seed),
        Relief::Hills => {
            14.0 * hills(x, z, size / 3.5, seed) + 3.0 * hills(x, z, size / 10.0, seed + 11)
        }
        Relief::Mountains => {
            let rise = 0.25 + 0.75 * smoothstep(north);
            let peaks = 40.0 + 40.0 * smooth_noise(0.5, seed as f64 * 0.13, seed + 3);
            peaks * rise * ridges(x, z, size / 2.2, seed).powf(1.3)
                + 4.0 * hills(x, z, size / 8.0, seed + 5)
        }
        Relief::Valley => {
            let across = ((x - valley_meander(z, size, seed)).abs() / (size / 2.0)).min(1.2);
            45.0 * across.powf(1.6) + 4.0 * hills(x, z, size / 6.0, seed + 7) + 4.0 * north
        }
    }
}

/// An empty layout on a new landscape, ready for a town in `style`.
pub fn landscape_layout(
    params: &LandscapeParams,
    style: crate::ArchitectureStyle,
) -> crate::Layout {
    let (terrain, water) = generate_landscape(params);
    let mut layout = crate::Layout::new(
        format!("landscape_{}", params.seed),
        crate::style::StyleProfile::for_style(style),
        crate::style::Palette::choose(style, &mut Rng::new(params.seed)),
    );
    layout.description = format!(
        "{} · {} · seed {}",
        params.relief.label(),
        params.water.label(),
        params.seed
    );
    layout.terrain = Some(terrain);
    layout.water = water;
    layout
}

/// A landscape: the terrain and its water, ready to become a [`crate::Layout`]'s ground.
pub fn generate_landscape(params: &LandscapeParams) -> (Terrain, Option<Water>) {
    let size = params.size.clamp(120.0, 800.0);
    let rect = Rect::new(-size / 2.0, -size / 2.0, size, size);
    let mut t = Terrain::flat(rect, (size / 128.0).max(2.0));
    let seed = params.seed;
    for j in 0..=t.rows {
        for i in 0..=t.cols {
            let h = relief_height(params.relief, t.point(i, j), &rect, seed);
            t.set(i, j, h);
        }
    }
    let water = match params.water {
        WaterBody::None => None,
        WaterBody::Beach | WaterBody::Cliffs | WaterBody::BeachAndCliffs => {
            Some(sea(&mut t, params.water, seed))
        }
        WaterBody::Lake => Some(lake(&mut t, seed)),
        WaterBody::River => Some(river(&mut t, params.relief, seed)),
    };
    (t, water)
}

/// The sea along the south edge. Inland the ground keeps its relief, lifted to stand above
/// the water; at the shore it runs down a sandy beach or drops off a cliff.
fn sea(t: &mut Terrain, body: WaterBody, seed: u64) -> Water {
    let rect = t.rect;
    let size = rect.w;
    let shore = |x: f64| rect.z + size * 0.24 + size * 0.05 * hills(x, 0.0, size / 3.0, seed + 5);
    let cliffiness = |x: f64| match body {
        WaterBody::Cliffs => 1.0,
        WaterBody::BeachAndCliffs => smoothstep(
            ((smooth_noise(x / (size / 3.0), 3.7, seed + 9) - 0.35) / 0.3).clamp(0.0, 1.0),
        ),
        _ => 0.0,
    };
    let cliff_height = |x: f64| 14.0 + 10.0 * smooth_noise(x / (size / 5.0), 1.3, seed + 13);
    let lowest = t.heights.iter().fold(f64::MAX, |m, &h| m.min(h as f64));
    t.sand = vec![0; t.heights.len()];
    for j in 0..=t.rows {
        for i in 0..=t.cols {
            let p = t.point(i, j);
            let land = t.get(i, j) - lowest + 2.0;
            let d = p[1] - shore(p[0]);
            let beach = if d < 0.0 {
                (-0.3 + d * 0.05).max(-6.0)
            } else {
                lerp(-0.3 + d * 0.05, land, smoothstep((d / 45.0).min(1.0)))
            };
            let top = land + cliff_height(p[0]);
            let cliff = if d < -4.0 {
                (-6.0 + d * 0.02).max(-9.0)
            } else if d < 3.0 {
                lerp(-6.0, top, smoothstep((d + 4.0) / 7.0))
            } else {
                top
            };
            let c = cliffiness(p[0]);
            t.set(i, j, lerp(beach, cliff, c));
            if c < 0.5 && d > -8.0 && beach < 2.5 {
                t.sand[j * (t.cols + 1) + i] = (255.0 * (1.0 - 2.0 * c)) as u8;
            }
        }
    }
    let shoreline: Vec<P2> = (0..=64)
        .map(|k| {
            let x = rect.x + rect.w * k as f64 / 64.0;
            [x, shore(x)]
        })
        .collect();
    let reach = shoreline.iter().map(|p| p[1]).fold(f64::MIN, f64::max);
    Water {
        kind: WaterKind::Sea,
        level: 0.0,
        area: Rect::new(rect.x, rect.z - size, rect.w, reach + 4.0 - (rect.z - size)),
        path: shoreline,
        surface: Vec::new(),
        width: 0.0,
    }
}

/// A lake carved into the lowest ground near the middle of the map, ringed by a low bank.
fn lake(t: &mut Terrain, seed: u64) -> Water {
    let rect = t.rect;
    let size = rect.w;
    // The lowest point in the middle half of the map.
    let mut centre = [0.0, 0.0];
    let mut lowest = f64::MAX;
    for j in 0..=t.rows {
        for i in 0..=t.cols {
            let p = t.point(i, j);
            if p[0].abs() < size * 0.25 && p[1].abs() < size * 0.25 && t.get(i, j) < lowest {
                lowest = t.get(i, j);
                centre = p;
            }
        }
    }
    let base_radius = size * 0.13;
    let radius = |a: f64| {
        base_radius
            * (1.0
                + 0.36 * (smooth_noise(a.cos() * 1.5 + 5.0, a.sin() * 1.5 + 5.0, seed + 3) - 0.5))
    };
    let at = |a: f64, r: f64| [centre[0] + a.cos() * r, centre[1] + a.sin() * r];
    let level = (0..48)
        .map(|k| {
            let a = TAU * k as f64 / 48.0;
            t.height_at(at(a, radius(a) * 1.3))
        })
        .fold(f64::MAX, f64::min)
        - 0.4;
    let depth = 4.0;
    for j in 0..=t.rows {
        for i in 0..=t.cols {
            let p = t.point(i, j);
            let (dx, dz) = (p[0] - centre[0], p[1] - centre[1]);
            let r = (dx * dx + dz * dz).sqrt() / radius(dz.atan2(dx));
            let h = t.get(i, j);
            let carved = if r < 1.0 {
                h.min(level - depth * (1.0 - r * r))
            } else if r < 1.3 {
                let s = smoothstep((r - 1.0) / 0.3);
                level + 0.1 + (h.max(level + 0.1) - level - 0.1) * s
            } else {
                continue;
            };
            t.set(i, j, carved);
        }
    }
    let path: Vec<P2> = (0..64)
        .map(|k| {
            let a = TAU * k as f64 / 64.0;
            at(a, radius(a) * 1.08)
        })
        .collect();
    let (x0, z0, x1, z1) = path.iter().fold(
        (f64::MAX, f64::MAX, f64::MIN, f64::MIN),
        |(a, b, c, d), p| (a.min(p[0]), b.min(p[1]), c.max(p[0]), d.max(p[1])),
    );
    Water {
        kind: WaterKind::Lake,
        level,
        area: Rect::new(x0, z0, x1 - x0, z1 - z0),
        path,
        surface: Vec::new(),
        width: 0.0,
    }
}

/// A river meandering from the north edge to the south, always running downhill, in a channel
/// cut into the ground with banks either side.
fn river(t: &mut Terrain, relief: Relief, seed: u64) -> Water {
    let rect = t.rect;
    let size = rect.w;
    let mut rng = Rng::new(seed ^ 0x5249_5645);
    let phase = rng.range(0.0, TAU);
    let spacing = t.spacing().0;
    let width = rng.range(9.0, 14.0).max(spacing * 4.0);
    let centre_x = |z: f64| match relief {
        Relief::Valley => valley_meander(z, size, seed),
        _ => {
            size * 0.12 * (z / size * TAU * 0.9 + phase).sin()
                + size * 0.05 * hills(0.0, z, size / 4.0, seed + 21)
        }
    };
    let step = spacing * 1.5;
    let count = (rect.d / step).ceil() as usize;
    let path: Vec<P2> = (0..=count)
        .map(|k| {
            let z = rect.z1() - rect.d * k as f64 / count as f64;
            [centre_x(z), z]
        })
        .collect();
    let mut surface = Vec::with_capacity(path.len());
    let mut running = f64::MAX;
    for p in &path {
        running = running.min(t.height_at(*p) - 1.2);
        surface.push(running);
    }
    let water = Water {
        kind: WaterKind::River,
        level: *surface.last().unwrap_or(&0.0),
        area: Rect::new(rect.x, rect.z, rect.w, rect.d),
        path,
        surface,
        width,
    };
    // The bed stays a grid step inside the water's edge and the banks stand above the water
    // for a few steps beyond it, so the shoreline between grid points never dips under.
    let half = width / 2.0;
    let wet = half - spacing;
    let held = half + 2.5 * spacing;
    let bank = width * 1.2 + 2.5 * spacing;
    for j in 0..=t.rows {
        for i in 0..=t.cols {
            let p = t.point(i, j);
            let (d, s) = water.river_nearest(p);
            if d >= wet + bank {
                continue;
            }
            let h = t.get(i, j);
            let carved = if d < wet {
                h.min(s - 0.15 - 1.6 * (1.0 - (d / wet).powi(2)))
            } else {
                let shaped = lerp(s + 0.2, h, smoothstep(((d - wet) / bank).min(1.0)));
                if d < held {
                    shaped.max(s + 0.2)
                } else {
                    shaped
                }
            };
            t.set(i, j, carved);
        }
    }
    let (x0, x1) = water
        .path
        .iter()
        .fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p[0]), b.max(p[0])));
    Water {
        area: Rect::new(x0 - width, rect.z, x1 - x0 + 2.0 * width, rect.d),
        ..water
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make(relief: Relief, water: WaterBody) -> (Terrain, Option<Water>) {
        generate_landscape(&LandscapeParams {
            relief,
            water,
            size: 240.0,
            seed: 4,
        })
    }

    #[test]
    fn landscapes_are_deterministic() {
        for relief in Relief::ALL {
            for water in WaterBody::ALL {
                assert_eq!(
                    make(relief, water),
                    make(relief, water),
                    "{relief:?} {water:?}"
                );
            }
        }
    }

    #[test]
    fn the_sea_floods_the_south_edge() {
        for relief in Relief::ALL {
            for body in [
                WaterBody::Beach,
                WaterBody::Cliffs,
                WaterBody::BeachAndCliffs,
            ] {
                let (t, w) = make(relief, body);
                let w = w.unwrap();
                assert_eq!(w.kind, WaterKind::Sea);
                for i in 0..=t.cols {
                    assert!(
                        t.get(i, 0) < w.level,
                        "{relief:?} {body:?}: the south edge is under water"
                    );
                    assert!(
                        t.get(i, t.rows) > w.level,
                        "{relief:?} {body:?}: the north edge is dry land"
                    );
                }
                // Cliffs drop several metres within a few metres of the shore; beaches don't.
                let steepest = (0..t.rows)
                    .map(|j| (t.get(t.cols / 2, j + 1) - t.get(t.cols / 2, j)) / t.spacing().1)
                    .fold(0.0, f64::max);
                match body {
                    WaterBody::Cliffs => {
                        assert!(steepest > 2.0, "{relief:?}: cliffs are steep ({steepest})")
                    }
                    WaterBody::Beach if relief != Relief::Mountains => {
                        assert!(
                            t.sand.iter().any(|&s| s > 200),
                            "{relief:?}: the beach is sandy"
                        )
                    }
                    _ => {}
                }
            }
        }
    }

    #[test]
    fn the_lake_is_enclosed() {
        for relief in Relief::ALL {
            let (t, w) = make(relief, WaterBody::Lake);
            let w = w.unwrap();
            for p in &w.path {
                assert!(
                    t.height_at(*p) > w.level,
                    "{relief:?}: the shore stands above the water"
                );
            }
            let n = w.path.len() as f64;
            let c = w
                .path
                .iter()
                .fold([0.0, 0.0], |c, p| [c[0] + p[0] / n, c[1] + p[1] / n]);
            assert!(
                t.height_at(c) < w.level - 1.0,
                "{relief:?}: the middle is deep"
            );
            assert!(w.is_wet(c, t.height_at(c)));
        }
    }

    #[test]
    fn the_river_runs_downhill_below_its_banks() {
        for relief in Relief::ALL {
            let (t, w) = make(relief, WaterBody::River);
            let w = w.unwrap();
            assert!(
                w.surface.windows(2).all(|s| s[1] <= s[0]),
                "{relief:?}: flows downhill"
            );
            for (k, (p, s)) in w.path.iter().zip(&w.surface).enumerate() {
                assert!(
                    t.height_at(*p) < *s,
                    "{relief:?}: the bed lies under the water"
                );
                // A step across the river, square to its course.
                let (a, b) = (
                    w.path[k.saturating_sub(1)],
                    w.path[(k + 1).min(w.path.len() - 1)],
                );
                let len = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
                let off = w.width / 2.0 + 1.0;
                let bank = [
                    p[0] - (b[1] - a[1]) / len * off,
                    p[1] + (b[0] - a[0]) / len * off,
                ];
                if t.rect.x < bank[0] && bank[0] < t.rect.x1() && w.distance(bank) > 0.5 {
                    assert!(
                        t.height_at(bank) > *s - 0.3,
                        "{relief:?}: banks hold the water"
                    );
                }
            }
            assert!(w.path.first().unwrap()[1] >= t.rect.z1() - 1e-6);
            assert!(w.path.last().unwrap()[1] <= t.rect.z + 1e-6);
        }
    }

    #[test]
    fn water_and_ground_mesh_cleanly() {
        for water in WaterBody::ALL {
            let (t, w) = make(Relief::Mountains, water);
            let ground = ground_map(&t, w.as_ref());
            assert_eq!(ground.len(), t.heights.len());
            if let Some(w) = w {
                let set = w.mesh();
                let mesh = &set.meshes[&Material::Water];
                assert!(!mesh.indices.is_empty());
                assert!(mesh.positions.iter().flatten().all(|v| v.is_finite()));
                if water.is_sea() || water == WaterBody::Lake {
                    assert!(
                        ground.contains(&Ground::Sand) || water == WaterBody::Cliffs,
                        "{water:?}: a sandy shore"
                    );
                }
            }
        }
        let (t, _) = make(Relief::Mountains, WaterBody::None);
        assert!(
            ground_map(&t, None).contains(&Ground::Rock),
            "mountains have crags"
        );
    }
}
