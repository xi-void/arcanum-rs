# Original animated environmental fire

The streamed `art world` viewer renders original additive scenery with
the CPU compositor and the optional Vulkan and OpenGL backends.

```sh
game="$HOME/Work/arcanum-ce-test"
cargo run --offline --locked --release --features gpu -- \
  art world "$game" --sectors "k'na tha" 12 12 12 12 798 786 1280 720 \
  --lighting 0 --renderer vulkan --trace
```

Pass `--animation-ms 90` to freeze playback at 90 ms from the serialized
starting frame. Use `--renderer cpu` or `--renderer gles` at the same tick
and viewport for comparison. For Bates Mansion, use
`--sectors "bates mansion lev 1" 1 1 1 1 91 81 1280 720 --lighting 12`.
The older `--view` and room previews remain static fixtures.

## Original records and timing

Both winning sector files come from `modules/Arcanum.dat`. Each effect is
a scenery object (type 3). The instance records override current ART,
placement offsets and blit flags. The prototypes provide the 100% scale,
white blit color and alpha 255.

| Map sector | Flame record and byte span | World tile | Instance pixel offset | Prototype and inherited ART | Stored and effective ART |
| --- | --- | --- | --- | --- | --- |
| K’na Tha 805306380 | 103, 26993–27168 | (793,781) | (+2,+1) | 4040, `0x400003c0` | `0x40100540` |
| K’na Tha 805306380 | 105, 27264–27451 | (798,781) | (+2,+1) | 4040, `0x400003c0` | `0x40100540` |
| K’na Tha 805306380 | 107, 27551–27726 | (803,781) | (0,+3) | 4040, `0x400003c0` | `0x40100540` |
| Bates 67108865 | 19, 20077–20207 | (88,78) | (−13,−6) | 4041, `0x40000400` | `0x40200400` |
| Bates 67108865 | 27, 20801–20931 | (94,78) | (−10,−6) | 4041, `0x40000400` | `0x40200400` |

The K’na Tha bases are records 102, 104 and 106, all at the corresponding
flame's world tile. They use prototype 4036 and instance ART
`0x407002c0` (`brazurelarge.art`). The normal upright object key
places each base before its own overlay. All geometry uses the existing
projection and per-frame ART hotspot. For example, the leftmost projected
flame anchors at (240,363) at the reference camera and viewport; frame 0
has hotspot (40,155), so its upper-left pixel is (200,208).

The winning flame filenames are `art/scenery/brazurefire.art` and
`art/scenery/campfire.art` from `Arcanum2.dat`. Both request frame, rotation, and palette
zero, without mirroring. Both have 20 frames with varying dimensions and
hotspots. `brazurefire.art` starts at 83 × 122, hotspot (40,155),
and its next frame is 88 × 117, hotspot (44,154).
`campfire.art` starts at 56 × 62, hotspot (22,61).
Both store action frame 19. All 40 movement offsets are zero.
The stored rates are respectively 11
and 8 FPS. CE's `object.c::sub_43F710` auto-animates multi-frame
scenery unless its flags disable it. `anim.c::AGbeginAnimLoopAnim`
uses integer division `1000 / fps`, giving 90 ms and 125 ms per frame
and loops of 1,800 ms and 2,500 ms. `AGupdateAnimLoopAnim` advances the
current ART ID through `object_inc_current_aid`. The native viewer
picks the indexed frame from a monotonic elapsed time and the serialized
initial frame. The viewer starts matching stored frames on a shared
epoch. CE screenshot timing, distance-gated animation, and sound playback
remain outside that clock.

## Compositing and illumination

The court flames have object flags `0x00104430` and scenery flags
`0x20`. Bates uses `0x00104030` and `0xa0`, which also includes
the medium sound flag. Both have standalone blit flag `0x10`. CE's
`object_setup_blit` selects additive blending and the original
palette on the software path. TIG's indexed blit in `art.c` skips
index zero; `tig_color_add` in `tig/color.h` clamps each
destination-plus-source channel to its maximum. The observed palette
zero key is blue (0,0,255), with transparency determined by the decoded
index. The CPU and GPU paths add source RGB for nontransparent texels
in original object order. The Vulkan and OpenGL paths use ONE + ONE
on a byte-domain UNORM target. Additive objects bypass ambient object
tint.

CE's `sector_light_list_fold` and `light.c::sub_4D9590` attach the
primary light to the owning object's location and pixel offsets. In
the court the three instances supply flags `0x08000080` (primary
object-owned channel and modified), ART `0x90180000`, and RGB
(204,168,115). `disk4x4.art` is 293 × 147 at hotspot (145,72) and
has one frame, so `light_start_animating` does not schedule it.
These lights are separate from the sixteen sector-owned lights and
from the ceramic base objects. The prepared masks enter the same static
light-grid and object-sampling paths. The two Bates fireplaces also use verified one-frame primary lights. No lighting rebuild is
required between flame frames. Other object-light flags, animated
light masks, day/night-dependent scenery, and overlay light slots
remain outside this slice.

## Resource lifetime and redraws

Sector preparation resolves the loop before publishing its raster.
All its indexed ART frames are pinned by the sector clip and shared
through the bounded decoded-frame cache. GPU frames upload on their
first visible use and follow the existing residency and eviction
policy. Drawable bounds account for every frame's hotspot and
dimensions. Individual frames are still clipped against the viewport.
The viewer schedules a Wayland redraw for the next visible animation
change. Fixed-time captures and scenes without visible effects use the
ordinary idle wait. Pan, resize, and completed sector loads continue to
request redraws through the shared viewer.

## Fidelity and performance on the X270

The registered CE comparison uses the original K’na Tha screenshot from
10:42:09. The scale and translation were carried forward from the base
registration using the exposed brazier bases and water-channel geometry.
The native flame roots sit in the corresponding bowls. The CE screenshot
has different flame silhouettes and brighter, differently colored
paving. The capture has no synchronized animation clock or verified
lighting hour. Creatures, shadows and nocturnal effects are also outside
this comparison. The original bases, world anchors and floor geometry
are covered by the installed-data tests.

The native 1280 × 720 Wayland captures at 90 ms produce identical RGB
on CPU, Vulkan and OpenGL. Returning home after a resize at the same
tick produces identical pixels. Hardware readback additionally compares
0, 90 and 1,890 ms; the latter two match through loop wraparound and
the warm wrap uploads zero textures. The captured live sequence
contains seven distinct captured images out of eight samples.

The matched timing run used Intel HD Graphics 520 and Mesa 26.2.5.
At 1280 × 720, Vulkan showed 67 measured redraws in six seconds, with
all 66 intervening sampled ticks spaced by 90 ms. Bates rendered
48 redraws in six seconds, consistent with its 125 ms period. The
stationary K’na Tha seam and the fixed-time court submitted no extra
Wayland buffers in the observed idle intervals. Panning the live court
until all flames were clipped stopped submissions, and returning home
resumed playback. Traversing the populated K’na Tha seam and returning
home preserved the exact starting pixels.

| Warm court timings in ms | Median | p95 | Max |
| --- | ---: | ---: | ---: |
| CPU composition | 11.75 | 15.44 | 22.31 |
| CPU composition, pack and presentation | 13.58 | 17.36 | 26.26 |
| Vulkan host redraw through present call | 1.33 | 2.09 | 3.44 |
| Vulkan traced GPU completion | 3.31 | 4.24 | 6.46 |
| Vulkan ordered sprite pass, including additive draws | 1.42 | 2.29 | 3.86 |

Additive draws remain in the same ordered pass as the rest of the
scene, so the pass timestamp covers both modes. Warm playback in the
67-frame Vulkan sample performed zero texture uploads. Sector and ART
counters remained steady during playback. The cold court worker took
50.85 ms, including 7.27 ms of ART decoding and 5.36 ms of lighting
preparation. The saved Milestone 44 binary took 46.85 ms for the same
sector in its verification run. The two runs decoded respectively 249
and 228 frames, accounting for the twenty flame frames and one shared
light mask added in Milestone 45. Before the benchmark resize, the
first 1536 × 864 frame uploaded 121 textures in 11.26 ms versus 120
in 10.06 ms on Milestone 44. First-frame completion was 49.15 versus
46.38 ms. These startup figures are individual trace samples.

The warmed court retained 166 GPU textures and 3,664,480 bytes of
decoded RGBA payload on the GPU. i915 reported 67.1 MiB of resident
graphics memory, compared with 65.1 MiB for the same Milestone 44
court and 67.2 MiB in the Milestone 44 Bates baseline. The lit
Milestone 45 Bates sample also reported 67.2 MiB. These totals
include driver and shared presentation allocations.

Formatting, all-feature Clippy, offline debug and release builds,
177 all-feature portable tests, all 30 installed-data checks, and
four hardware tests per backend passed. Both schematic audits passed
119/119. Captures, registration, pixel differences and timing traces
are available under `target/visual-checks/milestone-45/`.

## Follow-up: Bates Mansion candlesticks

The sitting-room candle stands **have active object-owned lights in the
original sector**. Records 35 and 100 in
`maps/bates mansion lev 1/67108865.sec` from `modules/Arcanum.dat`
are the stands beside the sofas and rug. Their VFS-selected prototype
`proto/004032 - Scenery.pro` supplies ordinary source-over blending
and an invalid primary light ART ID (`0xffffffff`). The two instances
each override the primary light fields. Their four overlay light ART
IDs inherit the invalid sentinel.

| Record and byte span | World tile and pixel offset | Sprite from Arcanum2.dat | Instance primary light |
| --- | --- | --- | --- |
| 35, 21513–21631 | (100,78), (+8,−3) | `0x401001c0`, `candlestand1.art` | Flags `0x08000080`, ART `0x90580000`, RGB (238,184,117) |
| 100, 27340–27458 | (98,85), (−5,−6) | `0x401801c0`, `candlestand2.art` | Flags `0x08000080`, ART `0x90580000`, RGB (238,184,117) |

Light MES key 11 resolves `0x90580000` to `art/light/largelight.art`.
The original mask is 450 × 225 with hotspot (225,112) and one frame.
For example, at the floor center of each owning tile the masks sample
RGB (249,249,249) and (246,246,246), respectively. TIG's channel
multiplication gives individual light contributions (232,179,114)
and (229,177,112), respectively, before ambient light, other masks and
floor modulation.

The original sector contains 23 candle stands using the two sprite
names. Fifteen have an active primary mask (35, 45, 67, 97, 100, 106,
108, 127, 146, 162, 189, 201, 203, 268 and 310). Their eight distinct
mask assets all resolve to single-frame ART. The other eight (207, 269,
280, 300, 311, 330, 354 and 361) have an effective invalid light ID.
This includes both inherited absence (207 and 300) and explicit invalid
overrides. Prototype identity, sprite appearance and the stored
modified flag alone cannot establish that a stand emits.

CE's `sector_light_list_fold` calls `sub_4D9590`, which creates the
primary light for a valid ART ID at the owning object's tile and offset.
`LF_OFF` is clear on the two sitting-room lights. The `0x08000000`
bit identifies the primary owner channel and `0x80` is `LF_MODIFIED`.
The objects lack both `OF_OFF` and `OSCF_NOCTURNAL`. Their scenery
flag `0x20` means respawnable, so the outdoor 06:00–18:00 rule does
not apply. The one-frame masks do not start a light timer. CE samples
them for terrain and nearby object illumination independently of the
scenery's sprite blend. The base data and CE path establish emission,
and the supplied screenshots show it in both vanilla and CE.

The native Wayland pair uses camera (101,83), 1280 × 720, Dungeon
scheme 2 and a fixed 90 ms animation tick. Scheme 2 uses indoor RGB
(74,80,98) at every hour. Seven static features register the newly
supplied vanilla screenshot at native scale with translation (+360,+180).
For the newly supplied CE screenshot they give translation (−67,−40)
after reducing its 1.8× scale. The registered captures give the following
rounded mean RGB values for 9 × 9 floor patches. The patches are outside
the visible green pool around the party.

| Native patch center | Native before | Native corrected | Vanilla | CE |
| --- | --- | --- | --- | --- |
| (520,251), near record 35 | (22,20,17) | (79,59,34) | (79,59,34) | (79,59,34) |
| (840,356), near record 100 | (22,20,17) | (79,59,34) | (80,59,35) | (79,59,34) |

CE's `critter_light_get` and the equipment update in `item.c` can
contribute character-owned light. The supplied vanilla and CE captures
also show a cooler pool around the party. Their exact character or saved
light state is unverified. The local samples establish the candle
correction and do not imply whole-room pixel equivalence.

Milestone 45 originally discovered light fields only for additive
scenery. The follow-up decouples primary emitter eligibility from sprite
blend and skips the explicit `LF_OFF` state. The existing shared
light-grid and point/column shading paths still handle the original
mask, tint and offsets. Bates now prepares 19 scenery-owned lights
(15 candle stands, two hanging lanterns and two fireplaces) separately
from nine sector-owned lights. Records 213 and 295 are the additional
ordinary-blend lanterns. Their own valid primary fields use the same
single-frame `largelight.art` mask with their respective stored RGB
values. The existing fire art, animation timing, blend and geometry
are unaffected. At 90 ms native CPU and Vulkan match exactly, and
the K’na Tha scene matches its pre-fix capture exactly.

For a broader lighting milestone, verify non-primary and overlay
slots, ambient-tinted and dark object masks, animated masks, nocturnal
state transitions, and character-associated lights independently.
These cannot be inferred from the candle fixtures.
