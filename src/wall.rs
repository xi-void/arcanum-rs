//! Native placement and the verified legacy wall artwork selection.

use crate::{art::SpriteFrame, compositor::SpriteDraw, isometric::project_tile, mes::Mes};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WallArtId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArtFields {
    pub structure: u8,
    pub piece: u8,
    pub rotation: u8,
    pub variation: u8,
    pub palette: u8,
    pub flags: u8,
    pub damage: u16,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    NotWall,
    Unsupported(ArtFields),
    MissingStructure(u8),
    InvalidStructure(u8),
    MissingWallName(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotWall => f.write_str("current ART ID is outside the wall category"),
            Self::Unsupported(fields) => write!(f, "unsupported wall ART fields {fields:?}"),
            Self::MissingStructure(id) => write!(f, "no wall structure at ordinal {id}"),
            Self::InvalidStructure(id) => write!(f, "invalid wall structure at ordinal {id}"),
            Self::MissingWallName(name) => {
                write!(f, "wall family {name:?} is absent from wallname.mes")
            }
        }
    }
}

impl std::error::Error for Error {}

impl WallArtId {
    pub fn fields(self) -> Result<ArtFields, Error> {
        let raw = self.0;
        if raw >> 28 != 1 {
            return Err(Error::NotWall);
        }
        Ok(ArtFields {
            structure: (raw >> 20) as u8,
            piece: ((raw >> 14) & 63) as u8,
            rotation: ((raw >> 11) & 7) as u8,
            variation: ((raw >> 8) & 3) as u8,
            palette: ((raw >> 4) & 3) as u8,
            flags: (raw & 15) as u8,
            damage: (raw & 0x480) as u16,
        })
    }

    /// Resolve the verified undamaged interior base wall facing (rotation 1).
    pub fn resolve(self, structures: &Mes, names: &Mes) -> Result<String, Error> {
        let fields = self.fields()?;
        if fields.rotation != 1
            || fields.piece != 0
            || fields.palette != 0
            || fields.flags != 0
            || fields.damage != 0
            || self.0 & 0x40 != 0
        {
            return Err(Error::Unsupported(fields));
        }
        let id = fields.structure;
        let entry = structures
            .get(0)
            .and_then(|_| {
                structures
                    .entries()
                    .filter(|(key, _)| (0..1000).contains(key))
                    .nth(usize::from(id))
            })
            .ok_or(Error::MissingStructure(id))?
            .1;
        let interior = entry
            .split_ascii_whitespace()
            .next()
            .and_then(|word| word.get(..3))
            .filter(|stem| stem.bytes().all(|ch| ch.is_ascii_alphanumeric()))
            .ok_or(Error::InvalidStructure(id))?;
        if names.get(0).is_none()
            || !names.entries().any(|(key, value)| {
                key >= 0
                    && value
                        .get(..3)
                        .is_some_and(|stem| stem.eq_ignore_ascii_case(interior))
            })
        {
            return Err(Error::MissingWallName(interior.into()));
        }
        Ok(format!(
            "art/wall/{}bseu{}.art",
            interior.to_ascii_lowercase(),
            fields.variation
        ))
    }
}

/// Placement of the verified rotation-one wall at 100% scale.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placement {
    pub tile: (u32, u32),
    pub pixel_offset: (i32, i32),
}

impl Placement {
    pub fn draw(self, frame: &SpriteFrame, center: (u32, u32)) -> Option<SpriteDraw<'_>> {
        let delta = |tile, center| i32::try_from(i64::from(tile) - i64::from(center)).ok();
        let (x, y) = project_tile(delta(self.tile.0, center.0)?, delta(self.tile.1, center.1)?)?;
        // TIG changes the effective hotspot by (-40, +20) for this rotation.
        // Keep the decoded hotspot intact and express that as an anchor shift.
        let anchor = (
            i32::try_from(i64::from(x) + i64::from(self.pixel_offset.0) + 40).ok()?,
            i32::try_from(i64::from(y) + i64::from(self.pixel_offset.1) - 20).ok()?,
        );
        Some(SpriteDraw { frame, anchor })
    }
}

#[cfg(test)]
mod tests;
