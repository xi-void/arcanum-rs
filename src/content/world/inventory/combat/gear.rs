use super::resolution::Weapon;
use super::*;

impl InventoryReader {
    // Hydrate authored equipment and ammunition once. The same placements are later looted.
    pub(in crate::content::world) fn prepare_gear(
        &mut self,
        w: &mut InteractiveWorld,
        id: OriginalId,
    ) -> Result<(), String> {
        if id.kind != 2 || w.cache.inventory.combat.prepared.contains(&id) {
            return Ok(());
        }
        let t = Self::subject_aim(w, &Subject::Mobile(id)).ok_or("Actor unavailable")?;
        let Aim::Object(key) = t else {
            return Err("Actor unavailable".into());
        };
        let (r, _) = w.cache.npc(key).ok_or("NPC unavailable")?;
        let o = r.instance();
        let d = &r
            .prototype
            .as_ref()
            .ok_or("Missing NPC prototype")?
            .defaults;
        let counts: [u32; 4] = std::array::from_fn(|i| {
            o.field(79 + i)
                .and_then(FieldValue::scalar)
                .unwrap_or(d.npc_ammo[i])
        });
        let (character, stats) = w.cache.inventory.npc_vitals(id, d, &o);
        let mut wearer = Target::new(
            Subject::Mobile(id),
            r.tile.unwrap_or(w.reference),
            Some(key),
            Kind::Actor,
        );
        wearer.actor(&character, &stats);
        let owner = w.actor_owner(id);
        let mut layout = w
            .records(&mut self.vfs)
            .layout(&w.cache.inventory.state, &owner, None)?;
        let mut ammo = [0; 5];
        for (total, qty) in ammo[1..].iter_mut().zip(counts) {
            *total = u64::from(qty);
        }
        for item in layout.items.values().filter(|i| i.category == 6) {
            if let Some(kind) = item.stats.ammo_type.filter(|n| *n < 4) {
                ammo[kind as usize + 1] += u64::from(item.stats.quantity.unwrap_or(0));
            }
        }
        let mut wear = BTreeMap::new();
        for item in layout.items.values() {
            let flags = item.stats.equipment_flags.unwrap_or(0);
            let authored = item
                .authored_slot
                .and_then(|n| EquipmentSlot::ALL.get(n.checked_sub(1000)? as usize))
                .copied();
            let slot = authored.or_else(|| {
                EquipmentSlot::ALL
                    .into_iter()
                    .find(|s| s.accepts(item.category, item.inventory_art, flags))
            });
            if let Some(slot) = slot {
                let value = if slot == EquipmentSlot::Weapon {
                    let Some(value) = w.cache.inventory.weapon_score(&wearer, item, ammo) else {
                        continue;
                    };
                    value
                } else {
                    i64::from(item.worth())
                };
                if authored.is_none()
                    && w.cache
                        .inventory
                        .check_wearer(item, slot, &character, Some(id))
                        .is_err()
                {
                    continue;
                }
                let rank = (authored.is_some(), value);
                if wear.get(&slot).is_none_or(|(r, _)| *r < rank) {
                    wear.insert(slot, (rank, item.clone()));
                }
            }
        }
        let mut stock = Vec::new();
        let mut effects = Vec::new();
        for (slot, (_, item)) in wear {
            if let Some(effect) = w.cache.inventory.prepare_wield(&mut self.vfs, &item)? {
                effects.push((item.id, effect));
            }
            layout.grid.items.remove(&item.id);
            stock.push((
                item,
                Place::WornBy(w.actor_origin(id).to_owned(), id, slot),
                None,
            ));
        }
        let mut next = w.cache.inventory.state.next_generated;
        for (i, qty) in counts.into_iter().enumerate().filter(|(_, n)| *n > 0) {
            let mut identity = w.cache.inventory.state.generated_id();
            identity.payload[..8].copy_from_slice(&next.to_le_bytes());
            next = next.checked_add(1).ok_or("Object identity overflow")?;
            let mut item = Session::prototype_item(
                &mut self.vfs,
                &mut self.frames,
                identity,
                [7058, 7059, 7060, 7061][i],
                6,
            )?;
            item.stats.quantity = Some(qty);
            let cell = layout
                .grid
                .fit(identity, item.footprint.cells, owner.rows(), None)
                .ok_or("No room for NPC ammunition")?;
            layout
                .grid
                .items
                .insert(identity, (cell, item.footprint.cells));
            stock.push((item, Place::Owned(owner.clone()), Some(cell)));
        }
        let session = &mut w.cache.inventory;
        session.wielded.extend(effects);
        session.state.next_generated = next;
        for (item, place, cell) in stock {
            session.state.put(item, place, cell)
        }
        session.combat.prepared.insert(id);
        Ok(())
    }
    pub(super) fn ammo_plan(
        &mut self,
        w: &InteractiveWorld,
        actor: Actor,
        weapon: &Weapon,
    ) -> Result<Vec<(Item, u32, usize)>, String> {
        // CE combat_consume_ammo exempts independent NPCs.
        if actor.is_some_and(|id| {
            id.kind == 2
                && !w.cache.scripts.following(&id.key())
                && w.cache.inventory.magick.flags(&Subject::Mobile(id)).0 & 0x08000000 == 0
        }) {
            return Ok(vec![]);
        }
        let Some((kind, mut needed)) = weapon.ammo else {
            return Ok(vec![]);
        };
        let owner = actor.map_or(Owner::Player, |id| w.actor_owner(id));
        let layout = w
            .records(&mut self.vfs)
            .layout(&w.cache.inventory.state, &owner, None)?;
        let mut plan = vec![];
        for item in layout
            .items
            .into_values()
            .filter(|i| i.category == 6 && i.stats.ammo_type == Some(kind))
        {
            let take = needed.min(item.stats.quantity.unwrap_or(0));
            if take > 0
                && let Some(&(slot, _)) = layout.grid.items.get(&item.id)
            {
                plan.push((item, take, slot));
                needed -= take;
                if needed == 0 {
                    break;
                }
            }
        }
        if needed != 0 {
            return Err("Out of ammunition".into());
        }
        Ok(plan)
    }
    pub(in crate::content::world::inventory) fn corpse_coins(
        &mut self,
        w: &InteractiveWorld,
        id: OriginalId,
    ) -> Result<Option<(Item, u32)>, String> {
        if id.kind != 2 {
            return Ok(None);
        }
        let gold = w.npc_gold(&mut self.vfs, &id.key())?;
        if gold == 0 {
            return Ok(None);
        }
        let mut item = Session::prototype_item(
            &mut self.vfs,
            &mut self.frames,
            w.cache.inventory.state.generated_id(),
            9076,
            8,
        )?;
        item.stats.quantity = Some(gold);
        Ok(Some((item, gold)))
    }
    pub(in crate::content::world) fn open_corpse(
        &mut self,
        w: &mut InteractiveWorld,
        id: OriginalId,
    ) -> Result<(), String> {
        w.check_corpse(id)?;
        self.prepare_gear(w, id)?;
        let owner = w.actor_owner(id);
        let mut grid = w
            .records(&mut self.vfs)
            .layout(&w.cache.inventory.state, &owner, None)?
            .grid;
        let mut loot = Vec::new();
        for c in w
            .cache
            .inventory
            .state
            .changes
            .values()
            .filter(|c| matches!(c.place,Place::WornBy(_,a,_) if a==id))
        {
            let slot = grid
                .fit(c.item.id, c.item.footprint.cells, owner.rows(), None)
                .ok_or("No space to unpack corpse equipment")?;
            grid.items.insert(c.item.id, (slot, c.item.footprint.cells));
            loot.push((c.item.clone(), slot));
        }
        if let Some((item, gold)) = self.corpse_coins(w, id)? {
            let slot = grid
                .fit(item.id, item.footprint.cells, owner.rows(), None)
                .ok_or("No space for corpse coins")?;
            w.adjust_npc_gold(&mut self.vfs, &id.key(), -i64::from(gold))?;
            w.cache.inventory.state.next_generated += 1;
            loot.push((item, slot));
        }
        for (item, slot) in loot {
            w.cache
                .inventory
                .state
                .put(item, Place::Owned(owner.clone()), Some(slot));
        }
        w.cache.inventory.revision += 1;
        w.container_request = Some(id);
        Ok(())
    }
}
impl Session {
    pub(super) fn consume_ammo(&mut self, plan: Vec<(Item, u32, usize)>) -> u32 {
        let mut total = 0;
        for (mut item, n, slot) in plan {
            let qty = item.stats.quantity.as_mut().unwrap();
            *qty -= n;
            let (place, slot) = if *qty == 0 {
                (Place::Consumed, None)
            } else {
                (self.state.place(&item).clone(), Some(slot))
            };
            self.state.put(item, place, slot);
            total += n;
        }
        total
    }
}
