# Native sprite viewer

The optional `windowed` feature adds single-sprite and overlap demos to
the existing CLI. The synthetic demos use in-memory sprites. The ART
demos use the same VFS, frame decoder, source report and mod selection
as `art inspect`. Mounted resources are released before the window opens.

```sh
cargo run --locked --features windowed -- art demo
cargo run --locked --features windowed -- art overlap-demo

game="$HOME/Work/arcanum-ce-test"
cargo run --locked --features windowed -- art view "$game" art/item/bread.art 0
cargo run --locked --features windowed -- art view "$game" art/interface/align.art 10
cargo run --locked --features windowed -- art overlap "$game"
```

Frame indices are zero-based. `art view` and `art overlap` accept
`--mods <mods-directory> <mod-id> ...` as their final arguments. Their
terminal reports include the winning
resource and stored hotspot and movement offset. For artwork with
trailing data, it retains the decoder's diagnostic. A running Linux desktop
session is required to open a window. With the feature disabled the viewer
commands print how to enable it. With the feature enabled and no active
display, window initialization reports the display error.

## Pixel and placement contract

The core compositor takes a row-major straight RGBA8 `SpriteFrame` and
an initialized opaque RGB8 destination. Stride counts pixels. A valid
destination covers every visible row and its intervening padding. Padding
after the last row is optional. The compositor validates both layouts,
intersects the sprite rectangle with the visible destination, and
blends only the intersection. Fully transparent pixels preserve the
background. Fully opaque pixels replace it. Partial alpha uses
`round((source × alpha + destination × (255 − alpha)) / 255)`
per channel on the stored 8-bit values. This is byte-space blending.
[Portable compositor](../src/compositor.rs)

`blit_ordered` applies `SpriteDraw` entries in iterator order, back to
front. Each draw borrows a frame and supplies its own signed destination
anchor. Partial alpha blends against the pixel left by earlier draws.
Fully transparent pixels reveal earlier draws or the background. The
function applies each draw directly to the caller's destination using
the same checked `blit` path as a single sprite. If a later draw fails
validation, earlier draws remain applied.

The viewer holds anchor offsets relative to the center of the drawable
area in physical pixels. Resize changes the center and retains these
offsets. Each sprite's top-left is its anchor minus its signed ART
hotspot. It presents one source pixel per device pixel. A negative
hotspot can place the entire sprite to one side of the anchor. The
stored movement offset remains diagnostic metadata. [ART placement evidence](ART_FORMAT.md#stored-layout)

The asset-free overlap demo draws an amber ring behind a cyan one.
Their transparent interiors, opaque halves and half-alpha halves show
the layering. `art overlap` draws bread frame 0 from `Arcanum2.dat`
at (−12, 8), then alignment frame 10 from `Arcanum1.dat` at (12, 17),
relative to the center. The CLI reports the actual winning resources
and order. With the stock winners the alignment frame covers 128 opaque
bread pixels while 1,311 opaque bread pixels remain visible. Its other
37 opaque pixels extend beyond the bread silhouette.

## Window lifecycle and dependencies

`winit` 0.30.13 handles the window and events. `softbuffer` 0.4.8 presents
a CPU buffer. Both use the raw-window-handle 0.6 interface. The optional
build enables Wayland and X11 support. Ordinary builds and tests exclude
both graphical crates from the active dependency graph.
[Winit crate](https://docs.rs/crate/winit/0.30.13)
· [Softbuffer crate](https://docs.rs/crate/softbuffer/0.4.8)

The app creates the window and surface on resume. It requests an
initial redraw and reacts to resize, scale changes and desktop
redraw events. A zero-sized surface skips presentation. On a drawable
redraw, engine code clears one RGB8 buffer, draws every sprite in order,
and packs the completed pixels once into softbuffer's contiguous
`0x00RRGGBB` words. The viewer uses no per-sprite framebuffer. The
event loop waits between events and exits on close or a reported
presentation error. [Viewer](../src/viewer.rs)
· [Softbuffer pixel format](https://docs.rs/softbuffer/latest/softbuffer/struct.Buffer.html#data-representation)

The viewer presents static frames with caller-supplied ordering.
Animation, depth sorting, filtering, color-managed blending, GPU
rendering, and world or camera coordinates are outside this milestone.

## Milestone 27 verification and performance

On Hyprland 0.56.2 with 1.25× display scaling, a native Wayland window
displayed the synthetic ring and bread from `Arcanum2.dat`. The ring
capture contained the background, the opaque amber, and the expected
half-alpha amber. The bread window resized from 749 × 400 to 420 × 320
logical pixels. The sprite bounds stayed within two capture pixels of
their previous location relative to the center. The Wayland trace
showed fresh buffers after resize and no buffer submissions during a
750 ms idle observation once the initial draws settled. Alignment
frame 10 from `Arcanum1.dat` also displayed through X11 under XWayland.
The Acid schematic displayed on Wayland and retained its 3,409-byte
trailing-data diagnostic. All tested windows exited successfully after a compositor close request.
A launch with the display variables unset reported a display error.
Native Xorg and other platforms were outside the desktop smoke test.

An optimized CPU probe measured bread blitting into a preallocated RGB8
buffer, then measured clearing, blitting and packing the entire frame
into a preallocated XRGB buffer on the same i5-6300U Linux host. It used
Rust 1.100.0 at `-O`, one pinned logical CPU, `black_box`, and seven
batches after warming. The blit used 20,000 calls per batch and the full
buffer pass used 500. Values are medians with observed batch ranges in
brackets. Window event dispatch, allocation, platform presentation and
ART decoding are outside these timings.

| Drawable pixels | 81 × 24 sprite blit, µs | Clear, blit and pack, ms | RGB8 plus one XRGB buffer |
|---|---:|---:|---:|
| 640 × 480 | 4.21 [3.58, 4.74] | 1.17 [1.12, 1.35] | 2.05 MiB |
| 800 × 600 | 4.32 [3.36, 5.16] | 1.90 [1.67, 2.04] | 3.20 MiB |

Framebuffer sizes count three RGB8 bytes per drawable pixel and four
bytes per softbuffer pixel. The RGB vector can retain capacity after a
resize and the backend may hold more than one presentation buffer. The
viewer checks the drawable area before allocating, with a limit of
16,777,216 pixels.

Formatting, Clippy, tests and builds passed with the default and
windowed configurations, including the default release build. A
network-isolated headless environment passed all 113 portable tests,
ran all five simulation demos, and checked the windowed target without
display access. All 14 optional installed-data checks passed, including
the ART regressions and `vfs-smoke`. Both schematic audits reported
119/119 under their existing checks.

## Milestone 28 verification and performance

The single-sprite demo and both overlap demos ran in native Wayland
windows on Hyprland at 1.25× display scaling. The synthetic overlap
showed the expected half-alpha color and clipped when resized from
480 × 340 to 90 × 80 logical pixels. The original pair retained an
identical centered capture after resize from 480 × 340 to 420 × 320.
Both overlap windows presented fresh buffers after resize, submitted
no new buffers during a 750 ms idle observation, and closed normally.
Portable tests check exact pixel values for opaque, half-alpha and
transparent overlaps, reversed order, unchanged nonoverlapping
pixels, and signed anchors with all-edge and off-screen clipping.

A warmed CPU probe on the same i5-6300U host compared one bread draw
with the bread-and-alignment pair at 640 × 480. It used Rust 1.100.0 at
`-O`, one pinned logical CPU and preallocated buffers. The probe used
`black_box`, alternated single- and two-sprite measurement order,
discarded the first batch per workload, and measured seven batches
of 100,000 blend calls or 500 full-buffer passes. Display presentation,
allocation and ART decoding are outside these timings.

| Workload | Blend only, µs median [min, max] | Clear, blend and pack, ms median [min, max] |
|---|---:|---:|
| Milestone 27 bread, historical | 4.21 [3.58, 4.74] | 1.17 [1.12, 1.35] |
| Milestone 28 bread | 2.18 [2.09, 2.20] | 0.97 [0.93, 1.02] |
| Milestone 28 bread and alignment, overlapping | 2.50 [2.45, 2.60] | 0.94 [0.92, 0.97] |

The paired run measured about 0.32 µs of additional blending work.
Full-buffer ranges overlap. Clearing and packing the framebuffer
dominate the timings for this pair. The historical row was a separate
run and does not isolate a change in single-sprite speed. Both
Milestone 28 workloads use about 2.05 MiB for RGB8 plus one XRGB buffer.
The two native sprite buffers contain a total of 8,784 pixel bytes.

Formatting, Clippy, default debug and release builds, windowed build
and tests, and `git diff --check` passed. All 115 portable tests passed
offline in an isolated headless environment. All 14 optional installed
checks passed, including `vfs-smoke`. The five headless demos ran and
both schematic audits remained at 119/119.
