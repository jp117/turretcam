//! Application state, workflow steps, undo/redo, top bar and status bar.

use crate::canvas::View;
use crate::icons::{self, Glyph};
use crate::theme;
use crate::widgets;
use eframe::egui::{self, vec2, Align2, Color32, Margin, RichText, Sense, Stroke};
use std::path::PathBuf;
use turretcam_core::cam::ToolRef;
use turretcam_core::part::{Feature, Footprint, Part};
use turretcam_core::project::{Placement, Project, SheetSpec};
use turretcam_core::tooling::{ToolKind, ToolShape};
use turretcam_core::{BuildResult, Machine, Profile, ProfileStore};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Step {
    Parts,
    Sheet,
    Program,
    Machine,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DrawTool {
    Select,
    Hole,
    Slot,
    Rect,
    Grid,
    BoltCircle,
    Hit,
}

impl DrawTool {
    pub fn glyph(self) -> Glyph {
        match self {
            DrawTool::Select => Glyph::Select,
            DrawTool::Hole => Glyph::Hole,
            DrawTool::Slot => Glyph::Slot,
            DrawTool::Rect => Glyph::Rect,
            DrawTool::Grid => Glyph::Grid,
            DrawTool::BoltCircle => Glyph::Bolt,
            DrawTool::Hit => Glyph::Punch,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            DrawTool::Select => "Select",
            DrawTool::Hole => "Hole",
            DrawTool::Slot => "Slot",
            DrawTool::Rect => "Opening",
            DrawTool::Grid => "Louver grid",
            DrawTool::BoltCircle => "Bolt circle",
            DrawTool::Hit => "Tool hit",
        }
    }
    pub fn key(self) -> &'static str {
        match self {
            DrawTool::Select => "V",
            DrawTool::Hole => "H",
            DrawTool::Slot => "S",
            DrawTool::Rect => "O",
            DrawTool::Grid => "G",
            DrawTool::BoltCircle => "B",
            DrawTool::Hit => "T",
        }
    }
    pub const ALL: [DrawTool; 7] = [DrawTool::Select, DrawTool::Hole, DrawTool::Slot, DrawTool::Rect, DrawTool::Grid, DrawTool::BoltCircle, DrawTool::Hit];
}

pub struct ToolParams {
    pub hole_dia: f64,
    pub slot_len: f64,
    pub slot_w: f64,
    pub slot_vertical: bool,
    pub grid: (f64, f64, u32, u32),
    pub grid_shape: Footprint,
    pub bolt: (f64, u32, f64),
    pub hit_station: u32,
    pub rect_tabs: bool,
}

impl Default for ToolParams {
    fn default() -> Self {
        ToolParams {
            hole_dia: 0.25,
            slot_len: 0.5,
            slot_w: 0.25,
            slot_vertical: false,
            grid: (0.5, 1.25, 10, 4),
            grid_shape: Footprint { kind: ToolKind::OB, w: 0.188, h: 1.0 },
            bolt: (1.5, 6, 0.25),
            hit_station: 46,
            rect_tabs: true,
        }
    }
}

/// "New part" dialog values.
pub struct NewPart {
    pub open: bool,
    pub name: String,
    pub w: f64,
    pub h: f64,
    pub corner_holes: bool,
    pub hole_dia: f64,
    pub inset: f64,
}

impl Default for NewPart {
    fn default() -> Self {
        NewPart { open: false, name: "Cover".into(), w: 30.0, h: 20.0, corner_holes: true, hole_dia: 0.25, inset: 1.0 }
    }
}

/// Command line: `turretcam [job.tcam.json|program.CNC] [--step parts|sheet|program|machine] [--generate] [--sim N] [--select N] [--new-part] [--station N]`
#[derive(Default)]
pub struct Startup {
    pub open: Option<PathBuf>,
    pub step: Option<Step>,
    pub generate: bool,
    pub sim: Option<usize>,
    pub select: Option<usize>,
    pub new_part: bool,
    pub station: Option<u32>,
}

impl Startup {
    pub fn from_args() -> Startup {
        let mut s = Startup::default();
        let mut it = std::env::args().skip(1);
        while let Some(a) = it.next() {
            match a.as_str() {
                "--generate" => s.generate = true,
                "--new-part" => s.new_part = true,
                "--step" | "--tab" | "--module" => {
                    s.step = match it.next().as_deref() {
                        Some("parts") | Some("part") | Some("drawing") => Some(Step::Parts),
                        Some("sheet") | Some("nesting") => Some(Step::Sheet),
                        Some("program") => Some(Step::Program),
                        Some("machine") | Some("machines") | Some("jobs") => Some(Step::Machine),
                        _ => None,
                    }
                }
                "--sim" => s.sim = it.next().and_then(|v| v.parse().ok()),
                "--select" => s.select = it.next().and_then(|v| v.parse().ok()),
                "--station" => s.station = it.next().and_then(|v| v.parse().ok()),
                other => s.open = Some(PathBuf::from(other)),
            }
        }
        s
    }
}

pub struct App {
    pub step: Step,
    pub store: Option<ProfileStore>,
    pub profile: Option<Profile>,
    pub machine: Option<Machine>,
    pub project: Project,
    pub project_path: Option<PathBuf>,
    pub dirty: bool,
    pub status: String,
    pub error: Option<String>,

    // undo
    undo: Vec<Project>,
    redo: Vec<Project>,
    committed: Project,

    // parts step
    pub sel_part: usize,
    pub sel_feature: Option<usize>,
    pub hover_feature: Option<usize>,
    pub draw: DrawTool,
    pub params: ToolParams,
    pub snap: f64,
    pub part_view: View,
    pub drag_start: Option<(f64, f64)>,
    pub drag_orig: Option<Feature>,
    pub new_part: NewPart,

    // sheet step
    pub sheet_view: View,
    pub sel_placement: Option<usize>,
    pub nest_qty: std::collections::BTreeMap<String, u32>,

    // program step
    pub prog_view: View,
    pub build: Option<BuildResult>,
    pub build_err: Option<String>,
    pub opened_cnc: Option<(PathBuf, turretcam_core::cnc::Program, Vec<turretcam_core::Issue>)>,
    pub sim_pos: Option<usize>,
    pub sim_running: bool,
    pub follow_code: bool,

    // machine step
    pub sel_station: Option<u32>,
    pub override_edit: std::collections::BTreeMap<u32, String>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, startup: Startup) -> Self {
        theme::install(&cc.egui_ctx);
        let store = ProfileStore::discover().ok();
        let blank = Project::new("Untitled", "m1225", 9001, SheetSpec { x: 96.0, y: 48.0, thickness: 0.0315, material: "ST 42".into(), qty: 1 });
        let mut app = App {
            step: Step::Parts,
            store,
            profile: None,
            machine: None,
            project: blank.clone(),
            project_path: None,
            dirty: false,
            status: "Ready".into(),
            error: None,
            undo: vec![],
            redo: vec![],
            committed: blank,
            sel_part: 0,
            sel_feature: None,
            hover_feature: None,
            draw: DrawTool::Select,
            params: ToolParams::default(),
            snap: 0.0625,
            part_view: View::default(),
            drag_start: None,
            drag_orig: None,
            new_part: NewPart::default(),
            sheet_view: View::default(),
            sel_placement: None,
            nest_qty: Default::default(),
            prog_view: View::default(),
            build: None,
            build_err: None,
            opened_cnc: None,
            sim_pos: None,
            sim_running: false,
            follow_code: true,
            sel_station: None,
            override_edit: Default::default(),
        };
        let first = app.store.as_ref().and_then(|s| s.list().into_iter().next());
        match first {
            Some(id) => app.select_profile(&id),
            None => app.error = Some("No profiles folder found. Set TURRETCAM_HOME or run from the repository folder.".into()),
        }
        app.new_project();
        if let Some(path) = startup.open {
            app.open_path(path);
        }
        if let Some(s) = startup.step {
            app.step = s;
        }
        if startup.generate {
            app.generate();
        }
        if let Some(n) = startup.sim {
            app.sim_pos = Some(n);
        }
        if let Some(i) = startup.select {
            app.sel_feature = Some(i);
        }
        app.new_part.open = startup.new_part;
        app.sel_station = startup.station;
        app.committed = app.project.clone();
        app
    }

    // ------------------------------------------------------------------ state
    fn open_path(&mut self, path: PathBuf) {
        let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        if ext == "json" {
            match Project::load(&path) {
                Ok(p) => {
                    let m = p.machine.clone();
                    self.project = p;
                    self.project_path = Some(path.clone());
                    self.reset_views();
                    self.select_machine(&m);
                    self.status = format!("Opened {}", path.display());
                    self.undo.clear();
                    self.redo.clear();
                    self.committed = self.project.clone();
                    self.dirty = false;
                }
                Err(e) => self.error = Some(e),
            }
        } else {
            match turretcam_core::cnc::load(&path) {
                Ok(p) => {
                    let issues = self.machine.as_ref().map(|m| turretcam_core::check(&p, m, false)).unwrap_or_default();
                    self.status = format!("Opened {} ({} hits)", path.display(), p.hits.len());
                    self.opened_cnc = Some((path, p, issues));
                    self.build = None;
                    self.step = Step::Program;
                    self.sim_pos = None;
                    self.prog_view.needs_fit = true;
                }
                Err(e) => self.error = Some(e.to_string()),
            }
        }
    }

    pub fn select_profile(&mut self, id: &str) {
        let Some(store) = &self.store else { return };
        match store.load(id) {
            Ok(p) => {
                let m = p.default_machine.clone();
                self.profile = Some(p);
                self.select_machine(&m);
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    pub fn select_machine(&mut self, id: &str) {
        let Some(p) = &self.profile else { return };
        match p.load_machine(id) {
            Ok(m) => {
                self.project.machine = m.id().to_string();
                self.machine = Some(m);
                self.build = None;
                self.override_edit.clear();
            }
            Err(e) => self.error = Some(format!("machine {}: {}", id, e)),
        }
    }

    pub fn new_project(&mut self) {
        let (num, sheet, machine) = match &self.profile {
            Some(p) => (
                p.next_program_number,
                SheetSpec { x: p.defaults.sheet[0], y: p.defaults.sheet[1], thickness: p.defaults.thickness, material: p.defaults.material.clone(), qty: 1 },
                p.default_machine.clone(),
            ),
            None => (9001, self.project.sheet.clone(), "m1225".into()),
        };
        let mut pr = Project::new("Untitled job", &machine, num, sheet);
        if let Some(p) = &self.profile {
            pr.options.tab = p.defaults.tab;
            pr.options.tab_every = p.defaults.tab_every;
            pr.options.min_overlap = p.defaults.min_overlap;
        }
        pr.parts.push(Part::new_rect("P1", "Part 1", 20.0, 12.0));
        pr.placements.push(Placement { part: "P1".into(), x: 0.5, y: 3.5, rot: 0 });
        self.project = pr;
        self.project_path = None;
        self.reset_views();
        self.undo.clear();
        self.redo.clear();
        self.committed = self.project.clone();
        self.dirty = false;
    }

    pub fn reset_views(&mut self) {
        self.sel_part = 0;
        self.sel_feature = None;
        self.sel_placement = None;
        self.part_view.needs_fit = true;
        self.sheet_view.needs_fit = true;
        self.prog_view.needs_fit = true;
        self.build = None;
        self.build_err = None;
        self.opened_cnc = None;
        self.sim_pos = None;
        self.sim_running = false;
    }

    pub fn tool_shape(&self, t: &ToolRef) -> Option<ToolShape> {
        let m = self.machine.as_ref()?;
        let st = match t {
            ToolRef::Station(n) => *n,
            ToolRef::Name(n) => m.station_for_name(n)?,
        };
        m.station(st)?.shape()
    }

    pub fn current_part(&self) -> Option<&Part> {
        self.project.parts.get(self.sel_part)
    }

    pub fn current_part_mut(&mut self) -> Option<&mut Part> {
        self.project.parts.get_mut(self.sel_part)
    }

    /// Record an edit for undo after the frame (called by edits; cheap).
    fn commit_if_changed(&mut self, dragging: bool) {
        if dragging || self.project == self.committed {
            return;
        }
        self.undo.push(std::mem::replace(&mut self.committed, self.project.clone()));
        if self.undo.len() > 200 {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.dirty = true;
        self.build = None;
        self.build_err = None;
    }

    pub fn do_undo(&mut self) {
        if let Some(p) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.project, p.clone()));
            self.committed = p;
            self.build = None;
            self.fix_selection();
            self.status = "Undo".into();
        }
    }

    pub fn do_redo(&mut self) {
        if let Some(p) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.project, p.clone()));
            self.committed = p;
            self.build = None;
            self.fix_selection();
            self.status = "Redo".into();
        }
    }

    fn fix_selection(&mut self) {
        self.sel_part = self.sel_part.min(self.project.parts.len().saturating_sub(1));
        let nf = self.current_part().map(|p| p.features.len()).unwrap_or(0);
        if self.sel_feature.map(|i| i >= nf).unwrap_or(false) {
            self.sel_feature = None;
        }
        if self.sel_placement.map(|i| i >= self.project.placements.len()).unwrap_or(false) {
            self.sel_placement = None;
        }
    }

    // ------------------------------------------------------------------ files
    pub fn open_project(&mut self) {
        if let Some(path) = rfd::FileDialog::new().add_filter("TurretCAM job or CNC", &["json", "CNC", "cnc", "NC"]).pick_file() {
            self.open_path(path);
        }
    }

    pub fn save_project(&mut self, save_as: bool) {
        let path = match (&self.project_path, save_as) {
            (Some(p), false) => Some(p.clone()),
            _ => rfd::FileDialog::new().add_filter("TurretCAM job", &["json"]).set_file_name(format!("{}.tcam.json", self.project.name.replace(' ', "_"))).save_file(),
        };
        if let Some(p) = path {
            match self.project.save(&p) {
                Ok(()) => {
                    self.status = format!("Saved {}", p.display());
                    self.project_path = Some(p);
                    self.dirty = false;
                }
                Err(e) => self.error = Some(e.to_string()),
            }
        }
    }

    pub fn import_drawing(&mut self) {
        let Some(path) = rfd::FileDialog::new().add_filter("Drawings", &["dxf", "dwg", "DXF", "DWG"]).pick_file() else { return };
        let id = self.project.new_part_id();
        let oda = self.profile.as_ref().and_then(|p| p.oda_converter.clone());
        match turretcam_core::import::import_file(&path, &id, oda.as_deref()) {
            Ok(part) => {
                let warn = part.unsupported();
                self.status = format!("Imported {} with {} features{}", part.name, part.features.len(), if warn.is_empty() { String::new() } else { format!(" ({} need attention)", warn.len()) });
                self.project.parts.push(part);
                self.sel_part = self.project.parts.len() - 1;
                self.sel_feature = None;
                self.part_view.needs_fit = true;
                self.step = Step::Parts;
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    pub fn create_new_part(&mut self) {
        let np = &self.new_part;
        let id = self.project.new_part_id();
        let mut p = Part::new_rect(&id, &np.name, np.w, np.h);
        if np.corner_holes {
            for (x, y) in [(np.inset, np.inset), (np.w - np.inset, np.inset), (np.inset, np.h - np.inset), (np.w - np.inset, np.h - np.inset)] {
                p.features.push(Feature::Hole { x, y, dia: np.hole_dia });
            }
        }
        self.project.parts.push(p);
        self.sel_part = self.project.parts.len() - 1;
        self.sel_feature = None;
        self.part_view.needs_fit = true;
        self.new_part.open = false;
        self.step = Step::Parts;
        self.status = format!("Created part {}", id);
    }

    pub fn generate(&mut self) {
        self.opened_cnc = None;
        let Some(m) = &self.machine else {
            self.build_err = Some("no machine loaded".into());
            return;
        };
        match self.project.build(m, None) {
            Ok(r) => {
                self.status = format!("Program generated: {} hits, about {:.0} s{}", r.program.hits.len(), r.etime, if r.ok() { "" } else { ", has check errors" });
                self.build = Some(r);
                self.build_err = None;
                self.prog_view.needs_fit = true;
                self.sim_pos = None;
            }
            Err(e) => {
                self.build = None;
                self.build_err = Some(e.0);
            }
        }
        self.step = Step::Program;
    }

    pub fn export_cnc(&mut self) {
        let Some(b) = &self.build else { return };
        let name = format!("{:04}.CNC", self.project.program_number);
        let mut dlg = rfd::FileDialog::new().add_filter("CNC program", &["CNC"]).set_file_name(&name);
        if let Some(dir) = self.profile.as_ref().and_then(|p| p.output_dir.clone()) {
            dlg = dlg.set_directory(dir);
        }
        let Some(path) = dlg.save_file() else { return };
        if let Err(e) = std::fs::write(&path, &b.text) {
            self.error = Some(e.to_string());
            return;
        }
        self.status = format!("Exported {}", path.display());
        if let (Some(store), Some(p)) = (&self.store, &mut self.profile) {
            if p.next_program_number <= self.project.program_number {
                p.next_program_number = self.project.program_number + 1;
                let _ = store.save(p);
            }
        }
    }

    pub fn sim_len(&self) -> usize {
        self.build.as_ref().map(|b| b.program.hits.len()).or_else(|| self.opened_cnc.as_ref().map(|c| c.1.hits.len())).unwrap_or(0)
    }

    pub fn sim_tool_jump(&mut self, forward: bool) {
        let hits: Vec<Option<u32>> = match (&self.build, &self.opened_cnc) {
            (Some(b), _) => b.program.hits.iter().map(|h| h.station).collect(),
            (None, Some(c)) => c.1.hits.iter().map(|h| h.station).collect(),
            _ => return,
        };
        let n = hits.len();
        let cur = self.sim_pos.unwrap_or(n).min(n);
        let pos = if forward {
            (cur.max(1)..n).find(|&i| hits[i] != hits[i - 1]).map(|i| i + 1).unwrap_or(n)
        } else {
            (1..cur.saturating_sub(1)).rev().find(|&i| hits[i] != hits[i - 1]).map(|i| i + 1).unwrap_or(0)
        };
        self.sim_pos = Some(pos);
    }

    // ------------------------------------------------------------------ chrome
    fn top_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_centered(|ui| {
            ui.add_space(8.0);
            let (r, _) = ui.allocate_exact_size(vec2(26.0, 26.0), Sense::hover());
            icons::logo(ui.painter(), r.center(), 12.0);
            ui.label(RichText::new("TurretCAM").size(16.0).strong());
            ui.add_space(10.0);
            ui.separator();
            ui.add_space(4.0);
            // job name
            let job = format!("{}{}", self.project.name, if self.dirty { "  \u{2022}" } else { "" });
            ui.label(RichText::new(job).color(theme::TEXT_DIM));
            ui.add_space(14.0);
            // workflow steps
            let has_prog = self.build.is_some() || self.opened_cnc.is_some();
            for (i, (s, label, g)) in [(Step::Parts, "Parts", Glyph::Rect), (Step::Sheet, "Sheet", Glyph::Sheet), (Step::Program, "Program", Glyph::Program)].into_iter().enumerate() {
                if i > 0 {
                    ui.label(RichText::new(icons::CHEVRON).font(theme::icon_font(10.0)).color(theme::TEXT_FAINT));
                }
                if step_tab(ui, i + 1, label, g, self.step == s, s == Step::Program && has_prog).clicked() {
                    if s == Step::Program && !has_prog {
                        self.generate();
                    }
                    self.step = s;
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(8.0);
                // primary action
                let ok = self.build.as_ref().map(|b| b.ok());
                match ok {
                    Some(true) => {
                        if widgets::primary(ui, icons::DOWNLOAD, "Export CNC", theme::OK.gamma_multiply(0.8), true).on_hover_text("Save the .CNC program (Ctrl+E)").clicked() {
                            self.export_cnc();
                        }
                    }
                    Some(false) => {
                        let n = self.build.as_ref().map(|b| b.issues.iter().filter(|i| i.severity == turretcam_core::Severity::Error).count()).unwrap_or(0);
                        if widgets::primary(ui, icons::ERROR, &format!("{} error{}", n, if n == 1 { "" } else { "s" }), theme::ERR.gamma_multiply(0.75), true).on_hover_text("The program fails machine checks; see Program").clicked() {
                            self.step = Step::Program;
                        }
                    }
                    None => {
                        if widgets::primary(ui, icons::PROCESS, "Generate program", theme::ACCENT, self.machine.is_some()).on_hover_text("Machine every part on the sheet (Ctrl+G)").clicked() {
                            self.generate();
                        }
                    }
                }
                ui.add_space(6.0);
                if widgets::icon_button(ui, icons::SETTINGS, "Machine, turret and settings", self.step == Step::Machine).clicked() {
                    self.step = Step::Machine;
                }
                let mname = self.machine.as_ref().map(|m| m.spec.name.replace("LVD Strippit ", "")).unwrap_or_else(|| "no machine".into());
                widgets::chip(ui, &mname, theme::ACCENT);
                ui.separator();
                if widgets::icon_button(ui, icons::IMPORT, "Import DXF / DWG part", false).clicked() {
                    self.import_drawing();
                }
                if widgets::icon_button(ui, icons::SAVE, "Save job (Ctrl+S)", false).clicked() {
                    self.save_project(false);
                }
                if widgets::icon_button(ui, icons::FOLDER, "Open job or CNC program (Ctrl+O)", false).clicked() {
                    self.open_project();
                }
                if widgets::icon_button(ui, icons::ADD, "New job", false).clicked() {
                    self.new_project();
                }
                ui.separator();
                if widgets::icon_button(ui, icons::REDO, "Redo (Ctrl+Y)", false).clicked() {
                    self.do_redo();
                }
                if widgets::icon_button(ui, icons::UNDO, "Undo (Ctrl+Z)", false).clicked() {
                    self.do_undo();
                }
            });
        });
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_centered(|ui| {
            ui.add_space(10.0);
            if let Some(e) = self.error.clone() {
                ui.label(RichText::new(icons::ERROR).font(theme::icon_font(12.0)).color(theme::ERR));
                ui.label(RichText::new(e).color(theme::ERR).size(12.0));
                if ui.small_button("Dismiss").clicked() {
                    self.error = None;
                }
            } else {
                ui.label(RichText::new(&self.status).color(theme::TEXT_DIM).size(12.0));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(10.0);
                let s = &self.project.sheet;
                let mut parts = vec![format!("O{:04}", self.project.program_number), format!("{} × {} in  {} {}", s.x, s.y, s.thickness, s.material)];
                if let Some(b) = &self.build {
                    parts.push(format!("{} hits  ~{:.0} s", b.program.hits.len(), b.etime));
                }
                if let Some(p) = &self.profile {
                    parts.push(p.name.clone());
                }
                ui.label(RichText::new(parts.join("   ·   ")).color(theme::TEXT_FAINT).size(12.0));
            });
        });
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        let (z, y, s, o, g, e) = ctx.input(|i| {
            let c = i.modifiers.command;
            (
                c && i.key_pressed(egui::Key::Z) && !i.modifiers.shift,
                c && (i.key_pressed(egui::Key::Y) || (i.modifiers.shift && i.key_pressed(egui::Key::Z))),
                c && i.key_pressed(egui::Key::S),
                c && i.key_pressed(egui::Key::O),
                c && i.key_pressed(egui::Key::G),
                c && i.key_pressed(egui::Key::E),
            )
        });
        if z {
            self.do_undo();
        }
        if y {
            self.do_redo();
        }
        if s {
            self.save_project(false);
        }
        if o {
            self.open_project();
        }
        if g {
            self.generate();
        }
        if e && self.build.as_ref().map(|b| b.ok()).unwrap_or(false) {
            self.export_cnc();
        }
        if ctx.input(|i| i.modifiers.command) {
            return;
        }
        if self.step == Step::Parts {
            for t in DrawTool::ALL {
                let k = match t.key() {
                    "V" => egui::Key::V,
                    "H" => egui::Key::H,
                    "S" => egui::Key::S,
                    "O" => egui::Key::O,
                    "G" => egui::Key::G,
                    "B" => egui::Key::B,
                    _ => egui::Key::T,
                };
                if ctx.input(|i| i.key_pressed(k)) {
                    self.draw = t;
                }
            }
        }
        if ctx.input(|i| i.key_pressed(egui::Key::F)) {
            self.part_view.needs_fit = true;
            self.sheet_view.needs_fit = true;
            self.prog_view.needs_fit = true;
        }
        if self.step == Step::Program && ctx.input(|i| i.key_pressed(egui::Key::Space)) {
            self.toggle_play();
        }
    }

    pub fn toggle_play(&mut self) {
        self.sim_running = !self.sim_running;
        if self.sim_running && self.sim_pos.map(|p| p >= self.sim_len()).unwrap_or(true) {
            self.sim_pos = Some(0);
        }
    }
}

fn step_tab(ui: &mut egui::Ui, n: usize, label: &str, g: Glyph, active: bool, done: bool) -> egui::Response {
    let font = egui::FontId::proportional(13.5);
    let tw = ui.painter().layout_no_wrap(label.to_string(), font.clone(), theme::TEXT).size().x;
    let (r, resp) = ui.allocate_exact_size(vec2(tw + 56.0, 32.0), Sense::click());
    let bg = if active { theme::ACCENT_DIM } else if resp.hovered() { theme::CARD_HOVER } else { Color32::TRANSPARENT };
    ui.painter().rect_filled(r, 7.0, bg);
    if active {
        ui.painter().rect_stroke(r, 7.0, Stroke::new(1.0, theme::ACCENT), egui::StrokeKind::Inside);
    }
    let c = egui::pos2(r.left() + 15.0, r.center().y);
    let col = if active { theme::ACCENT } else if done { theme::OK } else { theme::TEXT_FAINT };
    ui.painter().circle_filled(c, 9.0, col.gamma_multiply(if active { 1.0 } else { 0.35 }));
    ui.painter().text(c, Align2::CENTER_CENTER, n.to_string(), egui::FontId::proportional(11.0), if active { Color32::WHITE } else { theme::TEXT });
    let _ = g;
    ui.painter().text(egui::pos2(r.left() + 32.0, r.center().y), Align2::LEFT_CENTER, label, font, if active { theme::TEXT } else { theme::TEXT_DIM });
    resp
}

/// Short description of a feature for lists.
pub fn feature_label(f: &Feature) -> String {
    match f {
        Feature::Hole { dia, .. } => format!("Hole Ø{}", dia),
        Feature::Slot { length, width, vertical, .. } => format!("Slot {} × {}{}", length, width, if *vertical { " vertical" } else { "" }),
        Feature::Rect { x0, y0, x1, y1, .. } => format!("Opening {:.3} × {:.3}", x1 - x0, y1 - y0),
        Feature::Grid { nx, ny, shape, .. } => format!("Louvers {} × {}  ({:?} {}×{})", nx, ny, shape.kind, shape.w, shape.h),
        Feature::BoltCircle { n, dia, r, .. } => format!("Bolt circle {} × Ø{} on R{}", n, dia, r),
        Feature::Hit { tool, .. } => format!("Tool hit {}", match tool { ToolRef::Station(s) => format!("T{}", s), ToolRef::Name(n) => n.clone() }),
        Feature::Contour { closed, .. } => format!("Unsupported {}", if *closed { "shape" } else { "line" }),
    }
}

pub fn feature_glyph(f: &Feature) -> Glyph {
    match f {
        Feature::Hole { .. } => Glyph::Hole,
        Feature::Slot { .. } => Glyph::Slot,
        Feature::Rect { .. } => Glyph::Rect,
        Feature::Grid { .. } => Glyph::Grid,
        Feature::BoltCircle { .. } => Glyph::Bolt,
        Feature::Hit { .. } => Glyph::Punch,
        Feature::Contour { .. } => Glyph::Select,
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.shortcuts(&ctx);
        egui::Panel::top("top").exact_size(52.0).frame(egui::Frame::new().fill(theme::BG).inner_margin(Margin::symmetric(4, 0))).show(ui, |ui| self.top_bar(ui));
        egui::Panel::bottom("status").exact_size(26.0).frame(egui::Frame::new().fill(theme::BG)).show(ui, |ui| self.status_bar(ui));
        if self.sim_running {
            let n = self.sim_len();
            let p = self.sim_pos.unwrap_or(0) + (n / 400).max(1);
            if p >= n {
                self.sim_pos = None;
                self.sim_running = false;
            } else {
                self.sim_pos = Some(p);
                ctx.request_repaint_after(std::time::Duration::from_millis(16));
            }
        }
        match self.step {
            Step::Parts => crate::parts_view::show(self, ui),
            Step::Sheet => crate::sheet_view::show(self, ui),
            Step::Program => crate::program_view::show(self, ui),
            Step::Machine => crate::machine_view::show(self, ui),
        }
        if self.new_part.open {
            crate::parts_view::new_part_dialog(self, &ctx);
        }
        let dragging = ctx.input(|i| i.pointer.any_down());
        self.commit_if_changed(dragging);
    }
}
