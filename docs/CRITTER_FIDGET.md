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
