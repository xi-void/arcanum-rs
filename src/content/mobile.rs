//! Render-only import of verified base-map mobile records.

use super::{
    art::{FrameCache, LoadedFrame, Reflection},
    load_mes, read_bounded,
    sector::read_static_prototype,
    shadow::LoadedLookup,
};
use crate::{
    isometric::project_world_tile,
    prototype::{MAX_PROTOTYPE_BYTES, SerializedPrototypeId},
    sector::objects::{self, AppearanceFields},
    shadow,
    vfs::Vfs,
};
use std::{collections::BTreeMap, error::Error, sync::Arc};

// Original standalone placements audited for this stationary slice. Other
// mobile families and the compiled mobile stream need separate validation.
pub(super) const MAP: &str = "bates mansion lev 1";
pub(super) const SECTOR: u64 = 67_108_865;
const VERIFIED_SHADOW: u32 = 0x9030_0001;
const RECORDS: [&str; 3] = [
    "g_0a68bdf7_1d83_4ec4_b208_b4c36b06bf87.mob",
    "g_931574ed_1454_40f7_aaab_5a5e19b13ef2.mob",
    "g_cdc859eb_d01d_472c_bc76_17750785cb55.mob",
];

#[derive(Debug, PartialEq, Eq)]
struct Pose {
    path: String,
    direction: u8,
    rotation: u8,
    frame: usize,
    palette: u8,
}

impl Pose {
    fn decode(raw: u32) -> Result<Self, &'static str> {
        if raw >> 28 != 2 || raw & 0x0008_07cf != 0 {
            return Err("expected an unarmed, shield-free standing critter");
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
        let direction = ((raw >> 11) & 7) as u8;
        let rotation = if (1..4).contains(&direction) {
            8 - direction
        } else {
            direction
        };
        Ok(Self {
            path: format!("art/critter/{body}{gender}/{body}{gender}{armor}xaa.art"),
            direction,
            rotation,
            frame: ((raw >> 14) & 31) as usize,
            palette: ((raw >> 4) & 3) as u8,
        })
    }

    fn mirrored(&self) -> bool {
        self.direction != self.rotation
    }
}

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
}

pub(super) struct MobileShadows {
    pub lookup: LoadedLookup,
    frames: Vec<Arc<LoadedFrame>>,
}

impl MobileShadows {
    pub fn frame(&self, mask: shadow::Mask) -> Arc<LoadedFrame> {
        Arc::clone(&self.frames[usize::from(mask.direction) * 7 + usize::from(mask.frame)])
    }

    fn load(
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
    reference: (u32, u32),
    lighting: bool,
) -> PreparedMobiles {
    let mut result = PreparedMobiles::default();
    let mut prototypes = BTreeMap::new();
    for name in RECORDS {
        let path = format!("maps/{MAP}/{name}");
        match load(vfs, cache, &mut prototypes, &path, reference) {
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
    prototypes: &mut BTreeMap<SerializedPrototypeId, AppearanceFields>,
    path: &str,
    reference: (u32, u32),
) -> Result<MobileFrame, Box<dyn Error>> {
    let start = std::time::Instant::now();
    let (_, bytes) = read_bounded(vfs, path, MAX_PROTOTYPE_BYTES)?;
    let mobile = objects::decode_mobile(&bytes)?;
    if super::world::tile_sector_id(mobile.tile) != SECTOR {
        return Err("mobile placement falls outside the audited sector".into());
    }
    let defaults = match prototypes.entry(mobile.prototype) {
        std::collections::btree_map::Entry::Occupied(entry) => *entry.get(),
        std::collections::btree_map::Entry::Vacant(entry) => {
            let (_, bytes) = read_static_prototype(vfs, mobile.prototype)?;
            *entry.insert(objects::decode_npc_prototype_appearance(
                &bytes,
                mobile.prototype,
            )?)
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
    let pose = cache.resolve(|| Pose::decode(mobile.art))?;
    let asset = cache.load_with_reflection(
        vfs,
        &pose.path,
        pose.frame,
        pose.palette,
        pose.rotation,
        if pose.mirrored() {
            Reflection::Critter
        } else {
            Reflection::None
        },
    )?;
    if asset.decoded.rotation_count != 8 {
        return Err("expected eight stored standing critter directions".into());
    }
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
    Ok(MobileFrame {
        caster: Caster {
            tile: mobile.tile,
            anchor,
        },
        offset,
        asset,
        diagnostic,
        casts_shadow,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_original_kitchen_stand_and_reflection() {
        let direct = Pose::decode(0x2880_2820).unwrap();
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
        let reflected = Pose::decode(0x2880_0820).unwrap();
        assert_eq!((reflected.direction, reflected.rotation), (1, 7));
        assert!(reflected.mirrored());
        assert!(Pose::decode(0x2880_2860).is_err());
    }
}

#[cfg(test)]
#[test]
#[ignore = "requires original Bates Mansion assets"]
fn installed_mobile_poses_and_shadow_bank_share_decoded_resources() {
    let game = std::env::var_os("ARCANUM_GAME_DIR").expect("set ARCANUM_GAME_DIR");
    let mut vfs = Vfs::from_game(game, crate::vfs::Policy::Ce).unwrap();
    let mut cache = FrameCache::default();
    let prepared = prepare(&mut vfs, &mut cache, (80, 81), true);
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
