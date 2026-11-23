//! Bounded decoder for palette-indexed legacy ART sprites.

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
    pub rotation_count: u8,
    pub stored_rotation: u8,
    pub stored_palette: u8,
    /// Original RGB of palette index zero, before sprite transparency.
    pub palette_key: [u8; 3],
    pub frames_per_second: i32,
    /// Stored animation action-frame marker.
    pub action_frame: i32,
    /// Bytes past the last declared frame payload. They do not enter pixel decoding.
    pub ignored_trailing_bytes: usize,
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
/// Bytes after all declared frames count toward the input limit and are reported.
pub fn decode_frame(input: &[u8], index: usize) -> Result<DecodedFrame, Error> {
    decode_frame_with_palette(input, index, 0)
}

/// Decode direction zero with the requested palette, falling back to slot zero
/// when the requested slot is absent.
pub fn decode_frame_with_palette(
    input: &[u8],
    index: usize,
    palette_slot: u8,
) -> Result<DecodedFrame, Error> {
    decode_direction(input, index, 0, palette_slot)
}

/// Decode one stored direction. Single-direction ART aliases every rotation to zero.
pub fn decode_direction(
    input: &[u8],
    index: usize,
    rotation: u8,
    palette_slot: u8,
) -> Result<DecodedFrame, Error> {
    if palette_slot >= 4 {
        return Err(Error::Unsupported("ART palette slot must be in 0..4"));
    }
    if rotation >= 8 {
        return Err(Error::Unsupported("ART rotation must be in 0..8"));
    }
    if input.len() > MAX_INPUT_BYTES {
        return Err(Error::InputTooLarge);
    }
    let header = section(input, 0, HEADER_BYTES, "header")?;
    let rotation_count = match word(header, 0) {
        0 | 0x02 => 8,
        0x01 | 0x05 | 0x09 => 1,
        _ => {
            return Err(Error::Unsupported(
                "expected verified ART flags 0x00, 0x01, 0x02, 0x05 or 0x09",
            ));
        }
    };
    let stored_rotation = if rotation_count == 1 { 0 } else { rotation };
    if word(header, 8) != 8 {
        return Err(Error::Unsupported(
            "only 8-bit palette indices are supported",
        ));
    }
    if word(header, 12) == 0 {
        return Err(Error::Unsupported("expected palette slot zero"));
    }
    let count = usize::try_from(word(header, 32))
        .ok()
        .filter(|count| (1..=MAX_FRAMES).contains(count))
        .ok_or_else(|| invalid(32, "frame count must be between 1 and 1024"))?;
    if index >= count {
        return Err(Error::FrameOutOfRange { index, count });
    }

    let mut palette = section(input, HEADER_BYTES, PALETTE_BYTES, "palette zero")?;
    let mut stored_palette = 0;
    let mut table_start = HEADER_BYTES + PALETTE_BYTES;
    for slot in 1..4 {
        if word(header, 12 + usize::from(slot) * 4) != 0 {
            let alternate = section(input, table_start, PALETTE_BYTES, "alternate palette")?;
            if palette_slot == slot {
                palette = alternate;
                stored_palette = slot;
            }
            table_start += PALETTE_BYTES;
        }
    }
    let selected_index = usize::from(stored_rotation) * count + index;
    let table = section(
        input,
        table_start,
        count * usize::from(rotation_count) * FRAME_BYTES,
        "frame table",
    )?;
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
        if number == selected_index {
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
        rotation_count,
        stored_rotation,
        stored_palette,
        palette_key: [palette[2], palette[1], palette[0]],
        frames_per_second: word(header, 4),
        action_frame: word(header, 28),
        ignored_trailing_bytes: input.len() - payload_offset,
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
