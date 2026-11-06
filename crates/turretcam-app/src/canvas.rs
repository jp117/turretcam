//! Shared 2-D canvas: world (inches, Y up) <-> screen, pan / zoom / fit, drawing helpers.

use eframe::egui::{self, pos2, Color32, Pos2, Rect, Response, Shape, Stroke, StrokeKind, Ui};
use turretcam_core::geom::{BBox, Pt};
use turretcam_core::part::{Feature, Footprint, Outline, Part};
use turretcam_core::tooling::{ToolKind, ToolShape};

#[derive(Clone, Copy)]
pub struct View {
    pub cx: f64,
    pub cy: f64,
    /// pixels per inch
    pub scale: f32,
    pub needs_fit: bool,
}

impl Default for View {
    fn default() -> Self {
        View { cx: 24.0, cy: 12.0, scale: 10.0, needs_fit: true }
    }
}

impl View {
    pub fn to_screen(&self, r: Rect, p: Pt) -> Pos2 {
        pos2(r.center().x + ((p.0 - self.cx) as f32) * self.scale, r.center().y - ((p.1 - self.cy) as f32) * self.scale)
    }
    pub fn to_world(&self, r: Rect, s: Pos2) -> Pt {
        (self.cx + ((s.x - r.center().x) / self.scale) as f64, self.cy - ((s.y - r.center().y) / self.scale) as f64)
    }
    pub fn fit(&mut self, r: Rect, b: BBox) {
        if b.is_empty() || b.w() <= 0.0 && b.h() <= 0.0 {
            return;
        }
        self.cx = (b.x0 + b.x1) / 2.0;
        self.cy = (b.y0 + b.y1) / 2.0;
        let sx = r.width() / (b.w().max(0.5) as f32 * 1.08);
        let sy = r.height() / (b.h().max(0.5) as f32 * 1.08);
        self.scale = sx.min(sy).clamp(0.5, 4000.0);
        self.needs_fit = false;
    }
    /// Wheel zooms about the cursor; middle- or right-drag pans.
    pub fn interact(&mut self, ui: &Ui, resp: &Response, r: Rect) {
        if resp.hovered() {
            let (scroll, zoom) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
            let factor = if zoom != 1.0 { zoom } else { (scroll / 300.0).exp() };
            if (factor - 1.0).abs() > 1e-4 {
                if let Some(hp) = resp.hover_pos() {
                    let before = self.to_world(r, hp);
                    self.scale = (self.scale * factor).clamp(0.5, 4000.0);
                    let after = self.to_world(r, hp);
                    self.cx += before.0 - after.0;
                    self.cy += before.1 - after.1;
                }
            }
        }
        if resp.dragged_by(egui::PointerButton::Middle) || resp.dragged_by(egui::PointerButton::Secondary) {
            let d = resp.drag_delta();
            self.cx -= (d.x / self.scale) as f64;
            self.cy += (d.y / self.scale) as f64;
        }
    }
}

pub fn snap(p: Pt, step: f64) -> Pt {
    if step <= 0.0 {
        return p;
    }
    ((p.0 / step).round() * step, (p.1 / step).round() * step)
}

pub fn poly(painter: &egui::Painter, v: &View, r: Rect, pts: &[Pt], closed: bool, stroke: Stroke, fill: Option<Color32>) {
    if pts.len() < 2 {
        return;
    }
    let sp: Vec<Pos2> = pts.iter().map(|p| v.to_screen(r, *p)).collect();
    if let (Some(f), true) = (fill, closed) {
        painter.add(Shape::convex_polygon(sp.clone(), f, Stroke::NONE));
    }
    if closed {
        painter.add(Shape::closed_line(sp, stroke));
    } else {
        painter.add(Shape::line(sp, stroke));
    }
}

pub fn rect(painter: &egui::Painter, v: &View, r: Rect, b: BBox, stroke: Stroke, fill: Color32) {
    let a = v.to_screen(r, (b.x0, b.y1));
    let c = v.to_screen(r, (b.x1, b.y0));
    let rr = Rect::from_two_pos(a, c);
    painter.rect_filled(rr, 0.0, fill);
    painter.rect_stroke(rr, 0.0, stroke, StrokeKind::Middle);
}

/// Light grid every `step` inches (only when it would not be too dense).
pub fn grid(painter: &egui::Painter, v: &View, r: Rect, step: f64, colour: Color32) {
    if step <= 0.0 || (step as f32 * v.scale) < 8.0 {
        return;
    }
    let (x0, y1) = v.to_world(r, r.left_top());
    let (x1, y0) = v.to_world(r, r.right_bottom());
    let mut x = (x0 / step).floor() * step;
    while x <= x1 {
        painter.line_segment([v.to_screen(r, (x, y0)), v.to_screen(r, (x, y1))], Stroke::new(1.0, colour));
        x += step;
    }
    let mut y = (y0 / step).floor() * step;
    while y <= y1 {
        painter.line_segment([v.to_screen(r, (x0, y)), v.to_screen(r, (x1, y))], Stroke::new(1.0, colour));
        y += step;
    }
}

pub fn footprint_shape(fp: &Footprint) -> ToolShape {
    let (a, b, mount) = if fp.w >= fp.h { (fp.w, fp.h, 0.0) } else { (fp.h, fp.w, 90.0) };
    ToolShape { name: String::new(), kind: fp.kind, a: Some(a), b: Some(b), mount: if fp.kind == ToolKind::RD || fp.kind == ToolKind::SQ { 0.0 } else { mount } }
}

/// Outlines (polygons, closed flag) that represent a feature, for drawing.
pub fn feature_geometry(f: &Feature, tool_shape: &dyn Fn(&turretcam_core::cam::ToolRef) -> Option<ToolShape>) -> Vec<(Vec<Pt>, bool)> {
    let circle = |x: f64, y: f64, d: f64| ToolShape { name: String::new(), kind: ToolKind::RD, a: Some(d), b: None, mount: 0.0 }.outline(x, y, 0.0, 48);
    match f {
        Feature::Hole { x, y, dia } => vec![(circle(*x, *y, *dia), true)],
        Feature::Slot { x, y, length, width, vertical } => {
            let s = ToolShape { name: String::new(), kind: ToolKind::OB, a: Some(*length), b: Some(*width), mount: if *vertical { 90.0 } else { 0.0 } };
            vec![(s.outline(*x, *y, 0.0, 32), true)]
        }
        Feature::Rect { x0, y0, x1, y1, .. } => vec![(vec![(*x0, *y0), (*x1, *y0), (*x1, *y1), (*x0, *y1)], true)],
        Feature::Grid { x, y, dx, dy, nx, ny, shape } => {
            let s = footprint_shape(shape);
            let mut v = vec![];
            for j in 0..*ny {
                for i in 0..*nx {
                    v.push((s.outline(x + i as f64 * dx, y + j as f64 * dy, 0.0, 20), true));
                }
            }
            v
        }
        Feature::BoltCircle { x, y, r, n, dia, start } => (0..*n)
            .map(|k| {
                let a = (start + 360.0 * k as f64 / *n as f64).to_radians();
                (circle(x + r * a.cos(), y + r * a.sin(), *dia), true)
            })
            .collect(),
        Feature::Hit { x, y, tool, angle } => match tool_shape(tool) {
            Some(s) => vec![(s.outline(*x, *y, *angle, 24), true)],
            None => vec![(circle(*x, *y, 0.1), true)],
        },
        Feature::Contour { points, closed, .. } => vec![(points.clone(), *closed)],
    }
}

pub fn outline_geometry(p: &Part) -> Option<Vec<Pt>> {
    match &p.outline {
        Outline::Rect { w, h, .. } => Some(vec![(0.0, 0.0), (*w, 0.0), (*w, *h), (0.0, *h)]),
        Outline::Polygon { points } => Some(points.clone()),
        Outline::None => None,
    }
}

pub fn transform_pts(pts: &[Pt], rot: i32, dx: f64, dy: f64) -> Vec<Pt> {
    pts.iter().map(|p| turretcam_core::geom::xform(*p, rot as f64, dx, dy)).collect()
}

