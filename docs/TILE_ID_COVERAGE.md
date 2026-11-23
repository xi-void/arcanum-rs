# Bates Mansion floor tile coverage

```sh
game="$HOME/Work/arcanum-ce-test"
cargo run --locked -- art sector-audit "$game"
cargo run --locked --features windowed -- art sector-wide "$game"
cargo run --locked --features windowed -- art sector "$game"
```

The headless audit lists every distinct raw ID in ascending order, its
occurrence count, decoded terrain pair, stored mask and variant, flip,
transition status, resolved path, and VFS provider or diagnostic.
It checks each distinct path once through the ART decoder and the floor
frame geometry check. Missing assets and unreadable or incompatible
frames are reported separately from unsupported IDs. Both commands use
the same VFS policy and optional `--mods` arguments as the 5 × 5 preview.
[Sector framing and provenance](SECTOR_FLOOR.md)

## Full sector audit

The inspected source is `maps/bates mansion lev 1/67108865.sec` in
`modules/Arcanum.dat`. All 4,096 raw words remain in the imported floor
array. A ready draw retains its original word too. Filename
normalization takes place only while resolving an artwork reference.

| Classification | Entries | Distinct raw IDs |
|---|---:|---:|
| Ready base tiles, including black floor | 2,777 | 13 |
| Ready `drt`/`blk` transitions | 1,214 | 19 |
| Horizontal flip among the transitions | 589 | 9 |
| Facade-type entries outside the floor renderer | 105 | 30 |
| Missing or incompatible artwork paths for supported tile IDs | 0 | 0 |

The 32 ready IDs refer to 23 distinct ART paths. The table's flip row
is a subset of the transition row. The 105 unresolved words have
category nibble `0xb` (facade), such as `0xb5be0801` and `0xb5c2081d`.
Their IDs and low bits remain verbatim in the array and report.
The floor-only importer rejects a patch containing one and the audit
keeps its frequency visible. The opt-in facade preview supports the
verified baked pieces. [Facade evidence](FACADE_IMPORT.md)

| Example raw ID | Frequency | Path under `art/tile/` | Transform |
|---|---:|---|---|
| `0x001c31c0` | 180 | `drtblk4a.art` | Normal |
| `0x001c31c1` | 225 | `drtblk4a.art` | Mirror X |
| `0x001c61c1` | 164 | `drtblk2a.art` | Mirror X |
| `0x004104c0` | 83 | `wodbse0c.art` | Normal |
| `0x01040e00` | 82 | `wd1bse0h.art` | Normal |
| `0x071c01c0` | 1,793 | `blkbse0a.art` | Normal |

## Supported original rules

In CE revision `fb721982`, `name_aid_to_fname` calls
`name_normalize_aid` before the tile filename callback. For tile IDs
that clears the low flag nibble and palette selector. The filename
therefore uses the **stored** four-bit mask and three-bit variant as
path inputs. Applying the logical mirrored-mask getter to the raw
flagged ID before naming would request paths absent from the
installed archives. `tig_art_blit` handles horizontal mirroring
separately. The supported case has bit 0 set and both terrain
components marked flippable. The Rust importer reverses each pixel row
with the floor rectangle in the same position. An asymmetric
`drtgrs4a.art` frame and a portable asymmetric fixture verify the
difference between a filename and its orientation.

The name table selects the two terrain families by indoor/outdoor
and flippable/non-flippable groups. Equal families and masks 0 or 15
use the direct base rules. For the other mixed masks,
`build_tile_file_name` ranks families by the outdoor groups. Reversed
filename order complements the mask with `15 − mask`. Its base-edge
fallback uses the corresponding first or second family if a name
lacks an outdoor rank. The stored variant chooses letter `a`–`h`.
The resolver implements these rules and requests that exact resource.
A missing result stays missing. Other low flags, alternate palettes,
inconsistent flip bits, unsupported shapes and non-tile types remain
explicit errors. [Resolver](../src/sector.rs)

## Expanded region

The 9 × 9 patch covers local tiles (20, 13) through (28, 21), or world
tiles (84, 77) through (92, 85), centered on (88, 81). It contains 72
interior tiles and nine mirrored `drt`/`blk` transition draws.
Artwork is derived from the sector words at every position. The
sampled `drt`/`blk` edge files have the same decoded black diamond
as `blkbse0a.art`, so their flips are visually symmetric in this
scene. No substitute artwork is selected. The separate asymmetric
tests check the transform.

The importer retains one decoded frame per VFS path and at most one
derived mirror per path within the region. It submits the existing
row-major floor draw order, using the same projection, framebuffer,
and event-driven window as the smaller preview.

## Visual verification and timing

Both previews rendered in native Wayland windows. The wider view showed
the original interior floor bands and black transition edge at the
recorded tile coordinates. Resizing it from 720 × 420 to 144 × 108
logical pixels clipped the scene around its center. The viewer presented
a fresh buffer for the resize, submitted none during the 750 ms idle
observation and closed normally. Validation compared the native drawing
with the source data and CE placement and naming code. It did not depend
on running the source game.

A release probe on the i5-6300U Linux host used one pinned logical CPU,
seven warmed batches, and preallocated 800 × 600 buffers. Loading reused
a mounted VFS and included sector and MES reads, name resolution, ART
decoding and the optional mirror. The draw and full-frame rows exclude
asset work, buffer allocation and window presentation.

| Region | Draws | Decoded frames | Derived mirrors | Load median µs [min, max] | Draw median µs [min, max] | Clear, draw and pack median ms [min, max] |
|---|---:|---:|---:|---:|---:|---:|
| 5 × 5 | 25 | 10 | 0 | 1,273 [1,000, 2,188] | 189 [177, 209] | 1.754 [1.740, 1.766] |
| 9 × 9 | 81 | 13 | 1 | 1,151 [1,036, 3,514] | 530 [524, 624] | 2.152 [2.114, 2.213] |

Loading ranges overlap. The wider scene retains 174,720 decoded and
mirrored pixel bytes. Both scenes use about 3.20 MiB for RGB8 and one
packed framebuffer, excluding backend overhead.

Formatting, Clippy, default debug and release builds, windowed build
and tests, and `git diff --check` passed. All 126 portable tests passed
in an asset-free network-isolated environment. All 17 installed checks
passed, including the full sector audit and `vfs-smoke`. Both schematic
audits remained at 119/119.

[Milestone 32 traces and renders the baked facade cells](FACADE_IMPORT.md).
