use super::*;
use flate2::{Compression, write::ZlibEncoder};
use std::io::Cursor;

const GUID: [u8; 16] = *b"0123456789abcdef";

struct Record<'a> {
    name: &'a [u8],
    flags: u32,
    size: u32,
    stored_size: u32,
    offset: u32,
}

fn fixture(prefix: &[u8], payload: &[u8], records: &[Record<'_>], format: Format) -> Vec<u8> {
    let mut bytes = [prefix, payload].concat();
    let index = bytes.len();
    append_u32(&mut bytes, payload.len() as u32 + 4);
    append_u32(&mut bytes, records.len() as u32);
    let mut name_bytes = 0;
    for record in records {
        let name_len = record.name.len() as u32 + 1;
        name_bytes += name_len;
        append_u32(&mut bytes, name_len);
        bytes.extend_from_slice(record.name);
        bytes.push(0);
        for value in [
            0xdead_beef,
            record.flags,
            record.size,
            record.stored_size,
            record.offset,
        ] {
            append_u32(&mut bytes, value);
        }
    }
    match format {
        Format::Dat => bytes.extend_from_slice(b" TAD"),
        Format::Dat1 { guid } => {
            bytes.extend_from_slice(&guid);
            bytes.extend_from_slice(b"1TAD");
        }
    }
    append_u32(&mut bytes, name_bytes);
    let distance = (bytes.len() - index) as u32;
    append_u32(&mut bytes, distance);
    bytes
}

fn append_u32(bytes: &mut Vec<u8>, number: u32) {
    bytes.extend_from_slice(&number.to_le_bytes());
}

fn replace_u32(bytes: &mut [u8], offset: usize, number: u32) {
    bytes[offset..offset + 4].copy_from_slice(&number.to_le_bytes());
}

fn read(bytes: Vec<u8>) -> Result<Archive<Cursor<Vec<u8>>>, Error> {
    Archive::new(Cursor::new(bytes))
}

fn zlib(data: &[u8]) -> Vec<u8> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(data).unwrap();
    encoder.finish().unwrap()
}

fn plain(name: &[u8]) -> Record<'_> {
    Record {
        name,
        flags: PLAIN,
        size: 4,
        stored_size: 4,
        offset: 0,
    }
}

#[test]
fn reads_dat1_index_and_extracts_plain_and_zlib_from_a_prefixed_archive() {
    let raw = (0..100_000).map(|n| (n % 251) as u8).collect::<Vec<_>>();
    let compressed = zlib(&raw);
    let payload = [b"raw!", compressed.as_slice()].concat();
    let prefix = b"archive prefix";
    let records = [
        plain(br"Art\Plain"),
        Record {
            name: br"Art\COMP",
            flags: ZLIB,
            size: raw.len() as u32,
            stored_size: compressed.len() as u32,
            offset: 4,
        },
        Record {
            name: b"Art",
            flags: DIRECTORY,
            size: 0,
            stored_size: 0,
            offset: u32::MAX,
        },
    ];
    let format = Format::Dat1 { guid: GUID };
    let mut archive = read(fixture(prefix, &payload, &records, format)).unwrap();
    assert_eq!(
        archive.metadata(),
        ArchiveMetadata {
            format,
            archive_offset: prefix.len() as u64,
            index_offset: (prefix.len() + payload.len()) as u64,
            name_table_size: 23,
        }
    );
    let paths = archive
        .entries()
        .iter()
        .map(|entry| entry.path.as_str())
        .collect::<Vec<_>>();
    assert_eq!(paths, ["art/plain", "art/comp", "art"]);
    assert_eq!(
        archive.entry(".//ART\\comp").unwrap().data_offset,
        Some((prefix.len() + 4) as u64)
    );
    assert!(matches!(
        archive.entry("missing"),
        Err(Error::EntryNotFound(_))
    ));
    assert!(matches!(
        archive.entry("../art"),
        Err(Error::InvalidPath { .. })
    ));
    assert!(matches!(
        archive.extract("Art", io::sink()),
        Err(Error::UnreadableEntry {
            kind: EntryKind::Directory,
            ..
        })
    ));
    let mut output = Vec::new();
    assert_eq!(
        archive.extract("ART/COMP", &mut output).unwrap(),
        raw.len() as u64
    );
    assert_eq!(output, raw);
    output.clear();
    assert_eq!(archive.extract(r"ART\Plain", &mut output).unwrap(), 4);
    assert_eq!(output, b"raw!");
}

#[test]
fn supports_legacy_dat_and_empty_resources() {
    let records = [Record {
        size: 0,
        stored_size: 19,
        ..plain(b"empty")
    }];
    let mut archive = read(fixture(b"", b"", &records, Format::Dat)).unwrap();
    assert_eq!(archive.metadata().format, Format::Dat);
    assert_eq!(archive.extract("empty", io::sink()).unwrap(), 0);
    let archive = read(fixture(b"", b"", &[], Format::Dat)).unwrap();
    assert!(archive.entries().is_empty());
}

#[test]
fn exposes_unknown_and_ignored_entries_without_extracting_them() {
    let records = [
        Record {
            flags: PLAIN | 0x10,
            ..plain(b"unknown")
        },
        Record {
            flags: PLAIN | IGNORED,
            ..plain(b"ignored")
        },
        Record {
            flags: PLAIN | ZLIB,
            ..plain(b"conflicting")
        },
        Record {
            flags: PLAIN | CE_RUNTIME_FLAGS,
            ..plain(b"supported")
        },
    ];
    let mut archive = read(fixture(b"", b"data", &records, Format::Dat)).unwrap();
    for (path, expected) in [
        ("unknown", EntryKind::Unsupported),
        ("ignored", EntryKind::Ignored),
        ("conflicting", EntryKind::Unsupported),
    ] {
        assert_eq!(archive.entry(path).unwrap().kind, expected);
        assert!(
            matches!(archive.extract(path, io::sink()), Err(Error::UnreadableEntry { kind, .. }) if kind == expected)
        );
    }
    assert_eq!(archive.extract("supported", io::sink()).unwrap(), 4);
}

#[test]
fn rejects_bad_footer_and_index_bounds() {
    assert!(matches!(read(vec![0; 8]), Err(Error::InvalidArchive(_))));
    assert!(matches!(
        read(vec![0; 12]),
        Err(Error::UnsupportedArchive([0, 0, 0, 0]))
    ));

    let bytes = fixture(b"", b"data", &[plain(b"item")], Format::Dat);
    for (offset, value) in [
        (bytes.len() - 4, u32::MAX), // Footer index distance
        (bytes.len() - 4, 0),
        (bytes.len() - 4, 8),
        (bytes.len() - 8, 0), // Footer name table size
        (4, 0),               // Payload span
        (4, 9),
        (8, u32::MAX),  // Record count
        (12, u32::MAX), // Name length
        (12, 1),
        (bytes.len() - 16, u32::MAX), // Payload offset
        (bytes.len() - 24, u32::MAX), // Payload size
    ] {
        let mut invalid = bytes.clone();
        replace_u32(&mut invalid, offset, value);
        assert!(
            matches!(read(invalid), Err(Error::InvalidArchive(_))),
            "{offset}, {value}"
        );
    }
    let mut unterminated = bytes;
    unterminated[20] = b'x';
    assert!(matches!(read(unterminated), Err(Error::InvalidArchive(_))));
}

#[test]
fn rejects_invalid_ambiguous_or_unsupported_names() {
    for name in [b"../file".as_slice(), b"/file", b"a\0b"] {
        assert!(matches!(
            read(fixture(b"", b"data", &[plain(name)], Format::Dat)),
            Err(Error::InvalidArchive(_))
        ));
    }
    assert!(matches!(
        read(fixture(b"", b"data", &[plain(b"\xff")], Format::Dat)),
        Err(Error::UnsupportedNameEncoding { .. })
    ));
    assert!(matches!(
        read(fixture(
            b"",
            b"data",
            &[plain(br"X\Y"), plain(b"x/y")],
            Format::Dat
        )),
        Err(Error::InvalidArchive(_))
    ));
}

#[test]
fn checks_zlib_size_stream_end_and_checksum() {
    let compressed = zlib(b"hello");
    let mut bad_checksum = compressed.clone();
    *bad_checksum.last_mut().unwrap() ^= 1;
    let mut trailing_byte = compressed.clone();
    trailing_byte.push(0);
    for (payload, size) in [
        (compressed.as_slice(), 4),
        (compressed.as_slice(), 6),
        (&compressed[..compressed.len() - 1], 5),
        (&bad_checksum, 5),
        (&trailing_byte, 5),
        (b"", 0),
    ] {
        let records = [Record {
            name: b"test",
            flags: ZLIB,
            size,
            stored_size: payload.len() as u32,
            offset: 0,
        }];
        let mut archive = read(fixture(b"", payload, &records, Format::Dat)).unwrap();
        let mut output = Vec::new();
        assert!(matches!(
            archive.extract("test", &mut output),
            Err(Error::InvalidPayload { .. })
        ));
        assert!(output.len() <= size as usize);
    }

    let empty_zlib = zlib(b"");
    let records = [Record {
        name: b"empty",
        flags: ZLIB,
        size: 0,
        stored_size: empty_zlib.len() as u32,
        offset: 0,
    }];
    let mut archive = read(fixture(b"", &empty_zlib, &records, Format::Dat)).unwrap();
    assert_eq!(archive.extract("empty", io::sink()).unwrap(), 0);
}

#[test]
fn prefix_queries_use_lexical_order_without_reordering_archive_records() {
    let archive = read(fixture(
        b"",
        b"data",
        &[
            plain(b"proto/017089 - npc.pro"),
            plain(b"Artwork"),
            plain(br"Proto\017088 - Scenery.PRO"),
            plain(b"proto/0170880.pro"),
            plain(b"proto/017088 - npc.pro"),
        ],
        Format::Dat,
    ))
    .unwrap();
    let prefix = |prefix: &str| {
        archive
            .entries_with_prefix(prefix)
            .map(|entry| entry.path.as_str())
            .collect::<Vec<_>>()
    };
    assert_eq!(archive.entries()[0].path, "proto/017089 - npc.pro");
    assert_eq!(
        prefix("proto/017088 - "),
        ["proto/017088 - npc.pro", "proto/017088 - scenery.pro"]
    );
    assert!(prefix("proto/017087").is_empty());
    assert!(prefix("zz").is_empty());
    assert_eq!(prefix("").len(), archive.entries().len());
}
