//! Engine-agnostic triangle meshes.
//!
//! [`build::Scene`] turns layout parts into a [`MeshSet`]: one indexed mesh per
//! [`Material`], with positions, normals and UVs. The OBJ exporter and the app
//! both consume it, so there is a single source of geometry.
//!
//! Conventions: metres, `y` up, triangles wound counter-clockwise seen from
//! the front. UVs are world-space and already divided by the material's
//! texture size, so textures tile seamlessly across parts; `v` grows upwards.

pub mod build;

use std::collections::BTreeMap;
use std::ops::{Add, Mul, Neg, Sub};

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct V3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

pub const fn v3(x: f64, y: f64, z: f64) -> V3 {
    V3 { x, y, z }
}

impl V3 {
    pub const ZERO: V3 = v3(0.0, 0.0, 0.0);
    pub const UP: V3 = v3(0.0, 1.0, 0.0);

    pub fn dot(self, o: V3) -> f64 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }

    pub fn cross(self, o: V3) -> V3 {
        v3(
            self.y * o.z - self.z * o.y,
            self.z * o.x - self.x * o.z,
            self.x * o.y - self.y * o.x,
        )
    }

    pub fn length(self) -> f64 {
        self.dot(self).sqrt()
    }

    pub fn normalized(self) -> V3 {
        let l = self.length();
        if l > 1e-12 { self * (1.0 / l) } else { V3::UP }
    }

    pub fn lerp(self, o: V3, t: f64) -> V3 {
        self + (o - self) * t
    }

    /// A point on the ground plane at height `y`.
    pub fn ground(p: [f64; 2], y: f64) -> V3 {
        v3(p[0], y, p[1])
    }
}

impl Add for V3 {
    type Output = V3;
    fn add(self, o: V3) -> V3 {
        v3(self.x + o.x, self.y + o.y, self.z + o.z)
    }
}

impl Sub for V3 {
    type Output = V3;
    fn sub(self, o: V3) -> V3 {
        v3(self.x - o.x, self.y - o.y, self.z - o.z)
    }
}

impl Mul<f64> for V3 {
    type Output = V3;
    fn mul(self, s: f64) -> V3 {
        v3(self.x * s, self.y * s, self.z * s)
    }
}

impl Neg for V3 {
    type Output = V3;
    fn neg(self) -> V3 {
        v3(-self.x, -self.y, -self.z)
    }
}

/// What a surface is made of. Each material has one tiling texture.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Material {
    /// Exterior lime plaster (intonaco), tinted with the palette.
    Plaster,
    InteriorPlaster,
    /// Rough bevelled stone blocks (bugnato).
    Rustication,
    Stone,
    Shutter,
    Door,
    Wood,
    Glass,
    RoofTile,
    /// Terracotta floor tiles.
    Cotto,
    Iron,
    Lawn,
    Gravel,
    Water,
    Foliage,
    OliveLeaf,
    CypressLeaf,
    Terracotta,
    Lemon,
    /// Dusty white earth: country roads and steep ground.
    Earth,
    /// Dark basalt cobbles.
    Basalt,
    /// Large rectangular paving slabs.
    Slabs,
    /// Rough dry-stone walling.
    Rubble,
    /// Beach sand.
    Sand,
    /// Bare rock: cliffs and steep mountainsides.
    Rock,
}

impl Material {
    pub const ALL: [Material; 25] = [
        Material::Plaster,
        Material::InteriorPlaster,
        Material::Rustication,
        Material::Stone,
        Material::Shutter,
        Material::Door,
        Material::Wood,
        Material::Glass,
        Material::RoofTile,
        Material::Cotto,
        Material::Iron,
        Material::Lawn,
        Material::Gravel,
        Material::Water,
        Material::Foliage,
        Material::OliveLeaf,
        Material::CypressLeaf,
        Material::Terracotta,
        Material::Lemon,
        Material::Earth,
        Material::Basalt,
        Material::Slabs,
        Material::Rubble,
        Material::Sand,
        Material::Rock,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Material::Plaster => "plaster",
            Material::InteriorPlaster => "interior_plaster",
            Material::Rustication => "rustication",
            Material::Stone => "stone",
            Material::Shutter => "shutter",
            Material::Door => "door",
            Material::Wood => "wood",
            Material::Glass => "glass",
            Material::RoofTile => "roof_tile",
            Material::Cotto => "cotto",
            Material::Iron => "iron",
            Material::Lawn => "lawn",
            Material::Gravel => "gravel",
            Material::Water => "water",
            Material::Foliage => "foliage",
            Material::OliveLeaf => "olive_leaf",
            Material::CypressLeaf => "cypress_leaf",
            Material::Terracotta => "terracotta",
            Material::Lemon => "lemon",
            Material::Earth => "earth",
            Material::Basalt => "basalt",
            Material::Slabs => "slabs",
            Material::Rubble => "rubble",
            Material::Sand => "sand",
            Material::Rock => "rock",
        }
    }

    /// Takes its colour from the building's palette (so each building needs its own texture).
    pub fn uses_palette(self) -> bool {
        matches!(
            self,
            Material::Plaster
                | Material::Rustication
                | Material::Stone
                | Material::Shutter
                | Material::Door
                | Material::RoofTile
                | Material::Slabs
                | Material::Rubble
        )
    }

    /// Metres of surface covered by one repeat of the texture.
    pub fn tile_metres(self) -> f64 {
        match self {
            Material::Plaster | Material::InteriorPlaster | Material::Lawn => 2.0,
            Material::Rustication => 1.8,
            Material::Shutter => 0.6,
            Material::Door | Material::Cotto => 1.2,
            Material::RoofTile => 0.9,
            Material::Water => 3.0,
            Material::Terracotta => 0.5,
            Material::Lemon => 0.3,
            Material::Earth | Material::Slabs => 3.0,
            Material::Basalt => 1.2,
            Material::Rubble => 2.0,
            Material::Sand => 2.5,
            Material::Rock => 4.0,
            _ => 1.0,
        }
    }

    /// Physically based shading: `(perceptual roughness, metallic)`.
    pub fn surface(self) -> (f32, f32) {
        match self {
            Material::Glass => (0.08, 0.2),
            Material::Water => (0.05, 0.0),
            Material::Iron => (0.45, 0.7),
            Material::Shutter | Material::Door => (0.6, 0.0),
            Material::Foliage | Material::OliveLeaf | Material::CypressLeaf | Material::Lemon => {
                (0.8, 0.0)
            }
            _ => (0.92, 0.0),
        }
    }
}

/// An indexed triangle mesh in one material.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeshData {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
}

impl MeshData {
    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }

    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    pub fn vertex_count(&self) -> usize {
        self.positions.len()
    }

    pub fn append(&mut self, other: &MeshData) {
        let base = self.positions.len() as u32;
        self.positions.extend_from_slice(&other.positions);
        self.normals.extend_from_slice(&other.normals);
        self.uvs.extend_from_slice(&other.uvs);
        self.indices.extend(other.indices.iter().map(|i| i + base));
    }

    fn vertex(&mut self, p: V3, n: V3, uv: [f64; 2], scale: f64) -> u32 {
        self.positions.push([p.x as f32, p.y as f32, p.z as f32]);
        self.normals.push([n.x as f32, n.y as f32, n.z as f32]);
        self.uvs
            .push([(uv[0] / scale) as f32, (uv[1] / scale) as f32]);
        (self.positions.len() - 1) as u32
    }
}

/// Meshes for several materials, built together.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeshSet {
    pub meshes: BTreeMap<Material, MeshData>,
}

/// Newell's method: the area-weighted normal of a polygon, following its winding.
fn polygon_normal(points: &[V3]) -> V3 {
    let mut n = V3::ZERO;
    for (i, p) in points.iter().enumerate() {
        let q = points[(i + 1) % points.len()];
        n.x += (p.y - q.y) * (p.z + q.z);
        n.y += (p.z - q.z) * (p.x + q.x);
        n.z += (p.x - q.x) * (p.y + q.y);
    }
    n
}

/// Texture coordinates for a flat face: project onto the plane it mostly faces.
pub fn planar_uv(p: V3, n: V3) -> [f64; 2] {
    let (ax, ay, az) = (n.x.abs(), n.y.abs(), n.z.abs());
    if ay >= ax && ay >= az {
        [p.x, p.z]
    } else if ax >= az {
        [p.z, p.y]
    } else {
        [p.x, p.y]
    }
}

impl MeshSet {
    pub fn mesh(&mut self, material: Material) -> &mut MeshData {
        self.meshes.entry(material).or_default()
    }

    pub fn merge(&mut self, other: MeshSet) {
        for (material, mesh) in other.meshes {
            self.mesh(material).append(&mesh);
        }
    }

    pub fn triangle_count(&self) -> usize {
        self.meshes.values().map(MeshData::triangle_count).sum()
    }

    pub fn vertex_count(&self) -> usize {
        self.meshes.values().map(MeshData::vertex_count).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.meshes.values().all(MeshData::is_empty)
    }

    /// A flat convex polygon facing roughly towards `facing`, textured by planar projection.
    pub fn polygon(&mut self, material: Material, points: &[V3], facing: V3) {
        let uvs: Vec<[f64; 2]> = points.iter().map(|&p| planar_uv(p, facing)).collect();
        self.polygon_uv(material, points, facing, &uvs);
    }

    /// A flat convex polygon with explicit texture coordinates (in metres).
    /// The winding is flipped if needed so the face points towards `facing`.
    pub fn polygon_uv(&mut self, material: Material, points: &[V3], facing: V3, uvs: &[[f64; 2]]) {
        if points.len() < 3 {
            return;
        }
        let mut normal = polygon_normal(points);
        if normal.length() < 1e-12 {
            return;
        }
        let flip = normal.dot(facing) < 0.0;
        if flip {
            normal = -normal;
        }
        let normal = normal.normalized();
        let scale = material.tile_metres();
        let mesh = self.mesh(material);
        let ids: Vec<u32> = points
            .iter()
            .zip(uvs)
            .map(|(&p, &uv)| mesh.vertex(p, normal, uv, scale))
            .collect();
        for i in 1..ids.len() - 1 {
            let (a, b, c) = (ids[0], ids[i], ids[i + 1]);
            if flip {
                mesh.indices.extend([a, c, b]);
            } else {
                mesh.indices.extend([a, b, c]);
            }
        }
    }

    /// A quad with per-vertex normals, for smooth curved surfaces.
    pub fn smooth_quad(&mut self, material: Material, p: [V3; 4], n: [V3; 4], uv: [[f64; 2]; 4]) {
        let avg = n[0] + n[1] + n[2] + n[3];
        let flip = polygon_normal(&p).dot(avg) < 0.0;
        let scale = material.tile_metres();
        let mesh = self.mesh(material);
        let ids: Vec<u32> = (0..4)
            .map(|i| mesh.vertex(p[i], n[i].normalized(), uv[i], scale))
            .collect();
        let tris = if flip {
            [0, 2, 1, 0, 3, 2]
        } else {
            [0, 1, 2, 0, 2, 3]
        };
        mesh.indices.extend(tris.iter().map(|&i| ids[i]));
    }

    /// A box in `frame`'s coordinates; empty ranges add nothing. Ranges may come in either order.
    pub fn cuboid(
        &mut self,
        f: &Frame,
        u: (f64, f64),
        v: (f64, f64),
        n: (f64, f64),
        mats: FaceMats,
    ) {
        let (u0, u1) = (u.0.min(u.1), u.0.max(u.1));
        let (v0, v1) = (v.0.min(v.1), v.0.max(v.1));
        let (n0, n1) = (n.0.min(n.1), n.0.max(n.1));
        if u1 - u0 <= 1e-6 || v1 - v0 <= 1e-6 || n1 - n0 <= 1e-6 {
            return;
        }
        let c = |u, v, n| f.at(u, v, n);
        let uv_n = |p: V3| [p.dot(f.u), p.dot(f.v)];
        let uv_u = |p: V3| [p.dot(f.n), p.dot(f.v)];
        let uv_v = |p: V3| [p.dot(f.u), p.dot(f.n)];
        type Face<'f> = (Material, [V3; 4], V3, &'f dyn Fn(V3) -> [f64; 2]);
        let faces: [Face; 6] = [
            (
                mats.front,
                [c(u0, v0, n1), c(u1, v0, n1), c(u1, v1, n1), c(u0, v1, n1)],
                f.n,
                &uv_n,
            ),
            (
                mats.back,
                [c(u0, v0, n0), c(u1, v0, n0), c(u1, v1, n0), c(u0, v1, n0)],
                -f.n,
                &uv_n,
            ),
            (
                mats.sides,
                [c(u1, v0, n0), c(u1, v0, n1), c(u1, v1, n1), c(u1, v1, n0)],
                f.u,
                &uv_u,
            ),
            (
                mats.sides,
                [c(u0, v0, n0), c(u0, v0, n1), c(u0, v1, n1), c(u0, v1, n0)],
                -f.u,
                &uv_u,
            ),
            (
                mats.top,
                [c(u0, v1, n0), c(u1, v1, n0), c(u1, v1, n1), c(u0, v1, n1)],
                f.v,
                &uv_v,
            ),
            (
                mats.bottom,
                [c(u0, v0, n0), c(u1, v0, n0), c(u1, v0, n1), c(u0, v0, n1)],
                -f.v,
                &uv_v,
            ),
        ];
        for (material, points, facing, uv) in faces {
            let uvs = points.map(uv);
            self.polygon_uv(material, &points, facing, &uvs);
        }
    }

    /// An axis-aligned box over a ground rectangle, from height `y0` to `y1`.
    pub fn slab(&mut self, rect: &crate::geometry::Rect, y0: f64, y1: f64, mats: FaceMats) {
        self.cuboid(
            &Frame::WORLD,
            (rect.x, rect.x1()),
            (y0, y1),
            (rect.z, rect.z1()),
            mats,
        );
    }

    /// A surface of revolution around the vertical axis through `base`.
    /// `profile` lists `(radius, height)` from bottom to top.
    pub fn lathe(
        &mut self,
        material: Material,
        base: V3,
        profile: &[(f64, f64)],
        segments: usize,
        smooth: bool,
    ) {
        if profile.len() < 2 || segments < 3 {
            return;
        }
        let ring = |i: usize, j: usize| {
            let a = std::f64::consts::TAU * j as f64 / segments as f64;
            let (r, y) = profile[i];
            base + v3(a.cos() * r, y, a.sin() * r)
        };
        let normal = |i: usize, j: usize| {
            let prev = profile[i.saturating_sub(1)];
            let next = profile[(i + 1).min(profile.len() - 1)];
            let (dr, dy) = (next.0 - prev.0, next.1 - prev.1);
            let a = std::f64::consts::TAU * j as f64 / segments as f64;
            v3(a.cos() * dy, -dr, a.sin() * dy).normalized()
        };
        let mut heights = vec![0.0; profile.len()];
        for i in 1..profile.len() {
            let (dr, dy) = (
                profile[i].0 - profile[i - 1].0,
                profile[i].1 - profile[i - 1].1,
            );
            heights[i] = heights[i - 1] + (dr * dr + dy * dy).sqrt();
        }
        let r_max = profile.iter().map(|p| p.0).fold(0.0, f64::max);
        let circumference = std::f64::consts::TAU * r_max;
        for i in 0..profile.len() - 1 {
            for j in 0..segments {
                let p = [
                    ring(i, j),
                    ring(i, j + 1),
                    ring(i + 1, j + 1),
                    ring(i + 1, j),
                ];
                let u0 = circumference * j as f64 / segments as f64;
                let u1 = circumference * (j + 1) as f64 / segments as f64;
                let uv = [
                    [u0, heights[i]],
                    [u1, heights[i]],
                    [u1, heights[i + 1]],
                    [u0, heights[i + 1]],
                ];
                if smooth {
                    let n = [
                        normal(i, j),
                        normal(i, j + 1),
                        normal(i + 1, j + 1),
                        normal(i + 1, j),
                    ];
                    self.smooth_quad(material, p, n, uv);
                } else {
                    let (dr, dy) = (
                        profile[i + 1].0 - profile[i].0,
                        profile[i + 1].1 - profile[i].1,
                    );
                    let a = std::f64::consts::TAU * (j as f64 + 0.5) / segments as f64;
                    let facing = v3(a.cos() * dy, -dr, a.sin() * dy);
                    self.polygon_uv(material, &p, facing, &uv);
                }
            }
        }
        if profile[profile.len() - 1].0 > 1e-6 {
            let cap: Vec<V3> = (0..segments).map(|j| ring(profile.len() - 1, j)).collect();
            self.polygon(material, &cap, V3::UP);
        }
    }

    /// A low-poly, faceted blob: an icosphere with radii `radii`, roughened by `jitter`.
    pub fn icosphere(
        &mut self,
        material: Material,
        center: V3,
        radii: V3,
        subdivisions: u32,
        jitter: f64,
        seed: u64,
    ) {
        let (vertices, faces) = icosphere_unit(subdivisions);
        let points: Vec<V3> = vertices
            .iter()
            .map(|&d| {
                let k = 1.0 + jitter * (hash3(d, seed) - 0.5);
                center + v3(d.x * radii.x, d.y * radii.y, d.z * radii.z) * k
            })
            .collect();
        for [a, b, c] in faces {
            let tri = [points[a], points[b], points[c]];
            let mid = (tri[0] + tri[1] + tri[2]) * (1.0 / 3.0);
            self.polygon(material, &tri, mid - center);
        }
    }

    /// A cylinder from `a` to `b`, with flat end caps.
    pub fn cylinder(&mut self, material: Material, a: V3, b: V3, radius: f64, segments: usize) {
        let axis = (b - a).normalized();
        let helper = if axis.y.abs() < 0.9 {
            V3::UP
        } else {
            v3(1.0, 0.0, 0.0)
        };
        let p1 = axis.cross(helper).normalized();
        let p2 = axis.cross(p1).normalized();
        let len = (b - a).length();
        let at = |j: usize, end: V3| {
            let t = std::f64::consts::TAU * j as f64 / segments as f64;
            let d = p1 * t.cos() + p2 * t.sin();
            (end + d * radius, d)
        };
        let circumference = std::f64::consts::TAU * radius;
        for j in 0..segments {
            let (q0, n0) = at(j, a);
            let (q1, n1) = at(j + 1, a);
            let (q2, _) = at(j + 1, b);
            let (q3, _) = at(j, b);
            let (u0, u1) = (
                circumference * j as f64 / segments as f64,
                circumference * (j + 1) as f64 / segments as f64,
            );
            self.smooth_quad(
                material,
                [q0, q1, q2, q3],
                [n0, n1, n1, n0],
                [[u0, 0.0], [u1, 0.0], [u1, len], [u0, len]],
            );
        }
        let cap = |end: V3| (0..segments).map(|j| at(j, end).0).collect::<Vec<_>>();
        self.polygon(material, &cap(a), -axis);
        self.polygon(material, &cap(b), axis);
    }

    /// Sweeps `profile` (`(outward, up)` offsets) around a closed loop of corners at
    /// their own heights: cornices, mouldings and coping.
    pub fn extrude_loop(&mut self, material: Material, corners: &[V3], profile: &[(f64, f64)]) {
        let count = corners.len();
        if count < 3 || profile.len() < 2 {
            return;
        }
        let centre = corners.iter().fold(V3::ZERO, |s, &c| s + c) * (1.0 / count as f64);
        let edge_normal = |i: usize| {
            let (a, b) = (corners[i], corners[(i + 1) % count]);
            let d = (b - a).normalized();
            let n = v3(-d.z, 0.0, d.x);
            if n.dot((a + b) * 0.5 - centre) < 0.0 {
                -n
            } else {
                n
            }
        };
        // Mitre offset at each corner (exact for right angles).
        let mitre: Vec<V3> = (0..count)
            .map(|i| edge_normal((i + count - 1) % count) + edge_normal(i))
            .collect();
        let point =
            |i: usize, k: usize| corners[i] + mitre[i] * profile[k].0 + V3::UP * profile[k].1;
        let mut along = 0.0;
        for i in 0..count {
            let j = (i + 1) % count;
            let n = edge_normal(i);
            let len = (corners[j] - corners[i]).length();
            let mut height = 0.0;
            for k in 0..profile.len() - 1 {
                let (dout, dup) = (
                    profile[k + 1].0 - profile[k].0,
                    profile[k + 1].1 - profile[k].1,
                );
                let step = (dout * dout + dup * dup).sqrt();
                let facing = n * dup + V3::UP * -dout;
                let quad = [point(i, k), point(j, k), point(j, k + 1), point(i, k + 1)];
                let uv = [
                    [along, height],
                    [along + len, height],
                    [along + len, height + step],
                    [along, height + step],
                ];
                self.polygon_uv(material, &quad, facing, &uv);
                height += step;
            }
            along += len;
        }
    }
}

/// Materials for the six faces of a [`MeshSet::cuboid`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FaceMats {
    /// The `+n` face.
    pub front: Material,
    /// The `-n` face.
    pub back: Material,
    /// The `±u` faces.
    pub sides: Material,
    pub top: Material,
    pub bottom: Material,
}

impl FaceMats {
    pub const fn all(m: Material) -> Self {
        Self {
            front: m,
            back: m,
            sides: m,
            top: m,
            bottom: m,
        }
    }

    pub const fn with_top(m: Material, top: Material) -> Self {
        Self {
            front: m,
            back: m,
            sides: m,
            top,
            bottom: m,
        }
    }
}

/// A local coordinate frame: `u` along, `v` up, `n` across.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub origin: V3,
    pub u: V3,
    pub v: V3,
    pub n: V3,
}

impl Frame {
    /// World axes: `u = x`, `v = y`, `n = z`.
    pub const WORLD: Frame = Frame {
        origin: V3::ZERO,
        u: v3(1.0, 0.0, 0.0),
        v: V3::UP,
        n: v3(0.0, 0.0, 1.0),
    };

    /// An upright frame whose `u` runs along `dir` on the ground.
    pub fn upright(origin: V3, dir: [f64; 2]) -> Frame {
        let u = v3(dir[0], 0.0, dir[1]).normalized();
        Frame {
            origin,
            u,
            v: V3::UP,
            n: u.cross(V3::UP),
        }
    }

    pub fn at(&self, u: f64, v: f64, n: f64) -> V3 {
        self.origin + self.u * u + self.v * v + self.n * n
    }

    /// The same frame with `n` reversed.
    pub fn flipped(&self) -> Frame {
        Frame {
            n: -self.n,
            ..*self
        }
    }
}

/// A deterministic pseudo-random value in `[0, 1)` for a point.
pub fn hash3(p: V3, seed: u64) -> f64 {
    let q = |x: f64| (x * 1000.0).round() as i64 as u64;
    let mut h = q(p.x).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ q(p.y).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
        ^ q(p.z).wrapping_mul(0x1656_67B1_9E37_79F9)
        ^ seed.wrapping_mul(0xD6E8_FEB8_6659_FD93);
    h ^= h >> 33;
    h = h.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    h ^= h >> 33;
    h = h.wrapping_mul(0xC4CE_B9FE_1A85_EC53);
    h ^= h >> 33;
    (h >> 11) as f64 / (1u64 << 53) as f64
}

/// Unit icosphere vertices and triangles.
fn icosphere_unit(subdivisions: u32) -> (Vec<V3>, Vec<[usize; 3]>) {
    let t = (1.0 + 5f64.sqrt()) / 2.0;
    let mut vertices: Vec<V3> = [
        (-1.0, t, 0.0),
        (1.0, t, 0.0),
        (-1.0, -t, 0.0),
        (1.0, -t, 0.0),
        (0.0, -1.0, t),
        (0.0, 1.0, t),
        (0.0, -1.0, -t),
        (0.0, 1.0, -t),
        (t, 0.0, -1.0),
        (t, 0.0, 1.0),
        (-t, 0.0, -1.0),
        (-t, 0.0, 1.0),
    ]
    .iter()
    .map(|&(x, y, z)| v3(x, y, z).normalized())
    .collect();
    let mut faces: Vec<[usize; 3]> = vec![
        [0, 11, 5],
        [0, 5, 1],
        [0, 1, 7],
        [0, 7, 10],
        [0, 10, 11],
        [1, 5, 9],
        [5, 11, 4],
        [11, 10, 2],
        [10, 7, 6],
        [7, 1, 8],
        [3, 9, 4],
        [3, 4, 2],
        [3, 2, 6],
        [3, 6, 8],
        [3, 8, 9],
        [4, 9, 5],
        [2, 4, 11],
        [6, 2, 10],
        [8, 6, 7],
        [9, 8, 1],
    ];
    for _ in 0..subdivisions {
        let mut cache = std::collections::HashMap::new();
        let mut midpoint = |a: usize, b: usize, vertices: &mut Vec<V3>| {
            *cache.entry((a.min(b), a.max(b))).or_insert_with(|| {
                vertices.push(((vertices[a] + vertices[b]) * 0.5).normalized());
                vertices.len() - 1
            })
        };
        let mut next = Vec::with_capacity(faces.len() * 4);
        for [a, b, c] in faces {
            let ab = midpoint(a, b, &mut vertices);
            let bc = midpoint(b, c, &mut vertices);
            let ca = midpoint(c, a, &mut vertices);
            next.extend([[a, ab, ca], [b, bc, ab], [c, ca, bc], [ab, bc, ca]]);
        }
        faces = next;
    }
    (vertices, faces)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_faces_point(set: &MeshSet, outward: impl Fn(V3, V3) -> bool) {
        for mesh in set.meshes.values() {
            for tri in mesh.indices.chunks(3) {
                let p: Vec<V3> = tri
                    .iter()
                    .map(|&i| {
                        let [x, y, z] = mesh.positions[i as usize];
                        v3(x as f64, y as f64, z as f64)
                    })
                    .collect();
                let geometric = (p[1] - p[0]).cross(p[2] - p[0]);
                let centre = (p[0] + p[1] + p[2]) * (1.0 / 3.0);
                assert!(
                    outward(centre, geometric),
                    "triangle faces inward at {centre:?}"
                );
            }
        }
    }

    #[test]
    fn cuboid_faces_point_outward() {
        let mut set = MeshSet::default();
        set.cuboid(
            &Frame::WORLD,
            (0.0, 2.0),
            (0.0, 1.0),
            (0.0, 3.0),
            FaceMats::all(Material::Stone),
        );
        assert_eq!(set.triangle_count(), 12);
        let centre = v3(1.0, 0.5, 1.5);
        assert_faces_point(&set, |c, n| n.dot(c - centre) > 0.0);
    }

    #[test]
    fn icosphere_faces_point_outward() {
        let mut set = MeshSet::default();
        set.icosphere(
            Material::Foliage,
            v3(1.0, 2.0, 3.0),
            v3(1.0, 0.6, 1.0),
            1,
            0.2,
            9,
        );
        assert_eq!(set.triangle_count(), 80);
        assert_faces_point(&set, |c, n| n.dot(c - v3(1.0, 2.0, 3.0)) > 0.0);
    }

    #[test]
    fn lathe_faces_point_outward() {
        let mut set = MeshSet::default();
        set.lathe(
            Material::Stone,
            V3::ZERO,
            &[(0.3, 0.0), (0.25, 1.0), (0.3, 2.0)],
            8,
            true,
        );
        set.lathe(
            Material::Stone,
            V3::ZERO,
            &[(0.3, 0.0), (0.25, 1.0), (0.3, 2.0)],
            8,
            false,
        );
        assert_faces_point(&set, |c, n| n.dot(c - v3(0.0, c.y, 0.0)) > -1e-9);
    }
}
