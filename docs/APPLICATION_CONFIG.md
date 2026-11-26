# Application configuration

Build with `cargo build --locked --release --features windowed`. Copy the
[example configuration](config.example.toml) to
`$XDG_CONFIG_HOME/arcanum-rs/config.toml`, or to
`~/.config/arcanum-rs/config.toml` when `XDG_CONFIG_HOME` is empty, unset or
relative. Set `game_data` to the directory containing the installed Arcanum
archives and campaign data. Launch the executable directly. Startup uses the
game's authored Crash Site start unless the `play` command selects another
location.

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
| `ui` | `hud`, `map`, `pause`, `scale` | `--ui classic\|off\|PATH`, `--map-ui PATH`, `--ui-scale PERCENT` |
| `camera` | `zoom`, `follow` | `--zoom 1\|2\|3`, `--follow`, `--no-follow` |
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

## Changes during play

The engine saves changed preferences on clean exit. This includes shared UI
scale from the HUD, inventory, pause menu or maps, camera zoom, lighting, shadows and the
default movement gait. The saved UI value is the requested percentage, so a
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
