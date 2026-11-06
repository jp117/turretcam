//! Simple rectangular (shelf) nesting.
//!
//! Parts are placed by bounding box, tallest first, left to right in rows, starting above the
//! clamp strip (Lantek-era programs keep part edges above ~3.4" so the clamps grip scrap).

use crate::project::{Placement, Project};

#[derive(Debug, Clone)]
pub struct NestOptions {
    pub gap: f64,
    pub margin_left: f64,
    pub margin_right: f64,
    pub margin_top: f64,
    /// Bottom strip kept free for the clamps.
    pub clamp_strip: f64,
    pub allow_rotate: bool,
}

impl Default for NestOptions {
    fn default() -> Self {
        NestOptions { gap: 0.5, margin_left: 0.5, margin_right: 0.5, margin_top: 0.5, clamp_strip: 3.5, allow_rotate: true }
    }
}

/// Replace the project's placements with `qty` copies of each listed part.
/// Returns how many copies did not fit.
pub fn nest(project: &mut Project, wanted: &[(String, u32)], o: &NestOptions) -> u32 {
    let mut items: Vec<(String, f64, f64)> = vec![];
    for (id, q) in wanted {
        if let Some(p) = project.part(id) {
            let b = p.bbox();
            for _ in 0..*q {
                items.push((id.clone(), b.w(), b.h()));
            }
        }
    }
    items.sort_by(|a, b| b.2.max(b.1).partial_cmp(&a.2.max(a.1)).unwrap());
    let (sx, sy) = (project.sheet.x, project.sheet.y);
    let x_max = sx - o.margin_right;
    let y_max = sy - o.margin_top;
    let mut placements = vec![];
    let mut missed = 0;
    let (mut cx, mut cy, mut row_h) = (o.margin_left, o.clamp_strip, 0.0f64);
    for (id, w, h) in items {
        let mut options = vec![(0, w, h)];
        if o.allow_rotate && (w - h).abs() > 1e-9 {
            options.push((90, h, w));
        }
        let mut placed = false;
        // try current row first, then a new row
        for new_row in [false, true] {
            let (x, y) = if new_row { (o.margin_left, cy + row_h + o.gap) } else { (cx, cy) };
            // prefer the orientation that keeps the row height lowest
            let mut best: Option<(i32, f64, f64)> = None;
            for &(r, ww, hh) in &options {
                if x + ww <= x_max + 1e-9 && y + hh <= y_max + 1e-9 && best.map(|b| hh < b.2).unwrap_or(true) {
                    best = Some((r, ww, hh));
                }
            }
            if let Some((r, ww, hh)) = best {
                if new_row {
                    cy += row_h + o.gap;
                    row_h = 0.0;
                }
                placements.push(Placement { part: id.clone(), x, y, rot: r });
                cx = x + ww + o.gap;
                row_h = row_h.max(hh);
                placed = true;
                break;
            }
        }
        if !placed {
            missed += 1;
        }
    }
    project.placements = placements;
    missed
}
