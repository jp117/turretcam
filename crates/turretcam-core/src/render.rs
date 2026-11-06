//! SVG previews of programs (simulated hits) for review by humans and agents.

use crate::cnc::Program;
use crate::machine::Machine;
use std::fmt::Write;

pub const PALETTE: [&str; 12] = ["#1f6feb", "#d1242f", "#1a7f37", "#bf8700", "#8250df", "#0a7ea4", "#cf222e", "#6e7781", "#953800", "#116329", "#0550ae", "#a40e26"];

/// Colour for a station, stable within one program.
pub fn station_colours(prog: &Program) -> Vec<(u32, &'static str)> {
    let mut st: Vec<u32> = prog.hits.iter().filter_map(|h| h.station).collect();
    st.sort();
    st.dedup();
    st.into_iter().enumerate().map(|(i, s)| (s, PALETTE[i % PALETTE.len()])).collect()
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

pub fn program_svg(prog: &Program, m: Option<&Machine>, scale: f64) -> String {
    let h = &prog.header;
    let (sx, sy) = h.sheet.as_ref().map(|s| (s.x, s.y)).unwrap_or((100.0, 50.0));
    let colours = station_colours(prog);
    let margin = 1.0;
    let legend_rows = colours.len().div_ceil(4);
    let w = (sx + 2.0 * margin) * scale;
    let ht = (sy + 2.0 * margin) * scale + 24.0 + 16.0 * legend_rows as f64;
    let t = |x: f64, y: f64| ((x + margin) * scale, (sy + margin - y) * scale + 22.0);
    let mut o = String::new();
    let _ = write!(o, r##"<svg xmlns="http://www.w3.org/2000/svg" width="{:.0}" height="{:.0}" font-family="Segoe UI,Arial" font-size="12"><rect width="100%" height="100%" fill="#fff"/>"##, w, ht);
    let title = format!("{}  {}", h.cnc_name.clone().unwrap_or_default(), h.job.join(" / "));
    let _ = write!(o, r#"<text x="6" y="15" font-weight="600">{}</text>"#, esc(&title));
    let (x0, y0) = t(0.0, sy);
    let (x1, y1) = t(sx, 0.0);
    let _ = write!(o, r##"<rect x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" fill="#f6f8fa" stroke="#57606a"/>"##, x0, y0, x1 - x0, y1 - y0);
    if let Some(m) = m {
        let z = m.spec.clamps.general_dead_zone;
        for cx in &h.clamps {
            let (a0, b0) = t(cx - z.left, z.top);
            let (a1, b1) = t(cx + z.right, 0.0);
            let _ = write!(o, r##"<rect x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" fill="#ff000022" stroke="#d1242f" stroke-dasharray="3 2"/>"##, a0, b0, a1 - a0, b1 - b0);
        }
    }
    for hit in &prog.hits {
        let Some(st) = hit.station else { continue };
        let Some(te) = h.tool(st) else { continue };
        let col = colours.iter().find(|c| c.0 == st).map(|c| c.1).unwrap_or("#000");
        let pts: Vec<String> = te.shape().outline(hit.x, hit.y, hit.angle, 24).iter().map(|&(x, y)| {
            let (a, b) = t(x, y);
            format!("{:.1},{:.1}", a, b)
        }).collect();
        let _ = write!(o, r#"<polygon points="{}" fill="{}" fill-opacity="0.55" stroke="{}" stroke-width="0.6"/>"#, pts.join(" "), col, col);
    }
    let ly = (sy + 2.0 * margin) * scale + 26.0;
    for (i, (st, col)) in colours.iter().enumerate() {
        let lx = 6.0 + (i % 4) as f64 * (w / 4.0);
        let yy = ly + 16.0 * (i / 4) as f64 + 10.0;
        let name = h.tool(*st).map(|t| t.name.clone()).unwrap_or_default();
        let _ = write!(o, r#"<rect x="{:.0}" y="{:.0}" width="10" height="10" fill="{}"/><text x="{:.0}" y="{:.0}">T{} {}</text>"#, lx, yy - 9.0, col, lx + 14.0, yy, st, esc(&name));
    }
    o.push_str("</svg>");
    o
}
