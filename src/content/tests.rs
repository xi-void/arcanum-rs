use super::*;
use crate::dat::EntryKind;
use crate::mods::Catalog;
use crate::vfs::Policy;
use flate2::{Compression, write::ZlibEncoder};
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        loop {
            let path = std::env::temp_dir().join(format!(
                "arcanum-content-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
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

    fn archive(&self) -> PathBuf {
        let name = b"mes\\test.mes\0";
        let text = b"{8}{Archive value.}\r\n{9}{Only in the archive}";
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(text).unwrap();
        let mut archive = encoder.finish().unwrap();
        let compressed = archive.len() as u32;
        let name_length = name.len() as u32;
        for value in [compressed + 4, 1, name_length] {
            archive.extend(value.to_le_bytes());
        }
        archive.extend(name);
        for value in [0, 2, text.len() as u32, compressed, 0] {
            archive.extend(value.to_le_bytes());
        }
        archive.extend(b" TAD");
        archive.extend(name_length.to_le_bytes());
        archive.extend((24 + name_length + 16).to_le_bytes());
        self.write("original.patch", &archive)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn loads_archive_then_mod_replacement_and_preserves_provenance() {
    let fixture = Fixture::new();
    let archive = fixture.archive().canonicalize().unwrap();
    let mut vfs = Vfs::new(Policy::Ordered);
    let base_id = vfs.mount_archive(&archive).unwrap();
    let base = load_mes(&mut vfs, r".\MES\TEST.MES").unwrap();
    assert_eq!(base.mes.get(8), Some("Archive value."));
    assert_eq!(base.source.virtual_path, "mes/test.mes");
    assert_eq!(base.source.mount_id, base_id);
    assert_eq!(base.source.mount_path, archive);
    assert_eq!(base.source.storage, Storage::Archive(EntryKind::Zlib));
    assert!(base.source.mod_id.is_none());

    fixture.write(
        "mods/patch/arcanum-mod.toml",
        br#"
manifest_version = 1
id = "patch"
name = "Synthetic MES patch"
version = "1.0.0"
description = "Temporary test fixture."
[[mounts]]
kind = "directory"
path = "resources"
"#,
    );
    fixture.write("mods/patch/resources/Mes/Test.mes", b"{8}{Mod override.}");
    let catalog = Catalog::discover(fixture.0.join("mods")).unwrap();
    let mut vfs = catalog.load_order(&["patch"]).unwrap().apply(vfs).unwrap();
    let replacement = load_mes(&mut vfs, "mes/test.mes").unwrap();
    assert_eq!(replacement.mes.get(8), Some("Mod override."));
    assert_eq!(replacement.mes.get(9), None);
    assert_eq!(replacement.source.virtual_path, base.source.virtual_path);
    assert_ne!(replacement.source.mount_id, base.source.mount_id);
    assert_eq!(replacement.source.mod_id.unwrap().as_str(), "patch");
    assert_eq!(replacement.source.storage, Storage::Loose);
    assert_eq!(
        replacement.source.loose_path.as_deref(),
        Some(Path::new("Mes/Test.mes"))
    );
}

#[test]
fn distinguishes_resolution_and_parse_errors_and_keeps_the_source() {
    let fixture = Fixture::new();
    fixture.write("mes/truncated.mes", b"{5}{unfinished");
    fixture.write("mes/encoded.mes", b"{5}{caf\xe9}");
    let mut vfs = Vfs::new(Policy::Ce);
    let id = vfs.mount_directory(&fixture.0).unwrap();
    assert!(matches!(
        load_mes(&mut vfs, "mes/absent.mes"),
        Err(Error::Resource {
            error: vfs::Error::NotFound(_),
            ..
        })
    ));
    assert!(matches!(
        load_mes(&mut vfs, "../escape"),
        Err(Error::Resource {
            error: vfs::Error::InvalidPath { .. },
            ..
        })
    ));
    assert!(matches!(
        load_mes(&mut vfs, "MES/TRUNCATED.MES"),
        Err(Error::Mes { provenance, error }) if provenance.mount_id == id && error.kind == mes::ErrorKind::UnterminatedField
    ));
    assert_eq!(
        load_mes_with_encoding(&mut vfs, "mes/encoded.mes", Encoding::Windows1252)
            .unwrap()
            .mes
            .get(5),
        Some("café")
    );
    assert!(matches!(
        load_mes_with_encoding(&mut vfs, "mes/encoded.mes", Encoding::Utf8),
        Err(Error::Mes { error, .. }) if error.kind == mes::ErrorKind::InvalidUtf8
    ));
}

#[test]
fn enforces_limits_on_indexed_size_and_on_actual_stream() {
    let fixture = Fixture::new();
    let path = fixture.write("large.mes", b"{1}{}");
    let mut vfs = Vfs::new(Policy::Ordered);
    vfs.mount_directory(&fixture.0).unwrap();
    fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .set_len(mes::MAX_INPUT_BYTES as u64 + 1)
        .unwrap();
    assert!(matches!(
        load_mes(&mut vfs, "large.mes"),
        Err(Error::TooLarge { .. })
    ));

    let mut grown = Vfs::new(Policy::Ordered);
    grown.mount_directory(&fixture.0).unwrap();
    assert!(matches!(
        load_mes(&mut grown, "large.mes"),
        Err(Error::TooLarge { .. })
    ));
}

#[test]
fn weapon_inspection_resolves_native_identity_and_tracks_overrides() {
    use crate::content::items::{
        AmmoKind, DamageKind, ItemId, WeaponAmmoDefinition, inspect_prototype,
    };
    use crate::prototype::{DescriptionId, SerializedPrototypeId, tests::weapon_bytes};

    let fixture = Fixture::new();
    let path = "proto/006136 - weapon.pro";
    fixture.write(&format!("base/{path}"), &weapon_bytes(6136, 400));
    fixture.write("base/mes/description.mes", b"{400}{Original name}");
    let mut vfs = Vfs::new(Policy::Ordered);
    vfs.mount_directory(fixture.0.join("base")).unwrap();
    let stock = inspect_prototype(&mut vfs, path).unwrap();
    let stock_name = stock.resolve_weapon_ammo(&mut vfs).unwrap().unwrap();
    assert!(stock.prototype.source.mod_id.is_none());
    assert!(stock_name.name_source.mod_id.is_none());

    fixture.write(
        "mods/patch/arcanum-mod.toml",
        br#"
manifest_version = 1
id = "patch"
name = "Synthetic weapon patch"
version = "1.0.0"
description = "Temporary test fixture."
[[mounts]]
kind = "directory"
path = "resources"
"#,
    );
    fixture.write(
        &format!("mods/patch/resources/{path}"),
        &weapon_bytes(6136, 30025),
    );
    fixture.write(
        "mods/patch/resources/mes/gamedesc.mes",
        b"{30025}{Override name}",
    );
    let catalog = Catalog::discover(fixture.0.join("mods")).unwrap();
    let mut vfs = catalog.load_order(&["patch"]).unwrap().apply(vfs).unwrap();
    let inspection = inspect_prototype(&mut vfs, path).unwrap();
    let resolved = inspection.resolve_weapon_ammo(&mut vfs).unwrap().unwrap();
    assert_eq!(inspection.prototype.header.id, SerializedPrototypeId(6136));
    assert_eq!(inspection.prototype.description, Some(DescriptionId(30025)));
    assert_eq!(
        inspection.prototype.source.mod_id.unwrap().as_str(),
        "patch"
    );
    assert_eq!(resolved.name_source.mod_id.unwrap().as_str(), "patch");
    let WeaponAmmoDefinition::Weapon(weapon) = resolved.definition else {
        panic!("expected Weapon");
    };
    assert_eq!(weapon.id, ItemId("arcanum:item/prototype-6136".into()));
    assert_eq!(weapon.display_name, "Override name");
    assert_eq!(
        weapon
            .damage
            .iter()
            .map(|damage| damage.kind)
            .collect::<Vec<_>>(),
        [DamageKind::Normal, DamageKind::Fatigue]
    );
    assert_eq!(weapon.ammunition.unwrap().kind, AmmoKind::Bullets);
}

#[test]
fn armour_inspection_preserves_identity_fit_and_mod_provenance() {
    use crate::content::items::{
        ArmourCoverage, ArmourSize, ItemId, ResistanceAdjustment, ResistanceKind, inspect_prototype,
    };
    use crate::prototype::tests::armour_bytes;

    let fixture = Fixture::new();
    let path = "proto/008111 - armour.pro";
    fixture.write(
        &format!("base/{path}"),
        &armour_bytes(8111, 400, 2, 0x6000_0080),
    );
    fixture.write("base/mes/description.mes", b"{400}{Original Armour}");
    let mut vfs = Vfs::new(Policy::Ordered);
    vfs.mount_directory(fixture.0.join("base")).unwrap();
    let stock = inspect_prototype(&mut vfs, path).unwrap();
    let stock = stock.resolve_armour(&mut vfs).unwrap().unwrap();
    assert_eq!(stock.definition.sizes, [ArmourSize::Medium]);
    assert_eq!(stock.definition.coverage, ArmourCoverage::Torso);
    assert!(stock.name_source.mod_id.is_none());

    fixture.write(
        "mods/patch/arcanum-mod.toml",
        br#"
manifest_version = 1
id = "patch"
name = "Synthetic armour patch"
version = "1.0.0"
description = "Temporary test fixture."
[[mounts]]
kind = "directory"
path = "resources"
"#,
    );
    let mut replacement = armour_bytes(8111, 30025, 5, 0x6000_0080);
    // Replace the sparse base array with all five ordered channels.
    replacement.splice(
        283..319,
        [4i32, 5, 0, 16, 4, -20, 6, 0, 2, 0b11111, 0]
            .into_iter()
            .flat_map(i32::to_le_bytes),
    );
    fixture.write(&format!("mods/patch/resources/{path}"), &replacement);
    fixture.write(
        "mods/patch/resources/mes/gamedesc.mes",
        b"{30025}{Modified Armour}",
    );
    let catalog = Catalog::discover(fixture.0.join("mods")).unwrap();
    let mut vfs = catalog.load_order(&["patch"]).unwrap().apply(vfs).unwrap();
    let inspection = inspect_prototype(&mut vfs, path).unwrap();
    let resolved = inspection.resolve_armour(&mut vfs).unwrap().unwrap();
    assert_eq!(
        inspection.prototype.source.mod_id.unwrap().as_str(),
        "patch"
    );
    assert_eq!(resolved.name_source.mod_id.unwrap().as_str(), "patch");
    let armour = resolved.definition;
    assert_eq!(armour.id, stock.definition.id);
    assert_eq!(armour.id, ItemId("arcanum:item/prototype-8111".into()));
    assert_eq!(armour.display_name, "Modified Armour");
    assert_eq!(armour.sizes, [ArmourSize::Small, ArmourSize::Large]);
    assert_eq!(armour.coverage, stock.definition.coverage);
    assert_eq!(
        armour.resistances,
        [
            ResistanceAdjustment {
                kind: ResistanceKind::Physical,
                percentage_points: 16
            },
            ResistanceAdjustment {
                kind: ResistanceKind::Fire,
                percentage_points: 4,
            },
            ResistanceAdjustment {
                kind: ResistanceKind::Electrical,
                percentage_points: -20
            },
            ResistanceAdjustment {
                kind: ResistanceKind::Poison,
                percentage_points: 6,
            },
            ResistanceAdjustment {
                kind: ResistanceKind::Magic,
                percentage_points: 0
            },
        ]
    );
}

#[test]
fn armour_coverage_comes_from_item_art_and_rejects_unknown_encodings() {
    use crate::content::items::{ArmourCoverage, ItemDefinitionError, inspect_prototype};
    use crate::prototype::tests::armour_bytes;

    let fixture = Fixture::new();
    fixture.write("mes/description.mes", b"{400}{Armour}");
    for (path, art_id) in [
        ("proto/helmet.pro", 0x6000_8000),
        ("proto/non-item.pro", 0x5000_0000),
        ("proto/reserved.pro", 0x6001_c000),
    ] {
        fixture.write(path, &armour_bytes(8111, 400, 2, art_id));
    }
    let mut vfs = Vfs::new(Policy::Ordered);
    vfs.mount_directory(&fixture.0).unwrap();
    let helmet = inspect_prototype(&mut vfs, "proto/helmet.pro").unwrap();
    assert_eq!(
        helmet
            .resolve_armour(&mut vfs)
            .unwrap()
            .unwrap()
            .definition
            .coverage,
        ArmourCoverage::Helmet
    );

    for (path, art_id) in [
        ("proto/non-item.pro", 0x5000_0000),
        ("proto/reserved.pro", 0x6001_c000),
    ] {
        let unknown = inspect_prototype(&mut vfs, path).unwrap();
        assert!(
            matches!(unknown.resolve_armour(&mut vfs), Err(ItemDefinitionError::UnsupportedArmourArt(id)) if id == art_id)
        );
    }
}
