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

/// Align the stored hotspot with the anchor. Frame movement offsets are
/// separate metadata for consumers that update the anchor.
pub fn sprite_origin(frame: &SpriteFrame, anchor: (i32, i32)) -> (i64, i64) {
    (
        i64::from(anchor.0) - i64::from(frame.hotspot.0),
        i64::from(anchor.1) - i64::from(frame.hotspot.1),
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
        let dest_start = (top + row) * stride + left;
        let source_start = (source_y + row) * source_width + source_x;
        let dest_row = &mut destination[dest_start..dest_start + columns];
        let source_row = &frame.pixels[source_start..source_start + columns];
        for (dest, &[r, g, b, alpha]) in dest_row.iter_mut().zip(source_row) {
            let color = [r, g, b];
            match alpha {
                0 => {}
                255 => *dest = color,
                alpha => {
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
