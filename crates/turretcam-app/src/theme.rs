//! TurretCAM visual design: dark palette, Windows 11 fonts (loaded from the system at
//! runtime, nothing bundled), spacing and widget styling.

use eframe::egui::{self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke, TextStyle};
use std::sync::Arc;

// ---- palette ---------------------------------------------------------------------------
pub const BG: Color32 = Color32::from_rgb(15, 17, 21);
pub const PANEL: Color32 = Color32::from_rgb(22, 26, 32);
pub const CARD: Color32 = Color32::from_rgb(29, 34, 42);
pub const CARD_HOVER: Color32 = Color32::from_rgb(36, 42, 52);
pub const BORDER: Color32 = Color32::from_rgb(42, 49, 60);
pub const TEXT: Color32 = Color32::from_rgb(230, 233, 239);
pub const TEXT_DIM: Color32 = Color32::from_rgb(154, 164, 178);
pub const TEXT_FAINT: Color32 = Color32::from_rgb(107, 118, 134);
pub const ACCENT: Color32 = Color32::from_rgb(79, 140, 255);
pub const ACCENT_DIM: Color32 = Color32::from_rgb(33, 58, 104);
pub const OK: Color32 = Color32::from_rgb(63, 185, 80);
pub const WARN: Color32 = Color32::from_rgb(210, 153, 34);
pub const ERR: Color32 = Color32::from_rgb(248, 81, 73);

pub const CANVAS: Color32 = Color32::from_rgb(11, 13, 16);
pub const GRID: Color32 = Color32::from_rgb(20, 24, 29);
pub const GRID_MAJOR: Color32 = Color32::from_rgb(30, 36, 44);
pub const GEOMETRY: Color32 = Color32::from_rgb(230, 233, 239);
pub const SELECT: Color32 = Color32::from_rgb(255, 204, 77);
pub const HOVER: Color32 = Color32::from_rgb(138, 180, 255);
pub const SHEET: Color32 = Color32::from_rgb(24, 29, 36);
pub const SHEET_EDGE: Color32 = Color32::from_rgb(92, 104, 120);

/// Per-tool colours on the dark canvas (stable by order of first use).
pub const TOOLS: [Color32; 12] = [
    Color32::from_rgb(79, 195, 247),
    Color32::from_rgb(255, 138, 101),
    Color32::from_rgb(174, 213, 129),
    Color32::from_rgb(206, 147, 216),
    Color32::from_rgb(255, 213, 79),
    Color32::from_rgb(77, 208, 225),
    Color32::from_rgb(240, 98, 146),
    Color32::from_rgb(144, 202, 249),
    Color32::from_rgb(255, 183, 77),
    Color32::from_rgb(129, 199, 132),
    Color32::from_rgb(179, 157, 219),
    Color32::from_rgb(255, 241, 118),
];

pub fn tool_colour(i: usize) -> Color32 {
    TOOLS[i % TOOLS.len()]
}

// ---- fonts -----------------------------------------------------------------------------
pub const ICONS: &str = "icons";

fn system_font(names: &[&str]) -> Option<Vec<u8>> {
    let dir = std::env::var("WINDIR").map(|w| format!("{}\\Fonts", w)).unwrap_or_else(|_| r"C:\Windows\Fonts".into());
    names.iter().find_map(|n| std::fs::read(format!("{}\\{}", dir, n)).ok())
}

pub fn icon_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(ICONS.into()))
}

pub fn install(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    if let Some(ui) = system_font(&["segoeui.ttf", "SegUIVar.ttf"]) {
        fonts.font_data.insert("ui".into(), Arc::new(FontData::from_owned(ui)));
        fonts.families.get_mut(&FontFamily::Proportional).unwrap().insert(0, "ui".into());
    }
    if let Some(b) = system_font(&["segoeuisb.ttf", "seguisb.ttf", "segoeuib.ttf"]) {
        fonts.font_data.insert("ui-bold".into(), Arc::new(FontData::from_owned(b)));
        fonts.families.insert(FontFamily::Name("bold".into()), vec!["ui-bold".into(), "ui".into()]);
    } else {
        fonts.families.insert(FontFamily::Name("bold".into()), fonts.families[&FontFamily::Proportional].clone());
    }
    if let Some(m) = system_font(&["CascadiaMono.ttf", "consola.ttf"]) {
        fonts.font_data.insert("mono".into(), Arc::new(FontData::from_owned(m)));
        fonts.families.get_mut(&FontFamily::Monospace).unwrap().insert(0, "mono".into());
    }
    match system_font(&["SegoeIcons.ttf", "segmdl2.ttf"]) {
        Some(i) => {
            fonts.font_data.insert(ICONS.into(), Arc::new(FontData::from_owned(i)));
            fonts.families.insert(FontFamily::Name(ICONS.into()), vec![ICONS.into()]);
            // also a fallback so icon glyphs can sit inside normal labels
            fonts.families.get_mut(&FontFamily::Proportional).unwrap().push(ICONS.into());
        }
        None => {
            fonts.families.insert(FontFamily::Name(ICONS.into()), fonts.families[&FontFamily::Proportional].clone());
        }
    }
    ctx.set_fonts(fonts);

    ctx.global_style_mut(|s| {
        s.text_styles = [
            (TextStyle::Small, FontId::proportional(11.5)),
            (TextStyle::Body, FontId::proportional(13.5)),
            (TextStyle::Button, FontId::proportional(13.5)),
            (TextStyle::Heading, FontId::new(17.0, FontFamily::Name("bold".into()))),
            (TextStyle::Monospace, FontId::monospace(12.5)),
        ]
        .into();
        s.spacing.item_spacing = egui::vec2(8.0, 6.0);
        s.spacing.button_padding = egui::vec2(10.0, 5.0);
        s.spacing.interact_size = egui::vec2(36.0, 26.0);
        s.spacing.combo_width = 140.0;
        let v = &mut s.visuals;
        *v = egui::Visuals::dark();
        v.panel_fill = PANEL;
        v.window_fill = CARD;
        v.extreme_bg_color = Color32::from_rgb(13, 16, 20);
        v.faint_bg_color = Color32::from_rgb(26, 31, 38);
        v.override_text_color = Some(TEXT);
        v.window_stroke = Stroke::new(1.0, BORDER);
        v.window_corner_radius = CornerRadius::same(10);
        v.menu_corner_radius = CornerRadius::same(8);
        v.selection.bg_fill = ACCENT_DIM;
        v.selection.stroke = Stroke::new(1.0, ACCENT);
        v.hyperlink_color = ACCENT;
        let r = CornerRadius::same(6);
        v.widgets.noninteractive.bg_fill = PANEL;
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
        v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT_DIM);
        v.widgets.noninteractive.corner_radius = r;
        v.widgets.inactive.bg_fill = CARD;
        v.widgets.inactive.weak_bg_fill = CARD;
        v.widgets.inactive.bg_stroke = Stroke::new(1.0, BORDER);
        v.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT);
        v.widgets.inactive.corner_radius = r;
        v.widgets.hovered.bg_fill = CARD_HOVER;
        v.widgets.hovered.weak_bg_fill = CARD_HOVER;
        v.widgets.hovered.bg_stroke = Stroke::new(1.0, Color32::from_rgb(64, 74, 90));
        v.widgets.hovered.fg_stroke = Stroke::new(1.0, TEXT);
        v.widgets.hovered.corner_radius = r;
        v.widgets.hovered.expansion = 0.0;
        v.widgets.active.bg_fill = ACCENT_DIM;
        v.widgets.active.weak_bg_fill = ACCENT_DIM;
        v.widgets.active.bg_stroke = Stroke::new(1.0, ACCENT);
        v.widgets.active.fg_stroke = Stroke::new(1.0, TEXT);
        v.widgets.active.corner_radius = r;
        v.widgets.open = v.widgets.active;
    });
}
