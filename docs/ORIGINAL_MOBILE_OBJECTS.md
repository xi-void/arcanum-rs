# Original mobile objects

## Shared format and inheritance

The version 119 standalone reader and sector object reader share the instance
identity header, typed field reader, and appearance projection in
[`sector/objects`](../src/sector/objects.rs). The descriptor layouts are
derived from Arcanum-CE's `obj.h` and `obj.c::sub_40A400`,
`sub_40B8E0`, and `sub_40C030`. The descriptor selects a scalar,
presence-prefixed wide value or reference, string, or sparse array with a
specific element width. The reader validates bitmap population, sparse
element counts, field type, and full standalone consumption. The playable
sector importer still projects its supported static categories. It keeps
Milestone 60's bounded preparation and strict list framing.

An instance bit selects the stored value, including zero or an explicit
optional null. A missing bit permits inheritance when the property supports
it. This follows CE's `obj.c::obj_field_fetch`. Mobile preparation resolves a
VFS-winning prototype by serialized ID and checks its type. It reads the
common appearance and relevant critter stat prefix shared by prototypes,
then distinguishes instance values, inherited values, and unresolved
properties. Complete prototype bodies and gameplay defaults remain outside
the implemented prefix. Location uses the instance placement field.

Each resident mobile retains its validated original payload. Consumers can
read explicitly assigned properties through the typed reader without losing
uninterpreted bits or sparse indices. The prototype cache shares resolved
values and provenance across preparations. Sector cache entries own active
objects and their rasters. ART frames remain shared by the regular cache.

## Discovery, identity and containment

The playable loader enumerates winning standalone `.mob` paths in the active
map namespace. The initial per-map discovery pass retains compact identity,
placement and ownership lookups. Same-map destination changes share that
immutable index. Object bodies are kept only in cached sectors. Ordinary
sector preparation also checks persistent GUIDs against
authored static records in the same sector. If different standalone
resources claim the same GUID in a map, neither copy enters the world.
A contained item can be inspected in its owner's sector without becoming a
world draw or collision occupant.

The canonical original key is `2:` followed by 32 hexadecimal digits in
serialized GUID-byte order. This is distinct from a localized UUID string.
The key survives sector reloads. Static objects retain their
`SECTOR:RECORD` inspection key. Temporary drawable ordering slots remain
internal. There is no saved runtime object registry yet.

Item parent and inventory-slot fields are decoded. Container and critter
inventory arrays supply reverse membership evidence. A parent, inverse
membership, or effective `OF_INVENTORY` prevents independent placement.
Conflicting owners and cycles are reported. References can resolve to a
known map GUID even when the owner is outside the resident sectors. Missing
owners and conflicted containment remain observable. Inventory and
equipment behavior still need runtime state and manipulation rules.

CE's `obj_file.c::objf_solitary_read`, `map.c::map_preprocess_mobile`,
and `map.c::map_load_mobile` connect solitary records to the regular
compiled stream. This implementation uses the installed standalone paths.
Compiled-stream-only resources, saved differences, and scripted spawning
remain future input sources.

## Rendering and collision

Stationary supported critter poses enter the Vulkan scene with their
original tile, palette, offsets and verified shadow rules. Container frames
use CE's `name.c` container table. Ground item names use the original
number/type/subtype and armour coverage rule in
`a_name.c::a_name_item_aid_to_fname`. Item, container, and supported
scenery mobile rasters use the normal draw order and lighting. Recognized
active critters and containers use the shared tile collision map with the
effective object flags. Collision preparation and appearance preparation
remain separate. Some correctly placed bodies retain collision while their
appearance is unresolved. The existing rabbit animation remains a diagnostic.

Unsupported scale, animation and equipment states, lights attached to
mobiles, and gameplay interactions remain explicit limitations. Container, item,
and actor classification gives later systems a common object foundation.
It leaves dialogue, loot, equipment management, combat, and scripting
outside this milestone.

## Inspection

Use the [agent workflow](AGENT_WORKFLOW.md) to launch with a restricted
Unix socket. After a location change, wait for `camera.view_ready`.
The native probe enabled developer collision bypass before the fixed
coordinate arrivals. Use `rpc tcl` if the inspection tile is blocked.

```sh
rpc coc 5011 82 83
rpc objects near 80 81 3 category npc
rpc objects original 2:f7bd680a831dc44eb208b4c36b06bf87
rpc objects original 2:40127b6e3b39d4118f1d00e02920220c
rpc objects original 2:98fd05e54e0ed4118f1d00e02920220c
rpc coc 5000 39628 33599
rpc objects original 2:445b953294385445b9cbc122298325c2
```

These examples inspect the Bates kitchen NPC, contained armour and a
world-placed chest, then an overworld NPC using inherited ART
(`art/monster/ape/apeuwxaa.art`, prototype 28413). Prototype 28416 is
another verified inheritance case whose 110% scale remains outside the
supported draw range. Look at `effective.current_art.origin`, `prototype_source`,
`placement`, `parent`, `inventory`, `rendering`, and
`collision_registered` together. Read-only queries leave the simulation
and redraw schedule alone.

Scanner schema 3 separates full instance framing, property and prototype
evaluation, placement candidates and containment, and actual preparation in
scanned sectors. The Milestone 60 scanner measured an NPC render prefix.
Compare the resource denominator and static sector counters directly, and
use the new mobile counters as a separate baseline.

## Installed-data verification

The schema 3 scan framed all 12,711 discovered standalone instances and
read 118,395 typed fields. Effective current ART came from an instance in
2,770 cases and a prototype in 9,941. Three instances had unresolved
prototype resources. The map indexes found 5,072 world placement
candidates and 7,639 contained objects, with 7,474 owner identities
resolved in their map index. Authored-sector preparation produced 1,859
mobile rasters, including 376 inherited NPC appearances, and 4,213
collision registrations. The scanner tracks registered collision
independently of visible ART.

An Intel HD 520 Vulkan probe used the same 936×500 scene before and after
the change. After the overworld index was built, three same-map arrivals
took a median of 466 ms, compared with 467 ms in Milestone 60. The warmed
scene submitted no additional idle frames during each half-second
sample. Eight hour-triggered redraws had median host time of 2.24 ms
(2.48 ms before). Peak process RSS went from 279 to 283 MiB. The initial
ready view took 1.49 s versus 1.26 s. These are focused runtime samples
with developer audit enabled. Vulkan GPU execution time was unavailable.
Captured frames and exact request evidence are under
`target/visual-checks/milestone-61/`.

### Tarant and Tulla population follow-up

RPC found the five missing Tarant shop NPCs, the inventor and the sampled
Tulla Interior NPCs resident at their authored positions. Their sources
were standalone `.mob` files. The VFS checks found no CE-style compiled
mobile stream for the audited overworld or Tulla Interior, and enumeration
across 79 original map directories and 32 mounted sources found no
matching compiled stream path. The reported absences came from ART
preparation.

The inventor's city clothing was already supported. Tarant's gun smith
uses leather, its quality blacksmith uses chain, and its magickal smith
and both Magick Shop Clerks use robes. One clerk also needs CE's
female-elf filename substitution. Both clerks share the same original
description ID. Tulla's mages use robes and its guardians and Simeon Tor
use the unique-NPC ART family. The standing resolver now uses CE's
clothing codes and body substitutions, plus the VFS-selected
`art/unique_npc/unique_npc.mes` table. It keeps each authored frame,
palette, and facing. RPC exposes the effective `description_id` for
cross-reference with the original message table.

```sh
rpc coc 5000 62083 65611
rpc objects original 2:b84d4703313ad4118f1d00e02920220c
rpc coc 5031 476 214
rpc objects original 2:520322108f1fe941857a9b2c9254c565
```

For fixed-coordinate probes, enable developer bypass as above. Trace the
gun smith to `art/critter/hmm/hmmlaxaa.art` and the Tulla guardian to
`art/unique_npc/hlbuw/hlbuwxaa.art`. Before/after RPC evidence and Vulkan
captures are under `target/visual-checks/milestone-61-population/`.

All eight previously missing probe objects prepared a raster in the
Vulkan session. Tulla Interior went from 1 to 31 prepared NPCs out of 34
placed. Two scaled creatures and one orc with a nonzero blit flag remain
unprepared. The full scan increased prepared NPC rasters from 887 to
1,526. Decode, placement, containment and collision totals were stable.
The scan took 26.9 seconds end to end with 132.6 MiB peak RSS. See the
[compatibility history](WORLD_COMPATIBILITY_BASELINES.json) for counts.

## Mutable instances

[Session objects and inventory](RUNTIME_INVENTORY.md) describes gameplay changes to these instances, current ownership, and projection after streaming.
