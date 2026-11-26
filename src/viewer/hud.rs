//! Declarative HUD layout, assets and hit testing, in physical window pixels.
mod inventory;
use super::ThemeSource;
use arcanum_rs::{
    art::{DecodedFrame, SpriteFrame},
    content::{
        Source,
        art::{LoadedFrame, load_frame},
    },
    gpu::OverlayDraw,
    vfs::{MountId, Policy, Storage, Vfs},
};
use inventory::{InventoryPanel, InventoryView};
use serde::Deserialize;
use std::{collections::BTreeMap, error::Error, sync::Arc};

type Color = [u8; 4];

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(super) enum Action {
    Pause,
    ToggleRun,
    ResetZoom,
    OpenMap,
    ToggleInventory,
    InventoryScrollUp,
    InventoryScrollDown,
    InventoryTakeAll,
    InventoryArrange,
    MapLocal,
    MapWorld,
    MapZoomIn,
    MapOverview,
    MapRecenter,
    MapTravel,
}

#[derive(Clone, Copy, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
enum Anchor {
    #[default]
    TopLeft,
    Top,
    TopRight,
    Left,
    Center,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
}

#[derive(Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Align {
    #[default]
    Left,
    Center,
    Right,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Theme {
    name: String,
    author: Option<String>,
    #[serde(default = "default_scale")]
    scale: u16,
    fit_viewport: Option<[u32; 2]>,
    route_color: Option<Color>,
    selected_route_color: Option<Color>,
    #[serde(default)]
    fonts: BTreeMap<String, Font>,
    #[serde(default)]
    hud: Vec<Element>,
}

fn default_scale() -> u16 {
    100
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Font {
    source: String,
    size: u32,
    color: Color,
    #[serde(default)]
    bold: bool,
    #[serde(default)]
    line_gap: u32,
    fallback: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum Binding {
    MapCanvas,
    PlayerLens,
    MapTitle,
    MapWest,
    MapSouth,
    MapStatus,
    PlaceLabel,
    PlaceMarker,
    SelectedMarker,
    ContinentPlaceMarker,
    ContinentPlayerMarker,
    ContinentSelectedMarker,
    PlayerMarker,
    WaypointMarker,
    GameClock,
    Backpack,
    ContainerGrid,
    ContainerThumb,
    InventoryLens,
    InventoryHover,
    NpcContext,
    DialogueText,
    DialogueResponse,
    InventoryGold,
    InventoryArrows,
    InventoryBullets,
    InventoryCharges,
    InventoryFuel,
    InventoryWeight,
    InventoryBurden,
    InventorySpeed,
    InventoryDefense,
    InventoryDrop,
}

impl Binding {
    fn is_label(self) -> bool {
        matches!(
            self,
            Self::MapTitle
                | Self::MapWest
                | Self::MapSouth
                | Self::MapStatus
                | Self::PlaceLabel
                | Self::NpcContext
                | Self::DialogueText
                | Self::DialogueResponse
                | Self::InventoryHover
                | Self::InventoryGold
                | Self::InventoryArrows
                | Self::InventoryBullets
                | Self::InventoryCharges
                | Self::InventoryFuel
                | Self::InventoryWeight
                | Self::InventoryBurden
                | Self::InventorySpeed
                | Self::InventoryDefense
        )
    }

    fn is_marker(self) -> bool {
        matches!(
            self,
            Self::PlaceLabel
                | Self::PlaceMarker
                | Self::SelectedMarker
                | Self::PlayerMarker
                | Self::WaypointMarker
                | Self::ContinentPlaceMarker
                | Self::ContinentPlayerMarker
                | Self::ContinentSelectedMarker
        )
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum MapView {
    Local,
    World,
    Continent,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Element {
    id: String,
    parent: Option<String>,
    #[serde(default)]
    anchor: Anchor,
    #[serde(default)]
    offset: [i32; 2],
    size: [u32; 2],
    image: Option<String>,
    binding: Option<Binding>,
    #[serde(default)]
    map_views: Vec<MapView>,
    travel_available: Option<bool>,
    #[serde(default)]
    inventory_panel: bool,
    #[serde(default)]
    dialogue_only: bool,
    response: Option<usize>,
    hover_text_color: Option<Color>,
    disabled_text_color: Option<Color>,
    #[serde(default)]
    inventory_views: Vec<InventoryView>,
    cell: Option<u32>,
    equipment_slot: Option<arcanum_rs::content::character::EquipmentSlot>,
    hover_message: Option<String>,
    thumb_height: Option<u32>,
    thumb_middle: Option<String>,
    thumb_bottom: Option<String>,
    clock_phases: Option<[String; 8]>,
    source_rect: Option<[u32; 4]>,
    #[serde(default)]
    frame: usize,
    fill: Option<Color>,
    hover_fill: Option<Color>,
    valid_drop_fill: Option<Color>,
    invalid_drop_fill: Option<Color>,
    hover_frame: Option<usize>,
    pressed_frame: Option<usize>,
    selected_frame: Option<usize>,
    text: Option<String>,
    font: Option<String>,
    text_shadow: Option<Color>,
    #[serde(default)]
    align: Align,
    #[serde(default)]
    wrap: bool,
    #[serde(default)]
    capture: bool,
    action: Option<Action>,
    #[serde(default)]
    disabled: bool,
    #[serde(default)]
    hidden: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

impl Rect {
    pub(super) fn contains(self, (x, y): (u32, u32)) -> bool {
        let (x, y) = (
            i64::from(x) - i64::from(self.x),
            i64::from(y) - i64::from(self.y),
        );
        x >= 0 && y >= 0 && x < i64::from(self.w) && y < i64::from(self.h)
    }
    fn intersect(self, other: Self) -> Option<Self> {
        let (left, top) = (self.x.max(other.x), self.y.max(other.y));
        let right = (self.x + self.w as i32).min(other.x + other.w as i32);
        let bottom = (self.y + self.h as i32).min(other.y + other.h as i32);
        (left < right && top < bottom).then(|| Self {
            x: left,
            y: top,
            w: (right - left) as u32,
            h: (bottom - top) as u32,
        })
    }
}

struct Component {
    config: Element,
    parent: Option<usize>,
    base: SpriteFrame,
    text: Option<Text>,
    label: Option<Label>,
    clock: Option<ClockArt>,
    art: Arc<LoadedFrame>,
    state_art: [Option<Arc<LoadedFrame>>; 3],
}

struct ClockArt {
    day: LoadedFrame,
    moon: Vec<LoadedFrame>,
    hours: Option<u64>,
}

impl ClockArt {
    fn paint(&self, dst: &mut SpriteFrame, hours: u64, fill: Color) {
        let day = &self.day.decoded.frame;
        let seconds = hours * 3600;
        let age = (seconds + 43200) / 84600 % 28;
        let phase = [0, 4, 9, 13, 14, 18, 23, 27].partition_point(|&end| end < age);
        let moon = &self.moon[phase].decoded.frame;
        let width = u64::from(day.width) + u64::from(moon.width);
        let center = ((seconds + 73800) % 86400) * width / 86400 + width;
        let start = (center - u64::from(dst.width / 2) % width) % width;
        for y in 0..dst.height {
            for x in 0..dst.width {
                let col = ((start + u64::from(x)) % width) as u32;
                let (image, sx) = if col < day.width {
                    (day, col)
                } else {
                    (moon, col - day.width)
                };
                let pixel = if y < image.height {
                    image.pixels[(y * image.width + sx) as usize]
                } else {
                    fill
                };
                dst.pixels[(y * dst.width + x) as usize] = if pixel[3] == 0 { fill } else { pixel };
            }
        }
    }
}

struct Text {
    glyphs: Vec<Option<(Arc<SpriteFrame>, char)>>,
    font: Font,
    native_height: u32,
}

struct Label {
    glyphs: BTreeMap<char, Arc<SpriteFrame>>,
    font: Font,
    native_height: u32,
    value: String,
    ink_height: u32,
}

impl Label {
    fn text(&self, value: &str) -> Text {
        let glyphs = value
            .chars()
            .take(4096)
            .map(|ch| {
                if ch == '\n' {
                    None
                } else {
                    Some((
                        Arc::clone(self.glyphs.get(&ch).unwrap_or(&self.glyphs[&'?'])),
                        ch,
                    ))
                }
            })
            .collect();
        Text {
            glyphs,
            font: self.font.clone(),
            native_height: self.native_height,
        }
    }
}

struct Placement {
    full: Rect,
    scale: u32,
    rect: Rect,
    live_asset: Option<Arc<LoadedFrame>>,
}

pub(crate) struct Hud {
    components: Vec<Component>,
    font_profiles: BTreeMap<String, serde_json::Value>,
    placed: Vec<Option<Placement>>,
    size: (u32, u32),
    scale: u16,
    fit_viewport: Option<[u32; 2]>,
    hover: Option<usize>,
    pressed: Option<usize>,
    route_color: Option<Color>,
    selected_route_color: Option<Color>,
    selected_action: Option<Action>,
    active_action: Option<Action>,
    unavailable_action: Option<Action>,
    focus: Option<usize>,
    map_view: Option<MapView>,
    travel_available: bool,
    effective_scale: u32,
    panel_band: Rect,
    external_panel: Option<[u32; 2]>,
    shared_band: Option<(Rect, u32)>,
    inventory: InventoryPanel,
    dialogue_open: bool,
    dialogue_available: [bool; 5],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Hit {
    World,
    Panel,
    Button(Action),
}

impl Hud {
    pub fn load(
        source: &ThemeSource,
        game: &mut Vfs,
        scale: Option<u16>,
    ) -> Result<Self, Box<dyn Error>> {
        let theme = parse(&source.read(128 * 1024 + 1)?)?;
        let mut files = Vfs::new(Policy::Ordered);
        if let Some(directory) = source.directory() {
            files.mount_directory(directory)?;
        }
        let scale = scale.unwrap_or(theme.scale);
        if !(50..=300).contains(&scale) {
            return Err("UI scale must be 50–300 percent".into());
        }
        let font_profiles = theme.fonts.iter().map(|(name,font)|
            (name.clone(), serde_json::json!({"source":font.source,"size":font.size,"fallback":font.fallback}))).collect();
        let mut components = Vec::new();
        let mut indices = BTreeMap::new();
        let mut glyphs = BTreeMap::new();
        for config in theme.hud {
            let index = components.len();
            let parent = config
                .parent
                .as_deref()
                .map(|id| {
                    indices
                        .get(id)
                        .copied()
                        .ok_or_else(|| format!("{}: unknown or forward parent {id}", config.id))
                })
                .transpose()?;
            if indices.insert(config.id.clone(), index).is_some() {
                return Err(format!("duplicate UI component {}", config.id).into());
            }
            let mut base = sprite(
                if config.fill.is_some()
                    || config.text.is_some()
                    || (config.hover_fill.is_some() && config.cell.is_none())
                    || config.binding.is_some_and(Binding::is_label)
                {
                    config.size
                } else {
                    [1, 1]
                },
                config.fill.unwrap_or([0; 4]),
            );
            let mut state_frames = [None, None, None];
            let mut prepared_text = None;
            let mut label = None;
            let mut clock = None;
            if !config.hidden {
                if config.binding == Some(Binding::GameClock) {
                    let asset = config
                        .image
                        .as_ref()
                        .ok_or("game-clock is missing its day strip")?;
                    let phases = config
                        .clock_phases
                        .as_ref()
                        .ok_or("game-clock requires eight moon strips")?;
                    let day = resource(game, &mut files, asset, config.frame)?;
                    let moon = phases
                        .iter()
                        .map(|path| resource(game, &mut files, path, 0))
                        .collect::<Result<_, _>>()?;
                    let art = ClockArt {
                        day,
                        moon,
                        hours: None,
                    };
                    art.paint(&mut base, 0, config.fill.unwrap_or([0; 4]));
                    clock = Some(art);
                } else if let Some(asset) = &config.image {
                    let mut load = |frame| -> Result<SpriteFrame, Box<dyn Error>> {
                        let art = resource(game, &mut files, asset, frame)
                            .map_err(|error| format!("{}: {error}", config.id))?;
                        let size = [
                            config.size[0],
                            config.thumb_height.unwrap_or(config.size[1]),
                        ];
                        let mut raster = sprite(size, config.fill.unwrap_or([0; 4]));
                        scale_into(&art.decoded.frame, &mut raster, config.source_rect)
                            .map_err(|error| format!("{}: {error}", config.id))?;
                        Ok(raster)
                    };
                    base = load(config.frame)?;
                    for (out, frame) in state_frames.iter_mut().zip([
                        config.hover_frame,
                        config.pressed_frame,
                        config.selected_frame,
                    ]) {
                        *out = frame.map(&mut load).transpose()?;
                    }
                }
                if let (Some(middle), Some(bottom)) = (&config.thumb_middle, &config.thumb_bottom) {
                    let middle = resource(game, &mut files, middle, 0)?.decoded.frame;
                    let bottom = resource(game, &mut files, bottom, 0)?.decoded.frame;
                    let width = base.width;
                    let top_height = base.height;
                    if top_height + middle.height + bottom.height > config.size[1] {
                        return Err(
                            format!("{}: thumb artwork exceeds its track", config.id).into()
                        );
                    }
                    let mut thumb =
                        sprite([width, top_height + middle.height + bottom.height], [0; 4]);
                    for (frame, y) in [
                        (&base, 0),
                        (&middle, top_height),
                        (&bottom, top_height + middle.height),
                    ] {
                        let mut rows = sprite([width, frame.height], [0; 4]);
                        scale_into(frame, &mut rows, None)?;
                        let begin = (y * width) as usize;
                        thumb.pixels[begin..begin + rows.pixels.len()]
                            .copy_from_slice(&rows.pixels);
                    }
                    base = thumb;
                }
                let dynamic = config.binding.is_some_and(Binding::is_label);
                if let Some(text) = config.text.as_deref().or(dynamic.then_some("")) {
                    let id = config
                        .font
                        .as_deref()
                        .ok_or_else(|| format!("{}: text requires a font", config.id))?;
                    let font = theme
                        .fonts
                        .get(id)
                        .ok_or_else(|| format!("{}: unknown font {id}", config.id))?;
                    let mut glyph = |ch| {
                        let key = (id.to_string(), ch);
                        if let std::collections::btree_map::Entry::Vacant(entry) =
                            glyphs.entry(key.clone())
                        {
                            let mask = font_glyph(game, &mut files, font, ch)
                                .or_else(|error| {
                                    let Some(fallback) = &font.fallback else {
                                        return Err(error);
                                    };
                                    font_glyph(game, &mut files, &theme.fonts[fallback], ch)
                                })
                                .map_err(|error| format!("{}: {error}", config.id))?;
                            entry.insert(Arc::new(mask));
                        }
                        Ok::<_, Box<dyn Error>>(Arc::clone(&glyphs[&key]))
                    };
                    let glyphs: Vec<_> = text
                        .chars()
                        .map(|ch| {
                            if ch == '\n' {
                                Ok(None)
                            } else {
                                glyph(ch).map(|frame| Some((frame, ch)))
                            }
                        })
                        .collect::<Result<_, _>>()?;
                    let native_height = glyph(' ').map_or_else(
                        |_| {
                            glyphs
                                .iter()
                                .flatten()
                                .next()
                                .map_or(font.size, |(g, _)| g.height)
                        },
                        |space| space.height,
                    );
                    if dynamic {
                        let mut alphabet = BTreeMap::new();
                        for ch in ' '..='~' {
                            if let Ok(frame) = glyph(ch) {
                                alphabet.insert(ch, frame);
                            }
                        }
                        if !alphabet.contains_key(&'?') {
                            return Err(format!(
                                "{}: dynamic label font needs a '?' glyph",
                                config.id
                            )
                            .into());
                        }
                        label = Some(Label {
                            glyphs: alphabet,
                            font: font.clone(),
                            native_height,
                            value: String::new(),
                            ink_height: 0,
                        });
                    }
                    prepared_text = Some(Text {
                        glyphs,
                        font: font.clone(),
                        native_height,
                    });
                }
            }
            let mut component = Component {
                config,
                parent,
                base,
                text: prepared_text,
                label,
                clock,
                art: generated_frame("empty", sprite([1, 1], [0; 4])),
                state_art: [None, None, None],
            };
            component.art = native_art(&component, &component.base, None);
            if !component.config.disabled && component.config.action.is_some() {
                component.state_art = std::array::from_fn(|i| {
                    if let Some(frame) = &state_frames[i] {
                        return Some(native_art(&component, frame, None));
                    }
                    (i == 0)
                        .then_some(component.config.hover_fill)
                        .flatten()
                        .map(|color| native_art(&component, &component.base, Some(color)))
                });
            }
            if component.config.cell.is_some()
                && let Some(color) = component.config.hover_fill
            {
                component.state_art[0] = Some(generated_frame("grid-hover", sprite([1, 1], color)));
            }
            if component.config.cell.is_some()
                || component.config.binding == Some(Binding::InventoryDrop)
            {
                for (index, color) in [
                    component
                        .config
                        .valid_drop_fill
                        .unwrap_or([40, 110, 40, 145]),
                    component
                        .config
                        .invalid_drop_fill
                        .unwrap_or([145, 28, 28, 170]),
                ]
                .into_iter()
                .enumerate()
                {
                    component.state_art[index + 1] = Some(generated_frame(
                        "inventory-placement",
                        sprite([1, 1], color),
                    ));
                }
            }
            components.push(component);
        }
        if let Some(author) = &theme.author {
            eprintln!("HUD author: {author}");
        }
        eprintln!(
            "HUD {} from {} at {}%",
            theme.name,
            source.path().display(),
            scale
        );
        Ok(Self {
            components,
            font_profiles,
            placed: Vec::new(),
            size: (0, 0),
            scale,
            fit_viewport: theme.fit_viewport,
            hover: None,
            pressed: None,
            route_color: theme.route_color,
            selected_route_color: theme.selected_route_color,
            selected_action: None,
            active_action: None,
            unavailable_action: None,
            focus: None,
            map_view: None,
            travel_available: false,
            effective_scale: 0,
            panel_band: Rect {
                x: 0,
                y: 0,
                w: 0,
                h: 0,
            },
            external_panel: None,
            shared_band: None,
            inventory: InventoryPanel::default(),
            dialogue_open: false,
            dialogue_available: [false; 5],
        })
    }

    pub fn layout(&mut self, size: (u32, u32)) {
        self.layout_group(size, None, None);
    }

    pub(super) fn panel_size(&self) -> [u32; 2] {
        let mut size = [1, 1];
        for c in self
            .components
            .iter()
            .filter(|c| c.parent.is_none() && !c.config.hidden)
        {
            for (axis, side) in size.iter_mut().enumerate() {
                *side = (*side).max(c.config.size[axis] + 2 * c.config.offset[axis].unsigned_abs());
            }
        }
        if let Some(fit) = self.fit_viewport {
            size[0] = size[0].max(fit[0]);
        }
        size
    }

    pub(super) fn layout_with_panel(&mut self, size: (u32, u32), panel: [u32; 2]) -> (Rect, u32) {
        self.layout_group(size, Some(panel), None);
        (self.panel_band, self.effective_scale)
    }

    pub(super) fn layout_group(
        &mut self,
        size: (u32, u32),
        external_panel: Option<[u32; 2]>,
        shared_band: Option<(Rect, u32)>,
    ) {
        if size == self.size
            && self.external_panel == external_panel
            && self.shared_band == shared_band
        {
            return;
        }
        self.size = size;
        self.external_panel = external_panel;
        self.shared_band = shared_band;
        self.placed.clear();
        let viewport = shared_band.map(|(rect, _)| rect).unwrap_or(Rect {
            x: 0,
            y: 0,
            w: size.0,
            h: size.1,
        });
        let panel = self.inventory.view();
        let mut top = 0;
        let mut bottom = 0;
        let [mut widest, mut middle] = external_panel.unwrap_or([1, 0]);
        if self.dialogue_open {
            let dialogue_height = self
                .components
                .iter()
                .filter(|c| c.parent.is_none() && c.config.dialogue_only && !c.config.hidden)
                .map(|c| {
                    widest = widest.max(c.config.size[0]);
                    c.config.size[1]
                })
                .sum();
            middle = middle.max(dialogue_height);
        }
        for c in self
            .components
            .iter()
            .filter(|c| c.parent.is_none() && !c.config.hidden && !c.config.dialogue_only)
        {
            let e = &c.config;
            if e.inventory_panel {
                // Reserve the full gameplay layout even while inventory is closed.
                middle = middle.max(e.size[1]);
                widest = widest.max(e.size[0]);
                continue;
            }
            widest = widest.max(e.size[0]);
            match e.anchor {
                Anchor::Top | Anchor::TopLeft | Anchor::TopRight => {
                    top = top.max(e.size[1].saturating_add_signed(e.offset[1]));
                }
                Anchor::Bottom | Anchor::BottomLeft | Anchor::BottomRight => {
                    bottom = bottom.max(e.size[1].saturating_add_signed(-e.offset[1]));
                }
                _ => {}
            }
        }
        let fit = self.fit_viewport.map_or(u32::MAX, |[width, height]| {
            (size.0.saturating_mul(100) / width).min(size.1.saturating_mul(100) / height)
        });
        let scale = shared_band.map(|(_, scale)| scale).unwrap_or_else(|| {
            u32::from(self.scale)
                .min(size.0.saturating_mul(100) / widest)
                .min(size.1.saturating_mul(100) / (top + bottom + middle).max(1))
                .min(fit)
        });
        self.effective_scale = scale;
        self.panel_band = Rect {
            w: 0,
            h: 0,
            ..viewport
        };
        if scale == 0 {
            return;
        }
        let top = top * scale / 100;
        let bottom = bottom * scale / 100;
        let band = Rect {
            y: viewport.y + top as i32,
            h: viewport.h.saturating_sub(top + bottom),
            ..viewport
        };
        self.panel_band = band;
        let mut bounds = Vec::with_capacity(self.components.len());
        for component in &self.components {
            let parent = component.parent.map_or_else(
                || {
                    let area = if component.config.inventory_panel || component.config.dialogue_only
                    {
                        band
                    } else {
                        viewport
                    };
                    Some((area, area))
                },
                |id| bounds[id],
            );
            let placement = parent.and_then(|(parent, clip)| {
                if self.map_view.is_some_and(|mode| {
                    !component.config.map_views.is_empty()
                        && !component.config.map_views.contains(&mode)
                }) || component.config.hidden
                    || (component.config.dialogue_only && !self.dialogue_open)
                    || (component.config.binding == Some(Binding::NpcContext) && panel.is_some())
                    || (self.dialogue_open
                        && component.config.binding == Some(Binding::InventoryHover))
                    || (component.config.inventory_panel && panel.is_none())
                    || (!component.config.inventory_views.is_empty()
                        && panel
                            .is_none_or(|view| !component.config.inventory_views.contains(&view)))
                    || component
                        .config
                        .travel_available
                        .is_some_and(|available| available != self.travel_available)
                    || component.config.binding.is_some_and(Binding::is_marker)
                {
                    return None;
                }
                let rect = layout_rect(&component.config, parent, scale);
                Some((rect, rect.intersect(clip)?))
            });
            bounds.push(placement);
            self.placed.push(placement.map(|(rect, clip)| Placement {
                full: rect,
                scale,
                rect: clip,
                live_asset: None,
            }));
        }
    }

    pub(super) fn diagnostics(&self) -> serde_json::Value {
        let control = |index: Option<usize>| {
            index.and_then(|index| {
                self.components
                    .get(index)
                    .map(|component| component.config.id.as_str())
            })
        };
        let images: std::collections::BTreeSet<_> = self
            .components
            .iter()
            .filter_map(|component| component.config.image.as_deref())
            .collect();
        let label = |binding| {
            self.bound(binding)
                .and_then(|index| self.components[index].label.as_ref())
                .map(|label| label.value.as_str())
        };
        let responses: Vec<_> = self.components.iter().zip(&self.placed).filter_map(|(c,p)| {
            let index = c.config.response?;
            let p = p.as_ref()?.rect;
            Some(serde_json::json!({"index":index,"bounds":[p.x,p.y,p.w,p.h],"available":self.dialogue_available[index]}))
        }).collect();
        serde_json::json!({"context_message":label(Binding::InventoryHover),
            "npc_context":label(Binding::NpcContext),"dialogue_responses":responses,
            "map_status":label(Binding::MapStatus),"scale_percent":self.scale,"effective_scale_percent":self.effective_scale,"inventory":self.inventory_diagnostics(),"fonts":self.font_profiles,"image_references":images,
            "focused":control(self.focus),"hovered":control(self.hover),"pressed":control(self.pressed)})
    }

    pub(super) fn scale(&self) -> u16 {
        self.scale
    }

    pub(super) fn set_scale(&mut self, scale: u16) {
        if self.scale != scale {
            self.scale = scale.clamp(50, 300);
            self.clear_layout();
        }
    }

    pub(super) fn set_travel_available(&mut self, available: bool) -> bool {
        if self.travel_available == available {
            return false;
        }
        self.travel_available = available;
        self.clear_layout();
        true
    }

    pub(super) fn set_map_view(&mut self, mode: MapView) {
        if self.map_view != Some(mode) {
            self.map_view = Some(mode);
            self.clear_layout();
        }
    }

    pub(super) fn lens_size(&self) -> Option<[u32; 2]> {
        Some(
            self.components[self.bound(Binding::PlayerLens)?]
                .config
                .size,
        )
    }

    pub(super) fn set_lens_asset(&mut self, binding: Binding, frame: Arc<LoadedFrame>) {
        if let Some(index) = self.placed_bound(binding)
            && let Some(Some(place)) = self.placed.get_mut(index)
        {
            place.live_asset = Some(frame);
        }
    }

    pub(super) fn has_canvas(&self) -> bool {
        self.bound(Binding::MapCanvas)
            .is_some_and(|index| !self.components[index].config.hidden)
    }

    pub(super) fn clear_layout(&mut self) {
        self.placed.clear();
        self.size = (0, 0);
    }

    fn bound(&self, binding: Binding) -> Option<usize> {
        self.components
            .iter()
            .position(|c| c.config.binding == Some(binding))
    }

    fn placed_bound(&self, binding: Binding) -> Option<usize> {
        self.components.iter().enumerate().position(|(index, c)| {
            c.config.binding == Some(binding) && self.placed.get(index).is_some_and(Option::is_some)
        })
    }

    pub(super) fn native_size(&self, binding: Binding) -> Option<[u32; 2]> {
        Some(self.components[self.placed_bound(binding)?].config.size)
    }

    pub(super) fn bounds(&self, binding: Binding) -> Option<(Rect, u32)> {
        let place = self.placed.get(self.placed_bound(binding)?)?.as_ref()?;
        Some((place.rect, place.scale))
    }

    pub(super) fn route_color(&self, selected: bool) -> Option<[u8; 4]> {
        if selected {
            self.selected_route_color.or(self.route_color)
        } else {
            self.route_color
        }
    }

    pub(super) fn marker(&self, binding: Binding) -> Option<&SpriteFrame> {
        Some(&self.components[self.bound(binding)?].base)
    }

    pub(super) fn set_canvas(&mut self, frame: SpriteFrame) {
        if let Some(index) = self.bound(Binding::MapCanvas) {
            self.components[index].art = generated_frame(&self.components[index].config.id, frame);
        }
    }

    pub(super) fn set_game_time(&mut self, hours: u64) -> bool {
        let Some(index) = self.bound(Binding::GameClock) else {
            return false;
        };
        let component = &mut self.components[index];
        let Some(clock) = &mut component.clock else {
            return false;
        };
        if clock.hours == Some(hours) {
            return false;
        }
        clock.hours = Some(hours);
        clock.paint(
            &mut component.base,
            hours,
            component.config.fill.unwrap_or([0; 4]),
        );
        component.art = native_art(component, &component.base, None);
        true
    }

    pub(super) fn set_label(&mut self, binding: Binding, value: &str) {
        if let Some(index) = self.bound(binding) {
            self.write_label(index, value);
        }
    }

    fn write_label(&mut self, index: usize, value: &str) {
        if let Some(label) = &mut self.components[index].label
            && label.value != value
        {
            label.value = value.into();
            self.components[index].text = Some(label.text(value));
            let component = &mut self.components[index];
            component.art = native_art(component, &component.base, None);
            if component.config.binding == Some(Binding::DialogueText) {
                let frame = &component.art.decoded.frame;
                component.label.as_mut().unwrap().ink_height = frame
                    .pixels
                    .chunks_exact(frame.width as usize)
                    .rposition(|row| row.iter().any(|pixel| pixel[3] != 0))
                    .map_or(0, |row| row as u32 + 1);
            }
            if let Some(text) = &component.text {
                let normal = text.font.color;
                for (variant, color) in [
                    (0, component.config.hover_text_color),
                    (2, component.config.disabled_text_color),
                ] {
                    if let Some(color) = color {
                        component.text.as_mut().unwrap().font.color = color;
                        component.state_art[variant] =
                            Some(native_art(component, &component.base, None));
                    }
                }
                component.text.as_mut().unwrap().font.color = normal;
            }
        }
    }

    pub(super) fn supports_dialogue(&self) -> bool {
        self.bound(Binding::DialogueText).is_some_and(|i| {
            !self.components[i].config.hidden && self.components[i].label.is_some()
        }) && (0..5).all(|row| {
            self.components
                .iter()
                .any(|c| c.config.response == Some(row) && !c.config.hidden && c.label.is_some())
        })
    }

    pub(super) fn set_dialogue(
        &mut self,
        session: Option<&arcanum_rs::content::dialogue::Conversation>,
        npc_at: Option<(i32, i32)>,
        size: (u32, u32),
    ) {
        if self.dialogue_open != session.is_some() {
            self.dialogue_open = session.is_some();
            self.clear_layout();
        }
        self.layout(size);
        let Some(session) = session else { return };
        self.set_label(Binding::NpcContext, &session.speaker);
        self.set_label(Binding::DialogueText, &session.turn.text);
        self.dialogue_available =
            std::array::from_fn(|row| session.turn.responses.get(row).is_some_and(|r| r.available));
        for index in 0..self.components.len() {
            if let Some(row) = self.components[index].config.response {
                self.write_label(
                    index,
                    session.turn.responses.get(row).map_or("", |r| &r.text),
                );
            }
        }
        let response_top = self
            .components
            .iter()
            .zip(&self.placed)
            .filter_map(|(c, p)| c.config.response.and(p.as_ref()).map(|p| p.rect.y))
            .min();
        if let Some(index) = self.bound(Binding::DialogueText)
            && let Some(place) = self.placed[index].as_mut()
            && let Some((x, y)) = npc_at
        {
            let band = self.panel_band;
            let rect = &mut place.full;
            let bottom = response_top.unwrap_or(band.y + band.h as i32);
            let visible = self.components[index]
                .label
                .as_ref()
                .map_or(0, |label| (label.ink_height * place.scale / 100) as i32);
            rect.x = (x - rect.w as i32 / 2)
                .clamp(band.x, band.x + band.w.saturating_sub(rect.w) as i32);
            rect.y = (y - visible).clamp(band.y, (bottom - visible).max(band.y));
            place.rect = rect
                .intersect(Rect {
                    h: (bottom - band.y).max(0) as u32,
                    ..band
                })
                .unwrap_or(Rect { w: 0, h: 0, ..band });
        }
    }

    pub(super) fn clear_dialogue_choices(&mut self) {
        self.dialogue_available.fill(false);
    }

    pub(super) fn dialogue_choice(&self, point: (u32, u32)) -> Option<usize> {
        let row = self.components[self.select(point)?].config.response?;
        self.dialogue_available[row].then_some(row)
    }

    pub(super) fn draw_map_label(
        &self,
        canvas: &mut SpriteFrame,
        point: (i32, i32),
        scale: u32,
        value: &str,
    ) {
        let Some(component) = self
            .bound(Binding::PlaceLabel)
            .map(|index| &self.components[index])
        else {
            return;
        };
        let Some(label) = component.label.as_ref() else {
            return;
        };
        let config = &component.config;
        let scaled = |value: i32| value * scale as i32 / 100;
        let full = Rect {
            x: point.0 + scaled(config.offset[0]),
            y: point.1 + scaled(config.offset[1]),
            w: (config.size[0] * scale / 100).max(1),
            h: (config.size[1] * scale / 100).max(1),
        };
        let canvas_bounds = Rect {
            x: 0,
            y: 0,
            w: canvas.width,
            h: canvas.height,
        };
        let Some(clip) = full.intersect(canvas_bounds) else {
            return;
        };
        let mut layer = sprite([clip.w, clip.h], [0; 4]);
        let mut text = label.text(value);
        if let Some(shadow) = config.text_shadow {
            let offset = scaled(1).max(1);
            let shifted = Rect {
                x: full.x + offset,
                y: full.y + offset,
                ..full
            };
            // The shared rasterizer assumes the output rectangle is inside the label.
            if let Some(shadow_clip) = clip.intersect(shifted) {
                let mut shadow_layer = sprite([shadow_clip.w, shadow_clip.h], [0; 4]);
                let color = std::mem::replace(&mut text.font.color, shadow);
                draw_text(
                    &mut shadow_layer,
                    &text,
                    config,
                    shifted,
                    shadow_clip,
                    scale,
                );
                text.font.color = color;
                for y in 0..shadow_clip.h {
                    let base = (y + (shadow_clip.y - clip.y) as u32) * clip.w
                        + (shadow_clip.x - clip.x) as u32;
                    let pixels = &shadow_layer.pixels
                        [(y * shadow_clip.w) as usize..((y + 1) * shadow_clip.w) as usize];
                    layer.pixels[base as usize..(base + shadow_clip.w) as usize]
                        .copy_from_slice(pixels);
                }
            }
        }
        draw_text(&mut layer, &text, config, full, clip, scale);
        for (index, color) in layer.pixels.into_iter().enumerate() {
            let x = index as u32 % clip.w + clip.x as u32;
            let y = index as u32 / clip.w + clip.y as u32;
            blend_glyph(
                &mut canvas.pixels[(y * canvas.width + x) as usize],
                color,
                255,
            );
        }
    }

    pub fn adjust_scale(&mut self, step: i16) {
        self.scale = (self.scale as i16 + step).clamp(50, 300) as u16;
        let size = std::mem::take(&mut self.size);
        self.layout_group(size, self.external_panel, self.shared_band);
    }

    fn select(&self, point: (u32, u32)) -> Option<usize> {
        self.placed
            .iter()
            .enumerate()
            .rev()
            .find_map(|(index, placement)| {
                let config = &self.components[index].config;
                (placement.as_ref()?.rect.contains(point)
                    && (config.capture || config.action.is_some() || config.response.is_some()))
                .then_some(index)
            })
    }
    pub(super) fn hit(&self, point: (u32, u32)) -> Hit {
        self.select(point).map_or(Hit::World, |index| {
            self.available_action(index).map_or(Hit::Panel, Hit::Button)
        })
    }
    pub fn hover(&mut self, point: Option<(u32, u32)>) -> bool {
        let hover = point.and_then(|point| self.select(point)).filter(|&index| {
            self.available_action(index).is_some()
                || self.components[index]
                    .config
                    .response
                    .is_some_and(|row| self.dialogue_available[row])
        });
        let controls_changed = std::mem::replace(&mut self.hover, hover) != hover;
        self.inventory_hover(point) || controls_changed
    }

    pub(super) fn press(&mut self, point: (u32, u32)) -> bool {
        self.hover(Some(point));
        self.pressed = self.hover;
        self.pressed.is_some()
    }

    pub(super) fn release(&mut self, point: (u32, u32)) -> Option<Action> {
        let index = self.pressed.take()?;
        (self.select(point) == Some(index))
            .then(|| self.available_action(index))
            .flatten()
    }

    pub(super) fn cancel_press(&mut self) -> bool {
        self.pressed.take().is_some()
    }

    pub(super) fn is_pressed(&self) -> bool {
        self.pressed.is_some()
    }

    pub(super) fn set_selected_action(&mut self, action: Action) {
        self.selected_action = Some(action);
    }

    pub(super) fn set_active_action(&mut self, action: Option<Action>) {
        self.active_action = action;
    }

    pub(super) fn set_unavailable_action(&mut self, action: Option<Action>) {
        self.unavailable_action = action;
    }

    pub fn advance_focus(&mut self, backwards: bool) {
        let len = self.components.len();
        if len == 0 {
            return;
        }
        let start = self.focus.unwrap_or(if backwards { 0 } else { len - 1 });
        self.focus = (1..=len)
            .map(|step| {
                if backwards {
                    (start + len - step) % len
                } else {
                    (start + step) % len
                }
            })
            .find(|&index| self.available_action(index).is_some());
    }

    pub(super) fn focused_action(&self) -> Option<Action> {
        self.available_action(self.focus?)
    }

    fn available_action(&self, index: usize) -> Option<Action> {
        self.placed.get(index)?.as_ref()?;
        let config = &self.components[index].config;
        config.action.filter(|action| {
            !config.disabled
                && Some(*action) != self.unavailable_action
                && (*action != Action::ToggleInventory || self.has_inventory())
        })
    }

    pub fn draws(&self, running: bool) -> impl Iterator<Item = OverlayDraw<'_>> {
        self.placed
            .iter()
            .enumerate()
            .filter_map(move |(index, placement)| {
                let p = placement.as_ref()?;
                let component = &self.components[index];
                if component
                    .config
                    .equipment_slot
                    .is_some_and(|slot| self.equipped(slot).is_some())
                {
                    return None;
                }
                if component.config.binding == Some(Binding::ContainerThumb)
                    || (p.live_asset.is_none()
                        && component.config.image.is_none()
                        && component.config.fill.is_none()
                        && component.config.hover_fill.is_none()
                        && component
                            .text
                            .as_ref()
                            .is_none_or(|text| text.glyphs.is_empty())
                        && component.config.binding != Some(Binding::MapCanvas))
                    || component.config.cell.is_some()
                {
                    return None;
                }
                let action = component.config.action;
                let hot = self.hover == Some(index);
                let state = if hot && self.pressed == Some(index) {
                    component.state_art[1].as_ref()
                } else {
                    None
                };
                let selected = (action.is_some()
                    && (action == self.selected_action
                        || action == self.active_action
                        || (action == Some(Action::ToggleInventory) && self.inventory_open())))
                .then_some(component.state_art[2].as_ref())
                .flatten();
                let highlighted = (hot
                    || self.focus == Some(index)
                    || (running && action == Some(Action::ToggleRun)))
                .then_some(component.state_art[0].as_ref())
                .flatten();
                let disabled_choice = component
                    .config
                    .response
                    .filter(|&row| !self.dialogue_available[row])
                    .and(component.state_art[2].as_ref());
                let asset = disabled_choice
                    .or(state)
                    .or(selected)
                    .or(highlighted)
                    .or(p.live_asset.as_ref())
                    .unwrap_or(&component.art);
                Some(overlay(asset, p.full, p.rect))
            })
            .chain(self.inventory_draws())
    }
}

fn parse(source: &str) -> Result<Theme, Box<dyn Error>> {
    if source.len() > 128 * 1024 {
        return Err("theme exceeds 128 KiB".into());
    }
    let theme: Theme = toml::from_str(source)?;
    if theme.name.trim().is_empty()
        || theme.hud.len() > 128
        || theme.fonts.len() > 16
        || theme
            .fit_viewport
            .is_some_and(|size| size.into_iter().any(|value| !(1..=4096).contains(&value)))
        || !(50..=300).contains(&theme.scale)
    {
        return Err("invalid theme name, scale or component count".into());
    }
    let mut pixels = 0;
    for e in &theme.hud {
        if e.id.is_empty()
            || e.size.iter().any(|&v| !(1..=1024).contains(&v))
            || e.offset.iter().any(|v| v.unsigned_abs() > 4096)
            || (e.image.is_none()
                && (e.source_rect.is_some()
                    || [e.hover_frame, e.pressed_frame, e.selected_frame]
                        .into_iter()
                        .any(|frame| frame.is_some())))
            || e.thumb_height.is_some_and(|height| {
                height == 0 || height > e.size[1] || e.binding != Some(Binding::ContainerThumb)
            })
            || (e.thumb_middle.is_some() != e.thumb_bottom.is_some())
            || (e.thumb_middle.is_some() && (e.thumb_height.is_none() || e.image.is_none()))
            || e.cell.is_some_and(|cell| {
                !(1..=128).contains(&cell) || e.size.iter().any(|&side| side % cell != 0)
            })
            || e.cell.is_some()
                != matches!(e.binding, Some(Binding::Backpack | Binding::ContainerGrid))
            || (e.inventory_panel
                && (e.parent.is_some()
                    || !matches!(e.anchor, Anchor::Center)
                    || e.offset != [0, 0]
                    || !e.inventory_views.is_empty()))
            || e.response.is_some_and(|index| index >= 5)
            || e.response.is_some() != (e.binding == Some(Binding::DialogueResponse))
            || e.text.as_ref().is_some_and(|s| s.len() > 256)
        {
            return Err(format!("invalid size, offset or text in component {}", e.id).into());
        }
        let variants = [e.hover_frame, e.pressed_frame, e.selected_frame]
            .into_iter()
            .flatten()
            .count() as u32;
        let hover_fill = e.hover_fill.is_some() && e.cell.is_none();
        if e.image.is_some()
            || e.text.is_some()
            || e.fill.is_some()
            || e.binding.is_some_and(Binding::is_label)
            || hover_fill
        {
            pixels += e.size[0]
                * e.size[1]
                * (1 + variants + u32::from(hover_fill && e.hover_frame.is_none()));
        }
    }
    if pixels > 1024 * 1024
        || theme.hud.iter().filter(|e| e.inventory_panel).count() > 1
        || theme.fonts.values().any(|font| {
            !(5..=48).contains(&font.size)
                || font.line_gap > 32
                || font
                    .fallback
                    .as_ref()
                    .is_some_and(|f| !theme.fonts.contains_key(f))
        })
    {
        return Err("theme exceeds image bounds or has an invalid font profile".into());
    }
    Ok(theme)
}

pub(super) fn resource(
    game: &mut Vfs,
    files: &mut Vfs,
    asset: &str,
    frame: usize,
) -> Result<LoadedFrame, Box<dyn Error>> {
    let (scope, path) = asset
        .split_once(':')
        .ok_or("asset must start with vfs: or file:")?;
    let vfs = match scope {
        "vfs" => game,
        "file" => files,
        _ => return Err("unknown asset scope".into()),
    };
    Ok(load_frame(vfs, path, frame)?)
}

fn layout_rect(e: &Element, parent: Rect, s: u32) -> Rect {
    let w = (e.size[0] * s / 100).max(1);
    let h = (e.size[1] * s / 100).max(1);
    let horizontal = match e.anchor {
        Anchor::TopLeft | Anchor::Left | Anchor::BottomLeft => 0,
        Anchor::Top | Anchor::Center | Anchor::Bottom => 1,
        _ => 2,
    };
    let vertical = match e.anchor {
        Anchor::TopLeft | Anchor::Top | Anchor::TopRight => 0,
        Anchor::Left | Anchor::Center | Anchor::Right => 1,
        _ => 2,
    };
    Rect {
        x: parent.x + (parent.w as i32 - w as i32) * horizontal / 2 + e.offset[0] * s as i32 / 100,
        y: parent.y + (parent.h as i32 - h as i32) * vertical / 2 + e.offset[1] * s as i32 / 100,
        w,
        h,
    }
}
fn sprite([width, height]: [u32; 2], color: Color) -> SpriteFrame {
    SpriteFrame {
        width,
        height,
        hotspot: (0, 0),
        offset: (0, 0),
        pixels: vec![color; (width * height) as usize],
    }
}
fn scale_into(
    src: &SpriteFrame,
    dst: &mut SpriteFrame,
    crop: Option<[u32; 4]>,
) -> Result<(), &'static str> {
    let [left, top, width, height] = crop.unwrap_or([0, 0, src.width, src.height]);
    if width == 0
        || height == 0
        || left.saturating_add(width) > src.width
        || top.saturating_add(height) > src.height
    {
        return Err("image source_rect exceeds ART frame bounds");
    }
    for y in 0..dst.height {
        for x in 0..dst.width {
            let sx = left + x * width / dst.width;
            let sy = top + y * height / dst.height;
            let px = src.pixels[(sy * src.width + sx) as usize];
            if px[3] != 0 {
                dst.pixels[(y * dst.width + x) as usize] = px;
            }
        }
    }
    Ok(())
}

fn overlay(asset: &Arc<LoadedFrame>, rect: Rect, clip: Rect) -> OverlayDraw<'_> {
    let pixels = |r: Rect| [r.x as f32, r.y as f32, r.w as f32, r.h as f32];
    OverlayDraw {
        asset,
        rect: pixels(rect),
        clip: pixels(clip),
    }
}

fn native_art(c: &Component, src: &SpriteFrame, hover: Option<Color>) -> Arc<LoadedFrame> {
    if c.text.is_none() && hover.is_none() {
        return generated_frame(&c.config.id, src.clone());
    }
    let rect = Rect {
        x: 0,
        y: 0,
        w: src.width,
        h: src.height,
    };
    raster(c, src, rect, rect, 100, hover)
}

fn raster(
    c: &Component,
    src: &SpriteFrame,
    full: Rect,
    clip: Rect,
    scale: u32,
    hover: Option<Color>,
) -> Arc<LoadedFrame> {
    let mut frame = sprite([clip.w, clip.h], [0; 4]);
    for y in 0..clip.h {
        for x in 0..clip.w {
            let sx = ((clip.x - full.x) as u32 + x) * src.width / full.w;
            let sy = ((clip.y - full.y) as u32 + y) * src.height / full.h;
            let mut color = src.pixels[(sy * src.width + sx) as usize];
            if let Some(highlight) = hover
                && (color == c.config.fill.unwrap_or([0; 4])
                    || sx == 0
                    || sy == 0
                    || sx + 1 == src.width
                    || sy + 1 == src.height)
            {
                color = highlight;
            }
            frame.pixels[(y * clip.w + x) as usize] = color;
        }
    }
    if let Some(text) = &c.text {
        if let Some(color) = c.config.text_shadow {
            draw_text_colored(
                &mut frame,
                text,
                &c.config,
                full,
                clip,
                scale,
                (color, (scale / 100).max(1)),
            );
        }
        draw_text(&mut frame, text, &c.config, full, clip, scale);
    }
    generated_frame(&c.config.id, frame)
}

pub(super) fn generated_frame(id: &str, frame: SpriteFrame) -> Arc<LoadedFrame> {
    Arc::new(LoadedFrame {
        source: Source {
            virtual_path: format!("ui/{id}"),
            mount_id: MountId::GENERATED,
            mount_path: Default::default(),
            loose_path: None,
            storage: Storage::Loose,
            mod_id: None,
        },
        decoded: DecodedFrame {
            frame,
            frame_count: 1,
            rotation_count: 1,
            stored_rotation: 0,
            stored_palette: 0,
            palette_key: [0; 3],
            frames_per_second: 0,
            action_frame: 0,
            ignored_trailing_bytes: 0,
        },
        input_bytes: 0,
    })
}

fn font_glyph(
    game: &mut Vfs,
    files: &mut Vfs,
    font: &Font,
    ch: char,
) -> Result<SpriteFrame, Box<dyn Error>> {
    if font.source == "bitmap" {
        let ch = u8::try_from(ch.to_ascii_uppercase() as u32)
            .map_err(|_| "bitmap font supports ASCII")?;
        let rows = super::menu::glyph_rows(ch);
        if ch != b' ' && rows == [0; 7] {
            return Err(format!("bitmap font has no glyph for {}", char::from(ch)).into());
        }
        let mut frame = sprite([6, 7], [0; 4]);
        for (y, row) in rows.into_iter().enumerate() {
            for x in 0..5 {
                if row & (16 >> x) != 0 {
                    frame.pixels[y * 6 + x] = [255; 4];
                }
            }
        }
        frame.hotspot.0 = 7;
        Ok(frame)
    } else {
        let frame = (ch as usize)
            .checked_sub(31)
            .ok_or("invalid font character")?;
        let mut glyph = resource(game, files, &font.source, frame)?.decoded.frame;
        if glyph.width > 64 || glyph.height > 64 || !(0..=128).contains(&glyph.hotspot.0) {
            return Err("ART font glyph dimensions or advance exceed bounds".into());
        }
        for [red, _, _, alpha] in &mut glyph.pixels {
            if *alpha != 0 {
                // TIG's source-alpha font blit takes coverage from palette red.
                *alpha = *red;
            }
        }
        Ok(glyph)
    }
}

fn draw_text(
    frame: &mut SpriteFrame,
    text: &Text,
    element: &Element,
    full: Rect,
    clip: Rect,
    scale: u32,
) {
    draw_text_colored(
        frame,
        text,
        element,
        full,
        clip,
        scale,
        (text.font.color, 0),
    );
}

fn draw_text_colored(
    frame: &mut SpriteFrame,
    text: &Text,
    element: &Element,
    full: Rect,
    clip: Rect,
    scale: u32,
    (color, offset): (Color, u32),
) {
    let Text {
        glyphs,
        font,
        native_height,
    } = text;
    let to_pixels = |units: u32| (units * scale + native_height * 50) / (native_height * 100);
    let scale_glyph = |units| to_pixels(units * font.size);
    let line_step = (font.size + font.line_gap) * native_height;
    let (mut start, mut line) = (0, 0);
    let visible_x = (clip.x - full.x) as u32;
    let visible_y = (clip.y - full.y) as u32;
    while start < glyphs.len() {
        let y = offset + to_pixels(line * line_step);
        if y >= visible_y + frame.height {
            break;
        }
        let (mut end, mut advance, mut width, mut space) = (start, 0, 0, None);
        while let Some(Some((glyph, ch))) = glyphs.get(end) {
            let next = width.max(advance + glyph.width);
            if element.wrap && scale_glyph(next) > full.w && end > start {
                if let Some((word_end, line_width)) = space {
                    end = word_end;
                    width = line_width;
                }
                break;
            }
            if *ch == ' ' {
                space = Some((end, width));
            }
            advance += glyph.hotspot.0 as u32;
            width = next;
            end += 1;
        }
        let space_left = full.w.saturating_sub(scale_glyph(width));
        let origin = match element.align {
            Align::Left => 0,
            Align::Center => space_left / 2,
            Align::Right => space_left,
        };
        let mut pen = 0;
        for (glyph, _) in glyphs[start..end].iter().flatten() {
            let x = offset + origin + scale_glyph(pen);
            let w = (offset + origin + scale_glyph(pen + glyph.width) - x).max(1);
            let h = scale_glyph(glyph.height).max(1);
            let bold = if font.bold { (scale / 100).max(1) } else { 0 };
            for py in y.max(visible_y)..(y + h).min(visible_y + frame.height) {
                let sy = (2 * (py - y) + 1) * glyph.height / (2 * h);
                for px in x.max(visible_x)..(x + w + bold).min(visible_x + frame.width) {
                    let u = px - x;
                    let coverage = |ix: u32| {
                        let sx = (2 * ix + 1) * glyph.width / (2 * w);
                        glyph.pixels[(sy * glyph.width + sx) as usize][3]
                    };
                    let alpha = if u < w { coverage(u) } else { 0 };
                    let alpha = if bold > 0 && u >= bold && u - bold < w {
                        alpha.max(coverage(u - bold))
                    } else {
                        alpha
                    };
                    let at = ((py - visible_y) * frame.width + px - visible_x) as usize;
                    blend_glyph(&mut frame.pixels[at], color, alpha);
                }
            }
            pen += glyph.hotspot.0 as u32;
        }
        start = end;
        if glyphs
            .get(start)
            .is_some_and(|g| g.as_ref().is_none_or(|(_, ch)| *ch == ' '))
        {
            start += 1;
        }
        line += 1;
    }
}

pub(super) fn blend_glyph(dst: &mut Color, color: Color, coverage: u8) {
    let alpha = (u32::from(color[3]) * u32::from(coverage) + 127) / 255;
    if alpha == 0 {
        return;
    }
    let background = u32::from(dst[3]) * (255 - alpha);
    let total = alpha * 255 + background;
    for i in 0..3 {
        let sum = u32::from(color[i]) * alpha * 255 + u32::from(dst[i]) * background;
        dst[i] = ((sum + total / 2) / total) as u8;
    }
    dst[3] = ((total + 127) / 255) as u8;
}

pub(super) fn console_font(
    game: &mut Vfs,
    theme: &ThemeSource,
    source: &str,
) -> Result<BTreeMap<char, SpriteFrame>, Box<dyn Error>> {
    let mut files = Vfs::new(Policy::Ordered);
    if let Some(directory) = theme.directory() {
        files.mount_directory(directory)?;
    }
    let font = Font {
        source: source.into(),
        size: 16,
        color: [255; 4],
        bold: false,
        line_gap: 0,
        fallback: None,
    };
    let mut glyphs = BTreeMap::new();
    for ch in ' '..='ÿ' {
        match font_glyph(game, &mut files, &font, ch) {
            Ok(glyph) => {
                glyphs.insert(ch, glyph);
            }
            Err(error) if source != "bitmap" && ch.is_ascii_alphanumeric() => return Err(error),
            Err(_) => {}
        }
    }
    Ok(glyphs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicUsize, Ordering},
    };

    struct Package(PathBuf);

    impl Package {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "arcanum-hud-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }

        fn theme(&self, source: &str) -> Result<Hud, Box<dyn Error>> {
            let path = self.0.join("theme.toml");
            fs::write(&path, source)?;
            Hud::load(
                &ThemeSource::external(path),
                &mut Vfs::new(Policy::Ordered),
                None,
            )
        }
    }

    impl Drop for Package {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    const EXAMPLE: &str = include_str!("../../ui/field-notes/theme.toml");

    #[test]
    fn component_can_use_original_art_subrect_without_stretching_other_pixels() {
        let source = SpriteFrame {
            pixels: vec![[1, 2, 3, 255], [2, 3, 4, 255], [3, 4, 5, 255]],
            ..sprite([3, 1], [0; 4])
        };
        let mut frame = sprite([2, 1], [0; 4]);
        scale_into(&source, &mut frame, Some([1, 0, 2, 1])).unwrap();
        assert_eq!(frame.pixels, source.pixels[1..]);
        assert!(scale_into(&source, &mut frame, Some([2, 0, 2, 1])).is_err());
    }

    #[test]
    fn text_preserves_coverage_and_cumulative_advance_at_fractional_scales() {
        let mut glyph = sprite([2, 2], [255, 255, 255, 64]);
        glyph.hotspot.0 = 3;
        let glyph = Arc::new(glyph);
        let text = Text {
            glyphs: vec![Some((Arc::clone(&glyph), 'A')), Some((glyph, 'A'))],
            font: Font {
                source: "bitmap".into(),
                size: 2,
                color: [255; 4],
                bold: false,
                line_gap: 0,
                fallback: None,
            },
            native_height: 2,
        };
        let theme = parse(EXAMPLE).unwrap();
        for (scale, second_x) in [(100, 3), (125, 4), (150, 5), (200, 6)] {
            let full = Rect {
                x: 0,
                y: 0,
                w: 20,
                h: 5,
            };
            let mut frame = sprite([full.w, full.h], [0, 0, 0, 255]);
            draw_text(&mut frame, &text, &theme.hud[0], full, full, scale);
            assert_eq!(frame.pixels[0], [64, 64, 64, 255]);
            assert_eq!(frame.pixels[second_x - 1], [0, 0, 0, 255]);
            assert_eq!(frame.pixels[second_x], [64, 64, 64, 255]);
        }
    }

    #[test]
    #[ignore = "requires installed Arcanum data"]
    fn classic_uses_native_button_bounds_and_installed_font_metrics() {
        let root = std::env::var_os("ARCANUM_GAME_DIR").expect("set ARCANUM_GAME_DIR");
        let mut game = Vfs::from_game(root, Policy::Ce).unwrap();
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("ui/classic/theme.toml");
        let mut files = Vfs::new(Policy::Ordered);
        let font = Font {
            source: "vfs:art/interface/morph15font.art".into(),
            size: 19,
            color: [255; 4],
            bold: false,
            line_gap: 0,
            fallback: None,
        };
        let glyph = font_glyph(&mut game, &mut files, &font, 'A').unwrap();
        assert_eq!(
            (glyph.width, glyph.height, glyph.hotspot),
            (11, 19, (11, 16))
        );
        assert!(
            glyph
                .pixels
                .iter()
                .any(|color| color[3] > 0 && color[3] < 255)
        );

        let source = ThemeSource::external(path);
        for scale in [100, 125, 150, 200] {
            let mut hud = Hud::load(&source, &mut game, Some(scale)).unwrap();
            hud.layout((1920, 1080));
            assert_eq!(hud.draws(false).count(), 33);
            assert_eq!(hud.scale(), scale);
            let effective = u32::from(scale).min(180);
            assert_eq!(hud.effective_scale, effective);
            let left = (1920 - 8 * effective) / 2;
            let button = hud.placed[2].as_ref().unwrap().rect;
            assert_eq!(button.x as u32, left + 4 * effective / 100);
            assert_eq!(button.y as u32, 2 * effective / 100);
            assert_eq!(button.w, 34 * effective / 100);
            assert_eq!(hud.hit((button.x as u32, button.y as u32)), Hit::Panel);
            let map = hud.placed[4].as_ref().unwrap().rect;
            assert_eq!(
                hud.hit((map.x as u32, map.y as u32)),
                Hit::Button(Action::OpenMap)
            );
            for id in [
                "maintained-spell-slot-1",
                "maintained-spell-slot-5",
                "clock-face",
            ] {
                let part = hud
                    .components
                    .iter()
                    .find(|part| part.config.id == id)
                    .unwrap();
                let center =
                    (part.base.height / 2 * part.base.width + part.base.width / 2) as usize;
                assert_eq!(part.base.pixels[center][3], 255, "{id}");
            }
            assert_eq!(hud.hit((left - 1, 5)), Hit::World);
            hud.advance_focus(false);
            assert_eq!(hud.focused_action(), Some(Action::OpenMap));
        }
    }

    #[test]
    #[ignore = "requires installed Arcanum data"]
    fn atlas_controls_use_authored_frames_and_cancel_press_outside() {
        let root = std::env::var_os("ARCANUM_GAME_DIR").expect("set ARCANUM_GAME_DIR");
        let mut game = Vfs::from_game(root, Policy::Ce).unwrap();
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("ui/classic-map/theme.toml");
        let mut hud = Hud::load(&ThemeSource::external(path), &mut game, Some(125)).unwrap();
        hud.set_map_view(MapView::Local);
        hud.layout((1920, 1080));
        let index = hud
            .components
            .iter()
            .position(|c| c.config.id == "zoom")
            .unwrap();
        let bounds = hud.placed[index].as_ref().unwrap().rect;
        let point = (bounds.x as u32 + 4, bounds.y as u32 + 4);
        let current = |hud: &Hud| {
            Arc::clone(
                hud.draws(false)
                    .find(|draw| draw.asset.source.virtual_path == "ui/zoom")
                    .unwrap()
                    .asset,
            )
        };
        let idle = current(&hud);
        hud.hover(Some(point));
        assert!(Arc::ptr_eq(&idle, &current(&hud)));
        assert!(hud.press(point));
        let pressed = current(&hud);
        assert_ne!(idle.decoded.frame.pixels, pressed.decoded.frame.pixels);
        assert_eq!(hud.release((0, 0)), None);
        assert!(Arc::ptr_eq(&idle, &current(&hud)));
        hud.set_selected_action(Action::MapLocal);
        assert_eq!(
            current(&hud).decoded.frame.pixels,
            pressed.decoded.frame.pixels
        );
        assert!(hud.press(point));
        assert_eq!(hud.release(point), Some(Action::MapLocal));
    }

    #[test]
    fn packaged_theme_replaces_layout_font_and_hit_regions() {
        let package = Package::new();
        let mut first = package.theme(EXAMPLE).unwrap();
        first.layout((1920, 1080));
        assert_eq!(first.hit((40, 65)), Hit::Button(Action::Pause));
        assert_eq!(first.hit((21, 22)), Hit::Panel);
        first.advance_focus(false);
        assert_eq!(first.focused_action(), Some(Action::Pause));
        first.advance_focus(true);
        assert_eq!(first.focused_action(), Some(Action::ResetZoom));
        let original_text = first.components[1].art.decoded.frame.pixels.clone();
        first.adjust_scale(25);
        assert_eq!(first.placed[0].as_ref().unwrap().rect.x, 21);

        let replaced = EXAMPLE
            .replace("top-left", "top-right")
            .replace("offset = [12, 12]", "offset = [-12, 12]")
            .replace("size = 10", "size = 12");
        let mut replacement = package.theme(&replaced).unwrap();
        replacement.layout((800, 600));
        assert_ne!(
            replacement.components[1].art.decoded.frame.pixels,
            original_text
        );
        assert_eq!(replacement.placed[0].as_ref().unwrap().rect.x, 578);
        assert_eq!(replacement.hit((40, 65)), Hit::World);
        assert_eq!(replacement.hit((640, 65)), Hit::Button(Action::Pause));
    }

    #[test]
    fn theme_reports_bad_references_and_clips_children_and_input() {
        let package = Package::new();
        assert!(parse("name = 'bad'\nsacle = 100").is_err());
        assert!(
            package
                .theme(&EXAMPLE.replace("parent = \"toolbar\"", "parent = \"missing\""))
                .is_err()
        );
        assert!(
            package
                .theme(&EXAMPLE.replace("source = \"bitmap\"", "source = \"file:missing.art\""))
                .is_err()
        );
        let mut clipped = package
            .theme(&EXAMPLE.replace("size = [136, 97]", "size = [40, 30]"))
            .unwrap();
        clipped.layout((800, 600));
        assert_eq!(clipped.hit((40, 61)), Hit::Button(Action::Pause));
        assert_eq!(clipped.hit((40, 65)), Hit::World);
        assert_eq!(clipped.hit((79, 61)), Hit::World);
        let mut hidden = package
            .theme(&EXAMPLE.replace("capture = true", "capture = true\nhidden = true"))
            .unwrap();
        hidden.layout((800, 600));
        assert_eq!(hidden.draws(false).count(), 0);
        assert_eq!(hidden.hit((40, 61)), Hit::World);
        hidden.advance_focus(false);
        assert_eq!(hidden.focused_action(), None);
    }

    #[test]
    fn package_art_is_explicit_and_installed_art_uses_vfs_precedence() {
        fn pixel_art(red: u8) -> Vec<u8> {
            let mut bytes = vec![0; 132 + 1024 + 28];
            let frame = 132 + 1024;
            for (offset, value) in [
                (0, 5_i32),
                (8, 8),
                (12, 10),
                (28, 1),
                (32, 1),
                (frame, 1),
                (frame + 4, 1),
                (frame + 8, 1),
            ] {
                bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            }
            bytes[138] = red;
            bytes.push(1);
            bytes
        }
        let source = Package::new();
        let overlay = Package::new();
        let package = Package::new();
        for (dir, red) in [(&source, 10), (&overlay, 20), (&package, 30)] {
            fs::write(dir.0.join("image.art"), pixel_art(red)).unwrap();
        }
        let mut game = Vfs::new(Policy::Ordered);
        game.mount_directory(&source.0).unwrap();
        game.mount_directory(&overlay.0).unwrap();
        let mut files = Vfs::new(Policy::Ordered);
        files.mount_directory(&package.0).unwrap();
        for (path, red) in [("vfs:image.art", 20), ("file:image.art", 30)] {
            let asset = resource(&mut game, &mut files, path, 0).unwrap();
            assert_eq!(asset.decoded.frame.pixels, [[red, 0, 0, 255]]);
        }
        assert!(resource(&mut game, &mut files, "file:../image.art", 0).is_err());
        let font = Font {
            source: "file:image.art".into(),
            size: 5,
            color: [255; 4],
            bold: false,
            line_gap: 0,
            fallback: None,
        };
        assert!(font_glyph(&mut game, &mut files, &font, '\u{1f}').is_ok());
        assert!(font_glyph(&mut game, &mut files, &font, 'A').is_err());
    }
}
