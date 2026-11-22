//! Verified subset of the legacy version 119 prototype format.

use std::fmt;

pub const HEADER_SIZE: usize = 56;
pub const MAX_PROTOTYPE_BYTES: usize = 64 * 1024;
const VERSION: u32 = 119;
const FOOD_TYPE: u32 = 9;
const FOOD_BITMAP_BYTES: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SerializedPrototypeId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DescriptionId(pub i32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub version: u32,
    pub id: SerializedPrototypeId,
    pub object_type: u32,
}

impl Header {
    pub fn is_food(self) -> bool {
        self.object_type == FOOD_TYPE
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FoodFields {
    pub description: DescriptionId,
    pub weight: i32,
    pub worth: i32,
    pub discipline: i32,
    pub magic_tech_complexity: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Error {
    pub offset: usize,
    pub kind: ErrorKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    Truncated,
    UnsupportedVersion(u32),
    NotPrototype(i16),
    UnsupportedIdentity(i16),
    InvalidNumericId,
    InvalidObjectType,
    UnsupportedFoodType(u32),
    InvalidPresence(u8),
    InvalidArrayWidth { expected: u32, actual: u32 },
    ArrayTooLarge,
    TrailingData,
    TooLarge,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "prototype byte {}: ", self.offset)?;
        match self.kind {
            ErrorKind::Truncated => f.write_str("truncated field"),
            ErrorKind::UnsupportedVersion(v) => write!(f, "unsupported version {v}; expected 119"),
            ErrorKind::NotPrototype(kind) => {
                write!(f, "unsupported prototype marker {kind}; expected -1")
            }
            ErrorKind::UnsupportedIdentity(kind) => write!(
                f,
                "unsupported identity kind {kind}; expected numeric kind 1"
            ),
            ErrorKind::InvalidNumericId => {
                f.write_str("expected a positive signed 32-bit prototype ID")
            }
            ErrorKind::InvalidObjectType => f.write_str("negative object type"),
            ErrorKind::UnsupportedFoodType(kind) => {
                write!(f, "object type {kind} is outside the Food decoder")
            }
            ErrorKind::InvalidPresence(flag) => write!(f, "invalid optional-field flag {flag}"),
            ErrorKind::InvalidArrayWidth { expected, actual } => {
                write!(f, "array element width {actual}; expected {expected}")
            }
            ErrorKind::ArrayTooLarge => {
                f.write_str("array length exceeds the prototype input limit")
            }
            ErrorKind::TrailingData => f.write_str("trailing bytes after the Food record"),
            ErrorKind::TooLarge => write!(f, "input exceeds {MAX_PROTOTYPE_BYTES} bytes"),
        }
    }
}

impl std::error::Error for Error {}

/// Decode the fixed 56-byte prefix. The record body is outside this check.
pub fn decode_header(bytes: &[u8]) -> Result<Header, Error> {
    let mut input = Reader { bytes, offset: 0 };
    let version = input.u32()?;
    if version != VERSION {
        return Err(at(0, ErrorKind::UnsupportedVersion(version)));
    }
    let marker = i16::from_le_bytes(input.take()?);
    if marker != -1 {
        return Err(at(4, ErrorKind::NotPrototype(marker)));
    }
    input.skip(22)?;
    let identity = i16::from_le_bytes(input.take()?);
    if identity != 1 {
        return Err(at(28, ErrorKind::UnsupportedIdentity(identity)));
    }
    input.skip(6)?;
    let id = input.u32()?;
    if id == 0 || id > i32::MAX as u32 {
        return Err(at(36, ErrorKind::InvalidNumericId));
    }
    input.skip(12)?;
    let object_type = input.u32()?;
    if object_type > i32::MAX as u32 {
        return Err(at(52, ErrorKind::InvalidObjectType));
    }
    Ok(Header {
        version,
        id: SerializedPrototypeId(id),
        object_type,
    })
}

/// Decode the selected Food fields, framing the remaining slots without
/// interpreting them. This validates the complete record length.
pub fn decode_food(bytes: &[u8]) -> Result<(Header, FoodFields), Error> {
    if bytes.len() > MAX_PROTOTYPE_BYTES {
        return Err(at(MAX_PROTOTYPE_BYTES, ErrorKind::TooLarge));
    }
    let header = decode_header(bytes)?;
    if !header.is_food() {
        return Err(at(52, ErrorKind::UnsupportedFoodType(header.object_type)));
    }
    let mut input = Reader {
        bytes,
        offset: HEADER_SIZE,
    };
    input.skip(FOOD_BITMAP_BYTES)?;
    // Common fields in serialized order. The bitmap does not suppress slots.
    input.skip(4)?; // Current art.
    input.optional(8)?; // Location.
    input.skip(3 * 4)?; // X/Y offsets and shadow.
    for _ in 0..3 {
        input.array(4)?;
    } // Overlay and underlay lists.
    input.skip(7 * 4)?; // Blit and lighting scalars.
    for _ in 0..3 {
        input.array(4)?;
    } // Overlay lighting lists.
    input.skip(4 * 4)?; // Flags, blocking mask and internal name.
    let description = DescriptionId(input.i32()?);
    input.skip(7 * 4)?; // Art, armor class, HP, material and damage.
    input.array(4)?; // Resistances.
    input.array(12)?; // Script records.
    input.skip(2 * 4)?; // Sound and category.
    input.array(4)?;
    input.array(8)?;

    input.skip(4)?; // Item flags.
    input.optional(24)?; // Parent object ID.
    let weight = input.i32()?;
    input.skip(4)?; // Magic weight adjustment.
    let worth = input.i32()?;
    input.skip(4 * 4)?; // Mana store and inventory/use-art scalars.
    let magic_tech_complexity = input.i32()?;
    let discipline = input.i32()?;
    input.skip(10 * 4)?; // Description keys, spell fields, AI action and padding.
    input.array(4)?;
    input.array(8)?;

    input.skip(3 * 4)?; // Food flags and padding.
    input.array(4)?;
    input.array(8)?;
    if input.offset != bytes.len() {
        return Err(at(input.offset, ErrorKind::TrailingData));
    }
    Ok((
        header,
        FoodFields {
            description,
            weight,
            worth,
            discipline,
            magic_tech_complexity,
        },
    ))
}

fn at(offset: usize, kind: ErrorKind) -> Error {
    Error { offset, kind }
}

struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl Reader<'_> {
    fn take<const N: usize>(&mut self) -> Result<[u8; N], Error> {
        let start = self.offset;
        self.skip(N)?;
        Ok(self.bytes[start..self.offset]
            .try_into()
            .expect("checked field width"))
    }

    fn skip(&mut self, n: usize) -> Result<(), Error> {
        if n > self.bytes.len() - self.offset {
            return Err(at(self.offset, ErrorKind::Truncated));
        }
        self.offset += n;
        Ok(())
    }

    fn u32(&mut self) -> Result<u32, Error> {
        Ok(u32::from_le_bytes(self.take()?))
    }

    fn i32(&mut self) -> Result<i32, Error> {
        Ok(i32::from_le_bytes(self.take()?))
    }

    fn present(&mut self) -> Result<bool, Error> {
        let start = self.offset;
        match self.take::<1>()?[0] {
            0 => Ok(false),
            1 => Ok(true),
            flag => Err(at(start, ErrorKind::InvalidPresence(flag))),
        }
    }

    fn optional(&mut self, size: usize) -> Result<(), Error> {
        if self.present()? {
            self.skip(size)?;
        }
        Ok(())
    }

    fn array(&mut self, width: u32) -> Result<(), Error> {
        if !self.present()? {
            return Ok(());
        }
        let start = self.offset;
        let actual = self.u32()?;
        if actual != width {
            return Err(at(
                start,
                ErrorKind::InvalidArrayWidth {
                    expected: width,
                    actual,
                },
            ));
        }
        let count = self.u32()?;
        self.skip(4)?; // Opaque array header word.
        self.skip_array(count, width)?;
        let bitmap_words = self.u32()?;
        self.skip_array(bitmap_words, 4)
    }

    fn skip_array(&mut self, count: u32, width: u32) -> Result<(), Error> {
        let size = u64::from(count) * u64::from(width);
        if size > MAX_PROTOTYPE_BYTES as u64 {
            return Err(at(self.offset, ErrorKind::ArrayTooLarge));
        }
        self.skip(size as usize)
    }
}

#[cfg(test)]
pub(crate) mod tests;
