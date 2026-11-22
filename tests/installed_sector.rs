use arcanum_rs::{
    compositor::{SpriteDraw, blit_ordered},
    content::sector::{DEMO_MAP, DEMO_ORIGIN, DEMO_SECTOR, load_patch},
    sector::TileArtId,
    vfs::{Policy, Vfs},
};
use std::path::PathBuf;

#[test]
#[ignore = "requires installed Arcanum data"]
fn bates_floor_matches_original_sector_ids_and_raster() {
    let game = PathBuf::from(std::env::var_os("ARCANUM_GAME_DIR").expect("set ARCANUM_GAME_DIR"));
    let mut vfs = Vfs::from_game(&game, Policy::Ce).unwrap();
    let patch = load_patch(&mut vfs, DEMO_MAP, DEMO_SECTOR, DEMO_ORIGIN).unwrap();
    assert_eq!(
        patch.source.mount_path,
        game.join("modules/Arcanum.dat").canonicalize().unwrap()
    );
    assert_eq!(
        patch.names_source.mount_path,
        game.join("Arcanum5.dat").canonicalize().unwrap()
    );
    assert_eq!((patch.light_count, patch.tile_offset), (9, 436));
    assert_eq!(patch.center, (95, 98));
    assert_eq!(patch.artwork().count(), 10);
    let silhouette = arcanum_rs::isometric::synthetic_floor();
    for frame in patch.artwork() {
        assert!(
            frame
                .decoded
                .frame
                .pixels
                .iter()
                .zip(&silhouette.pixels)
                .all(|(actual, expected)| actual[3] == expected[3]),
            "{}",
            frame.source.virtual_path
        );
    }
    assert_eq!(patch.tiles().len(), 25);
    for (index, coord, id, path) in [
        (0, (93, 96), 0x01040e00, "art/tile/wd1bse0h.art"),
        (4, (97, 96), 0x01040c00, "art/tile/wd1bse0g.art"),
        (5, (93, 97), 0x01040600, "art/tile/wd1bse0d.art"),
        (12, (95, 98), 0x004104c0, "art/tile/wodbse0c.art"),
        (24, (97, 100), 0x004106c0, "art/tile/wodbse0d.art"),
    ] {
        let tile = &patch.tiles()[index];
        assert_eq!(
            (tile.world, tile.id, tile.artwork.as_str()),
            (coord, TileArtId(id), path)
        );
    }
    assert_eq!(patch.draws().nth(12).unwrap().anchor, (0, 19));

    let background = [32, 43, 58];
    let mut full = vec![background; 400 * 200];
    let draws = |center: (i32, i32)| {
        patch.draws().map(move |draw| SpriteDraw {
            frame: draw.frame,
            anchor: (draw.anchor.0 + center.0, draw.anchor.1 + center.1),
        })
    };
    blit_ordered(&mut full, (400, 200), 400, draws((200, 100))).unwrap();
    assert_eq!(
        full.iter().filter(|&&pixel| pixel != background).count(),
        25 * 1600
    );

    let mut clipped = vec![background; 80 * 40];
    blit_ordered(&mut clipped, (80, 40), 80, draws((40, 20))).unwrap();
    for (row, pixels) in clipped.chunks(80).enumerate() {
        let start = (row + 80) * 400 + 160;
        assert_eq!(pixels, &full[start..start + 80]);
    }
}
