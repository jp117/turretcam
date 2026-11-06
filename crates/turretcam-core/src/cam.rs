//! Feature programming: turns holes, slots, windows, outlines and patterns into punch hits.
//!
//! Each strategy copies a convention measured in the shop's M-1225 archive:
//!
//! * **round hole**: exact RD tool, single hit.
//! * **large round hole**: circle nibble (G72 + G68) with the largest RD tool no bigger than
//!   half the hole, pitch chosen so the scallop equals the machine nibble tolerance, plus a
//!   centre hit and G77 ring(s) so the slug is destroyed. Reproduces Lantek's 4.125" meter
//!   hole exactly (75 hits, same area).
//! * **exact shape**: one hit of an SQ/RE/OB tool matching the opening (auto-index tools rotate).
//! * **rectangular window**: perimeter only, tool inside the opening, outer tool edge on the
//!   line, tool ends flush at the corners, even pitch, 0.025" microjoint tabs on long edges.
//! * **part outline**: same, tool outside the part; edges on the sheet edge are skipped.
//! * **grid / bolt circle**: G78 / G77 patterns.
//!
//! Order: small interior features first, windows next, outlines last, nearest-neighbour
//! within each tool, like Lantek.

use crate::check::{self, Issue, Severity};
use crate::cnc::{self, Header, Op, OpKind, Program, Sheet, ToolEntry};
use crate::geom::Pt;
use crate::machine::Machine;
use crate::tooling::{ToolKind, ToolShape};
use crate::writer;
use serde::{Deserialize, Serialize};

const TOL: f64 = 1e-6;

#[derive(Debug, thiserror::Error, Clone)]
#[error("{0}")]
pub struct CamError(pub String);

fn cam_err<T>(s: impl Into<String>) -> Result<T, CamError> {
    Err(CamError(s.into()))
}

/// A tool by station number or by exact name.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum ToolRef {
    Station(u32),
    Name(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Prio {
    Interior = 0,
    Window = 1,
    Outline = 2,
}

#[derive(Debug, Clone)]
pub enum SegKind {
    Pt,
    Line { i: f64, j: f64, k: u32 },
    Grid { i: f64, j: f64, p: u32, k: u32 },
    Bolt { i: f64, j: f64, pitch: f64, k: u32 },
    CNib { i: f64, j: f64, k: f64, q: f64 },
}

/// A group of hits with one tool that is emitted as one block of code.
#[derive(Debug, Clone)]
pub struct Seg {
    pub station: u32,
    pub kind: SegKind,
    pub x: f64,
    pub y: f64,
    pub angle: f64,
    pub prio: Prio,
    pub note: String,
}

impl Seg {
    pub fn hits(&self) -> Vec<Pt> {
        match &self.kind {
            SegKind::Pt => vec![(self.x, self.y)],
            SegKind::Line { i, j, k } => {
                let a = j.to_radians();
                (0..=*k).map(|n| (self.x + n as f64 * i * a.cos(), self.y + n as f64 * i * a.sin())).collect()
            }
            SegKind::Grid { i, j, p, k } => {
                let mut v = vec![];
                for r in 0..=*k {
                    for c in 0..=*p {
                        v.push((self.x + c as f64 * i, self.y + r as f64 * j));
                    }
                }
                v
            }
            SegKind::Bolt { i, j, pitch, k } => (0..*k)
                .map(|n| {
                    let a = (j + n as f64 * pitch).to_radians();
                    (self.x + i * a.cos(), self.y + i * a.sin())
                })
                .collect(),
            SegKind::CNib { i, j, k, q } => cnc::cnib_points(self.x, self.y, *i, *j, *k, *q),
        }
    }
    fn end(&self) -> Pt {
        match self.kind {
            SegKind::Pt | SegKind::Line { .. } | SegKind::Grid { .. } => *self.hits().last().unwrap(),
            _ => (self.x, self.y),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobOptions {
    pub min_overlap: f64,
    pub tab: f64,
    pub tab_every: f64,
    pub allow_stale_tools: bool,
    /// ISO date written into RELEASE DATE and used for tool staleness.
    pub date: String,
}

impl Default for JobOptions {
    fn default() -> Self {
        JobOptions {
            min_overlap: 0.06,
            tab: 0.025,
            tab_every: 24.0,
            allow_stale_tools: false,
            date: chrono::Local::now().format("%Y-%m-%d").to_string(),
        }
    }
}

/// One program being built.
pub struct Job<'m> {
    pub m: &'m Machine,
    pub number: u32,
    pub name: String,
    pub job: String,
    pub sheet: Sheet,
    pub clamps: Option<Vec<f64>>,
    pub opts: JobOptions,
    pub segs: Vec<Seg>,
    pub log: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct BuildResult {
    pub text: String,
    pub program: Program,
    pub issues: Vec<Issue>,
    pub log: Vec<String>,
    pub etime: f64,
    pub clamps: Vec<f64>,
}

impl BuildResult {
    pub fn ok(&self) -> bool {
        check::ok(&self.issues)
    }
}

impl<'m> Job<'m> {
    pub fn new(m: &'m Machine, number: u32, job: &str, sheet: Sheet, opts: JobOptions) -> Self {
        Job { m, number, name: format!("{:04}.CNC", number), job: job.to_string(), sheet, clamps: None, opts, segs: vec![], log: vec![] }
    }

    // ------------------------------------------------------------- tools
    fn usable(&self, st: u32) -> bool {
        self.m.station(st).map(|s| s.mounted.is_some()).unwrap_or(false) && (self.opts.allow_stale_tools || self.m.is_current(st, &self.opts.date))
    }

    fn tools(&self, kinds: &[ToolKind]) -> Vec<(u32, ToolShape)> {
        self.m.tools().into_iter().filter(|(n, s)| kinds.contains(&s.kind) && s.known() && self.usable(*n)).collect()
    }

    pub fn station_for(&self, t: &ToolRef) -> Result<u32, CamError> {
        match t {
            ToolRef::Station(n) => match self.m.station(*n) {
                Some(s) if s.mounted.is_some() => Ok(*n),
                _ => cam_err(format!("T{} has no known tool mounted", n)),
            },
            ToolRef::Name(name) => self.m.station_for_name(name).ok_or_else(|| CamError(format!("no station holds {:?}", name))),
        }
    }

    fn shape(&self, st: u32) -> ToolShape {
        self.m.station(st).and_then(|s| s.shape()).expect("station with tool")
    }

    fn autoindex(&self, st: u32) -> bool {
        self.m.station(st).map(|s| s.autoindex).unwrap_or(false)
    }

    fn push(&mut self, station: u32, kind: SegKind, x: f64, y: f64, angle: f64, prio: Prio, note: &str) {
        self.segs.push(Seg { station, kind, x, y, angle, prio, note: note.to_string() });
    }

    // ------------------------------------------------------------- primitives
    pub fn hit(&mut self, t: &ToolRef, x: f64, y: f64, angle: f64) -> Result<u32, CamError> {
        let st = self.station_for(t)?;
        if angle != 0.0 && !self.autoindex(st) {
            return cam_err(format!("T{} does not auto-index; it cannot be rotated to C{}", st, angle));
        }
        self.push(st, SegKind::Pt, x, y, angle, Prio::Interior, "hit");
        Ok(st)
    }

    /// `n` evenly spaced hits from (x0,y0) to (x1,y1) inclusive (emitted as X Y + G76).
    pub fn hits_line(&mut self, t: &ToolRef, x0: f64, y0: f64, x1: f64, y1: f64, n: u32, angle: f64) -> Result<u32, CamError> {
        let st = self.station_for(t)?;
        if n <= 1 {
            return self.hit(t, x0, y0, angle);
        }
        let l = ((x1 - x0).powi(2) + (y1 - y0).powi(2)).sqrt();
        let j = round(((y1 - y0).atan2(x1 - x0).to_degrees() + 360.0) % 360.0, 2);
        self.push(st, SegKind::Line { i: round(l / (n - 1) as f64, 4), j, k: n - 1 }, x0, y0, angle, Prio::Interior, "line");
        Ok(st)
    }

    /// nx x ny hits from (x, y) with pitch dx / dy (G78).
    pub fn grid(&mut self, st: u32, x: f64, y: f64, dx: f64, dy: f64, nx: u32, ny: u32, angle: f64) {
        self.push(st, SegKind::Grid { i: dx, j: dy, p: nx.saturating_sub(1), k: ny.saturating_sub(1) }, x, y, angle, Prio::Interior, "grid");
    }

    pub fn bolt_circle(&mut self, st: u32, cx: f64, cy: f64, r: f64, n: u32, start: f64) {
        self.push(st, SegKind::Bolt { i: r, j: start, pitch: 360.0 / n as f64, k: n }, cx, cy, 0.0, Prio::Interior, "bolt circle");
    }

    /// Find a mounted tool whose footprint is exactly w (X) x h (Y): (station, C angle).
    pub fn find_footprint(&self, kind: Option<ToolKind>, w: f64, h: f64, tol: f64) -> Option<(u32, f64)> {
        let kinds: Vec<ToolKind> = match kind {
            Some(k) => vec![k],
            None => vec![ToolKind::RD, ToolKind::SQ, ToolKind::RE, ToolKind::OB],
        };
        for (n, s) in self.tools(&kinds) {
            let angles: &[f64] = if self.autoindex(n) { &[0.0, 90.0] } else { &[0.0] };
            for &a in angles {
                let (ex, ey) = s.extent(a);
                if (ex - w).abs() <= tol && (ey - h).abs() <= tol {
                    return Some((n, a));
                }
            }
        }
        None
    }

    /// Exact stale tool exists? (for a helpful message)
    fn stale_footprint(&self, kind: ToolKind, w: f64, h: f64) -> Option<u32> {
        self.m.tools().into_iter().find(|(n, s)| s.kind == kind && s.known() && !self.usable(*n) && {
            let (ex, ey) = s.extent(0.0);
            (ex - w).abs() <= 0.002 && (ey - h).abs() <= 0.002
        }).map(|(n, _)| n)
    }

    // ------------------------------------------------------------- features
    pub fn hole(&mut self, x: f64, y: f64, dia: f64) -> Result<u32, CamError> {
        if let Some((st, _)) = self.find_footprint(Some(ToolKind::RD), dia, dia, 0.002) {
            self.push(st, SegKind::Pt, x, y, 0.0, Prio::Interior, &format!("hole D{}", dia));
            return Ok(st);
        }
        if let Some(st) = self.stale_footprint(ToolKind::RD, dia, dia) {
            self.log.push(format!("hole D{}: exact tool on T{} is stale (unused for over a year) so the hole was nibbled; pin T{} in turret_override.json if it is still mounted", dia, st, st));
        }
        let mut rds = self.tools(&[ToolKind::RD]);
        rds.sort_by(|a, b| b.1.a().partial_cmp(&a.1.a()).unwrap());
        let Some(smallest) = rds.last() else { return cam_err("no round tools on the turret") };
        if dia < smallest.1.a() {
            return cam_err(format!("hole D{} is smaller than the smallest round tool ({})", dia, smallest.1.name));
        }
        let pick = rds.iter().find(|(_, s)| s.a() <= dia / 2.0 + 1e-9).or_else(|| rds.iter().find(|(_, s)| s.a() < dia));
        let Some((st, _)) = pick.cloned() else { return cam_err(format!("no round tool can nibble a D{} hole", dia)) };
        self.circle_nibble(st, x, y, dia, true)?;
        Ok(st)
    }

    pub fn circle_nibble(&mut self, st: u32, cx: f64, cy: f64, dia: f64, destroy_slug: bool) -> Result<(), CamError> {
        let rt = self.shape(st).a() / 2.0;
        let r = dia / 2.0;
        let rc = r - rt;
        if rc <= 0.0 {
            return cam_err("tool is too big for the hole");
        }
        let h = self.m.spec.nibble_tolerance_in.min(rt * 0.5);
        let chord_edge = 2.0 * (rt * rt - (rt - h).powi(2)).sqrt();
        let q = round(chord_edge * rc / r, 4);
        self.push(st, SegKind::CNib { i: rc, j: 0.0, k: 360.0, q }, cx, cy, 0.0, Prio::Interior, &format!("hole D{} nibble", dia));
        let mut rings = 0;
        if destroy_slug {
            self.push(st, SegKind::Pt, cx, cy, 0.0, Prio::Interior, "slug centre");
            let spacing = 1.4 * rt;
            rings = ((rc / spacing).ceil() as i64 - 1).max(0) as u32;
            for k in 1..=rings {
                let rk = rc * k as f64 / (rings + 1) as f64;
                let nk = ((2.0 * std::f64::consts::PI * rk / spacing).ceil() as u32).max(3);
                self.push(st, SegKind::Bolt { i: round(rk, 4), j: 0.0, pitch: 360.0 / nk as f64, k: nk }, cx, cy, 0.0, Prio::Interior, "slug ring");
            }
        }
        self.log.push(format!("hole D{}: T{} circle nibble Q{}, {} slug ring(s)", dia, st, q, rings));
        Ok(())
    }

    /// Single hit of a tool whose footprint is exactly w x h; returns None if none fits.
    pub fn shape_hit(&mut self, cx: f64, cy: f64, w: f64, h: f64, kind: Option<ToolKind>) -> Option<u32> {
        let (st, a) = self.find_footprint(kind, w, h, 0.002)?;
        self.push(st, SegKind::Pt, cx, cy, a, Prio::Interior, &format!("shape {}x{}", w, h));
        Some(st)
    }

    /// Obround slot centred at (cx, cy): exact OB tool, else round-tool line nibble.
    pub fn slot(&mut self, cx: f64, cy: f64, length: f64, width: f64, vertical: bool) -> Result<u32, CamError> {
        let (w, h) = if vertical { (width, length) } else { (length, width) };
        if let Some(st) = self.shape_hit(cx, cy, w, h, Some(ToolKind::OB)) {
            return Ok(st);
        }
        let Some((st, s)) = self.find_footprint(Some(ToolKind::RD), width, width, 0.002).map(|(n, _)| (n, self.shape(n))) else {
            return cam_err(format!("no OB {}x{} and no RD {} to nibble the slot", length, width, width));
        };
        let half = (length - width) / 2.0;
        let (x0, y0, x1, y1) = if vertical { (cx, cy - half, cx, cy + half) } else { (cx - half, cy, cx + half, cy) };
        let rt = s.a() / 2.0;
        let pitch = 2.0 * (rt * rt - (rt - self.m.spec.nibble_tolerance_in).powi(2)).sqrt();
        let n = (((length - width) / pitch).ceil() as u32 + 1).max(2);
        self.hits_line(&ToolRef::Station(st), x0, y0, x1, y1, n, 0.0)
    }

    /// Tools that can cut a straight horizontal / vertical edge of `length`, best first:
    /// (station, length along edge, width across, C angle).
    fn edge_tools(&self, length: f64, horizontal: bool) -> Vec<(u32, f64, f64, f64)> {
        let mut out = vec![];
        for (n, s) in self.tools(&[ToolKind::RE, ToolKind::SQ]) {
            let aidx = self.autoindex(n);
            if s.kind == ToolKind::SQ && aidx {
                continue; // the auto-index SQ-2 is for clearing, not edges
            }
            let angles: &[f64] = if aidx { &[0.0, 90.0] } else { &[0.0] };
            for &a in angles {
                let (ex, ey) = s.extent(a);
                let (l, w) = if horizontal { (ex, ey) } else { (ey, ex) };
                if l >= w - TOL && l <= length + TOL {
                    out.push((n, l, w, a));
                }
            }
        }
        out.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap().then(a.2.partial_cmp(&b.2).unwrap()).then((a.3 != 0.0).cmp(&(b.3 != 0.0))));
        out
    }

    /// Cut along boundary a->b with the tool body on the `normal` side, ends flush at a and b.
    fn edge(&mut self, a: Pt, b: Pt, normal: Pt, prio: Prio, note: &str, tabs: bool) -> Result<u32, CamError> {
        let e = ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
        let horizontal = (b.1 - a.1).abs() < TOL;
        if !horizontal && (b.0 - a.0).abs() > TOL {
            return cam_err("only horizontal and vertical edges can be nibbled");
        }
        let cands = self.edge_tools(e, horizontal);
        let Some(&(st, l, w, ang)) = cands.first() else {
            return cam_err(format!("no tool fits a {:.4} in {} edge ({})", e, if horizontal { "horizontal" } else { "vertical" }, note));
        };
        let (ux, uy) = ((b.0 - a.0) / e, (b.1 - a.1) / e);
        let mut ntabs = if tabs && self.opts.tab > 0.0 { (e / self.opts.tab_every).floor() as u32 } else { 0 };
        let mut run = (e - ntabs as f64 * self.opts.tab) / (ntabs + 1) as f64;
        if ntabs > 0 && run < l {
            ntabs = 0;
            run = e;
        }
        let j = round(((uy.atan2(ux)).to_degrees() + 360.0) % 360.0, 2);
        let mut pos = 0.0;
        for _ in 0..=ntabs {
            let (s0, s1) = (pos, pos + run);
            let c0 = s0 + l / 2.0;
            let c1 = s1 - l / 2.0;
            let span = (c1 - c0).max(0.0);
            let off = w / 2.0;
            let p0 = (a.0 + ux * c0 + normal.0 * off, a.1 + uy * c0 + normal.1 * off);
            if span < TOL {
                self.push(st, SegKind::Pt, p0.0, p0.1, ang, prio, note);
            } else {
                let k = ((span / (l - self.opts.min_overlap)) - 1e-9).ceil().max(1.0) as u32;
                self.push(st, SegKind::Line { i: round(span / k as f64, 4), j, k }, p0.0, p0.1, ang, prio, note);
            }
            pos = s1 + self.opts.tab;
        }
        Ok(st)
    }

    /// Rectangular window: exact tool if one matches, else perimeter nibble (tool inside).
    pub fn cutout(&mut self, x0: f64, y0: f64, x1: f64, y1: f64, tabs: bool) -> Result<(), CamError> {
        let (x0, x1) = (x0.min(x1), x0.max(x1));
        let (y0, y1) = (y0.min(y1), y0.max(y1));
        if self.shape_hit((x0 + x1) / 2.0, (y0 + y1) / 2.0, x1 - x0, y1 - y0, None).is_some() {
            return Ok(());
        }
        self.edge((x0, y1), (x1, y1), (0.0, -1.0), Prio::Window, "window top", tabs)?;
        self.edge((x0, y0), (x1, y0), (0.0, 1.0), Prio::Window, "window bottom", tabs)?;
        self.edge((x0, y0), (x0, y1), (1.0, 0.0), Prio::Window, "window left", tabs)?;
        self.edge((x1, y0), (x1, y1), (-1.0, 0.0), Prio::Window, "window right", tabs)?;
        Ok(())
    }

    /// Cut a rectangular part free (tool outside the part). `edges` = [bottom, top, left, right].
    pub fn outline(&mut self, x0: f64, y0: f64, x1: f64, y1: f64, edges: [bool; 4], tabs: bool) -> Result<(), CamError> {
        let (x0, x1) = (x0.min(x1), x0.max(x1));
        let (y0, y1) = (y0.min(y1), y0.max(y1));
        let [bottom, top, left, right] = edges;
        let wv = self.edge_tools(y1 - y0, false).first().map(|t| t.2).unwrap_or(0.0);
        let ex0 = x0 - if left { wv } else { 0.0 };
        let ex1 = x1 + if right { wv } else { 0.0 };
        if top {
            self.edge((ex0, y1), (ex1, y1), (0.0, 1.0), Prio::Outline, "outline top", tabs)?;
        }
        if bottom {
            self.edge((ex0, y0), (ex1, y0), (0.0, -1.0), Prio::Outline, "outline bottom", tabs)?;
        }
        if left {
            self.edge((x0, y0), (x0, y1), (-1.0, 0.0), Prio::Outline, "outline left", tabs)?;
        }
        if right {
            self.edge((x1, y0), (x1, y1), (1.0, 0.0), Prio::Outline, "outline right", tabs)?;
        }
        Ok(())
    }

    // ------------------------------------------------------------- clamps
    pub fn auto_clamps(&self, hits: &[(u32, f64, f64)]) -> Result<Vec<f64>, CamError> {
        let sx = self.sheet.x;
        let n = if sx >= 36.0 { 3 } else if sx >= 22.0 { 2 } else { 1 }.min(self.m.spec.clamps.count);
        let ranges = &self.m.spec.clamps.ranges;
        let top = self.m.spec.clamps.general_dead_zone.top + 0.1;
        let low: Vec<&(u32, f64, f64)> = hits.iter().filter(|h| h.2 < top).collect();
        let free = |cx: f64| {
            low.iter().all(|(st, x, y)| {
                let z = self.m.clamp_zone(*st);
                let d = x - cx;
                !(-z.left < d && d < z.right && *y < z.top)
            })
        };
        for count in (1..=n).rev() {
            let ideal: Vec<f64> = if sx >= 90.0 && count == 3 { vec![12.0, 48.0, 80.0] } else { (0..count).map(|k| sx * (k as f64 + 0.5) / count as f64).collect() };
            let mut chosen: Vec<f64> = vec![];
            let mut prev = -1e9;
            for i in 0..count {
                let lo = ranges[i].min_x.max(prev + self.m.spec.clamps.min_gap);
                let hi = ranges[i].max_x.min(sx - 2.0);
                let cands: Vec<f64> = ((lo.ceil() as i64)..=(hi.floor() as i64)).map(|c| c as f64).filter(|c| free(*c)).collect();
                let Some(&c) = cands.iter().min_by(|a, b| (*a - ideal[i]).abs().partial_cmp(&(*b - ideal[i]).abs()).unwrap()) else { break };
                chosen.push(c);
                prev = c;
            }
            if chosen.len() == count {
                return Ok(chosen);
            }
        }
        cam_err("no clamp positions avoid the hits along the bottom edge; keep features above Y 3.4 or leave clamp gaps")
    }

    // ------------------------------------------------------------- build
    fn ordered(&self) -> Vec<Seg> {
        let area = |st: u32| {
            let (ex, ey) = self.shape(st).extent(0.0);
            ex * ey
        };
        let mut keys: Vec<(Prio, u32)> = self.segs.iter().map(|s| (s.prio, s.station)).collect();
        keys.sort_by(|a, b| a.0.cmp(&b.0).then(area(a.1).partial_cmp(&area(b.1)).unwrap()).then(a.1.cmp(&b.1)));
        keys.dedup();
        let mut out = vec![];
        let mut cur = (0.0, 0.0);
        for (p, st) in keys {
            let mut todo: Vec<Seg> = self.segs.iter().filter(|s| s.prio == p && s.station == st).cloned().collect();
            while !todo.is_empty() {
                let (i, _) = todo
                    .iter()
                    .enumerate()
                    .min_by(|a, b| crate::geom::dist((a.1.x, a.1.y), cur).partial_cmp(&crate::geom::dist((b.1.x, b.1.y), cur)).unwrap())
                    .unwrap();
                let s = todo.remove(i);
                cur = s.end();
                out.push(s);
            }
        }
        out
    }

    pub fn ops(&self) -> Vec<Op> {
        let mut ops = vec![];
        let mut cur_st: Option<u32> = None;
        let mut cur_c: Option<f64> = None;
        for s in self.ordered() {
            let first = cur_st != Some(s.station);
            let c = if self.autoindex(s.station) { Some(s.angle) } else { None };
            match &s.kind {
                SegKind::Bolt { i, j, pitch, k } => {
                    if first {
                        ops.push(Op::new(OpKind::Tool { t: s.station }));
                    }
                    ops.push(Op::new(OpKind::Ref { x: s.x, y: s.y }));
                    ops.push(Op::new(OpKind::Arc { i: *i, j: *j, pitch: *pitch, k: *k }));
                }
                SegKind::CNib { i, j, k, q } => {
                    if first {
                        ops.push(Op::new(OpKind::Tool { t: s.station }));
                    }
                    ops.push(Op::new(OpKind::Ref { x: s.x, y: s.y }));
                    ops.push(Op::new(OpKind::CNib { i: *i, j: *j, k: *k, comp: 0.0, q: *q }));
                }
                _ => {
                    if first {
                        ops.push(Op::new(OpKind::Load { x: s.x, y: s.y, t: s.station, c }));
                        cur_c = c;
                    } else {
                        let cc = if c.is_some() && c != cur_c { c } else { None };
                        if cc.is_some() {
                            cur_c = cc;
                        }
                        ops.push(Op::new(OpKind::Punch { x: Some(s.x), y: Some(s.y), c: cc }));
                    }
                    match &s.kind {
                        SegKind::Line { i, j, k } => ops.push(Op::new(OpKind::Line { i: *i, j: *j, k: *k })),
                        SegKind::Grid { i, j, p, k } => ops.push(Op::new(OpKind::Grid { x_first: true, i: *i, j: *j, p: *p, k: *k })),
                        _ => {}
                    }
                }
            }
            cur_st = Some(s.station);
        }
        ops
    }

    /// Generate, re-parse, self-verify and check the program.
    pub fn build(&self) -> Result<BuildResult, CamError> {
        if self.segs.is_empty() {
            return cam_err("nothing to punch");
        }
        let ops = self.ops();
        let mut used: Vec<u32> = vec![];
        for o in &ops {
            if let OpKind::Load { t, .. } | OpKind::Tool { t } = o.kind {
                if !used.contains(&t) {
                    used.push(t);
                }
            }
        }
        let tools: Vec<ToolEntry> = used
            .iter()
            .map(|&st| {
                let s = self.m.station(st).unwrap();
                let mt = s.mounted.as_ref().unwrap();
                ToolEntry { station: st, name: mt.tool.clone(), aidx: s.autoindex, c: if s.autoindex { None } else { Some(mt.mount_angle) }, f: Some("F1".into()) }
            })
            .collect();
        let intended: Vec<(u32, f64, f64)> = self.segs.iter().flat_map(|s| s.hits().into_iter().map(move |(x, y)| (s.station, x, y))).collect();
        let clamps = match &self.clamps {
            Some(c) => c.clone(),
            None => self.auto_clamps(&intended)?,
        };
        let mut h = Header {
            program_number: Some(format!("{:04}", self.number)),
            cnc_name: Some(self.name.clone()),
            job: if self.job.is_empty() { vec![] } else { vec![self.job.clone()] },
            release_date: Some(self.opts.date.clone()),
            sheet: Some(self.sheet.clone()),
            clamps: clamps.clone(),
            etime: Some(0.0),
            tools,
            ..Default::default()
        };
        let first = cnc::parse(&writer::program_text(&h, &ops, self.m, None));
        let etime = self.m.time_estimate(&first.hits);
        h.etime = Some(etime);
        let text = writer::program_text(&h, &ops, self.m, None);
        let program = cnc::parse(&text);
        let mut issues = check::check(&program, self.m, true);

        // self-verification: what the machine will do == what we intended
        let key = |st: u32, x: f64, y: f64| (st, (x * 1000.0).round() as i64, (y * 1000.0).round() as i64);
        let mut got: Vec<_> = program.hits.iter().map(|h| key(h.station.unwrap_or(0), h.x, h.y)).collect();
        let mut want: Vec<_> = intended.iter().map(|(s, x, y)| key(*s, *x, *y)).collect();
        got.sort();
        want.sort();
        let close = got.len() == want.len() && got.iter().zip(&want).all(|(g, w)| g.0 == w.0 && (g.1 - w.1).abs() <= 2 && (g.2 - w.2).abs() <= 2);
        if !close {
            issues.push(Issue { severity: Severity::Error, code: "selfcheck", msg: format!("re-parsed program has {} hits, intended {}", got.len(), want.len()) });
        }
        Ok(BuildResult { text, program, issues, log: self.log.clone(), etime, clamps })
    }
}

pub fn round(v: f64, dec: i32) -> f64 {
    let s = 10f64.powi(dec);
    (v * s).round() / s
}
