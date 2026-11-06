//! A project = one sheet program: parts, where they sit on the sheet, and job settings.
//! Saved as JSON (`*.tcam.json`). This is the file humans save and agents write.

use crate::cam::{BuildResult, CamError, Job, JobOptions};
use crate::cnc::Sheet;
use crate::geom::{xform, BBox};
use crate::machine::Machine;
use crate::part::{Feature, Outline, Part};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SheetSpec {
    pub x: f64,
    pub y: f64,
    pub thickness: f64,
    pub material: String,
    #[serde(default = "one")]
    pub qty: u32,
}
fn one() -> u32 {
    1
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Placement {
    /// Part id
    pub part: String,
    /// Sheet position of the placed part's lower-left corner
    pub x: f64,
    pub y: f64,
    /// 0, 90, 180 or 270
    #[serde(default)]
    pub rot: i32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectOptions {
    #[serde(default = "d_tab")]
    pub tab: f64,
    #[serde(default = "d_tab_every")]
    pub tab_every: f64,
    #[serde(default = "d_overlap")]
    pub min_overlap: f64,
    #[serde(default)]
    pub allow_stale_tools: bool,
}
fn d_tab() -> f64 {
    0.025
}
fn d_tab_every() -> f64 {
    24.0
}
fn d_overlap() -> f64 {
    0.06
}
impl Default for ProjectOptions {
    fn default() -> Self {
        ProjectOptions { tab: d_tab(), tab_every: d_tab_every(), min_overlap: d_overlap(), allow_stale_tools: false }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Project {
    #[serde(default = "one")]
    pub version: u32,
    pub name: String,
    /// Free text written as the job comment (order number + description).
    #[serde(default)]
    pub job: String,
    pub machine: String,
    pub program_number: u32,
    pub sheet: SheetSpec,
    /// None = choose clamp positions automatically.
    #[serde(default)]
    pub clamps: Option<Vec<f64>>,
    #[serde(default)]
    pub parts: Vec<Part>,
    #[serde(default)]
    pub placements: Vec<Placement>,
    #[serde(default)]
    pub options: ProjectOptions,
}

impl Project {
    pub fn new(name: &str, machine: &str, program_number: u32, sheet: SheetSpec) -> Project {
        Project {
            version: 1,
            name: name.into(),
            job: String::new(),
            machine: machine.into(),
            program_number,
            sheet,
            clamps: None,
            parts: vec![],
            placements: vec![],
            options: ProjectOptions::default(),
        }
    }

    pub fn load(path: &std::path::Path) -> Result<Project, String> {
        let txt = std::fs::read_to_string(path).map_err(|e| format!("{}: {}", path.display(), e))?;
        serde_json::from_str(&txt).map_err(|e| format!("{}: {}", path.display(), e))
    }

    pub fn save(&self, path: &std::path::Path) -> std::io::Result<()> {
        std::fs::write(path, serde_json::to_string_pretty(self).unwrap())
    }

    pub fn part(&self, id: &str) -> Option<&Part> {
        self.parts.iter().find(|p| p.id == id)
    }

    pub fn part_mut(&mut self, id: &str) -> Option<&mut Part> {
        self.parts.iter_mut().find(|p| p.id == id)
    }

    /// A part id not used yet ("P1", "P2", ...).
    pub fn new_part_id(&self) -> String {
        (1..).map(|n| format!("P{}", n)).find(|id| self.part(id).is_none()).unwrap()
    }

    /// Translation that puts the rotated part's bbox corner at the placement point.
    fn frame(&self, pl: &Placement) -> Option<(i32, f64, f64)> {
        let part = self.part(&pl.part)?;
        let b = part.bbox();
        let mut rb = BBox::empty();
        for p in [(b.x0, b.y0), (b.x1, b.y0), (b.x0, b.y1), (b.x1, b.y1)] {
            rb.add(xform(p, pl.rot as f64, 0.0, 0.0));
        }
        Some((pl.rot, pl.x - rb.x0, pl.y - rb.y0))
    }

    /// Placed part's bbox on the sheet.
    pub fn placed_bbox(&self, pl: &Placement) -> Option<BBox> {
        let part = self.part(&pl.part)?;
        let (r, dx, dy) = self.frame(pl)?;
        let b = part.bbox();
        let mut out = BBox::empty();
        for p in [(b.x0, b.y0), (b.x1, b.y0), (b.x0, b.y1), (b.x1, b.y1)] {
            out.add(xform(p, r as f64, dx, dy));
        }
        Some(out)
    }

    /// Every feature of every placement, in sheet coordinates.
    pub fn placed_features(&self) -> Vec<(usize, Feature)> {
        let mut v = vec![];
        for (i, pl) in self.placements.iter().enumerate() {
            let (Some(part), Some((r, dx, dy))) = (self.part(&pl.part), self.frame(pl)) else { continue };
            for f in &part.features {
                v.push((i, f.transformed(r, dx, dy)));
            }
        }
        v
    }

    /// Problems to show before building (unsupported geometry, parts off the sheet, overlaps).
    pub fn preflight(&self) -> Vec<String> {
        let mut v = vec![];
        for p in &self.parts {
            v.extend(p.unsupported());
        }
        let boxes: Vec<(usize, BBox)> = self.placements.iter().enumerate().filter_map(|(i, pl)| self.placed_bbox(pl).map(|b| (i, b))).collect();
        for (i, b) in &boxes {
            if b.x0 < -1e-6 || b.y0 < -1e-6 || b.x1 > self.sheet.x + 1e-6 || b.y1 > self.sheet.y + 1e-6 {
                v.push(format!("placement {} ({}) runs off the sheet", i + 1, self.placements[*i].part));
            }
        }
        for a in 0..boxes.len() {
            for b in a + 1..boxes.len() {
                let (p, q) = (&boxes[a].1, &boxes[b].1);
                if p.x0 < q.x1 - 1e-6 && q.x0 < p.x1 - 1e-6 && p.y0 < q.y1 - 1e-6 && q.y0 < p.y1 - 1e-6 {
                    v.push(format!("placements {} and {} overlap", boxes[a].0 + 1, boxes[b].0 + 1));
                }
            }
        }
        for pl in &self.placements {
            if self.part(&pl.part).is_none() {
                v.push(format!("placement refers to missing part {}", pl.part));
            }
        }
        v
    }

    /// Generate the CNC program for this sheet.
    pub fn build(&self, m: &Machine, date: Option<&str>) -> Result<BuildResult, CamError> {
        let mut opts = JobOptions { min_overlap: self.options.min_overlap, tab: self.options.tab, tab_every: self.options.tab_every, allow_stale_tools: self.options.allow_stale_tools, ..Default::default() };
        if let Some(d) = date {
            opts.date = d.to_string();
        }
        let s = &self.sheet;
        let sheet = Sheet { x: s.x, y: s.y, thickness: s.thickness, material: s.material.clone(), qty: s.qty };
        let mut job = Job::new(m, self.program_number, &self.job, sheet, opts);
        job.clamps = self.clamps.clone();
        for (i, f) in self.placed_features() {
            f.apply(&mut job).map_err(|e| CamError(format!("{} (placement {}): {}", self.placements[i].part, i + 1, e.0)))?;
        }
        for (i, pl) in self.placements.iter().enumerate() {
            let Some(part) = self.part(&pl.part) else { continue };
            if let Outline::Rect { tabs, .. } = part.outline {
                let b = self.placed_bbox(pl).unwrap();
                let e = 1e-3;
                let edges = [b.y0 > e, b.y1 < s.y - e, b.x0 > e, b.x1 < s.x - e];
                job.outline(b.x0, b.y0, b.x1, b.y1, edges, tabs).map_err(|er| CamError(format!("{} (placement {}) outline: {}", pl.part, i + 1, er.0)))?;
            }
        }
        let mut res = job.build()?;
        for p in self.preflight() {
            res.issues.push(crate::check::Issue { severity: crate::check::Severity::Warn, code: "project", msg: p });
        }
        Ok(res)
    }
}
