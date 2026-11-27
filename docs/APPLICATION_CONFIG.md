# Application configuration

On Linux, the windowed build requires ALSA development headers (`libasound2-dev` on Debian and Ubuntu, `alsa-lib` on Arch). Build with `cargo build --locked --release --features windowed`. Copy the
[example configuration](config.example.toml) to
`$XDG_CONFIG_HOME/arcanum-rs/config.toml`, or to
`~/.config/arcanum-rs/config.toml` when `XDG_CONFIG_HOME` is empty, unset or
relative. Set `game_data` to the directory containing the installed Arcanum
archives and campaign data. Launch the executable directly. Ordinary startup opens the Classic main menu. **Single Player → New Game** runs character creation and the starting merchant, then enters the campaign’s authored start. **Load Game** opens the slot list and **Last Save** resumes the newest native slot. The explicit map and center overrides remain available for development.

## Settings and precedence

Defaults are applied first, followed by configuration and then explicit CLI
arguments. `arcanum-rs play` uses the configured installation, and
`arcanum-rs play /path/to/Arcanum` overrides it for that invocation. The latter
also works before the config file has been created. Use `play --map MAP` and
`--center X Y` for development starts. Diagnostic subcommands keep their own
arguments and work independently of the play configuration.

| Section | Settings | CLI overrides for `play` |
| --- | --- | --- |
| Root | `game_data` | Positional installation directory |
| `window` | `width`, `height` in physical pixels, `fullscreen` (borderless) | `--window-size WIDTH HEIGHT`, `--fullscreen`, `--windowed` |
| `display` | `lighting`, `shadows` | `--lighting-on`, `--lighting-off`, `--shadows`, `--no-shadows` |
| `audio` | `effects`, `voice`, `music` (0–10, with 0 muted) | |
| `ui` | `hud`, `map`, `pause`, `scale` | `--ui classic\|off\|PATH`, `--map-ui PATH`, `--ui-scale PERCENT` |
| `camera` | `zoom`, `follow`, `edge_scroll`, `keyboard_pan`, `keyboard_scroll_speed`, `edge_scroll_speed`, `recenter_key` | `--zoom 1\|2\|3`, `--follow`, `--no-follow` |
| `input` | `run` | `--run`, `--walk-by-default` |
| `developer` | `enabled`, `rpc`, `diagnostics`, `console_ui`, `map_reveal`, `discover_all` | `--debug`, `--no-debug`, `--rpc SOCKET`, `--no-rpc`, `--diagnostics DIRECTORY`, `--console-ui PATH`, `--map-reveal`, `--no-map-reveal`, `--discover-all`, `--no-discover-all` |
| `mods` | `directory`, `enabled` | `--no-mods` or `--mods DIRECTORY ID …` at the end |

`--lighting HOUR` continues to set the gameplay starting hour. `--sectors`
continues to use its explicit viewport dimensions. Out-of-range zoom, scale and
window sizes report an error.

Paths in TOML are relative to the config file's directory. Explicit CLI paths
are relative to the invocation directory. The engine searches for the default
manifests in `ui/` beside the executable, then uses the bundled manifest text.
For example, placing `ui/classic/theme.toml` beside the executable overrides the
default HUD package. External packages use the same loaders, with `file:` assets
resolved within the package and original artwork read from the installed game.
See [UI themes](UI_THEMES.md). Older M63 files use `ui.hud_scale` as a fallback
when `ui.scale` is absent.

The default camera stays in place during movement. WASD, arrow keys and screen edges pan. Home centers once. F5 toggles continuous tracking. Set `camera.recenter_key` to a physical key code such as `"Home"` or `"End"` to change the binding.

Set `keyboard_scroll_speed` and `edge_scroll_speed` independently in `[camera]` and restart. Both accept whole numbers from 1 to 2400 and default to 300 screen pixels per second. For example, 900 crosses a 900-pixel distance in one second of uninterrupted scrolling at any world zoom. Shift multiplies either speed by 3⅓, up to 4000 screen pixels per second. Opposite directions cancel. Diagonals and overlapping inputs stay within the faster active speed. Set `keyboard_pan` or `edge_scroll` to false to disable that input. Sector loading suspends panning. A delayed update advances at most 250 ms of movement.

## Changes during play

The engine saves changed preferences on clean exit. This includes shared UI
scale from the HUD, inventory, pause menu or maps, camera zoom and follow mode, lighting, shadows and the
default movement gait. **Escape → Options** opens the Classic Game, Video
and Sound categories. **Done** saves changed preferences and returns to the pause
menu. Sound controls take effect immediately. Effects also adjusts the ambient
channel. Voice preferences apply independently of music and effects. The saved UI value is the requested percentage, so a
temporary viewport constraint does not reduce the saved preference. Window
resizing affects the current session. CLI overrides and diagnostic snapshot
restoration are session-local. If you subsequently change an overridden value
through a game control, the new value becomes a preference.

The engine reads the latest file before merging just the changed settings and
replaces it atomically. Other TOML values are preserved. Comments and formatting
may be rewritten when settings are saved.

## Developer mode and RPC

Set `developer.enabled = true` to enable the console. Press **F10** or the
backquote key to open it with the default console theme. A configured socket
requires both `developer.enabled = true` and `developer.rpc = true`. Each
instance creates a private directory under `$XDG_RUNTIME_DIR` when available,
or under `/tmp`, and prints the socket path to standard error. Use
`arcanum-rs rpc SOCKET pos` with that path. A clean exit removes both the
automatic socket and its directory. `--rpc SOCKET` selects an explicit path
in an existing private directory for automation. Developer options are disabled
by default. [Diagnostics protocol](MACHINE_DIAGNOSTICS.md)

Classic Game Options sets the default combat mode with two switches. Both default to false.

| Default Combat Mode | `gameplay.turn_based` | `gameplay.fast_turn_based` |
| --- | --- | --- |
| Real Time | false | false |
| Turn Based | true | false |
| Fast Turn Based | true | true |

Space toggles only `turn_based`. Switching from fast combat to real time this way retains the fast preference for the next toggle. Loading a slot restores its saved mode independently of the new-game default.

Game Options also writes `gameplay.difficulty` (`easy`, `normal` or `hard`). Normal is CE’s default difficulty. Difficulty applies immediately and carries over when loading a slot. On Easy, the local player's attack effectiveness and final HP damage increase by half. On Hard they decrease by a quarter. The damage adjustment follows armor and resistance.
