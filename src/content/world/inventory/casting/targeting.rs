//! Select loaded tiles, objects and actors using the original spell filters.
use super::*;

impl InventoryReader {
    pub(in crate::content::world::inventory) fn target(
        &mut self,
        w: &InteractiveWorld,
        aim: Aim,
    ) -> Result<Target, String> {
        let at = w.cache.player.as_ref().ok_or("Caster unavailable")?.tile();
        let inv = &w.cache.inventory;
        match aim {
            Aim::Player | Aim::Destination(_) => {
                let mut t = Target::new(Subject::Player, at, None, Kind::Actor);
                t.actor(&inv.character, &inv.statistics());
                // CE object_set_gender_and_race initializes the player voice bank.
                t.sound = inv.character.sound_effect.or_else(|| {
                    let stats = t.stats.as_ref()?;
                    Some(10 * (stats.gender + 2 * stats.race + 1))
                });
                let (s, c) = inv.magick.flags(&t.subject);
                t.spell |= s;
                t.critter |= c;
                Ok(t)
            }
            Aim::Tile(tile) => Ok(Target::new(
                Subject::Tile(w.map.clone(), tile),
                tile,
                None,
                Kind::Tile,
            )),
            Aim::Item(id) => {
                let change = inv
                    .state
                    .changes
                    .get(&id)
                    .filter(|c| matches!(c.place, Place::Owned(Owner::Player) | Place::Equipped(_)))
                    .ok_or("Item is no longer carried")?;
                Ok(Target::new(Subject::Item(id), at, None, Kind::Item)
                    .with_item(change.item.clone()))
            }
            Aim::Object(key) => {
                if let Some((r, tile)) = w.cache.npc(key) {
                    let d = &r
                        .prototype
                        .as_ref()
                        .ok_or("Missing NPC prototype")?
                        .defaults;
                    let (pc, stats) = inv.npc_vitals(r.id, d, &r.instance());
                    let mut t = Target::new(Subject::Mobile(r.id), tile, Some(key), Kind::Actor);
                    let instance = r.instance();
                    t.worth = std::array::from_fn(|i| {
                        instance
                            .field(133 + i)
                            .and_then(FieldValue::scalar)
                            .map_or(d.xp[i], |n| n as i32)
                    });
                    t.natural = instance.field(151).map_or(d.natural_damage, |v| {
                        std::array::from_fn(|i| {
                            v.array_entry(i)
                                .map_or(0, |b| i32::from_le_bytes(b.try_into().unwrap()))
                        })
                    });
                    t.actor(&pc, &stats);
                    t.party = w.cache.scripts.following(&r.id.key());
                    let f = r.instance().appearance();
                    t.sound = f.sound_effect.or(d.appearance.sound_effect);
                    t.material = f.material.or(d.appearance.material);
                    t.object_flags = f.flags.or(d.appearance.flags).unwrap_or(0);
                    return Ok(t);
                }
                let selected = w
                    .cache
                    .target(key)
                    .ok_or("Select an actor, item or portal")?;
                let tile = selected.tile;
                if let interaction::Action::Summon(id) = selected.action {
                    let s = inv.magick.spawn(id).ok_or("Summon expired")?;
                    let (cat, d) = match self.summon_defaults.entry(s.prototype) {
                        std::collections::btree_map::Entry::Occupied(e) => e.get().clone(),
                        std::collections::btree_map::Entry::Vacant(e) => {
                            let (cat, defaults) = spawn_defaults(&mut self.vfs, s.prototype)?;
                            e.insert((cat, Arc::new(defaults))).clone()
                        }
                    };
                    let mut t = Target::new(
                        Subject::Mobile(id),
                        tile,
                        Some(key),
                        if cat == 16 { Kind::Actor } else { Kind::Item },
                    );
                    t.sound = d.appearance.sound_effect;
                    t.material = d.appearance.material;
                    t.object_flags = d.appearance.flags.unwrap_or(0);
                    let mut pc = npc_character(&d, None);
                    inv.apply_actor_changes(id, &mut pc);
                    let rules = Rules {
                        temporary: inv.magick.effects(&t.subject, &inv.rules),
                        ..Rules::default()
                    };
                    t.natural = d.natural_damage;
                    t.actor(&pc, &inv.npc_statistics(id, &pc, rules));
                    t.spell |= s.spell_flags | 0x4000;
                    t.critter |= s.critter_flags;
                    return Ok(t);
                }
                if let interaction::Action::Pickup(id) = selected.action {
                    let item = w.records(&mut self.vfs).item(id, &inv.state)?;
                    return Ok(
                        Target::new(Subject::Item(id), tile, Some(key), Kind::Item).with_item(item)
                    );
                }
                let id = w
                    .lock_id(key)
                    .ok_or("The selected object cannot receive this spell")?;
                let (kind, flags, diff, notify) = match &selected.action {
                    interaction::Action::Door(d) => (Kind::Portal, d.flags, d.difficulty, d.notify),
                    interaction::Action::Window {
                        flags,
                        difficulty,
                        notify,
                        ..
                    } => (Kind::Portal, *flags, *difficulty, *notify),
                    interaction::Action::Container(id) => {
                        let c = w.records(&mut self.vfs).container(*id)?;
                        if c.flags & (UNAVAILABLE | CLICK_THROUGH | 0x1000) != 0 {
                            return Err("Unavailable container".into());
                        }
                        (Kind::Container, c.subtype_flags, c.difficulty, c.notify)
                    }
                    _ => return Err("Unsupported object".into()),
                };
                let mut t = Target::new(Subject::Lock(w.map.clone(), id), tile, Some(key), kind);
                t.flags = w.cache.door_states.flags(id, flags);
                t.difficulty = diff;
                t.notify = notify;
                Ok(t)
            }
        }
    }
}
fn distance(a: Tile, b: Tile) -> u32 {
    a.0.abs_diff(b.0).max(a.1.abs_diff(b.1))
}
pub(super) fn sees(w: &InteractiveWorld, from: Tile, to: Tile, portal: bool) -> bool {
    let cell = |t| SectorCache::player_cell(&w.cache.entries, &w.cache.door_states, t);
    collision::sight(from, to, portal, cell)
}
impl InventoryReader {
    pub(super) fn spell_targets(
        &mut self,
        w: &InteractiveWorld,
        p: &Program,
        aim: Aim,
    ) -> Result<Vec<Target>, String> {
        let from = w.cache.player.as_ref().ok_or("Caster unavailable")?.tile();
        let main = self.target(w, aim)?;
        if distance(from, main.tile) > p.range {
            return Err("Target outside spell range".into());
        }
        if !sees(w, from, main.tile, main.kind == Kind::Portal) {
            return Err("Target is outside line of sight".into());
        }
        let area = has(&p.targets, "tgt_obj_radius") || has(&p.targets, "tgt_cone");
        let tile_area =
            has(&p.targets, "tgt_tile_radius") || has(&p.targets, "tgt_tile_radius_wall");
        let tile_aim = p.targets.iter().any(|s| s.starts_with("tgt_tile"));
        if tile_aim && main.kind != Kind::Tile && !area {
            return Err("Select a ground tile".into());
        }
        let cell = |t| SectorCache::player_cell(&w.cache.entries, &w.cache.door_states, t);
        if has(&p.targets, "tgt_tile_pathable_to")
            && crate::content::player::path::find(from, main.tile, cell).is_none()
        {
            return Err("Destination is unreachable".into());
        }
        let on_tile = |tile: Tile| {
            tile == from
                || w.cache.ordered.iter().any(|r| {
                    r.key.1.2 == tile.0
                        && r.key.1.1 == tile.1
                        && matches!(
                            w.cache.target(r.key).map(|t| &t.action),
                            Some(interaction::Action::Npc(_) | interaction::Action::Summon(_))
                        )
                })
        };
        let empty = |tile| collision::can_stand(cell(tile)) && !on_tile(tile);
        let mut targets = vec![];
        if area {
            let center = if has(&p.targets, "tgt_self") {
                from
            } else {
                main.tile
            };
            let flags: Vec<_> = p
                .targets
                .iter()
                .filter(|s| {
                    !s.starts_with("tgt_tile") && !matches!(s.as_str(), "tgt_self" | "tgt_cone")
                })
                .cloned()
                .collect();
            for r in &w.cache.ordered {
                let tile = (r.key.1.2, r.key.1.1);
                if distance(center, tile) > p.radius
                    || !matches!(
                        w.cache.target(r.key).map(|t| &t.action),
                        Some(
                            interaction::Action::Npc(_)
                                | interaction::Action::Summon(_)
                                | interaction::Action::Pickup(_)
                        )
                    )
                {
                    continue;
                }
                // CE target.c enumerates the radius square for self-centered TGT_CONE.
                let t = self.target(w, Aim::Object(r.key))?;
                if t.matches(&flags)? && sees(w, center, tile, false) {
                    targets.push(t)
                }
            }
        }
        if tile_area {
            let r = i32::try_from(p.radius).map_err(|_| "Area outside loaded world")?;
            // Intersect with loaded sector extents before enumerating possible tiles.
            for id in &w.cache.active {
                let Some(c) = Coordinates::from_id(*id) else {
                    continue;
                };
                let (x, y) = main.tile;
                let (left, top) = (c.x * 64, c.y * 64);
                for ty in
                    top.max(y.saturating_sub(p.radius))..=(top + 63).min(y.saturating_add(p.radius))
                {
                    for tx in left.max(x.saturating_sub(p.radius))
                        ..=(left + 63).min(x.saturating_add(p.radius))
                    {
                        let tile = (tx, ty);
                        if has(&p.targets, "tgt_tile_radius_wall") {
                            let (dx, dy) = match npc::facing(from, main.tile).unwrap_or(0) {
                                1 | 2 => (0, -1),
                                3 => (-1, 0),
                                4 => (-1, 1),
                                5 => (0, 1),
                                6 => (1, 1),
                                7 => (1, 0),
                                _ => (1, -1),
                            };
                            let ax = i64::from(tx) - i64::from(x);
                            let ay = i64::from(ty) - i64::from(y);
                            if ax * dy != ay * dx || ax.abs().max(ay.abs()) > r.into() {
                                continue;
                            }
                        }
                        if !has(&p.targets, "tgt_tile_empty") || empty(tile) {
                            targets.push(self.target(w, Aim::Tile(tile))?)
                        }
                    }
                }
            }
        } else if area
            && has(&p.targets, "tgt_tile")
            && p.components
                .iter()
                .any(|c| matches!(c.operation, Operation::Summon { .. }))
            && empty(main.tile)
        {
            targets.push(self.target(w, Aim::Tile(main.tile))?);
        } else if !area {
            if has(&p.targets, "tgt_tile_empty") && !empty(main.tile) {
                return Err("The target tile is occupied".into());
            }
            if !main.matches(&p.targets)? {
                return Err("Invalid spell target".into());
            }
            targets.push(main);
            // target.c uses object_list_team for TGT_ALL_PARTY_CRITTERS.
            // Controlled actors on the loaded map share the caster's team.
            if has(&p.targets, "tgt_all_party_critters") {
                for raster in &w.cache.ordered {
                    if !matches!(
                        w.cache.target(raster.key).map(|t| &t.action),
                        Some(interaction::Action::Npc(_) | interaction::Action::Summon(_))
                    ) {
                        continue;
                    }
                    let member = self.target(w, Aim::Object(raster.key))?;
                    if (member.spell & 0x08000000 != 0
                        || matches!(member.subject, Subject::Mobile(id) if w.cache.scripts.following(&id.key())))
                        && member.matches(&p.targets)?
                    {
                        targets.push(member);
                    }
                }
            }
        }
        if targets.is_empty() {
            return Err("No eligible target in the area".into());
        }
        Ok(targets)
    }
}
