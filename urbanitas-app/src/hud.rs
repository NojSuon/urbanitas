//! The HUD: one toolbar at the bottom with the four steps (Land, Town, Build, Designs). Land
//! and Town open their options just above it; Build and Designs open a panel with every
//! piece in view. A small card follows the selection, and short notices appear at the top.
//! Everything else lives in the menu.

use bevy::prelude::*;
use bevy_egui::egui::{self, Align2, Color32, Margin, RichText, pos2, vec2};
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
use crate::project::{Project, Quality, Source};
use crate::scene::{MainCamera, SceneStats};
use crate::theme::{
    self, GOLD, GOLD_SOFT, PARCHMENT, PARCHMENT_DARK, PARCHMENT_LIGHT, SEPIA, SEPIA_SOFT,
    TERRACOTTA,
};
use crate::thumbs::Thumbs;
use crate::tools::{self, Category, Item, Pointer, Sculpt, Tool};

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<HudState>()
            .add_systems(EguiPrimaryContextPass, hud)
            .add_systems(Update, regenerate_when_due);
    }
}

/// The steps on the toolbar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Panel {
    Land,
    Town,
    Build,
    Designs,
}

impl Panel {
    const ALL: [Panel; 4] = [Panel::Land, Panel::Town, Panel::Build, Panel::Designs];

    fn label(self) -> &'static str {
        match self {
            Panel::Land => "Land",
            Panel::Town => "Town",
            Panel::Build => "Build",
            Panel::Designs => "Designs",
        }
    }

    /// Whether the step opens the parts panel rather than options above the toolbar.
    fn has_parts(self) -> bool {
        matches!(self, Panel::Build | Panel::Designs)
    }
}

#[derive(Resource)]
pub struct HudState {
    themed: bool,
    open_dialog: bool,
    projects: Vec<std::path::PathBuf>,
    fps: f32,
    /// The open step, if any.
    pub panel: Option<Panel>,
    /// The step's less-used settings are showing.
    more: bool,
    /// When the panel last changed, to fade it in.
    panel_at: f64,
    /// The house card's colours are showing.
    card_colours: bool,
    toast: Option<(String, f64)>,
    last_status: String,
    show_minimap: bool,
    minimap: Option<(egui::TextureHandle, u64)>,
    minimap_at: f64,
}

impl Default for HudState {
    fn default() -> Self {
        Self {
            themed: false,
            open_dialog: false,
            projects: Vec::new(),
            fps: 60.0,
            panel: Some(Panel::Land),
            more: false,
            panel_at: -1.0,
            card_colours: false,
            toast: None,
            last_status: String::new(),
            show_minimap: false,
            minimap: None,
            minimap_at: -1.0,
        }
    }
}

const DEBOUNCE: f64 = 0.2;
const PARTS_WIDTH: f32 = 312.0;
const CARD_WIDTH: f32 = 270.0;
const MARGIN: f32 = 12.0;
const TOAST_SECONDS: f64 = 3.5;

fn regenerate_when_due(mut project: ResMut<Project>, time: Res<Time>) {
    if project
        .regenerate_at
        .is_some_and(|at| time.elapsed_secs_f64() >= at)
    {
        project.regenerate_at = None;
        project.regenerate();
    }
}

/// A floating box: rounded, softly shadowed parchment.
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
        state.toast = (!project.status.is_empty()).then(|| (project.status.clone(), now));
    }

    top_buttons(&ctx, &mut project, &mut tool, &mut state, &stats);
    let bar_top = toolbar(&ctx, &mut project, &mut tool, &mut state, now);
    match state.panel {
        Some(panel) if panel.has_parts() => parts_panel(
            &ctx,
            panel,
            &mut project,
            &mut tool,
            &mut state,
            &thumbs,
            now,
        ),
        Some(panel) => options_tray(
            &ctx,
            panel,
            bar_top,
            &mut project,
            &mut tool,
            &mut state,
            now,
        ),
        None => {}
    }
    if let Ok((camera, transform)) = camera.single() {
        let left = if state.panel.is_some_and(Panel::has_parts) {
            PARTS_WIDTH + MARGIN * 2.0
        } else {
            MARGIN
        };
        floating_card(
            &ctx,
            &mut project,
            &mut tool,
            &mut state,
            &thumbs,
            camera,
            transform,
            left,
            now,
        );
    }
    let mut toast_y = MARGIN;
    if let Some(group) = project.open_group {
        open_house_banner(&ctx, &mut project, group);
        toast_y += 52.0;
    }
    toast(&ctx, &state, toast_y, now);
    if state.show_minimap {
        if let Ok(mut orbit) = orbit.single_mut() {
            minimap(&ctx, &mut state, &project, &mut orbit, now);
        }
    }
    if state.open_dialog {
        open_dialog(&ctx, &mut project, &mut state, now);
    }

    pointer.over_ui = ctx.is_pointer_over_egui() || ctx.egui_wants_pointer_input();
    pointer.ui_wants_keyboard = ctx.egui_wants_keyboard_input();
    Ok(())
}

/// Opens a step (or closes them all): any tool in hand is put away.
fn open_step(
    project: &mut Project,
    tool: &mut Tool,
    state: &mut HudState,
    panel: Option<Panel>,
    now: f64,
) {
    tool.pick(None);
    project.placing_piazza = false;
    state.panel = panel;
    state.more = false;
    state.panel_at = now;
    // Leaving the parts behind: back to the whole town, walls up.
    let building = panel.is_some_and(Panel::has_parts);
    if !building
        && project.open_group.is_none()
        && (project.level != 0 || project.walls != WallMode::Up)
    {
        project.level = 0;
        project.walls = WallMode::Up;
        project.mark_all();
    }
}

fn open_dialog(ctx: &egui::Context, project: &mut Project, state: &mut HudState, now: f64) {
    let mut open = true;
    let mut chosen = None;
    egui::Window::new(
        RichText::new("Open a project").font(egui::FontId::new(18.0, theme::heading_family())),
    )
    .open(&mut open)
    .collapsible(false)
    .resizable(false)
    .anchor(Align2::CENTER_CENTER, egui::Vec2::ZERO)
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
                state.panel = Some(Panel::Build);
                state.panel_at = now;
                project.status = format!("Opened {}", path.display());
            }
            Err(err) => project.status = format!("Could not open {}: {err}", path.display()),
        }
        open = false;
    }
    state.open_dialog = open;
}

/// Menu, Undo and Redo, in the top corner.
fn top_buttons(
    ctx: &egui::Context,
    project: &mut Project,
    tool: &mut Tool,
    state: &mut HudState,
    stats: &SceneStats,
) {
    egui::Area::new(egui::Id::new("top_buttons"))
        .fixed_pos(pos2(MARGIN, MARGIN))
        .order(egui::Order::Middle)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.menu_button("Menu", |ui| menu(ui, project, tool, state, stats));
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
            });
        });
}

fn menu(
    ui: &mut egui::Ui,
    project: &mut Project,
    tool: &mut Tool,
    state: &mut HudState,
    stats: &SceneStats,
) {
    if ui.button("New plot").clicked() {
        project.replace(files::empty_plot(
            project.site.building.style,
            project.site.building.seed,
        ));
        state.panel = Some(Panel::Build);
        tool.pick(None);
        project.status = "Empty plot: place a house, or start with a facade wall".into();
        ui.close();
    }
    if ui.button("Save").clicked() {
        project.status = match files::save(&project.layout) {
            Ok(path) => format!("Saved {}", path.display()),
            Err(err) => format!("Could not save: {err}"),
        };
        ui.close();
    }
    if ui.button("Open…").clicked() {
        state.projects = files::list_projects();
        state.open_dialog = true;
        ui.close();
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
    ui.checkbox(&mut state.show_minimap, "Minimap");
    ui.horizontal(|ui| {
        ui.label("Quality");
        for (quality, label) in [(Quality::Low, "Low"), (Quality::High, "High")] {
            if ui
                .selectable_label(project.quality == quality, label)
                .clicked()
                && project.quality != quality
            {
                project.quality = quality;
                project.mark_all();
            }
        }
    });
    ui.menu_button("Keys", |ui| {
        for line in [
            "Right-drag orbit · middle-drag pan · scroll zoom",
            "Click a house · Shift+click a single part",
            "R turn · Q/E fine turn · Delete remove",
            "PgUp/PgDn change floor · Alt place freely",
            "Ctrl+Z undo · Ctrl+Y redo · Esc done",
        ] {
            note(ui, line);
        }
    });
    ui.separator();
    note(
        ui,
        &format!(
            "{:.0} fps · {} k triangles · rebuilt in {:.0} ms",
            state.fps,
            stats.triangles() / 1000,
            stats.build_ms
        ),
    );
}

/// The four steps, at the bottom. Returns the toolbar's top edge.
fn toolbar(
    ctx: &egui::Context,
    project: &mut Project,
    tool: &mut Tool,
    state: &mut HudState,
    now: f64,
) -> f32 {
    egui::Area::new(egui::Id::new("toolbar"))
        .anchor(Align2::CENTER_BOTTOM, vec2(0.0, -14.0))
        .order(egui::Order::Middle)
        .show(ctx, |ui| {
            card().inner_margin(Margin::same(5)).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing = vec2(4.0, 0.0);
                    for panel in Panel::ALL {
                        let active = state.panel == Some(panel);
                        let (fill, ink) = if active {
                            (TERRACOTTA, PARCHMENT_LIGHT)
                        } else {
                            (Color32::TRANSPARENT, SEPIA)
                        };
                        let button =
                            egui::Button::new(RichText::new(panel.label()).size(16.0).color(ink))
                                .fill(fill)
                                .stroke(egui::Stroke::NONE)
                                .corner_radius(7)
                                .min_size(vec2(96.0, 38.0));
                        if ui.add(button).clicked() {
                            let next = if active { None } else { Some(panel) };
                            open_step(project, tool, state, next, now);
                        }
                    }
                });
            });
        })
        .response
        .rect
        .top()
}

// ---------------------------------------------------------------------------------------
// Small building blocks.

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

fn caption(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).small().color(SEPIA_SOFT));
}

/// A button that stays lit while its option is on.
fn choice(ui: &mut egui::Ui, label: &str, on: bool) -> bool {
    let (fill, ink) = if on {
        (TERRACOTTA, PARCHMENT_LIGHT)
    } else {
        (PARCHMENT_LIGHT, SEPIA)
    };
    ui.add(egui::Button::new(RichText::new(label).color(ink)).fill(fill))
        .clicked()
}

fn primary(ui: &mut egui::Ui, label: &str) -> bool {
    let button = egui::Button::new(RichText::new(label).size(15.0).color(PARCHMENT_LIGHT))
        .fill(TERRACOTTA)
        .corner_radius(6)
        .min_size(vec2(0.0, 32.0));
    ui.add(button).clicked()
}

fn more_link(ui: &mut egui::Ui, more: &mut bool) {
    let label = if *more { "Less" } else { "More" };
    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
    if ui.link(RichText::new(label).color(SEPIA_SOFT)).clicked() {
        *more = !*more;
    }
}

fn reroll(seed: &mut u64) {
    *seed = seed
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407)
        % 1_000_000;
}

fn lerp_colour(a: Color32, b: Color32, t: f32) -> Color32 {
    let mix = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t) as u8;
    Color32::from_rgb(mix(a.r(), b.r()), mix(a.g(), b.g()), mix(a.b(), b.b()))
}

/// Fades a panel in just after it opens.
fn fade_in(ui: &mut egui::Ui, panel_at: f64, now: f64) {
    ui.multiply_opacity((((now - panel_at) / 0.14).clamp(0.0, 1.0)) as f32);
}

// ---------------------------------------------------------------------------------------
// Land and Town: options just above the toolbar.

fn options_tray(
    ctx: &egui::Context,
    panel: Panel,
    bar_top: f32,
    project: &mut Project,
    tool: &mut Tool,
    state: &mut HudState,
    now: f64,
) {
    let view = ctx.viewport_rect();
    egui::Area::new(egui::Id::new(("tray", panel.label())))
        .pivot(Align2::CENTER_BOTTOM)
        .fixed_pos(pos2(view.center().x, bar_top - 8.0))
        .order(egui::Order::Middle)
        .show(ctx, |ui| {
            fade_in(ui, state.panel_at, now);
            ui.set_max_width(view.width() - 2.0 * MARGIN);
            card().show(ui, |ui| match panel {
                Panel::Town => town_options(ui, project, state, now),
                _ => land_options(ui, project, tool, state, now),
            });
        });
}

fn land_options(
    ui: &mut egui::Ui,
    project: &mut Project,
    tool: &mut Tool,
    state: &mut HudState,
    now: f64,
) {
    let before = project.landscape;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(18.0, 8.0);
        ui.vertical(|ui| {
            caption(ui, "Terrain");
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                for relief in Relief::ALL {
                    if choice(ui, relief.label(), project.landscape.relief == relief) {
                        project.landscape.relief = relief;
                    }
                }
            });
        });
        ui.vertical(|ui| {
            caption(ui, "Water");
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                for (water, label) in [
                    (WaterBody::None, "None"),
                    (WaterBody::Beach, "Beach"),
                    (WaterBody::Cliffs, "Cliffs"),
                    (WaterBody::BeachAndCliffs, "Both"),
                    (WaterBody::Lake, "Lake"),
                    (WaterBody::River, "River"),
                ] {
                    if choice(ui, label, project.landscape.water == water) {
                        project.landscape.water = water;
                    }
                }
            });
        });
        ui.vertical(|ui| {
            caption(ui, "Paint the ground");
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                for sculpt in Sculpt::ALL {
                    let item = Item::Terrain(sculpt);
                    if choice(ui, item.label(), tool.item == Some(item)) {
                        if tool.item == Some(item) {
                            tool.pick(None);
                        } else {
                            tool.pick(Some(item));
                            project.selected = None;
                            project.selected_group = None;
                            project.status = format!(
                                "{}: hold the left button and paint. Houses are left alone.",
                                item.label()
                            );
                        }
                    }
                }
            });
        });
        if matches!(tool.item, Some(Item::Terrain(_))) {
            ui.vertical(|ui| {
                caption(ui, "Brush size");
                ui.add(egui::Slider::new(&mut tool.brush_radius, 1.0..=30.0).suffix(" m"));
            });
        }
        ui.vertical(|ui| {
            ui.add_space(16.0);
            more_link(ui, &mut state.more);
        });
    });
    if state.more {
        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(18.0, 8.0);
            ui.vertical(|ui| {
                caption(ui, "Map size");
                ui.add(
                    egui::Slider::new(&mut project.landscape.size, 160.0..=640.0)
                        .step_by(20.0)
                        .suffix(" m"),
                );
            });
            ui.vertical(|ui| {
                caption(ui, "Shape of the land");
                if ui.button("Try another").clicked() {
                    reroll(&mut project.landscape.seed);
                }
            });
            ui.vertical(|ui| {
                caption(ui, "Brush strength");
                ui.add(egui::Slider::new(&mut tool.brush_strength, 0.1..=2.0));
            });
            if project.layout.water.is_some() {
                ui.vertical(|ui| {
                    caption(ui, "Water level");
                    ui.horizontal(|ui| {
                        for (label, rise) in [("Lower", -0.5), ("Raise", 0.5)] {
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
                });
            }
            ui.vertical(|ui| {
                caption(ui, "Start from a preset instead");
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    for (context, label) in [
                        (Context::Single, "One building"),
                        (Context::Urban, "Street of palazzi"),
                        (Context::Suburban, "Villas"),
                        (Context::Rural, "Farm"),
                    ] {
                        let on = project.source == Source::Site && project.site.context == context;
                        if choice(ui, label, on) {
                            project.source = Source::Site;
                            project.site.context = context;
                            project.plaza = None;
                            tool.pick(None);
                            project.regenerate();
                        }
                    }
                });
            });
        });
        if project.source == Source::Site {
            ui.add_space(8.0);
            site_settings(ui, project, now);
        }
    }
    if project.landscape != before {
        project.source = Source::Landscape;
        project.regenerate_at = Some(now + DEBOUNCE);
    }
}

fn site_settings(ui: &mut egui::Ui, project: &mut Project, now: f64) {
    let before = project.site.clone();
    let site = &mut project.site;
    ui.horizontal(|ui| {
        if site.context != Context::Single {
            ui.label("Buildings");
            let max = if site.context == Context::Urban {
                14
            } else {
                6
            };
            ui.add(egui::Slider::new(&mut site.buildings, 1..=max));
        }
        ui.label("Hills");
        ui.add(
            egui::Slider::new(&mut site.relief, 0.0..=10.0)
                .step_by(0.5)
                .suffix(" m"),
        );
    });
    egui::CollapsingHeader::new("Building")
        .id_salt("site_building")
        .show(ui, |ui| building_grid(ui, &mut site.building));
    if project.site != before {
        project.regenerate_at = Some(now + DEBOUNCE);
    }
}

fn town_options(ui: &mut egui::Ui, project: &mut Project, state: &mut HudState, now: f64) {
    let before = project.town;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(18.0, 8.0);
        ui.vertical(|ui| {
            caption(ui, "Houses");
            ui.add(egui::Slider::new(&mut project.town.houses, 4..=80));
        });
        let label = match (project.placing_piazza, project.plaza) {
            (true, _) => "Click the map…",
            (false, None) => "Place the piazza",
            (false, Some(_)) => "Move the piazza",
        };
        ui.vertical(|ui| {
            ui.add_space(10.0);
            if primary(ui, label) {
                project.placing_piazza = !project.placing_piazza;
                project.status = if project.placing_piazza {
                    "Click where the piazza goes: the town grows around it".into()
                } else {
                    "Piazza placing cancelled".into()
                };
            }
        });
        ui.vertical(|ui| {
            ui.add_space(16.0);
            more_link(ui, &mut state.more);
        });
    });
    if state.more {
        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(18.0, 8.0);
            ui.vertical(|ui| {
                caption(ui, "Style");
                egui::ComboBox::from_id_salt("town_style")
                    .selected_text(project.town.style.label())
                    .show_ui(ui, |ui| {
                        for style in ArchitectureStyle::ALL {
                            ui.selectable_value(&mut project.town.style, style, style.label());
                        }
                    });
            });
            ui.vertical(|ui| {
                caption(ui, "Density");
                ui.add(egui::Slider::new(&mut project.town.density, 0.0..=1.0).show_value(false));
            });
            ui.vertical(|ui| {
                caption(ui, "Houses from");
                egui::ComboBox::from_id_salt("mix")
                    .selected_text(project.town.mix.label())
                    .show_ui(ui, |ui| {
                        for mix in DesignMix::ALL {
                            ui.selectable_value(&mut project.town.mix, mix, mix.label());
                        }
                    });
            });
            ui.vertical(|ui| {
                caption(ui, "This town");
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
                        .add_enabled(has_town, egui::Button::new("Clear"))
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
            });
        });
        if project.town.mix != DesignMix::Generated && project.designs.is_empty() {
            note(
                ui,
                "You have no saved designs yet: save a house from its card.",
            );
        }
        egui::CollapsingHeader::new("Town colours")
            .id_salt("town_colours")
            .show(ui, |ui| palette_editor(ui, project, None));
    }
    if project.town != before && project.plaza.is_some() {
        project.regenerate_at = Some(now + DEBOUNCE);
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
            ui.add_sized([58.0, 20.0], egui::Label::new(label));
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

// ---------------------------------------------------------------------------------------
// Build and Designs: a panel with every piece in view.

fn parts_panel(
    ctx: &egui::Context,
    panel: Panel,
    project: &mut Project,
    tool: &mut Tool,
    state: &mut HudState,
    thumbs: &Thumbs,
    now: f64,
) {
    let view = ctx.viewport_rect();
    let top = MARGIN + 44.0;
    let height = (view.height() - top - MARGIN).max(160.0);
    egui::Area::new(egui::Id::new("parts_panel"))
        .fixed_pos(pos2(MARGIN, top))
        .order(egui::Order::Middle)
        .show(ctx, |ui| {
            fade_in(ui, state.panel_at, now);
            card().show(ui, |ui| {
                ui.set_width(PARTS_WIDTH - 24.0);
                ui.set_min_height(height - 24.0);
                egui::ScrollArea::vertical()
                    .max_height(height - 24.0)
                    .show(ui, |ui| match panel {
                        Panel::Designs => designs_list(ui, project, tool, thumbs),
                        _ => build_list(ui, project, tool, state, thumbs),
                    });
            });
        });
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
    let painter = ui.painter_at(rect);
    let fill = if selected {
        Color32::from_rgb(236, 205, 160)
    } else {
        lerp_colour(PARCHMENT_DARK, Color32::from_rgb(240, 225, 196), hover)
    };
    painter.rect_filled(rect, 6.0, fill);
    let border = if selected {
        egui::Stroke::new(2.0, TERRACOTTA)
    } else {
        egui::Stroke::new(1.0, GOLD_SOFT)
    };
    painter.rect_stroke(rect, 6.0, border, egui::StrokeKind::Inside);
    let image = egui::Rect::from_min_size(rect.min + vec2(6.0, 5.0), vec2(76.0, 76.0));
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
        pos2(rect.center().x, rect.max.y - 11.0),
        Align2::CENTER_CENTER,
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

fn build_list(
    ui: &mut egui::Ui,
    project: &mut Project,
    tool: &mut Tool,
    state: &mut HudState,
    thumbs: &Thumbs,
) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
        for category in Category::ALL {
            if choice(ui, category.label(), tool.category == category) && tool.category != category
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
    ui.add_space(8.0);
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
    ui.add_space(8.0);
    match tool.item {
        Some(Item::Building) => {
            egui::CollapsingHeader::new("New house settings")
                .id_salt("new_house")
                .default_open(true)
                .show(ui, |ui| building_grid(ui, &mut project.site.building));
        }
        Some(Item::Street) => note(
            ui,
            "Houses take the Town step's style, density and design mix.",
        ),
        _ => {}
    }
    more_link(ui, &mut state.more);
    if state.more {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label("Floor");
            if ui.button("‹").clicked() {
                tools::change_level(project, project.level.saturating_sub(1));
            }
            ui.label(RichText::new(urbanitas::export::svg::level_name(project.level)).color(SEPIA));
            if ui.button("›").clicked() {
                tools::change_level(project, (project.level + 1).min(project.top_level() + 1));
            }
        });
        ui.horizontal(|ui| {
            ui.label("Walls");
            for (mode, label) in [
                (WallMode::Up, "Up"),
                (WallMode::Cutaway, "Cutaway"),
                (WallMode::Down, "Down"),
            ] {
                if choice(ui, label, project.walls == mode) && project.walls != mode {
                    project.walls = mode;
                    let level = project.level;
                    project.mark(&[level]);
                }
            }
        });
    }
}

fn designs_list(ui: &mut egui::Ui, project: &mut Project, tool: &mut Tool, thumbs: &Thumbs) {
    match project.selected_group.or(project.open_group) {
        Some(g) => {
            let name = tools::house_name(project, g);
            if primary(ui, &format!("Save {name} as a design")) {
                save_design(project, g, &name);
            }
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
                    &format!("A copy of {} ({copies} here).", project.designs[index].name),
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
        None => note(ui, "Click a house in the view to save it as a design."),
    }
    ui.add_space(10.0);
    if project.designs.is_empty() {
        note(ui, "Saved designs appear here.");
        return;
    }
    note(
        ui,
        "Click a design, then click the map to place a copy (R turns it). Right-click to delete.",
    );
    ui.add_space(4.0);
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
        Ok(_) => {
            project.edit(&[], None, |l| {
                let module = l.module_mut(group);
                module.design = Some(id);
                module.name = name.to_string();
            });
            project.designs.push(design);
            project.designs.sort_by(|a, b| a.name.cmp(&b.name));
            project.status = format!("Design saved: {name}");
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

// ---------------------------------------------------------------------------------------
// The card that follows the selection.

/// The corners of what's selected, in the world: a house's plot up to its roof, or a part.
fn selection_corners(project: &Project) -> Vec<Vec3> {
    let layout = &project.layout;
    let (rect, low, high) = if let Some(g) = project.selected_group {
        let Some(lot) = layout
            .module(g)
            .and_then(|m| m.lot)
            .or_else(|| tools::group_bounds(layout, g))
        else {
            return Vec::new();
        };
        let ground = layout
            .group_parts(g)
            .filter(|p| p.level == 0)
            .map(|p| p.ground)
            .fold(f64::MAX, f64::min);
        let ground = if ground == f64::MAX { 0.0 } else { ground };
        let top = layout.elevation(project.group_levels(g).last().copied().unwrap_or(0));
        (lot, ground, ground + top)
    } else if let Some(part) = project.selected.and_then(|id| layout.get(id)) {
        let base = layout.base(part);
        (part.footprint(), base, base + 2.0)
    } else {
        return Vec::new();
    };
    let mut corners = Vec::with_capacity(8);
    for y in [low, high] {
        for (x, z) in [
            (rect.x, rect.z),
            (rect.x1(), rect.z),
            (rect.x, rect.z1()),
            (rect.x1(), rect.z1()),
        ] {
            corners.push(Vec3::new(x as f32, y as f32, z as f32));
        }
    }
    corners
}

#[allow(clippy::too_many_arguments)]
fn floating_card(
    ctx: &egui::Context,
    project: &mut Project,
    tool: &mut Tool,
    state: &mut HudState,
    thumbs: &Thumbs,
    camera: &Camera,
    transform: &GlobalTransform,
    left_edge: f32,
    now: f64,
) {
    let corners = selection_corners(project);
    if corners.is_empty() {
        return;
    }
    let view = ctx.viewport_rect();
    let on_screen: Vec<egui::Pos2> = corners
        .iter()
        .filter_map(|c| camera.world_to_viewport(transform, *c).ok())
        .map(|p| pos2(p.x, p.y))
        .collect();
    let seen = on_screen.iter().any(|p| view.contains(*p));
    // Beside the selection, on whichever side has room; in the top corner if it's out of view.
    let pos = if seen {
        let left = on_screen.iter().map(|p| p.x).fold(f32::MAX, f32::min);
        let right = on_screen.iter().map(|p| p.x).fold(f32::MIN, f32::max);
        let middle = on_screen.iter().map(|p| p.y).sum::<f32>() / on_screen.len() as f32;
        let last = view.right() - CARD_WIDTH - MARGIN;
        let x = if right + 16.0 <= last {
            right + 16.0
        } else {
            left - CARD_WIDTH - 16.0
        };
        pos2(
            x.clamp(left_edge, last.max(left_edge)),
            (middle - 90.0).clamp(MARGIN, (view.bottom() - 340.0).max(MARGIN)),
        )
    } else {
        pos2(view.right() - CARD_WIDTH - MARGIN, 60.0)
    };
    egui::Area::new(egui::Id::new("selection_card"))
        .fixed_pos(pos)
        .order(egui::Order::Middle)
        .show(ctx, |ui| {
            card().inner_margin(Margin::same(10)).show(ui, |ui| {
                ui.set_width(CARD_WIDTH - 20.0);
                if let Some(g) = project.selected_group {
                    house_card(ui, project, tool, state, g, now);
                } else if let Some(part) = project
                    .selected
                    .and_then(|id| project.layout.get(id).cloned())
                {
                    part_card(ui, project, thumbs, &part);
                }
            });
        });
}

/// The card's close button. Returns whether it was clicked.
fn card_close(ui: &mut egui::Ui) -> bool {
    ui.small_button("×").on_hover_text("Deselect").clicked()
}

fn house_card(
    ui: &mut egui::Ui,
    project: &mut Project,
    tool: &mut Tool,
    state: &mut HudState,
    group: u32,
    now: f64,
) {
    let mut name = tools::house_name(project, group);
    ui.horizontal(|ui| {
        if ui
            .add(
                egui::TextEdit::singleline(&mut name)
                    .desired_width(CARD_WIDTH - 56.0)
                    .font(egui::FontId::new(16.0, theme::heading_family())),
            )
            .changed()
        {
            let key = format!("name-{group}");
            project.edit(&[], Some(&key), |l| l.module_mut(group).name = name.clone());
        }
        if card_close(ui) {
            project.selected_group = None;
            state.card_colours = false;
        }
    });
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
    note(ui, &format!("{storeys} storeys{copy}"));
    ui.add_space(2.0);
    let half = vec2((CARD_WIDTH - 26.0) / 2.0, 28.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        if ui.add_sized(half, egui::Button::new("Turn")).clicked() {
            tools::turn_group(project, group);
        }
        if ui
            .add_sized(half, egui::Button::new("Edit inside"))
            .clicked()
        {
            tools::open_house(project, group);
            state.panel = Some(Panel::Build);
            state.panel_at = now;
            state.more = true;
            tool.pick(None);
        }
    });
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        if ui
            .add_sized(half, egui::Button::new("Save design"))
            .clicked()
        {
            let name = tools::house_name(project, group);
            save_design(project, group, &name);
            tool.pick(None);
            project.placing_piazza = false;
            state.panel = Some(Panel::Designs);
            state.panel_at = now;
        }
        if ui
            .add_sized(
                half,
                egui::Button::new(RichText::new("Delete").color(TERRACOTTA)),
            )
            .clicked()
        {
            tools::delete_group(project, group);
        }
    });
    if ui
        .link(RichText::new("Colours").color(SEPIA_SOFT))
        .clicked()
    {
        state.card_colours = !state.card_colours;
    }
    if state.card_colours && project.selected_group == Some(group) {
        palette_editor(ui, project, Some(group));
    }
}

fn part_card(ui: &mut egui::Ui, project: &mut Project, thumbs: &Thumbs, part: &Part) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(part.label()).strong().color(GOLD));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if card_close(ui) {
                project.selected = None;
            }
        });
    });
    if let Some(id) = thumbs.preview() {
        let width = CARD_WIDTH - 20.0;
        ui.add(
            egui::Image::new(egui::load::SizedTexture::new(
                id,
                [width, width * 168.0 / 264.0],
            ))
            .corner_radius(6),
        );
    }
    inspector(ui, project, part);
}

fn inspector(ui: &mut egui::Ui, project: &mut Project, part: &Part) {
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
        if ui.button("Turn").clicked() {
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
// Notices and the minimap.

fn open_house_banner(ctx: &egui::Context, project: &mut Project, group: u32) {
    egui::Area::new(egui::Id::new("open_house"))
        .anchor(Align2::CENTER_TOP, vec2(0.0, MARGIN))
        .order(egui::Order::Middle)
        .show(ctx, |ui| {
            card()
                .inner_margin(Margin::symmetric(14, 6))
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

/// The latest notice, at the top, fading away.
fn toast(ctx: &egui::Context, state: &HudState, y: f32, now: f64) {
    let Some((text, at)) = &state.toast else {
        return;
    };
    let age = now - at;
    if age >= TOAST_SECONDS {
        return;
    }
    let alpha = ((age / 0.15).min(1.0) * ((TOAST_SECONDS - age) / 0.5).min(1.0)) as f32;
    egui::Area::new(egui::Id::new("toast"))
        .anchor(Align2::CENTER_TOP, vec2(0.0, y))
        .order(egui::Order::Foreground)
        .interactable(false)
        .show(ctx, |ui| {
            ui.multiply_opacity(alpha);
            card()
                .inner_margin(Margin::symmetric(14, 8))
                .show(ui, |ui| {
                    ui.label(RichText::new(text).color(SEPIA));
                });
        });
}

/// The minimap, in the top corner: ground, water, streets and houses from above, with the
/// camera's focus. Clicking it moves the camera there.
fn minimap(
    ctx: &egui::Context,
    state: &mut HudState,
    project: &Project,
    orbit: &mut PanOrbitCamera,
    now: f64,
) {
    let Some(terrain) = project.layout.terrain.as_ref() else {
        return;
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
        return;
    };
    let size = 150.0;
    let rect_world = terrain.rect;
    egui::Area::new(egui::Id::new("minimap"))
        .anchor(Align2::RIGHT_TOP, vec2(-MARGIN, MARGIN))
        .order(egui::Order::Middle)
        .show(ctx, |ui| {
            card().inner_margin(Margin::same(5)).show(ui, |ui| {
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
                let focus = pos2(
                    area.left()
                        + (orbit.focus.x - rect_world.x as f32) / rect_world.w as f32
                            * area.width(),
                    area.bottom()
                        - (orbit.focus.z - rect_world.z as f32) / rect_world.d as f32
                            * area.height(),
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
    fn only_build_and_designs_open_the_parts_panel() {
        use super::Panel;
        assert!(Panel::Build.has_parts() && Panel::Designs.has_parts());
        assert!(!Panel::Land.has_parts() && !Panel::Town.has_parts());
    }
}
