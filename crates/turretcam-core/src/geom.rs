//! Small 2-D geometry helpers.

use serde::{Deserialize, Serialize};

pub type Pt = (f64, f64);

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BBox {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

impl Default for BBox {
    fn default() -> Self {
        Self::empty()
    }
}

impl BBox {
    pub fn empty() -> Self {
        BBox { x0: f64::INFINITY, y0: f64::INFINITY, x1: f64::NEG_INFINITY, y1: f64::NEG_INFINITY }
    }
    pub fn new(x0: f64, y0: f64, x1: f64, y1: f64) -> Self {
        BBox { x0: x0.min(x1), y0: y0.min(y1), x1: x0.max(x1), y1: y0.max(y1) }
    }
    pub fn add(&mut self, p: Pt) {
        self.x0 = self.x0.min(p.0);
        self.y0 = self.y0.min(p.1);
        self.x1 = self.x1.max(p.0);
        self.y1 = self.y1.max(p.1);
    }
    pub fn union(&mut self, o: &BBox) {
        if !o.is_empty() {
            self.add((o.x0, o.y0));
            self.add((o.x1, o.y1));
        }
    }
    pub fn w(&self) -> f64 {
        self.x1 - self.x0
    }
    pub fn h(&self) -> f64 {
        self.y1 - self.y0
    }
    pub fn is_empty(&self) -> bool {
        self.x0 > self.x1
    }
    pub fn contains(&self, p: Pt) -> bool {
        p.0 >= self.x0 && p.0 <= self.x1 && p.1 >= self.y0 && p.1 <= self.y1
    }
}

pub fn dist(a: Pt, b: Pt) -> f64 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}

/// Rotate `p` about the origin by `deg`, then translate.
pub fn xform(p: Pt, deg: f64, dx: f64, dy: f64) -> Pt {
    let (s, c) = deg.to_radians().sin_cos();
    (p.0 * c - p.1 * s + dx, p.0 * s + p.1 * c + dy)
}

/// Signed polygon area (positive = counter-clockwise).
pub fn area(poly: &[Pt]) -> f64 {
    let n = poly.len();
    let mut a = 0.0;
    for i in 0..n {
        let (x0, y0) = poly[i];
        let (x1, y1) = poly[(i + 1) % n];
        a += x0 * y1 - x1 * y0;
    }
    a / 2.0
}

pub fn point_in_poly(p: Pt, poly: &[Pt]) -> bool {
    let mut inside = false;
    let n = poly.len();
    if n < 3 {
        return false;
    }
    let mut j = n - 1;
    for i in 0..n {
        let (xi, yi) = poly[i];
        let (xj, yj) = poly[j];
        if ((yi > p.1) != (yj > p.1)) && (p.0 < (xj - xi) * (p.1 - yi) / (yj - yi) + xi) {
            inside = !inside;
        }
        j = i;
    }
    inside
}
