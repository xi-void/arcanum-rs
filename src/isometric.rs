//! Pixel placement for the verified 80 × 40 isometric floor grid.

use crate::{art::SpriteFrame, compositor::SpriteDraw};

const FLOOR_SIZE: (u32, u32) = (78, 40);
const FLOOR_HOTSPOT: (i32, i32) = (39, 39);

/// Project a signed tile coordinate to its center relative to tile (0, 0).
/// Increasing x moves down-left and increasing y moves down-right.
/// Return None when either pixel coordinate falls outside the signed range.
pub fn project_tile(x: i32, y: i32) -> Option<(i32, i32)> {
    let (x, y) = (i64::from(x), i64::from(y));
    Some((
        i32::try_from(40 * (y - x)).ok()?,
        i32::try_from(20 * (x + y)).ok()?,
    ))
}

/// Build the fixed demo's 3 × 3 floor pass, followed by one centered sprite.
/// The floor must match the verified frame dimensions and stored hotspot.
pub fn fixed_scene<'a>(
    floor: &'a SpriteFrame,
    foreground: &'a SpriteFrame,
) -> Option<impl Iterator<Item = SpriteDraw<'a>>> {
    if (floor.width, floor.height) != FLOOR_SIZE || floor.hotspot != FLOOR_HOTSPOT {
        return None;
    }
    let floor_draws = (-1..=1).flat_map(move |y| {
        (-1..=1).map(move |x| {
            let (sx, sy) = project_tile(x, y).expect("fixed patch fits signed screen coordinates");
            // CE places the 78 × 40 raster at center − (39, 20).
            // Its bottom hotspot is (39, 39), so the draw anchor is 19 px below center.
            SpriteDraw {
                frame: floor,
                anchor: (sx, sy + FLOOR_HOTSPOT.1 - 20),
            }
        })
    });
    Some(floor_draws.chain([SpriteDraw {
        frame: foreground,
        anchor: (0, 0),
    }]))
}

/// A diagnostic tile with the verified floor silhouette and visible edges.
pub fn synthetic_floor() -> SpriteFrame {
    let (width, height) = FLOOR_SIZE;
    let mut pixels = vec![[0; 4]; (width * height) as usize];
    for row in 0..height {
        let inset = 2 * row.abs_diff(19).min(row.abs_diff(20));
        for column in inset..width - inset {
            let color = if column == inset || column == width - inset - 1 {
                [37, 69, 63, 255]
            } else {
                [86, 132, 108, 255]
            };
            pixels[(row * width + column) as usize] = color;
        }
    }
    SpriteFrame {
        width,
        height,
        hotspot: FLOOR_HOTSPOT,
        offset: (0, 0),
        pixels,
    }
}

#[cfg(test)]
mod tests;
