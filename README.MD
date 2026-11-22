# Arcanum-RS

An experimental Rust-based, open-source replacement engine for *Arcanum: Of Steamworks and Magick Obscura*.

The long-term objective is to create a modern, extensible engine with original-game compatibility and an OpenMW-style modding architecture.

## Current Status

**Milestone 2: Mod Manifests and Dependency Resolution**

- DAT1 and legacy DAT archive reading
- Archive metadata and entry enumeration
- Plain and zlib-compressed resource extraction
- Normalized resource paths
- Read-only resource inspection CLI
- Differential validation against Arcanum CE's TIG resource reader
- Ordered DAT and loose-directory mounts with resource provenance
- Local TOML mod manifests, dependency validation, and deterministic load order

See the [manifest specification and CLI guide](README.md#local-mod-packages).

## Roadmap

- [x] DAT resource explorer
- [x] Virtual filesystem and resource precedence
- [x] Mod manifests and load ordering
- [ ] Extensible content and scripting systems
- [ ] Engine subsystem modernization
- [ ] Standalone playable engine

## Compatibility

Arcanum-RS is at an early experimental stage and cannot currently run the game.

The original game data is required for resource inspection. No copyrighted game assets are included in this repository.

## Related Projects

- [Arcanum Community Edition](https://github.com/alexbatalov/arcanum-ce)
- [Arcanum CE Vanilla Restoration Fork](https://github.com/xi-void/arcanum-ce)

Arcanum-RS is an independent project and is not an official release of Arcanum or Arcanum Community Edition.
