//! Resolve the selected item or spell against the live target before committing its cost and effects.
use super::*;
use abilities::{LockId, Method};
const DARK_SIGHT_BACKGROUND: i32 = 78;

impl Session {
    fn available_mana(&self, item: &Item) -> i32 {
        item.stats
            .mana_store
            .saturating_sub(self.recharges.get(&item.id.key()).map_or(0, |e| e.spent))
            .max(0)
    }

    pub(super) fn item_report(&self, item: &Item, place: &Place, slot: Option<usize>) -> Value {
        let mut report = item.report(place, slot);
        if item.stats.mana_store > 0 {
            report["current_mana"] = json!(self.available_mana(item));
            report["mana_recharge_due"] =
                json!(self.recharges.get(&item.id.key()).map(|event| event.due));
        }
        report
    }

    fn pick_tool(&self, id: OriginalId) -> Result<&Item, String> {
        let change = self
            .state
            .changes
            .get(&id)
            .filter(|c| c.place == Place::Owned(Owner::Player))
            .ok_or("Lockpicks must be in your backpack")?;
        let item = &change.item;
        if item.category != 14
            || item.stats.equipment_flags.unwrap_or_default() & 2 == 0
            || item.flags & 0x180 != 0x180
        {
            return Err("This item cannot be used as lockpicks".into());
        }
        if !item_visible(item) || item.effective.appearance.use_script.unwrap_or_default() != 0 {
            return Err("Lockpick tool unavailable or requires a Use script".into());
        }
        Ok(item)
    }
}

impl InteractiveWorld {
    pub fn select_tool(&mut self, id: &str) -> Result<(), String> {
        let id = parse_id(id)?;
        self.cancel_player_route();
        let method = Method::PickLocks(id);
        if let Err(reason) = self.cache.inventory.pick_tool(id) {
            self.ability.reject(method, &reason);
            return Err(reason);
        }
        self.ability.select(method);
        Ok(())
    }

    pub fn fatigue(&self) -> (i64, i64) {
        let stats = self.character_statistics();
        (stats.current_fatigue, stats.max_fatigue)
    }

    pub(in crate::content::world) fn unconscious(&self) -> bool {
        let (spell, critter) = self.player_flags();
        spell & 0x10000 != 0
            || critter & 0x60 != 0
            || critter & 4 == 0 && self.fatigue().0 <= 0
            || self.character_statistics().current_health <= 0
    }

    pub fn ability_character(&self) -> Value {
        let pc = &self.cache.inventory.character;
        let stats = self.character_statistics();
        json!({"pick_locks_points":stats.pick_locks.points,"pick_locks_training":stats.pick_locks.training,"pick_locks":stats.pick_locks,
            "magick":self.cache.inventory.magick.report(), "colleges":pc.colleges,"mastery":pc.mastery,"aptitude":stats.aptitude,"fatigue":stats.current_fatigue,
            "fatigue_damage":pc.fatigue_damage,"max_fatigue":stats.max_fatigue,"recovery_due":pc.recover_at})
    }

    pub fn next_ability_second(&self) -> Option<u64> {
        self.cache
            .door_states
            .locks
            .values()
            .filter_map(|lock| lock.relock_at.first().copied())
            .chain(self.cache.inventory.spell_deadline())
            .chain(self.cache.inventory.character.recover_at)
            .chain(
                self.cache
                    .inventory
                    .recharges
                    .values()
                    .map(|event| event.due),
            )
            .min()
    }

    pub fn update_ability_time(&mut self, now: u64) -> bool {
        self.game_second = now;

        let closed = self.cache.door_states.expire_locks(now);
        let inventory = &mut self.cache.inventory;
        let mut changed = false;
        if inventory.character.fatigue_damage > 0 {
            inventory
                .character
                .recover_at
                .get_or_insert(now.saturating_add(80));
        }
        if let Some(due) = inventory.character.recover_at.filter(|due| *due <= now) {
            let ticks = (now - due) / 80 + 1;
            let heal = ticks
                .saturating_mul(inventory.statistics().heal_rate as u64)
                .min(i32::MAX as u64) as i32;
            let pc = &mut inventory.character;
            pc.fatigue_damage = pc.fatigue_damage.saturating_sub(heal).max(0);
            pc.recover_at =
                (pc.fatigue_damage > 0).then(|| due.saturating_add(ticks.saturating_mul(80)));
            changed = true;
        }
        inventory.recharges.retain(|_, event| {
            if event.due <= now {
                let ticks = (now - event.due) / 480 + 1;
                event.spent = event
                    .spent
                    .saturating_sub(ticks.saturating_mul(8).min(i32::MAX as u64) as i32);
                event.due = event.due.saturating_add(ticks.saturating_mul(480));
                changed = true;
            }
            event.spent > 0
        });
        inventory.revision += u64::from(changed || closed);
        closed || changed
    }
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub(super) struct Recharge {
    pub spent: i32,
    pub due: u64,
}

impl InventoryReader {
    pub fn resolve_targeted_action(&mut self, world: &mut InteractiveWorld, now: Duration) -> bool {
        let (key, method, impact) = if let Some((key, method, due)) = world.ability.casting {
            if now < due {
                return false;
            }
            world.ability.casting = None;
            (key, method, true)
        } else if let Some((key, method)) = world.ability.request.take() {
            (key, method, false)
        } else {
            return false;
        };
        let result = match (method, key) {
            (Method::Spell(id), aim) => self.cast_spell(world, id, aim, now, impact),
            (Method::PickLocks(_), abilities::Aim::Object(key)) => {
                self.resolve_action(world, key, method)
            }
            _ => Err("Select a portal or container".into()),
        };
        match result {
            Ok((message, result)) => {
                world.ability.message = message;
                world.ability.last = Some(result);
            }
            Err(e) => world.ability.reject(method, &e),
        }
        true
    }
    fn resolve_action(
        &mut self,
        world: &mut InteractiveWorld,
        key: WorldOrder,
        method: Method,
    ) -> Result<(String, Value), String> {
        let id = world
            .lock_id(key)
            .ok_or("Lock target is no longer available")?;
        let player = world
            .cache
            .player
            .as_ref()
            .ok_or("Player is unavailable")?
            .tile();
        let target = world.cache.target(key).ok_or("Target is unavailable")?;
        let tile = target.tile;
        let (original, difficulty, near, notify) = match &target.action {
            interaction::Action::Door(d) => {
                (d.flags, d.difficulty, d.within_reach(player), d.notify)
            }
            interaction::Action::Window {
                flags,
                difficulty,
                notify,
                ..
            } => (
                *flags,
                *difficulty,
                tile.0.abs_diff(player.0) + tile.1.abs_diff(player.1) <= 1,
                *notify,
            ),
            interaction::Action::Container(id) => {
                let c = world.records(&mut self.vfs).container(*id)?;
                if c.flags & (UNAVAILABLE | CLICK_THROUGH | 0x1000) != 0
                    || (method.is_skill() && c.prototype == 3023 + 20)
                {
                    return Err("Unavailable lock target".into());
                }
                let cell =
                    |t| SectorCache::player_cell(&world.cache.entries, &world.cache.door_states, t);
                (
                    c.subtype_flags,
                    c.difficulty,
                    collision::can_reach_container(player, tile, cell),
                    c.notify,
                )
            }
            _ => return Err("Select a portal or container".into()),
        };
        let flags = world.cache.door_states.flags(id, original);
        let stats = world.character_statistics();
        if world.unconscious() {
            return Err("Unconscious from fatigue".into());
        }
        let target = LockTarget {
            id,
            key,
            difficulty,
            flags,
            near,
        };
        let mut result = json!({"method":method,"target":id.label(),"difficulty":difficulty,"flags_before":flags,"fatigue_cost":0,"notify_npc":notify});
        let Method::PickLocks(tool) = method else {
            return Err("Expected lockpicks".into());
        };
        let message = pick_lock(world, &target, &stats, tool, &mut result)?;
        result["flags_after"] = json!(world.cache.door_states.flags(id, original));
        world.cache.inventory.revision += u64::from(result["outcome"] != "casting");
        Ok((message.into(), result))
    }
}

struct LockTarget {
    key: WorldOrder,
    id: LockId,

    difficulty: i32,
    flags: u32,
    near: bool,
}

fn pick_lock(
    world: &mut InteractiveWorld,
    target: &LockTarget,
    stats: &CharacterStatistics,
    tool: OriginalId,
    result: &mut Value,
) -> Result<&'static str, String> {
    let LockTarget {
        id,
        key,
        difficulty,
        flags,
        near,
        ..
    } = *target;
    let locked = interaction::locked(flags, world.cache.hour);
    let pc = &world.cache.inventory.character;
    let now = world.game_second;
    if !near {
        return Err("Cannot reach the lock".into());
    }
    let inventory = &world.cache.inventory;
    let item = inventory.pick_tool(tool)?;
    let training = stats.pick_locks.training;
    let [_, hit, miss] = inventory.rules.pick_modifiers();
    let level = stats.pick_locks.level;
    let effectiveness = i64::from(stats.pick_locks.effectiveness);
    let mut penalty = if training == 3 && difficulty > 0 {
        difficulty / 2
    } else {
        difficulty
    } as i64
        - i64::from(item.stats.usage_bonus);
    let mut light = 0;
    if training < 2 && pc.critter_flags & 4 == 0 {
        let index = world
            .cache
            .ordered
            .binary_search_by_key(&key, |raster| raster.key)
            .map_err(|_| "Lock light sample is unavailable")?;
        let color = match world.cache.shades.get(index).map(PreparedShade::modulation) {
            Some(Modulation::Uniform(rgb)) => rgb,
            Some(Modulation::Unlit) => [255; 3],
            _ => return Err("Lock light sample is unavailable".into()),
        };
        light = 30 * (255 - i64::from(crate::shadow::grayscale(color))) / 255;
        light = match pc.critter_flags & 0x780000 {
            n if n >= 0x400000 => light / 4,
            n if n >= 0x200000 => light / 3,
            n if n >= 0x100000 => light / 2,
            n if n >= 0x80000 => 2 * light / 3,
            _ => light,
        };
        if pc.critter_flags2 & 0x800 != 0 || pc.background == DARK_SIGHT_BACKGROUND {
            light = 30 - light;
        }
    }
    penalty += light;
    if pc.critter_flags & 0x80 != 0 {
        penalty += 30;
    }
    penalty += if pc.critter_flags & 0x200 != 0 {
        50
    } else if pc.critter_flags & 0x100 != 0 {
        20
    } else {
        0
    };
    let [hp, adj, damage] = item.stats.hp;
    let maximum = i64::from(hp) + i64::from(adj);
    let condition = if maximum > 0 {
        50 * i64::from(damage).max(0) / maximum
    } else {
        0
    };
    result["skill"] = json!({"level":level,"training":training,"effectiveness":effectiveness,"tool":tool.key(),"tool_prototype":item.prototype,
                    "tool_source":super::super::super::diagnostics::source(&item.prototype_source),"tool_bonus":item.stats.usage_bonus,"difficulty_modifier":penalty,"light_penalty":light});
    let rng = &mut world.cache.inventory.skill_random;
    let roll = 1 + rng.below(100) as i64;
    let critical_roll = 1 + rng.below(100) as i64;
    let success = roll + penalty <= effectiveness;
    let critical = critical_roll
        <= if success {
            effectiveness / 2 + hit
        } else {
            ((100 - effectiveness) / 2 + condition + miss).max(2)
        };
    result["skill"]["roll"] = json!(roll);
    result["skill"]["critical_roll"] = json!(critical_roll);
    result["critical"] = json!(critical);
    if locked && result["notify_npc"] != 0 {
        result["guard_notice"] = json!({"loudness":"silent","failed":!success});
    }
    let message = if success {
        let change = world.cache.door_states.locks.entry(id).or_default();
        change.locked = Some(!locked);
        if locked {
            change.relock_at.insert(now.saturating_add(3600));
        }
        result["outcome"] = json!("success");
        if locked {
            "Picked the lock"
        } else {
            "Locked the target"
        }
    } else if critical && locked {
        let change = world.cache.door_states.locks.entry(id).or_default();
        change.jammed = Some(true);
        if flags & 2 == 0 {
            change.relock_at.insert(now.saturating_add(86400));
        }
        result["outcome"] = json!("jammed");
        "Critical failure — lock jammed"
    } else {
        result["outcome"] = json!("failed");
        "Lockpicking failed"
    };
    Ok(message)
}

impl Session {
    pub(super) fn charge_spell(
        &mut self,
        college: u32,
        base_cost: i32,
        fatigue: i64,
        now: u64,
        commit: bool,
    ) -> Result<(i32, i32), String> {
        let pc = &self.character;
        let cost = |mut n: i32| {
            if pc.race() == 1 {
                n = n.saturating_mul(2);
            }
            if n > 0 && pc.critter_flags & 0x80000000 != 0 {
                n = (n / 2).max(1);
            }
            n
        };
        let check = cost(base_cost);
        let charge = cost(if pc.mastery == Some(college) {
            let half = base_cost / 2;
            half + (half & 1)
        } else {
            base_cost
        });
        let inventory = self;
        let stores: Vec<_> = EquipmentSlot::ALL
            .into_iter()
            .filter_map(|slot| {
                let item = &inventory.equipped(slot)?.item;
                let key = item.id.key();
                let available = inventory.available_mana(item);
                (available > 0).then_some((key, available))
            })
            .collect();
        let available = stores.iter().map(|(_, v)| i64::from(*v)).sum::<i64>();
        if fatigue - (i64::from(check) - available).max(0) <= -15 {
            return Err("Not enough energy".into());
        }
        let mut remaining = charge;
        for (key, available) in stores {
            if remaining == 0 {
                break;
            }
            let used = remaining.min(available);
            if commit {
                inventory
                    .recharges
                    .entry(key)
                    .or_insert(Recharge {
                        spent: 0,
                        due: now.saturating_add(480),
                    })
                    .spent += used;
            }
            remaining -= used;
        }
        let pc = &mut inventory.character;
        if commit && remaining > 0 {
            pc.fatigue_damage = pc.fatigue_damage.saturating_add(remaining);
            pc.recover_at.get_or_insert(now.saturating_add(80));
        }

        Ok((remaining, charge - remaining))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fatigue_preflight_is_atomic_and_mastery_changes_the_actual_charge() {
        let mut inventory = Session::default();
        inventory.character.base[27] = 1;
        inventory.character.mastery = Some(0);
        assert!(inventory.charge_spell(0, 10, 5, 1, true).is_err());
        assert_eq!(inventory.character.fatigue_damage, 0);
        assert_eq!(inventory.character.recover_at, None);
        assert_eq!(inventory.charge_spell(0, 10, 6, 1, false).unwrap(), (12, 0));
        assert_eq!(inventory.character.fatigue_damage, 0);
        assert_eq!(inventory.character.recover_at, None);
        assert_eq!(inventory.charge_spell(0, 10, 6, 1, true).unwrap(), (12, 0));
        assert_eq!(inventory.character.recover_at, Some(81));
        inventory.character.critter_flags |= 0x80000000;
        assert_eq!(inventory.charge_spell(0, 10, 6, 20, true).unwrap(), (6, 0));
        assert_eq!(inventory.character.fatigue_damage, 18);
        assert_eq!(inventory.character.recover_at, Some(81));
    }
}
