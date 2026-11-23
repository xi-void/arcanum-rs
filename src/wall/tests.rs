use super::*;
use crate::compositor::{blit_ordered, sprite_origin};

#[test]
fn resolves_interior_top_corner_arms_with_original_orientation() {
    let structures = Mes::parse_text("{0}{In40 WoD0 nul nul}").unwrap();
    let names = Mes::parse_text("{0}{in40}{1}{wod0}").unwrap();
    let right = WallArtId(0x1001_8800);
    let left = WallArtId(0x1001_7811);
    for (id, expected, rotation, palette, mirror) in [
        (right, "art/wall/in4tcru0.art", 1, 0, false),
        (left, "art/wall/in4tclu0.art", 7, 1, true),
    ] {
        assert_eq!(id.resolve(&structures, &names).unwrap(), expected);
        let fields = id.fields().unwrap();
        assert_eq!(
            (fields.rotation, fields.palette, fields.mirror_x()),
            (rotation, palette, mirror)
        );
    }
}

#[test]
fn resolves_exterior_run_and_corner_for_both_facings() {
    let structures = Mes::parse_text("{0}{In40 Gb11 wd1 gb1}").unwrap();
    let names = Mes::parse_text("{0}{in40}{1}{gb11}").unwrap();
    let frame = SpriteFrame {
        width: 40,
        height: 154,
        hotspot: (39, 132),
        offset: (0, 0),
        pixels: Vec::new(),
    };
    for (id, path, pivot, palette, mirror) in [
        (0x1000_1811, "gb1bseu0.art", (-1, 132), 1, true),
        (0x1000_e900, "gb1bclu1.art", (39, 132), 0, false),
        (0x1001_1911, "gb1bcru1.art", (-1, 132), 1, true),
    ] {
        let id = WallArtId(id);
        assert_eq!(
            id.resolve(&structures, &names).unwrap(),
            format!("art/wall/{path}")
        );
        let fields = id.fields().unwrap();
        assert_eq!((fields.palette, fields.mirror_x()), (palette, mirror));
        assert_eq!(fields.pivot(&frame), pivot);
        assert_eq!(fields.order_key(), (-2, 38));
    }
}

#[test]
fn resolves_the_interior_variant_and_preserves_the_orientation_bits() {
    let structures = Mes::parse_text("{0}{WoD0 Ext0 nul nul}{7}{In40 Gb11 wd1 gb1}").unwrap();
    let names = Mes::parse_text("{0}{wod0}{1}{in42}").unwrap();
    let id = WallArtId(0x1010_0900);
    assert_eq!(
        id.resolve(&structures, &names).unwrap(),
        "art/wall/in4bseu1.art"
    );
    let mirrored = WallArtId(0x1010_3911);
    let fields = mirrored.fields().unwrap();
    assert_eq!((fields.rotation, fields.flags, fields.palette), (7, 1, 1));
    assert_eq!(
        mirrored.resolve(&structures, &names).unwrap(),
        "art/wall/in4bseu1.art"
    );
    assert_eq!(fields.order_key(), id.fields().unwrap().order_key());
    assert!(WallArtId(0x1010_1000).fields().unwrap().order_key() > fields.order_key());
    let unsupported = WallArtId(0x1010_3901);
    assert_eq!(
        unsupported.resolve(&structures, &names),
        Err(Error::Unsupported(unsupported.fields().unwrap()))
    );
    assert_eq!(WallArtId(0xb000_0000).fields(), Err(Error::NotWall));
    assert!(
        id.resolve(&structures, &Mes::parse_text("{0}{wod0}").unwrap())
            .is_err()
    );
}

#[test]
fn uses_wall_pivot_offsets_and_draws_visible_pixels_over_the_surface() {
    let placement = Placement {
        tile: (31, 32),
        pixel_offset: (-3, 5),
    };
    let pixels = vec![[0; 4], [255, 140, 60, 255], [50, 20, 10, 255]];
    let wall = SpriteFrame {
        width: 3,
        height: 1,
        hotspot: (78, 24),
        offset: (12, 9),
        pixels,
    };
    let draw = placement.draw(&wall, (31, 31)).unwrap();
    assert_eq!(draw.anchor, (77, 5));
    assert_eq!(sprite_origin(&wall, draw.anchor), (-1, -19));

    let floor = SpriteFrame {
        width: 3,
        height: 1,
        hotspot: (0, 0),
        offset: (0, 0),
        pixels: vec![[20, 40, 60, 255]; 3],
    };
    let mut viewport = [[0; 3]; 3];
    // Move the raster onto row zero and clip its transparent left pixel.
    blit_ordered(
        &mut viewport,
        (3, 1),
        3,
        [
            SpriteDraw {
                frame: &floor,
                anchor: (0, 0),
            },
            SpriteDraw {
                frame: &wall,
                anchor: (draw.anchor.0, draw.anchor.1 + 19),
            },
        ],
    )
    .unwrap();
    assert_eq!(viewport, [[255, 140, 60], [50, 20, 10], [20, 40, 60]]);
    let mut viewport = [[0; 3]; 2];
    blit_ordered(
        &mut viewport,
        (2, 1),
        2,
        [
            SpriteDraw {
                frame: &floor,
                anchor: (0, 0),
            },
            SpriteDraw {
                frame: &wall,
                anchor: (draw.anchor.0 + 1, draw.anchor.1 + 19),
            },
        ],
    )
    .unwrap();
    assert_eq!(viewport, [[20, 40, 60], [255, 140, 60]]);
    assert!(placement.draw(&wall, (u32::MAX, 0)).is_none());
}

#[test]
fn resolves_verified_exterior_and_doorway_piece_without_adjusting_placement() {
    let structures = Mes::parse_text("{0}{In40 WoD0 nul nul}").unwrap();
    let names = Mes::parse_text("{0}{in40}{1}{wod0}").unwrap();
    let id = WallArtId(0x1000_0000 | (27 << 14) | (5 << 11) | (1 << 8));
    assert_eq!(
        id.resolve(&structures, &names).unwrap(),
        "art/wall/wodd4ru1.art"
    );
    let frame = SpriteFrame {
        width: 40,
        height: 154,
        hotspot: (39, 132),
        offset: (0, 0),
        pixels: Vec::new(),
    };
    assert_eq!(id.fields().unwrap().pivot(&frame), (39, 132));
    assert_eq!(id.fields().unwrap().order_key(), (-2, 38));
}
