use super::*;

fn fixture(lights: u8) -> Vec<u8> {
    let mut bytes = i32::from(lights).to_le_bytes().to_vec();
    bytes.extend(std::iter::repeat_n(0xff, usize::from(lights) * LIGHT_BYTES));
    for index in 0..4096u32 {
        bytes.extend((0x1040000 + index).to_le_bytes());
    }
    bytes.extend([0x11; 12]);
    bytes
}

#[test]
fn parses_light_framing_then_row_major_ids() {
    for lights in [0, 1, 2, 3] {
        let floor = decode_floor(&fixture(lights)).unwrap();
        assert_eq!(floor.light_count, u32::from(lights));
        assert_eq!(floor.tile_offset, 4 + usize::from(lights) * 48);
        assert_eq!(floor.tile(0, 0), Some(TileArtId(0x1040000)));
        assert_eq!(floor.tile(63, 0), Some(TileArtId(0x104003f)));
        assert_eq!(floor.tile(0, 1), Some(TileArtId(0x1040040)));
        assert_eq!(floor.tile(63, 63), Some(TileArtId(0x1040fff)));
        assert_eq!(floor.tile(64, 0), None);
        assert_eq!(floor.tile(0, 64), None);
    }
}

#[test]
fn checks_sector_input_bounds() {
    assert!(matches!(
        decode_floor(&[0; 3]),
        Err(Error::MissingLightCount)
    ));
    assert!(matches!(
        decode_floor(&[255; 4]),
        Err(Error::InvalidLightCount)
    ));
    let bytes = fixture(2);
    assert!(matches!(
        decode_floor(&bytes[..4]),
        Err(Error::TruncatedLights { expected_end: 52 })
    ));
    assert!(matches!(
        decode_floor(&bytes[..51]),
        Err(Error::TruncatedLights { expected_end: 52 })
    ));
    assert!(matches!(
        decode_floor(&bytes[..52]),
        Err(Error::TruncatedLights { expected_end: 100 })
    ));
    let impossible = i32::MAX.to_le_bytes();
    assert!(matches!(
        decode_floor(&impossible),
        Err(Error::TruncatedLights { expected_end: 52 })
    ));
    assert!(matches!(
        decode_floor(&bytes[..99]),
        Err(Error::TruncatedLights { expected_end: 100 })
    ));
    assert!(matches!(
        decode_floor(&bytes[..100 + TILE_BYTES - 1]),
        Err(Error::TruncatedTiles { offset: 100 })
    ));
    assert!(matches!(
        decode_floor(&vec![0; MAX_INPUT_BYTES + 1]),
        Err(Error::InputTooLarge)
    ));
}

#[test]
fn converts_ids_and_world_tiles_without_wrapping() {
    let coords = Coordinates::from_id(67_108_865).unwrap();
    assert_eq!(coords, Coordinates { x: 1, y: 1 });
    assert_eq!(coords.id(), Some(67_108_865));
    assert_eq!(coords.world_tile(29, 32), Some((93, 96)));
    assert_eq!(coords.world_tile(29, 64), None);
    assert_eq!(Coordinates::from_id(1 << 52), None);
    assert_eq!(Coordinates { x: 1 << 26, y: 0 }.id(), None);
    assert_eq!(
        Coordinates::from_id((1 << 52) - 1)
            .unwrap()
            .world_tile(63, 63),
        Some((u32::MAX, u32::MAX))
    );
}

#[test]
fn resolves_base_art_ids_by_terrain_bucket_and_variant() {
    let names = Mes::parse_text("{0}{drt/n 0}{1}{grs/n 0}{200}{fl1 3}{201}{wod 5}{300}{dn1 3}{301}{wi1 3}{303}{dn2 3}{305}{cf1/n 0}{308}{wd1 5}").unwrap();
    for (id, path) in [
        (0x004104c0, "wodbse0c"),
        (0x01040e00, "wd1bse0h"),
        (0x00c3f000, "cf1bse0a"),
    ] {
        assert_eq!(
            TileArtId(id).base_path(&names).unwrap(),
            format!("art/tile/{path}.art")
        );
    }
    assert_eq!(TileArtId(0x004104c1).base_path(&names), Err(ArtError::Flip));
    assert_eq!(
        TileArtId(0x104104c0).base_path(&names),
        Err(ArtError::NotTile)
    );
    assert_eq!(
        TileArtId(0x000131c0).base_path(&names),
        Err(ArtError::Transition)
    );
    assert_eq!(
        TileArtId(0x004104d0).base_path(&names),
        Err(ArtError::FlagsOrPalette)
    );
    let bad = Mes::parse_text("{200}{../a}").unwrap();
    assert_eq!(
        TileArtId(0xc0).base_path(&bad),
        Err(ArtError::InvalidTerrainName)
    );
}

#[test]
fn rejects_a_cut_inside_the_light_list_even_with_plausible_tile_words() {
    let mut input = fixture(2);
    // Remove the opaque suffix so there are precisely 4,096 tile words.
    input.truncate(4 + 2 * LIGHT_BYTES + TILE_BYTES);
    // The count still declares two lights. A tile byte cannot compensate for
    // the missing byte, because the declared tile span must also be complete.
    input.remove(4 + LIGHT_BYTES + 20);
    assert!(matches!(
        decode_floor(&input),
        Err(Error::TruncatedTiles { offset: 100 })
    ));
    // A stream ending partway through the second light fails while reading it.
    assert!(matches!(
        decode_floor(&input[..4 + LIGHT_BYTES + 20]),
        Err(Error::TruncatedLights { expected_end: 100 })
    ));
}
