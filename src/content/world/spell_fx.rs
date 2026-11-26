//! One-shot spell overlays from SpellEyeCandy.mes, attached to the same depth layer as their owner.
use super::*;
use crate::{mes::Mes, resource_path::normalize};
use serde_json::{Value, json};
use std::error::Error;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

pub(super) struct Definition {
    basename: String,
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
    pub fn load(vfs: &mut Vfs, entry: i32) -> Result<Option<Self>> {
        let table = load_mes(vfs, "rules/spelleyecandy.mes")?;
        let Some(line) = table.mes.get(entry).filter(|line| !line.trim().is_empty()) else {
            return Ok(None);
        };
        let mut art = None;
        let mut palette = 0;
        let mut autoscale = false;
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
                }
                "blend" => {
                    additive = match value.as_str() {
                        "add" => true,
                        "normal" | "none" => false,
                        _ => return Err("unsupported EyeCandy blend".into()),
                    }
                }
                "sound" => sound = Some(value.parse()?),
                "flags" => {
                    for flag in value.split('|').map(str::trim) {
                        match flag {
                            "overlay_f" => layers[1] = true,
                            "overlay_b" => layers[0] = true,
                            "overlay_fb" => layers = [true; 2],
                            "animates" => (),
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
    end: Duration,
    layers: [Option<FrameLoop>; 2],
    scale: u32,
    additive: bool,
    pub(super) tile: (u32, u32),
    anchor: (i32, i32),
    pub light: Option<StaticLight>,
}

impl Playback {
    fn clip(&self) -> &FrameLoop {
        self.layers
            .iter()
            .rev()
            .flatten()
            .next()
            .expect("prepared overlay layer")
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
    ) -> Option<WorldDraw<'_>> {
        let layer = self.layers[usize::from(front)].as_ref()?;
        let index = layer.one_shot_index(now.checked_sub(self.start)?)?;
        let asset = &layer.frames[index];
        let draw = SpriteDraw {
            frame: &asset.decoded.frame,
            anchor: self.anchor,
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
        json!({"tile":self.tile,"scale":self.scale,"additive":self.additive,"light":self.light.is_some(),
            "layers":self.layers.iter().map(|clip| clip.as_ref().map(|clip| {
                let frame = &clip.frames[0];
                json!({"source":super::super::diagnostics::source(&frame.source), "frames":clip.frames.len(),
                "fps":frame.decoded.frames_per_second,"action_frame":frame.decoded.action_frame,"palette":frame.decoded.stored_palette,
                "frame":clip.one_shot_index(now.saturating_sub(self.start))})
            })).collect::<Vec<_>>()})
    }
}

impl SectorCache {
    pub(super) fn expire_effects(&mut self, now: Duration) -> bool {
        self.effect_time = now;
        let before = self.effects.len();
        let mut old = Vec::new();
        self.effects.retain(|_, fx| {
            if now < fx.end {
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
        self.effects.len() != before
    }
    fn refresh_effect_light(&mut self, old: Option<RasterBounds>, new: Option<RasterBounds>) {
        if self.ambient.is_some() && (old.is_some() || new.is_some()) {
            self.refresh_dynamic_grid();
            self.refresh_shades(Some([old, new]));
            self.refresh_shadows();
        }
    }
    pub(super) fn remove_cast_overlay(&mut self) {
        if let Some(fx) = self.effects.remove(&None) {
            self.refresh_effect_light(fx.light.map(|l| l.bounds()), None);
        }
    }
    pub(super) fn effect_delay(&self, now: Duration) -> Option<Duration> {
        self.effects
            .values()
            .filter(|fx| now < fx.end)
            .flat_map(|fx| {
                fx.layers.iter().flatten().filter_map(move |layer| {
                    let age = now.saturating_sub(fx.start);
                    layer.one_shot_index(age).map(|_| layer.delay(age, 1))
                })
            })
            .min()
    }
    pub(super) fn effect_report(&self) -> Value {
        json!(self.effects.iter().map(|(owner,fx)|json!({"caster":owner.is_none(),"playback":fx.report(self.effect_time)})).collect::<Vec<_>>())
    }
}

impl InteractiveWorld {
    pub fn advance_effects(&mut self, now: Duration) -> bool {
        self.cache.expire_effects(now)
    }

    pub(super) fn play_spell_effect(
        &mut self,
        vfs: &mut Vfs,
        frames: &mut FrameCache,
        def: &Definition,
        object: Option<WorldOrder>,
        now: Duration,
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
                None => (raster.anchor, raster.frame.decoded.stored_rotation),
            };
            (tile, anchor, rotation, raster.scale)
        } else {
            let pose = player.sample(now);
            let scale = if def.autoscale {
                match (player.appearance.art >> 24) & 7 {
                    1 | 2 => 63,
                    3 => 100,
                    _ => 87,
                }
            } else {
                100
            };
            (pose.tile, pose.anchor, player.facing(), scale)
        };
        let mut layers = [None, None];
        let mut duration = Duration::ZERO;
        for (index, suffix) in ["b", "f"]
            .into_iter()
            .enumerate()
            .filter(|(i, _)| def.layers[*i])
        {
            let path = format!("art/eye_candy/{}_{suffix}.art", def.basename);
            let mut load = |i| frames.load_rotated(vfs, &path, i, def.palette, false, rotation);
            let first = load(0)?;
            let fps = u32::try_from(first.decoded.frames_per_second)?;
            let mut sequence = Vec::with_capacity(first.decoded.frame_count);
            let count = first.decoded.frame_count;
            sequence.push(first);
            for frame in 1..count {
                sequence.push(load(frame)?);
            }
            let clip = FrameLoop::new(sequence, fps)?;
            duration = duration.max(clip.period() * count as u32);
            layers[index] = Some(clip);
        }
        if duration.is_zero() {
            return Err("missing EyeCandy layers".into());
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
            end: now + duration,
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
        report["requested_palette"] = json!(def.palette);
        let new = fx.light.as_ref().map(StaticLight::bounds);
        let old = self
            .cache
            .effects
            .insert(object, fx)
            .and_then(|fx| fx.light.map(|l| l.bounds()));
        self.cache.refresh_effect_light(old, new);
        if let Some(sound) = def.sound.filter(|&id| id >= 0) {
            self.queue_sound(SoundEvent::Effect(sound));
        }
        Ok((delay, report))
    }
}
