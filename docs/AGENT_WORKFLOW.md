# Agent workflow for Arcanum-RS diagnostics

Use local RPC to reproduce runtime defects and the headless scanner to investigate
original-data compatibility. See [Machine-readable diagnostics](MACHINE_DIAGNOSTICS.md)
for the protocol, socket limits, and command conventions.

## Quick start

For configured developer launches, set `enabled = true` and `rpc = true`
in `[developer]` and run the executable directly. Use the socket path printed
at startup. See [Application configuration](APPLICATION_CONFIG.md) for
path precedence and cleanup.

Run from the repository in a Wayland session with Vulkan available. Use a fresh
private socket directory for each debug instance.

```sh
cargo build --locked --release --features windowed
rpc_dir=$(mktemp -d /tmp/arcanum-rs.XXXXXX)
chmod 700 "$rpc_dir"
mkdir -p target/visual-checks/milestone-59
printf '%s\n' "$rpc_dir/engine.sock"
target/release/arcanum-rs play "$HOME/Work/arcanum-ce-test" --debug \
  --rpc "$rpc_dir/engine.sock" --diagnostics target/visual-checks/milestone-59
```

From a second terminal in the repository, substitute the printed socket path.
Keep this instance running for the whole investigation.

```sh
bin=target/release/arcanum-rs
sock=/tmp/arcanum-rs.REPLACE_ME/engine.sock
rpc() { "$bin" rpc "$sock" "$@"; }
rpc commands
rpc help objects
rpc pos
```

When finished, run `rpc quit`. The engine removes the socket on clean exit.
After it exits, remove the empty directory with `rmdir "$(dirname "$sock")"`.

## Runtime debugging

```sh
rpc maps Tulla
rpc coc Tulla
rpc teleport 28447 31171
rpc pos | jq '.result | {map, player, camera, sector, streaming}'
rpc sectors | jq '.result | {cached, required, speculative, stats}'
rpc objects near 28446 31171 1
rpc objects id 32682017212:7 | jq '.result.objects[] | {
  id, prototype, sector_source, effective, collision, interaction, rendering, issues
}'
rpc resource 'proto/001021 - wall.pro'
rpc resource art/wall/txxbseu1.art 0
```

- `coc` accepts a discovered map name or numeric ID, with optional `X Y`.
  Without coordinates it uses the authored start. `teleport X Y` stays within
  the current map. Check the returned map and tile, then confirm camera readiness
  as described below.
- `sectors` reports cached entries and required and speculative sector IDs.
  Check these and `pos.result.streaming` when investigating a pending load.
- `objects id SECTOR:RECORD` addresses a serialized sector record in the active map.
  Nearby queries also accept `category CATEGORY` and `prototype ID` filters.
  Read `sector_source`, `effective.prototype_source`, and
  `effective.asset_source` for provenance. Inspect `collision`, `interaction`,
  `rendering.state`, and `issues` to see what the engine knows. A missing field
  means the query did not supply that information. `resource` reports the VFS
  `winner`, override `candidates`, and available `decode` information.

```sh
# Toggle collision bypass for mouse routes and developer arrivals.
rpc tcl | jq '.result.collision_bypass'
rpc pos | jq '.result.player | {tile, collision, collision_bypass, navigation}'

# Set the hour (0–23). The game clock continues advancing.
rpc settime 23
rpc pos | jq '.result.world | {hour, world_tile, area, travel}'

# After editing the currently selected UI packages
rpc reloadui
rpc pos | jq '.result.ui | {reload_generation, reload_status}'
```

`world.travel` reports `travel_eligible`, `waypoints`, and `journey` when the
map interface is available. A failed `reloadui` leaves the working UI active
and reports a structured error. For a short event history, use `rpc trace on`
before reproduction, then `rpc events` or `rpc events snapshot_restored`.

For original mobiles, use `rpc objects category mobile` or a concrete
category such as `npc`, `container`, or `armor`. Use `rpc objects original
2:GUIDHEX` when a persistent GUID is known. Inspect effective property
origins, placement, and ownership together. See the
[mobile object reference](ORIGINAL_MOBILE_OBJECTS.md) for installed-data fixtures.

## Runtime inventory

On the base installation, load Bates Mansion Level 1 near the original chest.
Wait for `rpc pos` to report `camera.view_ready` after each `coc`.

```sh
rpc coc 5011 80 102
chest=2:98fd05e54e0ed4118f1d00e02920220c
item=2:5db66ad2fe3ad4118f1d00e02920220c
rpc inventory "$chest"
rpc itemowner "$item"
rpc transfer "$item" "$chest" player 9  # Expected inventory_rejected for the multi-cell item
rpc itemowner "$item"                    # Still in the chest at its previous slot
rpc transfer "$item" "$chest" player 2
rpc inventory
rpc coc 5078 2087 2018
rpc itemowner "$item"                    # Same GUID and player slot on the new map
rpc coc 5011 80 102
rpc drop "$item"
rpc itemowner "$item"                    # World placement at the player's tile
rpc pickup "$item" 2
rpc transfer "$item" player "$chest" 0
rpc inventory "$chest"
```

`inventory` defaults to the player. `itemowner` includes original and current
placement, the instance and prototype sources, and the ART used to determine
the footprint. Grid slots are zero-based in row-major order. Pickups and
transfers without an explicit slot use the first fit. The source and
destination containers must be nearby and accessible for a transfer. A failed
transaction preserves prior ownership and placement.

Player inventory reports list equipment, gold and the four ammunition
counters in Classic order, and derived statistics. Use `equip armor ITEM_ID`
and `unequip armor [SLOT]` to exercise the equipment transaction. Use
`transferqty ITEM_ID FROM TO QUANTITY [SLOT]` to split or transfer a stack.
The destination can be `ground` for partial drops. Generated items use
session IDs beginning with `65535:`. A merged source reports `current.kind`
as `merged` and `current.into` as the surviving item ID. The counter and
quantity of the surviving live item include the merged units.

For an original ammunition fixture, the crates at (2084, 2014) and
(2070, 1998) in Crash Site Cave (5078) contain 15 arrows and 15 bullets
respectively. Drag to the backpack, enter a quantity, and right-click the
remainder to exercise merging. Inspect the quantity, ownership and counters
after each step. Equipped items, generated IDs and stack changes survive
in-session map transitions.

With Classic active, click the bag button and click the authored chest in the
world to exercise the production paths. After each interaction, inspect
`rpc pos | jq '.result.ui.hud | {scale_percent, effective_scale_percent, inventory}'`.
The inventory report includes panel and lens bounds, visible grid bounds,
slots, footprints, resolved icon resources, scroll rows, hover, the active
drag's slot and validity, control bounds, and transaction errors. Use native
press, motion, and release events to test the drag path. Use `itemowner`
before and after rejected placements to compare identity and location.
`pos.result.player.navigation.hovered_item` reports the GUID selected by the
world picker under the pointer. Compare `.result.renderer.stats.preparation.uploaded` and
`.result.renderer.frames_presented` while changing scale and during idle
intervals. Use Vulkan capture for the presented result.

Bates Mansion Level 1 also has original containers at (78, 104) and
(78, 107) whose category is container and whose ART is scenery. The category
controls interaction and the authored ART controls appearance. A clear
diagonal is within container reach, with wall edges and blocked corners
checked during approach and again when opening. Check original flags and
script bindings before treating an access rejection as a rendering or
navigation defect.

## Asynchronous operations

Read the top-level `status` before using `result`. `completed` confirms the
command finished. `accepted` confirms it was queued. For token-bearing results,
poll `rpc operation TOKEN` with a bounded deadline until `completed` or
`failed`. Read `error.code`, `error.message`, and any `error.details` on failure.

`coc`, `teleport`, and `restore` can return `completed` with `view_ready: false`.
The destination is active and presentation follows. Before judging visible
results, confirm the target map and tile and poll
`rpc pos | jq -e '.result.camera.view_ready == true'`. Check `sectors` as well
when the investigation depends on a particular cached sector. Camera readiness
does not imply speculative prefetch finished.

The client exits with 0 for completed or accepted requests, 1 for a structured
failure, and 2 for usage, transport, or protocol errors. The client waits up
to 120 seconds for a response. A timeout or disconnect can leave a dispatched
mutation in effect. Reconnect and query state before retrying, especially for
toggles such as `tcl`. Use `rpc --id N COMMAND ...` to correlate requests.

## Diagnostic snapshots

```sh
snapshot=target/visual-checks/milestone-59/tulla-snapshot.json
rpc snapshot | jq -e '.result' > "$snapshot"
rpc restore "$(cat "$snapshot")"
rpc pos | jq '.result | {map, player, camera, world}'
```

Restore uses the normal location loader and expects the same UI packages and
declared VFS mods. It restores map, tile, facing, game hour, collision bypass,
zoom, UI scale, render toggles, diagnostic overlays, and the selected record
when available. Camera coordinates, viewport dimensions, and cached-sector
references are recorded for context. The camera follows the player after
restore and the window keeps its current size. Snapshots cover reproduction
context. Door changes, inventory and ground-item state, pending interactions,
travel progress, cache ordering, and exact animation phase are outside the
restore scope. A restore within the running session preserves current inventory
changes through the normal map loader.

## Vulkan screenshot capture

Use structured diagnostics for placement, provenance, and state. Capture a
frame when the question depends on actual visual output. After confirming
`camera.view_ready`, request a fresh simple PNG filename.

```sh
capture=$(rpc capture tulla-review.png)
token=$(printf '%s\n' "$capture" | jq -er 'select(.status == "accepted") | .result.token')
rpc operation "$token"
```

If the operation remains `accepted`, poll again until it completes or fails.
The completed result contains `path`, `backend`, and the physical `viewport`.
The file is written under the configured `--diagnostics` directory. Existing
files are protected. Capture records the presented Vulkan view and active UI.
Use `--ui off` at launch when a capture needs the HUD disabled.

## World compatibility scanner

Use the same installed data and override set for comparisons. Build the revision
under investigation in release mode and keep reports and timing files under
the ignored `target/` directory. The full scan needs no running game.

```sh
game="$HOME/Work/arcanum-ce-test"
out=target/visual-checks/milestone-60
mkdir -p "$out"
"$bin" world-scan "$game" --examples 32 > "$out/current.json"

"$bin" world-scan "$game" --map "Tulla Interior" --summary
"$bin" world-scan "$game" --map 5000 --sector 32682017212
"$bin" world-scan "$game" --category portal --group portal_opening_unsupported --examples 3
"$bin" world-scan "$game" --group object_list_rejected --examples 32
"$bin" world-scan "$game" --map 5061 --sector 67108871 | jq '.research.explicit_locations'
jq '{scope, coverage, elapsed_ms}' "$out/current.json"
jq -r '.issues[] | [.code, .occurrences, .affected_resources, .affected_world_sectors] | @tsv' \
  "$out/current.json"
```

`--limit N` bounds authored and template load attempts independently.
`--category` and `--group` filter issues. `--summary` omits representative
examples. Compare unfiltered runs to evaluate changes across the installed data.
Choose a saved report with the same scan scope and original resource set.
Schema 2 preserves the meanings of schema 1 coverage counters and parent
issue codes. The new metadata charge and door target counters have no
schema 1 equivalent. Schema 3 retains the static counters and changes the
mobile decode denominator interpretation from prefix acceptance to full
typed instance framing. Use a schema 3 baseline for equivalent mobile coverage.
Keep changes in evaluated coverage separate from changes in issue counts.

```bash
# Compare coverage before interpreting issue-count changes.
baseline=target/visual-checks/milestone-59/compatibility.json
diff -u <(jq -S '.coverage' "$baseline") \
        <(jq -S '.coverage' "$out/current.json")
diff -u <(jq -r '.issues[] | [.code, .occurrences] | @tsv' "$baseline" | sort) \
        <(jq -r '.issues[] | [.code, .occurrences] | @tsv' "$out/current.json" | sort)
```

Use `.issues[].causes[]` to compare subtype counts by parent code, cause code,
and category. Use `.research.mobile_headers` to distinguish mobile discovery,
header observations, and full instance acceptance. A falling issue count can
result from reduced evaluated coverage. Check `.coverage`, `.scope`, and
verification labels before reporting recovery.

```sh
jq -r '.issues[] | .code as $parent | .causes[]? |
  [$parent, .code, .category, .occurrences] | @tsv' "$out/current.json"
```

See the [tracked baseline summaries](WORLD_COMPATIBILITY_BASELINES.json) if the
full reports are unavailable, and [world compatibility research](WORLD_COMPATIBILITY_RESEARCH.md)
for count definitions, scope, and interpretation.

## Known limitations

- Runtime object queries cover cached sectors and return bounded results.
- Snapshot restoration has the limited scope described above.
- Compatibility scans exercise format decoding and preparation. Visual
  fidelity requires separate verification.
- Dynamically resolved world positions require targeted checks. Template
  scans cover resources and terrain references.
- General script execution remains incomplete. Transition metadata can be
  inspected within the supported subset.

## Recommended debugging workflow

1. Reproduce the reported defect through runtime automation.
2. Query structured engine state.
3. Identify original resource provenance.
4. Determine the generic engine deficiency.
5. Implement the smallest verified reusable correction.
6. Run focused regression tests.
7. Reproduce the same scenario through automation.
8. Capture visual evidence only when relevant.
9. Report remaining limitations.
