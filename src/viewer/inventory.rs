//! Inventory input and presentation. Only the world commits ownership changes.
use super::*;
use arcanum_rs::content::world::{InventoryItem, InventoryView};
use hud::{Action, Binding};
use std::path::PathBuf;

const SIDES: [Binding; 2] = [Binding::ContainerGrid, Binding::PlayerGrid];
type Point = (u32, u32);

struct Icon {
    id: String,
    side: usize,
    slot: usize,
    shape: [usize; 2],
    image: Arc<LoadedFrame>,
    tip: String,
}
impl Icon {
    fn contains(&self, cell: usize) -> bool {
        let (x, y) = (cell % 10, cell / 10);
        (self.slot % 10..self.slot % 10 + self.shape[0]).contains(&x)
            && (self.slot / 10..self.slot / 10 + self.shape[1]).contains(&y)
    }
}
struct Drag {
    id: String,
    side: usize,
    offset: [usize; 2],
    image: Arc<LoadedFrame>,
}
pub(super) struct InventoryUi {
    pub path: PathBuf,
    pub scale: u16,
    pub panel: Option<Hud>,
    revision: Option<u64>,
    container: Option<String>,
    icons: Vec<Icon>,
    scroll: [usize; 2],
    cursor: Option<Point>,
    hovered: Option<usize>,
    selected: Option<String>,
    drag: Option<Drag>,
    notice: String,
}

impl InventoryUi {
    pub fn new(path: PathBuf, scale: u16) -> Self {
        Self {
            path,
            scale,
            panel: None,
            revision: None,
            container: None,
            icons: Vec::new(),
            scroll: [0; 2],
            cursor: None,
            hovered: None,
            selected: None,
            drag: None,
            notice: String::new(),
        }
    }
    fn close(&mut self) {
        self.panel = None;
        self.revision = None;
        self.container = None;
        self.icons.clear();
        self.hovered = None;
        self.selected = None;
        self.drag = None;
        self.scroll = [0; 2];
        self.notice.clear();
    }
    fn icon(world: &mut InteractiveWorld, item: &InventoryItem, side: usize) -> Option<Icon> {
        let image = world.inventory_icon(
            item.icon.as_deref()?,
            ((item.inventory_art? >> 4) & 3) as u8,
        )?;
        let name = item.name.as_deref().unwrap_or("Unnamed item");
        let kind = item.category.unwrap_or("item");
        let mut tip = format!(" {name}\n {kind}");
        if let Some(q) = item.quantity {
            tip.push_str(&format!("   ×{q}"));
        }
        Some(Icon {
            id: item.id.clone(),
            side,
            slot: item.slot?,
            shape: item.footprint?,
            image,
            tip,
        })
    }
    pub fn prepare(&mut self, world: &mut InteractiveWorld, size: Point) -> Result<(), String> {
        if !world.inventory_is_open() {
            if self.panel.is_some() {
                self.close();
            }
            return Ok(());
        }
        if self.revision != Some(world.inventory_revision()) {
            let InventoryView {
                container,
                player,
                contents,
                container_name,
                ..
            } = world.inventory_view();
            if self.panel.is_none() || self.container != container {
                self.close();
                let path = self.path.join(if container.is_some() {
                    "container.toml"
                } else {
                    "inventory.toml"
                });
                self.panel = Some(
                    Hud::load(
                        &path,
                        &mut *world
                            .inventory_vfs()
                            .ok_or("Inventory resources unavailable")?,
                        Some(self.scale),
                    )
                    .map_err(|e| e.to_string())?,
                );
                self.container = container;
            }
            self.panel.as_mut().unwrap().set_label(
                Binding::ContainerName,
                container_name.as_deref().unwrap_or("Container"),
            );
            self.icons.clear();
            for (side, items) in [contents.as_deref().unwrap_or_default(), &player]
                .into_iter()
                .enumerate()
            {
                for item in items {
                    let icon = Self::icon(world, item, side).ok_or_else(|| {
                        format!("Unresolved inventory grid or artwork for {}", item.id)
                    })?;
                    self.icons.push(icon);
                }
            }
            self.drag = None;
            self.hovered = None;
            self.revision = Some(world.inventory_revision());
            self.panel.as_mut().unwrap().layout(size);
            self.refresh();
        }
        let panel = self.panel.as_mut().unwrap();
        panel.set_scale(self.scale);
        panel.layout(size);
        self.tooltip();
        Ok(())
    }
    fn cell(&self, p: Point) -> Option<(usize, usize)> {
        let panel = self.panel.as_ref()?;
        SIDES.iter().enumerate().find_map(|(side, &binding)| {
            let (r, _) = panel.bounds(binding)?;
            if !r.contains(p) {
                return None;
            }
            let x = (p.0 as i32 - r.x) as usize * 10 / r.w as usize;
            let y = (p.1 as i32 - r.y) as usize * 10 / r.w as usize + self.scroll[side];
            Some((side, y * 10 + x))
        })
    }
    fn hit_item(&self) -> Option<usize> {
        let (side, slot) = self.cell(self.cursor?)?;
        self.icons
            .iter()
            .position(|i| i.side == side && i.contains(slot))
    }
    fn tooltip(&mut self) {
        if let Some(panel) = self.panel.as_mut() {
            let tip = if self.drag.is_some() {
                ""
            } else if !self.notice.is_empty() {
                &self.notice
            } else {
                self.hovered
                    .and_then(|i| self.icons.get(i))
                    .map_or("", |i| i.tip.as_str())
            };
            panel.tooltip(self.cursor.unwrap_or_default(), tip);
        }
    }
    fn refresh(&mut self) {
        self.hovered = self.hit_item();
        let Some(panel) = self.panel.as_mut() else {
            return;
        };
        for (side, binding) in SIDES.into_iter().enumerate() {
            panel.paint_surface(binding, |frame, fill, line, active| {
                let cell = frame.width as usize / 10;
                if cell == 0 {
                    return;
                }
                for (idx, p) in frame.pixels.iter_mut().enumerate() {
                    let x = idx % frame.width as usize;
                    let y = idx / frame.width as usize;
                    *p = if x.is_multiple_of(cell) || y.is_multiple_of(cell) {
                        line
                    } else {
                        fill
                    };
                }
                for (index, i) in self
                    .icons
                    .iter()
                    .enumerate()
                    .filter(|(_, i)| i.side == side)
                {
                    if self.drag.as_ref().is_some_and(|d| d.id == i.id) {
                        continue;
                    }
                    let x = (i.slot % 10 * cell) as i32;
                    let y =
                        (i.slot / 10) as i32 * cell as i32 - self.scroll[side] as i32 * cell as i32;
                    let w = (i.shape[0] * cell) as i32;
                    let h = (i.shape[1] * cell) as i32;
                    if y + h <= 0 || y >= frame.height as i32 {
                        continue;
                    }
                    let src = &i.image.decoded.frame;
                    let iw = (src.width as usize * cell / 32).max(1) as i32;
                    let ih = (src.height as usize * cell / 32).max(1) as i32;
                    for dy in 0.max(-y)..h.min(frame.height as i32 - y) {
                        for dx in 0..w.min(frame.width as i32 - x) {
                            let pixel = &mut frame.pixels
                                [((y + dy) * frame.width as i32 + x + dx) as usize];
                            if self.hovered == Some(index) || self.selected.as_ref() == Some(&i.id)
                            {
                                *pixel = active;
                            }
                            let (sx, sy) = (dx - (w - iw) / 2, dy - (h - ih) / 2);
                            if (0..iw).contains(&sx) && (0..ih).contains(&sy) {
                                let c = src.pixels[((sy as u32 * src.height / ih as u32)
                                    * src.width
                                    + sx as u32 * src.width / iw as u32)
                                    as usize];
                                if c[3] != 0 {
                                    *pixel = c;
                                }
                            }
                        }
                    }
                }
            });
        }
        self.tooltip();
    }
    fn scroll(&mut self, side: usize, delta: isize) {
        let Some((r, _)) = self.panel.as_ref().and_then(|p| p.bounds(SIDES[side])) else {
            return;
        };
        let rows = (r.h * 10 / r.w).max(1) as usize;
        let end = self
            .icons
            .iter()
            .filter(|i| i.side == side)
            .map(|i| i.slot / 10 + i.shape[1])
            .max()
            .unwrap_or(0);
        self.scroll[side] = self.scroll[side]
            .saturating_add_signed(delta)
            .min(end.saturating_sub(rows));
        self.refresh();
    }
    fn start_drag(&mut self) {
        let Some(index) = self.hit_item() else { return };
        let item = &self.icons[index];
        let (_, cell) = self.cell(self.cursor.unwrap()).unwrap();
        let (r, _) = self
            .panel
            .as_ref()
            .unwrap()
            .bounds(SIDES[item.side])
            .unwrap();
        let src = &item.image.decoded.frame;
        let mut frame = hud::sprite(
            [
                (src.width * r.w / 320).max(1),
                (src.height * r.w / 320).max(1),
            ],
            [0; 4],
        );
        hud::scale_into(src, &mut frame, None).expect("valid inventory frame");
        self.drag = Some(Drag {
            id: item.id.clone(),
            side: item.side,
            offset: [cell % 10 - item.slot % 10, cell / 10 - item.slot / 10],
            image: hud::generated_frame("held-item", frame),
        });
        self.selected = Some(item.id.clone());
        self.refresh();
    }
    fn commit(
        &mut self,
        world: &mut InteractiveWorld,
        id: &str,
        op: &str,
        owner: Option<&str>,
        slot: Option<usize>,
    ) {
        self.notice = world
            .inventory_transfer_at(id, op, owner, slot)
            .err()
            .unwrap_or_default();
    }
    fn release(&mut self, world: &mut InteractiveWorld, point: Point) {
        let Some(drag) = self.drag.take() else {
            match self.panel.as_mut().and_then(|p| p.release(point)) {
                Some(Action::InventoryClose) => world.close_inventory(),
                Some(Action::InventoryDrop) => self.drop_selected(world),
                _ => {}
            }
            return;
        };
        if let Some((side, slot)) = self.cell(point) {
            let x = (slot % 10).checked_sub(drag.offset[0]);
            let y = (slot / 10).checked_sub(drag.offset[1]);
            if let (Some(x), Some(y)) = (x, y) {
                let op = if side == drag.side {
                    "position"
                } else if side == 1 {
                    "take"
                } else {
                    "put"
                };
                let owner = if op == "position" && side == 1 {
                    None
                } else {
                    self.container.clone()
                };
                let slot = y * 10 + x;
                if op != "position" || !self.icons.iter().any(|i| i.id == drag.id && i.slot == slot)
                {
                    self.commit(world, &drag.id, op, owner.as_deref(), Some(slot));
                }
            }
        } else if drag.side == 1
            && self
                .panel
                .as_ref()
                .and_then(|p| p.bounds(Binding::InventoryDrop))
                .is_some_and(|(r, _)| r.contains(point))
        {
            self.commit(world, &drag.id, "drop", None, None);
        }
        self.refresh();
    }
    fn drop_selected(&mut self, world: &mut InteractiveWorld) {
        if let Some(id) = self.selected.clone() {
            self.commit(world, &id, "drop", None, None);
        }
    }
    pub fn draws(
        &self,
    ) -> impl Iterator<Item = (&Arc<LoadedFrame>, arcanum_rs::compositor::SpriteDraw<'_>)> {
        self.panel
            .iter()
            .flat_map(|p| p.draws(false))
            .chain(self.drag.iter().filter_map(|d| {
                let point = self.cursor?;
                Some((
                    &d.image,
                    arcanum_rs::compositor::SpriteDraw {
                        frame: &d.image.decoded.frame,
                        anchor: (point.0 as i32 + 2, point.1 as i32 + 2),
                    },
                ))
            }))
    }
}

impl Viewer<'_> {
    pub(super) fn inventory_event(&mut self, event: &WindowEvent) -> bool {
        if self.pause.is_some() || self.map_since.is_some() {
            return false;
        }
        let ViewContent::Stream {
            world,
            inventory: Some(ui),
            ..
        } = &mut self.content
        else {
            return false;
        };
        let window = &self.view.as_ref().unwrap().window;
        if !world.inventory_is_open() {
            if let WindowEvent::KeyboardInput { event, .. } = event
                && event.state.is_pressed()
                && !event.repeat
                && matches!(event.logical_key.as_ref(), Key::Character("i" | "I"))
            {
                if world.inventory_open(None).is_ok() {
                    self.navigation = Navigation::default();
                    self.player_deadline = world.player_moving().then_some(Instant::now());
                }
                window.request_redraw();
                return true;
            }
            return false;
        }
        match event {
            WindowEvent::KeyboardInput { event, .. }
                if event.state.is_pressed() && !event.repeat =>
            {
                match event.logical_key.as_ref() {
                    Key::Named(NamedKey::Escape) if ui.drag.take().is_some() => ui.refresh(),
                    Key::Named(NamedKey::Escape) | Key::Character("i" | "I") => {
                        world.close_inventory()
                    }
                    Key::Character("d" | "D") => ui.drop_selected(world),
                    _ => return true,
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let point = Some((position.x as u32, position.y as u32));
                self.cursor = point;
                ui.cursor = point;
                ui.notice.clear();
                let changed = ui.panel.as_mut().is_some_and(|p| p.hover(point));
                if ui.hovered != ui.hit_item() {
                    ui.refresh();
                } else if ui.hovered.is_none() && ui.drag.is_none() && !changed {
                    return true;
                } else {
                    ui.tooltip();
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let Some(p) = self.cursor else { return true };
                ui.cursor = Some(p);
                ui.notice.clear();
                match (*button, state.is_pressed()) {
                    (MouseButton::Left, true) => {
                        if ui.hit_item().is_some() {
                            ui.start_drag();
                        } else if let Some(panel) = ui.panel.as_mut() {
                            panel.press(p);
                        }
                    }
                    (MouseButton::Left, false) => ui.release(world, p),
                    (MouseButton::Right, false) if ui.container.is_some() => {
                        if let Some(i) = ui.hit_item() {
                            let id = ui.icons[i].id.clone();
                            let op = if ui.icons[i].side == 1 { "put" } else { "take" };
                            let owner = ui.container.clone();
                            ui.commit(world, &id, op, owner.as_deref(), None);
                        }
                    }
                    _ => return true,
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                if let Some((side, _)) = self.cursor.and_then(|p| ui.cell(p)) {
                    let dy = match delta {
                        MouseScrollDelta::LineDelta(_, y) => *y,
                        MouseScrollDelta::PixelDelta(p) => p.y as f32,
                    };
                    ui.scroll(side, -dy.signum() as isize);
                }
            }
            WindowEvent::CursorLeft { .. } | WindowEvent::Focused(false) => {
                self.cursor = None;
                ui.cursor = None;
                ui.drag = None;
                ui.refresh();
            }
            WindowEvent::RedrawRequested
            | WindowEvent::Resized(_)
            | WindowEvent::CloseRequested
            | WindowEvent::Destroyed
            | WindowEvent::Occluded(_)
            | WindowEvent::ScaleFactorChanged { .. } => return false,
            _ => return true,
        }
        if !world.inventory_is_open() {
            ui.close();
        }
        window.request_redraw();
        true
    }
}
