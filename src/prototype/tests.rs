use super::*;

// Synthetic record with all optional fields absent. These offsets follow
// from the version 119 field sequence, independent of any installed data.
pub(crate) fn food_bytes(id: u32, description: i32, weight: i32) -> Vec<u8> {
    let mut bytes = vec![0; 280];
    bytes[0..4].copy_from_slice(&119u32.to_le_bytes());
    bytes[4..6].copy_from_slice(&(-1i16).to_le_bytes());
    bytes[28..30].copy_from_slice(&1i16.to_le_bytes());
    bytes[36..40].copy_from_slice(&id.to_le_bytes());
    bytes[52..56].copy_from_slice(&9u32.to_le_bytes());
    for (offset, value) in [
        (139, description),
        (188, weight),
        (196, 42),
        (216, -7),
        (220, 1),
    ] {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    bytes
}

#[test]
fn reads_only_the_fixed_header_for_other_types() {
    let mut bytes = food_bytes(9019, 42, 8);
    bytes[52..56].copy_from_slice(&13u32.to_le_bytes());
    bytes[6..28].fill(0x71);
    bytes[30..36].fill(0x83);
    bytes[40..52].fill(0x95);
    assert_eq!(
        decode_header(&bytes[..HEADER_SIZE]),
        Ok(Header {
            version: 119,
            id: SerializedPrototypeId(9019),
            object_type: 13,
        })
    );
    assert_eq!(
        decode_food(&bytes).unwrap_err().kind,
        ErrorKind::UnsupportedFoodType(13)
    );
}

#[test]
fn invalid_headers_and_truncated_records_report_offsets() {
    let bytes = food_bytes(9019, 42, 8);
    for len in 0..HEADER_SIZE {
        assert_eq!(
            decode_header(&bytes[..len]).unwrap_err().kind,
            ErrorKind::Truncated,
            "{len}"
        );
    }
    for len in HEADER_SIZE..bytes.len() {
        assert_eq!(
            decode_food(&bytes[..len]).unwrap_err().kind,
            ErrorKind::Truncated,
            "{len}"
        );
    }
    for (offset, contents, kind) in [
        (0, &[120, 0, 0, 0][..], ErrorKind::UnsupportedVersion(120)),
        (4, &[0, 0], ErrorKind::NotPrototype(0)),
        (28, &[2, 0], ErrorKind::UnsupportedIdentity(2)),
        (36, &[0; 4], ErrorKind::InvalidNumericId),
        (52, &[0xff; 4], ErrorKind::InvalidObjectType),
        (76, &[2], ErrorKind::InvalidPresence(2)),
    ] {
        let mut malformed = bytes.clone();
        malformed[offset..offset + contents.len()].copy_from_slice(contents);
        assert_eq!(decode_food(&malformed), Err(Error { offset, kind }));
    }
}

#[test]
fn food_follows_variable_field_framing() {
    let mut bytes = food_bytes(9019, 42, 8);
    // One common overlay array before the description. The additional header
    // word, element data and index word remain opaque.
    bytes[89] = 1;
    bytes.splice(
        90..90,
        [4u32, 1, 999, 123, 1, 1]
            .into_iter()
            .flat_map(u32::to_le_bytes),
    );
    assert_eq!(
        decode_food(&bytes).unwrap().1,
        FoodFields {
            description: DescriptionId(42),
            weight: 8,
            worth: 42,
            discipline: 1,
            magic_tech_complexity: -7,
        }
    );
    let mut malformed = bytes.clone();
    malformed[90..94].copy_from_slice(&8u32.to_le_bytes());
    assert_eq!(
        decode_food(&malformed).unwrap_err().kind,
        ErrorKind::InvalidArrayWidth {
            expected: 4,
            actual: 8
        }
    );
    let mut malformed = bytes.clone();
    malformed[94..98].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(
        decode_food(&malformed).unwrap_err().kind,
        ErrorKind::ArrayTooLarge
    );
    bytes.push(0);
    assert_eq!(
        decode_food(&bytes).unwrap_err().kind,
        ErrorKind::TrailingData
    );
    assert_eq!(
        decode_food(&vec![0; MAX_PROTOTYPE_BYTES + 1])
            .unwrap_err()
            .kind,
        ErrorKind::TooLarge
    );
}

#[test]
fn supported_non_food_items_decode_only_the_verified_description_prefix() {
    for object_type in [WEAPON_TYPE, AMMO_TYPE, ARMOR_TYPE, KEY_TYPE, GENERIC_TYPE] {
        let mut bytes = food_bytes(9019, 42, 8);
        bytes[52..56].copy_from_slice(&object_type.to_le_bytes());
        bytes.truncate(143);
        let header = decode_header(&bytes).unwrap();
        assert_eq!(
            decode_item_description(&bytes, header),
            Ok(Some(DescriptionId(42)))
        );
        bytes.pop();
        assert_eq!(
            decode_item_description(&bytes, header),
            Err(at(139, ErrorKind::Truncated))
        );
    }
}

pub(crate) fn weapon_bytes(id: u32, description: i32) -> Vec<u8> {
    let mut bytes = food_bytes(id, description, 12);
    bytes.truncate(266); // Common object and item slots.
    bytes[52..56].copy_from_slice(&WEAPON_TYPE.to_le_bytes());
    bytes.extend([0; 16]); // First four weapon scalars.
    for (min_or_max, fatigue) in [(2i32, 1i32), (6, 4)] {
        bytes.push(1);
        for field in [4, 2, 99, min_or_max, fatigue, 2, 0b10001, 0] {
            bytes.extend(field.to_le_bytes());
        }
    }
    bytes.push(0); // Absent magic damage array.
    for field in [5i32, 20, 15, 40, 7, 11, 1, 1] {
        bytes.extend(field.to_le_bytes());
    }
    bytes.extend([0; 42]); // Tail scalars and two absent arrays.
    bytes
}

fn ammo_bytes() -> Vec<u8> {
    let mut bytes = food_bytes(7059, 0, 1);
    bytes.truncate(266);
    bytes[52..56].copy_from_slice(&AMMO_TYPE.to_le_bytes());
    for field in [0i32, 10, 1, 0, 0] {
        bytes.extend(field.to_le_bytes());
    }
    bytes.extend([0; 2]);
    bytes
}

#[test]
fn decodes_weapon_damage_by_sparse_index_and_follows_optional_fields() {
    let mut bytes = weapon_bytes(6136, 99);
    bytes[76] = 1; // Optional common location.
    bytes.splice(77..77, [0; 8]);
    let (header, decoded) = decode_weapon_ammo(&bytes).unwrap();
    assert_eq!(header.id, SerializedPrototypeId(6136));
    let WeaponAmmoFields::Weapon(weapon) = decoded else {
        panic!("expected Weapon");
    };
    assert_eq!(weapon.item.description, DescriptionId(99));
    assert_eq!(weapon.item.weight, 12);
    assert_eq!(weapon.damage_lower, [Some(2), None, None, None, Some(1)]);
    assert_eq!(weapon.damage_upper, [Some(6), None, None, None, Some(4)]);
    assert_eq!(
        (
            weapon.speed_factor,
            weapon.range,
            weapon.ammo_type,
            weapon.ammo_consumption
        ),
        (5, 15, 1, 1)
    );
}

#[test]
fn decodes_ammo_kind_and_prototype_quantity_separately() {
    let (header, decoded) = decode_weapon_ammo(&ammo_bytes()).unwrap();
    assert_eq!(header.object_type, AMMO_TYPE);
    let WeaponAmmoFields::Ammo(ammo) = decoded else {
        panic!("expected Ammo");
    };
    assert_eq!(ammo.ammo_type, 1);
    assert_eq!(ammo.quantity, 10);
    assert_eq!(
        decode_weapon_ammo(&food_bytes(1, 2, 3)),
        Err(at(52, ErrorKind::UnsupportedWeaponAmmoType(9)))
    );
}

#[test]
fn validates_weapon_masks_and_weapon_ammo_record_boundaries() {
    let original = weapon_bytes(6136, 99);
    for (offset, value, expected) in [
        (
            283,
            8u32,
            at(
                283,
                ErrorKind::InvalidArrayWidth {
                    expected: 4,
                    actual: 8,
                },
            ),
        ),
        (287, u32::MAX, at(295, ErrorKind::ArrayTooLarge)),
        (303, u32::MAX, at(307, ErrorKind::ArrayTooLarge)),
        (307, 1, at(307, ErrorKind::DamageMaskCountMismatch)),
        (307, 0b10011, at(307, ErrorKind::DamageMaskCountMismatch)),
        (307, 0b100001, at(307, ErrorKind::DamageIndexOutOfRange)),
        (311, 1, at(311, ErrorKind::DamageIndexOutOfRange)),
    ] {
        let mut bytes = original.clone();
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        assert_eq!(decode_weapon_ammo(&bytes), Err(expected));
    }
    for mut bytes in [original, ammo_bytes()] {
        for len in 0..bytes.len() {
            assert_eq!(
                decode_weapon_ammo(&bytes[..len]).unwrap_err().kind,
                ErrorKind::Truncated,
                "length {len}"
            );
        }
        let len = bytes.len();
        bytes.push(0);
        assert_eq!(
            decode_weapon_ammo(&bytes),
            Err(at(len, ErrorKind::TrailingData))
        );
    }
}
