//! Base sector floor data and bounded legacy tile references.

use crate::mes::Mes;
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TileArtId(pub u32);

impl TileArtId {
    /// Resolve the unflipped base filename cases. Mixed transition masks
    /// require CE's terrain-pair arrangement rules and are outside this slice.
    pub fn base_path(self, names: &Mes) -> Result<String, ArtError> {
        let id = self.0;
        if id >> 28 != 0 {
            return Err(ArtError::NotTile);
        }
        if id & 1 != 0 {
            return Err(ArtError::Flip);
        }
        if id & 0x3e != 0 {
            return Err(ArtError::FlagsOrPalette);
        }
        let outdoor = id & 0x100 != 0;
        let first = terrain_name(names, outdoor, id & 0x80 != 0, (id >> 22) & 63)?;
        let second = terrain_name(names, outdoor, id & 0x40 != 0, (id >> 16) & 63)?;
        let mask = ((id >> 12) & 15) as usize;
        let terrain = match mask {
            15 => first,
            _ if first.eq_ignore_ascii_case(second) => first,
            0 => second,
            _ => return Err(ArtError::Transition),
        };
        let suffix = b"06b489237ea5dc10"[mask] as char;
        let variant = (b'a' + ((id >> 9) & 7) as u8) as char;
        Ok(format!(
            "art/tile/{}bse{suffix}{variant}.art",
            terrain.to_ascii_lowercase()
        ))
    }
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
    Transition,
    TerrainName { group: i32, index: u32 },
    InvalidTerrainName,
}

impl fmt::Display for ArtError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotTile => write!(f, "art ID is not a tile"),
            Self::Flip => write!(f, "flipped tiles are unsupported"),
            Self::FlagsOrPalette => write!(f, "tile requires unsupported flags or palette"),
            Self::Transition => write!(f, "mixed terrain transitions are unsupported"),
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
