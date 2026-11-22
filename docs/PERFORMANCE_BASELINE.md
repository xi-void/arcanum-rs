# Performance baseline

Milestone 24 measured release builds on `audit/runtime-consolidation`. The only code added is a [portable benchmark target](../benches/baseline.rs). Production runtime, resource loading, and gameplay behavior remain unchanged.

The primary runtime cost is **scanning every active grant to answer a single actor's stat query**. With four grants on the queried actor, three resistance reads took **6.05 µs** at 100 actors and **78.66 µs** at 1,000 actors. The activation handle lookup alone took **22.3 ns** and **67.7 ns**, respectively. Resource measurements showed a different pattern. Archive index loading and content decoding cost substantially more than a single virtual path lookup.

## Environment and method

- Intel Core i5-6300U at 2.40 GHz nominal, two physical cores, four logical CPUs, 3 MiB L3, and 15 GiB of RAM as reported by Linux. Omarchy x86-64, Linux `7.2.15-1-omarchy`.
- Rust `1.100.0 (0ef1bc386)`, LLVM 23.1.5 and Cargo `1.100.0 (3acc79c0c)`. Default Cargo release and bench profiles, with no project profile overrides or CPU-specific compiler flags.
- The reported timing runs were pinned to logical CPU 2 with `taskset`, with the system's `powersave` governor and other host activity present. The sandbox run separately verified portability. These are measurements on one host, with no implied minimum hardware specification.
- The harness uses `Instant`, one unreported warm-up batch, then seven timed batches. Figures are the median of per-operation batch means, with the minimum and maximum of those means in brackets. They are neither confidence intervals nor individual-call percentiles. The harness uses `black_box` on query inputs or results and activation handles. It reports world construction separately, once per workload. The measured operation includes its small loop and dispatch overhead.
- Runtime definitions and resource fixtures are deterministic and built in memory or in temporary files. Archive timings use a warm tmpfs fixture. They do not characterize cold storage. Memory observations use separate processes and include allocator and process overhead.

### Reproduce the portable measurements

```sh
cargo build --release --offline --locked
taskset -c 2 env TMPDIR=/dev/shm cargo bench --offline --locked --bench baseline
cargo bench --offline --locked --bench baseline -- runtime-memory 1000
cargo bench --offline --locked --bench baseline -- archive-memory 32768
```

Use `-- runtime` or `-- resource` to run either timing group separately. Omit `taskset` or choose an available Linux CPU on another host. The memory modes accept actor counts from 1 to 5,000 and archive entry counts from 3 to 65,536. The benchmarks require a Rust toolchain and locally available locked dependencies. Timings and type sizes are portable. The memory modes use Linux `/proc` when available.

## Runtime timings

Every actor has two item instances and four active grants. A shared native fixture gives additive AC, Electrical additions, and Fire, Electrical and Poison floors. Each base AC lookup, activation lookup and effective-stat query rotates through actor IDs in insertion order. Lifecycle timings use the first actor and one precreated spare item. The activation pair grants and removes once on an already equipped item. The equipment cycle equips, grants and unequips the item, including removal of its grant. The existing world and grants persist across iterations.

The single-actor case models a small local workload. The 100-actor and 1,000-actor cases are scene-sized and large synthetic workloads. The 5,000-actor case is a bounded stress test. Actual actor and query counts in a future playable game have yet to be measured.

| Actors | Active grants | Setup ms, one run | Lookup iterations per batch | Other iterations per batch |
| ---: | ---: | ---: | ---: | ---: |
| 1 | 4 | 0.028 | 1,000,000 | 100,000 |
| 100 | 400 | 0.931 | 100,000 | 10,000 |
| 1,000 | 4,000 | 81.415 | 20,000 | 2,000 |
| 5,000 | 20,000 | 4,654.754 | 4,000 | 400 |

| Actors | Base AC lookup, ns | Activation lookup, ns | Effective AC, µs | Three resistance queries, µs |
| ---: | ---: | ---: | ---: | ---: |
| 1 | 5.2 [4.6, 5.8] | 10.7 [9.9, 13.5] | 0.033 [0.025, 0.039] | 0.098 [0.085, 0.165] |
| 100 | 15.7 [14.1, 18.8] | 22.3 [20.5, 24.6] | 1.611 [1.485, 1.674] | 6.050 [5.965, 7.219] |
| 1,000 | 40.2 [37.9, 41.1] | 67.7 [57.4, 100.2] | 19.860 [19.179, 21.955] | 78.662 [75.980, 92.674] |
| 5,000 | 92.7 [67.6, 105.0] | 135.2 [105.3, 175.3] | 290.164 [245.518, 312.556] | 977.983 [903.416, 1,111.957] |

| Actors | Activate + remove, µs per pair | Equip + grant + unequip, µs per cycle |
| ---: | ---: | ---: |
| 1 | 0.173 [0.162, 0.300] | 0.196 [0.183, 0.216] |
| 100 | 5.319 [5.147, 5.425] | 6.888 [6.780, 7.062] |
| 1,000 | 77.130 [74.288, 81.387] | 96.718 [94.249, 103.804] |
| 5,000 | 1,070.194 [977.804, 1,543.434] | 1,520.535 [1,244.567, 1,750.977] |

### What scales and why

Here `N` is actor count, `I` is item count, `A` is total active grants, and `C` is the number of clauses in one effect definition.

| Operation | Cost in the current implementation |
| --- | --- |
| Actor, item or activation lookup | One `BTreeMap` search in the corresponding collection, with logarithmic growth. Equip checks actor and item and updates the wearer. Equipment lookup is represented by source inspection here. The timing table isolates actor and activation lookups. |
| Effective AC, Fire or Poison query | One actor lookup plus one full activation traversal, filtering for the target actor. Cost grows with `A` even when that actor has few grants. |
| Electrical query | The whole activation tree is traversed for the sum and again for the floor. Reading all three resistances therefore traverses it four times. |
| Activation | Walks `C` clauses once, then scans global grants for both checked AC and Electrical totals before one tree insertion. The activation stores captured points and floor options. Each query reads these fields without walking the original definition clauses. |
| Removal or unequip | Checks both surviving totals before modifying state. Removal deletes by activation ID. Unequip then traverses the activation tree again with `retain` and removes the matching grants, in addition to checking and updating the wearer. |
| Setup with fixed grants per actor | Cumulative validation visits a growing grant tree on construction, so the scan work grows quadratically with this workload. Tree depth, data locality and host variation also affect the observed time. |

The 100-to-1,000 step increases grants tenfold and the three-resistance median by about 13×. It increases the measured activation lookup by about 3×. The stress timings are more variable and include a larger working set. As an illustration, applying the 1,000-actor median of 78.7 µs once per actor projects roughly 79 ms for a three-resistance sweep. That is a multiplication of the measured cost, with no actual frame loop behind it. Single-actor headless use is on a different scale. These results justify testing actor-local iteration in a follow-up, without replacing the tree collections on lookup evidence alone. [Runtime implementation](../src/runtime.rs)

## Resource and VFS timings

The fixture is a legacy DAT with 4,096 lexically ordered entries. The 64 KiB binary payload repeats a deterministic pseudorandom 16 KiB block four times. A separate entry holds 1,024 generated MES records. Other index entries alias the shared plain payload. The four-mount case mounts four copies and validates that the last ordered mount wins the shared path. The hit uses a mixed-case Windows-style path. Copy timings include VFS resolution, DAT reads, and any decompression and validation, streaming to a sink. MES timing includes VFS loading, parsing and dropping the resulting table.

| Operation | Iterations per batch | Median µs per operation [min, max] |
| --- | ---: | ---: |
| DAT index open + drop | 30 | 2,191.5 [2,002.7, 3,328.4] |
| Normalize a path | 100,000 | 0.145 [0.120, 0.199] |
| Resolve hit, one mount | 50,000 | 0.461 [0.412, 0.535] |
| Resolve hit, four mounts | 50,000 | 1.162 [1.096, 1.441] |
| Resolve miss, four mounts | 50,000 | 1.256 [1.213, 1.279] |
| VFS plain copy, 64 KiB | 4,000 | 25.17 [19.26, 36.66] |
| VFS zlib copy, 64 KiB output | 4,000 | 99.26 [91.74, 105.95] |
| VFS MES load + drop, 1,024 records | 300 | 476.51 [386.16, 561.75] |

DAT open validates and normalizes every entry, stores an owned name on each record, and builds a sorted vector of indices for binary search. Sorting has a general logarithmic factor and this ordered fixture does not exercise worst-case order. Mounted archives keep their entry metadata, indices, and buffered input. Payloads are streamed on demand. A zlib read creates a decoder and checks the stream terminator and declared sizes. The measured warmed zlib copy is about four times the plain-copy cost for this fixture. That ratio depends on data and compression. [DAT implementation](../src/dat.rs)

VFS lookup normalizes the request, visits each mount, performs a binary search or loose-tree lookup, and materializes candidates for precedence and reporting. The two measured hit cases expose the mount fan-out. `Vfs::copy` resolves again for callers that already performed a metadata lookup, and DAT extraction normalizes and looks up the chosen entry. `read_bounded` follows that metadata-then-copy path to enforce its byte limit and retain provenance. The normalized name and candidate vector are transient allocations. Loose mounts retain both normalized and host-relative paths and inspect filesystem metadata during indexing. [VFS lookup and copy](../src/vfs.rs) · [Bounded content read](../src/content.rs) · [Loose mount indexing](../src/vfs/loose.rs)

MES parsing and decompression dominate the measured warmed lookups. This baseline provides no basis for a general resource cache. Revisit repeated work if a real consumer repeatedly reads and parses the same resources. Full schematic validation already shares one prototype candidate scan and memoizes MES tables within an audit. Readiness also reuses checks by virtual path within a run. [Schematic audit](../src/content/schematics/audit.rs) · [Readiness](../src/content/schematics/readiness.rs)

Mod discovery opens and validates manifests using filesystem capabilities. Ordering builds a deterministic dependency plan, and applying it mounts archives or indexes loose directories once. Lookup then uses the VFS precedence path and involves no dependency planning. The four-archive timing measures mount fan-out. It does not time package discovery, dependency planning, or a representative large mod set. [Mod catalog](../src/mods.rs) · [Load order](../src/mods/plan.rs)

## Memory and startup

### Runtime and archive memory

Each memory row was sampled in a fresh process. RSS is read from `/proc/self/smaps_rollup` before and after constructing the fixture. Peak is the corresponding `VmHWM` after construction. These are one-shot observations, including resident code, allocator overhead and page reuse. The delta is an approximate incremental footprint, without an allocation census.

| Fixture | Before RSS, KiB | Retained RSS, KiB | RSS delta, KiB | Peak RSS, KiB |
| --- | ---: | ---: | ---: | ---: |
| 1 actor, 4 grants | 2,476 | 2,552 | 76 | 2,552 |
| 100 actors, 400 grants | 2,444 | 2,596 | 152 | 2,596 |
| 1,000 actors, 4,000 grants | 2,520 | 3,408 | 888 | 3,412 |
| 5,000 actors, 20,000 grants | 2,516 | 6,448 | 3,932 | 6,448 |
| DAT index, 4,096 entries | 2,788 | 3,120 | 332 | 3,120 |
| DAT index, 32,768 entries | 2,840 | 5,872 | 3,032 | 5,872 |

On this build, the runtime root is 80 bytes, each `ItemInstance` is 32 bytes, each `EffectActivation` is 64 bytes, and the typed map keys are eight bytes. An actor stores four `i32` values. The fixture's logical runtime payload is about 392 bytes per actor with two items and four grants, including map keys but excluding tree nodes, spare capacity and allocator overhead. The benchmark separately keeps a 16-byte actor/activation ID pair per actor for iteration. Shared definitions are borrowed. Activation inserts a record into the tree with no per-grant clone of the operation vector or namespaced strings. Removing grants can let the tree reclaim nodes, while the allocator can retain the freed pages.

A DAT `Entry` is 56 bytes on this build, a sorted lookup slot is eight bytes, and each fixture path has 21 bytes of owned capacity. This accounts for 340 KiB at 4,096 entries and 2,720 KiB at 32,768 entries, before unused capacity, allocation overhead and the buffered reader. The footprint excludes the fixture's shared binary payloads. RSS deltas can fall below the logical figures when allocations reuse already-resident pages.

### Startup and optional installed content

The release CLI was **1,962,776 bytes** and the benchmark executable **966,264 bytes**. The release CLI's dynamic dependencies were the Linux loader, libc and libgcc_s.

These separate fresh-process measurements ran with warm files and stdout redirected to `/dev/null`. A Python parent used `posix_spawn` and `wait4`, discarded the first execution, and measured spawn through exit. The 13,424 KiB peak reported for every short run is affected by the launcher's footprint. The table omits that figure as an estimate of CLI memory. Installed rows report the observed child high-water values above that floor. The host-specific installed rows are optional, read-only, and may vary with the local content and mount set.

| Command | Runs | Wall ms, median [min, max] | Peak RSS KiB, median [min, max] |
| --- | ---: | ---: | ---: |
| `--help` | 30 | 1.557 [1.280, 5.229] | Below launcher resolution |
| `simulate equipment-demo` | 30 | 1.535 [1.345, 5.245] | Below launcher resolution |
| `mounts <game>` | 9 | 204.313 [199.548, 280.416] | 25,780 [25,596, 27,728] |
| `schematic audit <game> --all-references` | 9 | 234.308 [231.833, 339.454] | 29,220 [27,160, 29,268] |

For wall-clock reproduction without installed data:

```sh
taskset -c 2 python - <<'PY'
import os, statistics, time

exe = "./target/release/arcanum-rs"
with open(os.devnull, "wb") as sink:
    for args, runs in [(["--help"], 30), (["simulate", "equipment-demo"], 30)]:
        samples, peaks = [], []
        for i in range(runs + 1):
            start = time.perf_counter_ns()
            pid = os.posix_spawn(exe, [exe, *args], os.environ,
                                 file_actions=[(os.POSIX_SPAWN_DUP2, sink.fileno(), 1)])
            _, status, usage = os.wait4(pid, 0)
            end = time.perf_counter_ns()
            assert status == 0
            if i:
                samples.append((end - start) / 1_000_000)
                peaks.append(usage.ru_maxrss)
        for values in (samples, peaks):
            print(args, statistics.median(values), min(values), max(values))
PY
```

For the two optional installed measurements, substitute their argument lists and use nine runs. `wait4` exposes `ru_maxrss` on Linux. These separate commands include startup and complete command work. Subtracting one median from another does not isolate a reliable runtime phase.

## Code and dependency review

Line counts include comments and whitespace, with test files counted separately. They identify where responsibility has accumulated and do not measure cyclomatic complexity.

| Area | Lines | Observation |
| --- | ---: | --- |
| Runtime and runtime tests | 478 + 992 | The runtime owns state and effect application. Per-channel getters and setters repeat a small pattern. Checks for valid survivors and subsequent mutation are separated for atomicity. Repeated global scans are the measured concern. Test size reflects lifecycle, ordering, and overflow checks. |
| CLI | 1,412 | Argument dispatch, resource and content inspection, and multiple demos share the file. Several demos repeat fixture setup and reporting while each calls the library for calculations. Moving demo presentation to a module could improve navigation if this file keeps growing. |
| DAT and VFS | 421 + 503 core lines | DAT owns bounded archive parsing and extraction. VFS owns precedence and paths. VFS has 348 additional lines across game discovery, loose mounts and ignore sidecars. Candidate reporting and repeated normalization are visible overhead with measured sub-microsecond normalization. |
| Content loading | 210 core, 722 items, 724 schematics, 351 effects | Content modules keep legacy decoding and provenance separate from runtime. Schematic audit and readiness add 210 and 163 lines. Their shared per-audit work limits duplicate parsing, while individual calls can repeat table loads. |
| Mod loading | 227 core, 126 manifest, 242 plan | Discovery, manifest validation, and edge ordering have separate responsibilities. Mount planning stays out of stat queries and steady-state VFS resolution. |

| Direct dependency | Purpose |
| --- | --- |
| `cap-std` | Capability-relative resource and package filesystem access. |
| `cap-fs-ext` | No-follow directory and file opens for package paths. The separate API usage warrants keeping both capability crates with the current mount safety guarantees. |
| `flate2` | DAT zlib decoding with the existing Rust backend. The harness also reuses its encoder. |
| `serde` | Typed manifest and mount deserialization. |
| `toml` | Bounded manifest parsing with the selected parsing, Serde, and standard-library features. |
| `semver` | Package versions and dependency requirements. |

The host-target Cargo tree listed 34 distinct dependency package versions besides Arcanum-RS under normal edges. None of the direct dependencies was found redundant. No dependency or build profile was added for benchmarking. Loading and asset validation remain separate from the asset-free runtime.

## Follow-up and regression testing

- **Milestone 25** should prototype actor-local activation iteration behind the existing runtime API. Measure the 100- and 1,000-actor cases against this baseline and quantify any extra memory. Compare replacement and removal behavior with the existing regressions for independent sources, signed cancellation, atomic overflow, and addition before floors. Treat the 5,000-actor case as a stress diagnostic.
- Rerun the same workloads in release mode for changes to stat queries or lifecycle paths. Preserve workload dimensions and report medians and spread for several runs on the same host. Keep portable correctness tests as the required gate. Timing and RSS comparisons on shared CI can flag large changes for review without imposing a fixed cross-machine threshold.
- Profile real content consumers before adding resource caches, index sharing, or parser reuse. The narrow evidence today is that warmed index opening, decompression and MES work are measurable, while single-resource lookup is relatively small. Measure cold I/O and an actual mod loadout separately if either becomes a priority.

## Verification

`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, `cargo build`, `cargo build --release` and `git diff --check` passed. An offline sandbox without access to CE, TIG, game assets, or external mods passed all 98 portable tests, ran all five headless demos, and ran the release benchmarks. All 12 optional installed-data tests passed, including `vfs-smoke`. Both release schematic audits reported **119/119**.
