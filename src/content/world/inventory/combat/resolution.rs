//! CE skill invocation, item_weapon_damage, combat_calc_dmg and critter kill rewards.
use super::*;

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Weapon {
    pub(super) range: u32,
    pub(super) cost: i32,
    pub(super) art: u8,
    pub(super) ammo: Option<(u32, u32)>,
    pub(super) missile: Option<u32>,
    id: Actor,
    skill: usize,
    hit: i32,
    strength: i32,
    bounds: [[i32; 2]; 5],
    critical: [i32; 2],
    ignore_resistance: bool,
    #[serde(default)]
    aptitude_failure: i32,
}

fn attack_cost(speed: i32) -> i32 {
    match speed {
        25.. => 1,
        21..=24 => 2,
        _ => (8 - speed / 3).max(1),
    }
}

// Original skill_invocation_difficulty truncates half AC before multiplying.
fn hit_chance(
    effectiveness: i32,
    weapon: &Weapon,
    a: &crate::content::character::CharacterStatistics,
    d: &crate::content::character::CharacterStatistics,
    distance: u32,
    awareness: i32,
) -> i64 {
    let base = i64::from(effectiveness);
    let mut chance = base + i64::from(weapon.hit)
        - base * i64::from(d.armor_class / 2) / 100
        - 5 * (i64::from(weapon.strength) - i64::from(a.attributes[0])).max(0)
        + i64::from(awareness);
    if weapon.skill != 2 && a.skills[weapon.skill].training < 3 {
        chance -= 5 * (i64::from(distance) - i64::from(a.attributes[5] / 2)).max(0);
    }
    chance
}

impl Weapon {
    pub(super) fn valid(&self) -> bool {
        matches!(self.skill, 0 | 2 | 3 | 13)
            && self.cost > 0
            && self.bounds.iter().all(|p| p[0] >= 0 && p[1] >= 0)
    }
}
impl Session {
    pub(super) fn worn(&self, actor: Actor, slot: EquipmentSlot) -> Option<&Item> {
        self.equipment_change(actor, slot).map(|c| &c.item)
    }
    pub(super) fn weapon(&self, actor: Actor, t: &Target) -> Weapon {
        let weapon = self
            .worn(actor, EquipmentSlot::Weapon)
            .filter(|_| t.spell & 0x2003c == 0);
        self.weapon_stats(actor, t, weapon)
    }

    pub(super) fn weapon_stats(&self, actor: Actor, t: &Target, weapon: Option<&Item>) -> Weapon {
        let s = t.stats.as_ref().expect("actor statistics");
        let empty = ItemStats::default();
        let item = weapon.map_or(&empty, |i| &i.stats);
        let flags = item.equipment_flags.unwrap_or(0);
        let magic = |v| item.adjust_magic(Some(v), s.aptitude) as i32;
        let skill = if flags & 0x40 != 0 {
            3
        } else if item.ammo_type == Some(0) {
            0
        } else if item.complexity.unwrap_or(0) < 0
            && item.range >= 3
            && matches!(item.ammo_type, Some(1..=3))
        {
            13
        } else {
            2
        };
        let speed = item
            .weapon_speed
            .unwrap_or(10)
            .saturating_add(magic(item.magic_speed))
            .saturating_add(if weapon.is_some() && s.skills[skill].training > 0 {
                5
            } else {
                0
            });
        let cost = attack_cost(speed);
        let gloves = self
            .worn(actor, EquipmentSlot::Gauntlet)
            .map_or(0, |i| i.stats.unarmed_bonus);
        let bounds = std::array::from_fn(|i| {
            let [min, max] = if weapon.is_some() {
                [item.damage[0][i], item.damage[1][i]]
            } else if t.critter & 0x2000800c != 0 {
                [t.natural[i * 2], t.natural[i * 2 + 1]]
            } else if matches!(i, 0 | 4) {
                [(gloves - 25).max(1), gloves.saturating_add(5)]
            } else {
                [0, 0]
            };
            let cap = max
                .saturating_mul(if weapon.is_some() { 3 } else { 2 })
                .max(0);
            let bound = |v: i32| {
                let v = if skill == 2 && matches!(i, 0 | 4) {
                    v.saturating_add(s.damage_bonus).max(i32::from(v > 0))
                } else {
                    v
                };
                v.saturating_add(magic(item.damage[2][i])).clamp(0, cap)
            };
            [bound(min), bound(max)]
        });
        let shield = t.spell & 0x2003c == 0 && self.worn(actor, EquipmentSlot::Shield).is_some();
        let strength = item
            .weapon_min_strength
            .unwrap_or(1)
            .saturating_add(magic(item.magic_strength))
            .saturating_add(if flags & 8 != 0 {
                0
            } else if flags & 4 != 0 && shield {
                4
            } else if flags & 4 == 0 && !shield {
                -2
            } else {
                0
            });
        Weapon {
            id: weapon.map(|i| i.id),
            skill,
            art: item.use_art.map_or(1, |n| ((n >> 6) & 15) as u8),
            range: item.range.saturating_add(magic(item.magic_range)).max(1) as u32,
            hit: item
                .weapon_hit_bonus
                .unwrap_or(0)
                .saturating_add(magic(item.magic_hit)),
            strength,
            cost,
            bounds,
            ammo: item
                .ammo_type
                .filter(|&n| n < 4 && item.consumption > 0)
                .map(|n| (n, item.consumption as u32)),
            missile: item.missile,
            critical: item.magic_critical.map(magic),
            aptitude_failure: if skill == 3 {
                0
            } else {
                (-i64::from(item.complexity.unwrap_or(0).min(0)) * i64::from(s.aptitude.max(0))
                    / 100)
                    .min(100) as i32
            },
            ignore_resistance: flags & 0x80 != 0,
        }
    }

    /// CE item_wield_best compares the last damage channel for each eligible weapon.
    pub(in crate::content::world::inventory) fn weapon_score(
        &self,
        wielder: &Target,
        item: &Item,
        counts: [u64; 5],
    ) -> Option<i64> {
        let actor = match wielder.subject {
            Subject::Mobile(id) => Some(id),
            _ => None,
        };
        let weapon = self.weapon_stats(actor, wielder, Some(item));
        if weapon
            .ammo
            .is_some_and(|(kind, need)| u64::from(need) > counts[kind as usize + 1])
        {
            return None;
        }
        let skill = wielder.stats.as_ref()?.skills[weapon.skill];
        let effectiveness = if weapon.skill != 2 && skill.level == 0 {
            -1
        } else {
            self.attack_rating(wielder, wielder, &weapon).0
        };
        let [min, max] = weapon.bounds[4];
        Some((i64::from(min) + i64::from(max)) / 2 * i64::from(effectiveness) / 100)
    }

    fn attack_rating(&self, a: &Target, t: &Target, w: &Weapon) -> (i32, i64) {
        let s = a.stats.as_ref().unwrap();
        let d = t.stats.as_ref().unwrap();
        let skill = s.skills[w.skill];
        let mut effectiveness = 25
            + skill.level * if w.skill == 3 { 7 } else { 5 }
            + if s.attributes[4] >= 20 { 10 } else { 0 };
        if w.skill == 13 && t.spell & 0x00400000 != 0 {
            effectiveness -= 20
        }
        if a.subject == Subject::Player {
            effectiveness = self.difficulty.adjust(effectiveness);
        }
        let awareness = if t.critter & 0x40 != 0 || d.current_fatigue <= 0 {
            50
        } else if t.critter & 0x20 != 0 {
            30
        } else {
            0
        };
        let chance = hit_chance(effectiveness, w, s, d, distance(a.tile, t.tile), awareness);
        (effectiveness, chance)
    }

    pub(super) fn strike(
        &mut self,
        a: &mut Target,
        t: &mut Target,
        w: &Weapon,
        ammunition: u32,
    ) -> Value {
        let d = t.stats.as_ref().unwrap();
        let skill = a.stats.as_ref().unwrap().skills[w.skill];
        let (effectiveness, chance) = self.attack_rating(a, t, w);
        let malfunction =
            w.aptitude_failure > 0 && self.skill_random.below(100) < w.aptitude_failure as u64;
        let roll = 1 + self.skill_random.below(100) as i64;
        let crit_roll = 1 + self.skill_random.below(100) as i64;
        let mut hit = !malfunction && roll <= chance;
        let modifiers = if a.subject == Subject::Player {
            self.rules.pick_modifiers()
        } else {
            [0; 3]
        };
        let mut critical = malfunction
            || if hit {
                crit_roll <= i64::from(effectiveness / 20) + i64::from(w.critical[0]) + modifiers[1]
            } else {
                !(w.skill == 2 && skill.training == 3)
                    && crit_roll
                        <= (i64::from((100 - effectiveness) / 7)
                            + i64::from(w.critical[1])
                            + modifiers[2])
                            .max(2)
            };
        let dodge_base = 5 * d.skills[1].level + if d.attributes[4] >= 20 { 10 } else { 0 };
        let dodge_chance = (if t.subject == Subject::Player {
            self.difficulty.adjust(dodge_base)
        } else {
            dodge_base
        })
        .clamp(0, 95);
        let dodge = hit && !t.unable() && self.skill_random.below(100) < dodge_chance as u64;
        if dodge {
            hit = false;
            let trained = [0, 10, 50, 100][d.skills[1].training as usize];
            critical = self.skill_random.below(100) < (dodge_chance / 20) as u64
                && self.skill_random.below(100) < trained;
        }
        let before = t.hp;
        let source_before = a.hp;
        let mut values = [0; 5];
        // CE combat_calc_dmg applies only the first matching unarmed form bonus.
        let form_bonus = [(8, 0, 5), (16, 3, 15), (32, 4, 15), (0x80000000, 0, 2)]
            .into_iter()
            .find(|(flag, _, _)| w.id.is_none() && a.spell & flag != 0);
        let player_attack = a.subject == Subject::Player;
        let fumble = !hit && critical;
        let severity = a.stats.as_ref().unwrap().critical_severity[0];
        if hit || fumble {
            let receiver = if fumble { &mut *a } else { &mut *t };
            let severe = critical && (!fumble || self.skill_random.below(2) == 0);
            let roll_bonus = |random: &mut crate::random::Random, base: i32| {
                random.below(100) < severity.saturating_add(base).clamp(0, 100) as u64
            };
            let bonus = if !severe || !player_attack && receiver.subject == Subject::Player {
                0
            } else if roll_bonus(&mut self.skill_random, 10) {
                200
            } else if roll_bonus(&mut self.skill_random, 30) {
                100
            } else if roll_bonus(&mut self.skill_random, 60) {
                50
            } else {
                0
            };

            for (i, channel) in [
                Channel::Normal,
                Channel::Poison,
                Channel::Electrical,
                Channel::Fire,
                Channel::Fatigue,
            ]
            .into_iter()
            .enumerate()
            {
                let [min, max] = w.bounds[i];
                let mut amount = min
                    + self
                        .skill_random
                        .below((max.saturating_sub(min).max(0) as u64) + 1)
                        as i32;
                if let Some((_, _, bonus)) = form_bonus.filter(|(_, channel, _)| *channel == i) {
                    amount = amount.saturating_add(bonus);
                }
                if i == 1 && (receiver.critter & 0x20000004 != 0 || receiver.spell & 0x10000 != 0) {
                    amount = 0
                }
                if !w.ignore_resistance {
                    amount = channel.after_resistance(amount, &receiver.resistance)
                }
                if receiver.spell & 0x10000 != 0
                    || receiver.critter2 & 0x200 != 0
                    || i == 4 && receiver.critter & 0x04000000 != 0
                {
                    amount = 0
                }
                if i == 4 && receiver.critter2 & 8 != 0 {
                    amount = amount.saturating_mul(4)
                }
                if i == 4 && receiver.critter & 0x80000000 != 0 {
                    amount /= 4
                }
                values[i] = amount;
                if matches!(channel, Channel::Poison | Channel::Fatigue) && amount > 0 {
                    self.change_vitals(receiver, channel, amount);
                }
            }
            let damage = values[0]
                .saturating_add(values[2])
                .saturating_add(values[3]);
            let damage = if player_attack {
                self.difficulty.adjust(damage)
            } else {
                damage
            };
            let damage = (i64::from(damage) * (100 + bonus) / 100).min(i64::from(i32::MAX)) as i32;
            self.change_vitals(receiver, Channel::Normal, damage);
            if !fumble && w.skill == 2 && t.spell & 16 != 0 && a.spell & 16 == 0 {
                let mut burn = Channel::Fire.after_resistance(5, &a.resistance);
                if t.subject == Subject::Player {
                    burn = self.difficulty.adjust(burn);
                }
                self.change_vitals(a, Channel::Fire, burn);
            }
        }
        let actor = |target: &Target| match target.subject {
            Subject::Mobile(id) => Some(id),
            _ => None,
        };
        let attacker = actor(a);
        let defender = actor(t);
        let burn = !fumble && a.hp > source_before;
        self.note_death(t, before, attacker);
        self.note_death(a, source_before, if burn { defender } else { attacker });
        let allied = |actor: Actor| {
            actor.is_none() || self.combat.party.iter().any(|p| p.actor == actor && p.ally)
        };
        let credit = allied(attacker);
        let credit_burn = burn && allied(defender);
        let mut xp = if credit {
            self.combat_reward(t, before, attacker.is_none())
        } else {
            0
        };
        if credit_burn {
            xp = xp.saturating_add(self.combat_reward(a, source_before, defender.is_none()));
        }
        let outcome = match (hit, critical, dodge) {
            (true, true, _) => "Critical hit",
            (true, _, _) => "Hit",
            (_, true, _) => "Critical failure",
            (_, _, true) => "Dodged",
            _ => "Miss",
        };
        json!({"event":"attack","attacker":a.subject,"target":t.subject,"weapon":w.id.map(OriginalId::key),"skill":w.skill,"chance":chance.clamp(0,100),"roll":roll,"critical_roll":crit_roll,"outcome":outcome,"damage":values,"health_damage":t.hp-before,"attacker_health_damage":a.hp-source_before,"xp":xp,"target_hp":t.maximum.saturating_sub(t.hp).max(0),"ammo":ammunition,"cost":w.cost,"missile_art":w.missile})
    }
    pub(in crate::content::world::inventory) fn combat_reward(
        &mut self,
        t: &Target,
        before: i32,
        player_damage: bool,
    ) -> i32 {
        let Subject::Mobile(id) = t.subject else {
            return 0;
        };
        if t.hp <= before
            || before >= t.maximum
            || t.maximum <= 0
            || id.kind == u16::MAX
            || t.spell & 0x10000 != 0
        {
            return 0;
        }
        let pool = self
            .combat
            .pools
            .entry(id.key())
            .or_insert(t.worth.map(|n| n.max(0)));
        let ratio = i64::from(t.hp - before) * 100 / i64::from(t.maximum);
        let mut xp = (i64::from(pool[0]) * ratio / 100).min(i64::from(pool[1])) as i32;
        pool[1] -= xp;
        if !player_damage {
            xp = 0;
        }
        if t.hp >= t.maximum {
            self.combat.leader_kills.push(id);
            xp = xp.saturating_add(pool[0] / 5);
            *pool = [0; 2];
            let alignment = t.alignment;
            let killer = self.character.base[19];
            let signed = if alignment >= 0 || self.statistics().alignment > alignment {
                -alignment
            } else {
                alignment
            };
            let adj = signed / 20;
            // CE sub_45DC90 only applies negative adjustments.
            if adj < 0 {
                let limit = if alignment >= 0 {
                    -100 + alignment
                } else {
                    alignment - 100
                };
                self.character.base[19] = if limit < 0 {
                    (killer + adj).max(limit.min(killer))
                } else {
                    (killer + adj).min(limit.max(killer))
                }
                .clamp(-1000, 1000);
            }
        }
        self.character.gain_experience(xp, &self.rules);
        xp
    }
}

impl InventoryReader {
    pub(super) fn attack_feedback(
        &self,
        w: &mut InteractiveWorld,
        a: &Target,
        t: &Target,
        weapon: &Weapon,
        mut event: Value,
        now: Duration,
    ) {
        let actor = match a.subject {
            Subject::Mobile(id) => Some(id),
            _ => None,
        };
        let victim = match t.subject {
            Subject::Mobile(id) => Some(id),
            _ => None,
        };
        let inv = &mut w.cache.inventory;
        let item = weapon
            .id
            .and_then(|id| inv.state.changes.get(&id))
            .map(|c| &c.item);
        let base = item
            .and_then(|i| i.effective.appearance.sound_effect)
            .filter(|n| *n > 0);
        let variant = event["roll"].as_i64().unwrap_or(0) as i32 & 1;
        let critical = event["outcome"] == "Critical hit";
        let (sound, fallback) = if critical || event["outcome"] == "Hit" {
            let mat = |m: Option<u32>| match m {
                Some(0 | 1) => 3,
                Some(2 | 3) => 4,
                Some(5) => 2,
                Some(6 | 11) => 1,
                _ => 0,
            };
            let mut source = mat(item.and_then(|i| i.effective.appearance.material));
            if source == 2
                && item
                    .is_some_and(|i| i.weight(a.stats.as_ref().map_or(0, |s| s.aptitude)) <= 2000)
            {
                source = 1;
            }
            let target = mat(inv
                .worn(victim, EquipmentSlot::Armor)
                .and_then(|i| i.effective.appearance.material)
                .or(t.material));
            let fallback = 7000 + source * 20 + target * 3 + variant;
            (
                base.map_or(fallback, |n| n + if critical { 6 } else { 4 + variant }),
                fallback,
            )
        } else {
            (base.map_or(4010, |n| n + 3), 4010)
        };
        let due = if weapon.missile.is_some() && weapon.range > 1 {
            now
        } else {
            inv.combat.poses.get(&actor).map_or(now, |p| p.marker())
        };
        inv.combat.sounds.push((due, sound, fallback));
        if critical && let Some(voice) = t.sound.filter(|n| *n > 0) {
            inv.combat.sounds.push((due, voice, voice))
        }
        event["impact_sound"] = json!(sound);
        event["sound_delay_ms"] = json!(due.saturating_sub(now).as_millis());
        inv.record_attack(event);
    }
    /// CE intgame_message_window_display_critter shows percentages for independent actors.
    pub fn hovered_vitals(&mut self, w: &InteractiveWorld) -> Option<HoveredVitals> {
        let t = self.target(w, Aim::Object(w.cache.hover?)).ok()?;
        let s = t.stats.as_ref()?;
        if s.current_health <= 0 {
            return None;
        }
        let percent = |v: i64, max: i64| (v.max(0) * 100 / max.max(1)).clamp(0, 100) as u32;
        Some(HoveredVitals {
            percent: [
                percent(s.current_health, s.max_health),
                percent(s.current_fatigue, s.max_fatigue),
            ],
            poisoned: t.poison > 0,
            allied: t.party || t.spell & 0x08004000 != 0,
            values: [
                s.current_health,
                s.max_health,
                s.current_fatigue,
                s.max_fatigue,
            ],
        })
    }
    pub(in crate::content::world) fn combat_target_hint(
        &mut self,
        w: &InteractiveWorld,
        key: WorldOrder,
    ) -> Option<String> {
        let inv = &w.cache.inventory;
        if !inv.combat.armed {
            return None;
        }
        let target = self.target(w, Aim::Object(key)).ok()?;
        let stats = target.stats.as_ref()?;
        if stats.current_health <= 0 {
            return Some("Dead".into());
        }
        let player = self.target(w, Aim::Player).ok()?;
        let weapon = inv.weapon(None, &player);
        let vital = format!("Level {}", stats.level);
        if !reachable(w, player.tile, target.tile, weapon.range) {
            return Some(format!("{vital}  Out of range"));
        }
        let (_, chance) = inv.attack_rating(&player, &target, &weapon);
        Some(format!("{vital}  To Hit {}%", chance.clamp(0, 100)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_of_fire_unarmed_hit_applies_resisted_fire_to_actor_health() {
        let mut inv = Session::default();
        let mut attacker = Target::new(Subject::Player, (10, 10), None, Kind::Actor);
        let id = OriginalId {
            kind: 2,
            payload: [1; 16],
        };
        let mut defender = Target::new(Subject::Mobile(id), (11, 10), None, Kind::Actor);
        attacker.stats = Some(inv.statistics());
        defender.stats = attacker.stats.clone();
        defender.maximum = 200;
        defender.resistance[1] = 50;
        attacker.spell = 16;
        let mut fists = inv.weapon(None, &attacker);
        fists.hit = 1000;
        fists.critical[0] = -100;
        fists.bounds = [[0; 2]; 5];
        let result = inv.strike(&mut attacker, &mut defender, &fists, 0);
        assert_eq!(result["outcome"], "Hit");
        assert_eq!(result["damage"], json!([0, 0, 0, 8, 0]));
        assert_eq!(result["health_damage"], 8);
        assert_eq!(defender.hp, 8);
        assert_eq!(defender.fatigue, 0);
    }

    #[test]
    fn stock_attack_cost_at_speed_breakpoints() {
        for (speed, expected) in [
            (0, 8),
            (3, 7),
            (10, 5),
            (15, 3),
            (18, 2),
            (20, 2),
            (21, 2),
            (24, 2),
            (25, 1),
            (30, 1),
        ] {
            assert_eq!(attack_cost(speed), expected, "speed {speed}");
        }
    }

    #[test]
    fn stock_hit_chance_applies_strength_half_ac_and_master_range_exemption() {
        let inv = Session::default();
        let mut attacker = inv.statistics();
        attacker.attributes[0] = 8;
        attacker.attributes[5] = 9;
        let mut defender = attacker.clone();
        defender.armor_class = 31;
        let mut bow = Weapon {
            skill: 0,
            strength: 10,
            hit: 3,
            id: None,
            cost: 4,
            range: 12,
            art: 0,
            ammo: Some((0, 1)),
            missile: None,
            bounds: [[0; 2]; 5],
            critical: [0; 2],
            ignore_resistance: false,
            aptitude_failure: 0,
        };
        // CE skill_invocation_difficulty with effectiveness 75, AC penalty 11,
        // two missing ST (10), and three tiles past PE / 2 (15).
        assert_eq!(hit_chance(75, &bow, &attacker, &defender, 7, 0), 42);
        attacker.skills[0].training = 3;
        assert_eq!(hit_chance(75, &bow, &attacker, &defender, 7, 0), 57);
        bow.skill = 2;
        assert_eq!(hit_chance(75, &bow, &attacker, &defender, 1, 50), 107);
    }
}
