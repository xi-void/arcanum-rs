# Arcanum-RS

Arcanum-RS is an experimental Rust-based replacement engine for
*Arcanum: Of Steamworks and Magick Obscura*.

The long-term objective is a standalone, extensible engine with
original-game compatibility and OpenMW-style modding capabilities.

**Current status:** Read-only resource explorer, virtual filesystem,
mod manifest infrastructure, and mod-aware MES loading. The engine is not yet playable.

## Roadmap

- [x] DAT archive reader and resource explorer
- [x] Virtual filesystem and resource precedence
- [x] Mod manifests and dependency resolution
- [x] MES parsing and mod-aware content loading
- [ ] Extensible content definitions
- [ ] Lua scripting and gameplay interfaces
- [ ] World simulation and engine modernization
- [ ] Standalone playable Rust engine

See [Architecture](docs/ARCHITECTURE.md) for the project's
architectural principles and CE compatibility boundaries.

## Related Projects

- [Arcanum Community Edition](https://github.com/alexbatalov/arcanum-ce)
- [Arcanum CE Vanilla Restoration](https://github.com/xi-void/arcanum-ce)

Arcanum-RS is independent of both projects. Original copyrighted
game assets are not included.

## Usage

Build with `cargo build`, then run these commands from the project directory.

```sh
game="$HOME/Work/arcanum-ce-test"
target/debug/arcanum-rs list "$game/Arcanum1.dat"
target/debug/arcanum-rs info "$game/Arcanum1.dat" 'ART\BADART.ART'
target/debug/arcanum-rs extract "$game/Arcanum1.dat" 'art/badart.art' ./badart.art
target/debug/arcanum-rs mounts "$game"
target/debug/arcanum-rs resolve "$game" 'MES\GAMEDESC.MES'
target/debug/arcanum-rs conflicts "$game"
target/debug/arcanum-rs --ordered resolve "$game" 'mes/gamedesc.mes'
```

`list` prints the kind, logical size in bytes, and normalized virtual path in index order. `info` shows the archive format and the detected record's flags, sizes, and physical offset where available. `extract` streams a single binary resource to the exact output file supplied on the command line. The parent directory must exist. The CLI rejects existing files and symlinks at the output path and removes the incomplete file on a reported write or payload error.

`mounts` shows the highest-priority mount first. `resolve` prints the normalized resource path and every corresponding record, with the winner, shadowed candidates, and exclusions marked. Each candidate shows its mount and source, storage type, and logical size. `resolve` exits with status 1 if there is no winner. `conflicts` prints the same information for paths with multiple records, including loose names that collide after case folding. It sorts results by normalized path.

## Virtual filesystem and precedence

The CLI defaults to CE mode and discovers the stock `Arcanum` campaign. The following is the order in which it registers sources. Missing sources are skipped.

1. `tig.dat`, then the game directory as a loose mount.
2. Top-level `arcanum*.dat` files in ascending case-insensitive filename order, then the loose `data` directory.
3. `modules/Arcanum.dat`, followed by existing `Arcanum.patch0` through `Arcanum.patch9` in numeric order when the module DAT is present.
4. The loose `modules/Arcanum` directory.

This profile follows `tig_file_init`, `gamelib_load_data`, and `gamelib_load_module_data` in CE. The library also accepts explicitly ordered DAT/PATCH and directory mounts, with a stable `MountId` per registered source. `Vfs::resolve` returns typed provenance and `Vfs::copy` streams the winning file through the existing DAT reader or the loose mount. The VFS indexes loose names and metadata at mount time. It keeps each directory open with `cap-std` so a replaced path or symlink cannot redirect a file read outside the mount. Discovered symlinks and non-regular loose entries are skipped.

### Confirmed CE behavior

- `tig_file_repository_add_native` in `first_party/tig/src/file.c` prepends new repositories and promotes a re-registered path. `tig_file_open_internal_native` finds the first matching archive in that order. It probes loose directories ahead of that archive for overrides. Without a matching archive it falls back to the first loose match. For a stable set of readable files, this returns the highest-priority eligible record. `tig_file_exists_native` and `tig_file_list_create_native` use separate scans. The latter sorts names case-insensitively and retains the first occurrence of each name.
- TIG's archive lookup in `first_party/tig/src/database.c` binary-searches records by lowercased native path. Duplicate names across mounts obey repository precedence. The reader rejects ambiguous normalized duplicates within a single archive. CE assumes a sorted index and leaves the choice among equal records to its binary search.
- Archive records flagged `0x800` are skipped during TIG lookup. `<archive>.ignore`, including sidecars for patch archives, marks matching entries ignored within that archive only. CE lowercases the patterns, converts separators on Linux, and requires the pattern and path to have the same number of separators. A lower mount can still supply the resource. `resolve` includes these records with their exclusion reason.
- An unignored archive directory record participates in binary open. TIG can return a zero-byte handle for that record, even with a file of the same name in a lower mount. The VFS reports the directory winner in CE mode and returns a directory error from `copy`.
- Both source types are case-insensitive for ASCII on Linux. Archives use ASCII lowercasing and exact comparison. Loose lookup walks components using SDL's case-insensitive comparison and takes the first matching host directory entry. SDL can compare non-ASCII names differently from the archive lookup.

### Deliberate differences and remaining compatibility gaps

- `--ordered` gives the latest registered eligible file priority, treats directory records as metadata, and skips sidecar processing. On-disk ignored flags still suppress records. These are the rules for explicit mod ordering.
- Both modes use Milestone 0's ASCII-folded virtual paths, and order case collisions within a loose mount by the actual relative path. CE's host-order tie breaks, Unicode loose-name comparison, and native absolute or dot-prefixed path bypasses are outside the VFS contract. Loose directories contribute regular file descendants only.
- CE caches archive override misses on some open paths and can delete `TIGCache` on registration. The VFS uses a stable mount snapshot and skips `TIGCache` entries in CE mode. The VFS also excludes unsupported archive storage records. TIG's process-wide ignore API, runtime mount mutations from the game, and text-mode, write-mode, or cache-producing extraction behavior are outside the compatibility target. The differential tests used the Linux TIG build.

## DAT format

The format reference is Arcanum CE revision `fb721982`, particularly `first_party/tig/src/database.c` for opening and reading entries, `first_party/tig/include/tig/database.h` for flags, and `first_party/tig/src/file.c` for binary extraction and repository behavior. The DAT1 layout was checked against the installed archives.

- Integers in the footer and index are 32-bit little-endian. The file ends with the four signature bytes (` TAD` or `1TAD`), the total byte count of the names including terminators, and the backward distance from the start of the final word to the start of the index. In DAT1 the 16 raw GUID bytes immediately precede the signature. The CLI displays them in on-disk order.
- The index starts with the payload span including the four-byte span word, followed by the entry count. The archive base is the index position plus four minus the span. This permits a prefix before the archive data. Each entry has a length-prefixed, NUL-terminated name, one skipped word, flags, logical size, stored size, and an offset relative to the archive base. The declared records and names must fill the index up to the footer.
- Flag `0x001` denotes plain data, whose byte count comes from the logical size. Flag `0x002` denotes one zlib stream, bounded by the stored size. The reader checks the stream terminator, checksum, and exact output size, and rejects trailing bytes within the stored range. Directory records (`0x400`) and ignored records (`0x800`) are visible as metadata. Flags `0x100` and `0x200` are CE repository state bits.
- Virtual names use UTF-8 with ASCII case folding and slash separators. Separators and case follow CE's lookup conventions. This explorer also collapses repeated separators and dot components and rejects absolute paths, parent components, colons, control characters, and duplicate normalized names.

Unsupported features include other footer signatures, legacy code-page filenames, extraction of directory or ignored records, and unrecognized storage flags. Individual patch archives can be opened directly. The single-archive CLI commands inspect raw entries. Sidecar policies apply when resolving through the VFS.

## Local mod packages

Install a mod by placing its directory under a local mods directory, such as `~/Work/arcanum-mods`. Put `arcanum-mod.toml` at the package root, alongside the directories and archives it mounts. Discovery scans immediate child directories and uses manifest IDs independently of directory names. Explicitly selected package mounts are read-only.

### Manifest version 1

Manifests are UTF-8 TOML files up to 1 MiB.

```toml
manifest_version = 1
id = "interface-patch"
name = "Interface Patch"
version = "1.0.0"
description = "Updates interface resources."
# Optional ordering rules go here, above any tables.
# load_after = ["another-patch"]
# load_before = ["final-patch"]

[dependencies]
base-interface = "^1.2"

[[mounts]]
kind = "archive"
path = "archives/interface.patch9"

[[mounts]]
kind = "directory"
path = "resources"
```

- `manifest_version`, `id`, `name`, `version`, and `description` are required. The ID starts with a lowercase ASCII letter or digit and uses only lowercase ASCII letters, digits, `.`, `_`, and `-`. The version is a SemVer string.
- `[dependencies]` maps required mod IDs to SemVer requirements such as `^1.2`, `>=1.2, <2`, or `=1.2.3`. Use `"*"` when any stable version is acceptable. Prerelease matching follows SemVer requirement rules. Omit the table when the package has no dependencies.
- `load_after` and `load_before` are optional arrays of installed mod IDs. They apply when both packages are enabled and do not enable a package on their own.
- Each `[[mounts]]` entry takes `kind = "directory"` or `kind = "archive"` and a package-relative `path`. Paths preserve host case, accept either slash style, and reject absolute paths, parent traversal, and symlink components. A directory contributes files at its root and below. An archive contributes its DAT/PATCH resource names. All mounts contribute at the VFS root. Omit mounts for a package that only declares dependencies or ordering rules.
- Unknown fields, invalid manifests, duplicate IDs, missing required mods, version mismatches, and order cycles are errors. A cycle error identifies the participating IDs and fields. The validator checks that declared mounts exist, are of the correct kind, and have readable directory or archive indexes.

### Selection and precedence

Mod resolution mounts the base game in ordered mode, then adds the selected mods and their transitive dependencies. Dependencies load before their dependents. Among packages ready to load at the same step, the lexically smallest ID loads first. A later package wins resource conflicts, and later mount entries win within a package. The CLI lists the source package on both winning and shadowed mod records. Ordinary `resolve` continues to use CE mode by default. Library callers can apply a load order to any ordered VFS without an Arcanum or CE installation.

```sh
game="$HOME/Work/arcanum-ce-test"
mods="$HOME/Work/arcanum-mods"
target/debug/arcanum-rs mods list "$mods"
target/debug/arcanum-rs mods info "$mods/Interface Patch"
target/debug/arcanum-rs mods validate "$mods"
target/debug/arcanum-rs mods order "$mods" interface-patch
target/debug/arcanum-rs mods resolve "$game" "$mods" 'art/interface/interface.mes' interface-patch
```

`list` and `info` read manifest metadata. `validate` and `order` check all installed mods when IDs are omitted. `mods resolve` takes at least one ID. Discovery permits one installed version per ID. Selection is per invocation, and packages are supplied as local directories. Mount validation checks metadata. Compressed payloads are checked when read.

## MES content

The MES CLI uses the existing VFS and mod selection. Without `--mods`, it defaults to CE-mode resource precedence. With `--mods`, it uses the ordered VFS and includes the selected mods' dependencies. A replacement supplies the whole MES table.

```sh
game="$HOME/Work/arcanum-ce-test"
mods="$HOME/Work/arcanum-mods"
target/debug/arcanum-rs mes inspect "$game" 'mes/gamedesc.mes'
target/debug/arcanum-rs mes get "$game" 'mes/gamedesc.mes' 40000
target/debug/arcanum-rs mes get "$game" 'MES\GAMEDESC.MES' 40000 --mods "$mods" vfs-smoke
```

`mes::Mes::parse_text` accepts native UTF-8 text. `Mes::parse` accepts resource bytes, and `Mes::parse_with_encoding` selects UTF-8 or Windows-1252 explicitly. `get` looks up an ID and `entries` iterates in numeric order. The native `Mes` table stores only IDs and Unicode text. Standalone `mes::parse_source` and the VFS loaders report selected encoding and duplicate locations alongside the table. `content::load_mes` and `load_mes_with_encoding` use a caller-owned `&mut Vfs` and also return the normalized path, mount, storage type, and optional mod ID. The loader bounds both the declared size and the bytes streamed by the VFS.

### Syntax and compatibility

- Records pair `{id}` with `{text}`. IDs are signed 32-bit decimal integers with optional surrounding ASCII whitespace. Values may be empty or multiline. The parser preserves punctuation, spaces, and tabs inside values and normalizes CRLF and lone CR to LF. Whitespace and free-text annotations outside fields are ignored. `//` starts a line comment only outside fields, so braces on a comment line are ignored and `//` inside a value remains literal.
- The parser checks a UTF-8 BOM first, then valid UTF-8, then Windows-1252. The selected encoding is reported. Undefined Windows-1252 bytes, embedded NUL, and UTF-16 or UTF-32 BOMs produce errors. Other legacy encodings are unsupported. Input is limited to 4 MiB and 100,000 records, counting duplicate occurrences.
- Within one table, the last occurrence of an ID wins and each replacement is reported. Invalid IDs, unmatched or nested delimiters, empty tables, and truncated records produce errors with one-based lines and zero-based byte offsets into the original input. Literal braces and block-comment syntax are unsupported.
- CE revision `fb721982` in `src/game/mes.c` searches for braces in the raw text, including in comments. It uses `atoi` for IDs, retains duplicate records after sorting, and logs several malformed cases while continuing or returning the parsed prefix. CE leaves character encoding to callers and applies platform- or source-dependent text-mode newline handling. The Rust parser uses strict validation, deterministic duplicate handling, and explicit decoding and newline rules.

## Prototype and schematic inspection

```sh
game="$HOME/Work/arcanum-ce-test"
target/debug/arcanum-rs prototype inspect "$game" 'proto/010087 - Food.pro'
target/debug/arcanum-rs prototype inspect "$game" 'proto/006136 - Weapon.pro'
target/debug/arcanum-rs prototype inspect "$game" 'proto/007059 - Ammo.pro'
target/debug/arcanum-rs prototype inspect "$game" 'proto/008095 - Armor.pro'
target/debug/arcanum-rs schematic inspect "$game" 4220
target/debug/arcanum-rs schematic inspect "$game" 2450
target/debug/arcanum-rs schematic audit "$game" 2450 3250 4220
target/debug/arcanum-rs schematic audit "$game" --all-references
```

These commands accept the MES command's `--mods <mods-directory> <mod-id> ...` argument. Prototype inspection shows selected Food fields, Weapon damage channels, base speed and range, and ammunition compatibility. For Ammo it reports the stored prototype quantity separately. Armour inspection reports its base AC and resistance adjustments, coverage and fit sizes. Key and Generic inspection includes the description key. [Weapon and Ammo fields](docs/CONTENT_FORMATS.md#milestone-10-native-weapon-and-ammo-definitions) · [Armour fields](docs/CONTENT_FORMATS.md#milestone-11-native-armour-definitions)

Schematic inspection reports the selected group’s resource sources, unresolved references and any ordered input-to-output variants. [Content formats](docs/CONTENT_FORMATS.md#milestone-6-healing-jacket-variants)

With no selection, the audit reports structural coverage. Use `--all-references` for a full reference summary and incomplete recipe IDs. Pass individual IDs for detailed diagnostics. [Validation findings](docs/CONTENT_FORMATS.md#milestone-8-full-schematic-reference-validation)

```sh
ARCANUM_GAME_DIR="$HOME/Work/arcanum-ce-test" \
  cargo test --test installed_prototypes -- --ignored
```

## Implementation and verification

`dat` owns the index and keeps a seekable input for bounded payload reads. `resource_path` handles virtual path normalization independently of host paths. `vfs` owns mount ordering, discovery, and provenance. `mods` discovers packages, validates manifests, and plans the order before mounting resources. The CLI uses these library modules and owns output-file handling. `flate2` provides the Rust zlib backend. `cap-std` and `cap-fs-ext` confine filesystem access to mount roots and open manifest paths without following symlinks.

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build
```

Synthetic tests cover archive parsing and decompression, mount precedence and re-registration, mixed sources, sidecars, directory and ignored records, missing paths, case collisions, traversal and symlink confinement. Mod tests cover manifest validation, dependency graphs, ordering conflicts, selected overrides, and mount provenance. MES tests cover decoding, delimiters, source locations, deterministic duplicate handling, parser limits, and archive-to-mod replacement with provenance. The legacy DAT footer was verified with synthetic fixtures.

A standalone differential test linked the prebuilt CE TIG library and exercised `tig_file_repository_add` and `tig_file_fopen` in binary mode. The test pointed TIG at original archives and scratch copies of selected loose resources, since registering a directory in TIG can clear its cache. Ten synthetic and twelve installed resolution cases matched TIG, including missing results and archive directory winners. Twelve direct extractions matched byte for byte, including resources from `Arcanum.PATCH0`, `Arcanum.PATCH1`, `Arcanum.PATCH6`, and `Arcanum.PATCH9`. Verification used CE and the game files as read-only inputs and ran without launching the game.

The MES scan parsed 713 of 725 installed archive and loose tables. The remaining files fall into two diagnostic classes.

| Classification | Tables | Finding |
| --- | ---: | --- |
| Malformed delimiters | 10 | Unexpected closing braces in variants of `pcvars.mes` and in `BuriedSecrets.dat`'s `mes/gamequestlogdumb.mes`. |
| Unsupported source bytes | 2 | `rules/effect.mes` in `Arcanum5.dat` and `ArcanumXRaces.dat` contains `0x81`, which is undefined in Windows-1252. The source encoding of that byte is unconfirmed. |

A standalone comparison with CE's MES parser checked 7,177 unique IDs across seven tables. Each Rust value matched a corresponding CE record after decoding and newline normalization. For duplicate IDs this comparison allows any of CE's retained records, while Rust applies its last-record rule.

The external integration test checks that `mes/gamedesc.mes` comes from `Arcanum.PATCH6` in CE mode and that enabling `vfs-smoke` changes ID `40000` to `Arcanum-RS test override`, with `vfs-smoke` reported as the source. Run it against the existing read-only fixtures with the following command.

```sh
ARCANUM_GAME_DIR="$HOME/Work/arcanum-ce-test" \
ARCANUM_MODS_DIR="$HOME/Work/arcanum-mods" \
  cargo test --test installed_mes -- --ignored
```
