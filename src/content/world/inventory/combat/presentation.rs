//! Directional actor poses sampled by the world renderer and pointer picking.
use super::*;
use crate::content::player::{gait_period, stride_offset, tile_period};

pub(super) enum FallStart {
    New,
    Resume(Duration),
    Settled,
}

#[derive(Clone, Copy)]
enum Action {
    Strike,
    Fall {
        dead: bool,
        settled: bool,
        elapsed: Duration,
    },
    Walk {
        delta: (i32, i32),
        duration: Duration,
        period: Duration,
        phase: Duration,
    },
}
#[derive(Clone)]
pub(super) struct Motion {
    clip: Arc<FrameLoop>,
    start: Duration,
    dir: u8,
    action: Action,
}
impl Motion {
    fn index(&self, now: Duration) -> Option<usize> {
        let elapsed = now.saturating_sub(self.start);
        match self.action {
            Action::Walk {
                duration,
                period,
                phase,
                ..
            } => (elapsed < duration).then(|| {
                ((phase + elapsed).as_nanos() / period.as_nanos() % self.clip.frames.len() as u128)
                    as usize
            }),
            Action::Strike => self.clip.one_shot_index(elapsed),
            Action::Fall {
                settled,
                elapsed: prior,
                ..
            } => Some(if settled {
                self.clip.frames.len() - 1
            } else {
                self.clip
                    .one_shot_index(elapsed + prior)
                    .unwrap_or(self.clip.frames.len() - 1)
            }),
        }
    }
    pub(super) fn animating(&self, now: Duration) -> bool {
        match self.action {
            Action::Fall { settled, .. } => {
                !settled && self.index(now) != Some(self.clip.frames.len() - 1)
            }
            _ => self.index(now).is_some(),
        }
    }
    pub(super) fn awaiting_frame(&self, now: Duration) -> bool {
        matches!(self.action, Action::Fall { settled: false, .. }) || self.animating(now)
    }
    pub(super) fn needs_redraw(&self, last: Option<Duration>, now: Duration) -> bool {
        self.animating(now) || last.is_some_and(|t| self.animating(t))
    }
    pub(super) fn settle(&mut self, now: Duration) -> bool {
        if !self.animating(now)
            && let Action::Fall { settled, .. } = &mut self.action
            && !*settled
        {
            *settled = true;
            return true;
        }
        false
    }
    pub(super) fn marker(&self) -> Duration {
        let n = self.clip.frames[0]
            .decoded
            .action_frame
            .clamp(0, self.clip.frames.len() as i32 - 1);
        self.start + self.clip.period() * n as u32
    }
    pub(super) fn hold(&self) -> bool {
        matches!(self.action, Action::Fall { .. })
    }
    pub(super) fn dead(&self) -> bool {
        matches!(self.action, Action::Fall { dead: true, .. })
    }
    fn offset(&self, now: Duration) -> (i32, i32) {
        if let Action::Walk {
            delta, duration, ..
        } = self.action
        {
            let moved = stride_offset(delta, now.saturating_sub(self.start), duration);
            (moved.0 - delta.0, moved.1 - delta.1)
        } else {
            (0, 0)
        }
    }
    pub(super) fn adjoining_step(&self, now: Duration) -> bool {
        matches!(self.action, Action::Walk { duration, .. } if now <= self.start + duration + Duration::from_millis(160))
    }
    fn continuation(&self, dir: u8, now: Duration) -> Duration {
        if let Action::Walk {
            phase, duration, ..
        } = self.action
            && self.dir == dir
            && self.adjoining_step(now)
        {
            phase + duration
        } else {
            Duration::ZERO
        }
    }
}
impl Combat {
    pub(in crate::content::world::inventory) fn snapshot(&self, now: Duration) -> Self {
        let mut saved = self.clone();
        for &id in &self.falls {
            saved.fall_progress.push((id, 0));
        }
        for (id, p) in &self.poses {
            if let Some(id) = id
                && let Action::Fall {
                    dead: true,
                    elapsed,
                    ..
                } = p.action
                && p.animating(now)
            {
                saved.fall_progress.push((
                    *id,
                    (elapsed + now.saturating_sub(p.start)).as_millis() as u64,
                ));
            }
        }
        saved
    }
    pub(in crate::content::world) fn sprite(
        &self,
        actor: Actor,
        now: Duration,
    ) -> Option<(&Arc<LoadedFrame>, (i32, i32))> {
        let p = self.poses.get(&actor)?;
        Some((&p.clip.frames[p.index(now)?], p.offset(now)))
    }
    pub(in crate::content::world) fn pose(
        &self,
        actor: Actor,
        now: Duration,
    ) -> Option<&Arc<LoadedFrame>> {
        Some(self.sprite(actor, now)?.0)
    }
    pub(in crate::content::world::inventory) fn falling(
        &self,
        id: OriginalId,
        now: Duration,
    ) -> bool {
        self.fall_progress.iter().any(|(saved, _)| *saved == id)
            || self.falls.contains(&id)
            || self
                .poses
                .get(&Some(id))
                .is_some_and(|p| p.dead() && p.animating(now))
    }
    pub(super) fn pose_report(&self, now: Duration) -> Value {
        json!(self.poses.iter().map(|(id,p)|json!({"actor":subject(*id),"path":p.clip.frames[0].source.virtual_path,
            "frame":p.index(now),"frames":p.clip.frames.len(),"facing":p.dir,"offset":p.offset(now),"down":p.hold(),"dead":p.dead(),
            "action": match p.action {Action::Walk{..}=>"walk",Action::Strike=>"attack",Action::Fall{..} if p.animating(now)=>"fall",Action::Fall{dead:true,..}=>"corpse",Action::Fall{..}=>"prone"}})).collect::<Vec<_>>())
    }
}
impl InventoryReader {
    fn actor_clip(
        &mut self,
        w: &InteractiveWorld,
        id: Actor,
        weapon: u8,
        dir: u8,
        actions: &[u8],
    ) -> Result<Arc<FrameLoop>, String> {
        let frame = if let Some(id) = id {
            let Some(Aim::Object(key)) = Self::subject_aim(w, &Subject::Mobile(id)) else {
                return Err("Actor outside the loaded map".into());
            };
            &w.cache.ordered[w
                .cache
                .ordered
                .binary_search_by_key(&key, |r| r.key)
                .map_err(|_| "Missing actor raster")?]
            .frame
        } else {
            let player = w.cache.player.as_ref().ok_or("Missing player")?;
            w.cache
                .form_asset(w.cache.effect_time)
                .unwrap_or(player.sample(w.cache.effect_time).asset)
        };
        let path = &frame.source.virtual_path;
        let stem = path
            .strip_suffix(".art")
            .and_then(|s| s.get(..s.len().checked_sub(2)?))
            .ok_or("Invalid critter ART path")?;
        for &anim in actions {
            for kind in [weapon, 1, 0] {
                let code = b"abcdefghijkxynz"
                    .get(usize::from(kind))
                    .copied()
                    .unwrap_or(b'b');
                let path = format!("{stem}{}{}.art", char::from(code), char::from(b'a' + anim));
                if self
                    .vfs
                    .resolve(&path)
                    .map_err(|e| e.to_string())?
                    .winner()
                    .is_none()
                {
                    continue;
                }
                let pose = mobile::Pose::new(u32::from(frame.decoded.stored_palette) << 4, path)
                    .in_direction(dir);
                return casting::motion_clip(&mut self.vfs, &mut self.frames, &pose);
            }
        }
        Err(format!(
            "No directional ART for {path}, actions {actions:?}, weapon {weapon}"
        ))
    }
    pub(super) fn strike_pose(
        &mut self,
        w: &mut InteractiveWorld,
        id: Actor,
        weapon: u8,
        dir: u8,
        now: Duration,
    ) {
        if let Some(id) = id {
            w.cache.inventory.magick.face(id, dir)
        }
        let clip = self.actor_clip(w, id, weapon, dir, &[20]);
        self.install_pose(w, id, dir, now, Action::Strike, clip);
    }
    fn install_pose(
        &self,
        w: &mut InteractiveWorld,
        id: Actor,
        dir: u8,
        now: Duration,
        action: Action,
        clip: Result<Arc<FrameLoop>, String>,
    ) {
        let c = &mut w.cache.inventory.combat;
        match clip {
            Ok(clip) => {
                c.poses.insert(
                    id,
                    Motion {
                        clip,
                        start: now,
                        dir,
                        action,
                    },
                );
            }
            Err(reason) => {
                c.log(json!({"event":"animation_unavailable","actor":subject(id),"reason":reason}))
            }
        }
    }
    pub(super) fn downed_pose(
        &mut self,
        w: &mut InteractiveWorld,
        id: Actor,
        now: Duration,
        dead: bool,
    ) {
        if w.cache
            .inventory
            .combat
            .poses
            .get(&id)
            .is_some_and(|p| p.hold() && p.dead() == dead)
        {
            return;
        }
        // A corpse re-entering view resumes its final pose. Only a fresh lethal hit starts a fall.
        let fresh = id.is_some_and(|id| w.cache.inventory.combat.falls.remove(&id));
        let start = if dead && id.is_some() && !fresh {
            FallStart::Settled
        } else {
            FallStart::New
        };
        self.start_fall(w, id, now, dead, start);
    }
    pub(super) fn start_fall(
        &mut self,
        w: &mut InteractiveWorld,
        id: Actor,
        now: Duration,
        dead: bool,
        start: FallStart,
    ) {
        if id.is_some_and(|id| w.cache.inventory.magick.destroying(id)) {
            w.cache.inventory.combat.poses.remove(&id);
            return;
        }
        let dir = if let Some(id) = id {
            w.cache
                .inventory
                .magick
                .facing(id)
                .or_else(|| {
                    let Aim::Object(k) = Self::subject_aim(w, &Subject::Mobile(id))? else {
                        return None;
                    };
                    w.cache
                        .npc(k)
                        .and_then(|(r, _)| r.current_art.value().map(|a| ((a >> 11) & 7) as u8))
                        .or_else(|| w.cache.ordered.iter().find(|r| r.key == k)?.facing)
                })
                .unwrap_or(4)
        } else {
            w.cache.player.as_ref().map_or(4, Player::facing)
        };
        let anim = id
            .filter(|_| dead)
            .map_or(7, |id| w.cache.inventory.magick.death_anim(id));
        let clip = self.actor_clip(w, id, 0, dir, &[anim, 7]);
        if dead
            && matches!(start, FallStart::New)
            && let Ok(t) = self.fighter(w, id)
            && let Some(base) = t.sound.filter(|n| *n > 0)
        {
            let gruesome = clip
                .as_ref()
                .is_ok_and(|c| !c.frames[0].source.virtual_path.ends_with("h.art"));
            w.queue_sound(SoundEvent::Effect(base + if gruesome { 2 } else { 1 }));
            w.cache.inventory.combat.log(json!({"event":"death_sound","actor":subject(id),"sound":base+if gruesome{2}else{1}}));
        }
        self.install_pose(
            w,
            id,
            dir,
            now,
            Action::Fall {
                dead,
                settled: matches!(start, FallStart::Settled),
                elapsed: if let FallStart::Resume(age) = start {
                    age
                } else {
                    Duration::ZERO
                },
            },
            clip,
        );
    }
    pub(super) fn walk_actor(
        &mut self,
        w: &mut InteractiveWorld,
        t: &Target,
        id: OriginalId,
        dir: u8,
        to: Tile,
        now: Duration,
    ) -> Result<Duration, String> {
        let weapon = if w.cache.inventory.combat.active() {
            w.cache.inventory.weapon(Some(id), t).art
        } else {
            0
        };
        let clip = self.actor_clip(w, Some(id), weapon, dir, &[1])?;
        let key = t.key.ok_or("No actor raster")?;
        let art = w
            .cache
            .npc(key)
            .and_then(|(r, _)| r.current_art.value())
            .or_else(|| w.cache.ordered.iter().find(|r| r.key == key)?.actor_art)
            .unwrap_or(0);
        let speed = t.stats.as_ref().and_then(|s| s.speed).unwrap_or(8);
        let period = gait_period(art, speed, false, clip.period());
        let duration = tile_period(
            dir,
            clip.frames.iter().map(|f| f.decoded.frame.offset),
            period,
        )
        .ok_or_else(|| {
            format!(
                "Invalid directional stride in {} facing {dir}",
                clip.frames[0].source.virtual_path
            )
        })?;
        let delta = project_world_tile(to, t.tile).ok_or("Unprojectable actor step")?;
        let phase = w
            .cache
            .inventory
            .combat
            .poses
            .get(&Some(id))
            .map_or(Duration::ZERO, |p| p.continuation(dir, now));
        // Prepare the original model's facing before committing the step, even in a temporary form.
        let standing = w
            .cache
            .npc(key)
            .map(|(resident, _)| {
                let aid = resident.current_art.value().ok_or("Missing resident ART")?;
                super::super::casting::standing(
                    &mut self.vfs,
                    &mut self.frames,
                    (aid & !(7 << 11)) | u32::from(dir) << 11,
                )
            })
            .transpose()?;
        w.relocate_actor(id, Some(key), t.tile, to);
        w.cache.inventory.magick.face(id, dir);
        if let Some(frame) = standing {
            w.cache.inventory.magick.set_resident_frame(id, frame);
        }
        w.cache.activate(&w.cache.active.clone());
        self.install_pose(
            w,
            Some(id),
            dir,
            now,
            Action::Walk {
                delta,
                duration,
                period,
                phase,
            },
            Ok(clip),
        );
        Ok(duration)
    }
}

pub(super) struct Indicator {
    definition: spell_fx::Definition,
    art: FrameLoop,
    hostile: [u8; 3],
    friendly: [u8; 3],
}
impl Indicator {
    pub(super) fn load(vfs: &mut Vfs, frames: &mut FrameCache) -> Result<Arc<Self>, String> {
        let definition = spell_fx::Definition::from_table(vfs, "rules/combateyecandy.mes", 0)
            .map_err(|e| e.to_string())?
            .ok_or("Missing combat EyeCandy")?;
        let ([back, _], _) = definition
            .clips(vfs, frames, 0)
            .map_err(|e| e.to_string())?;
        let table = load_mes(vfs, "rules/reaction_colors.mes").map_err(|e| e.to_string())?;
        let color = |n| -> Result<[u8; 3], String> {
            table
                .mes
                .get(n)
                .ok_or("Missing reaction color")?
                .split(',')
                .map(|c| c.trim().parse::<u8>().map_err(|e| e.to_string()))
                .collect::<Result<Vec<_>, _>>()?
                .try_into()
                .map_err(|_| "Expected RGB reaction color".into())
        };
        Ok(Arc::new(Self {
            definition,
            art: back.ok_or("Combat EyeCandy has no underlay")?,
            hostile: color(6)?,
            friendly: color(0)?,
        }))
    }
}
impl Combat {
    pub(in crate::content::world) fn indicator(
        &self,
        cache: &SectorCache,
        r: &Raster,
        v: Viewport,
        elapsed: Duration,
    ) -> Option<WorldDraw<'_>> {
        if !self.timed() {
            return None;
        }
        let indicator = self.indicator.as_ref()?.as_ref().ok()?;
        let actor = actor_at(cache, r.key)?;
        let member = self.party.iter().find(|p| p.actor == Some(actor))?;
        let asset = &indicator.art.frames[indicator.art.index(0, elapsed)];
        let offset = self
            .poses
            .get(&Some(actor))
            .map_or((0, 0), |p| p.offset(elapsed));
        let draw = SpriteDraw {
            frame: &asset.decoded.frame,
            anchor: (r.anchor.0 + offset.0, r.anchor.1 + offset.1),
        };
        let scale = indicator.definition.scale(
            r.actor_art
                .or_else(|| cache.npc(r.key)?.0.current_art.value()),
        );
        let friendly = member.ally
            || cache.inventory.magick.flags(&Subject::Mobile(actor)).0 & 0x08000000 != 0;
        Some(WorldDraw {
            asset,
            scale,
            sprite: v.place(draw, RasterBounds::scaled(draw, scale))?,
            modulation: Modulation::AdditiveTint(if friendly {
                indicator.friendly
            } else {
                indicator.hostile
            }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires the installed original game"]
    fn installed_falling_actor_resumes_its_clip_and_holds_the_corpse_frame() {
        let game = std::env::var_os("ARCANUM_GAME_DIR").expect("set ARCANUM_GAME_DIR");
        let mut vfs = Vfs::from_game(&game, crate::vfs::Policy::Ce).unwrap();
        let pose = mobile::Pose::new(0, "art/monster/wlf/wlfuwxas.art".into()).in_direction(1);
        let clip = casting::motion_clip(&mut vfs, &mut FrameCache::default(), &pose).unwrap();
        let period = clip.period();
        let final_frame = clip.frames.len() - 1;
        assert!(final_frame > 3);
        let start = Duration::from_secs(15);
        let mut motion = Motion {
            clip,
            start,
            dir: 1,
            action: Action::Fall {
                dead: true,
                settled: false,
                elapsed: period * 2,
            },
        };
        assert_eq!(motion.index(start), Some(2));
        assert_eq!(motion.index(start + period), Some(3));
        assert!(!motion.settle(start + period));
        let end = start + period * (final_frame as u32 - 2);
        assert!(motion.settle(end));
        assert!(!motion.settle(end + period));
        assert_eq!(
            motion.index(end + Duration::from_secs(60)),
            Some(final_frame)
        );
        assert!(!motion.animating(end));
    }
}
