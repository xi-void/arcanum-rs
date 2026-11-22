use super::*;
use crate::mods::Catalog;
use crate::prototype::tests::food_bytes;
use crate::vfs::Policy;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

const GROUP: LegacyRecipeId = LegacyRecipeId(4200);
const VARIANT_DATA: &str = "{4200}{31}{4201}{32}{4202}{33}{4203}{10100 10100 10100}{4204}{10101 10103 10105}{4205}{10102 10104 10106}{4206}{2}";
const RULE_DATA: &str =
    "{4200}{31}\n{4201}{32}\n{4202}{33}\n{4203}{10100}\n{4204}{10101}\n{4205}{10102}\n{4206}{2}\n";

#[test]
fn decodes_one_group_and_maps_each_id_domain_explicitly() {
    let group = LegacyGroup::decode(GROUP, &Mes::parse_text(RULE_DATA).unwrap()).unwrap();
    assert_eq!(group.title, SchematicTextId(31));
    assert_eq!(group.description, SchematicTextId(32));
    assert_eq!(group.illustration, InterfaceArtId(33));
    let [variant] = group.variants.as_slice() else {
        panic!("expected one variant")
    };
    assert_eq!(
        variant.ingredients,
        [BasicPrototypeId(10100), BasicPrototypeId(10101)]
    );
    assert_eq!(
        variant.ingredients[0].serialized(),
        Some(SerializedPrototypeId(10120))
    );
    assert_eq!(variant.product, BasicPrototypeId(10102));
    assert_eq!(group.quantity.get(), 2);
    assert!(BasicPrototypeId(i32::MAX as u32).serialized().is_none());
    assert!(BasicPrototypeId(0).serialized().is_none());
}

#[test]
fn decodes_ordered_variants_for_one_fixed_ingredient() {
    let expected = [(10101, 10102), (10103, 10104), (10105, 10106)].map(|(ingredient, product)| {
        RecipeVariant {
            ingredients: [BasicPrototypeId(10100), BasicPrototypeId(ingredient)],
            product: BasicPrototypeId(product),
        }
    });
    for data in [
        VARIANT_DATA.to_owned(),
        VARIANT_DATA.replace("10100 10100 10100", "10100"),
    ] {
        let group = LegacyGroup::decode(GROUP, &Mes::parse_text(&data).unwrap()).unwrap();
        assert_eq!(group.variants, expected);
        assert_eq!(group.quantity.get(), 2);
    }
}

#[test]
fn rejects_unverified_or_ambiguous_variant_mappings() {
    for (old, new, key, reason) in [
        (
            "10100 10100 10100",
            "10100 10100 10107",
            4203,
            "first ingredient must be fixed",
        ),
        (
            "10100 10100 10100",
            "10100 10100",
            4203,
            "fixed ingredient repetitions must align with variants",
        ),
        (
            "10101 10103 10105",
            "10101 10101 10105",
            4204,
            "duplicate second-ingredient alternative",
        ),
        (
            "10102 10104 10106",
            "10102 10104 10106 10108",
            4205,
            "more than three items",
        ),
    ] {
        let data = VARIANT_DATA.replace(old, new);
        assert_eq!(
            LegacyGroup::decode(GROUP, &Mes::parse_text(&data).unwrap()),
            Err(GroupError::UnsupportedMapping { key, reason }),
        );
    }
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
            GroupError::UnsupportedMapping {
                key: 4205,
                reason: "each second-ingredient alternative needs one output",
            },
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
    let original = load_schematic(&mut fixture.vfs(), GROUP).unwrap();
    assert!(
        original.diagnostics.is_empty(),
        "{:?}",
        original.diagnostics
    );
    assert_eq!(
        original.items[&BasicPrototypeId(10100)]
            .name
            .as_ref()
            .unwrap()
            .text,
        "First"
    );
    assert_eq!(
        original.items[&BasicPrototypeId(10102)]
            .name
            .as_ref()
            .unwrap()
            .source
            .virtual_path,
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
        VARIANT_DATA.replace("{4206}{2}", "{4206}{3}"),
    );
    fixture.write(
        "mods/override/resources/proto/010120 - food.pro",
        food_bytes(10120, 401, 10),
    );
    for (id, description) in [(10123, 401), (10124, 400), (10125, 401), (10126, 401)] {
        fixture.write(
            &format!("base/proto/{id:06} - food.pro"),
            food_bytes(id, description, 5),
        );
    }
    let vfs = fixture.vfs();
    let catalog = Catalog::discover(fixture.0.join("mods")).unwrap();
    let mut vfs = catalog
        .load_order(&["override"])
        .unwrap()
        .apply(vfs)
        .unwrap();
    let report = load_schematic(&mut vfs, GROUP).unwrap();
    assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
    assert_eq!(report.rules_source.mod_id.unwrap().as_str(), "override");
    let ingredient = &report.items[&BasicPrototypeId(10100)];
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
    assert_eq!(definition.variants.len(), 3);
    assert_eq!(
        definition.variants[1].ingredients[1],
        ItemId("arcanum:item/prototype-10123".into())
    );
    assert_eq!(
        definition.variants[1].product,
        ItemId("arcanum:item/prototype-10124".into())
    );
    assert!(
        report.items[&BasicPrototypeId(10104)]
            .prototype
            .as_ref()
            .unwrap()
            .source
            .mod_id
            .is_none()
    );
    assert_eq!(
        definition.variants[0].ingredients[0],
        ItemId("arcanum:item/prototype-10120".into())
    );
    assert_eq!(
        load_schematic(&mut vfs, GROUP).unwrap().definition.unwrap(),
        definition
    );
    let audit = audit::audit_schematics(&mut vfs, &[GROUP]).unwrap();
    assert_eq!(
        audit.rules_source.mod_id.as_ref().unwrap().as_str(),
        "override"
    );
    assert_eq!(
        audit.references[0].1.as_ref().unwrap().definition.as_ref(),
        Some(&definition)
    );
}

#[test]
fn preserves_unresolved_ids_and_rejects_ambiguous_or_mismatched_prototypes() {
    let fixture = Fixture::new();
    fixture.populate();
    fixture.write("base/rules/schematic.mes", VARIANT_DATA);
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
    assert_eq!(report.legacy.variants[0].product, BasicPrototypeId(10102));
    assert_eq!(
        report.legacy.variants[2].ingredients[1],
        BasicPrototypeId(10105)
    );
    assert_eq!(report.legacy.variants[2].product, BasicPrototypeId(10106));
    assert_eq!(report.legacy.illustration, InterfaceArtId(33));
    assert!(report.definition.is_none());
    assert!(report.illustration.is_none());
    assert!(report.items.values().all(|item| item.definition.is_none()));
    for (field, message) in [
        ("description", "key 32"),
        ("illustration", "drawing.art"),
        ("ingredient 1", "ambiguous"),
        ("ingredient 2", "declares ID 10199"),
        ("product", "serialized 10122"),
        ("ingredient 2", "serialized 10125"),
        ("product", "serialized 10126"),
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

#[test]
fn audit_discovers_recipe_cells_and_reports_structure_in_id_order() {
    use audit::{GroupStatus, audit_groups};

    let variant = VARIANT_DATA
        .replace("420", "201")
        .replace("10100 10100 10100", "10100");
    let cases = [
        "{2023}{10100}",
        "{1999}{reserved}{2037}{unused}{6000}{title page}{7000}{factory}",
        RULE_DATA,
        &variant,
    ];
    let groups = audit_groups(&Mes::parse_text(&cases.concat()).unwrap());
    let reversed =
        audit_groups(&Mes::parse_text(&cases.into_iter().rev().collect::<String>()).unwrap());
    assert_eq!(groups, reversed);
    assert_eq!(
        groups.iter().map(|group| group.id.0).collect::<Vec<_>>(),
        [2010, 2020, 4200]
    );
    let shape = groups[0].shape.unwrap();
    assert_eq!(shape.ingredient_ids, [1, 3]);
    assert_eq!(shape.repeated_ingredients, [false, false]);
    assert_eq!(shape.product_ids, 3);
    assert_eq!(shape.output_quantity.get(), 2);
    assert!(shape.lists_align);
    let GroupStatus::Supported(group) = &groups[0].status else {
        panic!("expected ordered variants");
    };
    assert_eq!(
        group
            .variants
            .iter()
            .map(|variant| (variant.ingredients[1].0, variant.product.0))
            .collect::<Vec<_>>(),
        [(10101, 10102), (10103, 10104), (10105, 10106)]
    );
    assert_eq!(
        groups[1].status,
        GroupStatus::MalformedOrIncomplete(GroupError::Missing(2020))
    );
    assert!(groups[1].shape.is_none());
    assert!(matches!(groups[2].status, GroupStatus::Supported(_)));
}

#[test]
fn audit_separates_unsupported_shapes_from_bad_records() {
    use audit::{GroupStatus, audit_groups};

    let unsupported = VARIANT_DATA.replace("10102 10104 10106", "10102");
    let malformed = unsupported.replace("{4206}{2}", "{4206}{bad}");
    let overlong = VARIANT_DATA.replace("10102 10104 10106", "10102 10104 10106 10107");
    for (rules, is_unsupported) in [
        (&unsupported, true),
        (&malformed, false),
        (&overlong, false),
    ] {
        let groups = audit_groups(&Mes::parse_text(rules).unwrap());
        assert_eq!(groups.len(), 1);
        if is_unsupported {
            assert!(matches!(
                groups[0].status,
                GroupStatus::Unsupported(GroupError::UnsupportedMapping { key: 4205, .. })
            ));
            let shape = groups[0].shape.unwrap();
            assert!(!shape.lists_align);
            assert_eq!(shape.repeated_ingredients, [true, false]);
        } else {
            assert!(matches!(
                groups[0].status,
                GroupStatus::MalformedOrIncomplete(_)
            ));
        }
    }
}

#[test]
fn audit_checks_only_selected_references_and_reports_missing_groups() {
    let fixture = Fixture::new();
    fixture.write(
        "base/rules/schematic.mes",
        format!("{RULE_DATA}{{4206}}{{2}}"),
    );
    let mut vfs = fixture.vfs();
    let report = audit::audit_schematics(&mut vfs, &[]).unwrap();
    assert!(report.references.is_empty());
    assert_eq!(report.duplicate_keys, [4206]);
    assert!(matches!(
        report.groups[0].status,
        audit::GroupStatus::Supported(_)
    ));

    let absent = LegacyRecipeId(4190);
    let report = audit::audit_schematics(&mut vfs, &[GROUP, absent, GROUP]).unwrap();
    assert_eq!(report.references.len(), 2);
    assert_eq!(report.references[0].0, absent);
    assert!(matches!(
        report.references[0].1,
        Err(GroupError::Missing(4190))
    ));
    let selected = report.references[1].1.as_ref().unwrap();
    assert!(selected.definition.is_none());
    assert!(
        selected
            .diagnostics
            .iter()
            .any(|diag| diag.message.contains("serialized 10120"))
    );
}
