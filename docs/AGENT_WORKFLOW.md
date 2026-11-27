# Agent workflow for Arcanum-RS diagnostics

Use local RPC to reproduce runtime defects and the headless scanner to investigate
original-data compatibility. See [Machine-readable diagnostics](MACHINE_DIAGNOSTICS.md)
for the protocol, socket limits, and command conventions.

## Quick start

For configured developer launches, set `enabled = true` and `rpc = true`
in `[developer]` and run the executable directly. Use the socket path printed
at startup. See [Application configuration](APPLICATION_CONFIG.md) for
path precedence and cleanup.

Run from the repository in an X11 or Wayland session with Vulkan available. Use a fresh
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

## Native save diagnostics

Use the actual pause menu and F7/F8 for native input coverage. `pos.result.ui.pause_menu`
reports the active screen, physical control and slot row bounds, selection,
whether a preview is displayed, and any operation error. The save directory
is exposed in `pos.result.ui.save_directory`. For isolated tests, set
`XDG_DATA_HOME` to a private temporary directory before starting the game.
With tracing enabled, `events` can report `save_written`, `save_loaded`,
`save_deleted` and `save_operation_failed`. See [Native savegames](SAVEGAMES.md).

## Native audio and Options

Open **Escape → Options** with native input and use
`pos.result.ui.pause_menu.controls` for physical click bounds. The active
category is in `pos.result.ui.pause_menu.category`. Use `[` or `]` to exercise
the shared scale. The Sound controls have arrow buttons and clickable,
draggable bar fills, with ten steps and zero for mute.

`pos.result.audio` shows configured volumes and mixer gains. The output
reports the audio stream configuration, applied map and scheme IDs, current
hour, active loop paths, and the last 32 started clips with winning VFS
mounts. It also reports the last startup or resource error and read/cache
counters. Audio is reconstructed from the saved location and game hour.
A save does not contain the current playback offset or mixer preferences.
Use an OS audio monitor or listen to the output when confirming samples reach
the device. For location tests, check the output again after
`coc` or a native load and after `settime` changes the game hour.

## Native NPC dialogue

Use `rpc objects category npc` to locate original NPCs and see their effective
`dialog_script` attachment. Inspect `rpc pos` while hovering and clicking the
sprite. The navigation state reports `hovered_npc` and any pending approach.
`dialogue.active` reports the active turn and script provenance.
`dialogue.last_result` reports a rejected entry, rejected response or closure.
For response failures compare `turn`, `response`, `destination`, `effect`,
`stage` and `script_fault` with the active turn and its source.
Use `ui.hud.dialogue_responses` for physical mouse coordinates. Inspect
`audio.output.active_voice` and the winning VFS source in
`audio.output.recent` while a line plays. Use an isolated sound monitor for
sample-level verification. `rpc resource scr/01039constable_owens.scr`
reports SCR structure and the winning mount. `pos.result.scripts` reports
committed global and attachment state. Conversation diagnostics identify the
original entry statement, attachment owner and event, triggerer, and any
explicit continuation that ran. See [NPC dialogue and voice packs](DIALOGUE.md).

For an installed-data fixture, use `rpc coc 5000 90226 84176` and talk to
Constable Owens. Report the blimp crash, then reopen the conversation and
choose the response about having something to tell him. DLG response 431
depends on local flag 3 being clear. Reporting the crash sets that flag and
global flag 2342. Close dialogue before using F7, then use F8 and confirm the
response is still absent. This also distinguishes a saved script consequence
from the session-only meeting condition on the opening reply.

### Shrouded Hills interaction check

Launch `target/release/arcanum-rs` with the normal installed configuration and
developer RPC enabled. Use `coc` to position the player for each check and use
the real mouse for hover, approach and Use. Inspect `rpc pos` before and
after the click.

| Object | Reproduction arrival on map 5000 | Original identity and expected input result |
| --- | --- | --- |
| Doc Roberts | `rpc coc 5000 90213 84148` | `2:0341df77fe12d4118f1d00e02920220c`. Click him to approach and reach line 1. Close with Escape and click again for line 205 in the same session. |
| Constable Owens | `rpc coc 5000 90227 84175` | `2:eb7aed152e8d974391af827b51dcd1ea`. Clicking reaches line 1. |
| Jongle Dunne | `rpc coc 5000 90271 84171` | `2:ddaf30288e52d4118f1d00a0cc6511c6`. Clicking reaches line 1. |
| Wise Woman | `rpc coc 5000 90155 84179` | `2:837d39d717782a41bba337a11146b014`. Click, then choose the barter response to open her stock. |
| Herbalist, Black Root | `rpc coc 5000 70179 92088` | `2:5ce5e69a89ff8f4cb6e02aadf2f4cbf4`. Click, then choose the barter response. This merchant uses a different dialogue and stock source. |
| Lukan's bridge gate | `rpc coc 5000 90100 84189` | `88248157567:54`. Click its leaf to get `door is locked` in the window title. Click the ground toward `(90096,84189)` to check that the player cannot cross the closed gate. |

The gate record at `(90099,84189)` and `map.sbf` come from module
`Arcanum.dat`. Its prototype 2030 comes from loose `data/proto`.
`Arcanum2.dat` supplies `art/portal/xxxe3au0.art` and
`scr/30019unblockshroudedhills.scr`. From the town side, talk to Lukan,
ask to cross the bridge, ask the charge, and pay 1,000 gold. The winning
`dlg/01041lukan.dlg` from `Arcanum5.dat` debits the payment on response
97 and transfers item named 4402 on line 139. Its key code is 3.
Click the gate and click the bridge deck beyond it. Opening with the key
clears the direction-5 collision edge and displays ART frame 3. Closing
restores the edge and frame 0. The key stays in the backpack.

`rpc objects id 88248157567:54` reports the original key ID, matching
carried item ID, lock flag, live pose, collision and Use source.
`rpc inventory` reports carried codes under `item_statistics.key_ids`.
`pos.result.world.travel.sector_blocked` reports the effective overland
sector block. Script 30019 clears it on first use. The town-map departure
rule still applies. Use F7, restart, and use F8 to reload the result.

For a container check, Simon Plough's warehouse barrel at
`(62126,65725)` uses code 14. Its original ID is
`2:50cd22ea203ad4118f1d00e02920220c`. Its key is in the container
at `(62132,65722)`. Click the barrel from the interior, for example
`(62127,65725)`. `rpc inventory ID` reports the matching key and
current access error. Lukan's key alone cannot open the barrel.

For NPCs, `rpc objects original GUID` reports the original instance,
effective placement and flags, loaded ART, blit scale and interaction target.
These NPC instances come from module `Arcanum.dat` and their prototypes
from loose `data/proto`. Their visible ART comes from `Arcanum1.dat`.
Doc's SCR and DLG both come from `Arcanum5.dat`. The other four SCR files
come from `Arcanum2.dat`. Their DLG files come from `Arcanum5.dat` except
the Wise Woman's `03055sh_identify_lady.dlg`, which comes from `Arcanum3.dat`.
The Shrouded Hills placement is the Wise Woman at `(90157,84179)`.
The original NPCs named Herbalist with attachment 1815 are placed elsewhere
on the main map.


## Lockpicking and Unlocking Cantrip

Use a character who carries lockpicks, or who knows the second spell of Conveyance. A new default PC has no purchased spells. Press **C** and learn the first two Conveyance spells. A default human needs one additional point of Willpower for Unlocking Cantrip. Purchase Pick Locks on the technological skill tab and obtain an actual lockpick tool. Keep the player's real saves separate with `XDG_DATA_HOME`.

Press **I**, drag Crude Lockpicks or Lockpicks from the backpack onto the inventory’s **Use** box, then click the door leaf or container. The PC approaches and works on the lock. For Cantrip, click **Spells** on the bottom bar, click the Unlocking Cantrip icon, then click the target. Escape or right-click cancels targeting. A second ordinary click uses the door or container after a successful unlock. The caster faces the target. Escape or right-click during the casting animation interrupts it without spending energy. The blue vial follows current fatigue. Brackets scale the HUD and its targeting controls together.

The gate and barrel above have difficulty 20 and 10 respectively. Inspect `inventory` for signed aptitude, purchased skill points, training, effective level, percentage, college levels, fatigue damage and the recovery deadline. `pos` reports `player.navigation.ability` and the stable ID of the lock under the pointer. The last action includes accepted rolls and costs, critical status, guard notification and the winning tool or spell resource. `objects id SECTOR:RECORD` and `inventory GUID` report authored and effective flags, matching carried keys, and pending lock events. A valid cast that falls short of difficulty still costs fatigue. Compare fatigue before the next recovery tick.

With stock data, the winning `rules/spelllist.mes` in `Arcanum5.dat` supplies the target masks, cost of 10 and portal/container flag operations. The loose `015198 - Generic.pro` (crude) and `015104 - Generic.pro` (standard) give usage bonuses of 0 and 5. CE’s `skill.c` supplies the Dexterity cap, training rules, two rolls and jam consequence. `stat.c`, `critter.c` and `magictech.c` supply the signed aptitude, maximum fatigue, overdraw threshold, mana-store ordering and recovery. `object.c` schedules the one-hour relock and the 24-hour jam reset. Cantrip clears the lock flag directly and schedules no relock of its own.

The installed `rules/spelleyecandy.mes` entries 10 and 12 specify the caster and target overlays, palette, additive blend, purple light and sounds 9020 and 9025. `art/eye_candy/eye_candy.mes` names the two Conjure layers and the Unlock-Hit foreground layer. CE’s `anim.c` signals the cast at the ART action frame. Energy is committed at that frame. A completed attempt plays the target effect even when aptitude falls short. Inspect `pos.result.player.navigation.spell_effects` for active layers and frame indices. The last action retains effect provenance and any preparation error after playback ends.

The M75-R native checks used isolated engine saves to prepare skill points, training, college rank, aptitude, fatigue, equipment, skill-roll seed and starting position. The gate and barrel retained their installed records and the PC carried neither matching key. At aptitude 19 the difficulty-20 gate stayed locked and cost 10 fatigue. At aptitude 20 it unlocked for 10. At aptitude 11 the difficulty-10 barrel unlocked for 10. Interrupting the casting animation spent nothing. The same indoor barrel imposed a light penalty of 15 on the novice and zero on the expert. Timer checks moved the saved clock just before each deadline and let the game process it. Native F7, F8 and the Classic Load Game menu exercised persistence. Guard perception and reactions are tracked in LOCK-001.

## Vendor trading

Launch the configured playable binary with `target/release/arcanum-rs`. Click a merchant and select the authored barter response. Right-click an item for a whole-stack exchange, or drag it between inventories and enter the quantity. Escape cancels a pending drag or quantity entry. Escape or I closes barter and resumes the conversation at the authored destination.

Use `rpc pos` to inspect `dialogue.trade` for the NPC, stock owner, purse, buy-list mount, reaction, retail multiplier and scheduled restock. The HUD inventory reports each side's visible bounds, unit price, quantity and last rejection. Use `rpc inventory GUID` for the backing chest or NPC and `rpc itemowner GUID` for item identity. The native inventory inputs and developer transfers with the active NPC use the same transaction.

Black Root's General Store Clerk is at `(70152,92097)` and Shrouded Hills' Gnome Shopkeeper is at `(90256,84150)` on map 5000. Both use the original rural-store inventory source. The installed buy list resolves to `Arcanum5.dat`, and replenishment uses loose `data/rules/InvenSource.mes`. Check both purse balances and the original stack when testing partial trades. A merchant with one coin can offer it for one bullet. Selling a stack at that price requires enough gold for every unit. Lukan's paid bridge response at `(90105,84188)` debits 1,000 gold and transfers his existing key named 4402.

## Quest logbook

Use the original riddler interactions described in [Quests](QUESTS.md).
`pos.result.quests` reports the resolved journal text, mounting source,
and exact quest state. `pos.result.ui.hud.journal` reports page and control
bounds. Use a Vulkan capture to check the visible spread. `world-scan` reports
quest metadata and text resource coverage without running dialogue.

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
travel progress, script state, cache ordering, and exact animation phase are outside the
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

## Character progression and alignment

Launch `target/release/arcanum-rs` with the installed game directory configured. Open the character sheet with **C** or its top-HUD button. The skill, technology and magick tabs share the point balance on the left. A minus control returns a purchase made during this visit, provided the change preserves prerequisites. **Escape** closes the sheet. **[** and **]** scale it with the HUD. Select a learned schematic in the book and click **Combine** with both components in the backpack. Use the bottom spell button to select a learned college and an executable spell.

Use the `progression` RPC command for the character’s raw and effective values, skill training, XP threshold, signed aptitude and alignment, learned ranks, study requirements, wield scripts and winning MES sources. The `ui.hud.character_sheet` field of `pos` exposes physical controls and purchase or crafting feedback. `world-scan` reports structural spell and learned-schematic coverage in `research.character_progression`. Runtime crafting also resolves the ingredient and product prototypes.

CE’s `stat.c`, `skill.c`, `spell.c`, `tech.c` and `level.c` supply the advancement, derived stat, expertise and unspent-point rules. `rules/spelllist.mes` entry `1000 + 50 × spell + 1` can override the Willpower threshold with its original `IQ:` field. Alignment uses stat 19 on the original −1000 to +1000 scale. The Classic dial displays one tenth of the effective value. DLG **AL** writes change base alignment, **AL** and **NA** conditions inspect effective alignment, and completed quests read their XP class and alignment adjustment from `rules/gamequest.mes`.

For native checks, Doc Roberts offers apprentice Heal training near `coc 5000 90213 84148`. The generic beggar near `coc 5000 76135 48154` offers both a ten-coin donation and a hostile refusal. Reload the save before replaying alternative responses. Electrical degree 2 unlocks schematic **2410** in the tested installation. Copper Ring and Capacitor create a Charged Ring. Inspect Dexterity and speed before equipping it, after equipping it and after native loading. **F7** quicksaves and **F8** quickloads.

M76-R native verification used installed resources on Intel HD 520 Vulkan at 1280×720 and 1920×1080. With the same tool, gate and roll of 14, untrained Pick Locks failed at effective level 0 and succeeded at level 8 after two rank purchases and a Dexterity increase. Unlocking Cantrip at aptitude 10 failed on the difficulty-20 gate and opened the difficulty-10 barrel. At aptitude 20 it opened the gate. At level 15 with the required attributes, the sheet purchased all five Temporal spells and all seven Therapeutics degrees for twelve points, reaching 100 expertise at Doctorate. A worn Charged Ring granted one Dexterity at aptitude −11 and two at −16, with the bonus reversing on refund. A ten-coin donation added 2 alignment and a hostile response subtracted 8. A quest reward granted 800 XP and the character point for reaching level 2. Isolated saves staged the tools, ingredients, positions, skill random stream and starting values. Purchases, training, quest and alignment choices, casts, lock attempts, crafting, equipping, saving and loading used native controls.

## College spell execution

Open the Classic spell panel, click a college and learned rank, then click the target, inventory item or destination requested by the spell. Self-only casts can start on selection. A selected spell remains ready after casting. Release the right mouse button or press Escape to cancel targeting. Classic uses its animated spell cursor while a spell is selected. The top HUD displays maintained spells in casting order. Click a slot to dismiss it. Familiar has no upkeep slot. F7 and F8 save and restore current effects with the rest of the playable session.

Use `rpc progression` to inspect each installed spell's availability, source, program error and known runtime dependencies. Inspect a native action with the existing local client:

```sh
rpc pos | jq '.result.player.navigation | {ability, magick, spell_models, spell_effects}'
```

`ability.last` records the target, outcome, fatigue and item mana expenditure, resistance and effect details. `magick.active` records bindings, summoned IDs, map and tile, callback/upkeep deadlines and visual sequences. `spell_models` reports the sampled frame resource, facing, palette and scale for visible summoned objects and the current player pose. `spell_effects` reports the live EyeCandy. Compare these with the rendered Vulkan frame. An accepted cast or a resolved ART path alone does not verify appearance.

`world-scan` reports the installed program count in `research.character_progression.decoded_spell_programs`. The installed-resource tests resolve aptitude-dependent summon prototypes, Body directional clips and college EyeCandy through the world ART cache.

```sh
ARCANUM_GAME_DIR="$game" cargo test --locked --features windowed --lib installed_college_ -- --ignored
```

M77-R comparisons use the already-running vanilla game as the visual reference and CE/TIG source for resource interpretation. Local full frames, cropped side-by-side comparisons and state at impact, save/load and dismissal are in `target/visual-checks/m77-r/`. The original mage already had a Familiar. Compare its standing model there and use the native cast, repeat rejection and dispel captures to check the RS lifecycle. Conjure Spirit attaches to the original corpse. Create Undead and Resurrect also operate on that actor. For Resurrect, inspect vital damage after the EyeCandy callback. The initial impact report precedes the heal.

Reproduce wall and creature comparisons on open ground using the same caster-to-target direction. Record arrival, sustain and dismissal, then compare with vanilla. For Body spells click to move before dismissing, and check the returning pose after F7/F8. Use an isolated save for fatigue, resistance and persistent effects. Original archives remain read-only.

## Combat

Press `R` to ready or lower weapons, `Space` to switch combat modes and `E` to end the player’s turn. Click an opponent to attack. Release the right mouse button to put away a selected spell before making a physical attack or opening a corpse.

Use `rpc combat` to inspect the encounter mode, participants and factions, current actor, AP, round, opponent, outstanding projectiles, sampled walking, attack, falling and settled poses, remaining experience pools and the most recent 32 events. Attack events include the roll, chance before Dodge, damage by channel (normal, poison, electrical, fire, fatigue), remaining HP, XP and ammunition. `rpc pos` also exposes the encounter under `player.navigation.combat`. Spell impacts and resource charges appear in `player.navigation.ability.last`. Inspect the character and item ownership with `rpc inventory`.

For repeatable native input checks, load isolated saves near the Crash Site wolf (overland tile 92976, 82592), the Shrouded Hills bridge (90104, 84189) or the approach to Kree (40676, 76967). Fixture placement and character preparation establish starting conditions. Click targets in the production window, use the spellbook for hostile casts and inspect the results after impact. Compare the same build, target, mode and equipment across engines. The Kree reference is a level-61 elf mage. Exercise weapon skill and ammunition with separate trained fighters. Check a rejected shot with an empty quiver, a cancelled spell, F7/F8 during a player's turn and corpse inventory after a restart or a map round trip.

At Kree, the recorded Raethrys build had ST 18, DX 21, CO 19, BE 19, IN 20, PE 20, WP 21, CH 16, speed 26, maximum HP 183, maximum fatigue 597 and aptitude 100. Both installations supplied the same worn prototypes. The original installation selected Easy in arcanum.cfg. On Easy, three turn-based Harm casts killed the male Human Bandit at the gate in both engines. In RS the casts dealt 43 HP each, used 4 AP each, and charged the first 10 energy to equipment before spending 5 fatigue. On Moderate the same RS build dealt 29 per cast. The rolled enemy attacks and starting party composition are independent comparisons.

The reference calculations are CE's `combat_attack_cost`, `combat_consume_action_points`, `sort_combat_list` and `combat_dmg` in game/combat.c, `skill_invocation_difficulty` in game/skill.c and `item_weapon_magic_speed` in game/item.c. The winning `rules/spelllist.mes` entry 3755 in Arcanum5.dat supplies Harm's scaled 3–40 normal damage. Bandit actors use the installed loose prototypes 017107 and 017108. The native XP progression uses the contiguous thresholds from the winning `rules/xp_level.mes`, including the levels above 50 in this installation.


The combat pose report includes the original ART path, facing, frame and projected offset. During a fall the sprite still blocks movement. When it settles, the original actor becomes passable and its inventory can be opened from an adjacent tile. The summon owner runs its spell’s end sequence after a summoned creature falls. A lethal spell with a destruction callback removes the subject when its impact effect finishes. Verify actual item transfers, the final rendered pose and the absence or presence of the target after F8.

Hover a living actor to see its health and fatigue in the Classic message window. Compare the gauge values in `ui.hud.npc_gauges` and the world’s picked actor with a Vulkan capture. `ui.hud.spell_cursor_frame` identifies the current native pointer frame. `rpc pos` includes recently played sound IDs and winning resources under `audio.output.recent`. `ability.last.projectile` reports flight time in `travel_ms` and the full delay from cast start in `impact_delay_ms`.

For the Kree feedback checks, the installed Harm target fell using the 13-frame torso death and could be looted. Disintegrate played its projectile and target effect, then retired the bandit. The 40-pixel shot took 70 ms after the 990 ms casting action frame. Quench Life completed its 23-frame, 8 fps target effect during the player's turn and applied its callback once. A separate trained melee fixture in Body of Fire dealt 15 fire damage on a successful unarmed hit. CE's `combat_calc_dmg`, `sfx_item_sound`, `sfx_critter_sound`, `path_make_straight`, `AGupdateAnimMoveStraight` and `intgame_message_window_display_critter` supply the corresponding rules. The local Vulkan sequence captures are under `target/visual-checks/m78-r/`.


A second Kree bandit left five original items after lethal Harm. Advancing the saved decay deadline and loading the game moved all five identities to the corpse tile. Disintegrate on the equivalent encounter left no accessible corpse. A Quench Life save with 2.33 seconds remaining completed its callback after F8. The native audio output recorded the wolf death, humanoid death and material-specific hit resources. A full restart during the wolf fall reached the last of its 15 frames on the saved player turn without replaying the death sound.


## New game, character creation and original movies

Launch the configured windowed executable with `target/release/arcanum-rs`. Use Single Player, New Game, New Character. Exercise the name field, race and portrait selectors, the point-allocation gate, refunds, merchant buy/sell, final confirmation and the transition to the Crash Site. **Escape** skips one movie. Use **F7**, exit through the menus, restart and load from Single Player to test persistence. For session independence, return to the main menu from the loaded game and create a second character with a different background.

In `rpc pos`, `ui.pause_menu` reports the current screen, installed creation data sources, preview, active buttons and rejection messages. During allocation, inspect `player.statistics` and `ui.hud.character_sheet`. During shopping, inspect `ui.hud.inventory`. The native cursor asset, frame, winning mount and hot spot are in `ui.hud.cursor`. The surface capture excludes the system cursor. Inspect the actual window-system pointer to verify its pixels and hot spot. While a movie plays, `ui.cinematic` reports the active resource, queue, frame index, audio clock and number of decoded PCM samples.

The installed campaign’s `movies/movies.mes` maps IDs 1 and 7 to `00069.bik` and `01138.bik`. The installed original executable (SHA-256 `fd244584c4a40e1fca1013d6bf603e70a991491157f51d4d3a273c5bc6ab461e`, HighRes backup) writes 1 and 7 into the two teleport movie fields at addresses `0x541411` and `0x541436`. These match CE’s `sub_5412E0` in `mainmenu_ui.c`. The original teleport handler calls the movie player at `0x4d34be` and `0x4d34d5`, using the first and second fields respectively, with audio track zero. The Single Player intro uses the same order. CE startup selects movie 8 (`50000.bik`) after the optional Sierra and Troika logos. Asset overrides apply independently to each clip. Unavailable or undecodable clips advance the sequence.

Windowed builds pin `infinitier_bik_decoder` 0.1.0, GPL-3.0-or-later. Its authors identify the video and audio implementations as Rust ports of the FFmpeg 6.1 Bink sources. The engine uses the crate’s header parser and codecs, demultiplexes all audio tracks and plays track zero through the existing mixer. The decoder’s direct dependencies are bitflags, hound, infinitier_datasource, libm, log and thiserror. Linux and Windows use the same Rust code. Game data stays in the installed VFS. Release packaging must carry the applicable licence, notices and corresponding source for GPL-compatible distribution. [Published decoder source and provenance](https://github.com/ufoscout/infinitier/tree/7ca7d7c10548e2955442d20f29d4d7b5c36f365c/src/codecs/bik_decoder)


Native checks on Intel HD 520 Vulkan used independently created characters. A male Half-Ogre at IN 4 and a female human Super Model at IN 4 received Virgil’s low-Intelligence response and the low-Intelligence quest and rumor descriptions. Spending one point on a second Super Model’s Intelligence changed all three selections at IN 5. A female human Idiot Savant at functional IN 14 retained speech IN 1, the normal journal text and zero starting gold. The installed chemistry manual test reads the original insert/remove scripts and checks a bonus of one for the Savant. Native shop tests covered an unaffordable purchase, resale, re-purchase and a dagger equipped on arrival. A full process restart and Load Game restored the selected portrait, dagger, purse, background, spell and journal data.

Both post-confirmation movies completed and both could be skipped individually. On the test machine each produced 44.1 kHz stereo output. The original arrow’s measured hot spot was (0, 3), and the animated spell pointer’s was (1, 3). At 2× world zoom a native click cast Strength of Earth on Virgil and retained spell selection until right-click cancellation. A new character bought Lockpicks from the starting merchant and dragged them onto Use at the Crash Site. The native pointer switched to `skillcur.art` with hot spot (2, 3). Escape returned it to the original arrow.

The installed-data decoder check read all eight campaign Bink files through the VFS to their final frames with PCM audio, including the two-track movie `02112.bik`. A separate native run with a missing first movie and a truncated second movie reached the Crash Site with the selected Field Notes HUD. This run used an isolated resource override. The completion, skip and audio checks used the normal installation.
