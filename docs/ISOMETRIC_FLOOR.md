# Fixed isometric floor

```sh
cargo run --locked --features windowed -- art floor-demo
cargo run --locked --features windowed -- art floor "$HOME/Work/arcanum-ce-test"
```

The original demo loads one floor frame, draws it nine times, and draws
bread frame 0 over the center tile. The synthetic demo uses the same
positions and order, with a green diamond and the existing amber ring.
`art floor` accepts the same `--ordered` and `--mods` options as ART
inspection and reports the winning sources. The floor replacement must
match the verified dimensions and hotspot. [Viewer](SPRITE_VIEWER.md)

## Original tile

The CE-policy VFS selects `art/tile/fl1bse0a.art` from `Arcanum2.dat`.
The winning `art/tile/tilename.mes` in `Arcanum5.dat` identifies
`fl1` in entry 200, the first indoor flippable name. In CE
(`fb721982`), `src/game/a_name.c` loads the name table,
`a_name_tile_fname_to_aid` constructs the indoor base ID with the same
terrain number on both sides, mask 15 and variant 0, and
`build_tile_file_name` produces the `bse0a` suffix. Transition masks
and different variants can select other files. `tig_art_blit` in
`first_party/tig/src/art.c` toggles horizontal mirroring when both
terrain components permit a flip and the tile art ID carries the flip
flag. The selected base variant uses the stored orientation. The demo
loads its resolved path directly.

The 2,942-byte resource has flags 1, stored FPS 8, 8-bit indices, palette
slot zero only, one frame and a stored action marker of 1. The frame is
78 × 40 with hotspot (39, 39) and offset (0, 0). The 132-byte header is
followed by a 1,024-byte palette and a 28-byte frame record. All
1,758 bytes beginning at offset 1,184 form the declared RLE payload.
An independent expansion matched the existing decoder byte for byte.
Index zero accounts for 1,520 transparent pixels, leaving 1,600 opaque
pixels. The 40 row widths rise from 2 to 78 in steps of 4 and fall
symmetrically, with 78 opaque pixels in both middle rows. There was no
decoder change. [Installed ART test](../tests/installed_art.rs)

## Projection and order

The origin is the **center of tile (0, 0)**, with screen X increasing
right and screen Y increasing down. For signed tile coordinates `(x,y)`,
the center displacement is `(40 × (y − x), 20 × (x + y))`. Advancing X within a row moves (−40, +20), and advancing Y to the
next row moves (+40, +20) at the same X. The projector widens
the operands and rejects a result outside signed 32-bit screen
coordinates. It owns no camera or world state. [Projection](../src/isometric.rs)

CE's `location_xy` in `src/game/location.c` includes a −40 X offset
relative to its origin. CE `tile_draw_iso` in `src/game/tile.c` blits
a 78 × 40 rectangle at that location plus (1, 0). Relative to the
center convention `location_xy + (40, 20)`, the raster begins at
(−39, −20). The floor draws therefore use the projected center plus
(0, 19) as the stored (39, 39) hotspot's target. Subtracting the
hotspot yields the verified raster origin. CE's ordinary object
placement in `src/game/object.c` uses `location_xy + (40, 20)`
with additional instance offsets when present. The demo attaches the
foreground hotspot to the center with zero instance offset.

The floor pass visits increasing tile Y, then increasing X within
each row, corresponding to CE's row traversal. For this artwork the
neighboring rectangular footprints overlap, while their opaque diamonds
touch without overlap or gaps. Fully transparent edge pixels leave
earlier draws intact. `gamelib_draw_game` in `src/game/gamelib.c`
draws floors before objects. Our fixed list likewise places the
foreground after the floor. The draw list has no general depth sorting.

The viewer keeps tile (0, 0) at the drawable center, and preserves the
relative offsets on resize. It clips through the same compositor,
clears and packs one framebuffer per redraw, and reuses the decoded
tile buffer for all nine draws. The scene exercises one base tile.
Transitions, flipped variants, map loading, scrolling, and lighting
are outside the fixed demo.

## Verification and performance

The original stone floor and bread, and the synthetic floor and amber
ring, were inspected in native Wayland windows at 1.25× display scaling.
The original patch showed a continuous diamond with the bread drawn
above the stone. Both scenes remained centered and clipped to the
viewport when resized from 480 × 340 to 112 × 82 logical pixels. Each
resized window submitted a new buffer. Neither submitted a buffer
during a 750 ms idle observation after settling, and both closed
normally. Portable tests compare both diagonal seams row by row, check
the exact floor-pass order and foreground pixel, and verify the clipped
render against the center crop of the larger render. The optional
installed test compares the original's full alpha mask to the fixture
and checks its independently derived RGBA fingerprint.

The optimized 640 × 480 CPU probe alternated the Milestone 28 overlap
pair and the fixed patch on the same i5-6300U host. It used Rust
1.100.0 at `-O`, one pinned logical CPU, preallocated buffers and
`black_box`. The first batch was discarded. Seven batches measured
12,000 draws or 500 complete buffer passes each. The complete pass
includes clearing, projecting the center-relative draw anchors into
the viewport, drawing, and packing the framebuffer. Window presentation,
asset loading and allocation are outside the timings.

| Workload | Draw only, µs median [min, max] | Clear, draw and pack, ms median [min, max] |
|---|---:|---:|
| Milestone 28 overlap pair, historical | 2.50 [2.45, 2.60] | 0.94 [0.92, 0.97] |
| Overlap pair, fresh | 3.65 [3.57, 4.37] | 0.962 [0.938, 1.265] |
| Nine original tiles and bread | 49.74 [49.32, 50.24] | 0.988 [0.976, 1.493] |

The patch increases drawing work. The full-buffer ranges overlap,
so the difference in full-pass medians should be treated as approximate.
Both workloads use the same roughly 2.05 MiB of RGB8 and one XRGB
buffer, before backend or vector overhead. The patch retains 20,256
bytes of decoded tile and sprite pixels.

Formatting, Clippy, default debug and release builds, windowed builds
and tests, and `git diff --check` passed. All 118 portable tests also
passed in an asset-free, network-isolated environment. All 15 optional
installed-data checks passed, including `vfs-smoke`. The five headless
demos ran and both schematic audits reported 119/119.
