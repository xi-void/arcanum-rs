# Original mobile poses and shadow casters

```sh
game="$HOME/Work/arcanum-ce-test"
cargo run --offline --locked --release --features gpu -- art world "$game" --sectors "bates mansion lev 1" \
  1 1 1 1 91 79 1280 720 --lighting 12 --light-probe 90 79 \
  --renderer vulkan --animation-ms 90 --trace
```

In the sector viewer, **I/J/K/L** move probe 1, **1** switches that
probe on or off, **3** switches its directional-shadow eligibility,
and **S** switches both ambient and directional mobile shadows on or
off. Use **Home** to restore the camera. Probe illumination stays
enabled when only eligibility changes. Omit `--animation-ms` for live
fire. The same scene is available with `--renderer cpu`.

[Milestone 49 adds the original K’na Tha fidget using this shadow pass](CRITTER_FIDGET.md).

## Original records and position

CE's `map.c::map_load_mobile` distinguishes original map object data,
its compiled mobile stream and save differences. The VFS has original
standalone version 119 `.mob` records in `modules/Arcanum.dat` for
Bates Mansion Level 1. The normal CE compiled name calculated from
this map, `maps/1 iry abvfanz frgno`, is absent in the selected VFS.
The native importer reads the authored standalone records, without
assuming an equivalent running CE or saved actor state. The original
sector has no instance record for these objects.

The sector worker automatically imports three audited record identities
when it prepares Bates sector 67108865. Positions, X/Y offsets, ART
IDs, prototype IDs and appearance flags come from the records.
Inherited appearance fields come from the VFS-selected prototype.
The parser validates version, permanent identity kind, field bitmap
and bounds of the appearance prefix. The remainder of the standalone
record contains unused gameplay fields. The importer uses a bounded
set of verified original record identities and handles unsupported
appearance states explicitly.

| Standalone record prefix | Tile | Current ART | Direction | Stored rotation | Hotspot |
| --- | --- | --- | ---: | ---: | --- |
| `g_0a68bdf7` | (80,81) | `0x28802820` | 5 | 5 | (15,74) |
| `g_931574ed` | (87,105) | `0x28800820` | 1 | 7, reflected | (15,76) |
| `g_cdc859eb` | (91,79) | `0x28801820` | 3 | 5, reflected | (12,74) |

All three use prototype 17088, inherited scale 100, offsets (0,0),
standing frame 0, palette 2 and original shadow ART `0x90300001`.
None requires an active mobile-owned light. The current ART bits and
CE `name.c::name_resolve_path` name the male human city-dweller
unarmed stand at `art/critter/hmm/hmmcdxaa.art` in `Arcanum1.dat`.
TIG `tig_art_frame_data` reflects directions 1–3 from 7–5 by default.
The reflection reverses pixels, uses `width - hotspot_x - 1`, and
negates the horizontal movement offset. Normal isometric placement
subtracts that hotspot from the projected tile center plus the
instance offsets. Scene lighting uses the existing point receiver,
consistent with the non-wall/non-portal path in `light.c::sub_4DC210`.

## Shadow state and lifetime

The mobile shares the upright sprite order with scenery, walls and
portals. The original cast masks are inserted before that pass and use
the mobile's unmodified world anchor. The original map and supported
object emitters and diagnostic probes supply effective tints, offsets
and eligibility. An off probe is omitted. Active probes with `0x40`
still contribute light. Sources dimmer than the ambient, or beyond
the original lookup bounds, cannot contribute a directional mask.
Re-enabling a probe can therefore leave a caster unchanged until
the source moves closer or increases in effective brightness.
[Milestone 47 shadow algorithm](ORIGINAL_SHADOWS.md)

The bounded resource bank prepares 32 directions × 7 original frames
on the worker, shared across the three mobiles. A probe move or toggle
reselects these frames from memory and updates the existing light
receivers. The world raster order and flame epoch stay intact. Vulkan
uploads a direction when the renderer first encounters it, then
reuses the same texture identity across repeated states and resizes.
An invalid lookup sample leaves the affected caster with its ambient
mask and emits a diagnostic. Full mobile streams, actor AI, pose
changes, other critter families and gameplay light ownership remain
outside this slice.

## Visual verification

The dining-room mobile at (91,79) exposes more of the authored mask
than the kitchen example. At fixed animation tick 90, the
directional eligibility switch changes 1,596 pixels in a 77 × 38
area around that mobile. The light remains enabled throughout.
Disabling all mobile shadows changes 2,003 pixels in the scene.
Reactivating the moved probe restores the same pixels as before its
deactivation. The CPU and Vulkan Wayland captures match exactly in
all nine tested states, including the 960 × 560 resize and return
to the 1280 × 720 home camera. Both fixed-tick viewers submit zero
extra buffers while idle.

The supplied CE kitchen reference registers against the stoves,
pots and worktables using the Milestone 47 alignment. The authored
native mobile appears behind the correct table. The CE screenshot
shows a different actor state and has no equivalent standing
mobile at that location. Exact actor and shadow pixel equivalence
with CE remains unverified. Full captures, the enlarged dining-room
detail and the registered kitchen comparison are under
`target/visual-checks/milestone-48/`.

## X270 performance

Intel HD Graphics 520 with Mesa 26.2.5, 1280 × 720, hour 12 and a
fixed 90 ms animation timestamp. Each phase used the same 80 probe
move events, alternating between two prewarmed positions. Times are
median / p95 / worst. This comparison uses key-driven rendering for
both conditions.

| Stage | Shadows enabled | Shadows disabled |
| --- | ---: | ---: |
| CPU clear and compose, ms | 13.623 / 17.215 / 17.836 | 13.847 / 16.029 / 20.849 |
| Shadow refresh in Vulkan session, µs | 4.779 / 7.359 / 10.366 | 0.295 / 0.466 / 0.972 |
| Vulkan sprite pass, ms | 1.724 / 2.110 / 2.543 | 1.788 / 2.448 / 3.160 |
| Vulkan traced completion, ms | 3.630 / 4.421 / 6.985 | 4.761 / 7.048 / 17.962 |

The disabled phase had a higher presentation tail in this run. The
measurements isolate shadow selection cost and show no material
change in sprite-pass median. They do not establish a speedup.
Completion includes the Wayland presentation path.

Cold mobile preparation took 12.25 ms in the release inspection.
The first mobile and its shared prototype took 2.48 ms. The two
later poses took 56 and 35 µs. The original shadow bank took 9.67 ms
and retains 3,572,364 RGBA bytes. Each previously unseen mask on the measured route uploaded
8.3–21.0 kB in 0.115–0.239 ms on the host. Both warm sequences incurred zero sector reads, VFS reads, ART
decodes or texture uploads. i915 reported 64,676 KiB (63.2 MiB)
resident for the measured window, including presentation and
driver allocations.

The focused installed-data test verifies stored orientations,
reflected pixels and hotspots against the original frame, and tests
that a tint change retains the same mask texture identity. Hardware
tests check CPU/Vulkan and CPU/GLES parity across movement,
on/off state, eligibility, clipping and reset, with unchanged
warm resource counts.

Live Wayland sessions changed directional eligibility 24 times and
moved the probe through the selection path. CPU sampled 28 consecutive
125 ms fireplace ticks and Vulkan sampled 27, with no skipped tick
or backward clock jump in either measured interval. Panning offscreen
stopped buffer submissions, and Home resumed animation.
