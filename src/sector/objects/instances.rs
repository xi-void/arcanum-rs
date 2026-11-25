//! Version 119 instance fields shared by sector lists and standalone files.
//! Descriptor order follows CE obj.h and obj.c (sub_40A400 / sub_40C030).
use super::*;

const COMMON: &[u8] = b"iliii444iiiiiii444iiiiiiiiiiii4cii48";
const ITEMS: &[u8] = b"ihiiiiiiiiiiiiiiiiiii48";
const CRITTER: &[u8] = b"ii4444iiii44hihhhhhiriirliiiiii48";

fn sections(category: u32) -> Option<[&'static [u8]; 5]> {
    let mut result = [COMMON, b"", b"", b"", b""];
    match category {
        0 => result[2] = b"iii48",
        1 => result[2] = b"iiiiii48",
        2 => result[2] = b"iiiiriiii48",
        3 => result[2] = b"ihii48",
        4 => result[2] = b"iiihii48",
        5..=14 => {
            result[2] = ITEMS;
            result[3] = match category {
                5 => b"iiii444iiiiiiiiiiiiiiiiii48",
                6 => b"iiiii48",
                7 => b"iiii44iiii48",
                8 => b"iiii48",
                12 => b"i4ii48",
                13 => b"iiiiii48",
                _ => b"iii48",
            };
        }
        15 | 16 => {
            result[2] = &CRITTER[..32];
            result[3] = &CRITTER[32..];
            result[4] = if category == 15 {
                b"ii48iiq4848i8444isi44ii48"
            } else {
                b"ihihhii8illiiihiir44iii4r"
            };
        }
        17 => result[2] = b"iii48",
        _ => return None,
    }
    Some(result)
}

pub(super) fn field_type(category: u32, index: usize) -> Option<u8> {
    let layout = sections(category)?;
    if index < 64 {
        return COMMON.get(index).copied();
    }
    layout.get(index / 32)?.get(index % 32).copied()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct OriginalId {
    pub kind: u16,
    pub payload: [u8; 16],
}

impl OriginalId {
    pub(crate) fn from_wire(bytes: [u8; 24]) -> Self {
        let kind = u16::from_le_bytes([bytes[0], bytes[1]]);
        let payload = if kind == 0 {
            [0; 16]
        } else {
            bytes[8..].try_into().unwrap()
        };
        Self { kind, payload }
    }

    pub(crate) fn key(self) -> String {
        let mut key = format!("{}:", self.kind);
        use std::fmt::Write as _;
        for byte in self.payload {
            write!(key, "{byte:02x}").expect("formatting into a String");
        }
        key
    }

    pub(crate) fn parse_key(key: &str) -> Option<Self> {
        let (kind, hash) = key.split_once(':')?;
        let hash = hash.as_bytes();
        if kind != "2" || hash.len() != 32 || !hash.iter().all(u8::is_ascii_hexdigit) {
            return None;
        }
        let mut payload = [0; 16];
        for (byte, pair) in payload.iter_mut().zip(hash.as_chunks::<2>().0) {
            *byte = u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()?;
        }
        Some(Self { kind: 2, payload })
    }
}

#[derive(Debug, Copy, Clone)]
pub(crate) enum FieldValue<'a> {
    Scalar(u32),
    Wide(u64),
    Reference(OriginalId),
    Text(&'a [u8]),
    Array {
        width: usize,
        values: &'a [u8],
        bitmap: &'a [u8],
    },
    Null,
}

impl FieldValue<'_> {
    pub(crate) fn scalar(self) -> Option<u32> {
        match self {
            Self::Scalar(n) => Some(n),
            _ => None,
        }
    }

    pub(crate) fn reference(self) -> Option<OriginalId> {
        match self {
            Self::Reference(id) if id.kind != 0 => Some(id),
            _ => None,
        }
    }
}

impl<'a> FieldValue<'a> {
    pub(crate) fn array_entry(self, index: usize) -> Option<&'a [u8]> {
        let Self::Array {
            width,
            values,
            bitmap,
        } = self
        else {
            return None;
        };
        let (&byte, prior) = bitmap.get(index / 8).zip(bitmap.get(..index / 8))?;
        let mask = 1 << (index % 8);
        if byte & mask == 0 {
            return None;
        }
        let rank = prior.iter().map(|x| x.count_ones() as usize).sum::<usize>()
            + (byte & (mask - 1)).count_ones() as usize;
        values.get(rank * width..(rank + 1) * width)
    }
}

pub(super) fn read_value<'a>(
    reader: &mut Reader<'a>,
    index: usize,
    category: u32,
) -> Result<FieldValue<'a>, Error> {
    use FieldValue::*;
    let ty = field_type(category, index).ok_or_else(|| {
        reader
            .error(format!(
                "unsupported object fields: category {category}, bit {index}"
            ))
            .classified("instance_fields_unsupported")
    })?;
    if ty == b'i' {
        return Ok(Scalar(reader.u32("object scalar field")?));
    }
    if let Some(width) = match ty {
        b'4' => Some(4),
        b'8' => Some(8),
        b'c' => Some(12),
        b'q' => Some(16),
        b'r' => Some(24),
        _ => None,
    } {
        return reader.optional_array(width);
    }
    if !reader.present()? {
        return Ok(Null);
    }
    Ok(match ty {
        b'l' => Wide(u64::from_le_bytes(reader.fixed("object wide value")?)),
        b'h' => Reference(OriginalId::from_wire(
            reader.fixed("object field reference")?,
        )),
        b's' => {
            let len = reader.count("object string length")?;
            let bytes = reader.take(
                len.checked_add(1)
                    .ok_or_else(|| reader.error("object string size overflow"))?,
                "object string",
            )?;
            if bytes.last() != Some(&0) {
                return Err(reader.error("object string is missing its terminator"));
            }
            Text(&bytes[..len])
        }
        _ => unreachable!("validated field descriptor"),
    })
}

#[derive(Debug)]
pub(crate) struct Instance<'a> {
    pub id: OriginalId,
    pub prototype: SerializedPrototypeId,
    pub category: u32,
    pub bitmap: [u32; 5],
    pub fields: Vec<(usize, FieldValue<'a>)>,
}

impl<'a> Instance<'a> {
    pub fn field(&self, key: usize) -> Option<FieldValue<'a>> {
        self.fields
            .binary_search_by_key(&key, |(index, _)| *index)
            .ok()
            .map(|i| self.fields[i].1)
    }

    pub fn appearance(&self) -> AppearanceFields {
        let mut fields = Fields::default();
        for &(index, value) in &self.fields {
            fields.project(index, self.category, value);
        }
        fields.appearance
    }

    pub fn tile(&self) -> Option<(u32, u32)> {
        match self.field(1)? {
            FieldValue::Wide(value) => Some((value as u32, (value >> 32) as u32)),
            _ => None,
        }
    }
}

pub(crate) fn decode_instance(input: &[u8]) -> Result<Instance<'_>, Error> {
    let mut reader = Reader { input, offset: 0 };
    if input.len() > MAX_PROTOTYPE_BYTES {
        return Err(reader.error("standalone instance exceeds the input limit"));
    }
    let (proto, identity, category) = super::instance_identity(&mut reader)?;
    let layout = sections(category).ok_or_else(|| {
        reader
            .error(format!("unsupported object category {category}"))
            .classified("instance_category_unsupported")
    })?;
    let count = u16::from_le_bytes(reader.fixed("object field count")?);
    let mut bitmap = [0u32; 5];
    let words = layout
        .iter()
        .rposition(|fields| !fields.is_empty())
        .expect("common fields")
        + 1;
    for word in &mut bitmap[..words] {
        *word = reader.u32("object field bitmap")?;
    }
    if bitmap.iter().map(|word| word.count_ones()).sum::<u32>() != u32::from(count) {
        return Err(reader
            .error("field count and bitmap disagree")
            .classified("instance_bitmap_count_mismatch"));
    }
    // Validate field IDs while reading the actual typed payload. Never infer
    // a boundary from the next object's apparent signature.
    let mut fields = Vec::with_capacity(usize::from(count));
    for (word, bits) in bitmap.into_iter().enumerate() {
        for bit in 0..32 {
            if bits & (1 << bit) != 0 {
                let index = 32 * word + bit;
                fields.push((index, read_value(&mut reader, index, category)?));
            }
        }
    }
    if reader.offset != input.len() {
        return Err(reader
            .error("trailing bytes after standalone object")
            .classified("instance_trailing_bytes"));
    }
    Ok(Instance {
        id: OriginalId::from_wire(identity),
        prototype: SerializedPrototypeId(proto),
        category,
        bitmap,
        fields,
    })
}

#[derive(Debug, Clone)]
pub(crate) struct PrototypeDefaults {
    pub appearance: AppearanceFields,
    pub current_art: Option<u32>,
    pub description: Option<u32>,
    pub dexterity: Option<i32>,
    pub subtype_flags: Option<u32>,
    pub inventory_art: Option<u32>,
    pub inventory_source: Option<u32>,
    pub inventory_scripts: bool,
}

pub(crate) fn has_inventory_scripts(value: FieldValue<'_>) -> bool {
    // SAP_GET, DROP, INSERT_ITEM, REMOVE_ITEM and TRANSFER.
    [4, 5, 21, 28, 32].into_iter().any(|slot| {
        value
            .array_entry(slot)
            .is_some_and(|entry| entry[8..12] != [0; 4])
    })
}

pub(crate) fn prototype_defaults(
    input: &[u8],
    id: SerializedPrototypeId,
    category: u32,
) -> Result<PrototypeDefaults, Error> {
    let mut reader = Reader { input, offset: 0 };
    let header = decode_header(input).map_err(|error| {
        reader
            .error(error.to_string())
            .classified("prototype_header_rejected")
    })?;
    if header.object_type != category || header.id != id {
        return Err(reader.error("prototype type or identity mismatch"));
    }
    let layout =
        sections(category).ok_or_else(|| reader.error("unsupported prototype category"))?;
    let words = layout
        .iter()
        .rposition(|fields| !fields.is_empty())
        .expect("common fields")
        + 1;
    reader.take(
        HEADER_SIZE + words * 4,
        "prototype header and change bitmap",
    )?;
    // The prototype stores all slots. Read the prefixes used by runtime systems.
    let mut fields = Fields::default();
    let mut description = None;
    let mut inventory_scripts = false;
    for index in 0..COMMON.len() {
        let value = read_value(&mut reader, index, category)?;
        if index == 22 {
            description = value.scalar();
        }
        if index == 31 {
            inventory_scripts = has_inventory_scripts(value);
        }
        fields.project(index, category, value);
    }
    let dexterity = if matches!(category, 15 | NPC) {
        for index in 64..66 {
            fields.read(&mut reader, index, category)?;
        }
        read_value(&mut reader, 66, category)?
            .array_entry(1)
            .map(|bytes| i32::from_le_bytes(bytes.try_into().expect("32-bit statistic")))
    } else {
        None
    };
    let (mut subtype_flags, mut inventory_art, mut inventory_source) = (None, None, None);
    if matches!(category, 2 | 5..=14) {
        subtype_flags = read_value(&mut reader, 64, category)?.scalar();
        for index in 65..=if category == 2 { 69 } else { 70 } {
            let value = read_value(&mut reader, index, category)?;
            match (category, index) {
                (2, 69) => inventory_source = value.scalar(),
                (5..=14, 70) => inventory_art = value.scalar(),
                _ => {}
            }
        }
    }
    Ok(PrototypeDefaults {
        appearance: fields.appearance,
        current_art: fields.art_id,
        description,
        dexterity,
        subtype_flags,
        inventory_art,
        inventory_source,
        inventory_scripts,
    })
}
