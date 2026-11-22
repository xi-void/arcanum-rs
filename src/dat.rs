//! Arcanum DAT metadata and bounded payload extraction.

use crate::resource_path;
use flate2::{Decompress, FlushDecompress, Status};
use std::error::Error as StdError;
use std::fmt;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::Path;

const PLAIN: u32 = 0x001;
const ZLIB: u32 = 0x002;
const CE_RUNTIME_FLAGS: u32 = 0x100 | 0x200;
const DIRECTORY: u32 = 0x400;
const IGNORED: u32 = 0x800;
const MIN_RECORD_SIZE: u64 = 26;
const BUFFER_SIZE: usize = 32 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Dat,
    Dat1 { guid: [u8; 16] },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArchiveMetadata {
    pub format: Format,
    pub archive_offset: u64,
    pub index_offset: u64,
    pub name_table_size: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    Plain,
    Zlib,
    Directory,
    Ignored,
    Unsupported,
}

impl fmt::Display for EntryKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Plain => "plain",
            Self::Zlib => "zlib",
            Self::Directory => "directory",
            Self::Ignored => "ignored",
            Self::Unsupported => "unsupported",
        })
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Entry {
    pub path: String,
    pub kind: EntryKind,
    pub flags: u32,
    pub size: u32,
    /// Raw stored-size field. Plain entries use `size` as their payload length.
    pub stored_size: u32,
    /// Physical byte offset for a supported file payload.
    pub data_offset: Option<u64>,
}

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    InvalidArchive(String),
    UnsupportedArchive([u8; 4]),
    UnsupportedNameEncoding {
        record: u32,
    },
    InvalidPath {
        path: String,
    },
    EntryNotFound(String),
    UnreadableEntry {
        path: String,
        kind: EntryKind,
        flags: u32,
    },
    InvalidPayload {
        path: String,
        reason: String,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => err.fmt(f),
            Self::InvalidArchive(reason) => write!(f, "invalid DAT archive: {reason}"),
            Self::UnsupportedArchive(magic) => write!(
                f,
                "unsupported DAT footer signature {magic:02x?} (expected ' TAD' or '1TAD')"
            ),
            Self::UnsupportedNameEncoding { record } => write!(
                f,
                "unsupported resource name encoding in DAT record {record} (expected UTF-8)"
            ),
            Self::InvalidPath { path } => write!(
                f,
                "invalid virtual resource path {path:?}: {}",
                resource_path::InvalidResourcePath
            ),
            Self::EntryNotFound(path) => write!(f, "resource {path:?} not found"),
            Self::UnreadableEntry { path, kind, flags } => write!(
                f,
                "cannot extract {path:?}: {kind} entry (flags {flags:#x})"
            ),
            Self::InvalidPayload { path, reason } => {
                write!(f, "invalid payload for {path:?}: {reason}")
            }
        }
    }
}

impl StdError for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            _ => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Self {
        Self::Io(err)
    }
}

/// An indexed archive. The reader is retained for on-demand payload access.
pub struct Archive<R> {
    source: BufReader<R>,
    metadata: ArchiveMetadata,
    entries: Vec<Entry>,
    by_path: Vec<usize>,
}

impl Archive<File> {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, Error> {
        Self::new(File::open(path)?)
    }
}

impl<R: Read + Seek> Archive<R> {
    pub fn new(source: R) -> Result<Self, Error> {
        let mut source = BufReader::new(source);
        let end = source.seek(SeekFrom::End(0))?;
        let signature_offset = subtract(end, 12, "missing footer")?;
        source.seek(SeekFrom::Start(signature_offset))?;
        let magic = read_array(&mut source)?;
        let name_table_size = read_u32(&mut source)?;
        let index_distance = u64::from(read_u32(&mut source)?);

        let (format, footer_offset) = match &magic {
            b" TAD" => (Format::Dat, signature_offset),
            b"1TAD" => {
                let footer_offset = subtract(signature_offset, 16, "missing DAT1 GUID")?;
                source.seek(SeekFrom::Start(footer_offset))?;
                let guid = read_array(&mut source)?;
                (Format::Dat1 { guid }, footer_offset)
            }
            _ => return Err(Error::UnsupportedArchive(magic)),
        };

        let index_offset = subtract(end - 4, index_distance, "index offset precedes archive")?;
        let index_size = subtract(footer_offset, index_offset, "index overlaps footer")?;
        if index_size < 8 {
            return Err(invalid("index is too short for its header"));
        }
        source.seek(SeekFrom::Start(index_offset))?;
        let mut index = (&mut source).take(index_size);
        let payload_span = u64::from(read_u32(&mut index)?);
        let payload_size = subtract(payload_span, 4, "payload span omits its size word")?;
        let archive_offset =
            subtract(index_offset, payload_size, "payload starts before the file")?;
        let count = read_u32(&mut index)?;
        let bytes_for_records = u64::from(count) * 24 + u64::from(name_table_size);
        if bytes_for_records != index.limit() || u64::from(count) * MIN_RECORD_SIZE > index.limit()
        {
            return Err(invalid(
                "entry count and name table size do not fit the index",
            ));
        }

        let mut entries = Vec::new();
        let mut names_remaining = u64::from(name_table_size);
        for record in 0..count {
            let name_size = u64::from(read_u32(&mut index)?);
            let remaining_records = u64::from(count - record - 1);
            if name_size < 2
                || name_size + 20 + remaining_records * MIN_RECORD_SIZE > index.limit()
                || name_size + remaining_records * 2 > names_remaining
            {
                return Err(invalid(format!("invalid name length in record {record}")));
            }
            names_remaining -= name_size;
            // Grow as bytes arrive, including when a declared index is larger than its source.
            let mut name = Vec::new();
            (&mut index).take(name_size).read_to_end(&mut name)?;
            if name.len() as u64 != name_size || name.pop() != Some(0) || name.contains(&0) {
                return Err(invalid(format!(
                    "missing or embedded name terminator in record {record}"
                )));
            }
            let name = std::str::from_utf8(&name)
                .map_err(|_| Error::UnsupportedNameEncoding { record })?;
            let path = resource_path::normalize(name).map_err(|_| {
                invalid(format!("invalid virtual path {name:?} in record {record}"))
            })?;

            let _reserved = read_u32(&mut index)?;
            let flags = read_u32(&mut index)?;
            let size = read_u32(&mut index)?;
            let stored_size = read_u32(&mut index)?;
            let relative_offset = read_u32(&mut index)?;
            let kind = entry_kind(flags);
            let data_offset = match kind {
                EntryKind::Plain | EntryKind::Zlib => {
                    let length = if kind == EntryKind::Plain {
                        size
                    } else {
                        stored_size
                    };
                    if u64::from(relative_offset) + u64::from(length) > payload_size {
                        return Err(invalid(format!(
                            "payload for {path:?} extends outside the data region"
                        )));
                    }
                    Some(archive_offset + u64::from(relative_offset))
                }
                _ => None,
            };
            entries.push(Entry {
                path,
                kind,
                flags,
                size,
                stored_size,
                data_offset,
            });
        }
        if index.limit() != 0 || names_remaining != 0 {
            return Err(invalid("index lengths do not match the declared metadata"));
        }

        let mut by_path: Vec<_> = (0..entries.len()).collect();
        by_path.sort_unstable_by(|&a, &b| entries[a].path.cmp(&entries[b].path));
        for pair in by_path.windows(2) {
            let path = &entries[pair[0]].path;
            if *path == entries[pair[1]].path {
                return Err(invalid(format!(
                    "duplicate normalized resource path {path:?}"
                )));
            }
        }
        let metadata = ArchiveMetadata {
            format,
            archive_offset,
            index_offset,
            name_table_size,
        };
        Ok(Self {
            source,
            metadata,
            entries,
            by_path,
        })
    }

    pub fn metadata(&self) -> ArchiveMetadata {
        self.metadata
    }

    /// Entries in index order, including directory and unsupported records.
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn entry(&self, path: &str) -> Result<&Entry, Error> {
        self.find(path).map(|index| &self.entries[index])
    }

    pub(crate) fn entry_normalized(&self, key: &str) -> Option<&Entry> {
        self.find_normalized(key).map(|index| &self.entries[index])
    }

    /// Copy one resource as binary data. On error, the writer may contain a prefix.
    pub fn extract(&mut self, path: &str, mut output: impl Write) -> Result<u64, Error> {
        let entry = &self.entries[self.find(path)?];
        let offset = entry.data_offset.ok_or_else(|| Error::UnreadableEntry {
            path: entry.path.clone(),
            kind: entry.kind,
            flags: entry.flags,
        })?;
        self.source.seek(SeekFrom::Start(offset))?;
        let size = u64::from(entry.size);
        if entry.kind == EntryKind::Plain {
            let copied = io::copy(&mut (&mut self.source).take(size), &mut output)?;
            if copied != size {
                return Err(payload_error(
                    entry,
                    format!("expected {size} bytes, got {copied}"),
                ));
            }
        } else {
            let input = (&mut self.source).take(u64::from(entry.stored_size));
            decode_zlib(input, &mut output, entry)?;
        }
        Ok(size)
    }

    fn find(&self, path: &str) -> Result<usize, Error> {
        let key =
            resource_path::normalize(path).map_err(|_| Error::InvalidPath { path: path.into() })?;
        self.find_normalized(&key).ok_or(Error::EntryNotFound(key))
    }

    fn find_normalized(&self, key: &str) -> Option<usize> {
        self.by_path
            .binary_search_by(|&index| self.entries[index].path.as_str().cmp(key))
            .ok()
            .map(|index| self.by_path[index])
    }
}

fn entry_kind(flags: u32) -> EntryKind {
    match flags & !CE_RUNTIME_FLAGS {
        flags if flags & IGNORED != 0 => EntryKind::Ignored,
        DIRECTORY => EntryKind::Directory,
        PLAIN => EntryKind::Plain,
        ZLIB => EntryKind::Zlib,
        _ => EntryKind::Unsupported,
    }
}

fn decode_zlib<R: BufRead>(
    mut input: io::Take<R>,
    output: &mut impl Write,
    entry: &Entry,
) -> Result<(), Error> {
    let mut decoder = Decompress::new(true);
    let mut buffer = [0; BUFFER_SIZE];
    let expected = u64::from(entry.size);
    loop {
        let (before_in, before_out) = (decoder.total_in(), decoder.total_out());
        let status = decoder
            .decompress(input.fill_buf()?, &mut buffer, FlushDecompress::None)
            .map_err(|err| payload_error(entry, format!("zlib: {err}")))?;
        let consumed = decoder.total_in() - before_in;
        let produced = decoder.total_out() - before_out;

        if decoder.total_out() > expected {
            return Err(payload_error(
                entry,
                format!("zlib output exceeds declared size of {expected} bytes"),
            ));
        }
        input.consume(consumed as usize);
        output.write_all(&buffer[..produced as usize])?;

        if status == Status::StreamEnd {
            if decoder.total_out() != expected {
                return Err(payload_error(
                    entry,
                    format!(
                        "expected {expected} decompressed bytes, got {}",
                        decoder.total_out()
                    ),
                ));
            }
            if input.limit() != 0 {
                return Err(payload_error(
                    entry,
                    "zlib stream ends before declared stored size",
                ));
            }
            return Ok(());
        }
        if consumed == 0 && produced == 0 {
            return Err(payload_error(entry, "incomplete zlib stream"));
        }
    }
}

fn subtract(a: u64, b: u64, reason: &str) -> Result<u64, Error> {
    a.checked_sub(b).ok_or_else(|| invalid(reason))
}

fn read_array<const N: usize>(input: &mut impl Read) -> Result<[u8; N], Error> {
    let mut bytes = [0; N];
    input.read_exact(&mut bytes).map_err(|err| {
        if err.kind() == io::ErrorKind::UnexpectedEof {
            invalid("truncated metadata")
        } else {
            err.into()
        }
    })?;
    Ok(bytes)
}

fn read_u32(input: &mut impl Read) -> Result<u32, Error> {
    read_array(input).map(u32::from_le_bytes)
}

fn invalid(reason: impl Into<String>) -> Error {
    Error::InvalidArchive(reason.into())
}

fn payload_error(entry: &Entry, reason: impl Into<String>) -> Error {
    Error::InvalidPayload {
        path: entry.path.clone(),
        reason: reason.into(),
    }
}

#[cfg(test)]
mod tests;
