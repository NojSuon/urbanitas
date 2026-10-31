//! Saving and opening projects, and exporting the current layout.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use urbanitas::ArchitectureStyle;
use urbanitas::design::Design;
use urbanitas::export::{obj, svg};
use urbanitas::geometry::Rect;
use urbanitas::parts::{AreaKind, AreaPart, Layout, PartKind};
use urbanitas::rng::Rng;
use urbanitas::style::{Palette, StyleProfile};

pub const PROJECTS_DIR: &str = "projects";
pub const EXPORTS_DIR: &str = "exports";
const PROJECT_SUFFIX: &str = ".urbanitas.json";

/// Letters, digits, `-` and `_` only.
pub fn file_stem(name: &str) -> String {
    let stem: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
        .collect();
    if stem.is_empty() {
        "untitled".into()
    } else {
        stem
    }
}

pub fn save(layout: &Layout) -> io::Result<PathBuf> {
    fs::create_dir_all(PROJECTS_DIR)?;
    let path = Path::new(PROJECTS_DIR).join(format!("{}{PROJECT_SUFFIX}", file_stem(&layout.name)));
    fs::write(&path, layout.to_json())?;
    Ok(path)
}

pub fn list_projects() -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(PROJECTS_DIR) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.to_string_lossy().ends_with(PROJECT_SUFFIX))
        .collect();
    paths.sort();
    paths
}

pub fn open(path: &Path) -> Result<Layout, String> {
    let text = fs::read_to_string(path).map_err(|e| e.to_string())?;
    Layout::from_json(&text).map_err(|e| e.to_string())
}

/// An empty plot of lawn to build on from scratch.
pub fn empty_plot(style: ArchitectureStyle, seed: u64) -> Layout {
    let palette = Palette::choose(style, &mut Rng::new(seed));
    let mut layout = Layout::new(
        format!("plot_{seed}"),
        StyleProfile::for_style(style),
        palette,
    );
    layout.description = format!("{} · hand-built", style.label());
    layout.add(
        0,
        PartKind::Area(AreaPart {
            kind: AreaKind::Lawn,
            rect: Rect::new(-15.0, -15.0, 30.0, 30.0),
            height: 0.1,
        }),
    );
    layout
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Export {
    Obj,
    Svg,
    Json,
}

/// Writes the layout into `exports/<name>/`; returns the folder.
pub fn export(layout: &Layout, format: Export, texture_size: u32) -> io::Result<PathBuf> {
    let stem = file_stem(&layout.name);
    let dir = Path::new(EXPORTS_DIR).join(&stem);
    fs::create_dir_all(&dir)?;
    match format {
        Export::Obj => {
            obj::write_obj(layout, &dir.join(format!("{stem}.obj")), texture_size)?;
        }
        Export::Svg => {
            for level in 0..layout.levels() {
                fs::write(
                    dir.join(format!("{stem}_floor{level}.svg")),
                    svg::floor_plan_svg(layout, level),
                )?;
            }
        }
        Export::Json => fs::write(
            dir.join(format!("{stem}{PROJECT_SUFFIX}")),
            layout.to_json(),
        )?,
    }
    Ok(dir)
}

pub const DESIGNS_DIR: &str = "designs";
const DESIGN_SUFFIX: &str = ".design.json";

/// Saves a house design into `designs/<id>.design.json`.
pub fn save_design(design: &Design) -> io::Result<PathBuf> {
    fs::create_dir_all(DESIGNS_DIR)?;
    let path = Path::new(DESIGNS_DIR).join(format!("{}{DESIGN_SUFFIX}", file_stem(&design.id)));
    fs::write(&path, design.to_json())?;
    Ok(path)
}

/// Every saved design, sorted by name (files that fail to load are skipped).
pub fn list_designs() -> Vec<Design> {
    let Ok(entries) = fs::read_dir(DESIGNS_DIR) else {
        return Vec::new();
    };
    let mut designs: Vec<Design> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.to_string_lossy().ends_with(DESIGN_SUFFIX))
        .filter_map(|p| Design::from_json(&fs::read_to_string(p).ok()?).ok())
        .collect();
    designs.sort_by(|a, b| a.name.cmp(&b.name));
    designs
}

pub fn delete_design(id: &str) -> io::Result<()> {
    fs::remove_file(Path::new(DESIGNS_DIR).join(format!("{}{DESIGN_SUFFIX}", file_stem(id))))
}

/// A design id from its name that no saved design uses yet.
pub fn new_design_id(name: &str, taken: &[Design]) -> String {
    let base = match urbanitas::design::slug(name) {
        s if s.is_empty() => "house".to_string(),
        s => s,
    };
    let mut id = base.clone();
    let mut n = 2;
    while taken.iter().any(|d| d.id == id) {
        id = format!("{base}-{n}");
        n += 1;
    }
    id
}
