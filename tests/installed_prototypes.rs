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
