# Arcanum-RS — Known Issues

Baseline includes M79-R character creation and cinematic playback. Update this register as milestones are completed. This is a defect backlog, not a list of every unimplemented feature.

## Triage

- **P0 — Critical:** crashes, data loss, duplication, corrupted state or unsafe persistence. Fix immediately.
- **P1 — Blocking:** defects that prevent ordinary gameplay or invalidate the current milestone. Fix before merging the affected milestone.
- **P2 — Compatibility:** nonblocking visual, behavioral or content discrepancies. Defer to the relevant subsystem or the Grand Compatibility Sweep unless a shared correction is already in scope.

Status: **Open**, **Needs reproduction**, **Deferred**, or **Resolved**. Keep reproduction steps, evidence and dependencies current. Do not mark an issue resolved without verifying the affected production path.

## Open issues

- **M79 character sheet (P2, open).** The Classic sheet omits the player’s name.
- **Moving shadows (P2, open).** NPC shadows can lose their anchor while the actor moves.
- **Combat speed (P2, open).** Fast turn-based combat is missing. Configurable floating damage numbers are a separate optional future enhancement.

### COMBAT-001 — Remaining combat compatibility

- **Priority P2. Status open.** The stock CE skill rolls, armor and resistance, AP costs, NPC-first turn list, critical damage bonuses and base XP pool rules underpin both combat modes. The Born Under a Sign background supplies both critical severity modifiers. Its hit modifier feeds the original bonus-damage rolls. Its fumble modifier is retained for the remaining weapon and ammunition damage rules. Detailed critical locations, equipment damage and dropped weapons, special critical-chart injuries, ambient light penalties and tactical NPC spell and weapon selection still need coverage. Real-time attack intervals use weapon cost and actor speed. NPCs and summons play the directional walking, attack and death sequences available for their current models. The resolver supports ordinary and torso deaths with the original fallback. Additional injury-specific death variants depend on critical-chart support.
- Followers need recruitment, commands, party travel and party XP distribution. An allied summon can fight using its current form and ownership. Encounters use original hostility, sight, faction and supported script attachments. Some original dying and encounter scripts use operations beyond the script interpreter, reported by `rpc combat`.
- **Installed regression baseline.** The M79-R installed-data run reproduced the pre-M78 failures in the Bates lighting composition, the dining-room mobile shadow probe and the Kna Tha rabbit position. The other 74 installed checks passed, including the external VFS smoke check with its mod fixture. The dropped-item eviction assertion follows the item's original identity through the rendered pickup target.

### PROG-001 — Remaining character and ability execution

- **Priority** P2. **Status** Deferred.
- The Classic spellbook executes the installed programs for the 16 colleges through the casting and progression systems. Remaining spell dependencies are tracked in MAG-001.
- The learned schematic book supports the 56 degree recipes through the installed recipe and inventory definitions. Found schematics, followers supplying components, powered-device use and consumable effects need further runtime support. Conditional equipment scripts outside the supported reversible effect operations report the winning script when blocked.
- Manual point allocation, original skill training tiers, dialogue, quest and combat XP, and kill alignment operate for the player. Automatic leveling schemes, pickpocket alignment, follower attitude AI and college mastery quests remain deferred. Some background and item effect operators remain outside the statistic subset and appear in character diagnostics.

### MAG-001 — Spell interactions with combat, party and environmental systems

- **Priority** P1. **Status** Open for full M77-R acceptance.
- The interpreter reads all 80 original programs. Living targets, resistance, health and fatigue, movement, temporary effects, summons, forms and dispel use session-owned state. Combat consumes fear, hostility, paralysis and control flags. Controlled actors and summons select nearby hostile participants. Unarmed strikes use the current form and applicable equipment bonuses. Summons retain their identities and original placement on return to a map. Summoned followers still need party travel to accompany the caster between maps.
- Damage to carried and loose items has a persistent structural-damage field. Disintegrate can damage or destroy a portable item. Removing an immovable object, updating its collision and running its destruction script require the breakable-world-object lifecycle. The player renderer reads sinkable terrain and the water-walking flag set by Body of Air and Body of Water. Native shore transitions, NPC wading and water shadows remain unverified. Detect Traps needs trap instances and their discovery state. Player transformations store or drop worn gear according to available backpack space and re-equip previously worn items still carried. For authored NPCs, the combat resolver reads equipped armor and resistance. Full NPC loadout restoration after a transformation and tactical weapon selection remain open.
- Spell-specific results from the native input checks, including maintained state and renderer provenance, are available in the local M77-R visual captures. Representative casts covered every college. The original gate still resists Unlocking Cantrip at aptitude 10 and opens at 20 for ten fatigue each. A controlled summon and the player each receive one Regenerate binding, with its caster EyeCandy on the player.

### DLG-001 — Dialogue requires unimplemented script and game state

- **Priority:** P2 | **Status:** Deferred
- The initial SCR instruction subset and DLG script-state effects are
  implemented. Unsupported instructions, mixed effects, implicit exit
  handlers, and alternate dialogue overrides still need their original
  gameplay dependencies. The Bates house guards use equipment and combat
  checks and floating lines.
- Direct quest state conditions are implemented. Generated quest responses,
  reputation, encounter and party checks remain unresolved. Skill, training and alignment conditions read the acting character.
  Initial reaction uses the original base and available character modifiers.
  Authored reaction changes persist in the NPC account. Persistent meeting history and live AI changes are pending.
- Some greetings and response directives need class-, race-, quest- or
  service-specific dialogue. Companion recruitment and service-specific
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

### LOCK-001 — Remaining access dependencies

- **Priority:** P2 | **Status:** Deferred
- Key checks read carried keys and authored key-ring entries. Automatic collection into rings, follower key sharing and forced entry remain outside the playable subset.
- Pick Locks and Unlocking Cantrip change the original lock bit. Ordinary use still checks jam, magical hold and original Unlock and Use scripts. A successful Cantrip on a jammed lock leaves it jammed.
- Lock attempts expose the authored guard notification, loudness and outcome. Guard hearing, Catching Thief scripts and responses to lock attempts remain deferred. Fate-point spending, difficulty adjustments for lockpicking, timed equipment enchantments and party-assisted skill use are deferred. The pick delay represents the original work interval. Lockpicking still lacks a dedicated character pose. Cantrip uses its original caster and target EyeCandy. Other spell execution and character dependencies are tracked in MAG-001 and PROG-001.
- Unsupported script operations still refuse access. Bates Mansion's pedestrian gate has key ID 64 and Unlock script 1151. With global flag 1024 set, the script requests attachment removal at statement 1. The engine reports that dependency even with the right key. Its timed gate-control branch also needs the event scheduler. Container Use and Unlock checks support read-only predicates.

### TRADE-001 — Merchant service and equipment dependencies

- **Priority:** P2 | **Status:** Deferred
- Gambling, identification, NPC equipment selection and inventory scripts with unsupported side effects need their corresponding gameplay systems. A purchase predicate that the interpreter cannot evaluate rejects the sale. Shop stock uses original item records and probability tables for replacement.

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
- Items that require wield scripts report the dependency. Triggered item spells and specialized item lighting need their gameplay systems.
- Conditional background modifiers depend on time, the standing tile, lighting and later skill systems. The Classic Attack readout depends on combat skill and damage calculations.

### INV-001 — Stack consumption
- **Status:** Deferred
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

### HUD — Missing day/night indicator artwork
- Priority: P2
- The day/night indicator in the top HUD displays a blank pale rectangle instead of the original animated artwork.
- Predates M68-R.
- Investigate ART selection, frame resolution and game-time binding.

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
| MOVE-002 | P2 | If WASD character movement still exists, a single press previously triggered multiple steps. Confirm whether the optional binding has been removed. | Mouse navigation remains authoritative; no unintended movement from obsolete bindings. |
| CAM-001 | P2 | Check edge scrolling over passive HUD areas and compare pan speed with CE. Earlier builds had slow scrolling and HUD edge suppression. | Responsive scrolling, with suppression only during interactions that require it. |
| NPC-001 | P2 | Recheck Tulla Interior and previously unresolved scaled NPC/creature appearances against CE. | Resolve original appearance data through the generic mobile renderer. |
| UI-002 | P2 | Inspect the top HUD time/sky display. M63-R screenshots show an empty pale field. Determine whether this is an inactive component or a rendering discrepancy. | Display the applicable original ART/state once its dependencies are available. |

## Deferred gameplay dependencies

These are planned capabilities, not regressions. Implement in their own milestones rather than attempting to fix them through UI workarounds.

- **M64-R:** interactive inventory rearrangement, transfers, Take All, Arrange, ground pickup/drop and drag cancellation; developer unlocking of ordinary locked containers (verify existing command behavior first).
- **Inventory extensions:** generated loot, consumption, decay and scripted inventory behavior as their dependencies become available.
- **Later systems:** combat, broader script and spell execution, and uses of crafted devices.
- **Use action:** the inventory Use box accepts original lockpicks. Other targeted items need their own gameplay mechanics.

## Regression watchlist — previously corrected

- **M79 opening events and camera. Resolved.** A fresh campaign opens Virgil’s authored dialogue from his heartbeat. Scripted passengers fall and the original flat corpse poses expose Isaac’s camera and Wilhemina’s note. The independent camera stays put while walking and across save/load, and centers on map arrival. [Native reproduction](AGENT_WORKFLOW.md#crash-site-opening).
- **M79 menu music and creation sounds. Resolved.** The shopping map’s entry sector selects scheme 26 and the installed Arcanum theme. Native output captured the theme on launch, through creation and shopping, and again after returning from the Crash Site. Artwork buttons use the original press and release samples. At the merchant, picking up, buying and selling a dagger produced the item samples. Cancelling the drag and an unaffordable purchase produced no drop cue. A drag-purchased dagger was equipped after native save and load.
- **Classic main and pause lettering. Resolved.** Both menus use the original dark red face and bright selection. The relief uses the ART glyph coverage. Labels use their measured frame extents and configured top and center independently of the hitbox. Vulkan captures covered normal, hovered, keyboard-selected and modal-disabled entries. A narrowed external button at 120% effective scale retained the full label and its configured click area.

- **UI-009. Resolved.** Quit Game returns to the original main menu after confirmation. Native checks covered loading the previous character, creating a second character and exiting from the main menu. Starting a new campaign copies the new character and owned items from the merchant session in transaction order. Merchant state, old character effects, quests and rumors stay with their original session.
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
