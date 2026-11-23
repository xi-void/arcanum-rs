//! Read-only loading of original shadow lookup and mask artwork.

use super::{Source, art, read_bounded};
use crate::{mes::Mes, shadow, vfs::Vfs};
use std::error::Error;

pub struct LoadedLookup {
    pub source: Source,
    bytes: Vec<u8>,
}

impl LoadedLookup {
    pub fn load(vfs: &mut Vfs) -> Result<Self, Box<dyn Error>> {
        let path = "art/light/shadowmap.bmp";
        let (source, bytes) = read_bounded(vfs, path, shadow::MAX_LOOKUP_BYTES)?;
        shadow::Lookup::decode(&bytes).map_err(|error| format!("{path} from {source}: {error}"))?;
        Ok(Self { source, bytes })
    }

    pub fn lookup(&self) -> shadow::Lookup<'_> {
        shadow::Lookup::decode(&self.bytes).expect("validated original shadow lookup")
    }
}

/// Resolve one mask according to the original light MES and extended direction.
pub fn load_mask(
    vfs: &mut Vfs,
    names: &Mes,
    art: shadow::Art,
    mask: shadow::Mask,
) -> Result<art::LoadedFrame, Box<dyn Error>> {
    let (path, rotation) = art.resource(names, mask.direction)?;
    let frame =
        art::read_art(vfs, &path)?.decode_direction(usize::from(mask.frame), rotation, 0)?;
    if frame.decoded.rotation_count != 8 {
        return Err(format!(
            "{path} from {}: expected eight shadow rotations",
            frame.source
        )
        .into());
    }
    Ok(frame)
}

/// A cached shadow sprite at the caster's projected anchor, before camera
/// translation. The caller supplies masks in original shadow-pass order.
pub struct PreparedMask {
    pub frame: std::sync::Arc<art::LoadedFrame>,
    pub anchor: (i32, i32),
    pub mask: shadow::Mask,
}
