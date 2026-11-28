# Developer console

With the game data configured, run `target/release/arcanum-rs`. Set `enabled = true` under `[developer]` in the configuration to use the console. For an explicit developer launch, run the following command.

```sh
cargo run --locked --release --features windowed -- \
  play "$HOME/Work/arcanum-ce-test" --debug
```

**F10** or the physical grave key opens and closes the console. On a German
keyboard the physical key is the one marked **^** to the left of **1**.
**Escape** also closes the console. Close the pause menu or map before opening it.

- **Enter** runs a command. **Tab** completes a unique command name or lists matches.
- **Up/Down** browse session history. **Left/Right**, **Home/End**, **Backspace** and
  **Delete** edit the line. Hold **Shift** to select text or use **Ctrl+A**.
- **Ctrl+V** pastes Wayland clipboard text using `wl-paste` and the
  `timeout` utility. The console reports clipboard errors in its output.
- Use the mouse wheel or **Page Up/Down** to scroll. Click an exposed world
  sprite to select it for inspection. Console input and clicks leave normal
  player commands inactive while the panel is open.

Put multiword names in quotes, for example `coc "K'na Tha"`. You can also
use a numeric map ID. Enter `help` or `help <command>` for in-game usage.

| Command | Behavior |
| --- | --- |
| `maps [filter]` | Lists original maps and separately identified world areas. Filter by case-insensitive name or map ID. |
| `coc <map-or-area> [X Y]` | Loads an original map using its in-game map-list entry, or a named world area using its `GameArea.mes` coordinates on the overworld. An exact map name or ID has priority. |
| `pos` | Shows the current map, player tile, facing, sector, atlas position where available, terrain source, and collision state. |
| `teleport X Y` | Relocates the player within the active map through the same validated transition path. |
| `tcl` | Toggles collision for mouse routes and developer arrivals. **F6** invokes the same runtime action. |
| `inspect [X Y]` | Reports the selected object, or available raster and rejection evidence at a loaded tile. A tile with one supported door also selects that door. |
| `unlock` | Overrides an ordinary lock for this session. Use the door to open it and update passability. Scripted, jammed, and magically restricted doors retain their restrictions. |
| `settime <0..23>` | Sets the current game hour and updates HUD time and ambient lighting. The clock continues advancing. |
| `discoverall` | Reveals original destinations through the same discovery state as `--discover-all`. Normal travel eligibility still applies. |
| `reloadui` | Validates and reloads the selected HUD, available map theme, and console theme together. A failed reload keeps the working layouts. |
| `setuiscale <50..300>` | Updates UI scale in percent independently of camera zoom. |
| `showcollision` | Shows terrain blocks in red, occupied tiles in orange, wall and door edges in amber, and unavailable collision in violet. |
| `showsectors` | Shows original 64×64 sector boundaries in cyan. Use `pos` for the sector ID. |
| `fps` | Shows the latest presentation cadence, host frame time, frame count, and streaming statistics. Uses normal frame submissions. |
| `help [command]`, `clear` | Shows command usage or clears the output. |

## Player testing commands

Enable developer mode and press **F10**. With local RPC enabled, the same commands accept the same arguments, for example `arcanum-rs rpc SOCKET player additem basic:8132 2`. Use `help player` to list the group and `help player additem` for individual usage. Quote names containing spaces.

| Command | Effect |
| --- | --- |
| `player additem PROTOTYPE_ID [QUANTITY]` | Grant units to the backpack. Plain numbers and `serialized:N` use the numeric ID in the PRO header. `basic:N` uses the original script/schematic numbering. For example, `basic:8132` and `8152` both create a Copper Ring. The default quantity is one. Ammunition and currency use stack units. Other items receive individual identities. The entire grant must fit. |
| `player addgold AMOUNT` | Credit actual backpack gold. Merchant sales and dialogue refunds use the same credit operation. Purchases and dialogue payments debit those stacks. The maximum balance is 2,147,483,647. |
| `player addxp AMOUNT` | Award experience through the installed XP table, including character modifiers, level rewards and the experience ceiling. |
| `player addpoints AMOUNT` | Add unspent character points. The grant must leave at most 56 points. |
| `player setstat STAT VALUE` | Set a base attribute by installed name, index 0–7 or abbreviation ST, DX, CN, BE, IN, PE, WP, CH. Primary attributes range from 1 to the racial maximum. Alignment (19), fate points (20) and age (25) also accept their original ranges. Equipment, background and temporary effects apply to the base value. |
| `player setskill SKILL RANK` | Set investment from 0 to 5 by name or index 0–15. Training is preserved. Governing attributes and modifiers determine the effective level. |
| `player learnspell SPELL` | Learn a spell by installed name or index 0–79, including earlier spells in its college. |
| `player learnspells all` | Learn all 80 original spells. |
| `player learntech DEGREE`, `player learntech all` | Learn a schematic by installed title or recipe ID (for example `2410`), including earlier degrees in its discipline. Use `all` for all 56 degrees. |
| `tgm` | Toggle player-only God Mode. Inspect `combat.god_mode` through RPC. |

Learning grants bypass study prerequisites and cost no character points. Repeating a grant preserves the learned rank and aptitude. Normal cast eligibility, spell execution support, component requirements and expertise still apply. Use `progression` to inspect IDs and requirements and `inventory` or `itemowner` to inspect the resulting possessions. Save normally to keep grants.

God Mode prevents additional player health, fatigue and poison damage. Casting and upkeep consume zero fatigue and item mana, and player combat actions spend zero AP and ammunition. Normal turn order, manual end turn, existing injuries, consumables, ingredients and prices still apply. God Mode follows the player across map transitions and switches off on save **load**, new game or process restart.

## Collision and map loading

Try `coc Tulla 28447 31171`, a walkable tile beside the original palace wall
at `(28446,31171)`. With collision on, that edge blocks direct movement.
With `tcl` on, move across the wall or click the ground on its other side.
Mouse clicks issue movement commands during bypass. Automatic jump points
are suspended while it is active. Return to clear ground before restoring
collision. If you are stuck in blocked geometry, leave bypass enabled and
walk or use `teleport` to reach a clear tile, then toggle it off.

Developer map changes preserve the bypass flag. Ordinary gameplay
transitions reset it. Sector bounds and loaded terrain are always checked.
The usual click-to-move range and its loaded-sector limit still apply.
`coc "Tulla Interior"` selects map 5031. `coc Tulla` selects the original
world-area arrival on map 5000. `coc "Vendigroth Ruins"` selects the
overworld arrival. `coc "Vendigroth Underground"` selects the corresponding
interior map.

Selection is scoped to the active map. Object inspection can show detailed
prototype, serialized record and rendering information for prepared assets.
Failed raster and ART diagnostics are available by tile where the original
record provided a usable coordinate. Inspection cannot recover unknown
record fields or sprites the engine cannot load.

## Console theme

Copy [the default theme](../ui/developer-console.toml) into a package
directory and supply `--debug --console-ui /path/to/package/console.toml`.
The `keys` array accepts `Backquote`, `F8` through `F12`, or a combination.
Panel width and height are viewport percentages. `anchor = [0, 0]` is upper
left and `[100, 100]` is lower right within the remaining space. Colors are
RGBA arrays. Use `font = "bitmap"` or an original-format ART font through
`vfs:` or a package-relative `file:` reference. `font_size` scales with
the independent UI scale. The existing ART font path covers the western
character set. Input and history retain Unicode text.

With the game open, edit the selected files and issue `reloadui`. The
console retains its session input and history. Map discovery, gameplay and
camera state remain in their existing runtime objects. Use `--ui off` to
run without a gameplay HUD and keep the console available in debug mode.

See [machine-readable diagnostics](MACHINE_DIAGNOSTICS.md) for local RPC, snapshots, Vulkan captures and headless compatibility scans.
