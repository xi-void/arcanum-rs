# Original world compatibility research

Milestone 60 follows original data from discovery through preparation. The [agent workflow](AGENT_WORKFLOW.md) gives reproducible scan, comparison, and RPC commands. The [compatibility history](WORLD_COMPATIBILITY_BASELINES.json) preserves small comparable summaries. Full reports and runtime evidence belong under the ignored `target/visual-checks/` directory.

## How to read the counts

| Layer | What the scanner evaluates | What remains outside this check |
| --- | --- | --- |
| Discovery | Map metadata, winning authored sector paths, indexed terrain template paths, and logical mobile paths under scanned map namespaces | All theoretically addressable tiles, mod sets other than the selected VFS, and unreferenced assets |
| Structural decoding | Sector record lists accepted into preparation, failing-record evidence, and the existing mobile render-prefix decoder | Complete mobile payloads and object-list tails beyond a rejection |
| Semantics and resolution | Supported fields on that path, effective appearance at the renderer's acceptance gates, and prototype and ART lookup reached by the importer | Field semantics after a gate, full ART variant coverage, and dynamic inheritance |
| Placement and appearance | Floors, roofs, and supported static objects prepared by the gameplay loader | Vulkan presentation, clipping outcomes, animations, and visual fidelity |
| Collision, traversal, and interaction | Collision structures and recognized Use payloads prepared by the gameplay loader | Traversal correctness and runtime interaction, lock, quest, and script behavior |
| Dynamic state | Selected authored inputs appear in diagnostics | General scripted behavior and mobile simulation |

A single decoded list can supply hundreds of records and still leave some objects without a raster or interaction. Discovery, structural coverage, and compatibility at runtime have separate denominators. There is no overall completion percentage.

The Milestone 59 `object_lists_decoded` count is the number of lists accepted by the loader, including the preparation guard. `serialized_records_decoded` totals records in those accepted lists. `mobile_format_rejected` counts failed read or render-prefix checks on logical VFS paths enumerated per selected map. `object_appearance_unsupported` counts wall/scenery rejections at the effective appearance gate. `portal_opening_unsupported` counts failed static portal-to-wall associations. The latter two checks can stop work before ART lookup. All four are pipeline observations and can hide downstream effects.

## Original serialization

CE's `src/game/sector_object_list.c::objlist_load` reads the final count, seeks back, and calls `obj_read` for each object. Successful sector reads relocate object locations to the sector tile and run object postprocessing. The CE loop has no Rust overview cap. An actual framing failure invalidates the list. Rust's preparation fallback retains surfaces and separately read roofs but drops the decoded object collection and leaves collision unavailable. Door/Use registration and scenery-owned light preparation depend on that collection. Sector light records are separate from object-owned lights.

CE's `src/game/obj.c::obj_read` reads version and prototype identity, dispatching to the prototype reader for a blocked identity and to `obj_inst_read_file` for instances. Instances store the 24-byte identity, type, overridden field count and a type-dependent bitmap. `sub_40C030` calculates the bitmap width and `sub_40B8E0` sets word-aligned section offsets. Each present bit selects a field descriptor. `obj_field_fetch` supplies inherited values when the field is absent. `src/game/obj_private.c::obj_data_read_file_slow` distinguishes fixed INT32, presence-prefixed INT64 and object IDs, sized strings, and framed sparse arrays. Matching the version and a plausible header establishes the representation family. The complete field table is needed to frame records safely.

### Rejected lists

**All 21 lists decoded through their declared ends.** There are zero observed
serialization failures in this group. The common cause was a post-decode
Rust preparation guard of 1,024 records in streamed/playable mode, or 512 in
the bounded preview. Its placement and mode-specific thresholds established
its role as a limit on scene work. The existing documentation does not
establish a more specific reason for choosing those thresholds. CE's list
reader has neither threshold. The old guard withheld **28,311 decoded
records** in seven authored sectors (8,727 records) and fourteen template
resources (19,584 records). Template counts are records in unique resource
files. Runtime instances depend on terrain placement.

All entries below use metadata marker `0xaa0004`. The reader verified version
119 and accepted the field layout and identity of each decoded record.
The exact declared count equals the decoded count for every row.
There is no failed record, byte offset, category, or prototype associated
with this preparation rejection. File sizes are decompressed VFS lengths.
Authored rows resolve to `modules/Arcanum.dat`; templates to `Arcanum2.dat`.
Template IDs below are encoded filenames, with no asserted world location.

| Map ID | Winning logical resource | Encoded ID | Bytes | Declared and decoded |
| --- | --- | ---: | ---: | ---: |
| 5000 | `maps/arcanum1-024-fixed/80664855162.sec` | 80664855162 | 127,177 | 1,208 |
| 5000 | `maps/arcanum1-024-fixed/106166224215.sec` | 106166224215 | 122,672 | 1,192 |
| 5005 | `maps/bmc/67108872.sec` | 67108872 | 123,544 | 1,213 |
| 5006 | `maps/wheel clan/67108872.sec` | 67108872 | 107,924 | 1,025 |
| 5039 | `maps/ashbury haunted castle/67108867.sec` | 67108867 | 116,887 | 1,146 |
| 5061 | `maps/castle s'nel n'fa/67108871.sec` | 67108871 | 183,750 | 1,915 |
| 5072 | `maps/void-kerghans castle/67108870.sec` | 67108870 | 106,892 | 1,028 |
| — | `terrain/broad leaf forest to plains/134217729.sec` | 134217729 | 128,862 | 1,178 |
| — | `terrain/broad leaf forest to plains/134217730.sec` | 134217730 | 137,412 | 1,268 |
| — | `terrain/broad leaf forest to plains/134217731.sec` | 134217731 | 121,927 | 1,105 |
| — | `terrain/broad leaf forest to plains/67108865.sec` | 67108865 | 137,507 | 1,269 |
| — | `terrain/broad leaf forest to plains/67108867.sec` | 67108867 | 124,872 | 1,136 |
| — | `terrain/plains to broad leaf forest/0.sec` | 0 | 153,467 | 1,437 |
| — | `terrain/plains to broad leaf forest/1.sec` | 1 | 163,537 | 1,543 |
| — | `terrain/plains to broad leaf forest/134217729.sec` | 134217729 | 153,277 | 1,435 |
| — | `terrain/plains to broad leaf forest/134217730.sec` | 134217730 | 157,837 | 1,483 |
| — | `terrain/plains to broad leaf forest/134217731.sec` | 134217731 | 155,842 | 1,462 |
| — | `terrain/plains to broad leaf forest/2.sec` | 2 | 176,647 | 1,681 |
| — | `terrain/plains to broad leaf forest/3.sec` | 3 | 192,227 | 1,845 |
| — | `terrain/plains to broad leaf forest/67108865.sec` | 67108865 | 140,642 | 1,302 |
| — | `terrain/plains to broad leaf forest/67108867.sec` | 67108867 | 153,752 | 1,440 |

The replacement checks an 8 MiB per-sector charge for object metadata,
including decoded vectors, import descriptors, draw descriptors and an
allowance for debug provenance. The existing 8 MiB sector input limit,
minimum record header check, strict framing, 12-entry sector cache and
96 MiB decoded ART cache bound separate resources. The metadata charge
estimates preparation storage. It excludes allocator overhead, texture
pixels and VFS indexes. An exceeding charge yields a structured preparation
rejection before ART import or collision/interaction preparation. See the
scan comparison below for measured recovery.

## Standalone mobile resources

`src/game/obj_file.c::objf_solitary_read` calls the same `obj_read` as sectors. CE's `src/game/map.c::map_load_mobile` has an editor branch that reads separate `.mob` files. The regular path reads a module-GUID-prefixed compiled stream of objects, and then applies differences, deletes and map patches. Its compile path, `map_preprocess_mobile`, writes that stream from solitary files. Rust's full scan inspects the winning standalone VFS paths. It does not establish the contents or availability of CE's generated stream or interpret saved changes.

The Rust `decode_mobile` check is a permanent-NPC render-prefix decoder. It reads selected common fields and dexterity and only checks that some subtype data remains when relevant. A successful prefix is weaker evidence than a complete decode. Gameplay currently prepares selected legacy mobile fixtures in `src/content/mobile.rs::records`. Improving the decoder and discovering normal world mobiles are distinct tasks.

The scan observed **12,711 logical paths** across inspected namespaces.
There was one winning read per reported path in this dataset. Physical
archive entries, overridden copies and runtime object instances have
different counts. All 12,711 prefixes store version 119, prototype identity
kind 1 and instance kind 2 (GUID). All available field counts equal the
set bits across their known-width bitmaps. The observed 3-, 4- and 5-word
layout families follow the common/type sections. Reading the first current
ART and location values reuses the existing typed field reader. This
establishes neither full payload framing nor that a stored tile is the
object's effective world position. Parent/inventory columns count **field
bits**, which may store empty optionals or refer to unresolved identities.

| Object type | Paths | Render-prefix accepted | Nonzero location stored | Current ART override read | Parent bit | Inventory-list bit |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| ammo | 423 | 0 | 423 | 0 | 421 | 0 |
| armor | 1,440 | 0 | 1,440 | 35 | 1,432 | 0 |
| container | 906 | 0 | 906 | 448 | 0 | 635 |
| food | 2,135 | 0 | 2,135 | 7 | 1,909 | 0 |
| generic | 1,092 | 0 | 1,092 | 75 | 1,016 | 0 |
| gold | 797 | 0 | 797 | 0 | 768 | 0 |
| key | 97 | 0 | 97 | 0 | 96 | 0 |
| key_ring | 2 | 0 | 2 | 0 | 2 | 0 |
| npc | 3,781 | 2,170 | 3,781 | 2,170 | 0 | 1,389 |
| scroll | 180 | 0 | 180 | 0 | 176 | 0 |
| weapon | 1,723 | 0 | 1,723 | 27 | 1,709 | 0 |
| written | 135 | 0 | 135 | 8 | 122 | 0 |

The 10,541 prefix failures split exhaustively into **8,930 non-NPC
categories** excluded by the reader's early type check and **1,611 NPCs**
with no instance current-ART field. The latter group already reached the
missing-ART check. CE's inheritance rule is directly relevant. For example,
map 5000 `g_00d0f513_95b0_4e50_9215_ce7f1ec221a0.mob` from the module has
prototype 28416, tile (82590, 45634), and no bit 0. The winning
`data/proto/028416 - NPC.pro` has an NPC prototype header and current ART
`0xc8002010` at its first common field. This confirms a real inheritance
case. Whether every other NPC prototype and ART variant resolves remains
unmeasured. The current read prefix and standing-pose support can still
reject a complete original appearance for other reasons.

Example non-NPC instances include food prototype 10079 and armor prototype
8111 in the overworld's module archive. Both carry a location and an item
parent bit. CE's `OBJ_F_ITEM_PARENT` is a serialized object ID handle;
`OBJ_F_CONTAINER_INVENTORY_LIST_IDX` and
`OBJ_F_CRITTER_INVENTORY_LIST_IDX` are object-ID arrays. CE postprocesses
references into handles as appropriate. Cargo, worn equipment, and objects
stored at a tile can share this representation. A standalone-file count
cannot be converted into a count of NPCs or pickable world objects.

## Appearance gate

CE's `src/game/object.c::object_draw` applies the regular-play `OF_OFF`/`OF_DESTROYED` filter, the `OF_DONTDRAW` mask, and conditional invisible detection. Editor visibility can differ. Its wading branch treats the bottom 15 pixels of upright sprites specially for `OF_WADING` without `OF_WATER_WALKING`. Geometry, blending, and original state all matter. An authored suppression flag explains the missing normal draw for that state. It does not settle collision, later activation, or editor visibility. The Rust gate diagnostics preserve effective flags, mask violations, subtype flags, scale and blend. The cause bucket names the first applicable research classification in the documented precedence order. Evidence retains concurrent predicates.

All **1,593** original appearance warnings were scenery at this gate.

| Effective predicate reported | Original occurrences | Interpretation |
| --- | ---: | --- |
| Wading flag | 762 | CE has a dedicated water draw branch. Reference the original state and sprite before changing blending. |
| Regular-play suppression flag | 371 | CE skips the normal draw in this state. Retain the visibility record and investigate independent collision, light or activation behavior where needed. |
| Subtype flag or flat-pass constraint | 258 | Requires subtype semantics. The first observed pair has `OSCF_NOCTURNAL` bit 0x4 alongside additive parameters. |
| Blend or scale constraint | 198 | Rust rejected effective draw parameters before resolving the sprite. CE behavior needs individual variant checks. |
| Other object flag | 4 | Representative Tulla prop has `OF_DONTLIGHT` and disallowed subtype flags. The combined semantics need verification. |

Precedence is suppression, invisible detection, wading, inventory, blend/scale,
other object flags, then subtype/flat constraints. The invisible-detection
and inventory buckets had zero occurrences in the baseline. Examples can
carry several conditions. The 762 and 258 counts identify gate cohorts.
They do not establish a missing ART file or prove any particular rendering
correction sufficient.

Map 5000 sector `18387829941` record 10 (prototype 4030) has a neutral
opaque blend, scale 100, and only `OF_WADING` as the rejected object flag.
The scenery name resolves to `art/scenery/p_fern_7.art`. Sector
`32682017211` record 98 (prototype 4034) is a stronger negative visual
case. The Vulkan RPC audit reports the stored suppression predicate and
no raster, while `art/scenery/table_with_crystal_ball.art` decodes.
Nearby record 78 (prototype 4056) shows the distinct nocturnal flag gate.
CE consults `OSCF_NOCTURNAL` for light handling in
`src/game/sector_light_list.c` and `src/game/light.c`. Its full day/night
drawing and light behavior should be verified as a separate stage. That
flag alone does not establish that an object should be drawn or lit at a
particular hour.

## Portal association

Rust's `matched_opening` requires exactly one same-tile wall, a supported wall ART piece in 21–33, and matching rotation before it invokes `PortalImport::resolve`. CE's `src/game/sector_object_list.c::sub_4F20A0` uses neighboring wall and portal nodes in **draw order**. It can insert a portal with no matching wall and does not check piece or rotation in that comparison. CE also distinguishes doors and windows in `src/game/a_name.c::a_name_portal_aid_from_wall_aid`: doors use pieces 22, 25, 26, 29–32 and windows use 10, 13, 14, 17–19. That function derives a portal ID from a wall. It is separate from loading a serialized portal's existing ID and resolving it through `a_name_portal_aid_to_fname`.

CE's `src/game/portal.c::portal_is_open` checks current ART frame. `src/game/object.c::object_locked_get` evaluates authored lock flags and time conditions independently. The scanner's association failures come before these runtime decisions. It does not identify a legitimately locked door as a mismatch because it is locked. The collision layer in `src/content/collision.rs` consumes decoded wall/portal records independently of scene association, so a missing portal raster can coexist with a collision edge. Door interaction preparation depends on the imported portal. Changing the render gate alone would leave separate collision and interaction semantics to test.

| Static association result | Occurrences | Stored portal ART |
| --- | ---: | --- |
| Wall piece outside the Rust accepted range, same rotation | 1,030 | 1,030 windows |
| Multiple same-tile wall records | 4 | Three doors, one window |
| No same-tile wall record | 1 | One door |

There were no rotation-only warnings in this baseline. The window group
is a confirmed gap in the generic wall-piece filter. The scan establishes
which check failed, and the two representative window pieces are verified
against CE. The exact CE outcome of every warning still needs individual
asset/state or runtime evidence. For no-wall and multi-wall examples,
address authored draw order first and retain original restrictions when
testing traversal. There is a separate eight-occurrence legacy portal
variant group beyond the association warnings.

The Vulkan RPC probe inspected map 5000 sector `32614908347` portal 129 at
(28406, 31142). The stored window and its wall record 128 both have rotation
5. Wall piece 17 is in CE's window set. The wall produced a raster, the
portal had `no_raster`, and the separately resolved original window
`art/portal/touf5au1.art` decoded. Record 141 is a distinct rotation-1,
piece-13 window in the same sector. This isolates the first rejection
stage without inferring a lock or script effect. Record 179 in sector
`65431142903` has two same-tile walls; record 54 in sector `88248157567`
has none. Their gameplay outcomes remain unverified. The captured Vulkan
view and structured object queries are in `target/visual-checks/milestone-60/`.

## Dependencies and implementation order

1. **Milestone 61, generic object framing.** The immediate post-decode guard is corrected in Milestone 60. Keep the dense-sector regressions. Use one bounded typed-field reader for sector and standalone instances. Start with NPC current-ART inheritance and verify prototype and body framing before declaring the 1,611-prefix group supported. Preserve original object IDs and explicit overrides, verify full consumption at known list/file boundaries, and fail closed where a boundary is unknown.
2. **Milestone 61, mobile discovery and references.** Decode the structurally supported categories against the same field descriptors. Validate contained objects before spawning placed objects, including two-pass ID/parent/list resolution. Inspect regular compiled-stream versus solitary-source availability before selecting a loading path. Start with placed containers, scenery, and NPC fixtures with a known tile, then add item containment. NPC behavior and inventory gameplay remain independent. Treat successful re-scan counts as structural evidence.
3. **Milestone 62, portal rendering and interactions.** Establish a generic ordering rule using serialized door and window ART independently from interaction. Cover window wall pieces, differing rotations, standalone portals and multiple walls before expanding locked or scripted interactions. Compare matched authored records at runtime using current ART frame, collision edges, visibility, and lock/Use evidence. Add unaffected closed and open doors from Bates and a separate town fixture.
4. **Milestone 62, appearance semantics.** Implement original visibility and wading first where verified. Audit remaining flag and blend combinations with per-state runtime evidence. Expand prototype and ART rules only where a lookup was reached or a source comparison establishes the dependency. A legitimately suppressed record should remain accounted for when its draw policy is corrected.
5. **Later milestones.** Save/load should follow stable IDs, object reference resolution and runtime state ownership. Original audio can proceed independently once the needed resource identification is established. AI, dialogue, inventory gameplay, spellcasting, combat and general scripts build on those foundations.

Beyond restoring normal preparation for dense sectors, this milestone leaves appearance, mobile discovery, portal and script behavior for the follow-ups above.

## Measured recovery and verification

The intermediate schema 2 scan matched every Milestone 59 legacy coverage
field and parent count. The final schema 2 scan used the same discovered
resources. These changes measure admission into normal preparation.

| Measure | Milestone 59 | Milestone 60 | Change |
| --- | ---: | ---: | ---: |
| Authored / template resources attempted | 1,663 / 413 | 1,663 / 413 | Same scope |
| Accepted / rejected object lists | 2,055 / 21 | 2,076 / 0 | All 21 admitted |
| Records in accepted lists | 289,553 | 317,864 | +28,311 |
| Prepared walls | 87,899 | 93,607 | +5,708 |
| Prepared scenery | 196,966 | 218,422 | +21,456 |
| Prepared portals | 980 | 1,054 | +74 |
| Resources with collision preparation | 2,055 | 2,076 | +21 |
| Recognized authored Use transitions | 230 | 234 | +4 |
| Mobile prefix failures | 10,541 | 10,541 | Same behavior |
| Appearance warnings | 1,593 | 1,640 | +47 newly evaluated |
| Portal association warnings | 1,035 | 1,042 | +7 newly evaluated |
| Parsed traps without runtime support | 1,044 | 2,063 | +1,019 newly evaluated |

The 27,238 newly prepared wall, scenery and portal records plus the 1,073
newly reported trap, appearance and portal records reconcile to all
28,311 newly admitted records. Use targets are a downstream measure and
overlap with the object counts. One additional unsupported interaction
is also reported. No newly admitted static object was rejected at the
raster-overhang stage.

The full release scans took **44.497 s / 126,676 KiB** in Milestone 59,
**29.543 s / 129,312 KiB** for schema 2 classification, and
**19.188 s / 131,320 KiB** after recovery. I/O cache and concurrent load
differed across runs. Treat them as measured duration and peak process
RSS under each run, with no speedup conclusion. The corrected scan peaked
at about **128 MiB**, 4.5 MiB above Milestone 59.

### Dense original runtime fixture

With map 5061, sector `67108871` and camera (480, 96), a focused release
headless single-sector run with audit enabled spent **5.1 → 46.5 ms** in
loading and preparation. Object parsing alone was **0.42 → 0.48 ms**.
The additional work is primarily import (**0 → 21.8 ms**) and raster/provenance
preparation (**0.78 → 19.0 ms**). That run reported **1,015 supported
object rasters**, with **109 submitted** and 906 viewport-culled. A
separate playable sector scan registered 21 door targets and one other
interaction target and prepared collision. In Vulkan, RPC inspection found
door `67108871:955` closed and unlocked, with a registered door interaction.

On Intel HD 520 at 936×500, the same RPC arrival benchmark ran at the
same coordinates with the same debug settings. Three map loads per
binary gave median time from request to view readiness **282 → 348 ms**.
Ten hour-triggered warm redraws had median host frame time
**2.44 → 3.42 ms**, with worst samples **4.13 → 4.59 ms**. The warm view
submitted 373 → 482 draw packets and retained 1.52 → 2.45 MiB of Vulkan
texture payload in the sampled first warm frame. Peak runtime RSS
increased **219 → 253 MiB**. These are host timing and process memory
measurements with debug provenance enabled. GPU execution time was
unavailable in this runtime sample. Arrival used the same explicit
developer collision bypass in both runs.

The before/after Vulkan images, portal resource probe, runtime logs and
full scan reports are under `target/visual-checks/milestone-60/`. They
verify original content entering the normal loader and presentation
path. Door registration and collision preparation are verified for the
fixture. Traversal through each restored doorway, script outcomes, and
visual correctness throughout recovered sectors remain outside this
measurement.

To reproduce the dense-sector diagnostic after launching the debug game
from the agent guide, use `rpc tcl` to enable collision bypass, then
`rpc coc 5061 480 96`. Wait for `camera.view_ready` and query
`rpc sectors` or `rpc objects id 67108871:955`.
