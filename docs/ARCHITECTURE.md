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

#### Headless effect activation ownership

An immutable effect definition, a source instance, a target actor, and
an activation each have distinct identities. A source can be an equipped
item instance or another runtime owner. Each activation records which
definition supplied it and the contribution actually granted to its
target, such as the applied count or the individual grants. Removal
selects the activation owned by the ending source. Two sources that grant
the same definition to the same target remain independently removable.

Sword of Defense is the reference case. Its wield-on script derives a
count of effect 128 grants from aptitude and saves that count. Wield-off
uses the saved count. A native implementation must keep the outstanding
grants or applied count across aptitude changes so it can retire the
previous contribution before any refresh.

[The headless runtime](../src/runtime.rs) implements this contract for
explicit equipment grants, additive AC, Fire and Poison resistance floors,
and Electrical resistance additions.
`Runtime` owns actor base AC and underlying resistance values, item
associations and active records. Its typed actor, item instance
and activation IDs are allocated deterministically and never reused
within a runtime. Callers keep these handles with their issuing runtime.
Definitions remain immutable references supplied by the caller, and
their namespaced IDs are independent of runtime handles.

An item can have one wearer and grant effects only to that wearer.
Equipping associates the item, and each explicit grant creates an
activation with its source, target, definition and granted adjustments.
Removing an activation by ID leaves other activations intact. Unequipping
removes all activations from that source in one transition. Repeated
equip, unequip and removal return explicit errors. Effective AC is
recomputed from base AC and the remaining recorded points. Unsupported
operations and changes that would leave a total outside the signed
32-bit range fail before state changes. This includes a removal if
cancelling adjustments had kept the total in range.

AC and Electrical points are captured at activation. A Fire or Poison floor
stores the minimum as an active constraint. The Electrical intermediate
value is the actor's current signed underlying value plus its active
additions. The Fire and Poison intermediate values take the maximum of
each current underlying value and its active floors. CE evaluates additions before
bounds. This slice supports one of those stages per channel, with no
channel combining both.

The ordinary queries clamp the intermediate values to 0–95% without
modifying the inputs or active grants. A Poison floor of 97% therefore
remains 97% at the intermediate stage when the underlying value is at
or below 97%. This follows CE's ordinary
resistance query and omits its monstrous NPC exception. Electrical
overflow checks cover new grants, changes to the underlying value and
removals that could expose a previously cancelled overflow. The runtime
checks AC and Electrical totals before revoking grants or changing
equipment ownership. An unsupported operation rejects the entire
activation.

Effect 54's Fire minimum of 95%, Electrical addition of eight points,
and Poison minimum of 97% can now run as one activation. The runtime
collects each supported operation by channel before inserting the
activation. Changing clause order leaves the result unchanged.
Removal revokes the entire activation after checking the remaining
totals, so a failure leaves both floors and the addition intact.
The composite demo supplies the three verified clauses as one native
definition with effect 54's ID. The Electrical demo still exercises
the isolated +8 operation under its own ID. Both demos equip synthetic
items. Spell casting, transformation, duration, Constitution-derived
Poison resistance, and other resistance operations remain outside
this runtime slice.
[Resist Fire evidence](CONTENT_FORMATS.md#milestone-13-bounded-effect-definitions)
· [Body of Fire evidence](CONTENT_FORMATS.md#milestone-14-composite-effect-definitions)

The equipment demo supplies two item instances and counts of one and fifteen.
Effect 128's +1 AC and these counts come from the Sword of Defense
trace. The fixture items, base stat and simultaneous sources are
demonstration choices. The runtime receives explicit grants and has no
aptitude formula, script interpreter, slot rules or serialized state.
[Sword of Defense evidence](CONTENT_FORMATS.md#sword-of-defense-wield-scripts)

### 5. Behavioral Compatibility

Preserve original Arcanum gameplay behavior where appropriate,
including:

- Character progression
- Combat rules
- Magic and technology
- Inventory and crafting
- Dialogue and quests
- World simulation

Compatibility means preserving original game content and intended
gameplay behavior, not reproducing implementation defects.

Arcanum CE serves as a reference for established algorithms, formulas,
resource interpretation, and observable gameplay behavior.

Proven algorithms may be reused or adapted where appropriate, subject
to licensing and code provenance.

Behavioral compatibility does not require architectural equivalence.
Legacy implementation details must not dictate native Rust data models.

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

Preserve Arcanum's content, mechanics, and atmosphere.

Replace its technical limitations.

Use Arcanum CE as a source of behavioral evidence and proven algorithms,
not as the architectural blueprint for the new engine.

Design independent Rust systems around explicit ownership, native
content models, and extensibility.

Compatibility is an external contract. Architecture is our choice.
