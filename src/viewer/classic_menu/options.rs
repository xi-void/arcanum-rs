//! Original Options layout and controls, rendered on the shared menu canvas.
use super::*;
use crate::app_config::Audio;

#[derive(Deserialize)]
pub(super) struct OptionsLayout {
    background: String,
    lens: String,
    choice: String,
    slider: String,
    decrease: String,
    increase: String,
    gauge: String,
    heading_font: String,
    #[serde(default)]
    heading_font_size: u32,
    value_font: Option<String>,
    #[serde(default)]
    value_font_size: u32,
    strings: String,
    lens_at: Point,
    nav_center: u32,
    nav_y: [u32; 4],
    nav_width: u32,
    row_x: [u32; 2],
    row_y: [u32; 4],
    arrow_x: [u32; 2],
    arrow_y: u32,
    gauge_at: Point,
    pub(super) heading_color: [u8; 4],
    pub(super) inactive_color: [u8; 4],
    pub(super) value_color: [u8; 4],
    pub(super) nav_color: [u8; 4],
    pub(super) nav_hover_color: [u8; 4],
    pub(super) nav_shadow_color: [u8; 4],
    pub(super) nav_highlight_color: [u8; 4],
}

#[derive(Clone, Copy, Default)]
pub(in crate::viewer) enum Category {
    #[default]
    Game,
    Video,
    Sound,
}

impl Category {
    fn rows(self) -> &'static [Row] {
        match self {
            Self::Game => &[
                Row::Hidden,
                Row::Disabled("Difficulty", "Moderate"),
                Row::Disabled("Violence Filter", "Off"),
                Row::Disabled("Default Combat Mode", "Real-Time"),
                Row::Disabled("Auto Attack", "Off"),
                Row::Disabled("Auto Switch Weapons", "Off"),
                Row::Run,
                Row::Disabled("Follower Skills", "Off"),
            ],
            Self::Video => &[
                Row::InactiveBar("Brightness"),
                Row::InactiveBar("Text Duration"),
                Row::Disabled("Floats", "On"),
                Row::InactiveBar("Float Speed"),
                Row::Disabled("Combat Taunts", "On"),
                Row::Zoom,
                Row::Lighting,
                Row::Shadows,
            ],
            Self::Sound => &[Row::Effects, Row::Voice, Row::Music],
        }
    }
}

#[derive(Clone, Copy)]
enum Row {
    Hidden,
    Disabled(&'static str, &'static str),
    InactiveBar(&'static str),
    Run,
    Zoom,
    Lighting,
    Shadows,
    Effects,
    Voice,
    Music,
}

impl Row {
    fn label(self) -> &'static str {
        match self {
            Self::Hidden => "",
            Self::Disabled(title, _) | Self::InactiveBar(title) => title,
            Self::Run => "Always Run",
            Self::Zoom => "World Zoom",
            Self::Lighting => "Lighting",
            Self::Shadows => "Shadows",
            Self::Effects => "Effects",
            Self::Voice => "Voice",
            Self::Music => "Music",
        }
    }

    fn enabled(self) -> bool {
        !matches!(
            self,
            Self::Hidden | Self::Disabled(..) | Self::InactiveBar(_)
        )
    }

    fn volume(self, audio: &mut Audio) -> Option<&mut u8> {
        match self {
            Self::Effects => Some(&mut audio.effects),
            Self::Voice => Some(&mut audio.voice),
            Self::Music => Some(&mut audio.music),
            _ => None,
        }
    }

    fn value(self, settings: menu::Settings) -> String {
        let on = |enabled| if enabled { "On" } else { "Off" };
        match self {
            Self::Disabled(_, value) => value.to_owned(),
            Self::Run => on(settings.run).to_owned(),
            Self::Lighting => on(settings.lighting).to_owned(),
            Self::Shadows => on(settings.shadows).to_owned(),
            Self::Zoom => format!("{}x", settings.zoom),
            _ => String::new(),
        }
    }

    fn adjust(self, step: i8, settings: &mut menu::Settings) {
        if let Some(volume) = self.volume(&mut settings.audio) {
            *volume = volume.saturating_add_signed(step).min(10);
            return;
        }
        match self {
            Self::Run => settings.run = !settings.run,
            Self::Lighting => settings.lighting = !settings.lighting,
            Self::Shadows => settings.shadows = !settings.shadows,
            Self::Zoom => {
                settings.zoom = ((settings.zoom as i8 - 1 + step).rem_euclid(3) + 1) as u8
            }
            _ => {}
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Control {
    Nav(usize),
    Arrow(usize, usize),
    Gauge(usize),
}

#[derive(Default)]
pub(in crate::viewer) struct Options {
    category: Category,
    hover: Option<Control>,
    down: Option<Control>,
    focus: Option<usize>,
}

pub(super) struct Theme {
    pub(super) layout: OptionsLayout,
    strings: Mes,
    pub(super) heading_font: hud::MenuFont,
    pub(super) value_font: hud::MenuFont,
    background: Asset,
    lens: Asset,
    choice: Asset,
    slider: Asset,
    arrows: [[Asset; 3]; 2],
    gauge: Asset,
}

const NAV: [&str; 4] = ["Game", "Video", "Sound", "Done"];

impl Theme {
    pub(super) fn load(
        layout: OptionsLayout,
        source: &ThemeSource,
        vfs: &mut Vfs,
        files: &mut Vfs,
        fallback_value_font: &str,
    ) -> Result<Self, Box<dyn Error>> {
        if layout.nav_width == 0
            || layout.nav_center < layout.nav_width / 2
            || layout
                .nav_y
                .iter()
                .chain(&layout.row_x)
                .chain(&layout.row_y)
                .chain(&layout.arrow_x)
                .chain(&layout.gauge_at)
                .chain(&layout.lens_at)
                .chain([&layout.nav_center, &layout.arrow_y])
                .any(|&n| n > 4096)
        {
            return Err("Invalid Options geometry".into());
        }
        let mut load = |name, index| hud::resource(vfs, files, name, index).map(Arc::new);
        let background = load(&layout.background, 0)?;
        let lens = load(&layout.lens, 0)?;
        let choice = load(&layout.choice, 0)?;
        let slider = load(&layout.slider, 0)?;
        let gauge = load(&layout.gauge, 0)?;
        let mut arrows =
            |name| Ok::<_, Box<dyn Error>>([load(name, 0)?, load(name, 1)?, load(name, 2)?]);
        let arrows = [arrows(&layout.decrease)?, arrows(&layout.increase)?];
        Ok(Self {
            strings: load_mes(vfs, &layout.strings)?.mes,
            heading_font: hud::MenuFont::load(
                vfs,
                source,
                &layout.heading_font,
                layout.heading_font_size,
            )?,
            value_font: hud::MenuFont::load(
                vfs,
                source,
                layout.value_font.as_deref().unwrap_or(fallback_value_font),
                layout.value_font_size,
            )?,
            layout,
            background,
            lens,
            choice,
            slider,
            arrows,
            gauge,
        })
    }

    pub(super) fn canvas(&self) -> Point {
        [
            self.background.decoded.frame.width,
            self.background.decoded.frame.height,
        ]
    }

    fn origin(&self, row: usize) -> Point {
        [self.layout.row_x[row / 4], self.layout.row_y[row % 4]]
    }

    fn rect(&self, control: Control) -> Rect {
        match control {
            Control::Nav(index) => [
                self.layout.nav_center - self.layout.nav_width / 2,
                self.layout.nav_y[index],
                self.layout.nav_width,
                35,
            ],
            Control::Arrow(row, side) => {
                let [x, y] = self.origin(row);
                asset_rect(
                    [x + self.layout.arrow_x[side], y + self.layout.arrow_y],
                    &self.arrows[side][0],
                )
            }
            Control::Gauge(row) => {
                let [x, y] = self.origin(row);
                let [dx, dy] = self.layout.gauge_at;
                asset_rect([x + dx, y + dy], &self.gauge)
            }
        }
    }

    fn controls(&self, category: Category) -> impl Iterator<Item = Control> {
        (0..4).map(Control::Nav).chain(
            category
                .rows()
                .iter()
                .copied()
                .enumerate()
                .filter(|(_, row)| row.enabled())
                .flat_map(|(index, row)| {
                    [
                        Some(Control::Arrow(index, 0)),
                        Some(Control::Arrow(index, 1)),
                        row.volume(&mut Audio::default())
                            .map(|_| Control::Gauge(index)),
                    ]
                    .into_iter()
                    .flatten()
                }),
        )
    }

    pub(super) fn chrome(&self, frame: &mut Frame) {
        frame.push(&self.background, asset_rect([0, 0], &self.background));
        frame.options_lens = Some((
            asset_rect(self.layout.lens_at, &self.lens),
            Arc::clone(&self.lens),
        ));
    }
}

impl Classic {
    pub(super) fn render_options(
        &mut self,
        out: &mut Frame,
        options: &Options,
        mut settings: menu::Settings,
    ) {
        self.options.chrome(out);
        for (index, fallback) in NAV.iter().enumerate() {
            let rect = self.options.rect(Control::Nav(index));
            let focused =
                options.hover == Some(Control::Nav(index)) || options.focus == Some(index);
            let label = self.label(4000 + index as i32, fallback);
            let [x, y, w, h] = rect;
            for (at, style) in [
                (
                    [x.saturating_sub(1), y.saturating_sub(1), w, h],
                    Style::OptionNavShadow,
                ),
                ([x + 1, y + 1, w, h], Style::OptionNavHighlight),
                (
                    rect,
                    if focused {
                        Style::OptionNavHover
                    } else {
                        Style::OptionNav
                    },
                ),
            ] {
                self.draw_text(out, at, label.clone(), style, true);
            }
        }
        for (index, &row) in options.category.rows().iter().enumerate() {
            if matches!(row, Row::Hidden) {
                continue;
            }
            let [x, y] = self.options.origin(index);
            let volume = row.volume(&mut settings.audio).copied();
            let base = if volume.is_some() || matches!(row, Row::InactiveBar(_)) {
                &self.options.slider
            } else {
                &self.options.choice
            };
            let base_rect = asset_rect([x, y], base);
            out.push(base, base_rect);
            let title = self
                .options
                .strings
                .get(100 * (1 + options.category as i32) + index as i32)
                .filter(|_| !matches!(row, Row::Zoom | Row::Lighting | Row::Shadows))
                .unwrap_or_else(|| row.label())
                .to_owned();
            self.draw_text(
                out,
                [
                    x,
                    y + 2,
                    base_rect[2],
                    self.options
                        .heading_font
                        .line_height()
                        .min(self.options.layout.arrow_y.saturating_sub(2)),
                ],
                title,
                if row.enabled() {
                    Style::OptionHeading
                } else {
                    Style::OptionInactive
                },
                true,
            );
            if row.enabled() {
                for side in 0..2 {
                    let control = Control::Arrow(index, side);
                    let art = &self.options.arrows[side];
                    let frame = if options.hover != Some(control) {
                        0
                    } else if options.down == Some(control) {
                        1
                    } else {
                        2
                    };
                    out.push(&art[frame], self.options.rect(control));
                }
            }
            let rect = self.options.rect(Control::Gauge(index));
            if let Some(value) = volume {
                let width = rect[2] * u32::from(value) / 10;
                if width > 0 {
                    out.push_clipped(
                        &self.options.gauge,
                        rect,
                        [rect[0], rect[1], width, rect[3]],
                    );
                }
            } else {
                self.draw_text(
                    out,
                    rect,
                    row.value(settings),
                    if row.enabled() {
                        Style::OptionValue
                    } else {
                        Style::OptionInactiveValue
                    },
                    true,
                );
            }
        }
    }
}

impl Options {
    pub(in crate::viewer) fn cancel_pointer(&mut self) {
        self.down = None;
        self.hover = None;
    }

    pub(in crate::viewer) fn key(&mut self, key: &Key) -> bool {
        match key {
            Key::Named(NamedKey::Escape) => return true,
            Key::Named(NamedKey::ArrowUp) => {
                self.focus = Some((self.focus.unwrap_or(self.category as usize) + 3) % 4);
            }
            Key::Named(NamedKey::ArrowDown) => {
                self.focus = Some((self.focus.unwrap_or(self.category as usize) + 1) % 4);
            }
            Key::Named(NamedKey::Enter) => return self.navigate(self.focus.unwrap_or(3)),
            _ => {}
        }
        false
    }

    fn navigate(&mut self, index: usize) -> bool {
        if let Some(category) = [Category::Game, Category::Video, Category::Sound].get(index) {
            self.category = *category;
            self.focus = None;
            return false;
        }
        true
    }

    pub(in crate::viewer) fn hover(
        &mut self,
        cursor: Option<(u32, u32)>,
        size: Point,
        scale: u16,
        theme: &Classic,
        settings: &mut menu::Settings,
    ) -> bool {
        let transform = theme.options_transform(size, scale);
        let old = self.hover;
        self.hover = cursor.and_then(|point| {
            theme
                .options
                .controls(self.category)
                .find(|&control| transform.contains(theme.options.rect(control), point))
        });
        if let (Some(Control::Gauge(index)), Some((x, _))) = (self.down, cursor) {
            let [left, _, width, _] = transform.rect(theme.options.rect(Control::Gauge(index)));
            if width > 0.
                && let Some(volume) = self.category.rows()[index].volume(&mut settings.audio)
            {
                *volume = (((x as f32 - left) / width).clamp(0., 1.) * 10.).round() as u8;
            }
        }
        old != self.hover || matches!(self.down, Some(Control::Gauge(_)))
    }

    pub(in crate::viewer) fn mouse(
        &mut self,
        pressed: bool,
        cursor: Option<(u32, u32)>,
        size: Point,
        scale: u16,
        theme: &Classic,
        settings: &mut menu::Settings,
    ) -> (bool, Option<i32>) {
        self.hover(cursor, size, scale, theme, settings);
        if pressed {
            self.down = self.hover;
            self.hover(cursor, size, scale, theme, settings);
            return (
                false,
                self.down.map(|control| {
                    if matches!(control, Control::Nav(_)) {
                        3027
                    } else {
                        3000
                    }
                }),
            );
        }
        let down = self.down.take();
        if down != self.hover {
            return (false, None);
        }
        match down {
            Some(Control::Nav(index)) => (self.navigate(index), None),
            Some(Control::Arrow(index, side)) => {
                self.category.rows()[index].adjust(if side == 0 { -1 } else { 1 }, settings);
                (false, Some(3001))
            }
            Some(Control::Gauge(_)) => (false, Some(3001)),
            None => (false, None),
        }
    }

    pub(super) fn diagnostic(&self, theme: &Classic, size: Point, scale: u16) -> serde_json::Value {
        let transform = theme.options_transform(size, scale);
        let controls: BTreeMap<_, _> = theme
            .options
            .controls(self.category)
            .map(|control| {
                let name = match control {
                    Control::Nav(index) => NAV[index].to_ascii_lowercase(),
                    Control::Arrow(index, side) => format!(
                        "{}_{}",
                        self.category.rows()[index]
                            .label()
                            .to_ascii_lowercase()
                            .replace(' ', "_"),
                        ["decrease", "increase"][side]
                    ),
                    Control::Gauge(index) => {
                        self.category.rows()[index].label().to_ascii_lowercase()
                    }
                };
                (name, transform.rect(theme.options.rect(control)))
            })
            .collect();
        serde_json::json!({
            "screen":"options",
            "category": NAV[self.category as usize].to_ascii_lowercase(),
            "controls": controls,
            "bounds":transform.rect(asset_rect([0, 0], &theme.options.background)),
        })
    }
}
