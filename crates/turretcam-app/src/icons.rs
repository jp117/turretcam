//! Icons: Segoe Fluent Icons glyphs (system font) for generic actions, and hand-drawn
//! vector icons for the CAM tools that no icon font has.

use crate::theme;
use eframe::egui::{self, pos2, vec2, Color32, Painter, Pos2, Rect, Shape, Stroke};

// Segoe Fluent Icons code points (verified by rendering the font)
pub const ADD: &str = "\u{E710}";
pub const DELETE: &str = "\u{E74D}";
pub const COPY: &str = "\u{E8C8}";
pub const FOLDER: &str = "\u{E8B7}";
pub const SAVE: &str = "\u{E74E}";
pub const IMPORT: &str = "\u{E8B5}";
pub const DOWNLOAD: &str = "\u{E896}";
pub const UNDO: &str = "\u{E7A7}";
pub const REDO: &str = "\u{E7A6}";
pub const SETTINGS: &str = "\u{E713}";
pub const PLAY: &str = "\u{E768}";
pub const PAUSE: &str = "\u{E769}";
pub const PREV: &str = "\u{E892}";
pub const NEXT: &str = "\u{E893}";
pub const CHECK: &str = "\u{E73E}";
pub const WARNING: &str = "\u{E7BA}";
pub const ERROR: &str = "\u{E783}";
pub const FIT: &str = "\u{E740}";
pub const ROTATE: &str = "\u{E7AD}";
pub const CODE: &str = "\u{E943}";
pub const PROCESS: &str = "\u{E9F5}";
pub const LAYERS: &str = "\u{E81E}";
pub const TOOLBOX: &str = "\u{EC7A}";
pub const GRID: &str = "\u{E80A}";
pub const DOCUMENT: &str = "\u{E8A5}";
pub const CHEVRON: &str = "\u{E76C}";
pub const SYNC: &str = "\u{E895}";
pub const MACHINE: &str = "\u{E7F8}";

/// CAM tools drawn as vector icons.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Glyph {
    Select,
    Hole,
    Slot,
    Rect,
    Grid,
    Bolt,
    Punch,
    Sheet,
    Program,
}

pub fn paint(p: &Painter, r: Rect, g: Glyph, c: Color32) {
    let s = Stroke::new(1.6, c);
    let cx = r.center();
    let u = r.width().min(r.height()) / 2.0 * 0.78;
    match g {
        Glyph::Select => {
            let pts = vec![pos2(cx.x - u * 0.55, cx.y - u * 0.85), pos2(cx.x + u * 0.6, cx.y + u * 0.15), pos2(cx.x + u * 0.05, cx.y + u * 0.2), pos2(cx.x + u * 0.4, cx.y + u * 0.85), pos2(cx.x + u * 0.15, cx.y + u * 0.95), pos2(cx.x - u * 0.2, cx.y + u * 0.32), pos2(cx.x - u * 0.55, cx.y + u * 0.65)];
            p.add(Shape::closed_line(pts, s));
        }
        Glyph::Hole => {
            p.circle_stroke(cx, u * 0.85, s);
            p.line_segment([cx - vec2(u * 0.3, 0.0), cx + vec2(u * 0.3, 0.0)], Stroke::new(1.0, c));
            p.line_segment([cx - vec2(0.0, u * 0.3), cx + vec2(0.0, u * 0.3)], Stroke::new(1.0, c));
        }
        Glyph::Slot => {
            let h = u * 0.42;
            let l = u * 0.95;
            let mut pts = vec![];
            for i in 0..=12 {
                let a = -std::f32::consts::FRAC_PI_2 + std::f32::consts::PI * i as f32 / 12.0;
                pts.push(pos2(cx.x + (l - h) + h * a.cos(), cx.y + h * a.sin()));
            }
            for i in 0..=12 {
                let a = std::f32::consts::FRAC_PI_2 + std::f32::consts::PI * i as f32 / 12.0;
                pts.push(pos2(cx.x - (l - h) + h * a.cos(), cx.y + h * a.sin()));
            }
            p.add(Shape::closed_line(pts, s));
        }
        Glyph::Rect => {
            p.rect_stroke(Rect::from_center_size(cx, vec2(u * 1.9, u * 1.3)), 1.0, s, egui::StrokeKind::Middle);
            for (dx, dy) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                p.circle_filled(pos2(cx.x + dx * u * 0.95, cx.y + dy * u * 0.65), 1.8, c);
            }
        }
        Glyph::Grid => {
            for j in 0..3 {
                for i in 0..4 {
                    let x = cx.x - u * 0.8 + i as f32 * u * 0.53;
                    let y = cx.y - u * 0.6 + j as f32 * u * 0.6;
                    p.rect_filled(Rect::from_center_size(pos2(x, y), vec2(u * 0.18, u * 0.42)), 1.0, c);
                }
            }
        }
        Glyph::Bolt => {
            p.circle_stroke(cx, u * 0.65, Stroke::new(1.0, c.gamma_multiply(0.5)));
            for k in 0..6 {
                let a = std::f32::consts::TAU * k as f32 / 6.0;
                p.circle_stroke(pos2(cx.x + u * 0.65 * a.cos(), cx.y + u * 0.65 * a.sin()), u * 0.2, s);
            }
        }
        Glyph::Punch => {
            p.rect_stroke(Rect::from_min_max(pos2(cx.x - u * 0.35, cx.y - u * 0.95), pos2(cx.x + u * 0.35, cx.y + u * 0.15)), 1.0, s, egui::StrokeKind::Middle);
            p.line_segment([pos2(cx.x - u * 0.9, cx.y + u * 0.55), pos2(cx.x + u * 0.9, cx.y + u * 0.55)], s);
            p.line_segment([pos2(cx.x, cx.y + u * 0.25), pos2(cx.x, cx.y + u * 0.45)], s);
        }
        Glyph::Sheet => {
            p.rect_stroke(Rect::from_center_size(cx, vec2(u * 1.9, u * 1.3)), 1.0, s, egui::StrokeKind::Middle);
            p.rect_stroke(Rect::from_min_size(pos2(cx.x - u * 0.8, cx.y - u * 0.5), vec2(u * 0.7, u * 0.5)), 1.0, Stroke::new(1.2, c), egui::StrokeKind::Middle);
            p.rect_stroke(Rect::from_min_size(pos2(cx.x + u * 0.05, cx.y - u * 0.5), vec2(u * 0.7, u * 0.5)), 1.0, Stroke::new(1.2, c), egui::StrokeKind::Middle);
        }
        Glyph::Program => {
            for k in 0..4 {
                let y = cx.y - u * 0.7 + k as f32 * u * 0.46;
                p.line_segment([pos2(cx.x - u * 0.8, y), pos2(cx.x + u * (0.8 - 0.25 * (k % 2) as f32), y)], s);
            }
        }
    }
}

/// The TurretCAM mark: a turret ring with stations, accent blue.
pub fn logo(p: &Painter, centre: Pos2, radius: f32) {
    p.circle_filled(centre, radius, theme::ACCENT_DIM);
    p.circle_stroke(centre, radius, Stroke::new(1.5, theme::ACCENT));
    for k in 0..10 {
        let a = std::f32::consts::TAU * k as f32 / 10.0 - std::f32::consts::FRAC_PI_2;
        let rr = if k % 5 == 0 { radius * 0.2 } else { radius * 0.12 };
        p.circle_filled(pos2(centre.x + radius * 0.62 * a.cos(), centre.y + radius * 0.62 * a.sin()), rr, theme::TEXT);
    }
}
