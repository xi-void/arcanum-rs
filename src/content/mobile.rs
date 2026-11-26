//! Render-only import of verified base-map mobile records.

pub(super) mod population;
mod walk;
pub(super) use walk::{Step as WalkStep, Walk};

use super::{
    art::{FrameCache, FrameLoop, LoadedFrame, Reflection},
    load_mes, read_bounded,
    sector::read_static_prototype,
    shadow::LoadedLookup,
};
use crate::{
    isometric::project_world_tile, mes::Mes, prototype::MAX_PROTOTYPE_BYTES, sector::objects,
    shadow, vfs::Vfs,
};
use std::{
    collections::BTreeMap,
    error::Error,
    sync::Arc,
    time::{Duration, Instant},
};

#[cfg(test)]
pub(super) const MAP: &str = "bates mansion lev 1";
#[cfg(test)]
pub(super) const SECTOR: u64 = 67_108_865;
pub(super) const VERIFIED_SHADOW: u32 = 0x9030_0001;
const RABBIT_MAP: &str = "k'na tha";
pub(super) const RABBIT_SECTOR: u64 = 805_306_374;

pub(super) fn supports_walk(map: &str, sector: u64) -> bool {
    map.eq_ignore_ascii_case(RABBIT_MAP) && sector == RABBIT_SECTOR
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Pose {
    pub(super) path: String,
    direction: u8,
    rotation: u8,
    frame: usize,
    palette: u8,
}

impl Pose {
    pub(super) fn decode(raw: u32, names: Option<&Mes>) -> Result<Self, &'static str> {
        if raw & 0x0008_07cf != 0 {
            return Err("expected a weapon-free, shield-free standing critter");
        }

        // CE name_normalize_aid and name_resolve_path. The authored direction,
        // palette and frame stay on Pose, independent of filename selection.
        const CLOTHING: [&str; 9] = ["uw", "v1", "la", "cm", "pm", "rb", "pc", "bn", "cd"];
        let clothing = |armor| {
            CLOTHING
                .get(armor as usize)
                .copied()
                .ok_or("unknown standing critter clothing")
        };
        let path = match raw >> 28 {
            2 => {
                let armor = (raw >> 20) & 15;
                let mut body = ((raw >> 24) & 7) as usize;
                let mut gender = if raw & 0x0800_0000 != 0 { 'm' } else { 'f' };
                if body == 4 && gender == 'f' {
                    body = 0;
                }
                if matches!(armor, 4 | 6) {
                    gender = 'x';
                    body = match body {
                        4 => 0, // Elf plate uses the human body asset.
                        2 => 1, // Halfling plate uses the dwarven body asset.
                        _ => body,
                    };
                }
                let body = ["hm", "df", "gh", "hg", "ef"]
                    .get(body)
                    .ok_or("unknown critter body type")?;
                let name = format!("{body}{gender}");
                format!("art/critter/{name}/{name}{}xaa.art", clothing(armor)?)
            }
            kind @ (12 | 13) => {
                let (dir, id, armor) = if kind == 12 {
                    ("monster", (raw >> 23) & 31, clothing((raw >> 20) & 7)?)
                } else {
                    ("unique_npc", (raw >> 20) & 255, "")
                };
                let name = names
                    .and_then(|table| table.get(id as i32))
                    .ok_or("missing named critter ART entry")?;
                let name =
                    crate::resource_path::normalize(name).map_err(|_| "invalid critter name")?;
                if name.contains('/') {
                    return Err("expected a critter name basename");
                }
                format!("art/{dir}/{name}/{name}{armor}xaa.art")
            }
            _ => return Err("unsupported standing ART type"),
        };
        Ok(Self::new(raw, path))
    }

    pub(super) fn new(raw: u32, path: String) -> Self {
        let direction = ((raw >> 11) & 7) as u8;
        Self {
            path,
            direction,
            rotation: direction,
            frame: ((raw >> 14) & 31) as usize,
            palette: ((raw >> 4) & 3) as u8,
        }
        .in_direction(direction)
    }

    pub(super) fn in_direction(mut self, direction: u8) -> Self {
        self.direction = direction;
        self.rotation = if (1..4).contains(&direction) {
            8 - direction
        } else {
            direction
        };
        self
    }

    pub(super) fn load(
        &self,
        vfs: &mut Vfs,
        cache: &mut FrameCache,
        frame: usize,
    ) -> Result<Arc<LoadedFrame>, Box<dyn Error>> {
        let asset = cache.load_with_reflection(
            vfs,
            &self.path,
            frame,
            self.palette,
            self.rotation,
            if self.mirrored() {
                Reflection::Critter
            } else {
                Reflection::None
            },
        )?;
        if asset.decoded.rotation_count != 8 {
            return Err("expected eight stored critter directions".into());
        }
        Ok(asset)
    }

    fn mirrored(&self) -> bool {
        self.direction != self.rotation
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct Caster {
    pub tile: (u32, u32),
    pub anchor: (i32, i32),
}

pub(super) struct MobileFrame {
    pub object_index: usize,
    pub flat: bool,
    pub caster: Caster,
    pub offset: (i32, i32),
    pub asset: Arc<LoadedFrame>,
    pub diagnostic: String,
    pub casts_shadow: bool,
    pub fidget: Option<Arc<DirectionalFidget>>,
    pub walk: Option<Arc<Walk>>,
}

/// Stored stand frames. Each directional sequence shares the fire clock,
/// with the non-looping fidget policy used by CE's standing goal.
pub(super) struct DirectionalFidget {
    directions: [FrameLoop; 8],
    pub original_direction: u8,
}

impl DirectionalFidget {
    fn load(
        vfs: &mut Vfs,
        cache: &mut FrameCache,
        pose: Pose,
        first: &LoadedFrame,
    ) -> Result<Self, Box<dyn Error>> {
        let (count, fps) = (first.decoded.frame_count, first.decoded.frames_per_second);
        if pose.frame != 0 || !(2..=32).contains(&count) {
            return Err("expected a bounded fidget starting at authored stand frame zero".into());
        }
        let original_direction = pose.direction;
        let mut directions = Vec::with_capacity(8);
        for dir in 0..8 {
            let facing = pose.clone().in_direction(dir);
            let frames = (0..count)
                .map(|i| facing.load(vfs, cache, i))
                .collect::<Result<Vec<_>, _>>()?;
            directions.push(FrameLoop::new(frames, u32::try_from(fps)?)?);
        }
        Ok(Self {
            directions: directions.try_into().expect("eight fidget directions"),
            original_direction,
        })
    }

    pub fn sequence(&self, direction: Option<u8>) -> &FrameLoop {
        &self.directions[usize::from(direction.unwrap_or(self.original_direction))]
    }

    #[cfg(test)]
    pub fn fixture(directions: [FrameLoop; 8]) -> Self {
        Self {
            directions,
            original_direction: 3,
        }
    }

    pub fn frames(&self) -> impl Iterator<Item = &Arc<LoadedFrame>> {
        self.directions.iter().flat_map(|clip| &clip.frames)
    }
}

#[derive(Clone, Copy, Default)]
pub(super) struct FidgetPlayback {
    pub start: Option<Duration>,
    pub direction: Option<u8>,
}

pub(super) struct MobileShadows {
    pub lookup: LoadedLookup,
    frames: Vec<Arc<LoadedFrame>>,
}

impl MobileShadows {
    pub fn frame(&self, mask: shadow::Mask) -> Arc<LoadedFrame> {
        Arc::clone(self.asset(mask))
    }

    pub fn asset(&self, mask: shadow::Mask) -> &Arc<LoadedFrame> {
        &self.frames[usize::from(mask.direction) * 7 + usize::from(mask.frame)]
    }

    pub(super) fn load(
        vfs: &mut Vfs,
        cache: &mut FrameCache,
        art: shadow::Art,
    ) -> Result<Self, Box<dyn Error>> {
        let lookup = LoadedLookup::load(vfs)?;
        let names = load_mes(vfs, "art/light/light.mes")?;
        let mut frames = Vec::with_capacity(32 * 7);
        for direction in 0..32 {
            let (path, rotation) = art.resource(&names.mes, direction)?;
            for frame in 0..7 {
                let asset = cache.load_rotated(vfs, &path, frame, 0, false, rotation)?;
                if asset.decoded.rotation_count != 8 {
                    return Err(format!("expected eight rotations for {path}").into());
                }
                frames.push(asset);
            }
        }
        Ok(Self { lookup, frames })
    }
}

#[derive(Default)]
pub(super) struct PreparedMobiles {
    pub objects: Vec<population::Resident>,
    pub frames: Vec<MobileFrame>,
    pub shadows: Option<MobileShadows>,
    pub issues: Vec<String>,
}

pub(super) struct Prepare<'a> {
    pub index: &'a population::Index,
    pub prototypes: &'a mut population::Prototypes,
    pub excluded: &'a std::collections::BTreeSet<objects::OriginalId>,
    pub sector: u64,
    pub reference: (u32, u32),
    pub lighting: bool,
    pub fidget: bool,
    pub walking: bool,
}

pub(super) fn prepare(
    vfs: &mut Vfs,
    cache: &mut FrameCache,
    context: Prepare<'_>,
) -> PreparedMobiles {
    use population::{Effective, Resident};
    let Prepare {
        index,
        prototypes,
        excluded,
        sector,
        reference,
        lighting,
        fidget,
        walking,
    } = context;
    let mut result = PreparedMobiles::default();
    let mut names = BTreeMap::new();
    let mut bytes_kept = 0;
    // Bound retained payload and resident metadata independently of the
    // shared texture cache and the bounded sector cache.
    const RESIDENCY_BYTES: usize = 4 * 1024 * 1024;
    for (id, path) in index.entries(sector) {
        if excluded.contains(&id) {
            continue;
        }
        let prepared = (|| -> Result<(), Box<dyn Error>> {
            let (source, bytes) = read_bounded(vfs, path, MAX_PROTOTYPE_BYTES)?;
            let object = objects::decode_instance(&bytes)?;
            if object.id != id {
                return Err("mobile identity changed during discovery".into());
            }
            let (prototype, prototype_error) = match prototypes.get(vfs, &object) {
                Ok(proto) => (Some(proto), None),
                Err(error) => (None, Some(error)),
            };
            let effective = Effective::new(&object, prototype.as_deref());
            let parent = index.owner(id).map(|owner| (owner, index.contains(owner)));
            let inventory: Vec<_> = population::inventory(&object)
                .into_iter()
                .map(|item| (item, index.contains(item)))
                .collect();
            bytes_kept += bytes.len()
                + size_of::<Resident>()
                + path.len()
                + inventory.len() * size_of::<(objects::OriginalId, bool)>();
            if bytes_kept > RESIDENCY_BYTES {
                return Err("sector mobile metadata budget exceeded".into());
            }
            let contained = index.is_contained(&object, effective.flags.value());
            let tile = object.tile().filter(|tile| *tile != (0, 0));
            let world_tile =
                tile.filter(|&tile| !contained && super::world::tile_sector_id(tile) == sector);
            let flags = effective.flags.value();
            let placement = if contained {
                "contained"
            } else if world_tile.is_some() {
                "world"
            } else {
                "unresolved"
            };
            let mut render_issue = None;
            if let Some(tile) = world_tile {
                let frame = if flags.is_none() {
                    Err("unresolved object visibility flags".into())
                } else if flags.is_some_and(|flags| {
                    flags & (population::INACTIVE | population::DONT_DRAW | population::INVISIBLE)
                        != 0
                }) {
                    Err("original visibility flags suppress normal draw".into())
                } else if matches!(object.category, 15 | 16) {
                    let art = effective
                        .current_art
                        .value()
                        .ok_or("unresolved current ART");
                    art.map_err(Into::into).and_then(|art| {
                        let table = match art >> 28 {
                            12 => Some("art/monster/monster.mes"),
                            13 => Some("art/unique_npc/unique_npc.mes"),
                            _ => None,
                        };
                        let names = table
                            .map(|table| {
                                use std::collections::btree_map::Entry;
                                let loaded = match names.entry(table) {
                                    Entry::Occupied(entry) => entry.into_mut(),
                                    Entry::Vacant(entry) => entry.insert(load_mes(vfs, table)?),
                                };
                                Ok::<_, Box<dyn Error>>(&loaded.mes)
                            })
                            .transpose()?;
                        // Walking remains a diagnostic for the verified rabbit
                        // asset. Populating a map does not start NPC behavior.
                        let walk = walking && art >> 28 == 12;
                        load_critter(
                            vfs,
                            cache,
                            &effective,
                            (art, tile),
                            reference,
                            names,
                            (fidget, walk),
                        )
                    })
                } else {
                    load_prop(
                        vfs,
                        cache,
                        &mut names,
                        &effective,
                        (object.category, tile),
                        reference,
                    )
                };
                match frame {
                    Ok(mut frame) => {
                        frame.object_index = result.objects.len();
                        result.frames.push(frame);
                    }
                    Err(error) => render_issue = Some(error.to_string()),
                }
            } else {
                render_issue = Some(
                    if contained {
                        "owned or in inventory"
                    } else {
                        "unresolved placement"
                    }
                    .into(),
                );
            }
            let blocks = world_tile.is_some() && effective.blocks(object.category);
            let category = object.category;
            let prototype_id = object.prototype.0;
            result.objects.push(Resident {
                id,
                category,
                prototype_id,
                current_art: effective.current_art,
                source,
                prototype,
                prototype_error,
                parent,
                inventory,
                conflict: index.conflicting_owner(id),
                placement,
                tile: if contained { None } else { tile },
                blocks,
                render_issue,
                raster: None,
                bytes,
            });
            Ok(())
        })();
        if let Err(error) = prepared {
            if result.issues.len() < 32 {
                result.issues.push(format!("{path}: {error}"));
            }
            if bytes_kept > RESIDENCY_BYTES {
                break;
            }
        }
    }
    if lighting && result.frames.iter().any(|mobile| mobile.casts_shadow) {
        let start = std::time::Instant::now();
        let art = shadow::Art::decode(VERIFIED_SHADOW).expect("verified shadow ID");
        match MobileShadows::load(vfs, cache, art) {
            Ok(shadows) => {
                let bytes: usize = shadows
                    .frames
                    .iter()
                    .map(|frame| frame.decoded.frame.pixels.len() * 4)
                    .sum();
                result.issues.push(format!(
                    "mobile shadow bank: {} frames, {bytes} RGBA bytes, prepared in {:?}",
                    shadows.frames.len(),
                    start.elapsed()
                ));
                result.shadows = Some(shadows);
            }
            Err(error) => result.issues.push(format!("mobile shadow bank: {error}")),
        }
    }
    result
}

fn load_critter(
    vfs: &mut Vfs,
    cache: &mut FrameCache,
    effective: &population::Effective,
    (art, tile): (u32, (u32, u32)),
    reference: (u32, u32),
    names: Option<&Mes>,
    (animate, walking): (bool, bool),
) -> Result<MobileFrame, Box<dyn Error>> {
    let start = Instant::now();
    let f = effective.appearance;
    let flags = f.flags.ok_or("unresolved critter flags")?;
    let critter = f.critter_flags.ok_or("unresolved critter subtype flags")?;
    const UNSUPPORTED: u32 = 0x301f_93c7;
    if flags & UNSUPPORTED != 0
        || critter & 1 != 0
        || f.scale != Some(100)
        || f.blit_flags != Some(0)
    {
        return Err(format!("unsupported mobile appearance (flags {flags:#x}, critter {critter:#x}, scale {:?}, blit {:?})", f.scale, f.blit_flags).into());
    }
    let shadow_id = f.shadow_art.unwrap_or(u32::MAX);
    let casts_shadow = shadow_id == VERIFIED_SHADOW;
    let shadow_note = if casts_shadow {
        "verified"
    } else {
        "not prepared"
    };
    let offset = (
        f.offset_x.ok_or("unresolved critter X offset")?,
        f.offset_y.ok_or("unresolved critter Y offset")?,
    );
    let center = project_world_tile(tile, reference).ok_or("mobile projection overflow")?;
    let anchor = (
        center
            .0
            .checked_add(offset.0)
            .ok_or("mobile X offset overflow")?,
        center
            .1
            .checked_add(offset.1)
            .ok_or("mobile Y offset overflow")?,
    );
    let pose = cache.resolve(|| Pose::decode(art, names))?;
    let asset = pose.load(vfs, cache, pose.frame)?;
    let diagnostic = format!(
        "mobile at {:?} ART {:#010x}: {}, direction {} (stored {}), frame {}, palette {}, hotspot {:?}, offset {offset:?}, shadow {shadow_id:#010x} ({shadow_note}); prepared in {:?}",
        tile,
        art,
        pose.path,
        pose.direction,
        pose.rotation,
        pose.frame,
        asset.decoded.stored_palette,
        asset.decoded.frame.hotspot,
        start.elapsed()
    );
    let (fidget, diagnostic) = if animate && art >> 28 == 12 && asset.decoded.frame_count > 1 {
        let flags2 = f.critter_flags2.unwrap_or_default();
        let spell = f.spell_flags.unwrap_or_default();
        let diagnostic =
            format!("{diagnostic}; critter flags {critter:#x}/{flags2:#x}, spell flags {spell:#x}");
        let start = Instant::now();
        let sequence = if critter & 0x60 == 0 && spell & 0x10000 == 0 {
            DirectionalFidget::load(vfs, cache, pose.clone(), &asset)
        } else {
            Err("stunned, paralyzed or stoned mobile cannot fidget".into())
        };
        match sequence {
            Ok(sequence) => {
                let bytes: usize = sequence
                    .frames()
                    .map(|frame| frame.decoded.frame.pixels.len() * 4)
                    .sum();
                (
                    Some(Arc::new(sequence)),
                    format!(
                        "{diagnostic}; fidget {} frames at {} fps, eight facings ({bytes} RGBA bytes) prepared in {:?}",
                        asset.decoded.frame_count,
                        asset.decoded.frames_per_second,
                        start.elapsed()
                    ),
                )
            }
            Err(error) => (None, format!("{diagnostic}; fidget unavailable: {error}")),
        }
    } else {
        (None, diagnostic)
    };
    let caster = Caster { tile, anchor };
    let (walk, diagnostic) = if walking {
        let start = Instant::now();
        let movement = if fidget.is_some()
            && offset == (0, 0)
            && critter == 0x2008
            && f.spell_flags.unwrap_or_default() == 0
            && f.critter_flags2.unwrap_or_default() == 0
        {
            effective
                .dexterity
                .ok_or_else(|| "missing walk Dexterity".into())
                .and_then(|dx| Walk::load(vfs, cache, &pose, caster, reference, dx))
        } else {
            Err("unsupported initial mobile state for walking".into())
        };
        match movement {
            Ok(walk) => {
                let last = walk.steps.last().unwrap();
                let count = walk.steps.len() - 1;
                let frames = walk.frames().count();
                let pixel_bytes: usize = walk
                    .frames()
                    .map(|frame| frame.decoded.frame.pixels.len() * 4)
                    .sum();
                let diagnostic = format!(
                    "{diagnostic}; walk {frames} directional frames ({pixel_bytes} RGBA bytes), {count} steps at {:?}, stops at {:?} in {:?}, prepared in {:?}",
                    walk.period,
                    last.tile,
                    walk.deadline(count),
                    start.elapsed(),
                );
                (Some(Arc::new(walk)), diagnostic)
            }
            Err(error) => (None, format!("{diagnostic}; walk unavailable: {error}")),
        }
    } else {
        (None, diagnostic)
    };
    Ok(MobileFrame {
        object_index: 0,
        flat: false,
        caster,
        offset,
        asset,
        diagnostic,
        casts_shadow,
        fidget,
        walk,
    })
}

pub(super) fn item_art_entry(art: u32) -> i32 {
    let kind = (art & 15) as i32;
    let subtype = ((art >> 6) & 15) as i32;
    let coverage = ((art >> 14) & 7) as i32;
    let adjust = if kind == 2 && coverage != 0 {
        20 * (5 * coverage + 10)
    } else {
        0
    };
    ((art >> 17) & 0x7ff) as i32 + 20 * (subtype + 50 * kind) + adjust
}

pub(super) fn load_prop(
    vfs: &mut Vfs,
    cache: &mut FrameCache,
    names: &mut BTreeMap<&'static str, super::LoadedMes>,
    effective: &population::Effective,
    (category, tile): (u32, (u32, u32)),
    reference: (u32, u32),
) -> Result<MobileFrame, Box<dyn Error>> {
    let art = effective
        .current_art
        .value()
        .ok_or("unresolved current ART")?;
    let appearance = effective.appearance;
    let flags = effective
        .flags
        .value()
        .ok_or("unresolved visibility flags")?;
    if appearance.scale != Some(100)
        || appearance.blit_flags != Some(0)
        || appearance.blit_color != Some(0x00ff_ffff)
        || appearance.alpha != Some(255)
        || flags & 0x0002_00c0 != 0
    {
        return Err("unsupported mobile prop scale, blending or water state".into());
    }
    let (dir, table, entry, frame, rotation) = match (category, art >> 28) {
        (5..=14, 6) if (art >> 12) & 3 == 0 => (
            "item",
            "art/item/item_ground.mes",
            item_art_entry(art),
            0,
            0,
        ),
        (2, 7) => (
            "container",
            "art/container/container.mes",
            (((art >> 6) & 31) * 1000 + ((art >> 19) & 511)) as i32,
            ((art >> 14) & 31) as usize,
            ((art >> 11) & 7) as u8,
        ),
        // Original containers can use furniture and other scenery ART.
        (2 | 3, 4) => {
            let art = super::scenery::SceneryArt::decode(art)?;
            (
                "scenery",
                "art/scenery/scenery.mes",
                i32::from(art.family) * 1000 + i32::from(art.number),
                usize::from(art.frame),
                art.rotation,
            )
        }
        _ => {
            return Err(format!("unsupported mobile category {category} with ART {art:#x}").into());
        }
    };
    use std::collections::btree_map::Entry;
    let mes = match names.entry(table) {
        Entry::Occupied(entry) => entry.into_mut(),
        Entry::Vacant(entry) => entry.insert(load_mes(vfs, table)?),
    };
    let name = mes.mes.get(entry).ok_or("missing ground ART name")?.trim();
    let name = crate::resource_path::normalize(name)?;
    if name.contains('/') || !name.ends_with(".art") {
        return Err("invalid ground ART name".into());
    }
    let path = format!("art/{dir}/{name}");
    let asset = cache.load_rotated(vfs, &path, frame, ((art >> 4) & 3) as u8, false, rotation)?;
    let offset = (
        appearance.offset_x.ok_or("unresolved X offset")?,
        appearance.offset_y.ok_or("unresolved Y offset")?,
    );
    let (x, y) = project_world_tile(tile, reference).ok_or("mobile prop projection overflow")?;
    let anchor = (
        x.checked_add(offset.0)
            .ok_or("mobile prop X offset overflow")?,
        y.checked_add(offset.1)
            .ok_or("mobile prop Y offset overflow")?,
    );
    Ok(MobileFrame {
        object_index: 0,
        flat: flags & 4 != 0,
        caster: Caster { tile, anchor },
        offset,
        asset,
        diagnostic: format!("placed mobile prop {path} at {tile:?}"),
        casts_shadow: false,
        fidget: None,
        walk: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standing_paths_follow_original_clothing_and_body_substitution() {
        for (art, path) in [
            (0x2820_2000, "art/critter/hmm/hmmlaxaa.art"),
            (0x2930_2000, "art/critter/dfm/dfmcmxaa.art"),
            (0x2c50_2830, "art/critter/efm/efmrbxaa.art"),
            (0x2450_2030, "art/critter/hmf/hmfrbxaa.art"),
            (0x2a40_2000, "art/critter/dfx/dfxpmxaa.art"),
            (0x2c60_2000, "art/critter/hmx/hmxpcxaa.art"),
        ] {
            assert_eq!(Pose::decode(art, None).unwrap().path, path);
        }
        assert!(Pose::decode(0x2890_2000, None).is_err());
    }

    #[test]
    fn unique_npc_stand_uses_named_table_and_retains_authored_palette_and_facing() {
        let names = Mes::parse_text("{22}{HLBUW}").unwrap();
        let pose = Pose::decode(0xd160_1820, Some(&names)).unwrap();
        assert_eq!(pose.path, "art/unique_npc/hlbuw/hlbuwxaa.art");
        assert_eq!(
            (pose.direction, pose.rotation, pose.frame, pose.palette),
            (3, 5, 0, 2)
        );
        assert!(Pose::decode(0xd160_1860, Some(&names)).is_err());
        assert!(Pose::decode(0xd160_1820, None).is_err());
    }

    #[test]
    fn resolves_original_kitchen_stand_and_reflection() {
        let direct = Pose::decode(0x2880_2820, None).unwrap();
        assert_eq!(
            (
                direct.path.as_str(),
                direct.direction,
                direct.rotation,
                direct.frame,
                direct.palette
            ),
            ("art/critter/hmm/hmmcdxaa.art", 5, 5, 0, 2)
        );
        assert!(!direct.mirrored());
        let reflected = Pose::decode(0x2880_0820, None).unwrap();
        assert_eq!((reflected.direction, reflected.rotation), (1, 7));
        assert!(reflected.mirrored());
        assert!(Pose::decode(0x2880_2860, None).is_err());
    }

    #[test]
    fn resolves_monster_species_and_reflects_only_directions_one_to_three() {
        let names = Mes::parse_text("{9}{BUN}").unwrap();
        let raw = 0xc480_1810;
        let pose = Pose::decode(raw, Some(&names)).unwrap();
        assert_eq!(pose.path, "art/monster/bun/bunuwxaa.art");
        assert_eq!((pose.direction, pose.frame, pose.palette), (3, 0, 1));
        let shown: Vec<_> = (0..8)
            .map(|direction| {
                let facing = pose.clone().in_direction(direction);
                (facing.rotation, facing.mirrored())
            })
            .collect();
        assert_eq!(
            shown,
            [
                (0, false),
                (7, true),
                (6, true),
                (5, true),
                (4, false),
                (5, false),
                (6, false),
                (7, false)
            ]
        );
        assert!(Pose::decode(raw | 0x40, Some(&names)).is_err());
        assert!(Pose::decode(raw, None).is_err());
    }
}

#[cfg(test)]
#[test]
#[ignore = "requires original Bates Mansion assets"]
fn installed_mobile_poses_and_shadow_bank_share_decoded_resources() {
    let game = std::env::var_os("ARCANUM_GAME_DIR").expect("set ARCANUM_GAME_DIR");
    let mut vfs = Vfs::from_game(game, crate::vfs::Policy::Ce).unwrap();
    let mut cache = FrameCache::default();
    let index = population::Index::load(&mut vfs, MAP);
    let prepared = prepare(
        &mut vfs,
        &mut cache,
        Prepare {
            index: &index,
            prototypes: &mut Default::default(),
            excluded: &Default::default(),
            sector: SECTOR,
            reference: (80, 81),
            lighting: true,
            fidget: false,
            walking: false,
        },
    );
    let original = prepared
        .frames
        .iter()
        .find(|frame| frame.caster.tile == (80, 81))
        .expect("kitchen NPC");
    let reflected = prepared
        .frames
        .iter()
        .find(|frame| frame.caster.tile == (91, 79))
        .expect("reflected NPC");
    assert_eq!(original.caster.anchor, (0, 0));
    assert_eq!(original.asset.decoded.stored_rotation, 5);
    assert_eq!(original.asset.decoded.stored_palette, 2);
    assert_eq!(original.asset.decoded.frame.hotspot, (15, 74));
    assert_eq!(reflected.caster.tile, (91, 79));
    assert_eq!(reflected.asset.decoded.frame.hotspot, (12, 74));
    let original_pixels = &original.asset.decoded.frame;
    let reflected_pixels = &reflected.asset.decoded.frame;
    assert!(
        original_pixels
            .pixels
            .chunks_exact(original_pixels.width as usize)
            .zip(
                reflected_pixels
                    .pixels
                    .chunks_exact(reflected_pixels.width as usize)
            )
            .all(|(a, b)| a.iter().eq(b.iter().rev()))
    );
    let bank = prepared.shadows.unwrap();
    assert_eq!(bank.frames.len(), 224);
    let mask = shadow::Mask {
        direction: 31,
        frame: 2,
        gray: 41,
    };
    let reference = bank.frame(mask);
    let repeated = bank.frame(shadow::Mask { gray: 32, ..mask });
    assert!(Arc::ptr_eq(&reference, &repeated));
    assert_eq!(
        reference.source.virtual_path,
        "art/light/med_critter_s3.art"
    );
}

#[cfg(test)]
#[test]
#[ignore = "requires original K'na Tha assets"]
fn installed_standing_rabbit_supplies_authored_fidget_frames() {
    let game = std::env::var_os("ARCANUM_GAME_DIR").expect("set ARCANUM_GAME_DIR");
    let mut vfs = Vfs::from_game(game, crate::vfs::Policy::Ce).unwrap();
    let mut cache = FrameCache::default();
    let index = population::Index::load(&mut vfs, RABBIT_MAP);
    let prepared = prepare(
        &mut vfs,
        &mut cache,
        Prepare {
            index: &index,
            prototypes: &mut Default::default(),
            excluded: &Default::default(),
            sector: RABBIT_SECTOR,
            reference: (420, 814),
            lighting: true,
            fidget: true,
            walking: true,
        },
    );
    let rabbit = prepared
        .frames
        .iter()
        .find(|frame| frame.caster.tile == (420, 814))
        .expect("authored rabbit");
    assert_eq!(
        (rabbit.caster.tile, rabbit.caster.anchor),
        ((420, 814), (0, 0))
    );
    assert!(rabbit.casts_shadow);
    let walk = rabbit.walk.as_ref().expect(&rabbit.diagnostic);
    assert_eq!(walk.period, Duration::from_millis(41));
    assert_eq!(walk.deadline(23), Duration::from_millis(943));
    let first = walk.asset(walk.steps[0]).unwrap();
    let turned = walk.asset(walk.steps[17]).unwrap();
    assert_eq!(first.source.virtual_path, "art/monster/bun/bunuwxab.art");
    assert_eq!(
        (first.decoded.frame_count, first.decoded.frames_per_second),
        (6, 8)
    );
    assert_eq!(
        (first.decoded.stored_rotation, first.decoded.stored_palette),
        (5, 1)
    );
    assert_eq!(
        (first.decoded.frame.hotspot, first.decoded.frame.offset),
        ((23, 29), (9, 4))
    );
    assert_eq!(
        (turned.decoded.frame.hotspot, turned.decoded.frame.offset),
        ((13, 26), (-7, 4))
    );
    assert_eq!(walk.steps.last().unwrap().tile, (421, 817));
    let poses = rabbit.fidget.as_ref().expect(&rabbit.diagnostic);
    let stand = poses.sequence(None);
    assert_eq!(poses.original_direction, 3);
    assert_eq!(stand.frames.len(), 10);
    assert!(Arc::ptr_eq(&stand.frames[0], &rabbit.asset));
    assert_eq!(
        rabbit.asset.source.virtual_path,
        "art/monster/bun/bunuwxaa.art"
    );
    assert_eq!(rabbit.asset.decoded.stored_palette, 1);
    assert_eq!(rabbit.asset.decoded.stored_rotation, 5);
    assert_eq!(rabbit.asset.decoded.frame.hotspot, (10, 10));
    assert_ne!(stand.frames[0].decoded.frame, stand.frames[3].decoded.frame);
    assert_eq!(stand.one_shot_index(Duration::from_millis(999)), Some(9));
    assert_eq!(stand.one_shot_index(Duration::from_secs(1)), None);
    let normal = &poses.sequence(Some(5)).frames[3].decoded.frame;
    let mirrored = &stand.frames[3].decoded.frame;
    assert_eq!(
        mirrored.hotspot.0,
        normal.width as i32 - 1 - normal.hotspot.0
    );
    assert!(
        normal
            .pixels
            .chunks_exact(normal.width as usize)
            .zip(mirrored.pixels.chunks_exact(mirrored.width as usize))
            .all(|(a, b)| a.iter().eq(b.iter().rev()))
    );
}
