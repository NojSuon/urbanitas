//! Procedural, tileable textures, one per [`Material`].
//!
//! Generated from hashed value noise, so no image files ship with the crate
//! and every palette gets matching textures. Each texture tiles seamlessly
//! and comes with a full box-filtered mip chain.

use std::f64::consts::PI;
use std::fs::File;
use std::io::{self, BufWriter};
use std::path::Path;

use crate::mesh::Material;
use crate::style::{Color, Palette};

/// An RGBA8 square texture with its mip chain, largest first.
#[derive(Clone, Debug, PartialEq)]
pub struct Texture {
    pub size: u32,
    pub levels: Vec<Vec<u8>>,
}

impl Texture {
    pub fn rgba(&self) -> &[u8] {
        &self.levels[0]
    }

    /// All mip levels back to back, as GPUs expect them.
    pub fn mip_bytes(&self) -> Vec<u8> {
        self.levels.concat()
    }

    pub fn write_png(&self, path: &Path) -> io::Result<()> {
        let mut encoder =
            png::Encoder::new(BufWriter::new(File::create(path)?), self.size, self.size);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(io::Error::other)?;
        writer
            .write_image_data(self.rgba())
            .map_err(io::Error::other)
    }
}

type Rgb = [f64; 3];

fn rgb(c: Color) -> Rgb {
    c.rgb()
}

fn mix(a: Rgb, b: Rgb, t: f64) -> Rgb {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

fn shade(c: Rgb, k: f64) -> Rgb {
    [c[0] * k, c[1] * k, c[2] * k]
}

fn hash(x: i64, y: i64, seed: u64) -> f64 {
    let mut h = (x as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (y as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
        ^ seed.wrapping_mul(0x1656_67B1_9E37_79F9);
    h ^= h >> 33;
    h = h.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    h ^= h >> 33;
    h = h.wrapping_mul(0xC4CE_B9FE_1A85_EC53);
    h ^= h >> 33;
    (h >> 11) as f64 / (1u64 << 53) as f64
}

/// Smooth value noise in `[0, 1)` that tiles every `px` × `py` cells over the unit square.
fn noise(u: f64, v: f64, px: i64, py: i64, seed: u64) -> f64 {
    let (x, y) = (u * px as f64, v * py as f64);
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (x - x0, y - y0);
    let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let corner = |dx: i64, dy: i64| {
        hash(
            (x0 as i64 + dx).rem_euclid(px),
            (y0 as i64 + dy).rem_euclid(py),
            seed,
        )
    };
    let top = corner(0, 0) + (corner(1, 0) - corner(0, 0)) * sx;
    let bottom = corner(0, 1) + (corner(1, 1) - corner(0, 1)) * sx;
    top + (bottom - top) * sy
}

/// Fractal noise: `octaves` layers of [`noise`], each twice as fine.
fn fbm(u: f64, v: f64, period: i64, octaves: u32, seed: u64) -> f64 {
    let (mut sum, mut amp, mut norm, mut p) = (0.0, 1.0, 0.0, period);
    for o in 0..octaves {
        sum += noise(u, v, p, p, seed + o as u64) * amp;
        norm += amp;
        amp *= 0.5;
        p *= 2;
    }
    sum / norm
}

/// Fractal noise stretched along `u` (for wood grain).
fn grain(u: f64, v: f64, seed: u64) -> f64 {
    noise(u, v, 3, 48, seed) * 0.6 + noise(u, v, 6, 96, seed + 1) * 0.4
}

/// Distance to the nearest edge of a cell, as a fraction of the cell, for a grid of `cols` × `rows`.
fn cell(u: f64, v: f64, cols: f64, rows: f64) -> (i64, i64, f64, f64) {
    let (x, y) = (u * cols, v * rows);
    (x.floor() as i64, y.floor() as i64, x.fract(), y.fract())
}

/// The texture for `material` in `palette`'s colours, `size` pixels square (a power of two).
pub fn generate(material: Material, palette: &Palette, size: u32) -> Texture {
    assert!(
        size.is_power_of_two() && size >= 8,
        "texture size must be a power of two ≥ 8"
    );
    let n = size as usize;
    let mut pixels = vec![0u8; n * n * 4];
    let seed = material as u64 * 0x1000 + 17;
    for y in 0..n {
        for x in 0..n {
            let (u, v) = ((x as f64 + 0.5) / n as f64, (y as f64 + 0.5) / n as f64);
            let speck = hash(x as i64, y as i64, seed ^ 0xABCD) - 0.5;
            let c = texel(material, palette, u, v, speck, seed);
            let i = (y * n + x) * 4;
            for k in 0..3 {
                pixels[i + k] = (c[k].clamp(0.0, 1.0) * 255.0).round() as u8;
            }
            pixels[i + 3] = 255;
        }
    }
    let mut levels = vec![pixels];
    let mut s = n;
    while s > 1 {
        let half = s / 2;
        let prev = levels.last().expect("at least one level");
        let mut next = vec![0u8; half * half * 4];
        for y in 0..half {
            for x in 0..half {
                for k in 0..4 {
                    let at =
                        |dx: usize, dy: usize| prev[((y * 2 + dy) * s + x * 2 + dx) * 4 + k] as u32;
                    next[(y * half + x) * 4 + k] =
                        ((at(0, 0) + at(1, 0) + at(0, 1) + at(1, 1) + 2) / 4) as u8;
                }
            }
        }
        levels.push(next);
        s = half;
    }
    Texture { size, levels }
}

/// One texel. `u` and `v` run over one tile in `[0, 1)`; `v` grows downwards in the image,
/// which is upwards on walls once mapped.
fn texel(material: Material, palette: &Palette, u: f64, v: f64, speck: f64, seed: u64) -> Rgb {
    // Up on the wall is towards the top of the image.
    let up = 1.0 - v;
    match material {
        Material::Plaster => {
            let base = rgb(palette.walls);
            let body = fbm(u, v, 4, 5, seed);
            let stains = fbm(u, v, 2, 3, seed + 9);
            let mut c = shade(base, 0.86 + 0.2 * body + speck * 0.05);
            if stains > 0.62 {
                c = shade(c, 1.0 - (stains - 0.62) * 0.5);
            }
            c
        }
        Material::InteriorPlaster => shade(
            [0.95, 0.93, 0.89],
            0.95 + 0.07 * fbm(u, v, 4, 4, seed) + speck * 0.02,
        ),
        Material::Rustication => {
            // Four courses of blocks, two per course, staggered.
            let base = mix(rgb(palette.stone), rgb(palette.walls), 0.35);
            let rows = 4.0;
            let row = (v * rows).floor();
            let shift = if row as i64 % 2 == 1 { 0.25 } else { 0.0 };
            let (bx, by, fx, fy) = cell((u + shift).rem_euclid(1.0), v, 2.0, rows);
            let joint = 0.035;
            let block = hash(bx, by, seed) - 0.5;
            let rough = fbm(u, v, 8, 4, seed + 3);
            if fx < joint || fy < joint * 2.0 {
                return shade(base, 0.45 + rough * 0.1);
            }
            let bevel = 0.12;
            let light = if fy < bevel || fx < bevel / 2.0 {
                1.12
            } else if fy > 1.0 - bevel || fx > 1.0 - bevel / 2.0 {
                0.78
            } else {
                1.0
            };
            shade(
                base,
                (0.82 + 0.3 * rough + block * 0.14 + speck * 0.06) * light,
            )
        }
        Material::Stone => {
            let base = rgb(palette.stone);
            shade(base, 0.86 + 0.2 * fbm(u, v, 8, 5, seed) + speck * 0.08)
        }
        Material::Shutter => {
            let base = rgb(palette.trim);
            let wear = fbm(u, v, 6, 3, seed);
            if !(0.06..=0.94).contains(&u) {
                return shade(base, 0.9 + wear * 0.1);
            }
            let s = (up * 12.0).fract();
            let k = if s < 0.12 { 0.5 } else { 0.8 + 0.3 * (1.0 - s) };
            shade(base, k * (0.92 + wear * 0.12) + speck * 0.03)
        }
        Material::Door => {
            let base = rgb(palette.trim);
            let planks = 6.0;
            let p = u * planks;
            if p.fract() < 0.04 {
                return shade(base, 0.45);
            }
            let tint = hash(p.floor() as i64, 0, seed) - 0.5;
            let g = grain(u, v, seed);
            shade(
                mix(base, [0.36, 0.24, 0.14], 0.35),
                0.8 + 0.35 * g + tint * 0.1,
            )
        }
        Material::Wood => {
            let g = grain(v, u, seed);
            shade([0.45, 0.30, 0.18], 0.75 + 0.4 * g + speck * 0.04)
        }
        Material::Glass => {
            let streak = ((u + up) * 1.5).fract();
            let k = if (0.2..0.32).contains(&streak) {
                1.35
            } else {
                1.0
            };
            shade(mix([0.14, 0.22, 0.28], [0.32, 0.44, 0.5], up), k)
        }
        Material::RoofTile => {
            // Coppi: half-round tiles in columns running down the slope, in overlapping courses.
            let base = rgb(palette.roof);
            let (cx, cy, fx, fy) = cell(u, v, 5.0, 4.0);
            let round = 0.62 + 0.45 * (PI * fx).sin();
            let overlap = if fy > 0.86 { 0.62 } else { 1.0 - fy * 0.12 };
            let tint = hash(cx, cy, seed) - 0.5;
            let lichen = fbm(u, v, 6, 3, seed + 5);
            let c = shade(base, round * overlap * (0.95 + tint * 0.22) + speck * 0.05);
            if lichen > 0.7 {
                mix(c, [0.62, 0.6, 0.45], (lichen - 0.7) * 1.2)
            } else {
                c
            }
        }
        Material::Cotto => {
            let base = [0.72, 0.42, 0.28];
            let (cx, cy, fx, fy) = cell(u, v, 4.0, 4.0);
            if fx < 0.04 || fy < 0.04 {
                return [0.78, 0.72, 0.62];
            }
            let tint = hash(cx, cy, seed) - 0.5;
            shade(
                base,
                0.88 + tint * 0.18 + fbm(u, v, 8, 3, seed) * 0.14 + speck * 0.04,
            )
        }
        Material::Iron => shade([0.14, 0.14, 0.15], 0.85 + 0.3 * fbm(u, v, 6, 3, seed)),
        Material::Lawn => {
            let base = [0.40, 0.52, 0.24];
            let patches = fbm(u, v, 3, 4, seed);
            let blades = speck * 0.18;
            shade(
                mix(base, [0.55, 0.56, 0.3], (patches - 0.45).max(0.0)),
                0.85 + 0.2 * patches + blades,
            )
        }
        Material::Gravel => {
            let (cx, cy, fx, fy) = cell(u, v, 24.0, 24.0);
            let pebble = hash(cx, cy, seed);
            let edge = (fx - 0.5).abs().max((fy - 0.5).abs());
            let c = mix([0.78, 0.72, 0.6], [0.62, 0.6, 0.56], pebble);
            shade(c, if edge > 0.42 { 0.78 } else { 1.0 + speck * 0.1 })
        }
        Material::Water => {
            let f = fbm(u, v, 4, 4, seed);
            let caustic = (1.0 - ((f * 12.0).sin()).abs()).powi(6);
            mix([0.16, 0.5, 0.66], [0.75, 0.92, 0.95], caustic * 0.6)
        }
        Material::Foliage | Material::OliveLeaf | Material::CypressLeaf => {
            let base = match material {
                Material::OliveLeaf => [0.5, 0.58, 0.4],
                Material::CypressLeaf => [0.16, 0.3, 0.16],
                _ => [0.28, 0.45, 0.2],
            };
            let clusters = fbm(u, v, 6, 4, seed);
            let leaf = if speck > 0.3 {
                1.2
            } else if speck < -0.35 {
                0.7
            } else {
                1.0
            };
            shade(base, (0.75 + 0.45 * clusters) * leaf)
        }
        Material::Terracotta => shade(
            [0.75, 0.4, 0.24],
            0.88 + 0.2 * fbm(u, v, 6, 3, seed) + speck * 0.05,
        ),
        Material::Lemon => shade([0.96, 0.82, 0.2], 0.9 + 0.15 * fbm(u, v, 4, 2, seed)),
        Material::Earth => {
            // A strada bianca: pale, dusty, with darker ruts and pebbles.
            let base = [0.84, 0.78, 0.66];
            let dust = fbm(u, v, 5, 4, seed);
            let rut = ((u * 2.0 * PI).sin().abs()).powi(8) * 0.12;
            shade(base, 0.86 + 0.18 * dust - rut + speck * 0.08)
        }
        Material::Sand => {
            // Fine pale sand with wind ripples.
            let ripple = ((v * 9.0 + fbm(u, v, 3, 2, seed) * 3.0) * 2.0 * PI).sin() * 0.04;
            shade(
                [0.9, 0.82, 0.64],
                0.9 + 0.1 * fbm(u, v, 6, 4, seed) + ripple + speck * 0.07,
            )
        }
        Material::Rock => {
            // Layered limestone: strata, cracks and lichen-dark patches.
            let f = fbm(u, v, 4, 5, seed);
            let strata = ((v * 7.0 + f * 2.5) * 2.0 * PI).sin() * 0.07;
            let crack = if ((v * 7.0 + f * 2.5).fract() - 0.5).abs() < 0.03 {
                0.7
            } else {
                1.0
            };
            let c = mix(
                [0.66, 0.62, 0.55],
                [0.5, 0.49, 0.45],
                fbm(u, v, 2, 3, seed + 7),
            );
            shade(c, (0.88 + 0.16 * f + strata + speck * 0.08) * crack)
        }
        Material::Basalt => {
            // Dark cobbles in staggered rows, with pale joints.
            let rows = 8.0;
            let row = (v * rows).floor();
            let shift = if row as i64 % 2 == 1 { 0.5 / 8.0 } else { 0.0 };
            let (cx, cy, fx, fy) = cell((u + shift).rem_euclid(1.0), v, 8.0, rows);
            let edge = (fx - 0.5).abs().max((fy - 0.5).abs());
            if edge > 0.44 {
                return [0.55, 0.53, 0.5];
            }
            let tint = hash(cx, cy, seed) - 0.5;
            let dome = 1.0 - edge * 0.5;
            shade([0.3, 0.3, 0.31], (0.85 + tint * 0.25 + speck * 0.1) * dome)
        }
        Material::Slabs => {
            // Large stone flags, three to a course, staggered.
            let base = mix(rgb(palette.stone), [0.72, 0.68, 0.6], 0.5);
            let rows = 4.0;
            let row = (v * rows).floor();
            let shift = if row as i64 % 2 == 1 { 1.0 / 6.0 } else { 0.0 };
            let (cx, cy, fx, fy) = cell((u + shift).rem_euclid(1.0), v, 3.0, rows);
            if fx < 0.015 || fy < 0.02 {
                return shade(base, 0.55);
            }
            let tint = hash(cx, cy, seed) - 0.5;
            shade(
                base,
                0.86 + tint * 0.16 + fbm(u, v, 8, 3, seed) * 0.12 + speck * 0.05,
            )
        }
        Material::Rubble => {
            // Irregular field stones: jittered cells, dark gaps between them.
            let n = 10.0;
            let (x, y) = (u * n, v * n);
            let (ix, iy) = (x.floor() as i64, y.floor() as i64);
            let mut best = (f64::MAX, f64::MAX, 0i64, 0i64);
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let (cx, cy) = (ix + dx, iy + dy);
                    let (wx, wy) = (cx.rem_euclid(n as i64), cy.rem_euclid(n as i64));
                    let px = cx as f64 + 0.2 + 0.6 * hash(wx, wy, seed);
                    let py = cy as f64 + 0.2 + 0.6 * hash(wx, wy, seed + 1);
                    let d = ((x - px).powi(2) + (y - py).powi(2)).sqrt();
                    if d < best.0 {
                        best = (d, best.0, wx, wy);
                    } else if d < best.1 {
                        best.1 = d;
                    }
                }
            }
            let gap = best.1 - best.0;
            if gap < 0.08 {
                return [0.22, 0.2, 0.17];
            }
            let tint = hash(best.2, best.3, seed + 2) - 0.5;
            shade(rgb(palette.stone), 0.78 + tint * 0.3 + speck * 0.1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::ArchitectureStyle;
    use crate::rng::Rng;

    #[test]
    fn textures_are_deterministic_with_mips() {
        let palette = Palette::choose(ArchitectureStyle::Florentine, &mut Rng::new(4));
        for material in Material::ALL {
            let a = generate(material, &palette, 32);
            let b = generate(material, &palette, 32);
            assert_eq!(a, b, "{material:?}");
            assert_eq!(a.levels.len(), 6, "32, 16, 8, 4, 2, 1");
            assert_eq!(a.levels[0].len(), 32 * 32 * 4);
            assert_eq!(a.levels.last().unwrap().len(), 4);
        }
    }

    #[test]
    fn noise_tiles_seamlessly() {
        for i in 0..10 {
            let v = i as f64 / 10.0;
            assert!((noise(0.0, v, 4, 4, 1) - noise(1.0, v, 4, 4, 1)).abs() < 1e-9);
        }
    }
}
