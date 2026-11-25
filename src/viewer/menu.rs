//! Small in-game menu drawn through the same sprite compositor as the world.

use arcanum_rs::{
    art::{DecodedFrame, SpriteFrame},
    compositor::SpriteDraw,
    content::{Source, art::LoadedFrame},
    vfs::{MountId, Storage},
};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Page {
    Pause,
    Options,
    Controls,
}

#[derive(Clone, Copy)]
pub(super) struct Settings {
    pub zoom: u8,
    pub lighting: bool,
    pub shadows: bool,
    pub run: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            zoom: 1,
            lighting: true,
            shadows: true,
            run: false,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Command {
    Continue,
    Quit,
    Stay,
}

pub(super) struct Menu {
    page: Page,
    index: usize,
}

impl Menu {
    pub(super) fn new() -> Self {
        Self {
            page: Page::Pause,
            index: 0,
        }
    }

    fn rows(&self) -> usize {
        match self.page {
            Page::Pause => 3,
            Page::Options => 6,
            Page::Controls => 1,
        }
    }

    pub(super) fn adjust(&mut self, step: i32) {
        self.index = (self.index as i32 + step).rem_euclid(self.rows() as i32) as usize;
    }

    pub(super) fn back(&mut self) -> Command {
        self.index = 0;
        match self.page {
            Page::Pause => Command::Continue,
            Page::Options => {
                self.page = Page::Pause;
                Command::Stay
            }
            Page::Controls => {
                self.page = Page::Options;
                Command::Stay
            }
        }
    }

    pub(super) fn activate(&mut self, settings: &mut Settings) -> Command {
        match (self.page, self.index) {
            (Page::Pause, 0) => return Command::Continue,
            (Page::Pause, 1) => {
                self.page = Page::Options;
                self.index = 0;
            }
            (Page::Pause, 2) => return Command::Quit,
            (Page::Options, 0) => settings.zoom = settings.zoom % 3 + 1,
            (Page::Options, 1) => settings.lighting = !settings.lighting,
            (Page::Options, 2) => settings.shadows = !settings.shadows,
            (Page::Options, 3) => settings.run = !settings.run,
            (Page::Options, 4) => {
                self.page = Page::Controls;
                self.index = 0;
            }
            _ => return self.back(),
        }
        Command::Stay
    }

    pub(super) fn click(
        &mut self,
        point: (u32, u32),
        size: (u32, u32),
        percent: u16,
        settings: &mut Settings,
    ) -> Command {
        let (width, height) = dimensions(size, percent);
        let (x, y) = origin(size, (width, height));
        let in_x =
            ((i64::from(point.0) - i64::from(x)) * i64::from(WIDTH)).div_euclid(i64::from(width));
        let in_y = ((i64::from(point.1) - i64::from(y)) * i64::from(HEIGHT))
            .div_euclid(i64::from(height))
            - i64::from(self.row_start());
        if !(6..WIDTH as i64 - 6).contains(&in_x) || in_y < 0 {
            return Command::Stay;
        }
        let row = in_y as usize / ROW as usize;
        if row >= self.rows() {
            return Command::Stay;
        }
        self.index = row;
        self.activate(settings)
    }

    fn row_start(&self) -> u32 {
        if self.page == Page::Controls { 105 } else { 30 }
    }

    pub(super) fn frame(
        &self,
        settings: Settings,
        backend: &str,
        percent: u16,
        viewport: (u32, u32),
    ) -> Arc<LoadedFrame> {
        let mut frame = SpriteFrame {
            width: WIDTH,
            height: HEIGHT,
            hotspot: (0, 0),
            offset: (0, 0),
            pixels: vec![[31, 26, 22, 255]; (WIDTH * HEIGHT) as usize],
        };
        let title = match self.page {
            Page::Pause => "PAUSED",
            Page::Options => "OPTIONS",
            Page::Controls => "CONTROLS",
        };
        let on = |enabled| if enabled { "ON" } else { "OFF" };
        let labels: Vec<String> = match self.page {
            Page::Pause => ["RESUME", "OPTIONS", "QUIT"].map(String::from).into(),
            Page::Options => vec![
                format!("ZOOM  {}X  DEFAULT 1X", settings.zoom),
                format!("LIGHTING  {}", on(settings.lighting)),
                format!("SHADOWS  {}", on(settings.shadows)),
                format!("RUN BY DEFAULT  {}", on(settings.run)),
                "VIEW CONTROLS".into(),
                "BACK".into(),
            ],
            Page::Controls => vec!["BACK".into()],
        };
        print(&mut frame, 12, 10, title);
        if self.page == Page::Options {
            print(
                &mut frame,
                12,
                122,
                &format!("{backend}  RESTART TO CHANGE"),
            );
        }
        if self.page == Page::Controls {
            for (row, text) in [
                "WASD OR ARROWS  MOVE",
                "CLICK  MOVE OR USE",
                "SHIFT  CHANGE GAIT",
                "WHEEL  ZOOM    0  RESET",
                "ESC  PAUSE OR BACK",
            ]
            .iter()
            .enumerate()
            {
                print(&mut frame, 12, 32 + row as u32 * 12, text);
            }
        }
        for (index, label) in labels.iter().enumerate() {
            let y = self.row_start() + ROW * index as u32;
            let color = if index == self.index {
                [116, 77, 37, 255]
            } else {
                [50, 42, 35, 255]
            };
            for row in y..y + ROW - 1 {
                frame.pixels[(row * WIDTH + 6) as usize..(row * WIDTH + WIDTH - 6) as usize]
                    .fill(color);
            }
            print(&mut frame, 12, y + 4, label);
        }
        let (width, height) = dimensions(viewport, percent);
        if (width, height) != (WIDTH, HEIGHT) {
            frame.pixels = (0..height)
                .flat_map(|y| {
                    (0..width)
                        .map(move |x| (y * HEIGHT / height * WIDTH + x * WIDTH / width) as usize)
                })
                .map(|index| frame.pixels[index])
                .collect();
            frame.width = width;
            frame.height = height;
        }
        let source = Source {
            virtual_path: "play/menu".into(),
            mount_id: MountId::GENERATED,
            mount_path: Default::default(),
            loose_path: None,
            storage: Storage::Loose,
            mod_id: None,
        };
        Arc::new(LoadedFrame {
            source,
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
}

const WIDTH: u32 = 190;
const HEIGHT: u32 = 138;
const ROW: u32 = 14;

fn dimensions(viewport: (u32, u32), percent: u16) -> (u32, u32) {
    let scale = u32::from(percent)
        .min(viewport.0.saturating_mul(100) / WIDTH)
        .min(viewport.1.saturating_mul(100) / HEIGHT);
    ((WIDTH * scale / 100).max(1), (HEIGHT * scale / 100).max(1))
}
fn origin(size: (u32, u32), frame: (u32, u32)) -> (i32, i32) {
    (
        (size.0 as i32 - frame.0 as i32) / 2,
        (size.1 as i32 - frame.1 as i32) / 2,
    )
}
pub(super) fn layer(asset: &LoadedFrame, size: (u32, u32)) -> SpriteDraw<'_> {
    SpriteDraw {
        frame: &asset.decoded.frame,
        anchor: origin(
            size,
            (asset.decoded.frame.width, asset.decoded.frame.height),
        ),
    }
}

fn print(frame: &mut SpriteFrame, x: u32, y: u32, text: &str) {
    for (i, glyph) in text.bytes().enumerate() {
        for (dy, row) in glyph_rows(glyph).iter().enumerate() {
            for dx in 0..5 {
                let px = x + 6 * i as u32 + dx;
                if px < WIDTH && row & (16 >> dx) != 0 {
                    frame.pixels[((y + dy as u32) * WIDTH + px) as usize] = [245, 226, 187, 255];
                }
            }
        }
    }
}

pub(super) fn developer_status(text: &str) -> Arc<LoadedFrame> {
    let width = text.len() as u32 * 12 + 16;
    let mut frame = SpriteFrame {
        width,
        height: 28,
        hotspot: (0, 0),
        offset: (0, 0),
        pixels: vec![[31, 26, 22, 255]; width as usize * 28],
    };
    for (index, glyph) in text.bytes().enumerate() {
        for (y, row) in glyph_rows(glyph).into_iter().enumerate() {
            for x in 0..5 {
                if row & (16 >> x) != 0 {
                    let at = (7 + y * 2) * width as usize + 8 + index * 12 + x * 2;
                    for offset in [0, 1, width as usize, width as usize + 1] {
                        frame.pixels[at + offset] = [245, 226, 187, 255];
                    }
                }
            }
        }
    }
    super::hud::generated_frame("developer/status", frame)
}

pub(super) fn glyph_rows(c: u8) -> [u8; 7] {
    match c {
        b'A' => [14, 17, 17, 31, 17, 17, 17],
        b'B' => [30, 17, 17, 30, 17, 17, 30],
        b'C' => [14, 17, 16, 16, 16, 17, 14],
        b'D' => [30, 17, 17, 17, 17, 17, 30],
        b'E' => [31, 16, 16, 30, 16, 16, 31],
        b'F' => [31, 16, 16, 30, 16, 16, 16],
        b'G' => [14, 17, 16, 23, 17, 17, 14],
        b'H' => [17, 17, 17, 31, 17, 17, 17],
        b'I' => [14, 4, 4, 4, 4, 4, 14],
        b'J' => [7, 2, 2, 2, 2, 18, 12],
        b'K' => [17, 18, 20, 24, 20, 18, 17],
        b'L' => [16, 16, 16, 16, 16, 16, 31],
        b'M' => [17, 27, 21, 21, 17, 17, 17],
        b'N' => [17, 25, 21, 19, 17, 17, 17],
        b'O' => [14, 17, 17, 17, 17, 17, 14],
        b'P' => [30, 17, 17, 30, 16, 16, 16],
        b'Q' => [14, 17, 17, 17, 21, 18, 13],
        b'R' => [30, 17, 17, 30, 20, 18, 17],
        b'S' => [15, 16, 16, 14, 1, 1, 30],
        b'T' => [31, 4, 4, 4, 4, 4, 4],
        b'U' => [17, 17, 17, 17, 17, 17, 14],
        b'V' => [17, 17, 17, 17, 17, 10, 4],
        b'W' => [17, 17, 17, 21, 21, 21, 10],
        b'X' => [17, 17, 10, 4, 10, 17, 17],
        b'Y' => [17, 17, 10, 4, 4, 4, 4],
        b'Z' => [31, 1, 2, 4, 8, 16, 31],
        b'0' => [14, 17, 19, 21, 25, 17, 14],
        b'1' => [4, 12, 4, 4, 4, 4, 14],
        b'2' => [14, 17, 1, 2, 4, 8, 31],
        b'3' => [30, 1, 1, 14, 1, 1, 30],
        b'4' => [2, 6, 10, 18, 31, 2, 2],
        b'5' => [31, 16, 16, 30, 1, 1, 30],
        b'6' => [14, 16, 16, 30, 17, 17, 14],
        b'7' => [31, 1, 2, 4, 8, 8, 8],
        b'8' => [14, 17, 17, 14, 17, 17, 14],
        b'9' => [14, 17, 17, 15, 1, 1, 14],
        b'.' => [0, 0, 0, 0, 0, 0, 4],
        b':' => [0, 0, 4, 0, 0, 4, 0],
        b'-' => [0, 0, 0, 31, 0, 0, 0],
        b'/' => [1, 1, 2, 4, 8, 16, 16],
        b'?' => [14, 17, 1, 2, 4, 0, 4],
        _ => [0; 7],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_click_and_keyboard_only_change_the_selected_setting() {
        let mut menu = Menu::new();
        let mut settings = Settings::default();
        menu.adjust(1);
        assert!(menu.activate(&mut settings) == Command::Stay);
        assert_eq!(menu.page, Page::Options);
        let size = (640, 360);
        let (x, y) = origin(size, (WIDTH * 2, HEIGHT * 2));
        let click = (x as u32 + 24, y as u32 + 2 * (30 + ROW));
        assert!(menu.click(click, size, 200, &mut settings) == Command::Stay);
        assert!(!settings.lighting && settings.shadows);
        assert!(menu.back() == Command::Stay);
        assert!(menu.back() == Command::Continue);
    }
}
