# Original transitions and interactive doors

## Verified base resources

| Source | Original trigger | Destination |
| --- | --- | --- |
| Bates Mansion Level 1 | Six `map.jmp` records at X=109–110, Y=92–94 | Map 5000 at (61976, 65664) |
| Tarant, Bates entrance | Sector 68853695432, scenery record 0 at (61974, 65664), Use script 1267 | Map 5011 at (104, 92) |
| Bates Mansion Level 1 | Sector 67108865, staircase scenery records 179 and 209, Use script 1266 | Map 5012 at (99, 91) |
| Bates Mansion Level 2 | Staircase records 167 and 183, Use script 1267 | Map 5011 at (104, 92) |
| Tulla Interior | `map.jmp` source (307, 219) | Same map at (438, 219) |
| Zephyr crash site | Sector 86570436011, scenery record 7 at (92910, 82598), Use script 2793 | Map 5078 at (2087, 2018) |
| Crash Site Cave | Sector 2080374816, scenery record 99 at (2093, 2019), Use script 1532 | Map 5000 at (92913, 82598) |

The winning VFS resources supply the destinations. The Bates
`map.jmp` is in `modules/Arcanum.dat`. The exterior entrance record
is in `modules/Arcanum.PATCH1`. The original scripts are
`01266bates mansion to 2nd floor_tel.scr` and
`01267bates mansion to 1st floor_tel.scr`.

CE's `jumppoint_read_all` reads a count and 32-byte records.
`object.c` calls `map_process_jumppoint` when the player enters a
tile. Stored destination map 1 is public map 5000, and −1 is
the current map. The interpreter slice accepts a two-row
unconditional Use script containing `SAT_TELEPORT` for
`SFO_TRIGGERER` with literal coordinates and a terminal return.
Other script shapes are diagnosed. Original save overrides,
script variables, tile scripts, effects, and whole-party travel
are outside this slice.

Each transition validates the destination before replacing the scene.
It preserves the player's facing and the selected light hour. Pending
routes and the source map's active collision and raster cache are
discarded with the source scene. Session-local door state is kept by
map, sector, and record. Initial destination preparation is synchronous.
Large maps use a streaming rectangle around the arrival instead of
loading the entire overworld.

## Doors

Door use uses the sector object's original tile, rotation, flags,
and the same opaque pixels and drawing order as the compositor.
The player approaches a reachable tile on either side of the edge.
The door's original four visual states are frame zero and either
frames 1–3 or 4–6, as specified by CE `portal_open` and
`portal_close`. The original frame cadence and each frame's pivot
are retained. Portal edge counts are separate from wall collision,
so opening a door cannot remove a wall. Supported original locks,
jams and magical holding block opening. The bounded Unlock evaluator
supports global-flag comparisons and literal branches on the new-game
path. Unsupported executed instructions refuse opening with a diagnostic.
A matching carried key authorizes each attempt. Its code comes from the
effective key field or an authored key-ring list. CE's `item.c` rejects
zero on either side, accepts code 1 on either side for a nonzero pair,
and otherwise requires equality. `ai_attempt_open_portal` and
`ai_attempt_open_container` retain the key and the authored lock flag.
The runtime applies changes to door pose and collision together. It also
runs supported portal Use scripts after the access check. Lockpicking
and forced entry remain unimplemented.

The Tarant importer also recognizes the original INT32 portal
notification field and the original common scenery fields used by
the winning sector. Unknown categories still fail closed. The
adjacent sector 68786586569 has an unsupported category 17 trap at
record 199. That sector currently has no playable collision data.

## Tarant pedestrian gate and gatehouses

The mansion entrance in the transition table is separate from the
pedestrian door in the street fence. The pedestrian door is portal
record 646 at (61998, 65660) in sector 68786586568 from
`modules/Arcanum.PATCH1`. Its serialized bytes span `78061..78221`.
It uses prototype 2031, ART `0x32c02c00`, rotation 5, palette 0,
offset (0, 0), and the closed frame of
`art/portal/rire3au0.art` from `Arcanum2.dat`. The 41 × 114 closed
frame pivots at (36, 90). The opening is wall record 645,
ART `0x1185a800`. The gate was absent because the importer only
accepted closed frames at rotations 1 and 7. CE's `portal_open`
and `portal_close` use frame 0 for closed doors in every direction,
frames 1–3 for rotations 2–5, and frames 4–6 for 0–1 and 6–7.
TIG derives the mirrored palette for rotation groups 2–3 and 6–7.

The gate's `SAP_UNLOCK` attachment is script 1151,
`scr/01151batesmansiondoor.scr` from `Arcanum2.dat`.
`ai_attempt_open_portal` runs this hook before checking portal flags.
With CE's new-game global flags clear, row 0 jumps to row 3 and
row 3 returns `SAT_RETURN_AND_RUN_DEFAULT`. Flag 1024 instead takes
the handler-removal branch. With 2117 set and 1024 clear, the
handler can toggle the door, play sounds, and schedule a later
close. The engine checks the live global flags on each use or
route check. The winning sector supplies portal flags `0x11`
(`LOCKED | ALWAYS_LOCKED`) and key ID 64 in field 66. In a prepared
save with the original key and both flags clear, clicking the gate
opened it and cleared its collision edge. With flag 1024 set, the
same key and click left it closed and reported the unsupported
attachment removal at statement 1. The timed branch also needs
portal actions, sound and scheduled script execution. The separate
mansion entrance uses script 1267 to return to the interior.

The two gatehouse roofs use `art/roof/gb0.art` (roof ordinal 6).
The north group is in sector 68786586568 at zero-based slots
218, 219, 234 and 235. The south group is in sector
68853695432 at slots 26, 27, 42 and 43. Both use the words
`a031c000`, `a0300000`, `a0300011`, `a0310000` in that
order. The loader already read these lists. Play never enabled
the separate roof pass when creating a scene, including after travel.

Play now starts with the authored roof words. CE's `roof_recalc`
tests the player's completed tile against the piece's 4 × 4
interior rule. A fill change propagates to adjacent roof cells four
tiles away, using the piece-dependent exclusions in `roof_fill`.
An empty cell ends propagation. The play cache keeps the changes
across sector eviction on the active map and resumes blocked edges
when their sectors arrive. A rebuilt scene initializes its roof
state at the arrival tile. For objects, CE tests coverage using
the indoor tile at the object and the roof piece at (x+3, y+3).
Picking uses the visible roof cell at the inverted cursor at
(screen x, screen y+120) and tests its opaque texels.

Partial fading follows CE's `wallcheck_recalc`. It uses the local
player's tile relative to (2, 2) modulo 4, adjusts at wall boundaries
according to rotation, and selects wall runs using its quadrant mask
and 34 run definitions, each sampling up to three wall tiles. The
native viewer derives the current wall and roof fade set from prepared
sector records. CE's object pass still treats faded roofs as covering.
The roof pass uses the piece-specific corner opacities from
`art/roof/roofshade.mes` (255, 200, 80). CPU and GPU use the same
integer corner interpolation with the source ART mask. The separate
CE software stipple mode is outside this slice.

The winning Tarant sectors at X=967–968, Y=1025–1026 have no roof
slots centered in the mansion frontage band Y=65638–65690 except
the two gatehouse groups. The mansion entrance at (61974, 65664)
and the adjacent architectural silhouette use original object ART.
The roof pass draws the authored sector cells and preserves their gaps.

Direct play inspection from the mansion side of the pedestrian gate:

```sh
cargo run --locked --release --features windowed -- play "$HOME/Work/arcanum-ce-test" \
  --map 5000 --center 61997 65660
```

## New game and click-to-move

CE's new-game menu selects `MAP_TYPE_START_MAP` through
`rules/maplist.mes`, then takes that row's X and Y through
`map_get_starting_location`. The original row 5000 from
`modules/Arcanum.dat` is `Arcanum1-024-fixed`, (92958, 82592).
The directory's `startloc.txt` supplies a separate editor/default
loading coordinate. `--map` and `--center` still override the map
and initial position.

The cave links in the table use the same bounded `SAT_TELEPORT`
handler as the mansion stairs. The crash-site plane raster at
(92914, 82546), sector 86503327147 record 2, uses
`small_plane_crashed.art` and remains solid. Campfire records
127 and 138 in sector 86570436012 use animated `campfire.art`.
The site's darkness masks use `LF_DARK | LF_OUTDOOR` and
the original outdoor ambient tint. Flat `OSCF_UNDER_ALL` scenery
goes at the head of the tile's flat-object order. Unknown script
paths, crash-site NPCs and quest interactions remain outside the alpha.

Mouse routes can plan through an edge that becomes traversable by
opening an eligible door. Actual movement stops at that edge and uses
the door, waits for the last opening frame, then replans toward the
original destination. A new click replaces the pending route.
A locked edge remains blocked and can report the
reason the destination is unavailable.

For hover feedback, CE adds a copy of the object's sprite tinted
RGB (200, 200, 200) in the object pass. Play uses the same operator
for supported interaction targets, after ordinary objects and
before roofs, with the original transparency mask, zoom and
currently displayed pose.

## Viewer settings

Integer zoom uses the same logical transform for scene selection and mouse
picking. Vulkan expands the quads and samples exact texels. Pause, lighting,
shadow visibility, movement preference and current zoom remain session options.

Native tests opened Bates billiard-room door record 244, walked through
it, left for Tarant, and closed the still-open leaf after returning.
K’na Tha portal record 558 was refused by its original locked and
always-locked flags. Native round trips covered the Bates front door
and both levels. The separate Tulla jump used the same transition
loader.
