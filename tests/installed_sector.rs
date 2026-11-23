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

#[test]
#[ignore = "requires installed Arcanum data"]
fn bates_wall_uses_inherited_appearance_and_matches_the_independent_scene() {
    use arcanum_rs::{
        compositor::sprite_origin,
        content::{
            sector::{WIDE_SIDE, load_facade_region},
            wall,
        },
        prototype::SerializedPrototypeId,
    };

    let game = PathBuf::from(std::env::var_os("ARCANUM_GAME_DIR").expect("set ARCANUM_GAME_DIR"));
    let mut vfs = Vfs::from_game(&game, Policy::Ce).unwrap();
    let wall = wall::load(&mut vfs, DEMO_MAP, DEMO_SECTOR, wall::DEMO_RECORD).unwrap();
    assert_eq!(
        wall.sector_source.mount_path,
        game.join("modules/Arcanum.dat").canonicalize().unwrap()
    );
    assert_eq!(wall.object_list_offset, 18428);
    assert_eq!(wall.object_counts, (268, 19, 81));
    assert_eq!(wall.record.span, 23024..23107);
    assert_eq!(wall.record.prototype, SerializedPrototypeId(1021));
    assert_eq!(wall.record.art_id, 0x1030_0900);
    assert_eq!(wall.record.fields.scale, None);
    assert_eq!(wall.record.fields.offset_x, None);
    assert_eq!(wall.placement.tile, (98, 79));
    assert_eq!(wall.placement.pixel_offset, (0, 0));
    assert_eq!(wall.scale, 100);
    assert_eq!(wall.flags, 0x4000);
    assert_eq!(
        wall.prototype_source.virtual_path,
        "proto/001021 - wall.pro"
    );
    assert_eq!(wall.artwork.source.virtual_path, "art/wall/in4bseu1.art");
    assert_eq!(
        wall.artwork.source.mount_path,
        game.join("Arcanum2.dat").canonicalize().unwrap()
    );
    assert_eq!(wall.artwork.decoded.frame_count, 1);
    let frame = &wall.artwork.decoded.frame;
    assert_eq!(
        (frame.width, frame.height, frame.hotspot, frame.offset),
        (40, 154, (39, 132), (0, 0))
    );
    assert_eq!(
        frame.pixels.iter().filter(|pixel| pixel[3] != 0).count(),
        5368
    );

    let patch = load_facade_region(
        &mut vfs,
        DEMO_MAP,
        DEMO_SECTOR,
        wall::DEMO_ORIGIN,
        WIDE_SIDE,
    )
    .unwrap();
    assert_eq!(patch.center, (102, 82));
    assert_eq!(
        patch
            .tiles()
            .iter()
            .filter(|tile| tile.facade.is_some())
            .count(),
        15
    );
    let wall_draw = wall.draw(patch.center).unwrap();
    assert_eq!(wall_draw.anchor, (80, -160));
    assert_eq!(sprite_origin(frame, wall_draw.anchor), (41, -292));
    let draws = |center: (i32, i32)| {
        patch
            .draws()
            .chain([wall_draw])
            .map(move |draw| SpriteDraw {
                frame: draw.frame,
                anchor: (draw.anchor.0 + center.0, draw.anchor.1 + center.1),
            })
    };
    let mut full = vec![[32, 43, 58]; 800 * 600];
    blit_ordered(&mut full, (800, 600), 800, draws((400, 300))).unwrap();
    let hash = full
        .as_flattened()
        .iter()
        .fold(0xcbf2_9ce4_8422_2325_u64, |hash, &byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
        });
    // Python reference independently decoded source indices and applied CE's
    // 78x40 tile origin, rotation-one hotspot correction and object pass.
    assert_eq!(hash, 0x8c25_7375_1663_9a09);

    let mut cropped = vec![[32, 43, 58]; 400 * 400];
    blit_ordered(&mut cropped, (400, 400), 400, draws((200, 200))).unwrap();
    for (row, actual) in cropped.chunks(400).enumerate() {
        let start = (row + 100) * 800 + 200;
        assert_eq!(actual, &full[start..start + 400]);
    }
}

#[test]
#[ignore = "requires installed Arcanum data"]
fn bates_connected_wall_run_matches_original_order_palette_and_shared_coordinates() {
    use arcanum_rs::{
        compositor::sprite_origin,
        content::{
            sector::{FACADE_ORIGIN, WIDE_SIDE, load_facade_region},
            wall,
        },
    };

    let game = PathBuf::from(std::env::var_os("ARCANUM_GAME_DIR").expect("set ARCANUM_GAME_DIR"));
    let mut vfs = Vfs::from_game(&game, Policy::Ce).unwrap();
    let mut requested = wall::CONNECTED_RECORDS;
    requested.reverse();
    let walls = wall::load_many(&mut vfs, DEMO_MAP, DEMO_SECTOR, &requested).unwrap();
    assert_eq!(
        walls
            .iter()
            .map(|wall| wall.record.index)
            .collect::<Vec<_>>(),
        [
            30, 31, 32, 33, 34, 36, 37, 38, 39, 52, 60, 69, 77, 87, 93, 99, 107
        ]
    );
    assert!(std::sync::Arc::ptr_eq(&walls[0].artwork, &walls[2].artwork));
    assert!(!std::sync::Arc::ptr_eq(
        &walls[0].artwork,
        &walls[1].artwork
    ));
    assert_eq!(
        walls[0].artwork.source.virtual_path,
        walls[1].artwork.source.virtual_path
    );
    for (wall, tile, art, rotation, slot, pivot, path) in [
        (
            &walls[1],
            (98, 78),
            0x10300800,
            1,
            0,
            (-1, 152),
            "art/wall/in4bseu0.art",
        ),
        (
            &walls[2],
            (98, 78),
            0x10303811,
            7,
            1,
            (39, 152),
            "art/wall/in4bseu0.art",
        ),
        (
            &walls[4],
            (100, 78),
            0x10397911,
            7,
            1,
            (39, 152),
            "art/wall/in4p4lu1.art",
        ),
        (
            &walls[12],
            (98, 82),
            0x10378800,
            1,
            0,
            (-1, 152),
            "art/wall/in4d6bu0.art",
        ),
    ] {
        assert_eq!(wall.placement.tile, tile);
        assert_eq!(wall.record.art_id, art);
        assert_eq!(
            (wall.orientation.rotation, wall.orientation.palette),
            (rotation, slot)
        );
        assert_eq!(wall.orientation.pivot(&wall.artwork.decoded.frame), pivot);
        assert_eq!(wall.artwork.source.virtual_path, path);
        assert_eq!((wall.scale, wall.placement.pixel_offset), (100, (0, 0)));
    }
    let patch = load_facade_region(
        &mut vfs,
        DEMO_MAP,
        DEMO_SECTOR,
        wall::CONNECTED_ORIGIN,
        WIDE_SIDE,
    )
    .unwrap();
    let other =
        load_facade_region(&mut vfs, DEMO_MAP, DEMO_SECTOR, FACADE_ORIGIN, WIDE_SIDE).unwrap();
    assert_eq!(patch.center, (101, 81));
    let anchor_shift = |from: SpriteDraw<'_>, to: SpriteDraw<'_>| {
        assert_eq!(
            (to.anchor.0 - from.anchor.0, to.anchor.1 - from.anchor.1),
            (40, -60)
        );
    };
    for wall in &walls {
        anchor_shift(
            wall.draw(patch.center).unwrap(),
            wall.draw(other.center).unwrap(),
        );
    }
    let mut common = 0;
    for (tile, draw) in patch.tiles().iter().zip(patch.draws()) {
        if let Some((_, rebased)) = other
            .tiles()
            .iter()
            .zip(other.draws())
            .find(|(candidate, _)| candidate.world == tile.world)
        {
            anchor_shift(draw, rebased);
            common += 1;
        }
    }
    assert_eq!(common, 56);

    let last = walls.last().unwrap();
    assert_eq!(last.placement.tile, (98, 86));
    assert!(
        patch
            .tiles()
            .iter()
            .all(|tile| tile.world != last.placement.tile)
    );
    let last_draw = last.draw(patch.center).unwrap();
    assert_eq!(
        sprite_origin(last_draw.frame, last_draw.anchor),
        (321, -112)
    );
    let mut full = vec![[32, 43, 58]; 800 * 600];
    let draws = |(x, y): (i32, i32)| {
        patch
            .draws()
            .chain(walls.iter().map(|wall| wall.draw(patch.center).unwrap()))
            .map(move |mut draw| {
                draw.anchor.0 += x;
                draw.anchor.1 += y;
                draw
            })
    };
    // The independently decoded pixel in the actual same-column overlap.
    blit_ordered(&mut full, (800, 600), 800, draws((400, 300)).take(82)).unwrap();
    assert_eq!(full[28 * 800 + 401], [92, 88, 90]);
    blit_ordered(&mut full, (800, 600), 800, draws((400, 300)).skip(82)).unwrap();
    assert_eq!(full[28 * 800 + 401], [47, 48, 49]);
    let hash = full
        .as_flattened()
        .iter()
        .fold(0xcbf2_9ce4_8422_2325_u64, |hash, &byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
        });
    assert_eq!(hash, 0xc65c_248f_bb57_a534);

    let mut clipped = vec![[32, 43, 58]; 400 * 400];
    blit_ordered(&mut clipped, (400, 400), 400, draws((200, 200))).unwrap();
    for (row, actual) in clipped.chunks(400).enumerate() {
        let start = (row + 100) * 800 + 200;
        assert_eq!(actual, &full[start..start + 400]);
    }
}

#[test]
#[ignore = "requires installed Arcanum data"]
fn bates_room_imports_original_scenery_and_composes_shared_geometry() {
    use arcanum_rs::{
        compositor::{blit_ordered, sprite_origin},
        content::{scenery::load_room, sector::load_facade_region},
        sector::objects::StaticCategory,
    };

    let game = PathBuf::from(std::env::var_os("ARCANUM_GAME_DIR").expect("set ARCANUM_GAME_DIR"));
    let mut vfs = Vfs::from_game(&game, Policy::Ce).unwrap();
    let mut room = load_room(&mut vfs).unwrap();
    assert_eq!(room.floor.center, (101, 81));
    assert_eq!(room.floor.tiles().len(), 81);
    assert_eq!(
        room.floor
            .tiles()
            .iter()
            .filter(|tile| tile.facade.is_some())
            .count(),
        15
    );
    let expected = [
        (
            35,
            (100, 78),
            4032,
            0x4010_01c0,
            "candlestand1.art",
            (-88, -161),
        ),
        (
            40,
            (104, 78),
            4041,
            0x4058_0400,
            "painting02_r.art",
            (-270, -110),
        ),
        (
            53,
            (98, 79),
            4041,
            0x4040_0400,
            "painting01_l.art",
            (34, -217),
        ),
        (
            61,
            (103, 80),
            4039,
            0x4120_0380,
            "couch_flip.art",
            (-158, -58),
        ),
        (70, (101, 81), 4037, 0x4020_0300, "gc_table.art", (-30, -66)),
        (78, (100, 82), 4039, 0x4118_0380, "couch.art", (22, -72)),
        (
            100,
            (98, 85),
            4032,
            0x4018_01c0,
            "candlestand2.art",
            (256, -62),
        ),
    ];
    assert_eq!(room.scenery.len(), expected.len());
    for (s, (record, tile, prototype, art, name, origin)) in room.scenery.iter().zip(expected) {
        assert_eq!(s.record.index, record);
        assert_eq!(s.record.category, StaticCategory::Scenery);
        assert_eq!(s.tile, tile);
        assert_eq!(s.record.prototype.0, prototype);
        assert_eq!(s.record.art_id, art);
        assert_eq!(s.appearance.scale, 100);
        assert_eq!((s.art.rotation, s.art.frame, s.art.palette), (0, 0, 0));
        assert_eq!(s.artwork.source.virtual_path, format!("art/scenery/{name}"));
        let draw = s.draw(room.floor.center).unwrap();
        assert_eq!(sprite_origin(draw.frame, draw.anchor), origin);
        let shifted = s.draw((102, 81)).unwrap();
        assert_eq!(
            sprite_origin(shifted.frame, shifted.anchor),
            (origin.0 + 40, origin.1 - 20)
        );
    }

    let draws = room.draws().unwrap().collect::<Vec<_>>();
    // Records 34 and 35 share a tile. The stand's offset puts it after the wall.
    assert!(std::ptr::eq(
        draws[85].frame,
        &room.walls[4].artwork.decoded.frame
    ));
    assert!(std::ptr::eq(
        draws[86].frame,
        &room.scenery[0].artwork.decoded.frame
    ));
    let render = |width, height| {
        let mut pixels = vec![[32, 43, 58]; width as usize * height as usize];
        blit_ordered(
            &mut pixels,
            (width, height),
            width as usize,
            draws.iter().copied().map(|mut draw| {
                draw.anchor.0 += (width / 2) as i32;
                draw.anchor.1 += (height / 2) as i32;
                draw
            }),
        )
        .unwrap();
        pixels
    };
    let full = render(800, 600);
    let clipped = render(400, 400);
    let wall = draws[85];
    let stand = draws[86];
    let pixel_at = |draw: arcanum_rs::compositor::SpriteDraw<'_>, x: usize, y: usize| {
        let (left, top) = sprite_origin(draw.frame, draw.anchor);
        let dx = (x as i64 - 400 - left) as u64;
        let dy = (y as i64 - 300 - top) as u64;
        if dx >= u64::from(draw.frame.width) || dy >= u64::from(draw.frame.height) {
            return None;
        }
        Some(draw.frame.pixels[(dy * u64::from(draw.frame.width) + dx) as usize])
    };
    let (mut opaque_overlap, mut transparent_overlap) = (0, 0);
    for y in 135..220 {
        for x in 311..350 {
            let (Some([wr, wg, wb, 255]), Some([sr, sg, sb, alpha])) =
                (pixel_at(wall, x, y), pixel_at(stand, x, y))
            else {
                continue;
            };
            if alpha == 255 {
                assert_eq!(full[y * 800 + x], [sr, sg, sb]);
                opaque_overlap += 1;
            } else if alpha == 0 {
                assert_eq!(full[y * 800 + x], [wr, wg, wb]);
                transparent_overlap += 1;
            }
        }
    }
    assert!(opaque_overlap > 0 && transparent_overlap > 0);

    for (row, actual) in clipped.chunks(400).enumerate() {
        let start = (row + 100) * 800 + 200;
        assert_eq!(actual, &full[start..start + 400]);
    }

    // Narrowing the floor about the same center must keep independently
    // selected scenery draws, including objects whose tiles are now outside it.
    room.floor = load_facade_region(&mut vfs, DEMO_MAP, DEMO_SECTOR, (35, 15), 5).unwrap();
    assert_eq!(room.floor.center, (101, 81));
    let outside = &room.scenery[6];
    assert!(
        room.floor
            .tiles()
            .iter()
            .all(|tile| tile.world != outside.tile)
    );
    let expected = outside.draw(room.floor.center).unwrap();
    assert!(room.draws().unwrap().any(|draw| {
        std::ptr::eq(draw.frame, expected.frame) && draw.anchor == expected.anchor
    }));
}

#[test]
#[ignore = "requires installed Arcanum data"]
fn bates_room_overview_reuses_the_original_room_inside_the_larger_sector_scene() {
    use arcanum_rs::content::scenery::{load_room, load_room_overview};
    use arcanum_rs::sector::objects::StaticCategory;

    let game = PathBuf::from(std::env::var_os("ARCANUM_GAME_DIR").expect("set ARCANUM_GAME_DIR"));
    let mut vfs = Vfs::from_game(&game, Policy::Ce).unwrap();
    let small = load_room(&mut vfs).unwrap();
    let overview = load_room_overview(&mut vfs).unwrap();
    let wide = &overview.room;
    assert_eq!(overview.sector_object_counts, (268, 19, 81));
    assert_eq!(wide.floor.tiles().len(), 25 * 25);
    assert_eq!(wide.floor.center, (101, 83));
    assert_eq!(wide.floor.source, small.floor.source);
    assert!(overview.outside_floor_walls > 0);
    assert!(overview.outside_floor_scenery > 0);
    assert!(
        overview
            .issues
            .iter()
            .any(|issue| issue.category == StaticCategory::Portal)
    );
    for tile in small.floor.tiles() {
        let other = wide
            .floor
            .tiles()
            .iter()
            .find(|other| other.world == tile.world)
            .unwrap();
        assert_eq!(other.id, tile.id);
        assert_eq!(other.frame_index(), tile.frame_index());
    }
    let shifted = |small: arcanum_rs::compositor::SpriteDraw<'_>,
                   wide: arcanum_rs::compositor::SpriteDraw<'_>| {
        assert_eq!(wide.anchor, (small.anchor.0 - 80, small.anchor.1 - 40));
        assert_eq!(wide.frame, small.frame);
    };
    for wall in &small.walls {
        let same = wide
            .walls
            .iter()
            .find(|other| other.record.index == wall.record.index)
            .unwrap();
        shifted(
            wall.draw(small.floor.center).unwrap(),
            same.draw(wide.floor.center).unwrap(),
        );
    }
    for prop in &small.scenery {
        let same = wide
            .scenery
            .iter()
            .find(|other| other.record.index == prop.record.index)
            .unwrap();
        shifted(
            prop.draw(small.floor.center).unwrap(),
            same.draw(wide.floor.center).unwrap(),
        );
    }
    assert!(wide.walls.iter().any(|wall| wall.orientation.rotation == 5));
}

#[test]
#[ignore = "requires installed Arcanum data"]
fn bates_world_preserves_original_portal_states_geometry_and_wall_order() {
    use arcanum_rs::{
        compositor::{blit_ordered, sprite_origin},
        content::{
            portal::{Kind, PortalState},
            scenery::{WorldRegion, load_world},
        },
    };
    use std::sync::Arc;
    let game = PathBuf::from(std::env::var_os("ARCANUM_GAME_DIR").expect("set ARCANUM_GAME_DIR"));
    let mut vfs = Vfs::from_game(&game, Policy::Ce).unwrap();
    let world = load_world(&mut vfs, WorldRegion::BATES_MANSION).unwrap();
    let room = &world.room;
    assert_eq!(
        room.portals
            .iter()
            .map(|p| p.record.index)
            .collect::<Vec<_>>(),
        [150, 152, 184, 186, 191, 193, 244, 246]
    );
    assert_eq!(world.sector_object_counts.1, 19);
    assert_eq!(world.culled_portals.len(), 11);
    assert!(
        world
            .issues
            .iter()
            .all(|issue| issue.category != arcanum_rs::sector::objects::StaticCategory::Portal)
    );
    assert_eq!(room.walls.len(), 96);
    assert_eq!(room.scenery.len(), 24);
    let leaf = &room.portals[0];
    let other = &room.portals[1];
    assert_eq!(leaf.art.kind, Kind::Door);
    assert_eq!(leaf.state, PortalState::Closed);
    assert_eq!(leaf.record.span, 31758..31841);
    assert_eq!(leaf.record.art_id, 0x3210_3c11);
    assert_eq!(leaf.prototype_art_id, 0x3000_3411);
    assert_eq!(leaf.placement.tile, (94, 90));
    assert_eq!(leaf.appearance.offset, (0, 0));
    assert_eq!(leaf.appearance.scale, 100);
    assert_eq!(leaf.appearance.portal_flags, 9);
    assert_eq!(leaf.artwork.source.virtual_path, "art/portal/in4e4au0.art");
    assert_eq!(leaf.art.pivot(&leaf.artwork.decoded.frame), (42, 99));
    assert_eq!(other.art.pivot(&other.artwork.decoded.frame), (33, 103));
    assert!(Arc::ptr_eq(&leaf.artwork, &room.portals[6].artwork));
    assert!(!Arc::ptr_eq(&leaf.artwork, &room.portals[2].artwork));
    let draw = leaf.draw(room.floor.center).unwrap();
    assert_eq!(sprite_origin(draw.frame, draw.anchor), (518, -99));
    let shifted = leaf.draw((102, 83)).unwrap();
    assert_eq!(sprite_origin(shifted.frame, shifted.anchor), (558, -119));

    let wall = room
        .walls
        .iter()
        .find(|w| w.record.index == leaf.paired_wall_index)
        .unwrap();
    let wall_draw = wall.draw(room.floor.center).unwrap();
    let draws = room.draws().unwrap().collect::<Vec<_>>();
    let index = draws
        .iter()
        .position(|d| std::ptr::eq(d.frame, draw.frame) && d.anchor == draw.anchor)
        .unwrap();
    assert!(std::ptr::eq(draws[index - 1].frame, wall_draw.frame));
    assert_eq!(draws[index - 1].anchor, wall_draw.anchor);
    for (index, (id, wall, tile, raw, path, archive, pivot, origin)) in [3, 5].into_iter().zip([
        (
            186,
            185,
            (110, 93),
            0x3178_ec00,
            "gb1e4au0.art",
            "Arcanum2.dat",
            (26, 80),
            (14, 300),
        ),
        (
            193,
            192,
            (110, 94),
            0x3198_ec00,
            "gb1e4bu0.art",
            "Arcanum5.dat",
            (40, 68),
            (40, 332),
        ),
    ]) {
        let open = &room.portals[index];
        assert_eq!(open.record.index, id);
        assert_eq!(open.record.art_id, raw);
        assert_eq!(open.state, PortalState::Open);
        assert_eq!(open.paired_wall_index, wall);
        assert_eq!(open.placement.tile, tile);
        assert_eq!(
            (
                open.art.frame,
                open.art.rotation,
                open.art.palette,
                open.art.mirror_x
            ),
            (3, 5, 0, false)
        );
        assert_eq!(open.appearance.offset, (0, 0));
        assert_eq!(
            open.artwork.source.virtual_path,
            format!("art/portal/{path}")
        );
        assert!(open.artwork.source.mount_path.ends_with(archive));
        let draw = open.draw(room.floor.center).unwrap();
        assert_eq!(open.art.pivot(draw.frame), pivot);
        assert_eq!(sprite_origin(draw.frame, draw.anchor), origin);
        assert!(draw.frame.pixels.iter().any(|p| p[3] == 0));
        assert!(draw.frame.pixels.iter().any(|p| p[3] == 255));
        let wall_draw = room
            .walls
            .iter()
            .find(|w| w.record.index == wall)
            .unwrap()
            .draw(room.floor.center)
            .unwrap();
        let index = draws
            .iter()
            .position(|d| std::ptr::eq(d.frame, draw.frame) && d.anchor == draw.anchor)
            .unwrap();
        assert!(std::ptr::eq(draws[index - 1].frame, wall_draw.frame));
        assert_eq!(draws[index - 1].anchor, wall_draw.anchor);
    }
    let clip = |dimensions: (u32, u32), position: (i32, i32)| {
        let stride = dimensions.0 as usize;
        let mut pixels = vec![[32, 43, 58]; stride * dimensions.1 as usize];
        blit_ordered(
            &mut pixels,
            dimensions,
            stride,
            draws.iter().map(|d| {
                let mut d = *d;
                d.anchor.0 += 640 - position.0;
                d.anchor.1 += 360 - position.1;
                d
            }),
        )
        .unwrap();
        pixels
    };
    let full = clip((1280, 720), (0, 0));
    let part = clip((140, 115), (1140, 255));
    for (row, value) in part.chunks(140).enumerate() {
        let start = (255 + row) * 1280 + 1140;
        assert_eq!(value, &full[start..start + 140]);
    }
    let clipped_open = clip((100, 80), (630, 640));
    for (row, value) in clipped_open.chunks(100).enumerate() {
        let start = (640 + row) * 1280 + 630;
        assert_eq!(value, &full[start..start + 100]);
    }
}

#[test]
#[ignore = "requires installed Arcanum data"]
fn bates_camera_selects_loaded_art_across_the_sector_and_round_trips() {
    use arcanum_rs::{
        camera::{Camera, CameraScene},
        compositor::blit_ordered,
        content::scenery::{WorldRegion, load_navigation_world},
    };
    let game = PathBuf::from(std::env::var_os("ARCANUM_GAME_DIR").expect("set ARCANUM_GAME_DIR"));
    let mut vfs = Vfs::from_game(&game, Policy::Ce).unwrap();
    let region = WorldRegion {
        origin: (0, 0),
        side: 64,
        ..WorldRegion::BATES_MANSION
    };
    let mut bytes = Vec::new();
    vfs.copy("maps/bates mansion lev 1/67108865.sec", &mut bytes)
        .unwrap();
    let original = arcanum_rs::sector::objects::decode(&bytes).unwrap();
    let navigation = load_navigation_world(&mut vfs, region).unwrap();
    drop(vfs);

    let room = &navigation.room;
    assert_eq!(room.floor.tiles().len(), 4096);
    assert_stock_wall_variants(room, &original);
    // The kitchen corner is two same-tile arms, followed by one neighboring
    // base wall on each side. All four inherit a zero pixel offset.
    for (index, art_id, tile, pivot, origin, path) in [
        (
            0,
            0x10318800,
            (78, 78),
            (-1, 151),
            (721, -711),
            "in4tcru0.art",
        ),
        (
            1,
            0x10317811,
            (78, 78),
            (39, 151),
            (681, -711),
            "in4tclu0.art",
        ),
        (
            3,
            0x10303811,
            (79, 78),
            (39, 152),
            (641, -692),
            "in4bseu0.art",
        ),
        (
            49,
            0x10300800,
            (78, 79),
            (-1, 152),
            (761, -692),
            "in4bseu0.art",
        ),
    ] {
        let wall = room
            .walls
            .iter()
            .find(|wall| wall.record.index == index)
            .unwrap();
        assert_eq!(wall.record.art_id, art_id);
        assert_eq!(wall.record.stored_tile, tile);
        assert_eq!(wall.placement.tile, tile);
        assert_eq!(
            (wall.record.fields.offset_x, wall.record.fields.offset_y),
            (None, None)
        );
        assert_eq!(wall.placement.pixel_offset, (0, 0));
        assert!(wall.artwork.source.virtual_path.ends_with(path));
        let draw = wall.draw(region.center).unwrap();
        assert_eq!(wall.orientation.pivot(draw.frame), pivot);
        assert_eq!(
            arcanum_rs::compositor::sprite_origin(draw.frame, draw.anchor),
            origin
        );
    }
    assert_eq!(
        room.floor
            .tiles()
            .iter()
            .filter(|tile| tile.facade.is_some())
            .count(),
        105
    );
    assert_eq!(room.portals.len(), 19);
    let scene = CameraScene::new(room.draws().unwrap());
    let cache = scene.cache();
    assert!(cache.repeated_references >= 4096 - cache.rasters);
    let mut camera = Camera::new(region.center, (64, 64), 64).unwrap();
    let viewport = (320, 200);
    let render = |camera: &Camera, viewport: (u32, u32)| {
        let stride = viewport.0 as usize;
        let mut pixels = vec![[32, 43, 58]; stride * viewport.1 as usize];
        blit_ordered(
            &mut pixels,
            viewport,
            stride,
            scene.visible(camera, viewport),
        )
        .unwrap();
        pixels
    };
    let initial = render(&camera, viewport);
    for delta in [(512, -64), (-432, 416), (-10000, -10000), (10000, 10000)] {
        camera.pan(delta, viewport);
        let (x, y) = camera.position();
        let stride = viewport.0 as usize;
        let mut reference = vec![[32, 43, 58]; stride * viewport.1 as usize];
        let all = room.draws().unwrap().map(|mut draw| {
            draw.anchor.0 += viewport.0 as i32 / 2 - x;
            draw.anchor.1 += viewport.1 as i32 / 2 - y;
            draw
        });
        blit_ordered(&mut reference, viewport, stride, all).unwrap();
        assert_eq!(render(&camera, viewport), reference);
        assert_eq!(scene.cache(), cache);
    }
    let resized = (640, 400);
    camera.resize(resized);
    camera.reset(resized);
    let large = render(&camera, resized);
    for (row, actual) in initial.chunks(320).enumerate() {
        let start = (row + 100) * 640 + 160;
        assert_eq!(actual, &large[start..start + 320]);
    }
    assert_eq!(render(&camera, viewport), initial);

    let kitchen_view = (1280, 720);
    camera.pan((720, -460), kitchen_view);
    assert_eq!(camera.position(), (720, -460));
    let edge = render(&camera, kitchen_view);
    camera.pan((0, 64), kitchen_view);
    assert_eq!(camera.position(), (720, -396));
    let inset = render(&camera, kitchen_view);
    assert_eq!(&edge[64 * 1280..], &inset[..656 * 1280]);
    assert_ne!(edge[145 * 1280 + 636], [0, 0, 0]);

    // The south wall follows Y=110 for 33 tiles and turns at (110, 110).
    let south: Vec<_> = room
        .walls
        .iter()
        .filter(|wall| wall.placement.tile.1 == 110 && wall.orientation.rotation == 3)
        .collect();
    assert_eq!(south.len(), 33);
    for (x, wall) in (78..=110).zip(&south) {
        assert_eq!(wall.placement.tile, (x, 110));
        assert_eq!(wall.orientation.palette, 1);
        assert!(wall.orientation.mirror_x());
        assert_eq!(wall.placement.pixel_offset, (0, 0));
    }
    let corner = room
        .walls
        .iter()
        .find(|wall| wall.record.index == 366)
        .unwrap();
    assert_eq!(corner.record.art_id, 0x1030e900);
    assert_eq!(corner.placement.tile, (110, 110));
    assert_eq!(corner.orientation.rotation, 5);
    assert_eq!(corner.orientation.palette, 0);
    assert_eq!(south.last().unwrap().record.art_id, 0x10311911);

    camera.reset(kitchen_view);
    camera.pan((1360, 400), kitchen_view);
    assert_eq!(camera.position(), (1360, 400));
    let south_first = render(&camera, kitchen_view);
    camera.pan((-64, 0), kitchen_view);
    assert_eq!(camera.position(), (1296, 400));
    let south_left = render(&camera, kitchen_view);
    for (first, left) in south_first.chunks(1280).zip(south_left.chunks(1280)) {
        assert_eq!(&first[..1216], &left[64..]);
    }
    let middle = south.iter().find(|wall| wall.record.index == 347).unwrap();
    let draw = middle.draw(region.center).unwrap();
    for step in [64, -64] {
        camera.pan((step, 0), kitchen_view);
        let (x, y) = camera.position();
        assert!(scene.visible(&camera, kitchen_view).any(|visible| {
            std::ptr::eq(visible.frame, draw.frame)
                && visible.anchor == (draw.anchor.0 + 640 - x, draw.anchor.1 + 360 - y)
        }));
    }

    camera.reset(kitchen_view);
    camera.pan((840, 620), kitchen_view);
    assert_eq!(camera.position(), (840, 620));
    let east = render(&camera, kitchen_view);
    camera.pan((64, 0), kitchen_view);
    assert_eq!(camera.position(), (904, 620));
    let east_right = render(&camera, kitchen_view);
    for (first, right) in east.chunks(1280).zip(east_right.chunks(1280)) {
        assert_eq!(&first[64..], &right[..1216]);
    }

    let route = [(0, 0), (720, -460), (832, 624), (1360, 400), (512, -64)];
    let mut snapshots = Vec::new();
    for &offset in &route {
        camera.reset(kitchen_view);
        camera.pan(offset, kitchen_view);
        assert_eq!(camera.position(), offset);
        snapshots.push(render(&camera, kitchen_view));
    }
    for (&offset, expected) in route.iter().zip(snapshots).rev() {
        camera.reset(kitchen_view);
        camera.pan(offset, kitchen_view);
        let clipped = (960, 560);
        camera.resize(clipped);
        assert_eq!(camera.position(), offset);
        let smaller = render(&camera, clipped);
        for (row, pixels) in smaller.chunks(960).enumerate() {
            let start = (row + 80) * 1280 + 160;
            assert_eq!(pixels, &expected[start..start + 960]);
        }
        camera.resize(kitchen_view);
        assert_eq!(render(&camera, kitchen_view), expected);
    }
}

fn assert_stock_wall_variants(
    room: &arcanum_rs::content::scenery::Room,
    original: &arcanum_rs::sector::objects::ObjectList,
) {
    use std::collections::BTreeSet;

    // Stock structure 3 names and piece suffixes from CE's wall filename table.
    let names = [
        "bse", "lfc", "bse", "bcl", "bcr", "tcl", "tcr", "uec", "lec", "w3l", "w3a", "w3r", "w4l",
        "w4a", "w4b", "w4r", "w5l", "w5a", "w5b", "w5c", "w5r", "d3l", "d3a", "d3r", "d4l", "d4a",
        "d4b", "d4r", "d6l", "d6a", "d6b", "d6c", "d6d", "d6r", "p3l", "p3a", "p3r", "p4l", "p4a",
        "p4b", "p4r",
    ];
    assert_eq!(original.walls.len(), 268);
    assert_eq!(room.walls.len(), original.walls.len());
    let mut ids = BTreeSet::new();
    for (stored, wall) in original.walls.iter().zip(&room.walls) {
        assert_eq!(
            (wall.record.index, wall.record.art_id),
            (stored.index, stored.art_id)
        );
        assert_eq!(wall.placement.tile, stored.world_tile(DEMO_SECTOR).unwrap());
        let id = stored.art_id;
        assert_eq!((id >> 20) & 0xff, 3);
        let rotation = (id >> 11) & 7;
        let (family, palette, mirror) = match rotation {
            1 => ("in4", 0, false),
            3 => ("gb1", 1, true),
            5 => ("gb1", 0, false),
            7 => ("in4", 1, true),
            _ => panic!("unexpected stock wall rotation {rotation}"),
        };
        let suffix = names[((id >> 14) & 63) as usize];
        let variation = (id >> 8) & 3;
        assert_eq!(
            wall.artwork.source.virtual_path,
            format!("art/wall/{family}{suffix}u{variation}.art")
        );
        assert_eq!(
            (wall.orientation.palette, wall.orientation.mirror_x()),
            (palette, mirror)
        );
        assert_eq!(wall.artwork.decoded.frame_count, 1);
        ids.insert(id);
    }
    assert_eq!(ids.len(), 73);
}
