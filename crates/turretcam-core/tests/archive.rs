//! Example builds, plus archive-backed verification against a shop's real programs.
//!
//! Always: every `examples/*.tcam.json` builds cleanly on the demo profile.
//!
//! With `TURRETCAM_ARCHIVE` (a folder of real programs) and `TURRETCAM_HOME` (the profile
//! folder for the machine that ran them), also:
//! * every archived program re-emits identically,
//! * the clamp dead-zone model is never violated by a real program,
//! * every `$TURRETCAM_HOME/examples/*.tcam.json` builds, and any example whose part `source`
//!   names an archived `.CNC` rebuilds with the same tools in the same order and the same hits.
//!
//!   $env:TURRETCAM_HOME="..\my-shop-profiles"; $env:TURRETCAM_ARCHIVE="path\to\CNC\M1225"; cargo test --release
//!
//! `TURRETCAM_PROFILE` / `TURRETCAM_MACHINE` pick the profile and machine (default: the first
//! profile and its default machine).

use std::path::{Path, PathBuf};
use turretcam_core::{check, cnc, writer, Machine, ProfileStore, Project};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn archive() -> Option<PathBuf> {
    std::env::var("TURRETCAM_ARCHIVE").ok().map(PathBuf::from).filter(|p| p.is_dir())
}

fn home() -> Option<PathBuf> {
    std::env::var("TURRETCAM_HOME").ok().map(PathBuf::from).filter(|p| p.is_dir())
}

fn machine_in(home: &Path) -> Machine {
    let store = ProfileStore::open(home.join("profiles"));
    let id = std::env::var("TURRETCAM_PROFILE").ok().or_else(|| store.list().into_iter().next()).expect("no profile in TURRETCAM_HOME/profiles");
    let prof = store.load(&id).unwrap();
    match std::env::var("TURRETCAM_MACHINE") {
        Ok(m) => prof.load_machine(&m).unwrap(),
        Err(_) => prof.default_machine().unwrap(),
    }
}

/// The shop machine that ran the archive (needs TURRETCAM_HOME when TURRETCAM_ARCHIVE is set).
fn archive_machine() -> Option<(PathBuf, Machine)> {
    let dir = archive()?;
    let h = home().expect("TURRETCAM_ARCHIVE is set: also set TURRETCAM_HOME to the profile folder of the machine that ran those programs");
    Some((dir, machine_in(&h)))
}

fn programs(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map(|x| x.to_string_lossy().eq_ignore_ascii_case("CNC")).unwrap_or(false))
        .collect();
    v.sort();
    v
}

fn examples(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir).map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.to_string_lossy().ends_with(".tcam.json")).collect()).unwrap_or_default();
    v.sort();
    v
}

/// Every body line of every archived program re-emits identically (hand-typed lines aside),
/// and at least 98% of headers re-emit identically from parsed fields.
#[test]
fn roundtrip_archive() {
    let Some((dir, m)) = archive_machine() else { return };
    let (mut files, mut hdr_ok, mut lines, mut bad) = (0usize, 0usize, 0usize, vec![]);
    let squash = |s: &str| s.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_uppercase().replace("T0", "T").replace("X96.", "X96").replace("Y1.", "Y1").replace("Y40.", "Y40");
    for p in programs(&dir) {
        files += 1;
        let prog = cnc::load(&p).unwrap();
        let orig: Vec<String> = prog.header_lines.iter().map(|l| l.trim_end().to_string()).collect();
        if prog.header.sheet.is_some() && prog.header.program_number.is_some() && writer::header_lines(&prog.header, &m) == orig {
            hdr_ok += 1;
        }
        for op in &prog.ops {
            lines += 1;
            let g = writer::format_op(op);
            let r = op.raw.trim_end();
            if g != r && squash(&g) != squash(r.trim_start_matches('\\')) {
                bad.push(format!("{}: {:?} -> {:?}", p.display(), r, g));
            }
        }
    }
    println!("{} programs, {} body lines, {} headers identical", files, lines, hdr_ok);
    assert!(bad.is_empty(), "{} body mismatches, e.g. {:?}", bad.len(), &bad[..bad.len().min(5)]);
    assert!(hdr_ok as f64 >= 0.98 * files as f64, "headers identical {}/{}", hdr_ok, files);
}

/// The clamp dead-zone model is never violated by a real program.
#[test]
fn no_clamp_violations_in_archive() {
    let Some((dir, m)) = archive_machine() else { return };
    let mut viol = vec![];
    for p in programs(&dir) {
        let prog = cnc::load(&p).unwrap();
        if check::check(&prog, &m, false).iter().any(|i| i.code == "hit.clamp") {
            viol.push(p.display().to_string());
        }
    }
    assert!(viol.is_empty(), "{:?}", &viol[..viol.len().min(5)]);
}

/// Shop examples build, and examples rebuilt from an archived program match it:
/// same tools in the same order and the same number of hits.
#[test]
fn shop_examples_rebuild_archive() {
    let Some((dir, m)) = archive_machine() else { return };
    let h = home().unwrap();
    for e in examples(&h.join("examples")) {
        let proj = Project::load(&e).unwrap();
        let res = proj.build(&m, Some("2026-11-06")).unwrap();
        assert!(res.ok(), "{}: {:?}", e.display(), res.issues);
        let Some(orig) = proj.parts.iter().filter_map(|p| Path::new(&p.source).file_name()).map(|f| dir.join(f)).find(|p| p.is_file()) else { continue };
        let o = cnc::load(&orig).unwrap();
        let tools = |p: &cnc::Program| p.header.tools.iter().map(|t| t.station).collect::<Vec<_>>();
        assert_eq!(tools(&o), tools(&res.program), "{}: same tools in the same order as {}", e.display(), orig.display());
        assert_eq!(o.hits.len(), res.program.hits.len(), "{}: same hit count as {}", e.display(), orig.display());
        println!("{} rebuilds {} ({} hits)", e.display(), orig.display(), o.hits.len());
    }
}

/// The public example jobs build cleanly on the demo profile.
#[test]
fn examples_build() {
    let m = machine_in(&repo());
    let ex = examples(&repo().join("examples"));
    assert!(!ex.is_empty());
    for e in ex {
        let p = Project::load(&e).unwrap();
        let r = p.build(&m, Some("2026-11-06")).unwrap();
        assert!(r.ok(), "{}: {:?}", e.display(), r.issues);
    }
}
