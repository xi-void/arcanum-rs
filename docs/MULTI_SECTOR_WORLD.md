# Multi-sector world navigation

The windowed world viewer can stream a bounded rectangle of original map
sectors. The starting position is a world tile. The four sector bounds
are inclusive and use sector coordinates.

```sh
game="$HOME/Work/arcanum-ce-test"
cargo run --locked --features windowed -- art world "$game" \
  --sectors "k'na tha" 5 14 8 14 341 928 1280 720 --trace
cargo run --locked -- art world-inspect "$game" \
  --sectors "k'na tha" 5 14 8 14 384 928 1280 720
```

Arrows pan, Shift increases the step, Home restores the starting point and
Escape closes the window. The default Bates Mansion view and the existing
`--view` inspection mode are also available.

The current viewer uses [predictive sector preparation](SECTOR_PREFETCH.md).
The measurements below preserve the Milestone 39 synchronous baseline.

## Original data

CE's `sector_id_from_loc` shifts each world tile coordinate right by six.
`SECTOR_MAKE` packs the X sector into bits 0–25, then the Y sector into bits
26–51. `sector_loc_from_id` recovers the world-tile origin by shifting both
components left by six. `sector_rect_from_loc_rect` splits the footprint
at multiples of 64. The decimal packed ID is the stem of each base
`maps/<map>/<id>.sec` resource. The Rust viewer uses that convention
through its VFS.

| K'na Tha sector | Origin | Source | Floor cells | Original walls and scenery | Renderable walls and scenery |
| --- | --- | --- | ---: | ---: | ---: |
| (5,14), 939524101 | (320,896) | modules/Arcanum.dat | 4096 | 28 / 433 | 28 / 386 |
| (6,14), 939524102 | (384,896) | modules/Arcanum.dat | 4096 | 64 / 475 | 64 / 426 |

The original floor arrays begin at byte 4 after a zero light count in
both samples. At world Y=928, the seam cells at X=383 and X=384 store
`0x004109c0` and `0x004101c0`. The winning floor frames are
`art/tile/grsbse0e.art` and `art/tile/grsbse0a.art` from Arcanum2.dat.
The two selected sectors contain no baked facades or portals. The Bates
regression covers the existing facade and portal paths.

The table records the Milestone 39 baseline. Current loading uses map properties
and original wilderness templates through the same world stream.

## Map initialization and terrain reconstruction

CE's `map_open` sets tile and sector limits using the two 64-bit dimensions
in `map.prp`. The Rust catalog displays those limits independently of the
rectangle covered by authored sectors. The archive's `MapList.mes` still
supplies the gameplay entry. Streaming keeps a 12-sector cache across the
larger coordinate range.

CE's `sector_load_game` uses an authored `maps/<map>/<id>.sec` first. If that
resource is absent, `terrain_sector_path` reads the destination cell's word
from `terrain.tdf` and selects an original stock template. The first and
second terrain types occupy five bits each. The next four bits choose the
transition and its orientation, and the last two choose a base variant.
The type names and travel block bits come from `terrain/terrain.mes`. Rows
in the stock overworld TDF are length-prefixed zlib streams. A corrupt
authored winner is reported with its original provenance.

Terrain templates enter the same sector reader as authored data. Floor
tiles, facades, roof words, collision and supported object records keep their
original data. Object placement uses the local six bits plus the destination
sector origin, as CE's `objlist_load` does for a template. Travel uses the same
terrain decoder and can use a resolved template for an on-foot stop. It still
honors the original sector block list and verifies collision at arrival.

| Location | Destination sector | Installed source | Recovered surfaces and scenery |
| --- | --- | --- | --- |
| Tulla western approach | (441, 487) | `terrain/desert island/67108864.sec` | 4096 surfaces, 351 scenery records |
| Vendigroth eastern edge | (308, 585) | `terrain/desert island/0.sec` | 4096 surfaces, 364 scenery records |
| K’na Tha forest seam | (9, 14) | `terrain/forest/67108864.sec` | 4096 surfaces, 477 scenery records |

The Tulla inspection also exposed two generic rendering rules. CE's
`tile_draw_iso` puts each 78×40 baked facade in the floor cell's fixed blit
rectangle, independent of the frame hotspot. Its wall filename table maps
pieces 16–20 to the five-piece window family. Both use the shared importers.
The traced overworld sectors now retain 209/209 and 192/192 original walls.
One neighboring authored sector also recovers 200 baked facade cells.
The western travel fixture `(28320,31168)` is a walkable dirt transition in
sector `(442,487)`, 135 tiles west in X from Tulla's original `GameArea`
arrival `(28455,31171)` in sector `(444,487)`. It does not match the supplied
CE palace views. For example, tile
`(28324,31179)` in sector `32682017210` stores `0x071c01c0`, which resolves
to `art/tile/blkbse0a.art`. Its visible texels are opaque black, and the
winning tile-name entry carries `/b`. The two adjacent dirt-to-black
transition IDs remain original floor draws. The sector's serialized roof
list is empty. CE's `tile_is_blocking` reads the tile mask, a facade's
walkable bit or the tile-name flags. `tile_draw_iso` separately draws the
authored ART, with roof coverage tested before the blit. That distinction
accounts for the reproduced black cells without terrain substitution.

At the authored arrival, wall records 131 in `32614908348.sec` and 56 in
`32682017212.sec` use `0x11e1d811`, piece 7. CE's
`build_wall_file_name` maps pieces 7 and 8 to `uec` and `lec`. These
end caps now use the ordinary wall palette, pivot, mirror and depth rules.
The palace approach contains tile trigger 2341 at X 28450, which checks
global flag 2395 and other quest state. The tower door farther west is
scenery record 9 in `32682017211.sec`, prototype 4026 at `(28376,31171)`.
Its original Use script 2356 points to Tulla Interior (map 5031) at
`(507,221)`. The alpha still lacks general tile-script execution and
additional portal and scenery appearances.

The Vendigroth bell-tower entry is scenery record 256, prototype 4047,
at `(19689,37486)` in `39258685747.sec`. The instance's Use script 2379
teleports the triggerer to map 5032 at `(2051,2038)`. There is no additional
access condition in that script. Destination sector `2080374816.sec` used
to fail at record 91, a trap with prototype 26021. CE's trap schema defines
three 32-bit fields followed by 32-bit and 64-bit sparse arrays. The loader
now validates those fields and continues with the following records.
Trap detection and effects remain unsupported.


### Coverage and remaining mechanisms

- **Map and streaming.** Map selection, archive precedence, authored and
  template sector selection, viewport neighbors, prefetch, ordering and
  eviction are active. CE's final `terrain_fill` fallback uses original
  illegal-tile ART and its variant selection. The alpha reports unavailable
  templates and has no emulation of that final fallback or editor-created TDFs.
- **Surfaces and visibility.** Floor and supported facade IDs, original roof
  lists, player-based fading, ambient lighting and the verified darkness and
  shadow masks are active. Full CE visibility and all ART variants remain
  partial. Existing Tulla portal rejection reasons appear in the object trace.
- **Objects and collision.** Supported sector records use original effective
  prototypes, offsets and tile masks. An unframeable variable-length record
  reports its byte offset and disables unverified collision for that sector.
  Template light relocation remains unsupported and is reported separately.
- **Mobiles and scripts.** CE's `map_load_mobile` reads a GUID-prefixed
  compiled object stream whose filename is reversed ROT13. The alpha still
  has a limited set of verified individual mobile imports. The compiled
  stream, full postprocessing, arbitrary scripts, saved sector diffs and
  dynamic mobile state remain incomplete. Verified object-use scripts and
  `map.jmp` transitions use the shared world loader.

Inspect the source, reconstructed records and collision state with the
existing headless command.

```sh
cargo run --locked -- art world-inspect "$HOME/Work/arcanum-ce-test" \
  --map 5000 --center 28320 31168 --roofs --trace
```

The streamed viewer also supports opt-in [original static lighting](STATIC_LIGHTING.md)
at a fixed scheme hour, with mask preparation in the same sector worker
and shared shading for floors, scenery, walls and portals.

## Streaming and ordering

The camera keeps one pixel transform relative to the initial world tile.
Before drawing, inverse projection maps the viewport into the sector
grid with a conservative 640-pixel overhang allowance on both screen
axes. Static rasters outside that documented owner-tile bound are
reported. Final culling tests each actual raster rectangle, including its
pivot and instance offset.

Each sector has one owner for its cells and original records. Nearby
sector draws are merged in a single stable world order whenever the
active sector set changes. Surfaces render in world Y/X order, then flat
and upright objects use the same depth and tie-break rules as the single
sector viewer. Baked facades remain in the surface pass. No loading
occurs inside the compositor.

The sector cache retains at most 12 entries, including unavailable
sectors. It evicts the least recently used entry outside the required
set. A viewport whose conservative footprint needs more than 12
sectors receives an explicit error. A separate decoded ART cache uses
resource path, frame, direction, palette and mirroring as a key. It shares frames
across sectors and is capped at 96 MiB of retained RGBA and 4,096
frames. Only entries that no cached raster references can be evicted
from the ART cache.

`--trace` reports sector loading, ART resource reads and decoding,
global ordering, software composition, packing and client-side
submission. Slow frames are reported without enabling per-frame
tracing. These durations distinguish client work from an unexplained
stall. They do not measure an asynchronous compositor's display
latency.

## Verification

In a native 1280 × 720 Wayland window the path began at world tile
(341,928), crossed at the exact X=383.5 boundary and continued to X=402.7.
Returning Home after a 960 × 560 resize reproduced all 921,600 original
pixels. The Bates Mansion capture shows original geometry beside its
black void boundary. Stationary intervals showed no new buffer
attachments.

Portable fixtures cover a populated seam, cross-sector wall layering
and transparency, viewport overhang, missing sectors, warm reuse and
sector eviction. Installed tests cover K'na Tha's two original sectors,
world-space overlap during crossing and the Bates Mansion 268-wall
regression.

## Release measurements

This run used Rust 1.100.0 on a Linux ThinkPad with an i5-6300U, pinned to
CPU 1. The VFS and filesystem page cache were already warm. The first
sector took 20.73 ms to import and the neighboring cache miss took
13.43 ms. Ordering took a further 0.60 and 1.16 ms respectively.
The new neighbor required 11 additional VFS streams, including two ART
resource reads, and three frame decode attempts.

The warm benchmark rendered 250 frames at 1280 × 720, moving repeatedly
across the same seam. Composition includes the visibility pass,
framebuffer clear and packing, with window submission measured separately
in the Wayland trace.

| Warm operation | Median | p95 | Worst |
| --- | ---: | ---: | ---: |
| Camera and sector preparation | 0.0014 ms | 0.0060 ms | 1.455 ms |
| Visibility selection alone | 0.119 ms | 0.349 ms | 0.510 ms |
| Complete software composition | 7.227 ms | 8.676 ms | 10.329 ms |

The warm route performed no sector reads, VFS streams or ART decodes.
Two cached sectors used 2.16 MiB of RGBA and 2.72 MiB of raster descriptors,
excluding VFS indices, diagnostics and framebuffers. A sweep across
sector X=5 through X=18 at Y=14 reached the 12-entry cap. It evicted two
sectors outbound and one on returning Home. Reloading Home took
12.79 ms of import time and used nine VFS streams, zero ART reads and
zero frame decodes. The extended sweep retained approximately 2.27 MiB
of RGBA and 6.76 MiB of raster descriptors after returning.

In the separate Wayland trace the slowest observed frame was the first
1536 × 864 configure. It took 76.73 ms in total, including 44.34 ms
of sector preparation. The trace records each stage independently.
The Milestone 38 outlier remains unattributed.

## Milestone 51 generic reconstruction audit

### Invariant and current boundary

The streamed `art world --sectors` path resolves the winning sector
through the VFS. The floor parser reads the light list, all 4,096 floor
words and their tile/facade identities. The object parser walks the
roof-list framing, sector metadata and each version-119 record in field
bitmap order, ending exactly at the footer count. The static importer
joins each record to its original prototype, applies serialized overrides,
resolves ART through the original MES tables and VFS, and builds a
world-positioned raster. Sector assembly sorts accepted rasters by the
shared world keys. The viewport only selects intersecting rasters. CPU
and GPU consume the same draws and clipping geometry.

For a decoded list, each categorized static record enters the import,
gets an explicit unsupported reason, or contributes to the framing-only
count if it lacks placement or ART. Accepted objects either produce
rasters or get a projection/overhang rejection. Culling is counted
separately from rejection. If record framing fails, the worker can
still present the floor. The object counts then read **unavailable**
with the exact parser error, since a variable-length record without a
trusted boundary cannot be skipped. This is the main obstacle to
general world coverage. Current object-list schemas are bounded to
walls, portals and scenery. Roofs are framed but their rendering and
CE's party-dependent visibility effects are outside this pipeline.

Use `art world-inspect … --sectors … --trace` for provenance per
accepted static raster. It prints the record's original position, byte
span, identity, prototype source, stored and effective ART, resolved
resource and provider, direction, frame, selected palette, offsets,
effective pivot and render order. The ordinary summary includes
decoded, imported, rejected, submitted and clipped counts. The
provenance allocation is limited to opt-in headless inspection.

The legacy Bates showcase commands select fixed rooms or record lists.
Those selectors do not participate in streamed loading. Original MES
lookup, directional wall/portal rules and authored frame state are
shared across locations. The supported flame and mobile appearances
remain deliberately bounded. In particular, the mobile module selects
three Bates and one K'na Tha standalone `.mob` paths by map and sector.
It loads their identities, prototypes and starting positions from
the originals. The rabbit walk is an opt-in relative route. Tulla's
test exercises static reconstruction and does not imply general
mobile discovery, saved-world diffs or gameplay visibility.

### K'na Tha entrance and adjoining passage

Map 5067 in the winning `rules/maplist.mes` is K'na Tha. The dark
rectangle is the nine-by-nine **STD indoor stone floor** at
X=402–410, Y=778–786, in sector (6,12), ID 805306374. The sample
at (406,782) is `0x028a0400` and resolves to
`art/tile/stdbse0c.art`. Eight original variants cover those
81 cells. At (405,788), the approach uses `0x065901c0` and
`art/tile/ts1bse0a.art`. The sector has no facade IDs in its floor
array. Its roof empty marker is 1 at byte 16,484. Sector (6,13)
also has an empty roof list. The second adjoining stone corridor
runs from X=394–398 and Y=826–858, spanning the two sectors.

The source is `modules/Arcanum.dat` in both cases. In the first
sector the record stream starts at byte 17,044. Record 558 is a
91-byte portal at bytes `67710..67801`, followed immediately by
wall record 559. Its bitmap `[00000003,00000000,00000003]` has
current ART and location in word zero and portal flags and lock
difficulty in word two. CE `sub_40A8A0` in `obj.c` declares
both subtype fields as INT32. Their stored values are `0x11`
(locked and always locked) and 100. Rejecting the lock bit at
byte 67,780 had removed all 653 static records from the scene,
including walls parsed before the error. The generic reader now
consumes the verified difficulty word without applying gameplay
lock mechanics.

The instance specifies tile (395,826) and ART `0x33483c11`.
The winning loose version-119 portal prototype 2029 supplies
offset (0,0), scale 100, object flags `0x4000` and neutral
blit parameters. Its prototype ART `0x30003411`, flags `0x09`
and difficulty zero are overridden. The stored door means
frame 0, direction 7 mirrored and requested palette 1. It
resolves through portal name row 105 to `art/portal/dg2e3au0.art`
in `Arcanum2.dat`. The resource falls back to its available
palette zero. The effective pivot is (39,111) and the leaf
follows the same-tile opening at record 557.

At the northern entrance, wall 201 and portal 202 share
(406,786). The portal's `0x3348dc11` is the same resource at
frame 3 and mirrored direction 3, with pivot (−1,62). CE
`portal.c` verifies frame 3 as the open endpoint for this
direction. Along X=402, Y=779–785, the seven records 92, 107,
121, 130, 142, 157 and 169 use wall pieces 9–15. CE
`build_wall_file_name` resolves those pieces to `w3l/w3a/w3r`
and `w4l/w4a/w4b/w4r`. Structure 36 selects DG2 from the
original wall table. All seven resources decode and draw via
the existing wall pivot, palette and ordering path. Their
irregular window openings are in the original pixels.

The continuation sector separately rejected scenery record 720
at byte 82,710. Its extra common bit 26 is CE's `OBJ_F_HP_PTS`,
another verified INT32. The record spans `82640..82743` and
stores HP 25 at (434,885), prototype 4038. Skipping those four
bytes as a typed field restores the adjoining list without
interpreting hit points or guessing record boundaries.

CE draws roofs after objects in `gamelib_draw_game`. Roofs use a
separate four-by-four tile grid. CE's `roof_recalc` and
`wallcheck_recalc` update fades around party/view state, and
`object_draw` consults roof coverage and translucent wall flags.
Those rules cannot explain the missing objects at the two audited
K'na Tha roof-empty sectors. The loss was already present in the
native import counts, before visibility, sorting or drawing.

### Independent Tulla verification

The winning map list names **Tulla Interior** as map 5031 and
supplies (507,221). The winning `maps/tulla interior/startloc.txt`
supplies (477,178). The installed test discovers the entry by
name, reads the start file through VFS and divides both sets of
coordinates by 64 to select the bounded (7,2)–(7,3) region.
Neither selection nor loading uses a Tulla-specific code path.

| Map and sector | Original walls / scenery / portals | Imported walls / scenery / portals | Unsupported | Raster rejected |
| --- | ---: | ---: | ---: | ---: |
| Bates Mansion 67108865 | 268 / 81 / 19 | 268 / 81 / 19 | 0 | 0 |
| K'na Tha 805306374 | 251 / 400 / 2 | 251 / 400 / 2 | 0 | 0 |
| K'na Tha 872415238 | 373 / 447 / 4 | 373 / 447 / 4 | 0 | 0 |
| Tulla 134217735 | 356 / 51 / 5 | 331 / 50 / 5 | 26 | 0 |
| Tulla 201326599 | 695 / 53 / 5 | 650 / 53 / 5 | 45 | 0 |

Both Tulla records have 4,096 submitted-or-culled surface cells,
including 407 and 1,111 embedded facades. The viewport centered
on (507,221) submits 119 object rasters, and its native capture
shows the carpeted crossing, round pillars, banners, planters
and doors. Tulla's 71 unsupported instances comprise 68 wall
pieces numbered 41–45, two walls with the unsupported flag combination
`0x404000` (cover and invulnerability) at records 338 and 339 in sector 201326599, and one
additive scenery record 65 at (470,157) with subtype flags
`0xb0` in 134217735. The trace lists each occurrence and
reason. There are no unknown serialized categories in the
two decoded Tulla object lists. The separate roof lists also
contain 91 and 214 non-invalid entries. The native screenshots
show the interior surfaces and static objects without CE's
roof pass. Standalone mobiles and roof and wall fading remain
separate coverage limitations.

The before/after K'na Tha views use the original floor,
shared camera and fixed animation tick. CPU and Vulkan
comparisons also cover the reconstructed southern passage,
the rabbit at 246 ms and Tulla. No matching-camera CE image of
these positions was available, so the comparison establishes
agreement with the serialized locations and CE rules above.
It does not establish original gameplay lighting or visibility
parity.

The audit adds an asset-free portal boundary test and installed
regressions for the two K'na Tha lists and metadata-discovered Tulla.
Formatting, Clippy, debug and release builds, 196 portable tests with
all features, 36 installed-data checks, 10 tests on each of Vulkan and
OpenGL, and both 119/119 schematic audits passed. The fixed-time rabbit
retains the same tile, offset, facing and frame after the surrounding
objects are restored. The Tulla entrance at the authored start tile
also matches on CPU and Vulkan.
