//! Ordinary actor routes use the same collision, step transaction and directional gait as pursuit.
use super::*;

fn location(n: u64) -> Tile {
    (n as u32, (n >> 32) as u32)
}

impl InventoryReader {
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
        c.next_ordinary = now + Duration::from_millis(160);
        if w.unconscious() || w.ability.casting.is_some() {
            return false;
        }
        let day = (6..21).contains(&w.cache.hour);
        let mut routes = Vec::new();
        for r in &w.cache.ordered {
            let Some((resident, at)) = w.cache.npc(r.key) else {
                continue;
            };
            if r.hidden || distance(at, pc) > 16 || w.cache.inventory.magick.down(resident.id) {
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
                routes.push((resident.id, at, goal, waypoint))
            }
        }
        for (id, tile) in w
            .cache
            .inventory
            .magick
            .following_summons(&w.map, &w.cache.inventory.state)
        {
            if distance(tile, pc) > 2 {
                routes.push((id, tile, pc, None))
            }
        }
        let mut moved = false;
        for (id, at, goal, waypoint) in routes {
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
            if let Some((dir, to)) = combat_step(w, at, goal, false) {
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
