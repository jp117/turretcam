//! `tcam`: command-line TurretCAM for scripts and agents.
//!
//! Profiles are found via $TURRETCAM_HOME/profiles or a `profiles/` folder above the current
//! directory. `--profile` defaults to $TURRETCAM_PROFILE, else the only/first profile.

use anyhow::{anyhow, bail, Context, Result};
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};
use turretcam_core::{check, cnc, import, lvd, nest, render, writer, Machine, Profile, ProfileStore, Project};

#[derive(Parser)]
#[command(name = "tcam", version, about = "TurretCAM command line: build, check and inspect turret punch programs")]
struct Cli {
    /// Profile id (folder under profiles/)
    #[arg(long, global = true, env = "TURRETCAM_PROFILE")]
    profile: Option<String>,
    /// Machine id (defaults to the project's machine or the profile default)
    #[arg(long, global = true)]
    machine: Option<String>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// List profiles and their machines
    Profiles,
    /// Show the turret of a machine
    Turret {
        #[arg(long)]
        json: bool,
    },
    /// Generate a CNC program from a project file (*.tcam.json)
    Build {
        project: PathBuf,
        out: PathBuf,
        #[arg(long)]
        svg: Option<PathBuf>,
        /// Write even if checks report errors
        #[arg(long)]
        force: bool,
        /// Release date (YYYY-MM-DD); default today
        #[arg(long)]
        date: Option<String>,
    },
    /// Check an existing CNC program against the machine
    Check {
        file: PathBuf,
        /// Turret mismatches are warnings, not errors (for old programs)
        #[arg(long)]
        loose: bool,
    },
    /// Print a program's header, tools and hit count as JSON
    Info { file: PathBuf },
    /// Render a CNC program to SVG
    Render { file: PathBuf, out: PathBuf },
    /// Import a DXF/DWG as a part and print (or write) its JSON
    Import {
        file: PathBuf,
        #[arg(long)]
        out: Option<PathBuf>,
        /// Add the part to this project file instead
        #[arg(long)]
        into: Option<PathBuf>,
    },
    /// Auto-nest parts in a project: --part P1=4 --part P2=2
    Nest {
        project: PathBuf,
        #[arg(long = "part", value_parser = parse_qty)]
        parts: Vec<(String, u32)>,
        #[arg(long, default_value_t = 0.5)]
        gap: f64,
    },
    /// Build a machine definition from an LVD machine package + program archive
    MachineFromLvd {
        #[arg(long)]
        id: String,
        #[arg(long)]
        name: String,
        /// Unzipped LVD package folder (torreta.xml, <post>.PS1, <post>.CT1)
        #[arg(long)]
        package: PathBuf,
        #[arg(long, default_value = "PSTSTR05")]
        post: String,
        /// Folder of .CNC programs run on the machine
        #[arg(long)]
        archive: Option<PathBuf>,
        /// Output folder (machine.json + turret.json)
        #[arg(long)]
        out: PathBuf,
    },
    /// Re-emit every program in a folder and report format fidelity
    Roundtrip { dir: PathBuf },
}

fn parse_qty(s: &str) -> Result<(String, u32), String> {
    let (a, b) = s.split_once('=').ok_or("use PART=QTY")?;
    Ok((a.to_string(), b.parse().map_err(|_| "qty must be a number")?))
}

struct Ctx {
    profile: Profile,
}

fn ctx(cli: &Cli) -> Result<Ctx> {
    let store = ProfileStore::discover()?;
    let id = match &cli.profile {
        Some(p) => p.clone(),
        None => store.list().into_iter().next().ok_or_else(|| anyhow!("no profiles in {}", store.root.display()))?,
    };
    Ok(Ctx { profile: store.load(&id)? })
}

fn machine(c: &Ctx, cli: &Cli, project_machine: Option<&str>) -> Result<Machine> {
    let id = cli.machine.as_deref().or(project_machine).unwrap_or(&c.profile.default_machine).to_string();
    c.profile.load_machine(&id).with_context(|| format!("loading machine {} from profile {}", id, c.profile.id))
}

fn print_issues(issues: &[check::Issue]) {
    for i in issues {
        println!("{}", i);
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match &cli.cmd {
        Cmd::Profiles => {
            let store = ProfileStore::discover()?;
            println!("profiles in {}", store.root.display());
            for id in store.list() {
                let p = store.load(&id)?;
                println!("  {:<16} {}  machines: {}  (default {})  next O{:04}", p.id, p.name, p.machines.join(", "), p.default_machine, p.next_program_number);
            }
        }
        Cmd::Turret { json } => {
            let c = ctx(&cli)?;
            let m = machine(&c, &cli, None)?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&m.turret)?);
            } else {
                let today = chrono_today();
                println!("{} - {} ({} stations)", m.spec.name, m.turret.turret, m.turret.stations.len());
                for s in &m.turret.stations {
                    let mt = s.mounted.as_ref();
                    let stale = mt.is_some() && !m.is_current(s.station, &today);
                    println!(
                        "T{:<3} {} max {:<6} {:4} {:<26} mount {:<4} last {}{}",
                        s.station,
                        s.size_class,
                        s.max_tool_in,
                        if s.autoindex { "AIDX" } else { "" },
                        mt.map(|m| m.tool.as_str()).unwrap_or("-"),
                        mt.map(|m| m.mount_angle.to_string()).unwrap_or_default(),
                        mt.map(|m| m.last_used.as_str()).unwrap_or(""),
                        if stale { "  STALE" } else { "" }
                    );
                }
            }
        }
        Cmd::Build { project, out, svg, force, date } => {
            let c = ctx(&cli)?;
            let p = Project::load(project).map_err(|e| anyhow!(e))?;
            let m = machine(&c, &cli, Some(&p.machine))?;
            let res = p.build(&m, date.as_deref()).map_err(|e| anyhow!(e.0))?;
            print_issues(&res.issues);
            for l in &res.log {
                println!("note  {}", l);
            }
            println!("ETIME {}s  clamps {:?}  hits {}  ok={}", res.etime, res.clamps, res.program.hits.len(), res.ok());
            if !res.ok() && !force {
                bail!("not written: fix the ERRORs (or pass --force)");
            }
            std::fs::write(out, &res.text)?;
            println!("wrote {}", out.display());
            if let Some(s) = svg {
                std::fs::write(s, render::program_svg(&res.program, Some(&m), 12.0))?;
            }
        }
        Cmd::Check { file, loose } => {
            let c = ctx(&cli)?;
            let m = machine(&c, &cli, None)?;
            let p = cnc::load(file)?;
            let issues = check::check(&p, &m, !loose);
            print_issues(&issues);
            println!("{} hits, ok={}", p.hits.len(), check::ok(&issues));
            if !check::ok(&issues) {
                std::process::exit(1);
            }
        }
        Cmd::Info { file } => {
            let p = cnc::load(file)?;
            let v = serde_json::json!({"header": p.header, "hits": p.hits.len(), "repositions": p.repositions, "warnings": p.warnings});
            println!("{}", serde_json::to_string_pretty(&v)?);
        }
        Cmd::Render { file, out } => {
            let p = cnc::load(file)?;
            let m = ctx(&cli).ok().and_then(|c| machine(&c, &cli, None).ok());
            std::fs::write(out, render::program_svg(&p, m.as_ref(), 12.0))?;
        }
        Cmd::Import { file, out, into } => {
            let oda = ctx(&cli).ok().and_then(|c| c.profile.oda_converter.clone());
            if let Some(proj_path) = into {
                let mut proj = Project::load(proj_path).map_err(|e| anyhow!(e))?;
                let id = proj.new_part_id();
                let part = import::import_file(file, &id, oda.as_deref())?;
                for u in part.unsupported() {
                    println!("WARN  {}", u);
                }
                println!("added part {} ({}) with {} features", id, part.name, part.features.len());
                proj.parts.push(part);
                proj.save(proj_path)?;
            } else {
                let part = import::import_file(file, "P1", oda.as_deref())?;
                let txt = serde_json::to_string_pretty(&part)?;
                match out {
                    Some(o) => std::fs::write(o, txt)?,
                    None => println!("{}", txt),
                }
            }
        }
        Cmd::Nest { project, parts, gap } => {
            let mut p = Project::load(project).map_err(|e| anyhow!(e))?;
            let wanted = if parts.is_empty() { p.parts.iter().map(|x| (x.id.clone(), 1)).collect() } else { parts.clone() };
            let missed = nest::nest(&mut p, &wanted, &nest::NestOptions { gap: *gap, ..Default::default() });
            p.save(project)?;
            println!("placed {} part(s), {} did not fit", p.placements.len(), missed);
        }
        Cmd::MachineFromLvd { id, name, package, post, archive, out } => {
            let mut m = lvd::build_machine(id, name, package, post, archive.as_deref()).map_err(|e| anyhow!(e))?;
            if let Some(a) = archive {
                m.spec.proven_max_x = Some(lvd::proven_max_x(a)?);
            }
            m.save(out)?;
            println!("wrote {} ({} stations, {} with a mounted tool)", out.display(), m.turret.stations.len(), m.turret.stations.iter().filter(|s| s.mounted.is_some()).count());
        }
        Cmd::Roundtrip { dir } => roundtrip(&cli, dir)?,
    }
    Ok(())
}

fn chrono_today() -> String {
    turretcam_core::cam::JobOptions::default().date
}

/// Re-emit every archived program from its parsed form and count identical lines.
fn roundtrip(cli: &Cli, dir: &Path) -> Result<()> {
    let c = ctx(cli)?;
    let m = machine(&c, cli, None)?;
    let (mut files, mut hdr_ok, mut body_lines, mut body_bad, mut hand) = (0, 0, 0usize, 0usize, 0usize);
    let mut examples = vec![];
    for e in std::fs::read_dir(dir)? {
        let p = e?.path();
        if !p.extension().map(|x| x.to_string_lossy().eq_ignore_ascii_case("CNC")).unwrap_or(false) {
            continue;
        }
        files += 1;
        let prog = cnc::load(&p)?;
        let orig: Vec<String> = prog.header_lines.iter().map(|l| l.trim_end().to_string()).collect();
        if prog.header.sheet.is_some() && prog.header.program_number.is_some() && writer::header_lines(&prog.header, &m) == orig {
            hdr_ok += 1;
        }
        for op in &prog.ops {
            body_lines += 1;
            let g = writer::format_op(op);
            let r = op.raw.trim_end();
            if g != r {
                let squash = |s: &str| s.chars().filter(|c| !c.is_whitespace()).collect::<String>().replace("T0", "T").replace("X96.", "X96").replace("Y1.", "Y1");
                if squash(&g) == squash(r) {
                    hand += 1;
                } else {
                    body_bad += 1;
                    if examples.len() < 10 {
                        examples.push(format!("{}: {:?} -> {:?}", p.file_name().unwrap().to_string_lossy(), r, g));
                    }
                }
            }
        }
    }
    println!("programs {}  headers identical {} ({:.2}%)", files, hdr_ok, 100.0 * hdr_ok as f64 / files.max(1) as f64);
    println!("body lines {}  identical {:.3}%  hand-typed variants {}  real mismatches {}", body_lines, 100.0 * (body_lines - body_bad - hand) as f64 / body_lines.max(1) as f64, hand, body_bad);
    for e in examples {
        println!("  {}", e);
    }
    Ok(())
}
