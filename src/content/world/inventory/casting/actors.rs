//! Read and persist health, fatigue, conditions and spell-derived character statistics.
use super::*;

pub(super) fn npc_character(
    defaults: &objects::PrototypeDefaults,
    instance: Option<&objects::Instance<'_>>,
) -> Character {
    let mut pc = Character::default();
    let scalar = |i, base| {
        instance
            .and_then(|o| o.field(i))
            .and_then(FieldValue::scalar)
            .map_or(base, |n| n as i32)
    };
    let array = |field, index, base| {
        instance.and_then(|o| o.field(field)).map_or(base, |a| {
            a.array_entry(index)
                .map(|bytes| i32::from_le_bytes(bytes.try_into().unwrap()))
                .unwrap_or(0)
        })
    };
    for (i, base) in pc.base.iter_mut().enumerate() {
        *base = array(
            66,
            i,
            defaults
                .character
                .as_ref()
                .and_then(|a| a[i])
                .unwrap_or(*base),
        )
    }
    pc.basic_skills = std::array::from_fn(|i| array(67, i, defaults.basic_skills[i] as i32) as u32);
    pc.tech_skills = std::array::from_fn(|i| array(68, i, defaults.tech_skills[i] as i32) as u32);
    pc.colleges = std::array::from_fn(|i| array(69, i, defaults.colleges[i] as i32) as u32);
    pc.portrait = scalar(77, defaults.portrait);
    pc.base_ac = scalar(25, defaults.armor_class.unwrap_or(0));
    pc.critter_flags2 = scalar(65, defaults.appearance.critter_flags2.unwrap_or(0) as i32) as u32;
    pc.hp = std::array::from_fn(|i| scalar(26 + i, defaults.hp[i]));
    pc.fatigue = std::array::from_fn(|i| scalar(70 + i, defaults.fatigue[i]));
    pc.health_damage = scalar(28, defaults.npc_damage[0]);
    pc.fatigue_damage = scalar(72, defaults.npc_damage[1]);
    pc.base_resistance =
        std::array::from_fn(|i| array(30, i, defaults.resistance.and_then(|a| a[i]).unwrap_or(0)));
    let f = instance.map(|o| o.appearance()).unwrap_or_default();
    pc.critter_flags = f
        .critter_flags
        .or(defaults.appearance.critter_flags)
        .unwrap_or_default();
    pc.spell_flags = f
        .spell_flags
        .or(defaults.appearance.spell_flags)
        .unwrap_or_default();
    pc
}
impl Session {
    pub(in crate::content::world) fn displaced_actor(
        &self,
        id: OriginalId,
    ) -> Option<&mobile::population::Resident> {
        self.magick.moved.get(&id).map(|(r, _)| r.as_ref())
    }

    pub(in crate::content::world::inventory) fn spell_deadline(&self) -> Option<u64> {
        self.magick
            .active
            .iter()
            .flat_map(|r| [r.due, r.callback.filter(|_| r.callback_left.is_none())])
            .flatten()
            .min()
    }
    pub(in crate::content::world) fn program(&self, id: usize) -> Result<Arc<Program>, String> {
        if id >= self.rules.progression.spells.len()
            && let Some(item) = self.rules.progression.item_programs.get(&id)
        {
            return item.clone();
        }
        self.rules
            .progression
            .spells
            .get(id)
            .ok_or("Unknown spell")?
            .program
            .clone()
    }
    pub(super) fn refresh_magick(&mut self) {
        self.rules.temporary = self.magick.effects(&Subject::Player, &self.rules);
        let (spell, critter) = self.magick.flags(&Subject::Player);
        self.rules.temporary_critter = critter;
        self.rules.tempo = if spell & 0x04000000 != 0 { 10 } else { 0 };
    }
    pub(in crate::content::world::inventory) fn restore_magick(
        &mut self,
        vfs: &mut Vfs,
    ) -> Result<(), String> {
        for delta in self.magick.actors.values() {
            if let Some(profile) = &delta.advancement {
                let mut pc = Character::default();
                profile.apply(&mut pc);
                pc.validate_progression()?;
            }
        }
        let mut frames = FrameCache::default();
        let mut art = BTreeMap::new();
        let mut ids = BTreeSet::new();
        let mut stuns = BTreeMap::new();
        for run in &self.magick.active {
            let program = self.program(run.spell)?;
            if program.maintain.is_some() && run.due.is_none() && !run.ending {
                return Err("Missing saved spell deadline".into());
            }
            for b in &run.bindings {
                for (id, n) in &b.effects {
                    if *n < 0 || !self.rules.progression.spell_effects.contains_key(id) {
                        return Err("Invalid saved spell effect".into());
                    }
                }
                if let Some(art) = b.stun {
                    stuns.insert(art, stun_clip(vfs, &mut frames, art)?);
                }
                if let Some(form) = b.form {
                    let key = ModelKey::Form(form, b.form_palette);
                    if let std::collections::btree_map::Entry::Vacant(e) = art.entry(key) {
                        e.insert(key.load(vfs, &mut frames)?);
                    }
                }
            }
            for s in &run.summons {
                if s.facing >= 8 || s.visuals.iter().any(|v| {
                    !program.components.iter().any(|c|matches!(c.operation,Operation::Eye{index,..} if run.spell as i32*10+index==v.entry))
                    || v.until.is_some_and(|end|end<v.start)
                    || v.hazard.as_ref().is_some_and(|h| h.count==0 || h.count>u16::MAX as usize || h.frame>=h.count || h.period_ms==0 || h.period_ms>1000)
                }){
                    return Err("invalid saved summon animation".into())
                }
                if s.id.kind != u16::MAX
                    || s.id.payload[8..] != [0; 8]
                    || !ids.insert(s.id)
                    || self.state.changes.contains_key(&s.id)
                    || u64::from_le_bytes(s.id.payload[..8].try_into().unwrap())
                        >= self.state.next_generated
                {
                    return Err("Invalid saved summon identity".into());
                }
                let key = s.model_key(&self.state);
                if let std::collections::btree_map::Entry::Vacant(e) = art.entry(key) {
                    e.insert(key.load(vfs, &mut frames)?);
                }
            }
        }
        let mut indices = BTreeMap::new();
        let mut prototypes = Prototypes::default();
        let mut moved = BTreeMap::new();
        for (id, placement) in &self.magick.offsets {
            let id = parse_id(id)?;
            if placement.sequence >= self.state.next_generated
                || placement.sequence >= (usize::MAX / 2 - 2) as u64
            {
                return Err("Invalid displaced mobile sequence".into());
            }
            let mut payload = [0; 16];
            payload[..8].copy_from_slice(&placement.sequence.to_le_bytes());
            let sequence = OriginalId {
                kind: u16::MAX,
                payload,
            };
            if !ids.insert(sequence) || self.state.changes.contains_key(&sequence) {
                return Err("Duplicate runtime object sequence".into());
            }
            if let Some(prototype) = placement.prototype {
                if id != sequence {
                    return Err("Invalid generated actor identity".into());
                }
                let r = mobile::population::Resident::generated_npc(
                    id,
                    prototype,
                    placement.source,
                    vfs,
                    &mut prototypes,
                )?;
                let art = r.current_art.value().ok_or("Generated actor has no ART")?;
                let dir = self.magick.facing(id).unwrap_or(((art >> 11) & 7) as u8);
                let frame = standing(vfs, &mut frames, (art & !(7 << 11)) | u32::from(dir) << 11)?;
                moved.insert(id, (Arc::new(r), frame));
                continue;
            }
            if id.kind != 2 {
                return Err("Invalid original mobile identity".into());
            }
            let index = indices
                .entry(placement.origin().to_owned())
                .or_insert_with(|| Index::load(vfs, placement.origin()));
            if index.category(id) != Some(16) {
                return Err("Displaced mobile missing from original map".into());
            }
            let prepared = mobile::prepare(
                vfs,
                &mut frames,
                mobile::Prepare {
                    index,
                    prototypes: &mut prototypes,
                    excluded: &BTreeSet::new(),
                    sector: tile_sector_id(placement.source),
                    reference: placement.source,
                    lighting: false,
                    fidget: false,
                    walking: false,
                },
            );
            let r = prepared
                .objects
                .into_iter()
                .find(|r| r.id == id && !r.conflict && r.tile == Some(placement.source))
                .ok_or("Displaced mobile original record mismatch")?;
            let frame = standing(vfs, &mut frames, {
                let art = r
                    .current_art
                    .value()
                    .ok_or("Displaced mobile ART unavailable")?;
                self.magick
                    .facing(id)
                    .map_or(art, |dir| (art & !(7 << 11)) | u32::from(dir) << 11)
            })?;
            moved.insert(id, (Arc::new(r), frame));
        }
        self.origin_indices.extend(
            indices
                .into_iter()
                .filter(|(map, _)| {
                    self.magick
                        .offsets
                        .values()
                        .any(|p| p.origin_map.as_deref() == Some(map))
                })
                .map(|(m, i)| (m, Arc::new(std::sync::OnceLock::from(i)))),
        );
        self.magick.moved = moved;
        self.magick.stun = stuns;
        self.magick.art = art;
        self.refresh_magick();
        Ok(())
    }
    pub(super) fn apply_actor_changes(&self, id: OriginalId, pc: &mut Character) {
        if let Some(d) = self.magick.actors.get(&id.key()) {
            if let Some(advancement) = &d.advancement {
                advancement.apply(pc);
            }
            pc.health_damage = d.health.unwrap_or(pc.health_damage);
            pc.fatigue_damage = d.fatigue.unwrap_or(pc.fatigue_damage);
            pc.base[24] = d.poison.unwrap_or(pc.base[24]);
            pc.critter_flags = (pc.critter_flags | d.critter_on) & !d.critter_off;
            pc.spell_flags |= d.spell_on;
        }
        let (s, c) = self.magick.flags(&Subject::Mobile(id));
        pc.spell_flags |= s;
        pc.critter_flags |= c;
    }
    pub(in crate::content::world) fn npc_vitals(
        &self,
        id: OriginalId,
        defaults: &objects::PrototypeDefaults,
        instance: &objects::Instance<'_>,
    ) -> (Character, CharacterStatistics) {
        let mut pc = npc_character(defaults, Some(instance));
        self.apply_actor_changes(id, &mut pc);
        let rules = Rules {
            tempo: if self.rules.tempo == 0 {
                0
            } else if pc.spell_flags & 0x04000000 != 0 {
                10
            } else {
                -10
            },
            temporary: self.magick.effects(&Subject::Mobile(id), &self.rules),
            ..Rules::default()
        };
        let stats = self.npc_statistics(id, &pc, rules);
        (pc, stats)
    }
    pub(in crate::content::world::inventory) fn npc_statistics(
        &self,
        id: OriginalId,
        pc: &Character,
        mut rules: Rules,
    ) -> CharacterStatistics {
        let mut armor = 0;
        let mut resistance = [0; 5];
        for c in self.state.changes.values() {
            let worn = matches!(c.place,Place::WornBy(_,who,_) if who==id);
            if worn && pc.spell_flags & 0x2003c == 0 {
                rules.worn.extend(
                    c.item
                        .stats
                        .spells
                        .iter()
                        .map(|s| s.unwrap_or(10000))
                        .take_while(|&s| s != 10000)
                        .filter_map(|s| self.rules.progression.worn_spells.get(&(s & 255)))
                        .flatten()
                        .cloned(),
                );
            }
            if (worn || matches!(c.place,Place::Owned(Owner::OriginalObject(_,who)) if who==id))
                && let (Some(carry), Some(value)) = (&c.item.carry, c.item.insert_stat)
            {
                rules.worn.extend(std::iter::repeat_n(
                    carry.adjustment.clone(),
                    usize::from(value / carry.divisor),
                ));
            }
        }
        let aptitude = pc.aptitude(&rules);
        for (item, (program, effect)) in &self.wielded {
            if let Some(c) = self.state.changes.get(item)
                && matches!(c.place,Place::WornBy(_,who,_) if who==id)
                && pc.spell_flags & 0x2003c == 0
            {
                rules.worn.extend(std::iter::repeat_n(
                    effect.clone(),
                    program.count(c.item.stats.complexity.unwrap_or(0), aptitude),
                ));
            }
        }
        for c in self.state.changes.values().filter(|c| {
            pc.spell_flags & 0x2003c == 0
                && matches!(c.place,Place::WornBy(_,who,_) if who==id)
                && c.item.category == 7
        }) {
            let item = &c.item.stats;
            armor += i64::from(item.armor_class.unwrap_or_default())
                + item.adjust_magic(item.magic_armor_class, aptitude);
            for (i, r) in resistance.iter_mut().enumerate() {
                *r += i64::from(item.resistance.unwrap_or_default()[i].unwrap_or(0))
                    + item.adjust_magic(item.magic_resistance.unwrap_or_default()[i], aptitude);
            }
        }
        pc.statistics(&rules, Some(0), armor, resistance, false)
    }
    pub(in crate::content::world::inventory) fn tick_actor_poison(&mut self, now: u64) -> bool {
        let mut hits = Vec::new();
        let mut changed = false;
        for (key, d) in &mut self.magick.actors {
            let Some(timer) = d.poison_timer.as_mut().filter(|t| t.due() <= now) else {
                continue;
            };
            let (Some((rate, maximum, flags)), Some(level), Ok(id)) =
                (d.poison_stats, d.poison.as_mut(), parse_id(key))
            else {
                d.poison_timer = None;
                continue;
            };
            let damage = timer.advance(level, rate, now);
            if *level == 0 {
                d.poison_timer = None;
            }
            if damage > 0 && !d.dead {
                let mut t = Target::new(Subject::Mobile(id), (0, 0), None, Kind::Actor);
                t.maximum = maximum;
                t.hp = d.health.unwrap_or(0);
                t.critter2 = flags;
                hits.push((t, damage));
            }
            changed = true;
        }
        for (mut t, damage) in hits {
            let before = t.hp;
            self.change_vitals(&mut t, Channel::Normal, damage);
            self.note_death(&t, before, None);
        }
        changed
    }

    pub(in crate::content::world::inventory) fn change_vitals(
        &mut self,
        t: &mut Target,
        channel: Channel,
        delta: i32,
    ) {
        let previous_hp = t.hp;
        let previously_alive = previous_hp < t.maximum;
        let v = match channel {
            Channel::Poison => &mut t.poison,
            Channel::Fatigue => &mut t.fatigue,
            _ => &mut t.hp,
        };
        *v = v.saturating_add(delta).max(0);
        match &t.subject {
            Subject::Player => match channel {
                Channel::Poison => {
                    *v = (*v).min(1000);
                    self.character.base[24] = *v;
                    crate::content::character::PoisonTimer::synchronize(
                        &mut self.character.poison_timer,
                        *v,
                        self.game_second,
                    );
                }
                Channel::Fatigue => {
                    self.character.fatigue_damage = *v;
                    if *v > 0 {
                        self.character
                            .recover_at
                            .get_or_insert(self.game_second.saturating_add(80));
                    } else {
                        self.character.recover_at = None;
                    }
                }
                _ => self.character.health_damage = *v,
            },
            Subject::Mobile(id) => {
                if *v >= t.maximum
                    && t.maximum > 0
                    && !matches!(channel, Channel::Poison | Channel::Fatigue)
                {
                    // CE sub_459EA0 releases the caster's Familiar restriction on death.
                    for run in &mut self.magick.active {
                        if run
                            .summons
                            .iter()
                            .any(|s| s.id == *id && s.npc_flags & 0x8000 != 0)
                        {
                            for b in run
                                .bindings
                                .iter_mut()
                                .filter(|b| b.target == Subject::Player)
                            {
                                b.spell &= !0x40000000;
                            }
                        }
                    }
                }
                let d = self.magick.actors.entry(id.key()).or_default();
                match channel {
                    Channel::Poison => {
                        *v = (*v).min(1000);
                        d.poison = Some(*v);
                        d.health.get_or_insert(previous_hp);
                        if let Some(s) = &t.stats {
                            d.poison_stats = Some((s.poison_recovery, t.maximum, t.critter2));
                        }
                        crate::content::character::PoisonTimer::synchronize(
                            &mut d.poison_timer,
                            *v,
                            self.game_second,
                        );
                    }
                    Channel::Fatigue => {
                        d.fatigue = Some(*v);
                        if *v > 0 && t.critter & 0x04000004 == 0 {
                            d.heal_rate = t.stats.as_ref().map_or(1, |s| s.heal_rate.max(1));
                            d.recover_at
                                .get_or_insert(self.game_second.saturating_add(80));
                        } else {
                            d.recover_at = None;
                        }
                    }
                    _ => {
                        d.health = Some(*v);
                        d.dead = *v >= t.maximum && t.maximum > 0;
                        if d.dead {
                            d.poison_timer = None;
                        }
                        if d.dead && previously_alive {
                            // CE combat_dmg selects the torso death for untargeted hits over 20 HP.
                            d.death_anim = Some(if delta > 20 { 18 } else { 7 });
                            d.decay_at = (t.critter2 & 0x40 == 0)
                                .then(|| self.game_second.saturating_add(86400));
                            self.combat.falls.insert(*id);
                        } else if !d.dead {
                            if !previously_alive {
                                self.combat.clear_actor_pose(*id);
                            }
                            d.death_anim = None;
                            d.decay_at = None;
                            self.combat.falls.remove(id);
                        }
                    }
                }
            }
            Subject::Item(id)
                if t.item.is_some() && !matches!(channel, Channel::Fatigue | Channel::Poison) =>
            {
                let item = t.item.as_mut().unwrap();
                item.stats.hp[2] = *v;
                let broken = *v >= t.maximum && t.maximum > 0;
                let previous = self.state.changes.get(id);
                let place = if broken {
                    Place::Consumed
                } else {
                    self.state.place(item).clone()
                };
                let slot = if broken {
                    None
                } else {
                    previous.and_then(|c| c.slot)
                };
                self.state.put(item.clone(), place, slot);
                if broken {
                    self.ground.remove(id);
                }
            }
            _ => (),
        }
    }
    pub(super) fn permanent_flags(&mut self, t: &Subject, field: Field, mask: u32, on: bool) {
        match t {
            Subject::Player => {
                let bits = match field {
                    Field::Critter => &mut self.character.critter_flags,
                    Field::Spell => &mut self.character.spell_flags,
                    _ => return,
                };
                *bits = if on { *bits | mask } else { *bits & !mask }
            }
            Subject::Mobile(id) => {
                let d = self.magick.actors.entry(id.key()).or_default();
                match field {
                    Field::Critter => {
                        if on {
                            d.critter_on |= mask;
                            d.critter_off &= !mask
                        } else {
                            d.critter_off |= mask;
                            d.critter_on &= !mask
                        }
                    }
                    Field::Spell => {
                        if on {
                            d.spell_on |= mask
                        } else {
                            d.spell_on &= !mask
                        }
                    }
                    _ => (),
                }
            }
            _ => (),
        }
    }
}

impl InteractiveWorld {
    /// Keep the next walking edges available even when the camera or leader leaves a follower's sector.
    pub(in crate::content::world) fn party_navigation_tiles(
        &self,
    ) -> impl Iterator<Item = Tile> + '_ {
        let magick = &self.cache.inventory.magick;
        let recruited = self
            .cache
            .scripts
            .followers
            .iter()
            .filter(|(_, m)| !m.waiting && !m.dead)
            .filter_map(|(key, _)| magick.offsets.get(key));
        let controlled = magick
            .active
            .iter()
            .flat_map(|r| &r.bindings)
            .filter_map(|b| match b.target {
                Subject::Mobile(id) if b.spell & 0x08000000 != 0 => magick.offsets.get(&id.key()),
                _ => None,
            });
        recruited
            .chain(controlled)
            .filter(|p| p.map == self.map)
            .map(|p| p.tile)
            .chain(
                magick
                    .active
                    .iter()
                    .filter(|r| !r.ending)
                    .flat_map(|r| &r.summons)
                    .filter(|s| {
                        s.map == self.map
                            && !s.hidden
                            && !magick.down(s.id)
                            && magick
                                .art
                                .get(&s.model_key(&self.cache.inventory.state))
                                .is_some_and(|model| model.actor)
                    })
                    .map(|s| s.tile),
            )
    }
    pub(in crate::content::world::inventory) fn actor_origin(&self, id: OriginalId) -> &str {
        self.cache
            .inventory
            .magick
            .offsets
            .get(&id.key())
            .map_or(&self.map, Placement::origin)
    }

    pub(in crate::content::world::inventory) fn actor_owner(&self, id: OriginalId) -> Owner {
        Owner::OriginalObject(self.actor_origin(id).to_owned(), id)
    }

    pub(in crate::content::world::inventory) fn actor_records<'a>(
        &'a self,
        vfs: &'a mut Vfs,
        id: OriginalId,
    ) -> Records<'a> {
        let map = self.actor_origin(id);
        if map == self.map {
            return self.records(vfs);
        }
        save::records(
            vfs,
            map,
            self.cache
                .inventory
                .origin_indices
                .get(map)
                .and_then(|i| i.get()),
        )
    }

    pub(in crate::content::world::inventory) fn arrive_with_followers(&self, previous: &mut Self) {
        let Some(pc) = self.cache.player.as_ref() else {
            return;
        };
        let tile = pc.tile();
        let inv = &previous.cache.inventory;
        let mut members: BTreeSet<_> = previous
            .cache
            .scripts
            .followers
            .iter()
            .filter(|(_, f)| !f.waiting)
            .filter_map(|(key, _)| parse_id(key).ok())
            .collect();
        members.extend(
            inv.magick
                .active
                .iter()
                .flat_map(|r| &r.bindings)
                .filter_map(|b| match b.target {
                    Subject::Mobile(id) if b.spell & 0x08000000 != 0 => Some(id),
                    _ => None,
                }),
        );
        for id in members {
            if previous.cache.inventory.magick.down(id) {
                continue;
            }
            if let Some(Aim::Object(key)) =
                InventoryReader::subject_aim(previous, &Subject::Mobile(id))
                && let Some((r, at)) = previous.cache.npc(key)
                && let Some(d) = &r.prototype
            {
                let (_, stats) =
                    previous
                        .cache
                        .inventory
                        .npc_vitals(id, &d.defaults, &r.instance());
                if stats.current_fatigue <= 0 || stats.current_health <= 0 {
                    if let Some(f) = previous.cache.scripts.followers.get_mut(&id.key()) {
                        f.waiting = true;
                    }
                    continue;
                }
                previous.relocate_actor(id, Some(key), at, at);
            }
            if let Some(orders) = previous.follower_orders_mut(id) {
                orders.clear_target();
            }
            let inv = &mut previous.cache.inventory;
            if let Some(p) = inv.magick.offsets.get_mut(&id.key())
                && p.map == previous.map
            {
                if p.map != self.map && p.origin_map.is_none() {
                    inv.origin_indices
                        .insert(p.map.clone(), previous.mobile_index.clone());
                    p.origin_map = Some(p.map.clone());
                }
                p.map.clone_from(&self.map);
                p.tile = tile;
            }
        }
        let inv = &mut previous.cache.inventory;
        let summons: BTreeSet<_> = inv
            .magick
            .following_summons(&previous.map, &inv.state)
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        for spawn in inv
            .magick
            .active
            .iter_mut()
            .flat_map(|r| &mut r.summons)
            .filter(|s| summons.contains(&s.id))
        {
            spawn.orders.clear_target();
            spawn.tile = tile;
            spawn.map.clone_from(&self.map);
        }
        for c in inv.state.changes.values_mut() {
            if let Place::Owned(Owner::OriginalObject(map, id)) | Place::WornBy(map, id, _) =
                &mut c.place
                && summons.contains(id)
            {
                map.clone_from(&self.map);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn harm_difficulty_after_resistance_and_once_only_kill_rewards() {
        use crate::content::Difficulty;
        let table = crate::mes::Mes::parse_text(concat!(
            "{1000}{AoE: Tgt_Object}{1001}{Cost: 5}",
            "{1005}{[Begin], Type: Damage, DmgType: Dmg_Normal, Dmg: 3-40, Dmg_Flags: Scaled}"
        ))
        .unwrap();
        let harm = Program::decode(&table, 0).unwrap();
        for (difficulty, strike, casts) in [
            (Difficulty::Easy, 43, 3),
            (Difficulty::Normal, 29, 4),
            (Difficulty::Hard, 22, 6),
        ] {
            let mut inv = Session {
                difficulty,
                ..Default::default()
            };
            let id = OriginalId {
                kind: 2,
                payload: [1; 16],
            };
            let mut t = Target::new(Subject::Mobile(id), (0, 0), None, Kind::Actor);
            t.maximum = 112;
            t.alignment = 50;
            t.resistance[0] = 28;
            t.worth = [160, 160];
            for n in 1..=casts {
                apply_vital(&mut inv, &mut t, &harm.components[0], 100, 0);
                assert_eq!(t.hp, n * strike);
                assert_eq!(inv.magick.down(id), n == casts);
            }
            let xp = inv.character.base[18];
            assert_eq!(xp, 192);
            assert!(inv.character.base[19] < 0);
            assert_eq!(inv.combat_reward(&t, t.hp, true), 0);
            apply_vital(&mut inv, &mut t, &harm.components[0], 100, 0);
            assert_eq!(inv.character.base[18], xp);
        }
    }

    #[test]
    fn elemental_damage_survives_reload_and_full_healing_reverses_actor_damage() {
        let mut inv = Session::default();
        let id = inv.state.generated_id();
        let mut target = Target::new(Subject::Mobile(id), (0, 0), None, Kind::Actor);
        target.maximum = 80;
        target.resistance[1] = 50;
        let spell = crate::mes::Mes::parse_text(concat!(
            "{1000}{AoE: Tgt_Object}{1001}{Cost: 3}",
            "{1005}{[Begin], Type: Damage, DmgType: Dmg_Fire, Dmg: 8-32, Dmg_Flags: Scaled}",
            "{1006}{[Begin], Type: Heal, Dmg: 0, Dmg_Flags: Full}"
        ))
        .unwrap();
        let program = Program::decode(&spell, 0).unwrap();
        apply_vital(&mut inv, &mut target, &program.components[0], 100, 25);
        inv.change_vitals(&mut target, Channel::Fatigue, 7);
        inv.change_vitals(&mut target, Channel::Poison, 3);
        inv.magick = serde_json::from_value(serde_json::to_value(&inv.magick).unwrap()).unwrap();
        let mut npc = Character::default();
        inv.apply_actor_changes(id, &mut npc);
        assert_eq!(
            (npc.health_damage, npc.fatigue_damage, npc.base[24]),
            (12, 7, 3)
        );
        apply_vital(&mut inv, &mut target, &program.components[1], 100, 0);
        inv.apply_actor_changes(id, &mut npc);
        assert_eq!(
            (npc.health_damage, npc.fatigue_damage, npc.base[24]),
            (0, 0, 3)
        );
        assert_eq!(inv.character.health_damage, 0);
    }
}
