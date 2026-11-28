# Arcanum-RS — Known Issues

M83-R party and follower mechanics passed milestone acceptance. Update this register as milestones are completed. This is a defect backlog, not a list of every unimplemented feature.

## Triage

- **P0 — Critical:** crashes, data loss, duplication, corrupted state or unsafe persistence. Fix immediately.
- **P1 — Blocking:** defects that prevent ordinary gameplay or invalidate the current milestone. Fix before merging the affected milestone.
- **P2 — Compatibility:** nonblocking visual, behavioral or content discrepancies. Defer to the relevant subsystem or the Grand Compatibility Sweep unless a shared correction is already in scope.

Status: **Open**, **Needs reproduction**, **Deferred**, or **Resolved**. Keep reproduction steps, evidence and dependencies current. Do not mark an issue resolved without verifying the affected production path.

## Open issues

- **M79 character sheet (P2, open).** The Classic sheet omits the player’s name.
- **Moving shadows (P2, open).** NPC shadows can lose their anchor while the actor moves.
- Configurable floating damage numbers remain an optional future enhancement.
- **Audio output (P2, Needs reproduction).** One native Linux session repeatedly logged an ALSA `poll() returned POLLERR` after combat. The log reached 75 MiB before clean exit. A fresh launch completed the same gameplay checks. The device condition and recovery path need reproduction.

### PARTY-001 — Follower compatibility

- **Resolved in native play.** Original dialogue can establish and release an NPC's leader, including a waiting follower who resumes or leaves. Virgil followed from the Crash Site through the cave and overland to Shrouded Hills. His atlas instructions now discover the intended destinations. His staff retained its identity through removal, return and travel. Sogg’s own greeting override and Charisma branches work in the inn. In a saved-state replay his original AI packet made him leave when his reaction fell below its threshold. Vollinger joined through his original Dernholm dialogue.
- **Resolved in combat replays.** Recruited actors use the encounter scheduler, original AI packet and learned spells. Virgil used Minor Healing and earned kill credit in all three modes. Vollinger expended ammunition. Casting the original Resurrection spell on Virgil restored the same actor and party portrait. A summoned Familiar accompanied its caster through the cave exit, survived restart, fought and gained a level when its caster leveled.
- **P2, open.** Follower casting handles direct and callback vital effects. Tactical selection of maintained buffs, projectile spells and inventory spell devices needs broader caster support. Complex quest-specific SCR actions, interjections involving a second dialogue speaker and unsupported equipment scripts retain their interpreter diagnostics.

### PARTY-002 — Follower gait repeatedly stops and restarts

- **P2 · Open.** During ordinary following, the player sees the follower’s movement animation repeatedly stop and restart.
- **Reproduce.** Recruit Virgil at the Crash Site. Outside combat, click a distant point on clear ground once. Watch his gait while he is still closing the distance, then let him catch up.
- **Expected.** The walk or run cycle continues across tile boundaries while the follower is moving. It settles to idle when the follower stops within formation range.
- **Investigate.** Compare movement orders and path replanning with actor velocity and animation frame progression at each visible pause.

### PARTY-003 — Classic follower inventory lacks the equipment paperdoll toggle

- **P2 · Open.** Right-click Virgil’s party portrait and choose **Inventory** while he has his staff equipped. Look for the vanilla/CE control that switches the follower’s inventory to the equipment paperdoll. The toggle is missing.
- **Expected.** The toggle shows the selected follower’s worn equipment, including Virgil’s staff in the weapon slot, and allows switching back to his carried items.
- **Investigate.** Compare the Classic follower inventory controls with the vanilla/CE layout and input handling. The authoritative equipment slots already exist and can be inspected with `rpc inventory GUID` using the follower’s identity.

### FAMILIAR-001 — Fluorescent green Familiar

- **Priority P2. Status Needs reproduction.** The player reports that the Familiar uses the expected creature sprite but appears fluorescent green.
- Compare the same initial form and caster aptitude in vanilla and Arcanum-RS using the installed resources. Record the winning prototype and ART, creature AID, selected palette and summon appearance state. Palette selection, ART palette interpretation, the creature AID and the summoned object's appearance state are hypotheses. The cause is open.

### CAM-002 — Camera scroll distance from the player

- **Priority** P2. **Status** Needs reproduction.
- The player reports that manual panning can continue far across the map after the player sprite leaves the viewport. They recall vanilla at 800×600 stopping the camera when the sprite reaches the viewport edge.
- With follow off and the player stationary, pan in each direction in vanilla at 800×600 and record where scrolling stops relative to the sprite. Check the rule in CE, then compare viewport sizes and world zoom levels to determine what sets the boundary. Verify the rule before changing behavior. Preserve independent camera positioning and optional follow mode.

### SAVE-001 — Zephyr passengers appear to die again on return to the Crash Site

- **Status** Needs reproduction.
- After loading a save and returning to the Crash Site, the player saw Zephyr passengers who had already become corpses appear to die again. The save may predate the recent New Game changes.
- On a fresh campaign, record a passenger’s corpse state and the identity and owner of an item taken from the body. Leave the Crash Site, save, reload and return. Repeat with the older save if available. Compare the visible death animation with heartbeat and death script execution, persisted event and corpse state, and item ownership across load and map re-entry.
- Escalate to **P0** if a fresh save shows state corruption or inventory duplication.

### COMBAT-001 — Remaining combat compatibility

- **Priority P2. Status open.** The stock CE skill rolls, armor and resistance, AP costs, NPC-first turn list, critical damage bonuses and base XP pool rules underpin real-time combat and both turn-based speeds. The Born Under a Sign background supplies both critical severity modifiers. Its hit modifier feeds the original bonus-damage rolls. Its fumble modifier is retained for the remaining weapon and ammunition damage rules. Detailed critical locations, equipment damage and dropped weapons, special critical-chart injuries, ambient light penalties and tactical NPC spell and weapon selection still need coverage. Real-time attack intervals use weapon cost and actor speed. NPCs and summons play the directional walking, attack and death sequences available for their current models. The resolver supports ordinary and torso deaths with the original fallback. Additional injury-specific death variants depend on critical-chart support. Ordinary combat currently sets facing directly when starting an action. CE also has a separate incremental rotation goal for ordinary turns. Fast turns use the final facing directly.
- Recruitable followers use the original dialogue, party orders, equipment, travel and kill credit. An allied summon can fight using its current form and ownership. Encounters use original hostility, sight, faction and supported script attachments. Some original dying and encounter scripts use operations beyond the script interpreter, reported by `rpc combat`.
- **Installed regression baseline.** The M79-R installed-data run reproduced the pre-M78 failures in the Bates lighting composition, the dining-room mobile shadow probe and the Kna Tha rabbit position. The other 74 installed checks passed, including the external VFS smoke check with its mod fixture. M83-R reproduced those same three failures in the 43 installed library checks. The other 40 passed, as did all 11 installed prototype and effect checks. The dropped-item eviction assertion follows the item's original identity through the rendered pickup target.

### PROG-001 — Remaining character and ability execution

- **Priority** P2. **Status** Deferred.
- The Classic spellbook executes the installed programs for the 16 colleges through the casting and progression systems. Remaining spell dependencies are tracked in MAG-001.
- The learned schematic book supports the 56 degree recipes through the installed recipe and inventory definitions. Found schematics, followers supplying components, and powered-device use need further runtime support. Recovery consumables use their installed Use scripts and Item_Used programs. Conditional equipment scripts outside the supported reversible effect operations report the winning script when blocked.
- Manual point allocation, original skill training tiers, dialogue, quest and combat XP, and kill alignment operate for the player. Followers spend earned points using their installed auto-level schemes. Player auto-leveling, pickpocket alignment and college mastery quests remain deferred. Some background and item effect operators remain outside the statistic subset and appear in character diagnostics.

### MAG-001 — Spell interactions with combat, party and environmental systems

- **Priority** P1. **Status** Open for full M77-R acceptance.
- The interpreter reads all 80 original programs. Living targets, resistance, health and fatigue, movement, temporary effects, summons, forms and dispel use session-owned state. Combat consumes fear, hostility, paralysis and control flags. Controlled actors and summons select nearby hostile participants. Unarmed strikes use the current form and applicable equipment bonuses. Active summoned creatures accompany the caster through map transitions. Familiar retains its generated identity and installed aptitude-selected form. Spell maintenance, control bindings and NPC recruitment retain their respective lifetimes.
- Damage to carried and loose items has a persistent structural-damage field. Disintegrate can damage or destroy a portable item. Removing an immovable object, updating its collision and running its destruction script require the breakable-world-object lifecycle. The player renderer reads sinkable terrain and the water-walking flag set by Body of Air and Body of Water. Native shore transitions, NPC wading and water shadows remain unverified. Detect Traps needs trap instances and their discovery state. Player transformations store or drop worn gear according to available backpack space and re-equip previously worn items still carried. For authored NPCs, the combat resolver reads equipped armor and resistance. Full NPC loadout restoration after a transformation and selection of spell devices still need coverage. Recruited actors can equip a better usable weapon and consume its ammunition.
- Spell-specific results from the native input checks, including maintained state and renderer provenance, are available in the local M77-R visual captures. Representative casts covered every college. The original gate still resists Unlocking Cantrip at aptitude 10 and opens at 20 for ten fatigue each. A controlled summon and the player each receive one Regenerate binding, with its caster EyeCandy on the player.

### DLG-001 — Dialogue requires unimplemented script and game state

- **Priority:** P2 | **Status:** Deferred
- The initial SCR instruction subset and DLG script-state effects are
  implemented. Unsupported instructions, mixed effects, implicit exit
  handlers, and alternate dialogue overrides still need their original
  gameplay dependencies. The Bates house guards use equipment and combat
  checks and floating lines.
- Direct quest state conditions are implemented. Generated quest responses,
  reputation and encounter checks require broader gameplay support. Party presence, waiting and membership checks use the saved leader relationships. Skill, training and alignment conditions read the acting character.
  Initial reaction uses the original base and available character modifiers.
  Authored reaction changes persist in the NPC account. Persistent meeting history and live AI changes are pending.
- Some greetings and response directives need class-, race-, quest- or
  service-specific dialogue. Companion recruitment uses the installed join and refusal branches. Service-specific
  actions such as identification remain deferred. Scriptless NPCs have no conversation panel.

### QUEST-001 — Quest rewards and broader logbook coverage

- **Priority:** P2 | **Status:** Deferred
- The Classic book exposes Quests, Rumors & Notes, and Background. Other sections depend on their gameplay
  systems. The original logbook notification highlight is pending.
- Completion awards original quest XP and alignment. Quest-specific NPC reaction changes and the completion sound are pending. Floating-line
  responses and unbotching are deferred.
- In the randomized waterfront riddler, the wrong-answer responses in
  `dlg/01034riddler1.dlg` combine botching quest 1009 with `fl 33` (an
  authored floating line). The unresolved DLG action disables those
  responses and their quest writes. The correct-answer route reaches Completed.
  Its prize remains blocked at `dlg/01036riddler3.dlg` line 16 by the mixed
  `in -5700, gf2730 1, rp 1071` effect, which still needs reputation support.

### DLG-002 — Generated greetings and talk eligibility

- **Priority:** P2 | **Status:** Deferred
- Plain `G:` greetings and several original generic response directives
  use installed tables. Variant selection currently uses the first matching
  entry. Reputation, complex greeting branches and alternate dialogue
  overrides require the corresponding systems.
- Talk eligibility uses available object, stat and flag data. Temporary
  effects, hostility scripts, and scripted or autonomous NPC behavior need
  their respective runtime systems.

### SH-001 — Shrouded Hills interaction follow-up

- **Priority:** P2 | **Status:** Deferred
- Native clicks now open Doc Roberts's initial and repeat greetings. His
  authored recruitment continuation in `scr/01280docroberts.scr` starts at
  statement 16 and requires `SAT_CRITTER_FOLLOW` (action 21) at statement 17.
  Dialogue does not implement recruitment or the bank fight.
- Native clicks now reach the Wise Woman's and Black Root Herbalist's greetings and barter responses. Their other services, including identification, remain pending.
- **Bridge gate resolved in M74-R.** Paying Lukan transfers his original bridge key. Clicking the leaf opens the gate and runs Use script 30019. The changed collision edge allows passage. [Installed-data reproduction](AGENT_WORKFLOW.md#shrouded-hills-interaction-check).
- First and recurring NPC heartbeats now run on active mobiles. Some script operations in Shrouded Hills still require implementation. Doc's first heartbeat
  `01302docfirst_heartbeat.scr` can change his day standpoint and remove the
  attachment. His `01759docroberts_heartbeat1.scr` and the bridge copy's
  `02770docbridgehb.scr` use reputation 1050 and state toggles.
  Lukan's `02774bridgethugshb.scr` also checks reputation 1050 and can kill
  its attachee. The bridge copy starts with `OF_OFF`. The Wise Woman and
  Black Root Herbalist have no First Heartbeat or Heartbeat attachment.

### Manual gameplay findings — Crash Site, Shrouded Hills and Classic HUD

These observations come from manual playtesting. The subsystem leads below are hypotheses. P1 marks a blocked gameplay action or quest step.

#### Crash Site

- **WORLD-002 — Shrine inscription · P2 · Open.** Click the original shrine at the Crash Site. It should display its authored inscription, but no text appears. The scenery Use handler or the presentation of its text may be involved.

#### Shrouded Hills — NPCs, dialogue and quest items

These cases extend [SH-001](#sh-001--shrouded-hills-interaction-follow-up). Related interpreter and quest coverage is tracked in [DLG-001](#dlg-001--dialogue-requires-unimplemented-script-and-game-state) and [QUEST-001](#quest-001--quest-rewards-and-broader-logbook-coverage).

- **INV-002 — Overlapping bridge ogre corpses · P2 · Open.** Kill two bridge ogres at the same location, then try to select and loot each body. Their corpses overlap and only one can be selected and looted. Each corpse should give access to its own inventory. Picking among overlapping corpses may be involved.
- **SH-002 — Malochean Hand bridge encounter · P2 · Needs reproduction.** Cross the bridge. The tester expected the Malochean Hand encounter with William Radcliffe, but it did not trigger. Establish the original quest and script prerequisites and repeat the crossing with those conditions met. The relevant quest state or encounter trigger may determine whether the encounter should occur.
- **DLG-003 — William Radcliffe does not initiate dialogue · P2 · Needs reproduction.** With William present, approach him or walk past him. He does not start the expected conversation. Check when his original script calls for automatic dialogue. Proximity detection, NPC heartbeat dispatch or an authored condition may be involved.
- **DLG-004 — William Radcliffe’s disabled responses · P2 · Needs reproduction.** With William’s dialogue open, try selecting the greyed-out responses. They cannot be selected. Responses whose authored conditions are met should be selectable. Record the affected lines and character and quest state to distinguish an intended restriction from a response-condition or action the dialogue system cannot evaluate.
- **SH-003 — Ristezze cannot be spoken to · P1 · Open.** Approach Ristezze and click him to talk. No conversation opens, blocking quest progression that requires speaking to him. His authored dialogue should open for an eligible player. Talk eligibility or dispatch of his dialogue attachment may be involved. See [DLG-002](#dlg-002--generated-greetings-and-talk-eligibility).
- **QUEST-002 — Doc Roberts’s quest cannot be accepted · P1 · Open.** Talk to Doc Roberts and choose the quest-acceptance response. Instead of accepting the quest, dialogue reports `unresolved script action 7 at statement 30`. The SCR action reached by this response needs review. SH-001 separately records action 21 at statement 17 on the recruitment continuation.
- **QUEST-003 — Constable Owens’s quest cannot be completed · P1 · Open.** With Owens’s quest active, kill Lukan and return to Owens. The expected turn-in does not complete the quest. Lukan’s death script, the resulting quest state or Owens’s turn-in condition may be involved.
- **INV-003 — Joachim’s note for Virgil cannot be picked up · P1 · Open.** Click Joachim’s note to pick it up. It should enter the backpack so it can be taken to Virgil. Pickup fails, and the console reports that the item is unavailable or requires an unsupported script or equipment transaction. The item’s pickup script or ownership transfer may be involved. See [scripted inventory dependencies](#deferred-gameplay-dependencies).
- **SH-004 — Shrouded Hills herbalist missing · P2 · Open.** Visit the herbalist’s expected location in Shrouded Hills. The NPC is missing from the world, so the expected interaction is unavailable. Original mobile placement, visibility or activation conditions may be involved. SH-001 records the working Black Root Herbalist greeting and barter at that separate location.

#### Shrouded Hills — World interactions

- **WORLD-003 — Open windows block traversal · P2 · Open.** Open a window at Jongle Dunne’s house and try to move through it. The open window blocks passage. The tester verified that the same traversal works in vanilla. Check generic window-portal collision and traversal rules against the open state. [WORLD-001](#world-001--incomplete-window-rendering) tracks window appearance.
- **NPC-002 — Stationary church rats · P2 · Open.** Enter the Shrouded Hills church and observe the rats near the steam engine. They remain stationary instead of exhibiting their original movement. Ambient NPC movement goals, standpoints or script activation may be involved.
- **QUEST-004 — Steam-engine control box blocks Jongle Dunne’s quest · P1 · Open.** During Jongle Dunne’s quest, try to attack and destroy the steam-engine control box. It cannot be attacked or destroyed, so the quest cannot be completed. Check the original attack eligibility and whether a destruction callback advances the objective. See [Destructible World Objects](#destructible-world-objects).

#### Classic HUD

- **UI-011 — Missing level-up notification on the character-sheet button · P2 · Deferred.** Gain a level and leave character points unspent. The Classic character-sheet HUD button should turn red, but its appearance does not change. Check the level-up notification state and its button artwork in a later UI compatibility pass. [UI-006](#ui-006--missing-inventory-change-notification) tracks the separate inventory-button notification.
- **UI-012 — Missing experience progress bar · P2 · Open.** With the Classic HUD visible, gain XP and check progress toward the next level. The original experience progress indicator is absent. Check the original HUD artwork and how it reflects authoritative XP and level state. Any restoration should use the shared theme components and bindings.

### LOCK-001 — Remaining access dependencies

- **Priority:** P2 | **Status:** Deferred
- Key checks read carried keys and authored key-ring entries. Automatic collection into rings, follower key sharing and forced entry remain outside the playable subset.
- Pick Locks and Unlocking Cantrip change the original lock bit. Ordinary use still checks jam, magical hold and original Unlock and Use scripts. A successful Cantrip on a jammed lock leaves it jammed.
- Lock attempts expose the authored guard notification, loudness and outcome. Guard hearing, Catching Thief scripts and responses to lock attempts remain deferred. Fate-point spending, difficulty adjustments for lockpicking, timed equipment enchantments and party-assisted skill use are deferred. The pick delay represents the original work interval. Lockpicking still lacks a dedicated character pose. Cantrip uses its original caster and target EyeCandy. Other spell execution and character dependencies are tracked in MAG-001 and PROG-001.
- Unsupported script operations still refuse access. Bates Mansion's pedestrian gate has key ID 64 and Unlock script 1151. With global flag 1024 set, the script requests attachment removal at statement 1. The engine reports that dependency even with the right key. Its timed gate-control branch also needs the event scheduler. Container Use and Unlock checks support read-only predicates.

### TRADE-001 — Merchant service and equipment dependencies

- **Priority:** P2 | **Status:** Deferred
- Gambling, identification, NPC equipment selection and inventory scripts with unsupported side effects need their corresponding gameplay systems. A purchase predicate that the interpreter cannot evaluate rejects the sale. Shop stock uses original item records and probability tables for replacement.

### TRADE-002 — Barter dialogue response requires a second click

- **P2 · Open.** Speak to a merchant who offers the original **Barter** response, such as the Wise Woman. Click that response once and wait for the trading interface. The player reports that it opens only after a second click.
- **Expected.** A single click on the available **Barter** response opens trading with the speaking merchant.
- **Investigate.** Trace the first click through shared dialogue action dispatch and merchant service activation. Check when the accepted response requests trading and when the trading interface opens.

### MAP-003 — Installed town-map resources rejected

- On the Shrouded Hills merchant route the winning `townmap/shrouded hills/shrouded hills.tmi` from module `Arcanum.PATCH9` reports an invalid version, ID or length. World interaction remains available.

- **Priority:** P2 | **Status:** Needs reproduction
- M69-R native fixture discovery reported an invalid version, ID or length
  for `townmap/void-islands/void-islands.tmi` from `Arcanum.PATCH9`.
  The Void map loaded. Town-map interpretation remains to be investigated.

### AUD-002 — Unknown sound scheme ID 29

- **Priority:** P2 | **Status:** Needs reproduction
- M69-R native fixture discovery reported SchemeIndex ID 29 on Void and
  Tulla Interior – Battle – Morph. Map playback and dialogue testing continued.

### AUD-001 — Missing resources in the authored wasteland scheme

- **Priority:** P2 | **Status:** Open
- The installed `sound/schemelist.mes` entries 4300–4302 reference
  `wastland_day.wav` and `wastland_night.wav`. The VFS resolves the
  `wasteland_day.wav` spelling and reports the authored day path as missing.
  A content override can correct the scheme entry.

### DEV-001 — Atlas presents are absent from the frame counter
- **Priority:** P2 | **Status:** Open
- **Observed:** During M66-R native scale verification, an atlas redraw changed the displayed scale while `rpc pos` kept the previous `renderer.frames_presented` value.
- **Cause:** The existing world redraw increments the counter. The modal atlas submits through a separate redraw entry point that omits the increment.
- **Verification:** Use Vulkan capture to observe atlas redraws until the diagnostic counter covers both presentation paths.

### INPUT-001 — Synthetic X11 wheel tick advances world zoom twice
- **Priority:** P2 | **Status:** Needs reproduction
- **Observed:** One XTest Button4 press and release over the world advanced zoom from 1× to 3× in both the M66-R build and the pre-milestone native baseline.
- **Investigation:** Compare physical wheel input with the XTest sequence and inspect the wheel events delivered to winit.

### UI-001 — World-map hover description outside the HUD
- **Priority:** P2 | **Status:** Open
- **Observed:** M63-R manual playtest.
- **Reproduce:** Open the world map and hover over a discovered location, e.g. Vendigroth Ruins.
- **Actual:** The location description appears beneath the map panel; the bottom HUD remains empty.
- **Expected:** Contextual location information appears in the bottom HUD, following CE behavior.
- **Investigation:** Shared contextual-description rendering, panel layout and HUD state. Root cause unconfirmed.
- **Target:** M64-R if the shared hover mechanism is already being modified; otherwise UI compatibility sweep.

### WORLD-001 — Incomplete window rendering
- **Priority:** P2 | **Status:** Needs reproduction
- **Observed:** Earlier world-compatibility investigation identified unsupported window portal appearance handling.
- **Reproduce:** Inspect original buildings with windows in Tarant and compare against CE.
- **Expected:** Original window ART and applicable wall-piece rendering rules are interpreted generically.
- **Investigation:** Portal appearance resolution and wall-piece rules. Confirm current coverage using world-scan; do not introduce location-specific fixes.
- **Target:** World-rendering compatibility sweep.

### UI-004 — World map HUD toggle
- Priority: P2
- Clicking the world map HUD button while the map is open does not close it.
- Escape closes the map correctly.
- Investigate modal input routing and shared toggle behavior.

### UI-005 — Uneven inventory grid lines
- Priority: P2
- Some inventory grid lines appear thicker than others.
- Investigate fractional scaling, pixel alignment and texture sampling.
- Preserve original grid artwork and item hitbox alignment.

### UI-006 — Missing inventory change notification
- Priority: P2
- Picking up an item while the inventory is closed does not activate the original red inventory-button outline.
- Investigate CE's notification state and acknowledgement behavior.
- Connect the notification to authoritative inventory changes through the shared HUD system.

### CHAR-001 — Equipment scripts and conditional background effects
- **Priority:** P2 | **Status:** Deferred
- Static race, gender and background modifiers and numerical equipment bonuses are active.
- Items that require wield scripts report the dependency. Recovery items dispatch Item_Used spells. Other item triggers and specialized item lighting need their gameplay systems.
- Conditional background modifiers depend on time, the standing tile, lighting and later skill systems. The Classic Attack readout depends on combat skill and damage calculations.

### INV-001 — Stack consumption
- **Status:** Partially implemented
- Original recovery foods consume the selected inventory instance.
- Gold and ammunition can be split and merged through quantity-aware inventory transactions.
- Using charges, fuel and other ammunition depends on their associated gameplay mechanics.

### UI-008 — Quick-access hotbar
- Status: Planned
- Implement original hotbar assignment, removal and item activation behavior once the necessary gameplay systems exist.

### WORLD — Missing merchant signs
- Priority: P2
- Original merchant signs attached to building exteriors are missing or lack their original hover interaction.
- Investigate object classification, ART resolution and contextual descriptions using CE and installed resources.

### UI — Local map toggle
- Priority: P2
- Clicking the local map HUD button opens the map but does not close it.
- Escape works.
- Investigate alongside the existing world-map toggle issue through shared modal input routing.

### EQUIP — Gender-specific equipment restrictions
- Priority: P2
- The default male character can equip Ladies Boots.
- Verify original gender, race and body-type equipment restrictions against CE.
- Enforce applicable restrictions through the shared equipment validation system.

### UI — Pause menu background
- Priority: P2
- The game world remains visible around the pause menu.
- Render a black background outside the menu artwork.
- Preserve existing menu dimensions and scaling.

### SAVE — New save name entry
- Priority: P2
- Clicking New requires an additional click on the green confirmation arrow before entering a save name.

### HUD — Day/night indicator artwork

- **Resolved in M82-R.** The GameClock binding drew the strips onto a one-pixel surface. It now uses the theme aperture, the original day and eight moon strips, and the clock pointer. Native captures cover afternoon, evening and morning, including after a save, restart and map change.

### Recovery and rest presentation

- **P2, deferred.** Rest uses the original selector and hourly healing. The bed pose, sleep fade and follower sleep notifications need animation and party integration. Overland encounter transitions from the world-map travel screen remain part of travel simulation.
- Recovery items dispatch their installed Use SCR or supported Item_Used program. Unsupported scripted operations report their dependency and leave the item in the backpack. Remaining non-recovery device programs need their respective gameplay systems. Recruit inventory uses the shared transfer and equipment transactions.

### AUDIO — Incorrect inventory opening sound
- Priority: P2
- Opening inventory plays a different sound from vanilla.
- Verify the original sound ID and activation event against CE.
- Preserve the existing audio event and mixer architecture.

## Historical issues requiring revalidation

The M61 rollback removed the first M62–M64 implementation. The following observations must not be assumed to affect the reworked engine until reproduced.

| ID | Priority | Observation / reproduction | Expected behavior |
| --- | --- | --- | --- |
| MAP-001 | P2 | Open Tarant's local map. An earlier build reported `townmap/tarant/tarant.tmi: invalid atlas tile grid`. | Decode and display the original local atlas through normal VFS precedence. |
| MOVE-001 | P1 if ordinary movement is blocked | Click unobstructed ground at Crash Site; earlier logs repeatedly reported `cannot reach the destination`. It is unclear whether the clicked tiles were actually reachable. | Navigate to reachable destinations and reject genuinely blocked ones. |
| NPC-001 | P2 | Recheck Tulla Interior and previously unresolved scaled NPC/creature appearances against CE. | Resolve original appearance data through the generic mobile renderer. |
| UI-002 | P2 | Inspect the top HUD time/sky display. M63-R screenshots show an empty pale field. Determine whether this is an inactive component or a rendering discrepancy. | Display the applicable original ART/state once its dependencies are available. |

## Deferred gameplay dependencies

These are planned capabilities, not regressions. Implement in their own milestones rather than attempting to fix them through UI workarounds.

- **M64-R:** interactive inventory rearrangement, transfers, Take All, Arrange, ground pickup/drop and drag cancellation; developer unlocking of ordinary locked containers (verify existing command behavior first).
- **Inventory extensions:** generated loot, consumption, decay and scripted inventory behavior as their dependencies become available.
- **Later systems:** combat, broader script and spell execution, and uses of crafted devices.
- **Use action:** the inventory Use box accepts original lockpicks. Other targeted items need their own gameplay mechanics.

### Destructible World Objects

Original Arcanum allows physical destruction of eligible containers, including some locked chests, as an alternative way to access their contents. Scenery and container destruction are candidates for a dedicated milestone before the first full playthrough. QUEST-004 records the blocked quest. [LOCK-001](#lock-001--remaining-access-dependencies) tracks forced entry, and [MAG-001](#mag-001--spell-interactions-with-combat-party-and-environmental-systems) records the existing portable-item damage support.

Verify attack eligibility and durability against CE and installed object definitions, including examples of destructible and indestructible containers. Trace physical damage and resistance through the transition to a destroyed object. Check who owns the loot, how it becomes accessible, what collision changes, which script callbacks run, and whether partial damage, destruction and loot ownership persist across save/load and map re-entry.

## Regression watchlist — previously corrected

- **CAM-001 and MOVE-002. Verified in M81-R.** WASD and arrows pan independently of the player. Scrolling works across the passive bottom HUD. The character name, save name and developer console accept WASD without moving the view. CE’s `scroll_speed_changed` uses 20 horizontal or 10 vertical pixels per normal isometric update. The playable engine uses a configurable screen-pixel rate in every direction. [Camera configuration](APPLICATION_CONFIG.md#settings-and-precedence)

- **M79 opening events and camera. Resolved.** A fresh campaign opens Virgil’s authored dialogue from his heartbeat. Scripted passengers fall and the original flat corpse poses expose Isaac’s camera and Wilhemina’s note. The independent camera stays put while walking and across save/load, and centers on map arrival. [Native reproduction](AGENT_WORKFLOW.md#crash-site-opening).
- **M79 menu music and creation sounds. Resolved.** The shopping map’s entry sector selects scheme 26 and the installed Arcanum theme. Native output captured the theme on launch, through creation and shopping, and again after returning from the Crash Site. Artwork buttons use the original press and release samples. At the merchant, picking up, buying and selling a dagger produced the item samples. Cancelling the drag and an unaffordable purchase produced no drop cue. A drag-purchased dagger was equipped after native save and load.
- **Classic main and pause lettering. Resolved.** Both menus use the original dark red face and bright selection. The relief uses the ART glyph coverage. Labels use their measured frame extents and configured top and center independently of the hitbox. Vulkan captures covered normal, hovered, keyboard-selected and modal-disabled entries. A narrowed external button at 120% effective scale retained the full label and its configured click area.

- **UI-009. Resolved.** Quit Game returns to the original main menu after confirmation. Native checks covered loading the previous character, creating a second character and exiting from the main menu. Starting a new campaign copies the new character and owned items from the merchant session in transaction order. Merchant state, old character effects, quests and rumors stay with their original session.
- **Combat speed (M80-R). Resolved.** Classic Options loads the original three mode labels. Fast turns skip the acting character’s movement frames. Paired native Kree encounters retained the turn order, AP, rolls, damage and summoned ally. [Combat verification](AGENT_WORKFLOW.md#combat)
- **COMBAT-002. Resolved.** NPC and summon steps previously changed tile without a walking pose. Kree bandits, the Crash Site wolf and an Orcish Champion now use their directional walking and falling ART. A saved wolf fall resumed after a full restart and stayed on its last frame after a second F8 during the player's turn. Spell targeting remains active across casts until cancelled. Native right-button release restored the ordinary pointer. Quench Life completed its callback once during turn-based combat.

These are not open defects. Retain focused regression coverage and reopen only if reproduced.

- HUD and active panels overlap at large UI scales, or opening a panel changes HUD scale. M63-R reports a stable viewport-fitted layout based on 800×600.
- Inventory button cannot close an open panel; keyboard and mouse toggle behavior diverge.
- UI-010. Classic HUD and dialogue text discarded fine strokes by reducing
  15-pixel Flare12 frames to 12 pixels with nearest sampling. M70-R restores
  native ART metrics and preserves coverage for explicit reductions. Vulkan
  checks cover both requested desktop scales and the 800×600 reference.
- DLG-002 layout. Long Classic dialogue responses used fixed, clipped rows.
  M70-R wraps and scrolls rows with shared input bounds. Native checks use a
  long VFS override at 800×600 and select a response after scrolling.
- UI-007. Classic item hover text and counters display original stack quantities. Native checks use the authored Crash Site Cave ammunition.
- UI scale changes trigger expensive asset reconstruction, texture uploads or noticeable stalls.
- Scenery-ART containers fail to render or open. M63-R reports generic classification and reachability corrections; verify ordinary locked containers still reject access.
- Inventory transfers lose object identity, duplicate items, or mutate state on rejected placements. M62-R reports atomic whole-object transactions across map changes and sector eviction.

### MAG-002 — Wrong wall, summon and elemental ART

- **Priority** P1. **Status** Resolved and checked against vanilla captures and native Vulkan gameplay.
- Spell `Proto` operands use the basic prototype numbering in CE `magictech.c` and `proto.c`. The serialized ID is the basic ID plus 20. Wall of Stone, Wall of Fire, Wall of Force and Squall of Ice use serialized prototypes 4052, 4051, 4050 and 4054 respectively. Squall uses blank scenery with animated EyeCandy. The former raw-ID lookup loaded unrelated world objects. The winning spell and EyeCandy definitions were in Arcanum5.dat.
- A second error discarded `Apply_AoE: Tgt_Summoned` EyeCandy. The generated owner now supplies the world anchor, and the installed EyeCandy record supplies animation, layer, sound and lifetime. Stock wall casts form five tiles in vanilla. Stone and Force blocked a mouse-issued route into their rendered footprint until dismissal. Fire and Squall admitted the player and applied damage on the rendered tiles.
- TIG determines the stored rotation count from bit zero of the ART header. The earlier whitelist rejected original single-rotation summon arrival frames with flags 0x11, preventing the callback that reveals the model. Palette and frame bounds remain validated.
- Actor forms and summoned creatures use the normal directional ART and palette cache. The summoned Trait reset reads the prototype’s original AID, which distinguishes the Phantasmal Fiend and Hellgate demon even though they share a monster ART file. Guardian Ogre also needs its worn armor from prototype inventory source 93. The shared inventory generator and equipment appearance rules supply the armored model. The vanilla mage already had a Familiar when comparisons began. Its visible model supplied the reference for the newly cast RS Familiar.
- The installed Earth college names its fourth spell **Body of Stone**. Each elemental form walked using its directional ART, survived F7/F8 and restored the prior player appearance on dismissal. Native audio output confirmed the original Guardian Ogre caster, arrival and reverse cues 23030, 23035 and 23038. Frame-exact timing and sound parity across every pose remain unmeasured.

## Grand Compatibility Sweep

Once the principal gameplay systems are interconnected, revisit P2 issues in focused subsystem passes. Use CE source, installed data, existing reference screenshots, RPC and world-scan evidence. Group defects by root cause rather than location. Prioritize missing dependencies before downstream symptoms.

Use [PLAYTEST_CHECKLIST.md](PLAYTEST_CHECKLIST.md) for regression checks. Extend automated coverage where practical, but verify mouse/keyboard behavior and visual presentation through the actual playable executable. Update this register when an issue is reproduced, deferred, corrected or invalidated by a later implementation.
