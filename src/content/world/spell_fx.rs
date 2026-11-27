//! One-shot spell overlays from SpellEyeCandy.mes, attached to the same depth layer as their owner.
use super::*;
use crate::{mes::Mes, resource_path::normalize};
use serde_json::{Value, json};
use std::error::Error;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

#[derive(Clone, Copy)]
pub(super) enum Cue {
    Forward,
    Reverse,
    Silent,
}
impl Cue {
    pub(super) fn for_play(play: &[String]) -> Self {
        if play.iter().any(|s| s == "reverse") {
            Self::Reverse
        } else {
            Self::Forward
        }
    }
    fn event(self, sound: Option<i32>) -> Option<SoundEvent> {
        let id = sound.filter(|&n| n >= 0)?;
        match self {
            Self::Forward => Some(SoundEvent::Effect(id)),
            Self::Reverse => Some(SoundEvent::EffectVariant {
                preferred: id - id % 10 + 8,
                fallback: id,
            }),
            Self::Silent => None,
        }
    }
}

pub(super) struct Definition {
    basename: String,
    pub(super) looping: bool,
    pub(super) projectile_speed: u32,
    authored_scale: u32,
    underlay: bool,
    entry: i32,
    palette: u8,
    autoscale: bool,
    additive: bool,
    layers: [bool; 2], // background, foreground
    sound: Option<i32>,
    light: Option<(String, [u8; 3])>,
    source: super::super::Source,
}

fn asset_name(table: &Mes, id: i32) -> Result<String> {
    let name = normalize(table.get(id).ok_or("missing ART name")?.trim())?;
    if name.contains('/') {
        return Err("expected an ART basename".into());
    }
    Ok(name)
}

impl Definition {
    fn scale(&self, art: Option<u32>) -> u32 {
        if !self.autoscale {
            return self.authored_scale;
        }
        match art {
            Some(a) if a >> 28 == 2 => match (a >> 24) & 7 {
                1 | 2 => 62,
                3 => 100,
                _ => 87,
            },
            Some(a) if a >> 28 == 12 => {
                if (a >> 23) & 31 < 2 {
                    62
                } else {
                    87
                }
            }
            _ => 100,
        }
    }
    fn clips(
        &self,
        vfs: &mut Vfs,
        frames: &mut FrameCache,
        rotation: u8,
    ) -> Result<([Option<FrameLoop>; 2], Duration)> {
        let mut layers = [None, None];
        let mut duration = Duration::ZERO;
        for (index, suffix) in [if self.underlay { "u" } else { "b" }, "f"]
            .into_iter()
            .enumerate()
            .filter(|(i, _)| self.layers[*i])
        {
            let path = format!("art/eye_candy/{}_{suffix}.art", self.basename);
            let mut load = |i| frames.load_rotated(vfs, &path, i, self.palette, false, rotation);
            let first = load(0)?;
            let fps = u32::try_from(first.decoded.frames_per_second)?;
            let mut sequence = Vec::with_capacity(first.decoded.frame_count);
            let count = first.decoded.frame_count;
            sequence.push(first);
            for frame in 1..count {
                sequence.push(load(frame)?);
            }
            let clip = FrameLoop::timed(sequence, fps)?;
            duration = duration.max(clip.period() * count as u32);
            layers[index] = Some(clip);
        }
        Ok((layers, duration))
    }
    pub fn load(vfs: &mut Vfs, entry: i32) -> Result<Option<Self>> {
        let table = load_mes(vfs, "rules/spelleyecandy.mes")?;
        let Some(line) = table.mes.get(entry).filter(|line| !line.trim().is_empty()) else {
            return Ok(None);
        };
        let mut art = None;
        let mut palette = 0;
        let mut autoscale = false;
        let mut authored_scale = 100;
        let mut looping = false;
        let mut underlay = false;
        let mut projectile_speed = 4;
        let mut additive = false;
        let mut layers = [false; 2];
        let mut sound = None;
        let mut light = None;
        let mut color = [255; 3];
        for part in line.split(',') {
            let (key, value) = part.split_once(':').ok_or("invalid EyeCandy clause")?;
            let value = value.trim().to_ascii_lowercase();
            match key.trim().to_ascii_lowercase().as_str() {
                "art" => art = Some(value.parse()?),
                "palette" => palette = value.parse()?,
                "scale" => {
                    let value: u32 = value.trim_end_matches('%').parse()?;
                    if ![50, 62, 75, 87, 100, 125, 150, 200].contains(&value) {
                        return Err("unsupported EyeCandy scale".into());
                    }
                    autoscale = true;
                    authored_scale = value;
                }
                "blend" => {
                    additive = match value.as_str() {
                        "add" => true,
                        "normal" | "none" => false,
                        _ => return Err("unsupported EyeCandy blend".into()),
                    }
                }
                "sound" => {
                    // TIG parses the complete comma-delimited token with atoi.
                    // An empty Sound field followed by other text consumes that text.
                    let digits = value.trim_start_matches(['-', '+']);
                    let n = digits.bytes().take_while(u8::is_ascii_digit).count();
                    sound = Some(if n == 0 {
                        0
                    } else {
                        value[..value.len() - digits.len() + n].parse()?
                    });
                }
                "proj speed" => {
                    let n: u32 = value.parse()?;
                    if n != 0 {
                        projectile_speed = n
                    }
                }
                "flags" => {
                    for flag in value.split('|').map(str::trim) {
                        match flag {
                            "overlay_f" => layers[1] = true,
                            "overlay_b" => layers[0] = true,
                            "overlay_fb" => layers = [true; 2],
                            "animates" => (),
                            "anim_forever" => looping = true,
                            "underlay" => {
                                layers[0] = true;
                                underlay = true
                            }
                            _ => return Err(format!("unsupported EyeCandy flag {flag}").into()),
                        }
                    }
                }
                "light" => light = Some(value.parse()?),
                "light color" => {
                    let values = value
                        .trim_matches(['(', ')'])
                        .split('@')
                        .map(|n| n.trim().parse::<u8>())
                        .collect::<std::result::Result<Vec<_>, _>>()?;
                    color = values.try_into().map_err(|_| "expected RGB light color")?;
                }
                _ => return Err(format!("unsupported EyeCandy field {key}").into()),
            }
        }
        // CE animfx_build_eye_candy_effect derives unspecified sounds from the caster entry.
        let slot = (entry % 10) as usize;
        if sound.is_none()
            && slot != 0
            && let Some(offset) = [0i32, 4, 5, 6, 3, 7].get(slot)
        {
            sound = table
                .mes
                .get(entry - slot as i32)
                .and_then(|line| {
                    line.split(',').find_map(|c| {
                        let (key, value) = c.split_once(':')?;
                        key.trim()
                            .eq_ignore_ascii_case("sound")
                            .then(|| value.trim().parse::<i32>().ok())
                            .flatten()
                    })
                })
                .filter(|id| *id >= 0)
                .and_then(|id| id.checked_add(*offset));
        }
        let names = load_mes(vfs, "art/eye_candy/eye_candy.mes")?;
        let basename = asset_name(&names.mes, art.ok_or("missing EyeCandy ART")?)?;
        let light = match light {
            Some(id) => Some((
                format!(
                    "art/light/{}.art",
                    asset_name(&load_mes(vfs, "art/light/light.mes")?.mes, id)?
                ),
                color,
            )),
            None => None,
        };
        Ok(Some(Self {
            basename,
            authored_scale,
            looping,
            entry,
            underlay,
            projectile_speed,
            palette,
            autoscale,
            additive,
            layers,
            sound,
            light,
            source: table.source,
        }))
    }
}

pub(super) struct Playback {
    start: Duration,
    pending_sound: Option<SoundEvent>,
    pub(super) looping: bool,
    reverse: bool,
    initial: usize,
    age_offset: Duration,
    rotation: u8,
    end: Duration,
    destination: Option<(i32, i32)>,
    layers: [Option<FrameLoop>; 2],
    scale: u32,
    additive: bool,
    pub(super) tile: (u32, u32),
    anchor: (i32, i32),
    pub light: Option<StaticLight>,
}

impl Playback {
    pub(super) fn light_at(&self, now: Duration) -> Option<&StaticLight> {
        self.light.as_ref().filter(|_| now >= self.start)
    }
    fn clip(&self) -> &FrameLoop {
        self.layers
            .iter()
            .rev()
            .flatten()
            .next()
            .expect("prepared overlay layer")
    }
    fn frame(&self, layer: &FrameLoop, age: Duration) -> Option<usize> {
        let age = age + self.age_offset;
        let frame = if self.looping || self.destination.is_some() {
            layer.index(self.initial, age)
        } else {
            layer.one_shot_index(age)?
        };
        Some(if self.reverse {
            layer.frames.len() - 1 - frame
        } else {
            frame
        })
    }
    pub(super) fn configure(&mut self, reverse: bool, initial: usize, age: Duration) {
        self.reverse = reverse;
        self.initial = initial;
        self.age_offset = age;
        self.end = self.end.saturating_sub(age);
    }
    fn action_delay(&self) -> Duration {
        let clip = self.clip();
        let marker = clip.frames[0]
            .decoded
            .action_frame
            .clamp(0, (clip.frames.len() - 1) as i32);
        clip.period() * marker as u32
    }
    pub(super) fn draw(
        &self,
        front: bool,
        now: Duration,
        viewport: Viewport,
        anchor: Option<(i32, i32)>,
    ) -> Option<WorldDraw<'_>> {
        let layer = self.layers[usize::from(front)].as_ref()?;
        let age = now.checked_sub(self.start)?;
        let index = self.frame(layer, age)?;
        let asset = &layer.frames[index];
        let draw = SpriteDraw {
            frame: &asset.decoded.frame,
            anchor: if let Some(to) = self.destination {
                let f = (age.as_secs_f64() / (self.end - self.start).as_secs_f64()).min(1.);
                let pos = |a: i32, b: i32| {
                    (f64::from(a) + (f64::from(b) - f64::from(a)) * f).round() as i32
                };
                (pos(self.anchor.0, to.0), pos(self.anchor.1, to.1))
            } else {
                anchor.unwrap_or(self.anchor)
            },
        };
        Some(WorldDraw {
            asset,
            sprite: viewport.place(draw, RasterBounds::scaled(draw, self.scale))?,
            scale: self.scale,
            modulation: if self.additive {
                Modulation::Additive
            } else {
                Modulation::Unlit
            },
        })
    }
    fn report(&self, now: Duration) -> Value {
        json!({"tile":self.tile,"rotation":self.rotation,"reverse":self.reverse,"anchor":self.anchor,"scale":self.scale,"additive":self.additive,"light":self.light.is_some(),
            "layers":self.layers.iter().map(|clip| clip.as_ref().map(|clip| {
                let frame = &clip.frames[0];
                json!({"source":super::super::diagnostics::source(&frame.source), "frames":clip.frames.len(),
                "fps":frame.decoded.frames_per_second,"action_frame":frame.decoded.action_frame,"palette":frame.decoded.stored_palette,
                "looping":self.looping,"frame":now.checked_sub(self.start).and_then(|age|self.frame(clip,age))})
            })).collect::<Vec<_>>()})
    }
}

impl SectorCache {
    pub(super) fn expire_effects(&mut self, now: Duration) -> bool {
        let previous = std::mem::replace(&mut self.effect_time, now);
        let started: Vec<_> = self
            .effects
            .values()
            .filter(|fx| previous < fx.start && fx.start <= now && (fx.looping || now < fx.end))
            .filter_map(|fx| fx.light.as_ref().map(StaticLight::bounds))
            .collect();
        for bounds in &started {
            self.refresh_effect_light(None, Some(*bounds));
        }
        let before = self.effects.len();
        let mut old = Vec::new();
        self.effects.retain(|_, fx| {
            if fx.looping || now < fx.end {
                return true;
            }
            if let Some(light) = &fx.light {
                old.push(light.bounds())
            }
            false
        });
        for bounds in &old {
            self.refresh_effect_light(Some(*bounds), None);
        }
        self.effects.len() != before || !started.is_empty()
    }
    pub(super) fn launch_missile(
        &mut self,
        entry: i32,
        destination: (i32, i32),
        speed: u32,
    ) -> Duration {
        let Some(fx) = self.effects.get_mut(&(None, entry)) else {
            return Duration::ZERO;
        };
        let pixels = destination
            .0
            .abs_diff(fx.anchor.0)
            .max(destination.1.abs_diff(fx.anchor.1));
        let travel = Duration::from_millis(u64::from(pixels.div_ceil(speed.max(1)).max(1)) * 35);
        fx.destination = Some(destination);
        fx.end = fx.start + travel;
        travel
    }
    fn refresh_effect_light(&mut self, old: Option<RasterBounds>, new: Option<RasterBounds>) {
        if self.ambient.is_some() && (old.is_some() || new.is_some()) {
            self.refresh_dynamic_grid();
            self.refresh_shades(Some([old, new]));
            self.refresh_shadows();
        }
    }
    pub(super) fn stop_effect(&mut self, owner: Option<WorldOrder>, entry: i32) {
        if let Some(light) = self.effects.remove(&(owner, entry)).and_then(|f| f.light) {
            self.refresh_effect_light(Some(light.bounds()), None)
        }
    }
    pub(super) fn remove_cast_overlay(&mut self) {
        let mut old = Vec::new();
        self.effects.retain(|(owner, id), fx| {
            let remove = owner.is_none() && (id % 10 == 0 || fx.destination.is_some());
            if remove && let Some(l) = &fx.light {
                old.push(l.bounds())
            }
            !remove
        });
        for b in old {
            self.refresh_effect_light(Some(b), None)
        }
    }
    pub(super) fn effect_delay(&self, now: Duration) -> Option<Duration> {
        self.effects
            .values()
            .filter(|fx| fx.looping || now < fx.end)
            .flat_map(|fx| {
                fx.layers.iter().flatten().filter_map(move |layer| {
                    let age = now.saturating_sub(fx.start);
                    (fx.looping || fx.destination.is_some() || layer.one_shot_index(age).is_some())
                        .then(|| {
                            if fx.destination.is_some() {
                                Duration::from_millis(35)
                            } else {
                                layer.delay(age, 1)
                            }
                        })
                })
            })
            .min()
    }
    pub(super) fn effect_report(&self) -> Value {
        json!(self.effects.iter().map(|((owner,id),fx)|json!({"entry":id,"owner":owner,"caster":owner.is_none() && id%10==0,"playback":fx.report(self.effect_time)})).collect::<Vec<_>>())
    }
}

impl InteractiveWorld {
    pub fn advance_effects(&mut self, now: Duration) -> bool {
        let sounds: Vec<_> = self
            .cache
            .effects
            .values_mut()
            .filter(|fx| fx.start <= now)
            .filter_map(|fx| fx.pending_sound.take())
            .collect();
        for sound in sounds {
            self.queue_sound(sound);
        }
        self.cache.expire_effects(now)
    }

    pub(super) fn play_spell_effect(
        &mut self,
        vfs: &mut Vfs,
        frames: &mut FrameCache,
        def: &Definition,
        object: Option<WorldOrder>,
        now: Duration,
        cue: Cue,
    ) -> Result<(Duration, Value)> {
        let player = self.cache.player.as_ref().ok_or("missing caster")?;
        let (tile, anchor, rotation, scale) = if let Some(key) = object {
            let index = self
                .cache
                .ordered
                .binary_search_by_key(&key, |r| r.key)
                .map_err(|_| "missing target raster")?;
            let raster = &self.cache.ordered[index];
            let tile = (key.1.2, key.1.1);
            let (anchor, rotation) = match self
                .cache
                .target(key)
                .and_then(|t| t.action.portal_placement())
            {
                Some((offset, rotation)) => {
                    let (x, y) = project_world_tile(tile, self.reference)
                        .ok_or("target projection overflow")?;
                    (
                        (
                            x.checked_add(offset.0).ok_or("offset overflow")?,
                            y.checked_add(offset.1).ok_or("offset overflow")?,
                        ),
                        rotation,
                    )
                }
                None => (
                    raster.anchor,
                    raster
                        .facing
                        .or_else(|| {
                            self.cache
                                .npc(key)?
                                .0
                                .current_art
                                .value()
                                .map(|a| ((a >> 11) & 7) as u8)
                        })
                        .unwrap_or(raster.frame.decoded.stored_rotation),
                ),
            };
            let art = raster
                .actor_art
                .or_else(|| self.cache.npc(key)?.0.current_art.value());
            (tile, anchor, rotation, def.scale(art))
        } else {
            let pose = player.sample(now);
            let scale = def.scale(Some(
                self.cache
                    .player_form()
                    .map_or(player.appearance.art, |m| m.art),
            ));
            (pose.tile, pose.anchor, player.facing(), scale)
        };
        let (layers, duration) = def.clips(vfs, frames, rotation)?;
        if duration.is_zero() {
            if let Some(event) = cue.event(def.sound) {
                self.queue_sound(event);
            }
            return Ok((
                Duration::ZERO,
                json!({"definition": super::super::diagnostics::source(&def.source), "sound": def.sound, "layers": [], "entry":def.entry}),
            ));
        }
        let light = if let Some((path, color)) = &def.light {
            let center =
                project_world_tile(tile, self.reference).ok_or("light projection overflow")?;
            let art = frames.load(vfs, path, 0, 0, false)?;
            Some(StaticLight::new(
                art,
                tile,
                (anchor.0 - center.0, anchor.1 - center.1),
                self.reference,
                *color,
                false,
            )?)
        } else {
            None
        };
        let fx = Playback {
            start: now,
            pending_sound: cue.event(def.sound),
            reverse: false,
            initial: 0,
            age_offset: Duration::ZERO,
            rotation,
            looping: def.looping,
            end: now + duration,
            destination: None,
            layers,
            scale,
            additive: def.additive,
            tile,
            anchor,
            light,
        };
        let delay = fx.action_delay();
        let mut report = fx.report(now);
        report["definition"] = super::super::diagnostics::source(&def.source);
        report["sound"] = json!(def.sound);
        report["duration_ms"] = json!(duration.as_millis());
        report["requested_palette"] = json!(def.palette);
        let new = fx.light_at(self.cache.effect_time).map(StaticLight::bounds);
        let old = self
            .cache
            .effects
            .insert((object, def.entry), fx)
            .and_then(|fx| fx.light.map(|l| l.bounds()));
        self.cache.refresh_effect_light(old, new);
        Ok((delay, report))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires the installed original game"]
    fn installed_college_eyecandy_frames_decode() {
        let root = std::env::var_os("ARCANUM_GAME_DIR").expect("set ARCANUM_GAME_DIR");
        let mut vfs = Vfs::from_game(&root, crate::vfs::Policy::Ce).unwrap();
        let mut errors = Vec::new();
        for spell in 0..80 {
            for slot in 0..6 {
                let id = spell * 10 + slot;
                let result = (|| -> Result<()> {
                    let Some(def) = Definition::load(&mut vfs, id)? else {
                        return Ok(());
                    };
                    def.clips(&mut vfs, &mut FrameCache::default(), 0)?;
                    Ok(())
                })();
                if let Err(error) = result {
                    errors.push(format!("entry {id}: {error}"));
                }
            }
        }
        assert!(errors.is_empty(), "{}", errors.join("\n"));
    }
}
