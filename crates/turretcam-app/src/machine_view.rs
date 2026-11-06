//! Machine step: turret ring, station details and overrides, machine limits, profile.

use crate::app::App;
use crate::canvas::View;
use crate::icons;
use crate::parts_view::station_colour;
use crate::theme;
use crate::widgets;
use eframe::egui::{self, pos2, vec2, Align2, DragValue, FontId, Margin, RichText, Sense, Stroke};
use std::collections::BTreeMap;

fn today() -> String {
    turretcam_core::cam::JobOptions::default().date
}

fn turret_ring(app: &mut App, ui: &mut egui::Ui) {
    let Some(m) = app.machine.clone() else {
        ui.label("No machine loaded.");
        return;
    };
    let (resp, painter) = ui.allocate_painter(ui.available_size(), Sense::click());
    let r = resp.rect;
    painter.rect_filled(r, 0.0, theme::CANVAS);
    let c = r.center() + vec2(0.0, 10.0);
    let rad = (r.width().min(r.height()) / 2.0 - 70.0).max(120.0);
    painter.circle_filled(c, rad + 34.0, theme::PANEL);
    painter.circle_stroke(c, rad + 34.0, Stroke::new(1.0, theme::BORDER));
    painter.circle_stroke(c, rad - 34.0, Stroke::new(1.0, theme::BORDER));
    painter.text(c - vec2(0.0, 14.0), Align2::CENTER_CENTER, &m.turret.turret, FontId::proportional(16.0), theme::TEXT);
    let mounted = m.turret.stations.iter().filter(|s| s.mounted.is_some()).count();
    let stale = m.turret.stations.iter().filter(|s| s.mounted.is_some() && !m.is_current(s.station, &today())).count();
    painter.text(c + vec2(0.0, 10.0), Align2::CENTER_CENTER, format!("{} stations · {} loaded · {} stale", m.turret.stations.len(), mounted, stale), FontId::proportional(12.5), theme::TEXT_DIM);
    painter.text(c + vec2(0.0, 30.0), Align2::CENTER_CENTER, "click a station", FontId::proportional(11.5), theme::TEXT_FAINT);
    let n = m.turret.stations.len().max(1);
    let hover = resp.hover_pos();
    let mut hovered: Option<u32> = None;
    for (i, s) in m.turret.stations.iter().enumerate() {
        let a = std::f32::consts::TAU * i as f32 / n as f32 - std::f32::consts::FRAC_PI_2;
        let p = c + vec2(a.cos(), a.sin()) * rad;
        let sr = match s.size_class.as_str() {
            "A" => 11.0,
            "B" => 15.0,
            "C" => 19.0,
            _ => 24.0,
        };
        let is_cur = s.mounted.is_some() && m.is_current(s.station, &today());
        let sel = app.sel_station == Some(s.station);
        let hov = hover.map(|h| (h - p).length() < sr + 3.0).unwrap_or(false);
        if hov {
            hovered = Some(s.station);
        }
        let col = station_colour(s.station);
        let (fill, stroke) = match (&s.mounted, is_cur) {
            (Some(_), true) => (col.gamma_multiply(0.22), Stroke::new(1.5, col)),
            (Some(_), false) => (theme::WARN.gamma_multiply(0.12), Stroke::new(1.5, theme::WARN)),
            (None, _) => (theme::CANVAS, Stroke::new(1.0, theme::BORDER)),
        };
        painter.circle_filled(p, sr, fill);
        painter.circle_stroke(p, sr, stroke);
        if sel || hov {
            painter.circle_stroke(p, sr + 4.0, Stroke::new(2.0, if sel { theme::SELECT } else { theme::HOVER }));
        }
        // tool footprint inside the station
        if let Some(shp) = s.shape() {
            if shp.known() {
                let (ex, ey) = shp.extent(0.0);
                let k = (sr * 1.3) / (ex.max(ey) as f32).max(0.001);
                let v = View { cx: 0.0, cy: 0.0, scale: k.min(sr * 3.0), needs_fit: false };
                let rr = egui::Rect::from_center_size(p, vec2(sr * 2.0, sr * 2.0));
                crate::canvas::poly(&painter, &v, rr, &shp.outline(0.0, 0.0, 0.0, 20), true, Stroke::new(1.2, if is_cur { col } else { theme::WARN }), None);
            } else {
                painter.text(p, Align2::CENTER_CENTER, "SP", FontId::proportional(10.0), theme::TEXT_DIM);
            }
        }
        if s.autoindex {
            painter.circle_stroke(p, sr + 7.0, Stroke::new(1.0, theme::ACCENT.gamma_multiply(0.7)));
        }
        let lp = c + vec2(a.cos(), a.sin()) * (rad + 52.0);
        painter.text(lp, Align2::CENTER_CENTER, format!("{}", s.station), FontId::proportional(11.0), if s.mounted.is_some() { theme::TEXT_DIM } else { theme::TEXT_FAINT });
    }
    // legend
    let mut y = r.top() + 14.0;
    for (col, txt) in [(station_colour(18), "loaded (same colour as the tool everywhere)"), (theme::WARN, "stale: not used for over a year"), (theme::BORDER, "empty / unknown"), (theme::ACCENT, "outer ring: auto-index (rotates)")] {
        painter.circle_filled(pos2(r.left() + 20.0, y + 7.0), 5.0, col);
        painter.text(pos2(r.left() + 32.0, y), Align2::LEFT_TOP, txt, FontId::proportional(12.0), theme::TEXT_DIM);
        y += 18.0;
    }
    if let Some(st) = hovered {
        let name = m.station(st).and_then(|s| s.tool_name()).unwrap_or("empty");
        resp.clone().on_hover_text(format!("T{}  {}", st, name));
        if resp.clicked() {
            app.sel_station = Some(st);
        }
    }
}

fn station_panel(app: &mut App, ui: &mut egui::Ui) {
    let Some(m) = app.machine.clone() else { return };
    let Some(st) = app.sel_station.and_then(|n| m.station(n)).cloned() else {
        widgets::card(ui, |ui| {
            ui.label(RichText::new("Station details").strong());
            ui.label(RichText::new("Click a station on the turret to see its tool, history and override.").color(theme::TEXT_DIM));
        });
        return;
    };
    widgets::card(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("T{}", st.station)).size(22.0).strong().color(station_colour(st.station)));
            ui.vertical(|ui| {
                ui.label(RichText::new(st.tool_name().unwrap_or("Empty / unknown")).strong());
                ui.label(RichText::new(format!("{}-station · up to {} in{}", st.size_class, st.max_tool_in, if st.autoindex { " · auto-index" } else { "" })).size(12.0).color(theme::TEXT_DIM));
            });
        });
        ui.add_space(6.0);
        if let Some(mt) = &st.mounted {
            egui::Grid::new("stinfo").num_columns(2).spacing([12.0, 4.0]).show(ui, |ui| {
                ui.label(RichText::new("Mount angle").color(theme::TEXT_DIM));
                ui.label(if st.autoindex { "rotates (C axis)".into() } else { format!("{}°", mt.mount_angle) });
                ui.end_row();
                ui.label(RichText::new("Last used").color(theme::TEXT_DIM));
                let cur = m.is_current(st.station, &today());
                let when = match mt.last_used.as_str() {
                    "" => "not recorded",
                    "override" => "pinned by hand",
                    d => d,
                };
                ui.label(RichText::new(when).color(if cur { theme::TEXT } else { theme::WARN }));
                ui.end_row();
                ui.label(RichText::new("Clamp clearance").color(theme::TEXT_DIM));
                ui.label(format!("±{} in, below Y {} in", st.clamp_zone.left, st.clamp_zone.top));
                ui.end_row();
            });
            if !m.is_current(st.station, &today()) {
                widgets::issue_line(ui, false, "Not used for over a year, so it isn't picked automatically. Confirm it's still in the turret and pin it below.");
            }
        }
        if !st.history.is_empty() {
            ui.add_space(6.0);
            ui.label(RichText::new("History in the program archive").size(11.5).color(theme::TEXT_FAINT));
            for h in st.history.iter().take(6) {
                ui.label(RichText::new(format!("{}   {} programs   {} to {}", h.tool, h.programs, h.first, h.last)).size(11.5).color(theme::TEXT_DIM));
            }
        }
    });
    ui.add_space(8.0);
    widgets::titled_card(ui, icons::SYNC, "What's in the turret today", |ui| {
        ui.label(RichText::new("Pin the real tool if the archive is out of date. Type EMPTY for an empty station.").size(12.0).color(theme::TEXT_DIM));
        let e = app.override_edit.entry(st.station).or_default();
        ui.add(egui::TextEdit::singleline(e).hint_text(st.tool_name().unwrap_or("tool name, e.g. RD-0.5-M")).desired_width(f32::INFINITY));
        if widgets::button(ui, icons::SAVE, "Save override").clicked() {
            save_overrides(app);
        }
    });
}

fn save_overrides(app: &mut App) {
    let Some(m) = &app.machine else { return };
    let Some(dir) = m.dir.clone() else { return };
    let path = dir.join("turret_override.json");
    let mut map: BTreeMap<String, serde_json::Value> = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    let mut n = 0;
    for (st, v) in &app.override_edit {
        let v = v.trim();
        if v.is_empty() {
            continue;
        }
        n += 1;
        map.insert(st.to_string(), if v.eq_ignore_ascii_case("EMPTY") { serde_json::Value::Null } else { serde_json::Value::String(v.to_string()) });
    }
    match std::fs::write(&path, serde_json::to_string_pretty(&map).unwrap()) {
        Ok(()) => {
            app.status = format!("Saved {} turret override(s)", n);
            let id = m.id().to_string();
            app.select_machine(&id);
        }
        Err(e) => app.error = Some(e.to_string()),
    }
}

fn machine_panel(app: &mut App, ui: &mut egui::Ui) {
    ui.label(RichText::new("Machine").heading());
    ui.add_space(4.0);
    if let Some(m) = &app.machine {
        let ((x0, x1), (y0, y1)) = m.reach();
        widgets::titled_card(ui, icons::MACHINE, &m.spec.name, |ui| {
            egui::Grid::new("mspec").num_columns(2).spacing([12.0, 4.0]).show(ui, |ui| {
                for (k, v) in [
                    ("Control", m.spec.control.clone()),
                    ("Reach", format!("X {:.2} to {:.2}, Y {:.2} to {:.2} in", x0, x1, y0, y1)),
                    ("Clamps", format!("{}, auto-placed", m.spec.clamps.count)),
                    ("Press", format!("{:.0} US tons", m.capacity_tons())),
                    ("Nibble tolerance", format!("{} in", m.spec.nibble_tolerance_in)),
                ] {
                    ui.label(RichText::new(k).color(theme::TEXT_DIM));
                    ui.label(RichText::new(v).size(12.5));
                    ui.end_row();
                }
            });
            ui.label(RichText::new(&m.spec.source).size(11.0).color(theme::TEXT_FAINT));
        });
    }
    ui.add_space(8.0);
    let mut save = false;
    let oda_found = turretcam_core::import::find_oda(app.profile.as_ref().and_then(|p| p.oda_converter.as_deref()));
    if let Some(p) = &mut app.profile {
        widgets::titled_card(ui, icons::SETTINGS, &format!("Profile: {}", p.name), |ui| {
            egui::Grid::new("prof").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                ui.label(RichText::new("User").color(theme::TEXT_DIM));
                ui.text_edit_singleline(&mut p.user);
                ui.end_row();
                ui.label(RichText::new("Next program").color(theme::TEXT_DIM));
                ui.add(DragValue::new(&mut p.next_program_number).range(1..=9999).prefix("O"));
                ui.end_row();
                ui.label(RichText::new("Export folder").color(theme::TEXT_DIM));
                let mut od = p.output_dir.clone().unwrap_or_default();
                if ui.add(egui::TextEdit::singleline(&mut od).hint_text("folder the machine loads programs from")).changed() {
                    p.output_dir = if od.trim().is_empty() { None } else { Some(od) };
                }
                ui.end_row();
                ui.label(RichText::new("Default sheet").color(theme::TEXT_DIM));
                ui.horizontal(|ui| {
                    ui.add(DragValue::new(&mut p.defaults.sheet[0]).speed(0.5).suffix(" in"));
                    ui.label("×");
                    ui.add(DragValue::new(&mut p.defaults.sheet[1]).speed(0.5).suffix(" in"));
                });
                ui.end_row();
                ui.label(RichText::new("Material").color(theme::TEXT_DIM));
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut p.defaults.material).desired_width(90.0));
                    ui.add(DragValue::new(&mut p.defaults.thickness).speed(0.001).max_decimals(4).suffix(" in"));
                });
                ui.end_row();
                ui.label(RichText::new("Microjoints").color(theme::TEXT_DIM));
                ui.horizontal(|ui| {
                    ui.add(DragValue::new(&mut p.defaults.tab).speed(0.001).max_decimals(4).suffix(" in"));
                    ui.label("every");
                    ui.add(DragValue::new(&mut p.defaults.tab_every).speed(0.5).suffix(" in"));
                });
                ui.end_row();
                ui.label(RichText::new("ODA fallback").color(theme::TEXT_DIM));
                let mut oda = p.oda_converter.clone().unwrap_or_default();
                if ui.add(egui::TextEdit::singleline(&mut oda).hint_text("optional ODAFileConverter path")).changed() {
                    p.oda_converter = if oda.trim().is_empty() { None } else { Some(oda) };
                }
                ui.end_row();
            });
            ui.label(RichText::new(match &oda_found {
                Some(e) => format!("DWG and DXF are read built in (acadrust). ODA fallback found: {}", e.display()),
                None => "DWG (AutoCAD R13 to 2018) and DXF are read built in, nothing to install. ODA is only an optional fallback.".into(),
            }).size(11.0).color(theme::TEXT_FAINT));
            ui.add_space(4.0);
            save = widgets::button(ui, icons::SAVE, "Save profile").clicked();
        });
    }
    if save {
        if let (Some(s), Some(p)) = (&app.store, &app.profile) {
            match s.save(p) {
                Ok(()) => app.status = format!("Saved profile {}", p.id),
                Err(e) => app.error = Some(e.to_string()),
            }
        }
    }
    // switch machine / profile
    ui.add_space(8.0);
    let profiles = app.store.as_ref().map(|s| s.list()).unwrap_or_default();
    let machines = app.profile.as_ref().map(|p| p.machines.clone()).unwrap_or_default();
    if profiles.len() > 1 || machines.len() > 1 {
        widgets::card(ui, |ui| {
            let mut pick_p = None;
            let mut pick_m = None;
            for id in &profiles {
                if ui.selectable_label(app.profile.as_ref().map(|p| &p.id) == Some(id), format!("Profile {}", id)).clicked() {
                    pick_p = Some(id.clone());
                }
            }
            for id in &machines {
                if ui.selectable_label(app.machine.as_ref().map(|m| m.id()) == Some(id.as_str()), format!("Machine {}", id)).clicked() {
                    pick_m = Some(id.clone());
                }
            }
            if let Some(p) = pick_p {
                app.select_profile(&p);
            }
            if let Some(m) = pick_m {
                app.select_machine(&m);
            }
        });
    }
}

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    egui::Panel::left("machinfo").resizable(true).default_size(360.0).frame(egui::Frame::new().fill(theme::PANEL).inner_margin(Margin::same(12))).show(ui, |ui| {
        egui::ScrollArea::vertical().show(ui, |ui| machine_panel(app, ui));
    });
    egui::Panel::right("station").resizable(true).default_size(330.0).frame(egui::Frame::new().fill(theme::PANEL).inner_margin(Margin::same(12))).show(ui, |ui| {
        egui::ScrollArea::vertical().show(ui, |ui| station_panel(app, ui));
    });
    egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| turret_ring(app, ui));
}
