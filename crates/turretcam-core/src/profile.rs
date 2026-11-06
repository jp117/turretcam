//! User / shop profiles. A profile owns its machine definitions and defaults.
//!
//! ```text
//! <home>/profiles/<profile-id>/profile.json
//! <home>/profiles/<profile-id>/machines/<machine-id>/machine.json
//! <home>/profiles/<profile-id>/machines/<machine-id>/turret.json
//! <home>/profiles/<profile-id>/machines/<machine-id>/turret_override.json   (optional)
//! ```
//! `<home>` is `$TURRETCAM_HOME`, else the first of `./profiles/..`, `<exe dir>/..` that
//! contains a `profiles` folder.

use crate::machine::{Machine, MachineError};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Defaults {
    #[serde(default = "d_material")]
    pub material: String,
    #[serde(default = "d_thk")]
    pub thickness: f64,
    #[serde(default = "d_sheet")]
    pub sheet: [f64; 2],
    #[serde(default = "d_tab")]
    pub tab: f64,
    #[serde(default = "d_tab_every")]
    pub tab_every: f64,
    #[serde(default = "d_overlap")]
    pub min_overlap: f64,
    #[serde(default = "d_gap")]
    pub nest_gap: f64,
}
fn d_material() -> String {
    "ST 42".into()
}
fn d_thk() -> f64 {
    0.0315
}
fn d_sheet() -> [f64; 2] {
    [96.0, 48.0]
}
fn d_tab() -> f64 {
    0.025
}
fn d_tab_every() -> f64 {
    24.0
}
fn d_overlap() -> f64 {
    0.06
}
fn d_gap() -> f64 {
    0.5
}
impl Default for Defaults {
    fn default() -> Self {
        serde_json::from_str("{}").unwrap()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub user: String,
    pub machines: Vec<String>,
    pub default_machine: String,
    /// Next free program number (O-number); the app bumps it after each export.
    #[serde(default = "d_next")]
    pub next_program_number: u32,
    /// Where exported .CNC files go (e.g. the machine's share folder).
    #[serde(default)]
    pub output_dir: Option<String>,
    /// Full path to ODAFileConverter.exe for DWG import.
    #[serde(default)]
    pub oda_converter: Option<String>,
    #[serde(default)]
    pub defaults: Defaults,
    #[serde(skip)]
    pub dir: PathBuf,
}
fn d_next() -> u32 {
    9001
}

#[derive(Debug, thiserror::Error)]
pub enum ProfileError {
    #[error("no profiles folder found (set TURRETCAM_HOME)")]
    NoHome,
    #[error("profile {0} not found")]
    NotFound(String),
    #[error("io {0}: {1}")]
    Io(PathBuf, std::io::Error),
    #[error("json {0}: {1}")]
    Json(PathBuf, serde_json::Error),
    #[error(transparent)]
    Machine(#[from] MachineError),
}

#[derive(Debug, Clone)]
pub struct ProfileStore {
    pub root: PathBuf,
}

impl ProfileStore {
    pub fn open(root: impl Into<PathBuf>) -> Self {
        ProfileStore { root: root.into() }
    }

    /// Find the profiles folder (see module docs).
    pub fn discover() -> Result<Self, ProfileError> {
        if let Ok(h) = std::env::var("TURRETCAM_HOME") {
            return Ok(Self::open(PathBuf::from(h).join("profiles")));
        }
        let mut cands = vec![];
        if let Ok(cwd) = std::env::current_dir() {
            for a in cwd.ancestors() {
                cands.push(a.join("profiles"));
            }
        }
        if let Ok(exe) = std::env::current_exe() {
            for a in exe.ancestors().skip(1) {
                cands.push(a.join("profiles"));
            }
        }
        cands.into_iter().find(|p| p.is_dir()).map(Self::open).ok_or(ProfileError::NoHome)
    }

    pub fn list(&self) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(&self.root)
            .map(|rd| {
                rd.filter_map(|e| e.ok())
                    .filter(|e| e.path().join("profile.json").exists())
                    .map(|e| e.file_name().to_string_lossy().to_string())
                    .collect()
            })
            .unwrap_or_default();
        v.sort();
        v
    }

    pub fn load(&self, id: &str) -> Result<Profile, ProfileError> {
        let dir = self.root.join(id);
        let p = dir.join("profile.json");
        if !p.exists() {
            return Err(ProfileError::NotFound(id.into()));
        }
        let txt = std::fs::read_to_string(&p).map_err(|e| ProfileError::Io(p.clone(), e))?;
        let mut prof: Profile = serde_json::from_str(&txt).map_err(|e| ProfileError::Json(p.clone(), e))?;
        prof.dir = dir;
        Ok(prof)
    }

    pub fn save(&self, prof: &Profile) -> Result<(), ProfileError> {
        let dir = self.root.join(&prof.id);
        std::fs::create_dir_all(&dir).map_err(|e| ProfileError::Io(dir.clone(), e))?;
        let p = dir.join("profile.json");
        std::fs::write(&p, serde_json::to_string_pretty(prof).unwrap()).map_err(|e| ProfileError::Io(p, e))
    }
}

impl Profile {
    pub fn machine_dir(&self, id: &str) -> PathBuf {
        self.dir.join("machines").join(id)
    }
    pub fn load_machine(&self, id: &str) -> Result<Machine, ProfileError> {
        Ok(Machine::load(&self.machine_dir(id))?)
    }
    pub fn default_machine(&self) -> Result<Machine, ProfileError> {
        self.load_machine(&self.default_machine)
    }
    pub fn output_dir(&self) -> Option<&Path> {
        self.output_dir.as_deref().map(Path::new)
    }
}
