//! Part model: what a human draws or imports. Coordinates are inches in the part's own frame,
//! origin at the lower-left of the outline.

use crate::cam::{CamError, Job, ToolRef};
use crate::geom::{xform, BBox, Pt};
use crate::tooling::ToolKind;
use serde::{Deserialize, Serialize};

/// Punch footprint used by grids and single shape hits (the generator finds the tool).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Footprint {
    pub kind: ToolKind,
    /// size along X
    pub w: f64,
    /// size along Y
    pub h: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Feature {
    /// Round hole centred at (x, y).
    Hole { x: f64, y: f64, dia: f64 },
    /// Obround slot centred at (x, y); `vertical` = long axis along Y.
    Slot {
        x: f64,
        y: f64,
        length: f64,
        width: f64,
        #[serde(default)]
        vertical: bool,
    },
    /// Rectangular opening: one hit if a tool matches exactly, else a nibbled window.
    Rect {
        x0: f64,
        y0: f64,
        x1: f64,
        y1: f64,
        #[serde(default = "yes")]
        tabs: bool,
    },
    /// nx x ny hits of a footprint (louvers / vents), first hit at (x, y).
    Grid { x: f64, y: f64, dx: f64, dy: f64, nx: u32, ny: u32, shape: Footprint },
    /// n round holes of `dia` on radius r round (x, y).
    BoltCircle {
        x: f64,
        y: f64,
        r: f64,
        n: u32,
        dia: f64,
        #[serde(default)]
        start: f64,
    },
    /// Single hit of a named tool (escape hatch: corner notches, special tools).
    Hit {
        x: f64,
        y: f64,
        tool: ToolRef,
        #[serde(default)]
        angle: f64,
    },
    /// Imported geometry that was not recognised as a punchable feature.
    Contour {
        points: Vec<Pt>,
        closed: bool,
        #[serde(default)]
        note: String,
    },
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Outline {
    /// No outline cut (part is the whole sheet, or cut elsewhere).
    None,
    /// Rectangle 0,0 .. w,h.
    Rect {
        w: f64,
        h: f64,
        #[serde(default = "yes")]
        tabs: bool,
    },
    /// Any closed polygon (shown, not yet cut automatically).
    Polygon { points: Vec<Pt> },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Part {
    pub id: String,
    pub name: String,
    pub outline: Outline,
    #[serde(default)]
    pub features: Vec<Feature>,
    #[serde(default)]
    pub notes: String,
    /// Where it came from (DXF path etc.)
    #[serde(default)]
    pub source: String,
}

impl Part {
    pub fn new_rect(id: &str, name: &str, w: f64, h: f64) -> Part {
        Part { id: id.into(), name: name.into(), outline: Outline::Rect { w, h, tabs: true }, features: vec![], notes: String::new(), source: String::new() }
    }

    pub fn bbox(&self) -> BBox {
        let mut b = BBox::empty();
        match &self.outline {
            Outline::Rect { w, h, .. } => {
                b.add((0.0, 0.0));
                b.add((*w, *h));
            }
            Outline::Polygon { points } => points.iter().for_each(|p| b.add(*p)),
            Outline::None => {}
        }
        if b.is_empty() {
            // no outline: the part frame is anchored at its own origin
            b.add((0.0, 0.0));
            for f in &self.features {
                b.union(&f.bbox());
            }
        }
        b
    }

    /// Things the generator cannot program yet (shown to the user before they export).
    pub fn unsupported(&self) -> Vec<String> {
        let mut v = vec![];
        if let Outline::Polygon { .. } = self.outline {
            v.push(format!("{}: outline is not a rectangle; it will not be cut (draw it as a rectangle plus notches, or cut it elsewhere)", self.name));
        }
        for f in &self.features {
            if let Feature::Contour { note, .. } = f {
                v.push(format!("{}: unrecognised contour ({}) will not be punched", self.name, note));
            }
        }
        v
    }
}

impl Feature {
    pub fn bbox(&self) -> BBox {
        let mut b = BBox::empty();
        match self {
            Feature::Hole { x, y, dia } => {
                b.add((x - dia / 2.0, y - dia / 2.0));
                b.add((x + dia / 2.0, y + dia / 2.0));
            }
            Feature::Slot { x, y, length, width, vertical } => {
                let (w, h) = if *vertical { (*width, *length) } else { (*length, *width) };
                b.add((x - w / 2.0, y - h / 2.0));
                b.add((x + w / 2.0, y + h / 2.0));
            }
            Feature::Rect { x0, y0, x1, y1, .. } => {
                b.add((*x0, *y0));
                b.add((*x1, *y1));
            }
            Feature::Grid { x, y, dx, dy, nx, ny, shape } => {
                let xe = x + dx * nx.saturating_sub(1) as f64;
                let ye = y + dy * ny.saturating_sub(1) as f64;
                for (px, py) in [(*x, *y), (xe, ye)] {
                    b.add((px - shape.w / 2.0, py - shape.h / 2.0));
                    b.add((px + shape.w / 2.0, py + shape.h / 2.0));
                }
            }
            Feature::BoltCircle { x, y, r, dia, .. } => {
                b.add((x - r - dia / 2.0, y - r - dia / 2.0));
                b.add((x + r + dia / 2.0, y + r + dia / 2.0));
            }
            Feature::Hit { x, y, .. } => b.add((*x, *y)),
            Feature::Contour { points, .. } => points.iter().for_each(|p| b.add(*p)),
        }
        b
    }

    /// Rotate by `rot` degrees (multiple of 90) about the origin, then translate.
    pub fn transformed(&self, rot: i32, dx: f64, dy: f64) -> Feature {
        let r = ((rot % 360) + 360) % 360;
        let t = |p: Pt| xform(p, r as f64, dx, dy);
        let quarter = r == 90 || r == 270;
        match self {
            Feature::Hole { x, y, dia } => {
                let (nx, ny) = t((*x, *y));
                Feature::Hole { x: nx, y: ny, dia: *dia }
            }
            Feature::Slot { x, y, length, width, vertical } => {
                let (nx, ny) = t((*x, *y));
                Feature::Slot { x: nx, y: ny, length: *length, width: *width, vertical: if quarter { !vertical } else { *vertical } }
            }
            Feature::Rect { x0, y0, x1, y1, tabs } => {
                let a = t((*x0, *y0));
                let b = t((*x1, *y1));
                Feature::Rect { x0: a.0.min(b.0), y0: a.1.min(b.1), x1: a.0.max(b.0), y1: a.1.max(b.1), tabs: *tabs }
            }
            Feature::Grid { x, y, dx: gx, dy: gy, nx, ny, shape } => {
                // re-anchor at the transformed lower-left hit so pitches stay positive
                let xe = x + gx * nx.saturating_sub(1) as f64;
                let ye = y + gy * ny.saturating_sub(1) as f64;
                let a = t((*x, *y));
                let b = t((xe, ye));
                let (sw, sh, pnx, pny, pdx, pdy) = if quarter { (shape.h, shape.w, *ny, *nx, *gy, *gx) } else { (shape.w, shape.h, *nx, *ny, *gx, *gy) };
                Feature::Grid {
                    x: a.0.min(b.0),
                    y: a.1.min(b.1),
                    dx: pdx.abs(),
                    dy: pdy.abs(),
                    nx: pnx,
                    ny: pny,
                    shape: Footprint { kind: shape.kind, w: sw, h: sh },
                }
            }
            Feature::BoltCircle { x, y, r: rad, n, dia, start } => {
                let (nx, ny) = t((*x, *y));
                Feature::BoltCircle { x: nx, y: ny, r: *rad, n: *n, dia: *dia, start: (start + r as f64) % 360.0 }
            }
            Feature::Hit { x, y, tool, angle } => {
                let (nx, ny) = t((*x, *y));
                Feature::Hit { x: nx, y: ny, tool: tool.clone(), angle: *angle }
            }
            Feature::Contour { points, closed, note } => Feature::Contour { points: points.iter().map(|p| t(*p)).collect(), closed: *closed, note: note.clone() },
        }
    }

    /// Add this feature (already in sheet coordinates) to a job.
    pub fn apply(&self, job: &mut Job) -> Result<(), CamError> {
        match self {
            Feature::Hole { x, y, dia } => job.hole(*x, *y, *dia).map(|_| ()),
            Feature::Slot { x, y, length, width, vertical } => job.slot(*x, *y, *length, *width, *vertical).map(|_| ()),
            Feature::Rect { x0, y0, x1, y1, tabs } => job.cutout(*x0, *y0, *x1, *y1, *tabs),
            Feature::Grid { x, y, dx, dy, nx, ny, shape } => {
                let (st, a) = job
                    .find_footprint(Some(shape.kind), shape.w, shape.h, 0.002)
                    .ok_or_else(|| CamError(format!("no mounted {:?} tool is {} x {} for the grid", shape.kind, shape.w, shape.h)))?;
                job.grid(st, *x, *y, *dx, *dy, *nx, *ny, a);
                Ok(())
            }
            Feature::BoltCircle { x, y, r, n, dia, start } => {
                let (st, _) = job.find_footprint(Some(ToolKind::RD), *dia, *dia, 0.002).ok_or_else(|| CamError(format!("no round tool D{} for the bolt circle", dia)))?;
                job.bolt_circle(st, *x, *y, *r, *n, *start);
                Ok(())
            }
            Feature::Hit { x, y, tool, angle } => job.hit(tool, *x, *y, *angle).map(|_| ()),
            Feature::Contour { .. } => Ok(()),
        }
    }
}
