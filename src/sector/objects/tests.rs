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
    assert!(wrap(&instance(1, [4, 0, 0], &[0; 4])).contains("unsupported object fields"));
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
