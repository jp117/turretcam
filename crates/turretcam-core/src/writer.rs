//! Write programs in the exact M-1225 shop format (Lantek PSTSTR05 + the operators' ending).
//!
//! Decimal places per word were measured on the archive; `tests/archive.rs` re-emits all
//! archived programs and diffs them against the originals.

use crate::cnc::{Header, Op, OpKind, ToolEntry};
use crate::fmt::{num, plain, trunc3};
use crate::machine::Machine;

pub const CRLF: &str = "\r\n";

/// Shop ending typed by the operators on 7,513 programs: park the sheet, open clamps, stop.
pub const SHOP_ENDING: [&str; 5] = ["G70 X96 Y1", "M31", "M00", "M99", "%"];

#[derive(Clone, Copy, PartialEq)]
enum K {
    Punch,
    Load,
    Move,
    Ref,
    Line,
    Arc,
    Grid,
    CNib,
    LNib,
}

fn dec(k: K, letter: char) -> usize {
    match (k, letter) {
        (_, 'C') => 2,
        (K::Line, 'J') => 2,
        (K::CNib, 'J') => 5,
        (K::CNib, 'K') => 2,
        (K::LNib, 'J') => 2,
        (K::Arc, 'J') | (K::Arc, 'P') => 5,
        _ => 4,
    }
}

fn w(k: K, letter: char, v: f64) -> String {
    format!("{}{}", letter, num(v, dec(k, letter)))
}

/// One body line in canonical post format.
pub fn format_op(op: &Op) -> String {
    match &op.kind {
        OpKind::Punch { x, y, c } => {
            let mut p = vec![];
            if let Some(v) = x {
                p.push(w(K::Punch, 'X', *v));
            }
            if let Some(v) = y {
                p.push(w(K::Punch, 'Y', *v));
            }
            if let Some(v) = c {
                p.push(w(K::Punch, 'C', *v));
            }
            p.join(" ")
        }
        OpKind::Load { x, y, t, c } => {
            let mut s = format!("G31 {} {} T{}", w(K::Load, 'X', *x), w(K::Load, 'Y', *y), t);
            if let Some(v) = c {
                s.push(' ');
                s.push_str(&w(K::Load, 'C', *v));
            }
            s
        }
        OpKind::Tool { t } => format!("T{}", t),
        OpKind::Move { x, y } => format!("G70 {} {}", w(K::Move, 'X', *x), w(K::Move, 'Y', *y)),
        OpKind::Ref { x, y } => format!("G72 {} {}", w(K::Ref, 'X', *x), w(K::Ref, 'Y', *y)),
        OpKind::Line { i, j, k } => format!("G76 {} {} K{}", w(K::Line, 'I', *i), w(K::Line, 'J', *j), k),
        OpKind::Arc { i, j, pitch, k } => {
            format!("G77 {} {} {} K{}", w(K::Arc, 'I', *i), w(K::Arc, 'J', *j), w(K::Arc, 'P', *pitch), k)
        }
        OpKind::Grid { x_first, i, j, p, k } => format!(
            "G{} {} {} P{} K{}",
            if *x_first { 78 } else { 79 },
            w(K::Grid, 'I', *i),
            w(K::Grid, 'J', *j),
            p,
            k
        ),
        OpKind::CNib { i, j, k, comp, q } => format!(
            "G68 {} {} {} {} {}",
            w(K::CNib, 'I', *i),
            w(K::CNib, 'J', *j),
            w(K::CNib, 'K', *k),
            w(K::CNib, 'P', *comp),
            w(K::CNib, 'Q', *q)
        ),
        OpKind::LNib { i, j, comp, q } => format!(
            "G69 {} {} {} {}",
            w(K::LNib, 'I', *i),
            w(K::LNib, 'J', *j),
            w(K::LNib, 'P', *comp),
            w(K::LNib, 'Q', *q)
        ),
        OpKind::Raw { .. } => op.raw.trim_end().to_string(),
    }
}

pub fn tool_comment(t: &ToolEntry) -> String {
    let mid = if t.aidx { "AIDX".to_string() } else { format!("C={}", plain(t.c.unwrap_or(0.0))) };
    let tail = t.f.as_ref().map(|f| format!("; {}", f)).unwrap_or_default();
    format!("(T{:02}    {} {}{})", t.station, t.name, mid, tail)
}

/// Header block from `%` through `G52 G90 X0. Y0.`
pub fn header_lines(h: &Header, m: &Machine) -> Vec<String> {
    let mut l: Vec<String> = vec!["%".into(), "".into()];
    l.push(format!("O{}", h.program_number.clone().unwrap_or_default()));
    l.push(format!("(CNC NAME: {})", h.cnc_name.clone().unwrap_or_default()));
    let job: Vec<String> = h.job.iter().map(|j| format!("({})", j)).collect();
    let date = format!("(RELEASE DATE: {})", h.release_date.clone().unwrap_or_default());
    if h.date_before_job {
        l.push(date);
        l.extend(job);
    } else {
        l.extend(job);
        l.push(date);
    }
    let s = h.sheet.as_ref().expect("header needs a sheet");
    let mat = if s.material.is_empty() { String::new() } else { format!(" {}", s.material) };
    l.push(format!("(SHEET : {} X {} X {}{})(QTY, {})", trunc3(s.x), trunc3(s.y), trunc3(s.thickness), mat, s.qty));
    l.push(format!("(CLAMP : {})", h.clamps.iter().map(|c| plain(*c)).collect::<Vec<_>>().join(",")));
    l.push(format!("(ETIME = {:.2})", h.etime.unwrap_or(0.0)));
    l.extend(h.tools.iter().map(tool_comment));
    let (hx, hy) = m.reset_position();
    l.push("G28".into());
    l.push(format!("G92 X{} Y{}", num(hx, 4), num(hy, 4)));
    l.push(format!("X{} Y{} M75", num(hx, 3), num(hy, 3)));
    l.push("F1".into());
    l.push(format!("G06 Q{} X{} Y{}", num(s.thickness, 4), num(s.x, 4), num(s.y, 4)));
    l.push("M35".into());
    l.push("G52 G90 X0. Y0.".into());
    l
}

/// Full program text (CRLF, ends with "%\r\n" like the archive).
pub fn program_text(h: &Header, ops: &[Op], m: &Machine, ending: Option<&[&str]>) -> String {
    let mut lines = header_lines(h, m);
    lines.extend(ops.iter().map(format_op));
    lines.extend(ending.unwrap_or(&SHOP_ENDING).iter().map(|s| s.to_string()));
    let mut out = lines.join(CRLF);
    out.push_str(CRLF);
    out
}
