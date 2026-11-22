# Original map sector floor

```sh
cargo run --locked --features windowed -- art sector "$HOME/Work/arcanum-ce-test"
```

The native viewer imports a **5 × 5 floor patch from Bates Mansion level 1**.
It reads `maps/bates mansion lev 1/67108865.sec` from the winning
`modules/Arcanum.dat` mount. Local X runs 29–33 and local Y runs 32–36.
These are map tiles (93, 96) through (97, 100), centered at (95, 98).
The command reports the sources of the sector, terrain names and tile
frames. It accepts the existing `--ordered` and `--mods` options.
The existing `art floor-demo` still provides an asset-free scene.

## Base sector framing

CE revision `fb721982`, in `src/game/map.c` and `src/game/sector.c`,
constructs `maps/<map-name>/<sector-id>.sec` with an unsigned decimal
sector ID. `sector.h` packs two 26-bit coordinates as
`sector_x | (sector_y << 26)`. Map coordinates divided by 64 identify
the sector. Sector (1, 1) begins at map tile (64, 64). Local coordinates
are the low six bits of each map coordinate. `tile_id_from_loc` and
`TILE_MAKE` index a sector as `local_y × 64 + local_x`.

`sector_load_game` reads the light list before the tile list.
`sector_light_list_load` reads a signed little-endian 32-bit count.
`light_read` in `src/game/light.c` reads one fixed 48-byte wire record
per light. The Rust reader consumes and bounds-checks each record in
sequence, with no lighting state or interpretation of light fields.
`sector_tile_list_load_internal` then reads 4,096 little-endian 32-bit
tile art IDs, totaling 16,384 bytes. The 51,112-byte Bates resource
declares nine lights, so the tile array starts at **436** and ends at
16,820. That offset is computed from the list in every file.

The importer rejects incomplete light records and incomplete tile
arrays. A light record has no delimiter or integrity tag. Unrelated
bytes can fill the declared record span, and length checks alone
cannot identify that substitution if the required tile span also
fits. The parser never scans for plausible IDs to infer an earlier
boundary. It leaves post-floor records opaque and caps total input at
8 MiB. [Native sector reader and tests](../src/sector.rs)

## Tile artwork and precedence

The tile art type is zero in bits 28–31. The first and second terrain
indices occupy bits 22–27 and 16–21. Bits 7 and 6 select the flippable
group for each terrain, and bit 8 selects outdoors. Bits 12–15 select
the mask, and bits 9–11 select the variant letter. The base filename
rules come from `a_name_tile_aid_to_fname` and `build_tile_file_name`
in `src/game/a_name.c`. Names are indexed by ordinal within the
matching group of `art/tile/tilename.mes` (groups 0, 100, 200 and 300).
The winning table comes from `Arcanum5.dat`. The sampled base IDs use
mask zero and equal terrain names, which produce the `bse0` suffix.
Other verified direct base cases are mask 15 or mask zero with differing
names. Mixed transition masks, flip flags and alternate palettes return
explicit errors in the bounded resolver.

| Local tile | Map tile | Art ID | Resolved path under `art/tile/` |
|---|---|---|---|
| (29, 32) | (93, 96) | `0x01040e00` | `wd1bse0h.art` |
| (33, 32) | (97, 96) | `0x01040c00` | `wd1bse0g.art` |
| (31, 34) | (95, 98) | `0x004104c0` | `wodbse0c.art` |
| (33, 36) | (97, 100) | `0x004106c0` | `wodbse0d.art` |

An independent read of the extracted sector bytes produced the
sample IDs above. All ten selected artwork variants come from
`Arcanum2.dat`. Each distinct path is decoded once per import through
the existing VFS and ART loader. The 25 tile references retain their
map coordinates and order. The renderer uses the existing 78 × 40
floor placement and row traversal. It projects offsets relative to the
selected map tile and draws directly into the existing RGB framebuffer.
[Native patch loader](../src/content/sector.rs) · [Projection](ISOMETRIC_FLOOR.md)

The VFS selects whole-file replacements for the sector, names and ART.
For example, the inspected Bates Mansion level 2 sector at the same
numeric ID is shadowed by `modules/Arcanum.PATCH1`. CE also supports save
`.dif` overlays and terrain-file or generated fallbacks when it cannot
use the base sector. This importer opens the explicit winning map
resource. Save differences, editor temporary sectors, terrain fallback,
lighting, objects and other tail sections are outside this slice.

## Visual verification and timings

The native Wayland window displayed the imported mansion region with
its original plank and patterned floor bands. The complete patch
remained centered and clipped when resized from 560 × 380 to 144 × 108
logical pixels. A fresh buffer followed resize, and there was no new
buffer submission during a 750 ms idle observation. The installed
regression checks independent position, art-ID and path samples, the
exact alpha masks, all 40,000 opaque floor pixels, the center anchor, and pixel equality
between a small viewport render and the corresponding full render crop.

An optimized CPU probe on the i5-6300U Linux host used one pinned
logical CPU and seven warmed sample batches. Decode-only measurements
used previously loaded bytes. The VFS row reused initialized mounts
and includes archive reads, inflation, name lookup and decoding.
Composition used preallocated 640 × 480 buffers and excludes window
presentation.

| Stage | Median µs [min, max] |
|---|---:|
| Sector light framing and floor decode | 1.50 [1.42, 1.54] |
| Ten ART frame decodes | 23.24 [22.56, 24.06] |
| VFS import and artwork resolution | 748.08 [726.83, 779.53] |
| Compose 25 tile draws | 96.75 [95.40, 97.67] |
| Framebuffer clear and pack only | 1,033.70 [952.01, 1,684.31] |
| Full clear, draw and pack | 1,152.70 [1,100.54, 1,529.48] |

The ten decoded frames retain 124,800 pixel bytes. At 640 × 480 the RGB8
framebuffer and one packed presentation buffer occupy about 2.05 MiB
before capacity slack and backend allocations.

Formatting, Clippy, default debug and release builds, the windowed
build and tests, and `git diff --check` passed. All 123 portable tests
passed in a network-isolated environment without game assets. All
16 installed-data checks passed, including the feature-specific sector
regression and `vfs-smoke`. The five headless demos ran and both
schematic audits remained at 119/119.
