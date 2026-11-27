//! Settings shared by gameplay and the Classic menu, plus developer status text.

use arcanum_rs::{art::SpriteFrame, content::art::LoadedFrame};
use std::sync::Arc;

#[derive(Clone, Copy)]
pub(crate) struct Settings {
    pub zoom: u8,
    pub follow: bool,
    pub edge_scroll: bool,
    pub keyboard_pan: bool,
    pub recenter_key: winit::keyboard::KeyCode,
    pub lighting: bool,
    pub shadows: bool,
    pub run: bool,
    pub difficulty: arcanum_rs::content::Difficulty,
    pub combat_mode: arcanum_rs::content::world::CombatMode,
    pub audio: crate::app_config::Audio,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            zoom: 1,
            follow: false,
            edge_scroll: true,
            keyboard_pan: true,
            recenter_key: winit::keyboard::KeyCode::Home,
            lighting: true,
            shadows: true,
            run: false,
            difficulty: Default::default(),
            combat_mode: Default::default(),
            audio: Default::default(),
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
