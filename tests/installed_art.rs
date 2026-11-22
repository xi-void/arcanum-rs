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

fn fingerprint(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    })
}
