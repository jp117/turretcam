//! Pre-flight checks for a program (generated or hand-written).
//!
//! Each rule is either a hard limit from the machine definition or a habit that none of the
//! shop's 7,952 archived M-1225 programs break (clamp dead zones: 0 violations in 2.77 M hits).

use crate::cnc::Program;
use crate::machine::Machine;
use crate::tooling::same_tool;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, PartialOrd, Ord)]
pub enum Severity {
    Error,
    Warn,
}

#[derive(Debug, Clone, Serialize)]
pub struct Issue {
    pub severity: Severity,
    pub code: &'static str,
    pub msg: String,
}

impl std::fmt::Display for Issue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self.severity {
            Severity::Error => "ERROR",
            Severity::Warn => "WARN ",
        };
        write!(f, "{} {}: {}", s, self.code, self.msg)
    }
}

pub fn ok(issues: &[Issue]) -> bool {
    !issues.iter().any(|i| i.severity == Severity::Error)
}

/// `strict_turret`: a tool that differs from the turret record is an error (true for
/// generated programs) or a warning (archive programs written for an older turret load).
pub fn check(prog: &Program, m: &Machine, strict_turret: bool) -> Vec<Issue> {
    let mut out = vec![];
    let mut e = |code: &'static str, msg: String| out.push(Issue { severity: Severity::Error, code, msg });
    let h = &prog.header;
    let Some(sheet) = h.sheet.as_ref() else {
        e("header.sheet", "missing (SHEET : L X W X T MATERIAL)(QTY, n) comment".into());
        return out;
    };
    let mut issues: Vec<Issue> = vec![];
    let mut err = |code: &'static str, msg: String| issues.push(Issue { severity: Severity::Error, code, msg });
    let mut warns: Vec<Issue> = vec![];
    let mut warn = |code: &'static str, msg: String| warns.push(Issue { severity: Severity::Warn, code, msg });

    // ---- header
    if let Some(g) = h.g06 {
        if (g.1 - sheet.x).abs() > 1e-3 || (g.2 - sheet.y).abs() > 1e-3 {
            warn("header.g06", format!("G06 {:?} disagrees with the SHEET comment", g));
        }
    }
    if let Some(g) = h.g92 {
        let r = m.reset_position();
        if (g.0 - r.0).abs() > 1e-3 || (g.1 - r.1).abs() > 1e-3 {
            err("header.g92", format!("G92 {:?} is not the machine reset position {:?}", g, r));
        }
    }
    let onum_ok = h.program_number.as_deref().map(|p| p.chars().all(|c| c.is_ascii_digit()) && matches!(p.parse::<u32>(), Ok(1..=9999))).unwrap_or(false);
    if !onum_ok {
        err("header.onumber", format!("program number O{} must be 1..9999", h.program_number.as_deref().unwrap_or("?")));
    }
    for w in &prog.warnings {
        warn("parse", w.clone());
    }

    // ---- clamps
    let cl = &m.spec.clamps;
    if h.clamps.is_empty() || h.clamps.len() > cl.count {
        err("clamps.count", format!("{} clamps; machine has {}", h.clamps.len(), cl.count));
    }
    for (i, cx) in h.clamps.iter().enumerate() {
        if let Some(r) = cl.ranges.get(i) {
            if *cx < r.min_x - 0.2 || *cx > r.max_x + 0.2 {
                err("clamps.range", format!("clamp {} at {} is outside {:.2}..{:.2}", i + 1, cx, r.min_x, r.max_x));
            }
        }
        if *cx > sheet.x {
            warn("clamps.sheet", format!("clamp {} at {} is off the {} in sheet and will not grip", i + 1, cx, sheet.x));
        }
    }
    if h.clamps.windows(2).any(|w| w[1] <= w[0]) {
        err("clamps.order", "clamp positions must increase left to right".into());
    }

    // ---- turret
    let mut used: Vec<u32> = prog.hits.iter().filter_map(|x| x.station).collect();
    used.sort();
    used.dedup();
    for &st in &used {
        let Some(s) = m.station(st) else {
            err("turret.station", format!("T{} does not exist on {}", st, m.turret.turret));
            continue;
        };
        let Some(te) = h.tool(st) else {
            err("turret.header", format!("T{} is used but not listed in the tool comments", st));
            continue;
        };
        let shp = te.shape();
        match s.shape() {
            Some(ms) if !same_tool(&shp, &ms) => {
                let msg = format!(
                    "T{} programmed as {:?}, turret record says {:?} (last used {})",
                    st,
                    te.name,
                    ms.name,
                    s.mounted.as_ref().map(|x| x.last_used.as_str()).unwrap_or("?")
                );
                if strict_turret {
                    err("turret.mismatch", msg)
                } else {
                    warn("turret.mismatch", msg)
                }
            }
            None => warn("turret.unknown", format!("T{}: no record of what is mounted; confirm {:?} with the operator", st, te.name)),
            _ => {}
        }
        if shp.known() && shp.a() > s.max_tool_in + 1e-6 {
            err("turret.size", format!("T{} {} is larger than the {}-station maximum {} in", st, te.name, s.size_class, s.max_tool_in));
        }
        if te.aidx != s.autoindex {
            err("turret.aidx", format!("T{} AIDX flag {} but station auto-index = {}", st, te.aidx, s.autoindex));
        }
    }

    // ---- per hit
    let ((xlo, xhi), (ylo, yhi)) = m.reach();
    let (mut n_out, mut n_clamp, mut n_rot) = (0, 0, 0);
    let mut first_out = None;
    let mut first_clamp = None;
    let mut first_off = None;
    for x in &prog.hits {
        let Some(st) = x.station else { continue };
        let Some(s) = m.station(st) else { continue };
        if x.angle != 0.0 && !s.autoindex {
            n_rot += 1;
        }
        if prog.repositions == 0 && !(x.x >= xlo - 1e-6 && x.x <= xhi + 1e-6 && x.y >= ylo - 1e-6 && x.y <= yhi + 1e-6) {
            n_out += 1;
            first_out.get_or_insert((x.x, x.y, st));
        }
        if let Some(te) = h.tool(st) {
            let shp = te.shape();
            if shp.known() {
                let (ex, ey) = shp.extent(x.angle);
                let (hx, hy) = (ex / 2.0, ey / 2.0);
                if x.x + hx < -1e-6 || x.x - hx > sheet.x + 1e-6 || x.y + hy < -1e-6 || x.y - hy > sheet.y + 1e-6 {
                    first_off.get_or_insert((x.x, x.y, st));
                }
            }
        }
        let z = s.clamp_zone;
        for cx in &h.clamps {
            let d = x.x - cx;
            if -z.left < d && d < z.right && x.y < z.top {
                n_clamp += 1;
                first_clamp.get_or_insert((x.x, x.y, st, *cx));
                break;
            }
        }
    }
    if n_rot > 0 {
        err("hit.rotation", format!("{} hits ask for a C angle on a station that does not auto-index", n_rot));
    }
    if let Some((px, py, st)) = first_out {
        err("hit.reach", format!("{} hits outside X{:.2}..{:.2} Y{:.2}..{:.2} (first T{} X{:.4} Y{:.4}); needs a reposition", n_out, xlo, xhi, ylo, yhi, st, px, py));
    }
    if let Some((px, py, st, cx)) = first_clamp {
        err("hit.clamp", format!("{} hits inside a clamp dead zone (first T{} X{:.4} Y{:.4}, clamp at {})", n_clamp, st, px, py, cx));
    }
    if let Some((px, py, st)) = first_off {
        warn("hit.offsheet", format!("T{} hit at X{:.4} Y{:.4} does not touch the sheet", st, px, py));
    }

    // ---- tonnage
    let cap = m.capacity_tons();
    let tau = m.shear_ksi(&sheet.material);
    for &st in &used {
        let Some(te) = h.tool(st) else { continue };
        if let Some(per) = te.shape().perimeter() {
            let tons = per * sheet.thickness * tau / 2.0;
            if tons > 0.8 * cap {
                let msg = format!("T{} {} needs about {:.1} US tons in {} in {}; press is {:.1}", st, te.name, tons, sheet.thickness, sheet.material, cap);
                if tons > cap {
                    err("tonnage", msg)
                } else {
                    warn("tonnage", msg)
                }
            }
        }
    }
    out.extend(issues);
    out.extend(warns);
    out
}
