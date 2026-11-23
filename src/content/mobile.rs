//! Render-only import of verified base-map mobile records.

mod walk;
pub(super) use walk::{Step as WalkStep, Walk};

use super::{
    art::{FrameCache, FrameLoop, LoadedFrame, Reflection},
    load_mes, read_bounded,
    sector::read_static_prototype,
    shadow::LoadedLookup,
};
use crate::{
    isometric::project_world_tile,
    mes::Mes,
    prototype::{MAX_PROTOTYPE_BYTES, SerializedPrototypeId},
    sector::objects::{self, AppearanceFields},
    shadow,
    vfs::Vfs,
};
use std::{
    collections::BTreeMap,
    error::Error,
    sync::Arc,
    time::{Duration, Instant},
};

// Original standalone placements audited for this stationary slice. Other
// mobile families and the compiled mobile stream need separate validation.
pub(super) const MAP: &str = "bates mansion lev 1";
pub(super) const SECTOR: u64 = 67_108_865;
pub(super) const VERIFIED_SHADOW: u32 = 0x9030_0001;
const BATES_RECORDS: [&str; 3] = [
    "g_0a68bdf7_1d83_4ec4_b208_b4c36b06bf87.mob",
    "g_931574ed_1454_40f7_aaab_5a5e19b13ef2.mob",
    "g_cdc859eb_d01d_472c_bc76_17750785cb55.mob",
];

const RABBIT_MAP: &str = "k'na tha";
pub(super) const RABBIT_SECTOR: u64 = 805_306_374;
const RABBIT_RECORDS: [&str; 1] = ["g_5f490f51_d2d1_4e9d_ad68_07de7a74d824.mob"];

pub(super) fn supports_walk(map: &str, sector: u64) -> bool {
    map.eq_ignore_ascii_case(RABBIT_MAP) && sector == RABBIT_SECTOR
}

pub(super) fn supports(map: &str, sector: u64) -> bool {
    !records(map, sector).is_empty()
}

fn records(map: &str, sector: u64) -> &'static [&'static str] {
    if map.eq_ignore_ascii_case(MAP) && sector == SECTOR {
        &BATES_RECORDS
    } else if map.eq_ignore_ascii_case(RABBIT_MAP) && sector == RABBIT_SECTOR {
        &RABBIT_RECORDS
    } else {
        &[]
    }
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
    pub(super) fn decode(raw: u32, monster_names: Option<&Mes>) -> Result<Self, &'static str> {
        if raw & 0x0008_07cf != 0 {
            return Err("expected a weapon-free, shield-free standing critter");
        }
        if raw >> 28 == 12 {
            if raw & 0x0070_0000 != 0 {
                return Err("armored monster stand is outside the verified subset");
            }
            let species = ((raw >> 23) & 31) as i32;
            let name = monster_names
                .and_then(|names| names.get(species))
                .ok_or("missing monster name")?;
            let name = crate::resource_path::normalize(name).map_err(|_| "invalid monster name")?;
            if name.contains('/') {
                return Err("expected a monster name basename");
            }
            return Ok(Self::new(
                raw,
                format!("art/monster/{name}/{name}uwxaa.art"),
            ));
        }
        if raw >> 28 != 2 {
            return Err("unsupported standing ART type");
        }
        let body = ["hm", "df", "gh", "hg", "ef"]
            .get(((raw >> 24) & 7) as usize)
            .ok_or("unknown critter body type")?;
        let male = raw & 0x0800_0000 != 0;
        if !male && *body == "ef" {
            return Err("female elf body substitution is outside the verified subset");
        }
        let gender = if male { 'm' } else { 'f' };
        let armor = match (raw >> 20) & 15 {
            0 => "uw",
            1 => "v1",
            8 => "cd",
            _ => return Err("unsupported standing critter clothing"),
        };
        Ok(Self::new(
            raw,
            format!("art/critter/{body}{gender}/{body}{gender}{armor}xaa.art"),
        ))
    }

    fn new(raw: u32, path: String) -> Self {
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
    pub frames: Vec<MobileFrame>,
    pub shadows: Option<MobileShadows>,
    pub issues: Vec<String>,
}

pub(super) fn prepare(
    vfs: &mut Vfs,
    cache: &mut FrameCache,
    map: &str,
    sector: u64,
    reference: (u32, u32),
    lighting: bool,
    walking: bool,
) -> PreparedMobiles {
    let mut result = PreparedMobiles::default();
    let mut prototypes = BTreeMap::new();
    let names = if sector == RABBIT_SECTOR && map.eq_ignore_ascii_case(RABBIT_MAP) {
        match load_mes(vfs, "art/monster/monster.mes") {
            Ok(names) => Some(names),
            Err(error) => {
                result.issues.push(format!("monster names: {error}"));
                return result;
            }
        }
    } else {
        None
    };
    for name in records(map, sector) {
        let path = format!("maps/{map}/{name}");
        match load(
            vfs,
            cache,
            &mut prototypes,
            &path,
            (sector, reference),
            names.as_ref().map(|names| &names.mes),
            walking && names.is_some(),
        ) {
            Ok(frame) => result.frames.push(frame),
            Err(error) => result.issues.push(format!("{name}: {error}")),
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

fn load(
    vfs: &mut Vfs,
    cache: &mut FrameCache,
    prototypes: &mut BTreeMap<SerializedPrototypeId, (AppearanceFields, Option<i32>)>,
    path: &str,
    (sector, reference): (u64, (u32, u32)),
    monster_names: Option<&Mes>,
    walking: bool,
) -> Result<MobileFrame, Box<dyn Error>> {
    let start = std::time::Instant::now();
    let (_, bytes) = read_bounded(vfs, path, MAX_PROTOTYPE_BYTES)?;
    let mobile = objects::decode_mobile(&bytes)?;
    if super::world::tile_sector_id(mobile.tile) != sector {
        return Err("mobile placement falls outside the audited sector".into());
    }
    let (defaults, dexterity) = match prototypes.entry(mobile.prototype) {
        std::collections::btree_map::Entry::Occupied(entry) => *entry.get(),
        std::collections::btree_map::Entry::Vacant(entry) => {
            let (_, bytes) = read_static_prototype(vfs, mobile.prototype)?;
            let appearance = objects::decode_npc_prototype_appearance(&bytes, mobile.prototype)?;
            let dexterity = if walking {
                objects::decode_npc_prototype_dexterity(&bytes, mobile.prototype)?
            } else {
                None
            };
            *entry.insert((appearance, dexterity))
        }
    };
    let f = mobile.fields;
    let flags = f.flags.or(defaults.flags).unwrap_or_default();
    let critter = f
        .critter_flags
        .or(defaults.critter_flags)
        .unwrap_or_default();
    const UNSUPPORTED: u32 = 0x301f_93c7;
    if flags & UNSUPPORTED != 0
        || critter & 1 != 0
        || f.scale.or(defaults.scale) != Some(100)
        || f.blit_flags.or(defaults.blit_flags).unwrap_or_default() != 0
    {
        return Err(format!("unsupported mobile appearance (flags {flags:#x}, critter {critter:#x}, scale {:?}, blit {:?})", f.scale.or(defaults.scale), f.blit_flags.or(defaults.blit_flags)).into());
    }
    let shadow_id = f.shadow_art.or(defaults.shadow_art).unwrap_or(u32::MAX);
    let casts_shadow = shadow_id == VERIFIED_SHADOW;
    let shadow_note = if casts_shadow {
        "verified"
    } else {
        "not prepared"
    };
    let original_light = f.light_art.or(defaults.light_art).unwrap_or(u32::MAX);
    let light_flags = f.light_flags.or(defaults.light_flags).unwrap_or_default();
    if original_light != u32::MAX && light_flags & 1 == 0 {
        return Err(format!("unsupported mobile-owned emitter ART {original_light:#010x}").into());
    }
    let offset = (
        f.offset_x.or(defaults.offset_x).unwrap_or_default(),
        f.offset_y.or(defaults.offset_y).unwrap_or_default(),
    );
    let center = project_world_tile(mobile.tile, reference).ok_or("mobile projection overflow")?;
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
    let pose = cache.resolve(|| Pose::decode(mobile.art, monster_names))?;
    let asset = pose.load(vfs, cache, pose.frame)?;
    let diagnostic = format!(
        "mobile {path} prototype {} at {:?} ART {:#010x}: {}, direction {} (stored {}), frame {}, palette {}, hotspot {:?}, offset {offset:?}, shadow {shadow_id:#010x} ({shadow_note}); prepared in {:?}",
        mobile.prototype.0,
        mobile.tile,
        mobile.art,
        pose.path,
        pose.direction,
        pose.rotation,
        pose.frame,
        asset.decoded.stored_palette,
        asset.decoded.frame.hotspot,
        start.elapsed()
    );
    let (fidget, diagnostic) = if monster_names.is_some() && asset.decoded.frame_count > 1 {
        let flags2 = f
            .critter_flags2
            .or(defaults.critter_flags2)
            .unwrap_or_default();
        let spell = f.spell_flags.or(defaults.spell_flags).unwrap_or_default();
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
    let caster = Caster {
        tile: mobile.tile,
        anchor,
    };
    let (walk, diagnostic) = if walking {
        let start = Instant::now();
        let movement = if fidget.is_some()
            && offset == (0, 0)
            && critter == 0x2008
            && f.spell_flags.or(defaults.spell_flags).unwrap_or_default() == 0
            && f.critter_flags2
                .or(defaults.critter_flags2)
                .unwrap_or_default()
                == 0
        {
            mobile
                .dexterity
                .or(dexterity)
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
        caster,
        offset,
        asset,
        diagnostic,
        casts_shadow,
        fidget,
        walk,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
    let prepared = prepare(&mut vfs, &mut cache, MAP, SECTOR, (80, 81), true, false);
    assert_eq!(prepared.frames.len(), 3, "{:?}", prepared.issues);
    let original = &prepared.frames[0];
    let reflected = &prepared.frames[2];
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
    let prepared = prepare(
        &mut vfs,
        &mut cache,
        RABBIT_MAP,
        RABBIT_SECTOR,
        (420, 814),
        true,
        true,
    );
    let [rabbit] = prepared.frames.as_slice() else {
        panic!("{:?}", prepared.issues)
    };
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
