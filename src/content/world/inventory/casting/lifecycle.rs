//! Apply callbacks, upkeep, environmental damage and the authored end sequence.
use super::*;

impl InventoryReader {
    pub(in crate::content::world::inventory) fn subject_aim(
        w: &InteractiveWorld,
        subject: &Subject,
    ) -> Option<Aim> {
        match subject{
            Subject::Player=>Some(Aim::Player),
            Subject::Item(id)=>Some(Aim::Item(*id)),
            Subject::Tile(map,tile) if map==&w.map=>Some(Aim::Tile(*tile)),
            Subject::Mobile(id)=>w.cache.ordered.iter().find(|r|matches!(w.cache.target(r.key).map(|t|&t.action),Some(interaction::Action::Npc(target)|interaction::Action::Summon(target)) if target==id)).map(|r|Aim::Object(r.key)),
            Subject::Lock(map,id) if map==&w.map=>w.cache.ordered.iter().find(|r|w.lock_id(r.key)==Some(*id)).map(|r|Aim::Object(r.key)),
            _=>None
        }
    }
    /// CE's Stack schedules a subgoal after the previous clip on this owner.
    pub(super) fn start_attachment(
        &mut self,
        w: &mut InteractiveWorld,
        key: Option<WorldOrder>,
        entry: i32,
        play: &[String],
        now: Duration,
        previous: Option<&Attachment>,
    ) -> (Option<Attachment>, Value) {
        let clock = w.game_second;
        let start = if has(play, "stack") {
            previous.and_then(|v| v.until).unwrap_or(clock).max(clock)
        } else {
            clock
        };
        let (_, fx) = self.spell_eye(
            w,
            entry,
            key,
            now + Duration::from_millis((start - clock).saturating_mul(125)),
            Cue::for_play(play),
        );
        let Some(ms) = fx["duration_ms"].as_u64() else {
            return (None, fx);
        };
        let reverse = has(play, "reverse");
        let playback = w
            .cache
            .effects
            .get_mut(&(key, entry))
            .expect("prepared EyeCandy");
        let initial = if has(play, "random_start") {
            let (x, y) = playback.tile;
            let sequence = (u64::from(x) << 32 | u64::from(y)).wrapping_add(clock);
            sequence.wrapping_mul(0x9e3779b97f4a7c15).rotate_left(17) as usize
        } else {
            0
        };
        playback.configure(reverse, initial, Duration::ZERO);
        let until = (!playback.looping).then(|| start.saturating_add(ms.div_ceil(125).max(1)));
        let hazard = fx["layers"]
            .as_array()
            .and_then(|a| a.iter().rev().find(|v| v.is_object()))
            .filter(|_| has(play, "fire_dmg") || has(play, "ice_dmg"))
            .map(|layer| {
                let count = layer["frames"].as_u64().unwrap_or(1).max(1) as usize;
                Hazard {
                    fire: has(play, "fire_dmg"),
                    next: start.saturating_mul(125),
                    frame: initial % count,
                    count,
                    period_ms: ms / count as u64,
                }
            });
        (
            Some(Attachment {
                entry,
                start,
                until,
                reverse,
                initial,
                hazard,
            }),
            fx,
        )
    }
    pub(super) fn attach_eye(
        &mut self,
        w: &mut InteractiveWorld,
        id: OriginalId,
        entry: i32,
        play: &[String],
        now: Duration,
    ) -> Value {
        let Some(Aim::Object(k)) = Self::subject_aim(w, &Subject::Mobile(id)) else {
            return Value::Null;
        };
        let previous = w
            .cache
            .inventory
            .magick
            .spawn(id)
            .and_then(|s| s.visuals.last())
            .cloned();
        let (attachment, mut fx) =
            self.start_attachment(w, Some(k), entry, play, now, previous.as_ref());
        let Some(attachment) = attachment else {
            return fx;
        };
        let until = attachment.until;
        let run = w
            .cache
            .inventory
            .magick
            .active
            .iter_mut()
            .find(|r| r.summons.iter().any(|s| s.id == id))
            .unwrap();
        let spawn = run.summons.iter_mut().find(|s| s.id == id).unwrap();
        spawn.visuals.retain(|v| v.entry != entry);
        spawn.visuals.push(attachment);
        if (has(play, "callback") || has(play, "end_callback"))
            && let Some(due) = until
        {
            run.callback = Some(run.callback.unwrap_or(due).max(due));
            run.callback_left = Some(run.callback_left.unwrap_or_default().max(
                Duration::from_millis(due.saturating_sub(w.game_second) * 125),
            ));
        }
        fx["entry"] = json!(entry);
        fx["owner"] = json!(id);
        fx["role"] = json!("summoned");
        fx
    }
    fn apply_phase(
        &mut self,
        w: &mut InteractiveWorld,
        r: &Active,
        p: &Program,
        phase: Phase,
        now: Duration,
    ) {
        let mut targets = Vec::new();
        if phase == Phase::Maintain && !p.maintain_targets.is_empty() {
            if let Some((map, center)) = &r.center
                && map == &w.map
            {
                let mut area = p.clone();
                area.targets = p.maintain_targets.clone();
                if let Ok(t) = self.spell_targets(w, &area, Aim::Tile(*center)) {
                    targets = t
                }
            }
        } else {
            for b in &r.bindings {
                if let Some(t) =
                    Self::subject_aim(w, &b.target).and_then(|aim| self.target(w, aim).ok())
                {
                    targets.push(t)
                } else if let Subject::Mobile(id) = b.target
                    && b.maximum > 0
                    && matches!(phase, Phase::Callback | Phase::End)
                {
                    // Health and paralysis of an original mobile still change outside the active sectors.
                    let mut t = Target::new(b.target.clone(), (0, 0), None, Kind::Actor);
                    t.maximum = b.maximum;
                    if let Some(v) = w.cache.inventory.magick.actors.get(&id.key()) {
                        t.hp = v.health.unwrap_or(0);
                        t.poison = v.poison.unwrap_or(0);
                    }
                    targets.push(t)
                }
            }
        }
        for mut t in targets {
            let resisted = r
                .bindings
                .iter()
                .find(|b| b.target == t.subject)
                .map_or(0, |b| b.resistance);
            for c in p.components.iter().filter(|c| c.phase == phase) {
                if ((has(&c.targets, "tgt_self") || has(&c.apply, "tgt_self"))
                    && t.subject != Subject::Player)
                    || (!(c.targets.is_empty() || t.matches(&c.targets) == Ok(true)))
                    || c.spell_flags != 0 && t.spell & c.spell_flags == 0
                {
                    continue;
                }
                match &c.operation {
                    Operation::Vital { .. } => {
                        apply_vital(&mut w.cache.inventory, &mut t, c, r.aptitude, resisted)
                    }
                    Operation::Flag { field, mask, on } if phase == Phase::Callback => w
                        .cache
                        .inventory
                        .permanent_flags(&t.subject, *field, *mask, *on),
                    Operation::Eye { index, add, play }
                        if t.subject == Subject::Player || t.key.is_some() =>
                    {
                        let entry = r.spell as i32 * 10 + index;
                        if *add {
                            self.spell_eye(w, entry, t.key, now, Cue::for_play(play));
                            if has(play, "reverse")
                                && let Some(f) = w.cache.effects.get_mut(&(t.key, entry))
                            {
                                f.configure(true, 0, Duration::ZERO);
                            }
                        } else {
                            w.cache.stop_effect(t.key, entry);
                        }
                    }
                    _ => (),
                }
            }
        }
    }
    pub(super) fn end_spell(&mut self, w: &mut InteractiveWorld, index: usize, now: Duration) {
        let run = w.cache.inventory.magick.active[index].clone();
        if let Ok(program) = w.cache.inventory.program(run.spell)
            && !run.ending
        {
            self.apply_phase(w, &run, &program, Phase::End, now);
            let r = &mut w.cache.inventory.magick.active[index];
            r.ending = true;
            r.due = None;
            r.callback = None;
            r.callback_left = None;
            r.bindings.clear();
            for spawn in &mut r.summons {
                spawn.visuals.clear()
            }
            for s in &run.summons {
                for c in program.components.iter().filter(|c| c.phase == Phase::End) {
                    if let Operation::Eye {
                        index: slot,
                        add,
                        play,
                    } = &c.operation
                    {
                        let entry = run.spell as i32 * 10 + slot;
                        if *add {
                            self.attach_eye(w, s.id, entry, play, now);
                        } else if let Some(Aim::Object(k)) =
                            Self::subject_aim(w, &Subject::Mobile(s.id))
                        {
                            w.cache.stop_effect(Some(k), entry);
                        }
                    }
                }
            }
        }
        if run.ending || w.cache.inventory.magick.active[index].callback.is_none() {
            for s in w.cache.inventory.magick.active.remove(index).summons {
                for c in w.cache.inventory.state.changes.values_mut() {
                    if matches!(&c.place,Place::WornBy(_,id,_) | Place::Owned(Owner::OriginalObject(_,id)) if *id==s.id)
                    {
                        c.place = Place::Retired;
                        c.slot = None;
                    }
                }
                w.cache.inventory.magick.actors.remove(&s.id.key());
            }
        }
        w.cache.inventory.refresh_magick();
        if !run.ending {
            transformation::restore(w, &mut self.vfs, &run.bindings);
        }
        w.ability.message = w.progression_rules().spells.get(run.spell).map_or_else(
            || "Item effect ended".to_owned(),
            |s| format!("{} ended", s.name),
        );
    }
    pub fn dismiss_spell(&mut self, w: &mut InteractiveWorld, slot: usize, now: Duration) {
        let Some((index, _)) = w
            .cache
            .inventory
            .magick
            .active
            .iter()
            .enumerate()
            .filter(|(_, r)| {
                !r.ending
                    && w.cache
                        .inventory
                        .program(r.spell)
                        .is_ok_and(|p| p.maintain.is_some())
            })
            .nth(slot)
        else {
            return;
        };
        self.end_spell(w, index, now);
        w.cache.inventory.revision += 1;
        w.refresh_spell_state();
    }
    // The damage goal checks frame 1 of every three frames of its foreground clip.
    fn advance_hazards(&mut self, w: &mut InteractiveWorld, now: u64) -> bool {
        let clock = now.saturating_mul(125);
        let mut hits = Vec::new();
        for run in &mut w.cache.inventory.magick.active {
            if run.ending {
                continue;
            }
            for spawn in &mut run.summons {
                for v in &mut spawn.visuals {
                    let Some(h) = &mut v.hazard else { continue };
                    if h.next > clock || h.period_ms == 0 {
                        continue;
                    }
                    let steps = (clock - h.next) / h.period_ms + 1;
                    h.next = h.next.saturating_add(steps.saturating_mul(h.period_ms));
                    let cycle = h.count as u64;
                    let tail = steps % cycle;
                    let pulses = steps / cycle * ((cycle + 1) / 3)
                        + (0..tail)
                            .filter(|i| (h.frame as u64 + i) % cycle % 3 == 1)
                            .count() as u64;
                    h.frame = (h.frame + tail as usize) % h.count;
                    if pulses > 0
                        && spawn.map == w.map
                        && w.cache.active.contains(&tile_sector_id(spawn.tile))
                    {
                        hits.push((spawn.id, spawn.tile, h.fire, run.aptitude, pulses));
                    }
                }
            }
        }
        let mut changed = false;
        for (emitter, tile, fire, apt, pulses) in hits {
            let player = w
                .cache
                .player
                .as_ref()
                .filter(|p| p.tile() == tile)
                .map(|_| Aim::Player);
            let targets: Vec<_> = player
                .into_iter()
                .chain(w.cache.ordered.iter().filter_map(|r| {
                    if (r.key.1.2, r.key.1.1) != tile {
                        return None;
                    }
                    match w.cache.target(r.key)?.action {
                        interaction::Action::Npc(_) | interaction::Action::Pickup(_) => {
                            Some(Aim::Object(r.key))
                        }
                        interaction::Action::Summon(id) if id != emitter => {
                            Some(Aim::Object(r.key))
                        }
                        _ => None,
                    }
                }))
                .collect();
            for aim in targets {
                let Ok(mut t) = self.target(w, aim) else {
                    continue;
                };
                if t.kind != Kind::Actor && (t.item.is_none() || t.maximum <= 0) {
                    continue;
                }
                let damage =
                    (if fire { 3 } else { 1 }) + if apt > 0 { 5 * apt / 100 - 1 } else { 0 };
                let channel = if fire { Channel::Fire } else { Channel::Normal };
                let amount = channel.after_resistance(damage, &t.resistance);
                let total = amount.saturating_mul(i32::try_from(pulses).unwrap_or(i32::MAX));
                let before = t.hp;
                let inv = &mut w.cache.inventory;
                inv.change_vitals(&mut t, channel, total);
                inv.note_death(&t, before, Some(emitter));
                inv.combat_reward(&t, before);
                changed |= total != 0;
            }
        }
        changed
    }
    pub fn tick_magick(&mut self, w: &mut InteractiveWorld, now: u64, elapsed: Duration) -> bool {
        let dt = w
            .cache
            .inventory
            .magick
            .animation_tick
            .replace(elapsed)
            .map_or(Duration::ZERO, |last| elapsed.saturating_sub(last));
        for r in &mut w.cache.inventory.magick.active {
            if let Some(left) = &mut r.callback_left {
                *left = left.saturating_sub(dt);
                if left.is_zero() {
                    r.callback = Some(now);
                    r.callback_left = None;
                }
            }
        }
        let mut changed = false;
        while let Some((index, due)) = w
            .cache
            .inventory
            .magick
            .active
            .iter()
            .enumerate()
            .filter_map(|(i, r)| {
                Some((
                    i,
                    [r.callback.filter(|_| r.callback_left.is_none()), r.due]
                        .into_iter()
                        .flatten()
                        .min()?,
                ))
            })
            .filter(|(_, d)| *d <= now)
            .min_by_key(|(_, d)| *d)
        {
            changed = true;
            let run = w.cache.inventory.magick.active[index].clone();
            let p = w
                .cache
                .inventory
                .program(run.spell)
                .expect("validated spell");
            if run.callback == Some(due) && run.callback_left.is_none() {
                if run.ending {
                    self.end_spell(w, index, elapsed);
                    continue;
                }
                for c in p.components.iter().filter(|c| c.phase == Phase::Callback) {
                    if let Operation::Flag {
                        field: Field::Object,
                        mask,
                        on,
                    } = c.operation
                        && mask & 0x100 != 0
                    {
                        for s in &mut w.cache.inventory.magick.active[index].summons {
                            s.hidden = on
                        }
                    }
                }
                self.apply_phase(w, &run, &p, Phase::Callback, elapsed);
                for binding in &run.bindings {
                    if binding.destroy
                        && let Subject::Mobile(id) = binding.target
                    {
                        let inv = &mut w.cache.inventory;
                        if let Some(d) = inv.magick.actors.get_mut(&id.key()) {
                            d.decayed = true;
                            d.decay_at = None;
                        }
                        for c in inv.state.changes.values_mut().filter(|c|matches!(&c.place, Place::WornBy(_,owner,_) | Place::Owned(Owner::OriginalObject(_,owner)) if *owner==id)){
                            c.place=Place::Retired;
                            c.slot=None;
                        }
                    }
                }
                w.cache.inventory.magick.active[index].callback = None;
                if run.due.is_none()
                    && (run.summons.is_empty()
                        || p.components.iter().any(|c| {
                            c.phase == Phase::Callback && matches!(c.operation, Operation::Destroy)
                        }))
                {
                    self.end_spell(w, index, elapsed)
                }
                continue;
            }
            let mut keep = false;
            if let Some((cost, period)) = p.maintain {
                let inv = &mut w.cache.inventory;
                let stats = inv.statistics();
                let resistance = run.bindings.iter().map(|b| b.resistance).max().unwrap_or(0);
                let cost = cost.saturating_add(if resistance > 0 {
                    (cost.saturating_mul(resistance) / 100).max(1)
                } else {
                    0
                });
                if stats.current_fatigue > 0
                    && stats.current_health > 0
                    && stats.attributes[4] >= 5
                    && stats.attributes[6]
                        >= inv.rules.progression.spells[run.spell]
                            .willpower
                            .clone()
                            .unwrap_or(i32::MAX)
                {
                    keep = inv
                        .charge_spell(
                            (run.spell / 5) as u32,
                            cost,
                            stats.current_fatigue,
                            due,
                            true,
                        )
                        .is_ok_and(|(energy, _)| i64::from(energy) <= stats.current_fatigue)
                }
                if keep {
                    let r = &mut inv.magick.active[index];
                    r.due = due.checked_add(period);
                    keep = r.due.is_some();
                    if let Some(n) = &mut r.ticks {
                        *n = n.saturating_sub(1);
                        keep &= *n > 0
                    }
                    if p.components.iter().any(|c| c.phase == Phase::Maintain) {
                        self.apply_phase(w, &run, &p, Phase::Maintain, elapsed)
                    }
                }
            }
            if !keep {
                self.end_spell(w, index, elapsed)
            }
        }
        changed |= self.advance_hazards(w, now);
        if changed {
            w.cache.inventory.revision += 1;
            w.refresh_spell_state();
        }
        if std::mem::take(&mut w.cache.sync_spell_fx) {
            let mut required: BTreeSet<_> = w
                .cache
                .inventory
                .magick
                .active
                .iter()
                .flat_map(|r| &r.bindings)
                .filter(|b| !b.overlays.is_empty())
                .filter_map(|b| {
                    let owner = match Self::subject_aim(w, &b.target)? {
                        Aim::Object(k) => Some(k),
                        Aim::Player => None,
                        _ => return None,
                    };
                    Some(b.overlays.iter().map(move |id| (owner, *id)))
                })
                .flatten()
                .collect();
            let mut owners: Vec<_> = w
                .cache
                .inventory
                .magick
                .active
                .iter()
                .flat_map(|r| r.summons.iter().map(move |s| (r, s)))
                .filter_map(|(run, s)| {
                    let Aim::Object(k) = Self::subject_aim(w, &Subject::Mobile(s.id))? else {
                        return None;
                    };
                    Some((Some(k), s.visuals.clone(), run.callback, run.callback_left))
                })
                .collect();
            for (run, b) in w
                .cache
                .inventory
                .magick
                .active
                .iter()
                .flat_map(|r| r.bindings.iter().map(move |b| (r, b)))
                .filter(|(_, b)| !b.visuals.is_empty())
            {
                let k = match Self::subject_aim(w, &b.target) {
                    Some(Aim::Object(k)) => Some(k),
                    Some(Aim::Player) => None,
                    _ => continue,
                };
                owners.push((k, b.visuals.clone(), run.callback, run.callback_left));
            }
            for (k, visuals, callback, remaining) in owners {
                for v in visuals
                    .into_iter()
                    .filter(|v| v.until.is_none_or(|end| end > now))
                {
                    required.insert((k, v.entry));
                    if !w.cache.effects.contains_key(&(k, v.entry)) {
                        let delay =
                            Duration::from_millis(v.start.saturating_sub(now).saturating_mul(125));
                        let cue = if delay.is_zero() {
                            Cue::Silent
                        } else if v.reverse {
                            Cue::Reverse
                        } else {
                            Cue::Forward
                        };
                        let (_, fx) = self.spell_eye(w, v.entry, k, elapsed + delay, cue);
                        if let Some(f) = w.cache.effects.get_mut(&(k, v.entry)) {
                            let age = if callback.is_some()
                                && callback == v.until
                                && let Some(left) = remaining
                                && let Some(ms) = fx["duration_ms"].as_u64()
                            {
                                Duration::from_millis(ms).saturating_sub(left)
                            } else {
                                Duration::from_millis(
                                    now.saturating_sub(v.start).saturating_mul(125),
                                )
                            };
                            f.configure(v.reverse, v.initial, age);
                        }
                    }
                }
            }
            let before = w.cache.effects.len();
            w.cache
                .effects
                .retain(|k, fx| !fx.looping || required.contains(k));
            let removed = before != w.cache.effects.len();
            if removed {
                w.cache.refresh_dynamic_grid();
                w.cache.refresh_shades(None);
                w.cache.refresh_shadows();
            }
            for (owner, id) in required {
                if !w.cache.effects.contains_key(&(owner, id)) {
                    self.spell_eye(w, id, owner, elapsed, Cue::Silent);
                }
            }
            changed |= removed || before != w.cache.effects.len();
        }
        changed
    }
}

impl State {
    pub(in crate::content::world::inventory) fn following_summons(
        &self,
        map: &str,
        inv: &super::super::State,
    ) -> Vec<(OriginalId, Tile)> {
        self.active
            .iter()
            .filter(|r| !r.ending)
            .flat_map(|r| &r.summons)
            .filter(|s| {
                s.map == map
                    && !s.hidden
                    && !self.down(s.id)
                    && self.art.get(&s.model_key(inv)).is_some_and(|m| m.actor)
            })
            .map(|s| (s.id, s.tile))
            .collect()
    }
}

impl InventoryReader {
    pub(in crate::content::world::inventory) fn finish_bodies(
        &mut self,
        w: &mut InteractiveWorld,
        now: Duration,
    ) -> bool {
        // critter_notify_killed interrupts the owning summon. Finish its ART fall before
        // dispatching the spell's existing End / End_Callback and disposal transactions.
        let endings: Vec<_> = w
            .cache
            .inventory
            .magick
            .active
            .iter()
            .enumerate()
            .filter(|(_, r)| {
                !r.ending
                    && r.summons.iter().any(|s| {
                        w.cache.inventory.magick.down(s.id)
                            && !w.cache.inventory.combat.falling(s.id, now)
                    })
            })
            .map(|(i, _)| i)
            .collect();
        let mut changed = !endings.is_empty();
        for index in endings.into_iter().rev() {
            self.end_spell(w, index, now)
        }
        let due: Vec<_> = w
            .cache
            .inventory
            .magick
            .actors
            .iter()
            .filter(|(_, d)| d.dead && d.decay_at.is_some_and(|at| at <= w.game_second))
            .filter_map(|(k, _)| parse_id(k).ok())
            .collect();
        for id in due {
            let Some(Aim::Object(k)) = Self::subject_aim(w, &Subject::Mobile(id)) else {
                continue;
            };
            let Some(tile) = w.cache.target(k).map(|t| t.tile) else {
                continue;
            };
            match self.decay_body(w, id, tile) {
                Ok(()) => {
                    let d = w.cache.inventory.magick.actors.get_mut(&id.key()).unwrap();
                    d.decayed = true;
                    d.decay_at = None;
                    changed = true;
                }
                Err(reason) => w
                    .cache
                    .inventory
                    .combat
                    .log(json!({"event":"decay_unavailable","actor":id,"reason":reason})),
            }
        }
        if changed {
            w.cache.inventory.revision += 1;
            w.refresh_spell_state();
        }
        changed
    }
    fn decay_body(
        &mut self,
        w: &mut InteractiveWorld,
        id: OriginalId,
        tile: Tile,
    ) -> Result<(), String> {
        self.prepare_gear(w, id)?;
        let owner = Owner::OriginalObject(w.map.clone(), id);
        let coins = self.corpse_coins(w, id)?;
        let inv = &w.cache.inventory;
        let mut records = w.records(&mut self.vfs);
        let mut items = records.layout(&inv.state, &owner, None)?.items;
        for c in inv
            .state
            .changes
            .values()
            .filter(|c| matches!(c.place,Place::WornBy(_,who,_) if who==id))
        {
            items.insert(c.item.id, c.item.clone());
        }
        let gold = coins.map(|(item, quantity)| {
            items.insert(item.id, item);
            quantity
        });
        let mut drops = Vec::new();
        for item in items.into_values() {
            let keep = item.flags & ITEM_NO_DISPLAY == 0
                && item.effective.appearance.spell_flags.unwrap_or(0) & 0x4000 == 0;
            let art = keep
                .then(|| records.ground_art(&item, tile, w.reference))
                .transpose()?;
            drops.push((item, art));
        }
        if let Some(amount) = gold {
            w.adjust_npc_gold(&mut self.vfs, &id.key(), -i64::from(amount))?;
            w.cache.inventory.state.next_generated += 1;
        }
        for (item, art) in drops {
            let inv = &mut w.cache.inventory;
            let place = if let Some(art) = art {
                inv.ground.insert(item.id, art);
                Place::Ground(w.map.clone(), tile)
            } else {
                Place::Retired
            };
            inv.state.put(item, place, None);
        }
        Ok(())
    }
}
