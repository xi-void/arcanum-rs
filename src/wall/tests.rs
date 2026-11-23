use super::*;
use crate::compositor::{blit_ordered, sprite_origin};

#[test]
fn resolves_the_interior_variant_and_preserves_the_orientation_bits() {
    let structures = Mes::parse_text("{0}{WoD0 Ext0 nul nul}{7}{In40 Gb11 wd1 gb1}").unwrap();
    let names = Mes::parse_text("{0}{wod0}{1}{in42}").unwrap();
    let id = WallArtId(0x1010_0900);
    assert_eq!(
        id.resolve(&structures, &names).unwrap(),
        "art/wall/in4bseu1.art"
    );
    let unsupported = WallArtId(0x1010_3911);
    let fields = unsupported.fields().unwrap();
    assert_eq!((fields.rotation, fields.flags, fields.palette), (7, 1, 1));
    assert_eq!(
        unsupported.resolve(&structures, &names),
        Err(Error::Unsupported(fields))
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
