# Original transitions and interactive doors

## Verified base resources

| Source | Original trigger | Destination |
| --- | --- | --- |
| Bates Mansion Level 1 | Six `map.jmp` records at X=109–110, Y=92–94 | Map 5000 at (61976, 65664) |
| Tarant, Bates entrance | Sector 68853695432, scenery record 0 at (61974, 65664), Use script 1267 | Map 5011 at (104, 92) |
| Bates Mansion Level 1 | Sector 67108865, staircase scenery records 179 and 209, Use script 1266 | Map 5012 at (99, 91) |
| Bates Mansion Level 2 | Staircase records 167 and 183, Use script 1267 | Map 5011 at (104, 92) |
| Tulla Interior | `map.jmp` source (307, 219) | Same map at (438, 219) |

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
jams and magical holding block opening. Unknown Use or Unlock
scripts retain the original portal state. Key inventory, lockpicking,
and scripted side effects are outside the playable subset.

The Tarant importer also recognizes the original INT32 portal
notification field and the original common scenery fields used by
the winning sector. Unknown categories still fail closed. The
adjacent sector 68786586569 has an unsupported category 17 trap at
record 199. That sector currently has no playable collision data.

## Viewer settings

Integer zoom uses the same logical pixel transform for scene
selection, mouse picking, and both compositors. CPU expands nearest
pixels for presentation and GPU expands sprite quads and loads exact
texels. Pause stops player and animation time. Lighting, shadow
visibility, movement preference and current zoom are session options.
Change the backend at launch using `--renderer cpu|vulkan|gles`.

Native tests opened Bates billiard-room door record 244, walked through
it, left for Tarant, and closed the still-open leaf after returning.
K’na Tha portal record 558 was refused by its original locked and
always-locked flags. Native round trips covered the Bates front door
and both levels. The separate Tulla jump used the same transition
loader.
