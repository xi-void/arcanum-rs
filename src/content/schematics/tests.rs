use super::*;
use crate::mods::Catalog;
use crate::prototype::tests::food_bytes;
use crate::vfs::Policy;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

const GROUP: LegacyRecipeId = LegacyRecipeId(4200);
const RULE_DATA: &str =
    "{4200}{31}\n{4201}{32}\n{4202}{33}\n{4203}{10100}\n{4204}{10101}\n{4205}{10102}\n{4206}{2}\n";

#[test]
fn decodes_one_group_and_maps_each_id_domain_explicitly() {
    let group = LegacyGroup::decode(GROUP, &Mes::parse_text(RULE_DATA).unwrap()).unwrap();
    assert_eq!(group.title, SchematicTextId(31));
    assert_eq!(group.description, SchematicTextId(32));
    assert_eq!(group.illustration, InterfaceArtId(33));
    assert_eq!(
        group.ingredients,
        [BasicPrototypeId(10100), BasicPrototypeId(10101)]
    );
    assert_eq!(
        group.ingredients[0].serialized(),
        Some(SerializedPrototypeId(10120))
    );
    assert_eq!(group.product, BasicPrototypeId(10102));
    assert_eq!(group.quantity.get(), 2);
    assert!(BasicPrototypeId(i32::MAX as u32).serialized().is_none());
    assert!(BasicPrototypeId(0).serialized().is_none());
}

#[test]
fn rejects_incomplete_malformed_and_unsupported_groups() {
    for (data, error) in [
        (
            RULE_DATA.replace("{4201}{32}\n", ""),
            GroupError::Missing(4201),
        ),
        (
            RULE_DATA.replace("{4206}{2}", "{4206}{0}"),
            GroupError::InvalidNumber(4206),
        ),
        (
            RULE_DATA.replace("{4204}{10101}", "{4204}{10101 10103}"),
            GroupError::Alternatives(4204),
        ),
        (
            RULE_DATA.replace("{4204}{10101}", "{4204}{10101 invalid}"),
            GroupError::InvalidNumber(4204),
        ),
    ] {
        assert_eq!(
            LegacyGroup::decode(GROUP, &Mes::parse_text(&data).unwrap()),
            Err(error)
        );
    }
    assert!(matches!(
        LegacyGroup::decode(LegacyRecipeId(6000), &Mes::parse_text(RULE_DATA).unwrap()),
        Err(GroupError::UnsupportedGroup(_))
    ));
}

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        loop {
            let dir = std::env::temp_dir().join(format!(
                "arcanum-schematic-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&dir) {
                Ok(()) => return Self(dir),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => panic!("{error}"),
            }
        }
    }

    fn write(&self, path: &str, bytes: impl AsRef<[u8]>) {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    fn populate(&self) {
        self.write("base/rules/schematic.mes", RULE_DATA);
        self.write(
            "base/mes/schematic_text.mes",
            "{31}{A recipe}{32}{Sample description}",
        );
        self.write("base/art/interface/interface.mes", "{33}{Drawing.ART}");
        self.write("base/art/interface/drawing.art", b"illustration presence");
        self.write("base/proto/010120 - food.pro", food_bytes(10120, 400, 8));
        self.write(
            "base/proto/010121 - named food.pro",
            food_bytes(10121, 401, 5),
        );
        self.write("base/proto/010122 - food.pro", food_bytes(10122, 30011, 4));
        self.write("base/mes/description.mes", "{400}{First}{401}{Second}");
        self.write("base/mes/gamedesc.mes", "{30011}{Result}");
    }

    fn vfs(&self) -> Vfs {
        let mut vfs = Vfs::new(Policy::Ordered);
        vfs.mount_directory(self.0.join("base")).unwrap();
        vfs
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn resolves_descriptions_by_field_and_tracks_mod_overrides() {
    let fixture = Fixture::new();
    fixture.populate();
    let mut vfs = fixture.vfs();
    let original = load_schematic(&mut vfs, GROUP).unwrap();
    assert!(
        original.diagnostics.is_empty(),
        "{:?}",
        original.diagnostics
    );
    assert_eq!(original.ingredients[0].name.as_ref().unwrap().text, "First");
    assert_eq!(
        original.product.name.as_ref().unwrap().source.virtual_path,
        "mes/gamedesc.mes"
    );
    assert!(original.rules_source.mod_id.is_none());

    fixture.write(
        "mods/override/arcanum-mod.toml",
        r#"
manifest_version = 1
id = "override"
name = "Synthetic recipe override"
version = "1.0.0"
description = "Fixture"
[[mounts]]
kind = "directory"
path = "resources"
"#,
    );
    fixture.write(
        "mods/override/resources/rules/schematic.mes",
        RULE_DATA.replace("{4206}{2}", "{4206}{3}"),
    );
    fixture.write(
        "mods/override/resources/proto/010120 - food.pro",
        food_bytes(10120, 401, 10),
    );
    let catalog = Catalog::discover(fixture.0.join("mods")).unwrap();
    let mut vfs = catalog
        .load_order(&["override"])
        .unwrap()
        .apply(vfs)
        .unwrap();
    let report = load_schematic(&mut vfs, GROUP).unwrap();
    assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
    assert_eq!(report.rules_source.mod_id.unwrap().as_str(), "override");
    let ingredient = &report.ingredients[0];
    let prototype = ingredient.prototype.as_ref().unwrap();
    assert_eq!(prototype.header.id, SerializedPrototypeId(10120));
    assert_eq!(prototype.food.unwrap().description, DescriptionId(401));
    assert_eq!(
        prototype.source.mod_id.as_ref().unwrap().as_str(),
        "override"
    );
    assert!(ingredient.name.as_ref().unwrap().source.mod_id.is_none());
    assert_eq!(ingredient.definition.as_ref().unwrap().base_weight, 10);
    assert_eq!(ingredient.name.as_ref().unwrap().text, "Second");
    let definition = report.definition.unwrap();
    assert_eq!(definition.quantity.get(), 3);
    assert_eq!(
        definition.ingredients[0],
        ItemId("arcanum:item/prototype-10120".into())
    );
    assert_eq!(
        load_schematic(&mut vfs, GROUP).unwrap().definition.unwrap(),
        definition
    );
}

#[test]
fn preserves_unresolved_ids_and_rejects_ambiguous_or_mismatched_prototypes() {
    let fixture = Fixture::new();
    fixture.populate();
    fixture.write(
        "base/proto/010120 - other food.pro",
        food_bytes(10120, 400, 8),
    );
    fixture.write(
        "base/proto/010121 - named food.pro",
        food_bytes(10199, 401, 5),
    );
    fs::remove_file(fixture.0.join("base/proto/010122 - food.pro")).unwrap();
    fs::remove_file(fixture.0.join("base/art/interface/drawing.art")).unwrap();
    fixture.write("base/mes/schematic_text.mes", "{31}{A recipe}");
    let report = load_schematic(&mut fixture.vfs(), GROUP).unwrap();
    assert_eq!(report.legacy.product, BasicPrototypeId(10102));
    assert_eq!(report.legacy.illustration, InterfaceArtId(33));
    assert!(report.definition.is_none());
    assert!(report.illustration.is_none());
    assert!(
        report
            .ingredients
            .iter()
            .all(|item| item.definition.is_none())
    );
    for (field, message) in [
        ("description", "key 32"),
        ("illustration", "drawing.art"),
        ("ingredient 1", "ambiguous"),
        ("ingredient 2", "declares ID 10199"),
        ("product", "serialized 10122"),
    ] {
        assert!(
            report
                .diagnostics
                .iter()
                .any(|d| d.field == field && d.message.contains(message)),
            "{:?}",
            report.diagnostics
        );
    }
}
