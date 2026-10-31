//! The open project: parameters, the editable layout, undo history and what needs redrawing.

use std::collections::BTreeSet;

use bevy::prelude::*;
use urbanitas::mesh::build::WallMode;
use urbanitas::parts::{Layout, PartId};
use urbanitas::{BuildingParams, generate};

const HISTORY_LIMIT: usize = 100;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Tweak parameters and watch the building regenerate.
    Generate,
    /// Edit the building part by part.
    Build,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Quality {
    /// No shadows or MSAA, 256 px textures: for low-end PCs.
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
}

#[derive(Resource)]
pub struct Project {
    pub params: BuildingParams,
    pub layout: Layout,
    pub error: Option<String>,
    pub mode: Mode,
    /// The level being edited in build mode; higher levels are hidden.
    pub level: u32,
    pub walls: WallMode,
    pub quality: Quality,
    pub selected: Option<PartId>,
    pub status: String,
    /// When a parameter change should regenerate the building (debounced).
    pub regenerate_at: Option<f64>,
    dirty_all: bool,
    dirty: BTreeSet<u32>,
    history: Vec<Layout>,
    future: Vec<Layout>,
    /// The last coalescing key passed to [`Project::edit`], so slider drags make one undo step.
    last_edit: Option<String>,
}

impl Project {
    pub fn new(params: BuildingParams, quality: Quality) -> Self {
        let mut project = Self {
            layout: Layout::new(
                "urbanitas",
                urbanitas::style::StyleProfile::for_style(params.style),
                placeholder_palette(),
            ),
            params,
            error: None,
            mode: Mode::Generate,
            level: 0,
            walls: WallMode::Up,
            quality,
            selected: None,
            status: "Welcome to Urbanitas".into(),
            regenerate_at: None,
            dirty_all: true,
            dirty: BTreeSet::new(),
            history: Vec::new(),
            future: Vec::new(),
            last_edit: None,
        };
        project.regenerate();
        project
    }

    /// Rebuilds the layout from the parameters. Undoable.
    pub fn regenerate(&mut self) {
        match generate(&self.params) {
            Ok(building) => {
                if !self.layout.parts.is_empty() {
                    self.snapshot(None);
                }
                self.layout = building.to_layout();
                self.error = None;
                self.selected = None;
                self.level = self.level.min(self.top_level());
                self.mark_all();
            }
            Err(err) => self.error = Some(err.to_string()),
        }
    }

    /// Replaces the layout (a loaded project or a new empty plot). Undoable.
    pub fn replace(&mut self, layout: Layout) {
        self.snapshot(None);
        self.layout = layout;
        self.selected = None;
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
        self.status = status.into();
        self.mark_all();
    }

    /// Highest level holding any part (roofs included).
    pub fn top_level(&self) -> u32 {
        self.layout.parts.iter().map(|p| p.level).max().unwrap_or(0)
    }

    pub fn mark_all(&mut self) {
        self.dirty_all = true;
    }

    pub fn mark(&mut self, levels: &[u32]) {
        self.dirty.extend(levels);
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

    pub fn is_dirty(&self) -> bool {
        self.dirty_all || !self.dirty.is_empty()
    }
}

fn placeholder_palette() -> urbanitas::style::Palette {
    urbanitas::style::Palette::choose(
        urbanitas::ArchitectureStyle::Mediterranean,
        &mut urbanitas::rng::Rng::new(0),
    )
}
