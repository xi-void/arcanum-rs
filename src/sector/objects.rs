//! Framing for the base-sector object records exercised by the static renderer.

use super::{Coordinates, MAX_INPUT_BYTES, SIDE, decode_floor};
use crate::prototype::{HEADER_SIZE, MAX_PROTOTYPE_BYTES, SerializedPrototypeId, decode_header};
use std::{fmt, ops::Range};

mod instances;
pub(crate) use instances::{
    FieldValue, Instance, ItemStats, OriginalId, PrototypeDefaults, decode_instance,
    has_inventory_scripts, prototype_defaults,
};

const WALL: u32 = 0;
const PORTAL: u32 = 1;
const SCENERY: u32 = 3;
const NPC: u32 = 16;
const TRAP: u32 = 17;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize)]
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
    pub critter_flags2: Option<u32>,
    pub spell_flags: Option<u32>,
    pub material: Option<u32>,
    pub sound_effect: Option<i32>,
    pub use_script: Option<i32>,
    pub unlock_script: Option<i32>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
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

/// A parsed trap that the alpha does not simulate or draw.
#[derive(Debug)]
pub struct TrapRecord {
    pub index: usize,
    pub prototype: SerializedPrototypeId,
    pub stored_tile: Option<(u32, u32)>,
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
    pub traps: Vec<TrapRecord>,
    pub(crate) tile_blocks: [u8; SIDE * SIDE / 8],
}

#[derive(Debug)]
pub struct Error {
    pub offset: usize,
    pub message: String,
    pub(crate) code: &'static str,
}

impl Error {
    fn classified(mut self, code: &'static str) -> Self {
        self.code = code;
        self
    }
}

#[derive(Debug, Default, serde::Serialize)]
pub(crate) struct ListEvidence {
    pub file_bytes: usize,
    pub metadata_marker: Option<u32>,
    pub declared_records: Option<usize>,
    pub decoded_prefix_records: usize,
    pub record: Option<usize>,
    pub record_start: Option<usize>,
    pub header: Option<InstanceHeader>,
    pub field: Option<usize>,
    pub field_start: Option<usize>,
    pub error_offset: Option<usize>,
    pub unsupported_bitmap: Option<[u32; 3]>,
    pub preparation_bytes: Option<usize>,
    pub preparation_budget_bytes: Option<usize>,
}

// Fixed prefix shared by version 119 object instances in sectors and mobiles.
// This observes the header only. Subtype fields and record boundaries require
// the appropriate field table and validated field decoding.
#[derive(Debug, serde::Serialize)]
pub(crate) struct InstanceHeader {
    pub version: u32,
    pub prototype_kind: Option<u16>,
    pub prototype: Option<u32>,
    pub instance_kind: Option<u16>,
    pub category: Option<u32>,
    pub field_count: Option<u16>,
    pub common_bitmap: Option<[u32; 2]>,
    pub bitmap: Option<Vec<u32>>,
    pub current_art: Option<u32>,
    pub location: Option<(u32, u32)>,
}

impl InstanceHeader {
    pub(crate) fn observe(input: &[u8]) -> Option<Self> {
        fn word<const N: usize>(input: &[u8], at: usize) -> Option<[u8; N]> {
            input.get(at..at + N)?.try_into().ok()
        }
        let u16_at = |at| word(input, at).map(u16::from_le_bytes);
        let u32_at = |at| word(input, at).map(u32::from_le_bytes);
        let version = u32_at(0)?;
        let prototype_kind = (version == 119).then(|| u16_at(4)).flatten();
        let instance = prototype_kind == Some(1);
        let category = instance.then(|| u32_at(52)).flatten();
        let bitmap_words = match category {
            // CE obj.c sub_40C030 and sub_40B8E0. Common fields take
            // two words, and each inherited section starts on a new word.
            Some(0..=4 | TRAP) => Some(3),
            Some(5..=14) => Some(4),
            Some(15 | NPC) => Some(5),
            _ => None,
        };
        let field_count = instance.then(|| u16_at(56)).flatten();
        let bitmap: Option<Vec<u32>> =
            bitmap_words.and_then(|words| (0..words).map(|i| u32_at(58 + 4 * i)).collect());
        let prefix = bitmap.as_ref().and_then(|bitmap| {
            if field_count.map(u32::from) != Some(bitmap.iter().map(|n| n.count_ones()).sum()) {
                return None;
            }
            let mut reader = Reader {
                input,
                offset: 58 + 4 * bitmap.len(),
            };
            let mut fields = Fields::default();
            for index in 0..2 {
                if bitmap[0] & (1 << index) != 0 {
                    fields.read(&mut reader, index, category?).ok()?;
                }
            }
            Some(fields)
        });
        Some(Self {
            version,
            prototype_kind,
            prototype: instance.then(|| u32_at(12)).flatten(),
            instance_kind: instance.then(|| u16_at(28)).flatten(),
            category,
            field_count,
            common_bitmap: instance.then(|| Some([u32_at(58)?, u32_at(62)?])).flatten(),
            bitmap,
            current_art: prefix.as_ref().and_then(|fields| fields.art_id),
            location: prefix.and_then(|fields| fields.tile),
        })
    }
}

pub(crate) fn category_name(category: u32) -> Option<&'static str> {
    [
        "wall",
        "portal",
        "container",
        "scenery",
        "projectile",
        "weapon",
        "ammo",
        "armor",
        "gold",
        "food",
        "scroll",
        "key",
        "key_ring",
        "written",
        "generic",
        "pc",
        "npc",
        "trap",
        "monster",
        "unique_npc",
    ]
    .get(category as usize)
    .copied()
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "byte {}: {}", self.offset, self.message)
    }
}

impl std::error::Error for Error {}

/// The original roof table follows the floor array. An empty marker stores no entries.
pub(crate) fn roof_words(input: &[u8], tile_offset: usize) -> Result<&[[u8; 4]], Error> {
    let mut reader = Reader {
        input: &input[..input.len().saturating_sub(4)],
        offset: tile_offset.saturating_add(SIDE * SIDE * 4),
    };
    reader.roof_words()
}

/// Authored town map association and sector aptitude, independent of object-list validity.
pub(crate) fn sector_properties(
    input: &[u8],
    tile_offset: usize,
) -> Result<SectorProperties, Error> {
    let mut reader = Reader {
        input: &input[..input.len().saturating_sub(4)],
        offset: tile_offset + SIDE * SIDE * 4,
    };
    metadata(&mut reader).map(|(_, properties)| properties)
}

#[derive(Default)]
pub(crate) struct SectorProperties {
    pub townmap: u32,
    pub aptitude: i32,
    pub sound_schemes: [i32; 2],
}

fn metadata(reader: &mut Reader<'_>) -> Result<(u32, SectorProperties), Error> {
    reader.roof_words()?;
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
    let mut properties = SectorProperties::default();
    if marker >= 0xaa0003 {
        properties.townmap = reader.u32("townmap ID")?;
        properties.aptitude = reader.u32("sector aptitude adjustment")? as i32;
        reader.take(8, "light scheme and sound flags")?;
        properties.sound_schemes = [
            reader.u32("sector music scheme")? as i32,
            reader.u32("sector ambient scheme")? as i32,
        ];
    }
    Ok((marker, properties))
}

pub fn decode(input: &[u8]) -> Result<ObjectList, Error> {
    decode_with_evidence(input).0
}

pub(crate) fn decode_with_evidence(input: &[u8]) -> (Result<ObjectList, Error>, ListEvidence) {
    let mut evidence = ListEvidence {
        file_bytes: input.len(),
        ..ListEvidence::default()
    };
    let result = decode_list(input, &mut evidence);
    if let Err(error) = &result {
        evidence.error_offset = Some(error.offset);
        if let Some(start) = evidence.record_start {
            evidence.header = InstanceHeader::observe(&input[start..input.len() - 4]);
        }
    } else if let Ok(list) = &result {
        evidence.decoded_prefix_records = list.count;
        evidence.record = None;
        evidence.record_start = None;
        evidence.field = None;
        evidence.field_start = None;
    }
    (result, evidence)
}

fn instance_identity(reader: &mut Reader<'_>) -> Result<(u32, [u8; 24], u32), Error> {
    let version = reader.u32("object version")?;
    if version != 119 {
        return Err(reader
            .error(format!("unsupported object version {version}"))
            .classified("instance_version_unsupported"));
    }
    let prototype = reader.fixed::<24>("object prototype identity")?;
    let proto_kind = u16::from_le_bytes(prototype[..2].try_into().unwrap());
    let proto_id = u32::from_le_bytes(prototype[8..12].try_into().unwrap());
    if proto_kind != 1 || !(1..=i32::MAX as u32).contains(&proto_id) {
        return Err(reader
            .error("unsupported prototype identity")
            .classified("instance_prototype_identity_unsupported"));
    }
    let instance_id = reader.fixed::<24>("object identity")?;
    let identity = u16::from_le_bytes(instance_id[..2].try_into().unwrap());
    if !matches!(identity, 0 | 2 | 3) {
        return Err(reader
            .error(format!("unsupported instance identity {identity}"))
            .classified("instance_identity_unsupported"));
    }
    let category = reader.u32("object category")?;
    Ok((proto_id, instance_id, category))
}

fn decode_list(input: &[u8], evidence: &mut ListEvidence) -> Result<ObjectList, Error> {
    let floor = decode_floor(input).map_err(|error| Error {
        offset: 0,
        message: error.to_string(),
        code: "sector_floor_decode_failed",
    })?;
    let start = floor.tile_offset + SIDE * SIDE * 4;
    let count_offset = input.len() - 4; // A valid floor already requires over 4 bytes.
    let mut reader = Reader {
        input: &input[..count_offset],
        offset: start,
    };
    let (marker, _) =
        metadata(&mut reader).map_err(|error| error.classified("sector_metadata_decode_failed"))?;
    evidence.metadata_marker = Some(marker);
    let tile_blocks = if marker >= 0xaa0004 {
        reader.fixed("tile-block mask")?
    } else {
        [0; SIDE * SIDE / 8]
    };

    let start = reader.offset;
    let mut trailer = Reader {
        input,
        offset: count_offset,
    };
    let count = trailer.count("final object count")?;
    evidence.declared_records = Some(count);
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
        traps: Vec::new(),
        tile_blocks,
    };
    for index in 0..count {
        let start = reader.offset;
        evidence.record = Some(index);
        evidence.record_start = Some(start);
        evidence.decoded_prefix_records = index;
        evidence.field = None;
        evidence.field_start = None;
        let (proto_id, instance_id, category) = instance_identity(&mut reader)?;
        let valid = match category {
            WALL => [u32::MAX, 0xf, 0x1f],
            // OBJ_F_HP_PTS (common bit 26) is an INT32. Its instance
            // value does not select artwork in the static renderer.
            SCENERY => [u32::MAX, 0xf, 0x3f],
            // CE defines portal flags, difficulty, key ID, notification and pads.
            PORTAL => [u32::MAX, 0xf, 0xff],
            TRAP => [u32::MAX, 0xf, 0x1f],
            _ => {
                return Err(reader
                    .error(format!(
                        "unsupported object category {category} at record {index}"
                    ))
                    .classified("sector_instance_category_unsupported"));
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
            evidence.unsupported_bitmap = Some(std::array::from_fn(|i| bitmap[i] & !valid[i]));
            return Err(reader
                .error(format!(
                    "unsupported object fields {bitmap:08x?} at record {index}"
                ))
                .classified("sector_instance_fields_unsupported"));
        }
        if bitmap.iter().map(|n| n.count_ones()).sum::<u32>() != u32::from(count) {
            return Err(reader
                .error(format!("field count and bitmap disagree at record {index}"))
                .classified("instance_bitmap_count_mismatch"));
        }

        let mut fields = Fields::default();
        for (word, bits) in bitmap.into_iter().enumerate() {
            for bit in 0..32 {
                if bits & (1 << bit) != 0 {
                    let field = word * 32 + bit;
                    evidence.field = Some(field);
                    evidence.field_start = Some(reader.offset);
                    fields.read(&mut reader, field, category)?;
                }
            }
        }
        evidence.field = None;
        evidence.field_start = None;
        if category == TRAP {
            list.traps.push(TrapRecord {
                index,
                prototype: SerializedPrototypeId(proto_id),
                stored_tile: fields.tile,
            });
            continue;
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
    evidence.record = None;
    evidence.record_start = None;
    evidence.decoded_prefix_records = count;
    if reader.offset != count_offset {
        return Err(reader
            .error("object records do not end at the final count")
            .classified("sector_object_list_end_mismatch"));
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
            code: "prototype_art_missing",
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
        code: "prototype_header_rejected",
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
        let value = instances::read_value(reader, index, category)?;
        self.project(index, category, value);
        Ok(())
    }

    fn project(&mut self, index: usize, category: u32, value: FieldValue<'_>) {
        if index == 31 {
            let script = |slot| {
                value.array_entry(slot).map_or(0, |entry| {
                    i32::from_le_bytes(entry[8..12].try_into().unwrap())
                })
            };
            self.appearance.use_script = Some(script(1));
            self.appearance.unlock_script = Some(script(3));
        }
        if index == 1
            && let FieldValue::Wide(tile) = value
        {
            self.tile = Some((tile as u32, (tile >> 32) as u32));
        }
        let Some(value) = value.scalar() else { return };
        match index {
            0 => self.art_id = Some(value),
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
            19 => self.appearance.spell_flags = Some(value),
            29 => self.appearance.material = Some(value),
            32 => self.appearance.sound_effect = Some(value as i32),
            64 if matches!(category, 15 | NPC) => self.appearance.critter_flags = Some(value),
            65 if matches!(category, 15 | NPC) => self.appearance.critter_flags2 = Some(value),
            64 if category == WALL => self.appearance.wall_flags = Some(value),
            64 if category == SCENERY => self.appearance.scenery_flags = Some(value),
            64 if category == PORTAL => self.appearance.portal_flags = Some(value),
            _ => {}
        }
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
            code: "object_data_unclassified",
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
            [flag] => Err(self
                .error(format!("invalid optional field marker {flag}"))
                .classified("object_optional_marker_invalid")),
        }
    }

    fn roof_words(&mut self) -> Result<&'a [[u8; 4]], Error> {
        match self.u32("roof empty marker")? {
            0 => Ok(self.take(256 * 4, "roof IDs")?.as_chunks::<4>().0),
            1 => Ok(&[]),
            _ => Err(self.error("invalid roof empty marker")),
        }
    }

    fn optional_array(&mut self, width: usize) -> Result<FieldValue<'a>, Error> {
        if !self.present()? {
            return Ok(FieldValue::Null);
        }
        let actual = self.u32("array element width")?;
        if actual as usize != width {
            return Err(self
                .error(format!("array element width {actual}; expected {width}"))
                .classified("object_array_width_mismatch"));
        }
        let count = self.count("array element count")?;
        self.take(4, "opaque array header word")?;
        let values = self.take_product(count, width, "array elements")?;
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
        Ok(FieldValue::Array {
            width,
            values,
            bitmap,
        })
    }
}

#[cfg(test)]
mod tests;

pub fn decode_npc_prototype_appearance(
    input: &[u8],
    id: SerializedPrototypeId,
) -> Result<AppearanceFields, Error> {
    Ok(prototype_defaults(input, id, NPC)?.appearance)
}
