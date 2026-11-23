# Original Bates Mansion wall object

```sh
game="$HOME/Work/arcanum-ce-test"
cargo run --locked -- art sector-wall-inspect "$game"
cargo run --locked --features windowed -- art sector-wall "$game"
```

The preview draws one imported wall over the 9 × 9 base-sector patch
from local (34, 14) through (42, 22). The patch is centered at world
(102, 82) and includes the 15 original Rug3-Flip cells. The existing 5 × 5,
9 × 9 and facade preview commands remain available. The diagnostic and
window use the same object, prototype, name tables and ART resolution
through the existing VFS. [Wall loader](../src/content/wall.rs)

## Sector and object boundaries

The winning `maps/bates mansion lev 1/67108865.sec` comes from
`modules/Arcanum.dat`. The offsets below describe this 51,112-byte file.
The decoder derives them from the stored counts and markers.

| Section | Start byte | End byte, exclusive |
|---|---:|---:|
| Count and nine light records | 0 | 436 |
| 4,096 tile words | 436 | 16,820 |
| Roof empty marker and 256 roof words | 16,820 | 17,848 |
| Metadata marker `0xaa0004` | 17,848 | 17,852 |
| Tile script count and one 24-byte record | 17,852 | 17,880 |
| Sector script record | 17,880 | 17,892 |
| Townmap, aptitude, light scheme and sound fields | 17,892 | 17,916 |
| 512-byte tile-block mask | 17,916 | 18,428 |
| Object records | 18,428 | 51,108 |
| Final object count | 51,108 | 51,112 |

The original file has 368 records, including 268 walls, 19 portals and
81 scenery records. CE's `sector_load_game` reads the same preceding
sections. `objlist_load` obtains the count from the last four bytes,
seeks back to the framed start, reads that many instances and expects
to find the count again. The Rust reader reserves the trailer before
parsing and requires the records to consume exactly the intervening
span. The bounded reader accepts the base-sector metadata markers
`0xaa0000` through `0xaa0004` and accounts for each payload they enable.
Scripts, sounds, roofs and blocking bits are used only for framing.

CE's `obj_read` and `obj_inst_read_file` establish the variable version
119 instance header. Each of the supported types has a three-word
override bitmap, with two common words and one subtype word.

| Relative byte | Meaning |
|---:|---|
| 0 | Version word, 119 |
| 4 | 24-byte prototype identity, numeric kind 1 in this subset |
| 28 | 24-byte instance identity, preserved verbatim for inspected walls |
| 52 | Object type word. Wall is 0, Portal 1, Scenery 3 |
| 56 | 16-bit count of stored fields |
| 58 | Three little-endian words selecting stored fields |
| 70 | Field payload in ascending bitmap order |

Stored values use their wire types. The location has a presence byte
followed by the two 32-bit coordinates when present. Optional arrays
have a presence byte, element width, count, opaque header word, values
and a counted index bitmap. The reader checks the sizes, bitmap
cardinality, signed lengths and record boundaries. The observed
portal and scenery fields are validated and framed to reach walls
elsewhere in the list. Other object types and unverified portal or
scenery fields return an explicit error at the record. The reader
never scans forward for plausible header bytes. [Serialized object decoder](../src/sector/objects.rs)

## Selected wall and placement

Zero-based record **52**, bytes **23024–23106**, is a conventional wall.
Its two overridden fields are current ART ID `0x10300900` and location
(98, 79), the latter stored as local low bits (34, 15) within sector
(1, 1). Like CE's base-sector load, the importer combines those low
six bits per coordinate with the sector origin. The stored instance
identity is null and remains independent of the referenced numeric
prototype identity 1021.

The VFS selects the loose `proto/001021 - Wall.pro` from the installed
`data` directory. The wall prototype supplies offsets (0, 0), scale
100%, flags `0x4000` (cover) and wall subtype flags zero. It also stores
neutral blit color, alpha 255 and no custom blit flags. The importer
reads the complete version 119 prototype, checks its type and identity,
and resolves absent instance properties against it. Other render
modes, scale changes and wall translucency are explicit limitations.

TIG interprets the ID as wall art category 1, structure 3, piece 0,
variation 1, rotation 1, palette 0, damage zero and no flip flag.
`art/wall/structure.mes` selects interior family `in4` for rotation 1.
`wallname.mes` confirms the stem. CE's `build_wall_file_name` supplies
`bse` for this piece and `U1` for the undamaged variation. Both tables
and the winning `art/wall/in4bseu1.art` come from `Arcanum2.dat`.
The asset stores one direction, one 40 × 154 RLE frame and two palette
blocks. TIG selects frame zero and palette slot zero from this ID.
The native ART reader now accounts for the second palette block when
locating the table. It continues to decode using slot zero.
[ART format](ART_FORMAT.md) · [Wall art ID and placement](../src/wall.rs)

CE's `wall_draw` is the top-down path. Isometric `object_draw`
draws walls in its object pass, after the tile pass. Its
`object_get_rect` starts with `location_xy + (40, 20)` plus the
instance's effective pixel offsets. TIG's `tig_art_frame_data`
subtracts 40 from the stored hotspot X and adds 20 to its Y for the
selected rotation. The stored hotspot (39, 132) becomes (−1, 152).
No horizontal transform applies to the selected ID. The asset has
zero movement offsets. Native placement expresses the effective
hotspot as an anchor shift while keeping the stored frame metadata
unchanged.

Relative to the preview center, the wall's tile center projects to
(40, −140). The native anchor becomes (80, −160) using the stored
hotspot. Its raster starts at (441, 8) on the 800 × 600 drawable.
The preview submits all original floor and facade cells in normal
tile order and then the selected wall. CE also orders flat and
upright objects separately, tests roof coverage and handles wall
transparency in the object pass. The one-object preview covers
ordering relative to the surface. Ordering among multiple objects,
room visibility, roof fading, tint and shadows remain outside this
scene.

## Regression and fidelity checks

A separate byte reader walked all 368 object records and decoded the
wall's indexed pixels from the original ART file. Its source-based
unlit composition matches the complete Rust framebuffer, including
the 5,368 opaque wall pixels. The installed regression pins the
complete RGB8 hash `8c25737516639a09` and compares a clipped render
with the matching region of the full render. Portable fixtures
exercise the metadata variants, variable object boundaries, reserved
footer, malformed records, two-palette framing and the wall pivot
over a clipped surface.

The preview represents the unlit floor and one verified object.
An original gameplay screenshot with a clear view of this specific
wall was unavailable. The comparison uses original bytes and the CE
isometric placement rules. Exact in-game appearance remains open
because the preview omits ambient and local lighting, fading,
occluding objects and shadows.

## Next step

Verify a short connected wall run with an adjacent mirrored
perpendicular piece. Resolve the extra ART palette selection and
confirm the same-cell placement and object draw ordering before
generalizing to additional object types.

## Wayland captures and performance

The three live `grim` captures are under
`target/visual-checks/milestone-33/`. The full 800 × 600 drawable, the
400 × 400 close-up and the resized 400 × 400 drawable each matched
their respective regions of the independently constructed framebuffer
pixel for pixel. The resize clipped the upper wall while keeping the
map center fixed. Wayland submitted one fresh buffer for the resize
and none during a 750 ms idle check.

Release measurements on Linux with Rust 1.100.0 and one pinned
i5-6300U logical CPU used one warm-up batch and seven measured batches.
The parse and decode rows use preloaded bytes. Wall VFS import reuses
the mounted VFS and includes sector inflation, prototype and name
lookup, and the wall ART decode. Framebuffer timings use preallocated
800 × 600 buffers and exclude the window backend.

| Stage | Repetitions per batch | Median µs [min, max] |
|---|---:|---:|
| Frame and parse all 368 objects | 1,000 | 74.0 [69.4, 77.1] |
| Decode wall ART frame zero | 2,500 | 9.89 [9.49, 11.07] |
| Import selected wall through the VFS | 200 | 3,551.9 [3,440.4, 3,894.9] |
| Compose wall alone | 250 | 7.63 [6.78, 12.37] |
| Compose 81 surface draws | 250 | 419.2 [406.9, 463.6] |
| Compose 81 surface draws and wall | 250 | 423.5 [409.5, 443.6] |
| Clear, compose and pack complete scene | 140 | 2,249.6 [2,198.2, 3,010.0] |

The wall frame retains 24,640 pixel bytes. The RGB8 framebuffer and
one packed presentation buffer occupy about 3.20 MiB together,
excluding backend allocations. Timing ranges for surface-only and
surface-plus-wall composition overlap.

Formatting, default and windowed Clippy, default and windowed tests,
debug and release builds, and the diff check passed. All 136 portable
tests and all five headless demos passed with network and game data
isolated. All 20 installed-data tests passed, including `vfs-smoke`.
Both schematic audits returned 119/119.
