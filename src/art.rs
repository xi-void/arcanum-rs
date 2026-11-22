//! Bounded decoder for single-direction, palette-indexed legacy ART sprites.

use std::fmt;

pub const MAX_INPUT_BYTES: usize = 32 * 1024 * 1024;
const MAX_FRAMES: usize = 1024;
const MAX_FRAME_PIXELS: usize = 4 * 1024 * 1024;
const HEADER_BYTES: usize = 132;
const PALETTE_BYTES: usize = 256 * 4;
const FRAME_BYTES: usize = 28;

/// A selected frame as straight RGBA8 in row-major order, top row first.
#[derive(Debug, PartialEq, Eq)]
pub struct SpriteFrame {
    pub width: u32,
    pub height: u32,
    /// Signed pixel location of the hotspot relative to the top-left pixel.
    pub hotspot: (i32, i32),
    /// Signed movement offsets stored for this frame, in pixels.
    pub offset: (i32, i32),
    pub pixels: Vec<[u8; 4]>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct DecodedFrame {
    pub frame: SpriteFrame,
    pub frame_count: usize,
    pub frames_per_second: i32,
    /// Stored animation action-frame marker.
    pub action_frame: i32,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    InputTooLarge,
    Unsupported(&'static str),
    Truncated {
        offset: usize,
        section: &'static str,
    },
    Invalid {
        offset: usize,
        reason: &'static str,
    },
    FrameOutOfRange {
        index: usize,
        count: usize,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InputTooLarge => write!(f, "ART input exceeds {MAX_INPUT_BYTES} bytes"),
            Self::Unsupported(reason) => write!(f, "unsupported ART format: {reason}"),
            Self::Truncated { offset, section } => {
                write!(f, "truncated ART {section} at byte {offset}")
            }
            Self::Invalid { offset, reason } => write!(f, "invalid ART at byte {offset}: {reason}"),
            Self::FrameOutOfRange { index, count } => {
                write!(
                    f,
                    "ART frame {index} is out of range for {count} frames (zero-based)"
                )
            }
        }
    }
}

impl std::error::Error for Error {}

/// Decode one zero-based frame. Check all frame dimensions and payload spans.
/// Only the selected frame's RLE packets are decoded and checked.
pub fn decode_frame(input: &[u8], index: usize) -> Result<DecodedFrame, Error> {
    if input.len() > MAX_INPUT_BYTES {
        return Err(Error::InputTooLarge);
    }
    let header = section(input, 0, HEADER_BYTES, "header")?;
    if word(header, 0) != 1 {
        return Err(Error::Unsupported(
            "only the single-direction flag (1) is supported",
        ));
    }
    if word(header, 8) != 8 {
        return Err(Error::Unsupported(
            "only 8-bit palette indices are supported",
        ));
    }
    if word(header, 12) == 0 || [16, 20, 24].into_iter().any(|pos| word(header, pos) != 0) {
        return Err(Error::Unsupported("expected a palette in slot 0 only"));
    }
    let count = usize::try_from(word(header, 32))
        .ok()
        .filter(|count| (1..=MAX_FRAMES).contains(count))
        .ok_or_else(|| invalid(32, "frame count must be between 1 and 1024"))?;
    if index >= count {
        return Err(Error::FrameOutOfRange { index, count });
    }

    let palette = section(input, HEADER_BYTES, PALETTE_BYTES, "palette")?;
    let table_start = HEADER_BYTES + PALETTE_BYTES;
    let table = section(input, table_start, count * FRAME_BYTES, "frame table")?;
    let mut payload_offset = table_start + table.len();
    let mut selected = None;
    for (number, record) in table.as_chunks::<FRAME_BYTES>().0.iter().enumerate() {
        let record_offset = table_start + number * FRAME_BYTES;
        let width = word(record, 0);
        let height = word(record, 4);
        let pixel_count = usize::try_from(width)
            .ok()
            .zip(usize::try_from(height).ok())
            .and_then(|(width, height)| width.checked_mul(height))
            .filter(|pixels| (1..=MAX_FRAME_PIXELS).contains(pixels))
            .ok_or_else(|| {
                invalid(
                    record_offset,
                    "dimensions must be positive with at most 4194304 pixels per frame",
                )
            })?;
        let stored_size = usize::try_from(word(record, 8))
            .ok()
            .filter(|size| *size != 0)
            .ok_or_else(|| invalid(record_offset + 8, "stored frame length must be positive"))?;
        let payload = section(input, payload_offset, stored_size, "frame payload")?;
        if number == index {
            selected = Some((
                record,
                width as u32,
                height as u32,
                pixel_count,
                payload_offset,
                payload,
            ));
        }
        // The verified slice proves the end is representable and within input.
        payload_offset += payload.len();
    }
    if payload_offset != input.len() {
        return Err(Error::Unsupported("data after the final frame payload"));
    }

    let (record, width, height, pixel_count, offset, payload) =
        selected.expect("index lies in the checked frame table");
    let mut colors = [[0; 4]; 256];
    for (color, stored) in colors.iter_mut().zip(palette.as_chunks::<4>().0).skip(1) {
        *color = [stored[2], stored[1], stored[0], 255];
    }
    let pixels = if payload.len() == pixel_count {
        payload
            .iter()
            .map(|&index| colors[usize::from(index)])
            .collect()
    } else {
        decode_rle(payload, offset, pixel_count, &colors)?
    };
    Ok(DecodedFrame {
        frame: SpriteFrame {
            width,
            height,
            hotspot: (word(record, 12), word(record, 16)),
            offset: (word(record, 20), word(record, 24)),
            pixels,
        },
        frame_count: count,
        frames_per_second: word(header, 4),
        action_frame: word(header, 28),
    })
}

fn decode_rle(
    data: &[u8],
    base: usize,
    pixel_count: usize,
    colors: &[[u8; 4]; 256],
) -> Result<Vec<[u8; 4]>, Error> {
    let mut pixels = Vec::with_capacity(pixel_count);
    let mut pos = 0;
    while let Some(&control) = data.get(pos) {
        let len = usize::from(control & 0x7f);
        if len == 0 || len > pixel_count - pixels.len() {
            return Err(invalid(base + pos, "invalid RLE run length"));
        }
        pos += 1;
        if control & 0x80 == 0 {
            let &color = data.get(pos).ok_or(Error::Truncated {
                offset: base + pos,
                section: "RLE repeat index",
            })?;
            pixels.resize(pixels.len() + len, colors[usize::from(color)]);
            pos += 1;
        } else {
            let literals = data.get(pos..pos + len).ok_or(Error::Truncated {
                offset: base + pos,
                section: "RLE literal indices",
            })?;
            pixels.extend(literals.iter().map(|&color| colors[usize::from(color)]));
            pos += len;
        }
    }
    if pixels.len() != pixel_count {
        return Err(invalid(base + pos, "RLE output does not fill the frame"));
    }
    Ok(pixels)
}

fn section<'a>(
    input: &'a [u8],
    offset: usize,
    length: usize,
    section: &'static str,
) -> Result<&'a [u8], Error> {
    input
        .get(offset..)
        .and_then(|remaining| remaining.get(..length))
        .ok_or(Error::Truncated { offset, section })
}

fn word(record: &[u8], offset: usize) -> i32 {
    i32::from_le_bytes(
        record[offset..offset + 4]
            .try_into()
            .expect("fixed ART record layout"),
    )
}

fn invalid(offset: usize, reason: &'static str) -> Error {
    Error::Invalid { offset, reason }
}

#[cfg(test)]
mod tests;
