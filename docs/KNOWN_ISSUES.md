# Arcanum-RS — Known Issues

Baseline: Milestone 63-R (Classic inventory presentation). Update this register as milestones are completed. This is a defect backlog, not a list of every unimplemented feature.

## Triage

- **P0 — Critical:** crashes, data loss, duplication, corrupted state or unsafe persistence. Fix immediately.
- **P1 — Blocking:** defects that prevent ordinary gameplay or invalidate the current milestone. Fix before merging the affected milestone.
- **P2 — Compatibility:** nonblocking visual, behavioral or content discrepancies. Defer to the relevant subsystem or the Grand Compatibility Sweep unless a shared correction is already in scope.

Status: **Open**, **Needs reproduction**, **Deferred**, or **Resolved**. Keep reproduction steps, evidence and dependencies current. Do not mark an issue resolved without verifying the affected production path.

## Open issues

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

### UI-007 — Missing stack quantities in item descriptions
- Priority: P2
- Ammunition picked up in the Crash Site Cave displays only "Bullet" when hovered.
- Inspect serialized item quantities and CE's contextual description formatting.
- Display original stack quantities for bullets, arrows, charges, fuel and other applicable items.
- Do not infer quantity from the number of inventory icons.
- Stack splitting, merging and consumption remain separate gameplay requirements.

### INV-001 — Stack manipulation
- Status: Planned
- Implement original stack merging, splitting, quantity-aware transfers and consumption through authoritative inventory transactions.
- Preserve total quantities and prevent duplication or item loss.

### UI-008 — Quick-access hotbar
- Status: Planned
- Implement original hotbar assignment, removal and item activation behavior once the necessary gameplay systems exist.

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
- **Character/equipment:** starting loadout, equipment slots, character statistics, encumbrance, appearance changes and applicable item effects. Do not assume the archived M64 implementation is present.
- **Inventory extensions:** stack merging/splitting, generated loot, consumption, decay and scripted inventory behavior as their dependencies become available.
- **Later systems:** persistent application settings, disk save/load, audio, dialogue, scripting, combat, magick, technology, crafting, trading and progression.
- **Use action:** remains inactive until the relevant gameplay mechanics exist.

## Regression watchlist — previously corrected

These are not open defects. Retain focused regression coverage and reopen only if reproduced.

- HUD and active panels overlap at large UI scales, or opening a panel changes HUD scale. M63-R reports a stable viewport-fitted layout based on 800×600.
- Inventory button cannot close an open panel; keyboard and mouse toggle behavior diverge.
- UI scale changes trigger expensive asset reconstruction, texture uploads or noticeable stalls.
- Scenery-ART containers fail to render or open. M63-R reports generic classification and reachability corrections; verify ordinary locked containers still reject access.
- Inventory transfers lose object identity, duplicate items, or mutate state on rejected placements. M62-R reports atomic whole-object transactions across map changes and sector eviction.

## Grand Compatibility Sweep

Once the principal gameplay systems are interconnected, revisit P2 issues in focused subsystem passes. Use CE source, installed data, existing reference screenshots, RPC and world-scan evidence. Group defects by root cause rather than location. Prioritize missing dependencies before downstream symptoms.

Use [PLAYTEST_CHECKLIST.md](PLAYTEST_CHECKLIST.md) for regression checks. Extend automated coverage where practical, but verify mouse/keyboard behavior and visual presentation through the actual playable executable. Update this register when an issue is reproduced, deferred, corrected or invalidated by a later implementation.
