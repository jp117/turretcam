//! Small reusable widgets in the TurretCAM style.

use crate::icons::{self, Glyph};
use crate::theme;
use eframe::egui::{self, vec2, Align2, Color32, CornerRadius, FontId, Margin, Response, RichText, Sense, Stroke, Ui};

pub fn card<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Frame::new()
        .fill(theme::CARD)
        .stroke(Stroke::new(1.0, theme::BORDER))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::same(10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

/// Card with a title row.
pub fn titled_card<R>(ui: &mut Ui, icon: &str, title: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    card(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new(icon).font(theme::icon_font(14.0)).color(theme::TEXT_DIM));
            ui.label(RichText::new(title).strong());
        });
        ui.add_space(2.0);
        add(ui)
    })
}

/// Square icon button using a Fluent glyph.
pub fn icon_button(ui: &mut Ui, glyph: &str, tip: &str, selected: bool) -> Response {
    let (r, resp) = ui.allocate_exact_size(vec2(32.0, 30.0), Sense::click());
    let bg = if selected {
        theme::ACCENT_DIM
    } else if resp.hovered() {
        theme::CARD_HOVER
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(r, 6.0, bg);
    let col = if selected { theme::TEXT } else if resp.hovered() { theme::TEXT } else { theme::TEXT_DIM };
    ui.painter().text(r.center(), Align2::CENTER_CENTER, glyph, theme::icon_font(15.0), col);
    resp.on_hover_text(tip)
}

/// Large palette button with a drawn CAM glyph.
pub fn glyph_button(ui: &mut Ui, g: Glyph, tip: &str, selected: bool) -> Response {
    let (r, resp) = ui.allocate_exact_size(vec2(40.0, 38.0), Sense::click());
    let bg = if selected {
        theme::ACCENT_DIM
    } else if resp.hovered() {
        theme::CARD_HOVER
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(r, 7.0, bg);
    if selected {
        ui.painter().rect_stroke(r, 7.0, Stroke::new(1.0, theme::ACCENT), egui::StrokeKind::Inside);
    }
    icons::paint(ui.painter(), r.shrink(8.0), g, if selected { theme::TEXT } else { theme::TEXT_DIM });
    resp.on_hover_text(tip)
}

/// Filled call-to-action button.
pub fn primary(ui: &mut Ui, glyph: &str, text: &str, fill: Color32, enabled: bool) -> Response {
    let font = FontId::proportional(13.5);
    let tw = ui.painter().layout_no_wrap(text.to_string(), font.clone(), Color32::WHITE).size().x;
    let (r, resp) = ui.allocate_exact_size(vec2(tw + 46.0, 32.0), if enabled { Sense::click() } else { Sense::hover() });
    let f = if !enabled {
        theme::CARD
    } else if resp.is_pointer_button_down_on() {
        fill.gamma_multiply(0.8)
    } else if resp.hovered() {
        fill.gamma_multiply(1.15)
    } else {
        fill
    };
    ui.painter().rect_filled(r, 7.0, f);
    let tc = if enabled { Color32::WHITE } else { theme::TEXT_FAINT };
    ui.painter().text(egui::pos2(r.left() + 14.0, r.center().y), Align2::LEFT_CENTER, glyph, theme::icon_font(14.0), tc);
    ui.painter().text(egui::pos2(r.left() + 34.0, r.center().y), Align2::LEFT_CENTER, text, font, tc);
    resp
}

/// Secondary button with an icon and a label.
pub fn button(ui: &mut Ui, glyph: &str, text: &str) -> Response {
    ui.add(egui::Button::new(RichText::new(format!("{}  {}", glyph, text))))
}

/// Small rounded label, e.g. a tool station.
pub fn chip(ui: &mut Ui, text: &str, colour: Color32) -> Response {
    let font = FontId::proportional(11.5);
    let tw = ui.painter().layout_no_wrap(text.to_string(), font.clone(), colour).size().x;
    let (r, resp) = ui.allocate_exact_size(vec2(tw + 14.0, 18.0), Sense::hover());
    ui.painter().rect_filled(r, 9.0, colour.gamma_multiply(0.18));
    ui.painter().rect_stroke(r, 9.0, Stroke::new(1.0, colour.gamma_multiply(0.6)), egui::StrokeKind::Inside);
    ui.painter().text(r.center(), Align2::CENTER_CENTER, text, font, colour);
    resp
}

/// Big number + label tile.
pub fn stat(ui: &mut Ui, width: f32, label: &str, value: &str, colour: Color32) {
    egui::Frame::new().fill(theme::CARD).stroke(Stroke::new(1.0, theme::BORDER)).corner_radius(CornerRadius::same(8)).inner_margin(Margin::symmetric(10, 8)).show(ui, |ui| {
        ui.set_width(width.max(90.0));
        ui.vertical(|ui| {
            ui.label(RichText::new(value).size(20.0).color(colour).strong());
            ui.label(RichText::new(label).size(11.5).color(theme::TEXT_DIM));
        });
    });
}

/// Labelled inch field inside a 2-column Grid.
pub fn inch(ui: &mut Ui, label: &str, v: &mut f64) -> bool {
    ui.label(RichText::new(label).color(theme::TEXT_DIM));
    let r = ui.add(egui::DragValue::new(v).speed(0.01).max_decimals(4).suffix(" in")).changed();
    ui.end_row();
    r
}

pub fn count(ui: &mut Ui, label: &str, v: &mut u32, max: u32) -> bool {
    ui.label(RichText::new(label).color(theme::TEXT_DIM));
    let r = ui.add(egui::DragValue::new(v).speed(0.1).range(1..=max)).changed();
    ui.end_row();
    r
}

/// Severity icon + coloured text line.
pub fn issue_line(ui: &mut Ui, error: bool, text: &str) {
    ui.horizontal_wrapped(|ui| {
        let (g, c) = if error { (icons::ERROR, theme::ERR) } else { (icons::WARNING, theme::WARN) };
        ui.label(RichText::new(g).font(theme::icon_font(13.0)).color(c));
        ui.label(RichText::new(text).size(12.0).color(if error { theme::ERR } else { theme::TEXT_DIM }));
    });
}
