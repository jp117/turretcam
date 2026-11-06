//! turretcam-core: the engine behind TurretCAM.
//!
//! Pipeline: [`part`] (what to make) -> [`project`] (parts placed on a sheet)
//! -> [`cam`] (features to punch hits) -> [`writer`] (machine G-code) -> [`cnc`] (re-parse and
//! simulate) -> [`check`] (machine limits). [`import`] turns DXF/DWG drawings into parts;
//! [`machine`] and [`profile`] hold per-user machine definitions; [`lvd`] rebuilds a machine
//! definition from an LVD/Lantek machine package plus an archive of real programs.
//!
//! Units are inches throughout; sheet origin is the lower-left corner (clamp edge = Y 0).

pub mod cam;
pub mod check;
pub mod cnc;
pub mod fmt;
pub mod geom;
pub mod import;
pub mod lvd;
pub mod machine;
pub mod nest;
pub mod part;
pub mod profile;
pub mod project;
pub mod render;
pub mod tooling;
pub mod writer;

pub use cam::{BuildResult, CamError, Job};
pub use check::{check, Issue, Severity};
pub use machine::Machine;
pub use part::{Feature, Part};
pub use profile::{Profile, ProfileStore};
pub use project::Project;
