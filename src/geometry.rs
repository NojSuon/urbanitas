//! Axis-aligned geometry on the ground plane.
//!
//! `x` runs west → east and `z` runs south → north. The building's front
//! (street) side is south, at the lowest `z`. Heights are `y` and live in the
//! exporters, not here.

use serde::{Deserialize, Serialize};

pub const EPS: f64 = 1e-6;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f64,
    pub z: f64,
    pub w: f64,
    pub d: f64,
}

impl Rect {
    pub const fn new(x: f64, z: f64, w: f64, d: f64) -> Self {
        Self { x, z, w, d }
    }

    pub fn x1(&self) -> f64 {
        self.x + self.w
    }

    pub fn z1(&self) -> f64 {
        self.z + self.d
    }

    pub fn area(&self) -> f64 {
        self.w * self.d
    }

    pub fn center(&self) -> (f64, f64) {
        (self.x + self.w / 2.0, self.z + self.d / 2.0)
    }

    pub fn min_side(&self) -> f64 {
        self.w.min(self.d)
    }

    /// Grows each side by its own margin (negative margins shrink).
    pub fn expand(&self, west: f64, south: f64, east: f64, north: f64) -> Rect {
        Rect::new(
            self.x - west,
            self.z - south,
            self.w + west + east,
            self.d + south + north,
        )
    }

    pub fn grow(&self, margin: f64) -> Rect {
        self.expand(margin, margin, margin, margin)
    }

    /// True when the interiors overlap (touching edges do not count).
    pub fn intersects(&self, other: &Rect) -> bool {
        self.x < other.x1() - EPS
            && other.x < self.x1() - EPS
            && self.z < other.z1() - EPS
            && other.z < self.z1() - EPS
    }

    pub fn contains(&self, other: &Rect) -> bool {
        other.x >= self.x - EPS
            && other.z >= self.z - EPS
            && other.x1() <= self.x1() + EPS
            && other.z1() <= self.z1() + EPS
    }

    pub fn intersection(&self, other: &Rect) -> Option<Rect> {
        let (x0, z0) = (self.x.max(other.x), self.z.max(other.z));
        let (x1, z1) = (self.x1().min(other.x1()), self.z1().min(other.z1()));
        (x1 - x0 > EPS && z1 - z0 > EPS).then(|| Rect::new(x0, z0, x1 - x0, z1 - z0))
    }

    /// The parts of `self` left after cutting out `hole`, which must lie inside `self`.
    pub fn subtract(&self, hole: &Rect) -> Vec<Rect> {
        [
            Rect::new(self.x, self.z, hole.x - self.x, self.d),
            Rect::new(hole.x1(), self.z, self.x1() - hole.x1(), self.d),
            Rect::new(hole.x, self.z, hole.w, hole.z - self.z),
            Rect::new(hole.x, hole.z1(), hole.w, self.z1() - hole.z1()),
        ]
        .into_iter()
        .filter(|r| r.w > EPS && r.d > EPS)
        .collect()
    }

    /// The wall segment two touching rectangles share, if any.
    pub fn shared_wall(&self, other: &Rect) -> Option<Segment> {
        let vertical = if (self.x1() - other.x).abs() < EPS {
            Some(self.x1())
        } else if (other.x1() - self.x).abs() < EPS {
            Some(self.x)
        } else {
            None
        };
        if let Some(x) = vertical {
            let (start, end) = (self.z.max(other.z), self.z1().min(other.z1()));
            if end - start > EPS {
                return Some(Segment {
                    axis: Axis::Z,
                    coord: x,
                    start,
                    end,
                });
            }
        }
        let horizontal = if (self.z1() - other.z).abs() < EPS {
            Some(self.z1())
        } else if (other.z1() - self.z).abs() < EPS {
            Some(self.z)
        } else {
            None
        };
        if let Some(z) = horizontal {
            let (start, end) = (self.x.max(other.x), self.x1().min(other.x1()));
            if end - start > EPS {
                return Some(Segment {
                    axis: Axis::X,
                    coord: z,
                    start,
                    end,
                });
            }
        }
        None
    }

    /// Where this rectangle runs along one side of `outer`, as a range along that side.
    pub fn span_on_side(&self, side: Side, outer: &Rect) -> Option<(f64, f64)> {
        let touches = match side {
            Side::South => (self.z - outer.z).abs() < EPS,
            Side::North => (self.z1() - outer.z1()).abs() < EPS,
            Side::West => (self.x - outer.x).abs() < EPS,
            Side::East => (self.x1() - outer.x1()).abs() < EPS,
        };
        touches.then(|| match side.axis() {
            Axis::X => (self.x, self.x1()),
            Axis::Z => (self.z, self.z1()),
        })
    }
}

/// The direction a wall or segment runs in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Axis {
    /// Runs along `x` at a fixed `z`.
    X,
    /// Runs along `z` at a fixed `x`.
    Z,
}

/// A straight wall line: `start..end` along `axis`, at `coord` on the other axis.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Segment {
    pub axis: Axis,
    pub coord: f64,
    pub start: f64,
    pub end: f64,
}

impl Segment {
    pub fn len(&self) -> f64 {
        self.end - self.start
    }

    pub fn is_empty(&self) -> bool {
        self.len() <= EPS
    }

    pub fn mid(&self) -> f64 {
        (self.start + self.end) / 2.0
    }

    /// The part of this segment inside `outer` once its walls, `thickness` thick, are taken off.
    pub fn inside_walls(&self, outer: &Rect, thickness: f64) -> Segment {
        let (lo, hi) = match self.axis {
            Axis::X => (outer.x + thickness, outer.x1() - thickness),
            Axis::Z => (outer.z + thickness, outer.z1() - thickness),
        };
        Segment {
            start: self.start.max(lo),
            end: self.end.min(hi),
            ..*self
        }
    }
}

/// A straight piece of a building's outline, with the outside on one side of it.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Edge {
    pub seg: Segment,
    /// +1 when the outside lies towards increasing coordinates on the other axis.
    pub out: f64,
}

impl Edge {
    /// Unit vector pointing out of the building, as `[x, z]`.
    pub fn outward(&self) -> [f64; 2] {
        match self.seg.axis {
            Axis::X => [0.0, self.out],
            Axis::Z => [self.out, 0.0],
        }
    }

    /// The point `along` this edge, on the outline itself.
    pub fn point(&self, along: f64) -> [f64; 2] {
        match self.seg.axis {
            Axis::X => [along, self.seg.coord],
            Axis::Z => [self.seg.coord, along],
        }
    }

    /// Facing the street (south).
    pub fn is_street(&self) -> bool {
        self.seg.axis == Axis::X && self.out < 0.0
    }

    /// Where `rect` runs along this edge, if its face lies on it.
    pub fn span(&self, rect: &Rect) -> Option<(f64, f64)> {
        let (face, lo, hi) = match (self.seg.axis, self.out > 0.0) {
            (Axis::X, false) => (rect.z, rect.x, rect.x1()),
            (Axis::X, true) => (rect.z1(), rect.x, rect.x1()),
            (Axis::Z, false) => (rect.x, rect.z, rect.z1()),
            (Axis::Z, true) => (rect.x1(), rect.z, rect.z1()),
        };
        if (face - self.seg.coord).abs() > EPS {
            return None;
        }
        let (a, b) = (lo.max(self.seg.start), hi.min(self.seg.end));
        (b - a > EPS).then_some((a, b))
    }
}

/// The outline of the union of `rects`: straight edges with their outsides, including
/// the inner ring of a courtyard. Collinear touching pieces are merged.
pub fn outline(rects: &[Rect]) -> Vec<Edge> {
    let coords = |f: &dyn Fn(&Rect) -> [f64; 2]| {
        let mut v: Vec<f64> = rects.iter().flat_map(f).collect();
        v.sort_by(f64::total_cmp);
        v.dedup_by(|a, b| (*a - *b).abs() < EPS);
        v
    };
    let xs = coords(&|r| [r.x, r.x1()]);
    let zs = coords(&|r| [r.z, r.z1()]);
    if xs.len() < 2 || zs.len() < 2 {
        return Vec::new();
    }
    let (nx, nz) = (xs.len() - 1, zs.len() - 1);
    let filled = |i: isize, j: isize| {
        if i < 0 || j < 0 || i as usize >= nx || j as usize >= nz {
            return false;
        }
        let (cx, cz) = (
            (xs[i as usize] + xs[i as usize + 1]) / 2.0,
            (zs[j as usize] + zs[j as usize + 1]) / 2.0,
        );
        rects
            .iter()
            .any(|r| cx > r.x && cx < r.x1() && cz > r.z && cz < r.z1())
    };
    let mut pieces: Vec<Edge> = Vec::new();
    for i in 0..nx as isize {
        for j in 0..nz as isize {
            if !filled(i, j) {
                continue;
            }
            let (x0, x1, z0, z1) = (
                xs[i as usize],
                xs[i as usize + 1],
                zs[j as usize],
                zs[j as usize + 1],
            );
            let mut push = |axis, coord, start, end, out| {
                pieces.push(Edge {
                    seg: Segment {
                        axis,
                        coord,
                        start,
                        end,
                    },
                    out,
                })
            };
            if !filled(i, j - 1) {
                push(Axis::X, z0, x0, x1, -1.0);
            }
            if !filled(i, j + 1) {
                push(Axis::X, z1, x0, x1, 1.0);
            }
            if !filled(i - 1, j) {
                push(Axis::Z, x0, z0, z1, -1.0);
            }
            if !filled(i + 1, j) {
                push(Axis::Z, x1, z0, z1, 1.0);
            }
        }
    }
    pieces.sort_by(|a, b| {
        (a.seg.axis as u8, a.out > 0.0)
            .cmp(&(b.seg.axis as u8, b.out > 0.0))
            .then(a.seg.coord.total_cmp(&b.seg.coord))
            .then(a.seg.start.total_cmp(&b.seg.start))
    });
    let mut merged: Vec<Edge> = Vec::new();
    for piece in pieces {
        if let Some(last) = merged.last_mut() {
            if last.seg.axis == piece.seg.axis
                && last.out == piece.out
                && (last.seg.coord - piece.seg.coord).abs() < EPS
                && (last.seg.end - piece.seg.start).abs() < EPS
            {
                last.seg.end = piece.seg.end;
                continue;
            }
        }
        merged.push(piece);
    }
    merged
}

/// The edge of `outline` perpendicular to `edge` that meets it at `along`, if any.
pub fn corner_neighbour(outline: &[Edge], edge: &Edge, along: f64) -> Option<Edge> {
    outline.iter().copied().find(|e| {
        e.seg.axis != edge.seg.axis
            && (e.seg.coord - along).abs() < EPS
            && ((e.seg.start - edge.seg.coord).abs() < EPS
                || (e.seg.end - edge.seg.coord).abs() < EPS)
    })
}

/// A partition shortened so it stops at the inner face of the facade walls it runs into.
pub fn clip_to_outline(seg: &Segment, outline: &[Edge], thickness: f64) -> Segment {
    let meets = |at: f64, out: f64| {
        outline.iter().any(|e| {
            e.seg.axis != seg.axis
                && e.out == out
                && (e.seg.coord - at).abs() < EPS
                && seg.coord > e.seg.start - EPS
                && seg.coord < e.seg.end + EPS
        })
    };
    let mut clipped = *seg;
    if meets(seg.start, -1.0) {
        clipped.start += thickness;
    }
    if meets(seg.end, 1.0) {
        clipped.end -= thickness;
    }
    clipped
}

/// A side of the building footprint. South is the street front.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    South,
    North,
    West,
    East,
}

impl Side {
    pub const ALL: [Side; 4] = [Side::South, Side::North, Side::West, Side::East];

    pub fn axis(self) -> Axis {
        match self {
            Side::South | Side::North => Axis::X,
            Side::West | Side::East => Axis::Z,
        }
    }

    /// +1 when the outside of this side lies towards increasing coordinates.
    pub fn outward(self) -> f64 {
        match self {
            Side::South | Side::West => -1.0,
            Side::North | Side::East => 1.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_wall_finds_overlap() {
        let a = Rect::new(0.0, 0.0, 4.0, 3.0);
        let b = Rect::new(4.0, 1.0, 2.0, 5.0);
        let seg = a.shared_wall(&b).unwrap();
        assert_eq!(seg.axis, Axis::Z);
        assert!((seg.coord - 4.0).abs() < EPS);
        assert!((seg.start - 1.0).abs() < EPS && (seg.end - 3.0).abs() < EPS);
        assert!(a.shared_wall(&Rect::new(5.0, 0.0, 1.0, 1.0)).is_none());
    }

    #[test]
    fn outline_of_an_l_and_a_courtyard() {
        let l = [
            Rect::new(0.0, 0.0, 10.0, 5.0),
            Rect::new(0.0, 5.0, 4.0, 5.0),
        ];
        let edges = outline(&l);
        assert_eq!(edges.len(), 6, "an L has six sides: {edges:?}");
        let perimeter: f64 = edges.iter().map(|e| e.seg.len()).sum();
        assert!((perimeter - 40.0).abs() < 1e-9);

        let ring = [
            Rect::new(0.0, 0.0, 12.0, 4.0),
            Rect::new(0.0, 8.0, 12.0, 4.0),
            Rect::new(0.0, 4.0, 4.0, 4.0),
            Rect::new(8.0, 4.0, 4.0, 4.0),
        ];
        let edges = outline(&ring);
        assert_eq!(edges.len(), 8, "outer square and inner courtyard");
        let inner: Vec<&Edge> = edges.iter().filter(|e| e.seg.len() < 5.0).collect();
        assert_eq!(inner.len(), 4);
        // The courtyard's south side faces into the courtyard (north, +z).
        assert!(
            inner
                .iter()
                .any(|e| e.seg.axis == Axis::X && (e.seg.coord - 4.0).abs() < EPS && e.out > 0.0)
        );
    }

    #[test]
    fn partitions_stop_at_the_facade() {
        let edges = outline(&[Rect::new(0.0, 0.0, 10.0, 8.0)]);
        let seg = Segment {
            axis: Axis::Z,
            coord: 4.0,
            start: 0.0,
            end: 8.0,
        };
        let clipped = clip_to_outline(&seg, &edges, 0.4);
        assert!((clipped.start - 0.4).abs() < EPS && (clipped.end - 7.6).abs() < EPS);
    }

    #[test]
    fn subtract_keeps_area() {
        let outer = Rect::new(0.0, 0.0, 10.0, 8.0);
        let hole = Rect::new(0.0, 3.0, 2.6, 5.0);
        let parts = outer.subtract(&hole);
        let area: f64 = parts.iter().map(Rect::area).sum();
        assert!((area + hole.area() - outer.area()).abs() < 1e-9);
        assert!(parts.iter().all(|p| !p.intersects(&hole)));
    }
}
