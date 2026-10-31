//! The HUD: a slim icon rail whose panels slide in over the view (Landscape, Town, Build,
//! Designs, Colours, Settings), a bubble toolbar of terrain tools, a floating card that
//! follows the selection, toasts, a minimap and a status bar.

use std::f32::consts::TAU;

use bevy::prelude::*;
use bevy_egui::egui::{self, Align, Color32, Layout as UiLayout, Margin, RichText, pos2, vec2};
use bevy_egui::{EguiContexts, EguiPrimaryContextPass};
use bevy_panorbit_camera::PanOrbitCamera;
use urbanitas::design::{Design, update_copies};
use urbanitas::landscape::ground_map;
use urbanitas::mesh::build::WallMode;
use urbanitas::model::Roof;
use urbanitas::parts::{AreaKind, Part, PartKind, RoofPart};
use urbanitas::style::{Color as Rgb, Palette, Swatches};
use urbanitas::{
    ArchitectureStyle, BuildingParams, BuildingType, Context, DesignMix, Garden, Relief, Shape,
    WaterBody,
};

use crate::files::{self, Export};
use crate::project::{Mode, Project, Quality, Source};
use crate::scene::{MainCamera, SceneStats};
use crate::theme::{
    self, GOLD, GOLD_SOFT, PARCHMENT, PARCHMENT_DARK, PARCHMENT_LIGHT, SEPIA, SEPIA_SOFT,
    TERRACOTTA,
};
use crate::thumbs::Thumbs;
use crate::tools::{self, Category, Item, Placement, Pointer, Sculpt, Tool};

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<HudState>()
            .add_systems(EguiPrimaryContextPass, hud)
            .add_systems(Update, regenerate_when_due);
    }
}

/// The panels on the rail.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Panel {
    Landscape,
    Town,
    Build,
    Designs,
    Colours,
    Settings,
}

impl Panel {
    const ALL: [Panel; 6] = [
        Panel::Landscape,
        Panel::Town,
        Panel::Build,
        Panel::Designs,
        Panel::Colours,
        Panel::Settings,
    ];

    fn label(self) -> &'static str {
        match self {
            Panel::Landscape => "Landscape",
            Panel::Town => "Town",
            Panel::Build => "Build",
            Panel::Designs => "Designs",
            Panel::Colours => "Colours",
            Panel::Settings => "Settings",
        }
    }

    /// The mode the panel works in, if it needs one.
    fn mode(self) -> Option<Mode> {
        match self {
            Panel::Landscape | Panel::Town => Some(Mode::Generate),
            Panel::Build | Panel::Designs => Some(Mode::Build),
            Panel::Colours | Panel::Settings => None,
        }
    }
}

#[derive(Resource)]
pub struct HudState {
    themed: bool,
    open_dialog: bool,
    projects: Vec<std::path::PathBuf>,
    fps: f32,
    /// The open panel; `shown` keeps drawing the last one while it slides away.
    pub panel: Option<Panel>,
    shown: Panel,
    toasts: Vec<(String, f64)>,
    last_status: String,
    design_name: String,
    /// Which house the design name was filled in for.
    design_name_for: Option<u32>,
    water_open: bool,
    minimap: Option<(egui::TextureHandle, u64)>,
    minimap_at: f64,
    show_minimap: bool,
}

impl Default for HudState {
    fn default() -> Self {
        Self {
            themed: false,
            open_dialog: false,
            projects: Vec::new(),
            fps: 60.0,
            panel: Some(Panel::Landscape),
            shown: Panel::Landscape,
            toasts: Vec::new(),
            last_status: String::new(),
            design_name: String::new(),
            design_name_for: None,
            water_open: false,
            minimap: None,
            minimap_at: -1.0,
            show_minimap: true,
        }
    }
}

const DEBOUNCE: f64 = 0.2;
const PANEL_WIDTH: f32 = 318.0;
const TOAST_SECONDS: f64 = 4.0;

fn regenerate_when_due(mut project: ResMut<Project>, time: Res<Time>) {
    if project
        .regenerate_at
        .is_some_and(|at| time.elapsed_secs_f64() >= at)
    {
        project.regenerate_at = None;
        project.regenerate();
    }
}

fn bar_frame() -> egui::Frame {
    egui::Frame::NONE
        .fill(PARCHMENT)
        .inner_margin(Margin::same(12))
        .stroke(egui::Stroke::new(1.0, GOLD_SOFT))
}

/// A floating card: rounded, softly shadowed parchment.
fn card() -> egui::Frame {
    egui::Frame::NONE
        .fill(PARCHMENT)
        .inner_margin(Margin::same(12))
        .corner_radius(10)
        .stroke(egui::Stroke::new(1.0, GOLD_SOFT))
        .shadow(egui::Shadow {
            offset: [0, 4],
            blur: 18,
            spread: 0,
            color: Color32::from_black_alpha(60),
        })
}

/// Eases a 0–1 animation value out (fast start, gentle stop).
fn ease(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

#[allow(clippy::too_many_arguments)]
fn hud(
    mut contexts: EguiContexts,
    mut project: ResMut<Project>,
    mut tool: ResMut<Tool>,
    mut pointer: ResMut<Pointer>,
    mut state: ResMut<HudState>,
    stats: Res<SceneStats>,
    thumbs: Res<Thumbs>,
    time: Res<Time>,
    camera: Query<(&Camera, &GlobalTransform), With<MainCamera>>,
    mut orbit: Query<&mut PanOrbitCamera, With<MainCamera>>,
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
    if project.status != state.last_status {
        state.last_status = project.status.clone();
        if !project.status.is_empty() {
            state.toasts.push((project.status.clone(), now));
        }
    }
    state.toasts.retain(|(_, at)| now - at < TOAST_SECONDS);
    if state.toasts.len() > 4 {
        state.toasts.remove(0);
    }

    let mut root = egui::Ui::new(
        ctx.clone(),
        "hud".into(),
        egui::UiBuilder::new()
            .layer_id(egui::LayerId::background())
            .max_rect(ctx.viewport_rect()),
    );

    let top = egui::Panel::top("top_bar")
        .frame(bar_frame().inner_margin(Margin::symmetric(14, 8)))
        .show(&mut root, |ui| {
            top_bar(ui, &mut project, &mut state, &mut tool)
        })
        .response
        .rect;
    let bottom = egui::Panel::bottom("status_bar")
        .frame(bar_frame().inner_margin(Margin::symmetric(14, 5)))
        .show(&mut root, |ui| {
            status_bar(ui, &project, &tool, &stats, state.fps)
        })
        .response
        .rect;
    let rail = egui::Panel::left("rail")
        .exact_size(58.0)
        .frame(bar_frame().inner_margin(Margin::symmetric(7, 10)))
        .show(&mut root, |ui| {
            rail_buttons(ui, &mut project, &mut tool, &mut state)
        })
        .response
        .rect;

    // The panel slides out from under the rail.
    let open =
        ctx.animate_bool_with_time(egui::Id::new("panel_slide"), state.panel.is_some(), 0.22);
    if let Some(panel) = state.panel {
        state.shown = panel;
    }
    let slide = ease(open);
    let panel_left = rail.right() + 10.0 - (1.0 - slide) * (PANEL_WIDTH + 30.0);
    if open > 0.001 {
        let height = (bottom.top() - top.bottom() - 20.0).max(120.0);
        egui::Area::new(egui::Id::new("panel"))
            .fixed_pos(pos2(panel_left, top.bottom() + 10.0))
            .order(egui::Order::Middle)
            .show(&ctx, |ui| {
                ui.multiply_opacity(open);
                card().show(ui, |ui| {
                    ui.set_width(PANEL_WIDTH - 24.0);
                    // A full-height sheet, so the scroll area has room inside it.
                    ui.set_min_height(height - 24.0);
                    egui::ScrollArea::vertical().max_height(height - 24.0).show(
                        ui,
                        |ui| match state.shown {
                            Panel::Landscape => landscape_panel(ui, &mut project, &mut tool, now),
                            Panel::Town => town_panel(ui, &mut project, now),
                            Panel::Build => build_panel(ui, &mut project, &mut tool, &thumbs),
                            Panel::Designs => {
                                designs_panel(ui, &mut project, &mut tool, &mut state, &thumbs)
                            }
                            Panel::Colours => colours_panel(ui, &mut project),
                            Panel::Settings => settings_panel(ui, &mut project, &mut state),
                        },
                    );
                });
            });
    }

    // Terrain tools float beside the panel, in build mode or with the landscape open.
    if project.mode == Mode::Build || state.panel == Some(Panel::Landscape) {
        let x = rail.right() + 12.0 + slide * (PANEL_WIDTH + 12.0);
        bubbles(
            &ctx,
            x,
            (top.bottom() + bottom.top()) / 2.0,
            &mut project,
            &mut tool,
            &mut state,
        );
    }

    if let Some(group) = project.open_group {
        open_house_banner(&ctx, top.bottom() + 12.0, &mut project, group);
    }
    if project.placing_piazza {
        hint_banner(
            &ctx,
            top.bottom() + 12.0,
            "Click the map to place the piazza · Esc cancels",
        );
    }

    if project.mode == Mode::Build {
        if let Ok((camera, transform)) = camera.single() {
            floating_card(
                &ctx,
                &mut project,
                &mut state,
                &thumbs,
                camera,
                transform,
                panel_left + PANEL_WIDTH * slide,
            );
        }
    }

    let mut map_top = bottom.top() - 10.0;
    if state.show_minimap {
        if let Ok(mut orbit) = orbit.single_mut() {
            map_top = minimap(&ctx, &mut state, &project, &mut orbit, bottom.top(), now);
        }
    }
    toasts(&ctx, &state, map_top, now);

    if state.open_dialog {
        open_dialog(&ctx, &mut project, &mut state);
    }

    pointer.over_ui = ctx.is_pointer_over_egui() || ctx.egui_wants_pointer_input();
    pointer.ui_wants_keyboard = ctx.egui_wants_keyboard_input();
    Ok(())
}

fn open_dialog(ctx: &egui::Context, project: &mut Project, state: &mut HudState) {
    let mut open = true;
    let mut chosen = None;
    egui::Window::new(
        RichText::new("Open a project").font(egui::FontId::new(18.0, theme::heading_family())),
    )
    .open(&mut open)
    .collapsible(false)
    .resizable(false)
    .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
    .show(ctx, |ui| {
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
                state.panel = Some(Panel::Build);
                project.status = format!("Opened {}", path.display());
            }
            Err(err) => project.status = format!("Could not open {}: {err}", path.display()),
        }
        open = false;
    }
    state.open_dialog = open;
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
        if ui
            .button("New plot")
            .on_hover_text("Start from an empty lawn in build mode")
            .clicked()
        {
            project.replace(files::empty_plot(
                project.site.building.style,
                project.site.building.seed,
            ));
            project.mode = Mode::Build;
            state.panel = Some(Panel::Build);
            tool.pick(None);
            project.status = "Empty plot: place a building, or start with a facade wall".into();
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
            let mode = match project.mode {
                Mode::Generate => "Generating",
                Mode::Build => "Building",
            };
            ui.label(RichText::new(mode).italics().color(SEPIA_SOFT));
        });
    });
}

fn status_bar(ui: &mut egui::Ui, project: &Project, tool: &Tool, stats: &SceneStats, fps: f32) {
    ui.horizontal(|ui| {
        ui.label(&project.status);
        ui.with_layout(UiLayout::right_to_left(Align::Center), |ui| {
            ui.label(
                RichText::new(format!(
                    "{fps:.0} fps · {} k triangles · rebuilt in {:.0} ms",
                    stats.triangles() / 1000,
                    stats.build_ms
                ))
                .small()
                .color(SEPIA_SOFT),
            );
            ui.separator();
            let hint = match (project.mode, tool.item) {
                (_, _) if project.placing_piazza => "Click the map to place the piazza".to_string(),
                (Mode::Generate, _) => "Right-drag orbit · middle-drag pan · scroll zoom".to_string(),
                (Mode::Build, Some(item)) => {
                    format!("{} · Alt: free placement · Q/E turn · Esc: done", item.hint())
                }
                (Mode::Build, None) => "Click: select · Shift+click: whole house · drag: move · R turn · Del delete · PgUp/PgDn level".into(),
            };
            ui.label(RichText::new(hint).small().color(SEPIA_SOFT));
        });
    });
}

// ---------------------------------------------------------------------------------------
// Icons, painted so they look the same whatever fonts are installed.

#[derive(Clone, Copy)]
enum Icon {
    Panel(Panel),
    Sculpt(Sculpt),
    Water,
}

fn paint_icon(painter: &egui::Painter, rect: egui::Rect, icon: Icon, colour: Color32) {
    let p = |x: f32, y: f32| rect.min + vec2(x * rect.width(), y * rect.height());
    let stroke = egui::Stroke::new(1.8, colour);
    let line = |points: Vec<egui::Pos2>| {
        painter.add(egui::Shape::line(points, stroke));
    };
    let wave = |y: f32, amp: f32| -> Vec<egui::Pos2> {
        (0..=16)
            .map(|k| {
                let t = k as f32 / 16.0;
                p(0.15 + 0.7 * t, y + amp * (t * TAU * 1.5).sin())
            })
            .collect()
    };
    match icon {
        Icon::Panel(Panel::Landscape) => {
            line(vec![
                p(0.1, 0.8),
                p(0.38, 0.32),
                p(0.55, 0.58),
                p(0.68, 0.42),
                p(0.9, 0.8),
            ]);
            line(vec![p(0.1, 0.8), p(0.9, 0.8)]);
            line(vec![p(0.31, 0.44), p(0.38, 0.32), p(0.45, 0.44)]);
        }
        Icon::Panel(Panel::Town) => {
            for (x, w, h) in [(0.12, 0.22, 0.3), (0.62, 0.26, 0.24)] {
                let r = egui::Rect::from_min_max(p(x, 0.8 - h), p(x + w, 0.8));
                painter.rect_stroke(r, 1.0, stroke, egui::StrokeKind::Middle);
            }
            let tower = egui::Rect::from_min_max(p(0.4, 0.28), p(0.56, 0.8));
            painter.rect_stroke(tower, 1.0, stroke, egui::StrokeKind::Middle);
            line(vec![p(0.37, 0.3), p(0.48, 0.14), p(0.59, 0.3)]);
            line(vec![p(0.06, 0.8), p(0.94, 0.8)]);
        }
        Icon::Panel(Panel::Build) => {
            for row in 0..3 {
                let y = 0.3 + row as f32 * 0.17;
                line(vec![p(0.14, y), p(0.86, y)]);
                let shift = if row % 2 == 0 { 0.0 } else { 0.12 };
                for k in 0..3 {
                    let x = 0.26 + shift + k as f32 * 0.24;
                    line(vec![p(x, y), p(x, y + 0.17)]);
                }
            }
            line(vec![p(0.14, 0.81), p(0.86, 0.81)]);
        }
        Icon::Panel(Panel::Designs) => {
            line(vec![
                p(0.24, 0.14),
                p(0.62, 0.14),
                p(0.78, 0.3),
                p(0.78, 0.86),
                p(0.24, 0.86),
                p(0.24, 0.14),
            ]);
            line(vec![p(0.62, 0.14), p(0.62, 0.3), p(0.78, 0.3)]);
            line(vec![p(0.34, 0.66), p(0.51, 0.5), p(0.68, 0.66)]);
            line(vec![
                p(0.38, 0.62),
                p(0.38, 0.76),
                p(0.64, 0.76),
                p(0.64, 0.62),
            ]);
        }
        Icon::Panel(Panel::Colours) => {
            painter.circle_stroke(p(0.5, 0.5), rect.width() * 0.34, stroke);
            for (x, y) in [(0.36, 0.36), (0.58, 0.32), (0.68, 0.52), (0.38, 0.6)] {
                painter.circle_filled(p(x, y), rect.width() * 0.055, colour);
            }
        }
        Icon::Panel(Panel::Settings) => {
            let c = p(0.5, 0.5);
            painter.circle_stroke(c, rect.width() * 0.17, stroke);
            for k in 0..8 {
                let a = k as f32 * TAU / 8.0;
                let d = vec2(a.cos(), a.sin()) * rect.width();
                painter.line_segment([c + d * 0.25, c + d * 0.36], egui::Stroke::new(3.0, colour));
            }
        }
        Icon::Sculpt(Sculpt::Raise) => {
            line(vec![
                p(0.12, 0.8),
                p(0.32, 0.8),
                p(0.5, 0.52),
                p(0.68, 0.8),
                p(0.88, 0.8),
            ]);
            line(vec![p(0.5, 0.42), p(0.5, 0.14)]);
            line(vec![p(0.38, 0.26), p(0.5, 0.14), p(0.62, 0.26)]);
        }
        Icon::Sculpt(Sculpt::Lower) => {
            line(vec![
                p(0.12, 0.6),
                p(0.32, 0.6),
                p(0.5, 0.84),
                p(0.68, 0.6),
                p(0.88, 0.6),
            ]);
            line(vec![p(0.5, 0.14), p(0.5, 0.46)]);
            line(vec![p(0.38, 0.34), p(0.5, 0.46), p(0.62, 0.34)]);
        }
        Icon::Sculpt(Sculpt::Flatten) => {
            line(vec![p(0.14, 0.62), p(0.86, 0.62)]);
            line(vec![p(0.14, 0.78), p(0.86, 0.78)]);
            line(vec![
                p(0.3, 0.22),
                p(0.7, 0.22),
                p(0.7, 0.42),
                p(0.3, 0.42),
                p(0.3, 0.22),
            ]);
        }
        Icon::Sculpt(Sculpt::Smooth) => {
            line(wave(0.42, 0.12));
            line(wave(0.66, 0.05));
        }
        Icon::Sculpt(Sculpt::Sand) => {
            for (x, y) in [
                (0.3, 0.3),
                (0.52, 0.26),
                (0.7, 0.36),
                (0.4, 0.48),
                (0.62, 0.54),
                (0.28, 0.66),
                (0.5, 0.72),
                (0.72, 0.72),
            ] {
                painter.circle_filled(p(x, y), rect.width() * 0.045, colour);
            }
        }
        Icon::Water => {
            line(wave(0.4, 0.08));
            line(wave(0.6, 0.08));
            line(wave(0.8, 0.08));
        }
    }
}

/// A square icon button with an eased hover glow.
fn icon_button(
    ui: &mut egui::Ui,
    icon: Icon,
    size: f32,
    active: bool,
    round: bool,
    tip: &str,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(size, size), egui::Sense::click());
    let hover =
        ui.ctx()
            .animate_bool_with_time(response.id.with("hover"), response.hovered(), 0.12);
    let radius = if round { size / 2.0 } else { 8.0 };
    let fill = if active {
        TERRACOTTA
    } else {
        lerp_colour(PARCHMENT_LIGHT, Color32::from_rgb(240, 222, 186), hover)
    };
    let painter = ui.painter_at(rect.expand(6.0));
    if hover > 0.0 || active {
        painter.rect_filled(
            rect.expand(2.0 + 2.0 * hover),
            radius + 2.0,
            Color32::from_rgba_unmultiplied(
                214,
                168,
                72,
                (70.0 * hover.max(active as u8 as f32)) as u8,
            ),
        );
    }
    painter.rect_filled(rect, radius, fill);
    painter.rect_stroke(
        rect,
        radius,
        egui::Stroke::new(1.0, GOLD_SOFT),
        egui::StrokeKind::Inside,
    );
    let colour = if active { PARCHMENT_LIGHT } else { SEPIA };
    paint_icon(&painter, rect.shrink(size * 0.18), icon, colour);
    response.on_hover_text(tip)
}

fn lerp_colour(a: Color32, b: Color32, t: f32) -> Color32 {
    let mix = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t) as u8;
    Color32::from_rgb(mix(a.r(), b.r()), mix(a.g(), b.g()), mix(a.b(), b.b()))
}

fn rail_buttons(ui: &mut egui::Ui, project: &mut Project, tool: &mut Tool, state: &mut HudState) {
    ui.vertical_centered(|ui| {
        ui.spacing_mut().item_spacing = vec2(0.0, 8.0);
        for panel in Panel::ALL {
            let active = state.panel == Some(panel);
            if icon_button(ui, Icon::Panel(panel), 42.0, active, false, panel.label()).clicked() {
                if active {
                    state.panel = None;
                } else {
                    open_panel(project, tool, state, panel);
                }
            }
        }
    });
}

/// Opens a panel, switching to the mode it works in.
fn open_panel(project: &mut Project, tool: &mut Tool, state: &mut HudState, panel: Panel) {
    state.panel = Some(panel);
    if let Some(mode) = panel.mode() {
        if project.mode != mode {
            project.mode = mode;
            tool.pick(None);
            project.selected = None;
            project.selected_group = None;
            project.mark_all();
        }
    }
    if panel != Panel::Town {
        project.placing_piazza = false;
    }
}

// ---------------------------------------------------------------------------------------
// Panels.

fn capitalise(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().collect::<String>() + chars.as_str())
        .unwrap_or_default()
}

fn note(ui: &mut egui::Ui, text: &str) {
    ui.add(egui::Label::new(RichText::new(text).small().color(SEPIA_SOFT)).wrap());
}

fn big_button(ui: &mut egui::Ui, text: &str, enabled: bool) -> bool {
    let button = egui::Button::new(RichText::new(text).size(15.0).color(PARCHMENT_LIGHT))
        .fill(TERRACOTTA)
        .corner_radius(6);
    ui.add_enabled(enabled, button.min_size(vec2(ui.available_width(), 32.0)))
        .clicked()
}

fn reroll(seed: &mut u64) {
    *seed = seed
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407)
        % 1_000_000;
}

fn landscape_panel(ui: &mut egui::Ui, project: &mut Project, tool: &mut Tool, now: f64) {
    theme::title(ui, "Landscape");
    theme::ornament(ui);
    let before = project.landscape;
    let land = &mut project.landscape;
    egui::Grid::new("landscape")
        .num_columns(2)
        .spacing([10.0, 9.0])
        .show(ui, |ui| {
            ui.label("Terrain");
            egui::ComboBox::from_id_salt("relief")
                .selected_text(land.relief.label())
                .show_ui(ui, |ui| {
                    for relief in Relief::ALL {
                        ui.selectable_value(&mut land.relief, relief, relief.label());
                    }
                });
            ui.end_row();
            ui.label("Water");
            egui::ComboBox::from_id_salt("water")
                .selected_text(land.water.label())
                .show_ui(ui, |ui| {
                    for water in WaterBody::ALL {
                        ui.selectable_value(&mut land.water, water, water.label());
                    }
                });
            ui.end_row();
            ui.label("Map size");
            ui.add(
                egui::Slider::new(&mut land.size, 160.0..=640.0)
                    .step_by(20.0)
                    .suffix(" m"),
            );
            ui.end_row();
            ui.label("Seed");
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut land.seed).speed(1.0));
                if ui.button("Reroll").clicked() {
                    reroll(&mut land.seed);
                }
            });
            ui.end_row();
        });
    let changed = project.landscape != before;
    if project.source != Source::Landscape {
        ui.add_space(6.0);
        if big_button(ui, "Use this landscape", true) {
            project.source = Source::Landscape;
            project.regenerate();
        }
    } else if changed {
        project.regenerate_at = Some(now + DEBOUNCE);
    }
    note(
        ui,
        "Terrain tools float to the right: paint hills, valleys and beaches. Then grow a town in the Town panel.",
    );

    ui.add_space(10.0);
    theme::title(ui, "Or a preset site");
    theme::ornament(ui);
    ui.horizontal_wrapped(|ui| {
        for (context, label) in [
            (Context::Single, "One building"),
            (Context::Urban, "Street of palazzi"),
            (Context::Suburban, "Villas"),
            (Context::Rural, "Farm"),
        ] {
            let active = project.source == Source::Site && project.site.context == context;
            if ui.selectable_label(active, label).clicked() {
                project.source = Source::Site;
                project.site.context = context;
                project.plaza = None;
                tool.pick(None);
                project.regenerate();
            }
        }
    });
    if project.source == Source::Site {
        let before = project.site.clone();
        let site = &mut project.site;
        egui::Grid::new("site")
            .num_columns(2)
            .spacing([10.0, 9.0])
            .show(ui, |ui| {
                if site.context != Context::Single {
                    ui.label("Buildings");
                    let max = if site.context == Context::Urban {
                        14
                    } else {
                        6
                    };
                    ui.add(egui::Slider::new(&mut site.buildings, 1..=max));
                    ui.end_row();
                }
                ui.label("Hills");
                ui.add(
                    egui::Slider::new(&mut site.relief, 0.0..=10.0)
                        .step_by(0.5)
                        .suffix(" m"),
                );
                ui.end_row();
            });
        egui::CollapsingHeader::new("Building")
            .default_open(site.context == Context::Single)
            .show(ui, |ui| building_grid(ui, &mut site.building));
        if project.site != before {
            project.regenerate_at = Some(now + DEBOUNCE);
        }
    }
    if let Some(err) = &project.error {
        ui.add_space(4.0);
        ui.label(RichText::new(err).color(TERRACOTTA));
    }
}

fn building_grid(ui: &mut egui::Ui, p: &mut BuildingParams) {
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
            ui.label("Plan");
            egui::ComboBox::from_id_salt("shape")
                .selected_text(p.shape.label())
                .show_ui(ui, |ui| {
                    for shape in Shape::ALL {
                        ui.selectable_value(&mut p.shape, shape, shape.label());
                    }
                });
            ui.end_row();
            ui.label("Floors");
            ui.add(egui::Slider::new(&mut p.floors, 1..=6));
            ui.end_row();
            ui.label("Width");
            ui.add(
                egui::Slider::new(&mut p.size.width, 6.0..=34.0)
                    .step_by(0.5)
                    .suffix(" m"),
            );
            ui.end_row();
            ui.label("Depth");
            ui.add(
                egui::Slider::new(&mut p.size.depth, 6.0..=34.0)
                    .step_by(0.5)
                    .suffix(" m"),
            );
            ui.end_row();
            ui.label("Rooms / floor");
            ui.add(egui::Slider::new(&mut p.rooms, 1..=12));
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
                    reroll(&mut p.seed);
                }
            });
            ui.end_row();
        });
}

fn town_panel(ui: &mut egui::Ui, project: &mut Project, now: f64) {
    theme::title(ui, "Town");
    theme::ornament(ui);
    if project.source != Source::Landscape {
        note(ui, "Towns grow on landscapes.");
        if big_button(ui, "Use a landscape", true) {
            project.source = Source::Landscape;
            project.regenerate();
        }
        return;
    }
    let before = project.town;
    let designs = project.designs.len();
    let town = &mut project.town;
    egui::Grid::new("town")
        .num_columns(2)
        .spacing([10.0, 9.0])
        .show(ui, |ui| {
            ui.label("Style");
            egui::ComboBox::from_id_salt("town_style")
                .selected_text(town.style.label())
                .show_ui(ui, |ui| {
                    for style in ArchitectureStyle::ALL {
                        ui.selectable_value(&mut town.style, style, style.label());
                    }
                });
            ui.end_row();
            ui.label("Houses");
            ui.add(egui::Slider::new(&mut town.houses, 4..=80));
            ui.end_row();
            ui.label("Density");
            ui.add(egui::Slider::new(&mut town.density, 0.0..=1.0).show_value(false))
                .on_hover_text("How tightly houses pack, and how often streets branch");
            ui.end_row();
            ui.label("Houses from");
            egui::ComboBox::from_id_salt("mix")
                .selected_text(town.mix.label())
                .show_ui(ui, |ui| {
                    for mix in DesignMix::ALL {
                        ui.selectable_value(&mut town.mix, mix, mix.label());
                    }
                });
            ui.end_row();
            ui.label("Seed");
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut town.seed).speed(1.0));
                if ui.button("Reroll").clicked() {
                    reroll(&mut town.seed);
                }
            });
            ui.end_row();
        });
    if project.town.mix != DesignMix::Generated && designs == 0 {
        note(
            ui,
            "You have no saved designs yet: save houses in the Designs panel.",
        );
    }
    if project.town != before && project.plaza.is_some() {
        project.regenerate_at = Some(now + DEBOUNCE);
    }
    ui.add_space(8.0);
    let label = match (project.placing_piazza, project.plaza) {
        (true, _) => "Click the map… (Esc cancels)",
        (false, None) => "Place the piazza",
        (false, Some(_)) => "Move the piazza",
    };
    if big_button(ui, label, true) {
        project.placing_piazza = !project.placing_piazza;
        project.status = if project.placing_piazza {
            "Click where the piazza goes: the town grows around it".into()
        } else {
            "Piazza placing cancelled".into()
        };
    }
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        let has_town = project.plaza.is_some();
        if ui
            .add_enabled(has_town, egui::Button::new("Regrow"))
            .on_hover_text("Same piazza, new streets and houses")
            .clicked()
        {
            reroll(&mut project.town.seed);
            project.regenerate();
        }
        if ui
            .add_enabled(has_town, egui::Button::new("Clear town"))
            .clicked()
        {
            project.clear_town();
        }
        if ui
            .button("Suggest a spot")
            .on_hover_text("Grow the town on the gentlest dry ground near the middle")
            .clicked()
        {
            match urbanitas::suggest_piazza(&project.layout) {
                Some(p) => project.grow_town_at(p),
                None => project.status = "No dry ground for a town here".into(),
            }
        }
    });
    if project.plaza.is_some() {
        let streets = project
            .layout
            .parts
            .iter()
            .filter(|p| matches!(&p.kind, PartKind::Area(a) if a.kind == AreaKind::Street))
            .count();
        ui.add_space(4.0);
        note(
            ui,
            &format!(
                "{} houses on {streets} streets. Draw more streets in Build › Streets: houses fill both sides.",
                project.layout.palettes.len()
            ),
        );
    }
}

fn swatch(ui: &mut egui::Ui, colour: Rgb, selected: bool, name: &str) -> bool {
    let fill = Color32::from_rgb(colour.0, colour.1, colour.2);
    let stroke = if selected {
        egui::Stroke::new(2.0, TERRACOTTA)
    } else {
        egui::Stroke::new(1.0, GOLD_SOFT)
    };
    ui.add(
        egui::Button::new("")
            .fill(fill)
            .stroke(stroke)
            .corner_radius(11)
            .min_size(vec2(22.0, 22.0)),
    )
    .on_hover_text(name)
    .clicked()
}

type Field = fn(&mut Palette) -> &mut Rgb;
type SwatchRow<'a> = (&'a str, &'a [(&'a str, Rgb)], Field);

/// Curated swatches for the style, plus a free colour picker, for one building's palette.
fn palette_editor(ui: &mut egui::Ui, project: &mut Project, group: Option<u32>) {
    let swatches = Swatches::for_style(project.layout.style.style);
    let mut palette: Palette = project.layout.palette_of(group).clone();
    let before = palette.clone();
    let rows: [SwatchRow; 4] = [
        ("Walls", swatches.walls, |p| &mut p.walls),
        ("Shutters", swatches.trim, |p| &mut p.trim),
        ("Stone", swatches.stone, |p| &mut p.stone),
        ("Roof", swatches.roof, |p| &mut p.roof),
    ];
    for (label, options, field) in rows {
        ui.horizontal_wrapped(|ui| {
            ui.add_sized([62.0, 20.0], egui::Label::new(label));
            for (name, colour) in options {
                let current = *field(&mut palette);
                if swatch(ui, *colour, current == *colour, name) {
                    *field(&mut palette) = *colour;
                }
            }
            let slot = field(&mut palette);
            let mut rgb = [slot.0, slot.1, slot.2];
            if egui::color_picker::color_edit_button_srgb(ui, &mut rgb).changed() {
                *slot = Rgb(rgb[0], rgb[1], rgb[2]);
            }
        });
    }
    if palette != before {
        palette.name = "custom".into();
        let key = format!("colour-{group:?}");
        project.edit(&[], Some(&key), |layout| {
            match group.filter(|g| (*g as usize) < layout.palettes.len()) {
                Some(g) => layout.palettes[g as usize] = palette,
                None => layout.palette = palette,
            }
        });
        project.mark_all();
    }
}

fn colours_panel(ui: &mut egui::Ui, project: &mut Project) {
    let group = project.selected_group.or(project.open_group);
    match group {
        Some(g) => theme::title(ui, &format!("Colours · {}", tools::house_name(project, g))),
        None => theme::title(ui, "Colours"),
    }
    theme::ornament(ui);
    if group.is_none() {
        note(
            ui,
            "The town's own colours (paths, walls and parts that belong to no house). Shift+click a house to colour it.",
        );
    }
    palette_editor(ui, project, group);
}

fn settings_panel(ui: &mut egui::Ui, project: &mut Project, state: &mut HudState) {
    theme::title(ui, "Settings");
    theme::ornament(ui);
    egui::Grid::new("settings")
        .num_columns(2)
        .spacing([10.0, 9.0])
        .show(ui, |ui| {
            ui.label("Quality");
            let mut quality = project.quality;
            egui::ComboBox::from_id_salt("quality")
                .selected_text(match quality {
                    Quality::Low => "Low (fast)",
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
            ui.end_row();
            ui.label("Minimap");
            ui.checkbox(&mut state.show_minimap, "Show");
            ui.end_row();
        });
    ui.add_space(8.0);
    theme::title(ui, "Keys");
    theme::ornament(ui);
    for line in [
        "Right-drag orbit · middle-drag pan · scroll zoom",
        "Click select · Shift+click a whole house",
        "R turn · Q/E fine turn · Delete remove",
        "PgUp/PgDn change level · Alt place freely",
        "Ctrl+Z undo · Ctrl+Y redo · Esc done",
    ] {
        note(ui, line);
    }
}

fn picture_tile(
    ui: &mut egui::Ui,
    texture: Option<egui::TextureId>,
    label: &str,
    selected: bool,
) -> egui::Response {
    let size = vec2(88.0, 104.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    let hover =
        ui.ctx()
            .animate_bool_with_time(response.id.with("hover"), response.hovered(), 0.12);
    let painter = ui.painter_at(rect.expand(3.0));
    let lift = rect.translate(vec2(0.0, -2.0 * hover));
    let fill = if selected {
        Color32::from_rgb(236, 205, 160)
    } else {
        lerp_colour(PARCHMENT_DARK, Color32::from_rgb(240, 225, 196), hover)
    };
    painter.rect_filled(lift, 6.0, fill);
    let border = if selected {
        egui::Stroke::new(2.0, TERRACOTTA)
    } else {
        egui::Stroke::new(1.0, GOLD_SOFT)
    };
    painter.rect_stroke(lift, 6.0, border, egui::StrokeKind::Inside);
    let image = egui::Rect::from_min_size(lift.min + vec2(6.0, 5.0), vec2(76.0, 76.0));
    match texture {
        Some(id) => {
            painter.image(
                id,
                image,
                egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
        None => {
            painter.rect_filled(image, 4.0, Color32::from_rgb(233, 224, 204));
        }
    }
    painter.text(
        pos2(lift.center().x, lift.max.y - 11.0),
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::proportional(11.5),
        SEPIA,
    );
    response
}

/// A larger picture and a line about the tile, shown on hover.
fn hover_card(
    response: egui::Response,
    texture: Option<egui::TextureId>,
    title: &str,
    line: &str,
) -> egui::Response {
    response.on_hover_ui(|ui| {
        if let Some(id) = texture {
            ui.add(
                egui::Image::new(egui::load::SizedTexture::new(id, [168.0, 168.0]))
                    .corner_radius(6),
            );
        }
        ui.label(RichText::new(title).strong().color(GOLD));
        ui.label(RichText::new(line).small().color(SEPIA_SOFT));
    })
}

fn build_panel(ui: &mut egui::Ui, project: &mut Project, tool: &mut Tool, thumbs: &Thumbs) {
    theme::title(ui, "The workshop");
    theme::ornament(ui);

    ui.horizontal(|ui| {
        ui.label("Level");
        if ui.button("‹").clicked() {
            tools::change_level(project, project.level.saturating_sub(1));
        }
        let top = project.top_level();
        let roofs_here = project
            .layout
            .on_level(project.level)
            .any(|p| matches!(p.kind, PartKind::Roof(_)));
        let walls_here = project
            .layout
            .on_level(project.level)
            .any(|p| matches!(p.kind, PartKind::Wall(_)));
        let name = if project.level > 0 && roofs_here && !walls_here {
            "Roofs".to_string()
        } else {
            urbanitas::export::svg::level_name(project.level)
        };
        ui.label(RichText::new(name).color(SEPIA));
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
                && tool.category != category
            {
                tool.category = category;
                // A tool from another category is put away, so clicks don't place it.
                if tool
                    .item
                    .is_some_and(|i| !Item::catalogue(category).contains(&i))
                {
                    tool.pick(None);
                }
            }
        }
    });
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
        for item in Item::catalogue(tool.category) {
            let texture = thumbs.item(item);
            let tile = picture_tile(ui, texture, item.label(), tool.item == Some(item));
            if hover_card(tile, texture, item.label(), item.hint()).clicked() {
                if tool.item == Some(item) {
                    tool.pick(None);
                } else {
                    tool.pick(Some(item));
                    project.selected = None;
                    project.selected_group = None;
                    project.status = item.hint().into();
                }
            }
        }
    });
    match tool.item {
        Some(Item::Building) => {
            egui::CollapsingHeader::new("New building settings")
                .default_open(true)
                .show(ui, |ui| building_grid(ui, &mut project.site.building));
        }
        Some(Item::Street) => note(
            ui,
            "Houses take the Town panel's style, density and design mix.",
        ),
        Some(i) if i.placement() != Placement::Brush => {
            note(ui, "Snaps to a 0.5 m grid · hold Alt to place freely")
        }
        _ => {}
    }
}

fn designs_panel(
    ui: &mut egui::Ui,
    project: &mut Project,
    tool: &mut Tool,
    state: &mut HudState,
    thumbs: &Thumbs,
) {
    theme::title(ui, "Designs");
    theme::ornament(ui);
    match project.selected_group.or(project.open_group) {
        Some(g) => {
            if state.design_name_for != Some(g) {
                state.design_name_for = Some(g);
                state.design_name = tools::house_name(project, g);
            }
            ui.label(RichText::new("Save this house as a design").color(SEPIA));
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut state.design_name).desired_width(170.0));
                if ui.button("Save design").clicked() {
                    let name = state.design_name.trim().to_string();
                    save_design(project, g, if name.is_empty() { "House" } else { &name });
                }
            });
            let linked = project
                .layout
                .module(g)
                .and_then(|m| m.design.clone())
                .and_then(|id| project.designs.iter().position(|d| d.id == id));
            if let Some(index) = linked {
                let copies = project
                    .layout
                    .modules
                    .iter()
                    .filter(|m| m.design.as_deref() == Some(project.designs[index].id.as_str()))
                    .count();
                ui.add_space(4.0);
                note(
                    ui,
                    &format!(
                        "A copy of “{}” ({copies} in this town).",
                        project.designs[index].name
                    ),
                );
                ui.horizontal(|ui| {
                    if ui
                        .button("Update all copies")
                        .on_hover_text("Save this house's changes into the design and every copy")
                        .clicked()
                    {
                        update_all_copies(project, g, index);
                    }
                    if ui
                        .button("Unlink")
                        .on_hover_text("Make this house a one-off")
                        .clicked()
                    {
                        project.edit(&[], None, |l| urbanitas::design::unlink(l, g));
                        project.status = "Unlinked: this house is a one-off now".into();
                    }
                });
            }
        }
        None => note(
            ui,
            "Shift+click a house (generated or hand-built) to save it as a design.",
        ),
    }

    ui.add_space(8.0);
    theme::title(ui, "Library");
    theme::ornament(ui);
    if project.designs.is_empty() {
        note(ui, "No designs yet.");
        return;
    }
    note(
        ui,
        "Click a design, then click the map to place a copy (R turns it). Right-click for more.",
    );
    let mut delete = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
        for (index, design) in project.designs.iter().enumerate() {
            let texture = thumbs.design(design);
            let item = Item::Design(index);
            let tile = picture_tile(ui, texture, &design.name, tool.item == Some(item));
            let line = format!(
                "{:.0} × {:.0} m plot · {} parts",
                design.lot.w,
                design.lot.d,
                design.parts.len()
            );
            let tile = hover_card(tile, texture, &design.name, &line);
            tile.context_menu(|ui| {
                if ui.button("Delete design").clicked() {
                    delete = Some(index);
                    ui.close();
                }
            });
            if tile.clicked() {
                if tool.item == Some(item) {
                    tool.pick(None);
                } else {
                    tool.pick(Some(item));
                    project.selected = None;
                    project.selected_group = None;
                    project.status = format!("Click to place {} · R turns it", design.name);
                }
            }
        }
    });
    if let Some(index) = delete {
        let design = project.designs.remove(index);
        let _ = files::delete_design(&design.id);
        if matches!(tool.item, Some(Item::Design(_))) {
            tool.pick(None);
        }
        project.status = format!("Deleted design {}", design.name);
    }
}

/// Saves house `group` as a new design and links the house to it.
fn save_design(project: &mut Project, group: u32, name: &str) {
    let id = files::new_design_id(name, &project.designs);
    let Some(design) = Design::capture(&project.layout, group, &id, name) else {
        project.status = "That house has no parts to save".into();
        return;
    };
    match files::save_design(&design) {
        Ok(path) => {
            project.edit(&[], None, |l| {
                let module = l.module_mut(group);
                module.design = Some(id);
                module.name = name.to_string();
            });
            project.designs.push(design);
            project.designs.sort_by(|a, b| a.name.cmp(&b.name));
            project.status = format!("Design saved: {name} ({})", path.display());
        }
        Err(err) => project.status = format!("Could not save the design: {err}"),
    }
}

/// Saves house `group` back into design `index` and rebuilds every copy from it.
fn update_all_copies(project: &mut Project, group: u32, index: usize) {
    let (id, name) = (
        project.designs[index].id.clone(),
        project.designs[index].name.clone(),
    );
    let Some(design) = Design::capture(&project.layout, group, &id, &name) else {
        return;
    };
    if let Err(err) = files::save_design(&design) {
        project.status = format!("Could not save the design: {err}");
        return;
    }
    project.designs[index] = design.clone();
    let mut count = 0;
    project.edit(&[], None, |l| count = update_copies(l, &design));
    project.selected = None;
    project.selected_group = Some(group);
    project.mark_all();
    project.status = format!("Updated {count} copies of {name}");
}

fn inspector(ui: &mut egui::Ui, project: &mut Project, part: &Part) {
    ui.label(RichText::new(part.label()).strong().color(GOLD));
    let building = part
        .group
        .map_or("no house".to_string(), |g| tools::house_name(project, g));
    ui.label(
        RichText::new(format!(
            "Level {} · {building} · part #{}",
            part.level, part.id
        ))
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
                slider(ui, "Height", &mut p.height, 0.2..=14.0, "pheight");
            }
            PartKind::Area(a) => slider(ui, "Height", &mut a.height, 0.02..=16.0, "aheight"),
            PartKind::Stairs(s) => slider(ui, "Rise", &mut s.rise, 1.0..=5.0, "rise"),
            PartKind::Roof(r) => match &mut r.roof {
                Roof::Hip {
                    overhang,
                    ridge_height,
                    ..
                } => {
                    slider(ui, "Overhang", overhang, 0.0..=2.0, "overhang");
                    slider(ui, "Ridge", ridge_height, 0.3..=6.0, "ridge");
                }
                Roof::Flat { parapet_height, .. } => {
                    slider(ui, "Parapet", parapet_height, 0.3..=1.6, "parapet")
                }
            },
            PartKind::Floor(_) => {}
        }
        if let PartKind::Roof(RoofPart {
            roof: Roof::Flat { balustrade, .. },
            ..
        }) = &mut edited.kind
        {
            if ui.checkbox(balustrade, "Balustrade").changed() {
                changed = Some("balustrade".into());
            }
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
        if let Some(g) = part.group.filter(|_| project.open_group.is_none()) {
            if ui.button("Whole house").clicked() {
                project.selected = None;
                project.selected_group = Some(g);
            }
        }
    });
}

// ---------------------------------------------------------------------------------------
// Floating pieces: bubbles, the selection card, banners, toasts and the minimap.

fn bubbles(
    ctx: &egui::Context,
    x: f32,
    middle: f32,
    project: &mut Project,
    tool: &mut Tool,
    state: &mut HudState,
) {
    let size = 40.0;
    let gap = 8.0;
    let count = Sculpt::ALL.len() as f32 + 1.0;
    let top = middle - (count * (size + gap)) / 2.0;
    let mut active_y = None;
    egui::Area::new(egui::Id::new("bubbles"))
        .fixed_pos(pos2(x, top))
        .order(egui::Order::Middle)
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, gap);
            for sculpt in Sculpt::ALL {
                let item = Item::Terrain(sculpt);
                let active = tool.item == Some(item);
                let response =
                    icon_button(ui, Icon::Sculpt(sculpt), size, active, true, item.label());
                if active {
                    active_y = Some(response.rect.center().y);
                }
                if response.clicked() {
                    state.water_open = false;
                    if active {
                        // A click on the lit bubble puts the brush away.
                        tool.pick(None);
                        project.status = "Brush put away".into();
                    } else {
                        if project.mode != Mode::Build {
                            project.mode = Mode::Build;
                            project.mark_all();
                        }
                        tool.pick(Some(item));
                        project.selected = None;
                        project.selected_group = None;
                        project.status = format!(
                            "{}: hold the left button and paint · houses are spared",
                            item.label()
                        );
                    }
                }
            }
            let response =
                icon_button(ui, Icon::Water, size, state.water_open, true, "Water level");
            if state.water_open {
                active_y = Some(response.rect.center().y);
            }
            if response.clicked() {
                state.water_open = !state.water_open;
                if matches!(tool.item, Some(Item::Terrain(_))) {
                    tool.pick(None);
                }
            }
        });
    let Some(y) = active_y else { return };
    // The popover beside the lit bubble.
    egui::Area::new(egui::Id::new("bubble_popover"))
        .fixed_pos(pos2(x + size + 12.0, y - 40.0))
        .order(egui::Order::Middle)
        .show(ctx, |ui| {
            card().inner_margin(Margin::same(10)).show(ui, |ui| {
                ui.set_width(210.0);
                if state.water_open {
                    ui.label(RichText::new("Water level").strong().color(GOLD));
                    match project.layout.water.as_ref().map(|w| w.level) {
                        Some(level) => {
                            ui.label(RichText::new(format!("{level:+.1} m")).color(SEPIA));
                            ui.horizontal(|ui| {
                                for (label, rise) in [("Lower 0.5 m", -0.5), ("Raise 0.5 m", 0.5)] {
                                    if ui.button(label).clicked() {
                                        project.edit(&[], Some("water"), |l| {
                                            if let Some(w) = l.water.as_mut() {
                                                w.shift(rise);
                                            }
                                        });
                                        project.mark_terrain();
                                    }
                                }
                            });
                        }
                        None => note(ui, "This landscape has no water. Pick a sea, lake or river in the Landscape panel."),
                    }
                } else {
                    ui.horizontal(|ui| {
                        ui.label("Size");
                        ui.add(egui::Slider::new(&mut tool.brush_radius, 1.0..=30.0).suffix(" m"));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Strength");
                        ui.add(egui::Slider::new(&mut tool.brush_strength, 0.1..=2.0));
                    });
                    note(ui, "Houses, piazzas and terraces are left alone.");
                }
            });
        });
}

/// Where on screen to pin the selection's card: above the selected house or part.
fn selection_anchor(project: &Project) -> Option<Vec3> {
    let layout = &project.layout;
    if let Some(g) = project.selected_group {
        let lot = layout
            .module(g)
            .and_then(|m| m.lot)
            .or_else(|| tools::group_bounds(layout, g))?;
        let (cx, cz) = lot.center();
        let ground = layout
            .group_parts(g)
            .filter(|p| p.level == 0)
            .map(|p| p.ground)
            .fold(f64::MAX, f64::min);
        let ground = if ground == f64::MAX { 0.0 } else { ground };
        let top = layout.elevation(project.group_levels(g).last().copied().unwrap_or(0));
        return Some(Vec3::new(cx as f32, (ground + top + 1.0) as f32, cz as f32));
    }
    let part = layout.get(project.selected?)?;
    let (cx, cz) = part.footprint().center();
    Some(Vec3::new(
        cx as f32,
        (layout.base(part) + 1.5) as f32,
        cz as f32,
    ))
}

#[allow(clippy::too_many_arguments)]
fn floating_card(
    ctx: &egui::Context,
    project: &mut Project,
    state: &mut HudState,
    thumbs: &Thumbs,
    camera: &Camera,
    transform: &GlobalTransform,
    left_edge: f32,
) {
    let Some(anchor) = selection_anchor(project) else {
        return;
    };
    let view = ctx.viewport_rect();
    let width = 270.0;
    let target = camera
        .world_to_viewport(transform, anchor)
        .ok()
        .map(|p| pos2(p.x, p.y))
        .filter(|p| view.contains(*p));
    // Beside the selection when it's on screen, docked at the right otherwise.
    let pos = match target {
        Some(p) => pos2(
            (p.x + 46.0).clamp(left_edge + 70.0, view.right() - width - 16.0),
            (p.y - 120.0).clamp(view.top() + 70.0, view.bottom() - 420.0),
        ),
        None => pos2(view.right() - width - 16.0, view.top() + 70.0),
    };
    let response = egui::Area::new(egui::Id::new("selection_card"))
        .fixed_pos(pos)
        .order(egui::Order::Middle)
        .show(ctx, |ui| {
            card().show(ui, |ui| {
                ui.set_width(width - 24.0);
                ui.horizontal(|ui| {
                    ui.with_layout(UiLayout::right_to_left(Align::Min), |ui| {
                        if ui.small_button("×").on_hover_text("Deselect").clicked() {
                            project.selected = None;
                            project.selected_group = None;
                        }
                    });
                });
                if let Some(id) = thumbs.preview() {
                    ui.add(
                        egui::Image::new(egui::load::SizedTexture::new(
                            id,
                            [width - 24.0, (width - 24.0) * 168.0 / 264.0],
                        ))
                        .corner_radius(6),
                    );
                }
                if let Some(g) = project.selected_group {
                    house_card(ui, project, state, g);
                } else if let Some(part) = project
                    .selected
                    .and_then(|id| project.layout.get(id).cloned())
                {
                    inspector(ui, project, &part);
                }
            });
        })
        .response;
    // A thin leader from the card to what it describes.
    if let Some(p) = target {
        let from = pos2(response.rect.left(), response.rect.top() + 40.0);
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Background,
            egui::Id::new("leader"),
        ));
        painter.line_segment([from, p], egui::Stroke::new(1.5, GOLD));
        painter.circle_filled(p, 4.0, GOLD);
    }
}

fn house_card(ui: &mut egui::Ui, project: &mut Project, state: &mut HudState, group: u32) {
    let mut name = tools::house_name(project, group);
    ui.horizontal(|ui| {
        if ui
            .add(
                egui::TextEdit::singleline(&mut name)
                    .desired_width(200.0)
                    .font(egui::FontId::new(17.0, theme::heading_family())),
            )
            .changed()
        {
            let key = format!("name-{group}");
            project.edit(&[], Some(&key), |l| l.module_mut(group).name = name.clone());
        }
    });
    let parts = project.layout.group_parts(group).count();
    let storeys = project
        .layout
        .group_parts(group)
        .filter(|p| matches!(p.kind, PartKind::Wall(_)))
        .map(|p| p.level)
        .max()
        .map_or(0, |l| l + 1);
    let copy = project
        .layout
        .module(group)
        .and_then(|m| m.design.clone())
        .and_then(|id| project.designs.iter().find(|d| d.id == id))
        .map(|d| format!(" · copy of {}", d.name))
        .unwrap_or_default();
    note(ui, &format!("{parts} parts · {storeys} storeys{copy}"));
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        if ui
            .button("Turn")
            .on_hover_text("A quarter turn (R)")
            .clicked()
        {
            tools::turn_group(project, group);
        }
        if ui
            .button("Open house")
            .on_hover_text("Edit its parts on their own")
            .clicked()
        {
            tools::open_house(project, group);
        }
        if ui.button("Save design").clicked() {
            state.panel = Some(Panel::Designs);
        }
        if ui.button("Colours").clicked() {
            state.panel = Some(Panel::Colours);
        }
        if ui
            .button(RichText::new("Delete").color(TERRACOTTA))
            .clicked()
        {
            tools::delete_group(project, group);
        }
    });
}

fn hint_banner(ctx: &egui::Context, y: f32, text: &str) {
    egui::Area::new(egui::Id::new("hint_banner"))
        .anchor(egui::Align2::CENTER_TOP, vec2(0.0, y))
        .order(egui::Order::Middle)
        .show(ctx, |ui| {
            card()
                .inner_margin(Margin::symmetric(16, 8))
                .show(ui, |ui| {
                    ui.label(RichText::new(text).color(SEPIA));
                });
        });
}

fn open_house_banner(ctx: &egui::Context, y: f32, project: &mut Project, group: u32) {
    egui::Area::new(egui::Id::new("open_house"))
        .anchor(egui::Align2::CENTER_TOP, vec2(0.0, y))
        .order(egui::Order::Middle)
        .show(ctx, |ui| {
            card()
                .inner_margin(Margin::symmetric(16, 8))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(format!("Editing {}", tools::house_name(project, group)))
                                .color(SEPIA),
                        );
                        if ui.button("Close house").clicked() {
                            tools::close_house(project);
                        }
                    });
                });
        });
}

/// Short notices in the bottom right that fade in and away.
fn toasts(ctx: &egui::Context, state: &HudState, bottom: f32, now: f64) {
    let view = ctx.viewport_rect();
    let mut y = bottom;
    for (k, (text, at)) in state.toasts.iter().enumerate().rev() {
        let age = now - at;
        let alpha =
            ((age / 0.25).min(1.0) * ((TOAST_SECONDS - age) / 0.6).min(1.0)).clamp(0.0, 1.0) as f32;
        let slide = (1.0 - ease((age / 0.25).min(1.0) as f32)) * 30.0;
        let response = egui::Area::new(egui::Id::new(("toast", k)))
            .pivot(egui::Align2::RIGHT_BOTTOM)
            .fixed_pos(pos2(view.right() - 16.0 + slide, y))
            .order(egui::Order::Foreground)
            .interactable(false)
            .show(ctx, |ui| {
                ui.multiply_opacity(alpha);
                card()
                    .inner_margin(Margin::symmetric(14, 8))
                    .show(ui, |ui| {
                        ui.set_max_width(320.0);
                        ui.label(RichText::new(text).color(SEPIA));
                    });
            })
            .response;
        y = response.rect.top() - 8.0;
    }
}

/// The minimap: ground, water, streets and houses from above, with the camera's focus.
/// Clicking it moves the camera there. Returns its top edge.
fn minimap(
    ctx: &egui::Context,
    state: &mut HudState,
    project: &Project,
    orbit: &mut PanOrbitCamera,
    bottom: f32,
    now: f64,
) -> f32 {
    let Some(terrain) = project.layout.terrain.as_ref() else {
        return bottom - 10.0;
    };
    const N: usize = 128;
    let stale = state
        .minimap
        .as_ref()
        .is_none_or(|(_, r)| *r != project.revision);
    if stale && now - state.minimap_at > 0.5 {
        state.minimap_at = now;
        let image = minimap_image(project, N);
        match state.minimap.as_mut() {
            Some((handle, revision)) => {
                handle.set(image, egui::TextureOptions::LINEAR);
                *revision = project.revision;
            }
            None => {
                let handle = ctx.load_texture("minimap", image, egui::TextureOptions::LINEAR);
                state.minimap = Some((handle, project.revision));
            }
        }
    }
    let Some((handle, _)) = state.minimap.as_ref() else {
        return bottom - 10.0;
    };
    let size = 172.0;
    let view = ctx.viewport_rect();
    let rect = egui::Rect::from_min_size(
        pos2(view.right() - size - 16.0, bottom - size - 16.0),
        vec2(size, size),
    );
    let rect_world = terrain.rect;
    egui::Area::new(egui::Id::new("minimap"))
        .fixed_pos(rect.min - vec2(6.0, 6.0))
        .order(egui::Order::Middle)
        .show(ctx, |ui| {
            card().inner_margin(Margin::same(6)).show(ui, |ui| {
                let (area, response) =
                    ui.allocate_exact_size(vec2(size, size), egui::Sense::click_and_drag());
                let painter = ui.painter_at(area);
                painter.image(
                    handle.id(),
                    area,
                    egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
                // World ↔ map: north (+z) is up.
                let to_map = |x: f32, z: f32| {
                    pos2(
                        area.left()
                            + (x - rect_world.x as f32) / rect_world.w as f32 * area.width(),
                        area.bottom()
                            - (z - rect_world.z as f32) / rect_world.d as f32 * area.height(),
                    )
                };
                let focus = to_map(orbit.focus.x, orbit.focus.z);
                let yaw = orbit.yaw.unwrap_or(0.0);
                let look = vec2(-yaw.sin(), yaw.cos()) * 14.0;
                painter.line_segment(
                    [focus, focus + vec2(look.x, look.y)],
                    egui::Stroke::new(2.0, TERRACOTTA),
                );
                painter.circle_filled(focus, 4.0, TERRACOTTA);
                painter.circle_stroke(focus, 6.0, egui::Stroke::new(1.0, PARCHMENT_LIGHT));
                if let Some(p) = response
                    .interact_pointer_pos()
                    .filter(|_| response.clicked() || response.dragged())
                {
                    let x = rect_world.x as f32
                        + (p.x - area.left()) / area.width() * rect_world.w as f32;
                    let z = rect_world.z as f32
                        + (area.bottom() - p.y) / area.height() * rect_world.d as f32;
                    let y = project.layout.ground_at([x as f64, z as f64]) as f32;
                    orbit.target_focus = Vec3::new(x, y + 3.0, z);
                }
                response.on_hover_text("Click to look here");
            });
        });
    rect.top() - 18.0
}

fn minimap_image(project: &Project, n: usize) -> egui::ColorImage {
    let layout = &project.layout;
    let terrain = layout.terrain.as_ref().expect("checked by the caller");
    let ground = ground_map(terrain, layout.water.as_ref());
    let r = terrain.rect;
    let mut pixels = vec![0u8; n * n * 4];
    let (lo, hi) = terrain
        .heights
        .iter()
        .fold((f32::MAX, f32::MIN), |(a, b), &h| (a.min(h), b.max(h)));
    let span = (hi - lo).max(1.0) as f64;
    let world = |px: usize, py: usize| {
        [
            r.x + (px as f64 + 0.5) / n as f64 * r.w,
            r.z + (1.0 - (py as f64 + 0.5) / n as f64) * r.d,
        ]
    };
    let put = |pixels: &mut Vec<u8>, px: usize, py: usize, c: [u8; 3]| {
        let i = (py * n + px) * 4;
        pixels[i..i + 4].copy_from_slice(&[c[0], c[1], c[2], 255]);
    };
    let (sx, sz) = terrain.spacing();
    for py in 0..n {
        for px in 0..n {
            let p = world(px, py);
            let h = terrain.height_at(p);
            let wet = layout.water.as_ref().is_some_and(|w| w.is_wet(p, h));
            let colour = if wet {
                let depth = layout
                    .water
                    .as_ref()
                    .and_then(|w| w.surface_near(p, 0.0))
                    .map_or(0.0, |s| s - h);
                let k = (depth / 6.0).clamp(0.0, 1.0) as f32;
                [
                    (70.0 - 30.0 * k) as u8,
                    (150.0 - 40.0 * k) as u8,
                    (180.0 - 30.0 * k) as u8,
                ]
            } else {
                let i = ((p[0] - r.x) / sx).round().clamp(0.0, terrain.cols as f64) as usize;
                let j = ((p[1] - r.z) / sz).round().clamp(0.0, terrain.rows as f64) as usize;
                let [cr, cg, cb] = ground[j * (terrain.cols + 1) + i].colour();
                // Light from the north-west, plus a little height tint.
                let slope = terrain.height_at([p[0] + 2.0, p[1] - 2.0]) - h;
                let shade =
                    (1.0 - slope * 0.12 + ((h - lo as f64) / span) * 0.18).clamp(0.55, 1.3) as f32;
                [
                    (cr * shade * 255.0).min(255.0) as u8,
                    (cg * shade * 255.0).min(255.0) as u8,
                    (cb * shade * 255.0).min(255.0) as u8,
                ]
            };
            put(&mut pixels, px, py, colour);
        }
    }
    // Streets, then houses, on top.
    let mut fill = |rect: &urbanitas::geometry::Rect, c: [u8; 3]| {
        let x0 = (((rect.x - r.x) / r.w) * n as f64).floor().max(0.0) as usize;
        let x1 = (((rect.x1() - r.x) / r.w) * n as f64).ceil().min(n as f64) as usize;
        let y0 = ((1.0 - (rect.z1() - r.z) / r.d) * n as f64)
            .floor()
            .max(0.0) as usize;
        let y1 = ((1.0 - (rect.z - r.z) / r.d) * n as f64)
            .ceil()
            .min(n as f64) as usize;
        for py in y0..y1 {
            for px in x0..x1 {
                put(&mut pixels, px, py, c);
            }
        }
    };
    for part in layout.on_level(0) {
        if let PartKind::Area(a) = &part.kind {
            match a.kind {
                AreaKind::Street | AreaKind::Road => fill(&a.rect, [190, 176, 150]),
                AreaKind::Piazza => fill(&a.rect, [222, 208, 180]),
                _ => {}
            }
        }
    }
    for part in layout.on_level(0) {
        if let (Some(_), PartKind::Floor(f)) = (part.group, &part.kind) {
            fill(&f.rect, [178, 84, 58]);
        }
    }
    egui::ColorImage::from_rgba_unmultiplied([n, n], &pixels)
}

#[cfg(test)]
mod tests {
    #[test]
    fn capitalises() {
        assert_eq!(super::capitalise("villa"), "Villa");
        assert_eq!(super::capitalise(""), "");
    }

    #[test]
    fn easing_runs_from_zero_to_one() {
        assert_eq!(super::ease(0.0), 0.0);
        assert_eq!(super::ease(1.0), 1.0);
        assert!(super::ease(0.5) > 0.5);
    }
}
