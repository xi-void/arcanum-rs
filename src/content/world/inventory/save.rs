//! Persistent object transactions. Immutable item data is resolved from VFS on load.
use super::*;
use crate::content::maps::Catalog;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct Objects {
    #[serde(default)]
    combat: combat::Combat,
    #[serde(default)]
    encounter: casting::encounters::Schedule,
    #[serde(default)]
    accounts: trade::Accounts,
    #[serde(default)]
    restocks: stock::Events,
    #[serde(default)]
    skill_random: crate::random::Random,
    #[serde(default)]
    recharges: BTreeMap<String, use_action::Recharge>,
    #[serde(default)]
    magick: casting::State,
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
    #[serde(default)]
    identified: bool,
    #[serde(default)]
    structural_damage: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    insert_stat: Option<u8>,
    quantity: Option<u32>,
    place: Place,
    slot: Option<usize>,
    order: usize,
}

impl InteractiveWorld {
    pub fn starting_possessions(&self) -> Objects {
        let mut objects = self.save_objects();
        objects
            .items
            .retain(|item| matches!(item.place, Place::Owned(Owner::Player) | Place::Equipped(_)));
        // Discarded merchant records can leave gaps in the transaction sequence.
        objects.items.sort_by_key(|item| item.order);
        for (order, item) in objects.items.iter_mut().enumerate() {
            item.order = order;
        }
        objects.accounts.clear();
        objects.restocks.clear();
        objects
    }

    pub fn save_objects(&self) -> Objects {
        let session = &self.cache.inventory;
        Objects {
            combat: session.combat.snapshot(self.cache.effect_time),
            encounter: session.encounter.clone(),
            accounts: session.accounts.clone(),
            restocks: session.restocks.clone(),
            magick: session.magick.clone(),
            character: session.character.clone(),
            skill_random: session.skill_random,
            recharges: session.recharges.clone(),
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
                        identified: item.flags & 1 != 0,
                        insert_stat: item.insert_stat,
                        structural_damage: Some(item.stats.hp[2]),
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
        self = self.prepare_player(None, Some((inventory, appearance)), None)?;
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
        if let Some(haggle) = session.character.legacy_haggle.take() {
            session.character.basic_skills[9] = haggle;
        }
        session.character.validate_progression()?;
        session.character.sector_aptitude = 0;
        let backgrounds = load_mes(vfs, "rules/backgrnd.mes")?;
        session.load_rules(vfs, &backgrounds)?;
        self.combat.validate()?;
        session.combat = self.combat.clone();
        session.magick = self.magick.clone();

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
            if saved.identified {
                item.flags |= 1;
            }
            if let Some(damage) = saved.structural_damage {
                if damage < 0 {
                    return Err("Invalid saved item damage".into());
                }
                item.stats.hp[2] = damage;
            }
            item.insert_stat = saved.insert_stat;
            item.stats.quantity = saved.quantity;
            match &saved.place {
                Place::Owned(owner) if saved.slot.is_some() => {
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
                Place::WornBy(map, id, slot) if saved.slot.is_none() => {
                    if !(session.magick.owns_spawn(map, *id)
                        || map_index(vfs, catalog, &mut indices, map)?.category(*id) == Some(16))
                        || !slot.accepts(
                            item.category,
                            item.inventory_art,
                            item.stats.equipment_flags.unwrap_or_default(),
                        )
                        || session
                            .state
                            .changes
                            .values()
                            .any(|c| c.place == saved.place)
                    {
                        return Err("Invalid summoned equipment slot".into());
                    }
                }
                Place::Consumed if saved.slot.is_none() => {}
                Place::Retired if saved.slot.is_none() => {}
                Place::Spent
                    if item.category == 8 && saved.quantity == Some(0) && saved.slot.is_none() => {}
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

        session.restore_magick(vfs)?;
        for c in session
            .state
            .changes
            .values()
            .filter(|c| matches!(c.place, Place::Equipped(_) | Place::WornBy(..)))
        {
            if let Some(entry) = session.prepare_wield(vfs, &c.item)? {
                session.wielded.insert(c.item.id, entry);
            }
        }
        session.refresh_worn();
        session.skill_random = self.skill_random;
        for (key, event) in &self.recharges {
            let id = parse_id(key)?;
            if event.spent <= 0
                || session
                    .state
                    .changes
                    .get(&id)
                    .is_none_or(|c| c.item.stats.mana_store < event.spent)
            {
                return Err("Invalid saved item mana expenditure".into());
            }
        }
        session.recharges = self.recharges.clone();
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
            if let Owner::OriginalObject(map, id) = &owner
                && id.kind == u16::MAX
                && !session.magick.owns_spawn(map, *id)
            {
                return Err("Missing owner of generated creature inventory".into());
            }
            let (mut records, container) = if let Owner::Container(map, id) = &owner {
                let index = map_index(vfs, catalog, &mut indices, map)?;
                let mut records = records(vfs, map, Some(index));
                let container = records.container(*id)?;
                (records, Some(container))
            } else if let Owner::OriginalObject(map, _) = &owner {
                let index = map_index(vfs, catalog, &mut indices, map)?;
                (records(vfs, map, Some(index)), None)
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
        for (map, accounts) in &self.accounts {
            let index = map_index(vfs, catalog, &mut indices, map)?;
            for (key, account) in accounts {
                if index.category(parse_id(key)?) != Some(16) || account.gold > i32::MAX as u32 {
                    return Err("Invalid saved merchant account".into());
                }
            }
        }
        for (map, events) in &self.restocks {
            let index = map_index(vfs, catalog, &mut indices, map)?;
            for (key, event) in events {
                if !matches!(index.category(parse_id(key)?), Some(2 | 16))
                    || index.category(parse_id(&event.merchant)?) != Some(16)
                {
                    return Err("Invalid merchant restock identity".into());
                }
            }
        }
        session.restocks = self.restocks.clone();
        session.encounter = self.encounter.clone();
        session.accounts = self.accounts.clone();
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

pub(super) fn records<'a>(vfs: &'a mut Vfs, map: &'a str, index: Option<&'a Index>) -> Records<'a> {
    Records {
        vfs,
        map,
        index,
        foreign: None,
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
