//! Persistent object transactions. Immutable item data is resolved from VFS on load.
use super::*;
use crate::content::maps::Catalog;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct Objects {
    character: Character,
    next_generated: u64,
    items: Vec<SavedItem>,
    unlocked: BTreeMap<String, Vec<OriginalId>>,
}

#[derive(Serialize, Deserialize)]
struct SavedItem {
    id: OriginalId,
    prototype: u32,
    category: u32,
    origin_map: String,
    instance: String,
    quantity: Option<u32>,
    place: Place,
    slot: Option<usize>,
    order: usize,
}

impl InteractiveWorld {
    pub fn save_objects(&self) -> Objects {
        let session = &self.cache.inventory;
        Objects {
            character: session.character.clone(),
            next_generated: session.state.next_generated,
            items: session
                .state
                .changes
                .values()
                .map(|change| {
                    let item = &change.item;
                    SavedItem {
                        id: item.id,
                        prototype: item.prototype,
                        category: item.category,
                        origin_map: item.origin_map.clone(),
                        instance: item.instance_source.virtual_path.clone(),
                        quantity: item.stats.quantity,
                        place: change.place.clone(),
                        slot: change.slot,
                        order: change.order,
                    }
                })
                .collect(),
            unlocked: session
                .unlocked
                .iter()
                .map(|(map, ids)| (map.clone(), ids.iter().copied().collect()))
                .collect(),
        }
    }
}

impl WorldStream {
    pub fn with_saved_player(
        mut self,
        doors: DoorStates,
        objects: &Objects,
        catalog: &Catalog,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let (inventory, appearance) = objects.restore(&mut self.loader.vfs, catalog)?;
        self = self.prepare_player(None, Some((inventory, appearance)))?;
        self.cache.door_states = doors;
        self.cache.inventory_context = Some((self.loader.map.clone(), self.loader.reference));
        Ok(self)
    }
}

impl Objects {
    fn restore(
        &self,
        vfs: &mut Vfs,
        catalog: &Catalog,
    ) -> Result<(Session, Arc<Appearance>), Box<dyn std::error::Error>> {
        let mut session = Session::default();
        session.state.next_generated = self.next_generated;
        session.character = self.character.clone();
        session.character.sector_aptitude = 0;
        let backgrounds = load_mes(vfs, "rules/backgrnd.mes")?;
        session.load_rules(vfs, &backgrounds)?;

        let mut orders = BTreeSet::new();
        let mut equipment = BTreeMap::new();
        let mut owners = BTreeSet::new();
        let mut indices = BTreeMap::new();
        let mut regions = BTreeMap::new();
        let mut frames = FrameCache::default();
        for saved in &self.items {
            if !(5..=14).contains(&saved.category)
                || saved.order >= self.items.len()
                || !orders.insert(saved.order)
                || session.state.changes.contains_key(&saved.id)
            {
                return Err("Invalid or duplicate item identity or transaction order".into());
            }
            let generated = saved.id.kind == u16::MAX;
            if generated
                && (saved.id.payload[8..] != [0; 8]
                    || u64::from_le_bytes(saved.id.payload[..8].try_into()?) >= self.next_generated)
            {
                return Err("Generated item ID exceeds the saved allocator".into());
            }
            let mut item = if saved.origin_map.is_empty() && generated {
                Session::prototype_item(
                    vfs,
                    &mut frames,
                    saved.id,
                    saved.prototype,
                    saved.category,
                )?
            } else {
                let index = map_index(vfs, catalog, &mut indices, &saved.origin_map)?;
                let (_, bytes) = read_bounded(vfs, &saved.instance, MAX_PROTOTYPE_BYTES)?;
                let original = objects::decode_instance(&bytes)?.id;
                if (!generated && original != saved.id)
                    || index.path(original) != Some(&saved.instance)
                {
                    return Err("Saved item no longer matches its original record".into());
                }
                let mut records = records(vfs, &saved.origin_map, Some(index));
                records.item(original, &State::default())?
            };
            if item.prototype != saved.prototype
                || item.category != saved.category
                || item.stats.quantity.is_some() != saved.quantity.is_some()
                || (generated && !saved.origin_map.is_empty() && !matches!(item.category, 6 | 8))
                || saved
                    .quantity
                    .is_some_and(|quantity| quantity > i32::MAX as u32)
            {
                return Err("Saved item does not match its original definition".into());
            }

            item.id = saved.id;
            item.generated = generated;
            item.stats.quantity = saved.quantity;
            match &saved.place {
                Place::Owned(owner @ (Owner::Player | Owner::Container(..)))
                    if saved.slot.is_some() =>
                {
                    owners.insert(owner.clone());
                }
                Place::Equipped(slot) if saved.slot.is_none() => {
                    if equipment.insert(*slot, saved.id).is_some() {
                        return Err("Duplicate equipment slot".into());
                    }
                }
                Place::Ground(map, tile) if saved.slot.is_none() => {
                    if !regions.contains_key(map) {
                        regions.insert(map.clone(), exact_map(catalog, map)?.files(vfs)?.bounds());
                    }
                    if regions[map].camera(*tile).is_none() {
                        return Err("Dropped item is outside the saved map".into());
                    }
                    let art = records(vfs, map, None).ground_art(&item, *tile, *tile)?;
                    session.ground.insert(saved.id, art);
                }
                Place::Merged(target) if saved.slot.is_none() && *target != saved.id => {}
                _ => return Err("Invalid saved item placement".into()),
            }
            session.state.changes.insert(
                saved.id,
                super::super::super::inventory::Change {
                    item,
                    place: saved.place.clone(),
                    slot: saved.slot,
                    order: saved.order,
                },
            );
        }

        for (&slot, id) in &equipment {
            session.check_equipment(&session.state.changes[id].item, slot)?;
        }
        for change in session.state.changes.values() {
            let mut next = change;
            let mut remaining = session.state.changes.len();
            while let Place::Merged(id) = next.place {
                let target = session
                    .state
                    .changes
                    .get(&id)
                    .ok_or("Missing merged stack target")?;
                if remaining == 0 || !next.item.same_stack(&target.item) {
                    return Err("Invalid merged stack chain".into());
                }
                remaining -= 1;
                next = target;
            }
        }
        for owner in owners {
            let (mut records, container) = if let Owner::Container(map, id) = &owner {
                let index = map_index(vfs, catalog, &mut indices, map)?;
                let mut records = records(vfs, map, Some(index));
                let container = records.container(*id)?;
                (records, Some(container))
            } else {
                (records(vfs, "", None), None)
            };
            let layout = records.layout(&session.state, &owner, container.as_ref())?;
            for (&id, &(slot, cells)) in &layout.grid.items {
                if layout.grid.fit(id, cells, owner.rows(), Some(slot)) != Some(slot) {
                    return Err("Saved items overlap or exceed inventory capacity".into());
                }
            }
        }
        for (map, ids) in &self.unlocked {
            let index = map_index(vfs, catalog, &mut indices, map)?;
            let mut records = records(vfs, map, Some(index));
            let mut validated = BTreeSet::new();
            for &id in ids {
                records.container(id)?;
                if !validated.insert(id) {
                    return Err("Duplicate unlocked container identity".into());
                }
            }
            session.unlocked.insert(map.clone(), validated);
        }

        let armor = equipment
            .get(&EquipmentSlot::Armor)
            .map(|id| &session.state.changes[id].item);
        let art = session.body_art(armor)?;
        let appearance = Appearance::load(vfs, &mut frames, art)?;
        Ok((session, Arc::new(appearance)))
    }
}

fn records<'a>(vfs: &'a mut Vfs, map: &'a str, index: Option<&'a Index>) -> Records<'a> {
    Records {
        vfs,
        map,
        index,
        frames: FrameCache::default(),
        prototypes: Prototypes::default(),
    }
}

fn map_index<'a>(
    vfs: &mut Vfs,
    catalog: &Catalog,
    indices: &'a mut BTreeMap<String, Index>,
    map: &str,
) -> Result<&'a Index, String> {
    exact_map(catalog, map)?;
    Ok(indices
        .entry(map.into())
        .or_insert_with(|| Index::load(vfs, map)))
}

fn exact_map<'a>(
    catalog: &'a Catalog,
    directory: &str,
) -> Result<&'a crate::content::maps::Map, String> {
    catalog
        .maps
        .iter()
        .find(|map| map.directory == directory)
        .ok_or_else(|| format!("Save refers to unavailable map {directory:?}"))
}
