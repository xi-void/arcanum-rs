//! Fit the dialogue transcript and wrapped responses within the gameplay panel band.
use super::*;
use arcanum_rs::content::dialogue::Conversation;

#[derive(Default)]
pub(super) struct DialogueLayout {
    turn: Option<(String, i32)>,
    responses: Scroll,
    speech: Scroll,
}

impl DialogueLayout {
    pub(super) fn diagnostics(&self) -> serde_json::Value {
        let bounds = |scroll: &Scroll| {
            let viewport = scroll.viewport.map(|r| [r.x, r.y, r.w as i32, r.h as i32]);
            serde_json::json!({"offset":scroll.offset,"limit":scroll.limit,"bounds":viewport})
        };
        serde_json::json!({"speech":bounds(&self.speech),"responses":bounds(&self.responses)})
    }
}

#[derive(Default)]
struct Scroll {
    offset: u32,
    limit: u32,
    viewport: Option<Rect>,
}

impl Scroll {
    fn fit(&mut self, content: u32, viewport: u32) {
        self.limit = content.saturating_sub(viewport);
        self.offset = self.offset.min(self.limit);
    }
}

fn scaled(value: u32, scale: u32) -> u32 {
    (value * scale + 50) / 100
}

impl Hud {
    pub(in crate::viewer) fn set_dialogue(
        &mut self,
        session: Option<&Conversation>,
        npc_at: Option<(i32, i32)>,
        size: (u32, u32),
    ) {
        if self.dialogue_open != session.is_some() {
            self.dialogue_open = session.is_some();
            self.clear_layout();
        }
        self.layout(size);
        let Some(session) = session else {
            self.dialogue_layout = DialogueLayout::default();
            return;
        };
        if !self
            .dialogue_layout
            .turn
            .as_ref()
            .is_some_and(|(target, line)| *target == session.target && *line == session.turn.line)
        {
            self.dialogue_layout = DialogueLayout {
                turn: Some((session.target.clone(), session.turn.line)),
                ..Default::default()
            };
        }
        self.set_label(Binding::NpcContext, &session.speaker);
        self.set_label(Binding::DialogueText, &session.turn.text);
        self.dialogue_available =
            std::array::from_fn(|row| session.turn.responses.get(row).is_some_and(|r| r.available));
        let mut rows = Vec::new();
        for index in 0..self.components.len() {
            if let Some(row) = self.components[index].config.response {
                let response = session.turn.responses.get(row);
                self.write_label(index, response.map_or("", |r| &r.text));
                if !self.components[index].config.hidden {
                    rows.push((row, index, response.is_some()));
                }
            }
        }
        let band = self.panel_band;
        let scale = self.effective_scale;
        if scale == 0 {
            return;
        }
        let Some(speaker) = self.bound(Binding::DialogueText) else {
            return;
        };
        let speech_config = &self.components[speaker].config;
        let speech_height = self.components[speaker].text_height(speech_config.size[0]);
        let native_band = band.h * 100 / scale;
        let speech_reserve = speech_height
            .min(native_band / 2)
            .min(speech_config.size[1]);
        let mut speech_band = band;
        rows.sort_unstable();
        if let Some(parent) = self.response_parent(&rows) {
            let config = &self.components[parent].config;
            let mut panel = layout_rect(config, band, scale);
            let mut height = config.size[1] as i32;
            let mut growth = 0;
            let mut positions = Vec::new();
            for (_, index, present) in rows {
                self.placed[index] = None;
                if !present {
                    continue;
                }
                let c = &self.components[index];
                let [width, minimum] = c.config.size;
                let row_height = c.text_height(width).max(minimum);
                let top = c.config.offset[1] + growth;
                growth += row_height as i32 - minimum as i32;
                height = height.max(top + row_height as i32);
                positions.push((index, top, row_height));
            }
            let height = height as u32;
            let visible = height.min(native_band.saturating_sub(speech_reserve));
            let visible_px = scaled(visible, scale);
            panel.y -= visible_px as i32 - panel.h as i32;
            panel.h = visible_px;
            let clip = panel.intersect(band);
            self.placed[parent] = clip.map(|rect| Placement {
                full: panel,
                rect,
                scale,
                live_asset: None,
            });
            let scroll = &mut self.dialogue_layout.responses;
            scroll.fit(height, visible);
            scroll.viewport = clip;
            let offset = scroll.offset as i32;
            if let Some(clip) = clip {
                speech_band.h = (clip.y - band.y) as u32;
                for (index, top, height) in positions {
                    let config = &self.components[index].config;
                    let x = panel.x + config.offset[0] * scale as i32 / 100;
                    let relative = top - offset;
                    let y =
                        panel.y + scaled(relative.unsigned_abs(), scale) as i32 * relative.signum();
                    self.place_dialogue_text(index, x, y, height, clip, scale);
                }
            }
        }

        let config = &self.components[speaker].config;
        if config.hidden {
            return;
        }
        let authored = layout_rect(config, speech_band, scale);
        let visible = (speech_band.h * 100 / scale)
            .min(config.size[1])
            .min(speech_height);
        let height = scaled(visible, scale);
        let (x, y) = npc_at.map_or(
            (
                authored.x + authored.w as i32 / 2,
                authored.y + height as i32,
            ),
            |at| at,
        );
        let viewport = Rect {
            x: (x - authored.w as i32 / 2)
                .clamp(band.x, band.x + band.w.saturating_sub(authored.w) as i32),
            y: (y - height as i32).clamp(
                speech_band.y,
                speech_band.y + speech_band.h.saturating_sub(height) as i32,
            ),
            w: authored.w,
            h: height,
        };
        let scroll = &mut self.dialogue_layout.speech;
        scroll.fit(speech_height, visible);
        scroll.viewport = viewport.intersect(speech_band);
        let offset = scaled(scroll.offset, scale) as i32;
        self.placed[speaker] = None;
        if let Some(clip) = scroll.viewport {
            self.place_dialogue_text(
                speaker,
                viewport.x,
                viewport.y - offset,
                speech_height,
                clip,
                scale,
            );
        }
    }

    // Existing custom themes with independently positioned rows keep their authored bounds.
    fn response_parent(&self, rows: &[(usize, usize, bool)]) -> Option<usize> {
        let parent = self.components[rows.first()?.1].parent?;
        let container = &self.components[parent];
        (container.parent.is_none()
            && matches!(
                container.config.anchor,
                Anchor::Bottom | Anchor::BottomLeft | Anchor::BottomRight
            )
            && container.config.dialogue_only
            && !container.config.hidden
            && rows.iter().all(|&(_, i, _)| {
                let row = &self.components[i];
                row.parent == Some(parent) && matches!(row.config.anchor, Anchor::TopLeft)
            }))
        .then_some(parent)
    }

    fn place_dialogue_text(
        &mut self,
        index: usize,
        x: i32,
        y: i32,
        height: u32,
        clip: Rect,
        scale: u32,
    ) {
        let c = &mut self.components[index];
        let width = c.config.size[0];
        let full = Rect {
            x,
            y,
            w: scaled(width, scale),
            h: scaled(height, scale),
        };
        let Some(rect) = full.intersect(clip) else {
            return;
        };
        // Cache the visible native raster. Scrolling never allocates a texture
        // as tall as the entire text and camera motion can reuse the same slice.
        let from = ((rect.y - y) as u32 * 100 / scale).saturating_sub(1);
        let to = (((rect.y - y) as u32 + rect.h) * 100)
            .div_ceil(scale)
            .saturating_add(1)
            .min(height);
        let native = Rect {
            x: 0,
            y: 0,
            w: width,
            h: height,
        };
        let slice = Rect {
            y: from as i32,
            h: to - from,
            ..native
        };
        let top = scaled(from, scale);
        let drawn = Rect {
            y: y + top as i32,
            h: scaled(to, scale) - top,
            ..full
        };
        if let Some(label) = &mut c.label
            && label.raster_slice != Some((from, to))
        {
            label.raster_slice = Some((from, to));
            let render = |c: &Component| raster(c, &c.base, native, slice, 100, None);
            c.art = render(c);
            c.update_text_states(render);
        }
        self.placed[index] = Some(Placement {
            full: drawn,
            rect,
            scale,
            live_asset: None,
        });
    }

    pub(in crate::viewer) fn scroll_dialogue(
        &mut self,
        point: Option<(u32, u32)>,
        up: bool,
        page: bool,
    ) {
        let layout = &mut self.dialogue_layout;
        let scroll = if point.is_some_and(|p| layout.speech.viewport.is_some_and(|r| r.contains(p)))
            || (page && layout.responses.limit == 0)
        {
            &mut layout.speech
        } else {
            &mut layout.responses
        };
        let step = if page {
            scroll.viewport.map_or(1, |r| {
                (r.h * 100 / self.effective_scale).saturating_sub(24).max(1)
            })
        } else {
            24
        };
        scroll.offset = if up {
            scroll.offset.saturating_sub(step)
        } else {
            scroll.offset.saturating_add(step).min(scroll.limit)
        };
        self.clear_dialogue_choices();
    }
}
