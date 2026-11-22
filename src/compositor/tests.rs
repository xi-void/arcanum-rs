use super::*;

const BACKGROUND: [u8; 3] = [10, 20, 30];

fn frame(width: u32, height: u32, pixels: Vec<[u8; 4]>) -> SpriteFrame {
    SpriteFrame {
        width,
        height,
        hotspot: (0, 0),
        offset: (0, 0),
        pixels,
    }
}

#[test]
fn draws_row_major_with_opaque_transparent_and_half_alpha_pixels() {
    let sprite = frame(
        2,
        2,
        vec![
            [255, 0, 55, 0],
            [5, 6, 7, 255],
            [30, 10, 230, 128],
            [2, 3, 4, 255],
        ],
    );
    let expected = [
        BACKGROUND,
        BACKGROUND,
        BACKGROUND,
        BACKGROUND,
        BACKGROUND,
        BACKGROUND,
        [5, 6, 7],
        BACKGROUND,
        BACKGROUND,
        [20, 15, 130],
        [2, 3, 4],
        BACKGROUND,
    ];
    for _ in 0..2 {
        let mut destination = [BACKGROUND; 12];
        blit(&mut destination, (3, 3), 4, &sprite, (1, 1)).unwrap();
        assert_eq!(destination, expected);
    }
}

#[test]
fn clips_all_edges_and_preserves_source_alignment_and_padding() {
    let sprite = frame(
        2,
        2,
        vec![
            [1, 0, 0, 255],
            [2, 0, 0, 255],
            [3, 0, 0, 255],
            [4, 0, 0, 255],
        ],
    );
    for (origin, red_channels) in [
        ((-1, 0), [2, 10, 10, 4, 10, 10]),
        ((1, 0), [10, 1, 10, 10, 3, 10]),
        ((0, -1), [3, 4, 10, 10, 10, 10]),
        ((0, 1), [10, 10, 10, 1, 2, 10]),
        ((-1, -1), [4, 10, 10, 10, 10, 10]),
        ((i64::MAX, i64::MIN), [10; 6]),
    ] {
        let mut destination = [BACKGROUND; 6];
        blit(&mut destination, (2, 2), 3, &sprite, origin).unwrap();
        assert_eq!(destination.map(|pixel| pixel[0]), red_channels);
    }
}

#[test]
fn anchors_signed_hotspots_independently_of_frame_movement() {
    let mut sprite = frame(2, 1, vec![[12, 34, 56, 255], [8, 9, 10, 255]]);
    sprite.hotspot = (-1, 1);
    sprite.offset = (5, -7);
    let origin = sprite_origin(&sprite, (1, 2));
    assert_eq!(origin, (2, 1));
    let mut destination = [BACKGROUND; 8];
    blit(&mut destination, (4, 2), 4, &sprite, origin).unwrap();
    assert_eq!(destination[6..], [[12, 34, 56], [8, 9, 10]]);

    sprite.offset = (-9, 8);
    assert_eq!(sprite_origin(&sprite, (1, 2)), origin);
    sprite.hotspot = (i32::MIN, i32::MAX);
    assert_eq!(
        sprite_origin(&sprite, (i32::MAX, i32::MIN)),
        (4_294_967_295, -4_294_967_295)
    );
}

#[test]
fn validates_destination_and_sprite_before_touching_pixels() {
    let mut destination = [BACKGROUND; 4];
    let mut sprite = frame(1, 1, vec![[1, 2, 3, 255]]);
    for (dimensions, stride) in [((0, 1), 1), ((1, 0), 1), ((2, 2), 1), ((1, 2), usize::MAX)] {
        assert_eq!(
            blit(&mut destination, dimensions, stride, &sprite, (0, 0)),
            Err(Error::DestinationLayout)
        );
    }
    assert_eq!(
        blit(&mut destination, (2, 2), 3, &sprite, (0, 0)),
        Err(Error::DestinationLength {
            required: 5,
            actual: 4
        })
    );
    for width in [0, 2] {
        sprite.width = width;
        assert_eq!(
            blit(&mut destination, (2, 2), 2, &sprite, (0, 0)),
            Err(Error::SpriteLayout)
        );
    }
    assert_eq!(destination, [BACKGROUND; 4]);
}

#[test]
fn accepts_a_padded_stride_without_requiring_padding_after_the_last_row() {
    let sprite = frame(1, 1, vec![[1, 2, 3, 255]]);
    let mut destination = [BACKGROUND; 5];
    blit(&mut destination, (2, 2), 3, &sprite, (1, 1)).unwrap();
    assert_eq!(
        destination,
        [BACKGROUND, BACKGROUND, BACKGROUND, BACKGROUND, [1, 2, 3]]
    );
}

#[test]
fn ordered_overlap_preserves_transparency_and_changes_with_draw_order() {
    let [red, green, mix] = [[200, 0, 0], [0, 200, 0], [100, 100, 0]];
    let mut back = frame(4, 1, vec![[200, 0, 0, 255]; 4]);
    back.hotspot = (-1, 0);
    let mut front = frame(
        4,
        1,
        vec![
            [0, 200, 0, 255],
            [0, 200, 0, 128],
            [1, 2, 3, 0],
            [0, 200, 0, 255],
        ],
    );
    front.hotspot = (1, 0);
    let back = SpriteDraw {
        frame: &back,
        anchor: (-1, 0),
    };
    let front = SpriteDraw {
        frame: &front,
        anchor: (2, 0),
    };
    let mut destination = [BACKGROUND; 6];
    blit_ordered(&mut destination, (6, 1), 6, [back, front]).unwrap();
    assert_eq!(destination, [red, green, mix, red, green, BACKGROUND]);

    destination.fill(BACKGROUND);
    blit_ordered(&mut destination, (6, 1), 6, [front, back]).unwrap();
    assert_eq!(destination, [red, red, red, red, green, BACKGROUND]);
}

#[test]
fn viewport_size_changes_clipping_for_the_same_signed_anchors() {
    let back = frame(3, 3, vec![[1, 0, 0, 255]; 9]);
    let front = frame(3, 3, vec![[2, 0, 0, 255]; 9]);
    let offscreen = frame(1, 1, vec![[3, 0, 0, 255]]);
    let draws = || {
        [
            SpriteDraw {
                frame: &back,
                anchor: (-1, -1),
            },
            SpriteDraw {
                frame: &front,
                anchor: (1, 1),
            },
            SpriteDraw {
                frame: &offscreen,
                anchor: (-1, 0),
            },
            SpriteDraw {
                frame: &offscreen,
                anchor: (0, -1),
            },
            SpriteDraw {
                frame: &offscreen,
                anchor: (4, 0),
            },
            SpriteDraw {
                frame: &offscreen,
                anchor: (0, 4),
            },
        ]
    };

    let mut small = [BACKGROUND; 9];
    blit_ordered(&mut small, (3, 3), 3, draws()).unwrap();
    assert_eq!(small.map(|pixel| pixel[0]), [1, 1, 10, 1, 2, 2, 10, 2, 2]);

    let mut large = [BACKGROUND; 16];
    blit_ordered(&mut large, (4, 4), 4, draws()).unwrap();
    assert_eq!(
        large.map(|pixel| pixel[0]),
        [1, 1, 10, 10, 1, 2, 2, 2, 10, 2, 2, 2, 10, 2, 2, 2]
    );
}
