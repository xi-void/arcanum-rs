# Native sprite viewer

The optional `windowed` feature adds two commands to the existing CLI.
The synthetic demo uses an in-memory sprite. The ART viewer uses the same VFS,
frame decoder, source report and mod selection as `art inspect`. The viewer
consumes the decoded frame and does not need access to the mounted resources
once the frame has loaded.

```sh
cargo run --locked --features windowed -- art demo

game="$HOME/Work/arcanum-ce-test"
cargo run --locked --features windowed -- art view "$game" art/item/bread.art 0
cargo run --locked --features windowed -- art view "$game" art/interface/align.art 10
```

Frame indices are zero-based. `art view` accepts `--mods <mods-directory>
<mod-id> ...` as the final arguments. Its terminal report includes the winning
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

The viewer's anchor is the window's center in physical pixels. The
sprite's top-left is the anchor minus the signed ART hotspot. It presents
one source pixel per device pixel, including on scaled desktops. A
negative hotspot can place the entire sprite to one side of the anchor.
The stored movement offset is shown in the terminal and has no effect on
static placement. The synthetic ring uses a noncentral hotspot, a
negative X movement offset, transparent interior and outer pixels, and
opaque and partially transparent halves. [ART placement evidence](ART_FORMAT.md#stored-layout)

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
redraw, engine code clears the RGB8 buffer, composes the sprite, and
packs the pixels into softbuffer's contiguous `0x00RRGGBB` words.
A resize re-centers the hotspot against the new drawable region. The
event loop waits between events and exits on close or a reported
presentation error. [Viewer](../src/viewer.rs)
· [Softbuffer pixel format](https://docs.rs/softbuffer/latest/softbuffer/struct.Buffer.html#data-representation)

The viewer presents a single static frame. Animation timing, filtering,
color-managed blending, GPU rendering, and world or scene composition are
outside this milestone.

## Verification and performance

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
