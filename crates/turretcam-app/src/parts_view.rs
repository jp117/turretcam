//! Parts step: parts library, drawing canvas with tool palette, inspector.

use crate::app::{feature_glyph, feature_label, App, DrawTool};
use crate::canvas::{self, snap, View};
use crate::icons::{self, Glyph};
use crate::theme;
use crate::widgets;
use eframe::egui::{self, pos2, vec2, Align2, Color32, DragValue, FontId, Margin, RichText, Sense, Stroke};
use turretcam_core::cam::{Job, JobOptions, ToolRef};
use turretcam_core::cnc::Sheet;
use turretcam_core::geom::{BBox, Pt};
use turretcam_core::part::{Feature, Footprint, Outline, Part};
use turretcam_core::tooling::ToolKind;
use turretcam_core::Machine;

/// Stations a feature will be punched with, or why it can't be.
pub fn resolve(m: &Machine, f: &Feature) -> Result<(Vec<u32>, usize), String> {
    if let Feature::Contour { .. } = f {
        return Err("Imported geometry TurretCAM can't punch yet. Redraw it as holes or openings, or delete it.".into());
    }
    let mut job = Job::new(m, 1, "", Sheet { x: 1000.0, y: 1000.0, thickness: 0.0315, material: "ST 42".into(), qty: 1 }, JobOptions::default());
    f.transformed(0, 100.0, 100.0).apply(&mut job).map_err(|e| e.0)?;
    let mut st: Vec<u32> = job.segs.iter().map(|s| s.station).collect();
    st.sort();
    st.dedup();
    let hits = job.segs.iter().map(|s| s.hits().len()).sum();
    Ok((st, hits))
}

pub fn tool_shape(m: &Option<Machine>, t: &ToolRef) -> Option<turretcam_core::tooling::ToolShape> {
    let m = m.as_ref()?;
    let st = match t {
        ToolRef::Station(n) => *n,
        ToolRef::Name(n) => m.station_for_name(n)?,
    };
    m.station(st)?.shape()
}

/// Stable colour per station; stepping by 5 spreads neighbouring stations across the palette.
pub fn station_colour(st: u32) -> Color32 {
    theme::tool_colour((st as usize * 5) % theme::TOOLS.len())
}

// ---------------------------------------------------------------------------- left panel
fn thumbnail(ui: &mut egui::Ui, part: &Part, size: egui::Vec2, shape_of: &dyn Fn(&ToolRef) -> Option<turretcam_core::tooling::ToolShape>) {
    let (r, _) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter().rect_filled(r, 5.0, theme::CANVAS);
    let mut v = View::default();
    let b = part.bbox();
    if b.is_empty() {
        return;
    }
    v.fit(r.shrink(4.0), b);
    let p = ui.painter_at(r);
    if let Some(o) = canvas::outline_geometry(part) {
        canvas::poly(&p, &v, r, &o, true, Stroke::new(1.0, theme::GEOMETRY), None);
    }
    for f in &part.features {
        for (pts, closed) in canvas::feature_geometry(f, shape_of) {
            canvas::poly(&p, &v, r, &pts, closed, Stroke::new(0.8, if matches!(f, Feature::Contour { .. }) { theme::ERR } else { theme::HOVER }), None);
        }
    }
}

fn parts_panel(app: &mut App, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("Parts").heading());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if widgets::icon_button(ui, icons::IMPORT, "Import a DXF or DWG drawing", false).clicked() {
                app.import_drawing();
            }
            if widgets::icon_button(ui, icons::ADD, "New part", false).clicked() {
                app.new_part.open = true;
            }
        });
    });
    ui.add_space(4.0);
    let shape_of = |t: &ToolRef| app.tool_shape(t);
    let mut select = None;
    for (i, p) in app.project.parts.iter().enumerate() {
        let sel = i == app.sel_part;
        let resp = egui::Frame::new()
            .fill(if sel { theme::ACCENT_DIM } else { theme::CARD })
            .stroke(Stroke::new(1.0, if sel { theme::ACCENT } else { theme::BORDER }))
            .corner_radius(8)
            .inner_margin(Margin::same(6))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    thumbnail(ui, p, vec2(64.0, 44.0), &shape_of);
                    ui.vertical(|ui| {
                        ui.label(RichText::new(&p.name).strong());
                        let b = p.bbox();
                        let size = if b.is_empty() { "empty".into() } else { format!("{} × {} in", trim(b.w()), trim(b.h())) };
                        ui.label(RichText::new(format!("{}  ·  {} features", size, p.features.len())).size(11.5).color(theme::TEXT_DIM));
                        let bad = p.unsupported().len();
                        if bad > 0 {
                            ui.label(RichText::new(format!("{} {} need attention", icons::WARNING, bad)).size(11.5).color(theme::WARN));
                        }
                    });
                });
            })
            .response
            .interact(Sense::click());
        if resp.clicked() {
            select = Some(i);
        }
        ui.add_space(2.0);
    }
    if let Some(i) = select {
        app.sel_part = i;
        app.sel_feature = None;
        app.part_view.needs_fit = true;
    }
    ui.add_space(10.0);
    let Some(part) = app.current_part().cloned() else { return };
    ui.horizontal(|ui| {
        ui.label(RichText::new("Features").heading());
        ui.label(RichText::new(part.features.len().to_string()).color(theme::TEXT_FAINT));
    });
    if part.features.is_empty() {
        ui.label(RichText::new("Pick a tool on the left of the drawing (H hole, S slot, O opening, G louvers) and click on the part.").size(12.0).color(theme::TEXT_DIM));
    }
    let resolved: Vec<_> = part.features.iter().map(|f| app.machine.as_ref().map(|m| resolve(m, f))).collect();
    let mut sel = app.sel_feature;
    let mut hover = None;
    egui::ScrollArea::vertical().id_salt("feats").show(ui, |ui| {
        for (i, f) in part.features.iter().enumerate() {
            let is_sel = sel == Some(i);
            let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 28.0), Sense::click());
            let bg = if is_sel { theme::ACCENT_DIM } else if resp.hovered() { theme::CARD_HOVER } else { Color32::TRANSPARENT };
            ui.painter().rect_filled(r, 6.0, bg);
            let bad = matches!(f, Feature::Contour { .. }) || matches!(resolved[i], Some(Err(_)));
            icons::paint(ui.painter(), egui::Rect::from_center_size(pos2(r.left() + 16.0, r.center().y), vec2(18.0, 18.0)), feature_glyph(f), if bad { theme::ERR } else { theme::TEXT_DIM });
            ui.painter().text(pos2(r.left() + 32.0, r.center().y), Align2::LEFT_CENTER, feature_label(f), FontId::proportional(12.5), if bad { theme::ERR } else { theme::TEXT });
            // tool chips on the right
            let mut x = r.right() - 6.0;
            match &resolved[i] {
                Some(Ok((st, _))) => {
                    for s in st.iter().rev() {
                        let t = format!("T{}", s);
                        let w = ui.painter().layout_no_wrap(t.clone(), FontId::proportional(11.0), Color32::WHITE).size().x + 12.0;
                        let cr = egui::Rect::from_min_size(pos2(x - w, r.center().y - 9.0), vec2(w, 18.0));
                        let c = station_colour(*s);
                        ui.painter().rect_filled(cr, 9.0, c.gamma_multiply(0.2));
                        ui.painter().text(cr.center(), Align2::CENTER_CENTER, t, FontId::proportional(11.0), c);
                        x -= w + 4.0;
                    }
                }
                Some(Err(_)) => {
                    let cr = egui::Rect::from_min_size(pos2(x - 58.0, r.center().y - 9.0), vec2(58.0, 18.0));
                    ui.painter().rect_filled(cr, 9.0, theme::ERR.gamma_multiply(0.2));
                    ui.painter().text(cr.center(), Align2::CENTER_CENTER, "no tool", FontId::proportional(11.0), theme::ERR);
                }
                None => {}
            }
            if resp.clicked() {
                sel = Some(i);
            }
            if resp.hovered() {
                hover = Some(i);
            }
        }
    });
    app.sel_feature = sel;
    if hover.is_some() {
        app.hover_feature = hover;
    }
}

fn trim(v: f64) -> String {
    let s = format!("{:.4}", v);
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

// ---------------------------------------------------------------------------- inspector
fn feature_editor(ui: &mut egui::Ui, f: &mut Feature) {
    egui::Grid::new("feat").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| match f {
        Feature::Hole { x, y, dia } => {
            widgets::inch(ui, "Centre X", x);
            widgets::inch(ui, "Centre Y", y);
            widgets::inch(ui, "Diameter", dia);
        }
        Feature::Slot { x, y, length, width, vertical } => {
            widgets::inch(ui, "Centre X", x);
            widgets::inch(ui, "Centre Y", y);
            widgets::inch(ui, "Length", length);
            widgets::inch(ui, "Width", width);
            ui.label(RichText::new("Direction").color(theme::TEXT_DIM));
            ui.horizontal(|ui| {
                ui.selectable_value(vertical, false, "Horizontal");
                ui.selectable_value(vertical, true, "Vertical");
            });
            ui.end_row();
        }
        Feature::Rect { x0, y0, x1, y1, tabs } => {
            let (mut w, mut h) = (*x1 - *x0, *y1 - *y0);
            widgets::inch(ui, "Left X", x0);
            widgets::inch(ui, "Bottom Y", y0);
            widgets::inch(ui, "Width", &mut w);
            widgets::inch(ui, "Height", &mut h);
            *x1 = *x0 + w.max(0.01);
            *y1 = *y0 + h.max(0.01);
            ui.label(RichText::new("Microjoints").color(theme::TEXT_DIM));
            ui.checkbox(tabs, "hold slug in place");
            ui.end_row();
        }
        Feature::Grid { x, y, dx, dy, nx, ny, shape } => {
            widgets::inch(ui, "First X", x);
            widgets::inch(ui, "First Y", y);
            widgets::inch(ui, "Pitch X", dx);
            widgets::inch(ui, "Pitch Y", dy);
            widgets::count(ui, "Columns", nx, 500);
            widgets::count(ui, "Rows", ny, 500);
            footprint_ui(ui, shape);
        }
        Feature::BoltCircle { x, y, r, n, dia, start } => {
            widgets::inch(ui, "Centre X", x);
            widgets::inch(ui, "Centre Y", y);
            widgets::inch(ui, "Radius", r);
            widgets::count(ui, "Holes", n, 72);
            widgets::inch(ui, "Hole Ø", dia);
            ui.label(RichText::new("Start angle").color(theme::TEXT_DIM));
            ui.add(DragValue::new(start).speed(1.0).suffix("°"));
            ui.end_row();
        }
        Feature::Hit { x, y, tool, angle } => {
            widgets::inch(ui, "X", x);
            widgets::inch(ui, "Y", y);
            ui.label(RichText::new("Station").color(theme::TEXT_DIM));
            let mut st = match tool {
                ToolRef::Station(s) => *s,
                ToolRef::Name(_) => 0,
            };
            if ui.add(DragValue::new(&mut st).range(1..=99).prefix("T")).changed() {
                *tool = ToolRef::Station(st);
            }
            ui.end_row();
            ui.label(RichText::new("C angle").color(theme::TEXT_DIM));
            ui.add(DragValue::new(angle).speed(1.0).suffix("°"));
            ui.end_row();
        }
        Feature::Contour { points, closed, note } => {
            ui.label("Points");
            ui.label(points.len().to_string());
            ui.end_row();
            ui.label("Type");
            ui.label(format!("{} {}", if *closed { "closed" } else { "open" }, note));
            ui.end_row();
        }
    });
}

fn footprint_ui(ui: &mut egui::Ui, fp: &mut Footprint) {
    ui.label(RichText::new("Shape").color(theme::TEXT_DIM));
    ui.horizontal(|ui| {
        for (k, n) in [(ToolKind::RD, "Round"), (ToolKind::SQ, "Square"), (ToolKind::RE, "Rect"), (ToolKind::OB, "Obround")] {
            ui.selectable_value(&mut fp.kind, k, n);
        }
    });
    ui.end_row();
    widgets::inch(ui, "Shape width", &mut fp.w);
    widgets::inch(ui, "Shape height", &mut fp.h);
}

fn new_feature_params(app: &mut App, ui: &mut egui::Ui) {
    let t = app.draw;
    widgets::titled_card(ui, icons::ADD, &format!("New {}", t.name().to_lowercase()), |ui| {
        let p = &mut app.params;
        egui::Grid::new("params").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| match t {
            DrawTool::Hole => {
                widgets::inch(ui, "Diameter", &mut p.hole_dia);
            }
            DrawTool::Slot => {
                widgets::inch(ui, "Length", &mut p.slot_len);
                widgets::inch(ui, "Width", &mut p.slot_w);
                ui.label(RichText::new("Direction").color(theme::TEXT_DIM));
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut p.slot_vertical, false, "Horizontal");
                    ui.selectable_value(&mut p.slot_vertical, true, "Vertical");
                });
                ui.end_row();
            }
            DrawTool::Rect => {
                ui.label(RichText::new("Microjoints").color(theme::TEXT_DIM));
                ui.checkbox(&mut p.rect_tabs, "hold slug in place");
                ui.end_row();
            }
            DrawTool::Grid => {
                widgets::inch(ui, "Pitch X", &mut p.grid.0);
                widgets::inch(ui, "Pitch Y", &mut p.grid.1);
                widgets::count(ui, "Columns", &mut p.grid.2, 500);
                widgets::count(ui, "Rows", &mut p.grid.3, 500);
                footprint_ui(ui, &mut p.grid_shape);
            }
            DrawTool::BoltCircle => {
                widgets::inch(ui, "Radius", &mut p.bolt.0);
                widgets::count(ui, "Holes", &mut p.bolt.1, 72);
                widgets::inch(ui, "Hole Ø", &mut p.bolt.2);
            }
            DrawTool::Hit => {
                ui.label(RichText::new("Station").color(theme::TEXT_DIM));
                ui.add(DragValue::new(&mut p.hit_station).range(1..=99).prefix("T"));
                ui.end_row();
            }
            DrawTool::Select => {}
        });
        // live tool preview for the pending feature
        if let Some(m) = &app.machine {
            let probe = match t {
                DrawTool::Hole => Some(Feature::Hole { x: 0.0, y: 0.0, dia: app.params.hole_dia }),
                DrawTool::Slot => Some(Feature::Slot { x: 0.0, y: 0.0, length: app.params.slot_len, width: app.params.slot_w, vertical: app.params.slot_vertical }),
                DrawTool::Grid => Some(Feature::Grid { x: 0.0, y: 0.0, dx: app.params.grid.0, dy: app.params.grid.1, nx: 1, ny: 1, shape: app.params.grid_shape }),
                DrawTool::BoltCircle => Some(Feature::BoltCircle { x: 0.0, y: 0.0, r: app.params.bolt.0, n: app.params.bolt.1, dia: app.params.bolt.2, start: 0.0 }),
                DrawTool::Hit => Some(Feature::Hit { x: 0.0, y: 0.0, tool: ToolRef::Station(app.params.hit_station), angle: 0.0 }),
                _ => None,
            };
            if let Some(f) = probe {
                ui.add_space(4.0);
                tool_result(ui, m, &resolve(m, &f));
            }
        }
        ui.add_space(4.0);
        ui.label(RichText::new(if t == DrawTool::Rect { "Drag on the part to draw the opening." } else { "Click on the part to place it." }).size(12.0).color(theme::TEXT_DIM));
    });
}

fn tool_result(ui: &mut egui::Ui, m: &Machine, r: &Result<(Vec<u32>, usize), String>) {
    match r {
        Ok((st, hits)) => {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(icons::CHECK).font(theme::icon_font(12.0)).color(theme::OK));
                for s in st {
                    let name = m.station(*s).and_then(|x| x.tool_name()).unwrap_or("?");
                    widgets::chip(ui, &format!("T{}  {}", s, name), station_colour(*s));
                }
                ui.label(RichText::new(format!("{} hit{}", hits, if *hits == 1 { "" } else { "s" })).size(11.5).color(theme::TEXT_DIM));
            });
        }
        Err(e) => widgets::issue_line(ui, true, e),
    }
}

fn inspector(app: &mut App, ui: &mut egui::Ui) {
    if app.draw != DrawTool::Select {
        new_feature_params(app, ui);
        ui.add_space(8.0);
    }
    if let Some(i) = app.sel_feature {
        let resolved = match (&app.machine, app.current_part().and_then(|p| p.features.get(i))) {
            (Some(m), Some(f)) => Some(resolve(m, f)),
            _ => None,
        };
        let mut delete = false;
        let mut dup = false;
        let label = app.current_part().and_then(|p| p.features.get(i)).map(feature_label).unwrap_or_default();
        let glyph = app.current_part().and_then(|p| p.features.get(i)).map(feature_glyph).unwrap_or(Glyph::Select);
        widgets::card(ui, |ui| {
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::hover());
                icons::paint(ui.painter(), r, glyph, theme::SELECT);
                ui.label(RichText::new(label).strong());
            });
            ui.add_space(4.0);
            if let Some(f) = app.project.parts.get_mut(app.sel_part).and_then(|p| p.features.get_mut(i)) {
                feature_editor(ui, f);
            }
            ui.add_space(6.0);
            ui.label(RichText::new("Punched with").size(11.5).color(theme::TEXT_FAINT));
            if let (Some(m), Some(r)) = (&app.machine, &resolved) {
                tool_result(ui, m, r);
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                dup = widgets::button(ui, icons::COPY, "Duplicate").clicked();
                delete = widgets::button(ui, icons::DELETE, "Delete").clicked();
            });
        });
        if delete {
            if let Some(p) = app.current_part_mut() {
                if i < p.features.len() {
                    p.features.remove(i);
                }
            }
            app.sel_feature = None;
        } else if dup {
            if let Some(p) = app.current_part_mut() {
                if let Some(f) = p.features.get(i).cloned() {
                    p.features.push(f.transformed(0, 0.5, 0.5));
                    let n = p.features.len() - 1;
                    app.sel_feature = Some(n);
                }
            }
        }
        return;
    }
    // part properties
    let machine = app.machine.clone();
    let Some(part) = app.project.parts.get_mut(app.sel_part) else { return };
    widgets::titled_card(ui, icons::DOCUMENT, "Part", |ui| {
        egui::Grid::new("partprops").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            ui.label(RichText::new("Name").color(theme::TEXT_DIM));
            ui.text_edit_singleline(&mut part.name);
            ui.end_row();
            ui.label(RichText::new("Outline").color(theme::TEXT_DIM));
            ui.horizontal(|ui| {
                let is_rect = matches!(part.outline, Outline::Rect { .. });
                if ui.selectable_label(is_rect, "Rectangle").clicked() && !is_rect {
                    let b = part.bbox();
                    part.outline = Outline::Rect { w: if b.is_empty() { 20.0 } else { b.w() }, h: if b.is_empty() { 12.0 } else { b.h() }, tabs: true };
                }
                if ui.selectable_label(matches!(part.outline, Outline::None), "Whole sheet").on_hover_text("No outline cut: the part is the sheet").clicked() {
                    part.outline = Outline::None;
                }
            });
            ui.end_row();
            if let Outline::Rect { w, h, tabs } = &mut part.outline {
                widgets::inch(ui, "Width", w);
                widgets::inch(ui, "Height", h);
                ui.label(RichText::new("Microjoints").color(theme::TEXT_DIM));
                ui.checkbox(tabs, "keep part in the sheet");
                ui.end_row();
            }
        });
        if let Outline::Polygon { .. } = part.outline {
            widgets::issue_line(ui, false, "Imported outline isn't a rectangle. It will be shown but not cut.");
        }
    });
    ui.add_space(8.0);
    // tools this part needs
    if let Some(m) = &machine {
        let mut st: Vec<u32> = vec![];
        let mut problems = 0;
        let mut hits = 0;
        for f in &part.features {
            match resolve(m, f) {
                Ok((s, h)) => {
                    st.extend(s);
                    hits += h;
                }
                Err(_) => problems += 1,
            }
        }
        st.sort();
        st.dedup();
        widgets::titled_card(ui, icons::TOOLBOX, "Turret tools needed", |ui| {
            if st.is_empty() {
                ui.label(RichText::new("No features yet.").color(theme::TEXT_DIM));
            }
            for s in &st {
                let name = m.station(*s).and_then(|x| x.tool_name()).unwrap_or("?");
                ui.horizontal(|ui| {
                    widgets::chip(ui, &format!("T{}", s), station_colour(*s));
                    ui.label(RichText::new(name).size(12.5));
                });
            }
            ui.add_space(2.0);
            ui.label(RichText::new(format!("About {} hits for the features (outline extra)", hits)).size(11.5).color(theme::TEXT_DIM));
            if problems > 0 {
                widgets::issue_line(ui, true, &format!("{} feature(s) have no matching tool", problems));
            }
        });
    }
    if !part.source.is_empty() {
        ui.add_space(6.0);
        ui.label(RichText::new(format!("Imported from {}", part.source)).size(11.0).color(theme::TEXT_FAINT));
    }
}

// ---------------------------------------------------------------------------- canvas
fn pick(part: &Part, p: Pt, tol: f64) -> Option<usize> {
    let mut best: Option<(usize, f64)> = None;
    for (i, f) in part.features.iter().enumerate() {
        let b = f.bbox();
        let bb = BBox::new(b.x0 - tol, b.y0 - tol, b.x1 + tol, b.y1 + tol);
        if bb.contains(p) {
            let a = bb.w() * bb.h();
            if best.map(|x| a < x.1).unwrap_or(true) {
                best = Some((i, a));
            }
        }
    }
    best.map(|b| b.0)
}

fn dimension(p: &egui::Painter, v: &View, r: egui::Rect, a: Pt, b: Pt, offset: egui::Vec2, text: String) {
    let c = theme::TEXT_FAINT;
    let dir = offset.normalized();
    let (ga, gb) = (v.to_screen(r, a) + dir * 6.0, v.to_screen(r, b) + dir * 6.0);
    let sa = v.to_screen(r, a) + offset;
    let sb = v.to_screen(r, b) + offset;
    p.line_segment([ga, sa + dir * 5.0], Stroke::new(0.8, c));
    p.line_segment([gb, sb + dir * 5.0], Stroke::new(0.8, c));
    p.line_segment([sa, sb], Stroke::new(1.0, c));
    let along = (sb - sa).normalized();
    let perp = vec2(-along.y, along.x);
    for (tip, d) in [(sa, along), (sb, -along)] {
        p.add(egui::Shape::convex_polygon(vec![tip, tip + d * 8.0 + perp * 3.0, tip + d * 8.0 - perp * 3.0], c, Stroke::NONE));
    }
    let mid = sa + (sb - sa) / 2.0;
    let g = p.layout_no_wrap(text, FontId::proportional(12.0), theme::TEXT_DIM);
    let tr = egui::Rect::from_center_size(mid, g.size() + vec2(10.0, 2.0));
    p.rect_filled(tr, 4.0, theme::CANVAS);
    p.galley(tr.min + vec2(5.0, 1.0), g, theme::TEXT_DIM);
}

fn canvas_ui(app: &mut App, ui: &mut egui::Ui) {
    let (resp, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
    let r = resp.rect;
    painter.rect_filled(r, 0.0, theme::CANVAS);
    let Some(part) = app.project.parts.get(app.sel_part).cloned() else { return };
    let mut v: View = app.part_view;
    if v.needs_fit {
        let mut b = part.bbox();
        if b.is_empty() {
            b = BBox::new(0.0, 0.0, 20.0, 12.0);
        }
        let pad = (b.w().max(b.h()) * 0.12).max(1.0);
        v.fit(r.shrink2(vec2(70.0, 30.0)), BBox::new(b.x0 - pad, b.y0 - pad, b.x1 + pad * 0.3, b.y1 + pad * 0.3));
    }
    v.interact(ui, &resp, r);
    canvas::grid(&painter, &v, r, if app.snap > 0.0 { app.snap.max(0.25) } else { 1.0 }, theme::GRID);
    canvas::grid(&painter, &v, r, 1.0, theme::GRID_MAJOR);
    let o = v.to_screen(r, (0.0, 0.0));
    painter.line_segment([o, o + vec2(16.0, 0.0)], Stroke::new(1.5, theme::ERR.gamma_multiply(0.8)));
    painter.line_segment([o, o - vec2(0.0, 16.0)], Stroke::new(1.5, theme::OK.gamma_multiply(0.8)));
    if let Some(pts) = canvas::outline_geometry(&part) {
        canvas::poly(&painter, &v, r, &pts, true, Stroke::new(1.6, theme::GEOMETRY), Some(Color32::from_rgba_unmultiplied(255, 255, 255, 6)));
        let b = part.bbox();
        dimension(&painter, &v, r, (b.x0, b.y0), (b.x1, b.y0), vec2(0.0, 30.0), format!("{} in", trim(b.w())));
        dimension(&painter, &v, r, (b.x0, b.y0), (b.x0, b.y1), vec2(-34.0, 0.0), format!("{} in", trim(b.h())));
    }
    let mach = app.machine.clone();
    let shape_of = move |t: &ToolRef| tool_shape(&mach, t);
    let resolved: Vec<Option<Result<(Vec<u32>, usize), String>>> = part.features.iter().map(|f| app.machine.as_ref().map(|m| resolve(m, f))).collect();
    for (i, f) in part.features.iter().enumerate() {
        let selected = app.sel_feature == Some(i);
        let hovered = app.hover_feature == Some(i);
        let bad = matches!(resolved[i], Some(Err(_)));
        let base = match &resolved[i] {
            Some(Ok((st, _))) if !st.is_empty() => station_colour(st[0]),
            Some(Err(_)) => theme::ERR,
            _ => theme::GEOMETRY,
        };
        let col = if selected { theme::SELECT } else if hovered { theme::HOVER } else { base };
        for (pts, closed) in canvas::feature_geometry(f, &shape_of) {
            let fill = if closed && !bad { Some(Color32::from_rgba_unmultiplied(col.r(), col.g(), col.b(), if selected { 50 } else { 22 })) } else { None };
            canvas::poly(&painter, &v, r, &pts, closed, Stroke::new(if selected { 2.0 } else { 1.3 }, col), fill);
        }
    }
    app.hover_feature = None;

    // ---- interaction
    let hover = resp.hover_pos().map(|hp| snap(v.to_world(r, hp), app.snap));
    let raw_hover = resp.hover_pos().map(|hp| v.to_world(r, hp));
    let tol = (6.0 / v.scale) as f64;
    if app.draw == DrawTool::Select {
        if let Some(h) = raw_hover {
            app.hover_feature = pick(&part, h, tol);
        }
    }
    if let Some(h) = hover {
        let s = v.to_screen(r, h);
        let c = if app.draw == DrawTool::Select { theme::TEXT_FAINT } else { theme::ACCENT };
        painter.line_segment([s - vec2(9.0, 0.0), s + vec2(9.0, 0.0)], Stroke::new(1.0, c));
        painter.line_segment([s - vec2(0.0, 9.0), s + vec2(0.0, 9.0)], Stroke::new(1.0, c));
        // live preview of the feature that would be placed
        let p = &app.params;
        let ghost = match app.draw {
            DrawTool::Hole => Some(Feature::Hole { x: h.0, y: h.1, dia: p.hole_dia }),
            DrawTool::Slot => Some(Feature::Slot { x: h.0, y: h.1, length: p.slot_len, width: p.slot_w, vertical: p.slot_vertical }),
            DrawTool::Grid => Some(Feature::Grid { x: h.0, y: h.1, dx: p.grid.0, dy: p.grid.1, nx: p.grid.2, ny: p.grid.3, shape: p.grid_shape }),
            DrawTool::BoltCircle => Some(Feature::BoltCircle { x: h.0, y: h.1, r: p.bolt.0, n: p.bolt.1, dia: p.bolt.2, start: 0.0 }),
            DrawTool::Hit => Some(Feature::Hit { x: h.0, y: h.1, tool: ToolRef::Station(p.hit_station), angle: 0.0 }),
            _ => None,
        };
        if let Some(g) = ghost {
            for (pts, closed) in canvas::feature_geometry(&g, &shape_of) {
                canvas::poly(&painter, &v, r, &pts, closed, Stroke::new(1.0, theme::ACCENT.gamma_multiply(0.8)), None);
            }
        }
    }
    let primary_click = resp.clicked_by(egui::PointerButton::Primary);
    match app.draw {
        DrawTool::Select => {
            if resp.drag_started_by(egui::PointerButton::Primary) {
                if let (Some(h), Some(raw)) = (hover, raw_hover) {
                    app.sel_feature = pick(&part, raw, tol);
                    app.drag_start = Some(h);
                    app.drag_orig = app.sel_feature.and_then(|i| part.features.get(i).cloned());
                }
            }
            if resp.dragged_by(egui::PointerButton::Primary) {
                if let (Some(start), Some(h), Some(orig), Some(i)) = (app.drag_start, hover, app.drag_orig.clone(), app.sel_feature) {
                    if let Some(f) = app.project.parts.get_mut(app.sel_part).and_then(|p| p.features.get_mut(i)) {
                        *f = orig.transformed(0, h.0 - start.0, h.1 - start.1);
                    }
                }
            }
            if resp.drag_stopped() {
                app.drag_start = None;
                app.drag_orig = None;
            }
            if primary_click {
                if let Some(raw) = raw_hover {
                    app.sel_feature = pick(&part, raw, tol);
                }
            }
        }
        DrawTool::Rect => {
            if resp.drag_started_by(egui::PointerButton::Primary) {
                app.drag_start = hover;
            }
            if let (Some(a), Some(b)) = (app.drag_start, hover) {
                if resp.dragged_by(egui::PointerButton::Primary) {
                    canvas::poly(&painter, &v, r, &[(a.0, a.1), (b.0, a.1), (b.0, b.1), (a.0, b.1)], true, Stroke::new(1.5, theme::ACCENT), Some(theme::ACCENT.gamma_multiply(0.12)));
                    painter.text(v.to_screen(r, b) + vec2(12.0, 12.0), Align2::LEFT_TOP, format!("{} × {}", trim((b.0 - a.0).abs()), trim((b.1 - a.1).abs())), FontId::monospace(12.0), theme::ACCENT);
                }
                if resp.drag_stopped() {
                    if (b.0 - a.0).abs() > 0.05 && (b.1 - a.1).abs() > 0.05 {
                        let f = Feature::Rect { x0: a.0.min(b.0), y0: a.1.min(b.1), x1: a.0.max(b.0), y1: a.1.max(b.1), tabs: app.params.rect_tabs };
                        add_feature(app, f);
                    }
                    app.drag_start = None;
                }
            }
        }
        tool => {
            if primary_click {
                if let Some(h) = hover {
                    let p = &app.params;
                    let f = match tool {
                        DrawTool::Hole => Feature::Hole { x: h.0, y: h.1, dia: p.hole_dia },
                        DrawTool::Slot => Feature::Slot { x: h.0, y: h.1, length: p.slot_len, width: p.slot_w, vertical: p.slot_vertical },
                        DrawTool::Grid => Feature::Grid { x: h.0, y: h.1, dx: p.grid.0, dy: p.grid.1, nx: p.grid.2, ny: p.grid.3, shape: p.grid_shape },
                        DrawTool::BoltCircle => Feature::BoltCircle { x: h.0, y: h.1, r: p.bolt.0, n: p.bolt.1, dia: p.bolt.2, start: 0.0 },
                        _ => Feature::Hit { x: h.0, y: h.1, tool: ToolRef::Station(p.hit_station), angle: 0.0 },
                    };
                    add_feature(app, f);
                }
            }
        }
    }
    let keys = ui.input(|i| (i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace), i.key_pressed(egui::Key::Escape)));
    if keys.0 && !ui.ctx().egui_wants_keyboard_input() {
        if let Some(i) = app.sel_feature.take() {
            if let Some(p) = app.current_part_mut() {
                if i < p.features.len() {
                    p.features.remove(i);
                }
            }
        }
    }
    if keys.1 {
        app.draw = DrawTool::Select;
        app.sel_feature = None;
    }

    // ---- overlays
    let ctx = ui.ctx().clone();
    // tool palette
    egui::Area::new(ui.scope_id().with("palette")).fixed_pos(r.left_top() + vec2(12.0, 12.0)).order(egui::Order::Foreground).show(&ctx, |ui| {
        egui::Frame::new().fill(theme::PANEL).stroke(Stroke::new(1.0, theme::BORDER)).corner_radius(10).inner_margin(Margin::same(4)).show(ui, |ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, 2.0);
            for t in DrawTool::ALL {
                if widgets::glyph_button(ui, t.glyph(), &format!("{}   ({})", t.name(), t.key()), app.draw == t).clicked() {
                    app.draw = t;
                }
                if t == DrawTool::Select {
                    ui.add_space(4.0);
                }
            }
        });
    });
    // readouts
    if let Some(h) = hover {
        painter.text(r.left_bottom() + vec2(14.0, -12.0), Align2::LEFT_BOTTOM, format!("X {:>9.4}   Y {:>9.4}", h.0, h.1), FontId::monospace(12.5), theme::TEXT_DIM);
    }
    egui::Area::new(ui.scope_id().with("viewctl")).fixed_pos(r.right_bottom() - vec2(214.0, 46.0)).order(egui::Order::Foreground).show(&ctx, |ui| {
        egui::Frame::new().fill(theme::PANEL).stroke(Stroke::new(1.0, theme::BORDER)).corner_radius(8).inner_margin(Margin::symmetric(6, 3)).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Snap").size(12.0).color(theme::TEXT_DIM));
                egui::ComboBox::from_id_salt("snap").width(70.0).selected_text(if app.snap == 0.0 { "off".to_string() } else { trim(app.snap) }).show_ui(ui, |ui| {
                    for s in [0.0, 0.03125, 0.0625, 0.125, 0.25, 0.5, 1.0] {
                        ui.selectable_value(&mut app.snap, s, if s == 0.0 { "off".into() } else { trim(s) });
                    }
                });
                if widgets::icon_button(ui, icons::FIT, "Fit to part (F)", false).clicked() {
                    app.part_view.needs_fit = true;
                }
            });
        });
    });
    if !app.part_view.needs_fit {
        app.part_view = v;
    } else {
        app.part_view = View { needs_fit: true, ..v };
    }
    if part.features.is_empty() && app.draw == DrawTool::Select {
        painter.text(r.center() + vec2(0.0, r.height() * 0.32), Align2::CENTER_CENTER, "Choose a tool on the left (H hole, S slot, O opening, G louvers) and click on the part", FontId::proportional(13.0), theme::TEXT_FAINT);
    }
}

fn add_feature(app: &mut App, f: Feature) {
    if let Some(p) = app.current_part_mut() {
        p.features.push(f);
        let n = p.features.len() - 1;
        app.sel_feature = Some(n);
    }
}

// ---------------------------------------------------------------------------- dialog
pub fn new_part_dialog(app: &mut App, ctx: &egui::Context) {
    let mut open = true;
    let mut create = false;
    egui::Window::new("New part").collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0)).open(&mut open).show(ctx, |ui| {
        ui.set_width(340.0);
        let np = &mut app.new_part;
        egui::Grid::new("np").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
            ui.label(RichText::new("Name").color(theme::TEXT_DIM));
            ui.text_edit_singleline(&mut np.name);
            ui.end_row();
            widgets::inch(ui, "Width", &mut np.w);
            widgets::inch(ui, "Height", &mut np.h);
            ui.label(RichText::new("Corner holes").color(theme::TEXT_DIM));
            ui.checkbox(&mut np.corner_holes, "mounting holes in each corner");
            ui.end_row();
            if np.corner_holes {
                widgets::inch(ui, "Hole Ø", &mut np.hole_dia);
                widgets::inch(ui, "Inset from edge", &mut np.inset);
            }
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            for (n, w, h) in [("Cover 30×20", 30.0, 20.0), ("Dead front 36×20", 36.0, 20.0), ("Panel 24×36", 24.0, 36.0)] {
                if ui.small_button(n).clicked() {
                    np.name = n.split(' ').next().unwrap_or("Part").to_string();
                    np.w = w;
                    np.h = h;
                }
            }
        });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            create = widgets::primary(ui, icons::ADD, "Create part", theme::ACCENT, np.w > 0.0 && np.h > 0.0).clicked();
        });
    });
    if create {
        app.create_new_part();
    } else if !open {
        app.new_part.open = false;
    }
}

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    egui::Panel::left("parts").resizable(true).default_size(300.0).frame(egui::Frame::new().fill(theme::PANEL).inner_margin(Margin::same(12))).show(ui, |ui| {
        egui::ScrollArea::vertical().id_salt("partsscroll").show(ui, |ui| parts_panel(app, ui));
    });
    egui::Panel::right("inspector").resizable(true).default_size(320.0).frame(egui::Frame::new().fill(theme::PANEL).inner_margin(Margin::same(12))).show(ui, |ui| {
        egui::ScrollArea::vertical().id_salt("inspscroll").show(ui, |ui| inspector(app, ui));
    });
    egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| canvas_ui(app, ui));
}
