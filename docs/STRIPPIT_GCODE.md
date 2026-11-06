# Strippit M-1225 G-code reference

Machine: **LVD Strippit M-1225, Thick 47 turret, 3 work chutes**, Fanuc-based Strippit control.
The CAM system was Cadman-P (Lantek Expert v31), using postprocessor **PSTSTR05**.

## Sources

Everything here was worked out from an LVD Strippit M-1225 machine package (`torreta.xml`, the
`PSTSTR05` post parameters, machine and tech settings) and from an archive of 7,952 programs that
ran on one M-1225 between 2015 and 2026. The post binaries build their codes at runtime, so the
archive was the format reference. "Uses in archive" below counts that archive.

## Confidence levels

- **Confirmed:** the meaning was checked against geometry. Either rendered parts come out coherent (holes symmetric, outlines closed), or the generator rebuilt an archived part with matching hits and >99% matching removed material.
- **Config:** the meaning comes from an LVD/Lantek parameter description; nothing in the archive contradicts it.
- **Observed:** the code appears in the archive with a consistent pattern, but its exact function isn't documented anywhere we have.

## Program layout

```
%
                                  <- blank line
O9001                             program number, 4 digits (1..9999)
(CNC NAME: 9001.CNC)
(job text)
(RELEASE DATE: 2026-11-06)        date the program was generated
(SHEET : 96 X 48 X 0.031 ST 42)(QTY, 1)   L x W x T (truncated to 3 dp), material, qty
(CLAMP : 12,48,80)                clamp positions, inches from sheet left edge
(ETIME = 65.63)                   Lantek cycle-time estimate, seconds
(T47    RD-0.25-M C=0; F1)        one line per tool, in order of first use
(T18    RE-3X0.125-M AIDX; F1)    AIDX = auto-index station
G28                               reference return
G92 X97.0079 Y49.6063             set position = machine load point (2464, 1260 mm)
X97.008 Y49.606 M75               move to load position, M75 (load/sheet sequence)
F1                                feed/speed mode 1
G06 Q0.0315 X96. Y48.             Strippit macro: exact thickness and sheet size
M35                               clamp scanning
G52 G90 X0. Y0.                   local coordinates: origin = sheet lower-left, absolute
... body ...
G70 X96 Y1                        shop ending, typed by hand: park the sheet clear
M31                               open clamps
M00                               program stop
M99                               end of program
%
```

Line endings are CRLF. In the body, numbers have at most 4 decimals with trailing zeros removed. Whole numbers keep a trailing dot (`X96.`, `C0.`, `J270.`). Halves round up (`6.53125` becomes `6.5313`). Counts (`K`, and `P` in grids) are bare integers.

The ending `G70 X96 Y1 / M31 / M00` doesn't come from the post: its numbers have no trailing dots. It appears in 7,513 programs, so the operators added it by hand. TurretCAM's writer adds it by default.

## Body codes

Coordinates are inches from the sheet's lower-left corner. The "base" is the last position, or the point set by the most recent G72.

| Code | Meaning | Uses in archive | Confidence |
|---|---|---|---|
| `X_ Y_ [C_]` | Move and punch with the current tool. `C` rotates an auto-index tool (degrees). | 1,137,885 | Confirmed |
| `G31 X_ Y_ T_ [C_]` | Load tool T, move, punch. The first hit of each tool always uses G31. | 56,731 | Confirmed |
| `T_` | Load tool only, no hit. Used before a G72 pattern. | 216 | Confirmed |
| `G70 X_ Y_` | Move without punching | 18,787 | Confirmed |
| `G72 X_ Y_` | Set the pattern base point, no hit | 3,410 | Confirmed |
| `G76 I_ J_ K_` | Line at angle: K more hits from the base, spacing I, angle J. The base itself isn't hit by G76; it was already punched by the line before. | 120,441 | Confirmed |
| `G77 I_ J_ P_ K_` | Bolt circle around the base: radius I, start angle J, step P degrees, K hits. The centre isn't hit. | 375 | Confirmed |
| `G78 I_ J_ P_ K_` | Grid from the base, X first: spacing I (X) and J (Y), P extra columns, K extra rows | 6,316 | Confirmed (staggered vent patterns only line up symmetrically with extra-count semantics) |
| `G79 I_ J_ P_ K_` | Same grid, Y first | 4,821 | Confirmed |
| `G68 I_ J_ K_ P_ Q_` | Circle nibble around the base: tool-centre radius I, start J, sweep K degrees, tool compensation P (always 0 here), maximum pitch Q. Both ends are hit; a 360° sweep doesn't repeat the start. | 2,209 | Confirmed (generated meter hole has the same 75 hits and the same area as Lantek's) |
| `G69 I_ J_ P_ Q_` | Line nibble from the base: length I, angle J, compensation P, maximum pitch Q. Both ends are hit. | 826 | Confirmed |
| `M03 / G91 / Y.05 / X-d / G90 / M04 / F1` | Clamp reposition. Hold-downs down and clamps open (M03), the carriage moves incrementally, clamps close (M04). Afterwards sheet X = programmed X + d. | 15 programs | Config (PSTSTR02 B063) plus Observed; the X shift was confirmed by hits landing on the sheet |
| `G65 P9xxx ...` | Macro call (9040/9041 are the sheet loader/unloader per PS1 B023/B024) | 3 | Config |
| `M07 / M08` | Punch off / on | 10 | Config (PSTSTR02 B067) |
| `M81 I_ K_` | Unknown. Hand-inserted after M35 in 45 programs (`M81 I.300 K.300`). | 45 | Observed |

### Tool orientation

- At `C0`, the first dimension of an `RE` or `OB` tool lies along X. Checked on about 99k line-nibbles: the nibble direction always matches the tool's long axis.
- Fixed stations have a mount angle. It's the `C=` in the tool comment (`C=90` means the long axis lies along Y). Names often carry `_90` or `-90-` as well, but not always (T16, T28), so the comment is authoritative.
- Only stations 1, 18 and 31 auto-index (D size, 3.5"). A `C` word on any other station is an error.

### Clamp dead zones (per station class, inches)

No hit may land within `left`/`right` of a clamp centre in X while also below `top` in Y. That held for all 2.77 million archived hits, and D-station hits cluster 0.1–0.2" above the limit.

| Class | Max tool | left/right | top |
|---|---|---|---|
| A | 0.5 | 2.409 | 1.347 (1.784 at T20) |
| B | 1.25 | 2.847 | 1.784 (2.595 at T44) |
| C | 2.0 | 3.658 | 2.595 |
| D | 3.5 | 4.378 | 3.315 |

### Other limits

- Reach without a reposition: X -1.5 to 97.07 (the largest X ever used), Y -0.79 to 48.78.
- Clamp ranges: clamp 1 11.8–70.9, clamp 2 19.7–78.7, clamp 3 27.6–86.6. Shop habit: 3 clamps on sheets 36" and longer (usually `12,48,80` on 96" sheets), 2 clamps on shorter sheets.
- Press capacity in the Lantek config: 20,000 kg (about 22 US tons).
- Nibble scallop tolerance: 0.2 mm (0.0079"). Lantek's circle-nibble pitch reproduces it exactly.

### Cycle time (ETIME)

`ETIME ≈ 5.37 + 0.157·hits + 1.583·tool changes + 0.0263·travel(in, longer axis) + 0.252·index changes`, in seconds.
Fitted on 7,936 programs against Lantek's own estimates: median error 2.5%, 90th percentile 5.3%.
