//! Select an NPC's learned, direct-action spells from the installed AI lists.
use super::*;

const MIND_CONTROLLED: u32 = 0x08000000;

impl State {
    pub(in crate::content::world::inventory) fn controlled_orders(
        &self,
    ) -> impl Iterator<Item = (OriginalId, &FollowerOrders)> {
        self.active
            .iter()
            .filter(|a| !a.ending && a.caster.is_none())
            .flat_map(|a| {
                a.summons
                    .iter()
                    .filter(|s| s.spell_flags & MIND_CONTROLLED != 0)
                    .map(|s| (s.id, &s.orders))
                    .chain(a.bindings.iter().filter_map(|b| match b.target {
                        Subject::Mobile(id) if b.spell & MIND_CONTROLLED != 0 => {
                            Some((id, &b.orders))
                        }
                        _ => None,
                    }))
            })
    }

    fn controlled_orders_mut(&mut self, id: OriginalId) -> Option<&mut FollowerOrders> {
        for a in self
            .active
            .iter_mut()
            .filter(|a| !a.ending && a.caster.is_none())
        {
            if let Some(s) = a
                .summons
                .iter_mut()
                .find(|s| s.id == id && s.spell_flags & MIND_CONTROLLED != 0)
            {
                return Some(&mut s.orders);
            }
            if let Some(b) = a
                .bindings
                .iter_mut()
                .find(|b| b.target == Subject::Mobile(id) && b.spell & MIND_CONTROLLED != 0)
            {
                return Some(&mut b.orders);
            }
        }
        None
    }
}

impl InteractiveWorld {
    pub(in crate::content::world::inventory) fn follower_orders(
        &self,
        id: OriginalId,
    ) -> Option<&FollowerOrders> {
        self.cache
            .scripts
            .followers
            .get(&id.key())
            .filter(|f| !f.waiting && !f.dead)
            .map(|f| &f.orders)
            .or_else(|| {
                self.cache
                    .inventory
                    .magick
                    .controlled_orders()
                    .find(|(actor, _)| *actor == id)
                    .map(|(_, o)| o)
            })
    }

    pub(in crate::content::world::inventory) fn follower_orders_mut(
        &mut self,
        id: OriginalId,
    ) -> Option<&mut FollowerOrders> {
        self.cache
            .scripts
            .followers
            .get_mut(&id.key())
            .filter(|f| !f.waiting && !f.dead)
            .map(|f| &mut f.orders)
            .or_else(|| self.cache.inventory.magick.controlled_orders_mut(id))
    }
}

impl InventoryReader {
    pub(in crate::content::world::inventory) fn follower_cast(
        &mut self,
        w: &mut InteractiveWorld,
        id: OriginalId,
        enemy: Option<OriginalId>,
        now: Duration,
    ) -> bool {
        let Some(Aim::Object(key)) = Self::subject_aim(w, &Subject::Mobile(id)) else {
            return false;
        };
        let Some((resident, _)) = w.cache.npc(key) else {
            return false;
        };
        let Some(d) = resident.prototype.as_ref().map(|p| &p.defaults) else {
            return false;
        };
        let packet = resident
            .instance()
            .field(130)
            .and_then(FieldValue::scalar)
            .unwrap_or(d.ai_packet);
        let npc_flags = resident
            .instance()
            .field(128)
            .and_then(FieldValue::scalar)
            .or(d.npc_flags)
            .unwrap_or(0);
        let Some(params) = w.progression_rules().ai_packets.get(&packet).copied() else {
            return false;
        };
        let Ok(mut caster) = self.target(w, Aim::Object(key)) else {
            return false;
        };
        let Some(stats) = caster.stats.as_ref() else {
            return false;
        };
        if stats.current_health <= 0
            || stats.current_fatigue <= 0
            || stats.attributes[4] < 5
            || caster.spell & 0x1800 != 0
            || caster.critter & 0x70 != 0
            || w.cache
                .inventory
                .combat
                .check(Some(id), 4, stats.current_fatigue)
                .is_err()
            || w.cache
                .inventory
                .magick
                .actors
                .get(&id.key())
                .is_some_and(|c| c.cast_again > w.game_second)
        {
            return false;
        }
        let choices: Vec<_> = w
            .progression_rules()
            .spells
            .iter()
            .enumerate()
            .filter_map(|(i, spell)| {
                let p = spell.program.as_ref().ok()?;
                // Select direct and delayed vital spells implemented by the shared actor executor.
                (stats
                    .colleges
                    .get(i / 5)
                    .is_some_and(|rank| *rank as usize > i % 5)
                    && spell
                        .willpower
                        .as_ref()
                        .is_ok_and(|wp| stats.attributes[6] >= *wp)
                    && (npc_flags & 0x40000 == 0 || stats.colleges[i / 5] as usize == i % 5 + 1)
                    && stats.current_fatigue > i64::from(p.cost)
                    && p.maintain.is_none()
                    && p.missile.is_none()
                    && !p.ai.is_empty()
                    && p.components.iter().all(|c| {
                        matches!(c.phase, Phase::Begin | Phase::Callback)
                            && c.apply.is_empty()
                            && matches!(
                                c.operation,
                                Operation::Vital { .. } | Operation::Eye { add: true, .. }
                            )
                    }))
                .then(|| (i, p.clone()))
            })
            .collect();
        if choices.is_empty() {
            return false;
        }
        let combat = w.cache.inventory.combat.active();
        let allow = !combat || (w.cache.inventory.skill_random.below(100) as i32) < params[14];
        let mut patients = vec![Subject::Mobile(id), Subject::Player];
        patients.extend(
            w.cache
                .scripts
                .followers
                .iter()
                .filter(|(_, f)| !f.waiting)
                .filter_map(|(k, _)| parse_id(k).ok())
                .filter(|other| *other != id)
                .map(Subject::Mobile),
        );
        if let Some(enemy) = enemy {
            patients.push(Subject::Mobile(enemy));
        }
        for subject in patients {
            let Some(aim) = Self::subject_aim(w, &subject) else {
                continue;
            };
            let Ok(mut target) = self.target(w, aim) else {
                continue;
            };
            let hp = 100 * i64::from(target.maximum.saturating_sub(target.hp))
                / i64::from(target.maximum.max(1));
            let hostile = matches!(subject,Subject::Mobile(other) if Some(other)==enemy);
            if !hostile && !allow && hp > 30 {
                continue;
            }
            if !targeting::sees(w, caster.tile, target.tile, false) {
                continue;
            }
            let priorities = [
                (
                    "ai_resurrect",
                    !hostile && target.hp >= target.maximum && target.critter & 0x1000000 == 0,
                ),
                ("ai_healingheavy", !hostile && hp < 40),
                ("ai_healingmedium", !hostile && hp < 55),
                ("ai_curepoison", !hostile && target.poison > 0),
                (
                    "ai_healinglight",
                    !hostile && hp < if combat { 70 } else { 90 },
                ),
                ("ai_offensive", hostile),
            ];
            for (category, applicable) in priorities {
                if !applicable {
                    continue;
                }
                let spell = choices
                    .iter()
                    .filter(|(_, p)| {
                        p.ai.contains_key(category)
                            && p.disallowed_caster & caster.spell == 0
                            && p.disallowed_critter & target.critter == 0
                            && p.disallowed_target & target.spell == 0
                            && p.range
                                >= caster
                                    .tile
                                    .0
                                    .abs_diff(target.tile.0)
                                    .max(caster.tile.1.abs_diff(target.tile.1))
                            && target
                                .matches_from(&p.targets, &caster.subject)
                                .unwrap_or(false)
                            && p.components.iter().all(|c| {
                                target
                                    .matches_from(&c.targets, &caster.subject)
                                    .unwrap_or(false)
                            })
                    })
                    .max_by_key(|(_, p)| p.ai[category]);
                let Some((spell, p)) = spell else { continue };
                let inv = &mut w.cache.inventory;
                let aptitude = caster.aptitude;
                let before = target.hp;
                inv.change_vitals(&mut caster, Channel::Fatigue, p.cost);
                inv.pay_combat_ap(Some(id), 4);
                let (passes, resistance) = saving_throw(
                    p,
                    &target,
                    target.subject == caster.subject,
                    &mut inv.skill_random,
                );
                let passes = passes && target.spell & 0x800 == 0;
                inv.magick.actors.entry(id.key()).or_default().cast_again =
                    w.game_second.saturating_add(8);
                inv.revision += 1;
                self.spell_eye(w, *spell as i32 * 10, Some(key), now, Cue::Forward);
                if passes {
                    let mut binding = Binding::new(subject.clone());
                    binding.resistance = resistance;
                    binding.maximum = target.maximum;
                    let mut callback = None;
                    for c in p.components.iter().filter(|c| c.phase == Phase::Begin) {
                        match &c.operation {
                            Operation::Vital { .. } => self.apply_actor_vital(
                                w,
                                &mut target,
                                c,
                                aptitude,
                                resistance,
                                Some(id),
                            ),
                            Operation::Eye { index, play, .. } => {
                                let (effect, fx) = self.start_attachment(
                                    w,
                                    target.key,
                                    *spell as i32 * 10 + index,
                                    play,
                                    now,
                                    binding.visuals.last(),
                                );
                                if has(play, "callback") {
                                    callback = Some((
                                        effect
                                            .as_ref()
                                            .and_then(|v| v.until)
                                            .unwrap_or(w.game_second),
                                        Duration::from_millis(
                                            fx["duration_ms"].as_u64().unwrap_or(0),
                                        ),
                                    ));
                                }
                                if let Some(effect) = effect {
                                    binding.visuals.push(effect);
                                }
                            }
                            _ => unreachable!("validated direct-action spell"),
                        }
                    }
                    if let Some((due, left)) = callback {
                        w.cache.inventory.magick.active.push(Active {
                            caster: Some(id),
                            spell: *spell,
                            bindings: vec![binding],
                            summons: vec![],
                            due: None,
                            ticks: None,
                            aptitude,
                            callback: Some(due),
                            callback_left: Some(left),
                            ending: false,
                            center: Some((w.map.clone(), target.tile)),
                        });
                    }
                }
                w.cache.inventory.combat.log(json!({"event":"spell","caster":id.key(),"target":subject,"spell":spell,"category":category,"resisted":!passes,"health_damage_before":before,"health_damage_after":target.hp,"fatigue_cost":p.cost,"ap_cost":if combat{4}else{0}}));
                return true;
            }
        }
        false
    }
}

impl InventoryReader {
    pub(in crate::content::world::inventory) fn advance_followers(
        &mut self,
        w: &mut InteractiveWorld,
    ) {
        let level = w.cache.inventory.character.base[17];
        let mut pending = Vec::new();
        for (key, member) in &mut w.cache.scripts.followers {
            let old = std::mem::replace(&mut member.leader_level, level);
            if old > 0
                && level > old
                && let Ok(id) = parse_id(key)
            {
                pending.push((id, old, member.waiting));
            }
        }
        for spawn in w
            .cache
            .inventory
            .magick
            .active
            .iter_mut()
            .flat_map(|r| &mut r.summons)
            .filter(|s| s.npc_flags & 0x8000 != 0)
        {
            let old = std::mem::replace(&mut spawn.leader_level, level);
            if old > 0 && level > old {
                pending.push((spawn.id, old, false));
            }
        }
        for (id, old, waiting) in pending {
            let inv = &w.cache.inventory;
            let visible = Self::subject_aim(w, &Subject::Mobile(id)).and_then(|aim| match aim {
                Aim::Object(key) => w.cache.npc(key),
                _ => None,
            });
            if waiting
                && visible.is_none_or(|(_, tile)| {
                    w.cache.player.as_ref().is_none_or(|p| {
                        p.tile().0.abs_diff(tile.0).max(p.tile().1.abs_diff(tile.1)) > 30
                    })
                })
            {
                continue;
            }
            let (mut pc, scheme) =
                if let Some(r) = visible.map(|(r, _)| r).or_else(|| inv.displaced_actor(id)) {
                    let Some(d) = r.prototype.as_ref().map(|p| &p.defaults) else {
                        continue;
                    };
                    (
                        npc_character(d, Some(&r.instance())),
                        r.instance()
                            .field(91)
                            .and_then(FieldValue::scalar)
                            .map_or(d.level_scheme, |n| n as i32),
                    )
                } else if let Some(spawn) = inv.magick.spawn(id)
                    && let Ok((_, d)) = spawn_defaults(&mut self.vfs, spawn.prototype)
                {
                    (npc_character(&d, None), d.level_scheme)
                } else {
                    continue;
                };
            inv.apply_actor_changes(id, &mut pc);
            if let Some(profile) = pc.advance_as_follower(scheme, old, level, &inv.rules) {
                w.cache
                    .inventory
                    .magick
                    .actors
                    .entry(id.key())
                    .or_default()
                    .advancement = Some(profile);
                w.cache.inventory.revision += 1;
            }
        }
    }
}

/// Public HUD data uses the same actor statistics as spell targeting and combat.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct PartyPortrait {
    pub id: String,
    pub portrait: i32,
    pub interface_icon: Option<i32>,
    pub percent: [u32; 2],
    pub health: [i64; 2],
    pub fatigue: [i64; 2],
    pub colleges: [u32; 16],
    pub level: i32,
    pub tile: (u32, u32),
}

impl InventoryReader {
    pub fn party_portraits(&mut self, w: &InteractiveWorld) -> Vec<PartyPortrait> {
        let active = w
            .cache
            .scripts
            .followers
            .keys()
            .filter(|k| w.cache.scripts.following(k))
            .filter_map(|k| parse_id(k).ok());
        let summons = w
            .cache
            .inventory
            .magick
            .following_summons(&w.map, &w.cache.inventory.state);
        active
            .chain(
                w.cache
                    .inventory
                    .magick
                    .controlled_orders()
                    .map(|(id, _)| id),
            )
            .chain(summons.into_iter().map(|(id, _)| id))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .filter(|&id| !w.cache.inventory.magick.down(id))
            .filter_map(|id| {
                let aim = Self::subject_aim(w, &Subject::Mobile(id))?;
                let Aim::Object(k) = aim else {
                    return None;
                };
                let target = self.target(w, aim).ok()?;
                let portrait = if let Some((r, _)) = w.cache.npc(k) {
                    r.instance()
                        .field(77)
                        .and_then(FieldValue::scalar)
                        .map(|n| n as i32)
                        .or_else(|| Some(r.prototype.as_ref()?.defaults.portrait))?
                } else {
                    target.stats.as_ref()?.portrait
                };
                let stats = target.stats?;
                let pct = |v: i64, max: i64| (100 * v.max(0) / max.max(1)).clamp(0, 100) as u32;
                Some(PartyPortrait {
                    id: id.key(),
                    portrait,
                    interface_icon: (portrait == 0).then(|| {
                        // intgame_examine_portrait uses the critter flags before the race.
                        if target.critter & 4 != 0 {
                            384
                        } else if target.critter & 0x8000 != 0 {
                            383
                        } else if target.critter & 8 != 0 {
                            385
                        } else if target.critter & 0x20000000 != 0 {
                            434
                        } else {
                            *[375, 378, 376, 377, 379, 380, 381, 382, 376, 382, 381]
                                .get(stats.race as usize)
                                .unwrap_or(&432)
                        }
                    }),
                    percent: [
                        pct(stats.current_health, stats.max_health),
                        pct(stats.current_fatigue, stats.max_fatigue),
                    ],
                    health: [stats.current_health, stats.max_health],
                    fatigue: [stats.current_fatigue, stats.max_fatigue],
                    colleges: stats.colleges,
                    level: stats.level,
                    tile: target.tile,
                })
            })
            .collect()
    }

    pub fn party_portrait_frame(
        &mut self,
        w: &InteractiveWorld,
        member: &PartyPortrait,
    ) -> Result<Arc<LoadedFrame>, String> {
        if let Some(id) = member.interface_icon {
            let path = w
                .progression_rules()
                .interface
                .get(&id)
                .ok_or_else(|| format!("Missing follower interface icon {id}"))?;
            self.frames
                .load(&mut self.vfs, path, 0, 0, false)
                .map_err(|e| e.to_string())
        } else {
            self.portrait(member.portrait)
        }
    }

    pub fn issue_follower_order(
        &mut self,
        w: &mut InteractiveWorld,
        key: &str,
        command: usize,
    ) -> Result<(), String> {
        use crate::content::script::party::Order;
        let id = parse_id(key)?;
        if w.follower_orders(id).is_none() || w.cache.inventory.magick.down(id) {
            return Err("This follower cannot accept the order".into());
        }
        if w.incapacity().is_some() {
            return Err("The player is incapacitated".into());
        }
        match command {
            0 | 1 => {
                w.follower_aim = Some((id, command == 1));
            }
            2 | 3 => {
                w.follower_orders_mut(id).unwrap().set_spacing(command == 3);
            }
            4 => {
                let member = w.follower_orders_mut(id).unwrap();
                member.clear_target();
                member.back_off = true;
            }
            5 => {
                if w.cache
                    .inventory
                    .magick
                    .controlled_orders()
                    .any(|(actor, _)| actor == id)
                {
                    return Err("Mind-controlled followers cannot wait".into());
                }
                w.cache.scripts.order(key, Order::Wait, false)?;
            }
            6 => {
                w.check_actor_inventory(id)?;
                self.prepare_gear(w, id)?;
                w.container_request = Some(id);
            }
            _ => return Err("Unavailable follower command".into()),
        }
        Ok(())
    }
}
impl InteractiveWorld {
    pub fn select_party_actor(&mut self, id: &str, run: bool) {
        let Some(id) = OriginalId::parse_key(id) else {
            return;
        };
        let Some(Aim::Object(key)) = InventoryReader::subject_aim(self, &Subject::Mobile(id))
        else {
            return;
        };
        if self.ability.casting.is_some() {
            return;
        }
        if let Some(method) = self.cancel_route_keep_spell() {
            if let Some(reason) = self.incapacity() {
                self.ability.reject(method, reason);
            } else if matches!(
                method,
                crate::content::world::abilities::Method::PickLocks(_)
            ) {
                self.ability.reject(method, "Select a portal or container");
            } else {
                self.ability.request = Some((Aim::Object(key), method));
            }
        } else if self.incapacity().is_none() && !self.cache.inventory.combat.active() {
            self.approach_interaction(key, run);
        }
    }

    pub(in crate::content::world) fn complete_follower_order(
        &mut self,
        id: OriginalId,
        attack: bool,
        picked: Option<OriginalId>,
        tile: Option<Tile>,
    ) -> Result<(), String> {
        if self.follower_orders(id).is_none() || self.cache.inventory.magick.down(id) {
            return Err("Follower unavailable".into());
        }
        if attack {
            let foe = picked
                .filter(|actor| {
                    *actor != id
                        && self.follower_orders(*actor).is_none()
                        && self
                            .cache
                            .inventory
                            .magick
                            .flags(&Subject::Mobile(*actor))
                            .0
                            & 0x08000000
                            == 0
                        && !self.cache.inventory.magick.down(*actor)
                })
                .ok_or("Select a living opponent")?;
            let member = self.follower_orders_mut(id).unwrap();
            member.clear_target();
            member.attack = Some(foe);
        } else {
            let tile = tile.ok_or("Destination outside the world")?;
            if !collision::can_stand(SectorCache::player_cell(
                &self.cache.entries,
                &self.cache.door_states,
                tile,
            )) {
                return Err("Destination is blocked".into());
            }
            let member = self.follower_orders_mut(id).unwrap();
            member.clear_target();
            member.destination = Some(tile);
        }
        Ok(())
    }
}

impl InventoryReader {
    pub fn follower_statistics(
        &mut self,
        w: &InteractiveWorld,
        key: &str,
    ) -> Option<CharacterStatistics> {
        let id = parse_id(key).ok()?;
        let aim = Self::subject_aim(w, &Subject::Mobile(id))?;
        let Aim::Object(k) = aim else {
            return None;
        };
        let mut stats = self.target(w, aim).ok()?.stats?;
        if let Some((r, _)) = w.cache.npc(k) {
            stats.name = self.npc_name(r, true);
        }
        Some(stats)
    }
}
