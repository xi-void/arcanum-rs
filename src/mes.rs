//! Standalone parser for Arcanum's numbered MES text tables.

mod encoding;

pub use encoding::{Encoding, windows_1252};
use std::collections::BTreeMap;
use std::fmt;

pub const MAX_INPUT_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_RECORDS: usize = 100_000;

/// A numeric message identifier.
pub type EntryId = i32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Location {
    /// One-based line number.
    pub line: usize,
    /// Zero-based offset into the original bytes.
    pub byte_offset: usize,
}

impl fmt::Display for Location {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {} (byte {})", self.line, self.byte_offset)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Duplicate {
    pub id: EntryId,
    pub previous: Location,
    pub replacement: Location,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    TooLarge,
    TooManyRecords,
    UnsupportedBom,
    ConflictingBom,
    InvalidUtf8,
    UndefinedWindows1252(u8),
    Nul,
    NoRecords,
    UnexpectedClosingBrace,
    NestedOpeningBrace,
    UnterminatedField,
    MissingText,
    InvalidId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Error {
    pub kind: ErrorKind,
    pub location: Location,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: ", self.location)?;
        match self.kind {
            ErrorKind::TooLarge => write!(f, "MES input exceeds the {MAX_INPUT_BYTES}-byte limit"),
            ErrorKind::TooManyRecords => {
                write!(f, "MES input exceeds the {MAX_RECORDS}-record limit")
            }
            ErrorKind::UnsupportedBom => {
                f.write_str("unsupported MES encoding (UTF-16 or UTF-32 BOM)")
            }
            ErrorKind::ConflictingBom => {
                f.write_str("UTF-8 BOM conflicts with the requested Windows-1252 encoding")
            }
            ErrorKind::InvalidUtf8 => f.write_str("invalid UTF-8"),
            ErrorKind::UndefinedWindows1252(byte) => {
                write!(f, "undefined Windows-1252 byte {byte:#04x}")
            }
            ErrorKind::Nul => f.write_str("unexpected NUL byte in MES input"),
            ErrorKind::NoRecords => f.write_str("no MES records found"),
            ErrorKind::UnexpectedClosingBrace => {
                f.write_str("unexpected closing brace outside a field")
            }
            ErrorKind::NestedOpeningBrace => {
                f.write_str("opening brace inside a field; expected a closing brace first")
            }
            ErrorKind::UnterminatedField => {
                f.write_str("unterminated field; expected a closing brace")
            }
            ErrorKind::MissingText => {
                f.write_str("expected a text field after the numeric identifier")
            }
            ErrorKind::InvalidId => {
                f.write_str("invalid identifier; expected a signed 32-bit decimal integer")
            }
        }
    }
}

impl std::error::Error for Error {}

#[derive(Debug, PartialEq, Eq)]
pub struct Mes {
    entries: BTreeMap<EntryId, String>,
}

#[derive(Debug, PartialEq, Eq)]
struct Value {
    text: String,
    location: Location,
}

impl Mes {
    /// Decode UTF-8 when valid, or Windows-1252 otherwise. A UTF-8 BOM
    /// explicitly selects UTF-8. Undefined Windows-1252 bytes are errors.
    pub fn parse(bytes: &[u8]) -> Result<Self, Error> {
        Ok(parse_source(bytes)?.mes)
    }

    /// Parse with an explicit encoding to disambiguate legacy byte sequences.
    pub fn parse_with_encoding(bytes: &[u8], encoding: Encoding) -> Result<Self, Error> {
        Ok(parse_source_with_encoding(bytes, Some(encoding))?.mes)
    }

    /// Parse native UTF-8 text.
    pub fn parse_text(text: &str) -> Result<Self, Error> {
        Self::parse_with_encoding(text.as_bytes(), Encoding::Utf8)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn get(&self, id: EntryId) -> Option<&str> {
        self.entries.get(&id).map(String::as_str)
    }

    /// Unique entries in ascending numeric order.
    pub fn entries(&self) -> impl Iterator<Item = (EntryId, &str)> + '_ {
        self.entries.iter().map(|(&id, value)| (id, value.as_str()))
    }

    fn parse_fields(input: &encoding::Input<'_>) -> Result<ParsedSource, Error> {
        let mut cursor = Cursor {
            bytes: input.bytes,
            offset: input.start,
            line: 1,
        };
        let mut entries: BTreeMap<EntryId, Value> = BTreeMap::new();
        let mut duplicates = Vec::new();
        while let Some(key) = cursor.field()? {
            if entries.len() + duplicates.len() == MAX_RECORDS {
                return Err(Error {
                    kind: ErrorKind::TooManyRecords,
                    location: key.opening,
                });
            }
            let id = std::str::from_utf8(key.bytes)
                .ok()
                .and_then(|key| key.trim_ascii().parse::<EntryId>().ok())
                .ok_or(Error {
                    kind: ErrorKind::InvalidId,
                    location: key.opening,
                })?;
            let value = cursor
                .field()?
                .ok_or_else(|| cursor.error(ErrorKind::MissingText))?;
            let text = input.decode_field(value.bytes);
            let text = if text.contains('\r') {
                text.replace("\r\n", "\n").replace('\r', "\n")
            } else {
                text
            };
            let location = key.opening;
            if let Some(previous) = entries.insert(id, Value { text, location }) {
                duplicates.push(Duplicate {
                    id,
                    previous: previous.location,
                    replacement: location,
                });
            }
        }
        if entries.is_empty() {
            return Err(cursor.error(ErrorKind::NoRecords));
        }
        let entries = entries
            .into_iter()
            .map(|(id, value)| (id, value.text))
            .collect();
        Ok(ParsedSource {
            mes: Self { entries },
            encoding: input.encoding,
            duplicates,
        })
    }
}

/// A parsed table with metadata about its input representation.
#[derive(Debug, PartialEq, Eq)]
pub struct ParsedSource {
    pub mes: Mes,
    pub encoding: Encoding,
    /// Duplicate occurrences in source order. The last value for each ID wins.
    pub duplicates: Vec<Duplicate>,
}

/// Parse resource bytes and report the selected source decoding.
pub fn parse_source(bytes: &[u8]) -> Result<ParsedSource, Error> {
    parse_source_with_encoding(bytes, None)
}

pub(crate) fn parse_source_with_encoding(
    bytes: &[u8],
    encoding: Option<Encoding>,
) -> Result<ParsedSource, Error> {
    let input = encoding::Input::new(bytes, encoding)?;
    Mes::parse_fields(&input)
}

/// Read the same brace-delimited text fields used by legacy DLG resources.
pub(crate) fn fields(
    bytes: &[u8],
) -> Result<impl Iterator<Item = Result<String, Error>> + '_, Error> {
    let input = encoding::Input::new(bytes, None)?;
    let mut cursor = Cursor {
        bytes,
        offset: input.start,
        line: 1,
    };
    Ok(std::iter::from_fn(move || {
        cursor.field().transpose().map(|field| {
            field.map(|field| {
                input
                    .decode_field(field.bytes)
                    .replace("\r\n", "\n")
                    .replace('\r', "\n")
            })
        })
    }))
}

struct Field<'a> {
    bytes: &'a [u8],
    opening: Location,
}

struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
    line: usize,
}

impl<'a> Cursor<'a> {
    fn field(&mut self) -> Result<Option<Field<'a>>, Error> {
        loop {
            match self.peek() {
                None => return Ok(None),
                Some(b'{') => break,
                Some(b'}') => return Err(self.error(ErrorKind::UnexpectedClosingBrace)),
                Some(b'/') if self.bytes[self.offset..].starts_with(b"//") => {
                    while self
                        .peek()
                        .is_some_and(|byte| !matches!(byte, b'\r' | b'\n'))
                    {
                        self.advance();
                    }
                }
                Some(_) => self.advance(),
            }
        }
        let opening = self.location();
        self.advance();
        let start = self.offset;
        loop {
            match self.peek() {
                Some(b'}') => {
                    let bytes = &self.bytes[start..self.offset];
                    self.advance();
                    return Ok(Some(Field { bytes, opening }));
                }
                Some(b'{') => return Err(self.error(ErrorKind::NestedOpeningBrace)),
                Some(_) => self.advance(),
                None => {
                    return Err(Error {
                        kind: ErrorKind::UnterminatedField,
                        location: opening,
                    });
                }
            }
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.offset).copied()
    }

    fn advance(&mut self) {
        let current = self.bytes[self.offset];
        self.offset += 1;
        if current == b'\n' || (current == b'\r' && self.peek() != Some(b'\n')) {
            self.line += 1;
        }
    }

    fn location(&self) -> Location {
        Location {
            line: self.line,
            byte_offset: self.offset,
        }
    }

    fn error(&self, kind: ErrorKind) -> Error {
        Error {
            kind,
            location: self.location(),
        }
    }
}

#[cfg(test)]
mod tests;
