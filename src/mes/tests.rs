use super::*;

#[test]
fn parses_single_record_and_reports_missing_id() {
    let mes = Mes::parse(b"{40000}{A synthetic message.}").unwrap();
    assert_eq!(mes.len(), 1);
    assert_eq!(mes.get(40000), Some("A synthetic message."));
    assert_eq!(mes.get(10), None);
}

#[test]
fn preserves_empty_and_multiline_text_and_orders_entries_by_id() {
    let lf = b"{3}{  Hey! // literal\n\tNext.  }\n{0}{}\n{ -2 }{x}{+1}{y}";
    let crlf = std::str::from_utf8(lf).unwrap().replace('\n', "\r\n");
    let mes = Mes::parse(lf).unwrap();
    assert_eq!(
        mes.entries().collect::<Vec<_>>(),
        [
            (-2, "x"),
            (0, ""),
            (1, "y"),
            (3, "  Hey! // literal\n\tNext.  ")
        ]
    );
    assert_eq!(Mes::parse(crlf.as_bytes()).unwrap().get(3), mes.get(3));
}

#[test]
fn accepts_labels_and_line_comments_between_fields() {
    let mes = Mes::parse(b"// braces in a comment {fake} }\r\n Label { 7 } // comment { ignored\n  other label {literal // text}\ttrailer\n").unwrap();
    assert_eq!(mes.entries().collect::<Vec<_>>(), [(7, "literal // text")]);
}

#[test]
fn duplicates_are_reported_in_source_order_and_last_value_wins() {
    let raw = b"{1}{first}\n{2}{another}\n{1}{second}\n{1}{third}";
    let parsed = parse_source(raw).unwrap();
    assert_eq!(parsed, parse_source(raw).unwrap());
    assert_eq!(parsed.mes.len(), 2);
    assert_eq!(parsed.mes.get(1), Some("third"));
    assert_eq!(
        parsed.duplicates,
        &[
            Duplicate {
                id: 1,
                previous: Location {
                    line: 1,
                    byte_offset: 0
                },
                replacement: Location {
                    line: 3,
                    byte_offset: 24
                }
            },
            Duplicate {
                id: 1,
                previous: Location {
                    line: 3,
                    byte_offset: 24
                },
                replacement: Location {
                    line: 4,
                    byte_offset: 36
                }
            },
        ]
    );
}

#[test]
fn malformed_fields_report_locations_in_the_original_bytes() {
    for (raw, kind, line, byte_offset) in [
        (
            b"{0}{ok}\r\n}".as_slice(),
            ErrorKind::UnexpectedClosingBrace,
            2,
            9,
        ),
        (b"{0}{abc{def}", ErrorKind::NestedOpeningBrace, 1, 7),
        (b"{0}\r\n{unfinished", ErrorKind::UnterminatedField, 2, 5),
        (b"{0}\n// no text", ErrorKind::MissingText, 2, 14),
        (b"{", ErrorKind::UnterminatedField, 1, 0),
        (b"", ErrorKind::NoRecords, 1, 0),
        (b"{0}{\0}", ErrorKind::Nul, 1, 4),
    ] {
        let err = Mes::parse(raw).unwrap_err();
        assert_eq!(
            err,
            Error {
                kind,
                location: Location { line, byte_offset }
            },
            "{raw:?}"
        );
    }
}

#[test]
fn numeric_fields_are_strict_signed_32_bit_decimal_values() {
    for bad in [
        "",
        "-",
        "0x13",
        "1 trailing",
        "1.5",
        "1 2",
        "2147483648",
        "-2147483649",
        "１",
    ] {
        assert_eq!(
            Mes::parse(format!("{{{bad}}}{{x}}").as_bytes())
                .unwrap_err()
                .kind,
            ErrorKind::InvalidId
        );
    }
    let mes = Mes::parse(b"{-2147483648}{min}{2147483647}{max}").unwrap();
    assert_eq!(mes.get(i32::MIN), Some("min"));
    assert_eq!(mes.get(i32::MAX), Some("max"));
}

#[test]
fn encoding_selection_is_explicit_and_never_inserts_replacements() {
    let utf8 = parse_source("\u{feff}{1}{café — déjà vu}".as_bytes()).unwrap();
    assert_eq!(utf8.encoding, Encoding::Utf8);
    assert_eq!(utf8.mes.get(1), Some("café — déjà vu"));
    let legacy = b"{1}{\x93Caf\xe9\x94 \x96 \x80}";
    let legacy_source = parse_source(legacy).unwrap();
    assert_eq!(legacy_source.encoding, Encoding::Windows1252);
    assert_eq!(legacy_source.mes.get(1), Some("“Café” – €"));
    assert_eq!(
        legacy_source.mes,
        Mes::parse_text("{1}{“Café” – €}").unwrap()
    );
    assert_eq!(
        Mes::parse_with_encoding(legacy, Encoding::Utf8)
            .unwrap_err()
            .kind,
        ErrorKind::InvalidUtf8
    );
    let ambiguous = "{1}{é}".as_bytes();
    assert_eq!(
        Mes::parse_with_encoding(ambiguous, Encoding::Windows1252)
            .unwrap()
            .get(1),
        Some("Ã©")
    );
    for (data, kind) in [
        (b"\xef\xbb\xbf{1}{\xe9}".as_slice(), ErrorKind::InvalidUtf8),
        (b"\xff\xfe\0\0", ErrorKind::UnsupportedBom),
        (b"{1}{\x81}", ErrorKind::UndefinedWindows1252(0x81)),
    ] {
        assert_eq!(Mes::parse(data).unwrap_err().kind, kind);
    }
    assert_eq!(
        Mes::parse_with_encoding(b"\xef\xbb\xbf{1}{x}", Encoding::Windows1252)
            .unwrap_err()
            .kind,
        ErrorKind::ConflictingBom
    );
}

#[test]
fn limits_bytes_and_records_including_duplicates() {
    assert_eq!(
        Mes::parse(&vec![b'x'; MAX_INPUT_BYTES + 1])
            .unwrap_err()
            .kind,
        ErrorKind::TooLarge
    );
    let entries = b"{1}{}".repeat(MAX_RECORDS + 1);
    let err = Mes::parse(&entries).unwrap_err();
    assert_eq!(err.kind, ErrorKind::TooManyRecords);
    assert_eq!(err.location.byte_offset, 5 * MAX_RECORDS);
}
