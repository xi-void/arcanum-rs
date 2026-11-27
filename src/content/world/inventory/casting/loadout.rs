//! Prepare generated NPC equipment from the same inventory source tables as shops.
use super::*;

#[derive(Default)]
pub(super) struct Loadout {
    items: Vec<(Item, Option<EquipmentSlot>, Option<usize>)>,
}

impl Loadout {
    pub(super) fn prepare(
        vfs: &mut Vfs,
        frames: &mut FrameCache,
        d: &objects::PrototypeDefaults,
        rng: &mut crate::random::Random,
    ) -> Result<Self, String> {
        let Some(source) = d.inventory_source.filter(|&n| n != 0) else {
            return Ok(Self::default());
        };
        let table = load_mes(vfs, "rules/invensource.mes").map_err(|e| e.to_string())?;
        let set = stock::SourceSet::parse(
            table
                .mes
                .get(source as i32)
                .ok_or("Missing creature inventory source")?,
        )?;
        let mut equipment = BTreeMap::new();
        let mut items = Vec::new();
        let mut grid = Grid::default();
        let mut selected = Vec::new();
        let gold = set.coins(rng);
        if gold > 0 {
            selected.push((9076, Some(gold)));
        }
        for (rate, n) in set.items {
            if rng.below(100) < u64::from(rate) {
                selected.push((
                    crate::prototype::BasicPrototypeId(n)
                        .serialized()
                        .ok_or("Invalid inventory source prototype")?
                        .0,
                    None,
                ));
            }
        }
        let pc = npc_character(d, None);
        for (index, (proto, qty)) in selected.into_iter().enumerate() {
            let (_, bytes) = crate::content::sector::read_static_prototype(
                vfs,
                crate::prototype::SerializedPrototypeId(proto),
            )
            .map_err(|e| e.to_string())?;
            let category = crate::prototype::decode_header(&bytes)
                .map_err(|e| e.to_string())?
                .object_type;
            if !(5..=14).contains(&category) {
                return Err("Creature inventory source specifies a non-item".into());
            }
            let placeholder = OriginalId {
                kind: u16::MAX,
                payload: [0; 16],
            };
            let mut item = Session::prototype_item(vfs, frames, placeholder, proto, category)?;
            item.id.payload[..8].copy_from_slice(&(index as u64).to_le_bytes());
            if qty.is_some() {
                item.stats.quantity = qty
            }
            let flags = item.stats.equipment_flags.unwrap_or_default();
            let candidate = EquipmentSlot::ALL.into_iter().find(|slot| {
                slot.accepts(category, item.inventory_art, flags)
                    && (*slot != EquipmentSlot::Armor
                        || pc.armor_size().is_some_and(|size| size & flags != 0))
            });
            if let Some(slot) = candidate {
                let worth = item.stats.worth;
                let best = equipment.entry(slot).or_insert((index, worth));
                if worth > best.1 {
                    *best = (index, worth);
                }
            }
            items.push(item);
        }
        let mut result = Self::default();
        for (index, item) in items.into_iter().enumerate() {
            let wear = equipment
                .iter()
                .find_map(|(slot, (i, _))| (*i == index).then_some(*slot));
            let cell = if wear.is_none() {
                let Some(n) = grid.fit(item.id, item.footprint.cells, Owner::Player.rows(), None)
                else {
                    continue;
                };
                grid.items.insert(item.id, (n, item.footprint.cells));
                Some(n)
            } else {
                None
            };
            result.items.push((item, wear, cell));
        }
        Ok(result)
    }
    pub(super) fn count(&self) -> usize {
        self.items.len()
    }
    pub(super) fn armor(&self) -> Option<u32> {
        self.items.iter().find_map(|(i, slot, _)| {
            (*slot == Some(EquipmentSlot::Armor))
                .then_some(i.stats.use_art)
                .flatten()
        })
    }
    pub(super) fn commit(self, inv: &mut Session, map: &str, actor: OriginalId) {
        let owner = Owner::OriginalObject(map.to_owned(), actor);
        for (mut item, slot, cell) in self.items {
            item.id = inv.state.generated_id();
            inv.state.next_generated += 1;
            let place = slot.map_or_else(
                || Place::Owned(owner.clone()),
                |slot| Place::WornBy(map.to_owned(), actor, slot),
            );
            inv.state.put(item, place, cell);
        }
    }
}

impl Spawn {
    pub(super) fn model_key(&self, inventory: &super::super::State) -> ModelKey {
        let armor = inventory.changes.values().find_map(|c| match c.place {
            Place::WornBy(_, id, EquipmentSlot::Armor) if id == self.id => c.item.stats.use_art,
            _ => None,
        });
        ModelKey::summon(self.prototype, self.palette, self.restore_art, armor)
    }
}
