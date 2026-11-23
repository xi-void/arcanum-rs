use super::*;
use flate2::{Compression, write::ZlibEncoder};
use std::sync::atomic::{AtomicUsize, Ordering};

const PLAIN: u32 = 1;
const COMPRESSED: u32 = 2;
const DIRECTORY: u32 = 0x400;
const IGNORED: u32 = 0x800;

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        static SERIAL: AtomicUsize = AtomicUsize::new(0);
        loop {
            let serial = SERIAL.fetch_add(1, Ordering::Relaxed);
            let path =
                std::env::temp_dir().join(format!("arcanum-vfs-{}-{serial}", std::process::id()));
            match fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {}
                Err(err) => panic!("{err}"),
            }
        }
    }

    fn write(&self, path: &str, data: &[u8]) -> PathBuf {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, data).unwrap();
        path
    }

    fn archive(&self, path: &str, entries: &[(&str, u32, &[u8])]) -> PathBuf {
        let mut data = Vec::new();
        let mut index = Vec::new();
        let mut name_bytes = 0;
        for &(name, flags, payload) in entries {
            let length = name.len() as u32 + 1;
            name_bytes += length;
            word(&mut index, length);
            index.extend_from_slice(name.as_bytes());
            index.push(0);
            let stored = if flags == COMPRESSED {
                let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
                encoder.write_all(payload).unwrap();
                encoder.finish().unwrap()
            } else {
                payload.to_vec()
            };
            for value in [
                0,
                flags,
                payload.len() as u32,
                stored.len() as u32,
                data.len() as u32,
            ] {
                word(&mut index, value);
            }
            data.extend_from_slice(&stored);
        }
        let index_distance = index.len() as u32 + 16;
        let span = data.len() as u32 + 4;
        word(&mut data, span);
        word(&mut data, entries.len() as u32);
        data.extend(index);
        data.extend_from_slice(b" TAD");
        word(&mut data, name_bytes);
        word(&mut data, index_distance);
        self.write(path, &data)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn word(data: &mut Vec<u8>, number: u32) {
    data.extend_from_slice(&number.to_le_bytes());
}

fn copy(vfs: &mut Vfs, path: &str) -> Vec<u8> {
    let mut output = Vec::new();
    assert_eq!(vfs.copy(path, &mut output).unwrap(), output.len() as u64);
    output
}

#[test]
fn later_archives_win_and_reregistering_promotes_the_same_mount() {
    let f = Fixture::new();
    let first = f.archive("a.dat", &[("Art\\Item", PLAIN, b"first")]);
    let second = f.archive("b.patch9", &[("art/item", COMPRESSED, b"second")]);
    for policy in [Policy::Ce, Policy::Ordered] {
        let mut vfs = Vfs::new(policy);
        let a = vfs.mount_archive(&first).unwrap();
        let b = vfs.mount_archive(&second).unwrap();
        let resolved = vfs.resolve(r".\ART//ITEM").unwrap();
        assert_eq!(resolved.path, "art/item");
        assert_eq!(resolved.winner().unwrap().mount.id, b);
        assert_eq!(
            resolved.winner().unwrap().storage,
            Storage::Archive(EntryKind::Zlib)
        );
        assert_eq!(resolved.candidates()[1].mount.id, a);
        assert_eq!(copy(&mut vfs, "art/item"), b"second");

        assert_eq!(vfs.mount_archive(&first).unwrap(), a);
        assert_eq!(vfs.mounts().map(|m| m.id).collect::<Vec<_>>(), [a, b]);
        assert_eq!(copy(&mut vfs, "art/item"), b"first");
    }
}

#[test]
fn loose_files_override_only_across_lower_priority_mounts() {
    let f = Fixture::new();
    let low = f.archive("low.dat", &[("art/item", PLAIN, b"base")]);
    let high = f.archive("high.dat", &[("art/item", PLAIN, b"patch")]);
    f.write("loose/ART/ITEM", b"loose");
    f.write("loose/ART/only", b"fallback");
    f.write("top/art/item", b"top");
    let mut vfs = Vfs::new(Policy::Ce);
    vfs.mount_archive(low).unwrap();
    let loose = vfs.mount_directory(f.0.join("loose")).unwrap();
    assert_eq!(copy(&mut vfs, "art/item"), b"loose");
    let patch = vfs.mount_archive(high).unwrap();
    assert_eq!(copy(&mut vfs, "art/item"), b"patch");
    assert_eq!(copy(&mut vfs, "ArT/ONLY"), b"fallback");
    let top = vfs.mount_directory(f.0.join("top")).unwrap();
    let resolution = vfs.resolve("aRT/ItEm").unwrap();
    assert_eq!(resolution.winner().unwrap().mount.id, top);
    assert_eq!(resolution.candidates()[1].mount.id, patch);
    assert_eq!(resolution.candidates()[2].mount.id, loose);
    assert_eq!(
        resolution.candidates()[2].loose_path,
        Some(Path::new("ART/ITEM"))
    );
    assert_eq!(copy(&mut vfs, "art/item"), b"top");
}

#[test]
fn directory_records_mask_files_in_ce_mode_and_are_metadata_in_ordered_mode() {
    let f = Fixture::new();
    let low = f.archive("a.dat", &[("name", PLAIN, b"file")]);
    let high = f.archive("b.dat", &[("name", DIRECTORY, b"")]);
    let mut ce = Vfs::new(Policy::Ce);
    ce.mount_archive(&low).unwrap();
    let dir = ce.mount_archive(&high).unwrap();
    assert_eq!(ce.resolve("name").unwrap().winner().unwrap().mount.id, dir);
    assert!(matches!(
        ce.copy("name", io::sink()),
        Err(Error::NotAFile(_))
    ));
    let mut ordered = Vfs::new(Policy::Ordered);
    ordered.mount_archive(low).unwrap();
    ordered.mount_archive(high).unwrap();
    let result = ordered.resolve("name").unwrap();
    assert_eq!(
        result.candidates()[0].exclusion,
        Some(Exclusion::DirectoryRecord)
    );
    assert_eq!(copy(&mut ordered, "name"), b"file");
}

#[test]
fn ce_ignore_patterns_apply_to_one_archive_at_matching_depth() {
    let f = Fixture::new();
    let low = f.archive("a.dat", &[("art/a.txt", PLAIN, b"low")]);
    let high = f.archive(
        "b.dat",
        &[
            ("art/a.txt", PLAIN, b"high"),
            ("art/b.txt", PLAIN, b"high"),
            ("art/c.txt", PLAIN, b"high"),
            ("art/deep/a.txt", PLAIN, b"deep"),
            ("art/deep/keep.bin", PLAIN, b"deep wildcard"),
            ("art/top.bin", PLAIN, b"ignored wildcard"),
            ("hidden", PLAIN | IGNORED, b"ignored"),
            ("unknown", PLAIN | 0x10, b"unknown"),
        ],
    );
    f.write("b.dat.ignore", b"ART\\[a-b].TXT\nART\\*.BIN\ninvalid[\n");
    let mut ce = Vfs::new(Policy::Ce);
    ce.mount_archive(&low).unwrap();
    ce.mount_archive(&high).unwrap();
    assert_eq!(copy(&mut ce, "art/a.txt"), b"low");
    assert_eq!(copy(&mut ce, "art/c.txt"), b"high");
    assert_eq!(copy(&mut ce, "art/deep/a.txt"), b"deep");
    assert_eq!(copy(&mut ce, "art/deep/keep.bin"), b"deep wildcard");
    let conflicts = ce.conflicts().collect::<Vec<_>>();
    assert_eq!(conflicts.len(), 1);
    assert_eq!(
        conflicts[0].candidates()[0].exclusion,
        Some(Exclusion::IgnoreSidecar)
    );
    for (path, expected) in [
        ("art/b.txt", Exclusion::IgnoreSidecar),
        ("art/top.bin", Exclusion::IgnoreSidecar),
        ("hidden", Exclusion::IgnoredEntry),
        ("unknown", Exclusion::UnsupportedEntry),
    ] {
        let resolution = ce.resolve(path).unwrap();
        assert!(resolution.winner().is_none());
        assert_eq!(resolution.candidates()[0].exclusion, Some(expected));
    }

    let mut ordered = Vfs::new(Policy::Ordered);
    ordered.mount_archive(low).unwrap();
    ordered.mount_archive(high).unwrap();
    assert_eq!(copy(&mut ordered, "art/a.txt"), b"high");
    assert!(ordered.resolve("hidden").unwrap().winner().is_none());
}

#[test]
fn absent_resources_and_invalid_paths_are_distinct() {
    let mut vfs = Vfs::new(Policy::Ce);
    let missing = vfs.resolve("MISSING").unwrap();
    assert_eq!(missing.path, "missing");
    assert!(missing.candidates().is_empty());
    assert!(
        matches!(vfs.copy("MISSING", io::sink()), Err(Error::NotFound(path)) if path == "missing")
    );
    for path in [
        "../escape",
        "art/../escape",
        r"C:\escape",
        r"\server\share",
        "/escape",
    ] {
        assert!(
            matches!(vfs.resolve(path), Err(Error::InvalidPath(_))),
            "{path}"
        );
    }
}

#[test]
fn case_collisions_are_deterministic_and_visible() {
    let f = Fixture::new();
    f.write("art/Button", b"upper");
    if f.0.join("art/button").exists() {
        return; // The host filesystem cannot hold both spellings.
    }
    f.write("art/button", b"lower");
    let mut vfs = Vfs::new(Policy::Ce);
    vfs.mount_directory(&f.0).unwrap();
    assert_eq!(copy(&mut vfs, "Art/bUtTOn"), b"upper");
    let conflict = vfs.conflicts().next().unwrap();
    assert_eq!(conflict.path, "art/button");
    assert_eq!(
        conflict.candidates()[0].loose_path,
        Some(Path::new("art/Button"))
    );
    assert_eq!(
        conflict.candidates()[1].loose_path,
        Some(Path::new("art/button"))
    );
}

#[cfg(unix)]
#[test]
fn symlinks_never_redirect_loose_reads_outside_the_mount() {
    use std::os::unix::fs::symlink;

    let f = Fixture::new();
    let outside = f.write("outside/sub/secret", b"outside");
    let original = f.write("loose/sub/secret", b"inside");
    symlink(&outside, f.0.join("loose/file-link")).unwrap();
    symlink(f.0.join("outside"), f.0.join("loose/dir-link")).unwrap();
    symlink(&original, f.0.join("loose/internal-link")).unwrap();
    symlink(".", f.0.join("loose/cycle")).unwrap();

    let mut vfs = Vfs::new(Policy::Ce);
    vfs.mount_directory(f.0.join("loose")).unwrap();
    for path in [
        "file-link",
        "dir-link/sub/secret",
        "internal-link",
        "cycle/sub/secret",
    ] {
        assert!(vfs.resolve(path).unwrap().candidates().is_empty(), "{path}");
    }
    assert_eq!(copy(&mut vfs, "sub/secret"), b"inside");

    fs::rename(f.0.join("loose/sub"), f.0.join("loose/old-sub")).unwrap();
    symlink(f.0.join("outside/sub"), f.0.join("loose/sub")).unwrap();
    let mut output = Vec::new();
    assert!(matches!(
        vfs.copy("sub/secret", &mut output),
        Err(Error::Io { .. })
    ));
    assert!(output.is_empty());
}

#[test]
fn discovers_stock_ce_order_without_creating_or_deleting_directories() {
    let f = Fixture::new();
    for name in [
        "Arcanum3.DAT",
        "arcanum1.dat",
        "TIG.DAT",
        "Modules/ARCANUM.PATCH9",
        "Modules/Arcanum.PATCH3",
        "Modules/arcanum.dat",
    ] {
        f.archive(name, &[("resource", PLAIN, name.as_bytes())]);
    }
    f.archive("Modules/Other.dat", &[("resource", PLAIN, b"other module")]);
    f.write("DATA/a", b"loose data");
    f.write("Modules/Arcanum/a", b"loose module data");
    f.write("DATA/TIGCache/a", b"cache");

    let mut vfs = Vfs::from_game(&f.0, Policy::Ce).unwrap();
    let root = fs::canonicalize(&f.0).unwrap();
    let mounts = vfs
        .mounts()
        .map(|m| m.path.strip_prefix(&root).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        mounts,
        [
            "Modules/Arcanum",
            "Modules/ARCANUM.PATCH9",
            "Modules/Arcanum.PATCH3",
            "Modules/arcanum.dat",
            "DATA",
            "Arcanum3.DAT",
            "arcanum1.dat",
            "",
            "TIG.DAT",
        ]
        .map(Path::new)
    );
    assert_eq!(copy(&mut vfs, "resource"), b"Modules/ARCANUM.PATCH9");
    assert_eq!(copy(&mut vfs, "a"), b"loose module data");
    assert!(vfs.resolve("TIGCache/a").unwrap().winner().is_none());
    assert!(f.0.join("DATA/TIGCache/a").is_file());
}

#[test]
fn prefix_inventory_deduplicates_across_mounts_before_resolving_winners() {
    let fixture = Fixture::new();
    let base = fixture.archive(
        "base.dat",
        &[
            ("proto/017089 - npc.pro", PLAIN, b"other"),
            ("proto/017088 - scenery.pro", PLAIN, b"scenery"),
            ("Proto\\017088 - NPC.PRO", PLAIN, b"base"),
        ],
    );
    fixture.write("loose/proto/017088 - npc.pro", b"override");
    let mut vfs = Vfs::new(Policy::Ordered);
    vfs.mount_archive(base).unwrap();
    let loose = fixture.0.join("loose");
    let winner = vfs.mount_directory(loose).unwrap();
    let prefix = "proto/017088 - ";
    let paths = vfs
        .paths_with_prefix(prefix)
        .into_iter()
        .collect::<Vec<_>>();
    assert_eq!(
        paths,
        ["proto/017088 - npc.pro", "proto/017088 - scenery.pro"]
    );
    assert_eq!(
        vfs.resolve(paths[0]).unwrap().winner().unwrap().mount.id,
        winner
    );
}
