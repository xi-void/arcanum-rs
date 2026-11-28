use arcanum_rs::content::{maps::Catalog, world::WorldStream};
use arcanum_rs::developer::PlayerCommand;
use arcanum_rs::prototype::SerializedPrototypeId;
use arcanum_rs::vfs::{Policy, Vfs};

#[test]
#[ignore = "requires installed original game data"]
fn grants_preserve_inventory_identity_and_progression_on_repeats_and_rejection() {
    let game = std::env::var_os("ARCANUM_GAME_DIR").expect("set ARCANUM_GAME_DIR");
    let mut vfs = Vfs::from_game(&game, Policy::Ce).unwrap();
    let catalog = Catalog::load(&mut vfs).unwrap();
    let map = catalog.find("5011").unwrap();
    let region = map.files(&mut vfs).unwrap().bounds();
    let tile = (80, 102);
    let mut stream = WorldStream::new(
        Vfs::from_game(&game, Policy::Ce).unwrap(),
        map.directory.clone(),
        region,
        tile,
    )
    .unwrap()
    .with_player()
    .unwrap();
    stream
        .prepare(&region.camera(tile).unwrap(), (800, 600))
        .unwrap();
    let mut world = stream.into_interactive(|| {}, false).unwrap();
    world.start_player().unwrap();

    let before = world.inventory_counters()[0];
    world
        .player_command(&mut vfs, PlayerCommand::AddGold(1000))
        .unwrap();
    assert_eq!(world.inventory_counters()[0], before + 1000);
    let ammo = SerializedPrototypeId(7058);
    let first = world
        .player_command(&mut vfs, PlayerCommand::AddItem(ammo, 6))
        .unwrap();
    let second = world
        .player_command(&mut vfs, PlayerCommand::AddItem(ammo, 2))
        .unwrap();
    assert_eq!(first["items"], second["items"]);
    let arrows = world
        .inspect_item_owner(&mut vfs, first["items"][0].as_str().unwrap())
        .unwrap();
    assert_eq!(arrows["quantity"], 8);
    assert_eq!(arrows["current"]["kind"], "player");

    for command in [
        PlayerCommand::LearnAllSpells,
        PlayerCommand::LearnTech("all".into()),
    ] {
        world.player_command(&mut vfs, command).unwrap();
    }
    let learned = world.character_statistics();
    assert_eq!(learned.colleges, [5; 16]);
    assert_eq!(learned.degrees, [7; 8]);
    world
        .player_command(&mut vfs, PlayerCommand::LearnSpell("Harm".into()))
        .unwrap();
    world
        .player_command(&mut vfs, PlayerCommand::LearnTech("2410".into()))
        .unwrap();
    assert_eq!(world.character_statistics().base, learned.base);

    let saved = serde_json::to_value(world.save_objects()).unwrap();
    for rejected in [
        PlayerCommand::AddGold(i32::MAX),
        // Individual bows fill the grid partway through this single grant.
        PlayerCommand::AddItem(SerializedPrototypeId(6116), 100),
        PlayerCommand::SetSkill("melee".into(), 6),
    ] {
        assert!(world.player_command(&mut vfs, rejected).is_err());
        assert_eq!(serde_json::to_value(world.save_objects()).unwrap(), saved);
    }
}
