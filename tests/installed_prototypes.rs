use arcanum_rs::content::{items::load_prototype, schematics::*};
use arcanum_rs::prototype::{DescriptionId, SerializedPrototypeId};
use arcanum_rs::vfs::{Policy, Storage, Vfs};
use std::path::PathBuf;

#[test]
#[ignore = "requires the installed Arcanum data"]
fn fortifier_and_sample_prototype_headers() {
    let game = PathBuf::from(std::env::var_os("ARCANUM_GAME_DIR").expect("set ARCANUM_GAME_DIR"));
    let mut vfs = Vfs::from_game(&game, Policy::Ce).unwrap();
    let report = load_schematic(&mut vfs, LegacyRecipeId(4220)).unwrap();
    assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
    assert_eq!(report.rules_source.storage, Storage::Loose);
    assert_eq!(report.title.as_ref().unwrap().text, "Fortifier");
    assert_eq!(report.legacy.description, SchematicTextId(565));
    assert_eq!(report.legacy.illustration, InterfaceArtId(696));
    let art = &report.illustration.as_ref().unwrap().asset_source;
    assert_eq!(art.virtual_path, "art/interface/s_fortifier.art");
    assert_eq!(art.mount_path.file_name().unwrap(), "Arcanum1.dat");
    assert_eq!(report.definition.as_ref().unwrap().quantity.get(), 1);
    assert_eq!(
        report.legacy.variants,
        [RecipeVariant {
            ingredients: [BasicPrototypeId(10093), BasicPrototypeId(10067)],
            product: BasicPrototypeId(10119),
        }]
    );

    for (item, lookup, serialized, weight, worth, complexity) in [
        (
            &report.items[&BasicPrototypeId(10093)],
            10093,
            10113,
            20,
            10,
            0,
        ),
        (
            &report.items[&BasicPrototypeId(10067)],
            10067,
            10087,
            5,
            2,
            -10,
        ),
        (
            &report.items[&BasicPrototypeId(10119)],
            10119,
            10139,
            10,
            20,
            -55,
        ),
    ] {
        assert_eq!(
            BasicPrototypeId(lookup).serialized(),
            Some(SerializedPrototypeId(serialized))
        );
        let prototype = item.prototype.as_ref().unwrap();
        assert_eq!(prototype.header.id, SerializedPrototypeId(serialized));
        assert!(prototype.header.is_food());
        let fields = prototype.food.unwrap();
        assert_eq!(fields.description, DescriptionId(lookup as i32));
        assert_eq!(
            (
                fields.weight,
                fields.worth,
                fields.discipline,
                fields.magic_tech_complexity
            ),
            (weight, worth, 1, complexity)
        );
    }
    assert_eq!(
        report.items[&BasicPrototypeId(10067)]
            .name
            .as_ref()
            .unwrap()
            .text,
        "Varham's Aqua Vitae"
    );

    let written = load_prototype(&mut vfs, "proto/014099 - written.pro").unwrap();
    assert_eq!(written.header.id, SerializedPrototypeId(14099));
    assert_eq!(written.header.object_type, 13);
    assert!(written.food.is_none());
}

#[test]
#[ignore = "requires the installed Arcanum data"]
fn healing_jacket_preserves_ordered_size_variants_and_provenance() {
    let game = PathBuf::from(std::env::var_os("ARCANUM_GAME_DIR").expect("set ARCANUM_GAME_DIR"));
    let mut vfs = Vfs::from_game(&game, Policy::Ce).unwrap();
    let id = LegacyRecipeId(2450);
    let report = load_schematic(&mut vfs, id).unwrap();
    assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
    assert_eq!(report.title.as_ref().unwrap().text, "Healing Jacket");
    assert_eq!(report.legacy.title, SchematicTextId(48));
    assert_eq!(report.legacy.description, SchematicTextId(49));
    assert_eq!(report.legacy.illustration, InterfaceArtId(533));
    let art = &report.illustration.as_ref().unwrap().asset_source;
    assert_eq!(art.virtual_path, "art/interface/healing_jacket_scheme.art");
    assert_eq!(art.mount_path.file_name().unwrap(), "Arcanum1.dat");

    let rules = vfs.resolve("rules/schematic.mes").unwrap();
    assert_eq!(rules.candidates().len(), 4);
    assert_eq!(report.rules_source.storage, Storage::Loose);
    assert_eq!(
        report.rules_source.mount_path,
        game.join("data").canonicalize().unwrap()
    );

    let definition = report.definition.as_ref().unwrap();
    assert_eq!(definition.quantity.get(), 1);
    let expected = [(8091, 8148), (8099, 8149), (8107, 8150)];
    assert_eq!(
        report.legacy.variants,
        expected.map(|(input, output)| RecipeVariant {
            ingredients: [BasicPrototypeId(15108), BasicPrototypeId(input)],
            product: BasicPrototypeId(output),
        })
    );
    assert_eq!(report.items.len(), 7);
    for (lookup, serialized, object_type, name) in [
        (15108, 15128, 14, "Electrical Harness"),
        (8091, 8111, 7, "Leather Armour"),
        (8099, 8119, 7, "Small Leather Armour"),
        (8107, 8127, 7, "Large Leather Armour"),
        (8148, 8168, 7, "Regenerative Jacket"),
        (8149, 8169, 7, "Small Regenerative Jacket"),
        (8150, 8170, 7, "Large Regenerative Jacket"),
    ] {
        let item = &report.items[&BasicPrototypeId(lookup)];
        let proto = item.prototype.as_ref().unwrap();
        assert_eq!(proto.header.id, SerializedPrototypeId(serialized));
        assert_eq!(proto.header.object_type, object_type);
        assert_eq!(proto.description, Some(DescriptionId(lookup as i32)));
        assert_eq!(proto.source.storage, Storage::Loose);
        assert!(item.definition.is_none());
        let label = item.name.as_ref().unwrap();
        assert_eq!(label.text, name);
        assert_eq!(label.source.mount_path.file_name().unwrap(), "Arcanum5.dat");
    }
    for (variant, (input, output)) in definition.variants.iter().zip(expected) {
        assert_eq!(
            variant.ingredients[0],
            *report.items[&BasicPrototypeId(15108)].id.as_ref().unwrap()
        );
        assert_eq!(
            variant.ingredients[1],
            *report.items[&BasicPrototypeId(input)].id.as_ref().unwrap()
        );
        assert_eq!(
            variant.product,
            *report.items[&BasicPrototypeId(output)].id.as_ref().unwrap()
        );
    }
    assert_eq!(
        load_schematic(&mut vfs, id).unwrap().definition.as_ref(),
        Some(definition)
    );
}

#[test]
#[ignore = "requires the installed Arcanum data"]
fn audits_learned_and_found_coverage_and_selected_references() {
    use arcanum_rs::content::schematics::audit::{GroupStatus, audit_schematics};
    use std::collections::BTreeMap;

    let game = PathBuf::from(std::env::var_os("ARCANUM_GAME_DIR").expect("set ARCANUM_GAME_DIR"));
    let mut vfs = Vfs::from_game(&game, Policy::Ce).unwrap();
    let cases = [
        (2450, [3, 3], 15108, [8091, 8099, 8107], [8148, 8149, 8150]),
        (3250, [3, 3], 15123, [8091, 8099, 8107], [8130, 8131, 8292]),
        (3260, [3, 3], 15155, [8130, 8131, 8292], [8115, 8123, 8128]),
        (4450, [1, 3], 15108, [8075, 8083, 8088], [8229, 8230, 8231]),
        (5090, [1, 3], 15177, [8115, 8123, 8128], [8073, 8081, 8291]),
    ];
    let ids = [
        LegacyRecipeId(4220),
        LegacyRecipeId(2450),
        LegacyRecipeId(3250),
    ];
    let audit = audit_schematics(&mut vfs, &ids).unwrap();
    assert_eq!(audit.rules_source.storage, Storage::Loose);
    assert!(audit.duplicate_keys.is_empty());
    assert_eq!(audit.groups.len(), 119);
    assert_eq!(
        audit
            .groups
            .iter()
            .filter(|group| group.id.0 < 4000)
            .count(),
        56
    );
    let mut quantities = BTreeMap::new();
    let mut variants = Vec::new();
    for group in &audit.groups {
        let GroupStatus::Supported(decoded) = &group.status else {
            panic!("unsupported installed group {group:?}");
        };
        let shape = group.shape.unwrap();
        assert!(shape.lists_align);
        assert!(!shape.repeated_ingredients[1]);
        *quantities.entry(shape.output_quantity.get()).or_insert(0) += 1;
        if decoded.variants.len() == 1 {
            assert_eq!(shape.ingredient_ids, [1, 1]);
            assert_eq!(shape.product_ids, 1);
        } else {
            variants.push((group.id.0, shape, &decoded.variants));
        }
    }
    assert_eq!(
        quantities,
        BTreeMap::from([(1, 94), (2, 3), (3, 15), (4, 2), (5, 5)])
    );
    assert_eq!(variants.len(), cases.len());
    for ((id, shape, variants), (expected_id, widths, first, second, products)) in
        variants.into_iter().zip(cases)
    {
        assert_eq!(id, expected_id);
        assert_eq!(shape.ingredient_ids, widths);
        assert_eq!(shape.repeated_ingredients, [widths[0] > 1, false]);
        assert_eq!(shape.product_ids, 3);
        assert_eq!(shape.output_quantity.get(), 1);
        assert_eq!(
            *variants,
            second
                .into_iter()
                .zip(products)
                .map(|(input, product)| RecipeVariant {
                    ingredients: [BasicPrototypeId(first), BasicPrototypeId(input)],
                    product: BasicPrototypeId(product),
                })
                .collect::<Vec<_>>()
        );
    }
    assert_eq!(audit.references.len(), ids.len());
    for (_, report) in &audit.references {
        let report = report.as_ref().unwrap();
        assert!(report.definition.is_some());
        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
    }
    let chainmail = audit.references[1].1.as_ref().unwrap();
    assert_eq!(chainmail.legacy.id, LegacyRecipeId(3250));
    assert_eq!(
        chainmail.title.as_ref().unwrap().text,
        "Feather-Weight Chainmail"
    );
    let art = &chainmail.illustration.as_ref().unwrap().asset_source;
    assert_eq!(
        art.virtual_path,
        "art/interface/ultralite_chain_mail_scheme.art"
    );
    assert_eq!(art.mount_path.file_name().unwrap(), "Arcanum1.dat");
    for (basic, serialized, expected_name) in [
        (15123, 15143, "Spool of Heavy Wire"),
        (8292, 8312, "Large Feather-Weight Chain"),
    ] {
        let item = &chainmail.items[&BasicPrototypeId(basic)];
        let prototype = item.prototype.as_ref().unwrap();
        assert_eq!(prototype.header.id, SerializedPrototypeId(serialized));
        assert_eq!(prototype.description, Some(DescriptionId(basic as i32)));
        assert_eq!(prototype.source.storage, Storage::Loose);
        let label = item.name.as_ref().unwrap();
        assert_eq!(label.text, expected_name);
        assert_eq!(label.source.mount_path.file_name().unwrap(), "Arcanum5.dat");
    }
}

#[test]
#[ignore = "requires the installed Arcanum data"]
fn validates_references_for_all_learned_and_found_groups() {
    use arcanum_rs::content::schematics::audit::audit_all_schematics;

    let game = PathBuf::from(std::env::var_os("ARCANUM_GAME_DIR").expect("set ARCANUM_GAME_DIR"));
    let mut vfs = Vfs::from_game(&game, Policy::Ce).unwrap();
    let audit = audit_all_schematics(&mut vfs).unwrap();
    assert_eq!(audit.groups.len(), 119);
    assert_eq!(audit.references.len(), 119);
    assert!(audit.duplicate_keys.is_empty());
    let mut variant_ids = Vec::new();
    for (id, report) in audit.references {
        let report = report.unwrap_or_else(|error| panic!("recipe {}: {error}", id.0));
        assert!(
            report.diagnostics.is_empty(),
            "recipe {}: {:?}",
            id.0,
            report.diagnostics
        );
        let definition = report
            .definition
            .unwrap_or_else(|| panic!("recipe {} has no definition", id.0));
        assert_eq!(definition.quantity, report.legacy.quantity);
        let item_id = |basic: BasicPrototypeId| {
            let entry = &report.items[&basic];
            let prototype = entry.prototype.as_ref().unwrap();
            assert_eq!(Some(prototype.header.id), basic.serialized());
            assert!(entry.name.is_some());
            entry.id.as_ref().unwrap().clone()
        };
        let expected: Vec<_> = report
            .legacy
            .variants
            .iter()
            .map(|legacy| RecipeVariant {
                ingredients: legacy.ingredients.map(item_id),
                product: item_id(legacy.product),
            })
            .collect();
        assert_eq!(definition.variants, expected);
        if definition.variants.len() > 1 {
            variant_ids.push(id.0);
        }
    }
    assert_eq!(variant_ids, [2450, 3250, 3260, 4450, 5090]);
}
