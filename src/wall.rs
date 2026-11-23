//! Native placement and the verified legacy wall artwork selection.

use crate::{art::SpriteFrame, compositor::SpriteDraw, isometric::project_world_tile, mes::Mes};
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

    /// Resolve verified undamaged interior and exterior wall facings.
    pub fn resolve(self, structures: &Mes, names: &Mes) -> Result<String, Error> {
        let fields = self.fields()?;
        if !matches!(
            (fields.rotation, fields.palette, fields.flags),
            (1 | 5, 0, 0) | (3 | 7, 1, 1)
        ) || fields.damage != 0
            || self.0 & 0x40 != 0
        {
            return Err(Error::Unsupported(fields));
        }
        let piece = match fields.piece {
            0 | 2 => "bse",
            1 => "lfc",
            3 => "bcl",
            4 => "bcr",
            5 => "tcl",
            6 => "tcr",
            9..=11 => ["w3l", "w3a", "w3r"][(fields.piece - 9) as usize],
            12..=15 => ["w4l", "w4a", "w4b", "w4r"][(fields.piece - 12) as usize],
            21..=23 => ["d3l", "d3a", "d3r"][(fields.piece - 21) as usize],
            24..=27 => ["d4l", "d4a", "d4b", "d4r"][(fields.piece - 24) as usize],
            28..=33 => ["d6l", "d6a", "d6b", "d6c", "d6d", "d6r"][(fields.piece - 28) as usize],
            34..=36 => ["p3l", "p3a", "p3r"][(fields.piece - 34) as usize],
            37..=40 => ["p4l", "p4a", "p4b", "p4r"][(fields.piece - 37) as usize],
            41..=45 => ["p5l", "p5a", "p5b", "p5c", "p5r"][(fields.piece - 41) as usize],
            _ => return Err(Error::Unsupported(fields)),
        };
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
        let facing = entry
            .split_ascii_whitespace()
            .nth(usize::from((2..=5).contains(&fields.rotation)))
            .and_then(|word| word.get(..3))
            .filter(|stem| stem.bytes().all(|ch| ch.is_ascii_alphanumeric()))
            .ok_or(Error::InvalidStructure(id))?;
        if names.get(0).is_none()
            || !names.entries().any(|(key, value)| {
                key >= 0
                    && value
                        .get(..3)
                        .is_some_and(|stem| stem.eq_ignore_ascii_case(facing))
            })
        {
            return Err(Error::MissingWallName(facing.into()));
        }
        Ok(format!(
            "art/wall/{}{piece}u{}.art",
            facing.to_ascii_lowercase(),
            fields.variation
        ))
    }
}

impl ArtFields {
    pub fn mirror_x(self) -> bool {
        self.flags & 1 != 0
    }

    /// Adjust the legacy hotspot. Mirroring uses the wall blitter's width-minus-two rule.
    pub fn pivot(self, frame: &SpriteFrame) -> (i64, i64) {
        opening_pivot(frame, self.rotation, self.mirror_x())
    }

    /// Within-cell ordering is independent of the visual offset.
    pub fn order_key(self) -> (i32, i32) {
        opening_order_key(self.rotation)
    }
}

/// Walls and portals share the legacy directional and mirrored hotspot rules.
pub(crate) fn opening_pivot(frame: &SpriteFrame, rotation: u8, mirror: bool) -> (i64, i64) {
    let (mut x, mut y) = (i64::from(frame.hotspot.0), i64::from(frame.hotspot.1));
    if !(2..=5).contains(&rotation) {
        x -= 40;
        y += 20;
    }
    if mirror {
        x = i64::from(frame.width) - x - 2;
    }
    (x, y)
}

pub(crate) fn opening_order_key(rotation: u8) -> (i32, i32) {
    // (depth, horizontal) from sub_4F2230 and sub_4B93F0.
    if (2..=5).contains(&rotation) {
        (-2, 38)
    } else {
        (-40, 0)
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
        let pivot = (
            i64::from(frame.hotspot.0) - 40,
            i64::from(frame.hotspot.1) + 20,
        );
        self.draw_with_pivot(frame, center, pivot)
    }

    pub fn draw_with_pivot(
        self,
        frame: &SpriteFrame,
        center: (u32, u32),
        pivot: (i64, i64),
    ) -> Option<SpriteDraw<'_>> {
        let (x, y) = project_world_tile(self.tile, center)?;
        // Keep the ART hotspot intact and express the effective pivot as an anchor shift.
        let anchor = (
            i32::try_from(
                (i64::from(x) + i64::from(self.pixel_offset.0) + i64::from(frame.hotspot.0))
                    .checked_sub(pivot.0)?,
            )
            .ok()?,
            i32::try_from(
                (i64::from(y) + i64::from(self.pixel_offset.1) + i64::from(frame.hotspot.1))
                    .checked_sub(pivot.1)?,
            )
            .ok()?,
        );
        Some(SpriteDraw { frame, anchor })
    }
}

#[cfg(test)]
mod tests;
