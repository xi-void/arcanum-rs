# Session objects and inventory

The inventory works with original mobile object GUIDs. A chest, a loose item and a shirt inside a chest keep their Milestone 61 identities and VFS provenance. The playable character starts with an empty inventory. Character creation and its starting equipment are still pending.

## Play

To try the Bates Mansion wood chest, launch the playable build and click the rightmost of the three chests beside the player.

```sh
cargo run --locked --release --features windowed -- play "$HOME/Work/arcanum-ce-test" --map 5011 --center 80 103
```

Open a reachable container by clicking its world sprite. Use **I** for your backpack. In the container window the left grid shows its contents and the right grid shows your inventory. Hover over an icon for its original description name, category and known quantity.

| Input | Action |
|---|---|
| Drag an icon to an empty space | Transfer it or reposition it within its grid |
| Right-click an icon with a container open | Transfer it into the first available space on the opposite side |
| Click a carried item, then **Drop** (the down-arrow target) or **D** | Place it on the player's passable tile |
| Drag a carried item onto **Drop** (the down-arrow target) | Same drop transaction |
| Click an item on the ground | Approach and pick up the same object |
| Wheel over either grid | Scroll that inventory |
| Loot scrollbar arrows or track | Scroll the container by row |
| **Esc** | Cancel a drag, otherwise close the window |

The independently themed window works with an alternate HUD or with the HUD disabled. Supply `--inventory-ui DIRECTORY` to load another package. The two manifests are `inventory.toml` and `container.toml`. See [the default package](../ui/inventory/container.toml) and [the public theme format](UI_THEMES.md). A `player-grid` or `container-grid` element defines its rectangle, fill, grid line and hover colors. Its width spans ten equal cells, its height a whole number of cells. Placement, picking, tooltip text, fonts and artwork use the shared UI scale. The grid uses original inventory ART independently of ground ART.

## Object state and transactions

- The M61 instance and prototype remain immutable. A sparse session table keyed by original GUID retains the original descriptor, source, and one current placement for each changed item. A placement is the player, an original container GUID, or a map and world tile. The original physical parent remains available in diagnostics. Faction ownership and crime are separate, unevaluated gameplay properties.
- Accessible original loot is arranged by inventory ART footprint. Each object occupies the ceiling of its original icon dimensions in 32-pixel cells. The underlying capacity is 10×12 for the player and 10×96 for containers. The visible scrolling window can be smaller. Compact grid indexes record item locations. An occupied or out-of-bounds destination rejects the transfer.
- Input and RPC call the same transfer method. It checks source membership, identity, distance and the shared edge of an adjacent tile, access and item flags, script restrictions, destination space and ground appearance before committing. Only whole, supported item objects may move. Owners must be the player or a resident, independently placed container. This excludes self-containment, nested-container cycles and transfers from an NPC.
- Picking up a world object removes its active raster and interaction target. Dropping prepares the original world ART and inserts a target into the shared ordered scene. Supported loose item categories are nonblocking under the existing collision rules. Picking up an item leaves collision from scenery and its container intact.
- Map changes move the session state into the next world. Sector activation suppresses the old placement and projects drops at their current map and tile. Sector eviction can release the original raster, object payload and textures. Changes and modified inventory slot indexes live for the process lifetime. Closing a container releases its unmodified index. The map and inventory share one gameplay-thread VFS, while background sector preparation keeps its existing resource ownership.
- Snapshots restore their documented location context. An in-session restore keeps subsequent inventory, drops and chest edits. A new process starts from original data. The compatibility scanner still measures static containment. Gameplay and regression tests exercise dynamic transfers.

### Original evidence and boundaries

CE's `src/game/item.c` supplies `item_inv_icon_size`, `item_inventory_slots_has_room_for`, horizontal first-fit placement, `item_arrange_inventory`, `item_check_remove`, `item_transfer_ex` and `item_drop_ex`. Transferable items resolve `OBJ_F_ITEM_INV_AID` separately from their world appearance. The initial slot and parent remain available independently of current membership. CE's container use path in `src/game/ai.c` and lock evaluation in `src/game/object.c` inform the access checks. Unsupported transfer/use scripts, trapped or locked containers and unresolved inventory dependencies can prevent interaction. Diagnostics return the reason.

Ammo and gold retain a known serialized quantity when available. Splitting, stacking, auto-generating loot, consumption, theft, equipping and nested container manipulation are future work. There is no disk persistence. A future equipment system can extend the same placement and transaction model with validated worn slots and character-stat effects.
