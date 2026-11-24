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

/// Project a world tile relative to the scene's shared center tile.
pub fn project_world_tile(tile: (u32, u32), center: (u32, u32)) -> Option<(i32, i32)> {
    let delta = |tile, center| i32::try_from(i64::from(tile) - i64::from(center)).ok();
    project_tile(delta(tile.0, center.0)?, delta(tile.1, center.1)?)
}

/// Invert the same centered projection used to rasterize the floor.
pub fn world_tile_at_pixel((x, y): (i64, i64), (cx, cy): (u32, u32)) -> Option<(u32, u32)> {
    let twice_y = y.checked_mul(2)?.checked_add(40)?;
    Some((
        u32::try_from(i64::from(cx) + twice_y.checked_sub(x)?.div_euclid(80)).ok()?,
        u32::try_from(i64::from(cy) + twice_y.checked_add(x)?.div_euclid(80)).ok()?,
    ))
}

/// Place a verified 78 × 40 floor frame at a projected tile center.
pub fn floor_draw(frame: &SpriteFrame, x: i32, y: i32) -> Option<SpriteDraw<'_>> {
    if (frame.width, frame.height) != FLOOR_SIZE || frame.hotspot != FLOOR_HOTSPOT {
        return None;
    }
    surface_draw(frame, x, y)
}

/// CE tile_draw_iso blits a fixed rectangle for tiles and baked facades.
/// Serialized ART hotspots only affect object-style drawing.
pub(crate) fn surface_draw(frame: &SpriteFrame, x: i32, y: i32) -> Option<SpriteDraw<'_>> {
    if (frame.width, frame.height) != FLOOR_SIZE {
        return None;
    }
    let (sx, sy) = project_tile(x, y)?;
    Some(SpriteDraw {
        frame,
        anchor: (
            sx.checked_sub(39)?.checked_add(frame.hotspot.0)?,
            sy.checked_sub(20)?.checked_add(frame.hotspot.1)?,
        ),
    })
}

/// Build the fixed demo's 3 × 3 floor pass, followed by one centered sprite.
pub fn fixed_scene<'a>(
    floor: &'a SpriteFrame,
    foreground: &'a SpriteFrame,
) -> Option<impl Iterator<Item = SpriteDraw<'a>>> {
    floor_draw(floor, 0, 0)?;
    let floor_draws = (-1..=1).flat_map(move |y| {
        (-1..=1).map(move |x| floor_draw(floor, x, y).expect("fixed patch fits screen coordinates"))
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
