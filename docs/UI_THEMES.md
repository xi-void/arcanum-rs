# Gameplay HUD themes

The playable alpha loads one HUD package at startup. Classic and Field Notes
are both TOML packages. They use the same component types, actions, asset
loader and hit testing. The Escape menu is a separate modal interface and
remains available with or without the HUD.

```sh
# Classic, using installed Arcanum UI artwork.
cargo run --locked --release --features windowed,gpu -- \
  play "$HOME/Work/arcanum-ce-test" --ui classic --renderer vulkan

# Replacement package. The path can also name the manifest itself.
target/release/arcanum-rs play "$HOME/Work/arcanum-ce-test" \
  --ui ui/field-notes --ui-scale 175 --renderer cpu

# World controls and Escape menu, with no HUD instance.
target/release/arcanum-rs play "$HOME/Work/arcanum-ce-test" --ui off
```

Omit `--ui` for Classic and omit `--renderer` for the normal renderer
default. OpenGL uses `--renderer gles` with the GPU feature. Press **[** or
**]** to adjust UI scale by 25 percentage points during play. Use **Tab**
or **Shift+Tab** to move between available HUD actions and **Enter** or
**Space** to activate the focused action. The mouse can activate the same
actions. World zoom is controlled separately by the wheel and **0**.
Theme and scale choices can be specified at each launch. Main-menu
theming and runtime package switching are future work.

## Package format

A package directory contains `theme.toml` and any optional ART resources.
The manifest uses UTF-8 TOML. These keys form the public declarative
interface.

| Section | Keys |
| --- | --- |
| Package | `name` is required. `author` is optional. `scale` is the initial UI percentage, from 50 to 300, and defaults to 100. `--ui-scale` overrides it. |
| `[fonts.<profile>]` | `source`, `size` in base pixels, and `color` as RGBA bytes are required. `bold` adds a one-pixel stroke. `line_gap` adds pixels between lines. `fallback` optionally names another profile's glyph source. |
| `[[hud]]` layout | `id` must be unique. `size = [width, height]` is required. `offset = [x, y]` defaults to zero. `parent` can name a preceding component. `anchor` is `top-left` (default), `top`, `top-right`, `left`, `center`, `right`, `bottom-left`, `bottom` or `bottom-right`. |
| `[[hud]]` appearance | `fill` and `hover_fill` are RGBA byte arrays. `image` is an ART reference with an optional zero-based `frame`. Use `source_rect = [x, y, width, height]` to crop within that frame before scaling. A label uses `text`, a named `font`, optional `align` of `left`, `center` or `right`, and optional `wrap = true`. |
| `[[hud]]` behavior | `action` can be `pause`, `toggle-run` or `reset-zoom`. `capture = true` reserves the component bounds for UI clicks and scrolling. An action reserves its own bounds. `disabled = true` makes an action inactive. `hidden = true` removes the component and its children from layout, drawing and input. |

Declaration order is back to front. The matching points on the component
and parent define the anchor. The viewport serves as the parent of roots.
Children are clipped to their parent's visible area. An action takes
precedence over controls behind it. Passive labels and art pass pointer
input through unless configured to capture it. The same rectangle is used
for scissoring and for pointer hits. Provide `hover_fill` if the action
should show hover, keyboard focus or active-run feedback.

Images use nearest-neighbor scaling. Text samples each glyph directly at its final physical size, retaining the font’s palette coverage.
Artwork, text and hit regions share the same UI scale. If the widest root
would exceed the window, the effective scale is reduced to fit.
This works in physical window pixels on all backends, independently of
the camera. Layout changes on resize or when the user adjusts UI scale.

### Assets and fonts

- `vfs:art/interface/...` reads the winning asset in the installed game
  VFS, following engine archive/loose-file precedence and selected
  `--mods` overrides.
- `file:art/...` resolves within the selected theme directory, using the
  bounded loose-file VFS. Files in a theme do not implicitly replace VFS
  resources. No packages are merged, and the active package owns the HUD
  layout in its entirety.
- `bitmap` uses a distributable engine 5×7 alphabet. Its current glyph
  set is A–Z, 0–9, space and the punctuation `.:-/?`, with lowercase
  folded to uppercase.
- ART fonts use one glyph per frame, with the original byte code minus
  31 as the frame number. `file:` can point to independently authored
  ART fonts in the same format. Source palette red supplies coverage for
  non-key pixels, and `hot_x` is the horizontal advance. The stored space
  height supplies the line metric when present. Text uses the configured
  color and size, with cumulative glyph positions rounded to physical pixels. Wrapping and clipping are supported. Platform font
  discovery, OpenType shaping, font scrolling and typographic features
  beyond ART spacing and the simple bitmap profile are outside this
  foundation.

Unknown fields, invalid parents, bounds and missing referenced assets
produce startup errors. If a font has an explicit fallback, a failed glyph
lookup tries that source once. No unrelated art is selected automatically.
The example uses only procedural colors and a bitmap profile. Change its
`toolbar` anchor and `fonts.controls.size` in a copy of the package, then
launch with the existing executable and `--ui path/to/copy`.

## Original interface evidence and architecture

CE resolves interface names through `art/interface/interface.mes`.
`src/ui/intgame.c` uses entries 185 and 184 for the 800×41 top and
800×159 bottom panels and anchors them in the center when there is extra
horizontal space. In the tested installation these entries resolve to
`IntTop.ART` from `Arcanum1.dat` and `IntBotom.ART` from
`ArcanumZHighResBorders.dat` with CE precedence. The Classic manifest uses
the same panel resources and proportions. The manifest places the primary
buttons at x = 4, 41, 78 and 115 with y = 2 in the top panel. In
`intgame_button_create_ex`, CE converts the authored position to
panel-local coordinates. TIG `button.c` then draws frame 0 at that
top-left with its actual 34×34 bounds. The world-sprite hotspot (17,33)
is not a displacement in this UI draw path. Secondary controls, fate
and rest backgrounds, and the hotkey strip switch use their own
native bounds and original positions.

CE `hotkey_ui.c` uses x = 198, 237, 276, 315, 354, 418, 456, 495, 534
and 573 for the slots. `intgame_hotkey_refresh` draws the 32×32
`HKSlt1.ART` background at local y = 4 for every unassigned slot.
The message window uses `DialogueWindow.ART` at (196,51) in the
bottom panel. CE's bar draw reads `clrvial.art` for the empty region
and crops its upper-left 28×88 pixels. Classic shows that neutral
glass with no health or fatigue value assigned.
The installed `redvial.art`, `bluvial.art`, and `yelovial.art` supply
state-dependent fills or poison coloration. CE uses `Cloister18Font.ART`
(entry 171) for numeric counters. These assets are present in the game
data and await gameplay bindings.

CE's gameplay font `morph15font.art` (entry 27) stores 19-pixel-high
glyphs with varying advances and a grayscale edge palette.
TIG `font.c` uses the byte-minus-31 frame and stored horizontal advance.
Its source-alpha text blit in `art.c` uses `tig_color_alpha`, which
returns palette red in `color.h`. The HUD preserves that coverage and
samples the source directly at the requested physical glyph size. The
previous renderer discarded fractional coverage and resized text first
to its logical height and then again to the HUD scale.

Before this change, gameplay's only UI was a procedurally painted pause
and options sprite. Its controls and bitmap font were local to the
viewer, and its layout followed world zoom. The pause menu remains on
its existing path, which supplies the existing gameplay settings
actions. The HUD reuses the ART loader, VFS precedence, compositor
and bitmap alphabet. Gameplay owns settings and routes. The HUD reads
run preference for feedback and emits typed actions. Layout,
coordinates and artwork live in the package. Both CPU and GPU draw the
same unlit overlay after the world pass.

Classic’s map icon opens the atlas. Its other original buttons and
action slots are decorative. Alpha controls remain on the keyboard and in
the Escape menu. Health, fatigue, poison, currency, fate values,
party portraits, and recent-action or item overlays require
gameplay state that the HUD does not expose. The original day/night
clock (entries 207–215) samples the game calendar and is also deferred.
The Classic status display contains no fabricated numbers, colored
fills, or hotbar assignments. Inventory, character, maps and dialogue remain
independent future interfaces. The HUD foundation currently covers
panels, labels, buttons, layout and focus. Tooltips, general theme
modals and arbitrary gameplay-state binding remain future work.

The atlas is an independent package. See [Map UI](MAP_UI.md) for its
data bindings, map-specific actions and the `--map-ui` option.

Image-backed actions can use zero-based `hover_frame`, `pressed_frame`
and `selected_frame` from their existing `image`. Frame artwork takes
precedence over a configured hover fill. The Classic top panel uses
the original empty spell-slot artwork and a fixed daytime crop of
`Clk_timestrip.ART`. Clock simulation remains deferred.
