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

## Milestone 25 actor-local activation iteration

### Design and invariants

Each actor now owns a `Vec<ActivationId>` in increasing ID order. The
existing global `BTreeMap` owns the full records and serves direct ID lookup.
AC, resistance queries, and checked totals resolve only IDs in the relevant
actor's vector. With `k` grants on that actor and `A` grants in the runtime,
the traversal is over `k` IDs with logarithmic global lookups. Electrical
resistance still traverses the local list twice to calculate additions and
then floors. There are no remaining global activation traversals in these
runtime paths.

Validation precedes mutation. Activation inserts the record and appends its
ID. Single removal locates the sorted ID, shifts later IDs in the vector,
and deletes the record. Unequip scans only the wearer's vector, removes that
source's records, and retains surviving IDs in their original order. It
clears equipment ownership after those removals. Overflow, an unsupported
operation, or an invalid handle leaves both collections and equipment
associations intact. The queries still use wide signed sums, check before
applying floors, and clamp only the final ordinary resistance result.
[Runtime implementation](../src/runtime.rs) · [Ownership invariants](ARCHITECTURE.md#headless-effect-activation-ownership)

### Paired benchmark method

The Milestone 24 tables above are historical. This comparison reruns the
unmodified runtime at `b6a07a7` and the actor-local runtime using the same
updated harness. Both executables ran consecutively on the same i5-6300U,
pinned to logical CPU 2, using Rust 1.100.0 and the default release bench
profile. The governor was `powersave`. Other host activity was present,
particularly in the stress batches. Host timing results are descriptive.
The offline sandbox run is a separate verification.

The first four cases retain four grants and two items per actor. The dense
case has four actors and 256 grants per actor. The dense case uses 2,000
operations per timed batch and 20,000 for direct lookups. Setup occurs
outside timing. The harness now also times each resistance channel.
Isolated activation, removal and unequip timings bracket just that
operation, with the inverse or setup operation outside its timed interval.
The clock-read overhead is included in those isolated timings. The batch
pair and equipment cycle are directly comparable to the original harness.

Use a disk-backed benchmark directory with enough space for separate builds.
The resource fixture honors `TMPDIR` through `std::env::temp_dir` and removes
its scratch directory on normal exit. The runtime-only workloads create no
fixture files. For example, from the Milestone 25 worktree:

```sh
root=$(mktemp -d "${ARCANUM_BENCH_TMPDIR:-$PWD/target}/arcanum-m25.XXXXXX")
trap 'find "$root" -depth -delete' EXIT
mkdir -p "$root/before" "$root/tmp"
git archive b6a07a7 | tar -x -C "$root/before"
cp benches/baseline.rs "$root/before/benches/baseline.rs"
export TMPDIR="$root/tmp"
taskset -c 2 cargo bench --offline --locked --bench baseline \
  --manifest-path "$root/before/Cargo.toml" -- runtime
taskset -c 2 cargo bench --offline --locked --bench baseline -- runtime
```

Create the chosen parent directory first. Use the same two commands with
`-- runtime-memory 1000` in place of `-- runtime` for fresh-process memory
observations.

### Timings

Each cell shows the before and after medians in µs per operation. Each
workload used one warm-up batch and seven samples. The 1, 100, 1,000 and
5,000 actor cases used 100,000, 10,000, 2,000 and 400 operations per sample,
respectively, with ten times those counts for direct lookup. Query results
are consumed with `black_box`.

| Actors × grants per actor | Effective AC | AC speedup | Three resistance queries | Resistance speedup |
|---|---:|---:|---:|---:|
| 1 × 4 | 0.0226 → 0.0233 | 0.97× | 0.086 → 0.084 | 1.02× |
| 100 × 4 | 1.591 → 0.126 | 12.6× | 6.738 → 0.919 | 7.3× |
| 1,000 × 4 | 21.541 → 0.284 | 75.8× | 88.442 → 1.198 | 73.8× |
| 5,000 × 4 | 321.244 → 0.323 | 996× | 1,456.048 → 1.187 | 1,227× |
| 4 × 256 | 5.303 → 10.711 | 0.50× | 31.280 → 54.888 | 0.57× |

| Actors × grants per actor | Fire | Electrical | Poison | Direct activation lookup |
|---|---:|---:|---:|---:|
| 1 × 4 | 0.023 → 0.023 | 0.042 → 0.042 | 0.023 → 0.023 | 0.010 → 0.011 |
| 100 × 4 | 1.646 → 0.155 | 3.383 → 0.281 | 1.788 → 0.122 | 0.023 → 0.036 |
| 1,000 × 4 | 20.451 → 0.292 | 44.697 → 0.541 | 35.217 → 0.278 | 0.077 → 0.090 |
| 5,000 × 4 | 274.137 → 0.899 | 570.842 → 0.716 | 328.800 → 0.381 | 0.163 → 0.091 |
| 4 × 256 | 5.312 → 11.670 | 15.272 → 26.757 | 6.810 → 10.730 | 0.038 → 0.027 |

At 1,000 actors, the seven AC samples ranged from 20.41–22.21 µs before
and 0.24–0.76 µs after. The three-query batch ranged from 85.58–94.18 µs
before and 0.94–1.56 µs after. At 5,000 actors those ranges were
286.40–447.93 µs and 0.31–0.42 µs for AC, and 1,398.16–1,896.08 µs and
1.14–1.31 µs for the three queries. Direct ID lookup uses the same map
lookup in both revisions. Its variable measurements include changes in
cache conditions and host noise.

| Actors × grants per actor | Activate | Remove activation | Unequip | Activate + remove | Equip + grant + unequip |
|---|---:|---:|---:|---:|---:|
| 1 × 4 | 0.114 → 0.138 | 0.105 → 0.156 | 0.133 → 0.178 | 0.168 → 0.212 | 0.210 → 0.279 |
| 100 × 4 | 3.216 → 0.286 | 2.854 → 0.275 | 4.522 → 0.437 | 5.824 → 0.483 | 7.732 → 0.547 |
| 1,000 × 4 | 74.657 → 0.396 | 71.230 → 0.354 | 66.161 → 0.453 | 94.021 → 0.527 | 136.257 → 0.693 |
| 5,000 × 4 | 710.458 → 0.297 | 609.978 → 0.305 | 974.985 → 0.536 | 1,369.805 → 0.521 | 1,399.479 → 0.720 |
| 4 × 256 | 13.614 → 19.914 | 18.809 → 19.769 | 16.476 → 34.422 | 23.420 → 39.700 | 40.814 → 51.863 |

Mutation timings target the first actor in an already-populated runtime.
Each timed unequip removes one grant from the extra source. The runtime
tests exercise unequip with several grants from one source. The isolated
times and batch-cycle totals come from separate measurement loops.

Fixture setup took approximately 88.8 → 4.1 ms at 1,000 actors and
9,763 → 21.2 ms at 5,000 actors. These are single observations for
constructing the synthetic runtime and its grants, separate from the
seven-sample query and mutation measurements.

### Memory and scaling

Linux `/proc/self/status` provides coarse process RSS. Each cell below
is the median of three fresh processes. The setup increase subtracts
each process's pre-setup RSS before taking the median. That figure
includes the benchmark fixture, allocator and page-residency effects.

| Actors × grants per actor | Retained RSS before → after, KiB | Setup increase before → after, KiB |
|---|---:|---:|
| 1 × 4 | 2,580 → 2,632 | 132 → 132 |
| 100 × 4 | 2,704 → 2,720 | 208 → 216 |
| 1,000 × 4 | 3,400 → 3,488 | 888 → 980 |
| 5,000 × 4 | 6,380 → 6,836 | 3,868 → 4,388 |

On this 64-bit host the vector header is 24 bytes per actor and each ID
is 8 bytes. An empty vector has no heap allocation. Four IDs at capacity
four account for 56 bytes of header and element storage per populated
actor, about 273 KiB for 5,000 actors. The larger process-level difference
includes allocator and tree node overhead and is subject to RSS variation.
The runtime, item instance and activation records remained 80, 32 and
64 bytes respectively.

With *k* local activations and *A* total activations, each local pass
does *k* ordered ID reads and *k* authoritative tree lookups, with
`O(k log A)` lookup work after locating the actor. Electrical queries
currently make two local passes, one for additions and one for floors.
Validation before mutations also makes local passes. Individual removal
uses a binary search and shifts the remaining IDs. Unequip filters only
the owner's list and deletes matching records from the global tree.
Production runtime queries and mutations have no full activation-map scans.

The many-actor, few-grant workloads benefit enough to justify one
actor-local vector and its update invariant. Single-actor query timings
are roughly unchanged, while single-actor mutations take tens of additional
nanoseconds. With 256 grants on each of only four actors, repeated tree
lookups cost more than walking the prior global tree. Dense AC queries
took about twice as long, and unequip took about twice as long as well.
Treat this as a known stress-case regression. Measure grant counts from
the next runtime consumer before tuning that shape. If repeated Electrical
queries still dominate, measure a single local pass that accumulates the
sum and strongest floor, preserving the overflow check before applying
the floor. Keep both workloads in future comparisons.

### Verification

Formatting, Clippy with warnings denied, 99 portable tests, debug and
release builds, and `git diff --check` passed. A network-isolated
sandbox with the project, Rust toolchain and cached crates ran all 99
portable tests, all five demos and the complete release benchmark.
The sandbox had no CE, TIG, original assets or external mod directories.
All 12 optional installed-data tests passed, including `vfs-smoke`.
Reference validation and supported-content readiness each passed 119/119
learned and found schematics.

## Milestone 50 consolidation

### Matched workloads

Measurements use two offline optimized `--features windowed` binaries from
immediately before and after the three stabilization changes. The same
ThinkPad X270 ran both builds with its i5-6300U, Intel HD Graphics 520,
16 GiB RAM, kernel 7.2.15-1-omarchy, Rust 1.100.0 and Mesa 26.2.5 over
native Wayland. The Vulkan device was the Intel integrated GPU. The frame
timings below are independent of the offscreen pixel-readback tests.

- **Cold preparation.** Run `art world-inspect` in a fresh process per
  sample, seven times per scene with the OS file cache left enabled.
  Bates Mansion uses sector (1,1), camera (91,79) and hour 12.
  K’na Tha's brazier court uses sector (12,12), camera (798,786) and
  hour 0. Both use 1280×720. The reported load timer covers sector
  preparation. Object import and ART timings can overlap within that
  timer and should not be added to it.
- **Streaming.** Replay five runs along the original K’na Tha sector
  strip from (5,14) toward (18,14) in 2,560-by-1,280-pixel isometric
  steps and return to the first view. Use a 1280×720 viewport, wait on
  worker completion and use zero direction for a reproducible
  demand-only cache test. Run 250,000 unchanged ready-view calls per
  replay to measure scheduling in isolation.
- **Interactive rendering.** Run `art world` with the same scene
  positions, sizes and lighting and collect `--trace` output. For Bates,
  use `--animation-ms 90 --light-probe 90 79`, warm both light positions,
  then collect 60 alternating one-tile light moves. For the court, use
  live animation, warm the visible flame frames and sample 4.5 seconds
  of flame-driven redraws. Use the same input spacing for both backends
  and both builds. Vulkan completion is the traced wait through GPU
  submission completion. It includes host work and differs from the
  timestamped sprite pass and from the host return from `present`.
  Physical display scanout is outside these timers.

### Regression scenes

| Case | Primary check |
| --- | --- |
| Bates Mansion (91,79), lit and unlit | 268-wall audit, portals, furniture, dynamic-light move and exclusion, fixed animation time |
| Bates kitchen mobile (80,81) | Original shadow mask, overlap and clipping, CPU/GPU equality during light updates |
| K’na Tha wilderness and populated seam around (383,928) | Real adjacent sectors, global ordering, resize, eviction and repeatability |
| K’na Tha Forest Passage and brazier court around (798,786) | Original surfaces, static and object lighting, saturating flame blend, live deadline |
| K’na Tha rabbit at (420,814) | Finite fidget, direction and pivots, fixed-time comparison |
| K’na Tha boundary at (575,928) | Explicit missing sector and stable viewer background |

Run the headless regression suites with `cargo test --offline --locked`
and `cargo test --offline --locked --all-features`. Use
`ARCANUM_GAME_DIR` and `ARCANUM_MODS_DIR` with `cargo test --offline
--locked -- --ignored` for installed-data checks. On a hardware Wayland
host, run `cargo test --offline --locked --release --features windowed --lib
gpu::tests -- --ignored --test-threads=1` with `ARCANUM_GAME_DIR` and
the prototype’s historical backend selector. Those parity tests reported the
fixture, animation time, coordinate and colors of the first wrong pixel.
Native captures and per-stage timing traces belong under
`target/visual-checks/` and stay local.

### Consolidation results

| Fresh-process CPU stage | Before median | After median |
| --- | ---: | ---: |
| Bates sector preparation | 72.46 ms | 47.51 ms |
| Bates object import | 41.36 ms | 20.52 ms |
| Court sector preparation | 55.16 ms | 37.26 ms |
| Court object import | 36.40 ms | 16.03 ms |

Bates ART read/decode medians were 27.16/14.50 ms before and
25.84/13.71 ms after. Court ART read/decode medians were
19.03/6.95 ms before and 20.66/7.20 ms after. The largest
improvement was in object import, where indexed DAT prefix matches
replace repeated archive scans. The consolidated build's cold
sector-preparation maxima were 48.65 ms for Bates and 38.41 ms for
the court in the paired repeat.

| Streaming or scheduling stage | Before median | After median |
| --- | ---: | ---: |
| Unchanged ready-view scheduling, isolated | 1.893 µs | 0.065 µs |
| Outbound sector traversal, including worker readiness | 114.77 ms | 48.01 ms |
| Back to initial view after eviction | 19.04 ms | 7.11 ms |
| Bates Vulkan redraw sector scheduling | 6.56 µs | 0.14 µs |
| Court Vulkan redraw sector scheduling | 6.92 µs | 0.31 µs |

The traversal read or attempted 15 sectors, evicted three entries from
the 12-slot cache and decoded zero ART frames on the return in both
builds. Waiting for worker readiness in the replay measures elapsed wall
time. The Wayland event thread continued reporting zero synchronous
loading wait. A required sector can still delay publication of a new
view while the last complete frame remains visible.

| Warm 1280×720 workload | Before median / p95 / max | After median / p95 / max |
| --- | ---: | ---: |
| Bates CPU compose | 13.52 / 13.89 / 18.10 ms | 13.94 / 14.82 / 21.10 ms |
| Bates CPU compose, pack and submit | 15.16 / 15.54 / 19.82 ms | 15.59 / 16.46 / 22.80 ms |
| Bates Vulkan completion | 3.52 / 4.21 / 4.77 ms | 3.47 / 4.28 / 5.09 ms |
| Bates Vulkan timestamped sprite pass | 1.73 / 2.34 / 2.92 ms | 1.74 / 2.04 / 2.52 ms |
| Court CPU compose | 9.76 / 10.93 / 22.23 ms | 9.91 / 11.27 / 11.36 ms |
| Court CPU compose, pack and submit | 11.43 / 12.61 / 24.04 ms | 11.59 / 12.94 / 13.12 ms |
| Court Vulkan completion | 3.04 / 3.76 / 4.53 ms | 3.00 / 3.49 / 3.76 ms |
| Court Vulkan timestamped sprite pass | 1.18 / 1.50 / 1.90 ms | 1.17 / 1.55 / 1.60 ms |

Vulkan host submit and presentation-call medians were 0.127/0.165 ms
before and 0.127/0.162 ms after in Bates and 0.139/0.166 ms before
and 0.133/0.156 ms after in the court. Vulkan shadow selection during
the Bates probe moves took 4.54 → 4.76 µs median. The whole light
update took 0.164 → 0.163 ms median. There were no warm texture
uploads in either sampled GPU workload and no sector reads or ART
decodes during the 60 light moves. The court presented 50 traced frames
in each 4.5-second sample. A separate stationary Bates capture submitted
zero additional Wayland buffers across the two three-second idle
intervals per backend.

After resize and Home the i915 resident graphics counter was unchanged
at 64,676 KiB for Bates and 64,608 KiB for the court. Actual texture
payloads are smaller than this driver counter. At the matched initial
1,536×864 compositor-selected surface, both builds uploaded 157 Bates
textures (3,223,064 payload bytes), with 11.18 → 11.28 ms observed
upload time in the paired run. The paired warm measurements use only
1280×720 frames. CPU composition medians were slightly slower in the
repeat, with mixed tail results across runs. The selected changes do
not alter the CPU compositing inner loop. The measurements support
faster preparation and less scheduler work. They do not establish a
composition speedup or a sustained frame-rate claim.

Real Wayland PNGs matched exactly between the before and after builds
for fixed-time Bates, and between CPU and Vulkan for Bates, the brazier
court, the rabbit, the populated seam and the missing-sector boundary.
Pan, resize and Home restored the fixed-time images. OpenGL matched the
CPU captures for the seam and rabbit, including the resized view.
Synthetic fractional-alpha edges retain the existing one-byte tolerance
for two known fixture pixels. Original scene comparisons require exact
agreement.
