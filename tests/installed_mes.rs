use arcanum_rs::content::load_mes;
use arcanum_rs::dat::EntryKind;
use arcanum_rs::mods::Catalog;
use arcanum_rs::vfs::{Policy, Storage, Vfs};
use std::path::PathBuf;

#[test]
#[ignore = "requires the installed game and the external vfs-smoke package"]
fn installed_archive_and_external_vfs_smoke() {
    let game = PathBuf::from(std::env::var_os("ARCANUM_GAME_DIR").expect("set ARCANUM_GAME_DIR"));
    let mods = PathBuf::from(std::env::var_os("ARCANUM_MODS_DIR").expect("set ARCANUM_MODS_DIR"));
    let path = "mes/gamedesc.mes";
    let id = 40000;
    let override_text = "Arcanum-RS test override";

    let mut ce = Vfs::from_game(&game, Policy::Ce).unwrap();
    let original = load_mes(&mut ce, path).unwrap();
    assert_eq!(original.source.virtual_path, path);
    assert_eq!(
        original.source.mount_path.file_name().unwrap(),
        "Arcanum.PATCH6"
    );
    assert_eq!(original.source.storage, Storage::Archive(EntryKind::Zlib));
    assert!(original.source.mod_id.is_none());
    assert_ne!(
        original.mes.get(id).expect("original MES entry"),
        override_text
    );

    let catalog = Catalog::discover(&mods).unwrap();
    let plan = catalog.load_order(&["vfs-smoke"]).unwrap();
    let mut vfs = plan
        .apply(Vfs::from_game(&game, Policy::Ordered).unwrap())
        .unwrap();
    let replacement = load_mes(&mut vfs, r"MES\GAMEDESC.MES").unwrap();
    assert_eq!(replacement.source.virtual_path, path);
    assert_eq!(replacement.mes.get(id), Some(override_text));
    assert_eq!(replacement.mes.len(), 1);
    assert_eq!(replacement.source.mod_id.unwrap().as_str(), "vfs-smoke");
    assert_eq!(replacement.source.storage, Storage::Loose);
    assert_eq!(
        replacement
            .source
            .mount_path
            .join(replacement.source.loose_path.unwrap()),
        mods.canonicalize()
            .unwrap()
            .join("vfs-smoke/resources/mes/gamedesc.mes")
    );
}
