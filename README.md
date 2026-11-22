# Arcanum-RS

Milestone 0 is a read-only resource explorer for Arcanum DAT archives. It runs independently of Arcanum and Arcanum CE.

## Usage

Build with `cargo build`, then run these commands from the project directory.

```sh
game="$HOME/Work/arcanum-ce-test"
target/debug/arcanum-rs list "$game/Arcanum1.dat"
target/debug/arcanum-rs info "$game/Arcanum1.dat" 'ART\BADART.ART'
target/debug/arcanum-rs extract "$game/Arcanum1.dat" 'art/badart.art' ./badart.art
```

`list` prints the kind, logical size in bytes, and normalized virtual path in index order. `info` shows the archive format and the detected record's flags, sizes, and physical offset where available. `extract` streams a single binary resource to the exact output file supplied on the command line. The parent directory must exist. The CLI rejects existing files and symlinks at the output path and removes the incomplete file on a reported write or payload error.

## DAT format

The format reference is Arcanum CE revision `fb721982`, particularly `first_party/tig/src/database.c` for opening and reading entries, `first_party/tig/include/tig/database.h` for flags, and `first_party/tig/src/file.c` for binary extraction and repository behavior. The DAT1 layout was checked against the installed archives.

- Integers in the footer and index are 32-bit little-endian. The file ends with the four signature bytes (` TAD` or `1TAD`), the total byte count of the names including terminators, and the backward distance from the start of the final word to the start of the index. In DAT1 the 16 raw GUID bytes immediately precede the signature. The CLI displays them in on-disk order.
- The index starts with the payload span including the four-byte span word, followed by the entry count. The archive base is the index position plus four minus the span. This permits a prefix before the archive data. Each entry has a length-prefixed, NUL-terminated name, one skipped word, flags, logical size, stored size, and an offset relative to the archive base. The declared records and names must fill the index up to the footer.
- Flag `0x001` denotes plain data, whose byte count comes from the logical size. Flag `0x002` denotes one zlib stream, bounded by the stored size. The reader checks the stream terminator, checksum, and exact output size, and rejects trailing bytes within the stored range. Directory records (`0x400`) and ignored records (`0x800`) are visible as metadata. Flags `0x100` and `0x200` are CE repository state bits.
- Virtual names use UTF-8 with ASCII case folding and slash separators. Separators and case follow CE's lookup conventions. This explorer also collapses repeated separators and dot components and rejects absolute paths, parent components, colons, control characters, and duplicate normalized names.

Unsupported features include other footer signatures, legacy code-page filenames, extraction of directory or ignored records, and unrecognized storage flags. CE's external `.ignore` files, text-mode transformations, and loose-file or patch precedence are outside this release. Individual patch archives can be opened directly.

## Implementation and verification

`dat` owns the index and keeps a seekable input for bounded payload reads. `resource_path` handles virtual path normalization independently of host paths. The CLI uses these library modules and owns output-file handling. The only direct dependency is `flate2`, using its Rust backend.

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build
```

Synthetic tests cover both footer variants, prefixed archives, plain and compressed data, empty resources, malformed metadata, corrupt zlib streams, and virtual path handling. The legacy DAT footer was verified with synthetic fixtures.

A standalone verification program linked the prebuilt CE TIG library and read resources in binary mode through `tig_database_fopen` and `tig_database_fread`. The explorer listed all 222,460 records across 44 installed DAT and PATCH archives. Six resources matched TIG byte for byte, including two plain entries and the 1,771,118-byte compressed `maps/arcanum1-024-fixed/terrain.tdf`. CLI checks covered existing output files, dangling destination symlinks, traversal-like resource names, directories, and cleanup after corrupt input. Verification used CE and the game files as read-only inputs and ran without launching the game.

The next milestone is a read-only virtual filesystem with ordered DAT and loose-directory mounts, explicit precedence, and a way to report which mount supplied a resource.
