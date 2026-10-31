//! The Florentine look of the HUD: parchment panels, sepia ink, terracotta and gold.

use std::sync::Arc;

use bevy_egui::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke, TextStyle,
    Vec2,
};

pub const PARCHMENT: Color32 = Color32::from_rgb(243, 233, 210);
pub const PARCHMENT_LIGHT: Color32 = Color32::from_rgb(250, 244, 230);
pub const PARCHMENT_DARK: Color32 = Color32::from_rgb(232, 219, 190);
pub const SEPIA: Color32 = Color32::from_rgb(59, 42, 26);
pub const SEPIA_SOFT: Color32 = Color32::from_rgb(122, 101, 76);
pub const TERRACOTTA: Color32 = Color32::from_rgb(178, 84, 58);
pub const GOLD: Color32 = Color32::from_rgb(196, 155, 48);
pub const GOLD_SOFT: Color32 = Color32::from_rgb(214, 190, 140);

/// A serif face from the system, if one is installed: Georgia on Windows and macOS,
/// DejaVu Serif on Linux. Without one egui's default font is used.
fn system_serif() -> Option<(Vec<u8>, Option<Vec<u8>>)> {
    let pairs = [
        (
            "C:/Windows/Fonts/georgia.ttf",
            "C:/Windows/Fonts/georgiab.ttf",
        ),
        (
            "/System/Library/Fonts/Supplemental/Georgia.ttf",
            "/System/Library/Fonts/Supplemental/Georgia Bold.ttf",
        ),
        (
            "/Library/Fonts/Georgia.ttf",
            "/Library/Fonts/Georgia Bold.ttf",
        ),
        (
            "/usr/share/fonts/truetype/dejavu/DejaVuSerif.ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSerif-Bold.ttf",
        ),
    ];
    pairs.iter().find_map(|(regular, bold)| {
        let regular = std::fs::read(regular).ok()?;
        Some((regular, std::fs::read(bold).ok()))
    })
}

/// The family used for titles.
pub fn heading_family() -> FontFamily {
    FontFamily::Name("heading".into())
}

pub fn apply(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let mut heading = Vec::new();
    if let Some((regular, bold)) = system_serif() {
        fonts
            .font_data
            .insert("serif".into(), Arc::new(FontData::from_owned(regular)));
        fonts
            .families
            .entry(FontFamily::Proportional)
            .or_default()
            .insert(0, "serif".into());
        if let Some(bold) = bold {
            fonts
                .font_data
                .insert("serif-bold".into(), Arc::new(FontData::from_owned(bold)));
            heading.push("serif-bold".to_string());
        }
        heading.push("serif".to_string());
    }
    heading.extend(
        fonts
            .families
            .get(&FontFamily::Proportional)
            .cloned()
            .unwrap_or_default(),
    );
    fonts.families.insert(heading_family(), heading);
    ctx.set_fonts(fonts);

    let mut visuals = egui::Visuals::light();
    visuals.panel_fill = PARCHMENT;
    visuals.window_fill = PARCHMENT_LIGHT;
    visuals.window_stroke = Stroke::new(1.0, GOLD);
    visuals.window_corner_radius = CornerRadius::same(4);
    visuals.menu_corner_radius = CornerRadius::same(3);
    visuals.override_text_color = Some(SEPIA);
    visuals.extreme_bg_color = PARCHMENT_LIGHT;
    visuals.faint_bg_color = PARCHMENT_DARK;
    visuals.hyperlink_color = TERRACOTTA;
    visuals.selection.bg_fill = TERRACOTTA;
    visuals.selection.stroke = Stroke::new(1.0, PARCHMENT_LIGHT);
    visuals.slider_trailing_fill = true;
    visuals.error_fg_color = TERRACOTTA;
    visuals.warn_fg_color = GOLD;

    let w = &mut visuals.widgets;
    let radius = CornerRadius::same(3);
    w.noninteractive.bg_fill = PARCHMENT;
    w.noninteractive.weak_bg_fill = PARCHMENT;
    w.noninteractive.bg_stroke = Stroke::new(1.0, GOLD_SOFT);
    w.noninteractive.fg_stroke = Stroke::new(1.0, SEPIA);
    w.inactive.bg_fill = PARCHMENT_DARK;
    w.inactive.weak_bg_fill = PARCHMENT_DARK;
    w.inactive.bg_stroke = Stroke::new(1.0, GOLD_SOFT);
    w.inactive.fg_stroke = Stroke::new(1.0, SEPIA);
    w.hovered.bg_fill = Color32::from_rgb(238, 217, 176);
    w.hovered.weak_bg_fill = Color32::from_rgb(238, 217, 176);
    w.hovered.bg_stroke = Stroke::new(1.0, GOLD);
    w.hovered.fg_stroke = Stroke::new(1.5, SEPIA);
    w.active.bg_fill = TERRACOTTA;
    w.active.weak_bg_fill = TERRACOTTA;
    w.active.bg_stroke = Stroke::new(1.0, Color32::from_rgb(140, 60, 40));
    w.active.fg_stroke = Stroke::new(1.5, PARCHMENT_LIGHT);
    w.open = w.hovered;
    for state in [
        &mut w.noninteractive,
        &mut w.inactive,
        &mut w.hovered,
        &mut w.active,
        &mut w.open,
    ] {
        state.corner_radius = radius;
    }
    ctx.set_visuals_of(egui::Theme::Light, visuals);
    ctx.set_theme(egui::Theme::Light);

    ctx.all_styles_mut(|style| {
        style.spacing.item_spacing = Vec2::new(8.0, 7.0);
        style.spacing.button_padding = Vec2::new(10.0, 5.0);
        style.spacing.slider_width = 118.0;
        style.spacing.interact_size.y = 24.0;
        style
            .text_styles
            .insert(TextStyle::Heading, FontId::new(21.0, heading_family()));
        style
            .text_styles
            .insert(TextStyle::Body, FontId::proportional(14.5));
        style
            .text_styles
            .insert(TextStyle::Button, FontId::proportional(14.5));
        style
            .text_styles
            .insert(TextStyle::Small, FontId::proportional(11.5));
    });
}

/// A small gold rule with a lozenge in the middle, to separate sections.
pub fn ornament(ui: &mut egui::Ui) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 12.0), egui::Sense::hover());
    let painter = ui.painter();
    let y = rect.center().y;
    let c = rect.center().x;
    painter.hline(rect.left()..=c - 8.0, y, Stroke::new(1.0, GOLD_SOFT));
    painter.hline(c + 8.0..=rect.right(), y, Stroke::new(1.0, GOLD_SOFT));
    let d = 4.0;
    painter.add(egui::Shape::convex_polygon(
        vec![
            egui::pos2(c, y - d),
            egui::pos2(c + d, y),
            egui::pos2(c, y + d),
            egui::pos2(c - d, y),
        ],
        GOLD,
        Stroke::NONE,
    ));
}

/// A section title in the heading face.
pub fn title(ui: &mut egui::Ui, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .font(FontId::new(17.0, heading_family()))
            .color(SEPIA),
    );
}
