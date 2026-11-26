//! Connect session inventory to original map records and world presentation.
use super::super::character::{Character, CharacterStatistics, Encumbrance, EquipmentSlot, Rules};
use super::super::inventory::{COLUMNS, Footprint, Grid, Item, Owner, Place, State, Tile};
use super::super::player::Appearance;
use super::super::read_bounded;
use crate::sector::objects::ItemStats;

mod equipment;
mod save;
mod stock;
pub(super) mod trade;
use super::*;
use crate::prototype::MAX_PROTOTYPE_BYTES;
use crate::sector::objects::{self, FieldValue, OriginalId};
use mobile::population::{Effective, Index, Prototypes};
pub use save::Objects;
use serde_json::{Value, json};

const ITEM_NO_DISPLAY: u32 = 0x10;
const ITEM_NO_DROP: u32 = 0x20;
// Destroyed, off, hidden, extinct or trapped objects require other gameplay systems.
const UNAVAILABLE: u32 = 0x0380_0303;
const CLICK_THROUGH: u32 = 0x800;

#[derive(Default)]
pub(super) struct Session {
    state: State,
    revision: u64,
    character: Character,
    rules: Rules,
    accounts: trade::Accounts,
    restocks: stock::Events,
    ground: BTreeMap<OriginalId, GroundArt>,
    unlocked: BTreeMap<String, BTreeSet<OriginalId>>,
}

struct GroundArt {
    asset: Arc<LoadedFrame>,
    flat: bool,
}

impl GroundArt {
    fn raster(&self, tile: Tile, reference: Tile, order: usize) -> Option<Raster> {
        let anchor = project_world_tile(tile, reference)?;
        let key = super::super::scenery::order(tile, (0, 0), self.flat, usize::MAX - order);
        let mut raster = Raster::new((true, key), &self.asset, anchor);
        raster.object_light = ObjectLight::Point;
        within_overhang(raster.bounds, anchor).then_some(raster)
    }
}

// CE item.c sub_463340. A zero on either side takes precedence over the master code.
fn key_matches(lock: i32, key: i32) -> bool {
    lock != 0 && key != 0 && (lock == key || lock == 1 || key == 1)
}

impl Session {
    pub(super) fn matching_key(&self, lock_id: i32) -> Option<OriginalId> {
        if lock_id == 0 {
            return None;
        }
        self.state.changes.values().find_map(|change| {
            (change.place == Place::Owned(Owner::Player)
                && change
                    .item
                    .stats
                    .key_ids
                    .iter()
                    .any(|&id| key_matches(lock_id, id)))
            .then_some(change.item.id)
        })
    }

    pub(super) fn hides_original(&self, map: &str, id: OriginalId) -> bool {
        self.state
            .changes
            .get(&id)
            .is_some_and(|change| change.item.origin_map == map)
    }

    pub(super) fn current(&self, map: &str, id: OriginalId) -> Option<&Place> {
        let change = self.state.changes.get(&id)?;
        (change.item.origin_map == map).then_some(&change.place)
    }
}

impl SectorCache {
    pub(super) fn can_reach_item(
        at: Tile,
        tile: Tile,
        cell: impl Fn(Tile) -> Option<collision::Cell>,
    ) -> bool {
        cell(tile).is_some()
            && (at == tile
                || [1u8, 3, 5, 7].into_iter().any(|dir| {
                    let (dx, dy) = collision::DIRECTIONS[usize::from(dir)];
                    at.0.checked_add_signed(dx) == Some(tile.0)
                        && at.1.checked_add_signed(dy) == Some(tile.1)
                        && cell(at).is_some_and(|c| !c.blocks_edge(dir))
                        && cell(tile).is_some_and(|c| !c.blocks_edge((dir + 4) % 8))
                }))
    }

    pub(super) fn project_inventory(&mut self) {
        self.dropped_targets.clear();
        if self.inventory.state.changes.is_empty() {
            return;
        }
        let Some((map, reference)) = &self.inventory_context else {
            return;
        };
        let inventory = &self.inventory;
        self.ordered.retain(|draw| {
            let (_, (_, y, x, _, _, record, _)) = draw.key;
            let Some(index) = record.checked_sub(mobile_draw_slot(0)) else {
                return true;
            };
            self.entries
                .get(&tile_sector_id((x, y)))
                .and_then(|(sector, _)| sector.mobiles.get(index))
                .is_none_or(|object| !inventory.hides_original(map, object.id))
        });
        for (id, art) in &inventory.ground {
            let change = &inventory.state.changes[id];
            if let Place::Ground(at, tile) = &change.place
                && at == map
                && self.active.contains(&tile_sector_id(*tile))
                && let Some(raster) = art.raster(*tile, *reference, change.order)
            {
                self.dropped_targets.insert(
                    raster.key,
                    interaction::Target {
                        tile: *tile,
                        action: interaction::Action::Pickup(*id),
                    },
                );
                self.ordered.push(raster);
            }
        }
    }
}

struct Records<'a> {
    vfs: &'a mut Vfs,
    map: &'a str,
    index: Option<&'a Index>,
    frames: FrameCache,
    prototypes: Prototypes,
}

impl Records<'_> {
    fn index(&self) -> Result<&Index, String> {
        self.index
            .ok_or_else(|| "Original map object index is pending".into())
    }

    fn read(&mut self, id: OriginalId) -> Result<(super::super::Source, Vec<u8>), String> {
        let path = self
            .index()?
            .path(id)
            .ok_or_else(|| {
                format!(
                    "Original object {} is missing or ambiguous in this map",
                    id.key()
                )
            })?
            .to_owned();
        read_bounded(self.vfs, &path, MAX_PROTOTYPE_BYTES).map_err(|e| e.to_string())
    }

    fn item(&mut self, id: OriginalId, state: &State) -> Result<Item, String> {
        if let Some(change) = state.changes.get(&id) {
            return Ok(change.item.clone());
        }
        let (source, bytes) = self.read(id)?;
        let object = objects::decode_instance(&bytes).map_err(|e| e.to_string())?;
        if object.id != id || !(5..=14).contains(&object.category) {
            return Err("Original record is not the requested item".into());
        }
        let index = self.index()?;
        if index.conflicting_owner(id) {
            return Err("Conflicting original ownership".into());
        }
        let parent = index.owner(id).map(|owner| (owner, index.category(owner)));
        let proto = self
            .prototypes
            .get(self.vfs, &object)
            .map_err(|e| e.to_string())?;
        let effective = Effective::new(&object, Some(&proto));
        let original = match parent {
            Some((owner, category)) => Place::Owned(match category {
                Some(2) => Owner::Container(self.map.to_owned(), owner),
                Some(_) => Owner::OriginalObject(self.map.to_owned(), owner),
                None => return Err("Unresolved original inventory owner".into()),
            }),
            None if effective
                .flags
                .value()
                .is_some_and(|flags| flags & 0x1000 == 0) =>
            {
                let tile = object
                    .tile()
                    .filter(|&tile| tile != (0, 0))
                    .ok_or("Unresolved original ground placement")?;
                Place::Ground(self.map.to_owned(), tile)
            }
            None => return Err("Unresolved original inventory owner".into()),
        };
        let field = |slot| object.field(slot).and_then(FieldValue::scalar);
        let inventory_art = field(70).or(proto.defaults.inventory_art);
        Ok(Item {
            id,
            stats: ItemStats::resolve(&object, proto.defaults.item.as_deref()),
            generated: false,
            category: object.category,
            description: field(22).or(proto.defaults.description),
            prototype: object.prototype.0,
            instance_source: source,
            prototype_source: proto.source.clone(),
            origin_map: self.map.to_owned(),
            original,
            authored_slot: field(71).map(|n| n as i32),
            inventory_art,
            footprint: Footprint::load(self.vfs, &mut self.frames, inventory_art),
            effective,
            flags: field(64)
                .or(proto.defaults.subtype_flags)
                .ok_or("Unresolved item flags")?,
            scripted: object.field(31).map_or(
                proto.defaults.inventory_scripts,
                objects::has_inventory_scripts,
            ),
        })
    }

    fn container(&mut self, id: OriginalId) -> Result<Container, String> {
        let (source, bytes) = self.read(id)?;
        let object = objects::decode_instance(&bytes).map_err(|e| e.to_string())?;
        if object.id != id || object.category != 2 {
            return Err("Owner is not the requested original container".into());
        }
        let index = self.index()?;
        if index.conflicting_owner(id) || index.owner(id).is_some() {
            return Err("Original container placement is unresolved".into());
        }
        let members = index.members(id);
        let proto = self
            .prototypes
            .get(self.vfs, &object)
            .map_err(|e| e.to_string())?;
        let effective = Effective::new(&object, Some(&proto));
        let flags = effective
            .flags
            .value()
            .ok_or("Unresolved container visibility flags")?;
        let subtype = |slot, default| object.field(slot).and_then(FieldValue::scalar).or(default);
        Ok(Container {
            tile: object
                .tile()
                .filter(|&tile| tile != (0, 0))
                .ok_or("Unresolved container tile")?,
            flags,
            sound_effect: effective
                .appearance
                .sound_effect
                .filter(|&id| id > 0 && id <= i32::MAX - 2),
            unlock: interaction::handler(
                self.vfs,
                &id.key(),
                super::super::script::Event::Unlock,
                effective.appearance.unlock_script.unwrap_or_default(),
                effective.appearance.unlock_local.unwrap_or_default(),
            ),
            on_use: interaction::handler(
                self.vfs,
                &id.key(),
                super::super::script::Event::Use,
                effective.appearance.use_script.unwrap_or_default(),
                effective.appearance.use_local.unwrap_or_default(),
            ),
            key_id: effective
                .appearance
                .lock_key_id
                .ok_or("Missing container key field")?,
            subtype_flags: subtype(64, proto.defaults.subtype_flags)
                .ok_or("Unresolved container flags")?,
            inventory_source: subtype(69, proto.defaults.inventory_source),
            scripted: object.field(31).map_or(
                proto.defaults.inventory_scripts,
                objects::has_inventory_scripts,
            ),
            provenance: json!({
                "prototype":object.prototype.0,
                "instance_source":super::super::diagnostics::source(&source),
                "prototype_source":super::super::diagnostics::source(&proto.source),
            }),
            members,
        })
    }

    fn layout(
        &mut self,
        state: &State,
        owner: &Owner,
        container: Option<&Container>,
    ) -> Result<Layout, String> {
        let mut layout = Layout::default();
        let members = match owner {
            Owner::OriginalObject(map, id)
                if map == self.map && self.index()?.category(*id) == Some(16) =>
            {
                Some(self.index()?.members(*id))
            }
            Owner::OriginalObject(..) => return Err("Invalid NPC inventory owner".into()),
            _ => container.map(|c| c.members.clone()),
        };
        if let Some(members) = members {
            let mut originals = members
                .iter()
                .map(|&id| self.item(id, state))
                .collect::<Result<Vec<_>, _>>()?;
            // CE loot arrangement packs larger icons first, then scans rows.
            originals.sort_by_key(|item| {
                let [w, h] = item.footprint.cells;
                std::cmp::Reverse((h, w))
            });
            for item in originals {
                if item.original != Place::Owned(owner.clone()) {
                    return Err(format!(
                        "Inconsistent original container member {}",
                        item.id.key()
                    ));
                }
                if !item
                    .authored_slot
                    .is_some_and(|slot| matches!(slot, 1000..=1008 | 2000..=2009))
                {
                    let cells = item.footprint.cells;
                    let slot = layout
                        .grid
                        .fit(item.id, cells, owner.rows(), None)
                        .ok_or("Original container contents exceed spatial capacity")?;
                    layout.grid.items.insert(item.id, (slot, cells));
                }
                if !state.changes.contains_key(&item.id) {
                    layout.items.insert(item.id, item);
                }
            }
        }
        layout
            .grid
            .items
            .retain(|id, _| !state.changes.contains_key(id));
        for (&id, change) in &state.changes {
            if matches!(&change.place, Place::Owned(current) if current == owner) {
                let slot = change.slot.expect("owned items have a grid slot");
                layout
                    .grid
                    .items
                    .insert(id, (slot, change.item.footprint.cells));
                layout.items.insert(id, change.item.clone());
            }
        }
        Ok(layout)
    }

    fn ground_art(
        &mut self,
        item: &Item,
        tile: Tile,
        reference: Tile,
    ) -> Result<GroundArt, String> {
        // CE object_drop clears the inventory flag and resets both offsets.
        let mut effective = item.effective;
        effective.appearance.offset_x = Some(0);
        effective.appearance.offset_y = Some(0);
        let frame = mobile::load_prop(
            self.vfs,
            &mut self.frames,
            &mut BTreeMap::new(),
            &effective,
            (item.category, tile),
            reference,
        )
        .map_err(|e| format!("Ground ART unavailable: {e}"))?;
        let art = GroundArt {
            asset: frame.asset,
            flat: frame.flat,
        };
        art.raster(tile, reference, 0)
            .ok_or("Ground ART exceeds sector overhang or projection bounds")?;
        Ok(art)
    }
}

struct Container {
    tile: Tile,
    flags: u32,
    subtype_flags: u32,
    key_id: i32,
    unlock: interaction::Handler,
    on_use: interaction::Handler,
    inventory_source: Option<u32>,
    scripted: bool,
    sound_effect: Option<i32>,
    provenance: Value,
    members: BTreeSet<OriginalId>,
}

impl Container {
    fn check_access(
        &self,
        world: &InteractiveWorld,
        id: OriginalId,
        hour: u8,
    ) -> Result<(), String> {
        let player = world
            .cache
            .player
            .as_ref()
            .filter(|p| !p.moving())
            .ok_or("Player must be stationary")?;
        let cell =
            |tile| SectorCache::player_cell(&world.cache.entries, &world.cache.door_states, tile);
        if !collision::can_reach_container(player.tile(), self.tile, cell) {
            return Err("Container is outside loaded interaction reach".into());
        }
        if self.flags & (UNAVAILABLE | CLICK_THROUGH | 0x1000) != 0 {
            return Err("Container is hidden, inactive, trapped or otherwise unavailable".into());
        }
        if self.subtype_flags & 0x80 == 0 {
            interaction::check_handler(
                &self.unlock,
                hour,
                &world.cache.scripts,
                self.tile,
                Some(player.tile()),
            )?;
            interaction::check_lock(
                "Container",
                self.subtype_flags,
                hour,
                world.container_unlocked(id)
                    || world.cache.inventory.matching_key(self.key_id).is_some(),
            )?;
        }
        interaction::check_handler(
            &self.on_use,
            hour,
            &world.cache.scripts,
            self.tile,
            Some(player.tile()),
        )?;
        if self.scripted {
            return Err("Container requires unsupported inventory scripts".into());
        }
        Ok(())
    }

    fn report(&self) -> Value {
        json!({"tile":self.tile,"flags":self.subtype_flags,"inventory_source":self.inventory_source,
            "key_id":self.key_id,"scripted":self.scripted || self.unlock.is_some() || self.on_use.is_some(),"provenance":self.provenance})
    }
}

#[derive(Default)]
struct Layout {
    items: BTreeMap<OriginalId, Item>,
    grid: Grid,
}

fn parse_id(key: &str) -> Result<OriginalId, String> {
    OriginalId::parse_key(key)
        .or_else(|| OriginalId::parse_with_kind(key, u16::MAX))
        .ok_or_else(|| "Expected an original GUID or a session item ID".into())
}

/// Read-only presentation data projected from the same owner layout as inventory RPC.
pub struct InventoryContents {
    pub columns: usize,
    pub rows: usize,
    pub entries: Vec<InventoryEntry>,
    pub equipment: Vec<InventoryEntry>,
    pub container_sound: Option<i32>,
    grid: Grid,
}

impl InventoryContents {
    pub fn fits(&self, item: &InventoryEntry, slot: usize, same_owner: bool) -> bool {
        if item.quantity.is_some()
            && self.entries.iter().any(|other| {
                other.id != item.id
                    && other.prototype == item.prototype
                    && other.category == item.category
                    && other.ammo_type == item.ammo_type
                    && other.movable
                    && other.quantity.is_some()
                    && (!same_owner
                        || parse_id(&other.id).is_ok_and(|id| self.grid.contains_cell(id, slot)))
            })
        {
            return true;
        }
        parse_id(&item.id).is_ok_and(|id| {
            self.grid
                .fit(id, item.cells, self.rows, Some(slot))
                .is_some()
        })
    }
}

pub struct InventoryEntry {
    pub id: String,
    pub equipment: Option<EquipmentSlot>,
    pub quantity: Option<u32>,
    pub detail: String,
    pub price: Option<u32>,
    pub category: u32,
    pub ammo_type: Option<u32>,
    pub prototype: u32,
    pub slot: usize,
    pub cells: [usize; 2],
    pub name: String,
    pub movable: bool,
    pub pickup_sound: i32,
    pub removable: bool,
    pub icon: Result<Arc<LoadedFrame>, String>,
}

pub struct InventoryReader {
    pub(super) vfs: Vfs,
    pub(super) frames: FrameCache,
    pub(super) descriptions: Vec<super::super::LoadedMes>,
    key_descriptions: Option<super::super::LoadedMes>,
    pub quest_catalog: super::super::quest::Catalog,
}

impl InventoryReader {
    pub fn new(mut vfs: Vfs) -> Self {
        let descriptions = ["mes/gamedesc.mes", "mes/description.mes"]
            .into_iter()
            .filter_map(|path| super::super::load_mes(&mut vfs, path).ok())
            .collect();
        let key_descriptions = super::super::load_mes(&mut vfs, "mes/gamekey.mes").ok();
        let quest_catalog = super::super::quest::Catalog::load(&mut vfs);
        Self {
            vfs,
            frames: FrameCache::default(),
            descriptions,
            key_descriptions,
            quest_catalog,
        }
    }

    pub fn transfer(
        &mut self,
        world: &mut InteractiveWorld,
        item: &str,
        from: &str,
        to: &str,
        slot: Option<usize>,
    ) -> Result<Value, String> {
        world.transfer_item(&mut self.vfs, item, from, to, slot, None)
    }

    pub fn drop_item(&mut self, world: &mut InteractiveWorld, item: &str) -> Result<Value, String> {
        world.drop_item(&mut self.vfs, item)
    }

    pub fn pickup(&mut self, world: &mut InteractiveWorld, item: &str) -> Result<Value, String> {
        world.pickup_item(&mut self.vfs, item, None)
    }

    pub fn take_all(&mut self, world: &mut InteractiveWorld, target: &str) -> Result<(), String> {
        world.take_all(&mut self.vfs, target)
    }

    pub fn arrange(
        &mut self,
        world: &mut InteractiveWorld,
        target: Option<&str>,
        vertical: bool,
    ) -> Result<(), String> {
        world.arrange_inventory(&mut self.vfs, target, vertical)
    }

    pub fn contents(
        &mut self,
        world: &InteractiveWorld,
        target: Option<&str>,
    ) -> Result<InventoryContents, String> {
        let trading = world
            .trade
            .as_ref()
            .filter(|t| target == Some(t.npc.as_str()));
        let target = target.map(parse_id).transpose()?;
        let owner = if let Some(t) = trading {
            t.stock.clone()
        } else {
            target.map_or(Owner::Player, |id| Owner::Container(world.map.clone(), id))
        };
        let mut records = world.records(&mut self.vfs);
        let container = if let Owner::Container(_, id) = &owner {
            Some(records.container(*id)?)
        } else {
            None
        };
        if trading.is_none()
            && let (Some(container), Some(id)) = (&container, target)
        {
            container.check_access(world, id, world.cache.hour)?;
        }
        let layout = records.layout(&world.cache.inventory.state, &owner, container.as_ref())?;
        let mut entries = Vec::new();
        let mut equipment = Vec::new();
        let grid_items = layout
            .grid
            .items
            .iter()
            .map(|(&id, &(slot, cells))| (&layout.items[&id], slot, cells, None));
        let state = &world.cache.inventory.state;
        let equipped_items = state.changes.values().filter_map(|change| {
            let Place::Equipped(slot) = change.place else {
                return None;
            };
            (owner == Owner::Player).then_some((
                &change.item,
                0,
                change.item.footprint.cells,
                Some(slot),
            ))
        });
        for (item, slot, cells, equipped) in grid_items.chain(equipped_items) {
            if !item_visible(item) {
                continue;
            }
            let key_name = if item.category == 11 {
                item.stats
                    .key_ids
                    .first()
                    .and_then(|id| self.key_descriptions.as_ref()?.mes.get(*id))
            } else {
                None
            };
            let name = key_name
                .or_else(|| {
                    item.description.and_then(|id| {
                        self.descriptions
                            .iter()
                            .find_map(|table| table.mes.get(id as i32))
                    })
                })
                .or_else(|| objects::category_name(item.category))
                .unwrap_or("Item")
                .to_owned();
            let icon = super::super::inventory::inventory_frame(
                &mut self.vfs,
                &mut self.frames,
                equipped
                    .and(item.stats.paper_doll_art)
                    .filter(|n| n >> 28 == 6)
                    .or(item.inventory_art),
            )
            .map_err(|error| error.to_string());
            let quote = world
                .trading()
                .then(|| world.quote(item, trading.is_some()).map(|(p, _)| p));
            let mut detail = item.detail(world.cache.inventory.aptitude());
            if let Some(quote) = &quote {
                match quote {
                    Ok(price) => detail.push_str(&format!("  {} gold each", price)),
                    Err(e) => {
                        detail.push_str("  ");
                        detail.push_str(e);
                    }
                }
            }
            let entry = InventoryEntry {
                pickup_sound: item.handling_sound(true, world.cache.inventory.aptitude()),
                id: item.id.key(),
                equipment: equipped,
                quantity: item.stats.quantity,
                detail,
                price: quote.and_then(Result::ok),
                category: item.category,
                ammo_type: item.stats.ammo_type,
                prototype: item.prototype,
                slot,
                cells,
                name,
                movable: check_item(item).is_ok(),
                removable: item.flags & ITEM_NO_DROP == 0,
                icon,
            };
            if equipped.is_some() {
                equipment.push(entry);
            } else {
                entries.push(entry);
            }
        }
        self.frames.finish_sector();
        Ok(InventoryContents {
            columns: COLUMNS,
            container_sound: container
                .as_ref()
                .and_then(|container| container.sound_effect),
            rows: owner.rows(),
            entries,
            equipment,
            grid: layout.grid,
        })
    }
}

impl InteractiveWorld {
    pub fn inventory_revision(&self) -> u64 {
        self.cache.inventory.revision
    }

    fn records<'a>(&'a self, vfs: &'a mut Vfs) -> Records<'a> {
        Records {
            vfs,
            map: &self.map,
            index: self.mobile_index.get(),
            frames: FrameCache::default(),
            prototypes: Prototypes::default(),
        }
    }

    fn inventory_owner(&self, key: &str) -> Result<Owner, String> {
        if key == "player" {
            return Ok(Owner::Player);
        }
        let id = parse_id(key)?;
        Ok(
            if self.mobile_index.get().and_then(|index| index.category(id)) == Some(16) {
                Owner::OriginalObject(self.map.clone(), id)
            } else {
                Owner::Container(self.map.clone(), id)
            },
        )
    }

    pub fn inspect_inventory(&self, vfs: &mut Vfs, key: &str) -> Result<Value, String> {
        let owner = self.inventory_owner(key)?;
        let state = &self.cache.inventory.state;
        let mut records = self.records(vfs);
        let container = match &owner {
            Owner::Player => None,
            Owner::Container(_, id) => Some(records.container(*id)?),
            Owner::OriginalObject(..) => None,
        };
        let layout = records.layout(state, &owner, container.as_ref())?;
        let npc_gold = matches!(owner, Owner::OriginalObject(..))
            .then(|| self.npc_gold(vfs, key))
            .transpose()?;
        let place = Place::Owned(owner.clone());
        let mut items: Vec<_> = layout.items.values().collect();
        items.sort_by_key(|item| {
            (
                layout
                    .grid
                    .items
                    .get(&item.id)
                    .map(|entry| entry.0)
                    .unwrap_or(usize::MAX),
                item.id,
            )
        });
        Ok(
            json!({"owner":place.report(),"gold":npc_gold,"columns":COLUMNS,"rows":owner.rows(),
            "container":container.as_ref().map(|container| {
                let mut report = container.report();
                if let Owner::Container(_, id) = owner {
                    report["access_error"] = json!(container.check_access(self, id, self.cache.hour).err());
                    report["matching_key"] = json!(self.cache.inventory.matching_key(container.key_id).map(OriginalId::key));
                    report["developer_unlock"] = json!(self.container_unlocked(id));
                }
                report
            }),
            "character":(owner == Owner::Player).then(|| self.character_statistics()),
            "counters":(owner == Owner::Player).then(|| self.inventory_counters()),
            "equipment":(owner == Owner::Player).then(|| EquipmentSlot::ALL.map(|slot| {
                state.changes.values().find(|change| change.place == Place::Equipped(slot))
                    .map(|change| change.item.report(&change.place, None))
            })),
            "items":items.into_iter().map(|item| item.report(&place, layout.grid.items.get(&item.id).map(|entry| entry.0))).collect::<Vec<_>>() }),
        )
    }

    pub fn inspect_item_owner(&self, vfs: &mut Vfs, key: &str) -> Result<Value, String> {
        let id = parse_id(key)?;
        let state = &self.cache.inventory.state;
        let mut records = self.records(vfs);
        let item = records.item(id, state)?;
        let place = state.place(&item);
        let slot = if let Some(change) = state.changes.get(&id) {
            change.slot
        } else if let Place::Owned(owner @ Owner::Container(_, container)) = place {
            // NPC-owned items retain provenance but aren't loot containers.
            records
                .container(*container)
                .ok()
                .map(|c| records.layout(state, owner, Some(&c)))
                .transpose()?
                .and_then(|layout| layout.grid.items.get(&id).map(|entry| entry.0))
        } else {
            None
        };
        let mut report = item.report(place, slot);
        if let Some(ground) = self.cache.inventory.ground.get(&id) {
            report["ground_art_source"] = super::super::diagnostics::source(&ground.asset.source);
        }
        Ok(report)
    }

    fn check_inventory_reach(&self, tile: Tile) -> Result<(), String> {
        let player = self
            .cache
            .player
            .as_ref()
            .filter(|p| !p.moving())
            .ok_or("Player must be stationary")?;
        let at = player.tile();
        let cell =
            |tile| SectorCache::player_cell(&self.cache.entries, &self.cache.door_states, tile);
        let reachable = SectorCache::can_reach_item(at, tile, cell);
        reachable
            .then_some(())
            .ok_or_else(|| "Item or container is outside loaded interaction reach".into())
    }

    pub fn pickup_item(
        &mut self,
        vfs: &mut Vfs,
        key: &str,
        slot: Option<usize>,
    ) -> Result<Value, String> {
        self.move_item(vfs, key, Move::Pickup(slot), 0, None)
    }

    pub fn drop_item(&mut self, vfs: &mut Vfs, key: &str) -> Result<Value, String> {
        self.move_item(vfs, key, Move::Drop, 0, None)
    }

    pub fn transfer_item(
        &mut self,
        vfs: &mut Vfs,
        key: &str,
        from: &str,
        to: &str,
        slot: Option<usize>,
        hour: Option<u8>,
    ) -> Result<Value, String> {
        if self.merchant_owns(from) || self.merchant_owns(to) {
            return self.trade_item(vfs, key, from, to, slot, None);
        }
        let from = self.inventory_owner(from)?;
        let to = self.inventory_owner(to)?;
        self.move_item(
            vfs,
            key,
            Move::Transfer(from, to, slot),
            hour.unwrap_or(self.cache.hour),
            None,
        )
    }

    pub fn transfer_quantity(
        &mut self,
        vfs: &mut Vfs,
        key: &str,
        from: &str,
        to: &str,
        slot: Option<usize>,
        quantity: u32,
    ) -> Result<Value, String> {
        if self.merchant_owns(from) || self.merchant_owns(to) {
            return self.trade_item(vfs, key, from, to, slot, Some(quantity));
        }
        let from = self.inventory_owner(from)?;
        let movement = if to == "ground" {
            if from != Owner::Player {
                return Err("Only player-owned items can be dropped".into());
            }
            Move::Drop
        } else {
            Move::Transfer(from, self.inventory_owner(to)?, slot)
        };
        self.move_item(vfs, key, movement, self.cache.hour, Some(quantity))
    }

    fn move_item(
        &mut self,
        vfs: &mut Vfs,
        key: &str,
        action: Move,
        hour: u8,
        quantity: Option<u32>,
    ) -> Result<Value, String> {
        let id = parse_id(key)?;
        let mut records = self.records(vfs);
        let state = &self.cache.inventory.state;
        let mut item = records.item(id, state)?;
        let current = state.place(&item).clone();
        let flags = check_item(&item)?;
        let mut remainder = None;
        if let Some(quantity) = quantity {
            let total = item.stats.quantity.ok_or("Item is not a divisible stack")?;
            if quantity == 0 || quantity > total {
                return Err("Quantity must be between 1 and the current stack size".into());
            }
            if quantity < total {
                let mut rest = item.clone();
                rest.stats.quantity = Some(total - quantity);
                item.id = state.generated_id();
                item.generated = true;
                item.stats.quantity = Some(quantity);
                remainder = Some(rest);
            }
        }
        let pickup = matches!(action, Move::Pickup(_));
        let authorized = matches!(action, Move::Exchange(..));
        let (destination, requested) = match action {
            Move::Pickup(slot) => {
                let Place::Ground(map, tile) = &current else {
                    return Err("Item is not on the ground".into());
                };
                if map != &self.map || flags & CLICK_THROUGH != 0 {
                    return Err("Item is not selectable on this map".into());
                }
                self.check_inventory_reach(*tile)?;
                (Place::Owned(Owner::Player), slot)
            }
            Move::Drop if current == Place::Owned(Owner::Player) => {
                let player = self
                    .cache
                    .player
                    .as_ref()
                    .filter(|p| !p.moving())
                    .ok_or("Player must be stationary")?;
                let tile = player.tile();
                if !collision::can_stand(SectorCache::player_cell(
                    &self.cache.entries,
                    &self.cache.door_states,
                    tile,
                )) {
                    return Err("No traversable ground at the player tile".into());
                }
                (Place::Ground(self.map.clone(), tile), None)
            }
            Move::Drop => return Err("Only player-owned items can be dropped".into()),
            Move::Transfer(from, to, slot) | Move::Exchange(from, to, slot) => {
                if !authorized
                    && matches!(
                        (&from, &to),
                        (Owner::OriginalObject(..), _) | (_, Owner::OriginalObject(..))
                    )
                {
                    return Err("NPC transfers require trade or script authorization".into());
                }
                if current != Place::Owned(from) {
                    return Err("Item does not belong to the specified source".into());
                }
                (Place::Owned(to), slot)
            }
        };
        if remainder.is_some() && current == destination {
            return Err("Split a stack by moving units to another owner or dropping them".into());
        }
        if current != destination && matches!(current, Place::Owned(_)) {
            check_removal(&item)?;
        }
        if !authorized && let Place::Owned(Owner::Container(_, source)) = &current {
            records
                .container(*source)?
                .check_access(self, *source, hour)?;
        }
        if destination == Place::Owned(Owner::Player) && current != destination {
            let inventory = &self.cache.inventory;
            if inventory.character.background == 72
                && item.stats.complexity.is_some_and(|value| value < 0)
            {
                return Err("Technophobia prevents picking up technological items".into());
            }
            if inventory
                .weight()
                .saturating_add(item.weight(inventory.aptitude()))
                > i64::from(inventory.statistics().carry_capacity)
            {
                return Err("Item exceeds the character's carry capacity".into());
            }
        }
        let mut ground = None;
        let layout = match &destination {
            Place::Owned(owner) => {
                let container = match owner {
                    Owner::Player => None,
                    Owner::Container(_, target) => {
                        let container = records.container(*target)?;
                        if !authorized {
                            container.check_access(self, *target, hour)?;
                        }
                        Some(container)
                    }
                    Owner::OriginalObject(..) if authorized => None,
                    Owner::OriginalObject(..) => return Err("Unsupported inventory owner".into()),
                };
                Some(records.layout(state, owner, container.as_ref())?)
            }
            Place::Ground(_, tile) => {
                ground = Some(records.ground_art(&item, *tile, self.reference)?);
                None
            }
            Place::Equipped(_) | Place::Merged(_) | Place::Spent | Place::Retired => {
                return Err("Invalid transfer destination".into());
            }
        };
        let remainder_ground = match (&remainder, &current) {
            (Some(rest), Place::Ground(_, tile))
                if !self.cache.inventory.ground.contains_key(&id) =>
            {
                Some(records.ground_art(rest, *tile, self.reference)?)
            }
            _ => None,
        };
        let source_slot = if let Some(change) = state.changes.get(&id) {
            change.slot
        } else if remainder.is_some() {
            match &current {
                Place::Owned(owner @ Owner::Container(_, source)) => {
                    let container = records.container(*source)?;
                    records
                        .layout(state, owner, Some(&container))?
                        .grid
                        .items
                        .get(&id)
                        .map(|entry| entry.0)
                }
                Place::Owned(owner @ Owner::OriginalObject(..)) => records
                    .layout(state, owner, None)?
                    .grid
                    .items
                    .get(&id)
                    .map(|v| v.0),
                _ => None,
            }
        } else {
            None
        };
        let merge = layout.as_ref().and_then(|layout| {
            // CE merges on insertion into another owner. An explicit move inside
            // a grid only merges when it is aimed at a different matching stack.
            layout
                .items
                .values()
                .filter(|candidate| {
                    candidate.id != id
                        && item.same_stack(candidate)
                        && item_visible(candidate)
                        && !candidate.scripted
                        && layout.grid.items.contains_key(&candidate.id)
                        && (current != destination
                            || requested
                                .is_some_and(|slot| layout.grid.contains_cell(candidate.id, slot)))
                })
                .min_by_key(|candidate| {
                    (
                        layout.grid.items.get(&candidate.id).map(|pair| pair.0),
                        candidate.id,
                    )
                })
                .cloned()
        });
        let world_changed = matches!(current, Place::Ground(..)) || ground.is_some();
        let session = &mut self.cache.inventory;
        let moved_id = item.id;
        let sound = item.handling_sound(pickup, session.aptitude());
        let mut result = if let Some(mut target) = merge {
            let quantity = item.stats.quantity.expect("matching stack");
            let total = target
                .stats
                .quantity
                .expect("matching stack")
                .checked_add(quantity)
                .filter(|&n| n <= i32::MAX as u32)
                .ok_or("Merged stack exceeds the original quantity range")?;
            target.stats.quantity = Some(total);
            let target_id = target.id;
            let slot = layout
                .as_ref()
                .and_then(|layout| layout.grid.items.get(&target_id))
                .map(|entry| entry.0);
            session.state.put(target, destination.clone(), slot);
            session.state.put(item, Place::Merged(target_id), None);
            json!({"id":moved_id.key(),"source_id":id.key(),"merged_into":target_id.key(),
                "quantity":quantity,"total":total,"from":current.report(),"to":destination.report(),"slot":slot})
        } else {
            session.state.relocate(
                item,
                destination,
                layout.as_ref().map(|l| (&l.grid, requested)),
            )?
        };
        if let Some(rest) = remainder {
            result["from"] = current.report();
            result["source_id"] = json!(id.key());
            result["remaining_quantity"] = json!(rest.stats.quantity);
            session.state.put(rest, current, source_slot);
            session.state.next_generated += 1;
        } else {
            session.ground.remove(&id);
        }
        if let Some(art) = ground {
            session.ground.insert(moved_id, art);
        }
        if let Some(art) = remainder_ground {
            session.ground.insert(id, art);
        }
        session.revision += 1;
        self.cache.update_character_movement();
        if world_changed {
            self.cache.activate(&self.cache.active.clone());
        }
        self.queue_sound(super::SoundEvent::Effect(sound));
        Ok(result)
    }

    fn take_all(&mut self, vfs: &mut Vfs, target: &str) -> Result<(), String> {
        if self.trading() {
            return Err("Take all is unavailable while trading".into());
        }
        let id = parse_id(target)?;
        let source = Owner::Container(self.map.clone(), id);
        let mut records = self.records(vfs);
        let container = records.container(id)?;
        container.check_access(self, id, self.cache.hour)?;
        let loot = records.layout(&self.cache.inventory.state, &source, Some(&container))?;
        let mut slots: Vec<_> = loot.grid.items.into_iter().collect();
        slots.sort_unstable_by_key(|(_, (slot, _))| *slot);
        let mut remaining = 0;
        let mut reason = None;
        for (id, _) in slots {
            if !item_visible(&loot.items[&id]) {
                continue;
            }
            if let Err(error) = self.move_item(
                vfs,
                &id.key(),
                Move::Transfer(source.clone(), Owner::Player, None),
                self.cache.hour,
                None,
            ) {
                reason.get_or_insert(error);
                remaining += 1;
            }
        }
        reason.map_or(Ok(()), |reason| {
            Err(format!("{remaining} item(s) left: {reason}"))
        })
    }

    fn arrange_inventory(
        &mut self,
        vfs: &mut Vfs,
        target: Option<&str>,
        vertical: bool,
    ) -> Result<(), String> {
        let mut records = self.records(vfs);
        let state = &self.cache.inventory.state;
        let mut planned = Vec::new();
        for owner in ["player"]
            .into_iter()
            .chain(target.filter(|_| !self.trading()))
        {
            let owner = self.inventory_owner(owner)?;
            let container = match &owner {
                Owner::Container(_, id) => {
                    let container = records.container(*id)?;
                    container.check_access(self, *id, self.cache.hour)?;
                    Some(container)
                }
                _ => None,
            };
            let mut layout = records.layout(state, &owner, container.as_ref())?;
            let mut moving = Vec::new();
            layout.grid.items.retain(|id, entry| {
                let (slot, cells) = *entry;
                let item = &layout.items[id];
                if check_item(item).is_err() {
                    return true;
                }
                moving.push((std::cmp::Reverse((cells[1], cells[0])), slot, *id));
                false
            });
            moving.sort_unstable();
            let mut changes = Vec::new();
            for (_, previous, id) in moving {
                let item = layout.items.remove(&id).expect("layout item");
                let cells = item.footprint.cells;
                let slot = layout
                    .grid
                    .fit_ordered(id, cells, owner.rows(), None, vertical)
                    .ok_or("Inventory cannot be arranged within its capacity")?;
                layout.grid.items.insert(id, (slot, cells));
                if previous != slot {
                    changes.push((item, slot));
                }
            }
            planned.push((Place::Owned(owner), layout.grid, changes));
        }
        let session = &mut self.cache.inventory;
        for (place, grid, changes) in planned {
            for (item, slot) in changes {
                session
                    .state
                    .relocate(item, place.clone(), Some((&grid, Some(slot))))
                    .expect("arrangement slots validated before commit");
                session.revision += 1;
            }
        }
        Ok(())
    }

    pub(super) fn container_unlocked(&self, id: OriginalId) -> bool {
        self.cache
            .inventory
            .unlocked
            .get(&self.map)
            .is_some_and(|ids| ids.contains(&id))
    }

    pub(super) fn unlock_container(
        &mut self,
        vfs: &mut Vfs,
        id: OriginalId,
    ) -> Result<bool, String> {
        let container = self.records(vfs).container(id)?;
        if container.scripted
            || container.unlock.is_some()
            || container.on_use.is_some()
            || container.subtype_flags & 6 != 0
        {
            return Err("Container has a scripted, jammed or magical restriction".into());
        }
        if container.flags & (UNAVAILABLE | CLICK_THROUGH | 0x1000) != 0 {
            return Err("Container is unavailable".into());
        }
        if container.subtype_flags & 1 == 0 || container.subtype_flags & (8 | 0x80) != 0 {
            return Ok(false);
        }
        let session = &mut self.cache.inventory;
        let changed = session
            .unlocked
            .entry(self.map.clone())
            .or_default()
            .insert(id);
        session.revision += u64::from(changed);
        Ok(changed)
    }

    pub fn inherit_object_state(&mut self, previous: &mut Self) {
        self.met_npcs = std::mem::take(&mut previous.met_npcs);
        self.cache.inventory = std::mem::take(&mut previous.cache.inventory);
        self.restore_script_state(std::mem::take(&mut previous.cache.scripts));
        if let (Some(player), Some(old)) = (&mut self.cache.player, &previous.cache.player) {
            player.appearance = Arc::clone(&old.appearance);
        }
        self.cache.update_character_sector();
        self.cache.update_character_movement();
        if !self.cache.active.is_empty() {
            self.cache.activate(&self.cache.active.clone());
        }
    }
}

fn item_visible(item: &Item) -> bool {
    item.flags & ITEM_NO_DISPLAY == 0
        && item
            .effective
            .flags
            .value()
            .is_none_or(|flags| flags & UNAVAILABLE == 0)
}

fn check_removal(item: &Item) -> Result<(), String> {
    (item.flags & ITEM_NO_DROP == 0)
        .then_some(())
        .ok_or_else(|| "Original item cannot be removed from its owner".into())
}

fn check_item(item: &Item) -> Result<u32, String> {
    let flags = item
        .effective
        .flags
        .value()
        .ok_or("Unresolved item visibility flags")?;
    if !item_visible(item)
        || item.scripted
        || item
            .authored_slot
            .is_some_and(|slot| matches!(slot, 1000..=1008 | 2000..=2009))
    {
        return Err(
            "Item is unavailable or requires an unsupported scripted or equipment transaction"
                .into(),
        );
    }
    Ok(flags)
}

enum Move {
    Pickup(Option<usize>),
    Drop,
    Transfer(Owner, Owner, Option<usize>),
    Exchange(Owner, Owner, Option<usize>),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn original_key_codes() {
        for (lock, key, allowed) in [
            (0, 0, false),
            (0, 1, false),
            (1, 0, false),
            (1, 62, true),
            (62, 1, true),
            (3, 3, true),
            (3, 14, false),
        ] {
            assert_eq!(key_matches(lock, key), allowed);
        }
    }

    #[test]
    #[ignore = "requires the installed original game"]
    fn original_inventory_transactions_survive_eviction_and_map_changes() {
        let game = std::env::var_os("ARCANUM_GAME_DIR").expect("set ARCANUM_GAME_DIR");
        let open = || Vfs::from_game(&game, crate::vfs::Policy::Ce).unwrap();
        let mut data = open();
        let catalog = crate::content::maps::Catalog::load(&mut data).unwrap();
        let size = (936, 500);
        let make_world = |map: &str, tile| {
            let mut vfs = open();
            let map = catalog.find(map).unwrap();
            let region = map.files(&mut vfs).unwrap().bounds();
            let camera = region.camera(tile).unwrap();
            let mut stream = WorldStream::new(vfs, map.directory.clone(), region, tile)
                .unwrap()
                .with_player()
                .unwrap();
            stream.prepare(&camera, size).unwrap();
            let world = stream.into_interactive(|| {}, false).unwrap();
            (world, camera)
        };
        let tile = (80, 102);
        let (mut world, camera) = make_world("5011", tile);
        world.start_player().unwrap();
        let chest = "2:98fd05e54e0ed4118f1d00e02920220c";
        let contents = world.inspect_inventory(&mut data, chest).unwrap();
        assert_eq!(
            (contents["columns"].as_u64(), contents["rows"].as_u64()),
            (Some(10), Some(96))
        );
        let item = contents["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["footprint"][0].as_u64().unwrap() > 1 && item["slot"].is_number())
            .expect("original chest has a multi-cell item");
        let id = item["id"].as_str().unwrap();
        assert!(item["footprint_source"]["path"].is_string());
        let fail = world.transfer_item(&mut data, id, chest, "player", Some(9), None);
        assert!(fail.unwrap_err().contains("does not fit"));
        assert_eq!(world.inspect_item_owner(&mut data, id).unwrap(), *item);
        assert!(world.pickup_item(&mut data, id, None).is_err());
        world
            .transfer_item(&mut data, id, chest, "player", Some(2), None)
            .unwrap();
        assert!(
            world
                .transfer_item(&mut data, id, chest, "player", None, None)
                .is_err()
        );
        assert_eq!(
            world.inspect_inventory(&mut data, "player").unwrap()["rows"],
            12
        );
        let carried = world.inspect_item_owner(&mut data, id).unwrap();
        assert_eq!(carried["current"]["kind"], "player");

        // Persist base character data and the modified original item independently of caches.
        world.cache.inventory.character.base[0] = 13;
        let saved = serde_json::to_vec(&world.save_objects()).unwrap();
        let saved = serde_json::from_slice(&saved).unwrap();
        let map = catalog.find("5011").unwrap();
        let region = map.files(&mut data).unwrap().bounds();
        let mut stream = WorldStream::new(open(), map.directory.clone(), region, tile)
            .unwrap()
            .with_saved_player(DoorStates::default(), &saved, &catalog)
            .unwrap();
        stream.prepare(&camera, size).unwrap();
        let mut restored = stream.into_interactive(|| {}, false).unwrap();
        restored.start_player().unwrap();
        assert_eq!(restored.cache.inventory.character.base[0], 13);
        assert_eq!(restored.inspect_item_owner(&mut data, id).unwrap(), carried);

        // A faded wall lies between the camera and this authored room tile.
        let (mut room, _) = make_world("5011", (84, 104));
        room.inherit_object_state(&mut world);
        room.drop_item(&mut data, id).unwrap();
        let (&key, _) = room.cache.dropped_targets.first_key_value().unwrap();
        let raster = room
            .cache
            .ordered
            .iter()
            .find(|raster| raster.key == key)
            .unwrap();
        let sprite = raster.frame_draw(&raster.frame);
        let (x, y) = crate::compositor::sprite_origin(sprite.frame, sprite.anchor);
        let width = sprite.frame.width as usize;
        assert!(
            sprite
                .frame
                .pixels
                .iter()
                .enumerate()
                .any(|(offset, pixel)| {
                    let point = (x + (offset % width) as i64, y + (offset / width) as i64);
                    pixel[3] > 0
                        && room.cache.pick(point, room.reference, Duration::ZERO) == Some(key)
                })
        );
        room.pickup_item(&mut data, id, Some(2)).unwrap();
        world.inherit_object_state(&mut room);

        let (mut away, _) = make_world("5078", (2087, 2018));
        away.inherit_object_state(&mut world);
        assert_eq!(away.inspect_item_owner(&mut data, id).unwrap(), carried);
        world.inherit_object_state(&mut away);
        world.drop_item(&mut data, id).unwrap();
        let on_ground = world.inspect_item_owner(&mut data, id).unwrap();
        assert_eq!(on_ground["current"]["tile"], json!(tile));
        let rendered_count = |world: &InteractiveWorld| {
            let change = &world.cache.inventory.state.changes[&parse_id(id).unwrap()];
            world
                .cache
                .ordered
                .iter()
                .filter(|r| r.key.1.5 == usize::MAX - change.order)
                .count()
        };
        assert_eq!(rendered_count(&world), 1);
        let dropped = *world.cache.dropped_targets.keys().next().unwrap();
        assert!(matches!(
            world.cache.target(dropped).map(|target| &target.action),
            Some(interaction::Action::Pickup(picked)) if *picked == parse_id(id).unwrap()
        ));
        world.cache.activate(&[]);
        let ground_sector = tile_sector_id(tile);
        while world.cache.entries.contains_key(&ground_sector) {
            assert!(world.cache.evict(1, &[], false));
        }
        assert_eq!(world.inspect_item_owner(&mut data, id).unwrap(), on_ground);
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            world.receive();
            if world.prepare(&camera, size, (0, 0)).unwrap().ready {
                break;
            }
            assert!(Instant::now() < deadline, "sector reload timed out");
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(world.cache.entries.contains_key(&ground_sector));
        assert_eq!(rendered_count(&world), 1);

        away.inherit_object_state(&mut world);
        assert_eq!(away.inspect_item_owner(&mut data, id).unwrap(), on_ground);
        assert_eq!(rendered_count(&away), 0);
        assert!(away.pickup_item(&mut data, id, None).is_err());
        world.inherit_object_state(&mut away);
        assert_eq!(rendered_count(&world), 1);
        world.pickup_item(&mut data, id, Some(2)).unwrap();
        assert_eq!(rendered_count(&world), 0);
        world
            .transfer_item(
                &mut data,
                id,
                "player",
                chest,
                item["slot"].as_u64().map(|s| s as usize),
                None,
            )
            .unwrap();
        assert_eq!(world.inspect_inventory(&mut data, chest).unwrap(), contents);
    }
}
