//! Base sector floor data and bounded legacy tile references.

use crate::mes::Mes;
pub mod objects;
use std::fmt;

pub const SIDE: usize = 64;
pub const MAX_INPUT_BYTES: usize = 8 * 1024 * 1024;
const LIGHT_BYTES: usize = 0x30;
const TILE_BYTES: usize = SIDE * SIDE * 4;
const SECTOR_MASK: u32 = (1 << 26) - 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Coordinates {
    pub x: u32,
    pub y: u32,
}

impl Coordinates {
    pub fn from_id(id: u64) -> Option<Self> {
        (id >> 52 == 0).then_some(Self {
            x: (id as u32) & SECTOR_MASK,
            y: (id >> 26) as u32 & SECTOR_MASK,
        })
    }

    pub fn id(self) -> Option<u64> {
        (self.x <= SECTOR_MASK && self.y <= SECTOR_MASK)
            .then_some(u64::from(self.x) | (u64::from(self.y) << 26))
    }

    pub fn world_tile(self, x: u8, y: u8) -> Option<(u32, u32)> {
        self.id()?;
        tile_index(x, y)?;
        Some(((self.x << 6) + u32::from(x), (self.y << 6) + u32::from(y)))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct TileArtId(pub u32);

#[derive(Debug, PartialEq, Eq)]
pub struct TileFields {
    pub terrain: [u8; 2],
    pub flippable: [bool; 2],
    pub outdoor: bool,
    pub mask: u8,
    pub variant: u8,
    pub flip_x: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub struct TileResolution {
    pub fields: TileFields,
    pub families: [String; 2],
    pub path: String,
    pub transition: bool,
}

impl TileArtId {
    pub fn decode(self) -> Result<TileFields, ArtError> {
        let id = self.0;
        if id >> 28 != 0 {
            return Err(ArtError::NotTile);
        }
        if id & 0x3e != 0 {
            return Err(ArtError::FlagsOrPalette);
        }
        let fields = TileFields {
            terrain: [((id >> 22) & 63) as u8, ((id >> 16) & 63) as u8],
            flippable: [id & 0x80 != 0, id & 0x40 != 0],
            outdoor: id & 0x100 != 0,
            mask: ((id >> 12) & 15) as u8,
            variant: ((id >> 9) & 7) as u8,
            flip_x: id & 1 != 0,
        };
        if fields.flip_x && fields.flippable != [true, true] {
            return Err(ArtError::Flip);
        }
        Ok(fields)
    }

    pub fn resolve(self, names: &Mes) -> Result<TileResolution, ArtError> {
        let fields = self.decode()?;
        let first = terrain_name(
            names,
            fields.outdoor,
            fields.flippable[0],
            u32::from(fields.terrain[0]),
        )?;
        let second = terrain_name(
            names,
            fields.outdoor,
            fields.flippable[1],
            u32::from(fields.terrain[1]),
        )?;
        let mask = fields.mask;
        let transition = !first.eq_ignore_ascii_case(second) && !matches!(mask, 0 | 15);
        // CE clears the flip flag before the filename callback interprets the mask.
        let (left, right, file_mask) = if transition {
            match (outdoor_rank(names, first), outdoor_rank(names, second)) {
                (None, _) => (first, "bse", mask),
                (_, None) => (second, "bse", 15 - mask),
                (Some(a), Some(b)) if a < b => (first, second, mask),
                _ => (second, first, 15 - mask),
            }
        } else if mask == 0 && !first.eq_ignore_ascii_case(second) {
            (second, "bse", mask)
        } else {
            (first, "bse", mask)
        };
        let suffix = b"06b489237ea5dc10"[usize::from(file_mask)] as char;
        let variant = (b'a' + fields.variant) as char;
        Ok(TileResolution {
            fields,
            families: [first.to_ascii_lowercase(), second.to_ascii_lowercase()],
            path: format!("art/tile/{left}{right}{suffix}{variant}.art").to_ascii_lowercase(),
            transition,
        })
    }
}

fn outdoor_rank(names: &Mes, name: &str) -> Option<usize> {
    names
        .entries()
        .filter(|&(key, _)| (0..200).contains(&key))
        .position(|(_, value)| {
            name_prefix(value).is_ok_and(|candidate| candidate.eq_ignore_ascii_case(name))
        })
}

fn terrain_name(names: &Mes, outdoor: bool, flippable: bool, index: u32) -> Result<&str, ArtError> {
    let start = (if outdoor { 0 } else { 200 }) + if flippable { 0 } else { 100 };
    let mut group = names
        .entries()
        .skip_while(|&(key, _)| key < start)
        .take_while(|&(key, _)| key < start + 100)
        .peekable();
    if !group.peek().is_some_and(|&(key, _)| key == start) {
        return Err(ArtError::TerrainName {
            group: start,
            index,
        });
    }
    let (_, value) = group.nth(index as usize).ok_or(ArtError::TerrainName {
        group: start,
        index,
    })?;
    name_prefix(value)
}

fn name_prefix(value: &str) -> Result<&str, ArtError> {
    let prefix = value
        .split_once('/')
        .map(|(name, _)| name)
        .or_else(|| value.get(..3));
    prefix
        .filter(|name| name.len() == 3 && name.bytes().all(|byte| byte.is_ascii_alphanumeric()))
        .ok_or(ArtError::InvalidTerrainName)
}

#[derive(Debug, PartialEq, Eq)]
pub enum ArtError {
    NotTile,
    Flip,
    FlagsOrPalette,
    TerrainName { group: i32, index: u32 },
    InvalidTerrainName,
}

impl fmt::Display for ArtError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotTile => write!(f, "art ID is not a tile"),
            Self::Flip => write!(f, "flip bit requires two flippable terrain components"),
            Self::FlagsOrPalette => write!(f, "tile requires unsupported flags or palette"),
            Self::TerrainName { group, index } => {
                write!(f, "missing terrain name {index} in MES group {group}")
            }
            Self::InvalidTerrainName => write!(f, "invalid three-character terrain name"),
        }
    }
}

impl std::error::Error for ArtError {}

#[derive(Debug)]
pub struct Floor {
    pub light_count: u32,
    pub tile_offset: usize,
    ids: [TileArtId; SIDE * SIDE],
}

impl Floor {
    pub fn ids(&self) -> impl Iterator<Item = TileArtId> + '_ {
        self.ids.iter().copied()
    }

    pub fn tile(&self, x: u8, y: u8) -> Option<TileArtId> {
        tile_index(x, y).map(|index| self.ids[index])
    }
}

pub fn tile_index(x: u8, y: u8) -> Option<usize> {
    let (x, y) = (usize::from(x), usize::from(y));
    (x < SIDE && y < SIDE).then_some(y * SIDE + x)
}

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    InputTooLarge,
    MissingLightCount,
    InvalidLightCount,
    TruncatedLights { expected_end: usize },
    TruncatedTiles { offset: usize },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InputTooLarge => write!(f, "sector exceeds {MAX_INPUT_BYTES} bytes"),
            Self::MissingLightCount => write!(f, "missing sector light count"),
            Self::InvalidLightCount => write!(f, "invalid sector light count"),
            Self::TruncatedLights { expected_end } => {
                write!(f, "truncated lights, expected end at byte {expected_end}")
            }
            Self::TruncatedTiles { offset } => {
                write!(f, "truncated sector tile array at byte {offset}")
            }
        }
    }
}

impl std::error::Error for Error {}

/// Read the light-list framing and exactly 4,096 little-endian tile IDs.
/// Later sector sections stay outside the floor importer.
pub fn decode_floor(input: &[u8]) -> Result<Floor, Error> {
    if input.len() > MAX_INPUT_BYTES {
        return Err(Error::InputTooLarge);
    }
    let (count, mut remaining) = input.split_first_chunk().ok_or(Error::MissingLightCount)?;
    let light_count =
        u32::try_from(i32::from_le_bytes(*count)).map_err(|_| Error::InvalidLightCount)?;
    let mut tile_offset = 4;
    for _ in 0..light_count {
        // The verified light wire record is fixed-width and has no length tag.
        // Its fields are opaque to the floor importer; there is no marker
        // to distinguish full-width misplaced bytes from a light record.
        let (_, rest) =
            remaining
                .split_first_chunk::<LIGHT_BYTES>()
                .ok_or(Error::TruncatedLights {
                    expected_end: tile_offset + LIGHT_BYTES,
                })?;
        remaining = rest;
        tile_offset += LIGHT_BYTES;
    }
    let tiles = remaining.get(..TILE_BYTES).ok_or(Error::TruncatedTiles {
        offset: tile_offset,
    })?;
    let (words, _) = tiles.as_chunks::<4>();
    Ok(Floor {
        light_count,
        tile_offset,
        ids: std::array::from_fn(|i| TileArtId(u32::from_le_bytes(words[i]))),
    })
}

#[cfg(test)]
mod tests;
