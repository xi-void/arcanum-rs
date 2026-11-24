# Vulkan renderer

The `windowed` build presents through wgpu's Vulkan backend on a compatible
hardware adapter. The headless build provides data inspection and asset-free tests.

```sh
cargo run --locked --release --features windowed -- \
  play "$HOME/Work/arcanum-ce-test"
```

The world, diagnostics, atlas inset and HUD use the same Vulkan compositor.
The atlas inset is rendered into a small resident texture and sampled in the
UI pass. Asset decoding, UI layout, sector preparation and gameplay run on
the host. The selected hardware adapter and presentation format are logged.
An incompatible graphics driver produces a Vulkan initialization diagnostic.

## Rendering contract

The world cache produces one ordered sequence of visible sprite draws and
their prepared light multipliers. CPU and GPU consume the same anchors,
decoded frame allocations, mirrored or rotated sprite pixels and
modulation. Each GPU texture is one decoded RGBA8 ART frame. Integer
texel lookup gives nearest-neighbor sampling. The shader multiplies
palette RGB by the original byte-domain uniform, floor or wall-column
values before source-over blending. It computes the same floor weights
and integer truncation as the CPU. Transparent texels discard. The
tested GPU target must offer a non-sRGB RGBA8 or BGRA8 UNORM surface so
the blend stays in the reference renderer's byte-domain color space.

The GPU encodes ordered six-vertex sprite quads. Consecutive draws with
the same texture and blend mode batch into one instanced call. The bounded
GPU texture cache tracks decoded allocation identity, using a weak reference
so reloads cannot alias an old texture. It evicts inactive textures
under a 96 MiB or 4,096-texture limit. Instance and wall-column uploads
use bounded reusable buffers. The renderer uses no CPU-composed frame
as input. It neither duplicates nor changes world loading, light preparation
or sprite order.

The offscreen additive fixture verifies original indexed-color behavior
from TIG's `tig_color_add` and additive blits. Each nontransparent RGB
channel adds to the destination with saturation. Transparent texels leave
the destination unchanged. The fixture deliberately includes nonzero RGB
in a transparent texel, destination saturation and two overlapping draws.
World objects retain their existing source-over mode. Animated fire and
dynamic emitters remain outside this milestone.

The dependency APIs are from [wgpu 26.0.1](https://docs.rs/wgpu/26.0.1/wgpu/)
and [winit 0.30](https://docs.rs/winit/0.30.13/winit/).

## Validation

```sh
cargo test --locked --features windowed
ARCANUM_GAME_DIR="$HOME/Work/arcanum-ce-test" \
  cargo test --locked --features windowed --lib gpu::tests -- --ignored --test-threads=1
```

The opt-in Vulkan suite covers palette colors, fixed lighting and blending,
animation, cache lifetime, clipping, world navigation, and the render-to-texture
map inset. [Earlier hardware measurements](GPU_WORLD_INTEGRATION.md) record the
pre-consolidation baseline.
