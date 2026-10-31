//! Geometry for every kind of layout part.
//!
//! [`Scene`] precomputes what parts need to know about each other, where
//! walls meet (they are trimmed and a corner post fills the joint) and which
//! openings sit on which wall (the wall cuts a hole for each), then builds
//! meshes per part, per level or for the whole layout.

use std::collections::HashMap;
use std::f64::consts::{FRAC_PI_2, PI};

use super::{FaceMats, Frame, Material, MeshSet, V3, hash3, v3};
use crate::geometry::Rect;
use crate::model::{OpeningKind, Roof, RoomKind};
use crate::parts::*;

const ARCH_SEGMENTS: usize = 10;
const STAIR_RISER: f64 = 0.19;
/// Wall height left standing in cutaway mode.
const CUT_HEIGHT: f64 = 1.0;
/// Wall height left standing in walls-down mode.
const DOWN_HEIGHT: f64 = 0.3;

/// How much of the walls to show, like the walls up / cutaway / down toggle in the Sims.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum WallMode {
    #[default]
    Up,
    /// Walls facing the camera are cut down to waist height.
    Cutaway,
    Down,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct View {
    pub walls: WallMode,
    /// Horizontal direction the camera looks in; needed for cutaway.
    pub look: Option<P2>,
}

struct WallInfo {
    /// `u` along the wall from `a`, `n` outwards for facades. The origin is at ground height.
    frame: Frame,
    start: f64,
    end: f64,
    /// Where the wall itself starts.
    base: f64,
    /// The base of its level, which openings measure their sills from.
    level_base: f64,
    group: Option<u32>,
    outer: Material,
    inner: Material,
    edge: Material,
    exterior: bool,
    level: u32,
    mid: P2,
}

struct Post {
    level: u32,
    group: Option<u32>,
    base: f64,
    at: P2,
    size: f64,
    height: f64,
    material: Material,
    walls: Vec<PartId>,
}

/// A layout prepared for meshing.
pub struct Scene<'a> {
    layout: &'a Layout,
    walls: HashMap<PartId, WallInfo>,
    mounts: HashMap<PartId, Vec<&'a Part>>,
    posts: Vec<Post>,
    /// The middle of each building's floor area on each level.
    centroids: HashMap<(Option<u32>, u32), P2>,
}

/// A hole through a wall, in the wall frame (`v` is the absolute height).
#[derive(Clone, Copy, Debug)]
struct Hole {
    u0: f64,
    u1: f64,
    v0: f64,
    v1: f64,
    arched: bool,
}

impl Hole {
    fn radius(&self) -> f64 {
        (self.u1 - self.u0) / 2.0
    }

    fn centre(&self) -> f64 {
        (self.u0 + self.u1) / 2.0
    }

    /// Where the arch springs from; the plain top otherwise.
    fn rect_top(&self) -> f64 {
        if self.arched {
            self.v1 - self.radius()
        } else {
            self.v1
        }
    }
}

fn ground_dot(frame: &Frame, d: P2) -> f64 {
    frame.n.x * d[0] + frame.n.z * d[1]
}

/// Distance of `p` along a frame's `u` axis.
fn along(frame: &Frame, p: P2) -> f64 {
    (V3::ground(p, 0.0) - frame.origin).dot(frame.u)
}

impl<'a> Scene<'a> {
    pub fn new(layout: &'a Layout) -> Self {
        let style = &layout.style;
        let mut floor_sums: HashMap<(Option<u32>, u32), (f64, f64, f64)> = HashMap::new();
        let mut wall_sums: HashMap<(Option<u32>, u32), (f64, f64, f64)> = HashMap::new();
        for p in &layout.parts {
            let key = (p.group, p.level);
            match &p.kind {
                PartKind::Floor(f) => {
                    let (cx, cz) = f.rect.center();
                    let a = f.rect.area();
                    let e = floor_sums.entry(key).or_default();
                    *e = (e.0 + cx * a, e.1 + cz * a, e.2 + a);
                }
                PartKind::Wall(w) if w.finish == WallFinish::Building => {
                    let l = w.length();
                    let e = wall_sums.entry(key).or_default();
                    *e = (
                        e.0 + (w.a[0] + w.b[0]) / 2.0 * l,
                        e.1 + (w.a[1] + w.b[1]) / 2.0 * l,
                        e.2 + l,
                    );
                }
                _ => {}
            }
        }
        let mut centroids = HashMap::new();
        for (key, (sx, sz, sw)) in wall_sums.into_iter().chain(floor_sums) {
            if sw > 0.0 {
                centroids.insert(key, [sx / sw, sz / sw]);
            }
        }

        let all_walls: Vec<(&Part, &Wall)> = layout
            .parts
            .iter()
            .filter_map(|p| match &p.kind {
                PartKind::Wall(w) if w.length() > 1e-3 => Some((p, w)),
                _ => None,
            })
            .collect();
        let mut walls = HashMap::new();
        let mut posts: Vec<Post> = Vec::new();
        for &(part, w) in &all_walls {
            let len = w.length();
            let dir = w.dir();
            let mid = w.point_at(len / 2.0);
            let mut frame = Frame::upright(V3::ground(w.a, 0.0), dir);
            if let Some(c) = centroids.get(&(part.group, part.level)) {
                if w.exterior && ground_dot(&frame, [mid[0] - c[0], mid[1] - c[1]]) < 0.0 {
                    frame = frame.flipped();
                }
            }
            let outer = match w.finish {
                WallFinish::Garden => Material::Plaster,
                WallFinish::DryStone => Material::Rubble,
                WallFinish::Fence => Material::Stone,
                WallFinish::Building if !w.exterior => Material::InteriorPlaster,
                WallFinish::Building if part.level == 0 && style.rusticated_ground => {
                    Material::Rustication
                }
                WallFinish::Building => Material::Plaster,
            };
            let edge = match outer {
                Material::Rustication => Material::Stone,
                m => m,
            };

            let mut trims = [0.0f64; 2];
            for (end, p) in [w.a, w.b].into_iter().enumerate() {
                for &(other_part, other) in &all_walls {
                    if other_part.id == part.id
                        || other_part.level != part.level
                        || other_part.group != part.group
                    {
                        continue;
                    }
                    let (_, d) = other.closest(p);
                    if d > other.thickness / 2.0 + 1e-4 {
                        continue;
                    }
                    let corner = dist(p, other.a) < 1e-3 || dist(p, other.b) < 1e-3;
                    if !corner {
                        // T-junction: stop at the other wall's face.
                        trims[end] = trims[end].max(other.thickness / 2.0 - d);
                        continue;
                    }
                    let od = other.dir();
                    if (dir[0] * od[1] - dir[1] * od[0]).abs() < 1e-3 {
                        continue; // collinear walls simply meet end to end
                    }
                    trims[end] = trims[end].max(other.thickness / 2.0);
                    let size = w.thickness.max(other.thickness);
                    let height = w.height.max(other.height);
                    match posts
                        .iter_mut()
                        .find(|q| q.level == part.level && dist(q.at, p) < 1e-3)
                    {
                        Some(post) => {
                            post.size = post.size.max(size);
                            post.height = post.height.max(height);
                            if w.exterior {
                                post.material = outer;
                            }
                            if !post.walls.contains(&part.id) {
                                post.walls.push(part.id);
                            }
                        }
                        None => posts.push(Post {
                            level: part.level,
                            group: part.group,
                            base: layout.base(part) + w.base.min(other.base),
                            at: p,
                            size,
                            height,
                            material: outer,
                            walls: vec![part.id],
                        }),
                    }
                }
            }
            walls.insert(
                part.id,
                WallInfo {
                    frame,
                    start: trims[0],
                    end: len - trims[1],
                    base: layout.base(part) + w.base,
                    level_base: layout.base(part),
                    group: part.group,
                    outer,
                    inner: if w.exterior || w.finish != WallFinish::Building {
                        Material::InteriorPlaster
                    } else {
                        outer
                    },
                    edge,
                    exterior: w.exterior,
                    level: part.level,
                    mid,
                },
            );
        }

        let mut mounts: HashMap<PartId, Vec<&Part>> = HashMap::new();
        for part in &layout.parts {
            let at = match &part.kind {
                PartKind::Opening(o) => o.at,
                PartKind::Balcony(b) => b.at,
                _ => continue,
            };
            if let Some((host, _)) = layout.host_wall_in(part.level, part.group, at) {
                mounts.entry(host.id).or_default().push(part);
            }
        }

        Self {
            layout,
            walls,
            mounts,
            posts,
            centroids,
        }
    }

    pub fn layout(&self) -> &Layout {
        self.layout
    }

    /// Everything, at full height.
    pub fn build_all(&self, view: &View) -> MeshSet {
        let mut set = MeshSet::default();
        for part in &self.layout.parts {
            self.part_into(&mut set, part, view);
        }
        let levels: Vec<u32> = self.posts.iter().map(|p| p.level).collect();
        for level in levels {
            set.merge(self.build_posts(level, view));
        }
        set
    }

    /// Every part on one level, plus the corner posts where its walls meet.
    pub fn build_level(&self, level: u32, view: &View) -> MeshSet {
        let mut set = MeshSet::default();
        for part in self.layout.on_level(level) {
            self.part_into(&mut set, part, view);
        }
        set.merge(self.build_posts(level, view));
        set
    }

    pub fn build_part(&self, part: &Part, view: &View) -> MeshSet {
        let mut set = MeshSet::default();
        self.part_into(&mut set, part, view);
        set
    }

    /// The square posts that fill the joints where walls on `level` meet.
    pub fn build_posts(&self, level: u32, view: &View) -> MeshSet {
        let mut set = MeshSet::default();
        for post in self.posts.iter().filter(|p| p.level == level) {
            self.post_into(&mut set, post, view);
        }
        set
    }

    /// The corner posts on `level`, one set per building.
    pub fn build_posts_by_group(&self, level: u32, view: &View) -> Vec<(Option<u32>, MeshSet)> {
        let mut out: Vec<(Option<u32>, MeshSet)> = Vec::new();
        for post in self.posts.iter().filter(|p| p.level == level) {
            let i = match out.iter().position(|(g, _)| *g == post.group) {
                Some(i) => i,
                None => {
                    out.push((post.group, MeshSet::default()));
                    out.len() - 1
                }
            };
            self.post_into(&mut out[i].1, post, view);
        }
        out
    }

    fn post_into(&self, set: &mut MeshSet, post: &Post, view: &View) {
        let cap = post
            .walls
            .iter()
            .filter_map(|id| self.walls.get(id).and_then(|w| self.wall_cap(w, view)))
            .fold(None, |acc: Option<f64>, c| {
                Some(acc.map_or(c, |a| a.min(c)))
            });
        let height = cap.map_or(post.height, |c| c.min(post.height));
        let s = post.size;
        let rect = Rect::new(post.at[0] - s / 2.0, post.at[1] - s / 2.0, s, s);
        set.slab(
            &rect,
            post.base,
            post.base + height,
            FaceMats::all(post.material),
        );
    }

    fn part_into(&self, set: &mut MeshSet, part: &Part, view: &View) {
        match &part.kind {
            PartKind::Wall(w) => self.wall(set, part, w, view),
            PartKind::Opening(o) => self.opening(set, part, o, view),
            PartKind::Balcony(b) => self.balcony(set, part, b),
            PartKind::Floor(f) => self.floor(set, part, f),
            PartKind::Stairs(s) => self.stairs(set, part, s),
            PartKind::Roof(r) => self.roof(set, part, r),
            PartKind::Area(a) => self.area(set, part, a),
            PartKind::Prop(p) => self.prop(set, part, p),
        }
    }

    /// The height a wall is cut down to in the current view, if any.
    fn wall_cap(&self, info: &WallInfo, view: &View) -> Option<f64> {
        match view.walls {
            WallMode::Up => None,
            WallMode::Down => Some(DOWN_HEIGHT),
            WallMode::Cutaway => {
                let look = view.look?;
                let toward_camera = if info.exterior {
                    -ground_dot(&info.frame, look)
                } else {
                    let c = self.centroids.get(&(info.group, info.level))?;
                    -((info.mid[0] - c[0]) * look[0] + (info.mid[1] - c[1]) * look[1])
                };
                (toward_camera > 0.25).then_some(CUT_HEIGHT)
            }
        }
    }

    /// The frame an opening or balcony hangs in, with `n` pointing towards `facing`:
    /// `(frame, centre along u, half wall thickness, on a facade, cut height)`.
    fn mount_frame(
        &self,
        part: &Part,
        at: P2,
        facing: P2,
        view: &View,
    ) -> (Frame, f64, f64, bool, Option<f64>) {
        let host = self
            .layout
            .host_wall_in(part.level, part.group, at)
            .and_then(|(hp, w)| self.walls.get(&hp.id).map(|info| (w, info)));
        match host {
            Some((w, info)) => {
                let mut frame = info.frame;
                if ground_dot(&frame, facing) < 0.0 {
                    frame = frame.flipped();
                }
                (
                    frame,
                    along(&frame, at),
                    w.thickness / 2.0,
                    w.exterior,
                    self.wall_cap(info, view),
                )
            }
            None => {
                let frame = Frame::upright(V3::ground(at, 0.0), [facing[1], -facing[0]]);
                (
                    frame,
                    0.0,
                    self.layout.style.wall_thickness / 2.0,
                    true,
                    None,
                )
            }
        }
    }

    fn wall(&self, set: &mut MeshSet, part: &Part, w: &Wall, view: &View) {
        let Some(info) = self.walls.get(&part.id) else {
            return;
        };
        let style = &self.layout.style;
        let cap = self.wall_cap(info, view);
        let y0 = info.base;
        let y1 = y0 + cap.map_or(w.height, |c| c.min(w.height));
        let half = w.thickness / 2.0;

        let mut holes: Vec<Hole> = Vec::new();
        for mount in self.mounts.get(&part.id).into_iter().flatten() {
            let PartKind::Opening(o) = &mount.kind else {
                continue;
            };
            let centre = along(&info.frame, o.at);
            let mut hole = Hole {
                u0: (centre - o.width / 2.0).max(info.start),
                u1: (centre + o.width / 2.0).min(info.end),
                v0: info.level_base + o.sill,
                v1: if o.kind == OpeningKind::Gate {
                    y1 + 1.0
                } else {
                    info.level_base + o.sill + o.height
                },
                arched: o.arched && o.kind != OpeningKind::Gate,
            };
            if hole.v0 >= y1 - 0.01 || hole.u1 - hole.u0 < 0.05 {
                continue;
            }
            if o.kind == OpeningKind::Gate || (cap.is_some() && hole.v1 > y1 - 0.05) {
                hole.v1 = y1;
                hole.arched = false;
            } else if hole.v1 > y1 - 0.05 {
                hole.v1 = y1 - 0.05;
            }
            hole.arched &= hole.v1 - hole.v0 >= hole.radius() && hole.u1 - hole.u0 > o.width - 1e-6;
            holes.push(hole);
        }
        holes.sort_by(|a, b| a.u0.total_cmp(&b.u0));
        let mut kept: Vec<Hole> = Vec::new();
        for hole in holes {
            if kept.last().is_none_or(|k| hole.u0 >= k.u1) {
                kept.push(hole);
            }
        }

        let mats = FaceMats {
            front: info.outer,
            back: info.inner,
            sides: info.edge,
            top: info.edge,
            bottom: info.edge,
        };
        // A fence is a low stone plinth carrying iron railings.
        let solid_top = if w.finish == WallFinish::Fence {
            (y0 + 0.5).min(y1)
        } else {
            y1
        };
        wall_with_holes(
            set,
            &info.frame,
            info.start,
            info.end,
            y0,
            solid_top,
            half,
            &kept,
            mats,
        );
        let gaps = |u: f64| kept.iter().any(|h| u > h.u0 - 0.05 && u < h.u1 + 0.05);
        match w.finish {
            WallFinish::Garden if cap.is_none() => {
                let mut cursor = info.start;
                for h in kept.iter().chain(std::iter::once(&Hole {
                    u0: info.end,
                    u1: info.end,
                    v0: 0.0,
                    v1: 0.0,
                    arched: false,
                })) {
                    set.cuboid(
                        &info.frame,
                        (cursor, h.u0),
                        (y1, y1 + 0.08),
                        (-half - 0.05, half + 0.05),
                        FaceMats::all(Material::Stone),
                    );
                    cursor = h.u1;
                }
            }
            WallFinish::Fence if y1 > solid_top + 0.1 => {
                let iron = FaceMats::all(Material::Iron);
                let mut u = info.start + 0.06;
                while u < info.end - 0.06 {
                    if !gaps(u) {
                        set.cuboid(
                            &info.frame,
                            (u - 0.015, u + 0.015),
                            (solid_top, y1 - 0.06),
                            (-0.015, 0.015),
                            iron,
                        );
                    }
                    u += 0.14;
                }
                let mut cursor = info.start;
                for h in kept.iter().chain(std::iter::once(&Hole {
                    u0: info.end,
                    u1: info.end,
                    v0: 0.0,
                    v1: 0.0,
                    arched: false,
                })) {
                    set.cuboid(
                        &info.frame,
                        (cursor, h.u0),
                        (y1 - 0.08, y1 - 0.03),
                        (-0.03, 0.03),
                        iron,
                    );
                    cursor = h.u1;
                }
            }
            _ => {}
        }

        // Quoins: alternating long and short stone blocks up the corners.
        if w.exterior && style.quoins && cap.is_none() {
            let rows = ((y1 - y0) / 0.42).floor() as usize;
            for k in 0..rows {
                let long = if k % 2 == 0 { 0.75 } else { 0.42 };
                let (v0, v1) = (
                    y0 + k as f64 * 0.42 + 0.02,
                    y0 + (k + 1) as f64 * 0.42 - 0.02,
                );
                let n = (half, half + 0.035);
                if info.start > 1e-6 {
                    let u0 = info.start - half;
                    set.cuboid(
                        &info.frame,
                        (u0, u0 + long),
                        (v0, v1),
                        n,
                        FaceMats::all(Material::Stone),
                    );
                }
                if info.end < w.length() - 1e-6 {
                    let u1 = info.end + half;
                    set.cuboid(
                        &info.frame,
                        (u1 - long, u1),
                        (v0, v1),
                        n,
                        FaceMats::all(Material::Stone),
                    );
                }
            }
        }

        // String course at each floor line and a plinth at the ground.
        if w.exterior && style.string_courses && y1 - y0 > 0.6 {
            let (top, proj) = if part.level == 0 {
                (y0 + 0.5, 0.05)
            } else {
                (y0 + 0.24, 0.07)
            };
            let start = if info.start > 1e-6 {
                info.start - half - proj
            } else {
                info.start
            };
            let end = if info.end < w.length() - 1e-6 {
                info.end + half + proj
            } else {
                info.end
            };
            let mut cursor = start;
            for hole in kept.iter().filter(|h| h.v0 < top) {
                set.cuboid(
                    &info.frame,
                    (cursor, hole.u0),
                    (y0, top),
                    (half, half + proj),
                    FaceMats::all(Material::Stone),
                );
                cursor = cursor.max(hole.u1);
            }
            if end > cursor {
                set.cuboid(
                    &info.frame,
                    (cursor, end),
                    (y0, top),
                    (half, half + proj),
                    FaceMats::all(Material::Stone),
                );
            }
        }
    }

    fn opening(&self, set: &mut MeshSet, part: &Part, o: &OpeningPart, view: &View) {
        if o.kind == OpeningKind::Archway {
            return;
        }
        let (frame, centre, half, exterior, cap) = self.mount_frame(part, o.at, o.facing, view);
        if cap.is_some() {
            return; // cut-down walls show the hole, not the fittings
        }
        let style = &self.layout.style;
        let base = self.layout.base(part);
        if o.kind == OpeningKind::Gate {
            self.gate(set, part, o, &frame, centre, half.max(0.2), base);
            return;
        }
        let hole = Hole {
            u0: centre - o.width / 2.0,
            u1: centre + o.width / 2.0,
            v0: base + o.sill,
            v1: base + o.sill + o.height,
            arched: o.arched && o.height >= o.width / 2.0,
        };
        let top = hole.rect_top();
        let r = hole.radius();
        let wood = FaceMats::all(Material::Wood);
        let stone = FaceMats::all(Material::Stone);

        match o.kind {
            OpeningKind::Window | OpeningKind::BalconyDoor | OpeningKind::Storefront => {
                set.cuboid(
                    &frame,
                    (hole.u0, hole.u1),
                    (hole.v0, top),
                    (-0.015, 0.015),
                    FaceMats::all(Material::Glass),
                );
                if hole.arched {
                    half_disc(set, Material::Glass, &frame, centre, top, r, 0.015);
                    half_disc(set, Material::Glass, &frame, centre, top, r, -0.015);
                }
                let bar = 0.05;
                let depth = (-0.03, 0.03);
                set.cuboid(
                    &frame,
                    (hole.u0, hole.u0 + bar),
                    (hole.v0, top),
                    depth,
                    wood,
                );
                set.cuboid(
                    &frame,
                    (hole.u1 - bar, hole.u1),
                    (hole.v0, top),
                    depth,
                    wood,
                );
                set.cuboid(
                    &frame,
                    (hole.u0, hole.u1),
                    (hole.v0, hole.v0 + bar),
                    depth,
                    wood,
                );
                let transom = if hole.arched {
                    top
                } else {
                    hole.v0 + (hole.v1 - hole.v0) * 0.66
                };
                set.cuboid(
                    &frame,
                    (hole.u0, hole.u1),
                    (transom - bar / 2.0, transom + bar / 2.0),
                    depth,
                    wood,
                );
                let mullions = if o.kind == OpeningKind::Storefront {
                    3
                } else {
                    1
                };
                for m in 1..=mullions {
                    let u = hole.u0 + o.width * m as f64 / (mullions + 1) as f64;
                    let reach = if hole.arched && mullions == 1 {
                        hole.v1
                    } else {
                        top
                    };
                    set.cuboid(
                        &frame,
                        (u - bar / 2.0, u + bar / 2.0),
                        (hole.v0, reach),
                        depth,
                        wood,
                    );
                }
            }
            OpeningKind::Door => {
                let leaf = (-0.09, -0.01);
                set.cuboid(
                    &frame,
                    (hole.u0, hole.u1),
                    (hole.v0, top),
                    leaf,
                    FaceMats::all(Material::Door),
                );
                if hole.arched {
                    half_disc(set, Material::Door, &frame, centre, top, r, leaf.1);
                    half_disc(set, Material::Door, &frame, centre, top, r, leaf.0);
                }
                if part.level == 0 {
                    set.cuboid(
                        &frame,
                        (hole.u0 - 0.3, hole.u1 + 0.3),
                        (base, base + o.sill.max(0.05)),
                        (half, half + 0.45),
                        stone,
                    );
                }
            }
            OpeningKind::Archway | OpeningKind::Gate => {}
        }

        if !exterior {
            return;
        }
        if o.kind == OpeningKind::Window {
            set.cuboid(
                &frame,
                (hole.u0 - 0.08, hole.u1 + 0.08),
                (hole.v0 - 0.07, hole.v0),
                (0.0, half + 0.1),
                stone,
            );
        }

        let surround = style.window_surrounds;
        let sw = if matches!(o.kind, OpeningKind::Door | OpeningKind::Storefront) {
            0.24
        } else {
            0.16
        };
        let sp = 0.05;
        if surround {
            let n = (half, half + sp);
            set.cuboid(&frame, (hole.u0 - sw, hole.u0), (hole.v0, top), n, stone);
            set.cuboid(&frame, (hole.u1, hole.u1 + sw), (hole.v0, top), n, stone);
            if hole.arched {
                archivolt(set, &frame, centre, top, r, sw, half, half + sp);
                set.cuboid(
                    &frame,
                    (centre - 0.11, centre + 0.11),
                    (hole.v1 - 0.06, hole.v1 + sw + 0.08),
                    (half, half + sp + 0.04),
                    stone,
                );
            } else {
                set.cuboid(
                    &frame,
                    (hole.u0 - sw, hole.u1 + sw),
                    (hole.v1, hole.v1 + sw),
                    n,
                    stone,
                );
                if style.pediments
                    && matches!(o.kind, OpeningKind::Window | OpeningKind::BalconyDoor)
                    && o.height > 1.0
                {
                    pediment(
                        set,
                        &frame,
                        hole.u0 - sw - 0.08,
                        hole.u1 + sw + 0.08,
                        hole.v1 + sw,
                        half,
                        hash3(V3::ground(o.at, 0.0), 5) < 0.5,
                    );
                }
            }
        }

        let shuttered = matches!(o.kind, OpeningKind::Window | OpeningKind::BalconyDoor)
            && o.width >= 0.9 * style.window.width.min(style.balcony_door.width);
        if style.shutters && shuttered {
            let n0 = half + if surround { sp + 0.01 } else { 0.01 };
            let n = (n0, n0 + 0.04);
            let leaf = o.width / 2.0;
            let gap = if surround { sw } else { 0.0 };
            let mats = FaceMats::all(Material::Shutter);
            set.cuboid(
                &frame,
                (hole.u0 - gap - leaf, hole.u0 - gap),
                (hole.v0, top),
                n,
                mats,
            );
            set.cuboid(
                &frame,
                (hole.u1 + gap, hole.u1 + gap + leaf),
                (hole.v0, top),
                n,
                mats,
            );
        }
    }

    fn balcony(&self, set: &mut MeshSet, part: &Part, b: &BalconyPart) {
        let (frame, centre, half, _, _) = self.mount_frame(part, b.at, b.facing, &View::default());
        let style = &self.layout.style;
        let base = self.layout.base(part);
        let floor = base + style.slab_thickness;
        let (u0, u1) = (centre - b.width / 2.0, centre + b.width / 2.0);
        let (n0, n1) = (half, half + b.depth);
        let stone = FaceMats::all(Material::Stone);
        set.cuboid(&frame, (u0, u1), (base - 0.04, floor), (n0, n1), stone);
        let corbels = ((b.width / 1.0).round() as usize).max(2);
        for k in 0..corbels {
            let u = u0 + 0.25 + (b.width - 0.5) * k as f64 / (corbels - 1) as f64;
            set.cuboid(
                &frame,
                (u - 0.08, u + 0.08),
                (base - 0.32, base - 0.04),
                (n0, n0 + b.depth * 0.7),
                stone,
            );
            set.cuboid(
                &frame,
                (u - 0.08, u + 0.08),
                (base - 0.5, base - 0.32),
                (n0, n0 + b.depth * 0.35),
                stone,
            );
        }
        let rail = 1.0;
        if style.stone_balustrade {
            let w = 0.14;
            let edges = [
                ((u0, u1), (n1 - w, n1)),
                ((u0, u0 + w), (n0, n1 - w)),
                ((u1 - w, u1), (n0, n1 - w)),
            ];
            for (u, n) in edges {
                set.cuboid(&frame, u, (floor, floor + 0.1), n, stone);
                set.cuboid(&frame, u, (floor + rail - 0.1, floor + rail), n, stone);
            }
            let h = rail - 0.2;
            let profile: Vec<(f64, f64)> = [
                (0.05, 0.0),
                (0.05, 0.08),
                (0.035, 0.14),
                (0.07, 0.42),
                (0.03, 0.78),
                (0.05, 0.86),
                (0.05, 1.0),
            ]
            .iter()
            .map(|&(r, y)| (r, y * h))
            .collect();
            let mut spots: Vec<(f64, f64)> = Vec::new();
            let along_front = ((b.width - 0.3) / 0.26).floor() as usize;
            for k in 0..=along_front {
                spots.push((
                    u0 + 0.15 + (b.width - 0.3) * k as f64 / along_front.max(1) as f64,
                    n1 - w / 2.0,
                ));
            }
            let along_side = ((b.depth - 0.3) / 0.26).floor() as usize;
            for k in 0..along_side {
                let n = n0 + 0.1 + (b.depth - 0.3) * k as f64 / along_side.max(1) as f64;
                spots.push((u0 + w / 2.0, n));
                spots.push((u1 - w / 2.0, n));
            }
            for (u, n) in spots {
                set.lathe(
                    Material::Stone,
                    frame.at(u, floor + 0.1, n),
                    &profile,
                    5,
                    true,
                );
            }
        } else {
            let iron = FaceMats::all(Material::Iron);
            let bar = 0.03;
            set.cuboid(
                &frame,
                (u0, u1),
                (floor + rail - 0.05, floor + rail),
                (n1 - 0.05, n1),
                iron,
            );
            for u in [(u0, u0 + 0.05), (u1 - 0.05, u1)] {
                set.cuboid(
                    &frame,
                    u,
                    (floor + rail - 0.05, floor + rail),
                    (n0, n1),
                    iron,
                );
            }
            let posts = ((u1 - u0) / 0.14).floor() as usize;
            if style.curved_railings {
                // "Petto d'oca": the bars swell outwards near the bottom like a goose's breast.
                let bulge = 0.22;
                set.cuboid(
                    &frame,
                    (u0, u1),
                    (floor + 0.3, floor + 0.34),
                    (n1 + bulge - 0.03, n1 + bulge),
                    iron,
                );
                for p in 0..=posts {
                    let u = u0 + 0.02 + (u1 - u0 - 0.04) * p as f64 / posts.max(1) as f64;
                    let points = [
                        frame.at(u, floor, n1 - 0.02),
                        frame.at(u, floor + 0.32, n1 + bulge - 0.015),
                        frame.at(u, floor + 0.72, n1 + 0.04),
                        frame.at(u, floor + rail, n1 - 0.025),
                    ];
                    for pair in points.windows(2) {
                        set.cylinder(Material::Iron, pair[0], pair[1], 0.012, 4);
                    }
                }
            } else {
                for p in 0..=posts {
                    let u = u0 + (u1 - u0 - bar) * p as f64 / posts.max(1) as f64;
                    set.cuboid(
                        &frame,
                        (u, u + bar),
                        (floor, floor + rail),
                        (n1 - bar, n1),
                        iron,
                    );
                }
            }
            let side_posts = (b.depth / 0.14).floor() as usize;
            for p in 1..side_posts {
                let n = n0 + b.depth * p as f64 / side_posts as f64;
                for u in [u0, u1 - bar] {
                    set.cuboid(
                        &frame,
                        (u, u + bar),
                        (floor, floor + rail),
                        (n, n + bar),
                        iron,
                    );
                }
            }
        }
    }

    fn floor(&self, set: &mut MeshSet, part: &Part, f: &FloorPart) {
        let base = self.layout.base(part);
        let top = if f.room == Some(RoomKind::Bathroom) {
            Material::Stone
        } else {
            Material::Cotto
        };
        let mats = FaceMats::with_top(Material::InteriorPlaster, top);
        set.slab(&f.rect, base, base + self.layout.style.slab_thickness, mats);
    }

    fn stairs(&self, set: &mut MeshSet, part: &Part, s: &StairsPart) {
        let r = s.rect;
        let base = self.layout.base(part) + self.layout.style.slab_thickness;
        let (origin, run_dir, width_dir, run_len, width) = match s.heading {
            Heading::North => ([r.x, r.z], [0.0, 1.0], [1.0, 0.0], r.d, r.w),
            Heading::South => ([r.x1(), r.z1()], [0.0, -1.0], [-1.0, 0.0], r.d, r.w),
            Heading::East => ([r.x, r.z1()], [1.0, 0.0], [0.0, -1.0], r.w, r.d),
            Heading::West => ([r.x1(), r.z], [-1.0, 0.0], [0.0, 1.0], r.w, r.d),
        };
        let frame = Frame {
            origin: V3::ground(origin, 0.0),
            u: v3(width_dir[0], 0.0, width_dir[1]),
            v: V3::UP,
            n: v3(run_dir[0], 0.0, run_dir[1]),
        };
        let steps = (s.rise / STAIR_RISER).ceil().max(1.0) as usize;
        let riser = s.rise / steps as f64;
        let landing = (run_len * 0.08).min(0.3);
        let run = ((run_len - 2.0 * landing) / steps as f64).max(0.05);
        for i in 0..steps {
            let n0 = landing + run * i as f64;
            set.cuboid(
                &frame,
                (0.05, width - 0.05),
                (base, base + riser * (i + 1) as f64),
                (n0, n0 + run),
                FaceMats::all(Material::Stone),
            );
        }
    }

    fn roof(&self, set: &mut MeshSet, part: &Part, r: &RoofPart) {
        let style = &self.layout.style;
        let base = self.layout.base(part);
        let inner = r.rect.grow(-style.wall_thickness / 2.0);
        let ceiling_top = match r.roof {
            Roof::Flat { .. } => Material::Cotto,
            Roof::Hip { .. } => Material::InteriorPlaster,
        };
        set.slab(
            &inner,
            base,
            base + style.slab_thickness,
            FaceMats::with_top(Material::InteriorPlaster, ceiling_top),
        );
        let y = base + style.slab_thickness;
        match r.roof {
            Roof::Flat {
                parapet_height,
                balustrade,
            } => {
                let (rect, t) = (r.rect, 0.25);
                let h = y + parapet_height;
                if style.cornice_depth > 0.0 && !r.open.iter().any(|o| *o) {
                    let d = style.cornice_depth;
                    let hc = d * 0.8;
                    let profile = [
                        (0.0, 0.0),
                        (0.15 * d, 0.0),
                        (0.15 * d, 0.2 * hc),
                        (0.5 * d, 0.45 * hc),
                        (0.55 * d, 0.7 * hc),
                        (d, hc),
                        (d, hc + 0.08),
                    ];
                    set.extrude_loop(
                        Material::Stone,
                        &rect_corners(&rect, y - 0.08 - hc),
                        &profile,
                    );
                }
                // South, north, west, east; side bands stop short of the end bands they meet.
                let z0 = if r.open[0] { rect.z } else { rect.z + t };
                let z1 = if r.open[1] { rect.z1() } else { rect.z1() - t };
                let bands = [
                    Rect::new(rect.x, rect.z, rect.w, t),
                    Rect::new(rect.x, rect.z1() - t, rect.w, t),
                    Rect::new(rect.x, z0, t, z1 - z0),
                    Rect::new(rect.x1() - t, z0, t, z1 - z0),
                ];
                for (k, band) in bands.iter().enumerate() {
                    if r.open[k] {
                        continue;
                    }
                    if balustrade {
                        balustrade_band(set, band, y, parapet_height);
                    } else {
                        set.slab(
                            band,
                            y,
                            h,
                            FaceMats::with_top(Material::Plaster, Material::Stone),
                        );
                        set.slab(
                            &band.grow(0.03),
                            h,
                            h + 0.07,
                            FaceMats::all(Material::Stone),
                        );
                    }
                }
            }
            Roof::Hip {
                overhang,
                ridge_height,
                ..
            } => {
                let fascia = 0.14;
                let soffit = y - fascia;
                if style.cornice_depth > 0.0 {
                    let d = style.cornice_depth;
                    let hc = d * 0.9;
                    let profile = [
                        (0.0, 0.0),
                        (0.12 * d, 0.0),
                        (0.12 * d, 0.12 * hc),
                        (0.3 * d, 0.3 * hc),
                        (0.55 * d, 0.5 * hc),
                        (0.55 * d, 0.62 * hc),
                        (0.9 * d, 0.85 * hc),
                        (d, hc),
                    ];
                    set.extrude_loop(
                        Material::Stone,
                        &rect_corners(&r.rect, soffit - hc),
                        &profile,
                    );
                }
                let eaves = r.rect.grow(overhang);
                hip_roof(set, &eaves, y, ridge_height);
                eave_trim(set, &eaves, soffit, y);
                if style.exposed_rafters {
                    rafters(set, &r.rect, &eaves, soffit, style.cornice_depth * 0.9);
                }
            }
        }
    }

    fn area(&self, set: &mut MeshSet, part: &Part, a: &AreaPart) {
        let base = self.layout.base(part);
        let r = a.rect;
        let style = &self.layout.style;
        if let Some(terrain) = self.layout.terrain.as_ref().filter(|_| a.kind.drapes()) {
            self.drape(set, a, terrain);
            return;
        }
        match a.kind {
            AreaKind::Lawn => set.slab(&r, base - 0.1, base, FaceMats::all(Material::Lawn)),
            AreaKind::Street => set.slab(
                &r,
                base - 0.2,
                base + 0.05,
                FaceMats::with_top(Material::Stone, Material::Basalt),
            ),
            AreaKind::Road => set.slab(&r, base - 0.1, base + 0.03, FaceMats::all(Material::Earth)),
            AreaKind::Piazza => {
                let low = self
                    .layout
                    .terrain
                    .as_ref()
                    .map_or(base - 0.2, |t| t.range_in(&r).0 - 0.3);
                set.slab(
                    &r,
                    low.min(base - 0.2),
                    base + 0.05,
                    FaceMats::with_top(Material::Stone, Material::Slabs),
                );
            }
            AreaKind::Platform => {
                // Rises `height` above its ground, and reaches down to the lowest terrain beneath it.
                let top = base + a.height;
                let low = self
                    .layout
                    .terrain
                    .as_ref()
                    .map_or(base - 0.05, |t| t.range_in(&r).0 - 0.4);
                if top - low > 0.15 {
                    set.slab(
                        &r,
                        low,
                        top,
                        FaceMats::with_top(Material::Rustication, Material::Stone),
                    );
                }
            }
            AreaKind::Courtyard => self.courtyard(set, a, base),
            AreaKind::Parterre => parterre(set, &r, base, a.height, part.id),
            AreaKind::Vineyard => {
                set.slab(&r, base - 0.1, base + 0.02, FaceMats::all(Material::Earth));
                vines(set, &r, a.height, &|_| base + 0.02);
            }
            AreaKind::Tower => tower(set, &r, base, a.height, style.roof_pitch_deg),
            AreaKind::Path => set.slab(
                &r,
                base,
                base + a.height.max(0.02),
                FaceMats::with_top(Material::Stone, Material::Gravel),
            ),
            AreaKind::Terrace => {
                let top = if style.loggia {
                    Material::Cotto
                } else {
                    Material::Stone
                };
                set.slab(
                    &r,
                    base,
                    base + a.height.max(0.02),
                    FaceMats::with_top(Material::Stone, top),
                );
            }
            AreaKind::Pool => {
                set.polygon(Material::Water, &rect_corners(&r, base + 0.02), V3::UP);
                let coping = [
                    (0.0, 0.0),
                    (0.32, 0.0),
                    (0.36, 0.04),
                    (0.32, 0.09),
                    (0.0, 0.09),
                    (0.0, 0.0),
                ];
                set.extrude_loop(Material::Stone, &rect_corners(&r, base), &coping);
            }
            AreaKind::Pergola => self.pergola(set, part, a, base),
            AreaKind::Loggia => self.loggia(set, part, a, base),
            AreaKind::Hedge => {
                set.slab(
                    &r,
                    base,
                    base + a.height * 0.85,
                    FaceMats::all(Material::Foliage),
                );
                let long = r.w >= r.d;
                let len = if long { r.w } else { r.d };
                let bumps = (len / 0.8).ceil().max(1.0) as usize;
                for k in 0..bumps {
                    let t = (k as f64 + 0.5) / bumps as f64;
                    let (cx, cz) = if long {
                        (r.x + r.w * t, r.z + r.d / 2.0)
                    } else {
                        (r.x + r.w / 2.0, r.z + r.d * t)
                    };
                    let rad = r.w.min(r.d) / 2.0;
                    set.icosphere(
                        Material::Foliage,
                        v3(cx, base + a.height * 0.82, cz),
                        v3(rad, a.height * 0.2, rad),
                        1,
                        0.3,
                        part.id + k as u64,
                    );
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn gate(
        &self,
        set: &mut MeshSet,
        part: &Part,
        o: &OpeningPart,
        frame: &Frame,
        centre: f64,
        half: f64,
        base: f64,
    ) {
        gate_mesh(
            set,
            frame,
            centre - o.width / 2.0,
            centre + o.width / 2.0,
            base,
            o.height,
            half,
            part.id,
        );
    }

    /// A paved courtyard ringed by a portico: columns and arches round a covered walk.
    fn courtyard(&self, set: &mut MeshSet, a: &AreaPart, base: f64) {
        let r = a.rect;
        set.slab(
            &r,
            base,
            base + 0.08,
            FaceMats::with_top(Material::Stone, Material::Slabs),
        );
        let inner = r.grow(-crate::generate::PORTICO_DEPTH);
        if inner.w < 2.0 || inner.d < 2.0 {
            return;
        }
        let spring = base + 2.7;
        // Corners clockwise seen from above, so each arcade faces out towards the covered walk.
        let corners = [
            [inner.x, inner.z],
            [inner.x, inner.z1()],
            [inner.x1(), inner.z1()],
            [inner.x1(), inner.z],
        ];
        let edges: Vec<(P2, P2)> = (0..4).map(|i| (corners[i], corners[(i + 1) % 4])).collect();
        let max_bay = edges
            .iter()
            .map(|(p, q)| bay_width(dist(*p, *q)))
            .fold(0.0, f64::max);
        let top = (base + a.height - 0.2).max(spring + max_bay / 2.0 + 0.4);
        for edge in &edges {
            arcade(set, *edge, spring, top, base + 0.08, true, true);
        }
        for walk in r.subtract(&inner.grow(-0.2)) {
            set.slab(
                &walk,
                top,
                top + 0.22,
                FaceMats::with_top(Material::Stone, Material::Cotto),
            );
        }
    }

    /// A ground covering laid over the terrain.
    fn drape(&self, set: &mut MeshSet, a: &AreaPart, terrain: &crate::terrain::Terrain) {
        let (material, lift) = match a.kind {
            AreaKind::Lawn => return, // the terrain is already grass
            AreaKind::Street => (Material::Basalt, 0.07),
            AreaKind::Terrace => (Material::Slabs, 0.03),
            AreaKind::Path => (Material::Gravel, 0.05),
            _ => (Material::Earth, 0.04),
        };
        let r = a.rect;
        let (sx, sz) = terrain.spacing();
        let nx = ((r.w / sx.max(1.0)).ceil() as usize).clamp(1, 200);
        let nz = ((r.d / sz.max(1.0)).ceil() as usize).clamp(1, 200);
        let h = |x: f64, z: f64| terrain.height_at([x, z]) + lift;
        for i in 0..nx {
            for j in 0..nz {
                let (x0, x1) = (
                    r.x + r.w * i as f64 / nx as f64,
                    r.x + r.w * (i + 1) as f64 / nx as f64,
                );
                let (z0, z1) = (
                    r.z + r.d * j as f64 / nz as f64,
                    r.z + r.d * (j + 1) as f64 / nz as f64,
                );
                let quad = [
                    v3(x0, h(x0, z0), z0),
                    v3(x1, h(x1, z0), z0),
                    v3(x1, h(x1, z1), z1),
                    v3(x0, h(x0, z1), z1),
                ];
                set.polygon(material, &[quad[0], quad[1], quad[2]], V3::UP);
                set.polygon(material, &[quad[0], quad[2], quad[3]], V3::UP);
            }
        }
        if a.kind == AreaKind::Vineyard {
            vines(set, &r, a.height, &|p| terrain.height_at(p));
        }
    }

    /// The ground itself: grass, with bare earth where it is steep.
    pub fn build_terrain(&self) -> MeshSet {
        let mut set = MeshSet::default();
        let Some(t) = &self.layout.terrain else {
            return set;
        };
        let at = |i: usize, j: usize| {
            let p = t.point(i, j);
            v3(p[0], t.get(i, j), p[1])
        };
        let ground = crate::landscape::ground_map(t, self.layout.water.as_ref());
        for j in 0..t.rows {
            for i in 0..t.cols {
                let quad = [at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j + 1)];
                let material = ground[j * (t.cols + 1) + i].material();
                for tri in [[quad[0], quad[1], quad[2]], [quad[0], quad[2], quad[3]]] {
                    set.polygon(material, &tri, V3::UP);
                }
            }
        }
        set
    }

    /// The sea, lake or river surface.
    pub fn build_water(&self) -> MeshSet {
        self.layout
            .water
            .as_ref()
            .map(|w| w.mesh())
            .unwrap_or_default()
    }

    /// Every building group on `level`, meshed separately so each can wear its own colours.
    pub fn build_level_groups(&self, level: u32, view: &View) -> Vec<(Option<u32>, MeshSet)> {
        let mut groups: Vec<(Option<u32>, MeshSet)> = Vec::new();
        let slot = |groups: &mut Vec<(Option<u32>, MeshSet)>, g: Option<u32>| match groups
            .iter()
            .position(|(k, _)| *k == g)
        {
            Some(i) => i,
            None => {
                groups.push((g, MeshSet::default()));
                groups.len() - 1
            }
        };
        for part in self.layout.on_level(level) {
            let i = slot(&mut groups, part.group);
            self.part_into(&mut groups[i].1, part, view);
        }
        for post in self.posts.iter().filter(|p| p.level == level) {
            let i = slot(&mut groups, post.group);
            self.post_into(&mut groups[i].1, post, view);
        }
        groups
    }

    fn pergola(&self, set: &mut MeshSet, part: &Part, a: &AreaPart, base: f64) {
        let r = a.rect;
        let h = base + a.height;
        let wood = FaceMats::all(Material::Wood);
        let post = 0.15;
        for (x, z) in [
            (r.x, r.z),
            (r.x1() - post, r.z),
            (r.x, r.z1() - post),
            (r.x1() - post, r.z1() - post),
        ] {
            set.cuboid(&Frame::WORLD, (x, x + post), (base, h), (z, z + post), wood);
        }
        let long_x = r.w >= r.d;
        if long_x {
            set.cuboid(
                &Frame::WORLD,
                (r.x, r.x1()),
                (h - 0.2, h),
                (r.z, r.z + post),
                wood,
            );
            set.cuboid(
                &Frame::WORLD,
                (r.x, r.x1()),
                (h - 0.2, h),
                (r.z1() - post, r.z1()),
                wood,
            );
        } else {
            set.cuboid(
                &Frame::WORLD,
                (r.x, r.x + post),
                (h - 0.2, h),
                (r.z, r.z1()),
                wood,
            );
            set.cuboid(
                &Frame::WORLD,
                (r.x1() - post, r.x1()),
                (h - 0.2, h),
                (r.z, r.z1()),
                wood,
            );
        }
        let span = if long_x { r.w } else { r.d };
        let slats = (span / 0.45).floor().max(1.0) as usize;
        for s in 0..=slats {
            let t = (span - 0.08) * s as f64 / slats as f64;
            if long_x {
                set.cuboid(
                    &Frame::WORLD,
                    (r.x + t, r.x + t + 0.08),
                    (h, h + 0.12),
                    (r.z - 0.1, r.z1() + 0.1),
                    wood,
                );
            } else {
                set.cuboid(
                    &Frame::WORLD,
                    (r.x - 0.1, r.x1() + 0.1),
                    (h, h + 0.12),
                    (r.z + t, r.z + t + 0.08),
                    wood,
                );
            }
        }
        let vines = ((r.w * r.d) / 2.5).ceil() as u64;
        for k in 0..vines {
            let p = v3(k as f64, part.id as f64, 0.0);
            let (fx, fz) = (hash3(p, 1), hash3(p, 2));
            let c = v3(
                r.x + 0.3 + (r.w - 0.6) * fx,
                h + 0.14,
                r.z + 0.3 + (r.d - 0.6) * fz,
            );
            set.icosphere(
                Material::Foliage,
                c,
                v3(0.55, 0.22, 0.45),
                1,
                0.4,
                part.id * 31 + k,
            );
        }
    }

    /// An arcaded porch: columns carrying round arches on every side but the one
    /// against the house, under a small tiled roof.
    fn loggia(&self, set: &mut MeshSet, part: &Part, a: &AreaPart, base: f64) {
        let r = a.rect;
        set.slab(
            &r,
            base,
            base + 0.12,
            FaceMats::with_top(Material::Stone, Material::Cotto),
        );
        let corners = [[r.x, r.z], [r.x1(), r.z], [r.x1(), r.z1()], [r.x, r.z1()]];
        let edges: Vec<(P2, P2)> = (0..4).map(|i| (corners[i], corners[(i + 1) % 4])).collect();
        let attached = self.centroids.get(&(part.group, part.level)).and_then(|c| {
            (0..4).min_by(|&i, &j| {
                let mid = |k: usize| {
                    [
                        (edges[k].0[0] + edges[k].1[0]) / 2.0,
                        (edges[k].0[1] + edges[k].1[1]) / 2.0,
                    ]
                };
                dist(mid(i), *c).total_cmp(&dist(mid(j), *c))
            })
        });
        let spring = base + 2.7;
        let max_bay = edges
            .iter()
            .map(|(p, q)| bay_width(dist(*p, *q)))
            .fold(0.0, f64::max);
        let top = (base + a.height).max(spring + max_bay / 2.0 + 0.4);
        for (i, &edge) in edges.iter().enumerate() {
            if Some(i) == attached {
                continue;
            }
            let prev_open = Some((i + 3) % 4) != attached;
            let next_open = Some((i + 1) % 4) != attached;
            arcade(set, edge, spring, top, base + 0.12, prev_open, next_open);
        }
        set.slab(&r, top, top + 0.22, FaceMats::all(Material::Stone));
        let eaves = r.grow(0.35);
        let rise = eaves.w.min(eaves.d) / 2.0 * (18f64).to_radians().tan();
        hip_roof(set, &eaves, top + 0.22, rise);
        eave_trim(set, &eaves, top + 0.1, top + 0.22);
    }

    fn prop(&self, set: &mut MeshSet, part: &Part, p: &PropPart) {
        let base = V3::ground(p.at, self.layout.base(part));
        let s = p.scale.max(0.05);
        let h = p.height.max(0.1);
        let seed = part.id;
        let jitter = |k: u64, axis: u64| hash3(v3(k as f64, axis as f64, 0.0), seed) - 0.5;
        match p.kind {
            PropKind::OliveTree => {
                let knee = base + v3(0.1 * s, h * 0.25, 0.05 * s);
                let crown = base + v3(0.2 * s, h * 0.5, -0.1 * s);
                set.cylinder(Material::Wood, base, knee, 0.17 * s, 7);
                set.cylinder(Material::Wood, knee, crown, 0.12 * s, 7);
                let centre = base + V3::UP * (h * 0.66);
                for k in 0..4 {
                    let offset = v3(
                        jitter(k, 0) * 1.1 * s,
                        jitter(k, 1) * h * 0.15,
                        jitter(k, 2) * 1.1 * s,
                    );
                    let size = 0.75 + 0.3 * (jitter(k, 3) + 0.5);
                    set.icosphere(
                        Material::OliveLeaf,
                        centre + offset,
                        v3(0.85 * s * size, h * 0.16 * size, 0.85 * s * size),
                        1,
                        0.35,
                        seed * 7 + k,
                    );
                }
            }
            PropKind::CypressTree => {
                set.cylinder(Material::Wood, base, base + V3::UP * 0.6, 0.12 * s, 6);
                let w = 0.5 * s;
                let height = h - 0.4;
                let profile: Vec<(f64, f64)> = [
                    (0.55, 0.0),
                    (0.95, 0.12),
                    (1.0, 0.3),
                    (0.85, 0.55),
                    (0.55, 0.78),
                    (0.25, 0.93),
                    (0.0, 1.0),
                ]
                .iter()
                .map(|&(r, y)| (r * w, y * height))
                .collect();
                set.lathe(
                    Material::CypressLeaf,
                    base + V3::UP * 0.4,
                    &profile,
                    8,
                    false,
                );
            }
            PropKind::LemonTree => {
                set.cylinder(Material::Wood, base, base + V3::UP * (h * 0.45), 0.1 * s, 6);
                let centre = base + V3::UP * (h * 0.66);
                let radii = v3(0.9 * s, h * 0.3, 0.9 * s);
                set.icosphere(Material::Foliage, centre, radii, 1, 0.3, seed);
                for k in 0..7 {
                    let d = v3(jitter(k, 0), jitter(k, 1) * 0.8, jitter(k, 2)).normalized();
                    let at = centre + v3(d.x * radii.x, d.y * radii.y, d.z * radii.z) * 0.98;
                    set.icosphere(
                        Material::Lemon,
                        at,
                        v3(0.08, 0.07, 0.08) * s.sqrt(),
                        0,
                        0.0,
                        seed + k,
                    );
                }
            }
            PropKind::Pot => {
                let ys = h / 0.6;
                let profile: Vec<(f64, f64)> = [
                    (0.18, 0.0),
                    (0.2, 0.04),
                    (0.26, 0.3),
                    (0.3, 0.5),
                    (0.3, 0.56),
                    (0.26, 0.6),
                ]
                .iter()
                .map(|&(r, y)| (r * s, y * ys))
                .collect();
                set.lathe(Material::Terracotta, base, &profile, 10, true);
                set.icosphere(
                    Material::Foliage,
                    base + V3::UP * (h + 0.12 * s),
                    v3(0.27 * s, 0.24 * s, 0.27 * s),
                    1,
                    0.3,
                    seed,
                );
            }
            PropKind::Fountain => {
                let k = s;
                let basin: Vec<(f64, f64)> = [
                    (0.9, 0.0),
                    (1.0, 0.05),
                    (1.0, 0.45),
                    (0.94, 0.5),
                    (0.86, 0.5),
                    (0.86, 0.12),
                    (0.0, 0.12),
                ]
                .iter()
                .map(|&(r, y)| (r * k, y * k))
                .collect();
                set.lathe(Material::Stone, base, &basin, 18, true);
                disc(
                    set,
                    Material::Water,
                    base + V3::UP * (0.38 * k),
                    0.86 * k,
                    18,
                );
                let column: Vec<(f64, f64)> = [
                    (0.12, 0.12),
                    (0.12, 0.85),
                    (0.38, 0.98),
                    (0.4, 1.05),
                    (0.1, 1.1),
                    (0.08, 1.4),
                    (0.0, 1.48),
                ]
                .iter()
                .map(|&(r, y)| (r * k, y * k))
                .collect();
                set.lathe(Material::Stone, base, &column, 12, true);
                disc(
                    set,
                    Material::Water,
                    base + V3::UP * (1.06 * k),
                    0.36 * k,
                    12,
                );
            }
            PropKind::Bench => {
                let frame = Frame::upright(base, [p.yaw.cos(), p.yaw.sin()]);
                let stone = FaceMats::all(Material::Stone);
                let y = base.y;
                set.cuboid(
                    &frame,
                    (-0.8 * s, 0.8 * s),
                    (y + 0.4, y + 0.48),
                    (-0.22, 0.22),
                    stone,
                );
                for u in [-0.6 * s, 0.6 * s] {
                    set.cuboid(
                        &frame,
                        (u - 0.09, u + 0.09),
                        (y, y + 0.4),
                        (-0.18, 0.18),
                        stone,
                    );
                }
            }
            PropKind::Column => column(set, base, h, 0.2 * s),
            PropKind::Well => {
                let rim: Vec<(f64, f64)> = [
                    (0.7, 0.0),
                    (0.7, 0.85),
                    (0.76, 0.9),
                    (0.76, 0.98),
                    (0.6, 0.98),
                    (0.6, 0.25),
                    (0.0, 0.25),
                ]
                .iter()
                .map(|&(r, y)| (r * s, y))
                .collect();
                set.lathe(Material::Stone, base, &rim, 14, true);
                disc(set, Material::Water, base + V3::UP * 0.6, 0.6 * s, 12);
                let left = base + v3(-0.65 * s, 0.95, 0.0);
                let right = base + v3(0.65 * s, 0.95, 0.0);
                set.cylinder(Material::Iron, left, left + V3::UP * (h - 1.35), 0.03, 5);
                set.cylinder(Material::Iron, right, right + V3::UP * (h - 1.35), 0.03, 5);
                let arc: Vec<V3> = (0..=8)
                    .map(|k| {
                        let a = PI * k as f64 / 8.0;
                        base + v3(-0.65 * s * a.cos(), h - 0.4 + 0.35 * a.sin(), 0.0)
                    })
                    .collect();
                for pair in arc.windows(2) {
                    set.cylinder(Material::Iron, pair[0], pair[1], 0.025, 4);
                }
                set.icosphere(
                    Material::Iron,
                    base + V3::UP * (h - 0.25),
                    v3(0.09, 0.09, 0.09),
                    0,
                    0.0,
                    seed,
                );
            }
            PropKind::Urn => {
                let stone = FaceMats::all(Material::Stone);
                let w = 0.25 * s;
                let ped = (h * 0.55).max(0.4);
                set.cuboid(
                    &Frame::WORLD,
                    (base.x - w, base.x + w),
                    (base.y, base.y + ped),
                    (base.z - w, base.z + w),
                    stone,
                );
                set.cuboid(
                    &Frame::WORLD,
                    (base.x - w - 0.04, base.x + w + 0.04),
                    (base.y + ped, base.y + ped + 0.06),
                    (base.z - w - 0.04, base.z + w + 0.04),
                    stone,
                );
                let urn = h - ped - 0.06;
                let profile: Vec<(f64, f64)> = [
                    (0.1, 0.0),
                    (0.16, 0.1),
                    (0.28, 0.35),
                    (0.3, 0.55),
                    (0.2, 0.78),
                    (0.25, 0.86),
                    (0.0, 1.0),
                ]
                .iter()
                .map(|&(r, y)| (r * s, y * urn))
                .collect();
                set.lathe(
                    Material::Stone,
                    base + V3::UP * (ped + 0.06),
                    &profile,
                    12,
                    true,
                );
            }
            PropKind::StonePine => {
                let crown = base + v3(0.3 * s, h * 0.78, 0.2 * s);
                set.cylinder(Material::Wood, base, crown, 0.22 * s, 7);
                let r = 2.4 * s;
                set.icosphere(
                    Material::CypressLeaf,
                    crown + V3::UP * 0.5,
                    v3(r, 0.9 * s.sqrt(), r * 0.9),
                    1,
                    0.25,
                    seed,
                );
                for k in 0..3 {
                    let off = v3(jitter(k, 0) * r, 0.2 + jitter(k, 1) * 0.4, jitter(k, 2) * r);
                    set.icosphere(
                        Material::CypressLeaf,
                        crown + off + V3::UP * 0.6,
                        v3(r * 0.55, 0.6, r * 0.5),
                        1,
                        0.3,
                        seed + k,
                    );
                }
            }
            PropKind::Palm => {
                let crown = base + v3(0.15 * s, h * 0.85, 0.0);
                let rings = 6;
                for k in 0..rings {
                    let a = base.lerp(crown, k as f64 / rings as f64);
                    let b = base.lerp(crown, (k + 1) as f64 / rings as f64);
                    set.cylinder(Material::Wood, a, b, 0.2 * s - 0.01 * k as f64, 6);
                }
                for k in 0..10 {
                    let angle = PI * 2.0 * k as f64 / 10.0 + jitter(k, 0);
                    let dir = v3(angle.cos(), 0.0, angle.sin());
                    let side = v3(-dir.z, 0.0, dir.x) * (0.22 * s);
                    let tip = crown + dir * (1.9 * s) - V3::UP * (0.7 + jitter(k, 1) * 0.4);
                    let mid = crown + dir * (1.0 * s) + V3::UP * 0.25;
                    for facing in [V3::UP, -V3::UP] {
                        set.polygon(
                            Material::Foliage,
                            &[crown, mid + side, tip, mid - side],
                            facing,
                        );
                    }
                }
            }
            PropKind::Lamp => {
                let y = base.y;
                let iron = FaceMats::all(Material::Iron);
                set.cuboid(
                    &Frame::WORLD,
                    (base.x - 0.15, base.x + 0.15),
                    (y, y + 0.08),
                    (base.z - 0.15, base.z + 0.15),
                    iron,
                );
                set.cylinder(Material::Iron, base, base + V3::UP * (h - 0.45), 0.05, 8);
                set.cuboid(
                    &Frame::WORLD,
                    (base.x - 0.12, base.x + 0.12),
                    (y + h - 0.45, y + h - 0.1),
                    (base.z - 0.12, base.z + 0.12),
                    FaceMats::all(Material::Glass),
                );
                set.cuboid(
                    &Frame::WORLD,
                    (base.x - 0.16, base.x + 0.16),
                    (y + h - 0.1, y + h - 0.04),
                    (base.z - 0.16, base.z + 0.16),
                    iron,
                );
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn wall_with_holes(
    set: &mut MeshSet,
    f: &Frame,
    start: f64,
    end: f64,
    y0: f64,
    y1: f64,
    half: f64,
    holes: &[Hole],
    mats: FaceMats,
) {
    let n = (-half, half);
    let mut cursor = start;
    for h in holes {
        if h.u0 > cursor {
            set.cuboid(f, (cursor, h.u0), (y0, y1), n, mats);
        }
        if h.v0 > y0 {
            set.cuboid(f, (h.u0, h.u1), (y0, h.v0), n, mats);
        }
        if y1 > h.v1 {
            set.cuboid(f, (h.u0, h.u1), (h.v1, y1), n, mats);
        }
        if h.arched {
            arch_spandrels(set, f, h, half, mats);
        }
        cursor = cursor.max(h.u1);
    }
    if end > cursor {
        set.cuboid(f, (cursor, end), (y0, y1), n, mats);
    }
}

/// Square stone piers with ball finials, and an iron gate between them.
#[allow(clippy::too_many_arguments)]
fn gate_mesh(
    set: &mut MeshSet,
    frame: &Frame,
    u0: f64,
    u1: f64,
    base: f64,
    height: f64,
    half: f64,
    seed: u64,
) {
    let stone = FaceMats::all(Material::Stone);
    let iron = FaceMats::all(Material::Iron);
    let pier = 0.55;
    let top = base + height + 0.35;
    for (a, b) in [(u0 - pier, u0), (u1, u1 + pier)] {
        set.cuboid(
            frame,
            (a, b),
            (base, top),
            (-half - 0.06, half + 0.06),
            stone,
        );
        set.cuboid(
            frame,
            (a - 0.05, b + 0.05),
            (top, top + 0.1),
            (-half - 0.11, half + 0.11),
            stone,
        );
        let c = frame.at((a + b) / 2.0, top + 0.3, 0.0);
        set.cylinder(
            Material::Stone,
            frame.at((a + b) / 2.0, top + 0.1, 0.0),
            c,
            0.07,
            6,
        );
        set.icosphere(Material::Stone, c, v3(0.2, 0.2, 0.2), 1, 0.0, seed);
    }
    let mut u = u0 + 0.06;
    while u < u1 - 0.05 {
        set.cuboid(
            frame,
            (u - 0.015, u + 0.015),
            (base + 0.06, base + height - 0.05),
            (-0.015, 0.015),
            iron,
        );
        u += 0.12;
    }
    for v in [base + 0.08, base + height * 0.5, base + height - 0.08] {
        set.cuboid(
            frame,
            (u0 + 0.02, u1 - 0.02),
            (v - 0.03, v + 0.03),
            (-0.025, 0.025),
            iron,
        );
    }
}

/// A projecting stone ledge over a window, crowned by a triangular pediment or a second ledge.
fn pediment(set: &mut MeshSet, f: &Frame, u0: f64, u1: f64, v: f64, half: f64, triangular: bool) {
    let stone = FaceMats::all(Material::Stone);
    set.cuboid(f, (u0, u1), (v, v + 0.09), (half, half + 0.13), stone);
    let mid = (u0 + u1) / 2.0;
    let v0 = v + 0.09;
    if triangular {
        let apex = v0 + (u1 - u0) * 0.22;
        let (front, back) = (half + 0.11, half);
        let (a, b) = (u0 + 0.04, u1 - 0.04);
        set.polygon(
            Material::Stone,
            &[
                f.at(a, v0, front),
                f.at(b, v0, front),
                f.at(mid, apex, front),
            ],
            f.n,
        );
        let up_left = f.v * (mid - a) - f.u * (apex - v0);
        let up_right = f.v * (b - mid) + f.u * (apex - v0);
        set.polygon(
            Material::Stone,
            &[
                f.at(a, v0, front),
                f.at(mid, apex, front),
                f.at(mid, apex, back),
                f.at(a, v0, back),
            ],
            up_left,
        );
        set.polygon(
            Material::Stone,
            &[
                f.at(mid, apex, front),
                f.at(b, v0, front),
                f.at(b, v0, back),
                f.at(mid, apex, back),
            ],
            up_right,
        );
    } else {
        set.cuboid(
            f,
            (u0 + 0.08, u1 - 0.08),
            (v0, v0 + 0.07),
            (half, half + 0.09),
            stone,
        );
    }
}

/// Box-hedge beds in four quarters around a gravel cross, with topiary balls.
fn parterre(set: &mut MeshSet, r: &Rect, base: f64, height: f64, seed: u64) {
    set.slab(r, base, base + 0.03, FaceMats::all(Material::Gravel));
    let hedge = FaceMats::all(Material::Foliage);
    let path = 1.2;
    let (cx, cz) = r.center();
    let beds = [
        Rect::new(r.x, r.z, cx - path / 2.0 - r.x, cz - path / 2.0 - r.z),
        Rect::new(
            cx + path / 2.0,
            r.z,
            r.x1() - cx - path / 2.0,
            cz - path / 2.0 - r.z,
        ),
        Rect::new(
            r.x,
            cz + path / 2.0,
            cx - path / 2.0 - r.x,
            r.z1() - cz - path / 2.0,
        ),
        Rect::new(
            cx + path / 2.0,
            cz + path / 2.0,
            r.x1() - cx - path / 2.0,
            r.z1() - cz - path / 2.0,
        ),
    ];
    for (k, bed) in beds.iter().enumerate() {
        if bed.w < 0.8 || bed.d < 0.8 {
            continue;
        }
        let t = 0.3;
        for band in [
            Rect::new(bed.x, bed.z, bed.w, t),
            Rect::new(bed.x, bed.z1() - t, bed.w, t),
            Rect::new(bed.x, bed.z + t, t, bed.d - 2.0 * t),
            Rect::new(bed.x1() - t, bed.z + t, t, bed.d - 2.0 * t),
        ] {
            set.slab(&band, base, base + height, hedge);
        }
        let inner = bed.grow(-t);
        if inner.w > 0.2 && inner.d > 0.2 {
            set.slab(&inner, base, base + 0.06, FaceMats::all(Material::Lawn));
        }
        let (bx, bz) = bed.center();
        set.icosphere(
            Material::Foliage,
            v3(bx, base + height + 0.35, bz),
            v3(0.4, 0.45, 0.4),
            1,
            0.1,
            seed + k as u64,
        );
    }
}

/// Rows of vines on wooden posts, following `ground`.
fn vines(set: &mut MeshSet, r: &Rect, height: f64, ground: &dyn Fn(P2) -> f64) {
    let mut z = r.z + 1.0;
    while z < r.z1() - 0.5 {
        let mut x = r.x + 0.5;
        while x < r.x1() - 0.5 {
            let x1 = (x + 4.0).min(r.x1() - 0.5);
            let (g0, g1) = (ground([x, z]), ground([x1, z]));
            let g = g0.min(g1);
            set.cylinder(
                Material::Wood,
                v3(x, g0 - 0.1, z),
                v3(x, g0 + height, z),
                0.04,
                4,
            );
            set.cuboid(
                &Frame::WORLD,
                (x, x1),
                (g + 0.55, g0.max(g1) + height),
                (z - 0.22, z + 0.22),
                FaceMats::all(Material::Foliage),
            );
            x = x1;
        }
        z += 2.2;
    }
}

/// A square belvedere tower: a plastered shaft with stone corners and an open loggia on top.
fn tower(set: &mut MeshSet, r: &Rect, base: f64, height: f64, pitch: f64) {
    let shaft_top = base + (height - 3.4).max(2.0);
    set.slab(
        r,
        base,
        shaft_top,
        FaceMats::with_top(Material::Plaster, Material::Cotto),
    );
    let stone = FaceMats::all(Material::Stone);
    let q = 0.35;
    let piers = [
        Rect::new(r.x - 0.03, r.z - 0.03, q, q),
        Rect::new(r.x1() - q + 0.03, r.z - 0.03, q, q),
        Rect::new(r.x - 0.03, r.z1() - q + 0.03, q, q),
        Rect::new(r.x1() - q + 0.03, r.z1() - q + 0.03, q, q),
    ];
    for p in &piers {
        set.slab(p, base, shaft_top + 2.7, stone);
    }
    set.slab(&r.grow(0.08), shaft_top - 0.15, shaft_top, stone);
    let roof_base = shaft_top + 2.7;
    set.slab(&r.grow(0.1), roof_base, roof_base + 0.2, stone);
    let eaves = r.grow(0.45);
    let rise = eaves.w.min(eaves.d) / 2.0 * pitch.to_radians().tan();
    hip_roof(set, &eaves, roof_base + 0.2, rise);
}

/// Fills the corners above a round arch and lines its curved soffit.
fn arch_spandrels(set: &mut MeshSet, f: &Frame, h: &Hole, half: f64, mats: FaceMats) {
    let r = h.radius();
    let c = h.centre();
    let spring = h.v1 - r;
    for (corner, from, to) in [(h.u0, PI, FRAC_PI_2), (h.u1, FRAC_PI_2, 0.0)] {
        let arc: Vec<(f64, f64)> = (0..=ARCH_SEGMENTS / 2)
            .map(|i| {
                let a = from + (to - from) * i as f64 / (ARCH_SEGMENTS / 2) as f64;
                (c + r * a.cos(), spring + r * a.sin())
            })
            .collect();
        for pair in arc.windows(2) {
            let ((pu, pv), (qu, qv)) = (pair[0], pair[1]);
            for (n, material, facing) in [(half, mats.front, f.n), (-half, mats.back, -f.n)] {
                let tri = [f.at(corner, h.v1, n), f.at(pu, pv, n), f.at(qu, qv, n)];
                let uvs = tri.map(|p| [p.dot(f.u), p.dot(f.v)]);
                set.polygon_uv(material, &tri, facing, &uvs);
            }
            let quad = [
                f.at(pu, pv, -half),
                f.at(pu, pv, half),
                f.at(qu, qv, half),
                f.at(qu, qv, -half),
            ];
            let facing = f.u * (c - (pu + qu) / 2.0) + f.v * (spring - (pv + qv) / 2.0);
            set.polygon(mats.sides, &quad, facing);
        }
    }
}

/// A half disc closing the top of an arched opening, facing out of the `n` side it sits on.
fn half_disc(
    set: &mut MeshSet,
    material: Material,
    f: &Frame,
    c: f64,
    spring: f64,
    r: f64,
    n: f64,
) {
    let points: Vec<V3> = (0..=ARCH_SEGMENTS)
        .map(|i| {
            let a = PI * i as f64 / ARCH_SEGMENTS as f64;
            f.at(c + r * a.cos(), spring + r * a.sin(), n)
        })
        .collect();
    let facing = if n >= 0.0 { f.n } else { -f.n };
    let uvs: Vec<[f64; 2]> = points.iter().map(|p| [p.dot(f.u), p.dot(f.v)]).collect();
    set.polygon_uv(material, &points, facing, &uvs);
}

/// The stone ring around an arch, standing proud of the wall between `n0` and `n1`.
#[allow(clippy::too_many_arguments)]
fn archivolt(
    set: &mut MeshSet,
    f: &Frame,
    c: f64,
    spring: f64,
    r: f64,
    width: f64,
    n0: f64,
    n1: f64,
) {
    let point =
        |a: f64, radius: f64, n: f64| f.at(c + radius * a.cos(), spring + radius * a.sin(), n);
    let centre = |n: f64| f.at(c, spring, n);
    for k in 0..ARCH_SEGMENTS {
        let (a0, a1) = (
            PI * k as f64 / ARCH_SEGMENTS as f64,
            PI * (k + 1) as f64 / ARCH_SEGMENTS as f64,
        );
        let (ro, ri) = (r + width, r);
        let front = [
            point(a0, ri, n1),
            point(a0, ro, n1),
            point(a1, ro, n1),
            point(a1, ri, n1),
        ];
        set.polygon(Material::Stone, &front, f.n);
        let outer = [
            point(a0, ro, n0),
            point(a1, ro, n0),
            point(a1, ro, n1),
            point(a0, ro, n1),
        ];
        let mid = (outer[0] + outer[2]) * 0.5;
        set.polygon(Material::Stone, &outer, mid - centre(n0 + (n1 - n0) / 2.0));
        let inner = [
            point(a0, ri, n0),
            point(a1, ri, n0),
            point(a1, ri, n1),
            point(a0, ri, n1),
        ];
        let mid = (inner[0] + inner[2]) * 0.5;
        set.polygon(Material::Stone, &inner, centre(n0 + (n1 - n0) / 2.0) - mid);
    }
}

fn rect_corners(r: &Rect, y: f64) -> [V3; 4] {
    [
        v3(r.x, y, r.z),
        v3(r.x1(), y, r.z),
        v3(r.x1(), y, r.z1()),
        v3(r.x, y, r.z1()),
    ]
}

fn disc(set: &mut MeshSet, material: Material, centre: V3, radius: f64, segments: usize) {
    let points: Vec<V3> = (0..segments)
        .map(|j| {
            let a = std::f64::consts::TAU * j as f64 / segments as f64;
            centre + v3(a.cos() * radius, 0.0, a.sin() * radius)
        })
        .collect();
    set.polygon(material, &points, V3::UP);
}

/// Four tiled slopes from the eave rectangle at height `y` up to a ridge `rise` higher,
/// with terracotta caps along the ridge and hips.
fn hip_roof(set: &mut MeshSet, e: &Rect, y: f64, rise: f64) {
    let ry = y + rise;
    // (eave start, eave end, upper end, upper start, outward)
    let (planes, ridge): (Vec<[V3; 4]>, (V3, V3)) = if e.w >= e.d {
        let half = e.d / 2.0;
        let rz = e.z + half;
        let (r0, r1) = (v3(e.x + half, ry, rz), v3(e.x1() - half, ry, rz));
        let (sw, se, ne, nw) = (
            v3(e.x, y, e.z),
            v3(e.x1(), y, e.z),
            v3(e.x1(), y, e.z1()),
            v3(e.x, y, e.z1()),
        );
        (
            vec![
                [sw, se, r1, r0],
                [ne, nw, r0, r1],
                [nw, sw, r0, r0],
                [se, ne, r1, r1],
            ],
            (r0, r1),
        )
    } else {
        let half = e.w / 2.0;
        let rx = e.x + half;
        let (r0, r1) = (v3(rx, ry, e.z + half), v3(rx, ry, e.z1() - half));
        let (sw, se, ne, nw) = (
            v3(e.x, y, e.z),
            v3(e.x1(), y, e.z),
            v3(e.x1(), y, e.z1()),
            v3(e.x, y, e.z1()),
        );
        (
            vec![
                [se, ne, r1, r0],
                [nw, sw, r0, r1],
                [sw, se, r0, r0],
                [ne, nw, r1, r1],
            ],
            (r0, r1),
        )
    };
    let centre = v3(e.x + e.w / 2.0, y, e.z + e.d / 2.0);
    for [a, b, c, d] in planes {
        let eave_dir = (b - a).normalized();
        let eave_mid = (a + b) * 0.5;
        let top_mid = (c + d) * 0.5;
        let slope_dir = (top_mid - eave_mid).normalized();
        let facing = (eave_mid - centre) + V3::UP * 0.5;
        let points: Vec<V3> = if (c - d).length() < 1e-6 {
            vec![a, b, c]
        } else {
            vec![a, b, c, d]
        };
        let uvs: Vec<[f64; 2]> = points
            .iter()
            .map(|&p| [(p - a).dot(eave_dir), (p - a).dot(slope_dir)])
            .collect();
        set.polygon_uv(Material::RoofTile, &points, facing, &uvs);
    }
    let lift = V3::UP * 0.03;
    if (ridge.1 - ridge.0).length() > 0.05 {
        set.cylinder(Material::RoofTile, ridge.0 + lift, ridge.1 + lift, 0.09, 6);
    }
    for (corner, end) in [
        (v3(e.x, y, e.z), ridge.0),
        (v3(e.x, y, e.z1()), ridge.0),
        (v3(e.x1(), y, e.z), ridge.1),
        (v3(e.x1(), y, e.z1()), ridge.1),
    ] {
        set.cylinder(Material::RoofTile, corner + lift, end + lift, 0.08, 6);
    }
}

/// A stone balustrade along a strip: a plinth, turned balusters and a handrail.
fn balustrade_band(set: &mut MeshSet, band: &Rect, y: f64, height: f64) {
    let stone = FaceMats::all(Material::Stone);
    set.slab(band, y, y + 0.14, stone);
    set.slab(&band.grow(0.02), y + height - 0.1, y + height, stone);
    let h = height - 0.24;
    let profile: Vec<(f64, f64)> = [
        (0.05, 0.0),
        (0.04, 0.15),
        (0.08, 0.42),
        (0.035, 0.8),
        (0.05, 1.0),
    ]
    .iter()
    .map(|&(r, v)| (r, v * h))
    .collect();
    let along_x = band.w >= band.d;
    let len = if along_x { band.w } else { band.d };
    let count = ((len - 0.2) / 0.3).floor().max(1.0) as usize;
    let (cx, cz) = band.center();
    for k in 0..=count {
        let t = 0.1 + (len - 0.2) * k as f64 / count as f64;
        let p = if along_x {
            v3(band.x + t, y + 0.14, cz)
        } else {
            v3(cx, y + 0.14, band.z + t)
        };
        set.lathe(Material::Stone, p, &profile, 5, true);
    }
}

/// The fascia board around the eaves and the soffit under them.
fn eave_trim(set: &mut MeshSet, e: &Rect, soffit: f64, y: f64) {
    let c = rect_corners(e, soffit);
    let up = V3::UP * (y - soffit);
    let centre = v3(e.x + e.w / 2.0, soffit, e.z + e.d / 2.0);
    for i in 0..4 {
        let (a, b) = (c[i], c[(i + 1) % 4]);
        set.polygon(
            Material::Wood,
            &[a, b, b + up, a + up],
            (a + b) * 0.5 - centre,
        );
    }
    set.polygon(Material::Wood, &c, -V3::UP);
}

/// Rafter ends under the eaves, from just past the cornice to the fascia.
fn rafters(set: &mut MeshSet, wall: &Rect, eaves: &Rect, soffit: f64, start: f64) {
    let wood = FaceMats::all(Material::Wood);
    let (y0, y1) = (soffit - 0.14, soffit);
    let spacing = 0.7;
    let along_x = ((wall.w - 0.4) / spacing).floor() as usize;
    for k in 0..=along_x {
        let x = wall.x + 0.2 + (wall.w - 0.4) * k as f64 / along_x.max(1) as f64;
        set.cuboid(
            &Frame::WORLD,
            (x - 0.05, x + 0.05),
            (y0, y1),
            (eaves.z + 0.05, wall.z - start),
            wood,
        );
        set.cuboid(
            &Frame::WORLD,
            (x - 0.05, x + 0.05),
            (y0, y1),
            (wall.z1() + start, eaves.z1() - 0.05),
            wood,
        );
    }
    let along_z = ((wall.d - 0.4) / spacing).floor() as usize;
    for k in 0..=along_z {
        let z = wall.z + 0.2 + (wall.d - 0.4) * k as f64 / along_z.max(1) as f64;
        set.cuboid(
            &Frame::WORLD,
            (eaves.x + 0.05, wall.x - start),
            (y0, y1),
            (z - 0.05, z + 0.05),
            wood,
        );
        set.cuboid(
            &Frame::WORLD,
            (wall.x1() + start, eaves.x1() - 0.05),
            (y0, y1),
            (z - 0.05, z + 0.05),
            wood,
        );
    }
}

const PIER: f64 = 0.42;
const ARCADE_DEPTH: f64 = 0.4;

fn bays(length: f64) -> usize {
    ((length / 3.0).round() as usize).max(1)
}

fn bay_width(length: f64) -> f64 {
    let n = bays(length);
    (length - PIER * (n + 1) as f64) / n as f64
}

/// One open side of a loggia: columns under round arches, from `edge.0` to `edge.1`.
fn arcade(
    set: &mut MeshSet,
    edge: (P2, P2),
    spring: f64,
    top: f64,
    floor: f64,
    prev_open: bool,
    next_open: bool,
) {
    let (a, b) = edge;
    let length = dist(a, b);
    let n = bays(length);
    let bay = bay_width(length);
    if bay < 0.6 {
        return;
    }
    let dir = [(b[0] - a[0]) / length, (b[1] - a[1]) / length];
    // Edges run counter-clockwise seen from above, so `n` points into the loggia;
    // the arcade's outer face lies on the edge.
    let mut frame = Frame::upright(V3::ground(a, 0.0), dir);
    frame.origin = frame.origin + frame.n * (ARCADE_DEPTH / 2.0);
    let holes: Vec<Hole> = (0..n)
        .map(|k| {
            let u0 = PIER * (k + 1) as f64 + bay * k as f64;
            Hole {
                u0,
                u1: u0 + bay,
                v0: spring,
                v1: spring + bay / 2.0,
                arched: true,
            }
        })
        .collect();
    let start = if prev_open { ARCADE_DEPTH } else { 0.0 };
    let mats = FaceMats::all(Material::Stone);
    wall_with_holes(
        set,
        &frame,
        start,
        length,
        spring,
        top,
        ARCADE_DEPTH / 2.0,
        &holes,
        mats,
    );
    // A column under every pier; the corner shared with the next open side is left to it.
    let last = if next_open { n } else { n + 1 };
    for k in 0..last {
        let u = PIER / 2.0 + (PIER + bay) * k as f64;
        column(set, frame.at(u, floor, 0.0), spring - floor, 0.17);
    }
}

/// A Tuscan column: square base, slightly tapering shaft, capital and abacus.
fn column(set: &mut MeshSet, base: V3, height: f64, radius: f64) {
    let stone = FaceMats::all(Material::Stone);
    let w = radius * 1.35;
    let y = base.y;
    set.cuboid(
        &Frame::WORLD,
        (base.x - w, base.x + w),
        (y, y + 0.16),
        (base.z - w, base.z + w),
        stone,
    );
    let shaft_h = (height - 0.16 - 0.28).max(0.2);
    let shaft = [
        (radius, 0.0),
        (radius * 0.97, shaft_h * 0.4),
        (radius * 0.85, shaft_h),
    ];
    set.lathe(Material::Stone, base + V3::UP * 0.16, &shaft, 12, true);
    let cap_base = base + V3::UP * (0.16 + shaft_h);
    set.lathe(
        Material::Stone,
        cap_base,
        &[
            (radius * 0.85, 0.0),
            (radius * 1.1, 0.1),
            (radius * 1.2, 0.16),
        ],
        12,
        true,
    );
    set.cuboid(
        &Frame::WORLD,
        (base.x - w, base.x + w),
        (cap_base.y + 0.16, y + height),
        (base.z - w, base.z + w),
        stone,
    );
}
