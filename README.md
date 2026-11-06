# TurretCAM

Turret punch CAM for the Strippit M-1225, written in Rust as a replacement for Cadman-P (Lantek Expert v31). Every part of it is plain, editable source code.

- **Draw parts or import DXF/DWG**, nest them on a sheet, generate the punch program, simulate it and check it, then save the `.CNC`.
- **Profiles store the machines.** Each profile holds its machine definitions: turret, clamps, limits, defaults and program numbering.
- **Programs come out in the exact format the M-1225 already runs.** The writer is verified against an archive of 7,952 programs that ran on a real M-1225.
- **Agents and scripts use the `tcam` CLI and the JSON job files.** See `AGENTS.md`.

## Run it

```powershell
cargo build --release
target\release\turretcam.exe                                          # the desktop app
target\release\turretcam.exe examples\meter_cover_2up.tcam.json       # open a job
target\release\tcam.exe build examples\meter_cover_2up.tcam.json 9001.CNC --svg 9001.svg
target\release\tcam.exe check path\to\program.CNC --loose
target\release\tcam.exe turret
```

DXF and DWG (AutoCAD R13 through 2018) import is built in. DWG is read in-process by [acadrust](https://github.com/hakanaktt/acadrust), the same approach as [CADCraft](https://github.com/storytold/cadcraft), so it works the same on Windows, macOS and Linux with nothing to install. ODA File Converter is only used as a fallback if it's installed and acadrust can't read a file.

Profiles are found in `$TURRETCAM_HOME\profiles`, or in a `profiles\` folder above the current directory or the executable.

### The demo profile, and your own

`profiles/demo` has a Strippit M-1225 Thick 47 with the real turret geometry (station sizes, key angles, auto-index stations, clamp dead zones) but a **made-up tool load**. It's for trying the app and for the tests. Don't cut parts with it.

For real work, keep your shop's profile in its own (private) folder or repo and point `TURRETCAM_HOME` at it. Generate the machine from your LVD package and your own program archive (below), then put today's real turret load in `machines/<id>/turret_override.json`, or pin tools in the Machine view.

## The app

Dark design with bundled fonts (Inter, JetBrains Mono and Phosphor icons), so it looks the same on Windows, macOS and Linux. Work runs left to right through three steps, with one primary button that always shows the next thing to do: **Generate program**, then **Export CNC**, or the error count if checks fail.

| Step | What you do there |
|---|---|
| **1 Parts** | Parts library with thumbnails; draw holes, slots, openings, louver grids, bolt circles and tool hits from the palette (V H S O G B T). Snapping, drag to move, exact sizes in the inspector. Every feature shows the turret tool it will use (T34 RD-1.184 …) or "no tool" in red. New-part dialog with common switchboard sizes and corner holes; DXF/DWG import. |
| **2 Sheet** | Sheet size presets and material, automatic or manual clamps, auto-nest with quantities, drag/rotate placements, clamp strip, reach limit and % of sheet used. |
| **3 Program** | Hits, tools, cycle time and sheet use at a glance; machine checks; tools in order of use (click to jump); simulation with a timeline scrubber and play/pause (Space); G-code viewer that highlights and follows the current hit; Export / Copy. |
| **Machine** (gear icon) | The turret as a ring of all 47 stations, coloured by tool, with stale and empty stations marked. Click a station for its tool, history and clamp clearance, and pin today's real tool. Machine limits and profile settings. |

Undo/redo (Ctrl+Z / Ctrl+Y), Ctrl+S save, Ctrl+O open, Ctrl+G generate, Ctrl+E export, F fit.

## Building a machine definition

The machine definition is generated, not typed in:

| Input | Used for |
|---|---|
| The LVD machine package (`... - V31 - SETUP.exe`, a WinZip self-extractor; unzip it) | Turret stations, clamp dead zones, reset position, work zone, press capacity |
| An archive of programs that ran on the machine | Which tool sits in each station, mount angles, the longest X actually used |

```powershell
tcam machine-from-lvd --id m1225 --name "LVD Strippit M-1225 Thk47 3-WC" --package path\to\lvd_package --post PSTSTR05 --archive path\to\CNC\M1225 --out $env:TURRETCAM_HOME\profiles\<profile>\machines\m1225
```

## Verification

```powershell
cargo test --release                       # unit tests + the demo examples
$env:TURRETCAM_HOME="..\my-shop-profiles"; $env:TURRETCAM_ARCHIVE="path\to\CNC\M1225"; cargo test --release
```

Given an archive and the matching profile, the tests also check the format against every real program and rebuild the shop's examples against their archived `.CNC`. Results on the 7,952-program M-1225 archive (2015 to 2026) this was built from:

| Test | Result |
|---|---|
| Re-emit every archived program from parsed form | All 1,391,214 body lines identical (hand-typed lines aside); 98.9% of headers identical |
| Clamp dead-zone model against every archived program | 0 violations |
| Rebuild a dead front from geometry only, compared with its archived `.CNC` | Same tools in the same order, same 76 hits, 99.84% identical removed area |
| Example jobs | Build with no errors |

**Not yet run on the machine.** Dry-run or graphics-check the first programs on the control before cutting.

## Layout

```
crates/turretcam-core   engine: parse/simulate (cnc), writer, cam, check, part, project, nest, import (DXF/DWG), machine, profile, lvd, render
crates/tcam-cli         `tcam` command line
crates/turretcam-app    desktop app (egui): theme, icons, widgets, Parts / Sheet / Program / Machine views
profiles/demo           demo profile: machines/m1225 (machine.json, turret.json with a made-up tool load)
examples/               job files (*.tcam.json)
docs/                   Strippit M-1225 G-code reference
scripts/                screenshot.ps1
```

## Licence

MIT or Apache-2.0, at your option (`LICENSE-MIT`, `LICENSE-APACHE`).

## Third-party

acadrust (DWG reading) is MPL-2.0, used unmodified from crates.io. Other dependencies are MIT or Apache-2.0.

Bundled fonts in `crates/turretcam-app/fonts/`: Inter and JetBrains Mono (SIL Open Font License 1.1) and Phosphor Icons (MIT). Their licences are next to them.
