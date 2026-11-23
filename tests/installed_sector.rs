use arcanum_rs::{
    compositor::{SpriteDraw, blit_ordered},
    content::sector::{DEMO_MAP, DEMO_ORIGIN, DEMO_SECTOR, load_patch},
    sector::TileArtId,
    vfs::{Policy, Vfs},
};
use std::path::PathBuf;

#[test]
#[ignore = "requires installed Arcanum data"]
fn bates_floor_matches_original_sector_ids_and_raster() {
    let game = PathBuf::from(std::env::var_os("ARCANUM_GAME_DIR").expect("set ARCANUM_GAME_DIR"));
    let mut vfs = Vfs::from_game(&game, Policy::Ce).unwrap();
    let patch = load_patch(&mut vfs, DEMO_MAP, DEMO_SECTOR, DEMO_ORIGIN).unwrap();
    assert_eq!(
        patch.source.mount_path,
        game.join("modules/Arcanum.dat").canonicalize().unwrap()
    );
    assert_eq!(
        patch.names_source.mount_path,
        game.join("Arcanum5.dat").canonicalize().unwrap()
    );
    assert_eq!((patch.light_count, patch.tile_offset), (9, 436));
    assert_eq!(patch.center, (95, 98));
    assert_eq!(patch.artwork().count(), 10);
    let silhouette = arcanum_rs::isometric::synthetic_floor();
    for frame in patch.artwork() {
        assert!(
            frame
                .decoded
                .frame
                .pixels
                .iter()
                .zip(&silhouette.pixels)
                .all(|(actual, expected)| actual[3] == expected[3]),
            "{}",
            frame.source.virtual_path
        );
    }
    assert_eq!(patch.tiles().len(), 25);
    for (index, coord, id, path) in [
        (0, (93, 96), 0x01040e00, "art/tile/wd1bse0h.art"),
        (4, (97, 96), 0x01040c00, "art/tile/wd1bse0g.art"),
        (5, (93, 97), 0x01040600, "art/tile/wd1bse0d.art"),
        (12, (95, 98), 0x004104c0, "art/tile/wodbse0c.art"),
        (24, (97, 100), 0x004106c0, "art/tile/wodbse0d.art"),
    ] {
        let tile = &patch.tiles()[index];
        assert_eq!(
            (tile.world, tile.id, tile.artwork.as_str()),
            (coord, TileArtId(id), path)
        );
    }
    assert_eq!(patch.draws().nth(12).unwrap().anchor, (0, 19));

    let background = [32, 43, 58];
    let mut full = vec![background; 400 * 200];
    let draws = |center: (i32, i32)| {
        patch.draws().map(move |draw| SpriteDraw {
            frame: draw.frame,
            anchor: (draw.anchor.0 + center.0, draw.anchor.1 + center.1),
        })
    };
    blit_ordered(&mut full, (400, 200), 400, draws((200, 100))).unwrap();
    assert_eq!(
        full.iter().filter(|&&pixel| pixel != background).count(),
        25 * 1600
    );

    let mut clipped = vec![background; 80 * 40];
    blit_ordered(&mut clipped, (80, 40), 80, draws((40, 20))).unwrap();
    for (row, pixels) in clipped.chunks(80).enumerate() {
        let start = (row + 80) * 400 + 160;
        assert_eq!(pixels, &full[start..start + 80]);
    }
}

#[test]
#[ignore = "requires installed Arcanum data"]
fn bates_audit_verifies_original_transitions_and_keeps_facade_ids() {
    use arcanum_rs::content::{
        load_mes,
        sector::{ArtworkCheck, WIDE_ORIGIN, WIDE_SIDE, audit_floor, load_region},
    };
    use arcanum_rs::sector::ArtError;

    let game = PathBuf::from(std::env::var_os("ARCANUM_GAME_DIR").expect("set ARCANUM_GAME_DIR"));
    let mut vfs = Vfs::from_game(&game, Policy::Ce).unwrap();
    let audit = audit_floor(&mut vfs, DEMO_MAP, DEMO_SECTOR).unwrap();
    assert_eq!(audit.entries.len(), 62);
    assert_eq!(
        audit
            .entries
            .iter()
            .map(|entry| entry.occurrences)
            .sum::<usize>(),
        4096
    );
    assert_eq!(audit.artwork.len(), 23);
    assert!(
        audit
            .artwork
            .values()
            .all(|status| matches!(status, ArtworkCheck::Ready(_)))
    );
    assert_eq!(
        audit
            .entries
            .iter()
            .filter(|entry| entry.resolution == Err(ArtError::NotTile))
            .map(|entry| entry.occurrences)
            .sum::<usize>(),
        105
    );
    for (raw, count, path, flip) in [
        (0x001c31c0, 180, "art/tile/drtblk4a.art", false),
        (0x001c31c1, 225, "art/tile/drtblk4a.art", true),
        (0x001c61c1, 164, "art/tile/drtblk2a.art", true),
    ] {
        let entry = audit
            .entries
            .iter()
            .find(|entry| entry.id == TileArtId(raw))
            .unwrap();
        assert_eq!(entry.occurrences, count);
        let resolved = entry.resolution.as_ref().unwrap();
        assert_eq!(resolved.path, path);
        assert_eq!(resolved.fields.flip_x, flip);
        assert!(resolved.transition);
    }
    assert!(
        audit
            .entries
            .iter()
            .any(|entry| entry.id == TileArtId(0xb5be0801)
                && entry.occurrences == 1
                && entry.resolution == Err(ArtError::NotTile))
    );

    let patch = load_region(&mut vfs, DEMO_MAP, DEMO_SECTOR, WIDE_ORIGIN, WIDE_SIDE).unwrap();
    assert_eq!(patch.tiles().len(), 81);
    assert_eq!(patch.artwork().count(), 13);
    assert_eq!(patch.flipped_frame_count(), 1);
    assert_eq!(patch.center, (88, 81));
    assert_eq!(patch.tiles()[0].world, (84, 77));
    assert_eq!(patch.tiles()[80].world, (92, 85));
    assert_eq!(patch.tiles().iter().filter(|tile| tile.flip_x).count(), 9);
    assert!(
        patch.tiles()[..9]
            .iter()
            .all(|tile| tile.id == TileArtId(0x001c31c1))
    );

    let names = load_mes(&mut vfs, "art/tile/tilename.mes").unwrap();
    let grass = TileArtId(0x000131c1).resolve(&names.mes).unwrap();
    let frame = arcanum_rs::content::art::load_frame(&mut vfs, &grass.path, 0).unwrap();
    assert_eq!(grass.path, "art/tile/drtgrs4a.art");
    assert!(
        frame
            .decoded
            .frame
            .pixels
            .chunks(78)
            .any(|row| row.iter().ne(row.iter().rev()))
    );
}

#[test]
#[ignore = "requires installed Arcanum data"]
fn bates_facade_replays_baked_rug_frames_in_sector_order() {
    use arcanum_rs::content::sector::{FACADE_ORIGIN, WIDE_SIDE, load_facade_region};

    let game = PathBuf::from(std::env::var_os("ARCANUM_GAME_DIR").expect("set ARCANUM_GAME_DIR"));
    let mut vfs = Vfs::from_game(&game, Policy::Ce).unwrap();
    let patch =
        load_facade_region(&mut vfs, DEMO_MAP, DEMO_SECTOR, FACADE_ORIGIN, WIDE_SIDE).unwrap();
    assert_eq!(patch.center, (103, 82));
    assert_eq!(patch.tiles().len(), 81);
    let archive = game.join("Arcanum2.dat").canonicalize().unwrap();
    assert_eq!(
        patch.facade_names_source.as_ref().unwrap().mount_path,
        archive
    );
    let rug_path = "art/facade/rug3-flip.art";
    let rug_frames: Vec<_> = patch
        .artwork()
        .filter(|frame| frame.source.virtual_path == rug_path)
        .collect();
    assert_eq!(rug_frames.len(), 15);
    let silhouette = arcanum_rs::isometric::synthetic_floor();
    for frame in &rug_frames {
        assert_eq!(frame.source.mount_path, archive);
        assert_eq!(frame.input_bytes, 28_135);
        assert_eq!(frame.decoded.frame_count, 15);
        assert_eq!(frame.decoded.ignored_trailing_bytes, 0);
        assert_eq!(frame.decoded.frame.hotspot, (39, 39));
        assert!(
            frame
                .decoded
                .frame
                .pixels
                .iter()
                .zip(&silhouette.pixels)
                .all(|(actual, expected)| actual[3] == expected[3])
        );
    }
    // Read independently from the sector bytes and matching FacWalk piece positions.
    let mut pieces = Vec::new();
    for (tile, draw) in patch.tiles().iter().zip(patch.draws()) {
        if let Some(id) = tile.facade {
            pieces.push(tile.frame_index());
            assert_eq!(id.raw(), tile.id.0);
            assert_eq!(tile.artwork, rug_path);
            assert!(!tile.flip_x);
            assert!(id.fields().walkable && id.fields().flippable);
            assert_eq!(id.fields().number, 223);
            assert_eq!(id.fields().terrain, 1);
            assert!(std::ptr::eq(
                draw.frame,
                &rug_frames[tile.frame_index()].decoded.frame
            ));
        }
    }
    assert_eq!(pieces, [0, 1, 2, 3, 4, 9, 8, 7, 6, 5, 10, 11, 12, 13, 14]);
    for (index, raw, world, anchor, rgb) in [
        (29, 0xb5be0801, (101, 81), (40, -41), [66, 49, 41, 255]),
        (38, 0xb5be0813, (101, 82), (80, -21), [82, 57, 33, 255]),
        (51, 0xb5be081d, (105, 83), (-40, 79), [90, 82, 49, 255]),
    ] {
        assert_eq!(
            (patch.tiles()[index].id.0, patch.tiles()[index].world),
            (raw, world)
        );
        let draw = patch.draws().nth(index).unwrap();
        assert_eq!(draw.anchor, anchor);
        assert_eq!(draw.frame.pixels[20 * 78 + 39], rgb);
    }

    let background = [32, 43, 58];
    let mut full = vec![background; 720 * 400];
    let draws = |center: (i32, i32)| {
        patch.draws().map(move |draw| SpriteDraw {
            frame: draw.frame,
            anchor: (draw.anchor.0 + center.0, draw.anchor.1 + center.1),
        })
    };
    blit_ordered(&mut full, (720, 400), 720, draws((360, 200))).unwrap();
    assert_eq!(
        full.iter().filter(|&&pixel| pixel != background).count(),
        81 * 1600
    );
    let mut clipped = vec![background; 80 * 40];
    blit_ordered(&mut clipped, (80, 40), 80, draws((40, 20))).unwrap();
    for (row, pixels) in clipped.chunks(80).enumerate() {
        let start = (row + 180) * 720 + 320;
        assert_eq!(pixels, &full[start..start + 80]);
    }

    let rug4 = load_facade_region(&mut vfs, DEMO_MAP, DEMO_SECTOR, (22, 29), 5).unwrap();
    assert_eq!(rug4.tiles()[0].id.0, 0xb5c20801);
    assert_eq!(rug4.tiles()[0].artwork, "art/facade/rug4-flip.art");
    assert_eq!(
        rug4.tiles()
            .iter()
            .filter(|tile| tile.facade.is_some())
            .count(),
        15
    );
    assert_eq!(
        rug4.draws().next().unwrap().frame.pixels[20 * 78 + 39],
        [41, 24, 33, 255]
    );
}

#[test]
#[ignore = "requires installed Arcanum data"]
fn bates_facades_match_independent_unlit_compositions() {
    use arcanum_rs::content::sector::{FACADE_ORIGIN, WIDE_SIDE, load_facade_region};

    let game = PathBuf::from(std::env::var_os("ARCANUM_GAME_DIR").expect("set ARCANUM_GAME_DIR"));
    let mut vfs = Vfs::from_game(&game, Policy::Ce).unwrap();
    for (number, walkmask, origin, rug_world, expected_rgb) in [
        (223, "df", FACADE_ORIGIN, (101, 81), 0x6e6e_ed19_8e62_2b3b),
        (225, "e1", (35, 35), (101, 102), 0x1786_1112_b697_6e51),
    ] {
        let path = format!("art/facade/facwalk.{walkmask}");
        assert_eq!(vfs.resolve(&path).unwrap().winner().unwrap().size, 218);
        let mut bytes = Vec::new();
        vfs.copy(&path, &mut bytes).unwrap();
        assert_eq!(bytes.len(), 218);
        assert_eq!(&bytes[..14], b"FacWalk V101  ");
        let patch = load_facade_region(&mut vfs, DEMO_MAP, DEMO_SECTOR, origin, WIDE_SIDE).unwrap();
        assert_eq!(patch.tiles().len(), patch.draws().count());
        for (index, record) in bytes[38..].as_chunks::<12>().0.iter().enumerate() {
            let word = |offset| u32::from_le_bytes(record[offset..offset + 4].try_into().unwrap());
            let world = (rug_world.0 + word(0), rug_world.1 + word(4));
            let (tile, draw) = patch
                .tiles()
                .iter()
                .zip(patch.draws())
                .find(|(tile, _)| tile.world == world)
                .unwrap();
            let facade = tile.facade.unwrap();
            assert_eq!(tile.frame_index(), index);
            assert_eq!(facade.fields().number, number);
            assert_eq!(facade.fields().walkable, word(8) == 1);
            assert!(!tile.flip_x);
            assert_eq!((draw.frame.width, draw.frame.height), (78, 40));
            assert_eq!(draw.frame.hotspot, (39, 39));
            assert_eq!(draw.frame.offset, (0, 0));
        }

        let mut rgb = vec![[32, 43, 58]; 800 * 600];
        blit_ordered(
            &mut rgb,
            (800, 600),
            800,
            patch.draws().map(|draw| SpriteDraw {
                frame: draw.frame,
                anchor: (400 + draw.anchor.0, 300 + draw.anchor.1),
            }),
        )
        .unwrap();
        // Independently decoded from the indexed ART by a Python byte reader.
        // The reference places facade frames by their walkmask record coordinates
        // and copies each occupied pixel at the neutral CE tile-pass origin.
        let hash = rgb
            .as_flattened()
            .iter()
            .fold(0xcbf2_9ce4_8422_2325_u64, |hash, &byte| {
                (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
            });
        assert_eq!(hash, expected_rgb, "facade {number}");
    }
}
