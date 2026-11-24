# Original dynamic lighting

Milestone 46 adds a bounded position and on/off diagnostic to the
streamed world viewer. The scene still uses the Milestone 45 shared
CPU/Vulkan lighting pipeline. Original static lights, candles and
animated flames remain part of the scene.

```sh
game="$HOME/Work/arcanum-ce-test"
cargo run --offline --locked --release --features windowed -- \
  art world "$game" --sectors "bates mansion lev 1" 1 1 1 1 101 83 1280 720 \
  --lighting 12 --light-probe 101 82 --light-probe 103 83 --trace
```

The two optional probes start enabled. Press **1** or **2** to toggle a
probe. **J/L** change probe 1's world tile X by one, and **I/K** change
Y by one. Arrow keys and Home still control only the camera. Keep the
same placement and add `--animation-ms 90` for a fixed-tick capture.
Omitting that option gives live flame playback.

## Original rules and verified source data

The base sector light list contains map-owned lights. CE's
`sector_light_list_fold` also builds the lists of primary and overlay
object lights from serialized fields through `light.c::sub_4D9590`.
A light has a tile, pixel offsets, ART mask, color and flags. The
mask and hotspot define the footprint and falloff. Palette-derived
samples are multiplied by the stored RGB tint. Floor lighting uses
the original 40 × 20 sample grid and separate additive and dark
channels. Point receivers and wall columns use the same masks and
their existing sampling geometry.

CE's object relocation path calls `sub_4DD020` for primary and overlay
lights. `sub_4DD320` and `sub_4DE4F0` move an existing light, maintain
its sector membership and invalidate both the old and new footprints.
`LF_OFF` excludes it from illumination, while the sentinel ART ID
represents the absence of a source. `light_start_animating` schedules
multi-frame masks through a light time event at integer `1000 / fps`.
`light_inc_frame` can apply the next frame's offsets and invalidates
the old and new bounds. This slice prepares single-frame positive
primary masks and updates their state and geometry. It has no lamp
intensity curve or fabricated flicker.

The probes read the original traits from the VFS-selected
`maps/k'na tha/805306380.sec` in `modules/Arcanum.dat`. Both templates
are scenery prototype 4040 and have `0x08000080` primary light flags.
Only the diagnostic placement and switch state are supplied by the
viewer. They do not instantiate the scenery or alter the original
object records.

| Probe | Source record | Original tile and offset | Stored mask and tint |
| --- | ---: | --- | --- |
| `--light-probe X Y` | 103, brazier flame | (793,781), (+2,+1) | `0x90180000`, `disk4x4.art`, (204,168,115) |
| `--nocturnal-probe X Y` | 38, torch flame | (794,774), (−14,−12) | `0x90880000`, `lr.art`, (210,183,98) |

The torch has scenery flags `0x24` (respawnable and nocturnal) and
its base record has `OF_OFF`. CE's sector fold and hour refresh turn
nocturnal outdoor sources off from 06:00 inclusive through 18:00
exclusive, while indoor tiles are exempt. The opt-in nocturnal probe
applies that rule to the receiving sector's original tile-type bit
and the existing fixed `--lighting` hour. At its original outdoor tile
(794,774), installed checks found it on at hours 0, 5 and 18, and off
at 6, 12 and 17. Its probe only demonstrates the emitter rule. The
authored torch sprite, respawn logic and a moving clock remain
separate. Nothing changes the original fixtures in an ordinary view.

The green pool in the Bates references is absent from the verified base
primary emitters. CE supports equipment-driven primary lights through
`item.c` and `critter_light_get`, and also supports tinted effect
overlays through `animfx.c`. The supplied snapshots do not determine
the exact mask, owner or effective overlay slot, and the historical
Bates save comparison did not establish an exact matching party state.
This milestone does not assign the green pool to an actor or reproduce
it with an invented tint. The prior vanilla/CE comparison establishes
the matching static candle geometry. There is no equivalent CE capture
for the manually moved diagnostic probe.

## State, redraw and resource reuse

Each probe retains a decoded mask and an independent tile, offset and
enabled state. A movement changes its original projection and sector
membership. The update rebuilds only the bounded probe grid and
resamples floor nodes, points and wall columns that intersect the old or
new footprint. The scene retains its raster order, sprite bounds and
static light grid. CPU and GPU use the same prepared shade data. A
changed shade is passed through the existing GPU instance and column
buffers. Texture contents remain resident and reusable.

The viewer requests a redraw on light changes. Frames with live
animated scenery simultaneously keep their original animation
deadlines. Frame selection continues to use the viewer's monotonic
epoch and the selected resource's original tick interval. The
pre-existing fixed-tick flag freezes only animation for deterministic
captures. Offscreen effects continue to use the existing idle wait.

## Shadow evidence and remaining work

Bates sector 67108865 light-list record 7 at (89,82) has `0x40`.
`shadow_apply` rejects `flags & 0x43`, so that record contributes to
ordinary illumination while being ineligible for directional shadows.
This behavior does not establish a symbolic name for the bit.
Off and dark lights also fail the check, and a light does not cast a
directional shadow for its own object.

Original shadow rendering needs each object's `OBJ_F_SHADOW` ART,
original sprite pivots and the indexed `art/light/shadowmap.bmp` lookup.
CE examines nearby light positions
in the lookup, rejects invalid indices and compares source grayscale
with indoor or outdoor ambient grayscale. It converts the nonzero
index into direction and distance frame, retains up to four candidate
directional shadows plus the applicable ambient shadow, and sorts the
directional frames. The world draw queues them in the shadow pass
before upright objects, using a color-tinted subtractive blit. It also
handles concealed critters, flat objects and wading. A faithful
extension must preserve these inputs, eligibility, ordering and
invalidation as light or object positions change. Advanced occluders,
character/overlay light state and animated light masks remain outside
this diagnostic. Milestone 47 traces the prepared palette pointers
through to the actual blit and verifies the indexed mask pass.
See [Original isometric shadows](ORIGINAL_SHADOWS.md).

## X270 measurements and visual checks

The stats below come from 100 Wayland light moves at 1280 × 720 with a
fixed 90 ms animation tick, using Intel HD Graphics 520 and Mesa 26.2.5.
Both probes were active. Each alternating move touched 51 light
receivers. Every measured update incurred zero sector reads, VFS reads,
ART decodes and texture uploads. Traced GPU completion includes a
completion wait. The sprite-pass timestamp includes light modulation
in the sprite shader, since there is no separate GPU lighting pass.

| Stage in ms | Median | p95 | Max |
| --- | ---: | ---: | ---: |
| Probe-grid preparation, Vulkan session | 0.011 | 0.024 | 0.036 |
| Affected receiver shading, Vulkan session | 0.124 | 0.173 | 0.214 |
| Main-thread light update, Vulkan session | 0.148 | 0.208 | 0.257 |
| CPU reference composition | 15.816 | 21.710 | 28.840 |
| Vulkan sprite pass | 1.846 | 2.505 | 3.761 |
| Vulkan traced completion | 3.687 | 4.544 | 5.701 |

The render loop remained close to the earlier roughly 3.3 ms warm
completion baseline, which used the K’na Tha court. Bates reported
63.2 MiB of resident i915 graphics memory in this run. Memory sampled
through i915 includes driver and presentation allocations.

Fixed-tick CPU and Vulkan screenshots agree exactly in the static,
single, moved and overlapping cases. The static Bates and original
K’na Tha flame screenshots match Milestone 45 pixel for pixel. A
lighting-enabled Wayland route also moves a probe from world X=383 to
X=384 across the populated K’na Tha seam, moves the camera onward,
and returns to identical home pixels. Resident resources and the
event-driven presentation path are preserved. The build, measurement
and visual artifacts are under `target/visual-checks/milestone-46/`.

## Live animation continuity

The fixed-tick comparison windows above intentionally used
`--animation-ms 90`. For the live verification, that option was
omitted and each Wayland session received 48 interleaved moves and
toggles. The trace records elapsed animation milliseconds on CPU and
Vulkan. Both sampled 27 consecutive 125 ms ticks in Bates and 38
consecutive 90 ms ticks in the court while the light states changed.
No observed tick jumped or restarted during those sequences. Light
changes can also request frames between animation boundaries. All
four sessions submitted zero extra Wayland buffers during the
settled offscreen intervals and resumed live playback on Home.

The portable regression checks that a move and an off event preserve
the same shared flame frame and deadline, with the old footprint removed
and no raster rebuild. The GPU regression compares the actual Bates fire
at 90 ms, 125 ms and its 2,625 ms wraparound while dynamic lights are
active. CPU and GPU pixels agree at each selected state.

Formatting, Clippy, asset-free default and windowed runs, debug and
release builds, 179 all-feature portable tests, 30 installed-data tests,
five hardware tests on each backend, and both 119/119 schematic audits
passed. The live traces and captures reside in
`target/visual-checks/milestone-46/live/`.
