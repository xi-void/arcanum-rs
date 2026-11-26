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
its numbered `scr/*.scr` resource in the VFS and uses the matching
`dlg/*.dlg` stem. SCR begins at statement zero. The shared interpreter
supports equality, signed less-than-or-equal, local and global flag tests,
daytime and NPC tests, the attachee's meeting history with the local PC,
tile-distance checks for the triggerer and attachee, player and global quest conditions,
assignment, addition, subtraction and random ranges,
flag and quest writes, branches, both return modes, and dialogue entry. It reads
literals, global state, attachment counters, and invocation-local temporary
variables. It reports unsupported instructions, invalid branches, and
execution beyond 1,000 statements with the failing statement and SCR source.

CE's `script_file_load_code` defines 132-byte statements with a condition
and two 44-byte actions. The resource flags are separate from the flags and
packed counters in each object's 12-byte attachment record. Nonzero resource
flags defer execution because traps, automatic removal, speech eligibility, and
movement triggers require their original event behavior. The existing literal
Use teleport decoder shares this framing. Unlock checks evaluate read-only
handlers against the current world script state. Static worlds initialize that
state to new-game defaults.

SCR action 37, `SAT_RANDOM`, reads its first two operands as numeric
bounds and assigns the inclusive result through the third operand. CE's
`random_between` accepts either bound order and consumes no draw for equal
bounds. The shared SCR setter handles invocation temporaries, global
variables and flags, and attachment counters. It follows CE's packed-word
writes and ignores immediate destinations. Per-PC variable and flag operands
remain deferred. Read-only unlock checks reject rolls that need to advance
the random stream.

The SCR stream uses the engine's xorshift generator independently of audio.
A successful transition commits its draws together with its other script
state. A rejected transition discards the staged draws and writes. Native
save/load resumes the committed stream. Saves from before this field was
introduced start a fresh stream. See [Native saves](SAVEGAMES.md).

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
aptitude, race, level, gold, initial reaction, whether the actors have met in
this session, and shared script state. The `gf`, `gv`, `lf`, and `lc` tests
compare the addressed global flag, global variable, attachment flag, and
attachment counter to the specified value. The same opcodes can write state
in a speaker or response effect list. See [Quests](QUESTS.md) for the
`qu`, `qa` and `qb` conditions and the `qu` effect.

Response effects run in source order before the destination. Speaker effects
run when entering the line, before building its choices. A zero destination
closes the conversation after applying the response effects. A negative
destination executes the Dialogue attachment at the absolute statement index.
An explicit continuation can return or open another dialogue line. Effects
from a rejected transition are discarded together with its staged script
writes. Runtime rejections leave the current conversation open and show the
reason in the NPC context area. Themes without that binding display the reason
with the speaker text. `dialogue.last_result` records the attempted response,
destination, effect, rejection stage and the SCR fault when applicable.
The implicit exit handler and the default floating reply on an
entry return remain deferred. Diagnostics retain the original return mode.

Unresolved conditions hide the associated responses and identify their line
IDs in diagnostics. A mixed effect list with an unsupported operation cannot
commit a partial set of writes. The existing text fallback can display
speaker lines with deferred gameplay effects. A standalone reaction change
can still follow its literal text link with the skipped effect reported.
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
`rpc resource dlg/01324virgil.dlg` and `rpc resource scr/01324virgil.scr`
report resource framing and VFS provenance. `rpc pos` includes committed
script state under `scripts`. Globals initialize to zero. Local state starts
from the original attachment and writes are stored by persistent object
identity and attachment point. It survives sector eviction, map changes and
native save/load. Conversations, invocation temporaries, and the session's
meeting history are transient. Diagnostic snapshots preserve the current
session's script state without capturing or restoring it.
