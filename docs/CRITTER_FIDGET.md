# Original critter fidget playback

The sector viewer loads one audited rabbit at its original K'na Tha
placement. It starts in the authored standing pose. **F** requests one
fidget in a live window. **Comma** and **Period** turn the rabbit one
direction without moving its anchor or restarting the sequence. **S**
toggles its original shadows. The existing camera and light controls
still work.

```sh
game="$HOME/Work/arcanum-ce-test"
cargo run --offline --locked --release --features gpu -- \
  art world "$game" --sectors "k'na tha" 6 12 6 12 420 814 1280 720 \
  --lighting 12 --renderer vulkan
```

For a deterministic frame, add `--fidget --animation-ms 300`.
`--fidget` starts one pass at the shared viewer epoch. With a fixed
timestamp, F is disabled and the direction controls still work.

## Original data and behavior

All eight audited standing NPCs in Bates Mansion Level 1 use
one-frame resources. The three already rendered mobiles use
`hmmcdxaa.art`. The other five use `hmmcmxaa.art`. Both files have
6 fps headers and one frame per direction. Native Bates mobiles
stay in their original static poses.

The K'na Tha mobile is
`maps/k'na tha/g_5f490f51_d2d1_4e9d_ad68_07de7a74d824.mob` in
`modules/Arcanum.dat`. Its original placement is (420,814) in sector
805306374, with no X/Y pixel offset. Prototype 28344 supplies scale
100 and shadow ART `0x90300001`. Current ART `0xc4801810` selects
monster species 9, unarmored, weapon and shield free, STAND
(animation ID zero), direction 3, frame zero and palette 1.
CE's `name_resolve_path` resolves that species through
`art/monster/monster.mes` to `art/monster/bun/bunuwxaa.art` in
`Arcanum1.dat`. The effective critter flags are `0x2008`, with
secondary and spell flags zero. The authored record has no
`OCF2_AUTO_ANIMATES` bit.

The resource has eight stored directions, ten frames per direction
and 10 fps. The selected frame uses palette slot 1 and the usual
transparent index zero. TIG's frame data maps directions 1–3 to
mirrored stored directions 7–5. Reflection reverses rows, uses
`width - hotspot_x - 1` for the pivot and negates X movement offsets.
Direction 3 frame zero has the reflected hotspot (10,10). The
ten frames have changing pivots and zero movement offsets across
all eight directions. CE applies authored movement offsets during
`object_inc_current_aid`. This stationary slice rejects a sequence
with any nonzero movement offset.

CE's `anim_fidget_timeevent_process` periodically chooses an
eligible standing actor. The normal event schedules another
selection after 4,000 ms. `sub_42CDF0` starts the selected ART
at frame zero and uses integer `1000 / fps` between steps.
`sub_42CF40` ends the goal at the last frame and the goal's
cleanup `sub_42B090` restores STAND frame zero. The rabbit therefore
consumes 100 ms per frame and returns to its base pose at 1,000 ms.
The viewer reproduces that finite playback and leaves original
random target selection, gameplay prerequisites and automatic
NPC goals out of scope. Stunned, paralyzed and stoned variants
are excluded from the preview.

The shadow is selected from the original `OBJ_F_SHADOW` using
the projected object anchor, ambient color and eligible lights.
CE's shadow pass has no input from the current sprite direction
or frame. The rabbit's frames leave its world offsets unchanged,
so turning or fidgeting leaves its shadow geometry unchanged.
The existing shadow pass remains before upright objects. A
light change may select a different mask independently of playback.

## Scheduling and resource lifetime

Both fidgets and environmental flames use the existing elapsed
timestamp and viewport deadline query. The earlier visible deadline
wins when both are present. Turning, light updates and camera
changes preserve the fidget start timestamp. Starting a fidget
schedules the first visible transition without presenting the
unchanged frame zero again. A finished or fully offscreen fidget
schedules no frames. The worker prepares each directional asset once,
and CPU and GPU use the same decoded frame and hotspot.

The stationary Bates mobiles and the verified rabbit occupy different
maps. The live Bates check exercises the original fireplaces with
the stationary mobiles. A portable scene regression exercises
simultaneous fire and fidget deadlines. The currently supplied CE
captures do not show the rabbit at a known phase. The original
K'na Tha sector's existing partial static-object import limitation
remains visible in `art world-inspect`. Record 558 uses a
static-object field combination the existing importer rejects. The
floor and light lists still load and retain their original placement.

## Native captures and frame verification

Wayland screenshots are under `target/visual-checks/milestone-49/`.
At 100, 300 and 600 ms, the original fidget changes respectively
372, 402 and 377 pixels compared with the standing screenshot.
All changed pixels remain within the rabbit raster. The authored
frame 9 at 900 ms already has the same rendered pixels as frame
zero. At 1,000 ms the standing frame is restored and the deadline
sequence ends. Turning at 300 ms changes the sprite while retaining
the shared anchor. Disabling shadows at that time changes 1,156
pixels beneath the rabbit, with no change to its pose.

CPU and Vulkan captures match byte for byte at 300 ms in the
authored direction and both measured turns, after resizing to
960 × 560 and upon returning Home. Hardware tests additionally
compare CPU and Vulkan or GLES offscreen, with dynamic light
moves and fixed time. The live Wayland sessions present exactly
70 frames over seven warm fidgets per backend. A live light
change keeps the same animation epoch. Finished and offscreen
fidgets leave the Wayland buffer count unchanged. The separate
Bates capture samples 28 consecutive 125 ms fireplace ticks
with the stationary mobiles in the scene.

## X270 performance

Intel HD Graphics 520 and Mesa 26.2.5, 1280 × 720 Wayland window,
hour 12. Each backend used seven warm ten-frame passes. Timings
are median / p95 / worst in milliseconds.

| Stage | Timing |
| --- | ---: |
| CPU clear and compose | 4.112 / 4.999 / 5.478 |
| Vulkan command generation | 0.120 / 0.191 / 0.241 |
| Vulkan sprite pass | 0.647 / 0.813 / 0.873 |
| Vulkan traced completion | 2.398 / 2.725 / 4.057 |

Preparing the 80 direction/frame combinations after the base
sprite took 0.253 ms and retains 173,204 bytes of RGBA data.
The original shadow bank took 12.81 ms and retains 3,572,364
RGBA bytes. A separate headless probe measured roughly 50–53 ns
per single-caster visible deadline query and 33–65 µs per
viewport traversal through the visible rabbit draw. The latter
includes selecting and clipping earlier scene draws.

Each first-seen animated frame in direction 3 uploaded about
1.8–1.9 kB and took 0.099–0.145 ms in the host upload stage.
Warm Vulkan playback incurred zero uploads. The warm animation
and light route did not read sectors or VFS resources or decode
ART again. i915 reported 62,552 KiB (61.1 MiB) graphics residency,
including presentation and driver allocations.

## Original walking playback

Use the same K'na Tha sector command above with `--walk`. The viewer
prepares the route while loading the sector and opens in the serialized
standing pose. **W** starts the walk and **R** resets to the original
tile and enables another replay. Pressing W during or after a route
keeps its progress until reset. With `--walk --animation-ms <ms>`,
the same route starts at the fixed clock epoch and shows its state at
that timestamp. The camera, light, shadow and fidget controls retain
their bindings. While the rabbit is walking, fidget and turn input
cannot override its current walk pose. The stopped rabbit can fidget or
turn at its final position.

### Source and route

CE's `AGbeginAnimMove` selects WALK (ART animation ID 1), sets the
first path rotation and starts frame zero. The rabbit's original
prototype and mobile have the same identity, position, palette and
shadow described above. `name_resolve_path` combines BUN, unarmored
`uw`, shield-free `x`, weapon-free `a` and WALK `b` to resolve
`art/monster/bun/bunuwxab.art`. The winning `Arcanum1.dat` resource
has flags `0x02`, six frames in each of eight directions, four
palettes and an 8 fps header. This walk uses palette 1. Frame zero
in direction 3 has hotspot (23,29), mirrored from the stored
rotation 5. Direction 5 uses that rotation directly. Reflection
negates the incoming X offset and reflects the hotspot across the
frame width. Palette index zero is transparent.

In the installed base prototype, version 119 sparse field
`OBJ_F_CRITTER_STAT_BASE_IDX` supplies Dexterity 17 at logical
index 1. The mobile does not override that entry. For this
unmodified rabbit, CE's `stat_base_get` and `stat_level_get` yield
Speed 17. `sub_437990` computes
`17 + 17 × (30 − 17) / 30 = 24` with integer division for an
ordinary monster walk. `sub_42EE90` uses integer `1000 / 24`
and its 30–800 ms clamp, giving **41 ms** between frames. The
header's 8 fps would correspond to 125 ms and is not this
mobile's effective movement interval.

`object_inc_current_aid` applies the *incoming* frame's signed
offset. `location_normalize` converts the projected pixel point
back into a tile plus a pixel remainder while preserving its
projected position. Tile origins use the existing
`(40 × (Δy − Δx), 20 × (Δy + Δx))` transform. CE tests for
arrival at the next tile's center on subsequent frames. At a
turn it clears the remainder and rotates the ART ID saved
before the increment. At the end it restores STAND frame zero
with the final rotation and clears the remainder. The preview
uses the same steps and center corrections. It has no
interpolation between authored frames.

The diagnostic path is CE rotations **3, 3, 3, 5** relative to
the original spawn, or three Y+ steps and one X+ step. It remains
within source sector 805306374 (6,12). It assumes those outdoor
floor cells are clear for this preview. Gameplay collision,
pathfinding, terrain effects and NPC decisions are outside this
test. No scenery is moved or added.

| Time | Tile | Pixel remainder | Anchor relative to (420,814) | Pose |
| ---: | --- | --- | --- | --- |
| 0 ms | (420,814) | (0,0) | (0,0) | WALK 3 frame 0 |
| 164 ms | (420,815) | (−18,−8) | (22,12) | WALK 3 frame 4 |
| 246 ms | (420,815) | (3,2) | (43,22) | WALK 3 frame 0 |
| 697 ms | (420,817) | (0,0) | (120,60) | WALK 5 frame 4 |
| 820 ms | (421,817) | (14,−7) | (94,73) | WALK 5 frame 1 |
| 943 ms | (421,817) | (0,0) | (80,80) | STAND 5 frame 0 |

The existing world draw key is recomputed from the normalized
tile and pixel offsets. The moving draw joins the shared ordered
upright pass, using the selected frame's hotspot and actual
bounds. The original shadow bank, eligible-light lookup and
subtractive pass use the same moving world anchor. The shadow
selection takes the anchor, light state and ambient scheme and
has no critter animation frame or facing input. The route
resamples point tint and shadow choices at each changed step.
Light edits update those choices without restarting playback.
The worker prepares the 12 directional walk frame instances
through the existing cache. A backend may upload a texture
when first encountered. Warm playback reuses it.

The route is sampled on the existing viewer clock. Only visible
sprite or shadow changes add walk redraw deadlines. Flame
deadlines still compete independently. The rabbit and Bates
fireplaces are on different original maps. A synthetic scene
tests concurrent deadlines and a late fidget after stopping.

### Verification

The native captures show the serialized stand, both walk facings,
an intermediate position and the final stand. At 246 ms the
original shadow changes 1,184 pixels when toggled. CPU and Vulkan
captures match exactly at the sampled timestamps, after panning and
resizing, and after returning Home. OpenGL is also checked at the
turn timestamp. Original geometry outside the changing mobile and
shadow footprint is pixel-identical.

The available CE captures do not reproduce this rabbit's diagnostic
route at known timestamps. The numeric comparison is against the
original mobile, prototype, ART records and the cited CE movement
functions. The original walking benchmark ran before the adjacent
scenery was restored by the [generic sector
audit](MULTI_SECTOR_WORLD.md#milestone-51-generic-reconstruction-audit).
Occlusion against adjacent
scenery is covered by a portable overlap fixture that checks both
draw order and final pixel colors. A separate live Bates run checks
the original fireplaces. Synthetic and hardware fixtures cover
the independent animation deadlines and lighting updates without
requiring that the same camera see two different original maps.

### ThinkPad X270 measurements

The release viewer ran at 1280×720 on the i5-6300U and Intel HD
Graphics 520 under native Wayland, with Mesa 26.2.5. Each backend
warmed the route once, then timed seven live routes and resets
with a loaded diagnostic light. Each sample contained 168 walk
poses and 175 rendered frames. A separate headless probe measured
4,800 warm selections without a diagnostic emitter. Its median
traversal of the visible draw stream was 30.78 µs. The single
moving draw's insertion, retint and shadow refresh took 0.45 µs
median in that probe. The original walk bank took 0.24 ms to
prepare in the same run and added 39,352 bytes of RGBA pixels.

| Wayland measurement | Median | p95 | Maximum |
| --- | ---: | ---: | ---: |
| CPU walking update | 6.57 µs | 10.86 µs | 28.09 µs |
| Vulkan host walking update | 6.86 µs | 19.35 µs | 35.93 µs |
| CPU composition | 7.63 ms | 23.03 ms | 49.24 ms |
| Vulkan command preparation | 0.158 ms | 0.250 ms | 1.859 ms |
| Vulkan sprite pass timestamp | 0.738 ms | 1.583 ms | 1.694 ms |
| Vulkan traced frame completion | 2.67 ms | 5.07 ms | 12.77 ms |

Host submission and the Vulkan presentation call took 0.162 ms and
0.193 ms median. Completion includes host work and a traced GPU
completion wait. Physical scanout time is outside the measurement.
The i915 graphics resident counter was 62,552 KiB after replay.
All 24 states were drawn in each timed replay. Warm routes and
light moves did no sector reads, VFS reads or ART decodes. GPU
replays uploaded zero textures after warming. Three-second
Wayland checks after stopping, resetting and starting an entirely
offscreen walk submitted zero additional buffers on both backends.
