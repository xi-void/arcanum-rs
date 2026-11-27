//! Project saved object identities, forms and poses into the current sector set.
use super::*;

impl SectorCache {
    pub(in crate::content::world) fn project_spells(&mut self) {
        let Some((map, reference)) = &self.inventory_context else {
            return;
        };
        let state = &self.inventory.magick;
        self.sync_spell_fx = true;
        self.door_states.spell_holds = state
            .active
            .iter()
            .flat_map(|r| &r.bindings)
            .filter_map(|b| match &b.target {
                Subject::Lock(m, lock) if m == map && b.portal != 0 => Some(*lock),
                _ => None,
            })
            .collect();
        self.door_states.summons = state
            .active
            .iter()
            .flat_map(|r| &r.summons)
            .filter(|s| s.blocks && &s.map == map)
            .map(|s| s.tile)
            .collect();
        self.door_states.vacated.clear();
        if !state.offsets.is_empty() {
            self.ordered.retain(|r| {
                let id = self
                    .entries
                    .get(&tile_sector_id((r.key.1.2, r.key.1.1)))
                    .and_then(|s| s.0.interactions.get(&r.key))
                    .and_then(|t| {
                        if let interaction::Action::Npc(id) = t.action {
                            Some(id)
                        } else {
                            None
                        }
                    });
                !id.is_some_and(|id| state.moved.contains_key(&id))
            });
            for (id, (resident, frame)) in &state.moved {
                let p = &state.offsets[&id.key()];
                if &p.map != map {
                    continue;
                }
                if resident.blocks {
                    *self.door_states.vacated.entry(p.source).or_default() += 1;
                    self.door_states.summons.insert(p.tile);
                }
                if !self.active.contains(&tile_sector_id(p.tile)) {
                    continue;
                }
                let Some(anchor) = project_world_tile(p.tile, *reference) else {
                    continue;
                };
                // Runtime actor slots use the saved allocator, shared with summons and item transactions.
                let key = (
                    true,
                    crate::content::scenery::order(
                        p.tile,
                        (0, 0),
                        false,
                        usize::MAX - 2 - 2 * p.sequence as usize,
                    ),
                );
                let mut raster = Raster::new(key, frame, anchor);
                raster.object_light = ObjectLight::Point;
                self.dropped_targets.insert(
                    key,
                    interaction::Target {
                        tile: p.tile,
                        action: interaction::Action::Npc(*id),
                    },
                );
                self.ordered.push(raster);
            }
        }
        for spawn in state
            .active
            .iter()
            .flat_map(|r| &r.summons)
            .filter(|s| s.map == *map && self.active.contains(&tile_sector_id(s.tile)))
        {
            let Some(model) = state.art.get(&spawn.model_key(&self.inventory.state)) else {
                continue;
            };
            let Some(anchor) = project_world_tile(spawn.tile, *reference) else {
                continue;
            };
            let sequence = u64::from_le_bytes(spawn.id.payload[..8].try_into().unwrap()) as usize;
            // Dropped items and runtime mobiles occupy separate parity slots below the PC.
            let key = (
                true,
                crate::content::scenery::order(
                    spawn.tile,
                    (0, 0),
                    false,
                    usize::MAX - 2 - sequence * 2,
                ),
            );
            let mut r = model.raster(key, anchor, spawn.facing);
            r.hidden = spawn.hidden;
            self.dropped_targets.insert(
                key,
                interaction::Target {
                    tile: spawn.tile,
                    action: interaction::Action::Summon(spawn.id),
                },
            );
            self.ordered.push(r)
        }
        for run in &state.active {
            for b in &run.bindings {
                let Subject::Mobile(id) = b.target else {
                    continue;
                };
                if b.stun.is_none() && b.form.is_none() && b.spell & 0x80001 == 0 {
                    continue;
                }
                for r in &mut self.ordered {
                    let tile = (r.key.1.2, r.key.1.1);
                    let subject = self.dropped_targets.get(&r.key).or_else(|| {
                        self.entries
                            .get(&tile_sector_id(tile))?
                            .0
                            .interactions
                            .get(&r.key)
                    });
                    if !matches!(subject.map(|t|&t.action),Some(interaction::Action::Npc(i)|interaction::Action::Summon(i)) if *i==id)
                    {
                        continue;
                    }
                    if let Some(model) = b
                        .form
                        .and_then(|n| state.art.get(&ModelKey::Form(n, b.form_palette)))
                    {
                        *r = model.raster(
                            r.key,
                            r.anchor,
                            r.facing.unwrap_or(r.frame.decoded.stored_rotation),
                        );
                    }
                    if let Some(clip) = b.stun.and_then(|art| state.stun.get(&art)) {
                        r.frame = clip.frames[0].clone();
                        r.animation = Some(RasterAnimation::Loop(clip.clone(), 0));
                        for f in &clip.frames {
                            r.include_frame(f)
                        }
                    }
                    if b.spell & 0x80000 != 0 {
                        r.scale = (r.scale / 2).max(1);
                        r.bounds = RasterBounds::scaled(r.frame_draw(&r.frame), r.scale);
                    }
                    if b.spell & 1 != 0 {
                        r.translucent = true;
                        r.hidden |= self.inventory.character.spell_flags & 0x200 == 0
                            && state.flags(&Subject::Player).0 & 0x200 == 0;
                    }
                }
            }
        }
    }
    pub(in crate::content::world) fn spell_models(&self) -> Value {
        let now = self.effect_time;
        let models:Vec<_>=self.ordered.iter().filter_map(|r|{
            let action=&self.target(r.key)?.action;
            let (interaction::Action::Summon(id)|interaction::Action::Npc(id))=action else{return None};
            if !matches!(action,interaction::Action::Summon(_)) && self.inventory.magick.form(&Subject::Mobile(*id)).is_none(){return None}
            let frame=r.asset_at(now,self.fidget);
            Some(json!({"id":id,"serialized_prototype":self.inventory.magick.spawn(*id).and_then(|s|crate::prototype::BasicPrototypeId(s.prototype).serialized()).map(|p|p.0),"tile":[r.key.1.2,r.key.1.1],"source":crate::content::diagnostics::source(&frame.source),"scale":r.scale,"facing":r.facing,"palette":frame.decoded.stored_palette,"hidden":r.hidden,"frames":frame.decoded.frame_count,"additive":matches!(r.object_light,ObjectLight::Additive)}))
        }).collect();
        let player=self.player.as_ref().map(|p|{
            let pose=p.sample(now);
            let art=self.form_asset(now).unwrap_or(pose.asset);
            json!({"source":crate::content::diagnostics::source(&art.source),"facing":p.facing(),"gait_frame":pose.gait_frame,"frames":art.decoded.frame_count,"form":self.inventory.magick.form(&Subject::Player),"additive":self.player_form().is_some_and(|m|m.additive)})
        });
        json!({"player":player,"objects":models})
    }
    pub(in crate::content::world) fn player_spell_flags(&self) -> u32 {
        self.inventory.character.spell_flags | self.inventory.magick.flags(&Subject::Player).0
    }
    pub(in crate::content::world) fn player_scale(&self) -> u32 {
        if self.player_spell_flags() & 0x80000 != 0 {
            50
        } else {
            100
        }
    }
    pub(in crate::content::world) fn player_form(&self) -> Option<&Model> {
        let state = &self.inventory.magick;
        state
            .bindings(&Subject::Player)
            .find_map(|b| state.art.get(&ModelKey::Form(b.form?, b.form_palette)))
    }
    pub(in crate::content::world) fn form_asset(
        &self,
        elapsed: Duration,
    ) -> Option<&Arc<LoadedFrame>> {
        let model = self.player_form()?;
        let player = self.player.as_ref()?;
        let pose = player.sample(elapsed);
        Some(model.sample(
            player.facing(),
            pose.gait_phase,
            player.is_running(),
            elapsed,
        ))
    }
}
