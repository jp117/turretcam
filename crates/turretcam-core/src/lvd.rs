//! Build a machine definition from an LVD / Lantek machine package plus real programs.
//!
//! Inputs (unzip the LVD `... - V31 - SETUP.exe`; it is a WinZip self-extractor):
//! * `<post>.PS1` post parameters (B016/B017 reset position)
//! * `<post>.CT1` machine config (A100-A122 clamps, A150-A155 work zone, A504-A509 hold-downs,
//!   C000 nibble tolerance, C001 press capacity)
//! * `torreta.xml` turret stations (size class, max tool, autoindex, clamp dead zones)
//! * an archive folder of `.CNC` programs run on the machine: their tool comments say which
//!   tool sat in each station, when, and at what mount angle.
//!
//! All Lantek values are millimetres; the result is in inches.

use crate::cnc;
use crate::machine::*;
use std::collections::BTreeMap;
use std::path::Path;

const MM: f64 = 25.4;

/// Lantek key/value config: `A150 2500.000000 ; Longitud.` -> {"A150": ("2500.000000", "Longitud.")}
pub fn read_cfg(path: &Path) -> std::io::Result<BTreeMap<String, (String, String)>> {
    let bytes = std::fs::read(path)?;
    let text: String = bytes.iter().map(|&b| b as char).collect();
    let mut m = BTreeMap::new();
    for line in text.lines() {
        let line = line.trim_end();
        let b = line.as_bytes();
        if b.len() < 4 || !b[0].is_ascii_uppercase() || !b[1..4].iter().all(|c| c.is_ascii_digit()) {
            continue;
        }
        if b.len() > 4 && !(b[4] as char).is_whitespace() {
            continue;
        }
        let rest = line[4..].trim();
        let (val, comment) = match rest.split_once(';') {
            Some((v, c)) => (v.trim().to_string(), c.trim().to_string()),
            None => (rest.to_string(), String::new()),
        };
        m.insert(line[..4].to_string(), (val, comment));
    }
    Ok(m)
}

fn nums(s: &str) -> Vec<f64> {
    let mut out = vec![];
    let mut cur = String::new();
    for ch in s.chars().chain(std::iter::once(' ')) {
        if ch.is_ascii_digit() || ch == '.' || (ch == '-' && cur.is_empty()) {
            cur.push(ch);
        } else {
            if let Ok(v) = cur.trim_end_matches('.').parse::<f64>() {
                if cur.chars().any(|c| c.is_ascii_digit()) {
                    out.push(v);
                }
            }
            cur.clear();
        }
    }
    out
}

fn v(m: &BTreeMap<String, (String, String)>, k: &str) -> f64 {
    m.get(k).map(|x| nums(&x.0).first().copied().unwrap_or(0.0)).unwrap_or(0.0)
}

fn r4(x: f64) -> f64 {
    (x * 10000.0).round() / 10000.0
}

/// Turret stations from `torreta.xml`.
pub fn read_turret(path: &Path) -> Result<(String, Vec<Station>), String> {
    use quick_xml::events::Event;
    let xml = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let mut rd = quick_xml::Reader::from_str(&xml);
    // stack of S table names ("" for R)
    let mut stack: Vec<String> = vec![];
    let mut name = String::new();
    let mut stations = vec![];
    let mut cur: BTreeMap<String, String> = BTreeMap::new();
    let mut angles: Vec<f64> = vec![];
    loop {
        match rd.read_event() {
            Ok(Event::Start(e)) => {
                let tag = e.name().as_ref().to_string();
                let t = e.attributes().flatten().find(|a| a.key.as_ref() == "T").map(|a| a.value.to_string()).unwrap_or_default();
                if tag == "R" && stack.last().map(|s| s == "RS_TTRR_00000700").unwrap_or(false) {
                    cur.clear();
                    angles.clear();
                }
                stack.push(if tag == "S" { t } else { tag });
            }
            Ok(Event::End(e)) => {
                let tag = e.name().as_ref().to_string();
                stack.pop();
                if tag == "R" && stack.last().map(|s| s == "RS_TTRR_00000700").unwrap_or(false) {
                    let g = |k: &str| cur.get(k).and_then(|x| x.parse::<f64>().ok()).unwrap_or(0.0);
                    stations.push(Station {
                        station: g("TNumber") as u32,
                        size_class: cur.get("StTRef").cloned().unwrap_or_default(),
                        max_tool_in: r4(g("DMax") / MM),
                        autoindex: cur.get("Autoindex").map(|x| x == "YES").unwrap_or(false),
                        key_angles: angles.clone(),
                        clamp_zone: Zone { left: r4(g("ZLeft") / MM), right: r4(g("ZRight") / MM), top: r4(g("ZTop") / MM) },
                        mounted: None,
                        history: vec![],
                    });
                }
            }
            Ok(Event::Empty(e)) if e.name().as_ref() == "F" => {
                let mut fname = String::new();
                let mut fval = String::new();
                for a in e.attributes().flatten() {
                    match a.key.as_ref() {
                        "FN" => fname = a.value.to_string(),
                        "V" => fval = a.value.to_string(),
                        _ => {}
                    }
                }
                let n = stack.len();
                let parent_table = if n >= 2 { stack[n - 2].as_str() } else { "" };
                match parent_table {
                    "RS_TTRR_00000700" => {
                        cur.insert(fname, fval);
                    }
                    "RS_TTRR_00000800" if fname == "SAngle" => {
                        if let Ok(a) = fval.parse() {
                            angles.push(a);
                        }
                    }
                    "RS_TTRR_00000600" if fname == "TurrRef" => name = fval,
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(e.to_string()),
            _ => {}
        }
    }
    stations.sort_by_key(|s| s.station);
    Ok((name, stations))
}

/// Fill `mounted` and `history` from an archive of programs.
pub fn scan_archive(dir: &Path, stations: &mut [Station]) -> std::io::Result<usize> {
    #[derive(Default)]
    struct U {
        programs: u32,
        first: String,
        last: String,
        c: BTreeMap<String, u32>,
    }
    let mut use_: BTreeMap<(u32, String), U> = BTreeMap::new();
    let mut latest: BTreeMap<u32, (String, String)> = BTreeMap::new();
    let mut n = 0;
    for e in std::fs::read_dir(dir)? {
        let p = e?.path();
        let ext = p.extension().map(|x| x.to_string_lossy().to_uppercase()).unwrap_or_default();
        if ext != "CNC" {
            continue;
        }
        let day = std::fs::metadata(&p)?.modified().ok().map(|t| chrono::DateTime::<chrono::Local>::from(t).format("%Y-%m-%d").to_string()).unwrap_or_default();
        let bytes = std::fs::read(&p)?;
        let text: String = bytes.iter().map(|&b| b as char).collect();
        n += 1;
        for line in text.lines().take(80) {
            let l = line.trim();
            if !l.starts_with("(T") {
                continue;
            }
            if let Some(t) = cnc::parse_tool_comment(l) {
                let u = use_.entry((t.station, t.name.clone())).or_default();
                u.programs += 1;
                if u.first.is_empty() || day < u.first {
                    u.first = day.clone();
                }
                if day > u.last {
                    u.last = day.clone();
                }
                let key = if t.aidx { "AIDX".to_string() } else { format!("{}", t.c.unwrap_or(0.0)) };
                *u.c.entry(key).or_default() += 1;
                let e = latest.entry(t.station).or_insert((String::new(), String::new()));
                if day >= e.0 {
                    *e = (day.clone(), t.name.clone());
                }
            }
        }
    }
    for s in stations.iter_mut() {
        let mut hist: Vec<ToolUse> = use_.iter().filter(|((st, _), _)| *st == s.station).map(|((_, name), u)| ToolUse { tool: name.clone(), programs: u.programs, first: u.first.clone(), last: u.last.clone() }).collect();
        hist.sort_by(|a, b| b.programs.cmp(&a.programs));
        s.history = hist;
        if let Some((day, name)) = latest.get(&s.station) {
            let u = &use_[&(s.station, name.clone())];
            let common = u.c.iter().max_by_key(|x| x.1).map(|x| x.0.clone()).unwrap_or_default();
            let mount = common.parse::<f64>().unwrap_or(0.0);
            s.mounted = Some(Mounted { tool: name.clone(), last_used: day.clone(), mount_angle: mount });
        }
    }
    Ok(n)
}

/// Build a machine from an unpacked LVD package (post name e.g. "PSTSTR05") and an archive.
pub fn build_machine(id: &str, name: &str, pkg: &Path, post: &str, archive: Option<&Path>) -> Result<Machine, String> {
    let find = |ext: &str| -> Result<std::path::PathBuf, String> {
        std::fs::read_dir(pkg)
            .map_err(|e| e.to_string())?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .find(|p| p.file_name().map(|f| f.to_string_lossy().eq_ignore_ascii_case(&format!("{}.{}", post, ext))).unwrap_or(false))
            .ok_or_else(|| format!("{}.{} not found in {}", post, ext, pkg.display()))
    };
    let ps = read_cfg(&find("PS1")?).map_err(|e| e.to_string())?;
    let ct = read_cfg(&find("CT1")?).map_err(|e| e.to_string())?;
    let (turret_name, mut stations) = read_turret(&pkg.join("torreta.xml"))?;
    let mut programs = 0;
    if let Some(a) = archive {
        programs = scan_archive(a, &mut stations).map_err(|e| e.to_string())?;
    }
    let ranges = ["A120", "A121", "A122"]
        .iter()
        .filter_map(|k| ct.get(*k).map(|x| nums(&x.0)))
        .filter(|n| n.len() >= 2 && n[0] > -99999.0)
        .map(|n| Range { min_x: r4(n[0] / MM), max_x: r4(n[1] / MM) })
        .collect::<Vec<_>>();
    let spec = MachineSpec {
        id: id.into(),
        name: name.into(),
        post: match post.to_uppercase().as_str() {
            "PSTSTR05" => "strippit_pststr05".into(),
            other => other.to_lowercase(),
        },
        control: format!("Lantek postprocessor {}", post.to_uppercase()),
        units: "inch".into(),
        source: format!("LVD machine package {} + {} archived programs", pkg.display(), programs),
        reset_position: [r4(v(&ps, "B016") / MM), r4(v(&ps, "B017") / MM)],
        work_zone: WorkZone {
            x: r4(v(&ct, "A150") / MM),
            y: r4(v(&ct, "A151") / MM),
            offset_left: r4(v(&ct, "A153") / MM),
            offset_right: r4(v(&ct, "A152") / MM),
            offset_top: r4(v(&ct, "A154") / MM),
            offset_bottom: r4(v(&ct, "A155") / MM),
        },
        proven_max_x: None,
        clamps: Clamps {
            count: v(&ct, "A103") as usize,
            default_positions: ["A104", "A105", "A106"].iter().map(|k| v(&ct, k)).filter(|x| *x < 99000.0).map(|x| r4(x / MM)).collect(),
            ranges,
            general_dead_zone: Zone { left: r4(v(&ct, "A100") / MM), right: r4(v(&ct, "A101") / MM), top: r4(v(&ct, "A102") / MM) },
            min_gap: 8.0,
        },
        press_capacity_kg: v(&ct, "C001"),
        nibble_tolerance_in: r4(v(&ct, "C000") / MM),
        time_model: TimeModel::default(),
        shear_ksi: default_shear(),
        stale_tool_days: 365,
        reposition: serde_json::json!({
            "hold_downs": [
                {"x": r4(v(&ct, "A504") / MM), "y": r4(v(&ct, "A505") / MM), "radius": r4(v(&ct, "A506") / MM)},
                {"x": r4(v(&ct, "A507") / MM), "y": r4(v(&ct, "A508") / MM), "radius": r4(v(&ct, "A509") / MM)}
            ],
            "y_retract": r4(v(&ct, "A521") / MM),
            "code": "G70 X_ Y_ / M03 / G91 / Y<retract> / X<shift> / G90 / M04 / F1"
        }),
        post_parameters: serde_json::to_value(ps.iter().map(|(k, (a, b))| (k.clone(), serde_json::json!({"value": a, "meaning": b}))).collect::<BTreeMap<_, _>>()).unwrap(),
    };
    Ok(Machine { spec, turret: Turret { turret: turret_name, stations, note: "mounted = tool named in the most recent archived program that used the station".into() }, dir: None })
}

/// Largest hit X in archived programs that do not reposition (the proven reach).
pub fn proven_max_x(dir: &Path) -> std::io::Result<f64> {
    let mut mx: f64 = 0.0;
    for e in std::fs::read_dir(dir)? {
        let p = e?.path();
        if p.extension().map(|x| x.to_string_lossy().eq_ignore_ascii_case("CNC")).unwrap_or(false) {
            let prog = cnc::load(&p)?;
            if prog.repositions == 0 {
                for h in &prog.hits {
                    mx = mx.max(h.x);
                }
            }
        }
    }
    Ok(r4(mx))
}
