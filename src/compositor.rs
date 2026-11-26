//! Software composition of straight RGBA8 sprites over opaque RGB8 surfaces.

use crate::art::SpriteFrame;
use std::fmt;

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    DestinationLayout,
    DestinationLength { required: usize, actual: usize },
    SpriteLayout,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DestinationLayout => write!(f, "invalid destination dimensions or stride"),
            Self::DestinationLength { required, actual } => {
                write!(f, "destination requires {required} pixels, got {actual}")
            }
            Self::SpriteLayout => write!(f, "sprite pixels do not match nonzero dimensions"),
        }
    }
}

impl std::error::Error for Error {}

/// A sprite whose hotspot is placed at a signed destination anchor.
#[derive(Clone, Copy)]
pub struct SpriteDraw<'a> {
    pub frame: &'a SpriteFrame,
    pub anchor: (i32, i32),
}

/// Sprite color and compositing operation. A floor grid covers four
/// 39 × 20 quadrants of a 78 × 40 sprite, with shared nodes in row order.
#[derive(Clone, Copy)]
pub enum Modulation<'a> {
    Unlit,
    /// Original indexed additive ART. Nonzero alpha selects a palette color.
    Additive,
    /// Original hover pass adds a color-modulated copy of the indexed sprite.
    AdditiveTint([u8; 3]),
    /// Original indexed shadow ART subtracted after grayscale multiplication.
    Shadow(u8),
    Uniform([u8; 3]),
    Floor([[u8; 3]; 9]),
    /// Screen-oriented multipliers, one per sprite column.
    Columns(&'a [[u8; 3]]),
    /// Opacity corners in clockwise order, starting in the top left.
    Fade {
        corners: [u8; 4],
        color: [u8; 3],
        columns: &'a [[u8; 3]],
    },
}

impl Modulation<'_> {
    pub fn floor(nodes: [[u8; 3]; 9]) -> Self {
        if nodes.iter().all(|&node| node == nodes[0]) {
            if nodes[0] == [255; 3] {
                Self::Unlit
            } else {
                Self::Uniform(nodes[0])
            }
        } else {
            Self::Floor(nodes)
        }
    }
}

pub(crate) fn multiply(source: [u8; 3], tint: [u8; 3]) -> [u8; 3] {
    std::array::from_fn(|i| (u16::from(source[i]) * u16::from(tint[i]) / 255) as u8)
}

// The two half-rows share the middle lattice node. Keep numerators
// until the pixel lookup so clipping and row setup never round early.
fn floor_row(nodes: &[[u8; 3]; 9], y: usize) -> [[[i32; 3]; 2]; 2] {
    let first = y / 20 * 3;
    let y = (y % 20) as i32;
    let vertical = |index: usize, channel: usize| {
        i32::from(nodes[index][channel]) * (20 - y) + i32::from(nodes[index + 3][channel]) * y
    };
    std::array::from_fn(|half| {
        let left = std::array::from_fn(|i| vertical(first + half, i));
        let right: [i32; 3] = std::array::from_fn(|i| vertical(first + half + 1, i));
        [
            left.map(|value| value * 39),
            std::array::from_fn(|i| right[i] - left[i]),
        ]
    })
}

/// Compose shaded and ordinary sprites in the same ordering and clipping pass.
pub fn blit_modulated<'a>(
    destination: &mut [[u8; 3]],
    dimensions: (u32, u32),
    stride: usize,
    draws: impl IntoIterator<Item = (SpriteDraw<'a>, Modulation<'a>)>,
) -> Result<(), Error> {
    for (draw, shading) in draws {
        let position = sprite_origin(draw.frame, draw.anchor);
        match shading {
            Modulation::Unlit => blit(destination, dimensions, stride, draw.frame, position)?,
            Modulation::Additive => blit_with::<ADD, _>(
                destination,
                dimensions,
                stride,
                draw.frame,
                position,
                |_| |rgb, _| rgb,
            )?,
            Modulation::AdditiveTint(color) => blit_with::<ADD, _>(
                destination,
                dimensions,
                stride,
                draw.frame,
                position,
                |_| move |rgb, _| multiply(rgb, color),
            )?,
            Modulation::Shadow(gray) => blit_with::<SUBTRACT, _>(
                destination,
                dimensions,
                stride,
                draw.frame,
                position,
                |_| move |rgb, _| multiply(rgb, [gray; 3]),
            )?,
            Modulation::Uniform(color) => {
                blit_with::<OVER, _>(
                    destination,
                    dimensions,
                    stride,
                    draw.frame,
                    position,
                    |_| move |rgb, _| multiply(rgb, color),
                )?;
            }
            Modulation::Floor(nodes) => {
                if (draw.frame.width, draw.frame.height) != (78, 40) {
                    return Err(Error::SpriteLayout);
                }
                blit_with::<OVER, _>(destination, dimensions, stride, draw.frame, position, |y| {
                    let row = floor_row(&nodes, y);
                    move |rgb, x| {
                        let [start, step] = row[usize::from(x >= 39)];
                        let x = (if x < 39 { x } else { x - 39 }) as i32;
                        multiply(
                            rgb,
                            std::array::from_fn(|i| ((start[i] + step[i] * x) / 780) as u8),
                        )
                    }
                })?;
            }
            Modulation::Fade {
                corners,
                color,
                columns,
            } => {
                if !columns.is_empty() && columns.len() != draw.frame.width as usize {
                    return Err(Error::SpriteLayout);
                }
                let w = draw.frame.width;
                let h = draw.frame.height;
                blit_with_alpha::<OVER, _>(
                    destination,
                    dimensions,
                    stride,
                    draw.frame,
                    position,
                    |_| |rgb, x| multiply(rgb, columns.get(x).copied().unwrap_or(color)),
                    |alpha, x, y| {
                        let (x, y) = (x as u32, y as u32);
                        let [tl, tr, br, bl] = corners.map(u32::from);
                        let left = tl * (h - y) + bl * y;
                        let right = tr * (h - y) + br * y;
                        let opacity = (left * (w - x) + right * x) / (w * h);
                        (u32::from(alpha) * opacity / 255) as u8
                    },
                )?;
            }
            Modulation::Columns(colors) => {
                if colors.len() != draw.frame.width as usize {
                    return Err(Error::SpriteLayout);
                }
                blit_with::<OVER, _>(
                    destination,
                    dimensions,
                    stride,
                    draw.frame,
                    position,
                    |_| |rgb, x| multiply(rgb, colors[x]),
                )?;
            }
        }
    }
    Ok(())
}

/// Draw in iterator order, back to front, over the existing RGB pixels.
/// The destination is not cleared. On error, earlier draws remain applied.
pub fn blit_ordered<'a>(
    destination: &mut [[u8; 3]],
    dimensions: (u32, u32),
    stride: usize,
    draws: impl IntoIterator<Item = SpriteDraw<'a>>,
) -> Result<(), Error> {
    for SpriteDraw { frame, anchor } in draws {
        blit(
            destination,
            dimensions,
            stride,
            frame,
            sprite_origin(frame, anchor),
        )?;
    }
    Ok(())
}

/// Align the stored hotspot with the anchor. Frame movement offsets are
/// separate metadata for consumers that update the anchor.
pub fn sprite_origin(frame: &SpriteFrame, anchor: (i32, i32)) -> (i64, i64) {
    (
        i64::from(anchor.0) - i64::from(frame.hotspot.0),
        i64::from(anchor.1) - i64::from(frame.hotspot.1),
    )
}

/// Original object blit scale applies to both the hotspot and the extent.
pub(crate) fn scaled_sprite_rect(draw: SpriteDraw<'_>, percent: u32) -> ((i64, i64), (i64, i64)) {
    let scale = |n: i64| n * i64::from(percent) / 100;
    let frame = draw.frame;
    (
        (
            i64::from(draw.anchor.0) - scale(i64::from(frame.hotspot.0)),
            i64::from(draw.anchor.1) - scale(i64::from(frame.hotspot.1)),
        ),
        (
            scale(i64::from(frame.width)),
            scale(i64::from(frame.height)),
        ),
    )
}

/// Composite one sprite at a signed top-left position over initialized RGB8.
/// Stride and dimensions count pixels. Row padding is left intact.
pub fn blit(
    destination: &mut [[u8; 3]],
    dimensions: (u32, u32),
    stride: usize,
    frame: &SpriteFrame,
    position: (i64, i64),
) -> Result<(), Error> {
    blit_with::<OVER, _>(destination, dimensions, stride, frame, position, |_| {
        |rgb, _| rgb
    })
}

const OVER: u8 = 0;
const ADD: u8 = 1;
const SUBTRACT: u8 = 2;

fn blit_with<const BLEND: u8, S: Fn([u8; 3], usize) -> [u8; 3]>(
    destination: &mut [[u8; 3]],
    dimensions: (u32, u32),
    stride: usize,
    frame: &SpriteFrame,
    position: (i64, i64),
    shade_row: impl Fn(usize) -> S,
) -> Result<(), Error> {
    blit_with_alpha::<BLEND, _>(
        destination,
        dimensions,
        stride,
        frame,
        position,
        shade_row,
        |alpha, _, _| alpha,
    )
}

fn blit_with_alpha<const BLEND: u8, S: Fn([u8; 3], usize) -> [u8; 3]>(
    destination: &mut [[u8; 3]],
    dimensions: (u32, u32),
    stride: usize,
    frame: &SpriteFrame,
    position: (i64, i64),
    shade_row: impl Fn(usize) -> S,
    alpha_at: impl Fn(u8, usize, usize) -> u8,
) -> Result<(), Error> {
    let (width, height) = dimensions;
    if width == 0 || height == 0 || stride < width as usize {
        return Err(Error::DestinationLayout);
    }
    let required = stride
        .checked_mul(height as usize - 1)
        .and_then(|offset| offset.checked_add(width as usize))
        .ok_or(Error::DestinationLayout)?;
    if destination.len() < required {
        return Err(Error::DestinationLength {
            required,
            actual: destination.len(),
        });
    }

    let source_width = frame.width as usize;
    if source_width == 0
        || frame.height == 0
        || source_width.checked_mul(frame.height as usize) != Some(frame.pixels.len())
    {
        return Err(Error::SpriteLayout);
    }
    let Some((left, source_x, columns)) = clipped_axis(position.0, frame.width, width) else {
        return Ok(());
    };
    let Some((top, source_y, rows)) = clipped_axis(position.1, frame.height, height) else {
        return Ok(());
    };
    for row in 0..rows {
        let shade = shade_row(source_y + row);
        let dest_start = (top + row) * stride + left;
        let source_start = (source_y + row) * source_width + source_x;
        let dest_row = &mut destination[dest_start..dest_start + columns];
        let source_row = &frame.pixels[source_start..source_start + columns];
        for (column, (dest, &[r, g, b, alpha])) in dest_row.iter_mut().zip(source_row).enumerate() {
            if BLEND != OVER {
                if alpha != 0 {
                    let color = shade([r, g, b], source_x + column);
                    for (dest, source) in dest.iter_mut().zip(color) {
                        *dest = if BLEND == ADD {
                            dest.saturating_add(source)
                        } else {
                            dest.saturating_sub(source)
                        };
                    }
                }
                continue;
            }
            match alpha_at(alpha, source_x + column, source_y + row) {
                0 => {}
                255 => *dest = shade([r, g, b], source_x + column),
                alpha => {
                    let color = shade([r, g, b], source_x + column);
                    let alpha = u16::from(alpha);
                    for (dest, source) in dest.iter_mut().zip(color) {
                        let weighted = u16::from(source) * alpha + u16::from(*dest) * (255 - alpha);
                        *dest = ((weighted + 127) / 255) as u8;
                    }
                }
            }
        }
    }
    Ok(())
}

fn clipped_axis(origin: i64, extent: u32, limit: u32) -> Option<(usize, usize, usize)> {
    // Widen before computing either edge, including for extreme signed inputs.
    let origin = i128::from(origin);
    let start = origin.max(0);
    let end = (origin + i128::from(extent)).min(i128::from(limit));
    if start >= end {
        return None;
    }
    Some((
        start as usize,
        (start - origin) as usize,
        (end - start) as usize,
    ))
}

#[cfg(test)]
mod tests;
