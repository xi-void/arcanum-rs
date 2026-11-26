# Arcanum-RS — Known Issues

Baseline: Milestone 70-R (Classic UI typography). Update this register as milestones are completed. This is a defect backlog, not a list of every unimplemented feature.

## Triage

- **P0 — Critical:** crashes, data loss, duplication, corrupted state or unsafe persistence. Fix immediately.
- **P1 — Blocking:** defects that prevent ordinary gameplay or invalidate the current milestone. Fix before merging the affected milestone.
- **P2 — Compatibility:** nonblocking visual, behavioral or content discrepancies. Defer to the relevant subsystem or the Grand Compatibility Sweep unless a shared correction is already in scope.

Status: **Open**, **Needs reproduction**, **Deferred**, or **Resolved**. Keep reproduction steps, evidence and dependencies current. Do not mark an issue resolved without verifying the affected production path.

## Open issues

### DLG-001 — Dialogue requires unimplemented script and game state

- **Priority:** P2 | **Status:** Deferred
- The initial SCR instruction subset and DLG script-state effects are
  implemented. Unsupported instructions, mixed effects, implicit exit
  handlers, and alternate dialogue overrides still need their original
  gameplay dependencies. The Bates house guards use equipment and combat
  checks and floating lines.
- Quest, reputation, skill, encounter and party checks remain unresolved.
  Initial reaction uses the original base and available character modifiers.
  Persistent reaction and meeting history and live AI changes are pending.
- Some greetings and response directives need class-, race-, quest- or
  service-specific dialogue. Talk-triggered scripts, trade and companion
  recruitment are deferred. Scriptless NPCs have no conversation panel.

### DLG-002 — Generated greetings and talk eligibility

- **Priority:** P2 | **Status:** Deferred
- Plain `G:` greetings and several original generic response directives
  use installed tables. Variant selection currently uses the first matching
  entry. Reputation, complex greeting branches and alternate dialogue
  overrides require the corresponding systems.
- Talk eligibility uses available object, stat and flag data. Temporary
  effects, hostility scripts, and scripted or autonomous NPC behavior need
  their respective runtime systems.

### MAP-003 — Void Islands town-map resource rejected

- **Priority:** P2 | **Status:** Needs reproduction
- M69-R native fixture discovery reported an invalid version, ID or length
  for `townmap/void-islands/void-islands.tmi` from `Arcanum.PATCH9`.
  The Void map loaded. Town-map interpretation remains to be investigated.

### AUD-002 — Unknown sound scheme ID 29

- **Priority:** P2 | **Status:** Needs reproduction
- M69-R native fixture discovery reported SchemeIndex ID 29 on Void and
  Tulla Interior – Battle – Morph. Map playback and dialogue testing continued.

### UI-009 — Quit Game exits the application

The Classic Quit Game button asks for confirmation and then exits. Return to
the original main menu when the main menu and new-game lifecycle exist.

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

### UI — Pause menu text color
- Priority: P2
- Menu lettering appears gold instead of the original red.
- Investigate original text colors and button states using CE and installed resources.

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
- **Later systems:** dialogue, scripting, combat, magick, technology, crafting, trading and progression.
- **Use action:** remains inactive until the relevant gameplay mechanics exist.

## Regression watchlist — previously corrected

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

## Grand Compatibility Sweep

Once the principal gameplay systems are interconnected, revisit P2 issues in focused subsystem passes. Use CE source, installed data, existing reference screenshots, RPC and world-scan evidence. Group defects by root cause rather than location. Prioritize missing dependencies before downstream symptoms.

Use [PLAYTEST_CHECKLIST.md](PLAYTEST_CHECKLIST.md) for regression checks. Extend automated coverage where practical, but verify mouse/keyboard behavior and visual presentation through the actual playable executable. Update this register when an issue is reproduced, deferred, corrected or invalidated by a later implementation.
