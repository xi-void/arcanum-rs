# Session objects and inventory

The inventory works with original mobile object GUIDs. A chest, a loose item and a shirt inside a chest keep their Milestone 61 identities and VFS provenance. The generic player starts wearing an original Nice Suit and carrying the no-background gold award. In the reference install this is 400 gold. Character creation, pre-game purchases and scripted opening rewards remain deferred.

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
| Drag a carried item onto an equipment silhouette | Equip there. If occupied, the old item returns to a free grid position |
| Drag a worn item onto the player grid | Unequip it to that position |
| Right-click a worn item | Unequip it to the first free grid position |
| Click a carried item, then **Drop** (the down-arrow target) or **D** | Place it on the player's passable tile |
| Drag a carried item onto **Drop** (the down-arrow target) | Same drop transaction |
| Click an item on the ground | Approach and pick up the same object |
| Wheel over either grid | Scroll that inventory |
| Loot scrollbar arrows or track | Scroll the container by row |
| **Esc** | Cancel a drag, otherwise close the window |

The independently themed window works with an alternate HUD or with the HUD disabled. Supply `--inventory-ui DIRECTORY` to load another package. The two manifests are `inventory.toml` and `container.toml`. See [the default package](../ui/inventory/container.toml) and [the public theme format](UI_THEMES.md). A `player-grid` or `container-grid` element defines its rectangle, fill, grid line and hover colors. Its width spans ten equal cells, its height a whole number of cells. Placement, picking, tooltip text, fonts and artwork use the shared UI scale. The grid uses original inventory ART independently of ground ART.

## Object state and transactions

- The M61 instance and prototype remain immutable. A sparse session table keyed by original GUID retains the original descriptor, source, and one current placement for each changed item. A placement is the player, an original container GUID, or a map and world tile, or one of the player’s nine worn slots. The original physical parent remains available in diagnostics. Faction ownership and crime are separate, unevaluated gameplay properties.
- Accessible original loot is arranged by inventory ART footprint. Each object occupies the ceiling of its original icon dimensions in 32-pixel cells. The underlying capacity is 10×12 for the player and 10×96 for containers. The visible scrolling window can be smaller. Compact grid indexes record item locations. An occupied or out-of-bounds destination rejects the transfer.
- Input and RPC call the same transfer method. It checks source membership, identity, distance and the shared edge of an adjacent tile, access and item flags, script restrictions, destination space and ground appearance before committing. Only whole, supported item objects may move. Owners must be the player or a resident, independently placed container. This excludes self-containment, nested-container cycles and transfers from an NPC.
- Picking up a world object removes its active raster and interaction target. Dropping prepares the original world ART and inserts a target into the shared ordered scene. Supported loose item categories are nonblocking under the existing collision rules. Picking up an item leaves collision from scenery and its container intact.
- Map changes move the session state into the next world. Sector activation suppresses the old placement and projects drops at their current map and tile. Sector eviction can release the original raster, object payload and textures. Changes and modified inventory slot indexes live for the process lifetime. Closing a container releases its unmodified index. The map and inventory share one gameplay-thread VFS, while background sector preparation keeps its existing resource ownership.
- Snapshots restore their documented location context. An in-session restore keeps subsequent inventory, drops and chest edits. A new process starts from original data. The compatibility scanner still measures static containment. Gameplay and regression tests exercise dynamic transfers.

### Original evidence and boundaries

CE's `src/game/item.c` supplies `item_inv_icon_size`, `item_inventory_slots_has_room_for`, horizontal first-fit placement, `item_arrange_inventory`, `item_check_remove`, `item_transfer_ex` and `item_drop_ex`. Transferable items resolve `OBJ_F_ITEM_INV_AID` separately from their world appearance. The initial slot and parent remain available independently of current membership. CE's container use path in `src/game/ai.c` and lock evaluation in `src/game/object.c` inform the access checks. Unsupported transfer/use scripts, trapped or locked containers and unresolved inventory dependencies can prevent interaction. Diagnostics return the reason.

Ammo and gold retain a known serialized quantity when available. Splitting, stacking, auto-generating loot, consumption, theft and nested container manipulation are future work. There is no disk persistence.

### Equipment and character statistics

The same original GUID moves between the backpack grid and the worn slot. Replacing gear validates space for the displaced item before changing either object. The space the incoming item vacates is available for the replacement. Items requiring wield scripts are rejected. The original category, armor coverage in the inventory ART ID, torso size and gender flags, destroyed ART flag and fixed two-handed conflicts determine supported compatibility. The two ring slots share the same compatibility rule. These rules come from CE's `item_location_get`, `sub_464D20` and TIG's item ART ID helpers.

The V119 reader retains the original 28-entry critter statistic array and reads equipment properties from the same instance/prototype fields used by the world loader. Instance fields override prototype fields, including explicit zero values and replaced arrays. The player starts with the original generic-PC baseline from CE's `stat_set_defaults`, or the installed generic PC prototype (description 16066, serialized ID 16086). The eight primary attributes are in CE order (strength, dexterity, constitution, beauty, intelligence, perception, willpower, charisma). Additive attribute, resistance and speed clauses come from the winning `rules/effect.mes` race and gender entries. Other clauses and NPC dynamic stat effects await their source systems.

Starting item creation uses the winning Nice Suit and Gold prototypes (serialized IDs 8069 and 9076 for the default human male). CE's `item_generate_inventory` chooses starter clothing by race size and gender, with an appropriately sized jacket fallback for incompatible clothing. `background_generate_inventory` reads entry 3 of `rules/backgrnd.mes` for the no-background gold award; entry 4 in the reference install has an empty item list. These objects participate in the same ownership store and inventory transactions as world items, with distinct `runtime:N` session identities and prototype provenance. The initial award runs once per application game session.

The backpack and worn items both contribute to weight. CE's `item_weight` counts gold as zero and scales ammo by the integer quotient of quantity divided by four. `stat_base_get` computes capacity from strength. `critter_encumbrance_level_get` uses the 15%, 30%, 45%, 60%, 75% and 90% tier boundaries, with a strict less-than comparison. The installed effect 158 sets the speed penalty per tier, with two additional applications at severe encumbrance. Walking and running use CE's `sub_437990` cadence and keep their authored timing at speed 8.

Equipment applies the supported AC and resistance modifiers using CE's `object_get_ac` and `object_get_resistance`. Diagnostic resistance order is normal, fire, electrical, poison and magick. Nonzero adjustments requiring magick/tech aptitude and triggered wear spells are surfaced as unresolved. Pickups whose resulting weight cannot be verified are rejected. Equipped items use original paper-doll ART where available, with the item's original grid icon as fallback. Body armor changes the standing, walking and running assets through the shared critter resolver. CE's `sub_465AE0` uses the armor subtype and palette from the item's use ART fragment; an empty armor slot selects underwear with the original character palette. The swap preserves the current movement step, facing and normalized animation phase. Weapon combat stances, triggered item effects and carried lights remain deferred. Weapon hit bonus, speed and minimum strength are decoded for inspection, with their combat effects deferred. There is no character creation, equipment training or disk persistence.

`character` reports the supported statistics and unresolved dependencies. `inventory` adds all worn items and their current slots. `equip GUID SLOT` and `unequip SLOT [CELL]` use the same transactions as dragging in the inventory. Failed equipment operations return `equipment_rejected` with the reason. `owner ITEM_ID` continues to report the original resource and current location even after a map change.
