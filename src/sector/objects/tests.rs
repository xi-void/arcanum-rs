use super::*;

fn sector_header(marker: u32) -> Vec<u8> {
    let mut bytes = vec![0; 4 + SIDE * SIDE * 4];
    bytes.extend(1u32.to_le_bytes()); // Empty roof list.
    bytes.extend(marker.to_le_bytes());
    if marker >= 0xaa0001 {
        bytes.extend(1u32.to_le_bytes());
        bytes.extend([0; 24]);
    }
    if marker >= 0xaa0002 {
        bytes.extend([0; 12]);
    }
    if marker >= 0xaa0003 {
        bytes.extend([0; 24]);
    }
    if marker >= 0xaa0004 {
        bytes.extend([0; 512]);
    }
    bytes
}

fn instance(kind: u32, bitmap: [u32; 3], data: &[u8]) -> Vec<u8> {
    let mut record = vec![0; 52];
    record[0..4].copy_from_slice(&119u32.to_le_bytes());
    record[4] = 1;
    record[12..16].copy_from_slice(&1021u32.to_le_bytes());
    record[28] = 2;
    record.extend(kind.to_le_bytes());
    record.extend((bitmap.iter().map(|bits| bits.count_ones()).sum::<u32>() as u16).to_le_bytes());
    for bits in bitmap {
        record.extend(bits.to_le_bytes());
    }
    record.extend(data);
    record
}

fn wall() -> Vec<u8> {
    let mut data = 0x1030_0900_u32.to_le_bytes().to_vec();
    data.push(1);
    data.extend(0x1234_1244_u32.to_le_bytes());
    data.extend(0x5678_5689_u32.to_le_bytes());
    data.extend((-7i32).to_le_bytes());
    data.extend(0i32.to_le_bytes());
    instance(0, [15, 0, 0], &data)
}

#[test]
fn frames_objects_after_variable_sector_sections() {
    let portal = instance(1, [0, 0, 0], &[]);
    let mut array = vec![1];
    for word in [12u32, 1, 0] {
        array.extend(word.to_le_bytes());
    }
    array.extend([0; 12]);
    array.extend(1u32.to_le_bytes());
    array.extend(2u32.to_le_bytes());
    let scenery = instance(3, [1 << 31, 0, 0], &array);
    for marker in 0xaa0000..=0xaa0004 {
        let mut bytes = sector_header(marker);
        let start = bytes.len();
        bytes.extend(&portal);
        bytes.extend(&scenery);
        bytes.extend(wall());
        bytes.extend(3u32.to_le_bytes());
        let list = decode(&bytes).unwrap();
        assert_eq!(
            (
                list.start,
                list.count,
                list.portal_count,
                list.scenery_count
            ),
            (start, 3, 1, 1)
        );
        let record = &list.walls[0];
        assert_eq!(record.index, 2);
        assert_eq!(
            record.span,
            start + portal.len() + scenery.len()..bytes.len() - 4
        );
        assert_eq!(record.art_id, 0x1030_0900);
        assert_eq!(record.instance_id[0], 2);
        assert_eq!(record.prototype, SerializedPrototypeId(1021));
        assert_eq!(record.fields.offset_x, Some(-7));
        assert_eq!(record.fields.offset_y, Some(0));
        assert_eq!(record.fields.scale, None);
        assert_eq!(
            record.world_tile(Coordinates { x: 2, y: 3 }),
            Some((132, 201))
        );
    }
}

#[test]
fn rejects_truncated_records_without_consuming_the_count_as_field_data() {
    let header = sector_header(0xaa0004);
    let wall = wall();
    for length in 0..wall.len() {
        let mut bytes = header.clone();
        bytes.extend(&wall[..length]);
        bytes.extend(1u32.to_le_bytes());
        assert!(
            decode(&bytes).is_err(),
            "accepted record prefix of length {length}"
        );
    }
    let mut roof = vec![0; 4 + SIDE * SIDE * 4];
    roof.extend(0u32.to_le_bytes());
    roof.extend([0; 1023]);
    roof.extend(0u32.to_le_bytes());
    assert!(
        decode(&roof)
            .unwrap_err()
            .message
            .contains("truncated roof IDs")
    );
}

#[test]
fn rejects_unsupported_types_and_fields_and_invalid_record_boundaries() {
    let wrap = |record: &[u8]| {
        let mut bytes = sector_header(0xaa0000);
        bytes.extend(record);
        bytes.extend(1u32.to_le_bytes());
        decode(&bytes).unwrap_err().message
    };
    assert!(wrap(&instance(16, [0; 3], &[])).contains("unsupported object category"));
    assert!(wrap(&instance(1, [0, 1 << 5, 0], &[0; 4])).contains("unsupported object fields"));
    let mut bad = wall();
    bad[56] = 1;
    assert!(wrap(&bad).contains("field count and bitmap disagree"));
    let mut bad = wall();
    bad[74] = 9;
    assert!(wrap(&bad).contains("invalid optional field marker"));
    let mut bad = wall();
    bad.push(0);
    assert!(wrap(&bad).contains("do not end at the final count"));
    let mut bad = wall();
    bad[0] = 118;
    assert!(wrap(&bad).contains("unsupported object version"));
}

#[test]
fn trap_fields_preserve_the_following_static_record() {
    let mut data = vec![1]; // Present location.
    for value in [2056u32, 1991, 0, 0, 0, 20, 0] {
        data.extend(value.to_le_bytes());
    }
    data.extend([0, 0]); // Absent 32-bit and 64-bit pad arrays.
    let trap = instance(TRAP, [14, 0, 0x1f], &data);
    let mut bytes = sector_header(0xaa0004);
    bytes.extend(&trap);
    let wall_start = bytes.len();
    bytes.extend(wall());
    bytes.extend(2u32.to_le_bytes());
    let list = decode(&bytes).unwrap();
    assert_eq!(list.count, 2);
    assert_eq!(list.traps[0].stored_tile, Some((2056, 1991)));
    assert_eq!(list.traps[0].index, 0);
    assert_eq!(list.walls[0].index, 1);
    assert_eq!(list.walls[0].span.start, wall_start);

    // A future subtype field must not be interpreted as a known pad or skipped.
    let mut unknown = sector_header(0xaa0004);
    unknown.extend(instance(TRAP, [0, 0, 0x20], &[0; 4]));
    unknown.extend(1u32.to_le_bytes());
    assert!(
        decode(&unknown)
            .unwrap_err()
            .message
            .contains("unsupported object fields")
    );
}

#[test]
fn retains_scenery_placements_and_rejects_a_truncated_override() {
    let art = 0x4120_0380_u32;
    let mut data = art.to_le_bytes().to_vec();
    data.push(1);
    for field in [103i32, 80, -9, 0, 100, 0x30, 0x20] {
        data.extend(field.to_le_bytes());
    }
    let record = instance(3, [0x40_80f, 0, 1], &data);
    let sector = |record: &[u8]| {
        let mut bytes = sector_header(0xaa0000);
        bytes.extend(record);
        bytes.extend(1u32.to_le_bytes());
        bytes
    };
    let list = decode(&sector(&record)).unwrap();
    let scene = &list.scenery[0];
    assert_eq!(scene.category, StaticCategory::Scenery);
    assert_eq!(scene.art_id, art);
    assert_eq!(scene.stored_tile, (103, 80));
    assert_eq!(scene.fields.offset_x, Some(-9));
    assert_eq!(scene.fields.offset_y, Some(0));
    assert_eq!(scene.fields.scale, Some(100));
    assert_eq!(scene.fields.flags, Some(0x30));
    assert_eq!(scene.fields.scenery_flags, Some(0x20));
    assert!(decode(&sector(&record[..record.len() - 1])).is_err());
    // Material is a CE INT32 field. Reserved common bits still fail.
    assert!(decode(&sector(&instance(3, [1 << 28, 0, 0], &[0; 4]))).is_ok());
    assert!(decode(&sector(&instance(3, [0, 1 << 4, 0], &[0; 4]))).is_err());
}

#[test]
fn distinguishes_inheritable_scenery_art_from_missing_placement() {
    let mut placed = vec![1];
    placed.extend(103u32.to_le_bytes());
    placed.extend(80u32.to_le_bytes());
    let mut bytes = sector_header(0xaa0000);
    bytes.extend(instance(3, [2, 0, 0], &placed));
    bytes.extend(instance(3, [1, 0, 0], &0x4120_0380u32.to_le_bytes()));
    bytes.extend(2u32.to_le_bytes());

    let list = decode(&bytes).unwrap();
    assert!(list.scenery.is_empty());
    let records = &list.incomplete_scenery;
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].prototype, SerializedPrototypeId(1021));
    assert_eq!(records[0].art_id, None);
    assert_eq!(records[0].stored_tile, Some((103, 80)));
    assert_eq!(records[1].art_id, Some(0x4120_0380));
    assert_eq!(records[1].stored_tile, None);
}

#[test]
fn portal_retains_current_art_and_optional_appearance_fields() {
    let mut data = 0x3210_3c11_u32.to_le_bytes().to_vec();
    data.push(1);
    data.extend(94u32.to_le_bytes());
    data.extend(90u32.to_le_bytes());
    data.extend((-6i32).to_le_bytes());
    data.extend(100u32.to_le_bytes());
    data.extend(0u32.to_le_bytes());
    let record = instance(1, [0x807, 0, 1], &data);
    let sector = |record: &[u8]| {
        let mut bytes = sector_header(0xaa0000);
        bytes.extend(record);
        bytes.extend(1u32.to_le_bytes());
        bytes
    };
    let list = decode(&sector(&record)).unwrap();
    let portal = &list.portals[0];
    assert_eq!(portal.category, StaticCategory::Portal);
    assert_eq!(portal.art_id, 0x3210_3c11);
    assert_eq!(portal.stored_tile, (94, 90));
    assert_eq!(portal.fields.offset_x, Some(-6));
    assert_eq!(portal.fields.offset_y, None);
    assert_eq!(portal.fields.scale, Some(100));
    assert_eq!(portal.fields.portal_flags, Some(0));

    for end in 0..record.len() {
        assert!(decode(&sector(&record[..end])).is_err(), "prefix {end}");
    }
}

#[test]
fn mobile_appearance_uses_permanent_identity_and_bounds_checked_fields() {
    let mut record = vec![0; 78];
    record[0..4].copy_from_slice(&119u32.to_le_bytes());
    record[4] = 1;
    record[12..16].copy_from_slice(&17088u32.to_le_bytes());
    record[28] = 2;
    record[52..56].copy_from_slice(&16u32.to_le_bytes());
    record[56..58].copy_from_slice(&4u16.to_le_bytes());
    record[58] = 0x07;
    record[66] = 1;
    record.extend(0x2880_2820u32.to_le_bytes());
    record.push(1);
    record.extend(80u32.to_le_bytes());
    record.extend(81u32.to_le_bytes());
    record.extend((-5i32).to_le_bytes());
    record.extend(1u32.to_le_bytes()); // OCF_IS_CONCEALED override.
    let imported = decode_mobile(&record).unwrap();
    assert_eq!(imported.tile, (80, 81));
    assert_eq!(imported.art, 0x2880_2820);
    assert_eq!(imported.fields.offset_x, Some(-5));
    assert_eq!(imported.fields.critter_flags, Some(1));
    for end in [4, 55, 77, 85, 98] {
        assert!(decode_mobile(&record[..end]).is_err(), "prefix {end}");
    }
    record[56] = 3;
    assert!(decode_mobile(&record).is_err());
}

#[test]
fn mobile_dexterity_uses_the_sparse_logical_index_and_rejects_truncation() {
    let mut record = vec![0; 78];
    record[0..4].copy_from_slice(&119u32.to_le_bytes());
    record[4] = 1;
    record[12..16].copy_from_slice(&28344u32.to_le_bytes());
    record[28] = 2;
    record[52..56].copy_from_slice(&16u32.to_le_bytes());
    record[56..58].copy_from_slice(&3u16.to_le_bytes());
    record[58] = 3;
    record[66] = 4;
    record.extend(0xc480_1810u32.to_le_bytes());
    record.push(1);
    record.extend(420u32.to_le_bytes());
    record.extend(814u32.to_le_bytes());
    record.push(1);
    for word in [4u32, 2, 0, 9, 17, 1, 3] {
        record.extend(word.to_le_bytes());
    }
    assert_eq!(decode_mobile(&record).unwrap().dexterity, Some(17));
    let bitmap = record.len() - 4;
    record[bitmap] = 5; // Indices 0 and 2. Index 1 inherits from the prototype.
    assert_eq!(decode_mobile(&record).unwrap().dexterity, None);
    assert!(decode_mobile(&record[..record.len() - 1]).is_err());
    record[bitmap] = 2; // One set bit cannot describe two serialized entries.
    assert!(decode_mobile(&record).is_err());
}

#[test]
fn portal_lock_difficulty_preserves_the_next_record_boundary() {
    let mut data = 0x3348_3c11u32.to_le_bytes().to_vec();
    data.push(1);
    for word in [395u32, 826, 0x11, 100] {
        data.extend(word.to_le_bytes());
    }
    let portal = instance(PORTAL, [3, 0, 3], &data);
    let mut bytes = sector_header(0xaa0004);
    let portal_start = bytes.len();
    bytes.extend_from_slice(&portal);
    let wall_start = bytes.len();
    bytes.extend(wall());
    bytes.extend(2u32.to_le_bytes());
    let list = decode(&bytes).unwrap();
    assert_eq!(list.portals[0].span, portal_start..wall_start);
    assert_eq!(list.portals[0].fields.portal_flags, Some(0x11));
    assert_eq!(list.walls[0].span.start, wall_start);
    assert!(list.walls[0].index > list.portals[0].index);

    let mut truncated = bytes[..wall_start - 1].to_vec();
    truncated.extend(1u32.to_le_bytes());
    assert!(decode(&truncated).is_err());

    let mut unknown = bytes;
    unknown[portal_start + 67] = 1; // A bit after the eight defined portal fields.
    assert!(decode(&unknown).is_err());
}

#[test]
fn roof_table_exposes_exactly_the_serialized_slots() {
    let mut bytes = vec![0; 4 + SIDE * SIDE * 4];
    bytes.extend(0u32.to_le_bytes());
    for slot in 0..256 {
        let id = if slot == 8 { 0xa071_c000 } else { u32::MAX };
        bytes.extend(id.to_le_bytes());
    }
    assert!(roof_words(&bytes, 4).is_err()); // Footer may not supply roof bytes.
    bytes.extend(0xaa0000u32.to_le_bytes());
    bytes.extend(0u32.to_le_bytes());
    let words = roof_words(&bytes, 4).unwrap();
    assert_eq!(words.len(), 256);
    assert_eq!(u32::from_le_bytes(words[8]), 0xa071_c000);
    assert!(decode(&bytes).unwrap().walls.is_empty());
}
