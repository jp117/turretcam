//! Import flat-pattern drawings (DXF or DWG) as parts.
//!
//! DWG is read in-process by the pure-Rust `acadrust` crate (MPL-2.0, used unmodified, the
//! same approach as CADCraft): DWG bytes -> acadrust document -> DXF bytes -> the DXF importer
//! below. Works on Windows, macOS and Linux with nothing to install. ODA File Converter is only
//! used as a fallback if it happens to be installed and acadrust can't read a file.
//!
//! Geometry is chained into closed loops. The largest loop is the outline; inner loops are
//! recognised as round holes, rectangular openings and obround slots. Anything else is kept
//! as a `Contour` feature so the user sees it (in red) instead of it silently vanishing.

use crate::geom::{area, BBox, Pt};
use crate::part::{Feature, Outline, Part};
use std::f64::consts::PI;
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("could not read {0}: {1}")]
    Read(PathBuf, String),
    #[error("no geometry found in {0}")]
    Empty(PathBuf),
    #[error("could not read DWG {0}: {1}")]
    Dwg(PathBuf, String),
    #[error("DWG import needs ODA File Converter (free from opendesign.com); install it or set oda_converter in the profile")]
    NoOda,
    #[error("ODA File Converter failed: {0}")]
    Oda(String),
}

#[derive(Debug, Clone, Copy)]
enum Seg {
    Line(Pt, Pt),
    /// counter-clockwise arc from a0 to a1 (degrees)
    Arc { c: Pt, r: f64, a0: f64, a1: f64 },
}

impl Seg {
    fn ends(&self) -> (Pt, Pt) {
        match *self {
            Seg::Line(a, b) => (a, b),
            Seg::Arc { c, r, a0, a1 } => ((c.0 + r * a0.to_radians().cos(), c.1 + r * a0.to_radians().sin()), (c.0 + r * a1.to_radians().cos(), c.1 + r * a1.to_radians().sin())),
        }
    }
    fn sweep(&self) -> f64 {
        match *self {
            Seg::Arc { a0, a1, .. } => {
                let mut s = a1 - a0;
                while s <= 0.0 {
                    s += 360.0;
                }
                s
            }
            _ => 0.0,
        }
    }
    /// Points along the segment from `from` end, excluding the final point.
    fn sample(&self, reversed: bool) -> Vec<Pt> {
        match *self {
            Seg::Line(a, b) => vec![if reversed { b } else { a }],
            Seg::Arc { c, r, a0, .. } => {
                let sw = self.sweep();
                let n = ((sw / 5.0).ceil() as usize).max(2);
                let mut v: Vec<Pt> = (0..=n).map(|i| {
                    let t = (a0 + sw * i as f64 / n as f64).to_radians();
                    (c.0 + r * t.cos(), c.1 + r * t.sin())
                }).collect();
                if reversed {
                    v.reverse();
                }
                v.pop();
                v
            }
        }
    }
    fn scaled(&self, k: f64) -> Seg {
        match *self {
            Seg::Line(a, b) => Seg::Line((a.0 * k, a.1 * k), (b.0 * k, b.1 * k)),
            Seg::Arc { c, r, a0, a1 } => Seg::Arc { c: (c.0 * k, c.1 * k), r: r * k, a0, a1 },
        }
    }
}

/// A transform for block inserts: scale, rotate, translate.
#[derive(Clone, Copy)]
struct Xf {
    sx: f64,
    sy: f64,
    rot: f64,
    tx: f64,
    ty: f64,
}
impl Xf {
    const ID: Xf = Xf { sx: 1.0, sy: 1.0, rot: 0.0, tx: 0.0, ty: 0.0 };
    fn p(&self, p: Pt) -> Pt {
        let (s, c) = self.rot.to_radians().sin_cos();
        let (x, y) = (p.0 * self.sx, p.1 * self.sy);
        (x * c - y * s + self.tx, x * s + y * c + self.ty)
    }
    fn then(&self, outer: &Xf) -> Xf {
        // apply self, then outer
        let o = outer.p((self.tx, self.ty));
        Xf { sx: self.sx * outer.sx, sy: self.sy * outer.sy, rot: self.rot + outer.rot, tx: o.0, ty: o.1 }
    }
    fn seg(&self, s: Seg) -> Seg {
        match s {
            Seg::Line(a, b) => Seg::Line(self.p(a), self.p(b)),
            Seg::Arc { c, r, a0, a1 } => {
                let mirrored = self.sx * self.sy < 0.0;
                let (b0, b1) = if mirrored { (180.0 - a1, 180.0 - a0) } else { (a0, a1) };
                Seg::Arc { c: self.p(c), r: r * self.sx.abs(), a0: b0 + self.rot, a1: b1 + self.rot }
            }
        }
    }
}

fn bulge_seg(p0: Pt, p1: Pt, b: f64) -> Seg {
    if b.abs() < 1e-9 {
        return Seg::Line(p0, p1);
    }
    let theta = 4.0 * b.atan();
    let d = ((p1.0 - p0.0).powi(2) + (p1.1 - p0.1).powi(2)).sqrt();
    let r = d / (2.0 * (theta / 2.0).sin()).abs();
    let (mx, my) = ((p0.0 + p1.0) / 2.0, (p0.1 + p1.1) / 2.0);
    let h = (r * r - (d / 2.0).powi(2)).max(0.0).sqrt();
    let (ux, uy) = ((p1.0 - p0.0) / d, (p1.1 - p0.1) / d);
    // centre is to the left of p0->p1 for a ccw (b>0) minor arc
    let sign = if (b > 0.0) == (theta.abs() < PI) { 1.0 } else { -1.0 };
    let c = (mx - uy * h * sign, my + ux * h * sign);
    let ang = |p: Pt| (p.1 - c.1).atan2(p.0 - c.0).to_degrees();
    if b > 0.0 {
        Seg::Arc { c, r, a0: ang(p0), a1: ang(p1) }
    } else {
        Seg::Arc { c, r, a0: ang(p1), a1: ang(p0) }
    }
}

fn collect(drawing: &dxf::Drawing, ents: &mut dyn Iterator<Item = &dxf::entities::Entity>, xf: Xf, depth: u32, segs: &mut Vec<Seg>, circles: &mut Vec<(Pt, f64)>) {
    use dxf::entities::EntityType as E;
    for e in ents {
        match &e.specific {
            E::Line(l) => segs.push(xf.seg(Seg::Line((l.p1.x, l.p1.y), (l.p2.x, l.p2.y)))),
            E::Arc(a) => segs.push(xf.seg(Seg::Arc { c: (a.center.x, a.center.y), r: a.radius, a0: a.start_angle, a1: a.end_angle })),
            E::Circle(c) => circles.push((xf.p((c.center.x, c.center.y)), c.radius * xf.sx.abs())),
            E::LwPolyline(p) => {
                let v = &p.vertices;
                let n = v.len();
                let closed = p.is_closed();
                for i in 0..n {
                    if i + 1 == n && !closed {
                        break;
                    }
                    let (a, b) = (&v[i], &v[(i + 1) % n]);
                    segs.push(xf.seg(bulge_seg((a.x, a.y), (b.x, b.y), a.bulge)));
                }
            }
            E::Polyline(p) => {
                let v: Vec<_> = p.vertices().collect();
                let n = v.len();
                let closed = p.is_closed();
                for i in 0..n {
                    if i + 1 == n && !closed {
                        break;
                    }
                    let (a, b) = (v[i], v[(i + 1) % n]);
                    segs.push(xf.seg(bulge_seg((a.location.x, a.location.y), (b.location.x, b.location.y), a.bulge)));
                }
            }
            E::Insert(ins) if depth < 8 => {
                if let Some(block) = drawing.blocks().find(|b| b.name == ins.name) {
                    let local = Xf { sx: ins.x_scale_factor, sy: ins.y_scale_factor, rot: ins.rotation, tx: ins.location.x, ty: ins.location.y };
                    let base = Xf { sx: 1.0, sy: 1.0, rot: 0.0, tx: -block.base_point.x, ty: -block.base_point.y };
                    let x = base.then(&local).then(&xf);
                    collect(drawing, &mut block.entities.iter(), x, depth + 1, segs, circles);
                }
            }
            _ => {}
        }
    }
}

/// Chain segments into loops (and leftover open chains).
fn chain(segs: Vec<Seg>, tol: f64) -> (Vec<Vec<(Seg, bool)>>, Vec<Vec<(Seg, bool)>>) {
    let close = |a: Pt, b: Pt| (a.0 - b.0).abs() <= tol && (a.1 - b.1).abs() <= tol;
    let mut used = vec![false; segs.len()];
    let mut loops = vec![];
    let mut open = vec![];
    for start in 0..segs.len() {
        if used[start] {
            continue;
        }
        used[start] = true;
        let mut chain = vec![(segs[start], false)];
        let (first, mut end) = segs[start].ends();
        // full circle as an arc
        if segs[start].sweep() >= 359.999 {
            loops.push(chain);
            continue;
        }
        loop {
            if close(end, first) {
                loops.push(chain);
                break;
            }
            let next = (0..segs.len()).find(|&i| !used[i] && {
                let (a, b) = segs[i].ends();
                close(a, end) || close(b, end)
            });
            match next {
                Some(i) => {
                    used[i] = true;
                    let (a, b) = segs[i].ends();
                    let rev = !close(a, end);
                    end = if rev { a } else { b };
                    chain.push((segs[i], rev));
                }
                None => {
                    open.push(chain);
                    break;
                }
            }
        }
    }
    (loops, open)
}

fn loop_points(l: &[(Seg, bool)]) -> Vec<Pt> {
    l.iter().flat_map(|(s, r)| s.sample(*r)).collect()
}

/// Merge collinear consecutive points.
fn simplify(pts: &[Pt]) -> Vec<Pt> {
    let n = pts.len();
    let mut out = vec![];
    for i in 0..n {
        let a = pts[(i + n - 1) % n];
        let b = pts[i];
        let c = pts[(i + 1) % n];
        let cross = (b.0 - a.0) * (c.1 - b.1) - (b.1 - a.1) * (c.0 - b.0);
        if cross.abs() > 1e-7 {
            out.push(b);
        }
    }
    out
}

fn as_rect(l: &[(Seg, bool)]) -> Option<BBox> {
    if l.iter().any(|(s, _)| matches!(s, Seg::Arc { .. })) {
        return None;
    }
    let pts = simplify(&loop_points(l));
    if pts.len() != 4 {
        return None;
    }
    for i in 0..4 {
        let (a, b) = (pts[i], pts[(i + 1) % 4]);
        if (a.0 - b.0).abs() > 1e-4 && (a.1 - b.1).abs() > 1e-4 {
            return None;
        }
    }
    let mut bb = BBox::empty();
    pts.iter().for_each(|p| bb.add(*p));
    Some(bb)
}

/// (centre, length, width, vertical)
fn as_obround(l: &[(Seg, bool)]) -> Option<(Pt, f64, f64, bool)> {
    let arcs: Vec<&Seg> = l.iter().map(|(s, _)| s).filter(|s| matches!(s, Seg::Arc { .. })).collect();
    let lines: Vec<&Seg> = l.iter().map(|(s, _)| s).filter(|s| matches!(s, Seg::Line(..))).collect();
    if arcs.len() != 2 || lines.len() != 2 {
        return None;
    }
    let (Seg::Arc { c: c1, r: r1, .. }, Seg::Arc { c: c2, r: r2, .. }) = (*arcs[0], *arcs[1]) else { return None };
    if (r1 - r2).abs() > 1e-4 || (arcs[0].sweep() - 180.0).abs() > 0.5 || (arcs[1].sweep() - 180.0).abs() > 0.5 {
        return None;
    }
    let vertical = (c1.0 - c2.0).abs() < 1e-4;
    let horizontal = (c1.1 - c2.1).abs() < 1e-4;
    if !(vertical || horizontal) {
        return None;
    }
    let d = ((c1.0 - c2.0).powi(2) + (c1.1 - c2.1).powi(2)).sqrt();
    Some((((c1.0 + c2.0) / 2.0, (c1.1 + c2.1) / 2.0), d + 2.0 * r1, 2.0 * r1, vertical && !horizontal))
}

fn as_circle(l: &[(Seg, bool)]) -> Option<(Pt, f64)> {
    let mut c0 = None;
    let mut total = 0.0;
    for (s, _) in l {
        let Seg::Arc { c, r, .. } = *s else { return None };
        match c0 {
            None => c0 = Some((c, r)),
            Some((cc, rr)) if (cc.0 - c.0).abs() < 1e-4 && (cc.1 - c.1).abs() < 1e-4 && (rr - r).abs() < 1e-4 => {}
            _ => return None,
        }
        total += s.sweep();
    }
    if (total - 360.0).abs() < 0.5 {
        c0
    } else {
        None
    }
}

/// Units scale from $INSUNITS (default inches).
fn unit_scale(d: &dxf::Drawing) -> f64 {
    use dxf::enums::Units;
    match d.header.default_drawing_units {
        Units::Millimeters => 1.0 / 25.4,
        Units::Centimeters => 1.0 / 2.54,
        Units::Meters => 1000.0 / 25.4,
        Units::Feet => 12.0,
        _ => 1.0,
    }
}

/// Import a DXF file as a part.
pub fn import_dxf(path: &Path, id: &str) -> Result<Part, ImportError> {
    let d = dxf::Drawing::load_file(path).map_err(|e| ImportError::Read(path.to_path_buf(), e.to_string()))?;
    import_drawing(&d, path, id)
}

/// Import a part from DXF bytes (e.g. a converted DWG); `path` is used for names and messages.
pub fn import_dxf_bytes(bytes: &[u8], path: &Path, id: &str) -> Result<Part, ImportError> {
    let d = dxf::Drawing::load(&mut std::io::Cursor::new(bytes)).map_err(|e| ImportError::Read(path.to_path_buf(), e.to_string()))?;
    import_drawing(&d, path, id)
}

/// DWG (R13-2018) bytes -> DXF bytes, in-process with acadrust. Returns the DXF and any
/// notes acadrust reported about records it had to skip.
pub fn dwg_to_dxf_bytes(bytes: &[u8]) -> Result<(Vec<u8>, Vec<String>), String> {
    let outcome = acadrust::DwgReader::from_stream_with_options(std::io::Cursor::new(bytes.to_vec()), acadrust::DwgReadOptions::failsafe())
        .read_with_stats()
        .map_err(|e| e.to_string())?;
    let st = &outcome.stats;
    let mut notes = vec![];
    if st.skipped_source_records > 0 {
        notes.push(format!("{} of {} DWG records ({}) couldn't be read and were left out; check the part against the drawing", st.skipped_source_records, st.source_records, st.source_version));
    }
    let doc = outcome.document;
    let dxf = acadrust::DxfWriter::new(&doc).write_to_vec().map_err(|e| e.to_string())?;
    Ok((dxf, notes))
}

/// Is this a DWG file? (magic "AC10..")
pub fn is_dwg(bytes: &[u8]) -> bool {
    bytes.len() > 6 && &bytes[..4] == b"AC10"
}

fn import_drawing(d: &dxf::Drawing, path: &Path, id: &str) -> Result<Part, ImportError> {
    let k = unit_scale(d);
    let mut segs = vec![];
    let mut circles = vec![];
    collect(d, &mut d.entities(), Xf::ID, 0, &mut segs, &mut circles);
    let segs: Vec<Seg> = segs.into_iter().map(|s| s.scaled(k)).collect();
    let circles: Vec<(Pt, f64)> = circles.into_iter().map(|(c, r)| ((c.0 * k, c.1 * k), r * k)).collect();
    if segs.is_empty() && circles.is_empty() {
        return Err(ImportError::Empty(path.to_path_buf()));
    }
    let name = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| id.to_string());
    Ok(build_part(segs, circles, id, &name, &path.display().to_string()))
}

fn build_part(segs: Vec<Seg>, circles: Vec<(Pt, f64)>, id: &str, name: &str, source: &str) -> Part {
    let (loops, open) = chain(segs, 0.002);
    // outline = loop with the biggest bbox
    let bb = |l: &Vec<(Seg, bool)>| {
        let mut b = BBox::empty();
        loop_points(l).iter().for_each(|p| b.add(*p));
        b
    };
    let outer_idx = loops.iter().enumerate().max_by(|a, b| {
        let (x, y) = (bb(a.1), bb(b.1));
        (x.w() * x.h()).partial_cmp(&(y.w() * y.h())).unwrap()
    }).map(|(i, _)| i);
    let mut all = BBox::empty();
    if let Some(i) = outer_idx {
        all = bb(&loops[i]);
    } else {
        for (c, r) in &circles {
            all.add((c.0 - r, c.1 - r));
            all.add((c.0 + r, c.1 + r));
        }
    }
    let (ox, oy) = (all.x0, all.y0);
    let sh = |p: Pt| (round4(p.0 - ox), round4(p.1 - oy));
    let mut features = vec![];
    let mut outline = Outline::None;
    let mut notes = vec![];
    for (i, l) in loops.iter().enumerate() {
        if Some(i) == outer_idx {
            outline = match as_rect(l) {
                Some(r) => Outline::Rect { w: round4(r.w()), h: round4(r.h()), tabs: true },
                None => {
                    let mut pts: Vec<Pt> = simplify(&loop_points(l)).into_iter().map(sh).collect();
                    if area(&pts) < 0.0 {
                        pts.reverse();
                    }
                    Outline::Polygon { points: pts }
                }
            };
            continue;
        }
        if let Some((c, r)) = as_circle(l) {
            let c = sh(c);
            features.push(Feature::Hole { x: c.0, y: c.1, dia: round4(2.0 * r) });
        } else if let Some(r) = as_rect(l) {
            let (a, b) = (sh((r.x0, r.y0)), sh((r.x1, r.y1)));
            features.push(Feature::Rect { x0: a.0, y0: a.1, x1: b.0, y1: b.1, tabs: true });
        } else if let Some((c, len, w, vertical)) = as_obround(l) {
            let c = sh(c);
            features.push(Feature::Slot { x: c.0, y: c.1, length: round4(len), width: round4(w), vertical });
        } else {
            features.push(Feature::Contour { points: loop_points(l).into_iter().map(sh).collect(), closed: true, note: "closed shape".into() });
        }
    }
    for (c, r) in circles {
        let c = sh(c);
        features.push(Feature::Hole { x: c.0, y: c.1, dia: round4(2.0 * r) });
    }
    for o in &open {
        let mut pts = loop_points(o);
        if let Some((s, r)) = o.last() {
            let (a, b) = s.ends();
            pts.push(if *r { a } else { b });
        }
        features.push(Feature::Contour { points: pts.into_iter().map(sh).collect(), closed: false, note: "open geometry (not a closed loop)".into() });
    }
    if !open.is_empty() {
        notes.push(format!("{} open chain(s) found; check the drawing for gaps", open.len()));
    }
    Part { id: id.into(), name: name.into(), outline, features, notes: notes.join("; "), source: source.into() }
}

fn round4(v: f64) -> f64 {
    (v * 10000.0).round() / 10000.0
}

/// Locate ODAFileConverter.exe (profile setting, PATH, or Program Files).
pub fn find_oda(configured: Option<&str>) -> Option<PathBuf> {
    if let Some(c) = configured {
        let p = PathBuf::from(c);
        if p.exists() {
            return Some(p);
        }
    }
    for root in [r"C:\Program Files\ODA", r"C:\Program Files (x86)\ODA"] {
        if let Ok(rd) = std::fs::read_dir(root) {
            let mut dirs: Vec<PathBuf> = rd.filter_map(|e| e.ok()).map(|e| e.path()).collect();
            dirs.sort();
            for d in dirs.into_iter().rev() {
                let exe = d.join("ODAFileConverter.exe");
                if exe.exists() {
                    return Some(exe);
                }
            }
        }
    }
    None
}

/// Convert a DWG to DXF with ODA File Converter; returns the DXF path.
pub fn dwg_to_dxf(dwg: &Path, oda: Option<&str>) -> Result<PathBuf, ImportError> {
    let exe = find_oda(oda).ok_or(ImportError::NoOda)?;
    let src_dir = dwg.parent().unwrap_or(Path::new("."));
    let out_dir = std::env::temp_dir().join(format!("turretcam-oda-{}", std::process::id()));
    std::fs::create_dir_all(&out_dir).map_err(|e| ImportError::Oda(e.to_string()))?;
    let file = dwg.file_name().unwrap().to_string_lossy().to_string();
    let status = std::process::Command::new(&exe)
        .arg(src_dir)
        .arg(&out_dir)
        .arg("ACAD2018")
        .arg("DXF")
        .arg("0")
        .arg("1")
        .arg(&file)
        .status()
        .map_err(|e| ImportError::Oda(e.to_string()))?;
    let dxf = out_dir.join(Path::new(&file).with_extension("dxf"));
    if !status.success() || !dxf.exists() {
        return Err(ImportError::Oda(format!("exit {:?}, no {}", status.code(), dxf.display())));
    }
    Ok(dxf)
}

/// Import .dxf or .dwg. DWG goes through acadrust; ODA is tried only if acadrust fails and
/// ODA File Converter is installed.
pub fn import_file(path: &Path, id: &str, oda: Option<&str>) -> Result<Part, ImportError> {
    let bytes = std::fs::read(path).map_err(|e| ImportError::Read(path.to_path_buf(), e.to_string()))?;
    if !is_dwg(&bytes) {
        return import_dxf(path, id);
    }
    let first = dwg_to_dxf_bytes(&bytes).map_err(|e| ImportError::Dwg(path.to_path_buf(), e)).and_then(|(dxf, notes)| {
        let mut p = import_dxf_bytes(&dxf, path, id)?;
        if !notes.is_empty() {
            p.notes = [p.notes.clone(), notes.join("; ")].iter().filter(|s| !s.is_empty()).cloned().collect::<Vec<_>>().join("; ");
        }
        Ok(p)
    });
    match first {
        Ok(p) => Ok(p),
        Err(e) => match find_oda(oda) {
            Some(_) => {
                let dxf = dwg_to_dxf(path, oda)?;
                let mut p = import_dxf(&dxf, id)?;
                p.source = path.display().to_string();
                p.name = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or(p.name);
                p.notes = format!("read with ODA File Converter after: {}", e);
                Ok(p)
            }
            None => Err(e),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recognises_features() {
        // 10 x 6 plate, a 0.5 hole, a 2 x 1 window, an obround slot made of 2 lines + 2 arcs
        let mut segs = vec![];
        let rect = |x0: f64, y0: f64, x1: f64, y1: f64, v: &mut Vec<Seg>| {
            v.push(Seg::Line((x0, y0), (x1, y0)));
            v.push(Seg::Line((x1, y0), (x1, y1)));
            v.push(Seg::Line((x1, y1), (x0, y1)));
            v.push(Seg::Line((x0, y1), (x0, y0)));
        };
        rect(5.0, 5.0, 15.0, 11.0, &mut segs);
        rect(7.0, 6.0, 9.0, 7.0, &mut segs);
        segs.push(Seg::Line((10.0, 8.0), (11.0, 8.0)));
        segs.push(Seg::Arc { c: (11.0, 8.125), r: 0.125, a0: -90.0, a1: 90.0 });
        segs.push(Seg::Line((11.0, 8.25), (10.0, 8.25)));
        segs.push(Seg::Arc { c: (10.0, 8.125), r: 0.125, a0: 90.0, a1: 270.0 });
        let p = build_part(segs, vec![((12.0, 9.0), 0.25)], "P1", "t", "");
        assert_eq!(p.outline, Outline::Rect { w: 10.0, h: 6.0, tabs: true });
        assert!(p.features.contains(&Feature::Hole { x: 7.0, y: 4.0, dia: 0.5 }));
        assert!(p.features.contains(&Feature::Rect { x0: 2.0, y0: 1.0, x1: 4.0, y1: 2.0, tabs: true }));
        assert!(p.features.iter().any(|f| matches!(f, Feature::Slot { length, width, vertical: false, .. } if (*length - 1.25).abs() < 1e-9 && (*width - 0.25).abs() < 1e-9)));
    }
    #[test]
    fn dwg_roundtrip_through_acadrust() {
        use acadrust::{CadDocument, Circle, EntityType, Line, Vector3};
        let mut doc = CadDocument::new();
        let l = |x0: f64, y0: f64, x1: f64, y1: f64| EntityType::Line(Line::from_points(Vector3::new(x0, y0, 0.0), Vector3::new(x1, y1, 0.0)));
        for e in [l(0.0, 0.0, 10.0, 0.0), l(10.0, 0.0, 10.0, 6.0), l(10.0, 6.0, 0.0, 6.0), l(0.0, 6.0, 0.0, 0.0)] {
            doc.add_entity(e).unwrap();
        }
        doc.add_entity(EntityType::Circle(Circle::from_center_radius(Vector3::new(2.0, 3.0, 0.0), 0.25))).unwrap();
        let dwg = acadrust::DwgWriter::write_to_vec(&doc).unwrap();
        assert!(is_dwg(&dwg));
        let (dxf, _) = dwg_to_dxf_bytes(&dwg).unwrap();
        let p = import_dxf_bytes(&dxf, Path::new("plate.dwg"), "P1").unwrap();
        assert_eq!(p.outline, Outline::Rect { w: 10.0, h: 6.0, tabs: true });
        assert!(p.features.contains(&Feature::Hole { x: 2.0, y: 3.0, dia: 0.5 }), "{:?}", p.features);
    }

    #[test]
    fn bulge_semicircle() {
        let s = bulge_seg((0.0, 0.0), (2.0, 0.0), 1.0);
        if let Seg::Arc { c, r, .. } = s {
            assert!((c.0 - 1.0).abs() < 1e-9 && c.1.abs() < 1e-9 && (r - 1.0).abs() < 1e-9);
        } else {
            panic!()
        }
    }
}
