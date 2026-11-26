//! Connect session inventory to original map records and world presentation.
use super::super::inventory::{COLUMNS, Footprint, Grid, Item, Owner, Place, State, Tile};
use super::super::read_bounded;
use super::*;
use crate::prototype::MAX_PROTOTYPE_BYTES;
use crate::sector::objects::{self, FieldValue, OriginalId};
use mobile::population::{Effective, Index, Prototypes};
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

impl Session {
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
            subtype_flags: subtype(64, proto.defaults.subtype_flags)
                .ok_or("Unresolved container flags")?,
            inventory_source: subtype(69, proto.defaults.inventory_source),
            scripted: object.field(31).map_or(
                proto.defaults.inventory_scripts,
                objects::has_inventory_scripts,
            ) || effective.appearance.use_script != Some(0)
                || effective.appearance.unlock_script != Some(0),
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
        if let Some(container) = container {
            let mut originals = container
                .members
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
    inventory_source: Option<u32>,
    scripted: bool,
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
        let unlocked = world.container_unlocked(id);
        if self.subtype_flags & 0x80 == 0
            && (self.subtype_flags & 6 != 0
                || (!unlocked && interaction::locked(self.subtype_flags, hour)))
        {
            return Err("Container is locked, jammed or magically held".into());
        }
        if self.scripted {
            return Err("Container requires unsupported inventory, use or unlock scripts".into());
        }
        Ok(())
    }

    fn report(&self) -> Value {
        json!({"tile":self.tile,"flags":self.subtype_flags,"inventory_source":self.inventory_source,
            "scripted":self.scripted,"provenance":self.provenance})
    }
}

#[derive(Default)]
struct Layout {
    items: BTreeMap<OriginalId, Item>,
    grid: Grid,
}

fn parse_id(key: &str) -> Result<OriginalId, String> {
    OriginalId::parse_key(key).ok_or_else(|| "Expected a serialized item or container GUID".into())
}

/// Read-only presentation data projected from the same owner layout as inventory RPC.
pub struct InventoryContents {
    pub columns: usize,
    pub rows: usize,
    pub entries: Vec<InventoryEntry>,
    grid: Grid,
}

impl InventoryContents {
    pub fn fits(&self, item: &InventoryEntry, slot: usize) -> bool {
        parse_id(&item.id).is_ok_and(|id| {
            self.grid
                .fit(id, item.cells, self.rows, Some(slot))
                .is_some()
        })
    }
}

pub struct InventoryEntry {
    pub id: String,
    pub slot: usize,
    pub cells: [usize; 2],
    pub name: String,
    pub movable: bool,
    pub removable: bool,
    pub icon: Result<Arc<LoadedFrame>, String>,
}

pub struct InventoryReader {
    vfs: Vfs,
    frames: FrameCache,
    descriptions: Vec<super::super::LoadedMes>,
}

impl InventoryReader {
    pub fn new(mut vfs: Vfs) -> Self {
        let descriptions = ["mes/gamedesc.mes", "mes/description.mes"]
            .into_iter()
            .filter_map(|path| super::super::load_mes(&mut vfs, path).ok())
            .collect();
        Self {
            vfs,
            frames: FrameCache::default(),
            descriptions,
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
        let target = target.map(parse_id).transpose()?;
        let owner = target.map_or(Owner::Player, |id| Owner::Container(world.map.clone(), id));
        let mut records = world.records(&mut self.vfs);
        let container = target.map(|id| records.container(id)).transpose()?;
        if let (Some(container), Some(id)) = (&container, target) {
            container.check_access(world, id, world.cache.hour)?;
        }
        let layout = records.layout(&world.cache.inventory.state, &owner, container.as_ref())?;
        let mut entries = Vec::new();
        for (&id, &(slot, cells)) in &layout.grid.items {
            let item = &layout.items[&id];
            if !item_visible(item) {
                continue;
            }
            let name = item
                .description
                .and_then(|id| {
                    self.descriptions
                        .iter()
                        .find_map(|table| table.mes.get(id as i32))
                })
                .or_else(|| objects::category_name(item.category))
                .unwrap_or("Item")
                .to_owned();
            let icon = super::super::inventory::inventory_frame(
                &mut self.vfs,
                &mut self.frames,
                item.inventory_art,
            )
            .map_err(|error| error.to_string());
            entries.push(InventoryEntry {
                id: id.key(),
                slot,
                cells,
                name,
                movable: check_item(item).is_ok(),
                removable: item.flags & ITEM_NO_DROP == 0,
                icon,
            });
        }
        self.frames.finish_sector();
        Ok(InventoryContents {
            columns: COLUMNS,
            rows: owner.rows(),
            entries,
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
        Ok(Owner::Container(self.map.clone(), parse_id(key)?))
    }

    pub fn inspect_inventory(&self, vfs: &mut Vfs, key: &str) -> Result<Value, String> {
        let owner = self.inventory_owner(key)?;
        let state = &self.cache.inventory.state;
        let mut records = self.records(vfs);
        let container = match &owner {
            Owner::Player => None,
            Owner::Container(_, id) => Some(records.container(*id)?),
            Owner::OriginalObject(..) => return Err("Unsupported inventory owner".into()),
        };
        let layout = records.layout(state, &owner, container.as_ref())?;
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
            json!({"owner":place.report(),"columns":COLUMNS,"rows":owner.rows(),
            "container":container.as_ref().map(|container| {
                let mut report = container.report();
                if let Owner::Container(_, id) = owner {
                    report["developer_unlock"] = json!(self.container_unlocked(id));
                }
                report
            }),
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
        self.move_item(vfs, key, Move::Pickup(slot), 0)
    }

    pub fn drop_item(&mut self, vfs: &mut Vfs, key: &str) -> Result<Value, String> {
        self.move_item(vfs, key, Move::Drop, 0)
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
        let from = self.inventory_owner(from)?;
        let to = self.inventory_owner(to)?;
        self.move_item(
            vfs,
            key,
            Move::Transfer(from, to, slot),
            hour.unwrap_or(self.cache.hour),
        )
    }

    fn move_item(
        &mut self,
        vfs: &mut Vfs,
        key: &str,
        action: Move,
        hour: u8,
    ) -> Result<Value, String> {
        let id = parse_id(key)?;
        let mut records = self.records(vfs);
        let state = &self.cache.inventory.state;
        let item = records.item(id, state)?;
        let current = state.place(&item);
        let flags = check_item(&item)?;
        let (destination, requested) = match action {
            Move::Pickup(slot) => {
                let Place::Ground(map, tile) = current else {
                    return Err("Item is not on the ground".into());
                };
                if map != &self.map || flags & CLICK_THROUGH != 0 {
                    return Err("Item is not selectable on this map".into());
                }
                self.check_inventory_reach(*tile)?;
                (Place::Owned(Owner::Player), slot)
            }
            Move::Drop if *current == Place::Owned(Owner::Player) => {
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
            Move::Transfer(from, to, slot) => {
                if *current != Place::Owned(from) {
                    return Err("Item does not belong to the specified source".into());
                }
                (Place::Owned(to), slot)
            }
        };
        if current != &destination && matches!(current, Place::Owned(_)) {
            check_removal(&item)?;
        }
        if let Place::Owned(Owner::Container(_, source)) = current {
            records
                .container(*source)?
                .check_access(self, *source, hour)?;
        }
        let mut ground = None;
        let layout = match &destination {
            Place::Owned(owner) => {
                let container = match owner {
                    Owner::Player => None,
                    Owner::Container(_, target) => {
                        let container = records.container(*target)?;
                        container.check_access(self, *target, hour)?;
                        Some(container)
                    }
                    Owner::OriginalObject(..) => return Err("Unsupported inventory owner".into()),
                };
                Some(records.layout(state, owner, container.as_ref())?)
            }
            Place::Ground(_, tile) => {
                ground = Some(records.ground_art(&item, *tile, self.reference)?);
                None
            }
        };
        let world_changed = matches!(current, Place::Ground(..)) || ground.is_some();
        let session = &mut self.cache.inventory;
        let result = session.state.relocate(
            item,
            destination,
            layout.as_ref().map(|l| (&l.grid, requested)),
        )?;
        session.revision += 1;
        if let Some(art) = ground {
            session.ground.insert(id, art);
        } else {
            session.ground.remove(&id);
        }
        if world_changed {
            self.cache.activate(&self.cache.active.clone());
        }
        Ok(result)
    }

    fn take_all(&mut self, vfs: &mut Vfs, target: &str) -> Result<(), String> {
        let id = parse_id(target)?;
        let source = Owner::Container(self.map.clone(), id);
        let owner = Owner::Player;
        let destination = Place::Owned(owner.clone());
        let mut records = self.records(vfs);
        let container = records.container(id)?;
        container.check_access(self, id, self.cache.hour)?;
        let state = &self.cache.inventory.state;
        let mut loot = records.layout(state, &source, Some(&container))?;
        let mut backpack = records.layout(state, &owner, None)?.grid;
        let mut slots: Vec<_> = loot.grid.items.into_iter().collect();
        slots.sort_unstable_by_key(|(_, (slot, _))| *slot);
        let session = &mut self.cache.inventory;
        let mut remaining = 0;
        let mut reason = None;
        for (id, _) in slots {
            let item = loot.items.remove(&id).expect("layout item");
            if !item_visible(&item) {
                continue;
            }
            let cells = item.footprint.cells;
            let moved = check_item(&item)
                .and_then(|_| check_removal(&item))
                .and_then(|()| {
                    let slot = backpack
                        .fit(id, cells, owner.rows(), None)
                        .ok_or("Item does not fit in the requested destination")?;
                    session.state.relocate(
                        item,
                        destination.clone(),
                        Some((&backpack, Some(slot))),
                    )?;
                    backpack.items.insert(id, (slot, cells));
                    session.revision += 1;
                    Ok(())
                });
            if let Err(error) = moved {
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
        for owner in ["player"].into_iter().chain(target) {
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
        if container.scripted || container.subtype_flags & 6 != 0 {
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
        self.cache.inventory = std::mem::take(&mut previous.cache.inventory);
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
}

#[cfg(test)]
mod tests {
    use super::*;

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
            .transfer_item(&mut data, id, chest, "player", Some(0), None)
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
        room.pickup_item(&mut data, id, Some(0)).unwrap();
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
        world.pickup_item(&mut data, id, Some(0)).unwrap();
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
