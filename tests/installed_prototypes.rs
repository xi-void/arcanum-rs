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

    for (item, lookup, serialized, weight, worth, complexity) in [
        (&report.ingredients[0], 10093, 10113, 20, 10, 0),
        (&report.ingredients[1], 10067, 10087, 5, 2, -10),
        (&report.product, 10119, 10139, 10, 20, -55),
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
        report.ingredients[1].name.as_ref().unwrap().text,
        "Varham's Aqua Vitae"
    );

    let written = load_prototype(&mut vfs, "proto/014099 - written.pro").unwrap();
    assert_eq!(written.header.id, SerializedPrototypeId(14099));
    assert_eq!(written.header.object_type, 13);
    assert!(written.food.is_none());
}
