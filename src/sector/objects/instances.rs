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

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
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
        Self::parse_with_kind(key, 2)
    }

    pub(crate) fn parse_with_kind(key: &str, expected: u16) -> Option<Self> {
        let (kind, hash) = key.split_once(':')?;
        let kind: u16 = kind.parse().ok()?;
        let hash = hash.as_bytes();
        if kind != expected || hash.len() != 32 || !hash.iter().all(u8::is_ascii_hexdigit) {
            return None;
        }
        let mut payload = [0; 16];
        for (byte, pair) in payload.iter_mut().zip(hash.as_chunks::<2>().0) {
            *byte = u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()?;
        }
        Some(Self { kind, payload })
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

fn int_array<const N: usize>(value: FieldValue<'_>) -> [Option<i32>; N] {
    std::array::from_fn(|index| {
        value
            .array_entry(index)
            .map(|bytes| i32::from_le_bytes(bytes.try_into().expect("int32 array entry")))
    })
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub(crate) struct ItemStats {
    pub weight: Option<i32>,
    pub magic_weight: Option<i32>,
    pub complexity: Option<i32>,
    pub unknown_description: Option<u32>,
    pub use_art: Option<u32>,
    pub equipment_flags: Option<u32>,
    pub paper_doll_art: Option<u32>,
    pub armor_class: Option<i32>,
    pub magic_armor_class: Option<i32>,
    pub resistance: Option<[Option<i32>; 5]>,
    pub magic_resistance: Option<[Option<i32>; 5]>,
    pub weapon_hit_bonus: Option<i32>,
    pub weapon_speed: Option<i32>,
    pub weapon_min_strength: Option<i32>,
    pub wield_script: bool,
    pub quantity: Option<u32>,
    pub ammo_type: Option<u32>,
    pub spells: [Option<u32>; 5],
}

impl ItemStats {
    pub(crate) fn adjust_magic(&self, value: Option<i32>, aptitude: i32) -> i64 {
        let value = i64::from(value.unwrap_or_default());
        let complexity = i64::from(self.complexity.unwrap_or_default());
        let power = (complexity + i64::from(aptitude)) / 2;
        match complexity.cmp(&0) {
            std::cmp::Ordering::Greater => value * power.clamp(0, complexity) / complexity,
            std::cmp::Ordering::Less => value - value * power.clamp(complexity, 0) / complexity,
            std::cmp::Ordering::Equal => value,
        }
    }

    fn read(&mut self, category: u32, field: usize, value: FieldValue<'_>) {
        let scalar = value.scalar();
        let signed = scalar.map(|v| v as i32);
        match (category, field) {
            (_, 31) => {
                self.wield_script = [24, 25].into_iter().any(|slot| {
                    value
                        .array_entry(slot)
                        .is_some_and(|entry| entry[8..12] != [0; 4])
                })
            }
            (_, 66) => self.weight = signed,
            (_, 67) => self.magic_weight = signed,
            (_, 72) => self.use_art = scalar,
            (_, 73) => self.complexity = signed,
            (_, 75) => self.unknown_description = scalar,
            (_, 77..=81) => self.spells[field - 77] = scalar,
            (6 | 8, 97) => self.quantity = scalar,
            (6, 98) => self.ammo_type = scalar,
            (5 | 7 | 14, 96) => self.equipment_flags = scalar,
            (5 | 7, 97) => self.paper_doll_art = scalar,
            (5, 98) => self.weapon_hit_bonus = signed,
            (5, 103) => self.weapon_speed = signed,
            (5, 107) => self.weapon_min_strength = signed,
            (7, 98) => self.armor_class = signed,
            (7, 99) => self.magic_armor_class = signed,
            (7, 100) => self.resistance = Some(int_array(value)),
            (7, 101) => self.magic_resistance = Some(int_array(value)),
            _ => {}
        }
    }

    pub(crate) fn resolve(instance: &Instance<'_>, defaults: Option<&Self>) -> Self {
        let mut result = defaults.cloned().unwrap_or_default();
        for &(index, value) in &instance.fields {
            result.read(instance.category, index, value);
        }
        result
    }
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
    pub item: Option<Box<ItemStats>>,
    pub character: Option<Box<[Option<i32>; 28]>>,
    pub background: Option<i32>,
    pub original_art: Option<u32>,
    pub armor_class: Option<i32>,
    pub resistance: Option<[Option<i32>; 5]>,
    pub hp: [i32; 2],
    pub fatigue: [i32; 2],
    pub npc_flags: Option<u32>,
    pub npc_damage: [i32; 2],
    pub npc_unknown_description: Option<u32>,
    pub reaction_base: Option<i32>,
    pub social_class: Option<i32>,
    pub npc_leader: Option<OriginalId>,
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
    let mut hp = [0; 2];
    let mut npc_damage = [0; 2];
    let mut npc_unknown_description = None;
    let mut fatigue = [0; 2];
    let mut item = (5..=14).contains(&category).then(ItemStats::default);
    let (mut original_art, mut armor_class, mut resistance) = (None, None, None);
    for index in 0..COMMON.len() {
        let value = read_value(&mut reader, index, category)?;
        match index {
            22 => description = value.scalar(),
            23 if category == 15 => original_art = value.scalar(),
            25 if category == 15 => armor_class = value.scalar().map(|v| v as i32),
            26..=27 if matches!(category, 15 | NPC) => {
                hp[index - 26] = value.scalar().unwrap_or_default() as i32
            }
            28 if category == NPC => npc_damage[0] = value.scalar().unwrap_or_default() as i32,
            30 if category == 15 => resistance = Some(int_array(value)),
            _ => {}
        }
        if let Some(item) = &mut item {
            item.read(category, index, value);
        }
        if index == 31 {
            inventory_scripts = has_inventory_scripts(value);
        }
        fields.project(index, category, value);
    }
    let character = if matches!(category, 15 | NPC) {
        for index in 64..66 {
            fields.read(&mut reader, index, category)?;
        }
        Some(Box::new(int_array::<28>(read_value(
            &mut reader,
            66,
            category,
        )?)))
    } else {
        None
    };
    let mut background = None;
    let (mut npc_flags, mut reaction_base, mut npc_leader, mut social_class) =
        (None, None, None, None);
    if category == 15 {
        // PC-specific fields begin in bitmap word four. Background follows the two reputation arrays.
        for index in (67..=96).chain(128..=132) {
            if field_type(category, index).is_some() {
                let value = read_value(&mut reader, index, category)?;
                match index {
                    70..=71 => fatigue[index - 70] = value.scalar().unwrap_or_default() as i32,
                    132 => background = value.scalar().map(|v| v as i32),
                    _ => {}
                }
            }
        }
    }
    if category == NPC {
        for index in (67..=96).chain(128..=144) {
            if field_type(category, index).is_some() {
                let value = read_value(&mut reader, index, category)?;
                match index {
                    70..=71 => fatigue[index - 70] = value.scalar().unwrap_or_default() as i32,
                    72 => npc_damage[1] = value.scalar().unwrap_or_default() as i32,
                    86 => npc_unknown_description = value.scalar(),
                    128 => npc_flags = value.scalar(),
                    129 => npc_leader = value.reference(),
                    143 => reaction_base = value.scalar().map(|v| v as i32),
                    144 => social_class = value.scalar().map(|v| v as i32),
                    _ => {}
                }
            }
        }
    }
    let (mut subtype_flags, mut inventory_art, mut inventory_source) = (None, None, None);
    if matches!(category, 2 | 5..=14) {
        subtype_flags = read_value(&mut reader, 64, category)?.scalar();
        if let Some(item) = &mut item {
            let last = match category {
                5 => 107,
                6 => 98,
                7 => 101,
                8 => 97,
                14 => 96,
                _ => 95,
            };
            for index in (65..64 + ITEMS.len()).chain(96..=last) {
                let value = read_value(&mut reader, index, category)?;
                item.read(category, index, value);
                if index == 70 {
                    inventory_art = value.scalar();
                }
            }
        } else {
            for index in 65..=69 {
                let value = read_value(&mut reader, index, category)?;
                if index == 69 {
                    inventory_source = value.scalar();
                }
            }
        }
    }
    Ok(PrototypeDefaults {
        appearance: fields.appearance,
        current_art: fields.art_id,
        description,
        dexterity: character.as_ref().and_then(|values| values[1]),
        subtype_flags,
        inventory_art,
        inventory_source,
        inventory_scripts,
        item: item.map(Box::new),
        character,
        background,
        original_art,
        armor_class,
        resistance,
        hp,
        fatigue,
        npc_flags,
        npc_damage,
        npc_unknown_description,
        reaction_base,
        social_class,
        npc_leader,
    })
}
