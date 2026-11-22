use super::*;

fn set_word(bytes: &mut [u8], offset: usize, value: i32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn fixture(frames: &[(i32, i32, &[u8])]) -> Vec<u8> {
    let mut bytes = vec![0; HEADER_BYTES + PALETTE_BYTES + FRAME_BYTES * frames.len()];
    for (offset, value) in [
        (0, 1),
        (4, 8),
        (8, 8),
        (12, 42),
        (28, 1),
        (32, frames.len() as i32),
    ] {
        set_word(&mut bytes, offset, value);
    }
    // A bright stored RGB key, one color with a nonzero unused byte, and opaque black.
    bytes[132..144].copy_from_slice(&[222, 33, 111, 0, 3, 2, 1, 47, 0, 0, 0, 0]);
    for (index, &(width, height, data)) in frames.iter().enumerate() {
        let record = HEADER_BYTES + PALETTE_BYTES + index * FRAME_BYTES;
        for (offset, value) in [
            (0, width),
            (4, height),
            (8, data.len() as i32),
            (12, -3),
            (16, 4),
            (20, 5),
            (24, -6),
        ] {
            set_word(&mut bytes, record + offset, value);
        }
        bytes.extend_from_slice(data);
    }
    bytes
}

#[test]
fn decodes_raw_indices_rows_and_signed_metadata() {
    let input = fixture(&[(2, 2, &[0, 1, 2, 0])]);
    let decoded = decode_frame(&input, 0).unwrap();
    assert_eq!(decoded.frame_count, 1);
    assert_eq!(decoded.frames_per_second, 8);
    assert_eq!(decoded.action_frame, 1);
    assert_eq!(
        decoded.frame,
        SpriteFrame {
            width: 2,
            height: 2,
            hotspot: (-3, 4),
            offset: (5, -6),
            pixels: vec![[0; 4], [1, 2, 3, 255], [0, 0, 0, 255], [0; 4]],
        }
    );
}

#[test]
fn selects_one_frame_and_decodes_repeat_and_literal_packets_across_rows() {
    let input = fixture(&[(1, 1, &[1]), (3, 3, &[4, 1, 0x82, 2, 0, 3, 2])]);
    let decoded = decode_frame(&input, 1).unwrap();
    assert_eq!(decoded.frame_count, 2);
    assert_eq!(
        decoded.frame.pixels,
        [
            [1, 2, 3, 255],
            [1, 2, 3, 255],
            [1, 2, 3, 255],
            [1, 2, 3, 255],
            [0, 0, 0, 255],
            [0; 4],
            [0, 0, 0, 255],
            [0, 0, 0, 255],
            [0, 0, 0, 255],
        ]
    );
    assert_eq!(
        decode_frame(&input, 0).unwrap().frame.pixels,
        [[1, 2, 3, 255]]
    );
    assert_eq!(
        decode_frame(&input, 2),
        Err(Error::FrameOutOfRange { index: 2, count: 2 })
    );
}

#[test]
fn validates_framing_and_rejects_unsupported_layouts() {
    let original = fixture(&[(1, 1, &[1]), (2, 2, &[1; 4])]);
    for (length, section) in [
        (131, "header"),
        (1155, "palette"),
        (1211, "frame table"),
        (original.len() - 1, "frame payload"),
    ] {
        assert!(
            matches!(decode_frame(&original[..length], 0), Err(Error::Truncated {section: actual, ..}) if actual == section)
        );
    }
    for (offset, value) in [(0, 0), (0, 3), (8, 16), (12, 0), (16, 1)] {
        let mut bytes = original.clone();
        set_word(&mut bytes, offset, value);
        assert!(matches!(
            decode_frame(&bytes, 0),
            Err(Error::Unsupported(_))
        ));
    }
}

#[test]
fn reports_extra_bytes_past_all_frame_payloads_without_decoding_them() {
    let exact = fixture(&[(1, 1, &[1]), (2, 2, &[4, 2])]);
    let mut padded = exact.clone();
    padded.extend([0, 0x83, 42]); // Invalid RLE if consumed.
    for index in 0..2 {
        let reference = decode_frame(&exact, index).unwrap();
        assert_eq!(reference.ignored_trailing_bytes, 0);
        let decoded = decode_frame(&padded, index).unwrap();
        assert_eq!(decoded.ignored_trailing_bytes, 3);
        assert_eq!(decoded.frame, reference.frame);
    }
}

#[test]
fn trailing_bytes_cannot_complete_a_declared_rle_packet() {
    let mut input = fixture(&[(3, 3, &[0x83, 1])]);
    input.extend([2, 0, 6, 2]); // Would complete the frame if the payload were enlarged.
    assert!(matches!(
        decode_frame(&input, 0),
        Err(Error::Truncated {
            section: "RLE literal indices",
            ..
        })
    ));
}

#[test]
fn trailing_bytes_count_toward_the_overall_input_limit() {
    let mut input = fixture(&[(1, 1, &[1])]);
    input.resize(MAX_INPUT_BYTES + 1, 0);
    assert_eq!(decode_frame(&input, 0), Err(Error::InputTooLarge));
}

#[test]
fn rejects_invalid_dimensions_lengths_and_oversized_frames() {
    let original = fixture(&[(1, 1, &[1])]);
    for (offset, value) in [
        (32, 0),
        (32, 1025),
        (1156, -1),
        (1160, 0),
        (1156, i32::MAX),
        (1164, -1),
        (1164, 0),
    ] {
        let mut bytes = original.clone();
        set_word(&mut bytes, offset, value);
        assert!(
            matches!(decode_frame(&bytes, 0), Err(Error::Invalid { offset: position, .. }) if position == offset || (offset == 1160 && position == 1156))
        );
    }
    let mut bytes = original;
    set_word(&mut bytes, 1164, i32::MAX);
    assert_eq!(
        decode_frame(&bytes, 0),
        Err(Error::Truncated {
            offset: 1184,
            section: "frame payload"
        })
    );
}

#[test]
fn rejects_truncated_underfilled_overfilled_and_empty_rle_runs() {
    for (payload, section) in [
        (&[2][..], "RLE repeat index"),
        (&[0x83, 1][..], "RLE literal indices"),
    ] {
        assert!(
            matches!(decode_frame(&fixture(&[(3, 3, payload)]), 0), Err(Error::Truncated { section: actual, .. }) if section == actual)
        );
    }
    for payload in [[8, 1], [10, 1], [0, 1]] {
        assert!(matches!(
            decode_frame(&fixture(&[(3, 3, &payload)]), 0),
            Err(Error::Invalid { .. })
        ));
    }
    let input = fixture(&[(3, 3, &[0x83, 1]), (1, 1, &[2])]);
    assert!(decode_frame(&input, 0).is_err());
    assert_eq!(
        decode_frame(&input, 1).unwrap().frame.pixels,
        [[0, 0, 0, 255]]
    );
}
