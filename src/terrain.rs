//! The ground: a height map over a rectangle, with brushes to reshape it.
//!
//! Heights are stored on a regular grid of `(cols + 1) × (rows + 1)` points and
//! sampled bilinearly between them. Buildings sit on level pads cut into it
//! ([`Terrain::flatten_pad`]); the app's raise, lower, flatten and smooth tools
//! call [`Terrain::brush`].

use serde::{Deserialize, Serialize};

use crate::geometry::Rect;
use crate::parts::P2;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Terrain {
    /// The area covered.
    pub rect: Rect,
    pub cols: usize,
    pub rows: usize,
    /// Row-major heights, `(cols + 1) * (rows + 1)` of them.
    pub heights: Vec<f32>,
    /// Painted sand, 0–255 per grid point (empty until something is painted).
    #[serde(default)]
    pub sand: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Brush {
    Raise,
    Lower,
    /// Pull towards this height.
    Flatten(f64),
    /// Average with the neighbours.
    Smooth,
    /// Paint sand onto the ground (heights unchanged).
    Sand,
}

fn hash(x: i64, y: i64, seed: u64) -> f64 {
    let mut h = (x as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (y as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
        ^ seed.wrapping_mul(0x1656_67B1_9E37_79F9);
    h ^= h >> 33;
    h = h.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    h ^= h >> 33;
    (h >> 11) as f64 / (1u64 << 53) as f64
}

pub(crate) fn smooth_noise(x: f64, y: f64, seed: u64) -> f64 {
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (x - x0, y - y0);
    let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let c = |dx: i64, dy: i64| hash(x0 as i64 + dx, y0 as i64 + dy, seed);
    let top = c(0, 0) + (c(1, 0) - c(0, 0)) * sx;
    let bottom = c(0, 1) + (c(1, 1) - c(0, 1)) * sx;
    top + (bottom - top) * sy
}

/// Rolling hills in `[-1, 1]`, with features about `scale` metres across.
pub(crate) fn hills(x: f64, z: f64, scale: f64, seed: u64) -> f64 {
    let (mut sum, mut amp, mut norm, mut freq) = (0.0, 1.0, 0.0, 1.0 / scale);
    for octave in 0..4 {
        sum += smooth_noise(x * freq, z * freq, seed + octave) * amp;
        norm += amp;
        amp *= 0.45;
        freq *= 2.1;
    }
    (sum / norm - 0.5) * 2.0
}

pub(crate) fn smoothstep(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

impl Terrain {
    /// Flat ground over `rect`, with points roughly every `spacing` metres.
    pub fn flat(rect: Rect, spacing: f64) -> Self {
        let cols = ((rect.w / spacing).round() as usize).clamp(2, 256);
        let rows = ((rect.d / spacing).round() as usize).clamp(2, 256);
        Self {
            rect,
            cols,
            rows,
            heights: vec![0.0; (cols + 1) * (rows + 1)],
            sand: Vec::new(),
        }
    }

    /// Hills up to about `relief` metres high, plus a `slope` rise per metre towards the north.
    pub fn generate(rect: Rect, spacing: f64, relief: f64, slope: f64, seed: u64) -> Self {
        let mut t = Self::flat(rect, spacing);
        let scale = (rect.w.max(rect.d) / 2.5).max(20.0);
        for j in 0..=t.rows {
            for i in 0..=t.cols {
                let p = t.point(i, j);
                let h = relief * hills(p[0], p[1], scale, seed) + slope * (p[1] - rect.z);
                t.set(i, j, h);
            }
        }
        t
    }

    pub fn spacing(&self) -> (f64, f64) {
        (
            self.rect.w / self.cols as f64,
            self.rect.d / self.rows as f64,
        )
    }

    pub fn point(&self, i: usize, j: usize) -> P2 {
        let (sx, sz) = self.spacing();
        [self.rect.x + i as f64 * sx, self.rect.z + j as f64 * sz]
    }

    pub fn get(&self, i: usize, j: usize) -> f64 {
        self.heights[j * (self.cols + 1) + i] as f64
    }

    pub(crate) fn set(&mut self, i: usize, j: usize, h: f64) {
        let cols = self.cols;
        self.heights[j * (cols + 1) + i] = h as f32;
    }

    /// The height at `p`, interpolated; points outside take the nearest edge.
    pub fn height_at(&self, p: P2) -> f64 {
        let (sx, sz) = self.spacing();
        let fx = ((p[0] - self.rect.x) / sx).clamp(0.0, self.cols as f64);
        let fz = ((p[1] - self.rect.z) / sz).clamp(0.0, self.rows as f64);
        let (i, j) = (
            (fx.floor() as usize).min(self.cols - 1),
            (fz.floor() as usize).min(self.rows - 1),
        );
        let (tx, tz) = (fx - i as f64, fz - j as f64);
        let top = self.get(i, j) + (self.get(i + 1, j) - self.get(i, j)) * tx;
        let bottom = self.get(i, j + 1) + (self.get(i + 1, j + 1) - self.get(i, j + 1)) * tx;
        top + (bottom - top) * tz
    }

    /// Lowest and highest ground under `rect`.
    pub fn range_in(&self, rect: &Rect) -> (f64, f64) {
        let mut lo = f64::MAX;
        let mut hi = f64::MIN;
        let steps = 6;
        for a in 0..=steps {
            for b in 0..=steps {
                let p = [
                    rect.x + rect.w * a as f64 / steps as f64,
                    rect.z + rect.d * b as f64 / steps as f64,
                ];
                let h = self.height_at(p);
                lo = lo.min(h);
                hi = hi.max(h);
            }
        }
        (lo, hi)
    }

    /// Levels the ground to `height` under `rect`, blending back to the old ground over `falloff` metres.
    pub fn flatten_pad(&mut self, rect: &Rect, height: f64, falloff: f64) {
        for j in 0..=self.rows {
            for i in 0..=self.cols {
                let p = self.point(i, j);
                let dx = (rect.x - p[0]).max(p[0] - rect.x1()).max(0.0);
                let dz = (rect.z - p[1]).max(p[1] - rect.z1()).max(0.0);
                let d = (dx * dx + dz * dz).sqrt();
                if d >= falloff {
                    continue;
                }
                let k = 1.0 - smoothstep(d / falloff.max(1e-6));
                let h = self.get(i, j);
                self.set(i, j, h + (height - h) * k);
            }
        }
    }

    /// How much sand is painted at grid point `(i, j)`, in `[0, 1]`.
    pub fn sand_at(&self, i: usize, j: usize) -> f64 {
        self.sand
            .get(j * (self.cols + 1) + i)
            .map_or(0.0, |&v| v as f64 / 255.0)
    }

    /// Applies a brush stroke; returns whether anything changed. `strength` is metres per stroke.
    pub fn brush(&mut self, brush: Brush, centre: P2, radius: f64, strength: f64) -> bool {
        self.brush_masked(brush, centre, radius, strength, &|_| 1.0)
    }

    /// A brush stroke whose effect is scaled by `mask` (0 leaves a point alone, 1 is full effect),
    /// so building pads can be protected. Only grid points inside the radius are visited.
    pub fn brush_masked(
        &mut self,
        brush: Brush,
        centre: P2,
        radius: f64,
        strength: f64,
        mask: &dyn Fn(P2) -> f64,
    ) -> bool {
        let (sx, sz) = self.spacing();
        let lo_i =
            (((centre[0] - radius - self.rect.x) / sx).floor().max(0.0) as usize).min(self.cols);
        let hi_i =
            (((centre[0] + radius - self.rect.x) / sx).ceil().max(0.0) as usize).min(self.cols);
        let lo_j =
            (((centre[1] - radius - self.rect.z) / sz).floor().max(0.0) as usize).min(self.rows);
        let hi_j =
            (((centre[1] + radius - self.rect.z) / sz).ceil().max(0.0) as usize).min(self.rows);
        if brush == Brush::Sand && self.sand.len() != self.heights.len() {
            self.sand = vec![0; self.heights.len()];
        }
        let cols = self.cols;
        let rows = self.rows;
        let before: Vec<f32> = self.heights.clone();
        let at = |i: usize, j: usize| before[j * (cols + 1) + i] as f64;
        let mut changed = false;
        for j in lo_j..=hi_j {
            for i in lo_i..=hi_i {
                let p = self.point(i, j);
                let d = ((p[0] - centre[0]).powi(2) + (p[1] - centre[1]).powi(2)).sqrt();
                if d >= radius {
                    continue;
                }
                let k = (1.0 - smoothstep(d / radius)) * mask(p).clamp(0.0, 1.0);
                if k <= 0.0 {
                    continue;
                }
                let h = at(i, j);
                let target = match brush {
                    Brush::Raise => h + strength * k,
                    Brush::Lower => h - strength * k,
                    Brush::Flatten(level) => h + (level - h) * (k * strength).min(1.0),
                    Brush::Smooth => {
                        let mut sum = 0.0;
                        let mut n = 0.0;
                        for (di, dj) in [(-1i64, 0i64), (1, 0), (0, -1), (0, 1)] {
                            let (ni, nj) = (i as i64 + di, j as i64 + dj);
                            if ni >= 0 && nj >= 0 && ni as usize <= cols && nj as usize <= rows {
                                sum += at(ni as usize, nj as usize);
                                n += 1.0;
                            }
                        }
                        h + (sum / n - h) * (k * strength).min(1.0)
                    }
                    Brush::Sand => {
                        let index = j * (cols + 1) + i;
                        let old = self.sand[index];
                        let new = (old as f64 + 255.0 * k * strength.min(1.0)).min(255.0) as u8;
                        changed |= new != old;
                        self.sand[index] = new;
                        h
                    }
                };
                if (target - h).abs() > 1e-6 {
                    changed = true;
                    self.set(i, j, target);
                }
            }
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generation_is_deterministic() {
        let rect = Rect::new(-30.0, -30.0, 60.0, 60.0);
        let a = Terrain::generate(rect, 1.0, 4.0, 0.02, 9);
        let b = Terrain::generate(rect, 1.0, 4.0, 0.02, 9);
        assert_eq!(a, b);
        assert_ne!(a, Terrain::generate(rect, 1.0, 4.0, 0.02, 10));
        let (lo, hi) = a.range_in(&rect);
        assert!(hi - lo > 1.0, "hills have relief");
    }

    #[test]
    fn pads_are_level() {
        let rect = Rect::new(0.0, 0.0, 50.0, 50.0);
        let mut t = Terrain::generate(rect, 1.0, 5.0, 0.0, 3);
        let pad = Rect::new(10.0, 10.0, 12.0, 8.0);
        t.flatten_pad(&pad, 2.5, 4.0);
        let (lo, hi) = t.range_in(&pad);
        assert!((lo - 2.5).abs() < 1e-3 && (hi - 2.5).abs() < 1e-3);
    }

    #[test]
    fn masked_brushes_leave_pads_alone() {
        let rect = Rect::new(0.0, 0.0, 40.0, 40.0);
        let mut t = Terrain::flat(rect, 1.0);
        let pad = Rect::new(18.0, 18.0, 4.0, 4.0);
        let mask = |p: P2| {
            if p[0] >= pad.x && p[0] <= pad.x1() && p[1] >= pad.z && p[1] <= pad.z1() {
                0.0
            } else {
                1.0
            }
        };
        assert!(t.brush_masked(Brush::Raise, [20.0, 20.0], 8.0, 2.0, &mask));
        assert_eq!(t.height_at([20.0, 20.0]), 0.0, "the pad is untouched");
        assert!(t.height_at([25.0, 20.0]) > 0.1, "the ground around it rose");
        assert!(t.brush(Brush::Sand, [5.0, 5.0], 3.0, 1.0));
        assert!(t.sand_at(5, 5) > 0.9);
        assert_eq!(t.height_at([5.0, 5.0]), 0.0, "sand doesn't move the ground");
    }

    #[test]
    fn brushes_stay_inside_their_radius() {
        let rect = Rect::new(0.0, 0.0, 40.0, 40.0);
        let mut t = Terrain::flat(rect, 1.0);
        assert!(t.brush(Brush::Raise, [20.0, 20.0], 5.0, 1.0));
        assert!(t.height_at([20.0, 20.0]) > 0.9);
        assert_eq!(t.height_at([30.0, 20.0]), 0.0);
        assert_eq!(t.height_at([20.0, 5.0]), 0.0);
        let peak = t.height_at([20.0, 20.0]);
        t.brush(Brush::Smooth, [20.0, 20.0], 5.0, 1.0);
        assert!(t.height_at([20.0, 20.0]) < peak);
    }
}
