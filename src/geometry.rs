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
    fn subtract_keeps_area() {
        let outer = Rect::new(0.0, 0.0, 10.0, 8.0);
        let hole = Rect::new(0.0, 3.0, 2.6, 5.0);
        let parts = outer.subtract(&hole);
        let area: f64 = parts.iter().map(Rect::area).sum();
        assert!((area + hole.area() - outer.area()).abs() < 1e-9);
        assert!(parts.iter().all(|p| !p.intersects(&hole)));
    }
}
