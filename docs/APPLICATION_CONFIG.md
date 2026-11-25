# Launch and application settings

Build once, create the configuration directory and open the configuration file in your editor.

```sh
cargo build --locked --release --features windowed
config_dir="${XDG_CONFIG_HOME:-$HOME/.config}/arcanum-rs"
mkdir -p "$config_dir"
${EDITOR:-vi} "$config_dir/config.toml"
```

For an installation at `~/Work/arcanum-ce-test`, use an absolute path with your own home directory:

```toml
game_data = "/home/your-user/Work/arcanum-ce-test"

[display]
width = 1280
height = 720
fullscreen = false

[camera]
zoom = 1
follow = false
edge_scroll = true
speed = 1200

[ui]
hud_scale = 125
inventory_scale = 125
map_scale = 100
```

Then run `./target/release/arcanum-rs`. If deploying outside the repository, copy the executable and the bundled `ui/` directory next to each other, or put the packages under `share/arcanum-rs/ui/` relative to the installation prefix. Run `./arcanum-rs` from that deployment directory. Resource lookup starts from the executable, so the working directory can differ from the deployment directory. A normal launch starts at the original Crash Site.

## Precedence and supported settings

Defaults are applied first, followed by `$XDG_CONFIG_HOME/arcanum-rs/config.toml` (normally `~/.config/arcanum-rs/config.toml`) and explicit `play GAME [OPTIONS]` arguments. The existing diagnostic subcommands keep their explicit inputs. Config-relative paths resolve beside the configuration file. Use absolute paths for files elsewhere and expand shell variables in the shell before writing them to TOML.

| Field | Values |
|---|---|
| `game_data` | Directory containing the installed original data |
| `mods_directory`, `mods` | Existing mods directory and nonempty array of requested package IDs |
| `display.width`, `display.height`, `display.fullscreen` | Optional paired physical extents from 200 to 4096, and borderless fullscreen |
| `display.lighting`, `display.shadows`, `display.run` | Booleans controlling lighting, shadows and the default gait |
| `camera.zoom`, `camera.follow`, `camera.edge_scroll` | Zoom 1–3 and booleans for tracking and edge scrolling |
| `camera.speed` | 40–2400 horizontal projected pixels per second, default 1200. Vertical speed is half the horizontal speed. |
| `camera.recenter`, `camera.follow_toggle` | Distinct bindings chosen from Home and F7–F12 |
| `ui.hud`, `ui.inventory`, `ui.map`, `ui.console` | Independent packages. HUD also accepts `classic` or `off`. Inventory expects a directory and console takes a manifest |
| `ui.hud_scale`, `ui.inventory_scale`, `ui.map_scale`, `ui.console_scale`, `ui.menu_scale` | Percentages from 50 to 300. HUD and inventory share one gameplay scale. Map, console and menu scale independently. |
| `developer.enabled`, `developer.rpc`, `developer.diagnostics` | Enable development controls, automatic local RPC and optional existing capture directory |
| `developer.map`, `developer.center` | Explicit development-only map ID or name and optional `[x, y]` |
| `developer.map_reveal`, `developer.discover_all` | Existing map discovery overrides |

Examples of per-launch overrides include `--ui off`, `--ui-scale 150`, `--map 5011 --center 80 103`, `--window-size 1280 720`, `--fullscreen`, `--windowed`, `--zoom 2`, `--follow`, `--no-follow`, `--edge-scroll`, `--no-edge-scroll` and `--pan-speed 450`. `--ui-scale` overrides the themed interface groups for that launch. The base module is still resolved by the existing VFS.

## Controls and persistence

- **Alt + arrows** pan independently of the player. Move the pointer into the eight-pixel edge band to scroll, including over the gameplay HUD.
- **Home** recenters on the player. **F7** toggles tracking. Map arrival centers on the player. While tracking is off, entering a new tile beyond the viewport's recenter limits brings the moving player back into view. This generalizes the movement recenter rule in CE's `anim.c` to the active viewport. The edge band comes from `handle_mouse_scroll` in CE's `main.c`.
- **[** and **]** scale both HUD panels and inventory together during gameplay, including when inventory is open. The viewport limits the shared scale so the inventory fits between the top and bottom HUD. The map, console and pause menu keep separate scales. **Wheel** over the world changes gameplay zoom, with **0** to reset it.
- In-game changes to tracking, UI scale, zoom, or the existing lighting, shadows and gait options save the changed settings. The file is replaced atomically and retains unrelated values. Comments may be reformatted or lost on write. CLI and RPC overrides alone do not persist.

## Developer RPC

For configured playtesting, add the following section and launch without arguments. Clean shutdown removes the socket and its private directory. Existing explicit `--rpc SOCKET` workflows still work.

```toml
[developer]
enabled = true
rpc = true
diagnostics = "captures"
```

Create the optional capture directory beside your configuration first, or omit `diagnostics`. The process prints its active socket path at startup. Automatic sockets use a unique private directory under the XDG runtime directory when available, or the temporary directory otherwise.

```sh
./target/release/arcanum-rs rpc "$socket" pos
./target/release/arcanum-rs rpc "$socket" setuiscale 150 inventory
./target/release/arcanum-rs rpc "$socket" camerafollow off
./target/release/arcanum-rs rpc "$socket" centercamera
./target/release/arcanum-rs rpc "$socket" quit
```

`pos` reports the configuration path, effective display state, RPC socket, active interface, per-interface scales and camera state. The character preview in inventory uses the playable sprite. Equipment, character statistics and gameplay-dependent screens remain future work.
