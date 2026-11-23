# Original isometric shadows

## Source behavior

CE's `light.c::shadow_apply` selects a sprite shadow from the caster's
inherited or overridden `OBJ_F_SHADOW` ART. The invalid ID is
`0xffffffff`. The normal isometric object pass excludes off, destroyed,
hidden and flat objects. Concealed critters fail shadow selection too.
Ordinary wall and furniture sprites do not become casters just because
they cover a light. This path projects authored masks over surfaces
already drawn. It does not trace rays against scene geometry.

1. Project the caster's tile center and add its stored pixel offsets.
   Center the original indexed `art/light/shadowmap.bmp` at that point.
   Look up each candidate using its projected light center and offsets,
   independent of the light ART hotspot or footprint.
2. Traverse the nearby sector light lists in CE order. Map-owned and
   object-owned lights participate through the same lists. A light owned
   by the caster or with any of bits `0x01`, `0x02` or `0x40` set
   cannot produce a directional shadow. Bit `0x40` does not disable its
   ordinary illumination. The source must also be brighter than the
   caster's indoor or outdoor ambient grayscale.
3. A zero lookup index produces no shadow. For a valid nonzero index
   `i`, let `n = i - 32`. The direction is `(n / 7 + 16) % 32` and the
   frame is `n % 7`. Direction picks the suffix `_s{direction / 8}`
   through `a_name_light_aid_to_fname` and the ART rotation
   `direction % 8`. Keep the first four eligible candidates, then sort
   them by frame, keeping their original order at equal frames.
4. In 32-bit RGB, TIG grayscale is `(77*R + 150*G + 28*B) / 255`.
   Let `s = trunc((light_gray - ambient_gray) * 0.4)`. Directional
   tint is `s - frame * (s / 7)`. CE also draws the caster's ambient
   shadow at direction 2, retaining the base ART frame, with tint
   `trunc(ambient_gray * 0.4)`. It skips the ambient draw if the nearest
   accepted frame is 0 or 1. Disabling directional selection in CE
   leaves the ambient shadow enabled.
5. `object.c::object_draw` queues these masks after flat objects and
   before upright objects. `sub_443620` anchors each mask using its ART
   hotspot at the caster center plus offsets. CE also adjusts the
   shadow rectangle for scale, shrinking and wading, and dims a wading
   shadow. Those cases require the corresponding original actor state.
   TIG's 32-bit indexed `COLOR_CONST | SUB` blit skips index zero and
   uses `dst = max(dst - floor(src * tint / 255), 0)` per channel.
6. `shadow_init` creates seven tinted palettes and `shadow_apply`
   stores their pointers on shadow records. The examined isometric
   draw path never uses those pointers or sets
   `TIG_ART_BLT_PALETTE_OVERRIDE`. TIG therefore uses its normal
   cached light ART palette, which the ambient palette callback
   leaves unmodified. No extra palette tint enters this blit.

The checked 383 × 183 bitmap resides in the original archive. Its rows
are padded to 384 bytes and stored bottom-up. Three visible samples are
index 30, which would give an invalid negative frame in CE. The native
lookup reports a sampled out-of-range bin explicitly. Zero remains a
normal empty result. The four original `med_critter_s*.art` files have
eight stored rotations and eight frames per rotation. The directional
lookup reaches only frames 0–6. These resources use palette slot zero.

CE clears per-object cached shadow results when a caster changes or
lighting invalidates the object's drawable region. The original moving
light path updates sector membership before invalidating old and new
bounds. A mask changes when world-relative light position, eligibility
or ambient changes. Camera translation by itself never changes it.

## Original reference inventory

Bates Mansion Level 1 sector 67108865 has 268 walls, 19 portals and 81
scenery instances. The effective shadow ART on these records is absent.
The reference K'na Tha court (805306380), neighboring sector 805306381
and seam sectors 939524101–102 likewise contain no verified static
caster. Bates light record 7 at (89,82), offset (−12,+13), tint
(94,94,94), has bit `0x40` and remains an illuminator.

The original mobiles for Bates are separate from the sector list.
The original NPC prototype generator explicitly sets light ART
`0x90300001` as the shadow. That caster family is present on the
installed NPC prototypes. The supplied gameplay screenshots contain
actors and effects whose full placements and states are absent from
the native static scene. The base scene cannot establish exact
pixel equivalence for those actor shadows.

## Verified native slice

The base kitchen mobile `g_0a68bdf7_1d83_4ec4_b208_b4c36b06bf87.mob`
in `modules/Arcanum.dat` is at tile (80,81), pixel offset (0,0).
Its current sprite is `0x28802820`. Prototype 17088 supplies shadow
`0x90300001` and scale 100. The indoor ambient at hour 12 is (74,80,98).
Map light record 2 at (81,82), offset (+13,−6), has flags zero and
tint (254,213,163). Relative to the caster, its projected center is
(+13,+34). It samples index 139 and selects direction 31, frame 2,
grayscale 41. The ambient selection uses direction 2, frame 0,
grayscale 32. The lookup and both masks come from `Arcanum2.dat`.

The native loader verifies the bitmap, resolves the light MES and loads
the requested original rotation through the existing ART decoder.
`WorldStream::visible_assets_with_shadows` accepts prepared masks in
world coordinates and inserts them between the flat and upright
passes. CPU and GPU share the sprite, viewport culling and
grayscale. The GPU uses reverse subtraction with unit RGB weights
and preserves destination alpha. An `Arc` holds each mask through
resizes and redraws, and warm draws reuse the resident textures.
Its input is independent of the fire animation clock and probe
illumination. A caller can reselect the masks on a source move or
toggle without changing sprite data or reconstructing the scene.

The installed regression uses the original mobile and map data.
The diagnostic draws the masks in the full original kitchen at the
mobile's recorded placement. It omits the mobile sprite and its
gameplay state. The ordinary viewer continues loading only supported
static objects and effects. Automatic caster discovery, mobile
appearance and visibility state, cross-sector caster invalidation,
and scale, shrink and wading variations remain separate work.
Roofs and dynamic occluders are outside this slice.

## Wayland and hardware verification

Local captures under `target/visual-checks/milestone-47/` use the
same scene center (81,81), fixed animation time 90 ms and hour 12.
Both backends agree across all captured pixels at 1280 × 720 and at
the clipped 600 × 400 size. Panning, resizing and returning to the
same camera reproduces the original pixels. Idle CPU and Vulkan
sessions submit zero extra Wayland buffers. The original Bates candles
and K'na Tha flames also match their Milestone 46 captures exactly.

The kitchen table covers the ambient mask and most of the directional
mask. Enabling the directional mask changes 13 visible pixels between
(684,314) inclusive and (704,329) exclusive in the full capture. The
detail comparison enlarges the unchanged screenshot crops with
nearest-neighbor scaling. The independently supplied CE kitchen view
registers to the same stoves, pots and furniture at 1.8 CE pixels per
native pixel. After downsampling by 1.8, the geometric alignment
translates native coordinates by (−299,+38). The CE mobile pose and
lighting state differ, so this comparison establishes the kitchen
geometry and mask placement context. Exact CE shadow pixels remain
unverified.

### X270 measurements

Intel HD Graphics 520, Mesa 26.2.5, Vulkan Wayland surface, fixed
1280 × 720 camera. Each mode has 80 warm redraws. Values are
median / p95 / worst in milliseconds.

| Stage | Both masks omitted | Two prepared masks |
| --- | ---: | ---: |
| CPU clear and compose | 14.952 / 17.530 / 34.057 | 14.933 / 17.190 / 34.389 |
| Vulkan command generation | 0.335 / 0.538 / 2.348 | 0.341 / 0.501 / 0.754 |
| Vulkan sprite pass | 1.410 / 1.558 / 1.800 | 1.420 / 1.559 / 1.651 |
| Vulkan traced completion | 8.453 / 9.185 / 10.146 | 8.510 / 9.425 / 9.958 |

Selection took 81–94 ns per request across two local runs, each
repeating the same nine map-light candidates 20,000 times. Initial
VFS reads and decoding of the two masks took 1.79–2.34 ms. Their
combined RGBA payload is 19,544 bytes. No unchanged mask texture
was uploaded during either warm sequence. Total i915 graphics
residency for the window was 68,772 KiB (67.2 MiB), including the
driver and presentation buffers. The measured Vulkan sprite-pass
median changed by 0.010 ms. The 8.510 ms traced completion median
is higher than the Milestone 46 3.69 ms baseline. The earlier
baseline used a different camera and key-driven redraws. The
same-session omitted/enabled comparison above holds the camera
and consecutive FIFO presentation workload constant.

The original installed case passes byte-exact CPU/Vulkan and
CPU/GLES checks, including clipping and returning home. Portable
tests cover light eligibility, order and cap, RGB rounding, malformed
lookup data, projection, overlay order, transparency and dynamic
light updates alongside animated flames. The next bounded slice
is one visible original mobile using its stored frame and validated
visibility fields, then re-selection of that caster's masks when
a verified light changes sector, position or enabled state.
