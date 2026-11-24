# Machine-readable diagnostics

For development recipes and the Milestone 59 compatibility baseline, see the
[agent workflow guide](AGENT_WORKFLOW.md).

Arcanum-RS uses one developer registry for the console and for local automation.
Both dispatch typed engine requests and consume the same structured result.
The socket worker handles framing and I/O. The Vulkan window event thread
executes the commands and applies gameplay changes.

## Launch and discover

Run this from the repository. Use a new private socket directory per session.

```sh
cargo build --locked --release --features windowed
rpc_dir=$(mktemp -d /tmp/arcanum-rs.XXXXXX)
mkdir -p target/visual-checks/milestone-59
printf '%s\n' "$rpc_dir/engine.sock"
target/release/arcanum-rs play "$HOME/Work/arcanum-ce-test" --debug \
  --rpc "$rpc_dir/engine.sock" --diagnostics target/visual-checks/milestone-59
```

In another terminal, set `sock` to the printed path. The client emits one JSON
response on stdout. Exit code 0 is a completed or accepted request, 1 is a
structured failure, and 2 is a usage, transport, or protocol error.

```sh
bin=target/release/arcanum-rs
"$bin" rpc "$sock" commands
"$bin" rpc "$sock" help objects
"$bin" rpc "$sock" --id 10 trace on
"$bin" rpc "$sock" --id 11 pos
"$bin" rpc "$sock" --id 12 coc Tulla
"$bin" rpc "$sock" sectors
"$bin" rpc "$sock" objects near 28446 31171 1
"$bin" rpc "$sock" objects id 32682017212:7
"$bin" rpc "$sock" resource 'proto/001021 - wall.pro'
"$bin" rpc "$sock" resource art/wall/txxbseu1.art 0
```

`commands` lists aliases, typed argument descriptions and constraints,
mutation and world requirements, and result fields. The original
[console guide](DEVELOPER_CONSOLE.md) explains the gameplay operations.

The protocol is version 1. Send one UTF-8 JSON line and read one JSON response
line per Unix socket connection. Sequential requests reuse the running game.
Set a distinct unsigned integer `id` for correlation. The CLI's `--id`
defaults to 1.

```json
{"version":1,"id":42,"command":"resource","args":["art/wall/txxbseu1.art","0"]}
```

A successful response has the canonical command name, the request ID, a
`completed` or `accepted` status, a `result`, and a `warnings` array.
Failures use `status: "failed"` with `error.code` and `error.message`.
Provenance can appear in `error.details`, for example when a resource has
candidates but none is eligible. Malformed messages can have null IDs.
Examples of codes include `invalid_request`, `unsupported_protocol`,
`invalid_command`, `invalid_arguments`, `missing_resource`,
`unsupported_operation`, `busy`, and `capture_failed`.

The socket directory must belong to the process user and permit no group or
other access. The socket uses mode 0600. Requests are limited to 16 KiB and
responses to 4 MiB. I/O has deadlines and the local request queue is bounded.
Clients may disconnect while a dispatched command is running. Query state
before retrying an uncertain mutation. The engine removes its socket on
normal exit. If an abnormal exit leaves a path, verify the old process has
exited and remove only that socket, or use a fresh directory. `rpc "$sock"
quit` closes the session cleanly.

## State and capture

- `pos` combines player, camera, travel, renderer, streaming and UI state.
  `camera.view_ready` reports whether the current location has presented
  its view. `coc` and `teleport` validate and prepare the destination in
  the shared loader before replacing the active world. The window redraw
  follows. Poll `pos` when an operation requires the presented view.
- `sectors` reports cached, required and speculative sectors, their source
  kind, record coverage, collision availability and bounded issues.
- `objects` combines optional `near X Y RADIUS`, `id SECTOR:RECORD`,
  `category CATEGORY` and `prototype ID` filters. Results are limited to
  128 decoded records and 128 issues from cached sectors. Raster state is
  evaluated against the active scene and viewport. Whole object lists
  rejected by the format reader are represented as sector issues.
- `resource PATH [FRAME]` reports winning VFS provenance and alternatives
  in precedence order. ART frame decode and prototype-header decode are
  explicit. Other resources report the lookup and its untested decode
  status. No asset payload is returned.
- `trace on` enables a 128-entry event history. `events [CATEGORY]`
  returns it and the dropped-event count. Categories include command,
  map load, sector completion, game time, UI reload and snapshot restore.
  Request IDs correlate events generated synchronously during dispatch.
- Frame timings reflect the existing host acquire, preparation, encode,
  submit and present stages. GPU duration is available when timing is
  enabled and supported. Texture and raster memory fields are cache
  estimates. Queries do not request continuous rendering.

Capture saves the actual presented Vulkan surface, with its current world
and UI. Set `--diagnostics` to an existing directory. Use a fresh simple
`.png` filename. Existing files are protected, and relative paths with
directory components are rejected.

```sh
"$bin" rpc "$sock" tcl
"$bin" rpc "$sock" teleport 28447 31171
"$bin" rpc "$sock" settime 23
"$bin" rpc "$sock" pos
"$bin" rpc "$sock" capture tulla-night.png
"$bin" rpc "$sock" operation 1
```

`capture` returns `accepted` and a token. Query that token until completed
or failed. The completed response includes the path, backend and physical
viewport. One capture can be queued at a time. The surface must support
copy-source usage. Capture uses one requested redraw and one GPU readback.
Launch with `--ui off` to capture with the gameplay HUD disabled.

## Reproduction snapshots

```sh
"$bin" rpc "$sock" snapshot | jq '.result' > target/tulla-snapshot.json
"$bin" rpc "$sock" restore "$(cat target/tulla-snapshot.json)"
"$bin" rpc "$sock" pos
"$bin" rpc "$sock" events snapshot_restored
```

The snapshot restores map, tile, facing, current hour, bypass, world zoom,
UI scale, render toggles, overlays, and the selected record when it remains
available. It requires the same UI packages and declared VFS mods. Camera
coordinates, viewport dimensions and cached sector references are captured
for context. The camera is reconstructed by the normal player-follow logic
and the window retains its current size. Animation phase, route progress,
door changes, uncommitted interactions, textures and cache ordering are
outside the restoration scope.

## Headless compatibility scan

```sh
"$bin" world-scan "$HOME/Work/arcanum-ce-test" \
  > target/visual-checks/milestone-59/compatibility.json
"$bin" world-scan "$HOME/Work/arcanum-ce-test" --map 5000 --sector 32682017212
"$bin" world-scan "$HOME/Work/arcanum-ce-test" --map "Tulla Interior" --summary
"$bin" world-scan "$HOME/Work/arcanum-ce-test" --category portal --examples 3
```

The scanner uses the gameplay sector loader, script-transition recognizer,
roof and collision preparation, original map metadata, and VFS overrides.
It scans winning authored sectors and indexed terrain template files. Each
terrain grid adds a lookup check for distinct terrain words. Template
resources are loaded once as standalone fixtures. Dynamic placement at
every possible overworld tile remains untested. Mobile coverage exercises
the existing permanent-NPC decoder. The report distinguishes serialized
records, successfully imported objects, prepared collision and roofs, and
unsupported interactions. It performs no rendering or general script
execution. A successful scan can report compatibility issues.

Use `--limit N` to bound authored and template load attempts independently,
`--group CODE` or `--category CATEGORY` to filter reported issues, and
`--summary` to omit representative examples. Coverage totals remain scoped
to the selected resources. Group codes and ordering are stable for report
comparison. Representative examples expose original resource paths and
record numbers where the loader can determine them.
