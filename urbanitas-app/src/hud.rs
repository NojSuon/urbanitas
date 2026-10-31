//! The HUD: a top bar, a left panel for the current mode and a status bar.

use bevy::prelude::*;
use bevy_egui::egui::{self, Align, Layout as UiLayout, Margin, RichText};
use bevy_egui::{EguiContexts, EguiPrimaryContextPass};
use urbanitas::mesh::build::WallMode;
use urbanitas::model::Roof;
use urbanitas::parts::{Part, PartKind};
use urbanitas::{ArchitectureStyle, BuildingType, Garden};

use crate::files::{self, Export};
use crate::project::{Mode, Project, Quality};
use crate::scene::SceneStats;
use crate::theme::{self, GOLD, PARCHMENT, PARCHMENT_DARK, SEPIA_SOFT, TERRACOTTA};
use crate::tools::{self, Category, Item, Pointer, Tool};

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<HudState>()
            .add_systems(EguiPrimaryContextPass, hud)
            .add_systems(Update, regenerate_when_due);
    }
}

#[derive(Resource, Default)]
struct HudState {
    themed: bool,
    open_dialog: bool,
    projects: Vec<std::path::PathBuf>,
    fps: f32,
}

const DEBOUNCE: f64 = 0.15;

fn regenerate_when_due(mut project: ResMut<Project>, time: Res<Time>) {
    if project
        .regenerate_at
        .is_some_and(|at| time.elapsed_secs_f64() >= at)
    {
        project.regenerate_at = None;
        project.regenerate();
    }
}

fn panel_frame() -> egui::Frame {
    egui::Frame::NONE
        .fill(PARCHMENT)
        .inner_margin(Margin::same(12))
        .stroke(egui::Stroke::new(1.0, theme::GOLD_SOFT))
}

#[allow(clippy::too_many_arguments)]
fn hud(
    mut contexts: EguiContexts,
    mut project: ResMut<Project>,
    mut tool: ResMut<Tool>,
    mut pointer: ResMut<Pointer>,
    mut state: ResMut<HudState>,
    stats: Res<SceneStats>,
    time: Res<Time>,
) -> Result {
    let ctx = contexts.ctx_mut()?.clone();
    if !state.themed {
        // New fonts only take effect next frame, so draw nothing until then.
        theme::apply(&ctx);
        state.themed = true;
        return Ok(());
    }
    let dt = time.delta_secs().max(1e-4);
    state.fps = state.fps * 0.95 + (1.0 / dt) * 0.05;
    let now = time.elapsed_secs_f64();

    let mut root = egui::Ui::new(
        ctx.clone(),
        "hud".into(),
        egui::UiBuilder::new()
            .layer_id(egui::LayerId::background())
            .max_rect(ctx.viewport_rect()),
    );

    egui::Panel::top("top_bar")
        .frame(panel_frame().inner_margin(Margin::symmetric(14, 8)))
        .show(&mut root, |ui| {
            top_bar(ui, &mut project, &mut state, &mut tool)
        });

    egui::Panel::bottom("status_bar")
        .frame(panel_frame().inner_margin(Margin::symmetric(14, 5)))
        .show(&mut root, |ui| {
            status_bar(ui, &project, &tool, &stats, state.fps)
        });

    egui::Panel::left("side_panel")
        .exact_size(300.0)
        .frame(panel_frame())
        .show(&mut root, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| match project.mode {
                Mode::Generate => generate_panel(ui, &mut project, now),
                Mode::Build => build_panel(ui, &mut project, &mut tool),
            });
        });

    if state.open_dialog {
        let mut open = true;
        let mut chosen = None;
        egui::Window::new(
            RichText::new("Open a project").font(egui::FontId::new(18.0, theme::heading_family())),
        )
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(&ctx, |ui| {
            if state.projects.is_empty() {
                ui.label(format!(
                    "No saved projects in ./{}/ yet.",
                    files::PROJECTS_DIR
                ));
            }
            for path in &state.projects {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                if ui.button(name).clicked() {
                    chosen = Some(path.clone());
                }
            }
        });
        if let Some(path) = chosen {
            match files::open(&path) {
                Ok(layout) => {
                    project.replace(layout);
                    project.mode = Mode::Build;
                    project.status = format!("Opened {}", path.display());
                }
                Err(err) => project.status = format!("Could not open {}: {err}", path.display()),
            }
            open = false;
        }
        state.open_dialog = open;
    }

    pointer.over_ui = ctx.is_pointer_over_egui() || ctx.egui_wants_pointer_input();
    pointer.ui_wants_keyboard = ctx.egui_wants_keyboard_input();
    Ok(())
}

fn top_bar(ui: &mut egui::Ui, project: &mut Project, state: &mut HudState, tool: &mut Tool) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new("URBANITAS")
                .font(egui::FontId::new(24.0, theme::heading_family()))
                .color(TERRACOTTA),
        );
        ui.label(
            RichText::new("architettura procedurale")
                .italics()
                .color(SEPIA_SOFT),
        );
        ui.add_space(18.0);
        for (mode, label) in [(Mode::Generate, "Generate"), (Mode::Build, "Build")] {
            if ui.selectable_label(project.mode == mode, label).clicked() && project.mode != mode {
                project.mode = mode;
                tool.pick(None);
                project.selected = None;
                project.mark_all();
            }
        }
        ui.separator();
        if ui
            .button("New plot")
            .on_hover_text("Start from an empty lawn in build mode")
            .clicked()
        {
            project.replace(files::empty_plot(project.params.style, project.params.seed));
            project.mode = Mode::Build;
            project.status = "Empty plot: start with a facade wall".into();
        }
        if ui.button("Save").clicked() {
            project.status = match files::save(&project.layout) {
                Ok(path) => format!("Saved {}", path.display()),
                Err(err) => format!("Could not save: {err}"),
            };
        }
        if ui.button("Open…").clicked() {
            state.projects = files::list_projects();
            state.open_dialog = true;
        }
        ui.menu_button("Export", |ui| {
            for (format, label) in [
                (Export::Obj, "3D model (OBJ + textures)"),
                (Export::Svg, "Floor plans (SVG)"),
                (Export::Json, "Project (JSON)"),
            ] {
                if ui.button(label).clicked() {
                    project.status = match files::export(&project.layout, format, 512) {
                        Ok(dir) => format!("Exported to {}", dir.display()),
                        Err(err) => format!("Export failed: {err}"),
                    };
                    ui.close();
                }
            }
        });
        ui.separator();
        if ui
            .add_enabled(project.can_undo(), egui::Button::new("Undo"))
            .clicked()
        {
            project.undo();
        }
        if ui
            .add_enabled(project.can_redo(), egui::Button::new("Redo"))
            .clicked()
        {
            project.redo();
        }
        ui.with_layout(UiLayout::right_to_left(Align::Center), |ui| {
            let mut quality = project.quality;
            egui::ComboBox::from_id_salt("quality")
                .selected_text(match quality {
                    Quality::Low => "Low",
                    Quality::High => "High",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut quality, Quality::Low, "Low (fast)");
                    ui.selectable_value(&mut quality, Quality::High, "High");
                });
            if quality != project.quality {
                project.quality = quality;
                project.mark_all();
            }
            ui.label("Quality");
        });
    });
}

fn status_bar(ui: &mut egui::Ui, project: &Project, tool: &Tool, stats: &SceneStats, fps: f32) {
    ui.horizontal(|ui| {
        ui.label(&project.status);
        ui.with_layout(UiLayout::right_to_left(Align::Center), |ui| {
            ui.label(RichText::new(format!("{fps:.0} fps · {} k triangles · rebuilt in {:.0} ms", stats.triangles() / 1000, stats.build_ms)).small().color(SEPIA_SOFT));
            ui.separator();
            let hint = match (project.mode, tool.item) {
                (Mode::Generate, _) => "Right-drag orbit · middle-drag pan · scroll zoom".to_string(),
                (Mode::Build, Some(item)) => format!("{} · Alt: free placement · Q/E turn · Esc: done", item.hint()),
                (Mode::Build, None) => "Click to select · drag to move · R rotate · Del delete · PgUp/PgDn level · Ctrl+Z undo".into(),
            };
            ui.label(RichText::new(hint).small().color(SEPIA_SOFT));
        });
    });
}

fn generate_panel(ui: &mut egui::Ui, project: &mut Project, now: f64) {
    theme::title(ui, "The commission");
    theme::ornament(ui);
    let before = project.params.clone();
    let p = &mut project.params;

    egui::Grid::new("params")
        .num_columns(2)
        .spacing([10.0, 9.0])
        .show(ui, |ui| {
            ui.label("Style");
            egui::ComboBox::from_id_salt("style")
                .selected_text(p.style.label())
                .show_ui(ui, |ui| {
                    for style in ArchitectureStyle::ALL {
                        ui.selectable_value(&mut p.style, style, style.label());
                    }
                });
            ui.end_row();

            ui.label("Building");
            egui::ComboBox::from_id_salt("type")
                .selected_text(capitalise(p.building_type.label()))
                .show_ui(ui, |ui| {
                    for kind in [
                        BuildingType::House,
                        BuildingType::Villa,
                        BuildingType::Apartment,
                        BuildingType::Shop,
                    ] {
                        ui.selectable_value(&mut p.building_type, kind, capitalise(kind.label()));
                    }
                });
            ui.end_row();

            ui.label("Floors");
            ui.add(egui::Slider::new(&mut p.floors, 1..=6));
            ui.end_row();

            ui.label("Width");
            ui.add(
                egui::Slider::new(&mut p.size.width, 6.0..=30.0)
                    .step_by(0.5)
                    .suffix(" m"),
            );
            ui.end_row();

            ui.label("Depth");
            ui.add(
                egui::Slider::new(&mut p.size.depth, 6.0..=30.0)
                    .step_by(0.5)
                    .suffix(" m"),
            );
            ui.end_row();

            ui.label("Rooms / floor");
            ui.add(egui::Slider::new(&mut p.rooms, 1..=10));
            ui.end_row();

            ui.label("Garden");
            egui::ComboBox::from_id_salt("garden")
                .selected_text(capitalise(p.garden.label()))
                .show_ui(ui, |ui| {
                    for garden in [
                        Garden::None,
                        Garden::Front,
                        Garden::Back,
                        Garden::Surrounding,
                    ] {
                        ui.selectable_value(&mut p.garden, garden, capitalise(garden.label()));
                    }
                });
            ui.end_row();

            ui.label("Seed");
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut p.seed).speed(1.0));
                if ui.button("Reroll").clicked() {
                    p.seed = p
                        .seed
                        .wrapping_mul(6364136223846793005)
                        .wrapping_add(1442695040888963407)
                        % 1_000_000;
                }
            });
            ui.end_row();
        });

    if project.params != before {
        project.regenerate_at = Some(now + DEBOUNCE);
    }
    if let Some(err) = &project.error {
        ui.add_space(4.0);
        ui.label(RichText::new(err).color(TERRACOTTA));
    }

    ui.add_space(10.0);
    theme::ornament(ui);
    let layout = &project.layout;
    ui.add(egui::Label::new(RichText::new(&layout.description).small().color(SEPIA_SOFT)).wrap());
    let rooms = layout
        .parts
        .iter()
        .filter(|p| matches!(&p.kind, PartKind::Floor(f) if f.room.is_some()))
        .count();
    ui.label(format!(
        "{} parts · {rooms} rooms · {} storeys",
        layout.parts.len(),
        layout.levels()
    ));
    ui.add_space(8.0);
    let edit = egui::Button::new(
        RichText::new("Edit part by part")
            .size(16.0)
            .color(theme::PARCHMENT_LIGHT),
    )
    .fill(TERRACOTTA);
    if ui.add_sized([ui.available_width(), 34.0], edit).clicked() {
        project.mode = Mode::Build;
        project.level = 0;
        project.mark_all();
    }
}

fn build_panel(ui: &mut egui::Ui, project: &mut Project, tool: &mut Tool) {
    theme::title(ui, "The workshop");
    theme::ornament(ui);

    ui.horizontal(|ui| {
        ui.label("Level");
        if ui.button("‹").clicked() {
            tools::change_level(project, project.level.saturating_sub(1));
        }
        let top = project.top_level();
        let name = if project.level > 0
            && project.level >= top
            && project
                .layout
                .on_level(project.level)
                .any(|p| matches!(p.kind, PartKind::Roof(_)))
        {
            "Roof".to_string()
        } else {
            urbanitas::export::svg::level_name(project.level)
        };
        ui.label(RichText::new(name).color(theme::SEPIA));
        if ui.button("›").clicked() {
            tools::change_level(project, (project.level + 1).min(top + 1));
        }
    });
    ui.horizontal(|ui| {
        ui.label("Walls");
        for (mode, label) in [
            (WallMode::Up, "Up"),
            (WallMode::Cutaway, "Cutaway"),
            (WallMode::Down, "Down"),
        ] {
            if ui.selectable_label(project.walls == mode, label).clicked() && project.walls != mode
            {
                project.walls = mode;
                let level = project.level;
                project.mark(&[level]);
            }
        }
    });

    ui.add_space(6.0);
    theme::title(ui, "Catalogue");
    theme::ornament(ui);
    if ui
        .selectable_label(tool.item.is_none(), "Select & move")
        .clicked()
    {
        tool.pick(None);
    }
    ui.horizontal_wrapped(|ui| {
        for category in Category::ALL {
            if ui
                .selectable_label(tool.category == category, category.label())
                .clicked()
            {
                tool.category = category;
            }
        }
    });
    ui.add_space(4.0);
    egui::Frame::NONE
        .fill(PARCHMENT_DARK)
        .inner_margin(Margin::same(8))
        .corner_radius(3)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                for item in Item::catalogue(tool.category) {
                    if ui
                        .selectable_label(tool.item == Some(item), item.label())
                        .clicked()
                    {
                        tool.pick(Some(item));
                        project.selected = None;
                        project.status = item.hint().into();
                    }
                }
            });
        });
    ui.label(
        RichText::new("Snaps to a 0.5 m grid · hold Alt to place freely")
            .small()
            .color(SEPIA_SOFT),
    );

    ui.add_space(8.0);
    theme::title(ui, "Inspector");
    theme::ornament(ui);
    match project
        .selected
        .and_then(|id| project.layout.get(id).cloned())
    {
        Some(part) => inspector(ui, project, &part),
        None => {
            ui.label(RichText::new("Nothing selected.").color(SEPIA_SOFT));
        }
    }
}

fn inspector(ui: &mut egui::Ui, project: &mut Project, part: &Part) {
    ui.label(RichText::new(part.label()).strong().color(GOLD));
    ui.label(
        RichText::new(format!("Level {} · part #{}", part.level, part.id))
            .small()
            .color(SEPIA_SOFT),
    );
    let id = part.id;
    let level = part.level;
    let mut edited = part.clone();
    let mut changed = None;
    {
        let mut slider = |ui: &mut egui::Ui,
                          label: &str,
                          value: &mut f64,
                          range: std::ops::RangeInclusive<f64>,
                          key: &str| {
            ui.horizontal(|ui| {
                ui.label(label);
                if ui
                    .add(egui::Slider::new(value, range).max_decimals(2))
                    .changed()
                {
                    changed = Some(key.to_string());
                }
            });
        };
        match &mut edited.kind {
            PartKind::Wall(w) => {
                slider(ui, "Height", &mut w.height, 0.5..=8.0, "height");
                slider(ui, "Thickness", &mut w.thickness, 0.08..=1.0, "thickness");
            }
            PartKind::Opening(o) => {
                slider(ui, "Width", &mut o.width, 0.4..=4.0, "width");
                slider(ui, "Height", &mut o.height, 0.4..=4.0, "oheight");
                slider(ui, "Sill", &mut o.sill, 0.0..=2.5, "sill");
            }
            PartKind::Balcony(b) => {
                slider(ui, "Width", &mut b.width, 0.8..=6.0, "bwidth");
                slider(ui, "Depth", &mut b.depth, 0.4..=2.5, "bdepth");
            }
            PartKind::Prop(p) => {
                slider(ui, "Scale", &mut p.scale, 0.3..=3.0, "scale");
                slider(ui, "Height", &mut p.height, 0.2..=12.0, "pheight");
            }
            PartKind::Area(a) => slider(ui, "Height", &mut a.height, 0.02..=6.0, "aheight"),
            PartKind::Stairs(s) => slider(ui, "Rise", &mut s.rise, 1.0..=5.0, "rise"),
            PartKind::Roof(r) => {
                if let Roof::Hip {
                    overhang,
                    ridge_height,
                    ..
                } = &mut r.roof
                {
                    slider(ui, "Overhang", overhang, 0.0..=2.0, "overhang");
                    slider(ui, "Ridge", ridge_height, 0.3..=6.0, "ridge");
                }
            }
            PartKind::Floor(_) => {}
        }
    }
    if let Some(key) = changed {
        let coalesce = format!("{id}-{key}");
        project.edit(&[level], Some(&coalesce), |layout| {
            if let Some(target) = layout.get_mut(id) {
                *target = edited;
            }
        });
    }
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        if ui.button("Rotate 90°").clicked() {
            tools::rotate_selected(project, id);
        }
        if ui
            .button(RichText::new("Delete").color(TERRACOTTA))
            .clicked()
        {
            tools::delete_part(project, id);
        }
    });
}

fn capitalise(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().collect::<String>() + chars.as_str())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    #[test]
    fn capitalises() {
        assert_eq!(super::capitalise("villa"), "Villa");
        assert_eq!(super::capitalise(""), "");
    }
}
