//! Resolve the inventory changes attached to original creature ART traits.
use super::*;

pub(super) struct FormEquipment {
    items: Vec<(EquipmentSlot, Item, Option<usize>, Option<GroundArt>)>,
    appearance: Option<Arc<Appearance>>,
}

impl FormEquipment {
    // Prepare the whole change before charging fatigue or touching any item.
    pub(super) fn prepare(w: &InteractiveWorld, vfs: &mut Vfs) -> Result<Self, String> {
        let inv = &w.cache.inventory;
        let tile = w.cache.player.as_ref().ok_or("No active player")?.tile();
        let mut records = w.records(vfs);
        let mut grid = records.owner_layout(&inv.state, &Owner::Player)?.grid;
        let mut items = Vec::new();
        for slot in EquipmentSlot::ALL {
            let Some(change) = inv.equipped(slot) else {
                continue;
            };
            let item = &change.item;
            if check_removal(item).is_err() {
                continue;
            }
            let cell = grid.fit(item.id, item.footprint.cells, Owner::Player.rows(), None);
            let ground = if let Some(cell) = cell {
                grid.items.insert(item.id, (cell, item.footprint.cells));
                None
            } else {
                Some(records.ground_art(item, tile, w.reference)?)
            };
            items.push((slot, item.clone(), cell, ground));
        }
        let appearance = if items.iter().any(|(slot, ..)| *slot == EquipmentSlot::Armor) {
            let art = inv.body_art(None)?;
            Some(Arc::new(
                Appearance::load(vfs, &mut FrameCache::default(), art)
                    .map_err(|e| e.to_string())?,
            ))
        } else {
            None
        };
        Ok(Self { items, appearance })
    }

    pub(super) fn apply(self, w: &mut InteractiveWorld, tile: Tile, binding: &mut Binding) {
        let inv = &mut w.cache.inventory;
        for (slot, item, cell, ground) in self.items {
            let id = item.id;
            inv.wielded.remove(&id);
            let place = if let Some(art) = ground {
                inv.ground.insert(id, art);
                Place::Ground(w.map.clone(), tile)
            } else {
                binding.equipment.push((slot, id));
                Place::Owned(Owner::Player)
            };
            inv.state.put(item, place, cell);
        }
        if let (Some(appearance), Some(player)) = (self.appearance, &mut w.cache.player) {
            player.appearance = appearance;
        }
    }
}

pub(super) fn restore(w: &mut InteractiveWorld, vfs: &mut Vfs, bindings: &[Binding]) {
    for (slot, id) in bindings.iter().flat_map(|b| &b.equipment) {
        let inv = &w.cache.inventory;
        if inv.equipped(*slot).is_none()
            && inv
                .state
                .changes
                .get(id)
                .is_some_and(|c| c.place == Place::Owned(Owner::Player))
            && let Err(error) = w.change_equipment(vfs, *slot, Some(&id.key()), None)
        {
            w.notice = Some(format!("Could not re-equip {}: {error}", slot.name()));
        }
    }
}
