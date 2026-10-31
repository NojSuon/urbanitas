//! House designs: a house saved as a reusable module.
//!
//! A [`Design`] holds one house's parts relative to its plot, facing south and standing
//! at height zero, with its colours. [`Design::capture`] saves a house from a layout;
//! [`Design::place`] puts a copy anywhere, turned to face any way, and remembers which
//! design it came from, so [`update_copies`] can push later changes to every copy.

use serde::{Deserialize, Serialize};

use crate::Building;
use crate::geometry::Rect;
use crate::layout::append_building;
use crate::parts::{AreaKind, Layout, Module, P2, Part, PartKind};
use crate::style::Palette;

pub const DESIGN_FORMAT: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Design {
    pub format: u32,
    /// A stable name for the design, linking copies back to it (empty for one-off houses).
    pub id: String,
    pub name: String,
    pub palette: Palette,
    /// The plot, garden included, with its south-west corner at the origin.
    pub lot: Rect,
    /// The house's parts, relative to the plot's corner and to the ground it stands on.
    pub parts: Vec<Part>,
}

/// A file-name-friendly version of `name`: lower case, words joined by dashes.
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_string()
}

/// The plot `lot` (designed facing south) turned `turns` quarter turns and centred on `centre`.
fn turned_lot(lot: &Rect, turns: u8, centre: P2) -> Rect {
    let (w, d) = if turns % 2 == 1 {
        (lot.d, lot.w)
    } else {
        (lot.w, lot.d)
    };
    Rect::new(centre[0] - w / 2.0, centre[1] - d / 2.0, w, d)
}

impl Design {
    /// Saves building `group` as a design.
    pub fn capture(layout: &Layout, group: u32, id: &str, name: &str) -> Option<Design> {
        let module = layout.module(group).cloned().unwrap_or_default();
        let world = module.lot.or_else(|| layout.group_bounds(group))?;
        // The stone plinth a town lays under a whole plot belongs to the site, not the house.
        let plinth = |p: &Part| {
            matches!(&p.kind, PartKind::Area(a) if a.kind == AreaKind::Platform
                && a.rect.intersection(&world).is_some_and(|i| i.area() >= world.area() * 0.8))
        };
        let parts: Vec<&Part> = layout.group_parts(group).filter(|p| !plinth(p)).collect();
        if parts.is_empty() {
            return None;
        }
        let turns = module.turns % 4;
        // The plot as designed, before it was turned.
        let lot = turned_lot(&world, turns, [0.0, 0.0]);
        let lot = Rect::new(0.0, 0.0, lot.w, lot.d);
        let base = parts
            .iter()
            .filter(|p| p.level == 0)
            .map(|p| p.ground)
            .fold(f64::MAX, f64::min);
        let base = if base == f64::MAX { 0.0 } else { base };
        let (cx, cz) = world.center();
        let parts = parts
            .into_iter()
            .enumerate()
            .map(|(k, part)| {
                let mut part = part.clone();
                part.translate([-cx, -cz]);
                for _ in 0..(4 - turns) % 4 {
                    part.rotate_quarter_about([0.0, 0.0]);
                }
                part.translate([lot.w / 2.0, lot.d / 2.0]);
                part.ground -= base;
                part.group = None;
                part.id = k as u64 + 1;
                part
            })
            .collect();
        Some(Design {
            format: DESIGN_FORMAT,
            id: id.to_string(),
            name: name.to_string(),
            palette: layout.palette_of(Some(group)).clone(),
            lot,
            parts,
        })
    }

    /// A one-off design from a freshly generated building, on its plot.
    pub fn from_building(b: &Building) -> Design {
        let mut scratch = Layout::new("scratch", b.style.clone(), b.palette.clone());
        let group = scratch.new_group(b.palette.clone());
        append_building(&mut scratch, b, [0.0, 0.0], 0.0, Some(group));
        let lot = b
            .garden
            .as_ref()
            .map_or(Rect::new(0.0, 0.0, b.footprint.w, b.footprint.d), |g| g.lot);
        scratch.module_mut(group).lot = Some(lot);
        Design::capture(&scratch, group, "", &b.name).expect("a building has parts")
    }

    /// Places a copy centred on `centre`, turned `turns` quarter turns clockwise (a house
    /// designed facing south then faces west), standing at height `ground`. With `link`, the
    /// copy remembers this design so it can be updated later. Returns the new group.
    pub fn place(
        &self,
        layout: &mut Layout,
        centre: P2,
        turns: u8,
        ground: f64,
        link: bool,
    ) -> u32 {
        let group = layout.new_group(self.palette.clone());
        self.place_in(layout, group, centre, turns, ground);
        if link && !self.id.is_empty() {
            layout.module_mut(group).design = Some(self.id.clone());
        }
        group
    }

    /// Adds the design's parts to an existing (empty) group.
    fn place_in(&self, layout: &mut Layout, group: u32, centre: P2, turns: u8, ground: f64) {
        let turns = turns % 4;
        for part in &self.parts {
            let mut part = part.clone();
            part.translate([-self.lot.w / 2.0, -self.lot.d / 2.0]);
            for _ in 0..turns {
                part.rotate_quarter_about([0.0, 0.0]);
            }
            part.translate(centre);
            layout.add_in(part.level, part.ground + ground, Some(group), part.kind);
        }
        let module = layout.module_mut(group);
        module.name = self.name.clone();
        module.turns = turns;
        module.lot = Some(turned_lot(&self.lot, turns, centre));
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("a design always serialises to JSON")
    }

    pub fn from_json(text: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(text)
    }
}

/// Where building `group` stands: its plot's centre, its turns and its ground height.
fn placement(layout: &Layout, group: u32) -> Option<(P2, u8, f64)> {
    let module = layout.module(group).cloned().unwrap_or_default();
    let lot = module.lot.or_else(|| layout.group_bounds(group))?;
    let ground = layout
        .group_parts(group)
        .filter(|p| p.level == 0)
        .map(|p| p.ground)
        .fold(f64::MAX, f64::min);
    let (cx, cz) = lot.center();
    Some((
        [cx, cz],
        module.turns,
        if ground == f64::MAX { 0.0 } else { ground },
    ))
}

/// Rebuilds every copy of `design` from it, keeping each copy where it stands and the way it
/// faces. Returns how many copies were updated.
pub fn update_copies(layout: &mut Layout, design: &Design) -> usize {
    let copies: Vec<u32> = layout
        .modules
        .iter()
        .enumerate()
        .filter(|(_, m)| m.design.as_deref() == Some(design.id.as_str()) && !design.id.is_empty())
        .map(|(g, _)| g as u32)
        .collect();
    let mut updated = 0;
    for group in copies {
        let Some((centre, turns, ground)) = placement(layout, group) else {
            continue;
        };
        layout.remove_group(group);
        if let Some(palette) = layout.palettes.get_mut(group as usize) {
            *palette = design.palette.clone();
        }
        design.place_in(layout, group, centre, turns, ground);
        updated += 1;
    }
    updated
}

/// Turns building `group` into a one-off: it no longer follows its design.
pub fn unlink(layout: &mut Layout, group: u32) {
    layout.module_mut(group).design = None;
}

impl Module {
    /// Whether this house is a copy of a saved design.
    pub fn is_copy(&self) -> bool {
        self.design.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BuildingParams, Garden, generate};

    fn sample() -> Design {
        let b = generate(&BuildingParams {
            garden: Garden::Back,
            seed: 9,
            ..BuildingParams::default()
        })
        .unwrap();
        let mut d = Design::from_building(&b);
        d.id = "casa-nove".into();
        d.name = "Casa nove".into();
        d
    }

    fn empty() -> Layout {
        let b = sample();
        let style = crate::ArchitectureStyle::Mediterranean;
        Layout::new("t", crate::style::StyleProfile::for_style(style), b.palette)
    }

    #[test]
    fn slugs_are_tidy() {
        assert_eq!(slug("  Casa del Sole! (2) "), "casa-del-sole-2");
    }

    #[test]
    fn designs_round_trip_through_json() {
        let d = sample();
        assert_eq!(Design::from_json(&d.to_json()).unwrap(), d);
        assert!(d.parts.iter().all(|p| p.group.is_none()));
        // The plot starts at the origin and holds every part.
        assert_eq!((d.lot.x, d.lot.z), (0.0, 0.0));
        for p in &d.parts {
            let f = p.footprint();
            assert!(f.x >= -1.5 && f.x1() <= d.lot.w + 1.5, "{:?}", p.kind);
        }
    }

    #[test]
    fn a_placed_copy_matches_its_design_turned_and_moved() {
        let d = sample();
        for turns in 0..4 {
            let mut layout = empty();
            let g = d.place(&mut layout, [40.0, -12.0], turns, 3.5, true);
            let module = layout.module(g).unwrap().clone();
            assert_eq!(module.design.as_deref(), Some("casa-nove"));
            assert_eq!(module.turns, turns);
            let lot = module.lot.unwrap();
            assert!((lot.center().0 - 40.0).abs() < 1e-9 && (lot.center().1 + 12.0).abs() < 1e-9);
            if turns % 2 == 1 {
                assert!((lot.w - d.lot.d).abs() < 1e-9);
            }
            // Saving the copy gives back the design.
            let again = Design::capture(&layout, g, &d.id, &d.name).unwrap();
            assert_eq!(again.parts.len(), d.parts.len());
            for (a, b) in again.parts.iter().zip(&d.parts) {
                assert_eq!(a.level, b.level);
                assert!((a.ground - b.ground).abs() < 1e-9);
                let (fa, fb) = (a.footprint(), b.footprint());
                assert!(
                    (fa.x - fb.x).abs() < 1e-6
                        && (fa.z - fb.z).abs() < 1e-6
                        && (fa.w - fb.w).abs() < 1e-6,
                    "turns {turns}: {:?} vs {:?}",
                    fa,
                    fb
                );
            }
        }
    }

    #[test]
    fn updating_reaches_every_copy_and_nothing_else() {
        let d = sample();
        let mut layout = empty();
        let a = d.place(&mut layout, [0.0, 0.0], 0, 0.0, true);
        let b = d.place(&mut layout, [60.0, 0.0], 1, 2.0, true);
        let loose = d.place(&mut layout, [0.0, 60.0], 2, 0.0, false);
        let before_loose: Vec<Part> = layout.group_parts(loose).cloned().collect();
        // Edit the design: drop its roofs.
        let mut flat = d.clone();
        flat.parts.retain(|p| !matches!(p.kind, PartKind::Roof(_)));
        flat.name = "Casa piatta".into();
        assert_eq!(update_copies(&mut layout, &flat), 2);
        for g in [a, b] {
            assert!(
                layout
                    .group_parts(g)
                    .all(|p| !matches!(p.kind, PartKind::Roof(_)))
            );
            assert_eq!(layout.group_parts(g).count(), flat.parts.len());
            assert_eq!(layout.module(g).unwrap().name, "Casa piatta");
        }
        // The copy kept its place, turn and height.
        assert_eq!(layout.module(b).unwrap().turns, 1);
        assert!(
            layout
                .group_parts(b)
                .filter(|p| p.level == 0)
                .all(|p| p.ground >= 2.0 - 1e-9)
        );
        let after_loose: Vec<Part> = layout.group_parts(loose).cloned().collect();
        assert_eq!(before_loose, after_loose, "an unlinked copy is left alone");
    }
}
