# NPC dialogue

With Classic active, hover an NPC to read their description in the bottom HUD.
Left-click to approach and talk. Click a response to continue. **Escape**
closes the conversation. **[** and **]** change the shared interface scale
while dialogue is open. Scroll over the speech or responses to read
overflowing text. **Page Up** and **Page Down** advance by a page.
The Voice slider under **Escape → Options → Sound**
controls dialogue volume.

The world picker selects NPCs by their visible original ART. The same route
planner used for other object interactions handles approach. Talk eligibility
and distance are checked at arrival. Both actors face each other when an
authored conversation opens. Classic places the NPC’s speech above their
sprite and the responses immediately above the bottom HUD, following CE
`tb.c`, `tc.c` and `dialog_ui.c`.

## Original resources

The effective object script array supplies the Dialogue attachment
(`SAP_DIALOG`), with the original prototype as fallback. The engine resolves
its numbered `scr/*.scr` resource in the VFS and considers the matching
`dlg/*.dlg` stem. It follows literal dialogue entries, literal jumps, and a
small read-only condition subset from statement zero. An unknown condition
or action before entry rejects the conversation with its statement index.
There is no fallback entry guess.

A DLG record contains seven brace-delimited fields in CE order.

| Field | Contents |
| --- | --- |
| 1 | Stable numeric line ID |
| 2 | Speaker text for a male player, or player response text |
| 3 | Speaker text for a female player, or optional response gender (0 female, 1 male) |
| 4 | Blank for a speaker line. Positive response minimum Intelligence or negative maximum |
| 5 | Original speaker recording number, or response conditions |
| 6 | Response destination. Positive is an NPC line, zero closes, negative references a script continuation |
| 7 | Original effect string |

The parser accepts UTF-8 and Windows-1252, with CE-style `//` comments outside
fields. Responses are the following records in numeric order until the next
speaker line. The supported response tests use Charisma, Perception, alignment,
aptitude, race, level, gold, initial reaction, authored local flags, and
whether the actors have met in this session. Available character
statistics and inventory supply their values. Unresolved conditions are
recorded with their line IDs and hide the associated responses. Speaker
effects and script continuations retain their original references. No
dialogue effects or exit scripts run. A response with a standalone reaction
change may follow its literal text link with the skipped effect reported.
Later tests that rely on a skipped change remain unresolved.
Other effectful responses and negative continuations remain unavailable.
Plain `G:` greetings use the original class or race tables and available
character state. Several simple CE generic response directives use the installed `gd_*.mes`
tables. More complex directives depend on their corresponding gameplay systems.

## Voice packs and VFS overrides

Original recordings use `sound/speech/NNNNN/vS_m.mp3` and `vS_f.mp3`.
`NNNNN` is the five-digit dialogue script number, and `S` is the authored
recording number from field 5. For a female player the engine tries the female
recording before the male recording. A mod can replace an original recording
by mounting a file at that virtual path.

To map a recording to a DLG line ID, mount an optional
`dlg/<dialogue-stem>.voice.mes` beside the DLG resource. A map entry overrides
the original recording choice for that line. For example, for
`dlg/01324virgil.dlg` add `dlg/01324virgil.voice.mes` containing:

```text
{9}{sound/voices/my-pack/reply-9.wav}
```

Use a VFS-relative WAV or MP3 path. An empty value disables speech for the
line. The mapping can voice a line even if its DLG recording field is empty.
A missing mapped file is reported in dialogue diagnostics. The normal VFS
precedence applies to the DLG, map and recording independently. Use the
existing mod manifest and `[mods]` configuration to enable a dialogue or
voice pack. Speaker playback is replaced on each turn and stopped on close.

## Theme and diagnostics

Themes use the same HUD components and font definitions as inventory.
`dialogue_only = true` activates a component tree during dialogue. A root with this property is
placed in the shared band between the top and bottom HUDs. The
`dialogue-text` binding displays the speaker bubble. Five
`dialogue-response` bindings with `response = 0` through `4` define clickable
rows. Set `wrap = true` to wrap a response using the font metrics. Place
the rows with top-left anchors under a shared bottom-anchored dialogue
root to let the panel grow and scroll within the gameplay band. The
configured row sizes are minimums and the offsets set the gaps. Use `hover_text_color` for the available choice under the pointer.
`npc-context` displays the contextual name. See the Classic package for
working geometry and font definitions. Use `disabled_text_color` to
style a visible response with an unresolved action.

`rpc pos` includes `player.navigation.hovered_npc`, `dialogue.active`,
`dialogue.last_result`, physical choice bounds in `ui.hud.dialogue_responses`,
scroll bounds and offsets in `ui.hud.dialogue_scroll`, and
`audio.output.active_voice`. The active conversation includes the
NPC identity, script attachment and entry statement, DLG source, current
line, available response IDs and unresolved dependencies.
`rpc resource dlg/01324virgil.dlg` verifies DLG structure and VFS provenance. Dialogue is
transient and excluded from savegames and diagnostic snapshots.
