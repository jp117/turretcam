//! Turret tool names and footprints.
//!
//! Names as written in M-1225 CNC headers: `RD-0.25-M` (round), `SQ-0.4375-M` (square),
//! `RE-3X0.125-M` (rectangle; first dimension along X at C0), `OB-0.5X0.25_90-M` (obround,
//! mounted turned 90 deg), `SP-...` (special, geometry unknown).
//! The `C=` value in a CNC tool comment is the true mount angle; the `_90` / `-90-` name tag is
//! only a fallback (stations 16 and 28 are mounted at 90 without a tag).

use serde::{Deserialize, Serialize};
use std::f64::consts::PI;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub enum ToolKind {
    RD,
    SQ,
    RE,
    OB,
    SP,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolShape {
    pub name: String,
    pub kind: ToolKind,
    /// diameter / side / length, inches
    pub a: Option<f64>,
    /// width for RE / OB
    pub b: Option<f64>,
    /// holder rotation in degrees (0 or 90)
    pub mount: f64,
}

impl ToolShape {
    pub fn known(&self) -> bool {
        self.kind != ToolKind::SP && self.a.is_some()
    }
    pub fn a(&self) -> f64 {
        self.a.unwrap_or(0.0)
    }
    pub fn b(&self) -> f64 {
        self.b.or(self.a).unwrap_or(0.0)
    }
    pub fn with_mount(&self, mount: f64) -> ToolShape {
        let mut s = self.clone();
        s.mount = mount;
        s
    }

    /// Footprint width along X and Y at rotation `angle` (the C word, degrees).
    pub fn extent(&self, angle: f64) -> (f64, f64) {
        match self.kind {
            ToolKind::RD | ToolKind::SP => (self.a(), self.a()),
            _ => {
                let (a, b) = if self.kind == ToolKind::SQ { (self.a(), self.a()) } else { (self.a(), self.b()) };
                let t = (angle + self.mount).to_radians();
                let (s, c) = (t.sin().abs(), t.cos().abs());
                (a * c + b * s, a * s + b * c)
            }
        }
    }

    /// Cut perimeter, for tonnage estimates.
    pub fn perimeter(&self) -> Option<f64> {
        if !self.known() {
            return None;
        }
        Some(match self.kind {
            ToolKind::RD => PI * self.a(),
            ToolKind::SQ => 4.0 * self.a(),
            ToolKind::RE => 2.0 * (self.a() + self.b()),
            ToolKind::OB => 2.0 * (self.a() - self.b()) + PI * self.b(),
            ToolKind::SP => return None,
        })
    }

    /// Footprint polygon centred at (x, y) for rotation `angle`.
    pub fn outline(&self, x: f64, y: f64, angle: f64, seg: usize) -> Vec<(f64, f64)> {
        let t = (angle + self.mount).to_radians();
        let (st, ct) = t.sin_cos();
        let rot = |u: f64, v: f64| (x + u * ct - v * st, y + u * st + v * ct);
        match self.kind {
            ToolKind::RD | ToolKind::SP => {
                let r = if self.a() > 0.0 { self.a() / 2.0 } else { 0.05 };
                (0..seg)
                    .map(|i| {
                        let th = 2.0 * PI * i as f64 / seg as f64;
                        (x + r * th.cos(), y + r * th.sin())
                    })
                    .collect()
            }
            ToolKind::SQ | ToolKind::RE => {
                let (ha, hb) = if self.kind == ToolKind::SQ {
                    (self.a() / 2.0, self.a() / 2.0)
                } else {
                    (self.a() / 2.0, self.b() / 2.0)
                };
                vec![rot(-ha, -hb), rot(ha, -hb), rot(ha, hb), rot(-ha, hb)]
            }
            ToolKind::OB => {
                let (ha, hb) = (self.a() / 2.0, self.b() / 2.0);
                let r = hb;
                let cx = ha - r;
                let n = (seg / 2).max(4);
                let mut pts = Vec::with_capacity(2 * n + 2);
                for i in 0..=n {
                    let th = -PI / 2.0 + PI * i as f64 / n as f64;
                    pts.push(rot(cx + r * th.cos(), r * th.sin()));
                }
                for i in 0..=n {
                    let th = PI / 2.0 + PI * i as f64 / n as f64;
                    pts.push(rot(-cx + r * th.cos(), r * th.sin()));
                }
                pts
            }
        }
    }
}

/// Parse a tool name such as `RE-1X0.125 -90- M`.
pub fn parse_tool_name(name: &str) -> ToolShape {
    let n = name.trim();
    let kind = match n.get(..2) {
        Some("RD") => ToolKind::RD,
        Some("SQ") => ToolKind::SQ,
        Some("RE") => ToolKind::RE,
        Some("OB") => ToolKind::OB,
        _ => ToolKind::SP,
    };
    let rest = n.get(2..).unwrap_or("");
    let mut a = None;
    let mut b = None;
    let mut tail = rest;
    if let Some(r) = rest.strip_prefix('-') {
        let (v, after) = take_num(r);
        a = v;
        tail = after;
        let t2 = after.trim_start();
        if let Some(r2) = t2.strip_prefix('X').or_else(|| t2.strip_prefix('x')) {
            let (v2, after2) = take_num(r2.trim_start());
            if v2.is_some() {
                b = v2;
                tail = after2;
            }
        }
    }
    if kind == ToolKind::SP {
        return ToolShape { name: n.to_string(), kind, a, b: None, mount: 0.0 };
    }
    if matches!(kind, ToolKind::RE | ToolKind::OB) && b.is_none() {
        b = a;
    }
    let mount = if has_rot90(tail) { 90.0 } else { 0.0 };
    ToolShape { name: n.to_string(), kind, a, b, mount }
}

fn take_num(s: &str) -> (Option<f64>, &str) {
    let end = s.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(s.len());
    let txt = s[..end].trim_end_matches('.');
    (txt.parse().ok(), &s[end..])
}

fn has_rot90(tail: &str) -> bool {
    tail.contains("_90") || tail.contains("-90-") || tail.trim_end().ends_with(" 90") || tail.contains(" 90 ")
}

/// Same physical tool? (names differ in spacing across Lantek versions)
pub fn same_tool(a: &ToolShape, b: &ToolShape) -> bool {
    let squash = |s: &str| s.chars().filter(|c| !c.is_whitespace()).collect::<String>();
    if squash(&a.name) == squash(&b.name) {
        return true;
    }
    a.known() && b.known() && a.kind == b.kind && a.a == b.a && a.b == b.b && a.mount == b.mount
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn names() {
        let s = parse_tool_name("RE-1X0.125 -90- M");
        assert_eq!((s.kind, s.a, s.b, s.mount), (ToolKind::RE, Some(1.0), Some(0.125), 90.0));
        let s = parse_tool_name("OB-0.5X0.25_90-M");
        assert_eq!((s.kind, s.a, s.b, s.mount), (ToolKind::OB, Some(0.5), Some(0.25), 90.0));
        let s = parse_tool_name("RD-0.4375");
        assert_eq!((s.kind, s.a), (ToolKind::RD, Some(0.4375)));
        assert_eq!(parse_tool_name("OB-1X0.5625 - M 90").mount, 90.0);
        assert_eq!(parse_tool_name("RD-0.102 - M").a, Some(0.102));
        assert!(!parse_tool_name("SP-CUSTOM Z RAIL-M").known());
        let (ex, ey) = parse_tool_name("RE-3X0.125-M").extent(90.0);
        assert!((ex - 0.125).abs() < 1e-9 && (ey - 3.0).abs() < 1e-9);
    }
}
