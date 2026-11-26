//! Text decoding at the MES resource boundary.

use super::{Cursor, Error, ErrorKind, MAX_INPUT_BYTES};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    Utf8,
    Windows1252,
}

impl fmt::Display for Encoding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Utf8 => "UTF-8",
            Self::Windows1252 => "Windows-1252",
        })
    }
}

pub(super) struct Input<'a> {
    pub bytes: &'a [u8],
    pub start: usize,
    pub encoding: Encoding,
}

impl<'a> Input<'a> {
    pub fn new(bytes: &'a [u8], requested: Option<Encoding>) -> Result<Self, Error> {
        let encoding = detect_encoding(bytes, requested)?;
        let start = if bytes.starts_with(b"\xef\xbb\xbf") {
            3
        } else {
            0
        };
        Ok(Self {
            bytes,
            start,
            encoding,
        })
    }

    pub fn decode_field(&self, field: &[u8]) -> String {
        match self.encoding {
            Encoding::Utf8 => std::str::from_utf8(field)
                .expect("validated text at ASCII delimiters")
                .to_owned(),
            Encoding::Windows1252 => field
                .iter()
                .map(|&byte| windows_1252(byte).expect("validated Windows-1252 input"))
                .collect(),
        }
    }
}

fn detect_encoding(bytes: &[u8], requested: Option<Encoding>) -> Result<Encoding, Error> {
    let error_at = |kind, byte_offset| {
        let mut cursor = Cursor {
            bytes,
            offset: 0,
            line: 1,
        };
        while cursor.offset < byte_offset {
            cursor.advance();
        }
        cursor.error(kind)
    };
    if bytes.len() > MAX_INPUT_BYTES {
        return Err(error_at(ErrorKind::TooLarge, MAX_INPUT_BYTES));
    }
    if bytes.starts_with(b"\xff\xfe")
        || bytes.starts_with(b"\xfe\xff")
        || bytes.starts_with(b"\0\0\xfe\xff")
    {
        return Err(error_at(ErrorKind::UnsupportedBom, 0));
    }
    if bytes.starts_with(b"\xef\xbb\xbf") && requested == Some(Encoding::Windows1252) {
        return Err(error_at(ErrorKind::ConflictingBom, 0));
    }
    if let Some(offset) = bytes.iter().position(|&byte| byte == 0) {
        return Err(error_at(ErrorKind::Nul, offset));
    }
    let utf8 = std::str::from_utf8(bytes);
    let encoding = requested.unwrap_or_else(|| {
        if bytes.starts_with(b"\xef\xbb\xbf") || utf8.is_ok() {
            Encoding::Utf8
        } else {
            Encoding::Windows1252
        }
    });
    match encoding {
        Encoding::Utf8 => {
            utf8.map_err(|err| error_at(ErrorKind::InvalidUtf8, err.valid_up_to()))?;
        }
        Encoding::Windows1252 => {
            if let Some(offset) = bytes.iter().position(|&byte| windows_1252(byte).is_none()) {
                return Err(error_at(
                    ErrorKind::UndefinedWindows1252(bytes[offset]),
                    offset,
                ));
            }
        }
    }
    Ok(encoding)
}

pub fn windows_1252(byte: u8) -> Option<char> {
    // Bytes outside the C1 range have the same code points as Latin-1.
    const C1: [Option<char>; 32] = [
        Some('€'),
        None,
        Some('‚'),
        Some('ƒ'),
        Some('„'),
        Some('…'),
        Some('†'),
        Some('‡'),
        Some('ˆ'),
        Some('‰'),
        Some('Š'),
        Some('‹'),
        Some('Œ'),
        None,
        Some('Ž'),
        None,
        None,
        Some('‘'),
        Some('’'),
        Some('“'),
        Some('”'),
        Some('•'),
        Some('–'),
        Some('—'),
        Some('˜'),
        Some('™'),
        Some('š'),
        Some('›'),
        Some('œ'),
        None,
        Some('ž'),
        Some('Ÿ'),
    ];
    if (0x80..0xa0).contains(&byte) {
        C1[usize::from(byte - 0x80)]
    } else {
        Some(char::from(byte))
    }
}
