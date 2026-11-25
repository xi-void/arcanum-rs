//! Declarative HUD layout, assets and hit testing, in physical window pixels.
use arcanum_rs::{
    art::{DecodedFrame, SpriteFrame},
    compositor::SpriteDraw,
    content::{
        Source,
        art::{LoadedFrame, load_frame},
    },
    vfs::{MountId, Policy, Storage, Vfs},
};
use serde::Deserialize;
use std::{collections::BTreeMap, error::Error, fs::File, io::Read, path::Path, sync::Arc};

type Color = [u8; 4];

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(super) enum Action {
    Pause,
    ToggleRun,
    ResetZoom,
    OpenInventory,
    InventoryDrop,
    InventoryClose,
    OpenMap,
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
    ContainerName,
    ContainerGrid,
    PlayerGrid,
    InventoryDetail,
    InventoryDrop,
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
}

impl Binding {
    fn is_label(self) -> bool {
        matches!(
            self,
            Self::InventoryDetail
                | Self::ContainerName
                | Self::MapTitle
                | Self::MapWest
                | Self::MapSouth
                | Self::MapStatus
                | Self::PlaceLabel
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
    clock_phases: Option<[String; 8]>,
    source_rect: Option<[u32; 4]>,
    #[serde(default)]
    frame: usize,
    fill: Option<Color>,
    grid_line: Option<Color>,
    hover_fill: Option<Color>,
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
    state_frames: [Option<SpriteFrame>; 3],
    text: Option<Text>,
    label: Option<Label>,
    clock: Option<ClockArt>,
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
}

impl Label {
    fn text(&self, value: &str) -> Text {
        let glyphs = value
            .chars()
            .take(128)
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
    art: Arc<LoadedFrame>,
    state_art: [Option<Arc<LoadedFrame>>; 3],
}

pub(crate) struct Hud {
    components: Vec<Component>,
    font_profiles: BTreeMap<String, serde_json::Value>,
    placed: Vec<Option<Placement>>,
    size: (u32, u32),
    scale: u16,
    fit_viewport: Option<[u32; 2]>,
    tooltip_anchor: (i32, i32),
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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Hit {
    World,
    Panel,
    Button(Action),
}

impl Hud {
    pub fn load(path: &Path, game: &mut Vfs, scale: Option<u16>) -> Result<Self, Box<dyn Error>> {
        let mut source = String::new();
        File::open(path)?
            .take(128 * 1024 + 1)
            .read_to_string(&mut source)?;
        let theme = parse(&source)?;
        let mut files = Vfs::new(Policy::Ordered);
        files.mount_directory(
            path.parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new(".")),
        )?;
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
            let mut base = sprite(config.size, config.fill.unwrap_or([0; 4]));
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
                        let mut raster = sprite(config.size, config.fill.unwrap_or([0; 4]));
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
                            return Err(
                                format!("{}: map label font needs a '?' glyph", config.id).into()
                            );
                        }
                        label = Some(Label {
                            glyphs: alphabet,
                            font: font.clone(),
                            native_height,
                            value: String::new(),
                        });
                    }
                    prepared_text = Some(Text {
                        glyphs,
                        font: font.clone(),
                        native_height,
                    });
                }
            }
            components.push(Component {
                config,
                parent,
                base,
                state_frames,
                text: prepared_text,
                label,
                clock,
            });
        }
        if let Some(author) = &theme.author {
            eprintln!("HUD author: {author}");
        }
        eprintln!("HUD {} from {} at {}%", theme.name, path.display(), scale);
        Ok(Self {
            components,
            font_profiles,
            placed: Vec::new(),
            size: (0, 0),
            scale,
            fit_viewport: theme.fit_viewport,
            tooltip_anchor: (0, 0),
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
        })
    }

    pub fn layout(&mut self, size: (u32, u32)) {
        if size == self.size {
            return;
        }
        self.size = size;
        self.placed.clear();
        let viewport = Rect {
            x: 0,
            y: 0,
            w: size.0,
            h: size.1,
        };
        let widest = self
            .components
            .iter()
            .filter(|c| c.parent.is_none() && !c.config.hidden)
            .map(|c| c.config.size[0])
            .max()
            .unwrap_or(1);
        // Keep wide stock panels usable on narrow windows. Both hit testing and
        // raster placement use this exact effective scale.
        let fit = self.fit_viewport.map_or(u32::MAX, |[width, height]| {
            (size.0.saturating_mul(100) / width).min(size.1.saturating_mul(100) / height)
        });
        let scale = u32::from(self.scale)
            .min(size.0.saturating_mul(100) / widest)
            .min(fit)
            .max(1);
        let mut bounds = Vec::with_capacity(self.components.len());
        for component in &self.components {
            let parent = component
                .parent
                .map_or(Some((viewport, viewport)), |id| bounds[id]);
            let placement = parent.and_then(|(parent, clip)| {
                if self.map_view.is_some_and(|mode| {
                    !component.config.map_views.is_empty()
                        && !component.config.map_views.contains(&mode)
                }) || component.config.hidden
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
            self.placed.push(placement.map(|(rect, clip)| {
                let art = raster(component, &component.base, rect, clip, scale, None);
                let state_art = std::array::from_fn(|i| {
                    if component.config.disabled || component.config.action.is_none() {
                        return None;
                    }
                    if let Some(frame) = &component.state_frames[i] {
                        return Some(raster(component, frame, rect, clip, scale, None));
                    }
                    if i == 0 {
                        return component.config.hover_fill.map(|color| {
                            raster(component, &component.base, rect, clip, scale, Some(color))
                        });
                    }
                    None
                });
                Placement {
                    full: rect,
                    scale,
                    rect: clip,
                    art,
                    state_art,
                }
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
        serde_json::json!({"scale_percent":self.scale,"fonts":self.font_profiles,"image_references":images,
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

    pub(super) fn set_lens_asset(&mut self, frame: Arc<LoadedFrame>) {
        if let Some(index) = self.bound(Binding::PlayerLens)
            && let Some(Some(place)) = self.placed.get_mut(index)
        {
            place.art = frame;
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

    pub(super) fn bounds(&self, binding: Binding) -> Option<(Rect, u32)> {
        let place = self.placed.get(self.bound(binding)?)?.as_ref()?;
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
        if let Some(index) = self.bound(Binding::MapCanvas)
            && let Some(place) = self.placed[index].as_mut()
        {
            place.art = generated_frame(&self.components[index].config.id, frame);
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
        if let Some(Some(place)) = self.placed.get_mut(index) {
            place.art = raster(
                component,
                &component.base,
                place.full,
                place.rect,
                place.scale,
                None,
            );
        }
        true
    }

    pub(super) fn set_label(&mut self, binding: Binding, value: &str) {
        if let Some(index) = self.bound(binding)
            && let Some(label) = &mut self.components[index].label
            && label.value != value
        {
            label.value = value.into();
            self.components[index].text = Some(label.text(value));
            if let Some(place) = self.placed.get_mut(index).and_then(Option::as_mut) {
                place.art = raster(
                    &self.components[index],
                    &self.components[index].base,
                    place.full,
                    place.rect,
                    place.scale,
                    None,
                );
            }
        }
    }

    pub(super) fn paint_surface(
        &mut self,
        binding: Binding,
        paint: impl FnOnce(&mut SpriteFrame, Color, Color, Color),
    ) {
        let Some(i) = self.bound(binding) else {
            return;
        };
        let c = &mut self.components[i];
        let fill = c.config.fill.unwrap_or([0; 4]);
        paint(
            &mut c.base,
            fill,
            c.config.grid_line.unwrap_or(fill),
            c.config.hover_fill.unwrap_or(fill),
        );
        if let Some(p) = self.placed.get_mut(i).and_then(Option::as_mut) {
            p.art = raster(c, &c.base, p.full, p.rect, p.scale, None);
        }
    }
    pub(super) fn tooltip(&mut self, point: (u32, u32), text: &str) {
        let binding = Binding::InventoryDetail;
        self.set_label(binding, text);
        if let Some(i) = self.bound(binding)
            && let Some(p) = self.placed[i].as_ref()
        {
            self.tooltip_anchor.0 = point
                .0
                .saturating_add(14)
                .min(self.size.0.saturating_sub(p.rect.w))
                as i32;
            self.tooltip_anchor.1 = point
                .1
                .saturating_add(15)
                .min(self.size.1.saturating_sub(p.rect.h))
                as i32;
        }
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
        self.layout(size);
    }

    fn select(&self, point: (u32, u32)) -> Option<usize> {
        self.placed
            .iter()
            .enumerate()
            .rev()
            .find_map(|(index, placement)| {
                let config = &self.components[index].config;
                (placement.as_ref()?.rect.contains(point)
                    && (config.capture || config.action.is_some()))
                .then_some(index)
            })
    }
    pub(super) fn hit(&self, point: (u32, u32)) -> Hit {
        self.select(point).map_or(Hit::World, |index| {
            self.available_action(index).map_or(Hit::Panel, Hit::Button)
        })
    }
    pub fn hover(&mut self, point: Option<(u32, u32)>) -> bool {
        let hover = point
            .and_then(|point| self.select(point))
            .filter(|&index| self.available_action(index).is_some());
        std::mem::replace(&mut self.hover, hover) != hover
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

    pub(super) fn cancel_press(&mut self) {
        self.pressed = None;
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
        config
            .action
            .filter(|action| !config.disabled && Some(*action) != self.unavailable_action)
    }

    pub fn draws(
        &self,
        running: bool,
    ) -> impl Iterator<Item = (&Arc<LoadedFrame>, SpriteDraw<'_>)> {
        self.placed
            .iter()
            .enumerate()
            .filter_map(move |(index, placement)| {
                let p = placement.as_ref()?;
                if self.components[index].config.binding == Some(Binding::InventoryDetail)
                    && self.components[index]
                        .label
                        .as_ref()
                        .is_none_or(|l| l.value.is_empty())
                {
                    return None;
                }
                let action = self.components[index].config.action;
                let hot = self.hover == Some(index);
                let state = if hot && self.pressed == Some(index) {
                    p.state_art[1].as_ref()
                } else {
                    None
                };
                let selected = (action.is_some()
                    && (action == self.selected_action || action == self.active_action))
                    .then_some(p.state_art[2].as_ref())
                    .flatten();
                let highlighted = (hot
                    || self.focus == Some(index)
                    || (running && action == Some(Action::ToggleRun)))
                .then_some(p.state_art[0].as_ref())
                .flatten();
                let asset = state.or(selected).or(highlighted).unwrap_or(&p.art);
                Some((
                    asset,
                    SpriteDraw {
                        frame: &asset.decoded.frame,
                        anchor: if self.components[index].config.binding
                            == Some(Binding::InventoryDetail)
                        {
                            self.tooltip_anchor
                        } else {
                            (p.rect.x, p.rect.y)
                        },
                    },
                ))
            })
    }
}

fn parse(source: &str) -> Result<Theme, Box<dyn Error>> {
    if source.len() > 128 * 1024 {
        return Err("theme exceeds 128 KiB".into());
    }
    let theme: Theme = toml::from_str(source)?;
    if theme.name.trim().is_empty()
        || theme.hud.len() > 64
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
            || e.text.as_ref().is_some_and(|s| s.len() > 256)
        {
            return Err(format!("invalid size, offset or text in component {}", e.id).into());
        }
        if matches!(
            e.binding,
            Some(Binding::PlayerGrid | Binding::ContainerGrid)
        ) && (e.size[0] < 10 || e.size[0] % 10 != 0 || !e.size[1].is_multiple_of(e.size[0] / 10))
        {
            return Err(format!("{} requires ten equal columns and whole rows", e.id).into());
        }
        let variants = [e.hover_frame, e.pressed_frame, e.selected_frame]
            .into_iter()
            .flatten()
            .count() as u32;
        pixels += e.size[0] * e.size[1] * (1 + variants);
    }
    if pixels > 768 * 1024
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

fn resource(
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
pub(super) fn sprite([width, height]: [u32; 2], color: Color) -> SpriteFrame {
    SpriteFrame {
        width,
        height,
        hotspot: (0, 0),
        offset: (0, 0),
        pixels: vec![color; (width * height) as usize],
    }
}
pub(super) fn scale_into(
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
        let y = to_pixels(line * line_step);
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
            let x = origin + scale_glyph(pen);
            let w = (origin + scale_glyph(pen + glyph.width) - x).max(1);
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
                    blend_glyph(&mut frame.pixels[at], font.color, alpha);
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
    path: &Path,
    source: &str,
) -> Result<BTreeMap<char, SpriteFrame>, Box<dyn Error>> {
    let mut files = Vfs::new(Policy::Ordered);
    files.mount_directory(
        path.parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new(".")),
    )?;
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
            Hud::load(&path, &mut Vfs::new(Policy::Ordered), None)
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
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("ui/classic/theme.toml");
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

        for scale in [100, 125, 150, 200] {
            let mut hud = Hud::load(&path, &mut game, Some(scale)).unwrap();
            hud.layout((1920, 1080));
            assert_eq!(hud.draws(false).count(), 33);
            let left = (1920 - 8 * u32::from(scale)) / 2;
            let button = hud.placed[2].as_ref().unwrap().rect;
            assert_eq!(button.x as u32, left + 4 * u32::from(scale) / 100);
            assert_eq!(button.y as u32, 2 * u32::from(scale) / 100);
            assert_eq!(button.w, 34 * u32::from(scale) / 100);
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
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("ui/classic-map/theme.toml");
        let mut hud = Hud::load(&path, &mut game, Some(125)).unwrap();
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
                    .find(|(art, _)| art.source.virtual_path == "ui/zoom")
                    .unwrap()
                    .0,
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
        let original_text = first.placed[1]
            .as_ref()
            .unwrap()
            .art
            .decoded
            .frame
            .pixels
            .clone();
        first.adjust_scale(25);
        assert_eq!(first.placed[0].as_ref().unwrap().rect.x, 21);

        let replaced = EXAMPLE
            .replace("top-left", "top-right")
            .replace("offset = [12, 12]", "offset = [-12, 12]")
            .replace("size = 10", "size = 12");
        let mut replacement = package.theme(&replaced).unwrap();
        replacement.layout((800, 600));
        assert_ne!(
            replacement.placed[1]
                .as_ref()
                .unwrap()
                .art
                .decoded
                .frame
                .pixels,
            original_text
        );
        assert_eq!(replacement.placed[0].as_ref().unwrap().rect.x, 578);
        assert_eq!(replacement.hit((40, 65)), Hit::World);
        assert_eq!(replacement.hit((640, 65)), Hit::Button(Action::Pause));
    }

    #[test]
    fn tooltip_retext_after_moving_or_resizing_preserves_source_bounds() {
        let pkg = Package::new();
        let mut hud = pkg
            .theme(
                r#"
name="tips"
[fonts.body]
source="bitmap"
size=12
color=[255,255,255,255]
[[hud]]
id="tip"
binding="inventory-detail"
font="body"
size=[120,40]
"#,
            )
            .unwrap();
        for scale in [100, 125, 200] {
            hud.set_scale(scale);
            hud.layout((380, 220));
            hud.tooltip((360, 210), "First");
            hud.tooltip((13, 9), "Second");
            let (_, draw) = hud.draws(false).next().unwrap();
            assert_eq!(draw.anchor, (27, 24));
            hud.tooltip((20, 10), "");
            assert_eq!(hud.draws(false).count(), 0);
        }
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
