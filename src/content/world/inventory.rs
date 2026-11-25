//! Session changes keyed by serialized identity. Sector records stay immutable.
use super::*;
use crate::content::character::{
    Adjustments, Character, CharacterStatistics, EquipmentSlot, Rules,
};
use crate::sector::objects::{self, FieldValue, ItemStats, OriginalId};
use mobile::population::{Effective, Resident};
use serde_json::{Value, json};
use std::{
    cell::{RefCell, RefMut},
    rc::Rc,
};

type Tile = (u32, u32);

#[derive(Clone, PartialEq, Eq, Debug)]
enum Place {
    Player,
    Container(OriginalId),
    Equipped(EquipmentSlot, [usize; 2]),
    World(String, Tile),
}
impl Place {
    fn report(&self) -> Value {
        match self {
            Self::Player => json!({"kind":"player"}),
            Self::Equipped(slot, _) => json!({"kind":"equipped","slot":slot}),
            Self::Container(id) => json!({"kind":"container","id":id.key()}),
            Self::World(map, tile) => json!({"kind":"world","map":map,"tile":tile}),
        }
    }
}

#[derive(Clone)]
struct Original {
    id: OriginalId,
    category: u32,
    prototype: u32,
    source: Arc<crate::content::Source>,
    place: Place,
    effective: Effective,
    description: Option<u32>,
    icon: Option<u32>,
    quantity: Option<u32>,
    slot: Option<i32>,
    flags: u32,
    item: Option<ItemStats>,
    scripted: bool,
}
impl Original {
    fn read(entry: &Resident, map: &str) -> Result<Self, String> {
        let proto = entry
            .prototype
            .as_ref()
            .ok_or("Original prototype unavailable")?;
        if entry.conflict {
            return Err("Conflicting original containment".into());
        }
        let object = entry.instance();
        let field = |slot| object.field(slot).and_then(FieldValue::scalar);
        let place = match (entry.placement, entry.parent, entry.tile) {
            ("contained", Some((parent, true)), _) => Place::Container(parent),
            ("world", None, Some(tile)) => Place::World(map.to_owned(), tile),
            _ => return Err("Unresolved original placement".into()),
        };
        Ok(Self {
            id: entry.id,
            category: entry.category,
            prototype: entry.prototype_id,
            source: Arc::new(entry.source.clone()),
            place,
            effective: Effective::new(&object, Some(proto)),
            description: field(22).or(proto.defaults.description),
            icon: field(70).or(proto.defaults.inventory_art),
            quantity: matches!(entry.category, 6 | 8)
                .then(|| field(97).or(proto.defaults.quantity))
                .flatten(),
            slot: field(71).map(|n| n as i32),
            flags: field(64)
                .or(proto.defaults.subtype_flags)
                .ok_or("Unresolved object subtype flags")?,
            item: (5..=14)
                .contains(&entry.category)
                .then(|| ItemStats::resolve(&object, proto.defaults.item.as_deref())),
            scripted: object.field(31).map_or(
                proto.defaults.transfer_script,
                objects::has_inventory_script,
            ),
        })
    }
    fn create(
        vfs: &mut Vfs,
        id: OriginalId,
        prototype: u32,
        category: u32,
        quantity: Option<u32>,
    ) -> Result<Self, String> {
        let prototype_id = crate::prototype::SerializedPrototypeId(prototype);
        let (source, bytes) = crate::content::sector::read_static_prototype(vfs, prototype_id)
            .map_err(|error| error.to_string())?;
        let defaults = objects::prototype_defaults(&bytes, prototype_id, category)
            .map_err(|error| format!("{source}: {error}"))?;
        let identity = objects::Instance {
            id,
            prototype: prototype_id,
            category,
            bitmap: [0; 5],
            fields: Vec::new(),
        };
        let proto = mobile::population::Prototype { source, defaults };
        let defaults = &proto.defaults;
        Ok(Self {
            id,
            category,
            prototype,
            source: Arc::new(proto.source.clone()),
            place: Place::Player,
            effective: Effective::new(&identity, Some(&proto)),
            description: defaults.description,
            icon: defaults.inventory_art,
            quantity: quantity.or(defaults.quantity),
            slot: None,
            flags: defaults
                .subtype_flags
                .ok_or("Unresolved created item flags")?,
            item: defaults.item.as_deref().cloned(),
            scripted: defaults.transfer_script,
        })
    }

    fn container_scripts_supported(&self) -> bool {
        let appearance = self.effective.appearance;
        !self.scripted && appearance.use_script == Some(0) && appearance.unlock_script == Some(0)
    }

    fn container_locked(&self, hour: u8) -> bool {
        self.flags & 0x80 == 0 && interaction::locked(self.flags, hour)
    }

    fn weight(&self) -> Option<i64> {
        if self.category == 8 {
            return Some(0);
        }
        let item = self.item.as_ref()?;
        let base = item.weight?;
        let adjustment = item.magic_weight?;
        if adjustment != 0 && item.complexity? != 0 {
            return None;
        }
        let quantity = if self.category == 6 {
            self.quantity? / 4
        } else {
            1
        };
        (i64::from(base) + i64::from(adjustment)).checked_mul(i64::from(quantity))
    }
}

#[derive(serde::Serialize)]
pub struct InventoryItem {
    pub id: String,
    pub name: Option<String>,
    description_id: Option<u32>,
    pub category: Option<&'static str>,
    pub prototype: u32,
    source: Value,
    original: Value,
    current: Value,
    pub quantity: Option<u32>,
    pub inventory_art: Option<u32>,
    pub icon: Option<String>,
    pub slot: Option<usize>,
    pub footprint: Option<[usize; 2]>,
    original_slot: Option<i32>,
    runtime_created: bool,
    item_statistics: Option<ItemStats>,
    pub weight: Option<i64>,
    pub paper_doll_icon: Option<String>,
    pub paper_doll_art: Option<u32>,
    item_or_container_flags: u32,
    transfer_script_attached: bool,
}
#[derive(serde::Serialize)]
pub struct InventoryView {
    pub open: bool,
    pub container: Option<String>,
    pub player: Vec<InventoryItem>,
    pub contents: Option<Vec<InventoryItem>>,
    pub container_name: Option<String>,
    pub equipment: Vec<EquippedItem>,
    pub statistics: CharacterStatistics,
}
#[derive(serde::Serialize)]
pub struct EquippedItem {
    pub slot: EquipmentSlot,
    pub item: InventoryItem,
}

/// CE item_inventory_slots_get and item_inventory_slots_has_room_for.
const COLUMNS: usize = 10;
#[derive(Default)]
struct Grid(BTreeMap<OriginalId, (usize, [usize; 2])>);
impl Grid {
    fn fit(
        &self,
        id: OriginalId,
        size: [usize; 2],
        rows: usize,
        requested: Option<usize>,
    ) -> Option<usize> {
        let [w, h] = size;
        if w == 0 || h == 0 || w > COLUMNS || h > rows {
            return None;
        }
        let free = |slot: usize| {
            let (x, y) = (slot % COLUMNS, slot / COLUMNS);
            x + w <= COLUMNS
                && y <= rows - h
                && self.0.iter().all(|(other, &(occupied, [ow, oh]))| {
                    let (ox, oy) = (occupied % COLUMNS, occupied / COLUMNS);
                    *other == id || x + w <= ox || ox + ow <= x || y + h <= oy || oy + oh <= y
                })
        };
        if let Some(slot) = requested {
            return free(slot).then_some(slot);
        }
        (0..rows * COLUMNS).find(|&slot| free(slot))
    }
}
struct Change {
    original: Original,
    current: Place,
    order: usize,
}
struct Resources {
    vfs: Rc<RefCell<Vfs>>,
    cache: FrameCache,
    tables: BTreeMap<&'static str, LoadedMes>,
}
impl Resources {
    fn text(&self, table: &str, key: Option<u32>) -> Option<&str> {
        self.tables.get(table)?.mes.get(key? as i32)
    }
}
#[derive(Default)]
pub(super) struct Session {
    changes: BTreeMap<OriginalId, Change>,
    resources: Option<Box<Resources>>,
    pub(super) map: String,
    pub(super) reference: Tile,
    pub(super) dropped_targets: BTreeMap<WorldOrder, interaction::Target>,
    player_grid: Grid,
    character: Character,
    rules: Rules,
    pub(super) movement_speed: Option<i32>,
    containers: BTreeMap<OriginalId, Grid>,
    unlocked_containers: BTreeSet<OriginalId>,
    open: Option<Option<OriginalId>>,
    revision: u64,
    next_created_item: u64,
}

impl Session {
    fn ground(&mut self, original: &Original, tile: Tile, order: usize) -> Result<Raster, String> {
        let r = self
            .resources
            .as_mut()
            .ok_or("Inventory resources unavailable")?;
        let frame = mobile::load_prop(
            &mut r.vfs.borrow_mut(),
            &mut r.cache,
            &mut r.tables,
            &original.effective,
            (original.category, tile),
            self.reference,
        )
        .map_err(|e| e.to_string())?;
        let key = super::super::scenery::order(tile, frame.offset, frame.flat, order);
        let mut raster = Raster::new((true, key), &frame.asset, frame.caster.anchor);
        if !within_overhang(
            raster.bounds,
            project_world_tile(tile, self.reference).ok_or("Invalid ground anchor")?,
        ) {
            return Err("Ground ART exceeds supported overhang".into());
        }
        raster.object_light = ObjectLight::Point;
        Ok(raster)
    }

    fn release_unmodified_grids(&mut self) {
        let open = self.open.flatten();
        self.containers.retain(|id, _| {
            Some(*id) == open
                || self.changes.values().any(|c| {
                    c.original.place == Place::Container(*id) || c.current == Place::Container(*id)
                })
        });
    }
    fn grid(&self, place: &Place) -> Option<&Grid> {
        match place {
            Place::Player => Some(&self.player_grid),
            Place::Container(id) => self.containers.get(id),
            Place::World(..) | Place::Equipped(..) => None,
        }
    }
    fn grid_mut(&mut self, place: &Place) -> Option<&mut Grid> {
        match place {
            Place::Player => Some(&mut self.player_grid),
            Place::Container(id) => self.containers.get_mut(id),
            Place::World(..) | Place::Equipped(..) => None,
        }
    }
    fn icon_path(&self, original: &Original) -> Option<String> {
        self.item_art_path(original.icon?, "art/item/item_inven.mes")
    }
    fn item_art_path(&self, art: u32, table: &str) -> Option<String> {
        if art >> 28 != 6 {
            return None;
        }
        let name = self
            .resources
            .as_ref()?
            .text(table, Some(mobile::item_art_entry(art) as u32))?;
        let name = crate::resource_path::normalize(name.trim()).ok()?;
        (!name.contains('/')).then(|| format!("art/item/{name}"))
    }
    fn footprint(&mut self, obj: &Original) -> Result<[usize; 2], String> {
        let path = self
            .icon_path(obj)
            .ok_or_else(|| format!("Unresolved inventory ART for {}", obj.id.key()))?;
        let r = self
            .resources
            .as_mut()
            .ok_or("Inventory resources unavailable")?;
        let frame = r
            .cache
            .load(
                &mut r.vfs.borrow_mut(),
                &path,
                0,
                ((obj.icon.unwrap() >> 4) & 3) as u8,
                false,
            )
            .map_err(|e| e.to_string())?;
        let f = &frame.decoded.frame;
        Ok([
            f.width.div_ceil(32) as usize,
            f.height.div_ceil(32) as usize,
        ])
    }
    fn row(&self, original: &Original, place: &Place) -> InventoryItem {
        let resources = self.resources.as_deref();
        let name = original.description.and_then(|id| {
            resources?.text(
                if id < 30000 {
                    "mes/description.mes"
                } else {
                    "mes/gamedesc.mes"
                },
                Some(id),
            )
        });
        let icon = self.icon_path(original);
        let square = self.grid(place).and_then(|g| g.0.get(&original.id));
        let paper_doll_art = original.item.as_ref().and_then(|item| item.paper_doll_art);
        InventoryItem {
            weight: original.weight(),
            item_statistics: original.item.clone(),
            paper_doll_art,
            paper_doll_icon: paper_doll_art
                .and_then(|art| self.item_art_path(art, "art/item/item_paper.mes")),
            id: original.id.key(),
            name: name.map(str::to_owned),
            description_id: original.description,
            category: objects::category_name(original.category),
            prototype: original.prototype,
            source: super::super::diagnostics::source(&original.source),
            original: original.place.report(),
            current: place.report(),
            original_slot: original.slot,
            runtime_created: original.id.is_runtime(),
            item_or_container_flags: original.flags,
            transfer_script_attached: original.scripted,
            slot: square.map(|s| s.0),
            footprint: square.map(|s| s.1),
            quantity: original.quantity,
            inventory_art: original.icon,
            icon,
        }
    }
    fn create_carried_item(
        &mut self,
        prototype: u32,
        category: u32,
        quantity: Option<u32>,
    ) -> Result<OriginalId, String> {
        let serial = self
            .next_created_item
            .checked_add(1)
            .ok_or("Runtime item IDs exhausted")?;
        let id = OriginalId::runtime(serial);
        let obj = {
            let resources = self
                .resources
                .as_ref()
                .ok_or("Inventory resources unavailable")?;
            Original::create(
                &mut resources.vfs.borrow_mut(),
                id,
                prototype,
                category,
                quantity,
            )?
        };
        let shape = self.footprint(&obj)?;
        let slot = self
            .player_grid
            .fit(id, shape, 12, None)
            .ok_or("Created item does not fit in the player inventory")?;
        self.player_grid.0.insert(id, (slot, shape));
        self.changes.insert(
            id,
            Change {
                original: obj,
                current: Place::Player,
                order: usize::MAX - self.changes.len() - 1,
            },
        );
        self.next_created_item = serial;
        self.revision += 1;
        self.update_statistics();
        Ok(id)
    }

    fn prepare_appearance(
        &mut self,
        player: &Player,
        incoming: Option<OriginalId>,
    ) -> Result<Option<Arc<crate::content::player::Appearance>>, String> {
        let fragment = incoming
            .map(|id| {
                let change = self
                    .changes
                    .get(&id)
                    .ok_or("Equipment must come from the player inventory")?;
                if change.current != Place::Player {
                    return Err("Item does not belong to the player inventory".to_owned());
                }
                self.check_equipment(&change.original, EquipmentSlot::Armor)?;
                change
                    .original
                    .item
                    .as_ref()
                    .and_then(|item| item.use_art)
                    .ok_or_else(|| "Unresolved armor use ART fragment".into())
            })
            .transpose()?;
        let art = self.character.body_art(fragment)?;
        if player.appearance.art == art {
            return Ok(None);
        }
        let resources = self
            .resources
            .as_mut()
            .ok_or("Inventory resources unavailable")?;
        let result = crate::content::player::Appearance::load(
            &mut resources.vfs.borrow_mut(),
            &mut resources.cache,
            art,
        );
        resources.cache.finish_sector();
        result
            .map(|appearance| Some(Arc::new(appearance)))
            .map_err(|error| format!("Player cannot use armor ART {art:#010x}: {error}"))
    }

    fn equipped(&self, slot: EquipmentSlot) -> Option<&Change> {
        self.changes
            .values()
            .find(|entry| matches!(entry.current, Place::Equipped(current, _) if current == slot))
    }

    fn carried_weight(&self) -> Option<i64> {
        self.changes
            .values()
            .filter(|entry| matches!(entry.current, Place::Player | Place::Equipped(..)))
            .try_fold(0i64, |sum, entry| sum.checked_add(entry.original.weight()?))
    }

    fn statistics(&self) -> CharacterStatistics {
        let mut armor = 0i64;
        let mut resistance = [0i64; 5];
        let mut incomplete = false;
        for entry in self
            .changes
            .values()
            .filter(|c| matches!(c.current, Place::Equipped(..)))
        {
            let Some(item) = &entry.original.item else {
                continue;
            };
            incomplete |= item.spells.iter().flatten().any(|&spell| spell != 10_000)
                || entry.original.flags & 0x1e08 != 0;
            if entry.original.category != 7 {
                continue;
            }
            armor += i64::from(item.armor_class.unwrap_or_default());
            for (total, value) in resistance
                .iter_mut()
                .zip(item.resistance.unwrap_or_default())
            {
                *total += i64::from(value.unwrap_or_default());
            }
            if item.complexity == Some(0) {
                armor += i64::from(item.magic_armor_class.unwrap_or_default());
                for (total, value) in resistance
                    .iter_mut()
                    .zip(item.magic_resistance.unwrap_or_default())
                {
                    *total += i64::from(value.unwrap_or_default());
                }
            } else {
                incomplete |= item.magic_armor_class.is_some_and(|v| v != 0)
                    || item
                        .magic_resistance
                        .into_iter()
                        .flatten()
                        .flatten()
                        .any(|v| v != 0);
            }
        }
        self.character.statistics(
            &self.rules,
            self.carried_weight(),
            armor,
            resistance,
            incomplete,
        )
    }

    fn update_statistics(&mut self) {
        self.movement_speed = self.statistics().speed;
    }

    fn move_equipment(
        &mut self,
        slot: EquipmentSlot,
        incoming: Option<OriginalId>,
        requested: Option<usize>,
    ) -> Result<Value, String> {
        let worn = self.equipped(slot);
        let incoming = incoming
            .map(|id| {
                let &(_, shape) = self
                    .player_grid
                    .0
                    .get(&id)
                    .ok_or("Equipment must come from the player inventory")?;
                let change = self.changes.get(&id).ok_or("Unresolved item state")?;
                if change.current != Place::Player {
                    return Err("Item does not belong to the player inventory".to_owned());
                }
                self.check_equipment(&change.original, slot)?;
                Ok((id, shape))
            })
            .transpose()?;
        if incoming.is_none() && worn.is_none() {
            return Err("Equipment slot is empty".into());
        }
        let displaced = if let Some(entry) = worn {
            let item = &entry.original;
            if item.item.as_ref().is_some_and(|stats| stats.wield_script) {
                return Err("Removing this item requires an unsupported wield script".into());
            }
            if item.flags & 0x20 != 0 {
                return Err("This item cannot be removed once equipped".into());
            }
            let Place::Equipped(_, shape) = entry.current else {
                unreachable!()
            };
            // Ignore the incoming item's footprint while validating the replacement.
            let free = self
                .player_grid
                .fit(incoming.map_or(item.id, |(id, _)| id), shape, 12, requested)
                .ok_or("No room for the unequipped item in the player inventory")?;
            Some((item.id, (free, shape)))
        } else {
            None
        };
        if let Some((id, shape)) = incoming {
            self.player_grid.0.remove(&id);
            self.changes.get_mut(&id).expect("validated item").current =
                Place::Equipped(slot, shape);
        }
        if let Some((id, square)) = displaced {
            self.player_grid.0.insert(id, square);
            self.changes
                .get_mut(&id)
                .expect("validated equipment")
                .current = Place::Player;
        }
        self.revision += 1;
        let statistics = self.statistics();
        self.movement_speed = statistics.speed;
        Ok(json!({
            "slot": slot, "equipped": incoming.map(|(id, _)| id.key()),
            "unequipped": displaced.map(|(id, _)| id.key()),
            "inventory_slot": displaced.map(|(_, (cell, _))| cell),
            "statistics": statistics
        }))
    }

    fn check_equipment(&self, item: &Original, slot: EquipmentSlot) -> Result<(), String> {
        let stats = item
            .item
            .as_ref()
            .ok_or("Unresolved equipment properties")?;
        let flags = stats.equipment_flags.unwrap_or_default();
        if !slot.accepts(item.category, item.icon, flags) {
            return Err(format!(
                "Item is incompatible with the {} slot",
                slot.name()
            ));
        }
        if stats.wield_script {
            return Err("Equipping this item requires an unsupported wield script".into());
        }
        let art = item
            .effective
            .current_art
            .value()
            .ok_or("Unresolved current ART")?;
        if art >> 28 == 6 && art & 0x400 != 0 {
            return Err("Broken items cannot be equipped".into());
        }
        if slot == EquipmentSlot::Armor {
            if self
                .character
                .armor_size()
                .is_none_or(|size| flags & size == 0)
            {
                return Err("Body armor does not fit this character's race".into());
            }
            let gender = self.character.gender();
            if (flags & 8 != 0 && gender != 1) || (flags & 8 == 0 && flags & 16 != 0 && gender != 0)
            {
                return Err("Body armor does not fit this character's gender".into());
            }
        }
        let fixed_two_handed = |f: u32| f & 0xc == 0xc;
        if (slot == EquipmentSlot::Weapon
            && fixed_two_handed(flags)
            && self.equipped(EquipmentSlot::Shield).is_some())
            || (slot == EquipmentSlot::Shield
                && self
                    .equipped(EquipmentSlot::Weapon)
                    .and_then(|c| c.original.item.as_ref()?.equipment_flags)
                    .is_some_and(fixed_two_handed))
        {
            return Err("A fixed two-handed weapon requires both hands".into());
        }
        Ok(())
    }
}

impl SectorCache {
    fn move_player_equipment(
        &mut self,
        slot: EquipmentSlot,
        incoming: Option<OriginalId>,
        cell: Option<usize>,
    ) -> Result<Value, String> {
        let player = self.player.as_mut().ok_or("Player unavailable")?;
        let appearance = if slot == EquipmentSlot::Armor {
            self.inventory.prepare_appearance(player, incoming)?
        } else {
            None
        };
        let mut outcome = self.inventory.move_equipment(slot, incoming, cell)?;
        if let Some(appearance) = appearance {
            player.appearance = appearance;
        }
        outcome["appearance"] = player.appearance_report();
        Ok(outcome)
    }

    fn resident(&self, id: OriginalId) -> Option<&Resident> {
        self.entries
            .values()
            .find_map(|(s, _)| s.mobiles.iter().find(|r| r.id == id))
    }
    fn original(&self, id: OriginalId) -> Result<Original, String> {
        if let Some(change) = self.inventory.changes.get(&id) {
            return Ok(change.original.clone());
        }
        Original::read(
            self.resident(id)
                .ok_or("Original object unavailable in cached sectors")?,
            &self.inventory.map,
        )
    }
    fn current(&self, obj: &Original) -> Place {
        self.inventory
            .changes
            .get(&obj.id)
            .map_or_else(|| obj.place.clone(), |change| change.current.clone())
    }
    fn near_player(&self, tile: Tile) -> bool {
        let Some(player) = self.player.as_ref().filter(|p| !p.moving()) else {
            return false;
        };
        self.in_inventory_range(player.tile(), tile)
    }
    pub(super) fn in_inventory_range(&self, at: Tile, tile: Tile) -> bool {
        if at == tile {
            return true;
        }
        [1u8, 3, 5, 7].into_iter().any(|dir| {
            let (dx, dy) = collision::DIRECTIONS[dir as usize];
            at.0.checked_add_signed(dx) == Some(tile.0)
                && at.1.checked_add_signed(dy) == Some(tile.1)
                && Self::player_cell(&self.entries, &self.door_states, at)
                    .is_some_and(|c| !c.blocks_edge(dir))
                && Self::player_cell(&self.entries, &self.door_states, tile)
                    .is_some_and(|c| !c.blocks_edge((dir + 4) % 8))
        })
    }
    fn accessible(&self, id: OriginalId) -> Result<(), String> {
        let obj = self.original(id)?;
        let Place::World(_, tile) = obj.place else {
            return Err("Contained containers are unsupported".into());
        };
        if obj.category != 2
            || !self.near_player(tile)
            || self.resident(id).is_none_or(|r| r.raster.is_none())
        {
            return Err("Approach a visible original container".into());
        }
        let app = obj.effective.appearance;
        if (obj.flags & 0x80 == 0 && obj.flags & 6 != 0)
            || (obj.container_locked(self.hour)
                && !self.inventory.unlocked_containers.contains(&id))
        {
            return Err(format!(
                "Container locked, jammed or magically held (flags {:#x})",
                obj.flags
            ));
        }
        if obj
            .effective
            .flags
            .value()
            .is_none_or(|f| f & 0x0380_0b03 != 0)
        {
            return Err("Container visibility, trap or selection flags prevent opening".into());
        }
        if !obj.container_scripts_supported() {
            return Err(format!(
                "Unsupported container scripts (use {:?}, unlock {:?}, transfer {})",
                app.use_script, app.unlock_script, obj.scripted
            ));
        }
        Ok(())
    }

    pub(super) fn unlock_container(&mut self, id: OriginalId) -> Result<bool, String> {
        let obj = self.original(id)?;
        if obj.category != 2 {
            return Err("Selected object is neither a door nor a container".into());
        }
        if obj.flags & 6 != 0 || !obj.container_scripts_supported() {
            return Err("Container has a scripted, jammed or magical restriction".into());
        }
        // Override the ordinary lock for the session, including time-gated
        // locks when their schedule currently permits opening.
        Ok(obj.flags & 1 != 0
            && obj.flags & (8 | 0x80) == 0
            && self.inventory.unlocked_containers.insert(id))
    }

    pub(super) fn container_lock_status(&self, id: OriginalId) -> Option<Value> {
        let obj = self.original(id).ok()?;
        if obj.category != 2 {
            return None;
        }
        let unlocked = self.inventory.unlocked_containers.contains(&id);
        Some(json!({
            "kind": "container", "id": id.key(), "flags":obj.flags,
            "locked": obj.container_locked(self.hour) && !unlocked,
            "developer_unlock": unlocked,
        }))
    }

    fn prepare_container(&mut self, owner: OriginalId) -> Result<(), String> {
        self.accessible(owner)?;
        if self.inventory.containers.contains_key(&owner) {
            return Ok(());
        }
        self.inventory.release_unmodified_grids();
        let root = self.resident(owner).ok_or("Container unavailable")?;
        let tile = root.tile.ok_or("Container tile unresolved")?;
        let sector = &self.entries[&tile_sector_id(tile)].0;
        let by_id: BTreeMap<_, _> = sector
            .mobiles
            .iter()
            .filter(|r| r.parent == Some((owner, true)))
            .map(|r| (r.id, r))
            .collect();
        if by_id.len() != root.inventory.len() {
            return Err("Unresolved or inconsistent original container membership".into());
        }
        let mut sizes = Vec::new();
        for (id, resolved) in &root.inventory {
            let entry = by_id
                .get(id)
                .filter(|_| *resolved)
                .ok_or_else(|| format!("Unresolved container member {}", id.key()))?;
            let obj = Original::read(entry, &self.inventory.map)?;
            if obj.flags & 0x10 == 0 && obj.effective.flags.value().is_some_and(|f| f & 0x2 == 0) {
                sizes.push(obj);
            }
        }
        let mut icons = Vec::with_capacity(sizes.len());
        for obj in sizes {
            icons.push((obj.id, self.inventory.footprint(&obj)?));
        }
        // CE opens loot horizontally arranged, largest icon height then width first.
        icons.sort_by_key(|(_, [w, h])| std::cmp::Reverse((*h, *w)));
        let mut grid = Grid::default();
        for (id, shape) in icons {
            let slot = grid
                .fit(id, shape, 96, None)
                .ok_or("Original items exceed the loot grid")?;
            grid.0.insert(id, (slot, shape));
        }
        self.inventory.containers.insert(owner, grid);
        Ok(())
    }

    fn transfer(
        &mut self,
        id: OriginalId,
        operation: &str,
        owner: Option<OriginalId>,
        requested: Option<usize>,
    ) -> Result<Value, String> {
        let obj = self.original(id)?;
        if !(5..=14).contains(&obj.category)
            || obj.scripted
            || obj.flags & 0x10 != 0
            || obj
                .effective
                .flags
                .value()
                .is_none_or(|flags| flags & 0x0380_0303 != 0)
        {
            return Err("Only visible whole items with supported transfers can be moved".into());
        }
        let from = self.current(&obj);
        if let Some(owner) = owner {
            self.prepare_container(owner)?;
        }
        if !matches!(from, Place::World(..))
            && !self
                .inventory
                .grid(&from)
                .is_some_and(|g| g.0.contains_key(&id))
        {
            return Err("Unresolved inventory source or inaccessible owner".into());
        }
        if matches!(from, Place::World(..))
            && obj.effective.flags.value().is_some_and(|f| f & 0x800 != 0)
        {
            return Err("Original item cannot be selected in the world".into());
        }
        let dest = match (operation, &from, owner) {
            ("take", Place::Container(actual), Some(owner)) if *actual == owner => Place::Player,
            ("take", Place::World(map, tile), None)
                if *map == self.inventory.map && self.near_player(*tile) =>
            {
                let visible = if self.inventory.changes.contains_key(&id) {
                    self.inventory
                        .dropped_targets
                        .values()
                        .any(|t| matches!(t.action,interaction::Action::Inventory(i) if i==id))
                } else {
                    self.resident(id).is_some_and(|r| r.raster.is_some())
                };
                if !visible {
                    return Err("Ground item has no prepared appearance".into());
                }
                Place::Player
            }
            ("position", Place::Player, None) if requested.is_some() => from.clone(),
            ("position", Place::Container(actual), Some(id))
                if *actual == id && requested.is_some() =>
            {
                from.clone()
            }
            ("put", Place::Player, Some(owner)) => Place::Container(owner),
            ("drop", Place::Player, None) => {
                let player = self.player.as_ref().ok_or("Player unavailable")?;
                if player.moving() {
                    return Err("Stop moving before dropping an item".into());
                }
                let tile = player.tile();
                if !collision::can_stand(Self::player_cell(&self.entries, &self.door_states, tile))
                {
                    return Err("No passable ground at the player's position".into());
                }
                Place::World(self.inventory.map.clone(), tile)
            }
            _ => {
                return Err(
                    "Item is outside reach or does not belong to the specified source".into(),
                );
            }
        };
        if dest != from && !matches!(from, Place::World(..)) && obj.flags & 0x20 != 0 {
            return Err("Item cannot be dropped or given away".into());
        }
        if dest == Place::Player && from != Place::Player {
            let weight = self
                .inventory
                .carried_weight()
                .zip(obj.weight())
                .and_then(|(current, item)| current.checked_add(item))
                .ok_or("Unable to verify carry capacity for this item's effective weight")?;
            if weight > i64::from(self.inventory.statistics().carry_capacity) {
                return Err("Item exceeds the character's carry capacity".into());
            }
        }
        let order = self
            .inventory
            .changes
            .get(&id)
            .map_or(usize::MAX - self.inventory.changes.len() - 1, |c| c.order);
        // Validate world appearance and placement before changing either membership.
        let ground = if let Place::World(_, tile) = dest {
            Some(self.inventory.ground(&obj, tile, order)?)
        } else {
            None
        };
        let player_changed = from == Place::Player || dest == Place::Player;
        let result = json!({"id":id.key(),"from":from.report(),"to":dest.report()});
        let square = if matches!(dest, Place::World(..)) {
            None
        } else {
            let shape = self.inventory.footprint(&obj)?;
            let rows = if dest == Place::Player { 12 } else { 96 };
            let slot = self
                .inventory
                .grid(&dest)
                .and_then(|g| g.fit(id, shape, rows, requested))
                .ok_or("Item does not fit in the destination slots")?;
            Some((slot, shape))
        };
        if let Some(g) = self.inventory.grid_mut(&from) {
            g.0.remove(&id);
        }
        if let (Some(g), Some(square)) = (self.inventory.grid_mut(&dest), square) {
            g.0.insert(id, square);
        }
        self.inventory.revision += 1;
        self.inventory.changes.insert(
            id,
            Change {
                original: obj,
                current: dest,
                order,
            },
        );
        if player_changed {
            self.inventory.update_statistics();
        }
        if matches!(from, Place::World(..)) || ground.is_some() {
            if let Some(index) = self
                .ordered
                .iter()
                .position(|r| self.item_at(r.key) == Some(id))
            {
                self.inventory
                    .dropped_targets
                    .remove(&self.ordered[index].key);
                self.ordered.remove(index);
                if self.ambient.is_some() {
                    self.shades.remove(index);
                }
            }
            if let Some(raster) = ground {
                let key = raster.key;
                self.register_drop(id, &raster);
                let i = self.ordered.partition_point(|r| r.key < key);
                if let Some((ambient, _)) = self.ambient {
                    let base = if self.outdoor((key.1.2, key.1.1)) {
                        ambient.outdoor
                    } else {
                        ambient.indoor
                    };
                    let tint = lighting::point(self.light_emitters(), raster.anchor, base);
                    self.shades.insert(i, PreparedShade::uniform(tint));
                }
                self.ordered.insert(i, raster);
            }
            self.reindex_animations();
        }
        Ok(result)
    }

    fn item_at(&self, key: WorldOrder) -> Option<OriginalId> {
        if key.1.5 < mobile_draw_slot(0) {
            return None;
        }
        match self.target(key)?.action {
            interaction::Action::Inventory(id) => Some(id),
            _ => None,
        }
    }
    fn register_drop(&mut self, id: OriginalId, raster: &Raster) {
        self.inventory.dropped_targets.insert(
            raster.key,
            interaction::Target {
                tile: (raster.key.1.2, raster.key.1.1),
                action: interaction::Action::Inventory(id),
            },
        );
    }
    pub(super) fn project_inventory(&mut self) {
        self.inventory.dropped_targets.clear();
        if self.inventory.changes.is_empty() {
            return;
        }
        let entries = &self.entries;
        let changes = &self.inventory.changes;
        self.ordered.retain(|r| {
            if r.key.1.5 < mobile_draw_slot(0) {
                return true;
            }
            let tile = (r.key.1.2, r.key.1.1);
            let target = entries
                .get(&tile_sector_id(tile))
                .and_then(|(sector, _)| sector.interactions.get(&r.key));
            match target.map(|t| &t.action) {
                Some(interaction::Action::Inventory(id)) => !changes.contains_key(id),
                _ => true,
            }
        });
        let active: Vec<_> = self
            .inventory
            .changes
            .values()
            .filter_map(|c| {
                let Place::World(map, tile) = &c.current else {
                    return None;
                };
                (*map == self.inventory.map && self.active.contains(&tile_sector_id(*tile)))
                    .then(|| (c.original.clone(), *tile, c.order))
            })
            .collect();
        for (obj, tile, order) in active {
            match self.inventory.ground(&obj, tile, order) {
                Ok(raster) => {
                    self.register_drop(obj.id, &raster);
                    self.ordered.push(raster);
                }
                Err(e) => eprintln!("Dropped item {}: {e}", obj.id.key()),
            }
        }
    }

    pub(super) fn inventory_changed(&self) -> bool {
        !self.inventory.changes.is_empty()
    }
    pub(super) fn current_members(&self, entry: &Resident) -> Vec<Value> {
        let mut members: BTreeMap<_, _> = entry
            .inventory
            .iter()
            .copied()
            .filter(|(id, _)| !self.modified(*id))
            .collect();
        for (id, c) in &self.inventory.changes {
            if c.current == Place::Container(entry.id) {
                members.insert(*id, true);
            }
        }
        members
            .into_iter()
            .map(|(id, resolved)| json!({"original_id":id.key(),"resolved":resolved}))
            .collect()
    }
    pub(super) fn modified(&self, id: OriginalId) -> bool {
        self.inventory.changes.contains_key(&id)
    }

    pub(super) fn changed_reports<'a>(
        &'a self,
        filter: &'a crate::developer::ObjectFilter,
    ) -> impl Iterator<Item = Value> + 'a {
        self.inventory.changes.values().filter_map(move |change| {
            let obj = &change.original;
            let tile = match &change.current {
                Place::World(map, tile) if *map == self.inventory.map => Some(*tile),
                _ => None,
            };
            let category = objects::category_name(obj.category);
            if filter.record.is_some()
                || filter
                    .original
                    .as_ref()
                    .is_some_and(|id| *id != obj.id.key())
                || filter
                    .category
                    .as_deref()
                    .is_some_and(|c| c != "mobile" && Some(c) != category)
                || filter.prototype.is_some_and(|p| p != obj.prototype)
                || filter.tile.is_some_and(|(x, y)| {
                    tile.is_none_or(|(a, b)| a.abs_diff(x).max(b.abs_diff(y)) > filter.radius)
                })
            {
                return None;
            }
            let mut value = json!(self.inventory.row(obj, &change.current));
            value["tile"] = json!(tile);
            value["original_id"] = json!(obj.id.key());
            let in_world = matches!(change.current, Place::World(..));
            value["placement"] = json!(if in_world { "world" } else { "contained" });
            let prepared = self
                .inventory
                .dropped_targets
                .values()
                .any(|t| matches!(t.action,interaction::Action::Inventory(id) if id==obj.id));
            let state = if prepared {
                "prepared"
            } else if in_world {
                "outside_active_sectors"
            } else {
                "contained"
            };
            value["rendering"] = json!({"state":state});
            value["collision_registered"] = json!(false);
            Some(value)
        })
    }
    fn contents(&self, place: &Place) -> Vec<InventoryItem> {
        let mut rows = BTreeMap::new();
        if let Place::Container(owner) = place
            && let Some(root) = self.resident(*owner)
            && let Some(tile) = root.tile
            && let Some((sector, _)) = self.entries.get(&tile_sector_id(tile))
        {
            for entry in &sector.mobiles {
                if entry.parent == Some((*owner, true))
                    && !self.inventory.changes.contains_key(&entry.id)
                    && let Ok(obj) = Original::read(entry, &self.inventory.map)
                    && self
                        .inventory
                        .grid(place)
                        .is_some_and(|g| g.0.contains_key(&obj.id))
                {
                    rows.insert(obj.id, self.inventory.row(&obj, place));
                }
            }
        }
        for (&id, c) in &self.inventory.changes {
            if &c.current == place {
                rows.insert(id, self.inventory.row(&c.original, place));
            }
        }
        rows.into_values().collect()
    }

    pub(super) fn use_inventory_object(&mut self, id: OriginalId) -> Result<(), String> {
        if self.original(id)?.category == 2 {
            self.prepare_container(id)?;
            self.inventory.revision += 1;
            self.inventory.open = Some(Some(id));
            Ok(())
        } else {
            self.transfer(id, "take", None, None).map(|_| ())
        }
    }
}

impl WorldStream {
    pub fn with_inventory(mut self, vfs: Rc<RefCell<Vfs>>) -> Self {
        let mut tables = BTreeMap::new();
        for path in [
            "mes/description.mes",
            "mes/gamedesc.mes",
            "art/item/item_inven.mes",
            "art/item/item_paper.mes",
            "rules/effect.mes",
            "rules/backgrnd.mes",
        ] {
            match load_mes(&mut vfs.borrow_mut(), path) {
                Ok(m) => {
                    tables.insert(path, m);
                }
                Err(e) => eprintln!("Inventory resource {path}: {e}"),
            }
        }
        let inventory = &mut self.cache.inventory;
        // CE proto.c creates a generic PC prototype when the installed game does
        // not ship one. Use the matching baseline already used by the alpha.
        if let Ok((_, bytes)) = crate::content::sector::read_static_prototype(
            &mut vfs.borrow_mut(),
            crate::prototype::SerializedPrototypeId(16086),
        ) {
            match objects::prototype_defaults(
                &bytes,
                crate::prototype::SerializedPrototypeId(16086),
                15,
            ) {
                Ok(proto) => {
                    if let Some(stats) = proto.character {
                        for (base, value) in inventory.character.base.iter_mut().zip(*stats) {
                            if let Some(value) = value {
                                *base = value;
                            }
                        }
                    }
                    inventory.character.original_palette =
                        proto.original_character_art.map_or(0, |art| (art >> 4) & 3);
                    inventory.character.base_ac = proto.armor_class.unwrap_or_default();
                    inventory.character.base_resistance = proto
                        .resistance
                        .unwrap_or_default()
                        .map(Option::unwrap_or_default);
                }
                Err(error) => eprintln!("Invalid PC prototype: {error}"),
            }
        }
        let effect = |id| {
            let value = tables.get("rules/effect.mes")?.mes.get(id)?;
            Adjustments::parse(value)
                .map_err(|e| eprintln!("Character effect {id}: {e}"))
                .ok()
        };
        inventory.rules = Rules {
            race: inventory.character.race_effect().and_then(effect),
            gender: (inventory.character.gender() == 0)
                .then(|| effect(330))
                .flatten(),
            encumbrance: effect(158),
        };
        inventory.update_statistics();
        self.cache.inventory.resources = Some(Box::new(Resources {
            vfs,
            cache: FrameCache::default(),
            tables,
        }));
        self
    }
    /// New-game baseline for the generic PC, once per play session.
    pub fn with_starting_inventory(mut self) -> Result<Self, String> {
        let session = &mut self.cache.inventory;
        if session.next_created_item != 0 {
            return Ok(self);
        }
        // CE background_generate_inventory uses background zero (none), block
        // offset 3 for money. The no-background item list is empty.
        let gold = session
            .resources
            .as_ref()
            .and_then(|r| r.text("rules/backgrnd.mes", Some(3)))
            .and_then(|value| value.trim().parse::<u32>().ok())
            .ok_or("Missing or invalid default background money (rules/backgrnd.mes, entry 3)")?;
        let size = session
            .character
            .armor_size()
            .ok_or("Unsupported starting armor size")?;
        // CE item_generate_inventory: descriptions selected by race size and gender.
        // sub_468860 converts an original description number to its wire ID (+20).
        let description = match (session.character.gender(), size) {
            (0, 1) => 8134, // Small Ladies City Dweller Clothes
            (0, 2) => 8133, // Plain Dress
            (0, 4) => 8135,
            (1, 1) => 8065, // Small Nice Suit
            (1, 2) => 8049, // Nice Suit
            (1, 4) => 8069,
            _ => return Err("Unsupported starting character gender or size".into()),
        };
        if gold != 0 {
            session.create_carried_item(9056 + 20, 8, Some(gold))?;
        }
        let clothing = session.create_carried_item(description + 20, 7, None)?;
        if let Err(suit_error) =
            self.cache
                .move_player_equipment(EquipmentSlot::Armor, Some(clothing), None)
        {
            // CE removes an incompatible starter suit and tries the proper jacket size.
            let session = &mut self.cache.inventory;
            session.player_grid.0.remove(&clothing);
            session.changes.remove(&clothing);
            let jacket = match size {
                1 => 8118,
                2 => 8110,
                4 => 8126,
                _ => unreachable!("starting armor size already checked"),
            };
            let fallback = session.create_carried_item(jacket + 20, 7, None)?;
            self.cache
                .move_player_equipment(EquipmentSlot::Armor, Some(fallback), None)
                .map_err(|error| {
                    format!("Starting outfit unavailable ({suit_error}); fallback jacket: {error}")
                })?;
        }
        Ok(self)
    }
}
impl InteractiveWorld {
    pub fn inherit_inventory(&mut self, old: &mut Self) {
        self.cache.inventory = std::mem::take(&mut old.cache.inventory);
        if let (Some(player), Some(previous)) = (&mut self.cache.player, &old.cache.player) {
            player.appearance = Arc::clone(&previous.appearance);
        }
        self.cache.inventory.map = self.map.clone();
        self.cache.inventory.reference = self.reference;
        self.cache.inventory.open = None;
        if !self.cache.inventory.changes.is_empty() {
            let active = self.cache.active.clone();
            self.cache.activate(&active);
        }
    }

    pub fn inventory_revision(&self) -> u64 {
        self.cache.inventory.revision
    }
    pub fn inventory_vfs(&self) -> Option<RefMut<'_, Vfs>> {
        Some(self.cache.inventory.resources.as_ref()?.vfs.borrow_mut())
    }
    pub fn inventory_is_open(&self) -> bool {
        self.cache.inventory.open.is_some()
    }
    pub fn close_inventory(&mut self) {
        self.cache.inventory.open = None;
        self.cache.inventory.release_unmodified_grids();
        self.cache.inventory.revision += 1;
    }
    pub fn inventory_open(&mut self, owner: Option<&str>) -> Result<(), String> {
        let owner = owner.map(parse_id).transpose()?;
        if let Some(id) = owner {
            self.cache.prepare_container(id)?;
        }
        self.cancel_player_route();
        self.cache.inventory.revision += 1;
        self.cache.inventory.open = Some(owner);
        Ok(())
    }
    pub fn inventory_view(&self) -> InventoryView {
        let container = self.cache.inventory.open.flatten();
        InventoryView {
            open: self.inventory_is_open(),
            container: container.map(OriginalId::key),
            container_name: container
                .and_then(|id| self.cache.original(id).ok())
                .and_then(|c| self.cache.inventory.row(&c, &c.place).name),
            player: self.cache.contents(&Place::Player),
            equipment: EquipmentSlot::ALL
                .into_iter()
                .filter_map(|slot| {
                    let equipped = self.cache.inventory.equipped(slot)?;
                    Some(EquippedItem {
                        slot,
                        item: self
                            .cache
                            .inventory
                            .row(&equipped.original, &equipped.current),
                    })
                })
                .collect(),
            statistics: self.character_statistics(),
            contents: container.map(|id| self.cache.contents(&Place::Container(id))),
        }
    }
    pub fn character_statistics(&self) -> CharacterStatistics {
        self.cache.inventory.statistics()
    }
    pub fn equip_inventory_item(
        &mut self,
        key: &str,
        slot: EquipmentSlot,
    ) -> Result<Value, String> {
        self.cache
            .move_player_equipment(slot, Some(parse_id(key)?), None)
    }
    pub fn unequip_inventory_item(
        &mut self,
        slot: EquipmentSlot,
        cell: Option<usize>,
    ) -> Result<Value, String> {
        self.cache.move_player_equipment(slot, None, cell)
    }
    pub fn inventory_owner(&self, key: &str) -> Result<Value, String> {
        let obj = self.cache.original(parse_id(key)?)?;
        Ok(json!(
            self.cache.inventory.row(&obj, &self.cache.current(&obj))
        ))
    }
    pub fn inventory_transfer(
        &mut self,
        key: &str,
        op: &str,
        container: Option<&str>,
    ) -> Result<Value, String> {
        self.inventory_transfer_at(key, op, container, None)
    }
    pub fn inventory_transfer_at(
        &mut self,
        key: &str,
        op: &str,
        container: Option<&str>,
        slot: Option<usize>,
    ) -> Result<Value, String> {
        self.cache.transfer(
            parse_id(key)?,
            op,
            container.map(parse_id).transpose()?,
            slot,
        )
    }
    pub fn inventory_icon(&mut self, path: &str, palette: u8) -> Option<Arc<LoadedFrame>> {
        let resources = self.cache.inventory.resources.as_mut()?;
        resources
            .cache
            .load(&mut resources.vfs.borrow_mut(), path, 0, palette, false)
            .ok()
    }
}
fn parse_id(key: &str) -> Result<OriginalId, String> {
    OriginalId::parse(key)
        .ok_or_else(|| "Expected original 2:GUID or session runtime:N identity".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn footprint_validation_is_atomic_at_edges_overlaps_and_reposition() {
        let mut grid = Grid::default();
        let id = OriginalId {
            kind: 2,
            payload: [1; 16],
        };
        let other = OriginalId {
            kind: 2,
            payload: [2; 16],
        };
        grid.0.insert(id, (0, [3, 2]));
        assert_eq!(grid.fit(other, [3, 2], 12, None), Some(3));
        for slot in [1, 8, 110, usize::MAX] {
            assert_eq!(grid.fit(other, [3, 2], 12, Some(slot)), None);
        }
        assert_eq!(grid.fit(id, [3, 2], 12, Some(0)), Some(0));
        assert_eq!(grid.0.len(), 1);
    }

    #[test]
    fn equipment_replacement_and_full_grid_are_atomic() {
        use mobile::population::Property;

        let id = |n| OriginalId {
            kind: 2,
            payload: [n; 16],
        };
        let weapon = |n, shape| {
            let original = Original {
                id: id(n),
                category: 5,
                prototype: 6029,
                source: Arc::new(crate::content::Source {
                    virtual_path: "test.pro".into(),
                    mount_id: crate::vfs::MountId::GENERATED,
                    mount_path: Default::default(),
                    loose_path: None,
                    storage: crate::vfs::Storage::Loose,
                    mod_id: None,
                }),
                place: Place::World("original".into(), (2, 3)),
                effective: Effective {
                    current_art: Property::Instance(0x6000_0001),
                    flags: Property::Instance(0),
                    appearance: Default::default(),
                    dexterity: None,
                },
                description: None,
                icon: None,
                quantity: None,
                slot: None,
                flags: 0,
                item: Some(ItemStats {
                    weight: Some(650),
                    magic_weight: Some(0),
                    ..Default::default()
                }),
                scripted: false,
            };
            let mut session = Change {
                original,
                current: Place::Player,
                order: 0,
            };
            if n == 1 {
                session.current = Place::Equipped(EquipmentSlot::Weapon, shape);
            }
            session
        };
        let mut session = Session::default();
        let worn = id(1);
        let incoming = id(2);
        session.changes.insert(worn, weapon(1, [2, 1]));
        session.changes.insert(incoming, weapon(2, [1, 1]));
        session.player_grid.0.insert(incoming, (0, [1, 1]));
        for cell in 1..120 {
            session
                .player_grid
                .0
                .insert(id(cell as u8 + 2), (cell, [1, 1]));
        }
        assert!(
            session
                .move_equipment(EquipmentSlot::Helmet, Some(incoming), None)
                .is_err()
        );
        assert!(
            session
                .move_equipment(EquipmentSlot::Weapon, None, None)
                .is_err()
        );
        assert!(
            session
                .move_equipment(EquipmentSlot::Weapon, Some(incoming), None)
                .is_err()
        );
        assert_eq!(session.revision, 0);
        assert_eq!(
            session.equipped(EquipmentSlot::Weapon).unwrap().original.id,
            worn
        );
        assert_eq!(session.player_grid.0.get(&incoming), Some(&(0, [1, 1])));
        session.player_grid.0.remove(&id(3));
        let result = session
            .move_equipment(EquipmentSlot::Weapon, Some(incoming), None)
            .unwrap();
        assert_eq!(result["equipped"], incoming.key());
        assert_eq!(result["unequipped"], worn.key());
        assert_eq!(session.player_grid.0.get(&worn), Some(&(0, [2, 1])));
        assert!(!session.player_grid.0.contains_key(&incoming));
        assert_eq!(session.statistics().weight, Some(1300));
        assert_eq!(
            session.changes[&worn].original.place,
            Place::World("original".into(), (2, 3))
        );
        assert!(
            session
                .move_equipment(EquipmentSlot::Weapon, None, Some(0))
                .is_err()
        );
        session.player_grid.0.remove(&id(4));
        session
            .move_equipment(EquipmentSlot::Weapon, None, Some(2))
            .unwrap();
        assert_eq!(session.player_grid.0.get(&incoming), Some(&(2, [1, 1])));
        assert!(session.equipped(EquipmentSlot::Weapon).is_none());
    }

    #[test]
    #[ignore = "requires installed original data"]
    fn original_starting_items_and_armor_appearance() {
        let game = std::env::var_os("ARCANUM_GAME_DIR").unwrap();
        let vfs = || Vfs::from_game(&game, crate::vfs::Policy::Ce).unwrap();
        let mut data = vfs();
        let catalog = crate::content::maps::Catalog::load(&mut data).unwrap();
        let map = catalog.find("5011").unwrap();
        let region = map.files(&mut data).unwrap().bounds();
        let tile = (80, 102);
        let camera = region.camera(tile).unwrap();
        let mut stream = WorldStream::new(data, map.directory.clone(), region, tile)
            .unwrap()
            .with_player()
            .unwrap()
            .with_inventory(Rc::new(RefCell::new(vfs())))
            .with_starting_inventory()
            .unwrap()
            .with_starting_inventory()
            .unwrap();
        stream.prepare(&camera, (936, 500)).unwrap();
        let mut world = stream.into_interactive(|| {}, false).unwrap();
        world.start_player().unwrap();
        let inv = world.inventory_view();
        assert_eq!(inv.player.len(), 1);
        assert_eq!(inv.player[0].prototype, 9076);
        assert_eq!(inv.player[0].quantity, Some(400));
        assert_eq!(inv.equipment.len(), 1);
        let gear = &inv.equipment[0];
        assert_eq!(gear.item.prototype, 8069);
        assert_eq!(gear.slot, EquipmentSlot::Armor);
        assert_eq!(inv.statistics.attributes, [8; 8]);
        assert_eq!(inv.statistics.weight, Some(40));
        let paths = world.player_appearance().unwrap();
        assert_eq!(paths["standing"], "art/critter/hmm/hmmcdxaa.art");
        assert_eq!(paths["walking"], "art/critter/hmm/hmmcdxab.art");
        assert_eq!(paths["running"], "art/critter/hmm/hmmcdxag.art");
        let player = world.cache.player.as_mut().unwrap();
        player.update((1, 0), false, Duration::ZERO, |_, _| Some((80, 103)));
        let now = Duration::from_millis(70);
        let anchor = player.sample(now).anchor;
        let facing = player.facing();
        world
            .unequip_inventory_item(EquipmentSlot::Armor, None)
            .unwrap();
        let player = world.cache.player.as_ref().unwrap();
        assert!(player.stepping());
        assert_eq!(player.facing(), facing);
        assert_eq!(player.sample(now).anchor, anchor);
        assert_eq!(
            player.sample(now).asset.source.virtual_path,
            "art/critter/hmm/hmmuwxab.art"
        );
        assert_eq!(world.character_statistics().weight, Some(40));
        assert_eq!(world.character_statistics().resistance[0], 0);
        world
            .equip_inventory_item(&gear.item.id, EquipmentSlot::Armor)
            .unwrap();
        assert_eq!(world.player_appearance().unwrap(), paths);

        let ring = world
            .cache
            .inventory
            .create_carried_item(8304, 7, None)
            .unwrap()
            .key();
        assert_eq!(world.inventory_owner(&ring).unwrap()["name"], "Silver Ring");
        assert!(
            world
                .equip_inventory_item(&ring, EquipmentSlot::Medallion)
                .is_err()
        );
        for slot in [EquipmentSlot::Ring1, EquipmentSlot::Ring2] {
            world.equip_inventory_item(&ring, slot).unwrap();
            assert_eq!(
                world.inventory_owner(&ring).unwrap()["current"]["slot"],
                slot.name()
            );
            assert_eq!(world.player_appearance().unwrap(), paths);
            world.unequip_inventory_item(slot, None).unwrap();
            assert_eq!(
                world.inventory_owner(&ring).unwrap()["current"]["kind"],
                "player"
            );
        }

        let mail = world
            .cache
            .inventory
            .create_carried_item(8112, 7, None)
            .unwrap()
            .key();
        let _spare = world
            .cache
            .inventory
            .create_carried_item(8112, 7, None)
            .unwrap();
        let burdened = world.character_statistics();
        assert_eq!(
            burdened.encumbrance,
            Some(crate::content::character::Encumbrance::Light)
        );
        assert_eq!(burdened.speed, Some(6));
        let equipped = world
            .equip_inventory_item(&mail, EquipmentSlot::Armor)
            .unwrap();
        assert_eq!(equipped["unequipped"], gear.item.id);
        let stats = world.character_statistics();
        assert_eq!(stats.weight, burdened.weight);
        assert_eq!(stats.armor_class, 10);
        assert_eq!(stats.resistance[0], 23);
        assert_eq!(
            world.player_appearance().unwrap()["standing"],
            "art/critter/hmm/hmmcmxaa.art"
        );
    }

    #[test]
    #[ignore = "requires installed original data"]
    fn original_container_session_transfers() {
        let game = std::env::var_os("ARCANUM_GAME_DIR").unwrap();
        let vfs = || Vfs::from_game(&game, crate::vfs::Policy::Ce).unwrap();
        let mut data = vfs();
        let catalog = crate::content::maps::Catalog::load(&mut data).unwrap();
        let map = catalog.find("5011").unwrap();
        let region = map.files(&mut data).unwrap().bounds();
        let tile = (80, 102);
        let camera = region.camera(tile).unwrap();
        let mut stream = WorldStream::new(data, map.directory.clone(), region, tile)
            .unwrap()
            .with_player()
            .unwrap()
            .with_inventory(Rc::new(RefCell::new(vfs())));
        stream.prepare(&camera, (936, 500)).unwrap();
        let mut world = stream.into_interactive(|| {}, false).unwrap();
        world.start_player().unwrap();
        let chest = "2:98fd05e54e0ed4118f1d00e02920220c";
        world.inventory_open(Some(chest)).unwrap();
        let contents = world.inventory_view().contents.unwrap();
        assert!(!contents.is_empty());
        let id = contents[0].id.as_str();
        let original = world.inventory_owner(id).unwrap();
        assert!(original["icon"].is_string());
        assert!(
            world
                .inventory_transfer(chest, "take", Some(chest))
                .is_err()
        );
        assert!(world.inventory_transfer(id, "put", Some(id)).is_err());
        assert!(world.inventory_transfer(id, "take", None).is_err());
        assert_eq!(world.inventory_owner(id).unwrap(), original);

        world.inventory_transfer(id, "take", Some(chest)).unwrap();
        assert!(world.inventory_transfer(id, "take", Some(chest)).is_err());
        let carried = world.character_statistics().weight;
        assert!(carried.is_some_and(|weight| weight > 0));
        assert!(
            world
                .equip_inventory_item(id, EquipmentSlot::Weapon)
                .is_err()
        );
        world
            .equip_inventory_item(id, EquipmentSlot::Armor)
            .unwrap();
        assert_eq!(
            world.inventory_owner(id).unwrap()["current"]["kind"],
            "equipped"
        );
        assert_eq!(world.character_statistics().weight, carried);
        assert!(world.inventory_view().player.is_empty());
        world
            .unequip_inventory_item(EquipmentSlot::Armor, None)
            .unwrap();
        world.inventory_transfer(id, "drop", None).unwrap();
        let dropped = world.inventory_owner(id).unwrap();
        assert_eq!(dropped["current"]["tile"], json!(tile));
        let raw_id = parse_id(id).unwrap();
        let active = world.cache.active.clone();
        world.cache.activate(&[]);
        world.cache.activate(&active);
        assert_eq!(
            world
                .cache
                .ordered
                .iter()
                .filter(|r| world.cache.item_at(r.key) == Some(raw_id))
                .count(),
            1
        );
        assert!(world.inventory_view().player.is_empty());
        world.inventory_transfer(id, "take", None).unwrap();
        assert!(
            !world
                .cache
                .ordered
                .iter()
                .any(|r| world.cache.item_at(r.key) == Some(raw_id))
        );

        world.inventory_transfer(id, "put", Some(chest)).unwrap();
        assert_eq!(world.inventory_owner(id).unwrap(), original);
        assert_eq!(
            world.inventory_view().contents.unwrap().len(),
            contents.len()
        );
    }
}
