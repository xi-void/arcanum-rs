//! Session changes keyed by serialized identity. Sector records stay immutable.
use super::*;
use crate::sector::objects::{self, FieldValue, OriginalId};
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
    World(String, Tile),
}
impl Place {
    fn report(&self) -> Value {
        match self {
            Self::Player => json!({"kind":"player"}),
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
            scripted: object.field(31).map_or(
                proto.defaults.transfer_script,
                objects::has_inventory_script,
            ),
        })
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
    containers: BTreeMap<OriginalId, Grid>,
    open: Option<Option<OriginalId>>,
    revision: u64,
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
            Place::World(..) => None,
        }
    }
    fn grid_mut(&mut self, place: &Place) -> Option<&mut Grid> {
        match place {
            Place::Player => Some(&mut self.player_grid),
            Place::Container(id) => self.containers.get_mut(id),
            Place::World(..) => None,
        }
    }
    fn icon_path(&self, original: &Original) -> Option<String> {
        let art = original.icon.filter(|art| art >> 28 == 6)?;
        let name = self.resources.as_ref()?.text(
            "art/item/item_inven.mes",
            Some(mobile::item_art_entry(art) as u32),
        )?;
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
        InventoryItem {
            id: original.id.key(),
            name: name.map(str::to_owned),
            description_id: original.description,
            category: objects::category_name(original.category),
            prototype: original.prototype,
            source: super::super::diagnostics::source(&original.source),
            original: original.place.report(),
            current: place.report(),
            original_slot: original.slot,
            item_or_container_flags: original.flags,
            transfer_script_attached: original.scripted,
            slot: square.map(|s| s.0),
            footprint: square.map(|s| s.1),
            quantity: original.quantity,
            inventory_art: original.icon,
            icon,
        }
    }
}

impl SectorCache {
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
        if obj.flags & 0x80 == 0
            && (obj.flags & 6 != 0 || interaction::locked(obj.flags, self.hour))
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
        if obj.scripted || app.use_script != Some(0) || app.unlock_script != Some(0) {
            return Err(format!(
                "Unsupported container scripts (use {:?}, unlock {:?}, transfer {})",
                app.use_script, app.unlock_script, obj.scripted
            ));
        }
        Ok(())
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
        ] {
            match load_mes(&mut vfs.borrow_mut(), path) {
                Ok(m) => {
                    tables.insert(path, m);
                }
                Err(e) => eprintln!("Inventory resource {path}: {e}"),
            }
        }
        self.cache.inventory.resources = Some(Box::new(Resources {
            vfs,
            cache: FrameCache::default(),
            tables,
        }));
        self
    }
}
impl InteractiveWorld {
    pub fn inherit_inventory(&mut self, old: &mut Self) {
        self.cache.inventory = std::mem::take(&mut old.cache.inventory);
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
            contents: container.map(|id| self.cache.contents(&Place::Container(id))),
        }
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
    OriginalId::parse(key).ok_or_else(|| "Expected original 2:GUID identity".into())
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
