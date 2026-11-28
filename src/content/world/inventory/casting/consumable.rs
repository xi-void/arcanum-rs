//! Food Use runs its authored attachment or Item_Used spell against a live actor.
//! Validate the whole action before publishing script writes, effects, AP and ownership.
use super::*;
use crate::content::script::{self, Interaction, ItemUse, Transfer};

impl InteractiveWorld {
    pub fn is_food(&self, id: &str) -> bool {
        parse_id(id)
            .ok()
            .and_then(|id| self.cache.inventory.state.changes.get(&id))
            .is_some_and(|c| c.item.category == 9)
    }

    fn food(&self, id: OriginalId) -> Result<&Item, String> {
        let change = self
            .cache
            .inventory
            .state
            .changes
            .get(&id)
            .filter(|c| c.place == Place::Owned(Owner::Player))
            .ok_or("Item must be in your backpack")?;
        let item = &change.item;
        if item.category != 9
            || item.flags & 0x80 == 0
            || !item_visible(item)
            || item.flags & 0x10000 != 0
        {
            return Err("This item cannot be used for recovery".into());
        }
        if let Some(reason) = self.incapacity() {
            return Err(reason.into());
        }
        self.cache
            .inventory
            .combat
            .check(None, 4, self.character_statistics().current_fatigue)?;
        Ok(item)
    }
}

impl InventoryReader {
    pub fn use_backpack_item(
        &mut self,
        w: &mut InteractiveWorld,
        key: &str,
        self_target: bool,
    ) -> Result<(), String> {
        if !w.is_food(key) {
            return w.select_tool(key);
        }
        let id = parse_id(key)?;
        let method = Method::Consumable(id);
        let result: Result<(), String> = (|| {
            if w.ability.casting.is_some() || w.cache.inventory.magick.animation_callback_pending()
            {
                return Err("An action is still in progress".into());
            }
            let targeted = w.food(id)?.flags & 0x100 != 0 && !self_target;
            w.cancel_player_route();
            if targeted {
                w.ability.selected = Some(method);
                w.ability.message = "Select the recipient".into();
            } else {
                let (message, report) = self.consume(w, id, Aim::Player, w.cache.effect_time)?;
                w.ability.message = message;
                w.ability.last = Some(report);
            }
            Ok(())
        })();
        if let Err(reason) = &result {
            w.ability.reject(method, reason);
        }
        result
    }

    pub(in crate::content::world::inventory) fn consume(
        &mut self,
        w: &mut InteractiveWorld,
        id: OriginalId,
        aim: Aim,
        now: Duration,
    ) -> Result<(String, Value), String> {
        let item = w.food(id)?.clone();
        let mut target = self.target(w, aim)?;
        if target.kind != Kind::Actor || target.hp >= target.maximum {
            return Err("Select a living recipient".into());
        }
        let from = w.cache.player.as_ref().ok_or("Player unavailable")?.tile();
        if item.flags & 0x100000 != 0
            && from
                .0
                .abs_diff(target.tile.0)
                .max(from.1.abs_diff(target.tile.1))
                > 1
        {
            return Err("Cannot reach the recipient".into());
        }
        let method = Method::Consumable(id);
        let mut report = json!({"method":method,"prototype":item.prototype,"item":id.key(),"target":target.subject,
            "prototype_source":item.prototype_source.to_string(),"before":[target.hp,target.fatigue,target.poison]});
        if let Some(script) = item.effective.appearance.use_script.filter(|&n| n > 0) {
            self.use_food_script(w, &item, script, &mut target, now, &mut report)?;
        } else {
            let mut programs = Vec::new();
            for spell in item
                .stats
                .spells
                .into_iter()
                .map_while(|n| n.filter(|&n| n != 10000))
            {
                let spell = spell as usize;
                let inv = &w.cache.inventory;
                let p = inv
                    .rules
                    .progression
                    .item_programs
                    .get(&spell)
                    .ok_or_else(|| format!("No supported Item_Used program for spell {spell}"))?
                    .clone()?;
                if p.aggressive
                    || p.maintain.is_some()
                    || p.triggers.is_some()
                    || !target.matches(&p.targets)?
                    || from
                        .0
                        .abs_diff(target.tile.0)
                        .max(from.1.abs_diff(target.tile.1))
                        > p.range
                {
                    return Err("This item cannot affect that recipient".into());
                }
                if p.no_stack
                    && inv.magick.active.iter().any(|r| {
                        r.spell == spell && r.bindings.iter().any(|b| b.target == target.subject)
                    })
                {
                    return Err("Item effect is already active".into());
                }
                for c in &p.components {
                    let supported = match &c.operation {
                        Operation::Vital { heal: true, .. } => c.phase == Phase::Begin,
                        Operation::Eye { .. } => true,
                        Operation::Effect { id, .. } => {
                            p.duration.is_some()
                                && inv.rules.progression.spell_effects.contains_key(id)
                        }
                        Operation::Flag {
                            field: Field::Critter,
                            ..
                        } => p.duration.is_some(),
                        _ => false,
                    };
                    if !supported
                        || !matches!(c.phase, Phase::Begin | Phase::End)
                        || !c.targets.is_empty()
                        || !c.apply.is_empty()
                    {
                        return Err(format!("Unsupported consumable component in spell {spell}"));
                    }
                }
                programs.push((spell, p));
            }
            if programs.is_empty() {
                return Err("Item has no recovery effect".into());
            }
            report["programs"] = json!(programs.iter().map(|(id, _)| id).collect::<Vec<_>>());
            report["source"] = json!(
                w.progression_rules()
                    .spell_source
                    .as_ref()
                    .map(ToString::to_string)
            );
            let aptitude = w.character_statistics().aptitude;
            for (spell, p) in programs {
                let mut b = Binding::new(target.subject.clone());
                b.maximum = target.maximum;
                for c in p.components.iter().filter(|c| c.phase == Phase::Begin) {
                    match &c.operation {
                        Operation::Vital { .. } => {
                            apply_vital(&mut w.cache.inventory, &mut target, c, aptitude, 0)
                        }
                        Operation::Eye {
                            index,
                            add: true,
                            play,
                        } => {
                            let (v, fx) = self.start_attachment(
                                w,
                                target.key,
                                spell as i32 * 10 + index,
                                play,
                                now,
                                b.visuals.last(),
                            );
                            b.visuals.extend(v);
                            report["effect"] = fx;
                        }
                        Operation::Effect {
                            id,
                            add: true,
                            count,
                            scaled,
                        } => b.effects.push((
                            *id,
                            count
                                .saturating_add(scaled.saturating_mul(aptitude) / 100)
                                .max(0),
                        )),
                        Operation::Flag {
                            field: Field::Critter,
                            mask,
                            on: true,
                        } => b.critter |= mask,
                        _ => (),
                    }
                }
                if let Some(period) = p.duration {
                    w.cache.inventory.magick.active.push(Active {
                        caster: None,
                        spell,
                        bindings: vec![b],
                        summons: vec![],
                        due: Some(w.game_second.saturating_add(period)),
                        ticks: None,
                        aptitude,
                        callback: None,
                        callback_left: None,
                        ending: false,
                        center: Some((w.map.clone(), target.tile)),
                    });
                }
            }
        }
        let inv = &mut w.cache.inventory;
        inv.state.put(item, Place::Consumed, None);
        report["ap_cost"] = json!(inv.pay_combat_ap(None, 4));

        if target.subject == Subject::Player {
            target.fatigue = inv.character.fatigue_damage;
        }
        report["after"] = json!([target.hp, target.fatigue, target.poison]);
        report["outcome"] = json!("consumed");
        inv.revision += 1;
        if inv.combat.active() {
            inv.combat.log(report.clone());
        }
        w.refresh_spell_state();
        Ok(("Item used".into(), report))
    }

    fn use_food_script(
        &mut self,
        w: &mut InteractiveWorld,
        item: &Item,
        script: i32,
        t: &mut Target,
        now: Duration,
        report: &mut Value,
    ) -> Result<(), String> {
        let handler = script::Script::load(&mut self.vfs, script)?;
        let stats = t.stats.as_ref().ok_or("Missing recipient vitals")?;
        let caster = w.character_statistics();
        let mut target_stats = stats.script_values();
        target_stats[24] = Some(t.poison);
        let mut state = w.cache.scripts.clone();
        state.interaction = Some(Interaction {
            item_use: Some(ItemUse {
                target_stats,
                power_ratio: item.stats.power_ratio(stats.aptitude),
                ..ItemUse::default()
            }),
            vitals: [
                [stats.current_health, stats.max_health],
                [stats.current_fatigue, stats.max_fatigue],
            ]
            .map(|p| p.map(|n| n.clamp(i32::MIN.into(), i32::MAX.into()) as i32)),
            stats: caster.script_values(),
            aptitude: caster.aptitude,
            ..Interaction::default()
        });
        let attachment = script::Attachment {
            owner: item.id.key(),
            event: script::Event::Use,
            script,
            initial: item.effective.appearance.use_local.unwrap_or_default(),
        };
        let input = script::Invocation {
            attachment: &attachment,
            triggerer: script::Actor::Player,
            position: 0,
            hour: w.cache.hour,
            follows_player: None,
            npc_flags: None,
            locations: [w.cache.player.as_ref().map(Player::tile), None],
            met: None,
        };
        let result = handler
            .execute(&input, &mut state)
            .map_err(|e| format!("{}: {e}", handler.source))?;
        let facts = state.interaction.take().unwrap();
        let plan = facts.item_use.unwrap();
        if !plan.destroy || !matches!(result.outcome, script::Outcome::Return { .. }) {
            return Err(format!(
                "Item use declined at {} statement {}",
                handler.source.virtual_path, result.statement
            ));
        }
        if facts
            .operations
            .iter()
            .any(|o| !matches!(o, Transfer::Heal { .. }))
        {
            return Err("Unsupported item Use operation".into());
        }
        for op in facts.operations {
            if let Transfer::Heal { fatigue, amount } = op {
                w.cache.inventory.change_vitals(
                    t,
                    if fatigue {
                        Channel::Fatigue
                    } else {
                        Channel::Normal
                    },
                    -amount,
                );
            }
        }
        if plan.poison_heal > 0 {
            w.cache
                .inventory
                .change_vitals(t, Channel::Poison, -plan.poison_heal);
        }
        w.cache.scripts = state;
        report["script"] = json!({"path":handler.source.virtual_path,"source":handler.source.to_string(),"statement":result.statement,"power_percent":plan.power_ratio});
        for eye in plan.visuals {
            let visual =
                spell_fx::Definition::from_table(&mut self.vfs, "rules/scripteyecandy.mes", eye)
                    .and_then(|def| match def {
                        Some(def) => w
                            .play_spell_effect(
                                &mut self.vfs,
                                &mut self.frames,
                                &def,
                                t.key,
                                now,
                                Cue::Forward,
                            )
                            .map(|(_, fx)| fx),
                        None => Ok(Value::Null),
                    });
            report["effect"] = visual.unwrap_or_else(|e| json!({"error":e.to_string()}));
        }
        Ok(())
    }
}
