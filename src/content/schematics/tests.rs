use super::*;
use crate::mods::Catalog;
use crate::prototype::tests::{food_bytes, generic_bytes, key_bytes, weapon_bytes};
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
    let audit = audit::audit_all_schematics(&mut vfs).unwrap();
    assert_eq!(
        audit.rules_source.mod_id.as_ref().unwrap().as_str(),
        "override"
    );
    assert_eq!(
        audit.references[0].1.as_ref().unwrap().definition.as_ref(),
        Some(&definition)
    );
    let audited_item = &audit.references[0].1.as_ref().unwrap().items[&BasicPrototypeId(10100)];
    assert_eq!(
        audited_item
            .prototype
            .as_ref()
            .unwrap()
            .source
            .mod_id
            .as_ref()
            .unwrap()
            .as_str(),
        "override"
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
    use DiagnosticKind::*;
    for (kind, field, message) in [
        (MissingDescription, "description", "key 32"),
        (MissingIllustration, "illustration", "drawing.art"),
        (AmbiguousPrototype, "ingredient 1", "ambiguous"),
        (
            PrototypeIdentityMismatch,
            "ingredient 2",
            "declares ID 10199",
        ),
        (MissingPrototype, "product", "serialized 10122"),
        (MissingPrototype, "ingredient 2", "serialized 10125"),
        (MissingPrototype, "product", "serialized 10126"),
    ] {
        assert!(
            report
                .diagnostics
                .iter()
                .any(|d| d.kind == kind && d.field == field && d.message.contains(message)),
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

#[test]
fn readiness_reports_truncated_weapon_and_corrupt_artwork_from_the_winning_mod() {
    use super::readiness::{IssueKind, Status, audit_readiness};
    use flate2::{Compression, write::ZlibEncoder};
    use std::io::Write;

    let fixture = Fixture::new();
    fixture.populate();
    fs::remove_file(fixture.0.join("base/proto/010120 - food.pro")).unwrap();
    let weapon_path = "proto/010120 - weapon.pro";
    let weapon = weapon_bytes(10120, 400);
    fixture.write(&format!("base/{weapon_path}"), &weapon);
    let mut vfs = fixture.vfs();

    let baseline = audit_readiness(&mut vfs, &[]).unwrap();
    assert_eq!(baseline.statuses, [(GROUP, Status::Checked(Vec::new()))]);
    let base_report = baseline.references.references[0].1.as_ref().unwrap();
    assert!(base_report.diagnostics.is_empty());
    assert!(base_report.definition.is_some());

    fixture.write(
        "mods/broken/arcanum-mod.toml",
        r#"
manifest_version = 1
id = "broken"
name = "Broken schematic assets"
version = "1.0.0"
description = "Test VFS provenance"
[[mounts]]
kind = "directory"
path = "resources"
[[mounts]]
kind = "archive"
path = "art.dat"
"#,
    );
    fixture.write(
        &format!("mods/broken/resources/{weapon_path}"),
        &weapon[..143],
    );
    let mut compressor = ZlibEncoder::new(Vec::new(), Compression::default());
    compressor.write_all(b"drawing").unwrap();
    let mut archive = compressor.finish().unwrap();
    archive.push(b'!'); // Trailing byte inside the declared compressed span.
    let stored_size = archive.len() as u32;
    let index_start = archive.len();
    let path = b"art/interface/drawing.art\0";
    for word in [stored_size + 4, 1, path.len() as u32] {
        archive.extend(word.to_le_bytes());
    }
    archive.extend(path);
    for word in [0, 2, 7, stored_size, 0] {
        archive.extend(word.to_le_bytes());
    }
    archive.extend(b" TAD");
    archive.extend((path.len() as u32).to_le_bytes());
    archive.extend(((archive.len() - index_start) as u32).to_le_bytes());
    fixture.write("mods/broken/art.dat", archive);

    let catalog = Catalog::discover(fixture.0.join("mods")).unwrap();
    let mut vfs = catalog.load_order(&["broken"]).unwrap().apply(vfs).unwrap();
    let checked = audit_readiness(&mut vfs, &[GROUP]).unwrap();
    let report = checked.references.references[0].1.as_ref().unwrap();
    assert_eq!(report.definition, base_report.definition);
    assert!(report.diagnostics.is_empty());
    let [(id, Status::Checked(issues))] = checked.statuses.as_slice() else {
        panic!("expected a checked recipe");
    };
    assert_eq!(*id, GROUP);
    assert_eq!(issues.len(), 2);
    assert_eq!(issues[0].kind, IssueKind::ItemDefinition);
    assert_eq!(issues[0].source.virtual_path, weapon_path);
    assert!(issues[0].detail.contains("truncated"), "{issues:?}");
    assert_eq!(issues[1].kind, IssueKind::Illustration);
    assert_eq!(
        issues[1].source.virtual_path.as_bytes(),
        &path[..path.len() - 1]
    );
    assert!(
        issues[1].detail.contains("zlib stream ends before"),
        "{issues:?}"
    );
    for issue in issues {
        assert_eq!(issue.source.mod_id.as_ref().unwrap().as_str(), "broken");
    }
}

#[test]
fn item_name_validation_rejects_blanks_and_preserves_significant_spacing() {
    use super::readiness::{Status, audit_readiness};
    use crate::content::items::{ItemDefinitionError, WeaponAmmoDefinition, inspect_prototype};

    let fixture = Fixture::new();
    fixture.populate();
    fs::remove_file(fixture.0.join("base/proto/010120 - food.pro")).unwrap();
    let weapon_path = "proto/010120 - weapon.pro";
    fixture.write(&format!("base/{weapon_path}"), weapon_bytes(10120, 400));

    for (name, valid) in [("  \t  ", false), ("  First, Mk. II!  ", true)] {
        fixture.write(
            "base/mes/description.mes",
            format!("{{400}}{{{name}}}{{401}}{{Second}}"),
        );
        let mut vfs = fixture.vfs();
        let inspection = inspect_prototype(&mut vfs, weapon_path).unwrap();
        let standalone = inspection.resolve_weapon_ammo(&mut vfs);
        let audit = audit_readiness(&mut vfs, &[GROUP]).unwrap();
        let recipe = audit.references.references[0].1.as_ref().unwrap();
        let item = &recipe.items[&BasicPrototypeId(10100)];
        if valid {
            let WeaponAmmoDefinition::Weapon(definition) = standalone.unwrap().unwrap().definition
            else {
                panic!("expected a Weapon definition");
            };
            assert_eq!(definition.display_name, name);
            assert_eq!(item.name.as_ref().unwrap().text, name);
            assert_eq!(audit.statuses, [(GROUP, Status::Checked(Vec::new()))]);
        } else {
            assert!(matches!(
                standalone,
                Err(ItemDefinitionError::MissingName {
                    id: DescriptionId(400),
                    ..
                })
            ));
            assert!(item.name.is_none() && item.id.is_none());
            assert!(recipe.definition.is_none());
            assert!(recipe.diagnostics.iter().any(|diagnostic| {
                diagnostic.kind == DiagnosticKind::MissingDescription
                    && diagnostic.message.contains("blank item name at key 400")
            }));
            assert_eq!(audit.statuses, [(GROUP, Status::ReferencesIncomplete)]);
        }
    }
}

#[test]
fn key_readiness_checks_the_winning_body_and_name() {
    use super::readiness::{IssueKind, Status, audit_readiness};
    use crate::content::items::{ItemDefinitionError, KeyCode, inspect_prototype};

    let fixture = Fixture::new();
    fixture.populate();
    fs::remove_file(fixture.0.join("base/proto/010122 - food.pro")).unwrap();
    let path = "proto/010122 - key.pro";
    fixture.write(&format!("base/{path}"), key_bytes(10122, 30011, 0));
    let mut base = fixture.vfs();
    let inspection = inspect_prototype(&mut base, path).unwrap();
    assert_eq!(inspection.key.unwrap().key_id, 0);
    assert_eq!(
        inspection
            .resolve_key(&mut base)
            .unwrap()
            .unwrap()
            .definition
            .code,
        None
    );
    assert_eq!(
        audit_readiness(&mut base, &[GROUP]).unwrap().statuses,
        [(GROUP, Status::Checked(Vec::new()))]
    );

    fixture.write(
        "mods/key-test/arcanum-mod.toml",
        r#"
manifest_version = 1
id = "key-test"
name = "Key test"
version = "1.0.0"
description = "Fixture"
[[mounts]]
kind = "directory"
path = "resources"
"#,
    );
    let body = format!("mods/key-test/resources/{path}");
    let names = "mods/key-test/resources/mes/gamedesc.mes";
    let replacement = key_bytes(10122, 30012, 62);
    fixture.write(&body, &replacement);
    fixture.write(names, "{30012}{  Replacement key!  }");
    let modded_vfs = || {
        let catalog = Catalog::discover(fixture.0.join("mods")).unwrap();
        catalog
            .load_order(&["key-test"])
            .unwrap()
            .apply(fixture.vfs())
            .unwrap()
    };
    let mut vfs = modded_vfs();
    let inspection = inspect_prototype(&mut vfs, path).unwrap();
    let resolved = inspection.resolve_key(&mut vfs).unwrap().unwrap();
    assert_eq!(
        resolved.definition.id,
        ItemId("arcanum:item/prototype-10122".into())
    );
    assert_eq!(resolved.definition.display_name, "  Replacement key!  ");
    assert_eq!(
        resolved.definition.code,
        Some(KeyCode("arcanum:key-code/62".into()))
    );
    for source in [inspection.prototype.source, resolved.name_source] {
        assert_eq!(source.mod_id.unwrap().as_str(), "key-test");
    }
    assert_eq!(
        audit_readiness(&mut vfs, &[GROUP]).unwrap().statuses,
        [(GROUP, Status::Checked(Vec::new()))]
    );

    fixture.write(&body, &replacement[..266]);
    let audit = audit_readiness(&mut modded_vfs(), &[GROUP]).unwrap();
    let report = audit.references.references[0].1.as_ref().unwrap();
    assert!(report.definition.is_some());
    let [(_, Status::Checked(issues))] = audit.statuses.as_slice() else {
        panic!("expected readiness results");
    };
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].kind, IssueKind::ItemDefinition);
    assert!(
        issues[0]
            .detail
            .contains("prototype byte 266: truncated field")
    );
    assert_eq!(
        issues[0].source.mod_id.as_ref().unwrap().as_str(),
        "key-test"
    );

    fixture.write(&body, replacement);
    fixture.write(names, "{30012}{ \t }");
    let mut vfs = modded_vfs();
    let inspection = inspect_prototype(&mut vfs, path).unwrap();
    assert!(matches!(
        inspection.resolve_key(&mut vfs),
        Err(ItemDefinitionError::MissingName {
            id: DescriptionId(30012),
            ..
        })
    ));
    assert_eq!(
        audit_readiness(&mut vfs, &[GROUP]).unwrap().statuses,
        [(GROUP, Status::ReferencesIncomplete)]
    );
}

#[test]
fn generic_mod_uses_winning_definition_and_preserves_readiness_contract() {
    use super::readiness::{IssueKind, Status, audit_readiness};
    use crate::content::items::{GenericDefinition, ItemDefinitionError, inspect_prototype};

    let fixture = Fixture::new();
    fixture.populate();
    fs::remove_file(fixture.0.join("base/proto/010120 - food.pro")).unwrap();
    let path = "proto/010120 - generic.pro";
    fixture.write(&format!("base/{path}"), generic_bytes(10120, 400, 3, 0));
    fixture.write(
        "mods/generic-test/arcanum-mod.toml",
        r#"
manifest_version = 1
id = "generic-test"
name = "Generic test"
version = "1.0.0"
description = "Fixture"
[[mounts]]
kind = "directory"
path = "resources"
"#,
    );
    let mut base = fixture.vfs();
    let base_item = inspect_prototype(&mut base, path)
        .unwrap()
        .resolve_generic(&mut base)
        .unwrap()
        .unwrap();
    assert_eq!(base_item.definition.base_weight, 3);
    assert!(base_item.name_source.mod_id.is_none());
    let replacement_path = format!("mods/generic-test/resources/{path}");
    let replacement_names = "mods/generic-test/resources/mes/description.mes";
    let mut replacement = generic_bytes(10120, 405, 12, 1);
    replacement[216..220].copy_from_slice(&37i32.to_le_bytes());
    fixture.write(&replacement_path, &replacement);
    fixture.write(replacement_names, "{405}{  Coiled wire!  }{401}{Second}");

    let modded_vfs = || {
        let catalog = Catalog::discover(fixture.0.join("mods")).unwrap();
        catalog
            .load_order(&["generic-test"])
            .unwrap()
            .apply(fixture.vfs())
            .unwrap()
    };
    let mut vfs = modded_vfs();
    let inspection = inspect_prototype(&mut vfs, path).unwrap();
    assert_eq!(inspection.generic.unwrap().flags, 1);
    let resolved = inspection.resolve_generic(&mut vfs).unwrap().unwrap();
    assert_eq!(
        resolved.definition,
        GenericDefinition {
            id: base_item.definition.id,
            display_name: "  Coiled wire!  ".into(),
            base_weight: 12,
            base_worth: 42,
            magic_tech_complexity: 37,
        }
    );
    for source in [inspection.prototype.source, resolved.name_source] {
        assert_eq!(source.mod_id.unwrap().as_str(), "generic-test");
    }
    let audit = audit_readiness(&mut vfs, &[GROUP]).unwrap();
    assert_eq!(audit.statuses, [(GROUP, Status::Checked(Vec::new()))]);

    let mut invalid_worth = replacement.clone();
    invalid_worth[196..200].copy_from_slice(&(-1i32).to_le_bytes());
    for (body, message) in [
        (&replacement[..279], "truncated"),
        (invalid_worth.as_slice(), "base worth"),
    ] {
        fixture.write(&replacement_path, body);
        let audit = audit_readiness(&mut modded_vfs(), &[GROUP]).unwrap();
        assert!(
            audit.references.references[0]
                .1
                .as_ref()
                .unwrap()
                .definition
                .is_some()
        );
        let [(_, Status::Checked(issues))] = audit.statuses.as_slice() else {
            panic!("expected checked recipe");
        };
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].kind, IssueKind::ItemDefinition);
        assert!(issues[0].detail.contains(message), "{issues:?}");
        assert_eq!(
            issues[0].source.mod_id.as_ref().unwrap().as_str(),
            "generic-test"
        );
    }

    fixture.write(&replacement_path, replacement);
    fixture.write(replacement_names, "{405}{  \t  }{401}{Second}");
    let mut vfs = modded_vfs();
    let inspection = inspect_prototype(&mut vfs, path).unwrap();
    assert!(matches!(
        inspection.resolve_generic(&mut vfs),
        Err(ItemDefinitionError::MissingName {
            id: DescriptionId(405),
            ..
        })
    ));
    let audit = audit_readiness(&mut vfs, &[GROUP]).unwrap();
    assert_eq!(audit.statuses, [(GROUP, Status::ReferencesIncomplete)]);
}

#[test]
fn full_audit_classifies_independent_failures_in_recipe_order() {
    use DiagnosticKind::{InvalidPrototype, MissingDescription, MissingIllustration};

    let fixture = Fixture::new();
    fixture.populate();
    let rules = format!(
        "{{2023}}{{10100}}{RULE_DATA}
        {{4210}}{{31}}{{4211}}{{32}}{{4212}}{{34}}
        {{4213}}{{10103}}{{4214}}{{2147483647}}{{4215}}{{10104}}{{4216}}{{1}}"
    );
    fixture.write("base/rules/schematic.mes", rules);
    fixture.write("base/proto/010123 - food.pro", food_bytes(10123, 499, 8));
    fixture.write("base/proto/010124 - food.pro", [119, 0, 0, 0]);

    let mut vfs = fixture.vfs();
    let full = audit::audit_all_schematics(&mut vfs).unwrap();
    let ids = [LegacyRecipeId(4210), GROUP, LegacyRecipeId(2020), GROUP];
    let selected = audit::audit_schematics(&mut vfs, &ids).unwrap();
    assert_eq!(full.groups.len(), 3);
    assert_eq!(
        full.references
            .iter()
            .map(|(id, _)| id.0)
            .collect::<Vec<_>>(),
        [2020, 4200, 4210]
    );
    assert!(matches!(
        full.references[0].1,
        Err(GroupError::Missing(2020))
    ));
    assert!(full.references[1].1.as_ref().unwrap().definition.is_some());
    let incomplete = full.references[2].1.as_ref().unwrap();
    assert!(incomplete.definition.is_none());
    assert_eq!(
        incomplete.items[&BasicPrototypeId(10103)]
            .prototype
            .as_ref()
            .unwrap()
            .description,
        Some(DescriptionId(499))
    );
    assert_eq!(
        incomplete
            .diagnostics
            .iter()
            .map(|d| d.kind)
            .collect::<Vec<_>>(),
        [
            MissingIllustration,
            MissingDescription,
            InvalidPrototype,
            InvalidPrototype
        ]
    );
    assert!(
        incomplete.diagnostics[1]
            .message
            .contains("missing key 499")
    );
    assert!(incomplete.diagnostics[2].message.contains("truncated"));
    assert!(incomplete.diagnostics[3].message.contains("2147483647"));
    assert_eq!(
        format!("{:?}", full.references),
        format!("{:?}", selected.references)
    );
}
