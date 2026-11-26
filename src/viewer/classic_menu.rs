//! Classic in-play menu and the shared save/load shell.
use super::{ThemeSource, hud, menu};
use arcanum_rs::{
    art::SpriteFrame,
    content::{
        art::LoadedFrame,
        load_mes,
        save::{self, Slot},
    },
    gpu::OverlayDraw,
    mes::Mes,
    vfs::{Policy, Vfs},
};
use serde::Deserialize;
use std::{collections::BTreeMap, error::Error, sync::Arc};
use winit::keyboard::{Key, NamedKey};

type Point = [u32; 2];
type Rect = [u32; 4];
type Asset = Arc<LoadedFrame>;

#[derive(Deserialize)]
struct Layout {
    canvas: Point,
    pause: String,
    slots: String,
    top: String,
    bottom: String,
    lens: String,
    cover: String,
    dialog: String,
    font: String,
    menu_font: String,
    strings: String,
    text_color: [u8; 4],
    selected_color: [u8; 4],
    menu_color: [u8; 4],
    menu_hover_color: [u8; 4],
    pause_labels: [i32; 5],
    pause_center: u32,
    pause_y: [u32; 5],
    pause_hit_width: u32,
    slots_at: Point,
    bottom_at: Point,
    lens_at: Point,
    list: Rect,
    row_height: u32,
    preview: Rect,
    name: Rect,
    location: Rect,
    time: Rect,
    date: Rect,
    title: Rect,
    message: Rect,
    dialog_at: Point,
    dialog_text: Rect,
    scroll: Rect,
    next: Button,
    back: Button,
    delete: Button,
    info: Button,
    up: Button,
    down: Button,
    thumb_top: Button,
    thumb_middle: Button,
    thumb_bottom: Button,
    yes: Button,
    no: Button,
}

#[derive(Deserialize)]
struct Button {
    art: String,
    at: Point,
    frames: usize,
}

const BUTTONS: [Control; 11] = [
    Control::Next,
    Control::Back,
    Control::Delete,
    Control::Info,
    Control::Up,
    Control::Down,
    Control::ThumbTop,
    Control::ThumbMiddle,
    Control::ThumbBottom,
    Control::Yes,
    Control::No,
];

impl Layout {
    fn button(&self, control: Control) -> &Button {
        match control {
            Control::Next => &self.next,
            Control::Back => &self.back,
            Control::Delete => &self.delete,
            Control::Info => &self.info,
            Control::Up => &self.up,
            Control::Down => &self.down,
            Control::ThumbTop => &self.thumb_top,
            Control::ThumbMiddle => &self.thumb_middle,
            Control::ThumbBottom => &self.thumb_bottom,
            Control::Yes => &self.yes,
            Control::No => &self.no,
            _ => unreachable!(),
        }
    }
}

pub(crate) struct Classic {
    pub(super) source: ThemeSource,
    layout: Layout,
    strings: Mes,
    art: [Asset; 7],
    buttons: BTreeMap<Control, Vec<Asset>>,
    font: BTreeMap<char, SpriteFrame>,
    menu_font: BTreeMap<char, SpriteFrame>,
    text: BTreeMap<(String, u32, Style), Asset>,
    black: Asset,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Style {
    Normal,
    Selected,
    Pause,
    PauseHover,
}

impl Classic {
    pub(crate) fn load(source: &ThemeSource, vfs: &mut Vfs) -> Result<Self, Box<dyn Error>> {
        let layout: Layout = toml::from_str(&source.read(64 * 1024)?)?;
        if layout.canvas.iter().any(|&d| !(1..=4096).contains(&d))
            || !(1..=256).contains(&layout.row_height)
            || layout.list[3] < layout.row_height
            || layout.pause_center < layout.pause_hit_width / 2
            || layout.scroll[3] < 32
        {
            return Err("Invalid pause theme geometry".into());
        }
        for rect in [
            layout.list,
            layout.preview,
            layout.name,
            layout.location,
            layout.time,
            layout.date,
            layout.title,
            layout.message,
            layout.dialog_text,
            layout.scroll,
        ] {
            if rect.iter().any(|&d| d > 4096) || rect[2] == 0 || rect[3] == 0 {
                return Err("Invalid pause theme rectangle".into());
            }
        }
        let mut files = Vfs::new(Policy::Ordered);
        if let Some(directory) = source.directory() {
            files.mount_directory(directory)?;
        }
        let mut load =
            |asset: &str, index| hud::resource(vfs, &mut files, asset, index).map(Arc::new);
        let art = [
            load(&layout.pause, 0)?,
            load(&layout.slots, 0)?,
            load(&layout.top, 0)?,
            load(&layout.bottom, 0)?,
            load(&layout.lens, 0)?,
            load(&layout.cover, 0)?,
            load(&layout.dialog, 0)?,
        ];
        let mut buttons = BTreeMap::new();
        for control in BUTTONS {
            let button = layout.button(control);
            if !(1..=3).contains(&button.frames) || button.at.iter().any(|&d| d > 4096) {
                return Err("Invalid pause theme button".into());
            }
            buttons.insert(
                control,
                (0..button.frames)
                    .map(|i| load(&button.art, i))
                    .collect::<Result<_, _>>()?,
            );
        }
        let strings = load_mes(vfs, &layout.strings)?.mes;
        let font = hud::console_font(vfs, source, &layout.font)?;
        let menu_font = hud::console_font(vfs, source, &layout.menu_font)?;
        Ok(Self {
            source: source.clone(),
            layout,
            strings,
            art,
            buttons,
            font,
            menu_font,
            text: BTreeMap::new(),
            black: frame("pause/black", (1, 1), vec![[0, 0, 0, 255]]),
        })
    }

    fn label(&self, id: i32, fallback: &str) -> String {
        self.strings.get(id).unwrap_or(fallback).to_owned()
    }

    fn transform(&self, size: Point, scale: u16) -> Transform {
        let [w, h] = self.layout.canvas.map(|n| n as f32);
        let factor = (f32::from(scale) / 100.)
            .min(size[0] as f32 / w)
            .min(size[1] as f32 / h);
        Transform {
            factor,
            anchor: [
                (size[0] as f32 - w * factor) / 2.,
                (size[1] as f32 - h * factor) / 2.,
            ],
        }
    }

    fn rect(&self, control: Control) -> Rect {
        match control {
            Control::Pause(index) => [
                self.layout.pause_center - self.layout.pause_hit_width / 2,
                self.layout.pause_y[index],
                self.layout.pause_hit_width,
                35,
            ],
            Control::Row(row) => {
                let [x, y, w, h] = self.layout.list;
                let offset = row as u32 * self.layout.row_height;
                [x, y + offset, w, self.layout.row_height.min(h - offset)]
            }
            Control::Track => self.layout.scroll,
            _ => asset_rect(self.layout.button(control).at, &self.buttons[&control][0]),
        }
    }

    fn visible_rows(&self) -> usize {
        self.layout.list[3].div_ceil(self.layout.row_height) as usize
    }

    fn draw_text(
        &mut self,
        out: &mut Frame,
        rect: Rect,
        text: String,
        style: Style,
        centered: bool,
    ) {
        if text.is_empty() {
            return;
        }
        if self.text.len() >= 256 {
            self.text.clear();
        }
        let key = (text, rect[2], style);
        let asset = self
            .text
            .entry(key)
            .or_insert_with_key(|(text, max_width, style)| {
                let (font, color) = match style {
                    Style::Normal => (&self.font, self.layout.text_color),
                    Style::Selected => (&self.font, self.layout.selected_color),
                    Style::Pause => (&self.menu_font, self.layout.menu_color),
                    Style::PauseHover => (&self.menu_font, self.layout.menu_hover_color),
                };
                let (width, height, pixels) = text_pixels(font, text, *max_width, color);
                frame("pause/text", (width, height), pixels)
            });
        let sprite = &asset.decoded.frame;
        let x = rect[0]
            + if centered {
                rect[2].saturating_sub(sprite.width) / 2
            } else {
                0
            };
        let y = rect[1] + rect[3].saturating_sub(sprite.height) / 2;
        out.push_clipped(asset, [x, y, sprite.width, sprite.height], rect);
    }

    fn draw_message(&mut self, out: &mut Frame, rect: Rect, message: &str) {
        let mut lines = Vec::new();
        let mut line = String::new();
        let max = rect[2] as usize / 8;
        for word in message.split_whitespace() {
            if !line.is_empty() && line.chars().count() + word.chars().count() >= max {
                lines.push(std::mem::take(&mut line));
            }
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(word);
        }
        lines.push(line);
        for (row, text) in lines.into_iter().take((rect[3] / 16) as usize).enumerate() {
            self.draw_text(
                out,
                [rect[0], rect[1] + row as u32 * 16, rect[2], 16],
                text,
                Style::Normal,
                true,
            );
        }
    }

    fn button_frame(&self, out: &mut Frame, menu: &Menu, control: Control) {
        let frames = &self.buttons[&control];
        let index = if menu.down == Some(control) && menu.hover == Some(control) {
            1
        } else if menu.hover == Some(control) || control == Control::Info && !menu.info {
            frames.len().saturating_sub(1)
        } else {
            0
        };
        out.push(&frames[index.min(frames.len() - 1)], self.rect(control));
    }

    fn scrollbar(&self, out: &mut Frame, menu: &Menu) {
        self.button_frame(out, menu, Control::Up);
        self.button_frame(out, menu, Control::Down);
        let max = menu.max_top(self);
        if max == 0 {
            return;
        }
        let [x, y, w, h] = self.layout.scroll;
        let track = h - 24;
        let height =
            ((track as usize * self.visible_rows() / menu.count()) as u32).clamp(14, track);
        let at = y + 12 + ((track - height) as usize * menu.top / max) as u32;
        let top = &self.buttons[&Control::ThumbTop][0];
        let bottom = &self.buttons[&Control::ThumbBottom][0];
        let a = top.decoded.frame.height.min(height / 2);
        let b = bottom.decoded.frame.height.min(height / 2);
        out.push(top, [x, at, w, a]);
        out.push(
            &self.buttons[&Control::ThumbMiddle][0],
            [x, at + a, w, height - a - b],
        );
        out.push(bottom, [x, at + height - b, w, b]);
    }

    pub(super) fn render(
        &mut self,
        menu: &Menu,
        size: Point,
        scale: u16,
        settings: menu::Settings,
    ) -> Frame {
        let mut out = Frame {
            transform: self.transform(size, scale),
            parts: Vec::new(),
        };
        if let Some(options) = &menu.options {
            let asset = options.frame(settings, "VULKAN");
            let w = asset.decoded.frame.width * u32::from(settings.zoom);
            let h = asset.decoded.frame.height * u32::from(settings.zoom);
            out.transform = Transform {
                factor: 1.,
                anchor: [0., 0.],
            };
            out.push(
                &asset,
                [
                    size[0].saturating_sub(w) / 2,
                    size[1].saturating_sub(h) / 2,
                    w,
                    h,
                ],
            );
            return out;
        }
        if let Some(screen) = menu.slots_screen {
            self.render_slots(&mut out, menu, screen);
        } else {
            out.push(&self.art[0], asset_rect([0, 0], &self.art[0]));
            for (index, fallback) in ["Save Game", "Load Game", "Options", "Quit Game", "Continue"]
                .into_iter()
                .enumerate()
            {
                let control = Control::Pause(index);
                let style = if menu.hover == Some(control) || menu.focus == Some(index) {
                    Style::PauseHover
                } else {
                    Style::Pause
                };
                self.draw_text(
                    &mut out,
                    self.rect(control),
                    self.label(self.layout.pause_labels[index], fallback),
                    style,
                    true,
                );
            }
        }
        if let Some(confirm) = &menu.confirm {
            out.push(
                &self.art[6],
                asset_rect(self.layout.dialog_at, &self.art[6]),
            );
            let text = match confirm {
                Confirm::Quit => self.label(5100, "Are you sure you want to quit?"),
                Confirm::Delete(_) => self.label(5150, "Delete this saved game?"),
                Confirm::Overwrite(..) => self.label(5064, "Overwrite this saved game?"),
            };
            self.draw_message(&mut out, self.layout.dialog_text, &text);
            self.button_frame(&mut out, menu, Control::Yes);
            self.button_frame(&mut out, menu, Control::No);
        }
        out
    }

    fn render_slots(&mut self, out: &mut Frame, menu: &Menu, screen: SlotScreen) {
        for (asset, at) in [
            (&self.art[1], self.layout.slots_at),
            (&self.art[2], [0, 0]),
            (&self.art[3], self.layout.bottom_at),
            (&self.art[4], self.layout.lens_at),
        ] {
            out.push(asset, asset_rect(at, asset));
        }

        out.push(&self.black, self.layout.list);
        out.push(&self.black, self.layout.preview);
        if let Some(preview) = &menu.preview {
            out.push(preview, self.layout.preview);
        }
        if menu.info
            && let Some(slot) = menu.selected_slot()
        {
            match &slot.metadata {
                Ok(metadata) => {
                    let hour = metadata.hours % 24;
                    self.draw_text(
                        out,
                        self.layout.date,
                        format!("Day {}", metadata.hours / 24 + 1),
                        Style::Normal,
                        false,
                    );
                    self.draw_text(
                        out,
                        self.layout.time,
                        format!("{hour:02}:00"),
                        Style::Normal,
                        false,
                    );
                    self.draw_text(
                        out,
                        self.layout.name,
                        metadata.name.clone(),
                        Style::Normal,
                        true,
                    );
                    self.draw_text(
                        out,
                        self.layout.location,
                        metadata.location.clone(),
                        Style::Normal,
                        true,
                    );
                }
                Err(error) => self.draw_message(out, self.layout.name, error),
            }
        }
        out.push(
            &self.art[5],
            asset_rect(
                [self.layout.preview[0], self.layout.preview[1]],
                &self.art[5],
            ),
        );

        for row in 0..self.visible_rows() {
            let index = row + menu.top;
            if index >= menu.count() {
                break;
            }
            let selected = Some(index) == menu.selected;
            let style = if selected {
                Style::Selected
            } else {
                Style::Normal
            };
            let name = if screen == SlotScreen::Save && index == 0 {
                if selected && !menu.name.is_empty() {
                    format!("{}_", menu.name)
                } else {
                    self.label(5055, "New")
                }
            } else {
                slot_name(&menu.slots[index - usize::from(screen == SlotScreen::Save)])
            };
            self.draw_text(out, self.rect(Control::Row(row)), name, style, false);
        }
        self.scrollbar(out, menu);
        for control in [Control::Next, Control::Back, Control::Delete, Control::Info] {
            self.button_frame(out, menu, control);
        }
        let title = match screen {
            SlotScreen::Save => self.label(30, "Save Game"),
            SlotScreen::Load => self.label(31, "Load Game"),
        };
        let title = match menu.hover {
            Some(Control::Next) => {
                if screen == SlotScreen::Save {
                    self.label(7021, "Save Game")
                } else {
                    self.label(7020, "Load Game")
                }
            }
            Some(Control::Back) => self.label(7002, "Back"),
            Some(Control::Delete) => self.label(7022, "Delete Save Game"),
            Some(Control::Info) => self.label(7025, "Toggle Info Display"),
            _ => title,
        };
        self.draw_text(out, self.layout.title, title, Style::Normal, true);
        if let Some(message) = &menu.message {
            self.draw_message(out, self.layout.message, message);
        }
    }
}

fn slot_name(slot: &Slot) -> String {
    slot.metadata
        .as_ref()
        .map(|m| m.name.clone())
        .unwrap_or_else(|_| "Unreadable save".into())
}

fn text_pixels(
    font: &BTreeMap<char, SpriteFrame>,
    text: &str,
    max_width: u32,
    color: [u8; 4],
) -> (u32, u32, Vec<[u8; 4]>) {
    let mut width = 1;
    let mut height = 1;
    let mut x = 0;
    let mut glyphs = Vec::new();
    for ch in text.chars() {
        let Some(glyph) = font.get(&ch).or_else(|| font.get(&'?')) else {
            continue;
        };
        if x >= max_width {
            break;
        }
        width = width.max((x + glyph.width).min(max_width));
        height = height.max(glyph.height);
        glyphs.push((x, glyph));
        x += glyph.hotspot.0.max(1) as u32;
    }
    let mut pixels = vec![[0; 4]; (width * height) as usize];
    for (x, glyph) in glyphs {
        for (index, pixel) in glyph.pixels.iter().enumerate() {
            let gx = index as u32 % glyph.width + x;
            let gy = index as u32 / glyph.width;
            if gx < width {
                hud::blend_glyph(&mut pixels[(gx + gy * width) as usize], color, pixel[3]);
            }
        }
    }
    (width, height, pixels)
}

fn frame(name: &str, (width, height): (u32, u32), pixels: Vec<[u8; 4]>) -> Asset {
    hud::generated_frame(
        name,
        SpriteFrame {
            width,
            height,
            hotspot: (0, 0),
            offset: (0, 0),
            pixels,
        },
    )
}

fn asset_rect([x, y]: Point, asset: &Asset) -> Rect {
    [x, y, asset.decoded.frame.width, asset.decoded.frame.height]
}

#[derive(Clone, Copy)]
struct Transform {
    factor: f32,
    anchor: [f32; 2],
}

impl Transform {
    fn rect(self, [x, y, w, h]: Rect) -> [f32; 4] {
        let [left, top] = self.anchor;
        let s = self.factor;
        [
            left + x as f32 * s,
            top + y as f32 * s,
            w as f32 * s,
            h as f32 * s,
        ]
    }

    fn contains(self, rect: Rect, (px, py): (u32, u32)) -> bool {
        let [x, y, w, h] = self.rect(rect);
        (x..x + w).contains(&(px as f32)) && (y..y + h).contains(&(py as f32))
    }
}

pub(super) struct Frame {
    transform: Transform,
    parts: Vec<(Asset, Rect, Rect)>,
}

impl Frame {
    fn push(&mut self, asset: &Asset, rect: Rect) {
        self.push_clipped(asset, rect, rect);
    }

    fn push_clipped(&mut self, asset: &Asset, rect: Rect, clip: Rect) {
        self.parts.push((Arc::clone(asset), rect, clip));
    }

    pub(super) fn draws(&self) -> impl Iterator<Item = OverlayDraw<'_>> {
        self.parts.iter().map(|(asset, rect, clip)| OverlayDraw {
            asset,
            rect: self.transform.rect(*rect),
            clip: self.transform.rect(*clip),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Control {
    Pause(usize),
    Row(usize),
    Next,
    Back,
    Delete,
    Info,
    Up,
    Down,
    Track,
    ThumbTop,
    ThumbMiddle,
    ThumbBottom,
    Yes,
    No,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SlotScreen {
    Save,
    Load,
}

enum Confirm {
    Quit,
    Delete(String),
    Overwrite(String, String),
}

pub(super) enum Command {
    Stay,
    Continue,
    Quit,
    Save { id: Option<String>, name: String },
    Load(String),
    Delete(String),
}

#[derive(Default)]
pub(super) struct Menu {
    slots_screen: Option<SlotScreen>,
    pub(super) options: Option<menu::Menu>,
    confirm: Option<Confirm>,
    focus: Option<usize>,
    hover: Option<Control>,
    down: Option<Control>,
    slots: Vec<Slot>,
    selected: Option<usize>,
    top: usize,
    name: String,
    info: bool,
    preview: Option<Asset>,
    pub(super) message: Option<String>,
}

impl Menu {
    fn count(&self) -> usize {
        self.slots.len() + usize::from(self.slots_screen == Some(SlotScreen::Save))
    }

    fn max_top(&self, theme: &Classic) -> usize {
        self.count().saturating_sub(theme.visible_rows())
    }

    fn selected_slot(&self) -> Option<&Slot> {
        let index = self
            .selected?
            .checked_sub(usize::from(self.slots_screen == Some(SlotScreen::Save)))?;
        self.slots.get(index)
    }

    pub(super) fn refresh(&mut self, store: Result<&save::Store, &str>, theme: &Classic) {
        self.message = None;
        self.slots = match store.map_err(str::to_owned).and_then(save::Store::slots) {
            Ok(slots) => slots,
            Err(error) => {
                self.message = Some(error);
                Vec::new()
            }
        };
        self.top = 0;
        let first =
            usize::from(self.slots_screen == Some(SlotScreen::Save) && !self.slots.is_empty());
        self.select((self.count() != 0).then_some(first), theme, store.ok());
    }

    fn select(&mut self, index: Option<usize>, theme: &Classic, store: Option<&save::Store>) {
        self.selected = index;
        if let Some(index) = index {
            self.top = self
                .top
                .min(index)
                .max((index + 1).saturating_sub(theme.visible_rows()));
        }
        self.preview = self
            .selected_slot()
            .and_then(|slot| store?.preview(&slot.id).ok())
            .and_then(|rgb| {
                (!rgb.is_empty()).then(|| {
                    frame(
                        "pause/save-preview",
                        save::PREVIEW_SIZE,
                        rgb.as_chunks::<3>()
                            .0
                            .iter()
                            .map(|&[r, g, b]| [r, g, b, 255])
                            .collect(),
                    )
                })
            });
    }

    pub(super) fn back(&mut self) -> Command {
        self.down = None;
        self.hover = None;
        if self.confirm.take().is_some() || self.slots_screen.take().is_some() {
            return Command::Stay;
        }
        Command::Continue
    }

    fn finish_confirmation(&mut self) -> Command {
        match self.confirm.take() {
            Some(Confirm::Quit) => Command::Quit,
            Some(Confirm::Delete(id)) => Command::Delete(id),
            Some(Confirm::Overwrite(id, name)) => Command::Save { id: Some(id), name },
            None => Command::Stay,
        }
    }

    fn activate(
        &mut self,
        control: Control,
        theme: &Classic,
        store: Result<&save::Store, &str>,
    ) -> Command {
        match control {
            Control::Pause(0 | 1) => {
                self.slots_screen = Some(if control == Control::Pause(0) {
                    SlotScreen::Save
                } else {
                    SlotScreen::Load
                });
                self.name.clear();
                self.info = true;
                self.refresh(store, theme);
                self.hover = None;
            }
            Control::Pause(2) => self.options = Some(menu::Menu::new()),
            Control::Pause(3) => self.confirm = Some(Confirm::Quit),
            Control::Pause(4) => return Command::Continue,
            Control::Row(row) => self.select(Some(row + self.top), theme, store.ok()),
            Control::Next => {
                if let Some(slot) = self.selected_slot() {
                    if self.slots_screen == Some(SlotScreen::Save) {
                        self.confirm = Some(Confirm::Overwrite(slot.id.clone(), slot_name(slot)));
                    } else {
                        return Command::Load(slot.id.clone());
                    }
                } else if self.slots_screen == Some(SlotScreen::Save) {
                    return Command::Save {
                        id: None,
                        name: self.name.clone(),
                    };
                }
            }
            Control::Back | Control::No => return self.back(),
            Control::Delete => {
                if let Some(slot) = self.selected_slot() {
                    self.confirm = Some(Confirm::Delete(slot.id.clone()));
                }
            }
            Control::Info => self.info = !self.info,
            Control::Up => self.scroll(-1, theme),
            Control::Down => self.scroll(1, theme),
            Control::Yes => return self.finish_confirmation(),
            _ => {}
        }
        Command::Stay
    }

    pub(super) fn scroll(&mut self, delta: isize, theme: &Classic) {
        if self.slots_screen.is_some() && self.confirm.is_none() {
            self.top = self
                .top
                .saturating_add_signed(delta)
                .min(self.max_top(theme));
        }
    }

    fn hit(
        &self,
        point: Option<(u32, u32)>,
        transform: Transform,
        theme: &Classic,
    ) -> Option<Control> {
        let contains =
            |control| point.is_some_and(|point| transform.contains(theme.rect(control), point));
        if self.confirm.is_some() {
            [Control::Yes, Control::No]
                .into_iter()
                .find(|&c| contains(c))
        } else if self.slots_screen.is_some() {
            [
                Control::Next,
                Control::Back,
                Control::Delete,
                Control::Info,
                Control::Up,
                Control::Down,
                Control::Track,
            ]
            .into_iter()
            .chain(
                (0..self
                    .count()
                    .saturating_sub(self.top)
                    .min(theme.visible_rows()))
                    .map(Control::Row),
            )
            .find(|&c| contains(c))
        } else {
            (0..5).map(Control::Pause).find(|&c| contains(c))
        }
    }

    pub(super) fn hover(
        &mut self,
        point: Option<(u32, u32)>,
        size: Point,
        scale: u16,
        theme: &Classic,
    ) -> bool {
        let transform = theme.transform(size, scale);
        let next = self.hit(point, transform, theme);
        let previous_top = self.top;
        if self.down == Some(Control::Track)
            && let Some((_, y)) = point
        {
            self.move_scroll_thumb(y, transform, theme);
        }
        next != std::mem::replace(&mut self.hover, next) || self.top != previous_top
    }

    fn move_scroll_thumb(&mut self, y: u32, transform: Transform, theme: &Classic) {
        let [_, top, _, h] = transform.rect(theme.layout.scroll);
        let pad = h * 0.05;
        let fraction = ((y as f32 - top - pad) / (h - 2. * pad)).clamp(0., 1.);
        self.top = (fraction * self.max_top(theme) as f32).round() as usize;
    }

    pub(super) fn mouse(
        &mut self,
        pressed: bool,
        pointer: (Option<(u32, u32)>, Point, u16),
        theme: &Classic,
        store: Result<&save::Store, &str>,
    ) -> Command {
        if pressed {
            self.down = self.hover;
            if self.down == Some(Control::Track)
                && let (Some((_, y)), size, scale) = pointer
            {
                self.move_scroll_thumb(y, theme.transform(size, scale), theme);
            }
        } else if let Some(control) = self.down.take()
            && self.hover == Some(control)
        {
            return self.activate(control, theme, store);
        }
        Command::Stay
    }

    pub(super) fn key(
        &mut self,
        key: &Key,
        text: Option<&str>,
        theme: &Classic,
        store: Result<&save::Store, &str>,
    ) -> Command {
        if *key == Key::Named(NamedKey::Escape) {
            return self.back();
        }
        if self.confirm.is_some() {
            return match key {
                Key::Named(NamedKey::Enter) => self.finish_confirmation(),
                Key::Character(ch) if ch.eq_ignore_ascii_case("y") => self.finish_confirmation(),
                Key::Character(ch) if ch.eq_ignore_ascii_case("n") => self.back(),
                _ => Command::Stay,
            };
        }
        let step = match key {
            Key::Named(NamedKey::ArrowUp) => -1,
            Key::Named(NamedKey::ArrowDown) => 1,
            _ => 0,
        };
        if self.slots_screen.is_none() {
            if step != 0 {
                self.focus = Some(self.focus.map_or(if step > 0 { 0 } else { 4 }, |i| {
                    (i as isize + step).rem_euclid(5) as usize
                }));
            } else if *key == Key::Named(NamedKey::Enter)
                && let Some(index) = self.focus
            {
                return self.activate(Control::Pause(index), theme, store);
            }
        } else {
            match key {
                Key::Named(NamedKey::Enter) => return self.activate(Control::Next, theme, store),
                Key::Named(NamedKey::Delete) => {
                    return self.activate(Control::Delete, theme, store);
                }
                Key::Named(NamedKey::ArrowUp | NamedKey::ArrowDown) if self.count() != 0 => {
                    let index = self
                        .selected
                        .unwrap_or(0)
                        .saturating_add_signed(step)
                        .min(self.count() - 1);
                    self.select(Some(index), theme, store.ok());
                }
                Key::Named(NamedKey::Backspace) if self.editing() => {
                    self.name.pop();
                }
                _ if self.editing() => {
                    if let Some(text) = text {
                        let available = 80usize.saturating_sub(self.name.chars().count());
                        self.name
                            .extend(text.chars().filter(|ch| !ch.is_control()).take(available));
                    }
                }
                _ => {}
            }
        }
        Command::Stay
    }

    pub(super) fn editing(&self) -> bool {
        self.slots_screen == Some(SlotScreen::Save)
            && self.selected == Some(0)
            && self.confirm.is_none()
    }

    pub(super) fn diagnostic(&self, theme: &Classic, size: Point, scale: u16) -> serde_json::Value {
        let screen = if self.options.is_some() {
            "options"
        } else {
            match self.slots_screen {
                Some(SlotScreen::Save) => "save",
                Some(SlotScreen::Load) => "load",
                None => "pause",
            }
        };
        let transform = theme.transform(size, scale);
        let controls: BTreeMap<_, _> = [
            ("save", Control::Pause(0)),
            ("load", Control::Pause(1)),
            ("options", Control::Pause(2)),
            ("quit", Control::Pause(3)),
            ("continue", Control::Pause(4)),
            ("next", Control::Next),
            ("back", Control::Back),
            ("delete", Control::Delete),
            ("info", Control::Info),
            ("yes", Control::Yes),
            ("no", Control::No),
        ]
        .into_iter()
        .filter(|(_, control)| {
            if self.confirm.is_some() {
                matches!(control, Control::Yes | Control::No)
            } else if self.slots_screen.is_some() {
                matches!(
                    control,
                    Control::Next | Control::Back | Control::Delete | Control::Info
                )
            } else {
                matches!(control, Control::Pause(_))
            }
        })
        .map(|(label, control)| (label, transform.rect(theme.rect(control))))
        .collect();
        let rows: Vec<_> = (0..self
            .count()
            .saturating_sub(self.top)
            .min(theme.visible_rows()))
            .map(|row| transform.rect(theme.rect(Control::Row(row))))
            .collect();
        serde_json::json!({
            "screen":screen,"bounds":transform.rect([0,0,theme.layout.canvas[0],theme.layout.canvas[1]]),
            "controls":controls,"rows":rows,"selected":self.selected,"preview":self.preview.is_some(),
            "confirmation":self.confirm.is_some(),"message":self.message,
        })
    }
}
