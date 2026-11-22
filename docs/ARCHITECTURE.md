# Arcanum-RS Architecture

## Vision

Arcanum-RS is an independent, Rust-based replacement engine for
Arcanum: Of Steamworks & Magick Obscura.

The long-term objective is a standalone, playable, extensively
moddable engine with OpenMW-style extensibility.

Arcanum Community Edition (CE) provides a playable reference
implementation and a temporary integration platform.

CE is not the architectural foundation of Arcanum-RS.

## Core Architectural Principles

### 1. Rust-First Architecture

All core subsystems must compile, execute, and pass their own tests
without requiring Arcanum CE.

The standalone Rust engine is the ultimate runtime.

### 2. Strict Separation from CE

The Rust core must not depend on:

- CE global variables
- CE object handles or pointers
- CE memory layouts
- TIG internal structures
- CE-specific runtime initialization
- Undocumented CE lifecycle assumptions

CE interoperability belongs exclusively in compatibility adapters.

### 3. Dependency Direction

The dependency structure must be:

    Arcanum-RS Core
           ^
           |
    CE Compatibility Adapter
           ^
           |
    Arcanum CE Runtime

The adapter depends on the Rust core.

The Rust core must never depend on the CE adapter.

Removing CE must not prevent the Rust core from compiling or
passing its own tests.

External differential tests may use CE as a reference.

### 4. Explicit Ownership and State

Core systems should use:

- Explicit ownership
- Stable, typed identifiers
- Controlled state transitions
- Validated resource access
- Clear error handling
- Minimal implicit global state

Legacy engine behavior should not dictate internal data structures.

### 5. Behavioral Compatibility

Preserve original Arcanum gameplay behavior where appropriate,
including:

- Character progression
- Combat rules
- Magic and technology
- Inventory and crafting
- Dialogue and quests
- World simulation

Compatibility means reproducing observable behavior, not
reproducing implementation defects.

### 6. Modding as a First-Class Requirement

Extensibility must be considered when designing each subsystem.

The long-term modding architecture should support:

- Additional maps and locations
- New NPCs and creatures
- Dialogue and quests
- Items, weapons, and schematics
- Custom spells and technological disciplines
- Scripted gameplay mechanics
- Mod dependencies and load ordering
- Resource overrides and conflict detection
- Lua scripting through a stable, documented API

Mods should not require recompiling the engine or modifying
original game assets.

### 7. Resource Independence

Resources are accessed through abstract virtual filesystem APIs.

Core systems must not depend on TIG database pointers or
CE-specific file handles.

Resource provenance, deterministic resolution, and validation
are required.

### 8. Incremental Modernization

Development proceeds through independently testable milestones.

Initial components:

1. DAT resource reader
2. Virtual filesystem
3. Mod manifests and dependency resolution
4. Resource and content definitions
5. Scripting interfaces
6. World simulation and gameplay systems
7. Rendering and standalone runtime

The sequence may evolve as dependencies become clearer.

### 9. CE Integration Is Temporary

CE may consume Arcanum-RS components through a narrow C ABI.

Such integration must:

- Preserve Rust core independence
- Isolate legacy handle translation
- Avoid exposing internal Rust layouts
- Avoid introducing CE-specific assumptions into core APIs
- Remain optional for standalone Rust development

The eventual standalone engine must not require CE.

### 10. Independent Repositories

The projects remain separate:

- arcanum-ce: playable restoration and compatibility reference
- arcanum-rs: independent engine and modding infrastructure

Do not copy or translate CE implementation code without reviewing
its licensing and provenance.

Original copyrighted game assets are never committed to Arcanum-RS.

## Architectural Acceptance Test

Before accepting a new subsystem, verify:

1. Does it compile without CE?
2. Can it be tested without launching Arcanum?
3. Does it avoid legacy structures in public APIs?
4. Are ownership and state transitions explicit?
5. Can modding requirements be supported without redesigning it?
6. Is any CE-specific behavior isolated behind an adapter?

If the answer to any question is no, review the architecture
before expanding the implementation.

## Guiding Principle

Preserve Arcanum's behavior and atmosphere.

Replace its technical limitations.

Arcanum-RS must never become a direct translation of Arcanum CE
that reproduces the same architectural problems in Rust.
