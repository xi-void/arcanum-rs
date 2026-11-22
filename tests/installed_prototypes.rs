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

#[test]
#[ignore = "requires the installed Arcanum data"]
fn readiness_checks_all_supported_schematic_item_and_artwork_resources() {
    use arcanum_rs::content::schematics::readiness::{IssueKind, Status, audit_readiness};
    use std::collections::BTreeMap;

    let game = PathBuf::from(std::env::var_os("ARCANUM_GAME_DIR").expect("set ARCANUM_GAME_DIR"));
    let mut vfs = Vfs::from_game(game, Policy::Ce).unwrap();
    let audit = audit_readiness(&mut vfs, &[]).unwrap();
    assert_eq!(audit.statuses.len(), 119);
    let mut passed = 0;
    let mut counts = BTreeMap::new();
    let mut newly_passed = Vec::new();
    for ((id, status), (reference_id, report)) in
        audit.statuses.into_iter().zip(audit.references.references)
    {
        assert_eq!(id, reference_id);
        let Status::Checked(issues) = status else {
            panic!("recipe {} failed reference resolution", id.0);
        };
        if issues.is_empty() {
            passed += 1;
            if report
                .unwrap()
                .items
                .values()
                .any(|item| item.prototype.as_ref().unwrap().header.is_generic())
            {
                newly_passed.push(id.0);
            }
        } else {
            assert_eq!(id, LegacyRecipeId(5280));
        }
        for issue in issues {
            assert!(
                issue.kind == IssueKind::UnsupportedItemType(11),
                "recipe {}: {issue:?}",
                id.0
            );
            *counts.entry(issue.kind).or_insert(0) += 1;
        }
    }
    assert_eq!(passed, 118);
    assert_eq!(
        counts,
        BTreeMap::from([(IssueKind::UnsupportedItemType(11), 1)])
    );
    assert_eq!(newly_passed.len(), 80);
    for id in [2210, 2400, 2450] {
        assert!(newly_passed.contains(&id));
    }
}

#[test]
#[ignore = "requires the installed Arcanum data"]
fn installed_generic_ingredients_and_product_match_verified_prototypes() {
    use arcanum_rs::content::items::{GenericDefinition, ItemId, inspect_prototype};

    let game = PathBuf::from(std::env::var_os("ARCANUM_GAME_DIR").expect("set ARCANUM_GAME_DIR"));
    let mut vfs = Vfs::from_game(&game, Policy::Ce).unwrap();
    for (recipe, basic, serialized, name, weight, worth, complexity, flags, product) in [
        (2210, 15069, 15089, "Metal Plates", 10, 2, -5, 0, false),
        (
            2450,
            15108,
            15128,
            "Electrical Harness",
            30,
            50,
            -70,
            0,
            false,
        ),
        (2400, 15070, 15090, "Electric Light", 50, 100, -20, 1, true),
    ] {
        let report = load_schematic(&mut vfs, LegacyRecipeId(recipe)).unwrap();
        assert!(report.legacy.variants.iter().all(|variant| {
            let basic = BasicPrototypeId(basic);
            if product {
                variant.product == basic
            } else {
                variant.ingredients.contains(&basic)
            }
        }));
        let path = format!("proto/{serialized:06} - generic.pro");
        let inspection = inspect_prototype(&mut vfs, &path).unwrap();
        assert_eq!(
            inspection.prototype.header.id,
            SerializedPrototypeId(serialized)
        );
        assert_eq!(inspection.prototype.header.object_type, 14);
        assert_eq!(inspection.prototype.source.storage, Storage::Loose);
        assert_eq!(
            inspection.prototype.source.mount_path,
            game.join("data").canonicalize().unwrap()
        );
        let fields = inspection.generic.unwrap();
        assert_eq!(fields.item.description, DescriptionId(basic as i32));
        assert_eq!(fields.flags, flags);
        let resolved = inspection.resolve_generic(&mut vfs).unwrap().unwrap();
        assert_eq!(
            resolved.name_source.mount_path.file_name().unwrap(),
            "Arcanum5.dat"
        );
        assert_eq!(
            resolved.definition,
            GenericDefinition {
                id: ItemId(format!("arcanum:item/prototype-{serialized}")),
                display_name: name.into(),
                base_weight: weight,
                base_worth: worth,
                magic_tech_complexity: complexity,
            }
        );
    }
}

#[test]
#[ignore = "requires the installed Arcanum data"]
fn installed_weapon_and_ammo_match_serialized_and_native_fields() {
    use arcanum_rs::content::items::{
        AmmoKind, AmmoRequirement, DamageComponent, DamageKind, WeaponAmmoDefinition,
        inspect_prototype,
    };
    use arcanum_rs::prototype::WeaponAmmoFields;

    let game = PathBuf::from(std::env::var_os("ARCANUM_GAME_DIR").expect("set ARCANUM_GAME_DIR"));
    let mut vfs = Vfs::from_game(&game, Policy::Ce).unwrap();
    let loose = game.join("data").canonicalize().unwrap();
    let mut definitions = Vec::new();
    for (path, basic, serialized, object_type, name, weight, worth) in [
        ("proto/006049 - weapon.pro", 6029, 6049, 5, "Dagger", 20, 20),
        (
            "proto/006136 - weapon.pro",
            6116,
            6136,
            5,
            "Hand Crafted Flintlock",
            80,
            138,
        ),
        ("proto/007059 - ammo.pro", 7039, 7059, 6, "Bullet", 1, 3),
    ] {
        let inspection = inspect_prototype(&mut vfs, path).unwrap();
        let header = inspection.prototype.header;
        assert_eq!(header.object_type, object_type);
        assert_eq!(header.id, SerializedPrototypeId(serialized));
        assert_eq!(Some(header.id), BasicPrototypeId(basic).serialized());
        assert_eq!(
            inspection.prototype.description,
            Some(DescriptionId(basic as i32))
        );
        assert_eq!(inspection.prototype.source.mount_path, loose);
        assert_eq!(inspection.prototype.source.storage, Storage::Loose);
        let fields = inspection.weapon_ammo.unwrap();
        assert_eq!((fields.item().weight, fields.item().worth), (weight, worth));
        let resolved = inspection.resolve_weapon_ammo(&mut vfs).unwrap().unwrap();
        assert_eq!(
            resolved.name_source.mount_path.file_name().unwrap(),
            "Arcanum5.dat"
        );
        match &resolved.definition {
            WeaponAmmoDefinition::Weapon(definition) => assert_eq!(definition.display_name, name),
            WeaponAmmoDefinition::Ammo(definition) => assert_eq!(definition.display_name, name),
        }
        definitions.push((fields, resolved.definition));
    }

    for (index, (damage, speed, range, ammo_type, consumption)) in [
        ([(1, 4), (1, 2)], 10, 1, 10000, 0),
        ([(2, 6), (1, 4)], 5, 15, 1, 1),
    ]
    .into_iter()
    .enumerate()
    {
        let (WeaponAmmoFields::Weapon(raw), WeaponAmmoDefinition::Weapon(weapon)) =
            &definitions[index]
        else {
            panic!("expected weapon");
        };
        assert_eq!(
            (
                raw.speed_factor,
                raw.range,
                raw.ammo_type,
                raw.ammo_consumption
            ),
            (speed, range, ammo_type, consumption)
        );
        assert_eq!(
            raw.damage_lower,
            [Some(damage[0].0), None, None, None, Some(damage[1].0)]
        );
        assert_eq!(
            raw.damage_upper,
            [Some(damage[0].1), None, None, None, Some(damage[1].1)]
        );
        assert_eq!(weapon.range_tiles, range as u32);
        assert_eq!(weapon.speed_factor, speed);
        let expected = [DamageKind::Normal, DamageKind::Fatigue]
            .into_iter()
            .zip(damage)
            .map(|(kind, (minimum, maximum))| DamageComponent {
                kind,
                minimum: minimum as u32,
                maximum: maximum as u32,
            })
            .collect::<Vec<_>>();
        assert_eq!(weapon.damage, expected);
        assert_eq!(
            weapon.ammunition,
            (index == 1).then_some(AmmoRequirement {
                kind: AmmoKind::Bullets,
                units_per_attack: 1
            })
        );
    }
    let (WeaponAmmoFields::Ammo(raw), WeaponAmmoDefinition::Ammo(ammo)) = &definitions[2] else {
        panic!("expected ammo");
    };
    assert_eq!(raw.quantity, 10);
    assert_eq!(raw.ammo_type, 1);
    assert_eq!(ammo.kind, AmmoKind::Bullets);
}

#[test]
#[ignore = "requires the installed Arcanum data"]
fn installed_armour_definitions_preserve_fit_and_signed_base_protection() {
    use arcanum_rs::content::items::{
        ArmourCoverage, ArmourSize, ResistanceAdjustment, ResistanceKind, inspect_prototype,
    };

    let game = PathBuf::from(std::env::var_os("ARCANUM_GAME_DIR").expect("set ARCANUM_GAME_DIR"));
    let mut vfs = Vfs::from_game(&game, Policy::Ce).unwrap();
    let loose = game.join("data").canonicalize().unwrap();
    for (basic, serial, name, flags, size, ac, weight, worth, physical, fire, electrical) in [
        (
            8091,
            8111,
            "Leather Armour",
            2,
            ArmourSize::Medium,
            9,
            150,
            470,
            16,
            5,
            None,
        ),
        (
            8099,
            8119,
            "Small Leather Armour",
            1,
            ArmourSize::Small,
            9,
            140,
            470,
            14,
            5,
            None,
        ),
        (
            8107,
            8127,
            "Large Leather Armour",
            4,
            ArmourSize::Large,
            9,
            160,
            470,
            17,
            5,
            None,
        ),
        (
            8075,
            8095,
            "Basic Platemail",
            2,
            ArmourSize::Medium,
            15,
            1000,
            900,
            36,
            10,
            Some(-20),
        ),
    ] {
        let path = format!("proto/{serial:06} - armor.pro");
        let item = inspect_prototype(&mut vfs, &path).unwrap();
        assert_eq!(item.prototype.header.id, SerializedPrototypeId(serial));
        assert_eq!(item.prototype.header.object_type, 7);
        assert_eq!(
            Some(item.prototype.header.id),
            BasicPrototypeId(basic).serialized()
        );
        assert_eq!(
            item.prototype.description,
            Some(DescriptionId(basic as i32))
        );
        assert_eq!(item.prototype.source.virtual_path, path);
        assert_eq!(item.prototype.source.storage, Storage::Loose);
        assert_eq!(item.prototype.source.mount_path, loose);
        let raw = item.armour.unwrap();
        assert_eq!(raw.flags, flags);
        assert_eq!(
            raw.resistance_adjustments,
            [Some(physical), Some(fire), electrical, None, None]
        );

        let resolved = item.resolve_armour(&mut vfs).unwrap().unwrap();
        assert_eq!(
            resolved.name_source.mount_path.file_name().unwrap(),
            "Arcanum5.dat"
        );
        let armour = resolved.definition;
        assert_eq!(armour.id.0, format!("arcanum:item/prototype-{serial}"));
        assert_eq!(armour.display_name, name);
        assert_eq!(armour.coverage, ArmourCoverage::Torso);
        assert_eq!(armour.sizes, [size]);
        assert_eq!(
            (
                armour.armour_class_adjustment,
                armour.base_weight,
                armour.base_worth
            ),
            (ac, weight, worth)
        );
        let resistances: Vec<_> = [
            ResistanceKind::Physical,
            ResistanceKind::Fire,
            ResistanceKind::Electrical,
        ]
        .into_iter()
        .zip([Some(physical), Some(fire), electrical])
        .filter_map(|(kind, value)| {
            value.map(|percentage_points| ResistanceAdjustment {
                kind,
                percentage_points,
            })
        })
        .collect();
        assert_eq!(armour.resistances, resistances);
    }
}

#[test]
#[ignore = "requires the installed Arcanum data"]
fn verifies_direct_armour_adjustments_and_sword_script_references() {
    use arcanum_rs::content::items::{
        ResistanceAdjustment, ResistanceKind, WeaponAmmoDefinition, WieldAttachment,
        inspect_prototype,
    };
    use arcanum_rs::prototype::{LegacyScriptId, WeaponAmmoFields};

    let game = PathBuf::from(std::env::var_os("ARCANUM_GAME_DIR").expect("set ARCANUM_GAME_DIR"));
    let mut vfs = Vfs::from_game(&game, Policy::Ce).unwrap();
    let ring = inspect_prototype(&mut vfs, "proto/008184 - armor.pro").unwrap();
    assert_eq!(ring.prototype.header.id, SerializedPrototypeId(8184));
    assert_eq!(ring.prototype.description, Some(DescriptionId(8164)));
    assert_eq!(
        ring.prototype.source.mount_path.file_name().unwrap(),
        "Arcanum5.dat"
    );
    let fields = ring.armour.unwrap();
    assert_eq!(fields.item.magic_tech_complexity, 40);
    assert_eq!(fields.armour_class_adjustment, 0);
    assert_eq!(fields.magical_armour_class_adjustment, 10);
    assert_eq!(fields.resistance_adjustments, [None; 5]);
    assert_eq!(
        fields.magical_resistance_adjustments,
        [Some(10), None, None, None, None]
    );
    let resolved = ring.resolve_armour(&mut vfs).unwrap().unwrap();
    assert_eq!(resolved.definition.display_name, "Ring of Protection");
    assert_eq!(resolved.definition.magic_tech_complexity, 40);
    assert_eq!(resolved.definition.armour_class_adjustment, 0);
    assert_eq!(resolved.definition.magical_armour_class_adjustment, 10);
    assert!(resolved.definition.resistances.is_empty());
    assert_eq!(
        resolved.definition.magical_resistances,
        [ResistanceAdjustment {
            kind: ResistanceKind::Physical,
            percentage_points: 10,
        }]
    );

    let platemail = inspect_prototype(&mut vfs, "proto/008065 - armor.pro").unwrap();
    let fields = platemail.armour.unwrap();
    assert_eq!(fields.item.magic_tech_complexity, -30);
    assert_eq!(fields.magical_armour_class_adjustment, -10);
    assert_eq!(
        fields.magical_resistance_adjustments,
        [Some(-5), Some(-10), Some(-20), Some(-20), Some(0)]
    );
    let resolved = platemail.resolve_armour(&mut vfs).unwrap().unwrap();
    assert_eq!(resolved.definition.display_name, "Machined Platemail");
    assert_eq!(resolved.definition.magic_tech_complexity, -30);
    assert_eq!(resolved.definition.armour_class_adjustment, 30);
    assert_eq!(resolved.definition.magical_armour_class_adjustment, -10);
    let adjustments: Vec<_> = resolved
        .definition
        .magical_resistances
        .iter()
        .map(|adjustment| adjustment.percentage_points)
        .collect();
    assert_eq!(adjustments, [-5, -10, -20, -20, 0]);

    let sword = inspect_prototype(&mut vfs, "proto/006084 - weapon.pro").unwrap();
    assert_eq!(sword.prototype.header.id, SerializedPrototypeId(6084));
    assert_eq!(sword.prototype.description, Some(DescriptionId(6064)));
    assert_eq!(sword.prototype.source.storage, Storage::Loose);
    let Some(WeaponAmmoFields::Weapon(fields)) = sword.weapon_ammo else {
        panic!("Sword of Defense should be a Weapon");
    };
    assert_eq!(fields.item.magic_tech_complexity, 15);
    let WeaponAmmoDefinition::Weapon(definition) = sword
        .resolve_weapon_ammo(&mut vfs)
        .unwrap()
        .unwrap()
        .definition
    else {
        panic!("expected a native Weapon");
    };
    assert_eq!(definition.display_name, "Sword of Defense");
    let scripts = sword.resolve_wield_scripts(&vfs);
    assert_eq!(scripts.len(), 2);
    for (report, (attachment, id, path)) in scripts.into_iter().zip([
        (
            WieldAttachment::On,
            28001,
            "scr/28001magic_sword_defense_wield_on.scr",
        ),
        (
            WieldAttachment::Off,
            28002,
            "scr/28002magic_sword_defense_wield_off.scr",
        ),
    ]) {
        assert_eq!(report.attachment, attachment);
        assert_eq!(report.legacy_id, LegacyScriptId(id));
        let resolved = report.resolution.unwrap();
        assert_eq!(resolved.reference.resource_path, path);
        assert_eq!(
            resolved.source.mount_path.file_name().unwrap(),
            "Arcanum2.dat"
        );
    }
}
