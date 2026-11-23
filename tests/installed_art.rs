use arcanum_rs::content::art::load_frame;
use arcanum_rs::dat::EntryKind;
use arcanum_rs::vfs::{Policy, Storage, Vfs};
use std::path::PathBuf;

#[test]
#[ignore = "requires installed Arcanum data"]
fn original_art_frames_match_independent_rgba_fingerprints() {
    let game = PathBuf::from(std::env::var_os("ARCANUM_GAME_DIR").expect("set ARCANUM_GAME_DIR"));
    let mut vfs = Vfs::from_game(&game, Policy::Ce).unwrap();
    // Digests and alpha counts from a separate Python unpack and palette expansion.
    for (path, index, archive, count, size, hotspot, transparent, digest) in [
        (
            "art/item/bread.art",
            0,
            "Arcanum2.dat",
            1,
            (81, 24),
            (40, 23),
            505,
            0xc00e2ecb74eeaaa0,
        ),
        (
            "art/interface/align.art",
            0,
            "Arcanum1.dat",
            21,
            (27, 9),
            (39, 2),
            90,
            0x0bcc3acbb52866b1,
        ),
        (
            "art/interface/align.art",
            10,
            "Arcanum1.dat",
            21,
            (9, 28),
            (2, 39),
            87,
            0x771da9539c916557,
        ),
        (
            "art/interface/align.art",
            20,
            "Arcanum1.dat",
            21,
            (31, 9),
            (-13, 2),
            102,
            0xa45f6a86e2bfbe62,
        ),
    ] {
        let loaded = load_frame(&mut vfs, path, index).unwrap();
        assert_eq!(loaded.source.storage, Storage::Archive(EntryKind::Zlib));
        assert_eq!(
            loaded.source.mount_path,
            game.join(archive).canonicalize().unwrap()
        );
        assert_eq!(loaded.decoded.frame_count, count);
        assert_eq!(loaded.decoded.frames_per_second, 8);
        assert_eq!(loaded.decoded.action_frame, (count - 1) as i32);
        assert_eq!(loaded.decoded.ignored_trailing_bytes, 0);
        let frame = loaded.decoded.frame;
        assert_eq!((frame.width, frame.height), size);
        assert_eq!(frame.hotspot, hotspot);
        assert_eq!(frame.offset, (0, 0));
        assert_eq!(
            frame.pixels.iter().filter(|pixel| pixel[3] == 0).count(),
            transparent
        );
        assert_eq!(fingerprint(frame.pixels.as_flattened()), digest);
    }

    // The base copy uses raw indices. A BetaUI mount may shadow it in a game VFS.
    let mut base = Vfs::new(Policy::Ce);
    let mount = base.mount_archive(game.join("Arcanum1.dat")).unwrap();
    let raw = load_frame(&mut base, "art/interface/ap_green.art", 0).unwrap();
    assert_eq!(raw.source.mount_id, mount);
    assert_eq!(raw.input_bytes, 1316);
    assert_eq!(
        (raw.decoded.frame.width, raw.decoded.frame.height),
        (11, 12)
    );
    assert_eq!(
        fingerprint(raw.decoded.frame.pixels.as_flattened()),
        0x21338a70ced1d4e1
    );
}

#[test]
#[ignore = "requires installed Arcanum data"]
fn acid_schematic_decodes_declared_frame_and_reports_rle_residue() {
    let game = PathBuf::from(std::env::var_os("ARCANUM_GAME_DIR").expect("set ARCANUM_GAME_DIR"));
    let mut vfs = Vfs::from_game(&game, Policy::Ce).unwrap();
    let loaded = load_frame(&mut vfs, "art/interface/acid_scheme.art", 0).unwrap();
    assert_eq!(loaded.source.storage, Storage::Archive(EntryKind::Zlib));
    assert_eq!(
        loaded.source.mount_path,
        game.join("Arcanum1.dat").canonicalize().unwrap()
    );
    assert_eq!(loaded.input_bytes, 70_968);
    assert_eq!(loaded.decoded.ignored_trailing_bytes, 3_409);
    assert_eq!(loaded.decoded.frame_count, 1);
    let frame = loaded.decoded.frame;
    assert_eq!((frame.width, frame.height), (295, 225));
    assert_eq!(frame.hotspot, (147, 224));
    assert_eq!(frame.pixels[0], [207, 181, 135, 255]);
    assert!(frame.pixels.iter().all(|pixel| pixel[3] == 255));
    assert_eq!(fingerprint(frame.pixels.as_flattened()), 0xa525c396aaf50abe);
}

fn fingerprint(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    })
}

#[test]
#[ignore = "requires installed Arcanum data"]
fn indoor_floor_matches_verified_art_frame_and_diamond_footprint() {
    let game = PathBuf::from(std::env::var_os("ARCANUM_GAME_DIR").expect("set ARCANUM_GAME_DIR"));
    let mut vfs = Vfs::from_game(&game, Policy::Ce).unwrap();
    let loaded = load_frame(&mut vfs, "art/tile/fl1bse0a.art", 0).unwrap();
    assert_eq!(loaded.source.storage, Storage::Archive(EntryKind::Zlib));
    assert_eq!(
        loaded.source.mount_path,
        game.join("Arcanum2.dat").canonicalize().unwrap()
    );
    assert_eq!(loaded.input_bytes, 2942);
    assert_eq!(loaded.decoded.ignored_trailing_bytes, 0);
    assert_eq!(loaded.decoded.frame_count, 1);
    assert_eq!(loaded.decoded.frames_per_second, 8);
    assert_eq!(loaded.decoded.action_frame, 1);
    let frame = loaded.decoded.frame;
    assert_eq!((frame.width, frame.height), (78, 40));
    assert_eq!(frame.hotspot, (39, 39));
    assert_eq!(frame.offset, (0, 0));
    assert_eq!(fingerprint(frame.pixels.as_flattened()), 0x88a99bb39596342a);
    let fixture = arcanum_rs::isometric::synthetic_floor();
    assert!(
        frame
            .pixels
            .iter()
            .zip(&fixture.pixels)
            .all(|(original, synthetic)| original[3] == synthetic[3])
    );
}

#[test]
#[ignore = "requires installed Arcanum data"]
fn original_chair_directions_and_tree_palette_match_independent_rgba() {
    let game = PathBuf::from(std::env::var_os("ARCANUM_GAME_DIR").expect("set ARCANUM_GAME_DIR"));
    let mut vfs = Vfs::from_game(&game, Policy::Ce).unwrap();
    for (name, rotation, palette, stored, size, hotspot, transparent, digest) in [
        (
            "city_chair1",
            1,
            0,
            (1, 0),
            (41, 69),
            (21, 56),
            1543,
            0x8b76a13b9d17b51b,
        ),
        (
            "city_chair1",
            3,
            0,
            (3, 0),
            (41, 78),
            (21, 64),
            1772,
            0x16fc610d2aaea2bf,
        ),
        (
            "city_chair1",
            3,
            2,
            (3, 2),
            (41, 78),
            (21, 64),
            1772,
            0xa233632f857ed28b,
        ),
        (
            "tree",
            0,
            2,
            (0, 0),
            (249, 253),
            (142, 244),
            34941,
            0x4bcea6e8adc069f9,
        ),
    ] {
        let mut bytes = Vec::new();
        vfs.copy(&format!("art/scenery/{name}.art"), &mut bytes)
            .unwrap();
        let art = arcanum_rs::art::decode_direction(&bytes, 0, rotation, palette).unwrap();
        assert_eq!((art.stored_rotation, art.stored_palette), stored);
        assert_eq!((art.frame.width, art.frame.height), size);
        assert_eq!(art.frame.hotspot, hotspot);
        assert_eq!(
            art.frame.pixels.iter().filter(|p| p[3] == 0).count(),
            transparent
        );
        let hash = art
            .frame
            .pixels
            .iter()
            .flatten()
            .fold(0xcbf29ce484222325_u64, |hash, &byte| {
                (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
            });
        assert_eq!(hash, digest);
    }
}
