//! Original indexed shadow selection. Coordinates are in projected world pixels.

use crate::{compositor::Modulation, mes::Mes, resource_path::normalize};
use std::fmt;

pub const MAX_LOOKUP_BYTES: usize = 1024 * 1024;
const DIB_END: usize = 54;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    InvalidBitmap,
    InvalidBin(u8),
    UnsupportedArt(u32),
    MissingArtName(u16),
    InvalidArtName,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidBitmap => f.write_str("invalid or unsupported indexed shadow bitmap"),
            Self::InvalidBin(bin) => write!(f, "shadow lookup index {bin} has no valid frame"),
            Self::UnsupportedArt(raw) => write!(f, "unsupported extended shadow ART ID {raw:#x}"),
            Self::MissingArtName(num) => write!(f, "shadow ART number {num} has no name"),
            Self::InvalidArtName => f.write_str("invalid shadow ART name"),
        }
    }
}

impl std::error::Error for Error {}

/// Bounded Windows BITMAPINFOHEADER with uncompressed 8-bit, bottom-up rows.
/// The palette is skipped because TIG consumes the original byte indices.
#[derive(Debug)]
pub struct Lookup<'a> {
    width: usize,
    height: usize,
    pitch: usize,
    pixels: &'a [u8],
}

impl<'a> Lookup<'a> {
    pub fn decode(input: &'a [u8]) -> Result<Self, Error> {
        if input.len() < DIB_END || input.len() > MAX_LOOKUP_BYTES {
            return Err(Error::InvalidBitmap);
        }
        let word = |at| u32::from_le_bytes(input[at..at + 4].try_into().unwrap());
        if &input[..2] != b"BM"
            || word(2) as usize != input.len()
            || word(14) != 40
            || word(26) != 0x0008_0001
            || word(30) != 0
        {
            return Err(Error::InvalidBitmap);
        }
        let width = word(18) as i32;
        let height = word(22) as i32;
        if width <= 0 || height <= 0 {
            return Err(Error::InvalidBitmap);
        }
        let (width, height) = (width as usize, height as usize);
        let palette_count = match word(46) {
            0 => 256,
            count @ 1..=256 => count as usize,
            _ => return Err(Error::InvalidBitmap),
        };
        let pitch = width.next_multiple_of(4);
        let offset = word(10) as usize;
        let size = pitch.checked_mul(height).ok_or(Error::InvalidBitmap)?;
        let declared_pixels = word(34) as usize;
        if offset < DIB_END + 4 * palette_count || (declared_pixels != 0 && declared_pixels != size)
        {
            return Err(Error::InvalidBitmap);
        }
        let pixels = input
            .get(offset..)
            .and_then(|tail| tail.get(..size))
            .ok_or(Error::InvalidBitmap)?;
        Ok(Self {
            width,
            height,
            pitch,
            pixels,
        })
    }

    pub fn dimensions(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    fn index(&self, caster: (i32, i32), light: (i32, i32)) -> u8 {
        let coordinate = |origin: i32, target: i32, size: usize| {
            let delta = i64::from(target) - i64::from(origin) + (size / 2) as i64;
            usize::try_from(delta).ok().filter(|&pos| pos < size)
        };
        match (
            coordinate(caster.0, light.0, self.width),
            coordinate(caster.1, light.1, self.height),
        ) {
            (Some(x), Some(y)) => self.pixels[(self.height - y - 1) * self.pitch + x],
            _ => 0,
        }
    }
}

/// Light eligibility is independent of whether this source illuminates the scene.
#[derive(Debug, Clone, Copy)]
pub struct Candidate {
    pub center: (i32, i32),
    pub tint: [u8; 3],
    pub flags: u32,
    pub owned_by_caster: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mask {
    pub direction: u8,
    pub frame: u8,
    pub gray: u8,
}

impl Mask {
    pub fn modulation(self) -> Modulation<'static> {
        Modulation::Shadow(self.gray)
    }
}

/// The caller supplies candidates in original sector-list traversal order,
/// with effective light tints and flags at the requested world state.
pub fn select(
    lookup: &Lookup<'_>,
    caster: (i32, i32),
    ambient: [u8; 3],
    base_frame: u8,
    directional: bool,
    lights: impl IntoIterator<Item = Candidate>,
) -> Result<Vec<Mask>, Error> {
    let ambient = grayscale(ambient);
    let mut masks = Vec::with_capacity(5);
    if directional {
        for light in lights {
            if light.flags & 0x43 != 0 || light.owned_by_caster {
                continue;
            }
            let Some(delta) = grayscale(light.tint)
                .checked_sub(ambient)
                .filter(|&d| d > 0)
            else {
                continue;
            };
            let index = lookup.index(caster, light.center);
            if index == 0 {
                continue;
            }
            let bin = index.checked_sub(32).ok_or(Error::InvalidBin(index))?;
            let frame = bin % 7;
            let strength = two_fifths(delta);
            masks.push(Mask {
                direction: (bin / 7 + 16) % 32,
                frame,
                gray: strength - frame * (strength / 7),
            });
            if masks.len() == 4 {
                break;
            }
        }
        masks.sort_by_key(|mask| mask.frame);
    }
    if masks.first().is_none_or(|mask| mask.frame >= 2) {
        masks.insert(
            0,
            Mask {
                direction: 2,
                frame: base_frame,
                gray: two_fifths(ambient),
            },
        );
    }
    Ok(masks)
}

pub(crate) fn grayscale([r, g, b]: [u8; 3]) -> u8 {
    ((77 * u32::from(r) + 150 * u32::from(g) + 28 * u32::from(b)) / 255) as u8
}

fn two_fifths(value: u8) -> u8 {
    (2 * u16::from(value) / 5) as u8
}

/// Extended light ART uses four files of eight rotations, always palette zero.
#[derive(Debug, Clone, Copy)]
pub struct Art {
    number: u16,
    pub base_frame: u8,
}

impl Art {
    pub fn decode(raw: u32) -> Result<Self, Error> {
        if raw >> 28 != 9 || raw & 0xf != 1 {
            return Err(Error::UnsupportedArt(raw));
        }
        Ok(Self {
            number: ((raw >> 19) & 0x1ff) as u16,
            base_frame: ((raw >> 12) & 0x7f) as u8,
        })
    }

    pub fn resource(self, names: &Mes, direction: u8) -> Result<(String, u8), Error> {
        if direction >= 32 {
            return Err(Error::InvalidArtName);
        }
        let name = names
            .get(i32::from(self.number))
            .ok_or(Error::MissingArtName(self.number))?;
        let name = normalize(name).map_err(|_| Error::InvalidArtName)?;
        if name.contains('/') {
            return Err(Error::InvalidArtName);
        }
        Ok((
            format!("art/light/{name}_s{}.art", direction / 8),
            direction % 8,
        ))
    }
}

#[cfg(test)]
pub(crate) mod tests;
