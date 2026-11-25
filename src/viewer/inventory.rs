//! Inventory input and presentation. Only the world commits ownership changes.
use super::*;
use arcanum_rs::content::{
    character::EquipmentSlot,
    world::{InventoryItem, InventoryView},
};
use hud::{Action, Binding};
use std::path::PathBuf;

const SIDES: [Binding; 2] = [Binding::ContainerGrid, Binding::PlayerGrid];
type Point = (u32, u32);

fn equipment_binding(slot: EquipmentSlot) -> Binding {
    match slot {
        EquipmentSlot::Helmet => Binding::EquipmentHelmet,
        EquipmentSlot::Ring1 => Binding::EquipmentRing1,
        EquipmentSlot::Ring2 => Binding::EquipmentRing2,
        EquipmentSlot::Medallion => Binding::EquipmentMedallion,
        EquipmentSlot::Weapon => Binding::EquipmentWeapon,
        EquipmentSlot::Shield => Binding::EquipmentShield,
        EquipmentSlot::Armor => Binding::EquipmentArmor,
        EquipmentSlot::Gauntlet => Binding::EquipmentGauntlet,
        EquipmentSlot::Boots => Binding::EquipmentBoots,
    }
}

struct Icon {
    id: String,
    side: usize,
    worn: Option<EquipmentSlot>,
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
struct Lens {
    asset: Arc<LoadedFrame>,
    body_art: u32,
    center: (i32, i32),
    next: Option<Duration>,
}

struct Drag {
    id: String,
    side: usize,
    worn: Option<EquipmentSlot>,
    offset: [usize; 2],
    image: Arc<LoadedFrame>,
}
pub(super) struct InventoryUi {
    pub path: PathBuf,
    pub scale: u16,
    pub extent: [u32; 2],
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
    lens: Option<Lens>,
    scrolling: bool,
}

impl InventoryUi {
    pub fn new(path: PathBuf, scale: u16) -> Self {
        let extent = ["inventory.toml", "container.toml"]
            .into_iter()
            .filter_map(|name| Hud::theme_extent(&path.join(name)).ok())
            .fold([0; 2], |[w, h], [x, y]| [w.max(x), h.max(y)]);
        Self {
            path,
            scale,
            extent,
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
            lens: None,
            scrolling: false,
        }
    }
    pub(super) fn close(&mut self) {
        self.panel = None;
        self.revision = None;
        self.container = None;
        self.icons.clear();
        self.hovered = None;
        self.selected = None;
        self.drag = None;
        self.scroll = [0; 2];
        self.notice.clear();
        self.lens = None;
        self.scrolling = false;
    }
    fn icon(
        world: &mut InteractiveWorld,
        item: &InventoryItem,
        side: usize,
        worn: Option<EquipmentSlot>,
    ) -> Option<Icon> {
        let paper_doll = worn.and_then(|_| {
            let art = item.paper_doll_art?;
            world.inventory_icon(item.paper_doll_icon.as_deref()?, ((art >> 4) & 3) as u8)
        });
        let image = match paper_doll {
            Some(image) => image,
            None => world.inventory_icon(
                item.icon.as_deref()?,
                ((item.inventory_art? >> 4) & 3) as u8,
            )?,
        };
        let name = item.name.as_deref().unwrap_or("Unnamed item");
        let kind = item.category.unwrap_or("item");
        let mut tip = format!(" {name}\n {kind}");
        if let Some(q) = item.quantity {
            tip.push_str(&format!("   ×{q}"));
        }
        if let Some(weight) = item.weight {
            tip.push_str(&format!("   {weight} stone"));
        }
        Some(Icon {
            id: item.id.clone(),
            side,
            worn,
            slot: if worn.is_some() { 0 } else { item.slot? },
            shape: if worn.is_some() {
                [1, 1]
            } else {
                item.footprint?
            },
            image,
            tip,
        })
    }
    pub fn prepare(
        &mut self,
        world: &mut InteractiveWorld,
        size: Point,
        area: hud::Rect,
        scale: u32,
    ) -> Result<(), String> {
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
                equipment,
                statistics,
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
            if let Some(sprite) = world.player_portrait_frame() {
                self.panel.as_mut().unwrap().paint_surface(
                    Binding::PlayerPreview,
                    |panel, _, _| {
                        let ratio = (panel.width as f64 / sprite.width as f64)
                            .min(panel.height as f64 / sprite.height as f64)
                            .min(2.0);
                        let width = (f64::from(sprite.width) * ratio).floor().max(1.) as u32;
                        let height = (f64::from(sprite.height) * ratio).floor().max(1.) as u32;
                        let mut portrait = hud::sprite([width, height], [0; 4]);
                        if hud::scale_into(sprite, &mut portrait, None).is_ok() {
                            let x = (panel.width - width) / 2;
                            let y = (panel.height - height) / 2;
                            for row in 0..height {
                                for col in 0..width {
                                    let px = portrait.pixels[(row * width + col) as usize];
                                    if px[3] != 0 {
                                        panel.pixels
                                            [((row + y) * panel.width + x + col) as usize] = px;
                                    }
                                }
                            }
                        }
                    },
                );
            }
            self.panel.as_mut().unwrap().set_label(
                Binding::ContainerName,
                container_name.as_deref().unwrap_or("Container"),
            );
            let encumbrance = statistics
                .encumbrance
                .map_or("Unavailable", |value| value.name());
            let speed = statistics
                .speed
                .map_or("?".into(), |speed| speed.to_string());
            let weight = statistics
                .weight
                .map_or("?".into(), |weight| weight.to_string());
            self.panel.as_mut().unwrap().set_label(
                Binding::CharacterSummary,
                &format!("Weight: {weight} / {} stone\nEncumbrance: {encumbrance}\nSpeed: {speed}   AC: {}",
                    statistics.carry_capacity, statistics.armor_class),
            );
            self.icons.clear();
            for gear in &equipment {
                if self
                    .panel
                    .as_ref()
                    .unwrap()
                    .native_size(equipment_binding(gear.slot))
                    .is_some()
                    && let Some(icon) = Self::icon(world, &gear.item, 1, Some(gear.slot))
                {
                    self.icons.push(icon);
                }
            }
            for (side, items) in [contents.as_deref().unwrap_or_default(), &player]
                .into_iter()
                .enumerate()
            {
                for item in items {
                    let icon = Self::icon(world, item, side, None).ok_or_else(|| {
                        format!("Unresolved inventory grid or artwork for {}", item.id)
                    })?;
                    self.icons.push(icon);
                }
            }
            for side in 0..SIDES.len() {
                self.scroll[side] = self.scroll[side].min(self.last_row(side));
            }
            self.drag = None;
            self.hovered = None;
            self.revision = Some(world.inventory_revision());
            self.panel.as_mut().unwrap().layout_in(size, area, scale);
            self.refresh();
        }
        let panel = self.panel.as_mut().unwrap();
        panel.layout_in(size, area, scale);
        self.update_thumb();
        if self.hit_item() != self.hovered {
            self.refresh();
        } else {
            self.tooltip();
        }
        Ok(())
    }
    fn cell(&self, p: Point) -> Option<(usize, usize)> {
        let panel = self.panel.as_ref()?;
        SIDES.iter().enumerate().find_map(|(side, &binding)| {
            let (x, y) = panel.local_point(binding, p)?;
            let cell = panel.native_size(binding)?.0 / 10;
            let x = (x / cell) as usize;
            let y = (y / cell) as usize + self.scroll[side];
            Some((side, y * 10 + x))
        })
    }
    fn equipment_at(&self, point: Point) -> Option<EquipmentSlot> {
        let panel = self.panel.as_ref()?;
        EquipmentSlot::ALL.into_iter().find(|&slot| {
            panel
                .bounds(equipment_binding(slot))
                .is_some_and(|(rect, _)| rect.contains(point))
        })
    }

    fn hit_item(&self) -> Option<usize> {
        let point = self.cursor?;
        if let Some(slot) = self.equipment_at(point) {
            return self.icons.iter().position(|i| i.worn == Some(slot));
        }
        let (side, slot) = self.cell(point)?;
        self.icons
            .iter()
            .position(|i| i.worn.is_none() && i.side == side && i.contains(slot))
    }
    fn tooltip(&mut self) {
        if let Some(panel) = self.panel.as_mut() {
            let invalid_drop = self.drag.as_ref().is_some_and(|drag| drag.side == 0)
                && self.cursor.is_some_and(|point| {
                    panel
                        .bounds(Binding::InventoryDrop)
                        .is_some_and(|(rect, _)| rect.contains(point))
                });
            panel.set_active_action(invalid_drop.then_some(Action::InventoryDrop));
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
            panel.paint_surface(binding, |frame, grid_line, active| {
                let cell = frame.width as usize / 10;
                if cell == 0 {
                    return;
                }
                if let Some(line) = grid_line {
                    for (idx, pixel) in frame.pixels.iter_mut().enumerate() {
                        let x = idx % frame.width as usize;
                        let y = idx / frame.width as usize;
                        if x.is_multiple_of(cell) || y.is_multiple_of(cell) {
                            *pixel = line;
                        }
                    }
                }
                for (index, i) in self
                    .icons
                    .iter()
                    .enumerate()
                    .filter(|(_, i)| i.worn.is_none() && i.side == side)
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
        for slot in EquipmentSlot::ALL {
            let equipped = self
                .icons
                .iter()
                .find(|i| i.worn == Some(slot) && self.drag.as_ref().is_none_or(|d| d.id != i.id));
            panel.paint_surface(equipment_binding(slot), |frame, _, _| {
                let Some(icon) = equipped else { return };
                frame.pixels.fill([0; 4]);
                let src = &icon.image.decoded.frame;
                let ratio = (f64::from(frame.width) / f64::from(src.width))
                    .min(f64::from(frame.height) / f64::from(src.height))
                    .min(1.);
                let w = (f64::from(src.width) * ratio).floor().max(1.) as u32;
                let h = (f64::from(src.height) * ratio).floor().max(1.) as u32;
                let mut image = hud::sprite([w, h], [0; 4]);
                hud::scale_into(src, &mut image, None).expect("valid equipment frame");
                let (x, y) = ((frame.width - w) / 2, (frame.height - h) / 2);
                for row in 0..h {
                    let dest = ((row + y) * frame.width + x) as usize;
                    let source = (row * w) as usize;
                    frame.pixels[dest..dest + w as usize]
                        .copy_from_slice(&image.pixels[source..source + w as usize]);
                }
            });
        }
        self.tooltip();
    }
    fn last_row(&self, side: usize) -> usize {
        let Some((width, height)) = self.panel.as_ref().and_then(|p| p.native_size(SIDES[side]))
        else {
            return 0;
        };
        let rows = (height / (width / 10)) as usize;
        let end = self
            .icons
            .iter()
            .filter(|i| i.worn.is_none() && i.side == side)
            .map(|i| i.slot / 10 + i.shape[1])
            .max()
            .unwrap_or(0);
        end.saturating_sub(rows)
    }

    fn update_thumb(&mut self) {
        let last = self.last_row(0);
        if let Some(panel) = &mut self.panel {
            panel.set_inventory_scroll(self.scroll[0], last);
        }
    }

    fn scroll(&mut self, side: usize, delta: isize) {
        self.set_scroll(side, self.scroll[side].saturating_add_signed(delta));
    }

    fn set_scroll(&mut self, side: usize, row: usize) {
        let row = row.min(self.last_row(side));
        if row != self.scroll[side] {
            self.scroll[side] = row;
            self.update_thumb();
            self.refresh();
        }
    }

    fn scroll_to_pointer(&mut self, point: Point) {
        let Some(panel) = &self.panel else {
            return;
        };
        let Some((track, _)) = panel.bounds(Binding::InventoryScrollTrack) else {
            return;
        };
        let thumb_height = panel
            .bounds(Binding::InventoryScrollThumb)
            .map_or(1, |(r, _)| r.h);
        let span = track.h.saturating_sub(thumb_height).max(1) as usize;
        let offset = (i64::from(point.1) - i64::from(track.y) - i64::from(thumb_height / 2))
            .clamp(0, span as i64) as usize;
        self.set_scroll(0, (offset * self.last_row(0) + span / 2) / span);
    }

    pub(super) fn lens_delay(&self, elapsed: Duration) -> Option<Duration> {
        Some(self.lens.as_ref()?.next?.saturating_sub(elapsed))
    }

    pub(super) fn render_lens(
        &mut self,
        renderer: &mut gpu::WindowRenderer,
        world: &InteractiveWorld,
        camera: &Camera,
        elapsed: Duration,
        settings: Settings,
    ) -> Result<(), String> {
        let Some(panel) = &mut self.panel else {
            return Ok(());
        };
        let Some((rect, _)) = panel.bounds(Binding::PlayerLens) else {
            return Ok(());
        };
        let Some([width, height]) = panel.lens_size() else {
            return Ok(());
        };
        let Some((center, body_art)) = world.player_anchor(elapsed).zip(world.player_art_id())
        else {
            return Ok(());
        };
        if let Some(lens) = &self.lens
            && lens.center == center
            && lens.body_art == body_art
            && lens.asset.decoded.frame.width == rect.w
            && lens.asset.decoded.frame.height == rect.h
            && lens.next.is_none_or(|next| elapsed < next)
        {
            panel.set_lens_asset(Arc::clone(&lens.asset));
            return Ok(());
        }

        let mut lens_camera = camera.clone();
        lens_camera.follow(center);
        let source = (width * 2, height * 2);
        let scale = (rect.w as f32 / source.0 as f32).min(rect.h as f32 / source.1 as f32);
        let asset = hud::generated_frame(
            "inventory-player-inset",
            SpriteFrame {
                width: rect.w,
                height: rect.h,
                hotspot: (0, 0),
                offset: (0, 0),
                pixels: Vec::new(),
            },
        );
        renderer.render_inset(
            &asset,
            scale,
            stream_background(true),
            scene_draws(world, &lens_camera, source, elapsed, settings),
        )?;
        let next = world
            .animation_delay(&lens_camera, source, elapsed)
            .map(|delay| elapsed + delay);
        panel.set_lens_asset(Arc::clone(&asset));
        self.lens = Some(Lens {
            asset,
            body_art,
            center,
            next,
        });
        Ok(())
    }

    fn start_drag(&mut self) {
        let Some(index) = self.hit_item() else { return };
        let item = &self.icons[index];
        let panel = self.panel.as_ref().unwrap();
        let (scale, offset) = if let Some(slot) = item.worn {
            let (_, scale) = panel.bounds(equipment_binding(slot)).unwrap();
            (scale, [0, 0])
        } else {
            let (_, cell) = self.cell(self.cursor.unwrap()).unwrap();
            let (rect, _) = panel.bounds(SIDES[item.side]).unwrap();
            (
                rect.w * 100 / 320,
                [cell % 10 - item.slot % 10, cell / 10 - item.slot / 10],
            )
        };
        let src = &item.image.decoded.frame;
        let mut frame = hud::sprite(
            [
                (src.width * scale / 100).max(1),
                (src.height * scale / 100).max(1),
            ],
            [0; 4],
        );
        hud::scale_into(src, &mut frame, None).expect("valid inventory frame");
        self.drag = Some(Drag {
            id: item.id.clone(),
            side: item.side,
            worn: item.worn,
            offset,
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
                Some(Action::InventoryScrollUp) => self.scroll(0, -1),
                Some(Action::InventoryScrollDown) => self.scroll(0, 1),
                _ => {}
            }
            return;
        };
        if let Some(worn) = drag.worn {
            if let Some((1, cell)) = self.cell(point) {
                self.notice = world
                    .unequip_inventory_item(worn, Some(cell))
                    .err()
                    .unwrap_or_default();
            }
        } else if let Some(slot) = self.equipment_at(point) {
            if drag.side == 1 {
                self.notice = world
                    .equip_inventory_item(&drag.id, slot)
                    .err()
                    .unwrap_or_default();
            }
        } else if let Some((side, slot)) = self.cell(point) {
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
    pub(super) fn inventory_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        event: &WindowEvent,
    ) -> bool {
        if self.pause.is_some() || self.map_since.is_some() {
            return false;
        }
        let ViewContent::Stream {
            world,
            inventory: Some(ui),
            hud,
            ..
        } = &mut self.content
        else {
            return false;
        };
        if let WindowEvent::KeyboardInput {
            event,
            is_synthetic: false,
            ..
        } = event
            && event.state.is_pressed()
            && !event.repeat
            && matches!(event.logical_key.as_ref(), Key::Character("i" | "I"))
        {
            self.hud_action(Action::OpenInventory, event_loop);
            return true;
        }
        if !world.inventory_is_open() {
            return false;
        }
        let window = &self.view.as_ref().unwrap().window;
        let allow_hud_toggle = !ui.scrolling && ui.drag.is_none();
        match event {
            WindowEvent::KeyboardInput { event, .. }
                if event.state.is_pressed() && !event.repeat =>
            {
                match event.logical_key.as_ref() {
                    Key::Named(NamedKey::Escape) if ui.scrolling => ui.scrolling = false,
                    Key::Named(NamedKey::Escape) if ui.drag.take().is_some() => ui.refresh(),
                    Key::Named(NamedKey::Escape) => world.close_inventory(),
                    Key::Character("d" | "D") => ui.drop_selected(world),
                    _ => return true,
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let pixel = (position.x as u32, position.y as u32);
                let hud_changed = hud.as_mut().is_some_and(|hud| {
                    hud.hover(
                        (allow_hud_toggle
                            && hud.hit(pixel) == hud::Hit::Button(Action::OpenInventory))
                        .then_some(pixel),
                    )
                });
                let point = Some(pixel);
                self.cursor = point;
                ui.cursor = point;
                ui.notice.clear();
                if ui.scrolling {
                    ui.scroll_to_pointer(pixel);
                    window.request_redraw();
                    return true;
                }
                let changed = ui.panel.as_mut().is_some_and(|p| p.hover(point));
                if ui.hovered != ui.hit_item() {
                    ui.refresh();
                } else if ui.hovered.is_none() && ui.drag.is_none() && !changed && !hud_changed {
                    return true;
                } else {
                    ui.tooltip();
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let Some(p) = self.cursor else { return true };
                if *button == MouseButton::Left
                    && let Some(hud) = hud.as_mut()
                {
                    if state.is_pressed()
                        && allow_hud_toggle
                        && hud.hit(p) == hud::Hit::Button(Action::OpenInventory)
                    {
                        hud.press(p);
                        window.request_redraw();
                        return true;
                    }
                    if !state.is_pressed() && hud.is_pressed() {
                        let action = allow_hud_toggle.then(|| hud.release(p)).flatten();
                        hud.cancel_press();
                        window.request_redraw();
                        if action == Some(Action::OpenInventory) {
                            self.hud_action(Action::OpenInventory, event_loop);
                        }
                        return true;
                    }
                }
                ui.cursor = Some(p);
                ui.notice.clear();
                match (*button, state.is_pressed()) {
                    (MouseButton::Left, true) => {
                        if ui
                            .panel
                            .as_ref()
                            .and_then(|panel| panel.bounds(Binding::InventoryScrollTrack))
                            .is_some_and(|(track, _)| track.contains(p))
                        {
                            ui.scrolling = true;
                            ui.scroll_to_pointer(p);
                        } else if ui.hit_item().is_some() {
                            ui.start_drag();
                        } else if let Some(panel) = ui.panel.as_mut() {
                            panel.press(p);
                        }
                    }
                    (MouseButton::Left, false) if ui.scrolling => ui.scrolling = false,
                    (MouseButton::Left, false)
                        if ui.drag.is_some()
                            && hud.as_ref().is_some_and(|hud| {
                                hud.hit(p) == hud::Hit::Button(Action::OpenInventory)
                            }) =>
                    {
                        ui.drag = None;
                        ui.refresh();
                    }
                    (MouseButton::Left, false) => ui.release(world, p),
                    (MouseButton::Right, false) => {
                        if let Some(i) = ui.hit_item() {
                            if let Some(slot) = ui.icons[i].worn {
                                ui.notice = world
                                    .unequip_inventory_item(slot, None)
                                    .err()
                                    .unwrap_or_default();
                            } else if let Some(owner) = ui.container.clone() {
                                let id = ui.icons[i].id.clone();
                                let op = if ui.icons[i].side == 1 { "put" } else { "take" };
                                ui.commit(world, &id, op, Some(&owner), None);
                            }
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
                if let Some(hud) = hud {
                    hud.cancel_press();
                    hud.hover(None);
                }
                self.cursor = None;
                ui.cursor = None;
                ui.drag = None;
                ui.scrolling = false;
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
