use super::*;

#[test]
fn native_key_definition_accepts_namespaced_identity_and_code() {
    use items::{ItemId, KeyCode, KeyDefinition};

    let key = KeyDefinition {
        id: ItemId("clockwork:item/brass-key".into()),
        display_name: "Brass key".into(),
        base_weight: 1,
        base_worth: 0,
        magic_tech_complexity: 0,
        code: Some(KeyCode("clockwork:key-code/workshop".into())),
    };
    assert_eq!(key.id.0, "clockwork:item/brass-key");
    assert_eq!(key.code.unwrap().0, "clockwork:key-code/workshop");
}

#[test]
fn native_generic_definition_accepts_a_namespaced_item_without_legacy_data() {
    use items::{GenericDefinition, ItemId};

    let component = GenericDefinition {
        id: ItemId("clockwork:item/brass-cog".into()),
        display_name: "Brass cog".into(),
        base_weight: 3,
        base_worth: 5,
        magic_tech_complexity: -12,
    };
    assert_eq!(component.id.0, "clockwork:item/brass-cog");
    assert_eq!(component.magic_tech_complexity, -12);
}
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
    assert_eq!(stock.definition.magic_tech_complexity, -7);
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
    replacement[216..220].copy_from_slice(&30i32.to_le_bytes());
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
    assert_eq!(armour.magic_tech_complexity, 30);
    assert_eq!(armour.armour_class_adjustment, 9);
    assert_eq!(armour.magical_armour_class_adjustment, 20);
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

#[test]
fn wield_reference_resolution_preserves_provenance_and_reports_ambiguity() {
    use crate::content::items::{ScriptReferenceError, WieldAttachment, inspect_prototype};
    use crate::prototype::{LegacyScriptId, tests::weapon_with_wield_scripts};

    let fixture = Fixture::new();
    fixture.write(
        "base/proto/sword.pro",
        &weapon_with_wield_scripts(28001, 28002),
    );
    fixture.write("base/scr/28001sword.scr", b"base script");
    fixture.write(
        "base/scr/subdir/28002sword.scr",
        b"not a direct script candidate",
    );
    fixture.write(
        "mods/patch/arcanum-mod.toml",
        br#"
manifest_version = 1
id = "patch"
name = "Wield scripts"
version = "1.0.0"
description = "Temporary test fixture."
[[mounts]]
kind = "directory"
path = "resources"
"#,
    );
    fixture.write("mods/patch/resources/scr/28001sword.scr", b"replacement");
    fixture.write("mods/patch/resources/scr/28002first.scr", b"first");
    fixture.write("mods/patch/resources/scr/28002second.scr", b"second");

    let mut vfs = Vfs::new(Policy::Ordered);
    let base = vfs.mount_directory(fixture.0.join("base")).unwrap();
    let inspection = inspect_prototype(&mut vfs, "proto/sword.pro").unwrap();
    let old = inspection.resolve_wield_scripts(&vfs);
    assert_eq!(old[0].resolution.as_ref().unwrap().source.mount_id, base);
    assert_eq!(
        old[1].resolution.as_ref().unwrap_err(),
        &ScriptReferenceError::Missing
    );

    let catalog = Catalog::discover(fixture.0.join("mods")).unwrap();
    let vfs = catalog.load_order(&["patch"]).unwrap().apply(vfs).unwrap();
    let reports = inspection.resolve_wield_scripts(&vfs);
    assert_eq!(reports[0].attachment, WieldAttachment::On);
    assert_eq!(reports[0].legacy_id, LegacyScriptId(28001));
    let on = reports[0].resolution.as_ref().unwrap();
    assert_eq!(on.reference.resource_path, "scr/28001sword.scr");
    assert_eq!(on.source.mod_id.as_ref().unwrap().as_str(), "patch");
    assert_eq!(reports[1].attachment, WieldAttachment::Off);
    assert_eq!(
        reports[1].resolution.as_ref().unwrap_err(),
        &ScriptReferenceError::Ambiguous(vec![
            "scr/28002first.scr".into(),
            "scr/28002second.scr".into()
        ])
    );
}

#[test]
fn effect_import_uses_mod_precedence_and_keeps_its_namespaced_id() {
    use super::effects::{EffectId, EffectOperation, LegacyEffectId, load_effect};
    use super::items::ResistanceKind;

    let fixture = Fixture::new();
    fixture.write(
        "base/rules/effect.mes",
        b"{128}{ac +1}\n{54}{resistfire min 40, resistelectrical +7}",
    );
    fixture.write(
        "mods/patch/arcanum-mod.toml",
        br#"
manifest_version = 1
id = "patch"
name = "Effect adjustment"
version = "1.0.0"
description = "Temporary test fixture."
[[mounts]]
kind = "directory"
path = "resources"
"#,
    );
    fixture.write(
        "mods/patch/resources/Rules/Effect.mes",
        b"{128}{ac -2}\n{128}{ac +3}\n{54}{resistelectrical -2, resistpoison min 0}",
    );

    let mut vfs = Vfs::new(Policy::Ordered);
    let base = vfs.mount_directory(fixture.0.join("base")).unwrap();
    let legacy_id = LegacyEffectId(128);
    let original = load_effect(&mut vfs, legacy_id).unwrap();
    assert_eq!(original.source.mount_id, base);
    assert_eq!(
        original.definition.operations,
        [EffectOperation::AddArmourClass { points: 1 }]
    );

    let catalog = Catalog::discover(fixture.0.join("mods")).unwrap();
    let mut vfs = catalog.load_order(&["patch"]).unwrap().apply(vfs).unwrap();
    let replacement = load_effect(&mut vfs, legacy_id).unwrap();
    assert_eq!(replacement.legacy_id, legacy_id);
    assert_eq!(
        replacement.definition.id,
        EffectId("arcanum:effect/128".into())
    );
    assert_eq!(replacement.definition.id, original.definition.id);
    assert_eq!(
        replacement.definition.operations,
        [EffectOperation::AddArmourClass { points: 3 }]
    );
    assert_eq!(replacement.record, "ac +3");
    assert_eq!(replacement.source.mod_id.unwrap().as_str(), "patch");
    assert_eq!(replacement.source.virtual_path, "rules/effect.mes");

    let composite = load_effect(&mut vfs, LegacyEffectId(54)).unwrap();
    assert_eq!(
        composite.definition.id,
        EffectId("arcanum:effect/54".into())
    );
    assert_eq!(composite.source.mod_id.unwrap().as_str(), "patch");
    assert_eq!(composite.record, "resistelectrical -2, resistpoison min 0");
    assert_eq!(
        composite.definition.operations,
        [
            EffectOperation::AddResistance {
                channel: ResistanceKind::Electrical,
                percentage_points: -2
            },
            EffectOperation::ResistanceFloor {
                channel: ResistanceKind::Poison,
                minimum_percent: 0
            },
        ]
    );
}

#[test]
fn effect_import_distinguishes_missing_malformed_and_unsupported_records() {
    use super::effects::{ClauseError, DecodeError, LegacyEffectId, LoadEffectError, load_effect};

    let fixture = Fixture::new();
    fixture.write(
        "rules/effect.mes",
        b"{53}{resistfire min}\n{128}{ac +1%}\n{54}{resistfire min 95, speed +2}",
    );
    let mut vfs = Vfs::new(Policy::Ordered);
    let mount = vfs.mount_directory(&fixture.0).unwrap();
    assert!(matches!(
        load_effect(&mut vfs, LegacyEffectId(-1)),
        Err(LoadEffectError::InvalidId(LegacyEffectId(-1)))
    ));
    assert!(
        matches!(load_effect(&mut vfs, LegacyEffectId(0)), Err(LoadEffectError::Missing { id: LegacyEffectId(0), source }) if source.mount_id == mount)
    );
    for (id, index, kind) in [
        (53, 1, ClauseError::MissingValue),
        (128, 1, ClauseError::UnsupportedOperation),
        (54, 2, ClauseError::UnsupportedTarget),
    ] {
        assert!(
            matches!(load_effect(&mut vfs, LegacyEffectId(id)), Err(LoadEffectError::Decode { id: LegacyEffectId(key), source, error }) if key == id && source.mount_id == mount && error == DecodeError::Clause { index, kind })
        );
    }

    for (text, kind) in [
        (
            b"{53}{resistfire min 50".as_slice(),
            mes::ErrorKind::UnterminatedField,
        ),
        (
            b"{53}{resistfire min 50} // \x81".as_slice(),
            mes::ErrorKind::UndefinedWindows1252(0x81),
        ),
    ] {
        fixture.write("rules/effect.mes", text);
        let mut vfs = Vfs::new(Policy::Ordered);
        let mount = vfs.mount_directory(&fixture.0).unwrap();
        assert!(
            matches!(load_effect(&mut vfs, LegacyEffectId(53)), Err(LoadEffectError::Resource(Error::Mes { provenance, error })) if provenance.mount_id == mount && error.kind == kind)
        );
    }
}

#[test]
fn art_load_uses_mod_precedence_and_reports_the_invalid_winner() {
    fn one_pixel(red: u8) -> Vec<u8> {
        let mut bytes = vec![0; 1185];
        for (offset, value) in [
            (0, 1u32),
            (4, 8),
            (8, 8),
            (12, 1),
            (32, 1),
            (1156, 1),
            (1160, 1),
            (1164, 1),
        ] {
            bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        bytes[138] = red;
        bytes[1184] = 1;
        bytes
    }

    let fixture = Fixture::new();
    fixture.write("base/art/marker.art", &one_pixel(50));
    let mut vfs = Vfs::new(Policy::Ordered);
    let base_id = vfs.mount_directory(fixture.0.join("base")).unwrap();
    let base = art::load_frame(&mut vfs, "art/marker.art", 0).unwrap();
    assert_eq!(base.source.mount_id, base_id);
    assert_eq!(base.decoded.frame.pixels, [[50, 0, 0, 255]]);

    fixture.write(
        "mods/patch/arcanum-mod.toml",
        br#"
manifest_version = 1
id = "patch"
name = "Synthetic ART patch"
version = "1.0.0"
description = "Temporary test fixture."
[[mounts]]
kind = "directory"
path = "resources"
"#,
    );
    let path = fixture.write("mods/patch/resources/Art/Marker.art", &one_pixel(120));
    let catalog = Catalog::discover(fixture.0.join("mods")).unwrap();
    let mut vfs = catalog.load_order(&["patch"]).unwrap().apply(vfs).unwrap();
    let loaded = art::load_frame(&mut vfs, r"Art\MARKER.ART", 0).unwrap();
    assert_eq!(loaded.decoded.frame.pixels, [[120, 0, 0, 255]]);
    assert_eq!(loaded.source.virtual_path, "art/marker.art");
    assert_eq!(loaded.source.mod_id.as_ref().unwrap().as_str(), "patch");
    assert_eq!(
        loaded.source.loose_path.as_deref(),
        Some(Path::new("Art/Marker.art"))
    );

    fs::write(path, b"bad").unwrap();
    assert!(matches!(
        art::load_frame(&mut vfs, "art/marker.art", 0),
        Err(Error::Art { provenance, error: crate::art::Error::Truncated { section: "header", .. } }) if *provenance == loaded.source
    ));
}

#[test]
fn sector_draws_share_artwork_while_preserving_raw_ids_and_flip() {
    use crate::sector::{Coordinates, TileArtId};

    let files = Fixture::new();
    let mut sector_bytes = vec![0; 4 + 4096 * 4];
    for (x, raw) in [(0, 0x000131c0u32), (1, 0x000131c1)] {
        let offset = 4 + x * 4;
        sector_bytes[offset..offset + 4].copy_from_slice(&raw.to_le_bytes());
    }
    files.write("art/tile/tilename.mes", b"{0}{drt/n 0}{1}{grs/n 0}");
    let mut artwork = vec![0; 1184 + 78 * 40];
    for (offset, value) in [
        (0, 1u32),
        (4, 8),
        (8, 8),
        (12, 1),
        (32, 1),
        (1156, 78),
        (1160, 40),
        (1164, 3120),
        (1168, 39),
        (1172, 39),
    ] {
        artwork[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    artwork[138] = 35;
    artwork[142] = 90;
    artwork[1184] = 1;
    artwork[1184 + 77] = 2;
    files.write("art/tile/drtgrs4a.art", &artwork);
    // Fill the second row with the same verified IDs to make this a 2 × 2 region.
    sector_bytes.copy_within(4..12, 4 + 64 * 4);
    files.write("maps/demo/0.sec", &sector_bytes);

    let mut vfs = Vfs::new(Policy::Ordered);
    vfs.mount_directory(&files.0).unwrap();
    let patch = super::sector::load_region(&mut vfs, "demo", Coordinates { x: 0, y: 0 }, (0, 0), 2)
        .unwrap();
    assert_eq!(patch.artwork().count(), 1);
    assert_eq!(patch.flipped_frame_count(), 1);
    assert_eq!(patch.tiles()[0].id, TileArtId(0x000131c0));
    assert_eq!(patch.tiles()[1].id, TileArtId(0x000131c1));
    assert_eq!(patch.tiles()[0].artwork, patch.tiles()[1].artwork);
    let draws: Vec<_> = patch.draws().collect();
    assert_eq!(draws[0].frame.pixels[0], [35, 0, 0, 255]);
    assert_eq!(draws[1].frame.pixels[0], [90, 0, 0, 255]);
    assert_eq!(draws[1].anchor.0 - draws[0].anchor.0, -40);
}

#[test]
fn sector_facades_select_frames_and_share_the_floor_draw_order() {
    use crate::compositor::{SpriteDraw, blit_ordered};
    use crate::sector::Coordinates;

    let files = Fixture::new();
    let mut sector_bytes = vec![0; 4 + 4096 * 4];
    for (index, raw) in [(1, 0xb402_0803u32), (64, 0xb402_0800)] {
        let offset = 4 + index * 4;
        sector_bytes[offset..offset + 4].copy_from_slice(&raw.to_le_bytes());
    }
    files.write("maps/demo/0.sec", &sector_bytes);
    files.write("art/tile/tilename.mes", b"{300}{abc}");
    files.write("art/facade/facadename.mes", b"{0}{Rug}{7}{Rug-Flip}");
    let framed = |flags: u32, colors: &[u8]| {
        let mut bytes = vec![0; 1156 + 28 * colors.len()];
        for (offset, value) in [(0, flags), (8, 8), (12, 1), (32, colors.len() as u32)] {
            bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        bytes[138] = 40;
        bytes[142] = 90;
        for (index, &color) in colors.iter().enumerate() {
            let start = 1156 + index * 28;
            for (word, value) in [78u32, 40, 3120, 39, 39].into_iter().enumerate() {
                let offset = start + word * 4;
                bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            }
            // The facade's transparent left edge detects underdraw and double flipping.
            for _ in 0..40 {
                bytes.push(if flags == 9 { 0 } else { color });
                bytes.extend([color; 77]);
            }
        }
        bytes
    };
    files.write("art/tile/abcbse0a.art", &framed(1, &[1]));
    let rug = framed(9, &[1, 2]);
    let rug_path = files.write("art/facade/rug-flip.art", &rug);
    let mut vfs = Vfs::new(Policy::Ordered);
    let mount = vfs.mount_directory(&files.0).unwrap();
    let coords = Coordinates { x: 0, y: 0 };
    let load = |vfs: &mut Vfs| super::sector::load_facade_region(vfs, "demo", coords, (0, 0), 2);
    let patch = load(&mut vfs).unwrap();
    assert_eq!(patch.facade_names_source.as_ref().unwrap().mount_id, mount);
    assert_eq!(patch.artwork().count(), 3);
    assert_eq!(patch.flipped_frame_count(), 0);
    assert_eq!(
        patch
            .tiles()
            .iter()
            .map(|tile| (tile.world, tile.id.0, tile.frame_index()))
            .collect::<Vec<_>>(),
        [
            ((0, 0), 0, 0),
            ((1, 0), 0xb402_0803, 1),
            ((0, 1), 0xb402_0800, 0),
            ((1, 1), 0, 0)
        ]
    );
    let draws: Vec<_> = patch.draws().collect();
    assert_eq!(draws[1].anchor, (-40, -1));
    assert_eq!(draws[1].frame.pixels[0], [0; 4]);
    assert_eq!(draws[1].frame.pixels[77], [90, 0, 0, 255]);
    let mut clipped = [[0; 3]; 2];
    blit_ordered(
        &mut clipped,
        (2, 1),
        2,
        draws.iter().map(|draw| SpriteDraw {
            frame: draw.frame,
            anchor: (draw.anchor.0 + 5, draw.anchor.1 + 25),
        }),
    )
    .unwrap();
    assert_eq!(clipped, [[90, 0, 0]; 2]);
    let background = [12, 34, 56];
    let mut edge = [background; 2];
    blit_ordered(
        &mut edge,
        (2, 1),
        2,
        draws.iter().map(|draw| SpriteDraw {
            frame: draw.frame,
            anchor: (draw.anchor.0 + 79, draw.anchor.1 + 35),
        }),
    )
    .unwrap();
    assert_eq!(edge, [background, [90, 0, 0]]);
    assert!(super::sector::load_region(&mut vfs, "demo", coords, (0, 0), 2).is_err());

    fs::write(rug_path, &rug[..rug.len() - 1]).unwrap();
    let error = load(&mut vfs).err().unwrap().to_string();
    assert!(
        error.contains("art/facade/rug-flip.art") && error.contains("truncated ART frame payload")
    );
}

#[test]
fn selected_wall_instances_keep_depth_order_and_reuse_oriented_frames() {
    use crate::sector::Coordinates;

    let fixture = Fixture::new();
    let mut sector = vec![0; 4 + 64 * 64 * 4];
    sector.extend(1u32.to_le_bytes());
    sector.extend(0xaa0000u32.to_le_bytes());
    let mut records = Vec::new();
    for (art_id, x, offset, scale) in [
        (0x1000_3811u32, 8u32, None, 100i32),
        (0x1000_0800, 7, Some(-3i32), 100),
        (0x1000_3811, 7, None, 100),
        (0x1000_0800, 7, None, 80),
    ] {
        let mut header = [0; 70];
        header[0] = 119;
        header[4] = 1;
        header[12] = 20;
        header[56] = 3 + u8::from(offset.is_some());
        header[58..62]
            .copy_from_slice(&(0x803u32 | (u32::from(offset.is_some()) << 2)).to_le_bytes());
        records.push(sector.len());
        sector.extend(header);
        sector.extend(art_id.to_le_bytes());
        sector.push(1);
        sector.extend(x.to_le_bytes());
        sector.extend(10u32.to_le_bytes());
        if let Some(offset) = offset {
            sector.extend(offset.to_le_bytes());
        }
        sector.extend(scale.to_le_bytes());
    }
    sector.extend(4u32.to_le_bytes());
    let sector_path = fixture.write("maps/synthetic/0.sec", &sector);

    let mut prototype = vec![0; 68];
    prototype[0] = 119;
    prototype[4..6].copy_from_slice(&(-1i16).to_le_bytes());
    prototype[28] = 1;
    prototype[36] = 20;
    for slot in (0..36).chain(64..69) {
        match slot {
            1 | 5..=7 | 15..=17 | 30 | 31 | 34 | 35 | 67 | 68 => prototype.push(0),
            _ => {
                let value = match slot {
                    2 => 0u32,
                    3 => 2,
                    9 => 0x00ff_ffff,
                    10 => 255,
                    11 => 100,
                    _ => 0,
                };
                prototype.extend(value.to_le_bytes());
            }
        }
    }
    fixture.write("proto/000020 - wall.pro", &prototype);
    fixture.write("art/wall/structure.mes", b"{0}{In40 Gb11 wd1 gb1}");
    fixture.write("art/wall/wallname.mes", b"{0}{In40}");
    let mut artwork = vec![0; 132 + 2 * 1024 + 28];
    for (offset, value) in [
        (0, 1u32),
        (8, 8),
        (12, 1),
        (16, 1),
        (32, 1),
        (2180, 3),
        (2184, 1),
        (2188, 3),
        (2192, 39),
        (2196, 132),
    ] {
        artwork[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    artwork[138] = 10;
    artwork[1162] = 20;
    artwork.extend([1, 0, 0]);
    fixture.write("art/wall/in4bseu0.art", &artwork);

    let mut vfs = Vfs::new(Policy::Ordered);
    vfs.mount_directory(&fixture.0).unwrap();
    let coords = Coordinates { x: 0, y: 0 };
    let many = |vfs: &mut Vfs, indices| super::wall::load_many(vfs, "synthetic", coords, indices);
    let loaded = many(&mut vfs, &[2, 0, 1]).unwrap();
    assert_eq!(
        loaded
            .iter()
            .map(|wall| wall.record.index)
            .collect::<Vec<_>>(),
        [1, 2, 0]
    );
    assert_eq!(loaded[0].placement.pixel_offset, (-3, 2));
    assert_eq!(loaded[1].placement.pixel_offset, (0, 2));
    assert_eq!(loaded[0].scale, 100);
    assert!(std::sync::Arc::ptr_eq(
        &loaded[1].artwork,
        &loaded[2].artwork
    ));
    assert_eq!(loaded[0].artwork.decoded.frame.pixels[0], [10, 0, 0, 255]);
    assert_eq!(loaded[1].artwork.decoded.frame.pixels[2], [20, 0, 0, 255]);
    assert_eq!(loaded[1].artwork.decoded.frame.hotspot, (39, 132));
    let mut overlap = [[1, 2, 3]; 3];
    crate::compositor::blit_ordered(
        &mut overlap,
        (3, 1),
        3,
        loaded[..2].iter().map(|wall| {
            let mut draw = wall.draw((7, 10)).unwrap();
            draw.anchor.0 += 2;
            draw.anchor.1 += 150;
            draw
        }),
    )
    .unwrap();
    assert_eq!(overlap, [[10, 0, 0], [1, 2, 3], [20, 0, 0]]);
    let error = |vfs: &mut Vfs, indices| many(vfs, indices).err().unwrap().to_string();
    assert!(error(&mut vfs, &[3]).contains("scale 80"));
    assert!(error(&mut vfs, &[1, 1]).contains("duplicate"));
    assert!(error(&mut vfs, &[4]).contains("does not refer to walls"));
    sector[records[3] + 74] = 6;
    fs::write(sector_path, sector).unwrap();
    assert!(error(&mut vfs, &[0, 1]).contains("invalid optional field marker"));
}

#[test]
fn native_scenery_reuses_vfs_frame_and_clips_at_shared_anchor() {
    use crate::{
        compositor::{blit_ordered, sprite_origin},
        prototype::SerializedPrototypeId,
        sector::{
            Coordinates,
            objects::{AppearanceFields, StaticCategory, StaticRecord},
        },
    };
    use std::sync::Arc;

    let fixture = Fixture::new();
    fixture.write("art/scenery/scenery.mes", b"{14036}{Prop.ART}");
    let mut prototype = vec![0; 68];
    prototype[..4].copy_from_slice(&119u32.to_le_bytes());
    prototype[4..6].fill(255);
    prototype[28] = 1;
    prototype[36..40].copy_from_slice(&4039u32.to_le_bytes());
    prototype[52] = 3;
    for index in (0..36).chain(64..70) {
        if matches!(index, 1 | 5..=7 | 15..=17 | 30 | 31 | 34 | 35 | 65 | 68 | 69) {
            prototype.push(0);
        } else {
            let value: i32 = match index {
                2 => -1,
                3 => 2,
                9 => 0xff_ffff,
                10 => 255,
                11 => 100,
                _ => 0,
            };
            prototype.extend(value.to_le_bytes());
        }
    }
    fixture.write("proto/004039 - scenery.pro", &prototype);
    let mut art = vec![0; 132 + 1024 + 2 * 28];
    for (offset, word) in [
        (0, 1u32),
        (8, 8),
        (12, 1),
        (32, 2),
        (1156, 1),
        (1160, 1),
        (1164, 1),
        (1184, 3),
        (1188, 1),
        (1192, 3),
        (1196, 1),
        (1204, 7),
    ] {
        art[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
    }
    art[136..139].copy_from_slice(&[3, 2, 1]);
    art.extend([0, 1, 1, 0]);
    fixture.write("art/scenery/prop.art", &art);

    let mut vfs = Vfs::new(Policy::Ordered);
    let mount = vfs.mount_directory(&fixture.0).unwrap();
    let make_record = |index, offset_y| StaticRecord {
        category: StaticCategory::Scenery,
        index,
        span: 0..0,
        prototype: SerializedPrototypeId(4039),
        instance_id: [0; 24],
        art_id: 0x4120_4380,
        stored_tile: (1, 1),
        fields: AppearanceFields {
            offset_y,
            ..AppearanceFields::default()
        },
    };
    let scenery = scenery::resolve_scenery(
        &mut vfs,
        Coordinates { x: 0, y: 0 },
        vec![make_record(1, Some(0)), make_record(2, None)],
        &[2, 1],
    )
    .unwrap();
    let selected = &scenery[0];
    assert_eq!(selected.record.index, 1);
    assert_eq!(selected.appearance.offset, (-1, 0));
    assert_eq!(scenery[1].appearance.offset, (-1, 2));
    assert_eq!(selected.artwork.source.mount_id, mount);
    assert!(Arc::ptr_eq(&selected.artwork, &scenery[1].artwork));
    assert_eq!(selected.art.frame, 1);

    let mut draw = selected.draw((1, 1)).unwrap();
    assert_eq!(sprite_origin(draw.frame, draw.anchor), (-2, 0));
    assert_eq!(draw.frame.offset, (7, 0));
    let mut surface = [[20, 30, 40]; 3];
    draw.anchor.0 += 1;
    blit_ordered(&mut surface, (3, 1), 3, [draw]).unwrap();
    assert_eq!(surface, [[1, 2, 3], [20, 30, 40], [20, 30, 40]]);
}
