# Local and world maps

```sh
cargo run --locked --release --features windowed,gpu -- \
  play "$HOME/Work/arcanum-ce-test" --renderer vulkan
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
- Hover over a known location for its authored description. Click a
  discovered marker to select it. Selection lasts for the session, and
  closing the map restores the existing movement and camera controls.
- **[** and **]** adjust UI scale independently of gameplay zoom. Map
  input stays modal. The viewer samples the player's scene for the
  circular inset when opening the map and suspends gameplay deadlines
  while the atlas is open. The frozen gameplay scene remains visible
  behind the map at the current camera zoom.

The Classic frame is loaded through the same public component loader as
a third-party package. The inset samples the existing world scene at
twice its authored display dimensions and draws `PCWinCvr.ART` over the
scaled result. The world compositor supplies the inset, including the
current lighting and shadow settings. Gameplay is frozen in this alpha
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

## Discovery, diagnostics and future travel

A new session seeds discovery from the authored starting tile and
`GameArea` proximity. Entering a town-mapped sector on the overworld can
reveal a matching nearby location. Negative radii are excluded from
proximity discovery. Quest, dialogue and script-driven discovery remain
unsupported. `--map-reveal` allows inspection of undiscovered markers
without enabling their selection. `--trace` reports raw current
player tiles and town-map IDs in the terminal.

A discovered destination creates an `Atlas::destination` selection with
the area ID, its authored overworld tile, the `START_MAP` ID, and an original
sector availability check. This follows CE's arrival map selection in
`wmap_ui_teleport`. It does not check a route or promise passability.
Travel remains disabled. The future travel layer must validate route
eligibility, advance time, check the arrival tile and perform the map
transition. This milestone adds no teleport shortcut.

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
`map-zoom-in` and `map-recenter`.

The alpha currently omits route overlays, party markers, in-map notes,
map music and full travel controls. The Classic window keeps the
authored canvas, compass, coordinate plates and control artwork. Its
buttons use indexed ART for pressed and selected states. Two-frame
buttons retain their idle artwork on hover when unselected. Its
unavailable gameplay information remains unbound.
