use super::*;
use crate::vfs::Storage;
use flate2::{Compression, write::ZlibEncoder};
use std::io::Write;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        loop {
            let serial = NEXT.fetch_add(1, Ordering::Relaxed);
            let path =
                std::env::temp_dir().join(format!("arcanum-mods-{}-{serial}", std::process::id()));
            match fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {}
                Err(err) => panic!("{err}"),
            }
        }
    }

    fn write(&self, path: &str, data: impl AsRef<[u8]>) -> PathBuf {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, data).unwrap();
        path
    }

    fn package(&self, dir: &str, id: &str, fields: &str) -> PathBuf {
        self.write(&format!("mods/{dir}/{MANIFEST_NAME}"), format!(
            "manifest_version = 1\nid = {id:?}\nname = \"Test package\"\nversion = \"1.2.3\"\ndescription = \"Synthetic test package\"\n{fields}"
        )).parent().unwrap().to_owned()
    }

    fn catalog(&self) -> Catalog {
        Catalog::discover(self.0.join("mods")).unwrap()
    }

    fn archive(&self, output: &str, name: &[u8], data: &[u8]) {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(data).unwrap();
        let mut archive = encoder.finish().unwrap();
        let compressed_length = archive.len() as u32;
        let mut word = |value: u32| archive.extend(value.to_le_bytes());
        let name_length = name.len() as u32 + 1;
        word(compressed_length + 4);
        word(1);
        word(name_length);
        archive.extend(name);
        archive.push(0);
        for value in [0, 2, data.len() as u32, compressed_length, 0] {
            archive.extend(value.to_le_bytes());
        }
        archive.extend(b" TAD");
        archive.extend(name_length.to_le_bytes());
        archive.extend((24 + name_length + 16).to_le_bytes());
        self.write(output, archive);
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn ids<'a>(plan: &LoadOrder<'a>) -> Vec<&'a str> {
    plan.packages()
        .iter()
        .map(|package| package.manifest().id.as_str())
        .collect()
}

#[test]
fn reads_manifest_and_preserves_mount_spelling() {
    let f = Fixture::new();
    let dir = f.package(
        "Folder",
        "example.mod_01",
        r"
load_before = ['later']
[dependencies]
base = '^1.2'
[[mounts]]
kind = 'directory'
path = 'Resources\Assets'
[[mounts]]
kind = 'archive'
path = 'Archives/Extra.PATCH9'
",
    );
    let package = Package::open(dir).unwrap();
    let manifest = package.manifest();
    assert_eq!(manifest.manifest_version, 1);
    assert_eq!(manifest.id.as_str(), "example.mod_01");
    assert_eq!(manifest.version.to_string(), "1.2.3");
    assert_eq!(manifest.name, "Test package");
    assert_eq!(manifest.description, "Synthetic test package");
    assert_eq!(manifest.load_before[0].as_str(), "later");
    assert_eq!(manifest.dependencies["base"].to_string(), "^1.2");
    assert_eq!(manifest.mounts[0].path, Path::new("Resources/Assets"));
    assert_eq!(manifest.mounts[1].kind, vfs::MountKind::Archive);
}

#[test]
fn independent_order_uses_id_and_is_stable_across_selection_order() {
    let f = Fixture::new();
    f.package("aaa", "zenith", "");
    f.package("zzz", "alpha", "");
    f.package("mmm", "middle", "");
    f.write("mods/unrelated/document", "ignored");
    for _ in 0..2 {
        let catalog = f.catalog();
        assert_eq!(
            ids(&catalog.load_order(&[]).unwrap()),
            ["alpha", "middle", "zenith"]
        );
        assert_eq!(
            ids(&catalog.load_order(&["zenith", "alpha"]).unwrap()),
            ["alpha", "zenith"]
        );
        assert_eq!(
            ids(&catalog.load_order(&["alpha", "zenith"]).unwrap()),
            ["alpha", "zenith"]
        );
        assert!(matches!(
            catalog.load_order(&["gone"]),
            Err(Error::UnknownMod(_))
        ));
        assert!(matches!(
            catalog.load_order(&["alpha", "alpha"]),
            Err(Error::DuplicateSelection(_))
        ));
    }
}

#[test]
fn adds_transitive_dependencies_and_only_enforces_constraints_for_enabled_targets() {
    let f = Fixture::new();
    f.package(
        "alpha",
        "alpha",
        "load_after = ['bonus']\n[dependencies]\nmiddle = '^1.0'\n",
    );
    f.package("middle", "middle", "[dependencies]\nbase = '*'\n");
    f.package("base", "base", "");
    f.package("bonus", "bonus", "load_before = ['base']\n");
    let catalog = f.catalog();
    assert_eq!(
        ids(&catalog.load_order(&["alpha"]).unwrap()),
        ["base", "middle", "alpha"]
    );
    assert_eq!(
        ids(&catalog.load_order(&["alpha", "bonus"]).unwrap()),
        ["bonus", "base", "middle", "alpha"]
    );
}

#[test]
fn reports_missing_dependencies_versions_and_unknown_order_targets() {
    let f = Fixture::new();
    f.package("base", "base", "");
    f.package("a", "a", "[dependencies]\nbase = '>=2'\n");
    f.package("b", "b", "[dependencies]\nabsent = '*'\n");
    f.package("c", "c", "load_before = ['unknown']\n");
    let catalog = f.catalog();
    for (id, expected) in [
        (
            "a",
            "requires \"base\" (>=2), but installed version is 1.2.3",
        ),
        ("b", "requires \"absent\" (*), which is not installed"),
        (
            "c",
            "load_before references mod \"unknown\", which is not installed",
        ),
    ] {
        assert_eq!(
            catalog.load_order(&[id]).unwrap_err().to_string(),
            format!("mod {id:?}: {expected}")
        );
    }
}

#[test]
fn rejects_dependency_cycles_and_conflicting_explicit_constraints() {
    let f = Fixture::new();
    f.package("a", "a", "[dependencies]\nb = '*'\n");
    f.package("b", "b", "[dependencies]\nc = '*'\n");
    f.package("c", "c", "[dependencies]\na = '*'\n");
    f.package("d", "d", "load_before = ['e']\n[dependencies]\ne = '*'\n");
    f.package("e", "e", "");
    let catalog = f.catalog();
    let dependencies = catalog.load_order(&["a"]).unwrap_err();
    assert!(
        matches!(&dependencies, Error::Cycle(chain) if chain.contains("dependencies in a") && chain.contains("dependencies in b") && chain.contains("dependencies in c"))
    );
    let constraints = catalog.load_order(&["d"]).unwrap_err();
    assert!(
        matches!(&constraints, Error::Cycle(chain) if chain.contains("load_before in d") && chain.contains("dependencies in d"))
    );
}

#[test]
fn duplicate_id_reports_both_package_locations() {
    let f = Fixture::new();
    let first = f.package("one", "shared", "");
    let second = f.package("two", "shared", "");
    assert!(matches!(
        Catalog::discover(f.0.join("mods")),
        Err(Error::DuplicateId { id, first: left, second: right })
            if id.as_str() == "shared" && left == first.canonicalize().unwrap() && right == second.canonicalize().unwrap()
    ));
}

#[test]
fn rejects_malformed_and_unsupported_manifests() {
    let f = Fixture::new();
    let directory = f.package("test", "valid", "");
    let path = directory.join(MANIFEST_NAME);
    let valid = fs::read_to_string(&path).unwrap();
    for (text, expected) in [
        (
            valid.replace("manifest_version = 1", "manifest_version = 2") + "future_field = true\n",
            "unsupported manifest_version 2",
        ),
        (
            valid.replace("manifest_version = 1\n", ""),
            "manifest_version",
        ),
        (valid.replace("1.2.3", "unversioned"), "unversioned"),
        (valid.clone() + "unexpected = true\n", "unknown field"),
        (
            valid.clone() + "[dependencies]\na = 'nonsense'\n",
            "nonsense",
        ),
        (valid.clone() + "unclosed = [\n", "TOML parse error"),
        (valid.replace("valid", "Invalid ID"), "invalid mod ID"),
    ] {
        fs::write(&path, text).unwrap();
        let error = Package::open(&directory).unwrap_err().to_string();
        assert!(
            error.contains(MANIFEST_NAME) && error.contains(expected),
            "{error}"
        );
    }
}

#[test]
fn rejects_path_traversal_and_wrong_mount_targets() {
    let f = Fixture::new();
    for path in [
        "..",
        "../escape",
        "nested/../escape",
        "nested/.. /escape",
        "/etc",
        r"C:\etc",
        r"\\host\share",
        ".",
        "zero*",
    ] {
        let dir = f.package(
            "bad",
            "bad",
            &format!("[[mounts]]\nkind = 'directory'\npath = '{path}'\n"),
        );
        let result = Package::open(dir).unwrap_err().to_string();
        assert!(result.contains("mounts[0].path"), "{result}");
    }
    f.package(
        "bad",
        "bad",
        "[[mounts]]\nkind = 'directory'\npath = 'gone'\n",
    );
    let catalog = f.catalog();
    assert!(
        catalog
            .load_order(&[])
            .unwrap()
            .validate_mounts()
            .unwrap_err()
            .to_string()
            .contains("gone")
    );
    f.package(
        "bad",
        "bad",
        "[[mounts]]\nkind = 'directory'\npath = 'file'\n",
    );
    f.write("mods/bad/file", "a file rather than a directory");
    assert!(
        f.catalog()
            .load_order(&[])
            .unwrap()
            .validate_mounts()
            .is_err()
    );
    f.package(
        "bad",
        "bad",
        "[[mounts]]\nkind = 'archive'\npath = 'broken.dat'\n",
    );
    f.write("mods/bad/broken.dat", "bad data");
    assert!(
        matches!(f.catalog().load_order(&[]).unwrap().validate_mounts(), Err(Error::Mount { id, .. }) if id.as_str() == "bad")
    );
}

#[test]
fn resources_follow_package_and_mount_order_with_provenance() {
    let f = Fixture::new();
    f.write("game/data/art/item", b"stock");
    f.package(
        "base",
        "base",
        "[[mounts]]\nkind = 'directory'\npath = 'resources'\n",
    );
    f.write("mods/base/resources/ART/Item", b"dependency");
    f.package("add-on", "addon", "[dependencies]\nbase = '*'\n[[mounts]]\nkind = 'archive'\npath = 'content.patch9'\n[[mounts]]\nkind = 'directory'\npath = 'resources'\n");
    f.archive("mods/add-on/content.patch9", b"art/item", b"archive");
    f.write("mods/add-on/resources/art/item", b"dependent");
    f.package(
        "inactive",
        "inactive",
        "[[mounts]]\nkind = 'directory'\npath = 'resources'\n",
    );
    f.write("mods/inactive/resources/art/item", b"inactive");
    let catalog = f.catalog();
    let plan = catalog.load_order(&["addon"]).unwrap();
    let stock = || {
        let mut vfs = vfs::Vfs::new(vfs::Policy::Ordered);
        vfs.mount_directory(f.0.join("game/data")).unwrap();
        vfs
    };
    let mut vfs = plan.apply(stock()).unwrap();
    let conflict = vfs
        .conflicts()
        .find(|found| found.path == "art/item")
        .unwrap();
    let owners = conflict
        .candidates()
        .iter()
        .map(|candidate| candidate.mount.mod_id.as_ref().map(ModId::as_str))
        .collect::<Vec<_>>();
    assert_eq!(owners, [Some("addon"), Some("addon"), Some("base"), None]);
    assert_eq!(conflict.winner().unwrap().storage, Storage::Loose);
    assert_eq!(
        conflict.candidates()[1].storage,
        Storage::Archive(crate::dat::EntryKind::Zlib)
    );
    let mut output = Vec::new();
    vfs.copy(r"ART\ITEM", &mut output).unwrap();
    assert_eq!(output, b"dependent");
    fs::remove_file(f.0.join("mods/add-on/resources/art/item")).unwrap();
    let mut vfs = plan.apply(stock()).unwrap();
    output.clear();
    vfs.copy("art/item", &mut output).unwrap();
    assert_eq!(output, b"archive");

    let ce = vfs::Vfs::from_game(f.0.join("game"), vfs::Policy::Ce).unwrap();
    assert!(
        ce.resolve("art/item")
            .unwrap()
            .winner()
            .unwrap()
            .mount
            .mod_id
            .is_none()
    );
    assert!(matches!(plan.apply(ce), Err(Error::RequiresOrderedVfs)));
}

#[cfg(unix)]
#[test]
fn rejects_symlinked_manifests_and_mount_paths() {
    use std::os::unix::fs::symlink;

    let f = Fixture::new();
    f.write("outside/safe/file", "outside");
    f.write("outside/fake.dat", "outside");
    let package = f.package("test", "test", "");
    symlink(&package, f.0.join("mods/linked-package")).unwrap();
    assert_eq!(f.catalog().packages().count(), 1);

    let manifest = package.join(MANIFEST_NAME);
    let outside = f.0.join("outside").join(MANIFEST_NAME);
    fs::rename(&manifest, &outside).unwrap();
    symlink(outside, &manifest).unwrap();
    assert!(
        matches!(Package::open(&package), Err(Error::Io { source, .. }) if source.kind() == io::ErrorKind::InvalidInput)
    );
    fs::remove_file(&manifest).unwrap();

    symlink(f.0.join("outside"), package.join("link")).unwrap();
    symlink(f.0.join("outside/fake.dat"), package.join("link.dat")).unwrap();
    for (kind, path) in [
        ("directory", "link"),
        ("directory", "link/safe"),
        ("archive", "link.dat"),
        ("archive", "link/fake.dat"),
    ] {
        f.package(
            "test",
            "test",
            &format!("[[mounts]]\nkind = '{kind}'\npath = '{path}'\n"),
        );
        assert!(
            f.catalog()
                .load_order(&[])
                .unwrap()
                .validate_mounts()
                .is_err(),
            "{path}"
        );
    }

    f.package(
        "test",
        "test",
        "[[mounts]]\nkind = 'directory'\npath = 'data'\n",
    );
    f.write("mods/test/data/inside", "inside");
    symlink(f.0.join("outside/safe/file"), package.join("data/escaped")).unwrap();
    let catalog = f.catalog();
    let order = catalog.load_order(&[]).unwrap();
    let vfs = order.apply(vfs::Vfs::new(vfs::Policy::Ordered)).unwrap();
    assert!(vfs.resolve("escaped").unwrap().winner().is_none());
    assert!(vfs.resolve("inside").unwrap().winner().is_some());
    fs::rename(package.join("data"), package.join("old-data")).unwrap();
    symlink(f.0.join("outside/safe"), package.join("data")).unwrap();
    assert!(order.apply(vfs::Vfs::new(vfs::Policy::Ordered)).is_err());
}
