//! Declarative HUD layout, assets and hit testing, in physical window pixels.
mod dialogue;
mod inventory;
mod journal;
pub(super) mod sheet;
use super::{ActiveEventLoop, Duration, ThemeSource, Window};
use arcanum_rs::{
    art::{DecodedFrame, SpriteFrame},
    content::{
        Source,
        art::{LoadedFrame, load_frame},
        world::InteractiveWorld,
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
    ToggleCombat,
    EndTurn,
    ResetZoom,
    OpenMap,
    ToggleInventory,
    ToggleSpells,
    CastCantrip, // Existing external packages may still bind this action.
    CastSpell(usize),
    DismissSpell(usize),
    SpellCollege(usize),
    ToggleJournal,
    ToggleCharacter,
    ToggleSchematics,
    Sheet(sheet::Control),
    JournalQuests,
    JournalRumors,
    JournalBackground,
    JournalPrevious,
    JournalNext,
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
    spell_cursor: Option<String>,
    #[serde(default)]
    cursors: BTreeMap<CursorMode, String>,
    #[serde(default = "default_scale")]
    scale: u16,
    fit_viewport: Option<[u32; 2]>,
    route_color: Option<Color>,
    selected_route_color: Option<Color>,
    journal: Option<journal::Style>,
    sheet: Option<sheet::Style>,
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
    #[serde(default)]
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
    JournalLens,
    SheetLens,
    SchematicLens,
    SheetValue,
    HealthFill,
    HealthValue,
    JournalLeft,
    JournalRight,
    JournalLeftHeader,
    JournalRightHeader,
    JournalLeftFooter,
    JournalRightFooter,
    InventoryHover,
    NpcContext,
    NpcHealth,
    NpcFatigue,
    NpcPoison,
    NpcHealthEmpty,
    NpcFatigueEmpty,
    NpcPoisonEmpty,
    NpcHpText,
    NpcFtText,
    DialogueText,
    DialogueResponse,
    InventoryGold,
    MerchantName,
    MerchantGold,
    MerchantPortrait,
    MerchantOffer,
    InventoryArrows,
    InventoryBullets,
    InventoryCharges,
    InventoryFuel,
    InventoryWeight,
    InventoryBurden,
    InventorySpeed,
    InventoryDefense,
    InventoryDrop,
    InventoryUse,
    FatigueFill,
    FatigueValue,
}

impl Binding {
    fn is_label(self) -> bool {
        matches!(
            self,
            Self::SheetValue
                | Self::HealthValue
                | Self::FatigueValue
                | Self::MapTitle
                | Self::MapWest
                | Self::MapSouth
                | Self::MapStatus
                | Self::PlaceLabel
                | Self::NpcHpText
                | Self::NpcFtText
                | Self::NpcContext
                | Self::DialogueText
                | Self::DialogueResponse
                | Self::JournalLeft
                | Self::JournalRight
                | Self::JournalLeftHeader
                | Self::JournalRightHeader
                | Self::JournalLeftFooter
                | Self::JournalRightFooter
                | Self::InventoryHover
                | Self::MerchantName
                | Self::MerchantOffer
                | Self::MerchantGold
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
    turn_panel: bool,
    #[serde(default)]
    ap_offsets: Vec<i32>,
    #[serde(default)]
    inventory_panel: bool,
    #[serde(default)]
    journal_panel: bool,
    #[serde(default)]
    sheet_panel: bool,
    sheet_page: Option<sheet::Page>,
    sheet_value: Option<sheet::Value>,
    sheet_image: Option<sheet::Image>,
    aptitude_travel: Option<i32>,
    skill_gauge: Option<usize>,
    #[serde(default)]
    dialogue_only: bool,
    #[serde(default)]
    spell_panel: bool,
    required_conveyance: Option<u32>,
    spell_slot: Option<usize>,
    maintained_slot: Option<usize>,
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

impl Component {
    fn update_text_states(&mut self, rasterize: impl Fn(&Self) -> Arc<LoadedFrame>) {
        let Some(normal) = self.text.as_ref().map(|text| text.font.color) else {
            return;
        };
        for (variant, color) in [
            (0, self.config.hover_text_color),
            (2, self.config.disabled_text_color),
        ] {
            if let Some(color) = color {
                self.text.as_mut().unwrap().font.color = color;
                self.state_art[variant] = Some(rasterize(self));
            }
        }
        self.text.as_mut().unwrap().font.color = normal;
    }

    fn text_height(&self, width: u32) -> u32 {
        let shadow = u32::from(self.config.text_shadow.is_some());
        self.text
            .as_ref()
            .filter(|text| !text.glyphs.is_empty())
            .map_or(0, |text| {
                text.height(width.saturating_sub(shadow), self.config.wrap, 100) + shadow
            })
    }
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

#[derive(Debug, PartialEq, Eq)]
struct TextLine {
    glyphs: std::ops::Range<usize>,
    width: u32,
    top: u32,
    height: u32,
}

impl Text {
    fn size(&self) -> u32 {
        if self.font.size == 0 {
            self.native_height
        } else {
            self.font.size
        }
    }

    fn pixels(&self, units: u32, scale: u32) -> u32 {
        self.scale_units(units * self.size(), scale)
    }

    fn scale_units(&self, units: u32, scale: u32) -> u32 {
        let divisor = u64::from(self.native_height * 100);
        ((u64::from(units) * u64::from(scale) + divisor / 2) / divisor) as u32
    }

    fn lines(&self, width: u32, wrap: bool, scale: u32) -> impl Iterator<Item = TextLine> + '_ {
        let (mut start, mut top) = (0, 0);
        std::iter::from_fn(move || {
            if start == self.glyphs.len() {
                return None;
            }
            let (mut end, mut advance, mut ink, mut height, mut space) = (start, 0, 0, 0, None);
            let bold = if self.font.bold {
                (scale / 100).max(1)
            } else {
                0
            };
            while let Some(Some((glyph, ch))) = self.glyphs.get(end) {
                let next = ink.max(advance + glyph.width);
                if wrap && self.pixels(next, scale) + bold > width && end > start {
                    if *ch != ' '
                        && let Some((word_end, word_width, word_height)) = space
                    {
                        (end, ink, height) = (word_end, word_width, word_height);
                    }
                    break;
                }
                if *ch == ' ' && end > start {
                    space = Some((end, ink, height));
                }
                advance += glyph.hotspot.0 as u32;
                ink = next;
                height = height.max(glyph.height);
                end += 1;
            }
            let height = height.max(self.native_height) * self.size();
            let to_pixels = |value| self.scale_units(value, scale);
            let line = TextLine {
                glyphs: start..end,
                width: self.pixels(ink, scale) + if ink > 0 { bold } else { 0 },
                top: to_pixels(top),
                height: to_pixels(top + height) - to_pixels(top),
            };
            top += height + self.font.line_gap * self.native_height;
            start = end;
            if self
                .glyphs
                .get(start)
                .is_some_and(|g| g.as_ref().is_none_or(|(_, ch)| *ch == ' '))
            {
                start += 1;
            }
            Some(line)
        })
    }

    fn height(&self, width: u32, wrap: bool, scale: u32) -> u32 {
        self.lines(width, wrap, scale)
            .last()
            .map_or(0, |line| line.top + line.height)
    }
}

struct Label {
    glyphs: BTreeMap<char, Arc<SpriteFrame>>,
    font: Font,
    native_height: u32,
    value: String,
    raster_slice: Option<(u32, u32)>,
}

impl Label {
    fn text(&self, value: &str) -> Text {
        Text::new(&self.glyphs, self.font.clone(), self.native_height, value)
    }
}

impl Text {
    fn new(
        alphabet: &BTreeMap<char, Arc<SpriteFrame>>,
        font: Font,
        native_height: u32,
        value: &str,
    ) -> Self {
        let glyphs = value
            .chars()
            .filter(|ch| *ch != '\r')
            .take(4096)
            .map(|ch| {
                if ch == '\n' {
                    None
                } else {
                    let ch = if ch == '\t' { ' ' } else { ch };
                    Some((Arc::clone(alphabet.get(&ch).unwrap_or(&alphabet[&'?'])), ch))
                }
            })
            .collect();
        Self {
            glyphs,
            font,
            native_height,
        }
    }
}

pub(super) struct MenuFont {
    glyphs: BTreeMap<char, Arc<SpriteFrame>>,
    font: Font,
    native_height: u32,
}

impl MenuFont {
    pub(super) fn load(
        game: &mut Vfs,
        theme: &ThemeSource,
        source: &str,
        size: u32,
    ) -> Result<Self, Box<dyn Error>> {
        if size != 0 && !(5..=64).contains(&size) {
            return Err("menu font size must be native (0) or 5–64 pixels".into());
        }
        let glyphs = load_glyphs(game, theme, source)?;
        if !glyphs.contains_key(&'?') {
            return Err(format!("{source}: menu font needs a '?' glyph").into());
        }
        let native_height = glyphs.get(&' ').unwrap_or(&glyphs[&'?']).height;
        let glyphs = glyphs
            .into_iter()
            .map(|(ch, glyph)| (ch, Arc::new(glyph)))
            .collect();
        let font = Font {
            source: source.into(),
            size,
            color: [255; 4],
            bold: false,
            line_gap: 0,
            fallback: None,
        };
        Ok(Self {
            glyphs,
            font,
            native_height,
        })
    }

    pub(super) fn line_height(&self) -> u32 {
        if self.font.size == 0 {
            self.native_height
        } else {
            self.font.size
        }
    }

    pub(super) fn visible_suffix<'a>(&self, value: &'a str, width: u32) -> &'a str {
        let mut suffix = value.len();
        let mut extent = None;
        for (start, ch) in value.char_indices().rev() {
            let glyph = self.glyphs.get(&ch).unwrap_or(&self.glyphs[&'?']);
            let ink = glyph
                .width
                .max(extent.map_or(0, |right| right + glyph.hotspot.0 as u32));
            if (ink * self.line_height() + self.native_height / 2) / self.native_height > width {
                break;
            }
            extent = Some(ink);
            suffix = start;
        }
        &value[suffix..]
    }

    pub(super) fn render(
        &self,
        value: &str,
        bounds: [u32; 2],
        color: Color,
        wrap: bool,
    ) -> SpriteFrame {
        let text = Text::new(&self.glyphs, self.font.clone(), self.native_height, value);
        let width = text
            .lines(bounds[0], wrap, 100)
            .map(|line| line.width)
            .max()
            .unwrap_or(0)
            .clamp(1, bounds[0].max(1));
        let height = text.height(bounds[0], wrap, 100).clamp(1, bounds[1].max(1));
        let width = if wrap { bounds[0].max(1) } else { width };
        let rect = Rect {
            x: 0,
            y: 0,
            w: width,
            h: height,
        };
        let mut frame = sprite([width, height], [0; 4]);
        let align = if wrap { Align::Center } else { Align::Left };
        draw_text_colored(
            &mut frame,
            &text,
            (align, wrap, 0),
            rect,
            rect,
            100,
            (color, 0),
        );
        frame
    }
}

struct Placement {
    full: Rect,
    scale: u32,
    rect: Rect,
    live_asset: Option<Arc<LoadedFrame>>,
}

#[derive(Clone, Copy, Debug, Deserialize, serde::Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
pub(super) enum CursorMode {
    Default,
    Spell,
    Skill,
    Combat,
}
struct CursorClip {
    path: String,
    winner: String,
    hotspots: Vec<[u16; 2]>,
    period: Duration,
    sources: Vec<winit::window::CustomCursorSource>,
    handles: Vec<winit::window::CustomCursor>,
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
    combat_ap: Option<i32>,
    npc_gauges: Option<([u32; 2], bool)>,
    cursors: BTreeMap<CursorMode, CursorClip>,
    cursor_frame: Option<(CursorMode, usize)>,
    cursor_hidden: bool,
    effective_scale: u32,
    panel_band: Rect,
    external_panel: Option<[u32; 2]>,
    shared_band: Option<(Rect, u32)>,
    sheet: sheet::Sheet,
    sheet_style: Option<sheet::Style>,
    health_percent: u32,
    fatigue_percent: u32,
    fatigue_revision: Option<u64>,
    spells_open: bool,
    college_levels: [u32; 16],
    maintained: Vec<usize>,
    spell_college: usize,
    loaded_college: Option<usize>,
    inventory: InventoryPanel,
    journal: journal::Journal,
    journal_style: Option<journal::Style>,
    dialogue_open: bool,
    dialogue_layout: dialogue::DialogueLayout,
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
                                .map_or(font.size.max(1), |(g, _)| g.height)
                        },
                        |space| space.height,
                    );
                    if dynamic {
                        let mut alphabet = BTreeMap::new();
                        for ch in font_characters() {
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
                            raster_slice: None,
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
        let mut paths = theme.cursors;
        if let Some(path) = theme.spell_cursor {
            paths.insert(CursorMode::Spell, path);
        }
        let mut cursors = BTreeMap::new();
        for (mode, path) in paths {
            let first = resource(game, &mut files, &path, 0)?;
            let count = first.decoded.frame_count;
            let period =
                Duration::from_millis(1000 / first.decoded.frames_per_second.clamp(1, 1000) as u64);
            let winner = first.source.to_string();
            let mut sources = Vec::new();
            let mut hotspots = Vec::new();
            for i in 0..count {
                let frame = resource(game, &mut files, &path, i)?.decoded.frame;
                let x = frame.hotspot.0.clamp(0, frame.width as i32 - 1) as u16;
                let y = frame.hotspot.1.clamp(0, frame.height as i32 - 1) as u16;
                hotspots.push([x, y]);
                sources.push(winit::window::CustomCursor::from_rgba(
                    frame.pixels.into_flattened(),
                    frame.width.try_into()?,
                    frame.height.try_into()?,
                    x,
                    y,
                )?);
            }
            cursors.insert(
                mode,
                CursorClip {
                    path,
                    winner,
                    hotspots,
                    period,
                    sources,
                    handles: vec![],
                },
            );
        }
        Ok(Self {
            cursors,
            cursor_frame: None,
            cursor_hidden: false,
            npc_gauges: None,
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
            combat_ap: None,
            effective_scale: 0,
            panel_band: Rect {
                x: 0,
                y: 0,
                w: 0,
                h: 0,
            },
            external_panel: None,
            shared_band: None,
            sheet: sheet::Sheet::default(),
            sheet_style: theme.sheet,
            health_percent: 100,
            fatigue_percent: 100,
            fatigue_revision: None,
            spells_open: false,
            college_levels: [0; 16],
            maintained: Vec::new(),
            spell_college: 0,
            loaded_college: None,
            inventory: InventoryPanel::default(),
            journal: journal::Journal::default(),
            journal_style: theme.journal,
            dialogue_open: false,
            dialogue_layout: dialogue::DialogueLayout::default(),
            dialogue_available: [false; 5],
        })
    }

    pub fn reset_character(&mut self) {
        self.close_sheet();
        self.fatigue_revision = None;
        self.close_spells();
    }
    pub fn update_fatigue(&mut self, world: &InteractiveWorld) {
        if self.fatigue_revision == Some(world.inventory_revision()) {
            return;
        }
        self.fatigue_revision = Some(world.inventory_revision());
        let stats = world.character_statistics();
        let (current, maximum) = (stats.current_fatigue, stats.max_fatigue);
        self.health_percent = (stats.current_health.saturating_mul(100) / stats.max_health.max(1))
            .clamp(0, 100) as u32;
        self.set_label(
            Binding::HealthValue,
            &format!("{:03}", stats.current_health),
        );
        self.fatigue_percent = if maximum <= 0 {
            0
        } else {
            (current.saturating_mul(100) / maximum).clamp(0, 100) as u32
        };
        self.set_label(Binding::FatigueValue, &format!("{current:03}"));
    }
    pub fn toggle_spells(&mut self) {
        self.spells_open = !self.spells_open;
        self.clear_layout();
    }
    pub fn close_spells(&mut self) -> bool {
        if !self.spells_open {
            return false;
        }
        self.toggle_spells();
        true
    }
    pub fn choose_college(&mut self, i: usize) {
        if self.college_levels.get(i).is_some_and(|&n| n > 0) {
            self.spell_college = i;
            self.clear_layout();
        }
    }
    pub fn refresh_spellbook(
        &mut self,
        reader: &mut arcanum_rs::content::world::InventoryReader,
        world: &InteractiveWorld,
    ) {
        if !self
            .maintained
            .iter()
            .copied()
            .eq(world.maintained_spells())
        {
            self.maintained = world.maintained_spells().collect();
            if let Some(style) = &self.sheet_style {
                let rules = world.progression_rules();
                for component in &mut self.components {
                    let Some((slot, id)) = component
                        .config
                        .maintained_slot
                        .and_then(|n| Some((n, *self.maintained.get(n)?)))
                    else {
                        continue;
                    };
                    let art = style.spell_icon_bases[id / 5] + 1 + (id % 5) as i32;
                    component.config.action = Some(Action::DismissSpell(slot));
                    component.config.hover_message = Some(format!("End {}", rules.spells[id].name));
                    if let Some(path) = rules.interface.get(&art)
                        && let Ok(frame) = reader.sheet_art(path, 0)
                    {
                        component.art = frame
                    }
                }
            }
            self.clear_layout();
        }
        let levels = world.college_levels();
        if levels != self.college_levels {
            self.college_levels = levels;
            if levels[self.spell_college] == 0 {
                self.spell_college = levels.iter().position(|&n| n > 0).unwrap_or(0);
            }
            self.loaded_college = None;
            self.clear_layout();
        }
        if !self.spells_open || self.loaded_college == Some(self.spell_college) {
            return;
        }
        let Some(style) = &self.sheet_style else {
            return;
        };
        let rules = world.progression_rules();
        for c in &mut self.components {
            let Some(row) = c
                .config
                .spell_slot
                .filter(|&r| r < levels[self.spell_college] as usize)
            else {
                continue;
            };
            let id = self.spell_college * 5 + row;
            c.config.action = Some(Action::CastSpell(id));
            c.config.hover_message = rules.spells.get(id).map(|s| s.name.clone());
            let art = style.spell_icon_bases[self.spell_college] + 1 + row as i32;
            if let Some(path) = rules.interface.get(&art) {
                c.art = reader
                    .sheet_art(path, 0)
                    .unwrap_or_else(|_| generated_frame("empty", sprite([1, 1], [0; 4])));
            }
        }
        self.loaded_college = Some(self.spell_college);
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
        let mut turn_bottom = 0;
        let show_turn = self.combat_ap.is_some()
            && !self.spells_open
            && panel.is_none()
            && !self.sheet.open
            && !self.journal.open
            && !self.dialogue_open
            && external_panel.is_none();
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
            if e.turn_panel {
                if show_turn {
                    widest = widest.max(e.size[0]);
                    turn_bottom = turn_bottom.max(e.size[1].saturating_add_signed(-e.offset[1]));
                }
                continue;
            }
            if e.inventory_panel {
                // Reserve the full gameplay layout even while inventory is closed.
                middle = middle.max(e.size[1]);
                widest = widest.max(e.size[0]);
                continue;
            }
            if e.sheet_panel {
                if self.sheet.open {
                    middle = middle.max(e.size[1]);
                    widest = widest.max(e.size[0]);
                }
                continue;
            }
            if e.journal_panel {
                if self.journal.open {
                    middle = middle.max(e.size[1]);
                    widest = widest.max(e.size[0]);
                }
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
                .min(size.1.saturating_mul(100) / (top + (bottom + middle).max(turn_bottom)).max(1))
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
        let bottom = bottom.max(turn_bottom) * scale / 100;
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
                    let area = if component.config.sheet_panel
                        || component.config.inventory_panel
                        || component.config.dialogue_only
                        || component.config.journal_panel
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
                if component.config.turn_panel && !show_turn
                    || !self.sheet.visible(&component.config)
                    || self.map_view.is_some_and(|mode| {
                        !component.config.map_views.is_empty()
                            && !component.config.map_views.contains(&mode)
                    })
                    || (component.config.spell_panel
                        && (!self.spells_open
                            || self.dialogue_open
                            || panel.is_some()
                            || self.journal.open
                            || self.sheet.open))
                    || component
                        .config
                        .required_conveyance
                        .is_some_and(|n| n > self.college_levels[0])
                    || matches!(component.config.action,Some(Action::SpellCollege(i)) if self.college_levels.get(i).copied().unwrap_or(0)==0)
                    || component.config.spell_slot.is_some_and(|r|r>=self.college_levels[self.spell_college] as usize)
                    || component.config.maintained_slot.is_some_and(|i|i>=self.maintained.len())
                    || component.config.hidden
                    || (component.config.dialogue_only && !self.dialogue_open)
                    || (component.config.journal_panel && !self.journal.open)
                    || (component.config.binding == Some(Binding::NpcContext)
                        && (panel.is_some() || self.spells_open))
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
                let mut rect = layout_rect(&component.config, parent, scale);
                if let Some(travel) = component.config.aptitude_travel {
                    rect.y -=
                        (i64::from(self.sheet.aptitude) * i64::from(travel) * i64::from(scale)
                            / 10000) as i32;
                }
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
        let spell_controls: BTreeMap<_, _> = self
            .components
            .iter()
            .zip(&self.placed)
            .filter(|(c, _)| {
                matches!(
                    c.config.action,
                    Some(
                        Action::ToggleSpells
                            | Action::CastCantrip
                            | Action::CastSpell(_)
                            | Action::SpellCollege(_)
                            | Action::DismissSpell(_)
                    )
                )
            })
            .filter_map(|(c, p)| {
                let r = p.as_ref()?.rect;
                Some((&c.config.id, [r.x, r.y, r.w as i32, r.h as i32]))
            })
            .collect();
        serde_json::json!({"character_sheet":self.sheet_diagnostic(),"spell_controls":spell_controls,"spell_panel":self.spells_open,"journal":self.journal_diagnostic(),"context_message":label(Binding::InventoryHover),
            "npc_context":label(Binding::NpcContext),"npc_gauges":self.npc_gauges,"cursor":self.cursor_diagnostic(),"spell_cursor_frame":self.cursor_frame.filter(|(mode,_)| *mode==CursorMode::Spell).map(|(_,i)| i),"dialogue_responses":responses,"dialogue_scroll":self.dialogue_layout.diagnostics(),
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

    pub(super) fn update_cursor(
        &mut self,
        window: &Window,
        event_loop: &ActiveEventLoop,
        mode: CursorMode,
        now: Duration,
    ) {
        let dragging = self.inventory_cursor().is_some();
        if self.cursor_hidden != dragging {
            window.set_cursor_visible(!dragging);
            self.cursor_hidden = dragging;
        }
        let mode = if self.cursors.contains_key(&mode) {
            mode
        } else {
            CursorMode::Default
        };
        if let Some(clip) = self.cursors.get_mut(&mode) {
            clip.handles.extend(
                clip.sources
                    .drain(..)
                    .map(|s| event_loop.create_custom_cursor(s)),
            );
            let i =
                ((now.as_millis() / clip.period.as_millis()) % clip.handles.len() as u128) as usize;
            if self.cursor_frame != Some((mode, i)) {
                window.set_cursor(clip.handles[i].clone());
                self.cursor_frame = Some((mode, i));
            }
        } else if self.cursor_frame.take().is_some() {
            window.set_cursor(winit::window::CursorIcon::Default);
        }
    }
    pub(super) fn cursor_delay(&self) -> Option<Duration> {
        let clip = self.cursors.get(&self.cursor_frame?.0)?;
        (clip.handles.len() > 1).then_some(clip.period)
    }
    fn cursor_diagnostic(&self) -> Option<serde_json::Value> {
        if let Some((asset, grab)) = self.inventory_cursor() {
            return Some(
                serde_json::json!({"mode":"inventory_item","asset":asset,"grab_pixels":grab}),
            );
        }
        let (mode, frame) = self.cursor_frame?;
        let clip = &self.cursors[&mode];
        Some(
            serde_json::json!({"mode":mode,"frame":frame,"asset":clip.path,"winner":clip.winner,"hotspot":clip.hotspots[frame]}),
        )
    }
    pub(super) fn set_npc_gauges(
        &mut self,
        gauges: Option<arcanum_rs::content::world::HoveredVitals>,
    ) {
        self.npc_gauges = gauges.map(|g| (g.percent, g.poisoned));
        if let Some(g) = gauges {
            for (i, binding) in [Binding::NpcHpText, Binding::NpcFtText]
                .into_iter()
                .enumerate()
            {
                let label = if g.allied {
                    format!("{}/{}", g.values[2 * i], g.values[2 * i + 1])
                } else {
                    format!("{}%", g.percent[i])
                };
                self.set_label(binding, &label);
            }
        }
    }
    pub(super) fn set_combat_ap(&mut self, ap: Option<i32>) {
        if self.combat_ap.is_some() != ap.is_some() {
            self.clear_layout();
        }
        self.combat_ap = ap;
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
            label.raster_slice = None;
            self.components[index].text = Some(label.text(value));
            let component = &mut self.components[index];
            component.art = native_art(component, &component.base, None);
            component.update_text_states(|c| native_art(c, &c.base, None));
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
                && (config.spell_slot.is_none()
                    || config
                        .spell_slot
                        .is_some_and(|r| r < self.college_levels[self.spell_college] as usize))
                && Some(*action) != self.unavailable_action
                && (!matches!(
                    action,
                    Action::ToggleSpells | Action::CastCantrip | Action::CastSpell(_)
                ) || !self.inventory_open() && !self.dialogue_open)
                && (!self.sheet.open
                    || matches!(
                        action,
                        Action::ToggleCharacter | Action::ToggleSchematics | Action::Sheet(_)
                    ))
                && (!matches!(action, Action::ToggleCharacter | Action::ToggleSchematics)
                    || self.has_sheet() && !self.dialogue_open)
                && (*action != Action::ToggleInventory || self.has_inventory())
                && (!self.journal.open
                    || matches!(
                        action,
                        Action::ToggleJournal
                            | Action::JournalQuests
                            | Action::JournalRumors
                            | Action::JournalBackground
                            | Action::JournalPrevious
                            | Action::JournalNext
                    ))
                && (*action != Action::ToggleJournal || self.has_journal())
                && self.journal_action_visible(*action)
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
                if component
                    .config
                    .action
                    .is_some_and(|action| !self.journal_action_visible(action))
                {
                    return None;
                }
                if !component.config.ap_offsets.is_empty()
                    || component.config.binding == Some(Binding::ContainerThumb)
                    || (p.live_asset.is_none()
                        && component.config.sheet_image.is_none()
                        && component.config.spell_slot.is_none()
                        && component.config.maintained_slot.is_none()
                        && component.config.image.is_none()
                        && component.config.fill.is_none()
                        && component.config.hover_fill.is_none()
                        && component
                            .text
                            .as_ref()
                            .is_none_or(|text| text.glyphs.is_empty())
                        && !matches!(
                            component.config.binding,
                            Some(Binding::MapCanvas | Binding::JournalLeft | Binding::JournalRight)
                        ))
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
                    && (self.sheet.selected(action)
                        || action == Some(Action::SpellCollege(self.spell_college))
                        || action == self.selected_action
                        || action == self.active_action
                        || (action == Some(Action::ToggleInventory) && self.inventory_open())
                        || ((action == Some(Action::ToggleJournal)
                            || action == Some(self.journal.tab()))
                            && self.journal.open)))
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
                if matches!(
                    component.config.sheet_image,
                    Some(sheet::Image::Component(_))
                ) {
                    return Some(overlay(asset, inventory::equipment_icon(p, asset), p.rect));
                }
                if let Some(
                    b @ (Binding::NpcHealth
                    | Binding::NpcFatigue
                    | Binding::NpcPoison
                    | Binding::NpcHealthEmpty
                    | Binding::NpcFatigueEmpty
                    | Binding::NpcPoisonEmpty
                    | Binding::NpcHpText
                    | Binding::NpcFtText),
                ) = component.config.binding
                {
                    let (pct, poison) = self.npc_gauges?;
                    if self.dialogue_open
                        || self.spells_open
                        || self.inventory_open()
                        || self.sheet.open
                        || self.journal.open
                    {
                        return None;
                    }
                    if matches!(b, Binding::NpcHealth | Binding::NpcHealthEmpty) && poison
                        || matches!(b, Binding::NpcPoison | Binding::NpcPoisonEmpty) && !poison
                    {
                        return None;
                    }
                    if matches!(
                        b,
                        Binding::NpcHealth | Binding::NpcPoison | Binding::NpcFatigue
                    ) {
                        let value = pct[usize::from(b == Binding::NpcFatigue)];
                        let mut clip = p.full;
                        clip.w = (clip.w * value).div_ceil(100);
                        return Some(overlay(asset, p.full, clip.intersect(p.rect)?));
                    }
                }
                if let Some(row) = component.config.skill_gauge {
                    let mut clip = p.rect;
                    clip.w = clip.w * self.sheet.levels.get(row).copied().unwrap_or(0) / 20;
                    return (clip.w > 0).then(|| overlay(asset, p.full, clip));
                }
                if matches!(
                    component.config.binding,
                    Some(Binding::FatigueFill | Binding::HealthFill)
                ) {
                    let percent = if component.config.binding == Some(Binding::HealthFill) {
                        self.health_percent
                    } else {
                        self.fatigue_percent
                    };
                    if percent == 0 {
                        return None;
                    }
                    // CE draws the liquid surface eight original pixels above the fill line.
                    let track = component
                        .parent
                        .and_then(|i| self.placed[i].as_ref())
                        .map_or(p.full.h, |parent| parent.full.h);
                    let empty = track - track * percent / 100;
                    let extra = p.full.h.saturating_sub(track);
                    let rect = Rect {
                        y: p.full.y + empty as i32 - extra as i32,
                        ..p.full
                    };
                    return Some(overlay(asset, rect, p.rect.intersect(rect)?));
                }
                Some(overlay(asset, p.full, p.rect))
            })
            .chain(
                self.components
                    .iter()
                    .enumerate()
                    .filter_map(|(i, component)| {
                        if component.config.ap_offsets.is_empty() {
                            return None;
                        }
                        let place = self.placed.get(i)?.as_ref()?;
                        let parent = self.placed.get(component.parent?)?.as_ref()?;
                        Some(
                            component
                                .config
                                .ap_offsets
                                .iter()
                                .take(self.combat_ap.unwrap_or(0).max(0) as usize)
                                .filter_map(move |offset| {
                                    let rect = Rect {
                                        x: place.full.x + offset * place.scale as i32 / 100,
                                        ..place.full
                                    };
                                    Some(overlay(
                                        &component.art,
                                        rect,
                                        rect.intersect(parent.rect)?,
                                    ))
                                }),
                        )
                    })
                    .flatten(),
            )
            .chain(self.inventory_draws())
    }
}

fn parse(source: &str) -> Result<Theme, Box<dyn Error>> {
    if source.len() > 128 * 1024 {
        return Err("theme exceeds 128 KiB".into());
    }
    let theme: Theme = toml::from_str(source)?;
    if theme.name.trim().is_empty()
        || theme.hud.len() > 512
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
        if e.ap_offsets.iter().any(|n| !(0..=4096).contains(n))
            || (!e.ap_offsets.is_empty() && e.parent.is_none())
            || e.id.is_empty()
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
            || (e.inventory_panel && e.journal_panel)
            || e.sheet_value.is_some() != (e.binding == Some(Binding::SheetValue))
            || ((e.inventory_panel || e.journal_panel || e.sheet_panel)
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
    if pixels > 4 * 1024 * 1024
        || theme.hud.iter().filter(|e| e.inventory_panel).count() > 1
        || theme.hud.iter().filter(|e| e.journal_panel).count() > 1
        || theme.fonts.values().any(|font| {
            (font.size != 0 && !(5..=64).contains(&font.size))
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
                (c.config.align, c.config.wrap, 1),
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

fn font_characters() -> impl Iterator<Item = char> {
    (' '..='~')
        .chain((0xa0..=0xff).map(char::from))
        .chain((0x80..0xa0).filter_map(arcanum_rs::mes::windows_1252))
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
        let byte = (0x80..0xa0)
            .find(|&byte| arcanum_rs::mes::windows_1252(byte) == Some(ch))
            .or_else(|| u8::try_from(ch).ok())
            .ok_or("character is outside the ART font code page")?;
        let frame = usize::from(byte.checked_sub(31).ok_or("invalid font character")?);
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
        (
            element.align,
            element.wrap,
            u32::from(element.text_shadow.is_some()),
        ),
        full,
        clip,
        scale,
        (text.font.color, 0),
    );
}

fn draw_text_colored(
    frame: &mut SpriteFrame,
    text: &Text,
    (align, wrap, padding): (Align, bool, u32),
    full: Rect,
    clip: Rect,
    scale: u32,
    (color, offset): (Color, u32),
) {
    let Text { glyphs, font, .. } = text;
    let scale_glyph = |units| text.pixels(units, scale);
    let visible_x = (clip.x - full.x) as u32;
    let visible_y = (clip.y - full.y) as u32;
    for line in text.lines(full.w.saturating_sub(padding), wrap, scale) {
        let y = offset + line.top;
        if y >= visible_y + frame.height {
            break;
        }
        let bold = if font.bold { (scale / 100).max(1) } else { 0 };
        let space_left = full.w.saturating_sub(padding + line.width);
        let origin = match align {
            Align::Left => 0,
            Align::Center => space_left / 2,
            Align::Right => space_left,
        };
        let mut pen = 0;
        for (glyph, _) in glyphs[line.glyphs].iter().flatten() {
            let x = offset + origin + scale_glyph(pen);
            let w = (offset + origin + scale_glyph(pen + glyph.width) - x).max(1);
            let h = scale_glyph(glyph.height).max(1);
            for py in y.max(visible_y)..(y + h).min(visible_y + frame.height) {
                for px in x.max(visible_x)..(x + w + bold).min(visible_x + frame.width) {
                    let u = px - x;
                    let coverage = |ix| glyph_coverage(glyph, ix, py - y, w, h);
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
    }
}

pub(super) fn glyph_coverage(glyph: &SpriteFrame, x: u32, y: u32, w: u32, h: u32) -> u8 {
    if w >= glyph.width && h >= glyph.height {
        let sx = (2 * x + 1) * glyph.width / (2 * w);
        let sy = (2 * y + 1) * glyph.height / (2 * h);
        glyph.pixels[(sy * glyph.width + sx) as usize][3]
    } else {
        reduced_coverage(glyph, x, y, w, h)
    }
}

fn reduced_coverage(glyph: &SpriteFrame, x: u32, y: u32, w: u32, h: u32) -> u8 {
    let (left, right) = (x * glyph.width, (x + 1) * glyph.width);
    let (top, bottom) = (y * glyph.height, (y + 1) * glyph.height);
    let mut sum = 0;
    for sy in top / h..bottom.div_ceil(h) {
        let dy = bottom.min((sy + 1) * h) - top.max(sy * h);
        for sx in left / w..right.div_ceil(w) {
            let dx = right.min((sx + 1) * w) - left.max(sx * w);
            sum += dx * dy * u32::from(glyph.pixels[(sy * glyph.width + sx) as usize][3]);
        }
    }
    let area = glyph.width * glyph.height;
    ((sum + area / 2) / area) as u8
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

pub(super) fn load_glyphs(
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
    for ch in font_characters() {
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
    fn native_text_wraps_using_drawable_bounds_and_preserves_line_spacing() {
        let mut a = sprite([5, 7], [255; 4]);
        a.hotspot.0 = 3;
        let mut space = sprite([2, 7], [0; 4]);
        space.hotspot.0 = 2;
        let face = BTreeMap::from([
            ('?', Arc::new(a.clone())),
            ('A', Arc::new(a)),
            (' ', Arc::new(space)),
        ]);
        let mut font = parse(EXAMPLE).unwrap().fonts.remove("controls").unwrap();
        font.size = 0;
        font.bold = false;
        font.line_gap = 2;
        let text = Text::new(&face, font, 7, "AA A");
        assert_eq!(
            text.lines(8, true, 100).collect::<Vec<_>>(),
            [
                TextLine {
                    glyphs: 0..2,
                    width: 8,
                    top: 0,
                    height: 7
                },
                TextLine {
                    glyphs: 3..4,
                    width: 5,
                    top: 9,
                    height: 7
                },
            ]
        );
        assert_eq!(text.height(8, true, 100), 16);
        let frame_bounds = Rect {
            x: 0,
            y: 0,
            w: 8,
            h: 16,
        };
        let mut frame = sprite([8, 16], [0; 4]);
        draw_text_colored(
            &mut frame,
            &text,
            (Align::Left, true, 0),
            frame_bounds,
            frame_bounds,
            100,
            ([255; 4], 0),
        );
        assert_eq!(frame.pixels[7][3], 255);
        assert_eq!(frame.pixels[8 * 7][3], 0);
        assert_eq!(frame.pixels[8 * 9][3], 255);
    }

    #[test]
    fn downscaling_retains_coverage_of_a_stroke_between_sample_centers() {
        let mut glyph = sprite([1, 3], [0; 4]);
        glyph.pixels[1] = [255; 4];
        assert_eq!(reduced_coverage(&glyph, 0, 0, 1, 2), 85);
        assert_eq!(reduced_coverage(&glyph, 0, 1, 1, 2), 85);
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
            assert_eq!(hud.draws(false).count(), 35);
            assert_eq!(hud.scale(), scale);
            let effective = u32::from(scale).min(180);
            assert_eq!(hud.effective_scale, effective);
            let left = (1920 - 8 * effective) / 2;
            let button = hud.placed[2].as_ref().unwrap().rect;
            assert_eq!(button.x as u32, left + 4 * effective / 100);
            assert_eq!(button.y as u32, 2 * effective / 100);
            assert_eq!(button.w, 34 * effective / 100);
            assert_eq!(
                hud.hit((button.x as u32, button.y as u32)),
                Hit::Button(Action::ToggleCharacter)
            );
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
            assert_eq!(hud.focused_action(), Some(Action::ToggleCharacter));
            hud.advance_focus(false);
            assert_eq!(hud.focused_action(), Some(Action::ToggleJournal));
            hud.advance_focus(false);
            assert_eq!(hud.focused_action(), Some(Action::OpenMap));
            assert!(hud.bounds(Binding::JournalLeft).is_none());
            hud.toggle_journal();
            hud.layout((1920, 1080));
            let (page, _) = hud.bounds(Binding::JournalLeft).unwrap();
            assert_eq!(hud.hit((page.x as u32, page.y as u32)), Hit::Panel);
            assert!(hud.draws(false).count() > 35);
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
