//! Machine definition: turret stations, mounted tools, clamps, limits, time model.
//!
//! Stored per profile as `machines/<id>/machine.json` + `turret.json` (+ optional
//! `turret_override.json`, a map station -> tool name / null that pins today's turret load).

use crate::tooling::{parse_tool_name, ToolShape};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkZone {
    pub x: f64,
    pub y: f64,
    pub offset_left: f64,
    pub offset_right: f64,
    pub offset_top: f64,
    pub offset_bottom: f64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Zone {
    pub left: f64,
    pub right: f64,
    pub top: f64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Range {
    pub min_x: f64,
    pub max_x: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Clamps {
    pub count: usize,
    #[serde(default)]
    pub default_positions: Vec<f64>,
    pub ranges: Vec<Range>,
    pub general_dead_zone: Zone,
    /// Minimum spacing between clamp centres used by auto placement.
    #[serde(default = "default_clamp_gap")]
    pub min_gap: f64,
}
fn default_clamp_gap() -> f64 {
    8.0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeModel {
    pub constant: f64,
    pub per_hit: f64,
    pub per_tool_change: f64,
    pub per_inch_travel: f64,
    pub per_index_change: f64,
    pub per_nibble_hit: f64,
}

impl Default for TimeModel {
    /// Calibrated on 7,936 M-1225 programs against Lantek's ETIME (median error 2.5%).
    fn default() -> Self {
        TimeModel {
            constant: 5.3682,
            per_hit: 0.1572,
            per_tool_change: 1.5832,
            per_inch_travel: 0.0263,
            per_index_change: 0.2518,
            per_nibble_hit: -0.0451,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MachineSpec {
    pub id: String,
    pub name: String,
    /// Which postprocessor writes programs for this machine.
    pub post: String,
    #[serde(default)]
    pub control: String,
    #[serde(default = "default_units")]
    pub units: String,
    #[serde(default)]
    pub source: String,
    pub reset_position: [f64; 2],
    pub work_zone: WorkZone,
    /// Largest hit X used without a reposition in real programs.
    #[serde(default)]
    pub proven_max_x: Option<f64>,
    pub clamps: Clamps,
    pub press_capacity_kg: f64,
    pub nibble_tolerance_in: f64,
    #[serde(default)]
    pub time_model: TimeModel,
    /// Conservative ultimate shear strength (ksi) by material prefix.
    #[serde(default = "default_shear")]
    pub shear_ksi: BTreeMap<String, f64>,
    /// Days since last use after which a mounted tool is treated as unconfirmed.
    #[serde(default = "default_stale_days")]
    pub stale_tool_days: i64,
    #[serde(default)]
    pub reposition: serde_json::Value,
    #[serde(default)]
    pub post_parameters: serde_json::Value,
}
fn default_units() -> String {
    "inch".into()
}
fn default_stale_days() -> i64 {
    365
}
pub fn default_shear() -> BTreeMap<String, f64> {
    [("ST", 50.0), ("ALUMINUM", 25.0), ("SS", 75.0), ("COPPER", 30.0)]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolUse {
    pub tool: String,
    pub programs: u32,
    pub first: String,
    pub last: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mounted {
    pub tool: String,
    /// ISO date of the newest program that used it, or "override".
    #[serde(default)]
    pub last_used: String,
    /// Holder angle (the `C=` in the CNC tool comment).
    #[serde(default)]
    pub mount_angle: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Station {
    pub station: u32,
    pub size_class: String,
    pub max_tool_in: f64,
    pub autoindex: bool,
    #[serde(default)]
    pub key_angles: Vec<f64>,
    pub clamp_zone: Zone,
    #[serde(default)]
    pub mounted: Option<Mounted>,
    #[serde(default)]
    pub history: Vec<ToolUse>,
}

impl Station {
    pub fn tool_name(&self) -> Option<&str> {
        self.mounted.as_ref().map(|m| m.tool.as_str())
    }
    /// Footprint of the mounted tool, with the recorded mount angle applied.
    pub fn shape(&self) -> Option<ToolShape> {
        let m = self.mounted.as_ref()?;
        let s = parse_tool_name(&m.tool);
        Some(if self.autoindex { s } else { s.with_mount(m.mount_angle) })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Turret {
    pub turret: String,
    pub stations: Vec<Station>,
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Clone)]
pub struct Machine {
    pub spec: MachineSpec,
    pub turret: Turret,
    pub dir: Option<PathBuf>,
}

#[derive(Debug, thiserror::Error)]
pub enum MachineError {
    #[error("io {0}: {1}")]
    Io(PathBuf, std::io::Error),
    #[error("json {0}: {1}")]
    Json(PathBuf, serde_json::Error),
}

fn read_json<T: for<'de> Deserialize<'de>>(p: &Path) -> Result<T, MachineError> {
    let txt = std::fs::read_to_string(p).map_err(|e| MachineError::Io(p.to_path_buf(), e))?;
    serde_json::from_str(&txt).map_err(|e| MachineError::Json(p.to_path_buf(), e))
}

impl Machine {
    /// Load `<dir>/machine.json`, `<dir>/turret.json` and the optional override.
    pub fn load(dir: &Path) -> Result<Machine, MachineError> {
        let spec: MachineSpec = read_json(&dir.join("machine.json"))?;
        let mut turret: Turret = read_json(&dir.join("turret.json"))?;
        let ov = dir.join("turret_override.json");
        if ov.exists() {
            let map: BTreeMap<String, serde_json::Value> = read_json(&ov)?;
            apply_override(&mut turret, &map);
        }
        Ok(Machine { spec, turret, dir: Some(dir.to_path_buf()) })
    }

    pub fn save(&self, dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        std::fs::write(dir.join("machine.json"), serde_json::to_string_pretty(&self.spec).unwrap())?;
        std::fs::write(dir.join("turret.json"), serde_json::to_string_pretty(&self.turret).unwrap())?;
        Ok(())
    }

    pub fn id(&self) -> &str {
        &self.spec.id
    }

    pub fn station(&self, n: u32) -> Option<&Station> {
        self.turret.stations.iter().find(|s| s.station == n)
    }

    pub fn reset_position(&self) -> (f64, f64) {
        (self.spec.reset_position[0], self.spec.reset_position[1])
    }

    /// Hit-centre envelope without a reposition: ((xmin, xmax), (ymin, ymax)).
    pub fn reach(&self) -> ((f64, f64), (f64, f64)) {
        let wz = &self.spec.work_zone;
        let xmax = self.spec.proven_max_x.unwrap_or(wz.x - wz.offset_left);
        ((-wz.offset_left, xmax), (-wz.offset_bottom, wz.y - wz.offset_top))
    }

    pub fn clamp_zone(&self, st: u32) -> Zone {
        self.station(st).map(|s| s.clamp_zone).unwrap_or(self.spec.clamps.general_dead_zone)
    }

    /// (station, shape) for every station with a known mounted tool.
    pub fn tools(&self) -> Vec<(u32, ToolShape)> {
        self.turret.stations.iter().filter_map(|s| s.shape().map(|sh| (s.station, sh))).collect()
    }

    /// Was the station's tool used recently enough to trust without asking?
    pub fn is_current(&self, st: u32, today: &str) -> bool {
        let Some(m) = self.station(st).and_then(|s| s.mounted.as_ref()) else { return false };
        if m.last_used == "override" || m.last_used.is_empty() {
            return true;
        }
        match (chrono::NaiveDate::parse_from_str(&m.last_used, "%Y-%m-%d"), chrono::NaiveDate::parse_from_str(today, "%Y-%m-%d")) {
            (Ok(a), Ok(b)) => (b - a).num_days() <= self.spec.stale_tool_days,
            _ => true,
        }
    }

    /// Station holding a tool by exact name (spacing ignored).
    pub fn station_for_name(&self, name: &str) -> Option<u32> {
        let squash = |s: &str| s.chars().filter(|c| !c.is_whitespace()).collect::<String>();
        let want = squash(name);
        self.turret.stations.iter().find(|s| s.tool_name().map(squash).as_deref() == Some(want.as_str())).map(|s| s.station)
    }

    pub fn shear_ksi(&self, material: &str) -> f64 {
        let key = material.split_whitespace().next().unwrap_or("ST").to_uppercase();
        *self.spec.shear_ksi.get(&key).unwrap_or(&50.0)
    }

    /// Press capacity in US tons.
    pub fn capacity_tons(&self) -> f64 {
        self.spec.press_capacity_kg / 907.185
    }

    /// Cycle-time estimate in seconds (same scale as Lantek's ETIME).
    pub fn time_estimate(&self, hits: &[crate::cnc::Hit]) -> f64 {
        let m = &self.spec.time_model;
        let mut t = m.constant + m.per_hit * hits.len() as f64;
        for w in hits.windows(2) {
            let (a, b) = (&w[0], &w[1]);
            if a.station != b.station {
                t += m.per_tool_change;
            }
            t += m.per_inch_travel * (b.x - a.x).abs().max((b.y - a.y).abs());
            if a.angle != b.angle {
                t += m.per_index_change;
            }
        }
        t += m.per_nibble_hit * hits.iter().filter(|h| h.op == "G68" || h.op == "G69").count() as f64;
        (t * 100.0).round() / 100.0
    }
}

pub fn apply_override(turret: &mut Turret, map: &BTreeMap<String, serde_json::Value>) {
    for (k, v) in map {
        let Ok(n) = k.parse::<u32>() else { continue };
        if let Some(st) = turret.stations.iter_mut().find(|s| s.station == n) {
            match v {
                serde_json::Value::Null => st.mounted = None,
                serde_json::Value::String(name) => {
                    let shp = parse_tool_name(name);
                    st.mounted = Some(Mounted { tool: name.clone(), last_used: "override".into(), mount_angle: shp.mount });
                }
                serde_json::Value::Object(o) => {
                    let name = o.get("tool").and_then(|t| t.as_str()).unwrap_or_default().to_string();
                    let mount = o.get("mount_angle").and_then(|t| t.as_f64()).unwrap_or_else(|| parse_tool_name(&name).mount);
                    st.mounted = Some(Mounted { tool: name, last_used: "override".into(), mount_angle: mount });
                }
                _ => {}
            }
        }
    }
}
