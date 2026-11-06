//! Parse and simulate Strippit (Fanuc-style, Lantek PSTSTR05) punch programs.
//!
//! Body codes (coordinates are inches from the sheet's lower-left corner, after
//! `G52 G90 X0. Y0.`):
//!
//! | code | meaning |
//! |---|---|
//! | `X_ Y_ [C_]` | move and punch with the current tool |
//! | `G31 X_ Y_ T_ [C_]` | load tool T, move, punch |
//! | `T_` | load tool only |
//! | `G70 X_ Y_` | move, no punch |
//! | `G72 X_ Y_` | pattern base point, no punch |
//! | `G76 I J K` | line at angle: K more hits from base, pitch I, angle J |
//! | `G77 I J P K` | bolt circle round base: radius I, start J, step P deg, K hits |
//! | `G78/G79 I J P K` | grid from base, P extra columns, K extra rows (X / Y first) |
//! | `G68 I J K P Q` | circle nibble round base: radius I, start J, sweep K, comp P, pitch Q |
//! | `G69 I J P Q` | line nibble from base: length I, angle J, comp P, pitch Q |
//! | `M03 / G91 .. / G90 / M04` | clamp reposition; incremental X shifts the sheet frame |
//!
//! Every meaning was checked against the 7,952-program M-1225 archive (see docs/).

use crate::tooling::{parse_tool_name, ToolShape};
use serde::{Deserialize, Serialize};
use std::f64::consts::PI;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolEntry {
    pub station: u32,
    pub name: String,
    pub aidx: bool,
    /// Mount angle from `C=`; None for auto-index tools.
    pub c: Option<f64>,
    pub f: Option<String>,
}

impl ToolEntry {
    /// Footprint, with the comment's mount angle overriding the name tag.
    pub fn shape(&self) -> ToolShape {
        let s = parse_tool_name(&self.name);
        match (self.aidx, self.c) {
            (false, Some(c)) => s.with_mount(c),
            _ => s,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sheet {
    pub x: f64,
    pub y: f64,
    pub thickness: f64,
    pub material: String,
    pub qty: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Header {
    pub program_number: Option<String>,
    pub cnc_name: Option<String>,
    pub job: Vec<String>,
    pub release_date: Option<String>,
    pub sheet: Option<Sheet>,
    pub clamps: Vec<f64>,
    pub etime: Option<f64>,
    pub tools: Vec<ToolEntry>,
    pub g92: Option<(f64, f64)>,
    /// thickness, x, y
    pub g06: Option<(f64, f64, f64)>,
    /// Older post builds wrote RELEASE DATE before the job line.
    pub date_before_job: bool,
}

impl Header {
    pub fn tool(&self, st: u32) -> Option<&ToolEntry> {
        self.tools.iter().find(|t| t.station == st)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum OpKind {
    Punch { x: Option<f64>, y: Option<f64>, c: Option<f64> },
    Load { x: f64, y: f64, t: u32, c: Option<f64> },
    Tool { t: u32 },
    Move { x: f64, y: f64 },
    Ref { x: f64, y: f64 },
    Line { i: f64, j: f64, k: u32 },
    Arc { i: f64, j: f64, pitch: f64, k: u32 },
    Grid { x_first: bool, i: f64, j: f64, p: u32, k: u32 },
    CNib { i: f64, j: f64, k: f64, comp: f64, q: f64 },
    LNib { i: f64, j: f64, comp: f64, q: f64 },
    Raw { g: Vec<i32>, words: Vec<(char, String)> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Op {
    pub kind: OpKind,
    pub line_no: usize,
    pub raw: String,
}

impl Op {
    pub fn new(kind: OpKind) -> Op {
        Op { kind, line_no: 0, raw: String::new() }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Hit {
    pub x: f64,
    pub y: f64,
    pub station: Option<u32>,
    pub angle: f64,
    pub op: &'static str,
    pub line_no: usize,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct Program {
    pub header: Header,
    pub header_lines: Vec<String>,
    pub ops: Vec<Op>,
    pub hits: Vec<Hit>,
    pub moves: Vec<(f64, f64)>,
    pub warnings: Vec<String>,
    pub repositions: u32,
}

// ------------------------------------------------------------------ tokenizer
fn strip_comments(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut depth = 0;
    for ch in s.chars() {
        match ch {
            '(' => depth += 1,
            ')' if depth > 0 => depth -= 1,
            _ if depth == 0 => out.push(ch),
            _ => {}
        }
    }
    out
}

/// Words as (letter, number-text) pairs; number text may be empty.
pub fn words(code: &str) -> Vec<(char, String)> {
    let b: Vec<char> = code.to_uppercase().chars().collect();
    let mut out = vec![];
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c.is_ascii_uppercase() {
            i += 1;
            while i < b.len() && b[i] == ' ' {
                i += 1;
            }
            let start = i;
            if i < b.len() && (b[i] == '-' || b[i] == '+') {
                i += 1;
            }
            let digits_start = i;
            while i < b.len() && b[i].is_ascii_digit() {
                i += 1;
            }
            if i < b.len() && b[i] == '.' {
                i += 1;
                while i < b.len() && b[i].is_ascii_digit() {
                    i += 1;
                }
            }
            let has_digit = (digits_start..i).any(|k| b[k].is_ascii_digit());
            let num: String = if has_digit { b[start..i].iter().collect() } else { i = start; String::new() };
            out.push((c, num));
        } else {
            i += 1;
        }
    }
    out
}

fn f(v: &str) -> f64 {
    v.parse::<f64>().unwrap_or(0.0)
}

// ------------------------------------------------------------------ header comments
pub fn parse_tool_comment(s: &str) -> Option<ToolEntry> {
    // (T47    RD-0.25-M C=0; F1)   (T18    RE-3X0.125-M AIDX; F1)   (T12,3 RD-.. Multitool AIDX)
    let body = s.strip_prefix("(T")?.strip_suffix(')')?;
    let num_end = body.find(|c: char| !c.is_ascii_digit())?;
    let station: u32 = body[..num_end].parse().ok()?;
    let mut rest = &body[num_end..];
    if let Some(r) = rest.strip_prefix(',') {
        let e = r.find(|c: char| !c.is_ascii_digit()).unwrap_or(r.len());
        rest = &r[e..];
    }
    if !rest.starts_with(char::is_whitespace) {
        return None;
    }
    let rest = rest.trim();
    let (main, f) = match rest.rsplit_once(';') {
        Some((a, b)) => (a.trim(), Some(b.trim().to_string()).filter(|x| !x.is_empty())),
        None => (rest, None),
    };
    let (name, aidx, c) = if let Some(n) = main.strip_suffix("AIDX") {
        (n.trim(), true, None)
    } else if let Some(pos) = main.rfind(" C=") {
        (main[..pos].trim(), false, main[pos + 3..].trim().parse::<f64>().ok())
    } else {
        return None;
    };
    Some(ToolEntry { station, name: name.to_string(), aidx, c, f })
}

fn parse_header_comment(h: &mut Header, s: &str) {
    if let Some(t) = parse_tool_comment(s) {
        h.tools.push(t);
        return;
    }
    let body = s.strip_prefix('(').unwrap_or(s);
    let body = body.strip_suffix(')').unwrap_or(body);
    if let Some(v) = body.strip_prefix("CNC NAME:") {
        h.cnc_name = Some(v.trim().to_string());
    } else if let Some(v) = body.strip_prefix("RELEASE DATE:") {
        h.release_date = Some(v.trim().to_string());
        if h.job.is_empty() {
            h.date_before_job = true;
        }
    } else if let Some(v) = body.strip_prefix("SHEET :") {
        // "96 X 48 X 0.125 ALUMINUM 125)(QTY, 1"
        let (dims, qty) = match v.split_once(")(QTY,") {
            Some((a, b)) => (a, b.trim().trim_end_matches(')').trim().parse().unwrap_or(1)),
            None => (v, 1),
        };
        let parts: Vec<&str> = dims.splitn(3, " X ").collect();
        if parts.len() == 3 {
            let x = parts[0].trim().parse().unwrap_or(0.0);
            let y = parts[1].trim().parse().unwrap_or(0.0);
            let rest = parts[2].trim();
            let (t, mat) = rest.split_once(' ').unwrap_or((rest, ""));
            h.sheet = Some(Sheet { x, y, thickness: t.parse().unwrap_or(0.0), material: mat.trim().to_string(), qty });
        }
    } else if let Some(v) = body.strip_prefix("CLAMP :") {
        h.clamps = v.split(',').filter_map(|x| x.trim().parse().ok()).collect();
    } else if let Some(v) = body.strip_prefix("ETIME =") {
        h.etime = v.trim().parse().ok();
    } else {
        h.job.push(body.to_string());
    }
}

// ------------------------------------------------------------------ parser
pub fn parse(text: &str) -> Program {
    let norm = text.replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<&str> = norm.split('\n').collect();
    let mut h = Header::default();
    let mut header_lines = vec![];
    let mut ops = vec![];
    let mut in_body = false;
    for (idx, raw) in lines.iter().enumerate() {
        let ln = idx + 1;
        let s = raw.trim();
        if !in_body {
            header_lines.push(raw.to_string());
            if s.starts_with('(') {
                parse_header_comment(&mut h, s);
                continue;
            }
            let code = strip_comments(s).to_uppercase();
            let w = words(&code);
            let get = |l: char| w.iter().find(|(c, _)| *c == l).map(|(_, v)| v.as_str());
            let squashed: String = code.chars().filter(|c| !c.is_whitespace()).collect();
            if code.starts_with('O') {
                if let Some(o) = get('O').filter(|v| !v.is_empty()) {
                    h.program_number = Some(o.to_string());
                }
            }
            if squashed.contains("G92") {
                h.g92 = Some((f(get('X').unwrap_or("")), f(get('Y').unwrap_or(""))));
            }
            if squashed.starts_with("G06") {
                h.g06 = Some((f(get('Q').unwrap_or("")), f(get('X').unwrap_or("")), f(get('Y').unwrap_or(""))));
            }
            if squashed.starts_with("G52") {
                in_body = true;
            }
            continue;
        }
        if s.is_empty() {
            ops.push(Op { kind: OpKind::Raw { g: vec![], words: vec![] }, line_no: ln, raw: raw.to_string() });
            continue;
        }
        let code = strip_comments(s).to_uppercase();
        let w = words(&code);
        let gs: Vec<i32> = w.iter().filter(|(c, _)| *c == 'G').map(|(_, v)| f(v) as i32).collect();
        let mut d: Vec<(char, String)> = vec![];
        for (c, v) in &w {
            if *c != 'G' && !d.iter().any(|(k, _)| k == c) {
                d.push((*c, v.clone()));
            }
        }
        let has = |l: char| d.iter().any(|(c, _)| *c == l);
        let val = |l: char| d.iter().find(|(c, _)| *c == l).map(|(_, v)| f(v)).unwrap_or(0.0);
        let keys: String = {
            let mut k: Vec<char> = d.iter().map(|(c, _)| *c).collect();
            k.sort();
            k.into_iter().collect()
        };
        let kind = match (gs.as_slice(), keys.as_str()) {
            ([31], _) if has('X') && has('Y') && has('T') => OpKind::Load {
                x: val('X'),
                y: val('Y'),
                t: val('T') as u32,
                c: if has('C') { Some(val('C')) } else { None },
            },
            ([70], "XY") => OpKind::Move { x: val('X'), y: val('Y') },
            ([72], "XY") => OpKind::Ref { x: val('X'), y: val('Y') },
            ([76], "IJK") => OpKind::Line { i: val('I'), j: val('J'), k: val('K') as u32 },
            ([77], "IJKP") => OpKind::Arc { i: val('I'), j: val('J'), pitch: val('P'), k: val('K') as u32 },
            ([78], "IJKP") | ([79], "IJKP") => OpKind::Grid {
                x_first: gs[0] == 78,
                i: val('I'),
                j: val('J'),
                p: val('P') as u32,
                k: val('K') as u32,
            },
            ([68], "IJKPQ") => OpKind::CNib { i: val('I'), j: val('J'), k: val('K'), comp: val('P'), q: val('Q') },
            ([69], "IJPQ") => OpKind::LNib { i: val('I'), j: val('J'), comp: val('P'), q: val('Q') },
            ([], k) if !k.is_empty() && k.chars().all(|c| "XYC".contains(c)) && (has('X') || has('Y')) => OpKind::Punch {
                x: if has('X') { Some(val('X')) } else { None },
                y: if has('Y') { Some(val('Y')) } else { None },
                c: if has('C') { Some(val('C')) } else { None },
            },
            ([], "T") => OpKind::Tool { t: val('T') as u32 },
            _ => OpKind::Raw { g: gs.clone(), words: d.clone() },
        };
        ops.push(Op { kind, line_no: ln, raw: raw.to_string() });
    }
    if let (Some(g), Some(sh)) = (h.g06, h.sheet.as_mut()) {
        // G06 is exact (Q0.0315, X45.8125); the SHEET comment truncates to 3 dp
        if g.0 > 0.0 {
            sh.thickness = g.0;
            if g.1 > 0.0 && g.2 > 0.0 {
                sh.x = g.1;
                sh.y = g.2;
            }
        }
    }
    let mut prog = Program { header: h, header_lines, ops, ..Default::default() };
    simulate(&mut prog);
    prog
}

pub fn load(path: &std::path::Path) -> std::io::Result<Program> {
    let bytes = std::fs::read(path)?;
    // CNC files are ASCII/cp1252; map bytes 1:1 so nothing is lost
    let text: String = bytes.iter().map(|&b| b as char).collect();
    Ok(parse(&text))
}

// ------------------------------------------------------------------ simulator
pub fn simulate(prog: &mut Program) {
    let mut hits: Vec<Hit> = vec![];
    let mut moves = vec![];
    let mut warn = vec![];
    let (mut x, mut y) = (0.0f64, 0.0f64);
    let mut tool: Option<u32> = None;
    let mut c = 0.0f64;
    let mut off_x = 0.0f64;
    let mut in_repo = false;
    let mut absolute = true;
    let aidx: Vec<(u32, bool)> = prog.header.tools.iter().map(|t| (t.station, t.aidx)).collect();
    let is_aidx = |t: Option<u32>| t.map(|t| aidx.iter().any(|(s, a)| *s == t && *a)).unwrap_or(false);

    macro_rules! hit {
        ($px:expr, $py:expr, $op:expr, $ln:expr) => {{
            if tool.is_none() {
                warn.push(format!("line {}: punch with no tool loaded", $ln));
            }
            hits.push(Hit { x: $px + off_x, y: $py, station: tool, angle: if is_aidx(tool) { c } else { 0.0 }, op: $op, line_no: $ln });
        }};
    }

    for op in &prog.ops {
        let ln = op.line_no;
        match &op.kind {
            OpKind::Load { x: nx, y: ny, t, c: cc } => {
                tool = Some(*t);
                if let Some(v) = cc {
                    c = *v;
                }
                x = *nx;
                y = *ny;
                hit!(x, y, "G31", ln);
            }
            OpKind::Punch { x: px, y: py, c: cc } => {
                if let Some(v) = cc {
                    c = *v;
                }
                let (mut nx, mut ny) = (px.unwrap_or(x), py.unwrap_or(y));
                if in_repo || !absolute {
                    if !absolute {
                        nx = x + px.unwrap_or(0.0);
                        ny = y + py.unwrap_or(0.0);
                    }
                    if in_repo {
                        off_x += nx - x; // clamps move while the sheet is held: sheet = X + shift
                    }
                    x = nx;
                    y = ny;
                    continue;
                }
                x = nx;
                y = ny;
                hit!(x, y, "XY", ln);
            }
            OpKind::Tool { t } => tool = Some(*t),
            OpKind::Move { x: nx, y: ny } | OpKind::Ref { x: nx, y: ny } => {
                x = *nx;
                y = *ny;
                moves.push((x + off_x, y));
            }
            OpKind::Line { i, j, k } => {
                let a = j.to_radians();
                for n in 1..=*k {
                    hit!(x + n as f64 * i * a.cos(), y + n as f64 * i * a.sin(), "G76", ln);
                }
                if let Some(last) = hits.last() {
                    x = last.x - off_x;
                    y = last.y;
                }
            }
            OpKind::Arc { i, j, pitch, k } => {
                for n in 0..*k {
                    let a = (j + n as f64 * pitch).to_radians();
                    hit!(x + i * a.cos(), y + i * a.sin(), "G77", ln);
                }
            }
            OpKind::Grid { x_first, i, j, p, k } => {
                let op_name = if *x_first { "G78" } else { "G79" };
                for row in 0..=*k {
                    for col in 0..=*p {
                        if row == 0 && col == 0 {
                            continue;
                        }
                        hit!(x + col as f64 * i, y + row as f64 * j, op_name, ln);
                    }
                }
            }
            OpKind::CNib { i, j, k, comp, q } => {
                for (px, py) in cnib_points(x, y, i + comp, *j, *k, *q) {
                    hit!(px, py, "G68", ln);
                }
            }
            OpKind::LNib { i, j, q, .. } => {
                let a = j.to_radians();
                let n = if *q > 0.0 { ((i / q) - 1e-9).ceil().max(1.0) as u32 } else { 1 };
                for m in 0..=n {
                    let t = i * m as f64 / n as f64;
                    hit!(x + t * a.cos(), y + t * a.sin(), "G69", ln);
                }
            }
            OpKind::Raw { g, words: w } => {
                let get = |l: char| w.iter().find(|(c, _)| *c == l).map(|(_, v)| v.as_str());
                if g.contains(&91) {
                    absolute = false;
                }
                if g.contains(&90) {
                    absolute = true;
                }
                if let Some(m) = get('M') {
                    match f(m) as i32 {
                        3 => {
                            in_repo = true;
                            prog.repositions += 1;
                        }
                        4 => in_repo = false,
                        _ => {}
                    }
                }
                if let Some(t) = get('T') {
                    tool = Some(f(t) as u32);
                }
                let has_xy = get('X').is_some() || get('Y').is_some();
                if g == &[70] && has_xy {
                    x = get('X').map(f).unwrap_or(x);
                    y = get('Y').map(f).unwrap_or(y);
                    moves.push((x + off_x, y));
                } else if g == &[65] && get('X').is_some() {
                    x = f(get('X').unwrap());
                    y = get('Y').map(f).unwrap_or(y);
                    hit!(x, y, "G65", ln);
                    warn.push(format!("line {}: macro call {} treated as a single hit", ln, op.raw.trim()));
                } else if g.is_empty() && has_xy && !in_repo && absolute {
                    warn.push(format!("line {}: unrecognised motion line {:?}", ln, op.raw.trim()));
                } else if !op.raw.trim().is_empty() && get('M').is_none() && g.is_empty() && op.raw.trim() != "%" {
                    let only_f = !w.is_empty() && w.iter().all(|(c, _)| *c == 'F');
                    if !has_xy && !only_f {
                        warn.push(format!("line {}: unrecognised line {:?}", ln, op.raw.trim()));
                    }
                }
            }
        }
    }
    prog.hits = hits;
    prog.moves = moves;
    prog.warnings = warn;
}

/// Hit centres of a G68 circle nibble round (cx, cy).
pub fn cnib_points(cx: f64, cy: f64, r: f64, start: f64, sweep: f64, q: f64) -> Vec<(f64, f64)> {
    let sw = sweep.to_radians();
    let arc = sw.abs() * r;
    let n = if q > 0.0 { ((arc / q) - 1e-9).ceil().max(1.0) as u32 } else { 1 };
    let full = sweep.abs() >= 360.0 - 1e-6;
    let count = if full { n } else { n + 1 };
    (0..count)
        .map(|m| {
            let a = start.to_radians() + sw * m as f64 / n as f64;
            (cx + r * a.cos(), cy + r * a.sin())
        })
        .collect()
}

#[allow(dead_code)]
fn _pi() -> f64 {
    PI
}

#[cfg(test)]
mod tests {
    use super::*;
    const SRC: &str = "%\r\n\r\nO0001\r\n(CNC NAME: 0001.CNC)\r\n(RELEASE DATE: 2026-11-06)\r\n(SHEET : 48 X 24 X 0.031 ST 42)(QTY, 1)\r\n(CLAMP : 12,36)\r\n(ETIME = 1.00)\r\n(T47    RD-0.25-M C=0; F1)\r\nG28\r\nG92 X97.0079 Y49.6063\r\nX97.008 Y49.606 M75\r\nF1\r\nG06 Q0.0315 X48. Y24.\r\nM35\r\nG52 G90 X0. Y0.\r\nG31 X10. Y10. T47\r\nG76 I1. J0. K3\r\nX20. Y10.\r\nG78 I1. J1. P2 K1\r\nG72 X30. Y12.\r\nG77 I2. J0. P90. K4\r\nM99\r\n%\r\n";
    #[test]
    fn semantics() {
        let p = parse(SRC);
        assert_eq!(p.header.program_number.as_deref(), Some("0001"));
        assert_eq!(p.header.sheet.as_ref().unwrap().thickness, 0.0315);
        assert_eq!(p.header.clamps, vec![12.0, 36.0]);
        assert_eq!(p.hits.len(), 4 + 6 + 4);
        assert!((p.hits[3].x - 13.0).abs() < 1e-9);
        assert!(p.hits[10..].iter().all(|h| ((h.x - 30.0).powi(2) + (h.y - 12.0).powi(2)).sqrt() > 1.0));
        assert!(p.warnings.is_empty(), "{:?}", p.warnings);
    }
    #[test]
    fn tool_comments() {
        let t = parse_tool_comment("(T18    RE-3X0.125-M AIDX; F1)").unwrap();
        assert!(t.aidx && t.station == 18 && t.name == "RE-3X0.125-M");
        let t = parse_tool_comment("(T04    OB-1X0.188-M C=0; F1)").unwrap();
        assert_eq!((t.station, t.c), (4, Some(0.0)));
        let t = parse_tool_comment("(T44    OB-1X0.5625 - M 90 C=90; F1)").unwrap();
        assert_eq!(t.name, "OB-1X0.5625 - M 90");
    }
}
