//! Program step: summary, checks, simulation with timeline, G-code viewer.

use crate::app::App;
use crate::canvas::{self, View};
use crate::icons;
use crate::parts_view::station_colour;
use crate::theme;
use crate::widgets;
use eframe::egui::{self, pos2, vec2, Align2, Color32, FontId, Margin, RichText, Sense, Stroke};
use turretcam_core::cnc::Program;
use turretcam_core::geom::BBox;
use turretcam_core::{Issue, Severity};

struct Current {
    prog: Program,
    issues: Vec<Issue>,
    text: String,
    log: Vec<String>,
    title: String,
    etime: Option<f64>,
    generated: bool,
}

fn current(app: &App) -> Option<Current> {
    if let Some(b) = &app.build {
        Some(Current { prog: b.program.clone(), issues: b.issues.clone(), text: b.text.clone(), log: b.log.clone(), title: format!("O{:04}.CNC", app.project.program_number), etime: Some(b.etime), generated: true })
    } else if let Some((path, p, iss)) = &app.opened_cnc {
        let text = std::fs::read(path).map(|b| b.iter().map(|&c| c as char).collect()).unwrap_or_default();
        Some(Current { prog: p.clone(), issues: iss.clone(), text, log: vec![], title: path.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default(), etime: p.header.etime, generated: false })
    } else {
        None
    }
}

fn summary(app: &mut App, ui: &mut egui::Ui, c: &Current) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("Program").heading());
        ui.label(RichText::new(&c.title).color(theme::TEXT_FAINT));
    });
    if !c.generated {
        ui.label(RichText::new("Opened from disk: view, simulate and check only").size(12.0).color(theme::TEXT_DIM));
    }
    ui.add_space(6.0);
    let mut stations: Vec<u32> = c.prog.hits.iter().filter_map(|h| h.station).collect();
    stations.sort();
    stations.dedup();
    let errors = c.issues.iter().filter(|i| i.severity == Severity::Error).count();
    let warns = c.issues.len() - errors;
    let sheet_use = if c.generated {
        let used: f64 = app.project.placements.iter().filter_map(|pl| app.project.placed_bbox(pl)).map(|b| b.w() * b.h()).sum();
        format!("{:.0}%", 100.0 * used / (app.project.sheet.x * app.project.sheet.y).max(1.0))
    } else {
        "-".into()
    };
    let tw = (ui.available_width() - 8.0) / 2.0 - 22.0;
    let time = c.etime.map(|t| format!("{}:{:02}", (t / 60.0) as u32, (t % 60.0) as u32)).unwrap_or_else(|| "-".into());
    ui.horizontal(|ui| {
        widgets::stat(ui, tw, "punch hits", &c.prog.hits.len().to_string(), theme::TEXT);
        widgets::stat(ui, tw, "tools", &stations.len().to_string(), theme::TEXT);
    });
    ui.horizontal(|ui| {
        widgets::stat(ui, tw, "cycle time (est.)", &time, theme::TEXT);
        widgets::stat(ui, tw, "of sheet used", &sheet_use, theme::TEXT);
    });
    ui.add_space(8.0);
    widgets::card(ui, |ui| {
        ui.horizontal(|ui| {
            let (g, col, txt) = if errors > 0 {
                (icons::ERROR, theme::ERR, format!("{} error{} - can't export", errors, if errors == 1 { "" } else { "s" }))
            } else if warns > 0 {
                (icons::WARNING, theme::WARN, format!("Passed with {} warning{}", warns, if warns == 1 { "" } else { "s" }))
            } else {
                (icons::CHECK, theme::OK, "All machine checks passed".to_string())
            };
            ui.label(RichText::new(g).font(theme::icon_font(15.0)).color(col));
            ui.label(RichText::new(txt).strong().color(col));
        });
        if !c.issues.is_empty() {
            ui.add_space(4.0);
            for i in &c.issues {
                widgets::issue_line(ui, i.severity == Severity::Error, &i.msg);
            }
        }
        ui.add_space(4.0);
        ui.label(RichText::new("Checked: turret contents, auto-index, clamp dead zones, reach, tonnage, header").size(11.0).color(theme::TEXT_FAINT));
    });
    if let Some(b) = &app.build {
        let clamps = b.clamps.clone();
        ui.add_space(8.0);
        widgets::card(ui, |ui| {
            ui.label(RichText::new(format!("Clamps at {}", clamps.iter().map(|c| format!("{}", c)).collect::<Vec<_>>().join(", "))).size(12.5));
            for l in &c.log {
                ui.label(RichText::new(l).size(11.5).color(theme::TEXT_DIM));
            }
        });
    }
    ui.add_space(8.0);
    widgets::titled_card(ui, icons::TOOLBOX, "Tools in order of use", |ui| {
        let mut order: Vec<u32> = vec![];
        for h in &c.prog.hits {
            if let Some(s) = h.station {
                if !order.contains(&s) {
                    order.push(s);
                }
            }
        }
        for st in order {
            let name = c.prog.header.tool(st).map(|t| t.name.clone()).unwrap_or_default();
            let n = c.prog.hits.iter().filter(|h| h.station == Some(st)).count();
            let first = c.prog.hits.iter().position(|h| h.station == Some(st)).unwrap_or(0);
            let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::click());
            if resp.hovered() {
                ui.painter().rect_filled(r, 5.0, theme::CARD_HOVER);
            }
            let col = station_colour(st);
            ui.painter().rect_filled(egui::Rect::from_center_size(pos2(r.left() + 10.0, r.center().y), vec2(10.0, 10.0)), 2.0, col);
            ui.painter().text(pos2(r.left() + 22.0, r.center().y), Align2::LEFT_CENTER, format!("T{}", st), FontId::proportional(12.5), col);
            ui.painter().text(pos2(r.left() + 58.0, r.center().y), Align2::LEFT_CENTER, &name, FontId::proportional(12.5), theme::TEXT);
            ui.painter().text(pos2(r.right() - 6.0, r.center().y), Align2::RIGHT_CENTER, format!("{} hits", n), FontId::proportional(11.5), theme::TEXT_DIM);
            if resp.on_hover_text("Jump the simulation to this tool").clicked() {
                app.sim_running = false;
                app.sim_pos = Some(first + 1);
            }
        }
    });
}

fn sim_canvas(app: &mut App, ui: &mut egui::Ui, c: &Current) {
    let avail = ui.available_size();
    let (resp, painter) = ui.allocate_painter(vec2(avail.x, avail.y - 56.0), Sense::click_and_drag());
    let r = resp.rect;
    painter.rect_filled(r, 0.0, theme::CANVAS);
    let h = &c.prog.header;
    let (sx, sy) = h.sheet.as_ref().map(|s| (s.x, s.y)).unwrap_or((96.0, 48.0));
    let mut v: View = app.prog_view;
    if v.needs_fit {
        v.fit(r.shrink2(vec2(30.0, 30.0)), BBox::new(-1.0, -2.0, sx + 1.0, sy + 1.0));
    }
    v.interact(ui, &resp, r);
    canvas::rect(&painter, &v, r, BBox::new(0.0, 0.0, sx, sy), Stroke::new(1.0, theme::SHEET_EDGE), theme::SHEET);
    if let Some(m) = &app.machine {
        let z = m.spec.clamps.general_dead_zone;
        for cx in &h.clamps {
            canvas::rect(&painter, &v, r, BBox::new(cx - z.left, 0.0, cx + z.right, z.top), Stroke::new(1.0, theme::ERR.gamma_multiply(0.6)), theme::ERR.gamma_multiply(0.07));
            canvas::rect(&painter, &v, r, BBox::new(cx - 1.5, -1.2, cx + 1.5, 0.4), Stroke::new(1.0, theme::TEXT_DIM), theme::CARD);
        }
    }
    let shapes: Vec<(u32, turretcam_core::tooling::ToolShape)> = h.tools.iter().map(|t| (t.station, t.shape())).collect();
    let n = c.prog.hits.len();
    let upto = app.sim_pos.unwrap_or(n).min(n);
    for (i, hit) in c.prog.hits.iter().enumerate() {
        let Some(st) = hit.station else { continue };
        let Some((_, shp)) = shapes.iter().find(|s| s.0 == st) else { continue };
        let col = station_colour(st);
        let pts = shp.outline(hit.x, hit.y, hit.angle, 20);
        if i < upto {
            canvas::poly(&painter, &v, r, &pts, true, Stroke::new(1.0, col), Some(Color32::from_rgba_unmultiplied(col.r(), col.g(), col.b(), 70)));
        } else {
            canvas::poly(&painter, &v, r, &pts, true, Stroke::new(0.6, theme::GRID_MAJOR), None);
        }
    }
    if upto > 0 && upto < n {
        let hit = &c.prog.hits[upto - 1];
        let p = v.to_screen(r, (hit.x, hit.y));
        painter.circle_stroke(p, 10.0, Stroke::new(2.0, theme::SELECT));
        painter.line_segment([p - vec2(16.0, 0.0), p + vec2(16.0, 0.0)], Stroke::new(1.0, theme::SELECT));
        painter.line_segment([p - vec2(0.0, 16.0), p + vec2(0.0, 16.0)], Stroke::new(1.0, theme::SELECT));
        if let Some(nx) = c.prog.hits.get(upto) {
            painter.line_segment([p, v.to_screen(r, (nx.x, nx.y))], Stroke::new(1.0, theme::TEXT_FAINT));
        }
    }
    if !app.prog_view.needs_fit {
        app.prog_view = v;
    }

    // ---- timeline
    let (tr, _) = ui.allocate_exact_size(vec2(avail.x, 56.0), Sense::hover());
    ui.painter().rect_filled(tr, 0.0, theme::PANEL);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(tr.shrink2(vec2(12.0, 10.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
    let ui = &mut child;
    if widgets::icon_button(ui, icons::PREV, "Previous tool", false).clicked() {
        app.sim_running = false;
        app.sim_tool_jump(false);
    }
    let playing = app.sim_running;
    if widgets::icon_button(ui, if playing { icons::PAUSE } else { icons::PLAY }, "Play / pause (Space)", playing).clicked() {
        app.toggle_play();
    }
    if widgets::icon_button(ui, icons::NEXT, "Next tool", false).clicked() {
        app.sim_running = false;
        app.sim_tool_jump(true);
    }
    ui.add_space(8.0);
    let mut pos = app.sim_pos.unwrap_or(n) as u32;
    let w = (ui.available_width() - 260.0).max(120.0);
    ui.spacing_mut().slider_width = w;
    if ui.add(egui::Slider::new(&mut pos, 0..=n as u32).show_value(false)).changed() {
        app.sim_running = false;
        app.sim_pos = if pos as usize >= n { None } else { Some(pos as usize) };
    }
    ui.add_space(8.0);
    let cur = if upto > 0 { c.prog.hits.get(upto - 1) } else { None };
    let label = match cur {
        Some(hh) => {
            let name = hh.station.and_then(|s| h.tool(s)).map(|t| t.name.clone()).unwrap_or_default();
            format!("Hit {} / {}   T{} {}", upto, n, hh.station.unwrap_or(0), name)
        }
        None => format!("Hit 0 / {}", n),
    };
    ui.label(RichText::new(label).monospace().color(theme::TEXT_DIM));
}

fn code_panel(app: &mut App, ui: &mut egui::Ui, c: &Current) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(icons::CODE).font(theme::icon_font(14.0)).color(theme::TEXT_DIM));
        ui.label(RichText::new("G-code").heading());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.checkbox(&mut app.follow_code, "Follow");
        });
    });
    ui.add_space(4.0);
    let ok = app.build.as_ref().map(|b| b.ok()).unwrap_or(false);
    ui.horizontal(|ui| {
        if widgets::primary(ui, icons::DOWNLOAD, "Export CNC", theme::OK.gamma_multiply(0.8), ok).on_disabled_hover_text(if c.generated { "Fix the check errors first" } else { "Opened programs are read-only" }).clicked() {
            app.export_cnc();
        }
        if widgets::button(ui, icons::COPY, "Copy").clicked() {
            ui.ctx().copy_text(c.text.clone());
            app.status = "Program copied to the clipboard".into();
        }
    });
    ui.add_space(6.0);
    let lines: Vec<&str> = c.text.lines().collect();
    let n = c.prog.hits.len();
    let upto = app.sim_pos.unwrap_or(n).min(n);
    let cur_line = if upto > 0 && upto < n { Some(c.prog.hits[upto - 1].line_no) } else { None };
    let row_h = 17.0;
    let id = ui.scope_id().with("code_follow");
    let last: Option<usize> = ui.ctx().data(|d| d.get_temp(id));
    let mut sa = egui::ScrollArea::vertical().id_salt("code").auto_shrink([false, false]);
    if app.follow_code && cur_line.is_some() && cur_line != last {
        let target = (cur_line.unwrap() as f32 - 8.0).max(0.0) * row_h;
        sa = sa.vertical_scroll_offset(target);
        ui.ctx().data_mut(|d| d.insert_temp(id, cur_line));
    }
    egui::Frame::new().fill(theme::CANVAS).corner_radius(8).inner_margin(Margin::same(6)).show(ui, |ui| {
        sa.show_rows(ui, row_h, lines.len(), |ui, range| {
            for i in range {
                let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), row_h), Sense::hover());
                let ln = i + 1;
                let active = Some(ln) == cur_line;
                if active {
                    ui.painter().rect_filled(r, 3.0, theme::ACCENT_DIM);
                }
                ui.painter().text(pos2(r.left() + 34.0, r.center().y), Align2::RIGHT_CENTER, ln.to_string(), FontId::monospace(11.5), theme::TEXT_FAINT);
                let l = lines[i];
                let col = if l.starts_with('(') {
                    theme::OK.gamma_multiply(0.85)
                } else if l.starts_with('G') {
                    theme::HOVER
                } else if l.starts_with('M') || l.starts_with('%') || l.starts_with('O') {
                    theme::WARN
                } else {
                    theme::TEXT
                };
                ui.painter().text(pos2(r.left() + 44.0, r.center().y), Align2::LEFT_CENTER, l, FontId::monospace(12.0), if active { Color32::WHITE } else { col });
            }
        });
    });
}

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    let Some(c) = current(app) else {
        egui::CentralPanel::default().frame(egui::Frame::new().fill(theme::CANVAS)).show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(ui.available_height() * 0.3);
                if let Some(e) = app.build_err.clone() {
                    ui.label(RichText::new(icons::ERROR).font(theme::icon_font(32.0)).color(theme::ERR));
                    ui.label(RichText::new("Couldn't generate the program").heading());
                    ui.label(RichText::new(e).color(theme::TEXT_DIM));
                    ui.add_space(8.0);
                    if ui.button("Back to parts").clicked() {
                        app.step = crate::app::Step::Parts;
                    }
                } else {
                    ui.label(RichText::new(icons::PROCESS).font(theme::icon_font(32.0)).color(theme::TEXT_FAINT));
                    ui.label(RichText::new("No program yet").heading());
                    ui.label(RichText::new("Generate turns every part on the sheet into M-1225 G-code and checks it against the machine.").color(theme::TEXT_DIM));
                    ui.add_space(10.0);
                    if widgets::primary(ui, icons::PROCESS, "Generate program", theme::ACCENT, app.machine.is_some()).clicked() {
                        app.generate();
                    }
                    for w in app.project.preflight() {
                        widgets::issue_line(ui, false, &w);
                    }
                }
            });
        });
        return;
    };
    egui::Panel::left("progsum").resizable(true).default_size(330.0).frame(egui::Frame::new().fill(theme::PANEL).inner_margin(Margin::same(12))).show(ui, |ui| {
        egui::ScrollArea::vertical().id_salt("sumscroll").show(ui, |ui| summary(app, ui, &c));
    });
    egui::Panel::right("code").resizable(true).default_size(360.0).frame(egui::Frame::new().fill(theme::PANEL).inner_margin(Margin::same(12))).show(ui, |ui| code_panel(app, ui, &c));
    egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| sim_canvas(app, ui, &c));
}
