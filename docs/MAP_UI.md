# Local and world maps

```sh
cargo run --locked --release --features windowed -- \
  play "$HOME/Work/arcanum-ce-test" --discover-all
```

- **M** or the Classic map icon opens the map. **M** or **Escape** closes it.
  CE uses **W**. The alpha keeps **W** for walking.
- The original magnifying-glass button opens the local map when the
  current sector has one. Otherwise it opens the detailed world map.
  The original whole-map button opens the continent overview. **Z**
  switches between the overview and the local/detail view. **Shift+Z**
  opens the detailed world map directly.
- Drag or use the arrow keys to pan the local or detailed world map.
  Use the wheel to change atlas zoom. Scrolling in from the overview
  enters the detailed sheet. **Home** and the original navigate button
  recenter the detailed views. Use **Tab**, **Enter** and **Space** to
  operate map buttons with the keyboard.
- Hover over a known location for its authored description. In the detailed
  world map, click terrain or discovered locations to add waypoints in order.
  Click an existing waypoint and then another position to move it. Right-click
  a waypoint to remove it, or right-click elsewhere to remove the selected
  or final waypoint. On the continent overview, click an area to center the
  detailed map there.
- Click the green travel button on the lower right to start the route. Use
  the same button, right-click, **M** or **Escape** to cancel a journey.
  Closing the atlas restores on-foot controls.
- **[** and **]** adjust UI scale independently of gameplay zoom. Map
  input stays modal. The viewer samples the player's scene for the
  circular inset when opening the map and suspends gameplay deadlines
  while the atlas is open. The frozen gameplay scene remains visible
  behind the map at the current camera zoom.

The Classic frame is loaded through the same public component loader as
a third-party package. The inset uses the current world draw list at twice its authored display
dimensions. The Vulkan compositor writes it to a resident texture, and the
theme draws `PCWinCvr.ART` over it. The same lighting and shadow settings apply. Gameplay is frozen in this alpha
map view. CE may continue rendering and updating the gameplay inset.

## Original data and coordinates

CE's `src/ui/wmap_ui.c`, `src/game/townmap.c`, `src/game/area.c` and
`src/game/sector.c` establish the following.

| Source | Native use |
| --- | --- |
| `art/interface/interface.mes`, entry 138 | `MapMain.ART`, 800×400. The compass is part of this frame. CE's separate compass arrow setter is a stub. |
| Interface entries 139, 198, 217 and 811–813 | Navigate, player lens cover, continent navigate cover, magnifier, overview and travel artwork. The manifest uses frame-local positions derived from `wmap_ui_window_frame` at (0,41). |
| Interface entries 204–206 and 840 | Player cross, location and waypoint ART, and `NewTimes16Font.ART`. The theme also supplies the small continent marker colors. |
| `worldmap/worldmap.mes`, rows 20 and 50+ | Coordinate display offsets and the original atlas grid. The stock detailed sheet uses 64 indexed BMP tiles, `SmallMapChunks001.bmp` through `064.bmp`. The separate continent overview uses the authored 365×365 `Map_Zoomed.bmp` from `ZoomedName`. |
| `mes/gamearea.mes` | Area identifiers, overworld tiles, display names, descriptions, label offsets and proximity radii. |
| `rules/maplist.mes` | `START_MAP`, optional `WorldMap:` and `Area:` link fields. A linked interior uses the authored area position for its world marker. |
| `rules/townmap.mes`, base-sector prefix, and `townmap/<name>/<name>.tmi` | Sector town-map ID, waitable flag, and version-2 center, dimensions, grid and scale. Local art is archived indexed BMPs with zero-based six-digit suffixes. |

For the installed base world, continent coordinates are
`(2000 - tile_x / 64, tile_y / 64)` using integer division. CE displays the
cursor in `N W` and `N S` form using row 20 offsets. Local-map pointer
motion leaves this world readout as it was. Local player marks use
the relative isometric projection of their tile from the TMI center,
translated by half the authored unscaled dimensions and multiplied by the
TMI scale. The stock local BMPs are 64×32. Local positions and pointer
handling share the same UI-scale-aware transform as their map canvas.

CE displays all mounted bitmap tiles on waitable town maps. For nonwaitable
maps, the alpha retains each tile visited in the session and uses CE's
neighbor-corner weights to interpolate the edge tint. Missing local
fragments stay black. The alpha uses the current player tile rather than
multipart party positions and has no persistent map notes or custom pins.

### Stock data checks

| Area | ID | Overworld tile | Detailed atlas pixel |
| --- | ---: | --- | --- |
| Shrouded Hills | 1 | 90226, 84176 | 591, 1315 |
| Crash Site | 2 | 92960, 82592 | 548, 1290 |
| Qintarra | 6 | 98922, 42009 | 455, 656 |
| Tulla | 11 | 28455, 31171 | 1556, 487 |
| Tarant | 21 | 62182, 65662 | 1029, 1025 |
| Vendigroth Ruins | 22 | 19704, 37452 | 1693, 585 |

All metadata and artwork use the winning resource in the installed VFS,
including selected mod packages. The map bitmap decoder is bounded and
supports the installed uncompressed indexed Windows BMP format.

## Discovery and world travel

The new game still starts at the Crash Site at (92958, 82592). Its sector has
town-map ID 2. World travel becomes available when the player reaches an
outdoor overworld sector with no town-map association and no original
sector block. On the installed atlas, `MapKeyedTo: 1` refers to CE's
one-based map-list index. The first row, MES ID 5000, is the original
overworld and `START_MAP`. The Classic HUD switches between
`TMap_But.ART` and `WMap_But.ART` as eligibility changes. The same
rules apply when opening the atlas from the keyboard. The alpha allows
read-only world-map inspection from local views, including interiors
with a world atlas. CE also checks combat, party readiness, and active
encounters, which the alpha does not simulate.

Verified launch points for testing are on the **overworld**. Tulla Interior
(map 5031) and the separate K’na Tha map (5067) have different coordinate
systems and are not valid overland departure maps.

```sh
# Tulla western outskirts, an eligible developer departure point
cargo run --locked --release --features windowed -- \
  play "$HOME/Work/arcanum-ce-test" --discover-all \
  --map 5000 --center 28320 31168

# K’na Tha approach, using GameArea row 58 exactly
cargo run --locked --release --features windowed -- \
  play "$HOME/Work/arcanum-ce-test" --discover-all \
  --map 5000 --center 91902 39305

# Crash Site outskirts, beyond the town-mapped sector. Travel to Shrouded Hills.
cargo run --locked --release --features windowed -- \
  play "$HOME/Work/arcanum-ce-test" --discover-all \
  --map 5000 --center 92960 82692
```

To inspect Tulla's actual exterior arrival, launch at the `GameArea` row 11
coordinates. The three positions serve different purposes. The western starting
tile is walkable and travel-eligible, the authored exterior arrival is beside
the city's walls, and map 5031 is a separate interior.

```sh
cargo run --locked --release --features windowed -- \
  play "$HOME/Work/arcanum-ce-test" --map 5000 --center 28455 31171
```

For developer exploration, add `--debug` and press **F6** to toggle
collision for keyboard movement. The status appears in the window.
This works within loaded original or template sectors. Click-to-move and
object Use retain their ordinary rules. Automatic jump points are suspended
during bypass. To restore collision, come to a walkable tile and press **F6**.
A map transition resets bypass.

Crash Site is initially known. Overworld proximity discovery uses the
authored `GameArea` radius. Virgil's introductory dialogue
`dlg/01324virgil.dlg` contains the `mm 1` action. CE's
`DIALOG_ACTION_MM` calls `area_set_known` with area ID 1 for Shrouded Hills.
That dialogue is outside the alpha. `--map-reveal` only shows otherwise
undiscovered markers for inspection. `--discover-all` makes the original
locations known for testing while preserving every departure and routing
restriction. `--trace` reports player tiles and town-map IDs.

CE's `wmap_path_make` uses eight directions, directly approaches each
waypoint, and detours in a 16×16 sector grid around authored barriers.
The alpha follows those rules with deterministic tie-breaking. The winning
`terrain.tdf` uses a 1.2 header and length-prefixed zlib rows of 16-bit
terrain words. A `/b` on either terrain type in `terrain/terrain.mes`
blocks sectors without authored `.sec` data. `map.sbf` blocks override
both. The route supports 30 waypoints and 5,000 sector steps. Wilderness
waypoints can cross passable original terrain. A final arrival requires an authored sector or a resolved original terrain
template, followed by valid on-foot collision. Named destinations use
the exact `GameArea` tile. An unnamed waypoint uses the original sector
center. World loading follows the original terrain-template lookup for wilderness
sectors without a map-local `.sec`.

Results advance one sector per travel event (50 ms first step and 62 ms
thereafter, before presentation delays). Each traversed sector adds one
game hour, including a final step. The original source computes travel
time from the Chebyshev distance in 64-tile units. Outside the pause
menu, real elapsed time advances game time at CE's 8× rate. The original
initial hour is read from `rules/gameinit.mes`, entry 5. Arrival and
normal play use the same clock and ambient schemes, and the HUD clock
uses the original day strip and moon phase artwork. The modal map freezes
on-foot movement, keeps the gameplay scene behind the window, and updates
the party marker while travelling. An original portal and an overland
arrival share the same streaming and collision preparation.

Cancellation attempts to resume on foot at the current route sector
center. If the center is blocked or has no authored on-foot data, the
player resumes where they departed and keeps the elapsed game time.
The route step that advances the party marker and discovers areas is
the extension point for future encounter checks. CE's source exposes
encounter state and access restrictions. Encounter generation, arbitrary
terrain synthesis, and script-driven changes to sector travel blocks are
outside this slice.

## External layout

`--map-ui /path/to/package` or `--map-ui /path/to/theme.toml` replaces
the atlas package independently of `--ui`. `--ui off` preserves keyboard
map access. The map uses the same format, VFS precedence, font loading,
layout and input system as the gameplay HUD. `fit_viewport = [800, 600]`
in the Classic package reserves its original proportions and can cap
effective UI scale on smaller windows. It does not scale the world.

```sh
cp -r ui/classic-map /tmp/arcanum-map-theme
# Edit an asset, label color, anchor or offset in /tmp/arcanum-map-theme/theme.toml.
target/release/arcanum-rs play "$HOME/Work/arcanum-ce-test" \
  --ui ui/field-notes --map-ui /tmp/arcanum-map-theme
```

`binding` connects a component to shared state. `map-canvas` is required.
`player-lens`, `map-west`, `map-south`, `map-status` and `map-title` are
optional. Classic uses the original message region for location
descriptions and leaves the title slot unused. The unplaced
`place-marker`, `selected-marker`, `player-marker` and
`continent-*-marker` template components define marker artwork,
dimensions and color. The `place-label` template styles authored world
location names using their original offsets. `map_views` restricts drawing and input to named
views (`local`, `world`, `continent`). The public map actions are
`open-map`, `map-local`, `map-world`, `map-overview`,
`map-zoom-in`, `map-recenter` and `map-travel`. The optional
`waypoint-marker` binding and top-level `route_color` and
`selected_route_color` style straight dashed segments from the live party
marker through each outstanding waypoint. The terrain-aware route and its
journey timing remain independent of these visual connections. A component can use `travel_available = true`
or `false` to select the enabled or local form of the map icon. The
`game-clock` binding takes the day strip in `image` plus eight original
moon strips in `clock_phases`.

The alpha currently omits multi-member party markers, in-map notes,
and map music. The Classic window keeps the
authored canvas, compass, coordinate plates and control artwork. Its
buttons use indexed ART for pressed and selected states. Two-frame
buttons retain their idle artwork on hover when unselected. Its
unavailable gameplay information remains unbound.
