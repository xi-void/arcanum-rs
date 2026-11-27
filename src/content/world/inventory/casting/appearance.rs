//! Prepare actor and scenery ART from the original object or trait through the world frame cache.
use super::*;
pub(super) fn standing_pose(vfs: &mut Vfs, art: u32) -> Result<mobile::Pose, String> {
    let names = match art >> 28 {
        12 => Some("art/monster/monster.mes"),
        13 => Some("art/unique_npc/unique_npc.mes"),
        _ => None,
    }
    .map(|path| load_mes(vfs, path))
    .transpose()
    .map_err(|e| e.to_string())?;
    mobile::Pose::decode(art, names.as_ref().map(|t| &t.mes))?
        .resolve_stand(vfs)
        .map_err(|e| e.to_string())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum ModelKey {
    Form(u32, u8),
    Summon(u32, Option<u8>, bool),
    Equipped(u32, Option<u8>, bool, u32),
}
impl ModelKey {
    pub(super) fn summon(n: u32, p: Option<u8>, restore: bool, armor: Option<u32>) -> Self {
        armor.map_or(Self::Summon(n, p, restore), |a| {
            Self::Equipped(n, p, restore, a)
        })
    }
    pub(super) fn load(self, vfs: &mut Vfs, cache: &mut FrameCache) -> Result<Model, String> {
        match self {
            Self::Form(n, p) => form_frame(vfs, cache, n, p),
            Self::Summon(n, p, restore) => spawn_frame(vfs, cache, n, p, restore, None),
            Self::Equipped(n, p, r, a) => spawn_frame(vfs, cache, n, p, r, Some(a)),
        }
    }
}
#[derive(Clone)]
pub(in crate::content::world) struct Model {
    pub(in crate::content::world) art: u32,
    pub(super) actor: bool,
    pub(super) hidden: bool,
    pub(in crate::content::world) additive: bool,
    click_through: bool,
    scale: u32,
    offset: (i32, i32),
    stand: Vec<Arc<FrameLoop>>,
    walk: Vec<Arc<FrameLoop>>,
    run: Vec<Arc<FrameLoop>>,
}
impl Model {
    pub(super) fn facing(&self) -> u8 {
        ((self.art >> 11) & 7) as u8
    }
    pub(in crate::content::world) fn sample(
        &self,
        direction: u8,
        gait: Option<f64>,
        running: bool,
        elapsed: Duration,
    ) -> &Arc<LoadedFrame> {
        let clips = if gait.is_none() || self.walk.is_empty() {
            &self.stand
        } else if running {
            &self.run
        } else {
            &self.walk
        };
        let clip = &clips[usize::from(direction) % clips.len()];
        &clip.frames[gait.map_or_else(
            || clip.index(0, elapsed),
            |phase| ((phase * clip.frames.len() as f64) as usize) % clip.frames.len(),
        )]
    }
    pub(in crate::content::world) fn delay(&self, dir: u8, now: Duration) -> Option<Duration> {
        let clip = &self.stand[usize::from(dir) % self.stand.len()];
        (clip.frames.len() > 1).then(|| clip.delay(now, 1))
    }
    pub(super) fn raster(&self, key: WorldOrder, anchor: (i32, i32), direction: u8) -> Raster {
        let clip = &self.stand[usize::from(direction) % self.stand.len()];
        let anchor = (anchor.0 + self.offset.0, anchor.1 + self.offset.1);
        let mut r = Raster::scaled(key, &clip.frames[0], anchor, self.scale);
        r.object_light = if self.additive {
            ObjectLight::Additive
        } else {
            ObjectLight::Point
        };
        r.click_through = self.click_through;
        r.facing = Some(direction);
        r.actor_art = self.actor.then_some(self.art);
        if clip.frames.len() > 1 {
            r.set_animation(clip, 0)
        }
        r
    }
}
fn clip(
    vfs: &mut Vfs,
    cache: &mut FrameCache,
    pose: &mobile::Pose,
    animate: bool,
) -> Result<Arc<FrameLoop>, String> {
    let load = |v: &mut Vfs, c: &mut FrameCache, i| pose.load(v, c, i).map_err(|e| e.to_string());
    let first = load(vfs, cache, 0)?;
    let count = first.decoded.frame_count;
    let fps = first.decoded.frames_per_second;
    let mut frames = vec![first];
    if animate {
        for i in 1..count {
            frames.push(load(vfs, cache, i)?);
        }
    }
    Ok(Arc::new(FrameLoop::timed(frames, fps.max(1) as u32)?))
}
fn actor(
    vfs: &mut Vfs,
    cache: &mut FrameCache,
    art: u32,
    scale: u32,
    additive: bool,
    auto: bool,
    gaits: bool,
) -> Result<Model, String> {
    let pose = standing_pose(vfs, art)?;
    let mut model = Model {
        art,
        actor: true,
        hidden: false,
        click_through: false,
        additive,
        scale,
        offset: (0, 0),
        stand: vec![],
        walk: vec![],
        run: vec![],
    };
    let gait = |v: &mut Vfs, base: &str, action: char| -> Result<Option<String>, String> {
        let prefix = base
            .strip_suffix("aa.art")
            .or_else(|| base.strip_suffix("ba.art"))
            .ok_or("invalid standing ART name")?;
        for weapon in ['a', 'b'] {
            let path = format!("{prefix}{weapon}{action}.art");
            if v.resolve(&path)
                .map_err(|e| e.to_string())?
                .winner()
                .is_some()
            {
                return Ok(Some(path));
            }
        }
        Ok(None)
    };
    let walk = if gaits {
        gait(vfs, &pose.path, 'b')?
    } else {
        None
    };
    let run = if gaits {
        gait(vfs, &pose.path, 'g')?
    } else {
        None
    };
    for d in 0..8 {
        let mut p = pose.clone().in_direction(d);
        let stand = clip(vfs, cache, &p, auto)?;
        if gaits {
            let w = if let Some(path) = &walk {
                p.path = path.clone();
                clip(vfs, cache, &p, true)?
            } else {
                stand.clone()
            };
            let r = if let Some(path) = &run {
                p.path = path.clone();
                clip(vfs, cache, &p, true)?
            } else {
                w.clone()
            };
            model.walk.push(w);
            model.run.push(r);
        }
        model.stand.push(stand);
    }
    Ok(model)
}
pub(super) fn form_frame(
    vfs: &mut Vfs,
    cache: &mut FrameCache,
    n: u32,
    palette: u8,
) -> Result<Model, String> {
    if n > 31 {
        return Err("invalid monster species".into());
    }
    // MTComponentTrait_ProcFunc enables automatic animation and additive blitting for this TIG species range.
    let animated = (26..=28).contains(&n);
    actor(
        vfs,
        cache,
        0xc0000000 | n << 23 | u32::from(palette) << 4,
        100,
        animated,
        animated,
        true,
    )
}
pub(super) fn spawn_frame(
    vfs: &mut Vfs,
    cache: &mut FrameCache,
    n: u32,
    palette: Option<u8>,
    restore: bool,
    armor: Option<u32>,
) -> Result<Model, String> {
    let (category, d) = spawn_defaults(vfs, n)?;
    let a = d.appearance;
    let art = d
        .current_art
        .or(d.original_art)
        .ok_or("missing summon ART")?;
    let scale = u32::try_from(a.scale.unwrap_or(100)).map_err(|_| "negative object scale")?;
    let additive = a.blit_flags.unwrap_or(0) & 0x10 != 0;
    if category == 16 {
        // The original Trait art_num -1 restores OBJ_F_AID. Its palette distinguishes
        // summons sharing a monster ART file after their appearance animation.
        let base = if restore {
            d.original_art.unwrap_or(art)
        } else {
            art
        };
        let base = if let Some(fragment) = armor.filter(|a| a >> 28 == 6 && base >> 28 == 2) {
            (base & !0xf00000) | ((fragment >> 6) & 15) << 20
        } else {
            base
        };
        let palette = palette.unwrap_or(if restore { ((base >> 4) & 3) as u8 } else { 0 });
        let mut model = actor(
            vfs,
            cache,
            (base & !0x30) | u32::from(palette) << 4,
            scale,
            additive,
            a.critter_flags2.unwrap_or(0) & 2 != 0,
            false,
        )?;
        model.click_through = a.flags.unwrap_or(0) & CLICK_THROUGH != 0;
        return Ok(model);
    }
    let frame = mobile::prop_asset(vfs, cache, &mut BTreeMap::new(), category, art)
        .map_err(|e| e.to_string())?;
    Ok(Model {
        art,
        actor: false,
        click_through: a.flags.unwrap_or(0) & CLICK_THROUGH != 0,
        hidden: a.flags.unwrap_or(0) & 0x100 != 0,
        scale,
        additive,
        offset: (a.offset_x.unwrap_or(0), a.offset_y.unwrap_or(0)),
        stand: vec![Arc::new(FrameLoop::timed(vec![frame], 1)?)],
        walk: vec![],
        run: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires the installed original game"]
    fn installed_college_summons_and_forms_resolve_through_the_world_cache() {
        let root = std::env::var_os("ARCANUM_GAME_DIR").expect("set ARCANUM_GAME_DIR");
        let mut vfs = Vfs::from_game(&root, crate::vfs::Policy::Ce).unwrap();
        let programs = load_mes(&mut vfs, "rules/spelllist.mes").unwrap();
        let mut models = BTreeSet::new();
        for spell in 0..80 {
            let program = Program::decode(&programs.mes, spell).unwrap();
            let restore = program.restores_summoned_art();
            for c in program.components {
                match c.operation {
                    Operation::Summon { prototype, list } => {
                        for aptitude in 0..=100 {
                            models.insert(ModelKey::Summon(
                                summon_proto(prototype, list, aptitude).unwrap(),
                                c.palette,
                                restore,
                            ));
                        }
                    }
                    Operation::Form(n) if n >= 0 => {
                        models.insert(ModelKey::Form(n as u32, c.palette.unwrap_or(0)));
                    }
                    _ => (),
                }
            }
        }
        let mut failures = vec![];
        for key in models {
            let mut frames = FrameCache::default();
            let key = if let ModelKey::Summon(n, p, r) = key {
                let (category, d) = spawn_defaults(&mut vfs, n).unwrap();
                if category == 16 {
                    let gear = loadout::Loadout::prepare(
                        &mut vfs,
                        &mut frames,
                        &d,
                        &mut crate::random::Random::default(),
                    )
                    .unwrap();
                    ModelKey::summon(n, p, r, gear.armor())
                } else {
                    key
                }
            } else {
                key
            };
            let result = key.load(&mut vfs, &mut frames);
            match result {
                Ok(m) => {
                    assert!(m.scale > 0);
                    if m.actor {
                        assert_eq!(m.stand.len(), 8);
                    }
                    eprintln!(
                        "{key:?} {:08x} {} {:?}",
                        m.art, m.scale, m.stand[0].frames[0].source.virtual_path
                    );
                }
                Err(e) => failures.push((key, e)),
            }
        }
        assert!(failures.is_empty(), "{failures:?}");
    }
}
