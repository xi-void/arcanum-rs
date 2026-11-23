# Experimental GPU sprite rendering

The optional `gpu` feature adds the wgpu Vulkan and GLES backends to the
existing streamed world viewer. The default Cargo build remains headless.
The windowed CPU compositor, its ordering and its regression fixtures
remain available. GPU requests require an integrated or discrete hardware
adapter which can present to the window. The viewer logs the adapter and
capabilities and reports an error if the requested backend is unavailable.

```sh
cargo run --offline --locked --release --features gpu -- art world "$HOME/Work/arcanum-ce-test" \
  --sectors 'bates mansion lev 1' 1 1 1 1 101 83 1280 720 --renderer vulkan --lighting 12
```

Use `--renderer gles` to request Mesa EGL/GLES or `--renderer cpu` for
Softbuffer. Omit `--lighting 12` for the unlit view. Arrow keys pan, Home
restores the camera and Escape closes the viewer. The same event loop
handles sector completion, camera changes, expose events and resize.

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

## Hardware and presentation

Tests ran on the ThinkPad X270's Intel HD Graphics 520 (Skylake GT2,
PCI 8086:1916) with Linux 7.2.15, i915, Mesa 26.2.5 and Rust 1.100.
The Vulkan adapter identified itself as integrated Intel hardware.
The wgpu GL backend used Mesa EGL with a desktop OpenGL 4.6 core
context on the same Intel GPU. EGL also reported OpenGL ES 3.2 on
Wayland. Both wgpu runs reported shader model 5, fragment storage,
blendable RGBA8, timestamp queries and a compatible Wayland surface.
Both presented direct to opaque BGRA8 UNORM with FIFO presentation.
The OpenGL backend option requires fragment storage, so this
experiment does not cover GLES 2 or other older drivers.

A forced software EGL run identified llvmpipe as a CPU adapter and
exited with an explicit error directing the user to the CPU renderer.
The Vulkan and OpenGL measurements below are hardware runs.

Before the shutdown fix, two GLES runs crashed after Escape with a
Mesa EGL teardown stack in libwayland-client. The event loop previously
returned before the application released its renderer. The viewer now
drops presentation resources in winit's exit callback and retains an
owned display handle through EGL/Vulkan teardown. This leaves the
Wayland display alive while the driver releases its resources.

## Visual and regression checks

The 1280 × 720 Bates Mansion capture at (101,83) uses original floors,
facades, 268 wall records, 79 scenery records and 19 portals in its
prepared sector. The documented Milestone 42 noon ambient setting
uses the same input for both renderers. The CE comparison reuses
the earlier registration and illustrates the remaining differences
in object-owned light, animated fire and shadows.

Each lit and unlit Vulkan/GLES Bates capture was compared directly
to its matching CPU capture. The difference images encode eight
times the per-channel absolute difference. They are entirely black
for the matching frames. Native Vulkan and CPU also match on the
K’na Tha sector seam, across the neighbor view and on return. Native
pan, resize and Home retain exact pixels at the original camera and
viewport. Stationary Wayland traces showed zero extra buffer attaches.

Offscreen hardware tests compare exact RGB pixels on both backends
with the CPU reference at the Bates camera, a shifted camera and
the two-sector K’na Tha seam. Asset-free fixtures exercise clipped
quad sampling, floor and column light, ordered alpha compositing,
saturating additive overlap and warm texture reuse. The synthetic
partial-alpha edges allow at most one unit per channel of hardware
blend rounding. Opaque, transparent and additive fixtures require
exact matches. Hardware checks are opt-in. The ordinary portable
tests and installed content tests still work without a GPU.

```sh
cargo test --offline --locked --features gpu --lib gpu::tests
ARCANUM_GPU_BACKEND=vulkan ARCANUM_GAME_DIR="$HOME/Work/arcanum-ce-test" \
  cargo test --offline --locked --release --features gpu --lib gpu::tests -- --ignored
```

Capture files, registered comparisons, differences and measurement
traces are under `target/visual-checks/milestone-43/`.

Three post-fix hardware EGL runs exited cleanly after pan, resize and
Home, including both timed 120-frame runs. The local test inputs were
read-only.

## X270 performance

The Wayland benchmark used the same lit or unlit Bates camera and
1280 × 720 viewport for every backend. It submitted 120 discrete
keyboard steps per run with 35 ms between steps. All steps produced
a traced frame. The earlier Milestone 42 CPU-only benchmark measured
13.94 ms median lit composition without Wayland presentation.
The current CPU run measured 12.39 ms for composition and 1.71 ms
for packing, with 14.19 ms of overall host time.

| Backend | Lighting | Host acquire/draw to present call median / p95 / max | Traced GPU completion median / p95 / max | GPU pass median / p95 / max |
| --- | --- | ---: | ---: | ---: |
| CPU | Off | 7.76 / 11.84 / 18.10 ms | — | — |
| CPU | Static | 14.19 / 19.57 / 25.00 ms | — | — |
| Vulkan | Off | 1.30 / 3.54 / 8.73 ms | 2.95 / 4.84 / 12.58 ms | 1.06 / 1.36 / 1.65 ms |
| Vulkan | Static | 1.29 / 2.96 / 4.53 ms | 3.32 / 4.77 / 7.50 ms | 1.46 / 2.10 / 2.96 ms |
| GL | Off | 1.89 / 2.31 / 7.30 ms | 4.29 / 5.04 / 7.36 ms | 1.60 / 1.96 / 2.55 ms |
| GL | Static | 1.93 / 2.45 / 11.44 ms | 4.75 / 6.10 / 11.66 ms | 2.08 / 2.95 / 8.81 ms |

Median lit Vulkan command generation, buffer writes, command encoding,
submission and present API costs were 0.126, 0.049, 0.641, 0.135 and
0.142 ms. Lit GL measured 0.165, 0.121, 0.341, 1.077 and 0.154 ms.
The GPU-pass column comes from hardware timestamp queries. It excludes
uploads, command recording, queue wait and presentation. Traced
completion explicitly waits for the submission and reads the timestamps,
which adds synchronization and is confined to diagnostic runs. The host
measurement ends when the present call returns. Physical scanout time
and input-to-photon latency were unavailable via this instrumentation.

The first visible lit frame was 1536 × 864 due to the initial Wayland
configure. Each backend uploaded 129 sprite textures totaling 2,701,668
bytes. On the measured runs, CPU-side preparation and upload calls
took 45.91 ms for Vulkan and 12.55 ms for GL, while the first GPU pass ran
in 4.96 and 5.80 ms respectively. Adapter, surface and pipeline setup
took 70.06 ms for lit Vulkan and 75.48 ms for lit GL. Sector parsing, ART
decoding and light preparation remain on the world worker and have
separate stages in the raw traces. The first pan warmed the texture
cache with newly visible textures. The 120 timed frames on each
backend then uploaded zero textures. Resizing used live textures and
only reconfigured the target.

At the stationary lit camera, process RSS was about 60 MiB for CPU,
55 MiB for Vulkan and 88 MiB for GL. GPU runs had 153 retained textures
with 3,239,364 bytes of RGBA payload and 4 MiB of reserved draw buffers.
i915 reported about 187 MiB of resident system graphics allocations for
Vulkan and 80 MiB for GL in this run. These are driver-managed
allocations in shared system memory, including more than just sprite
textures. They should be tracked separately from process RSS.

Vulkan is the recommended prototype backend on this machine. It
preserved exact original-scene pixels, had the lowest measured lit GPU
completion time and offers room for additional sprite effects.
Cold uploads and graphics memory are the primary tradeoffs. Keep the
CPU renderer as the reference and default while collecting further
cold-start and memory data.

## Validation

Formatting, Clippy for the default, windowed and GPU features, and
offline debug and release builds passed. The default dependency tree
contains neither the windowing libraries nor wgpu. The default release
binary was 2.46 MiB and the GPU-enabled release binary was 13.19 MiB
on this toolchain. All 173 default portable tests and 30 installed-data
tests passed, including the 268-wall audit, VFS smoke check and the
existing seam tests. Both schematic audits returned 119/119. The optional
packing test and both offscreen hardware tests passed. Each hardware
test ran successfully on both Vulkan and GL.

[Milestone 44 GPU world integration and residency measurements](GPU_WORLD_INTEGRATION.md)
