use arcanum_rs::content::effects::{EffectId, EffectOperation, LegacyEffectId, load_effect};
use arcanum_rs::content::items::ResistanceKind;
use arcanum_rs::mes::Encoding;
use arcanum_rs::vfs::{Policy, Storage, Vfs};
use std::path::{Path, PathBuf};

#[test]
#[ignore = "requires installed Arcanum data"]
fn selected_effects_match_an_independently_mounted_archive() {
    let game = PathBuf::from(std::env::var_os("ARCANUM_GAME_DIR").expect("set ARCANUM_GAME_DIR"));
    let mut vfs = Vfs::from_game(&game, Policy::Ce).unwrap();
    let mut archive = Vfs::new(Policy::Ce);
    let archive_mount = archive.mount_archive(game.join("Arcanum3.dat")).unwrap();
    for (id, record, operation) in [
        (128, "ac +1", EffectOperation::AddArmourClass { points: 1 }),
        (
            53,
            "resistfire min 50",
            EffectOperation::ResistanceFloor {
                channel: ResistanceKind::Fire,
                minimum_percent: 50,
            },
        ),
    ] {
        let selected = load_effect(&mut vfs, LegacyEffectId(id)).unwrap();
        assert_eq!(selected.encoding, Encoding::Windows1252);
        assert_eq!(selected.source.storage, Storage::Loose);
        assert_eq!(selected.source.virtual_path, "rules/effect.mes");
        assert_eq!(
            selected.source.mount_path,
            game.join("data").canonicalize().unwrap()
        );
        assert_eq!(
            selected.source.loose_path.as_deref(),
            Some(Path::new("rules/effect.mes"))
        );
        assert_eq!(
            selected.definition.id,
            EffectId(format!("arcanum:effect/{id}"))
        );
        assert_eq!(selected.definition.operation, operation);
        assert_eq!(selected.record, record);

        let archived = load_effect(&mut archive, LegacyEffectId(id)).unwrap();
        assert_eq!(archived.source.mount_id, archive_mount);
        assert_eq!(archived.definition, selected.definition);
        assert_eq!(archived.record, selected.record);
    }
}
