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
    assert_eq!(room.scenery.len(), 34);
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
    let find = |index| {
        room.scenery
            .iter()
            .find(|prop| prop.record.index == index)
            .unwrap()
    };
    let (chair, alternate) = (find(59), find(242));
    assert_eq!(room.scenery.len(), 79);
    assert_eq!(chair.stored_art_id, Some(0x40d81b40));
    assert_eq!(chair.prototype_art_id, Some(0x40000340));
    assert_eq!(chair.appearance.offset, (-12, 7));
    assert_eq!(chair.artwork.decoded.stored_rotation, 3);
    assert_eq!(alternate.artwork.decoded.stored_palette, 2);
    assert!(std::sync::Arc::ptr_eq(
        &alternate.artwork,
        &find(249).artwork
    ));
    assert_eq!(
        navigation
            .issues
            .iter()
            .map(|i| i.record)
            .collect::<Vec<_>>(),
        [19, 27]
    );
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
    let mut streamed_bates = arcanum_rs::content::world::WorldStream::new(
        Vfs::from_game(&game, Policy::Ce).unwrap(),
        region.map.into(),
        arcanum_rs::content::world::SectorRegion {
            first: region.sector,
            last: region.sector,
        },
        region.center,
    )
    .unwrap();
    streamed_bates.prepare(&camera, viewport).unwrap();
    assert_eq!(streamed_bates.reports().next().unwrap().wall_count, 268);
    let mut streamed_pixels = vec![[32, 43, 58]; 320 * 200];
    blit_ordered(
        &mut streamed_pixels,
        viewport,
        320,
        streamed_bates.visible(&camera, viewport),
    )
    .unwrap();
    assert_eq!(streamed_pixels, initial);
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

#[test]
#[ignore = "requires installed Arcanum data"]
fn kna_tha_streams_both_original_sides_of_a_populated_boundary() {
    use arcanum_rs::{
        content::world::{SectorRegion, WorldStream},
        sector::Coordinates,
    };

    let game = PathBuf::from(std::env::var_os("ARCANUM_GAME_DIR").expect("set ARCANUM_GAME_DIR"));
    let vfs = Vfs::from_game(&game, Policy::Ce).unwrap();
    let region = SectorRegion {
        first: Coordinates { x: 5, y: 14 },
        last: Coordinates { x: 8, y: 14 },
    };
    let center = (341, 928);
    let size = (1280, 720);
    let mut camera = region.camera(center).unwrap();
    let mut world = WorldStream::new(vfs, "k'na tha".into(), region, center).unwrap();
    let render = |world: &WorldStream, camera: &_| {
        let mut output = vec![[0; 3]; 1280 * 720];
        blit_ordered(&mut output, size, 1280, world.visible(camera, size)).unwrap();
        output
    };
    world.prepare(&camera, size).unwrap();
    assert_eq!(
        world.reports().map(|report| report.id).collect::<Vec<_>>(),
        [939524101]
    );
    let initial = render(&world, &camera);
    let initial_decodes = world.stats().art.decodes;
    camera.pan((-1532, 934), size); // 2Y−X=3400, the X=383.5 boundary.
    assert_eq!(camera.position(), (-1532, 934));
    world.prepare(&camera, size).unwrap();
    assert_eq!(
        world
            .reports()
            .map(|r| (r.id, r.surfaces, r.wall_count, r.scenery_count))
            .collect::<Vec<_>>(),
        [(939524101, 4096, 28, 433), (939524102, 4096, 64, 475)]
    );
    assert_eq!(
        world.object_visibility(939524101, &camera, size),
        Some((22, 439))
    );
    assert_eq!(
        world.object_visibility(939524102, &camera, size),
        Some((51, 488))
    );
    let reads = world.stats().vfs_reads;
    let decodes = world.stats().art.decodes;
    assert_eq!(decodes - initial_decodes, 3);
    let seam = render(&world, &camera);
    camera.pan((-32, 0), size);
    world.prepare(&camera, size).unwrap();
    let crossing = render(&world, &camera);
    for (a, b) in seam.chunks(1280).zip(crossing.chunks(1280)) {
        assert_eq!(&a[..1248], &b[32..]);
    }
    camera.pan((-736, 384), size);
    world.prepare(&camera, size).unwrap();
    assert_eq!(world.stats().sector_reads, 2);
    camera.reset(size);
    world.prepare(&camera, size).unwrap();
    assert_eq!(render(&world, &camera), initial);
    assert_eq!(world.stats().vfs_reads, reads);
    assert_eq!(world.stats().art.decodes, decodes);
}

#[test]
#[ignore = "requires installed Arcanum data"]
fn kna_tha_brazier_court_is_in_its_original_sector() {
    use arcanum_rs::{
        content::{
            load_mes,
            scenery::SceneryArt,
            world::{SectorRegion, WorldStream},
        },
        sector::{Coordinates, objects},
    };

    let game = PathBuf::from(std::env::var_os("ARCANUM_GAME_DIR").expect("set ARCANUM_GAME_DIR"));
    let mut vfs = Vfs::from_game(&game, Policy::Ce).unwrap();
    let sector = Coordinates { x: 12, y: 12 };
    assert_eq!(sector.id(), Some(805306380));
    let mut bytes = Vec::new();
    vfs.copy("maps/k'na tha/805306380.sec", &mut bytes).unwrap();
    let court = objects::decode(&bytes).unwrap();
    let names = load_mes(&mut vfs, "art/scenery/scenery.mes").unwrap();
    let region = SectorRegion {
        first: sector,
        last: sector,
    };
    let center = (798, 786);
    let camera = region.camera(center).unwrap();
    let mut world = WorldStream::new(vfs, "k'na tha".into(), region, center).unwrap();
    world.prepare(&camera, (1280, 720)).unwrap();

    for (index, x) in [(102, 793), (104, 798), (106, 803)] {
        let base = court
            .scenery
            .iter()
            .find(|base| base.index == index)
            .unwrap();
        assert_eq!(base.world_tile(sector), Some((x, 781)));
        assert_eq!(base.art_id, 0x407002c0);
        assert_eq!(
            SceneryArt::decode(base.art_id)
                .unwrap()
                .resource_path(&names.mes)
                .unwrap(),
            "art/scenery/brazurelarge.art"
        );
        let flame = court
            .scenery
            .iter()
            .find(|flame| flame.index == index + 1)
            .unwrap();
        assert_eq!(flame.world_tile(sector), Some((x, 781)));
        assert_eq!(flame.prototype.0, 4040);
        assert_eq!(flame.art_id, 0x40100540);
        assert_eq!(flame.fields.blit_flags, Some(16));
        assert_eq!(flame.fields.light_flags, Some(0x08000080));
        assert_eq!(flame.fields.light_art, Some(0x90180000));
        assert_eq!(flame.fields.light_color, Some(0x00cca873));
    }
    use arcanum_rs::compositor::Modulation;
    use std::time::Duration;
    let size = (1280, 720);
    let flames_at = |ms| {
        world
            .visible_assets_at(&camera, size, Duration::from_millis(ms))
            .filter(|draw| matches!(draw.modulation, Modulation::Additive))
            .collect::<Vec<_>>()
    };
    let draws: Vec<_> = world.visible_assets(&camera, size).collect();
    for (base, flame) in [
        ((641, 159), (642, 161)),
        ((441, 261), (442, 261)),
        ((239, 361), (240, 363)),
    ] {
        let base = draws
            .iter()
            .position(|draw| {
                draw.asset.source.virtual_path == "art/scenery/brazurelarge.art"
                    && draw.sprite.anchor == base
            })
            .unwrap();
        let flame = draws
            .iter()
            .position(|draw| {
                matches!(draw.modulation, Modulation::Additive) && draw.sprite.anchor == flame
            })
            .unwrap();
        assert!(base < flame);
    }
    let first = flames_at(0);
    let next = flames_at(90);
    assert_eq!(first.len(), 3);
    assert_eq!(next.len(), 3);
    for (i, (from, to)) in first.iter().zip(&next).enumerate() {
        assert_eq!(
            from.asset.source.virtual_path,
            "art/scenery/brazurefire.art"
        );
        assert_eq!(from.asset.decoded.stored_palette, 0);
        assert_eq!(from.asset.decoded.frames_per_second, 11);
        assert_eq!(from.asset.decoded.frame_count, 20);
        assert_eq!(from.sprite.anchor, [(642, 161), (442, 261), (240, 363)][i]);
        assert_eq!(from.sprite.anchor, to.sprite.anchor);
        assert_eq!(from.sprite.frame.hotspot, (40, 155));
        assert_eq!(to.sprite.frame.hotspot, (44, 154));
        assert!(std::sync::Arc::ptr_eq(from.asset, first[0].asset));
        assert!(std::sync::Arc::ptr_eq(from.asset, flames_at(1800)[i].asset));
    }
}

#[test]
#[ignore = "requires installed Arcanum data"]
fn kna_tha_passage_is_placed_on_the_overworld() {
    use arcanum_rs::{
        content::{load_mes, scenery::SceneryArt},
        sector::{Coordinates, objects},
    };

    let game = PathBuf::from(std::env::var_os("ARCANUM_GAME_DIR").expect("set ARCANUM_GAME_DIR"));
    let mut vfs = Vfs::from_game(&game, Policy::Ce).unwrap();
    let sector = Coordinates { x: 1435, y: 614 };
    assert_eq!(sector.id(), Some(41204843931));
    let mut bytes = Vec::new();
    vfs.copy("maps/arcanum1-024-fixed/41204843931.sec", &mut bytes)
        .unwrap();
    let objects = objects::decode(&bytes).unwrap();
    let passage = objects
        .scenery
        .iter()
        .find(|record| record.index == 160)
        .unwrap();
    assert_eq!(passage.world_tile(sector), Some((91900, 39315)));
    assert_eq!(passage.prototype.0, 4047);
    assert_eq!(passage.art_id, 0x40c80500);
    assert_eq!(
        (passage.fields.offset_x, passage.fields.offset_y),
        (Some(5), Some(-15))
    );
    let names = load_mes(&mut vfs, "art/scenery/scenery.mes").unwrap();
    assert_eq!(
        SceneryArt::decode(passage.art_id)
            .unwrap()
            .resource_path(&names.mes)
            .unwrap(),
        "art/scenery/tree_door.art"
    );
}

#[test]
#[ignore = "requires installed Arcanum data"]
fn original_static_light_masks_modulate_the_scene_without_moving_rasters() {
    use arcanum_rs::{
        compositor::Modulation,
        content::world::{SectorRegion, WorldStream},
        sector::Coordinates,
    };

    let game = PathBuf::from(std::env::var_os("ARCANUM_GAME_DIR").expect("set ARCANUM_GAME_DIR"));
    for (map, sector, center, hour, expected_counts, expected_center) in [
        (
            DEMO_MAP,
            DEMO_SECTOR,
            (101, 83),
            12,
            (9, 9, 0),
            [124, 126, 139],
        ),
        (
            "k'na tha",
            Coordinates { x: 12, y: 12 },
            (798, 786),
            0,
            (16, 16, 0),
            [74, 170, 234],
        ),
    ] {
        let vfs = Vfs::from_game(&game, Policy::Ce).unwrap();
        let region = SectorRegion {
            first: sector,
            last: sector,
        };
        let size = (1280, 720);
        let mut camera = region.camera(center).unwrap();
        camera.resize(size);
        let mut world = WorldStream::new(vfs, map.into(), region, center)
            .unwrap()
            .with_lighting(hour)
            .unwrap();
        world.prepare(&camera, size).unwrap();
        let report = world.reports().next().unwrap();
        assert_eq!(report.light_counts, Some(expected_counts));
        if map == "k'na tha" {
            assert_eq!(report.object_lights, 3);
        }
        if map == DEMO_MAP {
            assert_eq!(report.wall_count, 268);
            assert_eq!(report.scenery_count, 81);
            // Fifteen candle stands and two hanging lanterns join the
            // two fireplace lights. Stands with invalid primary IDs stay dark.
            assert_eq!(report.object_lights, 19);
        }
        let center_anchor = (640 - camera.position().0, 379 - camera.position().1);
        let mut shaded_center = false;
        let mut lit_objects = 0;
        let mut column_lit = 0;
        let mut checked_original_objects = 0;
        let mut checked_candle_tiles = 0;
        for (ordinary, (draw, shade)) in world
            .visible(&camera, size)
            .zip(world.visible_modulated(&camera, size))
        {
            assert_eq!(ordinary.anchor, draw.anchor);
            assert!(std::ptr::eq(ordinary.frame, draw.frame));
            if map == DEMO_MAP {
                let position = (
                    draw.anchor.0 + camera.position().0 - 640,
                    draw.anchor.1 + camera.position().1 - 360,
                );
                let expected = match (draw.frame.width, draw.frame.height, position) {
                    (121, 109, (-177, -28)) => Some([129, 128, 138]), // Sofa 61.
                    (121, 109, (12, -42)) => Some([127, 125, 134]),   // Sofa 78.
                    _ => None,
                };
                if let Some(expected) = expected {
                    assert!(matches!(shade, Modulation::Uniform(actual) if actual == expected));
                    checked_original_objects += 1;
                }
                if (draw.frame.width, draw.frame.height, position) == (40, 154, (0, -160)) {
                    let Modulation::Columns(columns) = shade else {
                        panic!("candle 35 should light wall 52 across a gradient");
                    };
                    assert_eq!(columns[0], [169, 153, 144]);
                    assert_eq!(columns[19], [158, 144, 139]);
                    assert_eq!(columns[39], [142, 132, 131]);
                    checked_original_objects += 1;
                }
                if (draw.frame.width, draw.frame.height) == (78, 40) {
                    let candle_tile_sample = match position {
                        (-160, -101) => Some([255, 255, 212]), // Candle 35: tile (100,78).
                        (200, -1) => Some([255, 255, 210]),    // Candle 100: tile (98,85).
                        _ => None,
                    };
                    if let Some(expected) = candle_tile_sample {
                        assert!(matches!(shade, Modulation::Floor(nodes) if nodes[4] == expected));
                        checked_candle_tiles += 1;
                    }
                }
            }
            if draw.anchor == center_anchor
                && let Modulation::Floor(nodes) = shade
            {
                // Measured independently by decoding the original mask indices
                // and sampling the center location in each original record.
                assert_eq!(nodes[4], expected_center);
                shaded_center = true;
            }
            if (draw.frame.width, draw.frame.height) != (78, 40) {
                lit_objects += usize::from(!matches!(shade, Modulation::Unlit));
                if let Modulation::Columns(columns) = shade {
                    assert_eq!(columns.len(), draw.frame.width as usize);
                    assert!(columns.windows(2).any(|pair| pair[0] != pair[1]));
                    column_lit += 1;
                }
            }
        }
        assert!(shaded_center);
        assert!(lit_objects > 0);
        assert!(column_lit > 0);
        if map == DEMO_MAP {
            assert_eq!(checked_original_objects, 3);
            assert_eq!(checked_candle_tiles, 2);
        }
        let before = world.stats();
        camera.pan((32, -16), size);
        let warm_timing = world.prepare(&camera, size).unwrap();
        assert!(warm_timing.shading.is_zero());
        let after = world.stats();
        assert_eq!(after.vfs_reads, before.vfs_reads);
        assert_eq!(after.art.decodes, before.art.decodes);
        assert_eq!(after.light_samples, before.light_samples);
    }
}

#[test]
#[ignore = "requires installed Arcanum data"]
fn kna_tha_rabbit_sector_retains_building_objects_after_the_locked_portal() {
    use arcanum_rs::{
        content::{
            load_mes,
            portal::{PortalArt, PortalState},
            world::{SectorRegion, WorldStream},
        },
        sector::{Coordinates, decode_floor, objects},
    };

    let game = std::env::var_os("ARCANUM_GAME_DIR").expect("set ARCANUM_GAME_DIR");
    let mut vfs = Vfs::from_game(game, Policy::Ce).unwrap();
    let path = "maps/k'na tha/805306374.sec";
    assert!(vfs.resolve(path).unwrap().winner().unwrap().size < 80_000);
    let mut bytes = Vec::new();
    vfs.copy(path, &mut bytes).unwrap();
    let floor = decode_floor(&bytes).unwrap();
    assert_eq!(floor.tile_offset, 100);
    assert_eq!(&bytes[16_484..16_488], &1u32.to_le_bytes());
    assert_eq!(floor.ids().filter(|tile| tile.0 >> 28 != 0).count(), 0);
    let names = load_mes(&mut vfs, "art/tile/tilename.mes").unwrap();
    let tile = floor.tile(22, 14).unwrap();
    assert_eq!(tile, TileArtId(0x028a_0400));
    let floor_art = tile.resolve(&names.mes).unwrap();
    assert_eq!(floor_art.path, "art/tile/stdbse0c.art");
    assert!(!floor_art.fields.outdoor);
    let road = floor.tile(21, 20).unwrap().resolve(&names.mes).unwrap();
    assert_eq!(road.families, ["ts1", "ts1"]);
    assert_eq!(road.path, "art/tile/ts1bse0a.art");

    let objects = objects::decode(&bytes).unwrap();
    assert_eq!((objects.start, objects.count), (17_044, 653));
    assert_eq!(
        (
            objects.walls.len(),
            objects.scenery_count,
            objects.portals.len()
        ),
        (251, 400, 2)
    );
    let portal = objects
        .portals
        .iter()
        .find(|record| record.index == 558)
        .unwrap();
    assert_eq!(portal.span, 67_710..67_801);
    assert_eq!(portal.prototype.0, 2029);
    assert_eq!(portal.stored_tile, (395, 826));
    assert_eq!(portal.art_id, 0x3348_3c11);
    assert_eq!(portal.fields.portal_flags, Some(0x11));
    assert_eq!(&bytes[67_797..67_801], &100u32.to_le_bytes());
    let next_wall = objects
        .walls
        .iter()
        .find(|record| record.index == 559)
        .unwrap();
    assert_eq!(next_wall.span.start, portal.span.end);

    let north_door = objects
        .portals
        .iter()
        .find(|record| record.index == 202)
        .unwrap();
    assert_eq!(north_door.stored_tile, (406, 786));
    assert_eq!(
        PortalArt::decode(north_door.art_id)
            .unwrap()
            .visual_state()
            .unwrap(),
        PortalState::Open
    );
    let sector = Coordinates { x: 6, y: 12 };
    let region = SectorRegion {
        first: sector,
        last: sector,
    };
    let center = (406, 782);
    let size = (1280, 720);
    let camera = region.camera(center).unwrap();
    let mut world = WorldStream::new(vfs, "k'na tha".into(), region, center)
        .unwrap()
        .with_object_audit();
    world.prepare(&camera, size).unwrap();
    let report = world.reports().next().unwrap();
    assert!(!report.object_list_failed);
    assert_eq!(report.original_object_counts, (251, 2, 400));
    assert_eq!(report.object_raster_rejections, 0);
    assert_eq!(report.object_audit.len(), 653);
    assert!(
        report
            .object_audit
            .iter()
            .any(|line| { line.starts_with("record 558 Portal") && line.contains("dg2e3au0.art") })
    );
    assert_eq!(
        (
            report.surfaces,
            report.wall_count,
            report.portal_count,
            report.scenery_count
        ),
        (4096, 251, 2, 400),
        "{:?}",
        report.diagnostics
    );
    let draws = world
        .visible_assets(&camera, size)
        .map(|draw| draw.asset.source.virtual_path.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    for path in [
        "art/wall/dg2w3au0.art",
        "art/wall/dg2w4bu0.art",
        "art/portal/dg2e3au0.art",
    ] {
        assert!(draws.contains(path), "{path} missing from the north scene");
    }
}

#[test]
#[ignore = "requires installed Arcanum data"]
fn tulla_is_reconstructed_from_map_metadata_and_generic_sector_import() {
    use arcanum_rs::{
        content::{
            load_mes,
            world::{SectorRegion, WorldStream},
        },
        sector::{Coordinates, objects},
    };

    let game = std::env::var_os("ARCANUM_GAME_DIR").expect("set ARCANUM_GAME_DIR");
    let mut vfs = Vfs::from_game(game, Policy::Ce).unwrap();
    let maps = load_mes(&mut vfs, "rules/maplist.mes").unwrap();
    let (map, cx, cy) = maps
        .mes
        .entries()
        .find_map(|(_, value)| {
            let mut fields = value.split(',').map(str::trim);
            let map = fields.next()?;
            if !map.eq_ignore_ascii_case("Tulla Interior") {
                return None;
            }
            Some((
                map.to_ascii_lowercase(),
                fields.next()?.parse::<u32>().ok()?,
                fields.next()?.parse::<u32>().ok()?,
            ))
        })
        .expect("Tulla map metadata");
    let mut start = Vec::new();
    vfs.copy(&format!("maps/{map}/startloc.txt"), &mut start)
        .unwrap();
    let mut words = std::str::from_utf8(&start)
        .unwrap()
        .split_ascii_whitespace();
    let sx: u32 = words.next().unwrap().parse().unwrap();
    let sy: u32 = words.next().unwrap().parse().unwrap();
    let sector = |x, y| Coordinates { x, y };
    let region = SectorRegion {
        first: sector(cx.min(sx) / 64, cy.min(sy) / 64),
        last: sector(cx.max(sx) / 64, cy.max(sy) / 64),
    };
    let mut raw_counts = std::collections::BTreeMap::new();
    for y in region.first.y..=region.last.y {
        for x in region.first.x..=region.last.x {
            let id = sector(x, y).id().unwrap();
            let mut bytes = Vec::new();
            vfs.copy(&format!("maps/{map}/{id}.sec"), &mut bytes)
                .unwrap();
            let objects = objects::decode(&bytes).unwrap();
            let counts = (
                objects.walls.len(),
                objects.portal_count,
                objects.scenery_count,
            );
            assert_eq!(objects.count, counts.0 + counts.1 + counts.2);
            raw_counts.insert(id, counts);
        }
    }
    let center = (cx, cy);
    let size = (1280, 720);
    let camera = region.camera(center).unwrap();
    let mut world = WorldStream::new(vfs, map, region, center)
        .unwrap()
        .with_object_audit();
    world.prepare(&camera, size).unwrap();
    let mut retained = Vec::new();
    for report in world.reports() {
        assert!(!report.object_list_failed, "{:?}", report.diagnostics);
        assert_eq!(report.original_object_counts, raw_counts[&report.id]);
        assert_eq!(report.surfaces, 4096);
        assert_eq!(report.object_raster_rejections, 0);
        let (walls, portals, scenery) = report.original_object_counts;
        let imported = report.wall_count + report.portal_count + report.scenery_count;
        assert_eq!(report.object_audit.len(), imported);
        assert_eq!(
            report
                .diagnostics
                .iter()
                .filter(|line| line.starts_with("record "))
                .count(),
            walls + portals + scenery - imported
        );
        let (submitted, clipped) = world.object_visibility(report.id, &camera, size).unwrap();
        assert_eq!(submitted + clipped, imported);
        retained.push((
            report.facades,
            report.wall_count,
            report.portal_count,
            report.scenery_count,
        ));
    }
    assert_eq!(retained, [(407, 331, 5, 50), (1111, 650, 5, 53)]);
    let visible = world
        .visible_assets(&camera, size)
        .map(|draw| draw.asset.source.virtual_path.as_str())
        .collect::<Vec<_>>();
    for category in ["art/tile/", "art/wall/", "art/portal/", "art/scenery/"] {
        assert!(
            visible.iter().any(|path| path.starts_with(category)),
            "{category}"
        );
    }
}

#[test]
#[ignore = "requires installed Arcanum data"]
fn kna_tha_south_sector_preserves_geometry_across_the_floor_boundary() {
    use arcanum_rs::{
        content::world::{SectorRegion, WorldStream},
        sector::{Coordinates, objects},
    };
    let game = std::env::var_os("ARCANUM_GAME_DIR").expect("set ARCANUM_GAME_DIR");
    let mut vfs = Vfs::from_game(game, Policy::Ce).unwrap();
    let north = Coordinates { x: 6, y: 12 };
    let south = Coordinates { x: 6, y: 13 };
    let id = south.id().unwrap();
    let mut bytes = Vec::new();
    vfs.copy(&format!("maps/k'na tha/{id}.sec"), &mut bytes)
        .unwrap();
    let original = objects::decode(&bytes).unwrap();
    assert_eq!(
        (
            original.count,
            original.walls.len(),
            original.scenery_count,
            original.portal_count
        ),
        (824, 373, 447, 4)
    );
    let scenery = original
        .scenery
        .iter()
        .find(|record| record.index == 720)
        .unwrap();
    assert_eq!(scenery.span, 82_640..82_743);
    assert_eq!(scenery.prototype.0, 4038);
    assert_eq!(scenery.stored_tile, (434, 885));
    assert_eq!(
        &bytes[scenery.span.end - 4..scenery.span.end],
        &25u32.to_le_bytes()
    );

    let region = SectorRegion {
        first: north,
        last: south,
    };
    let center = (396, 829);
    let size = (1280, 720);
    let mut camera = region.camera(center).unwrap();
    camera.resize(size);
    let mut world = WorldStream::new(vfs, "k'na tha".into(), region, center).unwrap();
    world.prepare(&camera, size).unwrap();
    let report = world.reports().find(|report| report.id == id).unwrap();
    assert!(!report.object_list_failed);
    assert_eq!(
        (report.wall_count, report.scenery_count, report.portal_count),
        (373, 447, 4)
    );
    assert_eq!(report.object_raster_rejections, 0);
    assert_eq!(world.object_visibility(id, &camera, size), Some((53, 771)));
}
