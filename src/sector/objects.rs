//! Framing for the base-sector object records exercised by the static renderer.

use super::{Coordinates, MAX_INPUT_BYTES, SIDE, decode_floor};
use crate::prototype::{HEADER_SIZE, MAX_PROTOTYPE_BYTES, SerializedPrototypeId, decode_header};
use std::{fmt, ops::Range};

const WALL: u32 = 0;
const PORTAL: u32 = 1;
const SCENERY: u32 = 3;
const NPC: u32 = 16;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct AppearanceFields {
    pub offset_x: Option<i32>,
    pub offset_y: Option<i32>,
    pub shadow_art: Option<u32>,
    pub scale: Option<i32>,
    pub flags: Option<u32>,
    pub blit_flags: Option<u32>,
    pub blit_color: Option<u32>,
    pub alpha: Option<u32>,
    pub light_flags: Option<u32>,
    pub light_art: Option<u32>,
    pub light_color: Option<u32>,
    pub wall_flags: Option<u32>,
    pub scenery_flags: Option<u32>,
    pub portal_flags: Option<u32>,
    pub critter_flags: Option<u32>,
}

#[derive(Debug)]
pub struct StaticRecord {
    pub category: StaticCategory,
    pub index: usize,
    pub span: Range<usize>,
    pub prototype: SerializedPrototypeId,
    pub instance_id: [u8; 24],
    pub art_id: u32,
    pub stored_tile: (u32, u32),
    pub fields: AppearanceFields,
}

impl StaticRecord {
    /// Base sector loads replace the upper bits of both stored coordinates
    /// with the supplying sector's world origin.
    pub fn world_tile(&self, sector: Coordinates) -> Option<(u32, u32)> {
        sector.world_tile(
            (self.stored_tile.0 & 63) as u8,
            (self.stored_tile.1 & 63) as u8,
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StaticCategory {
    Wall,
    Scenery,
    Portal,
}

/// A scenery record whose current ART ID or instance location is absent.
#[derive(Debug)]
pub struct IncompleteScenery {
    pub index: usize,
    pub span: Range<usize>,
    pub prototype: SerializedPrototypeId,
    pub instance_id: [u8; 24],
    pub art_id: Option<u32>,
    pub stored_tile: Option<(u32, u32)>,
    pub fields: AppearanceFields,
}

#[derive(Debug)]
pub struct ObjectList {
    pub start: usize,
    pub count: usize,
    pub walls: Vec<StaticRecord>,
    pub scenery: Vec<StaticRecord>,
    pub incomplete_scenery: Vec<IncompleteScenery>,
    pub portals: Vec<StaticRecord>,
    pub scenery_count: usize,
    pub portal_count: usize,
}

#[derive(Debug)]
pub struct Error {
    pub offset: usize,
    pub message: String,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "byte {}: {}", self.offset, self.message)
    }
}

impl std::error::Error for Error {}

pub fn decode(input: &[u8]) -> Result<ObjectList, Error> {
    let floor = decode_floor(input).map_err(|error| Error {
        offset: 0,
        message: error.to_string(),
    })?;
    let start = floor.tile_offset + SIDE * SIDE * 4;
    let count_offset = input.len() - 4; // A valid floor already requires over 4 bytes.
    let mut reader = Reader {
        input: &input[..count_offset],
        offset: start,
    };
    match reader.u32("roof empty marker")? {
        0 => {
            reader.take(256 * 4, "roof IDs")?;
        }
        1 => {}
        _ => return Err(reader.error("invalid roof empty marker")),
    }
    let marker = reader.u32("sector metadata marker")?;
    if !(0xaa0000..=0xaa0004).contains(&marker) {
        return Err(reader.error(format!("unsupported sector metadata marker {marker:#x}")));
    }
    if marker >= 0xaa0001 {
        let count = reader.count("tile script count")?;
        reader.take_product(count, 24, "tile script records")?;
    }
    if marker >= 0xaa0002 {
        reader.take(12, "sector script record")?;
    }
    if marker >= 0xaa0003 {
        reader.take(24, "townmap, aptitude, light scheme and sound fields")?;
    }
    if marker >= 0xaa0004 {
        reader.take(512, "tile-block mask")?;
    }

    let start = reader.offset;
    let mut trailer = Reader {
        input,
        offset: count_offset,
    };
    let count = trailer.count("final object count")?;
    if count > (count_offset - start) / 70 {
        return Err(trailer.error("object count exceeds the available record headers"));
    }
    let mut list = ObjectList {
        start,
        count,
        walls: Vec::new(),
        scenery: Vec::new(),
        incomplete_scenery: Vec::new(),
        portals: Vec::new(),
        scenery_count: 0,
        portal_count: 0,
    };
    for index in 0..count {
        let start = reader.offset;
        let version = reader.u32("object version")?;
        if version != 119 {
            return Err(reader.error(format!(
                "unsupported object version {version} at record {index}"
            )));
        }
        let prototype = reader.fixed::<24>("object prototype identity")?;
        let proto_kind = u16::from_le_bytes(prototype[..2].try_into().unwrap());
        let proto_id = u32::from_le_bytes(prototype[8..12].try_into().unwrap());
        if proto_kind != 1 || !(1..=i32::MAX as u32).contains(&proto_id) {
            return Err(reader.error(format!("unsupported prototype identity at record {index}")));
        }
        let instance_id = reader.fixed::<24>("object identity")?;
        let identity = u16::from_le_bytes(instance_id[..2].try_into().unwrap());
        if !matches!(identity, 0 | 2 | 3) {
            return Err(reader.error(format!(
                "unsupported instance identity {identity} at record {index}"
            )));
        }
        let category = reader.u32("object category")?;
        let valid = match category {
            WALL => [u32::MAX, 0xf, 0x1f],
            SCENERY => [0x80c4_7f0f, 1, 5],
            PORTAL => [0x0004_0f0f, 0, 1],
            _ => {
                return Err(reader.error(format!(
                    "unsupported object category {category} at record {index}"
                )));
            }
        };
        let count = u16::from_le_bytes(reader.fixed("object field count")?);
        let bitmap = [
            reader.u32("object field bitmap")?,
            reader.u32("object field bitmap")?,
            reader.u32("object field bitmap")?,
        ];
        if bitmap
            .iter()
            .zip(valid)
            .any(|(bits, valid)| bits & !valid != 0)
        {
            return Err(reader.error(format!(
                "unsupported object fields {bitmap:08x?} at record {index}"
            )));
        }
        if bitmap.iter().map(|n| n.count_ones()).sum::<u32>() != u32::from(count) {
            return Err(reader.error(format!("field count and bitmap disagree at record {index}")));
        }

        let mut fields = Fields::default();
        for (word, bits) in bitmap.into_iter().enumerate() {
            for bit in 0..32 {
                if bits & (1 << bit) != 0 {
                    fields.read(&mut reader, word * 32 + bit, category)?;
                }
            }
        }
        let category = match category {
            WALL => StaticCategory::Wall,
            SCENERY => {
                list.scenery_count += 1;
                StaticCategory::Scenery
            }
            PORTAL => {
                list.portal_count += 1;
                StaticCategory::Portal
            }
            _ => unreachable!("already validated category"),
        };
        // Framing-only instances contribute to the category count and scenery audit.
        if category != StaticCategory::Wall && (fields.art_id.is_none() || fields.tile.is_none()) {
            if category == StaticCategory::Scenery {
                list.incomplete_scenery.push(IncompleteScenery {
                    index,
                    span: start..reader.offset,
                    prototype: SerializedPrototypeId(proto_id),
                    instance_id,
                    art_id: fields.art_id,
                    stored_tile: fields.tile,
                    fields: fields.appearance,
                });
            }
            continue;
        }
        let record = StaticRecord {
            category,
            index,
            span: start..reader.offset,
            prototype: SerializedPrototypeId(proto_id),
            instance_id,
            art_id: fields
                .art_id
                .ok_or_else(|| reader.error("missing current ART ID"))?,
            stored_tile: fields
                .tile
                .ok_or_else(|| reader.error("missing object location"))?,
            fields: fields.appearance,
        };
        match category {
            StaticCategory::Wall => list.walls.push(record),
            StaticCategory::Scenery => list.scenery.push(record),
            StaticCategory::Portal => list.portals.push(record),
        }
    }
    if reader.offset != count_offset {
        return Err(reader.error("object records do not end at the final count"));
    }
    Ok(list)
}

/// Read inherited appearance fields from a complete version 119 static prototype.
pub fn decode_wall_prototype(
    input: &[u8],
    id: SerializedPrototypeId,
) -> Result<AppearanceFields, Error> {
    decode_static_prototype(input, id, WALL, 69).map(|fields| fields.appearance)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SceneryPrototype {
    pub art_id: Option<u32>,
    pub appearance: AppearanceFields,
}

pub fn decode_scenery_prototype(
    input: &[u8],
    id: SerializedPrototypeId,
) -> Result<SceneryPrototype, Error> {
    let fields = decode_static_prototype(input, id, SCENERY, 70)?;
    Ok(SceneryPrototype {
        art_id: fields.art_id,
        appearance: fields.appearance,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PortalPrototype {
    pub art_id: u32,
    pub appearance: AppearanceFields,
}

pub fn decode_portal_prototype(
    input: &[u8],
    id: SerializedPrototypeId,
) -> Result<PortalPrototype, Error> {
    let fields = decode_static_prototype(input, id, PORTAL, 72)?;
    Ok(PortalPrototype {
        art_id: fields.art_id.ok_or_else(|| Error {
            offset: HEADER_SIZE + 12,
            message: "missing portal prototype ART ID".into(),
        })?,
        appearance: fields.appearance,
    })
}

fn decode_static_prototype(
    input: &[u8],
    id: SerializedPrototypeId,
    category: u32,
    end: usize,
) -> Result<Fields, Error> {
    let mut reader = Reader { input, offset: 0 };
    if input.len() > MAX_PROTOTYPE_BYTES {
        return Err(reader.error("static prototype exceeds the input limit"));
    }
    let header = decode_header(input).map_err(|error| Error {
        offset: error.offset,
        message: error.to_string(),
    })?;
    if header.object_type != category || header.id != id {
        return Err(reader.error("static prototype type or identity mismatch"));
    }
    reader.take(
        HEADER_SIZE + 12,
        "static prototype header and change bitmap",
    )?;
    let mut fields = Fields::default();
    // Unlike instances, prototypes serialize every declared slot.
    for index in (0..36).chain(64..end) {
        fields.read(&mut reader, index, category)?;
    }
    if reader.offset != input.len() {
        return Err(reader.error("trailing data after static prototype"));
    }
    Ok(fields)
}

#[derive(Default)]
struct Fields {
    art_id: Option<u32>,
    tile: Option<(u32, u32)>,
    appearance: AppearanceFields,
}

impl Fields {
    fn read(&mut self, reader: &mut Reader<'_>, index: usize, category: u32) -> Result<(), Error> {
        // Common fields occupy two bitmap words. The subtype starts at word 2.
        let field = "object field";
        match index {
            0 => self.art_id = Some(reader.u32(field)?),
            1 => {
                if reader.present()? {
                    self.tile = Some((reader.u32("tile x")?, reader.u32("tile y")?));
                }
            }
            5..=7 | 15..=17 | 30 | 34 => reader.array(4)?,
            31 => reader.array(12)?,
            35 => reader.array(8)?,
            65 if category == SCENERY => {
                if reader.present()? {
                    reader.take(24, "scenery reference identity")?;
                }
            }
            68 if category == SCENERY => reader.array(4)?,
            69 if category == SCENERY => reader.array(8)?,
            67 if category == WALL => reader.array(4)?,
            68 if category == WALL => reader.array(8)?,
            70 if category == PORTAL => reader.array(4)?,
            71 if category == PORTAL => reader.array(8)?,
            _ => {
                let value = reader.u32(field)?;
                match index {
                    2 => self.appearance.offset_x = Some(value as i32),
                    3 => self.appearance.offset_y = Some(value as i32),
                    4 => self.appearance.shadow_art = Some(value),
                    8 => self.appearance.blit_flags = Some(value),
                    9 => self.appearance.blit_color = Some(value),
                    10 => self.appearance.alpha = Some(value),
                    11 => self.appearance.scale = Some(value as i32),
                    12 => self.appearance.light_flags = Some(value),
                    13 => self.appearance.light_art = Some(value),
                    14 => self.appearance.light_color = Some(value),
                    18 => self.appearance.flags = Some(value),
                    64 if category == NPC => self.appearance.critter_flags = Some(value),
                    64 if category == WALL => self.appearance.wall_flags = Some(value),
                    64 if category == SCENERY => self.appearance.scenery_flags = Some(value),
                    64 if category == PORTAL => self.appearance.portal_flags = Some(value),
                    _ => {}
                }
            }
        }
        Ok(())
    }
}

struct Reader<'a> {
    input: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    fn error(&self, message: impl Into<String>) -> Error {
        Error {
            offset: self.offset,
            message: message.into(),
        }
    }

    fn take(&mut self, len: usize, section: &str) -> Result<&'a [u8], Error> {
        let bytes = self
            .input
            .get(self.offset..)
            .and_then(|tail| tail.get(..len))
            .ok_or_else(|| self.error(format!("truncated {section}")))?;
        self.offset += len;
        Ok(bytes)
    }

    fn fixed<const N: usize>(&mut self, section: &str) -> Result<[u8; N], Error> {
        Ok(self
            .take(N, section)?
            .try_into()
            .expect("validated field width"))
    }

    fn u32(&mut self, section: &str) -> Result<u32, Error> {
        Ok(u32::from_le_bytes(self.fixed(section)?))
    }

    fn count(&mut self, section: &str) -> Result<usize, Error> {
        let value = i32::from_le_bytes(self.fixed(section)?);
        usize::try_from(value).map_err(|_| self.error(format!("negative {section}")))
    }

    fn take_product(
        &mut self,
        count: usize,
        width: usize,
        section: &str,
    ) -> Result<&'a [u8], Error> {
        let len = count
            .checked_mul(width)
            .filter(|&len| len <= MAX_INPUT_BYTES)
            .ok_or_else(|| self.error(format!("{section} exceeds the input limit")))?;
        self.take(len, section)
    }

    fn present(&mut self) -> Result<bool, Error> {
        match self.fixed("optional field marker")? {
            [0] => Ok(false),
            [1] => Ok(true),
            [flag] => Err(self.error(format!("invalid optional field marker {flag}"))),
        }
    }

    fn array(&mut self, width: usize) -> Result<(), Error> {
        if self.present()? {
            let actual = self.u32("array element width")?;
            if actual as usize != width {
                return Err(self.error(format!("array element width {actual}; expected {width}")));
            }
            let count = self.count("array element count")?;
            self.take(4, "opaque array header word")?;
            self.take_product(count, width, "array elements")?;
            let words = self.count("array bitmap word count")?;
            let bitmap = self.take_product(words, 4, "array index bitmap")?;
            if bitmap
                .iter()
                .map(|byte| byte.count_ones() as usize)
                .sum::<usize>()
                != count
            {
                return Err(self.error("array element count and bitmap disagree"));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;

/// The render-relevant prefix of a standalone original mobile. The
/// remaining fields belong to gameplay and are not interpreted here.
#[derive(Debug)]
pub struct Mobile {
    pub identity: [u8; 24],
    pub prototype: SerializedPrototypeId,
    pub art: u32,
    pub tile: (u32, u32),
    pub fields: AppearanceFields,
}

pub fn decode_mobile(input: &[u8]) -> Result<Mobile, Error> {
    let mut reader = Reader { input, offset: 0 };
    if input.len() > MAX_PROTOTYPE_BYTES || reader.u32("mobile version")? != 119 {
        return Err(reader.error("expected bounded version 119 mobile"));
    }
    let prototype = reader.fixed::<24>("mobile prototype identity")?;
    let id = u32::from_le_bytes(prototype[8..12].try_into().unwrap());
    if prototype[..2] != [1, 0] || !(1..=i32::MAX as u32).contains(&id) {
        return Err(reader.error("unsupported mobile prototype identity"));
    }
    let identity = reader.fixed::<24>("mobile identity")?;
    if identity[..2] != [2, 0] || reader.u32("mobile object type")? != NPC {
        return Err(reader.error("expected original permanent NPC mobile"));
    }
    let count = u16::from_le_bytes(reader.fixed("mobile field count")?);
    let mut bitmap = [0u32; 5];
    for word in &mut bitmap {
        *word = reader.u32("mobile field bitmap")?;
    }
    if bitmap.iter().map(|x| x.count_ones()).sum::<u32>() != u32::from(count)
        || bitmap[1] & !0xf != 0
    {
        return Err(reader.error("invalid mobile field bitmap"));
    }
    let mut fields = Fields::default();
    for index in 0..=64 {
        if bitmap[index / 32] & (1 << (index % 32)) != 0 {
            fields.read(&mut reader, index, NPC)?;
        }
    }
    if (bitmap[2] & !1 != 0 || bitmap[3] != 0 || bitmap[4] != 0) && reader.offset == input.len() {
        return Err(reader.error("missing mobile subtype data"));
    }
    Ok(Mobile {
        identity,
        prototype: SerializedPrototypeId(id),
        art: fields
            .art_id
            .ok_or_else(|| reader.error("missing mobile current ART"))?,
        tile: fields
            .tile
            .ok_or_else(|| reader.error("missing mobile location"))?,
        fields: fields.appearance,
    })
}

pub fn decode_npc_prototype_appearance(
    input: &[u8],
    id: SerializedPrototypeId,
) -> Result<AppearanceFields, Error> {
    let mut reader = Reader { input, offset: 0 };
    if input.len() > MAX_PROTOTYPE_BYTES {
        return Err(reader.error("NPC prototype exceeds input limit"));
    }
    let header = decode_header(input).map_err(|error| Error {
        offset: error.offset,
        message: error.to_string(),
    })?;
    if header.object_type != NPC || header.id != id {
        return Err(reader.error("NPC prototype type or identity mismatch"));
    }
    reader.take(HEADER_SIZE + 20, "NPC prototype header and change bitmap")?;
    let mut fields = Fields::default();
    for index in (0..36).chain([64]) {
        fields.read(&mut reader, index, NPC)?;
    }
    Ok(fields.appearance)
}
