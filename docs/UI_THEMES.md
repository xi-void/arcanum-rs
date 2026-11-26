# Gameplay HUD themes

The playable alpha loads one HUD package at startup. Classic and Field Notes
are both TOML packages. They use the same component types, actions, asset
loader and hit testing. The Escape menu is a separate modal interface and
remains available with or without the HUD.

```sh
# Classic, using installed Arcanum UI artwork.
cargo run --locked --release --features windowed -- \
  play "$HOME/Work/arcanum-ce-test" --ui classic

# Replacement package. The path can also name the manifest itself.
target/release/arcanum-rs play "$HOME/Work/arcanum-ce-test" \
  --ui ui/field-notes --ui-scale 175

# World controls and Escape menu, with no HUD instance.
target/release/arcanum-rs play "$HOME/Work/arcanum-ce-test" --ui off
```

Omit `--ui` for Classic. The windowed build presents through Vulkan. Press **[** or
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
| `[fonts.<profile>]` | `source` and `color` as RGBA bytes are required. Omit `size` for the native ART height, or specify a height of 5–64 base pixels. `bold` adds a one-pixel stroke. `line_gap` adds pixels between lines. `fallback` optionally names another profile's glyph source. |
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

Vulkan scales native-size component textures, including labels with the
font's palette coverage. Resizing and adjusting the scale update placement
and clipping while retaining the component textures. The top HUD, active
panel and bottom HUD share an effective scale in physical window pixels.
The viewport fits the full layout even while the panel is closed, preserving the requested percentage.
Camera zoom is independent.

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
- ART fonts use the Windows-1252 byte minus 31 as the frame index.
  Source palette red supplies coverage for non-key pixels. Frame width
  and height determine the drawable rectangle, and `hot_x` advances the
  pen. The space frame supplies the native line-height minimum. A taller
  glyph increases the height of its line. `line_gap` adds base pixels
  between lines. Cumulative advances are rounded only when placing a
  glyph. Explicit reductions average source coverage so fine strokes
  survive rasterization. The Vulkan compositor scales the prepared labels
  with the rest of the interface.
- Classic uses native Flare12 (15-pixel frames) for item details,
  inventory summaries, and HUD context, Cloister18 (18 pixels) for
  counters, and Flare14 (16 pixels) for dialogue. The atlas uses NewTimes16
  (21 pixels). Asset names identify the faces and their nominal sizes.
  The developer console uses Courier10 at 17 pixels in fixed-width cells
  sized for the chosen face. Platform font discovery and OpenType shaping
  are outside the ART format.

Unknown fields, invalid parents, bounds and missing referenced assets
produce startup errors. If a font has an explicit fallback, a failed glyph
lookup tries that source once. No unrelated art is selected automatically.
The example uses only procedural colors and a bitmap profile. Change its
`toolbar` anchor and `fonts.controls.size` in a copy of the package, then
launch with the existing executable and `--ui path/to/copy`.

## Inventory and containers

Click the Classic bag button or press **I** to open or close the inventory.
Click an accessible original container in the world to approach and open its
loot panel. The mouse wheel and arrow buttons scroll the container grid.
Point to an item to see its footprint and name in the HUD. Drag items within
or between the grids, or right-click an item in the loot panel to transfer
it to the opposite grid's first available space. **Take All** moves items
that fit in the backpack. **Arrange** packs open grids by footprint,
alternating between rows and columns.

Drag a backpack item to the Drop box to place it at the player's feet.
Click a ground item to approach and pick it up. **Escape** cancels the
active drag or quantity entry, then closes the panel on a second press.
Drag a backpack item onto a compatible equipment slot to wear it. Drag
equipped gear back onto a free backpack cell or right-click it to remove it.
Replacement requires room in the backpack for the displaced item.

Dragging a multi-unit stack between owners or to Drop prompts for a quantity.
Type the amount and press **Enter**. Right-click in the loot panel to move
the entire stack. Hover over a stack to view its quantity.
The Use action remains inactive.

A package can opt in with a center-anchored root using
`inventory_panel = true`, zero offset, and `capture = true`. The root is
centered in the space between top and bottom HUD roots. Use
`inventory_views = ["inventory"]` or `["container"]` to select the modes
where a component is placed. Add `action = "toggle-inventory"` to a HUD button.

- Use `binding = "backpack"` or `"container-grid"` with `cell` in theme
  pixels. The width must match the runtime's ten columns. Grid height
  determines visible rows. `hover_fill` supplies the item footprint tint.
  Set `valid_drop_fill` and `invalid_drop_fill` to customize placement feedback.
- Use `inventory-arrange` and `inventory-take-all` actions for Arrange and
  Take All. Use `binding = "inventory-drop"` for the Drop target.
- Use `binding = "inventory-lens"` below the inset border ART. This
  component receives the live scene from the shared Vulkan compositor.
- Use `binding = "inventory-hover"` with a font for item names and
  optional component `hover_message` values. This bottom HUD label also
  displays hovered atlas location descriptions while the map is open.
- Place `inventory-scroll-up` and `inventory-scroll-down` actions next to a
  `container-thumb` binding covering the track. Bind the thumb's ART and
  top cap height with `image` and `thumb_height`. Optional `thumb_middle`
  and `thumb_bottom` add the other two ART pieces.

Classic follows CE's `src/ui/inven_ui.c`. Interface MES entries 221, 223
and 234 supply the backpack, paper-doll, and loot backgrounds. The backpack
starts at (368, 8), and the loot grid starts at (8, 136) with eight visible
rows. Both use 32-pixel cells. Entries 232 and 233 supply the lens borders.
CE’s `intgame.c` samples a 178×178 world area into the 89×89 lens. The nine equipment regions use the original cover ART. Item icons and
footprints come from the same original inventory ART IDs and authoritative
slots used by runtime inventory.

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

Installed `interface.mes` maps entries 327, 27, 229, 230 and 171 to
Morph30, Morph15, Flare12, Flare14 and Cloister18. CE's
`src/ui/mainmenu_ui.c` initializes the Morph menu fonts, and
`src/game/tb.c` and `tc.c` use Flare12 for speech and responses. Classic
uses the slightly larger Flare14 face for those two roles, with shadow
and separate response colors for normal, hovered and disabled states.
TIG `font.c` distinguishes the glyph rectangle from the horizontal
advance. Its source-alpha blit in `art.c` reads palette red through
`tig_color_alpha` in `color.h`.

The HUD, atlas and Classic menus use the same glyph measurement and
rasterizer. Vulkan draws the prepared UI after the world pass. Theme
packages own the layout and assets. Gameplay owns settings and actions.

Classic’s map icon opens the atlas, the bag opens the inventory, and the
logbook opens Quests. The other original buttons and action slots are decorative. Alpha controls remain on the keyboard and in
the Escape menu. Health, fatigue, poison, currency, fate values,
party portraits, and recent-action or item overlays require
gameplay state that the HUD does not expose. The original day/night
clock (entries 207–215) samples the game calendar and is also deferred.
The Classic status display contains no fabricated numbers, colored
fills, or hotbar assignments. The inventory paper doll has a character
summary, and inventory inspection uses the bottom HUD for item details.
The standalone character sheet and document reader have no active screens.

Set `journal_panel = true` on a centered gameplay-panel root to add the quest
spread. Use `toggle-journal` on the HUD button, `journal-quests` for the selected
tab, and `journal-previous` and `journal-next` for the page arrows. Bind
`journal-left` and `journal-right` to the page bodies and give them equal size
and the same font. Use the corresponding `-header` and `-footer` bindings for
titles and page numbers. The optional `journal-lens` displays the player inset.
The `[journal]` table defines the `active`, `completed`, and `failed` RGBA text
colors. The body font supplies the default text color. Classic uses the
original artwork, type, and book geometry. The text columns leave clearance
for the original page-turn arrows. [Quest logbook](QUESTS.md)

The atlas is an independent package. See [Map UI](MAP_UI.md) for its
data bindings, map-specific actions and the `--map-ui` option.

Image-backed actions can use zero-based `hover_frame`, `pressed_frame`
and `selected_frame` from their existing `image`. Frame artwork takes
precedence over a configured hover fill. The Classic top panel uses
the original empty spell-slot artwork and a fixed daytime crop of
`Clk_timestrip.ART`. Clock simulation remains deferred.


Bind an equipment component with `equipment_slot` set to `helmet`,
`ring1`, `ring2`, `medallion`, `weapon`, `shield`, `armor`,
`gauntlet` or `boots`. The component's image supplies the empty-slot
artwork and its rectangle supplies the icon placement and hit area.
Text bindings for the existing inventory counters are `inventory-gold`,
`inventory-arrows`, `inventory-bullets`, `inventory-charges` and
`inventory-fuel`. Paper-doll summary labels support `inventory-weight`,
`inventory-burden`, `inventory-speed` and `inventory-defense`.


## Pause packages

The default package is `ui/classic-pause/theme.toml`. To replace it, set
`ui.pause` in `config.toml` to a package directory or manifest path. The
manifest defines the authored canvas, ART resources, fonts, original string
table, control placement and clipping rectangles. ART sources use the same
`vfs:` and package-relative `file:` prefixes as HUD assets. The menu uses
the shared requested UI scale and fits inside the physical viewport, with
identical coordinates for input and Vulkan composition. Gameplay and slot
operations are independent of the package. `font` and `menu_font` take
native glyph metrics by default. Set `font_size` or `menu_font_size` to
specify a different base height. Save-list row height is at least the
font's line height, and only complete rows participate in slot selection.
The save preview wraps the full slot name, and the editor keeps the end of
the name visible in the list.

The pause package's optional `[options]` table supplies the Options
background, lens, cyclic controls, arrow and gauge ART, colors and
positions. Classic uses Morph30 (33 pixels) for navigation, Morph15
(19 pixels) for headings, and Flare12 for values. Use `heading_font`,
`value_font`, `heading_font_size`, and `value_font_size` to customize those
roles. Coordinates are in the background ART's canvas. Older
pause packages use the bundled Classic Options layout. Control actions use
the active gameplay settings and the shared pause-menu scale.

NPC dialogue uses HUD bindings for the speaker bubble, response rows, and
contextual information. See [NPC dialogue](DIALOGUE.md#theme-and-diagnostics).
