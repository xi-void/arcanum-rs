# Arcanum-RS

Arcanum-RS is an experimental Rust-based replacement engine for
*Arcanum: Of Steamworks and Magick Obscura*.

The long-term objective is a standalone, extensible engine with
original-game compatibility and OpenMW-style modding capabilities.

**Current status:** Resource and content inspection, mod-aware loading,
and a headless runtime for additive AC and selected resistance effects.

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

## First playable alpha

```sh
cargo build --locked --release --features windowed
config_dir="${XDG_CONFIG_HOME:-$HOME/.config}/arcanum-rs"
mkdir -p "$config_dir"
cp -n docs/config.example.toml "$config_dir/config.toml"
```

Set `game_data` in `config.toml` to your Arcanum installation directory, then run
`target/release/arcanum-rs`. From another working directory, run the executable
by its absolute path. An executable on your `PATH` can be launched with
`arcanum-rs`. [Configuration reference](docs/APPLICATION_CONFIG.md)

**Left-click** a destination on the floor to walk there, or **Shift + left-click**
to run there. A new click replaces the route after the current step reaches
the next tile center. Left-click an original door or staircase to approach
and use it. **Mouse wheel** zooms from 1× to 3×. Press **0** to reset.
Hover an NPC for their description and left-click to talk. Click a response
to continue or press **Escape** to close the conversation. [NPC dialogue and voice packs](docs/DIALOGUE.md)
Click the logbook icon or press **L** to read Quests. Turn spreads with the arrows,
**Page Up** or **Page Down**, or the wheel over a page. [Quest logbook](docs/QUESTS.md)
**Escape** opens the Classic pause menu. Choose Save Game, Load Game,
Options, Quit Game or Continue. **F7** writes the quicksave and **F8** loads
the newest save. [Native savegames](docs/SAVEGAMES.md)
The camera follows the character by default, and original sectors load in the
background. The default spawn is the original Zephyr crash site, resolved
from the game's `START_MAP` metadata. Click the nearby cave entrance to
enter, then click the exit inside to return. Use `arcanum-rs play --map 5011` for Bates Mansion or
`arcanum-rs play --map "k'na tha" --center 383 928` for K’na Tha.
The windowed build uses Vulkan through wgpu.

The default Classic HUD uses your installed Arcanum interface artwork.
Use **[** and **]** to scale the HUD and active panel together. Click the bag or press **I** to open inventory. Click an accessible container to open its loot panel and use the wheel to scroll it. Drag gear onto a paper-doll slot to equip it, or back to the backpack to unequip it. Drag a stack between owners and enter the amount to move, or right-click to transfer the entire stack.
Pass `--ui ui/field-notes` for the example replacement,
`--ui /path/to/package` for your own, or `--ui off` to play without a HUD.
[Theme format and controls](docs/UI_THEMES.md)

Press **M** or click the Classic map icon to open the in-game atlas.
Use **Z** for the continent overview and **Shift+Z** for the detailed
world map. At an eligible overworld position, place waypoints and use
the atlas travel button. `--discover-all` reveals destinations for alpha
testing. The default Crash Site spawn is inside the local-map area.
[Map controls, original data and theme configuration](docs/MAP_UI.md)

The alpha uses original human standing, walking and running sprites.
The shipped walk and run clips use 17 and 20 FPS respectively, with
movement speed derived from their stored per-frame strides.
Lighting uses the authored hour in `rules/gameinit.mes` (15:00 in the
installed game). Use `--lighting <hour>` to select another fixed hour.

Mouse routes use loaded sectors and stop when a destination is blocked or
unreachable. Movement checks sector tile-block masks, terrain and facade
flags, static scenery, and orientation-dependent wall and door edges. Open doors permit passage.
Hover an interactive object to highlight its visible sprite. Clicking
past an accessible closed door routes to the doorway, opens it, and
resumes the route after the animation. Click an open leaf to close it.
Carry the matching key and click the door or container to use it. The
key stays in your inventory. Hover a key there to read its authored name.
Locked doors and unsupported scripts report why ordinary use failed.
Door state survives map transitions and is included in native saves.

Walk across the original front exit of Bates Mansion to reach Tarant.
Click the mansion entrance from outside to return. Click the original
stairs in the mansion to change levels. Portal rules, authored jump
tiles and a bounded subset of original Use scripts determine transitions.
See [original transition sources](docs/ORIGINAL_TRANSITIONS.md).
Movement stops if a sector's collision data is unavailable. The
free-camera diagnostic is still available via `art world`.
Play draws original roofs, reveals roof groups on entry, and fades
walls and roof edges based on the player's tile.

## Usage

Build with `cargo build`, then run these commands from the project directory.

```sh
target/debug/arcanum-rs simulate equipment-demo
target/debug/arcanum-rs simulate resistance-demo
target/debug/arcanum-rs simulate electrical-demo
target/debug/arcanum-rs simulate composite-demo
target/debug/arcanum-rs simulate same-channel-demo
```

The asset-free equipment demo applies effect 128 to one actor from two item
instances. The actor's AC goes from 10 to 11 to 26. Removing the second
source leaves AC at 11, and removing the first restores AC to 10.

The resistance demo grants effect 53 from two independent item
instances. Fire resistance rises from 20% to 50%, stays at 50% after
removing one source, and returns to 20% when the last source is removed.

The Electrical demo applies the +8 percentage-point operation found in
Body of Fire using a synthetic one-operation definition. It shows two
independent grants, then changes the underlying value while one grant
is still active. At that point the intermediate result is 100% and the
ordinary query returns 95%.

The composite demo grants effect 54's three statistical operations as
one activation per item. It shows both resistance stages, including the
97% intermediate Poison floor and the 95% ordinary query. Body of Fire's
spell casting and transformation remain outside the simulation.

The same-channel demo combines a synthetic 50% Electrical floor with
the verified +8 operation from a separate source. It shows addition
before the floor, followed by the ordinary query clamp.
[Effect activation ownership](docs/ARCHITECTURE.md#headless-effect-activation-ownership)

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

## ART sprite inspection

```sh
game="$HOME/Work/arcanum-ce-test"
target/debug/arcanum-rs art inspect "$game" art/item/bread.art 0
target/debug/arcanum-rs art export "$game" art/interface/align.art 10 ./align-10.pam
```

Frame indices are zero-based. Both commands support `--mods <mods-directory> <mod-id> ...`.
Export writes transparent RGBA to a new PAM file. See the supported layout,
validation boundaries and original sample provenance. [ART format](docs/ART_FORMAT.md)

To open a native sprite window on Linux, enable the optional frontend:

```sh
cargo run --features windowed -- art demo
cargo run --features windowed -- art view "$game" art/item/bread.art 0
cargo run --features windowed -- art overlap-demo
cargo run --features windowed -- art overlap "$game"
cargo run --features windowed -- art floor-demo
cargo run --features windowed -- art floor "$game"
cargo run --features windowed -- art sector "$game"
cargo run --features windowed -- art sector-wide "$game"
cargo run --features windowed -- art sector-facade "$game"
cargo run --features windowed -- art sector-wall "$game"
cargo run -- art sector-wall-inspect "$game"
cargo run --features windowed -- art sector-walls "$game"
cargo run -- art sector-walls-inspect "$game"
cargo run --features windowed -- art sector-room "$game"
cargo run -- art sector-room-inspect "$game"
cargo run --features windowed -- art sector-room-wide "$game"
cargo run -- art sector-room-wide-inspect "$game"
cargo run --features windowed -- art world "$game"
cargo run -- art world-inspect "$game"
cargo run -- art sector-audit "$game"
```

The streamed world viewer also supports an experimental hardware backend
with the `windowed` feature. See [Vulkan rendering](docs/GPU_RENDERER.md)
for the production rendering path and hardware checks.

The streamed viewer plays original additive fire scenery. Pass
`--animation-ms 90` with `--sectors` to freeze a reproducible animation tick.
[Original animated fire](docs/ANIMATED_FIRE.md) · [Dynamic-light probes](docs/DYNAMIC_LIGHTING.md)

The synthetic demos require only a display session. `art overlap` shows
bread and alignment frames with explicit draw order and independent anchors.
In `art world`, use the arrow keys to pan, Shift to pan faster, Home to restore
the initial view, and Escape to close the viewer. [World camera](docs/WORLD_CAMERA.md) · [Multi-sector navigation](docs/MULTI_SECTOR_WORLD.md)
[Native sprite viewer](docs/SPRITE_VIEWER.md) · [Fixed isometric floor](docs/ISOMETRIC_FLOOR.md) · [Original map sector floor](docs/SECTOR_FLOOR.md) · [Tile-ID coverage](docs/TILE_ID_COVERAGE.md) · [Baked facade import](docs/FACADE_IMPORT.md) · [Conventional wall import](docs/WALL_OBJECT_IMPORT.md) · [Connected walls](docs/CONNECTED_WALLS.md) · [Bounded room scenery](docs/BOUNDED_ROOM_SCENERY.md) · [Static world portals](docs/STATIC_PORTALS.md)

## Prototype and schematic inspection

```sh
game="$HOME/Work/arcanum-ce-test"
target/debug/arcanum-rs prototype inspect "$game" 'proto/010087 - Food.pro'
target/debug/arcanum-rs prototype inspect "$game" 'proto/006136 - Weapon.pro'
target/debug/arcanum-rs prototype inspect "$game" 'proto/007059 - Ammo.pro'
target/debug/arcanum-rs prototype inspect "$game" 'proto/008095 - Armor.pro'
target/debug/arcanum-rs prototype inspect "$game" 'proto/008184 - Armor.pro'
target/debug/arcanum-rs prototype inspect "$game" 'proto/006084 - Weapon.pro'
target/debug/arcanum-rs prototype inspect "$game" 'proto/015090 - Generic.pro'
target/debug/arcanum-rs prototype inspect "$game" 'proto/012081 - Key.pro'
target/debug/arcanum-rs schematic inspect "$game" 4220
target/debug/arcanum-rs schematic inspect "$game" 2450
target/debug/arcanum-rs schematic audit "$game" 2450 3250 4220
target/debug/arcanum-rs schematic audit "$game" --all-references
target/debug/arcanum-rs schematic audit "$game" --readiness 2450 4220
```

These commands accept the MES command's `--mods <mods-directory> <mod-id> ...` argument. Prototype inspection shows selected Food fields, Weapon damage channels, base speed and range, and ammunition compatibility. For Ammo it reports the stored prototype quantity separately. Armour inspection reports its base AC and resistance adjustments, coverage and fit sizes. Generic and Key inspection show the resolved name, base weight and worth, and signed complexity. Generic inspection also reports legacy flags. Key inspection shows the stored and native key codes. [Weapon and Ammo fields](docs/CONTENT_FORMATS.md#milestone-10-native-weapon-and-ammo-definitions) · [Armour fields](docs/CONTENT_FORMATS.md#milestone-11-native-armour-definitions) · [Generic fields](docs/CONTENT_FORMATS.md#milestone-17-native-generic-definitions) · [Key fields](docs/CONTENT_FORMATS.md#milestone-18-native-key-definitions)

Prototype inspection also shows stored magical Armour adjustments separately and reports the source or resolution error for each Weapon wield script. [Equipment effect references](docs/CONTENT_FORMATS.md#milestone-12-equipment-effects-and-references)

Schematic inspection reports the selected group’s resource sources, unresolved references and any ordered input-to-output variants. [Content formats](docs/CONTENT_FORMATS.md#milestone-6-healing-jacket-variants)

With no selection, the audit reports structural coverage. Use `--all-references` for a full reference summary and incomplete recipe IDs. Pass individual IDs for detailed diagnostics. [Validation findings](docs/CONTENT_FORMATS.md#milestone-8-full-schematic-reference-validation)

Use `--readiness` to add item-definition and illustration-read checks for reference-complete recipes. With no recipe IDs after the flag, it checks all discovered recipes. An unsupported item subtype is reported separately from a failed decode. The illustration check validates readability and size with a 16 MiB cap. ART decoding and gameplay validation are outside this check. [Readiness guarantees](docs/CONTENT_FORMATS.md#milestone-16-schematic-readiness-and-armour-complexity) · [Current installed coverage](docs/CONTENT_FORMATS.md#milestone-18-native-key-definitions)

```sh
ARCANUM_GAME_DIR="$HOME/Work/arcanum-ce-test" \
  cargo test --test installed_prototypes -- --ignored
```

## Effect inspection

```sh
game="$HOME/Work/arcanum-ce-test"
target/debug/arcanum-rs effect inspect "$game" 128
target/debug/arcanum-rs effect inspect "$game" 53
target/debug/arcanum-rs effect inspect "$game" 54
```

The command shows the selected record, its VFS source, and all decoded operations in source order. It accepts `--mods <mods-directory> <mod-id> ...` like prototype inspection. [Supported effect definitions](docs/CONTENT_FORMATS.md#milestone-14-composite-effect-definitions)

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
