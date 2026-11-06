//! Sheet step: sheet setup, clamps, nesting, part placement.

use crate::app::App;
use crate::canvas::{self, snap, View};
use crate::icons;
use crate::parts_view::{resolve, station_colour};
use crate::theme;
use crate::widgets;
use eframe::egui::{self, vec2, Align2, Color32, DragValue, FontId, Margin, RichText, Sense, Stroke};
use turretcam_core::cam::ToolRef;
use turretcam_core::geom::BBox;
use turretcam_core::nest::{nest, NestOptions};
use turretcam_core::project::Placement;

const MATERIALS: [(&str, f64, &str); 4] = [("ST 42", 0.0315, "22 ga steel"), ("ST 42", 0.0598, "16 ga steel"), ("ALUMINUM 125", 0.125, "1/8 aluminum"), ("SS 304", 0.0625, "16 ga stainless")];

fn settings(app: &mut App, ui: &mut egui::Ui) {
    ui.label(RichText::new("Sheet").heading());
    ui.add_space(4.0);
    widgets::titled_card(ui, icons::DOCUMENT, "Job", |ui| {
        egui::Grid::new("job").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            ui.label(RichText::new("Name").color(theme::TEXT_DIM));
            ui.text_edit_singleline(&mut app.project.name);
            ui.end_row();
            ui.label(RichText::new("CNC comment").color(theme::TEXT_DIM));
            ui.text_edit_singleline(&mut app.project.job).on_hover_text("Order number and description, written into the program header");
            ui.end_row();
            ui.label(RichText::new("Program").color(theme::TEXT_DIM));
            ui.add(DragValue::new(&mut app.project.program_number).range(1..=9999).prefix("O"));
            ui.end_row();
        });
    });
    ui.add_space(8.0);
    widgets::titled_card(ui, icons::LAYERS, "Material and size", |ui| {
        let s = &mut app.project.sheet;
        ui.horizontal_wrapped(|ui| {
            for (x, y) in [(96.0, 48.0), (120.0, 48.0), (72.0, 48.0), (48.0, 48.0)] {
                if ui.selectable_label(s.x == x && s.y == y, format!("{} × {}", x, y)).clicked() {
                    s.x = x;
                    s.y = y;
                    app.sheet_view.needs_fit = true;
                }
            }
        });
        egui::Grid::new("sheet").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            widgets::inch(ui, "Length (X)", &mut s.x);
            widgets::inch(ui, "Width (Y)", &mut s.y);
            ui.label(RichText::new("Material").color(theme::TEXT_DIM));
            let cur = MATERIALS.iter().find(|m| m.0 == s.material && (m.1 - s.thickness).abs() < 1e-4).map(|m| m.2.to_string()).unwrap_or_else(|| format!("{} {}", s.material, s.thickness));
            egui::ComboBox::from_id_salt("mat").selected_text(cur).show_ui(ui, |ui| {
                for (m, t, label) in MATERIALS {
                    if ui.selectable_label(s.material == m && (s.thickness - t).abs() < 1e-4, format!("{}   ({} in {})", label, t, m)).clicked() {
                        s.material = m.into();
                        s.thickness = t;
                    }
                }
            });
            ui.end_row();
            widgets::inch(ui, "Thickness", &mut s.thickness);
        });
        if s.x > 97.07 {
            widgets::issue_line(ui, false, "Sheets longer than 97 in need a reposition, which TurretCAM doesn't generate. Keep hits within X 97.");
        }
    });
    ui.add_space(8.0);
    widgets::titled_card(ui, icons::LAYERS, "Clamps", |ui| {
        let mut auto = app.project.clamps.is_none();
        ui.horizontal(|ui| {
            if ui.selectable_label(auto, "Automatic").on_hover_text("Choose clamp positions clear of every hit").clicked() {
                auto = true;
            }
            if ui.selectable_label(!auto, "Manual").clicked() {
                auto = false;
            }
        });
        if auto && app.project.clamps.is_some() {
            app.project.clamps = None;
        }
        if !auto && app.project.clamps.is_none() {
            app.project.clamps = Some(app.build.as_ref().map(|b| b.clamps.clone()).unwrap_or_else(|| vec![12.0, 48.0, 80.0]));
        }
        if let Some(c) = &mut app.project.clamps {
            ui.horizontal(|ui| {
                for v in c.iter_mut() {
                    ui.add(DragValue::new(v).speed(0.5).max_decimals(2).suffix(" in"));
                }
                if c.len() < 3 && ui.small_button("+").clicked() {
                    c.push(c.last().copied().unwrap_or(12.0) + 30.0);
                }
                if c.len() > 1 && ui.small_button("−").clicked() {
                    c.pop();
                }
            });
        } else if let Some(b) = &app.build {
            ui.label(RichText::new(format!("Last program used {:?}", b.clamps)).size(12.0).color(theme::TEXT_DIM));
        } else {
            ui.label(RichText::new("Placed when the program is generated. Keep features 3.4 in clear of the bottom edge near clamps.").size(12.0).color(theme::TEXT_DIM));
        }
    });
    ui.add_space(8.0);
    widgets::titled_card(ui, icons::GRID, "Nest parts", |ui| {
        for p in &app.project.parts {
            let q = app.nest_qty.entry(p.id.clone()).or_insert(1);
            ui.horizontal(|ui| {
                ui.add(DragValue::new(q).range(0..=500).suffix(" ×"));
                ui.label(&p.name);
            });
        }
        ui.add_space(4.0);
        if widgets::primary(ui, icons::GRID, "Auto-nest", theme::ACCENT, true).on_hover_text("Rows from the lower left, above the 3.5 in clamp strip").clicked() {
            let wanted: Vec<(String, u32)> = app.project.parts.iter().map(|p| (p.id.clone(), *app.nest_qty.get(&p.id).unwrap_or(&1))).collect();
            let gap = app.profile.as_ref().map(|p| p.defaults.nest_gap).unwrap_or(0.5);
            let missed = nest(&mut app.project, &wanted, &NestOptions { gap, ..Default::default() });
            app.status = format!("Nested {} part(s){}", app.project.placements.len(), if missed > 0 { format!(", {} didn't fit", missed) } else { String::new() });
            app.sel_placement = None;
        }
    });
    ui.add_space(8.0);
    widgets::titled_card(ui, icons::DOCUMENT, "On the sheet", |ui| {
        let mut remove = None;
        let n = app.project.placements.len();
        if n == 0 {
            ui.label(RichText::new("Nothing placed yet.").color(theme::TEXT_DIM));
        }
        for i in 0..n {
            let sel = app.sel_placement == Some(i);
            let name = app.project.part(&app.project.placements[i].part).map(|p| p.name.clone()).unwrap_or_default();
            let pl = &mut app.project.placements[i];
            ui.horizontal(|ui| {
                if ui.selectable_label(sel, name).clicked() {
                    app.sel_placement = Some(i);
                }
                ui.add(DragValue::new(&mut pl.x).speed(0.0625).max_decimals(4).prefix("x "));
                ui.add(DragValue::new(&mut pl.y).speed(0.0625).max_decimals(4).prefix("y "));
                if widgets::icon_button(ui, icons::ROTATE, &format!("Rotate 90° (now {}°)", pl.rot), false).clicked() {
                    pl.rot = (pl.rot + 90) % 360;
                }
                if widgets::icon_button(ui, icons::DELETE, "Remove from sheet", false).clicked() {
                    remove = Some(i);
                }
            });
        }
        if let Some(i) = remove {
            app.project.placements.remove(i);
            app.sel_placement = None;
        }
        if let Some(p) = app.current_part() {
            let id = p.id.clone();
            let name = p.name.clone();
            if widgets::button(ui, icons::ADD, &format!("Place {}", name)).clicked() {
                app.project.placements.push(Placement { part: id, x: 0.5, y: 3.5, rot: 0 });
                app.sel_placement = Some(app.project.placements.len() - 1);
            }
        }
        for w in app.project.preflight() {
            widgets::issue_line(ui, false, &w);
        }
    });
}

fn hatch(p: &egui::Painter, v: &View, r: egui::Rect, b: BBox, c: Color32) {
    let a = v.to_screen(r, (b.x0, b.y1));
    let z = v.to_screen(r, (b.x1, b.y0));
    let rr = egui::Rect::from_two_pos(a, z);
    let clip = p.with_clip_rect(rr.intersect(r));
    let step = 9.0;
    let mut x = rr.left() - rr.height();
    while x < rr.right() {
        clip.line_segment([egui::pos2(x, rr.bottom()), egui::pos2(x + rr.height(), rr.top())], Stroke::new(1.0, c));
        x += step;
    }
}

fn sheet_canvas(app: &mut App, ui: &mut egui::Ui) {
    let (resp, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
    let r = resp.rect;
    painter.rect_filled(r, 0.0, theme::CANVAS);
    let pr = app.project.clone();
    let mut v: View = app.sheet_view;
    if v.needs_fit {
        v.fit(r.shrink2(vec2(30.0, 50.0)), BBox::new(-1.0, -3.0, pr.sheet.x + 1.0, pr.sheet.y + 1.0));
    }
    v.interact(ui, &resp, r);
    canvas::rect(&painter, &v, r, BBox::new(0.0, 0.0, pr.sheet.x, pr.sheet.y), Stroke::new(1.0, theme::SHEET_EDGE), theme::SHEET);
    if let Some(m) = &app.machine {
        let z = m.spec.clamps.general_dead_zone;
        hatch(&painter, &v, r, BBox::new(0.0, 0.0, pr.sheet.x, z.top), theme::WARN.gamma_multiply(0.18));
        match pr.clamps.clone().or_else(|| app.build.as_ref().map(|b| b.clamps.clone())) {
            Some(clamps) => {
                for cx in clamps {
                    canvas::rect(&painter, &v, r, BBox::new(cx - z.left, 0.0, cx + z.right, z.top), Stroke::new(1.0, theme::ERR.gamma_multiply(0.7)), theme::ERR.gamma_multiply(0.08));
                    canvas::rect(&painter, &v, r, BBox::new(cx - 1.5, -1.2, cx + 1.5, 0.4), Stroke::new(1.0, theme::TEXT_DIM), theme::CARD);
                    painter.text(v.to_screen(r, (cx, -1.4)), Align2::CENTER_TOP, format!("clamp {}", cx), FontId::proportional(11.0), theme::TEXT_DIM);
                }
            }
            None => {
                painter.text(v.to_screen(r, (pr.sheet.x / 2.0, -0.6)), Align2::CENTER_TOP, "clamps are placed automatically when the program is generated", FontId::proportional(11.5), theme::TEXT_FAINT);
            }
        }
        let ((_, xmax), _) = m.reach();
        if xmax < pr.sheet.x {
            let a = v.to_screen(r, (xmax, 0.0));
            let b = v.to_screen(r, (xmax, pr.sheet.y));
            painter.line_segment([a, b], Stroke::new(1.0, theme::ERR));
            painter.text(b + vec2(4.0, 2.0), Align2::LEFT_TOP, "reach limit", FontId::proportional(11.0), theme::ERR);
        }
    }
    let shape_of = |t: &ToolRef| app.tool_shape(t);
    let mut used_area = 0.0;
    for (i, pl) in pr.placements.iter().enumerate() {
        let Some(part) = pr.part(&pl.part) else { continue };
        let Some(bb) = pr.placed_bbox(pl) else { continue };
        used_area += bb.w() * bb.h();
        let sel = app.sel_placement == Some(i);
        let pb = part.bbox();
        let mut rb = BBox::empty();
        for p in [(pb.x0, pb.y0), (pb.x1, pb.y0), (pb.x0, pb.y1), (pb.x1, pb.y1)] {
            rb.add(turretcam_core::geom::xform(p, pl.rot as f64, 0.0, 0.0));
        }
        let (dx, dy) = (pl.x - rb.x0, pl.y - rb.y0);
        let ol = canvas::outline_geometry(part).unwrap_or_else(|| vec![(pb.x0, pb.y0), (pb.x1, pb.y0), (pb.x1, pb.y1), (pb.x0, pb.y1)]);
        canvas::poly(&painter, &v, r, &canvas::transform_pts(&ol, pl.rot, dx, dy), true, Stroke::new(if sel { 2.0 } else { 1.2 }, if sel { theme::SELECT } else { theme::GEOMETRY }), Some(Color32::from_rgba_unmultiplied(255, 255, 255, if sel { 14 } else { 6 })));
        for f in &part.features {
            let tf = f.transformed(pl.rot, dx, dy);
            let col = app.machine.as_ref().and_then(|m| resolve(m, f).ok()).and_then(|(s, _)| s.first().copied()).map(station_colour).unwrap_or(theme::ERR);
            for (pts, closed) in canvas::feature_geometry(&tf, &shape_of) {
                canvas::poly(&painter, &v, r, &pts, closed, Stroke::new(1.0, col), None);
            }
        }
        let c = v.to_screen(r, ((bb.x0 + bb.x1) / 2.0, bb.y1));
        painter.text(c + vec2(0.0, 6.0), Align2::CENTER_TOP, &part.name, FontId::proportional(12.0), theme::TEXT_FAINT);
    }
    // header overlay
    let util = 100.0 * used_area / (pr.sheet.x * pr.sheet.y).max(1.0);
    painter.text(r.left_top() + vec2(16.0, 14.0), Align2::LEFT_TOP, format!("{} × {} in   ·   {} part{}   ·   {:.0}% of sheet used", pr.sheet.x, pr.sheet.y, pr.placements.len(), if pr.placements.len() == 1 { "" } else { "s" }, util), FontId::proportional(13.0), theme::TEXT_DIM);
    painter.text(r.left_bottom() + vec2(16.0, -12.0), Align2::LEFT_BOTTOM, "Drag parts to move   ·   R rotate   ·   Delete remove   ·   hatched strip = clamp zone", FontId::proportional(12.0), theme::TEXT_FAINT);

    let hover = resp.hover_pos().map(|hp| v.to_world(r, hp));
    let pick = |p: (f64, f64)| pr.placements.iter().enumerate().rev().find(|(_, pl)| pr.placed_bbox(pl).map(|b| b.contains(p)).unwrap_or(false)).map(|(i, _)| i);
    if resp.clicked_by(egui::PointerButton::Primary) {
        app.sel_placement = hover.and_then(pick);
    }
    if resp.drag_started_by(egui::PointerButton::Primary) {
        if let Some(h) = hover {
            app.sel_placement = pick(h);
            if let Some(i) = app.sel_placement {
                app.drag_start = Some((h.0 - pr.placements[i].x, h.1 - pr.placements[i].y));
            }
        }
    }
    if resp.dragged_by(egui::PointerButton::Primary) {
        if let (Some(i), Some(off), Some(h)) = (app.sel_placement, app.drag_start, hover) {
            let s = snap((h.0 - off.0, h.1 - off.1), 0.0625);
            app.project.placements[i].x = s.0;
            app.project.placements[i].y = s.1;
        }
    }
    if resp.drag_stopped() {
        app.drag_start = None;
    }
    if let Some(i) = app.sel_placement {
        if !ui.ctx().egui_wants_keyboard_input() {
            if ui.input(|inp| inp.key_pressed(egui::Key::R)) && i < app.project.placements.len() {
                app.project.placements[i].rot = (app.project.placements[i].rot + 90) % 360;
            }
            if ui.input(|inp| inp.key_pressed(egui::Key::Delete)) && i < app.project.placements.len() {
                app.project.placements.remove(i);
                app.sel_placement = None;
            }
        }
    }
    let ctx = ui.ctx().clone();
    egui::Area::new(ui.scope_id().with("sheetctl")).fixed_pos(r.right_bottom() - vec2(52.0, 46.0)).order(egui::Order::Foreground).show(&ctx, |ui| {
        egui::Frame::new().fill(theme::PANEL).stroke(Stroke::new(1.0, theme::BORDER)).corner_radius(8).inner_margin(Margin::same(3)).show(ui, |ui| {
            if widgets::icon_button(ui, icons::FIT, "Fit sheet (F)", false).clicked() {
                app.sheet_view.needs_fit = true;
            }
        });
    });
    if !app.sheet_view.needs_fit {
        app.sheet_view = v;
    }
}

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    egui::Panel::left("sheetprops").resizable(true).default_size(340.0).frame(egui::Frame::new().fill(theme::PANEL).inner_margin(Margin::same(12))).show(ui, |ui| {
        egui::ScrollArea::vertical().show(ui, |ui| settings(app, ui));
    });
    egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| sheet_canvas(app, ui));
}
