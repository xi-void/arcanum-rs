# GPU world integration and resource lifetime

Milestone 44 uses the existing interactive world stream for both CPU and GPU
presentation. The worker owns sector reads, original object import and ART
decoding. The event thread keeps the previous complete frame during required
loads. When a worker result becomes visible, both renderers receive the same
ordered, clipped sprites and prepared lighting. GPU allocation and upload take
place on the presentation thread for newly visible textures. Sector prefetch
does not upload textures.

## Residency and recovery

The GPU texture cache is keyed by the decoded frame's shared allocation. Different
rotations, palettes and frames keep their existing ART cache identities. The GPU
holds a weak reference, so it cannot keep a decoded frame alive. The next
preparation retires a texture whose last strong ART reference is gone. A sector
eviction can leave the decoded ART resident and reusable, including its GPU
texture. wgpu keeps submitted resources alive until the relevant work completes.

Each frame first identifies its complete visible working set, including resident
hits and distinct misses. Only then does it retire least-recently-used inactive
textures if needed to admit the misses. A visible hit remains protected even
when its first draw follows a miss in draw order. An oversized visible set fails
with an explicit error before uploads or cache-pressure eviction. The logical
cache limits remain 4,096 textures and 96 MiB of RGBA payload. These limits
exclude driver allocations, the presentation surface, transient staging and fixed
GPU buffers.

Vulkan now requests wgpu's `MemoryUsage` strategy. In the pinned wgpu-hal 26.0.6
Vulkan adapter this starts free-list blocks at 8 MiB instead of 128 MiB. The
same option is harmless on backends that ignore it. An experimental batched
staging submission increased cold CPU preparation on this machine, so uploads
still use wgpu's standard queue write path. An atlas was unnecessary for the
measured scenes.

Resizing reconfigures the surface while keeping the device and textures. A lost
or outdated surface is reconfigured and acquired once more. The GPU device
also records loss and uncaptured errors for clean exit, with the world stream
kept separate from device resources. This does not attempt in-place device
recreation. The hardware test explicitly destroys a device to exercise this
path. It does not simulate a kernel or driver reset.

## Reproduce

The windowed build now uses Vulkan throughout. The measurements below describe
the original integration milestone before backend consolidation.

```sh
game="$HOME/Work/arcanum-ce-test"
cargo run --offline --locked --release --features windowed -- \
  art world "$game" --sectors "bates mansion lev 1" 1 1 1 1 101 83 1280 720 \
  --lighting 12 --trace

ARCANUM_GAME_DIR="$game" \
  cargo test --offline --locked --release --features windowed --lib gpu::tests \
  -- --ignored --test-threads=1
```

For the populated K’na Tha seam, use `--sectors "k'na tha" 5 14 6 14 341 928
1280 720`. To exercise eviction, extend the right sector limit to 18,
pan eastward on the map and return with Home. Some sectors in the extended
route have no sector resource. The loader reports them explicitly.

`--trace` reports acquire, CPU command generation, uploads, encoding,
submission, present-call and completed-frame timings. The Vulkan sprite-pass
timestamp excludes texture upload and presentation. Completion includes the
traced wait for GPU work and does not measure when pixels reach the display.
Ordinary viewing does not wait for GPU timestamps. A build with
`--features windowed,wgpu/counters` also logs supported driver allocation counters
on frames that change residency. Linux i915 process fdinfo gives a broader
driver memory estimate, including shared presentation allocations.

## K’na Tha diagonal terrain boundary

A repeatable example is K’na Tha, camera centered on world tile `(575,928)`
with zero pixel offset, sector bounds `(5,14)..(18,14)` and a 1280 × 720
framebuffer. At this resolution the screen center is `(640,360)`. With
`dx = screen_x - 640` and `dy = screen_y - 360`, the continuous world
position is `(575 + (2dy − dx)/80, 928 + (2dy + dx)/80)`. Round to the
nearest tile center away from a diamond edge.

```sh
cargo run --offline --locked --release --features windowed -- \
  art world "$HOME/Work/arcanum-ce-test" --sectors "k'na tha" 5 14 18 14 575 928 1280 720 \
  --trace
```

| Screen pixel | World tile | Original sector | Evidence |
| --- | --- | --- | --- |
| (198,360) | (581,922) | (9,14), 939524105 | No sector resource in the installed VFS |
| (1100,360) | (569,934) | (8,14), 939524104 | Original floor ART ID `0x004105c0` |

The populated sector comes from `modules/Arcanum.dat`. All 4,096 surfaces
imported with zero sector diagnostics. The trace shows a required request
for each sector. The missing sector completed with a VFS resource-not-found
diagnostic, and only then did the viewer present the required set. There
were no cache evictions during this capture. With the longer navigation
route, the missing sector can also be prefetched and return the same
diagnostic before it enters the viewport.

CPU and Vulkan agree at every pixel, including the blue-gray `(32,43,58)`
area. An additional Vulkan capture after the settled idle window agrees
pixel for pixel with the initial complete capture. The diagonal follows
the transition from the last original floor cells at world X=575 to the
absent sector beginning at X=576. At this camera position the seam follows
`screen_y = 60 + screen_x / 2`. The background in this reproduction is
the framebuffer clear exposed by missing sector data. Intentional original
black cells still resolve and draw as ART, as in the Bates void fixture.
The existing selection and clipping rules passed the boundary regression.

The portable coverage test enumerates floor raster rectangles and the
full supported object overhang across four sectors at several viewport
sizes, including odd sizes and a one-pixel view. Every raster intersecting
the viewport has its owning sector selected within the configured region.
This complements the real-seam parity tests.

## Measurements on the X270

The test machine used an i5-6300U, an Intel HD Graphics 520 (8086:1916)
with i915 and Mesa 26.2.5, Hyprland Wayland and the pinned wgpu 26.0.1.
The hardware adapter check passed for Vulkan and Mesa OpenGL. No software
adapter was used.

At the 1280 × 720 lit Bates home position, the baseline Vulkan process
reported 191,892 KiB of resident i915 graphics memory. The same route with
the new allocator policy reported 68,772 KiB, a 64% reduction. Both reported
11,100 KiB of shared graphics memory, consistent with the presentation
buffers. Process RSS was 55–57 MiB across the Vulkan runs. The driver counters
and process RSS are different observations and should not be added.

| Accounting at lit Bates | Measured bytes |
| --- | ---: |
| Retained ART RGBA payload, 153 distinct decoded frames | 3,239,364 |
| Vulkan texture suballocations for those frames | 8,093,696 |
| Explicit compositor buffers, including timestamp buffers | 4,194,352 |
| Vulkan live buffers after completion, including backend buffers | 6,816,000 |
| Extra live buffer bytes before submitting the first 129 textures | 3,091,904 |
| Shared i915 graphics memory at the settled 1280 × 720 viewport | 11,366,400 |

The buffer delta is a first-frame sample of transient staging and other
submission buffers. The application allocates no intermediate offscreen
framebuffer for this renderer. The live texture and buffer suballocations
plus the shared graphics sample explain about 25.1 MiB of the 67.2 MiB
resident total. OpenGL reported 72,220 KiB of i915 residency and
roughly 88 MiB of process RSS in the matched run. The Vulkan remainder
includes allocator block slack and driver
internals. wgpu's allocator report is unavailable for this Vulkan path, so
the individual contributions to that remainder were not measured.

### Timing comparison

The warm sample repeated Right, Down, Left and Up 30 times with 35 ms
between key events after settling the window at 1280 × 720. The baseline
is the saved Milestone 43 binary on the same machine and route. Times are
milliseconds, shown as median / p95 / maximum.

| Renderer | Redraw through present call | Traced GPU completion | GPU sprite pass |
| --- | ---: | ---: | ---: |
| Milestone 43 Vulkan | 1.268 / 1.847 / 2.924 | 3.251 / 3.769 / 5.030 | 1.428 / 1.704 / 1.914 |
| Milestone 44 Vulkan | 1.247 / 1.690 / 3.100 | 3.252 / 3.807 / 5.221 | 1.447 / 1.712 / 2.617 |
| Milestone 44 CPU | 13.084 / 15.603 / 21.411 | — | — |
| Milestone 44 OpenGL | 1.877 / 2.272 / 12.546 | 4.689 / 5.631 / 12.608 | 2.066 / 2.918 / 3.672 |

Warm Vulkan CPU command generation, render encoding, queue submission
and present-call medians were 0.127, 0.651, 0.127 and 0.165 ms. The
present-call measurement includes winit's pre-present notification.
The desktop initially configured the window at 1536 × 864 before the
benchmark resize. At that first rendered size both Vulkan runs uploaded
129 textures with 2,701,668 RGBA bytes. Host upload preparation fell
from 45.11 to 10.58 ms in the matched run, and first-frame completion
fell from 82.89 to 46.58 ms. A second Milestone 44 sample had 19.45 ms
upload preparation and 73.86 ms completion under a busier desktop.
These are traced samples from one machine and do not establish a frame
rate or input-to-display latency.

An exploratory OpenGL run had a 1.21-second host outlier while the
sprite-pass timestamp was 2.11 ms. The logged stages did not account for
the gap. The matched repeat did not reproduce it. Vulkan remains the
primary backend.

The extended K’na Tha Wayland run generated 720 timed 1280 × 720
navigation frames, including views across missing sectors. It finished
with 12 cached sectors, 3 sector evictions, 11 prefetch hits and zero
prefetch misses. The retained decoded ART held 91 frames and 2,943,752
bytes of RGBA. At the end of the route GPU residency peaked at 81 cached
textures with 2,398,496 bytes of payload. Driver residency at the home
and far samples was 64,604 and 62,552 KiB. These two samples are not a
peak-driver-memory measurement.

The home sector required a new sector read after eviction. The worker
took 31.48 ms and the viewer held the previous complete frame for 32.72 ms.
The event thread waited zero time for sector loading. The return required
zero ART-file reads, zero ART decodes and zero GPU uploads. Home pixels
matched exactly across the route and a resize. Vulkan median, p95 and
maximum traced completion were 3.17, 6.43 and 19.47 ms across this route.
GPU sprite-pass times were 0.52, 1.21 and 1.53 ms. The route includes
views with no draws, so these are traversal measurements rather than
steady-state timings for a populated scene.

Offscreen readback verified exact CPU parity on both hardware backends
for unlit and lit Bates, the populated K’na Tha seam, the lit brazier
court and Forest Passage. The test covers camera movement, odd-sized
resizing and sector eviction and re-entry. The original ART scenes
matched every RGB byte. The pre-existing synthetic half-alpha comparison
retains its one-code-point tolerance on two specified pixels. The
synthetic cache-pressure test covers visible texture protection, distinct
misses, oversized working-set rejection and retirement after the last
strong ART reference drops.

Native Wayland paired captures also matched, including the populated seam,
brazier court, Forest Passage, lit Bates and the diagonal boundary. Three
OpenGL sessions opened, resized and shut down cleanly. Stationary,
settled windows submitted zero extra buffers in the observed three-second
idle intervals. The captures and raw logs are under
`target/visual-checks/milestone-44/`.

## Validation

Formatting and all-feature Clippy passed. The local asset-free CI
commands passed, including 174 headless and windowed portable tests,
175 all-feature portable tests and debug/release builds. All 30
installed-data checks passed, including the external VFS fixture and the
Bates wall audit. Schematic references and readiness each passed 119/119.
Four opt-in hardware tests passed on each GPU backend. The default
dependency tree excludes the GPU and windowing dependencies.
