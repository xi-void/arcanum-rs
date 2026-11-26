# Quests and the Classic logbook

Click the logbook icon or press **L** to open Quests. Turn spreads with the
book arrows, **Page Up** and **Page Down**, or the mouse wheel over either page.
Use **[** and **]** to scale the logbook with the top and bottom HUD.
**Escape**, **L**, or the logbook icon closes it.

## Original state and resources

The local player character starts with every quest unknown. The module's
separate global quest state starts at Accepted, signifying that the quest
remains available. The logbook lists known quests by game time of the latest
state change. It uses the classic status label Accepted for both Accepted and
Achieved. The developer readout reports the exact state.

The ordinal states are Unknown (0), Mentioned (1), Accepted (2), Achieved (3),
Completed (4), Other Completed (5), and Botched (6). Progress can skip states.
Equal and backward writes leave the quest unchanged. Completed,
Other Completed, and Botched are terminal for normal writes. A botch retains
the player's prior state internally. Botching a quest before the player learns
about it leaves it out of the journal. Global completion or botching affects
subsequent player writes according to CE's `quest_state_set_internal`.

`dlg/*.dlg` uses `qu ID STATE` for an exact condition or a state write.
`qa` and `qb` are inclusive lower and upper bounds. SCR condition opcodes
6 and 7 read the object/PC and global state, respectively. SCR action opcodes
16 and 17 write local-PC and global quest state. Party and object selectors
that require additional runtime systems are deferred. Flags and variables
even with the same numeric ID are separate script records. Failed dialogue
continuations roll back their quest writes with their other staged writes.

The VFS resolves `mes/gamequestlog.mes`, `mes/gamequestlogdumb.mes`,
`mes/logbk_ui.mes`, `rules/gamequest.mes`, and `rules/gameinit.mes`. The
low-intelligence text applies at Intelligence 4 or below and falls back to
the normal description. The journal uses the authored starting year and
CE's twelve 30-day months. Native saves carry both quest stores and each
player record's timestamp and source. Older saves initialize absent stores
to the new-game defaults.

CE references are `game/quest.c`, `game/dialog.c`, `game/script.c`,
`game/gameinit.c`, `game/timeevent.c`, and `ui/logbook_ui.c`. Classic
loads the original logbook ART, fonts, and tab and page geometry through
the [theme system](UI_THEMES.md).

## Installed-data fixture

Use `rpc coc 5000 70111 92211` near the waterfront **Halfling Wizard** at
(70113, 92211). The object is `2:dafc7a5f7a75ae4383df6330af956658`,
prototype 17202. Both resource variants use that object and its Dialogue
attachment 1034. Left-click the NPC and choose **Sure.** Speaker line 22
writes Mentioned for quest 1009. The rendered **Ask away.** response is
line 23 in both variants and has an empty effect field.

| Winning resources | Result of clicking **Ask away.** |
| --- | --- |
| Fixed original riddles, DLG from `Arcanum5.dat` and SCR from `Arcanum2.dat` | The positive link opens speaker line 26 directly. Answer **A clock** for Accepted. |
| Optional `ArcanumXRiddlers.dat` from the test installation, winning both DLG and SCR | The negative link `-18` enters SCR statement 18 (zero-based). `SAT_RANDOM` (action opcode 37) rolls an inclusive 1–5 into script temporary 0. The following SCR branches open one of dialogue lines 26, 47, 53, 59 or 65. A correct answer advances the quest to Accepted. |

Capture `rpc pos` just before and after a real click on **Ask away.**.
The randomized variant reports the resulting SCR outcome in
`dialogue.active.continuation` and the advanced stream in `scripts.random`.
A rejected response reports `dialogue.last_result` and discards its staged
writes and draws. Save after closing the conversation and before asking the
riddle to replay the same question from that save.

M72-R's original progression test used the fixed variant. Native verification
with the installed randomized archive used the same waterfront NPC, clicked
response 23, and replayed the same riddle after quitting and loading a save from
before the draw. Correct answers advanced the journal through Mentioned,
Accepted, Achieved, and Completed. Use `rpc coc 5000 70201 91992` near the
second riddler and `rpc coc 5000 70170 91953` near the third. With the randomized
archive, the answer depends on the question. Close dialogue before saving
with **F7**. After restarting, press **F8** and open Quests again.

`rpc pos` reports `quests.entries` with the exact state, resolved text,
original rules, and last DLG line or SCR statement (zero-based) and object that
changed it. `scripts.quests` includes the complete player and global stores.
`ui.hud.journal` reports the current spread and physical page and control
bounds. `world-scan` reports static quest message coverage and VFS provenance
under `research.quest_resources`.

The remaining logbook tabs, generated quest responses, unbotch operations,
quest completion side effects (experience, alignment, reaction, sound), and
additional authored reward actions are deferred. In the randomized variant,
the first riddler's wrong answers in `dlg/01034riddler1.dlg` combine a Botched
write with `fl 33`, an unsupported floating line. Those choices stay disabled.
The third riddler's correct answer commits Completed, then opens speaker line
16 in `dlg/01036riddler3.dlg`. Its mixed reward effect
`in -5700, gf2730 1, rp 1071` needs inventory transfer and reputation support.
Reopening a resolved riddler's conversation may also require floating dialogue.
