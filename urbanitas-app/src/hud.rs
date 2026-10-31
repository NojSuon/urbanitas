//! The HUD: a top bar, a left panel for the current mode and a status bar.

use bevy::prelude::*;
use bevy_egui::egui::{self, Align, Color32, Layout as UiLayout, Margin, RichText};
use bevy_egui::{EguiContexts, EguiPrimaryContextPass};
use urbanitas::mesh::build::WallMode;
use urbanitas::model::Roof;
use urbanitas::parts::{Part, PartKind, RoofPart};
use urbanitas::style::{Color as Rgb, Palette, Swatches};
use urbanitas::{ArchitectureStyle, BuildingType, Context, Garden, Shape};

use crate::files::{self, Export};
use crate::project::{Mode, Project, Quality};
use crate::scene::SceneStats;
use crate::theme::{self, GOLD, PARCHMENT, PARCHMENT_DARK, SEPIA_SOFT, TERRACOTTA};
use crate::thumbs::Thumbs;
use crate::tools::{self, Category, Item, Placement, Pointer, Tool};

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

const DEBOUNCE: f64 = 0.2;

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
    thumbs: Res<Thumbs>,
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
        .exact_size(318.0)
        .frame(panel_frame())
        .show(&mut root, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| match project.mode {
                Mode::Generate => generate_panel(ui, &mut project, now),
                Mode::Build => build_panel(ui, &mut project, &mut tool, &thumbs),
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
                project.selected_group = None;
                project.mark_all();
            }
        }
        ui.separator();
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
                (Mode::Build, None) => "Click: select · Shift+click: whole building · drag: move · R rotate · Del delete · PgUp/PgDn level".into(),
            };
            ui.label(RichText::new(hint).small().color(SEPIA_SOFT));
        });
    });
}

fn capitalise(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().collect::<String>() + chars.as_str())
        .unwrap_or_default()
}

fn generate_panel(ui: &mut egui::Ui, project: &mut Project, now: f64) {
    let before = project.site.clone();

    theme::title(ui, "The site");
    theme::ornament(ui);
    let site = &mut project.site;
    egui::Grid::new("site")
        .num_columns(2)
        .spacing([10.0, 9.0])
        .show(ui, |ui| {
            ui.label("Context");
            egui::ComboBox::from_id_salt("context")
                .selected_text(site.context.label())
                .show_ui(ui, |ui| {
                    for context in Context::ALL {
                        ui.selectable_value(&mut site.context, context, context.label());
                    }
                });
            ui.end_row();
            if matches!(
                site.context,
                Context::Urban | Context::Suburban | Context::Rural
            ) {
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

    ui.add_space(8.0);
    theme::title(ui, "The building");
    theme::ornament(ui);
    let p = &mut project.site.building;
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
                    p.seed = p
                        .seed
                        .wrapping_mul(6364136223846793005)
                        .wrapping_add(1442695040888963407)
                        % 1_000_000;
                }
            });
            ui.end_row();
        });

    if project.site != before {
        project.regenerate_at = Some(now + DEBOUNCE);
    }
    if let Some(err) = &project.error {
        ui.add_space(4.0);
        ui.label(RichText::new(err).color(TERRACOTTA));
    }

    if project.site.context == Context::Single {
        ui.add_space(8.0);
        theme::title(ui, "Colours");
        theme::ornament(ui);
        palette_editor(ui, project, Some(0));
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
        "{} parts · {} buildings · {rooms} room floors",
        layout.parts.len(),
        layout.palettes.len().max(1)
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

fn swatch(ui: &mut egui::Ui, colour: Rgb, selected: bool, name: &str) -> bool {
    let fill = Color32::from_rgb(colour.0, colour.1, colour.2);
    let stroke = if selected {
        egui::Stroke::new(2.0, TERRACOTTA)
    } else {
        egui::Stroke::new(1.0, theme::GOLD_SOFT)
    };
    ui.add(
        egui::Button::new("")
            .fill(fill)
            .stroke(stroke)
            .min_size(egui::vec2(22.0, 22.0)),
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
        ui.horizontal(|ui| {
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

fn picture_tile(
    ui: &mut egui::Ui,
    texture: Option<egui::TextureId>,
    label: &str,
    selected: bool,
) -> bool {
    let size = egui::vec2(88.0, 104.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    let painter = ui.painter_at(rect);
    let fill = if selected {
        Color32::from_rgb(236, 205, 160)
    } else if response.hovered() {
        Color32::from_rgb(240, 225, 196)
    } else {
        PARCHMENT_DARK
    };
    painter.rect_filled(rect, 3.0, fill);
    let border = if selected {
        egui::Stroke::new(2.0, TERRACOTTA)
    } else {
        egui::Stroke::new(1.0, theme::GOLD_SOFT)
    };
    painter.rect_stroke(rect, 3.0, border, egui::StrokeKind::Inside);
    let image = egui::Rect::from_min_size(rect.min + egui::vec2(6.0, 5.0), egui::vec2(76.0, 76.0));
    match texture {
        Some(id) => {
            painter.image(
                id,
                image,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
        None => {
            painter.rect_filled(image, 2.0, Color32::from_rgb(233, 224, 204));
        }
    }
    painter.text(
        egui::pos2(rect.center().x, rect.max.y - 11.0),
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::proportional(11.5),
        theme::SEPIA,
    );
    response.on_hover_text(label).clicked()
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
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
        for item in Item::catalogue(tool.category) {
            if picture_tile(ui, thumbs.item(item), item.label(), tool.item == Some(item)) {
                tool.pick(Some(item));
                project.selected = None;
                project.selected_group = None;
                project.status = item.hint().into();
            }
        }
    });
    if tool.item.is_some_and(|i| i.placement() == Placement::Brush) {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label("Size");
            ui.add(egui::Slider::new(&mut tool.brush_radius, 1.0..=20.0).suffix(" m"));
        });
        ui.horizontal(|ui| {
            ui.label("Strength");
            ui.add(egui::Slider::new(&mut tool.brush_strength, 0.1..=2.0));
        });
    } else if tool.item == Some(Item::Building) {
        ui.label(
            RichText::new("Uses the style, size and plan from Generate.")
                .small()
                .color(SEPIA_SOFT),
        );
    } else {
        ui.label(
            RichText::new("Snaps to a 0.5 m grid · hold Alt to place freely")
                .small()
                .color(SEPIA_SOFT),
        );
    }

    ui.add_space(8.0);
    theme::title(ui, "Inspector");
    theme::ornament(ui);
    if project.selected.is_some() || project.selected_group.is_some() {
        if let Some(id) = thumbs.preview() {
            ui.add(
                egui::Image::new(egui::load::SizedTexture::new(id, [264.0, 168.0]))
                    .corner_radius(3),
            );
        }
    }
    if let Some(group) = project.selected_group {
        ui.label(
            RichText::new(format!("Building {}", group + 1))
                .strong()
                .color(GOLD),
        );
        let parts = project.layout.group_parts(group).count();
        let floors = project
            .layout
            .group_parts(group)
            .filter(|p| matches!(p.kind, PartKind::Wall(_)))
            .map(|p| p.level)
            .max()
            .map_or(0, |l| l + 1);
        ui.label(
            RichText::new(format!("{parts} parts · {floors} storeys"))
                .small()
                .color(SEPIA_SOFT),
        );
        ui.add_space(4.0);
        palette_editor(ui, project, Some(group));
        ui.add_space(4.0);
        if ui
            .button(RichText::new("Delete building").color(TERRACOTTA))
            .clicked()
        {
            tools::delete_group(project, group);
        }
        return;
    }
    match project
        .selected
        .and_then(|id| project.layout.get(id).cloned())
    {
        Some(part) => inspector(ui, project, &part),
        None => {
            ui.label(
                RichText::new("Nothing selected. Shift+click selects a whole building.")
                    .color(SEPIA_SOFT),
            );
        }
    }
}

fn inspector(ui: &mut egui::Ui, project: &mut Project, part: &Part) {
    ui.label(RichText::new(part.label()).strong().color(GOLD));
    let building = part
        .group
        .map_or("no building".to_string(), |g| format!("building {}", g + 1));
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
        if let Some(g) = part.group {
            if ui.button("Whole building").clicked() {
                project.selected = None;
                project.selected_group = Some(g);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn capitalises() {
        assert_eq!(super::capitalise("villa"), "Villa");
        assert_eq!(super::capitalise(""), "");
    }
}
