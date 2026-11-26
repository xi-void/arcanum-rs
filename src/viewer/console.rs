//! Modal command editing and themed console sprites for the shared UI pass.

use super::{LoadedFrame, SpriteFrame, ThemeSource, hud};
use arcanum_rs::{developer, vfs::Vfs};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, VecDeque},
    io::Read,
    sync::{Arc, mpsc},
};
use winit::{
    event_loop::EventLoopProxy,
    keyboard::{Key, KeyCode, ModifiersState, NamedKey, PhysicalKey},
};

const INPUT_LIMIT: usize = 4096;
const HISTORY_LIMIT: usize = 100;
const OUTPUT_LIMIT: usize = 500;

type Color = [u8; 4];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Theme {
    keys: Vec<ToggleKey>,
    width_percent: u32,
    height_percent: u32,
    #[serde(default)]
    anchor: [u32; 2],
    font: String,
    font_size: u32,
    background: Color,
    text: Color,
    accent: Color,
    error: Color,
    selection: Color,
}

#[derive(Deserialize)]
enum ToggleKey {
    Backquote,
    F8,
    F9,
    F10,
    F11,
    F12,
}

impl ToggleKey {
    fn code(&self) -> KeyCode {
        match self {
            Self::Backquote => KeyCode::Backquote,
            Self::F8 => KeyCode::F8,
            Self::F9 => KeyCode::F9,
            Self::F10 => KeyCode::F10,
            Self::F11 => KeyCode::F11,
            Self::F12 => KeyCode::F12,
        }
    }
}

pub(super) struct Console {
    theme: Theme,
    glyphs: BTreeMap<char, SpriteFrame>,
    pub open: bool,
    input: String,
    cursor: usize,
    anchor: usize,
    history: VecDeque<String>,
    history_index: Option<usize>,
    draft: String,
    log: VecDeque<(String, Color)>,
    scroll: usize,
    paste: Option<mpsc::Receiver<Result<String, String>>>,
    frame: Option<(Layout, Arc<LoadedFrame>)>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Layout {
    width: u32,
    height: u32,
    step: u32,
    font: u32,
    padding: u32,
}

impl Layout {
    fn columns(self) -> usize {
        (self.width.saturating_sub(self.padding * 2) / self.step).max(1) as usize
    }

    fn row(self) -> u32 {
        self.font + 4
    }
}

impl Console {
    pub fn load(source: &ThemeSource, vfs: &mut Vfs) -> Result<Self, Box<dyn std::error::Error>> {
        let theme = parse_theme(&source.read(128 * 1024 + 1)?)?;
        let glyphs = hud::console_font(vfs, source, &theme.font)?;
        let mut console = Self::new(theme, glyphs);
        console.notice("Developer console. Type help for commands.");
        Ok(console)
    }

    fn new(theme: Theme, glyphs: BTreeMap<char, SpriteFrame>) -> Self {
        Self {
            theme,
            glyphs,
            open: false,
            input: String::new(),
            cursor: 0,
            anchor: 0,
            history: VecDeque::new(),
            history_index: None,
            draft: String::new(),
            log: VecDeque::new(),
            scroll: 0,
            paste: None,
            frame: None,
        }
    }

    pub fn replace_theme(&mut self, next: Self) {
        self.theme = next.theme;
        self.glyphs = next.glyphs;
        self.frame = None;
    }

    pub fn is_toggle(&self, physical: PhysicalKey, logical: &Key) -> bool {
        self.theme.keys.iter().any(|candidate| {
            physical == PhysicalKey::Code(candidate.code())
                || match candidate {
                    ToggleKey::Backquote => {
                        matches!(logical, Key::Character(ch) if matches!(ch.as_str(), "`" | "~"))
                    }
                    ToggleKey::F8 => *logical == Key::Named(NamedKey::F8),
                    ToggleKey::F9 => *logical == Key::Named(NamedKey::F9),
                    ToggleKey::F10 => *logical == Key::Named(NamedKey::F10),
                    ToggleKey::F11 => *logical == Key::Named(NamedKey::F11),
                    ToggleKey::F12 => *logical == Key::Named(NamedKey::F12),
                }
        })
    }

    pub fn toggle(&mut self) {
        self.open = !self.open;
        self.paste = None;
    }

    pub fn panel_contains(&self, point: (u32, u32), size: (u32, u32), scale: u16) -> bool {
        let layout = self.layout(size, scale);
        let (x, y) = self.anchor(size, scale);
        (x as u32..x as u32 + layout.width).contains(&point.0)
            && (y as u32..y as u32 + layout.height).contains(&point.1)
    }

    pub fn anchor(&self, size: (u32, u32), scale: u16) -> (i32, i32) {
        let layout = self.layout(size, scale);
        (
            (size.0.saturating_sub(layout.width) * self.theme.anchor[0] / 100) as i32,
            (size.1.saturating_sub(layout.height) * self.theme.anchor[1] / 100) as i32,
        )
    }

    pub fn notice(&mut self, text: &str) {
        self.push(text, self.theme.accent);
    }

    pub fn output(&mut self, result: Result<developer::Output, String>) {
        match result {
            Ok(developer::Output::Clear) => self.log.clear(),
            Ok(developer::Output::Data(data) | developer::Output::Accepted(data)) => {
                if let Some(message) = data["message"].as_str() {
                    self.push(message, self.theme.text);
                }
                if let Some(commands) = data["commands"].as_array() {
                    for command in commands {
                        if let (Some(syntax), Some(description)) =
                            (command["syntax"].as_str(), command["description"].as_str())
                        {
                            self.push(&format!("{syntax}  -  {description}"), self.theme.text);
                        }
                    }
                } else {
                    let mut data = data;
                    if let Some(object) = data.as_object_mut() {
                        object.remove("message");
                    }
                    if !data.as_object().is_some_and(|object| object.is_empty()) {
                        let rendered = if data.get("version").is_some()
                            && data.get("loaded_sectors").is_some()
                        {
                            data.to_string()
                        } else {
                            serde_json::to_string_pretty(&data).expect("JSON data")
                        };
                        self.push(&rendered, self.theme.text);
                    }
                }
            }
            Err(error) => self.push(&error, self.theme.error),
        }
        self.scroll = 0;
        self.frame = None;
    }

    fn push(&mut self, text: &str, color: Color) {
        for line in text.lines() {
            if self.log.len() == OUTPUT_LIMIT {
                self.log.pop_front();
            }
            self.log
                .push_back((line.chars().take(2048).collect(), color));
        }
        self.frame = None;
    }

    pub fn scroll(&mut self, delta: isize) {
        self.scroll = self
            .scroll
            .saturating_add_signed(delta)
            .min(self.log.len() * 10);
        self.frame = None;
    }

    fn replace(&mut self, text: &str) {
        let (start, end) = (self.cursor.min(self.anchor), self.cursor.max(self.anchor));
        let limit = INPUT_LIMIT - (self.input.len() - (end - start));
        let text: String = text.chars().filter(|ch| !ch.is_control()).collect();
        let end_text = text
            .char_indices()
            .map(|(at, _)| at)
            .chain([text.len()])
            .take_while(|&at| at <= limit)
            .last()
            .unwrap_or(0);
        self.input.replace_range(start..end, &text[..end_text]);
        self.cursor = start + end_text;
        self.anchor = self.cursor;
    }

    fn move_cursor(&mut self, next: usize, select: bool) {
        self.cursor = next;
        if !select {
            self.anchor = next;
        }
    }

    fn history(&mut self, up: bool) {
        if self.history.is_empty() {
            return;
        }
        self.history_index = match (self.history_index, up) {
            (None, true) => {
                self.draft.clone_from(&self.input);
                Some(self.history.len() - 1)
            }
            (Some(index), true) => Some(index.saturating_sub(1)),
            (Some(index), false) if index + 1 < self.history.len() => Some(index + 1),
            _ => None,
        };
        self.input
            .clone_from(self.history_index.map_or(&self.draft, |i| &self.history[i]));
        self.move_cursor(self.input.len(), false);
    }

    pub fn key(
        &mut self,
        key: &Key,
        text: Option<&str>,
        modifiers: ModifiersState,
        repeat: bool,
        wake: impl FnOnce() -> EventLoopProxy<()>,
    ) -> Option<String> {
        self.frame = None;
        let select = modifiers.shift_key();
        let previous = self.input[..self.cursor]
            .char_indices()
            .last()
            .map_or(0, |(at, _)| at);
        let next = self.input[self.cursor..]
            .chars()
            .next()
            .map_or(self.cursor, |ch| self.cursor + ch.len_utf8());
        if modifiers.control_key() && !modifiers.alt_key() {
            match key {
                Key::Character(ch) if ch.eq_ignore_ascii_case("a") => {
                    self.anchor = 0;
                    self.cursor = self.input.len();
                }
                Key::Character(ch) if ch.eq_ignore_ascii_case("v") && !repeat => {
                    self.request_paste(wake())
                }
                _ => {}
            }
            return None;
        }
        match key {
            Key::Named(NamedKey::Enter) if !repeat && !self.input.trim().is_empty() => {
                let command = std::mem::take(&mut self.input);
                if self.history.back() != Some(&command) {
                    if self.history.len() == HISTORY_LIMIT {
                        self.history.pop_front();
                    }
                    self.history.push_back(command.clone());
                }
                self.history_index = None;
                self.draft.clear();
                self.move_cursor(0, false);
                self.scroll = 0;
                self.push(&format!("> {command}"), self.theme.accent);
                return Some(command);
            }
            Key::Named(NamedKey::Tab) if !repeat && !self.input.contains(char::is_whitespace) => {
                let options: Vec<_> = developer::complete(&self.input).collect();
                if let [only] = options.as_slice() {
                    self.input = format!("{only} ");
                    self.move_cursor(self.input.len(), false);
                } else if !options.is_empty() {
                    self.notice(&options.join("   "));
                }
            }
            Key::Named(NamedKey::ArrowLeft) => self.move_cursor(previous, select),
            Key::Named(NamedKey::ArrowRight) => self.move_cursor(next, select),
            Key::Named(NamedKey::Home) => self.move_cursor(0, select),
            Key::Named(NamedKey::End) => self.move_cursor(self.input.len(), select),
            Key::Named(NamedKey::ArrowUp) => self.history(true),
            Key::Named(NamedKey::ArrowDown) => self.history(false),
            Key::Named(NamedKey::PageUp) => self.scroll(8),
            Key::Named(NamedKey::PageDown) => self.scroll(-8),
            Key::Named(NamedKey::Backspace | NamedKey::Delete) => {
                if self.cursor == self.anchor {
                    self.anchor = if *key == Key::Named(NamedKey::Backspace) {
                        previous
                    } else {
                        next
                    };
                }
                self.replace("");
            }
            _ => {
                if let Some(text) = text.filter(|_| !modifiers.super_key()) {
                    self.replace(text);
                }
            }
        }
        None
    }

    fn request_paste(&mut self, wake: EventLoopProxy<()>) {
        if self.paste.is_some() {
            return;
        }
        let (send, receive) = mpsc::channel();
        self.paste = Some(receive);
        std::thread::spawn(move || {
            let load = || -> Result<String, Box<dyn std::error::Error>> {
                use std::process::{Command, Stdio};
                let mut child = Command::new("timeout")
                    .args(["1s", "wl-paste", "--no-newline", "--type", "text"])
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null())
                    .spawn()?;
                let mut data = Vec::new();
                let read = child
                    .stdout
                    .take()
                    .unwrap()
                    .take(INPUT_LIMIT as u64)
                    .read_to_end(&mut data);
                if data.len() == INPUT_LIMIT || read.is_err() {
                    let _ = child.kill();
                }
                let status = child.wait()?;
                read?;
                if !status.success() && data.is_empty() {
                    return Err("clipboard has no readable Wayland text".into());
                }
                Ok(String::from_utf8_lossy(&data).replace(['\r', '\n', '\t'], " "))
            };
            let _ = send.send(load().map_err(|e| format!("Paste failed: {e}")));
            let _ = wake.send_event(());
        });
    }

    pub fn receive_paste(&mut self) -> bool {
        let Some(result) = self.paste.as_ref().and_then(|paste| paste.try_recv().ok()) else {
            return false;
        };
        self.paste = None;
        match result {
            Ok(text) => self.replace(&text),
            Err(error) => self.push(&error, self.theme.error),
        }
        self.frame = None;
        true
    }

    fn layout(&self, (w, h): (u32, u32), scale: u16) -> Layout {
        let font = (self.theme.font_size * u32::from(scale)).div_ceil(100);
        Layout {
            width: (w * self.theme.width_percent / 100).max(1),
            height: (h * self.theme.height_percent / 100).max(1),
            step: font * 3 / 4 + 1,
            font,
            padding: font / 2 + 1,
        }
    }

    pub fn frame(&mut self, size: (u32, u32), scale: u16) -> Arc<LoadedFrame> {
        let layout = self.layout(size, scale);
        if let Some((same, frame)) = &self.frame
            && *same == layout
        {
            return Arc::clone(frame);
        }
        let mut frame = canvas(layout.width, layout.height, self.theme.background);
        let input_y = layout.height.saturating_sub(layout.padding + layout.font);
        let visible = input_y.saturating_sub(layout.padding * 2) / layout.row();
        let columns = layout.columns();
        let mut rows = self.log.iter().rev().flat_map(|(text, color)| {
            let chars: Vec<_> = text.chars().collect();
            let rows: Vec<_> = chars
                .chunks(columns)
                .rev()
                .map(|chunk| (chunk.to_vec(), *color))
                .collect();
            rows
        });
        let skip = if self.scroll == 0 {
            0
        } else {
            let total: usize = self
                .log
                .iter()
                .map(|(text, _)| text.chars().count().div_ceil(columns))
                .sum();
            self.scroll.min(total.saturating_sub(1))
        };
        for (index, (text, color)) in rows.by_ref().skip(skip).take(visible as usize).enumerate() {
            let y = input_y.saturating_sub(layout.row() * (index as u32 + 1));
            self.text(
                &mut frame,
                (layout.padding, y),
                layout,
                text.into_iter(),
                color,
            );
        }
        let caret = self.input[..self.cursor].chars().count();
        let mark = self.input[..self.anchor].chars().count();
        let width = columns.saturating_sub(2).max(1);
        let first = caret.saturating_sub(width - 1);
        let left = layout.padding + 2 * layout.step;
        for i in caret.min(mark).max(first)..caret.max(mark).min(first + width) {
            fill(
                &mut frame,
                left + (i - first) as u32 * layout.step,
                input_y,
                layout.step,
                layout.font,
                self.theme.selection,
            );
        }
        self.text(
            &mut frame,
            (layout.padding, input_y),
            layout,
            ['>'].into_iter(),
            self.theme.accent,
        );
        self.text(
            &mut frame,
            (left, input_y),
            layout,
            self.input.chars().skip(first).take(width),
            self.theme.text,
        );
        fill(
            &mut frame,
            left + (caret - first) as u32 * layout.step,
            input_y + layout.font.saturating_sub(2),
            layout.step,
            2,
            self.theme.accent,
        );
        let frame = hud::generated_frame("developer/console", frame);
        self.frame = Some((layout, Arc::clone(&frame)));
        frame
    }

    pub fn status(&self, text: &str, scale: u16, max_width: u32) -> Arc<LoadedFrame> {
        let mut layout = self.layout((1, 1), scale);
        let chars: Vec<_> = text.chars().collect();
        layout.width =
            (layout.padding * 2 + layout.step * chars.len() as u32).min(max_width.max(1));
        let rows = chars.chunks(layout.columns()).take(4);
        layout.height = layout.row() * rows.len() as u32 + layout.padding;
        let mut frame = canvas(layout.width, layout.height, self.theme.background);
        for (index, row) in rows.enumerate() {
            self.text(
                &mut frame,
                (
                    layout.padding,
                    layout.padding / 2 + layout.row() * index as u32,
                ),
                layout,
                row.iter().copied(),
                self.theme.accent,
            );
        }
        hud::generated_frame("developer/statistics", frame)
    }

    fn text(
        &self,
        frame: &mut SpriteFrame,
        (x, y): (u32, u32),
        layout: Layout,
        chars: impl Iterator<Item = char>,
        color: Color,
    ) {
        for (index, ch) in chars.enumerate() {
            if ch == ' ' {
                continue;
            }
            let Some(glyph) = self.glyphs.get(&ch).or_else(|| self.glyphs.get(&'?')) else {
                continue;
            };
            let width = (glyph.width * layout.font / glyph.height.max(1))
                .max(1)
                .min(layout.step);
            let x = x + index as u32 * layout.step;
            for dy in 0..layout.font.min(frame.height.saturating_sub(y)) {
                for dx in 0..width.min(frame.width.saturating_sub(x)) {
                    let src = ((dy * glyph.height / layout.font) * glyph.width
                        + dx * glyph.width / width) as usize;
                    let dst = ((y + dy) * frame.width + x + dx) as usize;
                    if let Some(pixel) = glyph.pixels.get(src) {
                        hud::blend_glyph(&mut frame.pixels[dst], color, pixel[3]);
                    }
                }
            }
        }
    }
}

fn canvas(width: u32, height: u32, color: Color) -> SpriteFrame {
    SpriteFrame {
        width,
        height,
        hotspot: (0, 0),
        offset: (0, 0),
        pixels: vec![color; width as usize * height as usize],
    }
}

fn fill(frame: &mut SpriteFrame, x: u32, y: u32, width: u32, height: u32, color: Color) {
    for row in y..(y + height).min(frame.height) {
        if x < frame.width {
            let start = (row * frame.width + x) as usize;
            let width = width.min(frame.width - x) as usize;
            frame.pixels[start..start + width].fill(color);
        }
    }
}

fn parse_theme(source: &str) -> Result<Theme, Box<dyn std::error::Error>> {
    let theme: Theme = toml::from_str(source)?;
    if theme.keys.is_empty()
        || theme.keys.len() > 6
        || !(25..=100).contains(&theme.width_percent)
        || !(20..=90).contains(&theme.height_percent)
        || !(8..=32).contains(&theme.font_size)
        || theme.anchor.iter().any(|&coordinate| coordinate > 100)
    {
        return Err("invalid console shortcut, dimensions or font size".into());
    }
    Ok(theme)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_edits_unicode_history_selection_and_completion() {
        let theme = parse_theme(include_str!("../../ui/developer-console.toml")).unwrap();
        let mut console = Console::new(theme, BTreeMap::new());
        let key = |console: &mut Console, key: NamedKey, mods: ModifiersState| {
            console.key(&Key::Named(key), None, mods, false, || {
                panic!("unexpected clipboard request")
            })
        };
        console.replace("coc \"K’na Tha\"");
        key(&mut console, NamedKey::ArrowLeft, ModifiersState::SHIFT);
        key(&mut console, NamedKey::Delete, ModifiersState::empty());
        console.replace("\"");
        assert_eq!(
            key(&mut console, NamedKey::Enter, ModifiersState::empty()).as_deref(),
            Some("coc \"K’na Tha\"")
        );
        console.replace("draft");
        key(&mut console, NamedKey::ArrowUp, ModifiersState::empty());
        assert_eq!(console.input, "coc \"K’na Tha\"");
        key(&mut console, NamedKey::ArrowDown, ModifiersState::empty());
        assert_eq!(console.input, "draft");
        console.move_cursor(0, true);
        console.replace("sho");
        key(&mut console, NamedKey::Tab, ModifiersState::empty());
        assert_eq!(console.input, "sho");
        console.move_cursor(0, true);
        console.replace("showc");
        key(&mut console, NamedKey::Tab, ModifiersState::empty());
        assert_eq!(console.input, "showcollision ");
        assert!(console.is_toggle(
            PhysicalKey::Code(KeyCode::Backquote),
            &Key::Character("^".into())
        ));
    }

    #[test]
    fn default_and_external_theme_validation() {
        let source = include_str!("../../ui/developer-console.toml");
        assert!(parse_theme(source).is_ok());
        assert!(parse_theme(&source.replace("font_size = 17", "font_size = 700")).is_err());
        assert!(parse_theme(&source.replace("F10", "A")).is_err());
    }
}
