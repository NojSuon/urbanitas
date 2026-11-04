//! The open project: what it is generated from (a landscape with a town, or a site preset),
//! the editable layout, the design library, undo history and what needs redrawing.

use std::collections::BTreeSet;

use bevy::prelude::*;
use urbanitas::design::Design;
use urbanitas::mesh::build::WallMode;
use urbanitas::parts::{Layout, P2, PartId};
use urbanitas::style::{Palette, StyleProfile};
use urbanitas::{
    LandscapeParams, SiteParams, TownParams, generate_site, grow_town, landscape_layout,
};

const HISTORY_LIMIT: usize = 60;

/// What the layout is generated from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// A landscape, with a town grown round the piazza once one is placed.
    Landscape,
    /// One of the site presets (a single building, a street, villas, a farm).
    Site,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Quality {
    /// No shadows or MSAA, 256 px textures, a coarser terrain mesh: for low-end PCs.
    Low,
    High,
}

impl Quality {
    pub fn texture_size(self) -> u32 {
        match self {
            Quality::Low => 256,
            Quality::High => 512,
        }
    }

    /// Terrain grid points skipped between mesh vertices.
    pub fn terrain_stride(self) -> usize {
        match self {
            Quality::Low => 2,
            Quality::High => 1,
        }
    }
}

#[derive(Resource)]
pub struct Project {
    pub source: Source,
    pub site: SiteParams,
    pub landscape: LandscapeParams,
    pub town: TownParams,
    /// Where the town's piazza is; no town until one is placed.
    pub plaza: Option<P2>,
    /// Clicking the map places the piazza.
    pub placing_piazza: bool,
    /// Saved house designs (the `designs/` folder).
    pub designs: Vec<Design>,
    pub layout: Layout,
    pub error: Option<String>,
    /// The level being edited; higher levels are hidden while it is above the ground.
    pub level: u32,
    pub walls: WallMode,
    pub quality: Quality,
    pub selected: Option<PartId>,
    /// A whole building selected (Shift+click).
    pub selected_group: Option<u32>,
    /// A house opened for editing on its own: the others are hidden and can't be picked.
    pub open_group: Option<u32>,
    pub status: String,
    /// When a parameter change should regenerate (debounced).
    pub regenerate_at: Option<f64>,
    /// Bumped on every change, so pictures of the selection can refresh.
    pub revision: u64,
    dirty_all: bool,
    dirty: BTreeSet<u32>,
    terrain_dirty: bool,
    history: Vec<Layout>,
    future: Vec<Layout>,
    /// The last coalescing key passed to [`Project::edit`], so slider drags make one undo step.
    last_edit: Option<String>,
}

impl Project {
    pub fn new(
        source: Source,
        site: SiteParams,
        landscape: LandscapeParams,
        town: TownParams,
        quality: Quality,
    ) -> Self {
        let style = site.building.style;
        let mut project = Self {
            layout: Layout::new(
                "urbanitas",
                StyleProfile::for_style(style),
                Palette::choose(style, &mut urbanitas::rng::Rng::new(0)),
            ),
            source,
            site,
            landscape,
            town,
            plaza: None,
            placing_piazza: false,
            designs: crate::files::list_designs(),
            error: None,
            level: 0,
            walls: WallMode::Up,
            quality,
            selected: None,
            selected_group: None,
            open_group: None,
            status: "Welcome to Urbanitas".into(),
            regenerate_at: None,
            revision: 0,
            dirty_all: true,
            dirty: BTreeSet::new(),
            terrain_dirty: true,
            history: Vec::new(),
            future: Vec::new(),
            last_edit: None,
        };
        project.regenerate();
        project
    }

    /// The layout the current parameters generate.
    fn generated(&self) -> Result<(Layout, String), String> {
        match self.source {
            Source::Site => generate_site(&self.site)
                .map(|l| {
                    let note = l.description.clone();
                    (l, note)
                })
                .map_err(|e| e.to_string()),
            Source::Landscape => {
                let mut layout = landscape_layout(&self.landscape, self.town.style);
                let note = match self.plaza {
                    Some(plaza) => {
                        let report = grow_town(&mut layout, plaza, &self.town, &self.designs)
                            .map_err(|e| e.to_string())?;
                        layout.name = format!("town_{}", self.town.seed);
                        format!(
                            "Town grown: {} houses on {} streets",
                            report.houses, report.streets
                        )
                    }
                    None => format!(
                        "{} · {} · open the Town panel to place a piazza",
                        self.landscape.relief.label(),
                        self.landscape.water.label()
                    ),
                };
                Ok((layout, note))
            }
        }
    }

    /// Rebuilds the layout from the current parameters. Undoable.
    pub fn regenerate(&mut self) {
        match self.generated() {
            Ok((layout, note)) => {
                if !self.layout.parts.is_empty() || self.layout.terrain.is_some() {
                    self.snapshot(None);
                }
                self.layout = layout;
                self.error = None;
                self.selected = None;
                self.selected_group = None;
                self.open_group = None;
                self.level = self.level.min(self.top_level());
                self.status = note;
                self.mark_all();
            }
            // A new landscape can leave the piazza under water: keep the land, drop the town.
            Err(err) if self.source == Source::Landscape && self.plaza.is_some() => {
                self.plaza = None;
                self.regenerate();
                self.status = format!("Town removed: {err}. Place the piazza again.");
            }
            Err(err) => {
                self.status = format!("Can't do that: {err}");
                self.error = Some(err);
            }
        }
    }

    /// Grows the town round a piazza at `plaza` (replacing any town already there).
    pub fn grow_town_at(&mut self, plaza: P2) {
        let previous = self.plaza.replace(plaza);
        self.source = Source::Landscape;
        self.regenerate();
        if self.error.is_some() {
            self.plaza = previous;
        }
    }

    /// Removes the town, leaving the bare landscape.
    pub fn clear_town(&mut self) {
        self.plaza = None;
        self.regenerate();
        self.status = "Town cleared".into();
    }

    /// Replaces the layout (a loaded project or a new empty plot). Undoable.
    pub fn replace(&mut self, layout: Layout) {
        self.snapshot(None);
        self.layout = layout;
        self.selected = None;
        self.selected_group = None;
        self.open_group = None;
        self.level = 0;
        self.mark_all();
    }

    /// Changes the layout through `change`, redrawing `levels`. Consecutive edits with the
    /// same `coalesce` key share one undo step.
    pub fn edit(
        &mut self,
        levels: &[u32],
        coalesce: Option<&str>,
        change: impl FnOnce(&mut Layout),
    ) {
        self.snapshot(coalesce);
        change(&mut self.layout);
        self.mark(levels);
    }

    fn snapshot(&mut self, coalesce: Option<&str>) {
        let same = coalesce.is_some() && self.last_edit.as_deref() == coalesce;
        self.last_edit = coalesce.map(str::to_string);
        if same {
            return;
        }
        self.history.push(self.layout.clone());
        if self.history.len() > HISTORY_LIMIT {
            self.history.remove(0);
        }
        self.future.clear();
    }

    pub fn can_undo(&self) -> bool {
        !self.history.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.future.is_empty()
    }

    pub fn undo(&mut self) {
        if let Some(previous) = self.history.pop() {
            self.future
                .push(std::mem::replace(&mut self.layout, previous));
            self.after_history_jump("Undone");
        }
    }

    pub fn redo(&mut self) {
        if let Some(next) = self.future.pop() {
            self.history.push(std::mem::replace(&mut self.layout, next));
            self.after_history_jump("Redone");
        }
    }

    fn after_history_jump(&mut self, status: &str) {
        self.last_edit = None;
        if self
            .selected
            .is_some_and(|id| self.layout.get(id).is_none())
        {
            self.selected = None;
        }
        for group in [&mut self.selected_group, &mut self.open_group] {
            if group.is_some_and(|g| self.layout.group_parts(g).next().is_none()) {
                *group = None;
            }
        }
        self.status = status.into();
        self.mark_all();
    }

    /// Highest level holding any part (roofs included).
    pub fn top_level(&self) -> u32 {
        self.layout.parts.iter().map(|p| p.level).max().unwrap_or(0)
    }

    /// Every level a building's parts are on.
    pub fn group_levels(&self, group: u32) -> Vec<u32> {
        let mut levels: Vec<u32> = self.layout.group_parts(group).map(|p| p.level).collect();
        levels.sort_unstable();
        levels.dedup();
        levels
    }

    pub fn mark_all(&mut self) {
        self.dirty_all = true;
        self.terrain_dirty = true;
        self.revision += 1;
    }

    pub fn mark(&mut self, levels: &[u32]) {
        self.dirty.extend(levels);
        self.revision += 1;
    }

    pub fn mark_terrain(&mut self) {
        self.terrain_dirty = true;
        self.revision += 1;
    }

    /// Levels to rebuild, or `None` for everything; clears the request.
    pub fn take_dirty(&mut self) -> Option<Option<Vec<u32>>> {
        if self.dirty_all {
            self.dirty_all = false;
            self.dirty.clear();
            return Some(None);
        }
        if self.dirty.is_empty() {
            return None;
        }
        Some(Some(std::mem::take(&mut self.dirty).into_iter().collect()))
    }

    pub fn take_terrain_dirty(&mut self) -> bool {
        std::mem::take(&mut self.terrain_dirty)
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty_all || !self.dirty.is_empty() || self.terrain_dirty
    }
}
