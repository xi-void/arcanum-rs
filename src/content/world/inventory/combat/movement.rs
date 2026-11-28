//! Ordinary actor routes use the same collision, step transaction and directional gait as pursuit.
use super::*;

fn location(n: u64) -> Tile {
    (n as u32, (n >> 32) as u32)
}

impl InventoryReader {
    /// CE ai_action_perform_combat occasionally withdraws to the packet's minimum range.
    pub(super) fn follower_withdrawal(
        &self,
        w: &mut InteractiveWorld,
        id: OriginalId,
        actor: &Target,
        enemy: Tile,
    ) -> Option<(u8, Tile)> {
        let (npc, _) = w.cache.npc(actor.key?)?;
        let packet = npc
            .instance()
            .field(130)
            .and_then(FieldValue::scalar)
            .or_else(|| Some(npc.prototype.as_ref()?.defaults.ai_packet))?;
        let minimum = w.progression_rules().ai_packets.get(&packet)?[15];
        let close = minimum > 1
            && distance(actor.tile, enemy) < minimum as u32
            && actor.spell & 0x01000000 == 0;
        let inv = &mut w.cache.inventory;
        let index = inv.combat.party.iter().position(|p| p.actor == Some(id))?;
        let retreat =
            close && (inv.combat.party[index].withdrawing || inv.skill_random.below(20) == 0);
        let step = retreat
            .then(|| combat_step(w, actor.tile, enemy, true))
            .flatten();
        w.cache.inventory.combat.party[index].withdrawing = step.is_some();
        step
    }

    pub(super) fn ordinary_movement(
        &mut self,
        w: &mut InteractiveWorld,
        now: Duration,
        pc: Tile,
    ) -> bool {
        let c = &mut w.cache.inventory.combat;
        if now < c.next_ordinary {
            return false;
        }
        c.ordinary_due = None;
        c.next_ordinary = now
            + Duration::from_millis(if w.cache.scripts.followers.is_empty() {
                160
            } else {
                80
            });
        if w.unconscious() || w.ability.casting.is_some() {
            return false;
        }
        let check_loyalty =
            !w.cache.scripts.followers.is_empty() && now >= w.cache.inventory.combat.next_loyalty;
        let player = check_loyalty.then(|| w.character_statistics());
        if check_loyalty {
            w.cache.inventory.combat.next_loyalty = now + Duration::from_secs(2);
        }
        let mut departures = Vec::new();
        let day = (6..21).contains(&w.cache.hour);
        let mut routes = Vec::new();
        let mut arrived = Vec::new();
        let mut followers = Vec::new();
        for r in &w.cache.ordered {
            let Some((resident, at)) = w.cache.npc(r.key) else {
                continue;
            };
            if r.hidden || w.cache.inventory.magick.down(resident.id) {
                continue;
            }
            if let Some(member) = w.cache.scripts.followers.get(&resident.id.key()) {
                if member.waiting || member.dead {
                    continue;
                }
                let actor_key = resident.id.key();
                if !member.forced
                    && let Some(pc) = player.as_ref().filter(|s| s.skills[11].training < 3)
                    && let Some(d) = resident.prototype.as_ref().map(|p| &p.defaults)
                {
                    let o = resident.instance();
                    let field = |i| o.field(i).and_then(FieldValue::scalar);
                    let (actor, stats) = w.cache.inventory.npc_vitals(resident.id, d, &o);
                    if actor.spell_flags & 0x08000000 == 0
                        && let Some(packet) = w
                            .progression_rules()
                            .ai_packets
                            .get(&field(130).unwrap_or(d.ai_packet))
                        && let Some(reaction) = npc::initial_reaction(
                            field(143).map(|n| n as i32).or(d.reaction_base),
                            field(128).or(d.npc_flags),
                            Some(stats.race),
                            actor.critter_flags,
                            pc,
                        )
                        && let Err(reason) = crate::content::script::party::eligibility(
                            packet,
                            reaction.saturating_add(w.reaction_delta(&actor_key)),
                            [pc.level, stats.level],
                            [pc.alignment, stats.alignment],
                        )
                    {
                        departures.push((actor_key, reason));
                        continue;
                    }
                }
                followers.push(resident.id);
            }
            if let Some(orders) = w.follower_orders(resident.id) {
                if let Some((goal, retreat)) = orders.route(at, pc, distance(at, pc)) {
                    routes.push((resident.id, at, goal, None, retreat));
                } else if orders.destination == Some(at) {
                    arrived.push(resident.id);
                }
                continue;
            }
            if distance(at, pc) > 16 {
                continue;
            }
            let o = resident.instance();
            if o.field(129).and_then(FieldValue::reference).is_some() {
                continue;
            }
            let defaults = resident.prototype.as_ref().map(|p| &p.defaults);
            let flags = o
                .field(128)
                .and_then(FieldValue::scalar)
                .or_else(|| defaults?.npc_flags)
                .unwrap_or(0);
            let mut waypoint = None;
            let goal = if flags & (if day { 2 } else { 4 }) != 0
                && let Some(points) = o.field(135)
            {
                let idx = w
                    .cache
                    .inventory
                    .magick
                    .waypoint(resident.id)
                    .or_else(|| o.field(136)?.scalar().map(|n| n as usize))
                    .unwrap_or(0);
                let get = |n| {
                    Some(location(u64::from_le_bytes(
                        points.array_entry(n)?.try_into().ok()?,
                    )))
                };
                let Some(goal) = get(idx).or_else(|| get(0)) else {
                    continue;
                };
                if distance(at, goal) <= 3 {
                    let next = if get(idx.saturating_add(1)).is_some() {
                        idx + 1
                    } else {
                        0
                    };
                    waypoint = Some(next);
                    get(next).unwrap_or(goal)
                } else {
                    goal
                }
            } else if let Some(FieldValue::Wide(n)) = o.field(if day { 137 } else { 138 })
                && n != 0
            {
                let standpoint = location(n);
                if distance(at, standpoint) <= 1 {
                    continue;
                }
                standpoint
            } else {
                continue;
            };
            if distance(at, goal) > 1 {
                routes.push((resident.id, at, goal, waypoint, false))
            }
        }
        for (id, tile) in w
            .cache
            .inventory
            .magick
            .following_summons(&w.map, &w.cache.inventory.state)
        {
            let orders = w.follower_orders(id).copied().unwrap_or_default();
            if let Some((goal, retreat)) = orders.route(tile, pc, distance(tile, pc)) {
                routes.push((id, tile, goal, None, retreat));
            } else if orders.destination == Some(tile) {
                arrived.push(id);
            }
        }
        for id in arrived {
            if let Some(orders) = w.follower_orders_mut(id) {
                orders.destination = None;
            }
        }
        let mut moved = !departures.is_empty();
        for (id, reason) in departures {
            w.cache.scripts.followers.remove(&id);
            w.cache.scripts.jilted.insert(id.clone());
            w.cache
                .inventory
                .combat
                .log(json!({"event":"follower_departed","actor":id,"reason":reason}));
            w.ability.message = reason.to_owned();
        }
        for id in followers {
            if self.follower_cast(w, id, None, now) {
                moved = true;
                w.cache.inventory.combat.ordinary_due = Some(now + Duration::from_secs(1));
            }
        }
        for (id, at, goal, waypoint, retreat) in routes {
            let c = &mut w.cache.inventory.combat;
            c.ordinary_due = Some(c.next_ordinary);
            if c.poses.get(&Some(id)).is_some_and(|p| p.animating(now)) {
                continue;
            }
            let Ok(t) = self.fighter(w, Some(id)) else {
                continue;
            };
            if t.unable() || t.spell & 0x01000000 != 0 {
                continue;
            }
            // A separated member walks towards the leader through a short loaded section of the route.
            // Only request an intermediate tile when the two sectors do not touch.
            let range = distance(at, goal);
            let goal = if goal == pc && range > 64 {
                let along = |a: u32, b: u32| {
                    (i64::from(a) + (i64::from(b) - i64::from(a)) * 16 / i64::from(range)) as u32
                };
                (along(at.0, goal.0), along(at.1, goal.1))
            } else {
                goal
            };
            let step = combat_step(w, at, goal, retreat).or_else(|| {
                if retreat || !w.cache.scripts.following(&id.key()) {
                    return None;
                }
                let (resident, _) = w.cache.npc(t.key?)?;
                let packet = resident
                    .instance()
                    .field(130)
                    .and_then(FieldValue::scalar)
                    .or_else(|| Some(resident.prototype.as_ref()?.defaults.ai_packet))?;
                if w.progression_rules().ai_packets.get(&packet)?[16] == 0 {
                    return None;
                }
                w.cache.follower_door_step(at, goal, now)
            });
            if let Some((dir, to)) = step {
                match self.walk_actor(w, &t, id, dir, to, now) {
                    Ok(_) => {
                        moved = true;
                        if let Some(n) = waypoint {
                            w.cache.inventory.magick.set_waypoint(id, n)
                        }
                    }
                    Err(reason) => {
                        w.cache.inventory.combat.log(
                            json!({"event":"animation_unavailable","actor":id,"reason":reason}),
                        )
                    }
                }
            }
        }
        moved
    }
}
