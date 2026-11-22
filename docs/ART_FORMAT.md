# Original ART sprites

The decoder imports a bounded subset of Arcanum's indexed ART files into
one native RGBA8 frame. The decoder takes bytes and a zero-based frame index.
The content loader reads the winning resource from the existing VFS and
returns its mount, storage and mod provenance. The runtime and native sprite
pixels have no dependency on TIG art IDs or display state.
[Decoder](../src/art.rs) · [VFS loader](../src/content/art.rs)

## Original samples

| Resource | Source | Frames | Frame details |
|---|---|---:|---|
| `art/item/bread.art` | Winning `Arcanum2.dat` | 1 | RLE, 81 × 24, hotspot (40, 23), 505 transparent pixels |
| `art/interface/align.art` | Winning `Arcanum1.dat` | 21 | Frame 0 is 27 × 9, frame 10 is 9 × 28, and frame 20 is 31 × 9 with hotspot (−13, 2). The sample frames use RLE. |
| `art/interface/ap_green.art` | Base copy in `Arcanum1.dat` | 1 | 132 raw indices for an 11 × 12 frame, hotspot (5, 11), 24 transparent pixels |
| `art/interface/ap_green.art` | Winning `ArcanumXBetaUI.dat` in the tested installation | 1 | Same dimensions, 127 stored RLE bytes, and different visible colors |

The three primary samples store 8 FPS and zero movement offsets. The
alignment sprite stores action frame 20. The static sprites store zero.
The VFS winners above were checked in CE-compatible mode. The base
`ap_green.art` copy was also read through a VFS with only `Arcanum1.dat`
mounted. Each listed archive stores its entry in a DAT zlib stream, a
separate compression layer from the sprite's pixel encoding.

## Stored layout

These files have no verified magic or version field. The first word is
flags. CE's `art_read_header` and `sub_51B710` read the following layout at
reference revision `fb721982`. All header and frame words are 32-bit
little-endian. The decoder reads them explicitly from bytes.

| Header byte offset | Contents |
|---:|---|
| 0 | Flags. Bit 0 selects one stored direction. CE reads eight directions when it is clear. |
| 4, 8 | Stored FPS and bits per palette index |
| 12–27 | Four palette-presence words. A nonzero word indicates a palette. The words are not file offsets. |
| 28, 32 | Stored action-frame marker and frame count |
| 36–67 | Legacy frame table words skipped by CE |
| 68–99 | Eight legacy data-size words. CE reads them and derives the frame payload spans from each frame's stored length. |
| 100–131 | Legacy pixel table words skipped by CE |

After the 132-byte header, each present palette occupies 1,024 bytes.
Each of its 256 entries is a little-endian `0x00RRGGBB` color in the
samples. CE's `tig_color_index_of` selects only the RGB components. The
decoder ignores the fourth byte, maps index zero to transparent black
and maps every other index, including any other black entry, to opaque
RGB. Index zero is the transparency key for the ordinary blit. CE also has
separate blending and hit-test behavior, which this decoder leaves to
future consumers. [CE source locations](#ce-reference-locations)

The palette is followed by all 28-byte frame records and then their
consecutive payloads. Each record contains width, height, stored byte
length, hotspot X/Y and movement offset X/Y. A length equal to width ×
height marks raw indices. Otherwise the payload uses RLE. The high bit of
the packet control byte selects literal indices. With that bit clear, the
following byte is one repeated index. The low seven bits give the number
of output pixels. Packets continue through row boundaries.

The decoder returns tightly packed top-to-bottom rows, left to right,
with straight RGBA8. Hotspots and movement offsets are signed pixel
coordinates. The hotspot is measured relative to the frame's top-left.
CE places a sprite by subtracting its hotspot from the destination anchor.
Some alignment-sprite hotspots are outside the image. CE uses movement
offsets separately in paths such as moving lights. Callers receive the
stored values without category-specific flips or coordinate adjustments.

CE's name layer maps interface IDs through `art/interface/interface.mes`
and uses a separate item naming routine. The Rust decoder only needs a
virtual path and frame index. The current VFS handles path normalization
and archive or mod precedence, including a malformed winning override.

## Bounds and limitations

- Accepted files use flags exactly 1, 8-bit indices, and palette slot zero
  only. Other flag combinations, eight-direction storage, other palette
  configurations and other bit depths return unsupported-format errors.
- Input is limited to 32 MiB, with at most 1,024 frames and 4,194,304
  pixels per frame. The decoder checks every frame's dimensions and stored
  span, and validates the selected frame's RLE packet boundaries and
  exact decoded pixel count. Unselected packet streams are left unread by
  the decoder. Zero-length RLE packets are rejected.
- The decoder rejects data after the last frame. An inspected
  `art/interface/acid_scheme.art` in `Arcanum1.dat` has a 295 × 225 raw
  frame and 3,409 trailing bytes. Their purpose remains unverified. The
  schematic audit still checks illustration readability and does not
  certify ART decoding.
- No terrain-specific sampling, rotation mirroring, palette transformations,
  legacy art-ID resolution, blend modes or animation scheduling is provided.
  The stored FPS and action marker are exposed as metadata.

## Visual inspection

```sh
cargo build --offline --locked
game="$HOME/Work/arcanum-ce-test"
target/debug/arcanum-rs art inspect "$game" art/item/bread.art 0
target/debug/arcanum-rs art export "$game" art/interface/align.art 10 ./align-10.pam
```

Export writes a Netpbm PAM with an `RGB_ALPHA` tuple. It uses exclusive
file creation and retains the transparency channel. A viewer that supports
PAM can display it directly. For a viewer that expects PNG, an optional
local conversion is `magick ./align-10.pam ./align-10.png`. ImageMagick is
a verification tool only. No graphics or encoder crate is required.

## CE reference locations

- `first_party/tig/src/art.c` at `fb721982`. `art_read_header`,
  `sub_51B710`, `sub_51BE30`, `tig_art_frame_data`, and the indexed
  blit paths.
- `first_party/tig/src/color.c`. `tig_color_index_of` for byte order.
- `src/game/name.c`. Interface table loading and ID-to-path resolution.
- `src/ui/charedit_ui.c` and `src/game/light.c`. Hotspot placement and
  movement offset consumers.

## Validation and decode cost

A separate Python probe unpacked the extracted bytes, expanded the indices
and computed RGBA fingerprints for the installed-data tests. It also matched
each PAM's payload byte for byte for bread, alignment frames 0, 10 and 20,
and the winning `ap_green.art` override. The exports opened in ImageMagick.
A nearest-neighbor preview against a solid background showed the bread
silhouette, the different alignment frame orientations and the green
indicator with transparent borders. A second export to the same path
was rejected. The probe and image outputs were kept outside tracked files.
[Installed-data test](../tests/installed_art.rs)

The following is a decoder-only measurement on Linux x86-64, Intel
i5-6300U, with Rust 1.100.0 at `-O` and the process pinned to one logical
CPU. Each input was loaded once before one warm-up decode and seven
25,000-decode batches. Each timed decode constructs and drops the result
and uses `black_box` on the input and result. The numbers exclude VFS
reads, DAT inflation, image export and display.

| Frame | Median µs per decode [min, max] | Input bytes | Returned pixel bytes |
|---|---:|---:|---:|
| Bread 0 | 2.376 [1.959, 4.547] | 2,731 | 7,776 |
| Alignment 10 | 0.838 [0.808, 0.981] | 6,833 | 1,008 |
| Base green indicator 0 | 0.516 [0.486, 0.547] | 1,316 | 528 |

The loader buffers one bounded ART resource. Decoding allocates the
selected frame's RGBA buffer and uses a 1 KiB stack palette. RLE expands
directly to RGBA. Other frames contribute table and span checks without
decoded pixel buffers. The byte counts describe input and pixel payloads.
They exclude allocation overhead, capacity slack, mount indices and process
memory. The loader releases the input after decoding.

Formatting, Clippy, Cargo tests, debug and release builds, and
`git diff --check` passed. An offline sandbox with no game, external mod
or CE directories passed all 105 portable tests and ran all five headless
demos. All 13 installed-data tests passed, including the ART comparison
and `vfs-smoke`. The schematic reference and readiness audits each
passed 119/119 under their existing checks.

## First windowed renderer

Use one optional frontend with a window/event-loop crate and a single
presentation backend. For an initial CPU sprite blit, `winit` and
`softbuffer` can supply those two roles. Keep composition, clipping,
hotspot placement and nearest-neighbor scaling in engine-owned Rust.
The decoder already provides the pixel format and placement metadata the
frontend needs. Verify a synthetic transparent sprite first, then present
one VFS-loaded original frame with the same path. Resize and close handling
are enough for that slice. Defer GPU selection until measurement or a
rendering requirement justifies it.
[Winit](https://docs.rs/winit/latest/winit/)
· [Softbuffer](https://github.com/rust-windowing/softbuffer)
