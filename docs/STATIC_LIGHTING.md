# Original static lighting

The streamed world viewer accepts a fixed lighting hour after the viewport
arguments. Omitting it selects the original, unmodulated Arcanum-RS view.
The same option works with `art world-inspect` for light counts, diagnostics,
and preparation timings.

```sh
cargo run --locked --offline --release --features windowed -- art world "$HOME/Work/arcanum-ce-test" \
  --sectors "bates mansion lev 1" 1 1 1 1 101 83 1280 720 --trace --lighting 12
cargo run --locked --offline --release --features windowed -- art world "$HOME/Work/arcanum-ce-test" \
  --sectors "k'na tha" 12 12 12 12 798 786 1280 720 --trace --lighting 0
```

## Original format and provenance

A base `.sec` begins with a signed little-endian light count, then count
48-byte records, then 4,096 tile IDs. The original Bates Mansion Level 1
sector `67108865.sec` has nine light records and its tile array begins at
byte 436. K’na Tha's brazier court sector `805306380.sec` has 16. Both are
from `modules/Arcanum.dat`. The two original wilderness seam fixtures
have no serialized lights. CE's `sector_load_game` passes the direct
base sector through with global light coordinates. Its terrain-template
branch applies separate location handling.

| Offset | Verified fields |
| --- | --- |
| 0x00 | Legacy owner, 64 bits. Zero identifies an unowned sector emitter. |
| 0x08 | Global tile X and Y as two 32-bit words. |
| 0x10 | Signed screen pixel offsets X and Y. |
| 0x18 | Light flags. |
| 0x1c | Legacy light ART ID. |
| 0x20–0x22 | Individual red, blue and green bytes, in that order. |
| 0x23 | Alignment byte. |
| 0x24–0x2f | Saved tint, legacy palette and padding. CE clears the palette pointer and recomputes tint after reading. |

Light mask name keys come from bits 19–27 of light ART IDs via
`art/light/light.mes`. The renderer supports base-mask IDs at stored
frame and rotation zero. In the tested installation, the names and masks
resolve from `Arcanum2.dat`. Bates uses `largelight.art`, a 450 × 225 mask
with hotspot (225,112). The court uses `largelight.art`, `ul.art`, `ll.art`
and `lr.art`. The frame dimensions, hotspot and mask indices specify
extent, shape and falloff. The RGB bytes tint the sampled mask. There is
no radius or scalar intensity field.

Bates prepares all nine emitters. Record 7 at (89,82) uses `0x40`.
CE's `shadow_apply` excludes it through the `flags & 0x43` eligibility
check. `light_render_internal`, `sub_4D89E0`, and `sub_4DC210` still
sample it for illumination. `light_read` retains the original bit. This
establishes its effect on static rendering and shadow selection without
assigning an undocumented flag name.
All 16 court emitters prepare, including two with flag `0x10`, which take
their tint from outdoor ambient illumination. Supported flags cover off
(`0x01`), dark (`0x02`), indoor ambient tint (`0x08`) and outdoor ambient
tint (`0x10`). The loader caps preparation at 256 records per sector and
limits the footprint to 640 pixels on each axis from its owning tile.
This fits the existing streaming selector's overhang allowance. Unsupported
individual records leave the rest of the sector available.

## Verified surface render path

1. Read the winning `maps/<map>/mapinfo.txt` and its `LightScheme` through
   the VFS. `rules/lighting schemes.mes` names the original hourly table.
   Its rows store outdoor RGB followed by indoor RGB. Empty hours use the
   signed, integer per-channel step between known rows described by
   `light_scheme_parse`.
2. Project the emitter's world tile center. Add the stored pixel offset
   and subtract the mask's hotspot. Sample the original palette RGB at
   the 40 × 20 world lattice. Skip the color key, including matching RGB
   values at other palette indices. Tint each component with
   `floor(mask × tint / 255)`.
3. Sum overlapping positive and dark samples separately, saturating each
   RGB channel. Apply the floor formula
   `saturating_sub(saturating_add(ambient, positive), dark)`. Ordinary
   floor IDs use bit 8 to select indoor or outdoor ambient. Baked façade
   IDs use bit 25. The nine samples around the tile color its four
   39 × 20 quadrants.
4. Bilinearly interpolate within each quadrant, then multiply the source
   RGB before source-over. The deterministic native interpolation uses
   integer weights with a denominator of 780. The original software
   renderer uses incremental floating point, which can differ at
   rounding boundaries. The existing order, raster offsets, clipping,
   palette choice and alpha are retained.

The sector worker retains the imported emitter masks and converts them into
sparse scene-space samples for floors. The same mask sampler handles
arbitrary sprite sample positions. The ART cache shares the masks with
the existing resource lifetime rules. An active sector set merges the
prepared samples and caches the light multipliers once for all its draws.
Masks can reach adjacent sectors inside the selected world bounds.
Camera redraws reuse the cached multipliers. Variation across a floor row
now uses two sets of linear coefficients, with one truncation at the final
pixel, in place of recalculating the four bilinear weights at every pixel.
The framebuffer and Wayland presentation path retain their existing
storage and lifecycle.

## Upright sprites and walls

CE's `sub_4DC210` and `sub_4D89E0` combine the map's indoor or outdoor
ambient light with an exact mask sample at a scenery object's projected
tile center plus its instance offset. Critters enter the same general
point-sampling branch. Portals instead use stored rotation to choose
a ±20 horizontal and −10 vertical offset. Rotations 3 and 5 after
rounding to an odd rotation also move the sample to the next Y or X
tile, respectively.

Conventional walls use an even rotation pair and one sample per raster
column along a half-slope line on the wall plane. The line starts at
the effective mirrored or directional left edge. The light's original
tile decides whether it is on the eligible side. The software renderer
pads the sample width to an even number and advances the sample Y
after every second column. The renderer preserves the original
uniform-tint decision for that array.

| Wall rotation pair | Mask eligibility | Base ambient |
| --- | --- | --- |
| 0–1 | Light tile X ≥ wall tile X | Wall tile |
| 2–3 | Light tile Y > wall tile Y | Outdoor |
| 4–5 | Light tile X > wall tile X | Outdoor |
| 6–7 | Light tile Y ≥ wall tile Y | Wall tile |

CE's object path applies saturating additions and subtractions in light
list order. Sector and light lists both prepend their entries, giving
reverse sector and record traversal for these samples. Floors continue
to accumulate separate light and dark sums. TIG's constant-tint path
multiplies the selected palette. Its software column-tint path multiplies
palette RGB per screen column. The native compositor expresses these as
uniform and column multipliers on the existing RGBA sprite, before its
unchanged source-over operation. Palette choice, transparency, clipping
and draw order stay tied to the original raster.

Bates installed-data checks compare two sofa multipliers (101,107,125)
and (94,100,118), and the wall-face multiplier (74,80,98) for record 52,
against independently decoded originals. Synthetic tests isolate
wall-side selection, per-column sampling, clipping and alpha.

## Object lights, shadows and other limits

CE's `sector_light_list_fold` and `sub_4D9590` create lights independently
from object light and overlay-light fields. Scenery with the nocturnal
flag is subject to indoor/outdoor and 06:00–18:00 behavior. In the court,
flame records 103, 105 and 107 have blend `0x10` and light fields
`0x08000080`, ART `0x90180000` (`disk4x4.art`) and RGB (204,168,115).
The brazier bases 102, 104 and 106 have no instance override for those
three light fields. The 16 unowned sector emitters are distinct from
the warm flame lights.

CE's `shadow_apply` selects projected shadow sprites using a separate
shadow-map resource and object shadow IDs. Roof coverage and dynamic
visibility are separate. Objects with `OF_DONTLIGHT` or special blend
paths bypass normal tint in CE. Critter rendering, object-owned lights,
shadow sprites, light animations, saved lighting changes, terrain
templates, subsector lighting behavior and gameplay time progression
remain outside this slice.

## Wayland and installed-data verification

Bates pairs use (101,83) at 1280 × 720. Scheme 2 (*Lighting Dungeon*)
specifies indoor RGB (74,80,98) and outdoor RGB (0,0,0) at all hours.
The independently decoded local mask gives the center façade light
(107,113,131). The registered CE reference is the original Level 1 capture
at 07:38:47. The comparison reuses the verified Milestone 35 geometry
registration. The follow-up comparison uses the same camera and shows both the prior
floor-only image and current unified static rendering. Furniture and
interior walls now share the local illumination. The native sitting room
remains darker around the CE characters and fireplace. The exact
brightness contributions from character and object lights, shadows and
saved world state are outside this comparison.

The court uses (798,786) at 1280 × 720. Its scheme 1 outdoor ambient is
white at noon. The prior surface-only noon capture matched disabled
lighting because the supported positive contributions saturated to white. At hour
0, the center floor sample is (74,170,234), from original blue-green
sector masks. The registered CE geometry uses the 10:42:09 capture.
Its in-game hour could not be established from the screenshot, so the
nighttime native view establishes light placement and color behavior
without making a brightness comparison to that capture.

Both disabled Wayland captures match their Milestone 41 equivalents
exactly. Both lit Home returns match the initial lit images after movement
and resize. The follow-up captures, comparisons, tests and benchmarks are under
`target/visual-checks/milestone-42/upright/`. The idle trace
showed zero additional buffer submissions during the stationary checks.

## Floor-only baseline

The initial optimized CPU measurements on the ThinkPad X270 (i5-6300U)
at 1280 × 720 used CPU affinity 0,1, 20 warm-up frames and 250 timed frames
per view. The floor-only clear-and-compose medians were 12.43 ms in Bates
and 13.51 ms at the court. The disabled medians were 4.92 ms and 3.99 ms,
respectively. The unlit Milestone 41 seam baseline was 8.10 ms combined
clear, compose and pack.

## Unified lighting measurements

The follow-up reran the original benchmark and measured the new renderer
with the same viewport, movement route and sample count on the X270.
Each clear-and-compose result excludes packing and presentation. The
new floor-only control keeps the new draw path and disables tint for
upright objects. Worker light preparation and active-set shading
preparation are separate measurements. The latter builds the cached
surface tints and wall columns when the active sector set changes.

| View | Disabled median | Prior floor-only median | New floor-only control median | Unified median / p95 / worst | Worker light preparation | Active-set shading preparation |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Bates, hour 12 | 5.48 ms | 13.64 ms | 12.00 ms | 13.94 / 15.65 / 22.79 ms | 1.23 ms | 2.08 ms |
| Court, hour 0 | 4.38 ms | 14.58 ms | 11.18 ms | 11.48 / 13.67 / 30.00 ms | 5.10 ms | 2.39 ms |
| Wilderness, hour 12 | 5.60 ms | 5.45 ms | 5.66 ms | 6.21 / 9.79 / 13.60 ms | <0.01 ms | 0.11 ms |

The floor-only control accounts for most of the composition overhead.
The floor row optimization reduces the median of that control by about
1.64 ms in Bates and 3.40 ms at the court compared with the rerun of
the original floor-only renderer. Timing varies across batches. At
rest or during movement within an unchanged sector set, the light
preparation stages do no work. All warm benchmark runs recorded zero
VFS reads and ART decodes. Median unified packing took 1.81–1.94 ms.

The active draw multipliers and wall columns retain about 144 KiB in
Bates and 147 KiB at the court. The corresponding sparse light grid
payloads, including the sector and active copies, are about 21 and
35 KiB. The decoded ART cache retains the same masks it used in the
floor-only path. The output directory holds both benchmark runs and
Wayland traces with separate shading, composition, pack and submit
timings.
