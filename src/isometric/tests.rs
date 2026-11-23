use super::*;
use crate::compositor::{blit_ordered, sprite_origin};

#[test]
fn projects_signed_tile_centers_with_checked_range() {
    for (tile, pixels) in [
        ((0, 0), (0, 0)),
        ((1, 0), (-40, 20)),
        ((0, 1), (40, 20)),
        ((-1, 0), (40, -20)),
        ((0, -1), (-40, -20)),
        ((-2, 3), (200, 20)),
        ((i32::MAX / 40, i32::MAX / 40), (0, 2_147_483_640)),
    ] {
        assert_eq!(project_tile(tile.0, tile.1), Some(pixels));
    }
    assert_eq!(project_tile(i32::MIN, i32::MAX), None);
    assert_eq!(project_tile(i32::MAX, i32::MAX), None);
    assert_eq!(project_tile(i32::MIN, i32::MIN), None);
}

#[test]
fn floor_pass_matches_ce_row_traversal_and_places_the_sprite_last() {
    let floor = synthetic_floor();
    let object = SpriteFrame {
        width: 1,
        height: 1,
        hotspot: (0, 0),
        offset: (0, 0),
        pixels: vec![[250, 21, 9, 255]],
    };
    let draws: Vec<_> = fixed_scene(&floor, &object).unwrap().collect();
    assert_eq!(draws.len(), 10);
    assert_eq!(
        draws[..9]
            .iter()
            .map(|draw| sprite_origin(draw.frame, draw.anchor))
            .collect::<Vec<_>>(),
        [
            (-39, -60),
            (-79, -40),
            (-119, -20),
            (1, -40),
            (-39, -20),
            (-79, 0),
            (41, -20),
            (1, 0),
            (-39, 20)
        ]
    );
    assert!(
        draws[..9]
            .iter()
            .all(|draw| std::ptr::eq(draw.frame, &floor))
    );
    assert!(std::ptr::eq(draws[9].frame, &object));
    assert_eq!(draws[9].anchor, (0, 0));
    assert!(fixed_scene(&object, &object).is_none());

    let background = [10, 20, 30];
    let mut complete = vec![background; 240 * 120];
    let shifted = draws.iter().map(|&draw| SpriteDraw {
        frame: draw.frame,
        anchor: (draw.anchor.0 + 120, draw.anchor.1 + 60),
    });
    blit_ordered(&mut complete, (240, 120), 240, shifted).unwrap();
    assert_eq!(
        complete.iter().filter(|&&p| p != background).count(),
        9 * 1600
    );
    assert_eq!(complete[60 * 240 + 120], [250, 21, 9]);
    assert_eq!(complete[0], background);
    // At this diagonal join, two opaque pixels touch without a gap.
    assert_eq!(complete[60 * 240 + 158], [37, 69, 63]);
    assert_eq!(complete[60 * 240 + 159], [37, 69, 63]);

    let mut cropped = vec![background; 60 * 36];
    let shifted = draws.iter().map(|&draw| SpriteDraw {
        frame: draw.frame,
        anchor: (draw.anchor.0 + 30, draw.anchor.1 + 18),
    });
    blit_ordered(&mut cropped, (60, 36), 60, shifted).unwrap();
    for (row, actual) in cropped.chunks(60).enumerate() {
        let offset = (row + 42) * 240 + 90;
        assert_eq!(actual, &complete[offset..offset + 60]);
    }
}

#[test]
fn both_diagonal_neighbors_share_edges_without_a_hole_or_double_coverage() {
    let floor = synthetic_floor();
    assert_tight_floor_edges(&floor);
}

fn assert_tight_floor_edges(floor: &SpriteFrame) {
    let row_span = |row: usize| {
        let alpha = &floor.pixels[row * 78..(row + 1) * 78];
        let start = alpha.iter().position(|pixel| pixel[3] == 255).unwrap();
        let end = alpha.iter().rposition(|pixel| pixel[3] == 255).unwrap() + 1;
        assert!(alpha[start..end].iter().all(|p| p[3] == 255));
        (start as i32, end as i32)
    };
    for neighbor in [(1, 0), (0, 1)] {
        let (dx, dy) = project_tile(neighbor.0, neighbor.1).unwrap();
        for row in dy..40 {
            let a = row_span(row as usize);
            let b = row_span((row - dy) as usize);
            if dx < 0 {
                assert_eq!(b.1 + dx, a.0, "left edge on row {row}");
            } else {
                assert_eq!(a.1, b.0 + dx, "right edge on row {row}");
            }
        }
    }
}

#[test]
fn inverse_projection_round_trips_original_tile_centers() {
    let reference = (104, 92);
    for tile in [(104, 92), (103, 93), (110, 95), (0, 0)] {
        let (x, y) = project_world_tile(tile, reference).unwrap();
        assert_eq!(
            world_tile_at_pixel((i64::from(x), i64::from(y)), reference),
            Some(tile)
        );
    }
    assert_eq!(world_tile_at_pixel((0, -40), (0, 0)), None);
}
