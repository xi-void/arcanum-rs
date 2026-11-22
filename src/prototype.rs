//! Verified subset of the legacy version 119 prototype format.

use std::fmt;

pub const HEADER_SIZE: usize = 56;
pub const MAX_PROTOTYPE_BYTES: usize = 64 * 1024;
const VERSION: u32 = 119;
const WEAPON_TYPE: u32 = 5;
const AMMO_TYPE: u32 = 6;
const ARMOR_TYPE: u32 = 7;
const FOOD_TYPE: u32 = 9;
const KEY_TYPE: u32 = 11;
const GENERIC_TYPE: u32 = 14;
const ITEM_BITMAP_BYTES: usize = 16;
const DAMAGE_TYPES: usize = 5;
const RESISTANCE_TYPES: usize = 5;

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

    pub fn is_armour(self) -> bool {
        self.object_type == ARMOR_TYPE
    }

    pub fn is_weapon_or_ammo(self) -> bool {
        matches!(self.object_type, WEAPON_TYPE | AMMO_TYPE)
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
pub struct CommonItemFields {
    pub description: DescriptionId,
    pub weight: i32,
    pub worth: i32,
    pub discipline: i32,
    pub magic_tech_complexity: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WeaponFields {
    pub item: CommonItemFields,
    /// Legacy damage indices in order. None means the sparse index is absent.
    pub damage_lower: [Option<i32>; DAMAGE_TYPES],
    pub damage_upper: [Option<i32>; DAMAGE_TYPES],
    pub speed_factor: i32,
    pub range: i32,
    pub ammo_type: i32,
    pub ammo_consumption: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AmmoFields {
    pub item: CommonItemFields,
    pub ammo_type: i32,
    /// Quantity stored on the prototype, separate from the native definition.
    pub quantity: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WeaponAmmoFields {
    Weapon(WeaponFields),
    Ammo(AmmoFields),
}

impl WeaponAmmoFields {
    pub fn item(&self) -> &CommonItemFields {
        match self {
            Self::Weapon(fields) => &fields.item,
            Self::Ammo(fields) => &fields.item,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArmourFields {
    pub item: CommonItemFields,
    pub inventory_art_id: u32,
    pub flags: u32,
    pub armour_class_adjustment: i32,
    /// Legacy resistance indices in order, with absent entries preserved.
    pub resistance_adjustments: [Option<i32>; RESISTANCE_TYPES],
}

impl ArmourFields {
    /// Only inventory art IDs of the item type carry the coverage index.
    pub(crate) fn coverage_index(self) -> Option<u32> {
        let art_id = self.inventory_art_id;
        (art_id >> 28 == 6).then_some((art_id >> 14) & 7)
    }
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
    UnsupportedWeaponAmmoType(u32),
    UnsupportedArmourType(u32),
    DamageIndexOutOfRange,
    DamageMaskCountMismatch,
    ResistanceIndexOutOfRange,
    ResistanceMaskCountMismatch,
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
            ErrorKind::UnsupportedWeaponAmmoType(kind) => {
                write!(
                    f,
                    "object type {kind} is outside the Weapon and Ammo decoder"
                )
            }
            ErrorKind::UnsupportedArmourType(kind) => {
                write!(f, "object type {kind} is outside the Armour decoder")
            }
            ErrorKind::ResistanceIndexOutOfRange => {
                f.write_str("resistance bitmap contains an unsupported index")
            }
            ErrorKind::ResistanceMaskCountMismatch => {
                f.write_str("resistance bitmap count does not match the payload")
            }
            ErrorKind::DamageIndexOutOfRange => {
                f.write_str("damage bitmap contains an unsupported index")
            }
            ErrorKind::DamageMaskCountMismatch => {
                f.write_str("damage bitmap count does not match the payload")
            }
            ErrorKind::InvalidPresence(flag) => write!(f, "invalid optional-field flag {flag}"),
            ErrorKind::InvalidArrayWidth { expected, actual } => {
                write!(f, "array element width {actual}; expected {expected}")
            }
            ErrorKind::ArrayTooLarge => {
                f.write_str("array length exceeds the prototype input limit")
            }
            ErrorKind::TrailingData => f.write_str("trailing bytes after the prototype record"),
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
    let (item, _) = read_item(&mut input)?;
    input.skip(3 * 4)?; // Food flags and padding.
    input.array(4)?;
    input.array(8)?;
    input.finish()?;
    Ok((
        header,
        FoodFields {
            description: item.description,
            weight: item.weight,
            worth: item.worth,
            discipline: item.discipline,
            magic_tech_complexity: item.magic_tech_complexity,
        },
    ))
}

/// Decode selected Weapon or Ammo fields. The remaining slots are framed to
/// validate record length, with their contents left opaque.
pub fn decode_weapon_ammo(bytes: &[u8]) -> Result<(Header, WeaponAmmoFields), Error> {
    if bytes.len() > MAX_PROTOTYPE_BYTES {
        return Err(at(MAX_PROTOTYPE_BYTES, ErrorKind::TooLarge));
    }
    let header = decode_header(bytes)?;
    if !header.is_weapon_or_ammo() {
        return Err(at(
            52,
            ErrorKind::UnsupportedWeaponAmmoType(header.object_type),
        ));
    }
    let mut input = Reader {
        bytes,
        offset: HEADER_SIZE,
    };
    let (item, _) = read_item(&mut input)?;
    let fields = if header.object_type == WEAPON_TYPE {
        input.skip(4 * 4)?; // Flags, paper-doll art and hit modifiers.
        let damage_lower = input.damage()?;
        let damage_upper = input.damage()?;
        input.array(4)?; // Magic damage adjustments.
        let speed_factor = input.i32()?;
        input.skip(4)?; // Magic speed adjustment.
        let range = input.i32()?;
        input.skip(3 * 4)?; // Magic range adjustment and strength fields.
        let ammo_type = input.i32()?;
        let ammo_consumption = input.i32()?;
        input.skip(10 * 4)?; // Art, critical hit/miss fields and padding.
        WeaponAmmoFields::Weapon(WeaponFields {
            item,
            damage_lower,
            damage_upper,
            speed_factor,
            range,
            ammo_type,
            ammo_consumption,
        })
    } else {
        input.skip(4)?; // Flags.
        let quantity = input.i32()?;
        let ammo_type = input.i32()?;
        input.skip(2 * 4)?; // Padding.
        WeaponAmmoFields::Ammo(AmmoFields {
            item,
            ammo_type,
            quantity,
        })
    };
    input.array(4)?;
    input.array(8)?;
    input.finish()?;
    Ok((header, fields))
}

/// Decode selected Armour fields, framing the remaining slots to validate the record.
pub fn decode_armour(bytes: &[u8]) -> Result<(Header, ArmourFields), Error> {
    if bytes.len() > MAX_PROTOTYPE_BYTES {
        return Err(at(MAX_PROTOTYPE_BYTES, ErrorKind::TooLarge));
    }
    let header = decode_header(bytes)?;
    if !header.is_armour() {
        return Err(at(52, ErrorKind::UnsupportedArmourType(header.object_type)));
    }
    let mut input = Reader {
        bytes,
        offset: HEADER_SIZE,
    };
    let (item, inventory_art_id) = read_item(&mut input)?;
    let flags = input.u32()?;
    input.skip(4)?; // Paper doll art.
    let armour_class_adjustment = input.i32()?;
    input.skip(4)?; // Magical armour class adjustment.
    let resistance_adjustments = input.sparse_i32(
        ErrorKind::ResistanceIndexOutOfRange,
        ErrorKind::ResistanceMaskCountMismatch,
    )?;
    input.array(4)?; // Magical resistance adjustments.
    input.skip(4 * 4)?; // Stealth, unarmed damage, and padding.
    input.array(4)?;
    input.array(8)?;
    input.finish()?;
    Ok((
        header,
        ArmourFields {
            item,
            inventory_art_id,
            flags,
            armour_class_adjustment,
            resistance_adjustments,
        },
    ))
}

fn read_item(input: &mut Reader<'_>) -> Result<(CommonItemFields, u32), Error> {
    let description = read_description(input)?;
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
    input.skip(4)?; // Mana store.
    let inventory_art_id = input.u32()?;
    input.skip(2 * 4)?; // Inventory location and use-art fragment.
    let magic_tech_complexity = input.i32()?;
    let discipline = input.i32()?;
    input.skip(10 * 4)?; // Description keys, spell fields, AI action and padding.
    input.array(4)?;
    input.array(8)?;
    Ok((
        CommonItemFields {
            description,
            weight,
            worth,
            magic_tech_complexity,
            discipline,
        },
        inventory_art_id,
    ))
}

/// Read only the common description prefix for verified non-Food types.
pub(crate) fn decode_item_description(
    bytes: &[u8],
    header: Header,
) -> Result<Option<DescriptionId>, Error> {
    if !matches!(
        header.object_type,
        WEAPON_TYPE | AMMO_TYPE | ARMOR_TYPE | KEY_TYPE | GENERIC_TYPE
    ) {
        return Ok(None);
    }
    read_description(&mut Reader {
        bytes,
        offset: HEADER_SIZE,
    })
    .map(Some)
}

fn read_description(input: &mut Reader<'_>) -> Result<DescriptionId, Error> {
    input.skip(ITEM_BITMAP_BYTES)?;
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
    Ok(DescriptionId(input.i32()?))
}

fn at(offset: usize, kind: ErrorKind) -> Error {
    Error { offset, kind }
}

struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
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

    fn finish(&self) -> Result<(), Error> {
        if self.offset == self.bytes.len() {
            Ok(())
        } else {
            Err(at(self.offset, ErrorKind::TrailingData))
        }
    }

    fn damage(&mut self) -> Result<[Option<i32>; DAMAGE_TYPES], Error> {
        self.sparse_i32(
            ErrorKind::DamageIndexOutOfRange,
            ErrorKind::DamageMaskCountMismatch,
        )
    }

    fn sparse_i32<const N: usize>(
        &mut self,
        index_error: ErrorKind,
        count_error: ErrorKind,
    ) -> Result<[Option<i32>; N], Error> {
        let mut result = [None; N];
        let Some(array) = self.array_data(4)? else {
            return Ok(result);
        };
        let mut values = array.values.as_chunks::<4>().0.iter();
        for (word, bytes) in array.mask.as_chunks::<4>().0.iter().enumerate() {
            let bits = u32::from_le_bytes(*bytes);
            let offset = array.mask_offset + word * 4;
            if (word == 0 && bits >> N != 0) || (word > 0 && bits != 0) {
                return Err(at(offset, index_error));
            }
            if word == 0 {
                for (index, slot) in result.iter_mut().enumerate() {
                    if bits & (1 << index) != 0 {
                        let value = values.next().ok_or(at(offset, count_error))?;
                        *slot = Some(i32::from_le_bytes(*value));
                    }
                }
            }
        }
        if values.next().is_some() {
            return Err(at(array.mask_offset, count_error));
        }
        Ok(result)
    }

    fn array(&mut self, width: u32) -> Result<(), Error> {
        self.array_data(width)?;
        Ok(())
    }

    fn array_data(&mut self, width: u32) -> Result<Option<ArrayData<'a>>, Error> {
        if !self.present()? {
            return Ok(None);
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
        let start_values = self.offset;
        self.skip_array(count, width)?;
        let values = &self.bytes[start_values..self.offset];
        let bitmap_words = self.u32()?;
        let mask_offset = self.offset;
        self.skip_array(bitmap_words, 4)?;
        Ok(Some(ArrayData {
            values,
            mask: &self.bytes[mask_offset..self.offset],
            mask_offset,
        }))
    }

    fn skip_array(&mut self, count: u32, width: u32) -> Result<(), Error> {
        let size = u64::from(count) * u64::from(width);
        if size > MAX_PROTOTYPE_BYTES as u64 {
            return Err(at(self.offset, ErrorKind::ArrayTooLarge));
        }
        self.skip(size as usize)
    }
}

struct ArrayData<'a> {
    values: &'a [u8],
    mask: &'a [u8],
    mask_offset: usize,
}

#[cfg(test)]
pub(crate) mod tests;
