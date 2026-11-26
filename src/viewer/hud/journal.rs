//! The Classic quest spread, using the HUD's native text and layout.
use super::*;
use arcanum_rs::content::{
    quest::{Catalog, Entry, Status},
    world::InteractiveWorld,
};

const PAGES: [Binding; 2] = [Binding::JournalLeft, Binding::JournalRight];
const HEADERS: [Binding; 2] = [Binding::JournalLeftHeader, Binding::JournalRightHeader];
const FOOTERS: [Binding; 2] = [Binding::JournalLeftFooter, Binding::JournalRightFooter];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Style {
    active: Color,
    completed: Color,
    failed: Color,
}

struct Row {
    text: Text,
    top: u32,
    strike: bool,
}

#[derive(Default)]
pub(super) struct Journal {
    pub(super) open: bool,
    spread: usize,
    pages: Vec<Vec<Row>>,
    entries: Vec<(Status, String)>,
}

impl Journal {
    fn max_spread(&self) -> usize {
        self.pages.len().saturating_sub(1) / 2
    }

    fn can_turn(&self, backwards: bool) -> bool {
        if backwards {
            self.spread > 0
        } else {
            self.spread < self.max_spread()
        }
    }
}

impl Hud {
    pub(in crate::viewer) fn has_journal(&self) -> bool {
        self.journal_style.is_some()
            && self
                .components
                .iter()
                .any(|c| c.config.journal_panel && !c.config.hidden)
            && PAGES.iter().all(|&binding| {
                self.bound(binding)
                    .is_some_and(|index| self.components[index].label.is_some())
            })
    }

    pub(in crate::viewer) fn journal_open(&self) -> bool {
        self.journal.open
    }

    pub(in crate::viewer) fn close_journal(&mut self) {
        if self.journal.open {
            self.toggle_journal();
        }
    }

    pub(in crate::viewer) fn toggle_journal(&mut self) {
        if !self.has_journal() {
            return;
        }
        let open = !self.journal.open;
        self.close_inventory();
        self.cancel_press();
        self.journal.open = open;
        self.focus = None;
        self.clear_layout();
    }

    pub(in crate::viewer) fn refresh_journal(
        &mut self,
        catalog: &Catalog,
        world: &InteractiveWorld,
    ) {
        if !self.journal.open {
            return;
        }
        let entries = catalog.entries(
            &world.script_state().quests,
            world.character_statistics().attributes[4],
        );
        let entries: Vec<_> = entries
            .iter()
            .map(|entry| (entry.status, entry_text(catalog, entry)))
            .collect();
        if entries == self.journal.entries && !self.journal.pages.is_empty() {
            return;
        }
        let [Some(left), Some(right)] = PAGES.map(|binding| self.bound(binding)) else {
            return;
        };
        let [width, height] = std::array::from_fn(|axis| {
            self.components[left].config.size[axis].min(self.components[right].config.size[axis])
        });
        let label = self.components[left].label.as_ref().unwrap();
        let style = self.journal_style.as_ref().unwrap();
        self.journal.pages = paginate(&entries, label, style, width, height);
        self.journal.entries = entries;
        self.journal.spread = self.journal.spread.min(self.journal.max_spread());
        let title = format!("-{}-", catalog.label(1).unwrap_or("Quests"));
        for binding in HEADERS {
            self.set_label(binding, &title);
        }
        self.paint_journal();
    }

    fn paint_journal(&mut self) {
        for (side, binding) in PAGES.into_iter().enumerate() {
            let page = self.journal.spread * 2 + side;
            let Some(index) = self.bound(binding) else {
                continue;
            };
            let component = &mut self.components[index];
            let [w, h] = component.config.size;
            let mut frame = sprite([w, h], [0; 4]);
            if let Some(rows) = self.journal.pages.get(page) {
                for row in rows {
                    let Some(line) = row.text.lines(w, false, 100).next() else {
                        continue;
                    };
                    let height = line.height.min(h - row.top);
                    let bounds = Rect {
                        x: 0,
                        y: 0,
                        w,
                        h: height,
                    };
                    let color = row.text.font.color;
                    let mut raster = sprite([w, height], [0; 4]);
                    draw_text_colored(
                        &mut raster,
                        &row.text,
                        (Align::Left, false, 0),
                        bounds,
                        bounds,
                        100,
                        (color, 0),
                    );
                    if row.strike {
                        let from = ((height / 2) * w) as usize;
                        raster.pixels[from..from + line.width.min(w) as usize].fill(color);
                    }
                    let from = (row.top * w) as usize;
                    frame.pixels[from..from + raster.pixels.len()].copy_from_slice(&raster.pixels);
                }
            }
            component.art = generated_frame(&component.config.id, frame);
            self.set_label(FOOTERS[side], &format!("-{}-", page + 1));
        }
    }

    pub(in crate::viewer) fn turn_journal(&mut self, backwards: bool) {
        if self.journal.can_turn(backwards) {
            if backwards {
                self.journal.spread -= 1;
            } else {
                self.journal.spread += 1;
            }
            self.paint_journal();
        }
    }

    pub(super) fn journal_action_visible(&self, action: Action) -> bool {
        match action {
            Action::JournalPrevious => self.journal.can_turn(true),
            Action::JournalNext => self.journal.can_turn(false),
            _ => true,
        }
    }

    pub(in crate::viewer) fn journal_scroll(&mut self, point: Option<(u32, u32)>, backwards: bool) {
        if point.is_some_and(|point| {
            PAGES.iter().any(|&binding| {
                self.bounds(binding)
                    .is_some_and(|(rect, _)| rect.contains(point))
            })
        }) {
            self.turn_journal(backwards);
        }
    }

    pub(super) fn journal_diagnostic(&self) -> serde_json::Value {
        use serde_json::json;
        let bounds = |binding| {
            self.bounds(binding)
                .map(|(r, _)| [r.x, r.y, r.w as i32, r.h as i32])
        };
        let controls: BTreeMap<_, _> = self
            .components
            .iter()
            .enumerate()
            .filter(|(index, _)| {
                matches!(
                    self.available_action(*index),
                    Some(
                        Action::ToggleJournal
                            | Action::JournalQuests
                            | Action::JournalPrevious
                            | Action::JournalNext
                    )
                )
            })
            .filter_map(|(index, c)| {
                let r = self.placed[index].as_ref()?.rect;
                Some((&c.config.id, [r.x, r.y, r.w as i32, r.h as i32]))
            })
            .collect();
        json!({"open":self.journal.open,"spread":self.journal.spread,"spread_count":self.journal.max_spread()+1,
            "pages":PAGES.map(bounds),"controls":controls})
    }
}

fn entry_text(catalog: &Catalog, entry: &Entry<'_>) -> String {
    let days = entry.minute / 1440;
    let hour = entry.minute / 60 % 24;
    let month = catalog.label(7 + (days / 30 % 12) as i32).unwrap_or("?");
    let status = catalog
        .label(19 + entry.status as i32)
        .unwrap_or(match entry.status {
            Status::Unknown => "Unknown",
            Status::Mentioned => "Mentioned",
            Status::Accepted | Status::Achieved => "Accepted",
            Status::Completed => "Completed",
            Status::OtherCompleted => "Done by rival",
            Status::Botched => "Botched",
        });
    let missing = format!("Quest {} (description unavailable)", entry.id);
    format!(
        "{month} {}, {}  {}:{:02}{}m    {status}\n{}",
        days % 30 + 1,
        catalog.start_year() + days / 360,
        (hour + 11) % 12 + 1,
        entry.minute % 60,
        if hour >= 12 { 'p' } else { 'a' },
        entry.description.unwrap_or(&missing)
    )
}

fn paginate(
    entries: &[(Status, String)],
    label: &Label,
    style: &Style,
    width: u32,
    height: u32,
) -> Vec<Vec<Row>> {
    let mut pages = vec![Vec::new()];
    let mut top = 0;
    for (status, value) in entries {
        let text = label.text(value);
        let (color, strike) = match status {
            Status::Accepted | Status::Achieved => (style.active, false),
            Status::Completed => (style.completed, true),
            Status::OtherCompleted | Status::Botched => (style.failed, true),
            _ => (text.font.color, false),
        };
        let lines: Vec<_> = text.lines(width, true, 100).collect();
        if let Some(last) = lines.last()
            && top > 0
            && top + last.top + last.height > height
        {
            pages.push(Vec::new());
            top = 0;
        }
        for line in lines {
            if top > 0 && top + line.height > height {
                pages.push(Vec::new());
                top = 0;
            }
            let mut font = text.font.clone();
            font.color = color;
            pages.last_mut().unwrap().push(Row {
                text: Text {
                    glyphs: text.glyphs[line.glyphs].to_vec(),
                    font,
                    native_height: text.native_height,
                },
                top,
                strike,
            });
            top = top.saturating_add(line.height + text.font.line_gap);
        }
        top = top.saturating_add(text.size());
    }
    pages
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn journal_paginates_wrapped_entries_and_bounds_spread_turns() {
        let mut glyph = sprite([5, 10], [255; 4]);
        glyph.hotspot.0 = 5;
        let label = Label {
            glyphs: BTreeMap::from([('?', Arc::new(glyph))]),
            font: Font {
                source: "fixture".into(),
                size: 0,
                color: [0, 0, 0, 255],
                bold: false,
                line_gap: 0,
                fallback: None,
            },
            native_height: 10,
            value: String::new(),
            raster_slice: None,
        };
        let style = Style {
            active: [0, 0, 150, 255],
            completed: [0, 120, 0, 255],
            failed: [150, 0, 0, 255],
        };
        let pages = paginate(
            &[(Status::Completed, "ABCD EFGH IJKL MNOP QRST".into())],
            &label,
            &style,
            20,
            20,
        );
        assert_eq!(pages.iter().map(Vec::len).collect::<Vec<_>>(), [2, 2, 1]);
        assert!(
            pages
                .iter()
                .flatten()
                .all(|row| row.top < 20 && row.strike && row.text.font.color == style.completed)
        );
        let mut journal = Journal {
            pages,
            ..Journal::default()
        };
        assert!(journal.can_turn(false) && !journal.can_turn(true));
        journal.spread = 1;
        assert!(journal.can_turn(true) && !journal.can_turn(false));
    }
}
