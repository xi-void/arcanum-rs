# Native savegames

Play starts at the original Crash Site. Press **Escape** or click the HUD menu
button to open the Classic pause menu. Choose **Save Game**, select **New**,
type a name, and click the right arrow or press **Enter**. Select an existing
slot and confirm to overwrite it under the same name. **Load Game** uses the
same slot list. Use the red delete button or **Delete** and confirm to remove
the selected save. The info toggle hides or shows the name, location and game
time over the world preview. **Quit Game** confirms and exits the application.

**F7** writes the fixed **Auto-Save** slot during play. **F8** loads the newest
save by file modification time, including named saves. These are the stock
key bindings in the CE reference. The unofficial patch moves quickload to F9.
The native engine uses F8.

On Linux, slots live in `$XDG_DATA_HOME/arcanum-rs/saves/`, with
`$HOME/.local/share/arcanum-rs/saves/` as the fallback. The directory is
created on the first write and is separate from configuration and installed
game files. Keep the same original data and gameplay mods when loading a slot.

Merchant purses, trade reaction adjustments, identified items, named-item transfers, split stacks and pending restock events persist across maps and restarts. Opening a version-1 slot written before trading initializes the new fields from original data on demand.

Using a key retains its original inventory identity. The engine restores its code from the winning instance and prototype on load. Open doors retain their pose and collision state. Optional `scripts.sector_toggles` records overland sector block changes by map directory and sector ID. Older saves start with the authored `map.sbf` blocks.

Lockpicking and Cantrip persist the changed lock bits by original door or container identity. Jam and relock deadlines use absolute game seconds. Character skill entries, known colleges, college mastery, fatigue damage, recovery deadline, equipped-item mana expenditure and the skill random stream travel with the player. Current fatigue, effective skills and aptitude are recalculated using the restored character, location and installed rules. Loading a pre-ability version-1 save starts with no purchased technological skills or spells and zero fatigue damage. Pending targeting and unfinished attempts are transient.

## Format version 1

Each slot is a single `.ars` file. Its name is an engine-generated identifier.
`auto.ars` is the quicksave slot. The user-facing name is stored in metadata.

| Section | Encoding |
| --- | --- |
| Signature | Eight ASCII bytes, `ARCRSAVE` |
| Header | Five little-endian unsigned 32-bit words for version, metadata byte count, preview byte count, state byte count and CRC-32 |
| Metadata | UTF-8 JSON with `name`, `location`, total game `hours` and `saved_at_ms` (Unix epoch milliseconds). Additional metadata fields may be added. |
| Preview | A 468 × 300 opaque RGB8 image, top-to-bottom row order, captured through the Vulkan world compositor. A zero-length preview is accepted by the format. |
| State | UTF-8 JSON with map directory, committed tile and facing, total game hours and elapsed milliseconds within the current hour, character data, item transactions, door changes, script state, discovered area IDs and explored town-map pixels |
| CRC-32 | IEEE CRC-32 over the three sections, stored in the header |

Object state carries changed item IDs, original-record or prototype references,
owners, grid slots, equipment slots, stack quantities, drop positions and
merged-stack links. Generated IDs retain the allocator state. Door IDs use
the original map directory, sector and record index. The engine rebuilds
sector caches and GPU resources from the installed data during restoration.

The optional `scripts` field stores committed global flags and variables and
local state keyed by original object identity and attachment point. Earlier
version 1 files lack this field and initialize globals to zero and locals from
their original attachments. The script state includes the local PC’s quest
records, their timestamps and last-write provenance, and global quest
resolution. The optional `scripts.random` field stores the committed SCR
random stream. Loading resumes the next draw, and older saves without the
field initialize a fresh stream. Rejected transitions leave the committed
stream unchanged. Earlier version 1 files without the quest field retain
their script flags and initialize quests to their new-game defaults. Active
conversations and interpreter temporaries are transient.

Writes sync a complete temporary file in the save directory before publishing
it atomically. Loads validate the header, checksum and saved state, rebuild
item and character resources from the mounted VFS, and prepare the destination
before replacing the running session. The destination uses the saved committed
tile. Routes, in-progress animations, camera settings and UI panels are
transient. Door changes restore to their settled open or closed state.

The format is native to Arcanum-RS. The save list does not display portraits,
levels or quest summaries.


The Classic package uses CE's `mainmenu_ui.c` in-play button positions,
save/load content rectangles and interface ART IDs 329, 745–748.
The pause labels use `Morph30Font.ART` (327). Slot text uses
`Flare12Font.ART` (229). The confirmation geometry comes from TIG's
`window.c`. Each asset is resolved through the existing VFS.

Version-1 saves also carry manual attribute and vital investments, health damage, all basic and technological skill ranks and training, colleges, degrees, base alignment, cumulative experience and unspent points. Earlier saves retain Haggle, the existing aptitude counters and known abilities. Missing newer fields take their original empty defaults. Loading rebuilds equipped effects from the winning item and rule resources. Consumed crafting components retain stable identities and stay unavailable after a map change or restart. The current sheet visit’s refundable purchases expire when the sheet closes or a game is loaded.
