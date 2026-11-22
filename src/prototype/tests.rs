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
