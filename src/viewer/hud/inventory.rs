use super::*;
use arcanum_rs::content::character::EquipmentSlot;
use arcanum_rs::content::world::{
    InteractiveWorld, InventoryContents, InventoryEntry, InventoryReader,
};
use serde_json::{Value, json};
use winit::keyboard::{Key, NamedKey};

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum InventoryView {
    Inventory,
    Container,
    Trade,
}

#[derive(Default)]
pub(super) struct InventoryPanel {
    view: Option<InventoryView>,
    target: Option<String>,
    revision: u64,
    backpack: Option<InventoryContents>,
    container: Option<InventoryContents>,
    scroll: [usize; 2],
    hovered: Option<String>,
    error: Option<String>,
    vertical: bool,
    drag: Option<Drag>,
    pointer: Option<(u32, u32)>,
    quantity: Option<Quantity>,
}

impl InventoryPanel {
    pub(super) fn view(&self) -> Option<InventoryView> {
        self.view
    }

    fn owner(&self, index: usize) -> Option<&str> {
        if index == 0 {
            Some("player")
        } else {
            self.target.as_deref()
        }
    }

    fn contents(&self, index: usize) -> Option<&InventoryContents> {
        if index == 0 {
            self.backpack.as_ref()
        } else {
            self.container.as_ref()
        }
    }
}

struct Quantity {
    id: String,
    from: String,
    to: String,
    slot: Option<usize>,
    max: u32,
    input: String,
    replace: bool,
}

struct Drag {
    id: String,
    source: usize,
    equipped: Option<EquipmentSlot>,
    grab_cell: [i32; 2],
    grab_pixels: [i32; 2],
    scale: u32,
}

struct PlacementPreview {
    component: usize,
    rect: Rect,
    clip: Rect,
    valid: bool,
    slot: Option<(usize, usize)>,
}

const GRIDS: [Binding; 2] = [Binding::Backpack, Binding::ContainerGrid];

struct GridView {
    clip: Rect,
    full: Rect,
    scale: u32,
    cell: u32,
    rows: usize,
    offset: usize,
}

impl GridView {
    fn area(&self, col: i32, row: i32, cells: [usize; 2]) -> Rect {
        let edge = |at: i32| (at * self.cell as i32 * self.scale as i32).div_euclid(100);
        // Euclidean rounding keeps a partially scrolled multi-cell item aligned
        // with the same cell boundaries as the visible rows.
        let y = |at: i32| (at * self.cell as i32 * self.scale as i32).div_euclid(100);
        Rect {
            x: self.full.x + edge(col),
            y: self.full.y + y(row),
            w: (edge(col + cells[0] as i32) - edge(col)) as u32,
            h: (y(row + cells[1] as i32) - y(row)) as u32,
        }
    }

    fn cell_at(&self, point: (u32, u32)) -> Option<[i32; 2]> {
        self.clip.contains(point).then(|| {
            let axis = |pixel: u32, origin: i32| {
                ((i64::from(pixel) - i64::from(origin) + 1) * 100 - 1)
                    .div_euclid(i64::from(self.cell * self.scale)) as i32
            };
            [axis(point.0, self.full.x), axis(point.1, self.full.y)]
        })
    }

    fn item_area(&self, slot: usize, columns: usize, cells: [usize; 2]) -> Rect {
        self.area(
            (slot % columns) as i32,
            (slot / columns) as i32 - self.offset as i32,
            cells,
        )
    }

    fn icon_area(
        &self,
        slot: usize,
        columns: usize,
        cells: [usize; 2],
        frame: &SpriteFrame,
    ) -> Rect {
        let col = (slot % columns) as i32 * self.cell as i32;
        let row = ((slot / columns) as i32 - self.offset as i32) * self.cell as i32;
        let edge = |unit: i32| (unit * self.scale as i32).div_euclid(100);
        let width = frame.width;
        let height = frame.height;
        let x = col + (cells[0] as i32 * self.cell as i32 - width as i32) / 2;
        let y = row + (cells[1] as i32 * self.cell as i32 - height as i32) / 2;
        Rect {
            x: self.full.x + edge(x),
            y: self.full.y + edge(y),
            w: (edge(x + width as i32) - edge(x)).max(1) as u32,
            h: (edge(y + height as i32) - edge(y)).max(1) as u32,
        }
    }
}

impl Hud {
    pub(in crate::viewer) fn has_inventory(&self) -> bool {
        self.components
            .iter()
            .any(|part| part.config.inventory_panel && !part.config.hidden)
    }

    pub(in crate::viewer) fn inventory_open(&self) -> bool {
        self.inventory.view.is_some()
    }

    pub(in crate::viewer) fn container_sound(&self) -> Option<(&str, i32)> {
        Some((
            self.inventory.target.as_deref()?,
            self.inventory.container.as_ref()?.container_sound?,
        ))
    }

    pub(in crate::viewer) fn close_inventory(&mut self) -> bool {
        let was_open = self.inventory_open();
        if was_open {
            self.inventory = InventoryPanel::default();
            self.set_label(Binding::InventoryHover, "");
            self.clear_layout();
        }
        was_open
    }

    pub(in crate::viewer) fn open_inventory(
        &mut self,
        reader: &mut InventoryReader,
        world: &InteractiveWorld,
        target: Option<String>,
    ) -> Result<(), String> {
        if !self.has_inventory() {
            return Err("The active UI package has no inventory panel".into());
        }
        let next = (|| -> Result<_, String> {
            let container = target
                .as_deref()
                .map(|id| reader.contents(world, Some(id)))
                .transpose()?;
            let backpack = reader.contents(world, None)?;
            Ok((backpack, container))
        })();
        let (backpack, container) = next.inspect_err(|error| {
            self.inventory.error = Some(error.clone());
        })?;
        self.close_inventory();
        self.inventory = InventoryPanel {
            view: Some(if world.trading() {
                InventoryView::Trade
            } else if target.is_some() {
                InventoryView::Container
            } else {
                InventoryView::Inventory
            }),
            target,
            backpack: Some(backpack),
            container,
            revision: world.inventory_revision(),
            ..InventoryPanel::default()
        };
        self.update_inventory_labels(world);
        self.clear_layout();
        Ok(())
    }

    pub(in crate::viewer) fn refresh_inventory(
        &mut self,
        reader: &mut InventoryReader,
        world: &InteractiveWorld,
    ) {
        let revision = world.inventory_revision();
        if !self.inventory_open() || self.inventory.revision == revision {
            return;
        }
        let scroll = self.inventory.scroll;
        let vertical = self.inventory.vertical;
        let error = self.inventory.error.take();
        let target = self.inventory.target.clone();
        match self.open_inventory(reader, world, target) {
            Ok(()) => {
                self.inventory.scroll = scroll;
                self.inventory.vertical = vertical;
                self.inventory.error = error;
            }
            Err(error) => {
                if world.trading() {
                    self.inventory.revision = revision;
                } else {
                    self.close_inventory();
                }
                self.inventory.error = Some(error);
            }
        }
    }

    fn update_inventory_labels(&mut self, world: &InteractiveWorld) {
        if let Some(trade) = world.trade_diagnostic() {
            self.set_label(Binding::MerchantName, trade["name"].as_str().unwrap_or(""));
            self.set_label(Binding::MerchantGold, &trade["gold"].to_string());
        }
        let counts = world.inventory_counters();
        for (binding, value) in [
            Binding::InventoryGold,
            Binding::InventoryArrows,
            Binding::InventoryBullets,
            Binding::InventoryCharges,
            Binding::InventoryFuel,
        ]
        .into_iter()
        .zip(counts)
        {
            self.set_label(binding, &value.to_string());
        }
        let stats = world.character_statistics();
        self.set_label(
            Binding::InventoryWeight,
            &format!("Weight: {} st", stats.weight.unwrap_or_default()),
        );
        self.set_label(
            Binding::InventoryBurden,
            &format!(
                "Encumbrance: {} ({})",
                stats.encumbrance.map_or("", |tier| tier.name()),
                stats
                    .encumbrance
                    .map_or(0, |tier| tier.threshold(stats.carry_capacity))
            ),
        );
        self.set_label(
            Binding::InventorySpeed,
            &stats
                .speed
                .map_or_else(String::new, |speed| format!("Speed: {speed}")),
        );
        self.set_label(Binding::InventoryDefense, &stats.defense.to_string());
    }

    fn equipment_at(&self, point: (u32, u32)) -> Option<(EquipmentSlot, &Placement)> {
        self.placed
            .iter()
            .enumerate()
            .find_map(|(index, placement)| {
                let placement = placement.as_ref()?;
                let slot = self.components[index].config.equipment_slot?;
                placement.rect.contains(point).then_some((slot, placement))
            })
    }

    pub(super) fn equipped(&self, slot: EquipmentSlot) -> Option<&InventoryEntry> {
        self.inventory
            .backpack
            .as_ref()?
            .equipment
            .iter()
            .find(|item| item.equipment == Some(slot))
    }

    fn grid(&self, index: usize) -> Option<(GridView, &InventoryContents)> {
        let component = self.placed_bound(GRIDS[index])?;
        let placement = self.placed.get(component)?.as_ref()?;
        let contents = self.inventory.contents(index)?;
        let config = &self.components[component].config;
        let cell = config.cell?;
        let rows = (config.size[1] / cell) as usize;
        // A package can change the visual cell size and visible height. Slot
        // numbering always uses the authoritative column count.
        if (config.size[0] / cell) as usize != contents.columns {
            return None;
        }
        Some((
            GridView {
                clip: placement.rect,
                full: placement.full,
                scale: placement.scale,
                cell,
                rows,
                offset: self.inventory.scroll[index].min(contents.rows.saturating_sub(rows)),
            },
            contents,
        ))
    }

    pub(in crate::viewer) fn scroll_inventory(
        &mut self,
        point: Option<(u32, u32)>,
        delta: i32,
    ) -> bool {
        let index = if let Some(point) = point {
            if self
                .grid(0)
                .is_some_and(|(grid, _)| grid.clip.contains(point))
            {
                0
            } else if self
                .grid(1)
                .is_some_and(|(grid, _)| grid.clip.contains(point))
                || self
                    .bounds(Binding::ContainerThumb)
                    .is_some_and(|(rect, _)| rect.contains(point))
            {
                1
            } else {
                return false;
            }
        } else {
            1
        };
        let Some((grid, contents)) = self.grid(index) else {
            return false;
        };
        let offset = grid
            .offset
            .saturating_add_signed(delta as isize)
            .min(contents.rows.saturating_sub(grid.rows));
        if offset == grid.offset {
            return false;
        }
        self.inventory.scroll[index] = offset;
        true
    }

    fn grid_item(&self, point: (u32, u32)) -> Option<(usize, GridView, &InventoryEntry)> {
        (0..GRIDS.len()).find_map(|index| {
            let (grid, contents) = self.grid(index)?;
            if !grid.clip.contains(point) {
                return None;
            }
            let item = contents.entries.iter().find(|item| {
                grid.item_area(item.slot, contents.columns, item.cells)
                    .contains(point)
            })?;
            Some((index, grid, item))
        })
    }

    pub(in crate::viewer) fn carried_item_at(&self, point: (u32, u32)) -> Option<&str> {
        let item = self
            .grid_item(point)
            .filter(|(index, _, _)| *index == 0)
            .map(|(_, _, i)| i)
            .or_else(|| {
                self.equipment_at(point)
                    .and_then(|(slot, _)| self.equipped(slot))
            })?;
        Some(&item.id)
    }

    pub(in crate::viewer) fn begin_inventory_drag(&mut self, point: (u32, u32)) -> Option<i32> {
        if let Some((slot, placement)) = self.equipment_at(point)
            && let Some(item) = self.equipped(slot)
        {
            let sound = item.pickup_sound;
            self.inventory.drag = Some(Drag {
                id: item.id.clone(),
                source: 0,
                equipped: Some(slot),
                grab_cell: [0, 0],
                grab_pixels: [
                    point.0 as i32 - placement.full.x,
                    point.1 as i32 - placement.full.y,
                ],
                scale: placement.scale,
            });
            self.inventory.pointer = Some(point);
            self.inventory.error = None;
            return Some(sound);
        }
        let (source, grid, item) = self.grid_item(point)?;
        let sound = item.pickup_sound;
        let columns = self
            .inventory
            .contents(source)
            .expect("visible grid")
            .columns;
        let [col, row] = grid.cell_at(point).expect("point over grid");
        let rect = grid.item_area(item.slot, columns, item.cells);
        self.inventory.drag = Some(Drag {
            id: item.id.clone(),
            source,
            equipped: None,
            grab_cell: [
                col - (item.slot % columns) as i32,
                row + grid.offset as i32 - (item.slot / columns) as i32,
            ],
            grab_pixels: [point.0 as i32 - rect.x, point.1 as i32 - rect.y],
            scale: grid.scale,
        });
        self.inventory.pointer = Some(point);
        self.inventory.error = None;
        Some(sound)
    }

    pub(super) fn inventory_cursor(&self) -> Option<(&str, [i32; 2])> {
        let drag = self.inventory.drag.as_ref()?;
        let item = self.dragged_item(drag)?;
        Some((
            &item.icon.as_ref().ok()?.source.virtual_path,
            drag.grab_pixels,
        ))
    }
    pub(in crate::viewer) fn dragging_inventory(&self) -> bool {
        self.inventory.drag.is_some()
    }

    pub(in crate::viewer) fn cancel_inventory_drag(&mut self) -> bool {
        self.inventory.drag.take().is_some() | self.inventory.quantity.take().is_some()
    }

    pub(in crate::viewer) fn choosing_quantity(&self) -> bool {
        self.inventory.quantity.is_some()
    }

    pub(in crate::viewer) fn inventory_quantity_key(
        &mut self,
        reader: &mut InventoryReader,
        world: &mut InteractiveWorld,
        key: &Key,
    ) {
        let Some(quantity) = &mut self.inventory.quantity else {
            return;
        };
        match key {
            Key::Named(NamedKey::Enter) => {
                if let Some(value) = quantity
                    .input
                    .parse::<u32>()
                    .ok()
                    .filter(|value| (1..=quantity.max).contains(value))
                {
                    let quantity = self.inventory.quantity.take().expect("pending quantity");
                    self.inventory.error = reader
                        .transfer_quantity(
                            world,
                            &quantity.id,
                            (&quantity.from, &quantity.to),
                            quantity.slot,
                            value,
                        )
                        .err();
                }
            }
            Key::Named(NamedKey::Backspace) => {
                quantity.input.pop();
                quantity.replace = false;
            }
            Key::Character(key) if key.len() == 1 && key.as_bytes()[0].is_ascii_digit() => {
                if quantity.replace {
                    quantity.input.clear();
                    quantity.replace = false;
                }
                if quantity.input.len() < 10 {
                    quantity.input.push_str(key);
                }
            }
            _ => {}
        }
        self.inventory_hover(self.inventory.pointer);
    }

    fn dragged_item(&self, drag: &Drag) -> Option<&InventoryEntry> {
        if let Some(slot) = drag.equipped {
            return self.equipped(slot);
        }
        self.inventory
            .contents(drag.source)?
            .entries
            .iter()
            .find(|item| item.id == drag.id)
    }

    fn preview(&self, drag: &Drag, point: (u32, u32)) -> Option<PlacementPreview> {
        let item = self.dragged_item(drag)?;
        for (index, binding) in GRIDS.into_iter().enumerate() {
            let Some((grid, contents)) = self.grid(index) else {
                continue;
            };
            let Some([col, row]) = grid.cell_at(point) else {
                continue;
            };
            let x = col - drag.grab_cell[0];
            let y = row - drag.grab_cell[1];
            let absolute_row = y + grid.offset as i32;
            let slot = (x >= 0 && absolute_row >= 0)
                .then(|| absolute_row as usize * contents.columns + x as usize);
            return Some(PlacementPreview {
                component: self.placed_bound(binding)?,
                rect: grid.area(x, y, item.cells),
                clip: grid.clip,
                valid: item.movable
                    && (drag.equipped.is_none() || index == 0 && item.removable)
                    && (drag.source == index || item.removable)
                    && slot.is_some_and(|slot| contents.fits(item, slot, drag.source == index)),
                slot: slot.map(|slot| (index, slot)),
            });
        }
        let component = self.placed_bound(Binding::InventoryDrop)?;
        let placement = self.placed[component].as_ref()?;
        placement.rect.contains(point).then_some(PlacementPreview {
            component,
            rect: placement.full,
            clip: placement.rect,
            valid: drag.source == 0 && drag.equipped.is_none() && item.movable && item.removable,
            slot: None,
        })
    }

    pub(in crate::viewer) fn finish_inventory_drag(
        &mut self,
        reader: &mut InventoryReader,
        world: &mut InteractiveWorld,
        point: Option<(u32, u32)>,
    ) {
        let Some(drag) = self.inventory.drag.take() else {
            return;
        };
        if let Some((rect, _)) = self.bounds(Binding::InventoryUse)
            && point.is_some_and(|p| rect.contains(p))
        {
            if drag.source == 0
                && drag.equipped.is_none()
                && self.inventory.view != Some(InventoryView::Trade)
            {
                match world.select_tool(&drag.id) {
                    Ok(()) => {
                        self.close_inventory();
                    }
                    Err(e) => self.inventory.error = Some(e),
                }
            }
            return;
        }
        if let Some((slot, _)) = point.and_then(|point| self.equipment_at(point)) {
            if drag.source == 0 && drag.equipped.is_none() {
                self.inventory.error = reader
                    .change_equipment(world, slot, Some(&drag.id), None)
                    .err();
            }
            return;
        }
        let Some(preview) = point.and_then(|point| self.preview(&drag, point)) else {
            return;
        };
        if !preview.valid {
            self.inventory.error = Some("Item cannot be placed at this destination".into());
            return;
        }
        if let Some(slot) = drag.equipped {
            if let Some((0, cell)) = preview.slot {
                self.inventory.error = reader.change_equipment(world, slot, None, Some(cell)).err();
            }
            return;
        }
        let from = self.inventory.owner(drag.source).expect("drag source");
        let to = preview
            .slot
            .and_then(|(index, _)| self.inventory.owner(index))
            .unwrap_or("ground");
        if from != to
            && let Some(max) = self
                .dragged_item(&drag)
                .and_then(|item| item.quantity)
                .filter(|&n| n > 1)
        {
            self.inventory.quantity = Some(Quantity {
                id: drag.id,
                from: from.into(),
                to: to.into(),
                slot: preview.slot.map(|(_, cell)| cell),
                max,
                input: max.to_string(),
                replace: true,
            });
            self.inventory_hover(point);
            return;
        }
        let result = if let Some((index, slot)) = preview.slot {
            if index == drag.source
                && self
                    .dragged_item(&drag)
                    .is_some_and(|item| item.slot == slot)
            {
                return;
            }
            reader.transfer(
                world,
                &drag.id,
                self.inventory.owner(drag.source).expect("drag source"),
                self.inventory.owner(index).expect("drag destination"),
                Some(slot),
            )
        } else {
            reader.drop_item(world, &drag.id)
        };
        self.inventory.error = result.err();
    }

    pub(in crate::viewer) fn quick_inventory_transfer(
        &mut self,
        reader: &mut InventoryReader,
        world: &mut InteractiveWorld,
        point: (u32, u32),
    ) {
        if let Some((slot, _)) = self.equipment_at(point) {
            if self.equipped(slot).is_some() {
                self.inventory.error = reader.change_equipment(world, slot, None, None).err();
            }
        } else if let Some((index, _, item)) = self.grid_item(point)
            && let (Some(from), Some(to)) =
                (self.inventory.owner(index), self.inventory.owner(1 - index))
        {
            self.inventory.error = reader.transfer(world, &item.id, from, to, None).err();
        }
    }

    pub(in crate::viewer) fn inventory_action(
        &mut self,
        reader: &mut InventoryReader,
        world: &mut InteractiveWorld,
        action: Action,
    ) {
        self.inventory.error = match action {
            Action::InventoryTakeAll => self
                .inventory
                .target
                .as_deref()
                .and_then(|target| reader.take_all(world, target).err()),
            Action::InventoryArrange => {
                let result = reader.arrange(
                    world,
                    self.inventory.target.as_deref(),
                    self.inventory.vertical,
                );
                if result.is_ok() {
                    self.inventory.vertical = !self.inventory.vertical;
                    self.inventory.scroll = [0; 2];
                }
                result.err()
            }
            _ => return,
        };
    }

    pub(super) fn inventory_hover(&mut self, point: Option<(u32, u32)>) -> bool {
        if !self.inventory_open() {
            return false;
        }
        let moved = std::mem::replace(&mut self.inventory.pointer, point) != point;
        let item = if let Some(drag) = &self.inventory.drag {
            self.dragged_item(drag)
        } else {
            point.and_then(|point| {
                self.grid_item(point).map(|(_, _, item)| item).or_else(|| {
                    self.equipment_at(point)
                        .and_then(|(slot, _)| self.equipped(slot))
                })
            })
        };
        let id = item.map(|item| item.id.clone());
        let message = if let Some(quantity) = &self.inventory.quantity {
            format!(
                "Quantity (1–{}): {}   Enter to move, Esc to cancel",
                quantity.max, quantity.input
            )
        } else if let Some(error) = &self.inventory.error {
            error.clone()
        } else if let Some(item) = item {
            format!("{}\n{}", item.name, item.detail)
        } else {
            point
                .and_then(|point| {
                    self.placed
                        .iter()
                        .enumerate()
                        .rev()
                        .find_map(|(index, placement)| {
                            placement
                                .as_ref()?
                                .rect
                                .contains(point)
                                .then(|| self.components[index].config.hover_message.clone())
                                .flatten()
                        })
                })
                .unwrap_or_default()
        };
        if self.inventory.view == Some(InventoryView::Trade) {
            let offer = self.inventory.error.clone().unwrap_or_else(|| {
                item.map_or_else(String::new, |i| {
                    i.price
                        .map_or_else(|| "No offer".into(), |n| format!("{n} gold each"))
                })
            });
            self.set_label(Binding::MerchantOffer, &offer);
        }
        let item_changed = self.inventory.hovered != id;
        self.inventory.hovered = id;
        let changed = self
            .bound(Binding::InventoryHover)
            .and_then(|index| self.components[index].label.as_ref())
            .is_some_and(|label| label.value != message);
        if changed {
            self.set_label(Binding::InventoryHover, &message);
        }
        changed || item_changed || (moved && self.inventory.drag.is_some())
    }

    pub(super) fn inventory_draws(&self) -> impl Iterator<Item = OverlayDraw<'_>> {
        let icons = (0..GRIDS.len())
            .filter_map(|index| self.grid(index))
            .flat_map(move |(grid, contents)| {
                contents.entries.iter().filter_map(move |item| {
                    if self
                        .inventory
                        .drag
                        .as_ref()
                        .is_some_and(|drag| drag.id == item.id)
                    {
                        return None;
                    }
                    let footprint = grid.item_area(item.slot, contents.columns, item.cells);
                    let asset = item.icon.as_ref().ok()?;
                    let rect = grid.icon_area(
                        item.slot,
                        contents.columns,
                        item.cells,
                        &asset.decoded.frame,
                    );
                    Some(overlay(
                        asset,
                        rect,
                        rect.intersect(footprint)?.intersect(grid.clip)?,
                    ))
                })
            });
        let equipped = self
            .placed
            .iter()
            .enumerate()
            .filter_map(|(index, placement)| {
                let p = placement.as_ref()?;
                let slot = self.components[index].config.equipment_slot?;
                let item = self.equipped(slot)?;
                if self
                    .inventory
                    .drag
                    .as_ref()
                    .is_some_and(|drag| drag.id == item.id)
                {
                    return None;
                }
                let asset = item.icon.as_ref().ok()?;
                let rect = equipment_icon(p, asset);
                Some(overlay(asset, rect, rect.intersect(p.rect)?))
            });
        let highlight = (0..GRIDS.len()).find_map(|index| {
            if self.inventory.drag.is_some() {
                return None;
            }
            let hot = self.inventory.hovered.as_ref()?;
            let asset = self.components[self.placed_bound(GRIDS[index])?].state_art[0].as_ref()?;
            let (grid, contents) = self.grid(index)?;
            let item = contents.entries.iter().find(|item| &item.id == hot)?;
            let rect = grid.item_area(item.slot, contents.columns, item.cells);
            Some(overlay(asset, rect, rect.intersect(grid.clip)?))
        });
        let thumb = self.grid(1).and_then(|(grid, contents)| {
            let index = self.placed_bound(Binding::ContainerThumb)?;
            let placement = self.placed[index].as_ref()?;
            let track = placement.full;
            let max = contents.rows.saturating_sub(grid.rows);
            let asset = &self.components[index].art;
            let native_height = asset.decoded.frame.height * placement.scale / 100;
            let height = native_height.max(1).min(track.h);
            let y = grid.offset * (track.h - height) as usize / max.max(1);
            let rect = Rect {
                y: track.y + y as i32,
                h: height,
                ..track
            };
            Some(overlay(asset, rect, rect.intersect(placement.rect)?))
        });
        let placement = self.inventory.drag.as_ref().and_then(|drag| {
            let preview = self.preview(drag, self.inventory.pointer?)?;
            let asset = self.components[preview.component].state_art
                [if preview.valid { 1 } else { 2 }]
            .as_ref()?;
            Some(overlay(
                asset,
                preview.rect,
                preview.rect.intersect(preview.clip)?,
            ))
        });
        let ghost = self.inventory.drag.as_ref().and_then(|drag| {
            let point = self.inventory.pointer?;
            let item = self.dragged_item(drag)?;
            let asset = item.icon.as_ref().ok()?;
            let (mut rect, origin, scale) = if let Some(slot) = drag.equipped {
                let component = self
                    .components
                    .iter()
                    .position(|component| component.config.equipment_slot == Some(slot))?;
                let p = self.placed[component].as_ref()?;
                (equipment_icon(p, asset), p.full, p.scale)
            } else {
                let (grid, contents) = self.grid(drag.source)?;
                (
                    grid.icon_area(
                        item.slot,
                        contents.columns,
                        item.cells,
                        &asset.decoded.frame,
                    ),
                    grid.item_area(item.slot, contents.columns, item.cells),
                    grid.scale,
                )
            };
            rect.x +=
                point.0 as i32 - origin.x - drag.grab_pixels[0] * scale as i32 / drag.scale as i32;
            rect.y +=
                point.1 as i32 - origin.y - drag.grab_pixels[1] * scale as i32 / drag.scale as i32;
            let viewport = Rect {
                x: 0,
                y: 0,
                w: self.size.0,
                h: self.size.1,
            };
            Some(overlay(asset, rect, rect.intersect(viewport)?))
        });
        highlight
            .into_iter()
            .chain(icons)
            .chain(equipped)
            .chain(thumb)
            .chain(placement)
            .chain(ghost)
    }

    pub(super) fn inventory_diagnostics(&self) -> Value {
        let rect = |r: Rect| [r.x, r.y, r.w as i32, r.h as i32];
        let component_bounds = |binding| self.bounds(binding).map(|(r, _)| rect(r));
        let owner = |index| {
            self.grid(index).map(|(grid, contents)| {
                json!({"bounds":rect(grid.clip),"visible_rows":grid.rows,"scroll_row":grid.offset,
                "columns":contents.columns,"rows":contents.rows,
                "items":contents.entries.iter().map(|item| {
                    let footprint = grid.item_area(item.slot, contents.columns, item.cells);
                    json!({"id":item.id,"name":item.name,"slot":item.slot,"cells":item.cells,
                        "visible_bounds":footprint.intersect(grid.clip).map(rect),"quantity":item.quantity,"unit_price":item.price,
                        "art":item.icon.as_ref().ok().map(|a|&a.source.virtual_path),
                        "art_error":item.icon.as_ref().err()})
                }).collect::<Vec<_>>()})
            })
        };
        let panel_bounds = self
            .components
            .iter()
            .position(|c| c.config.inventory_panel)
            .and_then(|index| self.placed.get(index)?.as_ref().map(|p| rect(p.rect)));
        let drag = self.inventory.drag.as_ref().map(|drag| {
            let preview = self.inventory.pointer.and_then(|point| self.preview(drag, point));
            json!({"id":drag.id,"source":self.inventory.owner(drag.source),"preview":preview.map(|preview| {
                json!({"bounds":rect(preview.rect),"valid":preview.valid,
                    "owner":preview.slot.and_then(|(index, _)| self.inventory.owner(index)),
                    "slot":preview.slot.map(|(_, slot)| slot)})
            })})
        });
        let controls = self
            .placed
            .iter()
            .enumerate()
            .filter_map(|(index, placement)| {
                let component = &self.components[index].config;
                (matches!(
                    component.action,
                    Some(
                        Action::InventoryTakeAll
                            | Action::InventoryArrange
                            | Action::InventoryScrollUp
                            | Action::InventoryScrollDown
                    )
                ) || matches!(
                    component.binding,
                    Some(Binding::InventoryDrop | Binding::InventoryUse)
                ))
                .then_some((component.id.as_str(), rect(placement.as_ref()?.rect)))
            })
            .collect::<BTreeMap<_, _>>();
        let equipment: BTreeMap<_, _> = self.placed.iter().enumerate().filter_map(|(index, placement)| {
            let slot = self.components[index].config.equipment_slot?;
            let item = self.equipped(slot);
            Some((slot.name(), json!({"bounds":rect(placement.as_ref()?.rect),
                "item":item.map(|item| &item.id), "name":item.map(|item| &item.name),
                "art":item.and_then(|item| item.icon.as_ref().ok()).map(|art|&art.source.virtual_path)})))
        }).collect();
        json!({"mode":self.inventory.view,"equipment":equipment,"quantity":self.inventory.quantity.as_ref()
            .map(|quantity| json!({"max":quantity.max,"input":quantity.input})),"container_id":self.inventory.target,
            "panel_bounds":panel_bounds,"lens_bounds":component_bounds(Binding::InventoryLens),
            "backpack":owner(0),"container":owner(1),"hovered_item":self.inventory.hovered,
            "error":self.inventory.error,"drag":drag,"controls":controls})
    }
}

pub(super) fn equipment_icon(placement: &Placement, asset: &LoadedFrame) -> Rect {
    let frame = &asset.decoded.frame;
    let scale = placement
        .scale
        .min(placement.full.w * 100 / frame.width)
        .min(placement.full.h * 100 / frame.height);
    let (w, h) = (
        (frame.width * scale / 100).max(1),
        (frame.height * scale / 100).max(1),
    );
    Rect {
        x: placement.full.x + ((placement.full.w - w) / 2) as i32,
        y: placement.full.y + ((placement.full.h - h) / 2) as i32,
        w,
        h,
    }
}

impl Hud {
    pub(in crate::viewer) fn inventory_message(&self) -> &str {
        self.inventory
            .error
            .as_deref()
            .or_else(|| {
                self.components
                    .get(self.bound(Binding::InventoryHover)?)?
                    .label
                    .as_ref()
                    .map(|label| label.value.as_str())
            })
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrolled_footprints_and_icons_share_fractional_grid_edges() {
        for scale in [83, 125, 180] {
            let full = Rect {
                x: 40,
                y: 50,
                w: 320 * scale / 100,
                h: 256 * scale / 100,
            };
            let grid = GridView {
                clip: full,
                full,
                scale,
                cell: 32,
                rows: 8,
                offset: 1,
            };
            let first = grid.item_area(0, 10, [2, 2]);
            let below = grid.item_area(20, 10, [2, 1]);
            let visible = first.intersect(full).unwrap();
            assert_eq!(visible.y, full.y);
            assert_eq!(visible.y + visible.h as i32, below.y);
            assert!(visible.contains((full.x as u32, full.y as u32)));
            assert!(!visible.contains((full.x as u32, below.y as u32)));
            let icon = grid.icon_area(0, 10, [2, 2], &sprite([64, 64], [255; 4]));
            assert_eq!(icon.intersect(full), Some(visible));
            for col in 0..10 {
                let area = grid.area(col, 0, [1, 1]);
                assert_eq!(grid.cell_at((area.x as u32, area.y as u32)), Some([col, 0]));
                assert_eq!(
                    grid.cell_at(((area.x + area.w as i32 - 1) as u32, area.y as u32)),
                    Some([col, 0])
                );
            }
        }
    }

    #[test]
    fn inventory_group_fits_both_windows_and_preserves_the_requested_scale() {
        let mut game = Vfs::new(Policy::Ordered);
        let package =
            std::env::temp_dir().join(format!("arcanum-inventory-layout-{}", std::process::id()));
        std::fs::create_dir(&package).unwrap();
        let manifest = package.join("theme.toml");
        std::fs::write(
            &manifest,
            r#"
name = "Inventory layout test"
[[hud]]
id = "top"
anchor = "top"
size = [800, 41]
capture = true
[[hud]]
id = "bottom"
anchor = "bottom"
size = [800, 159]
capture = true
[[hud]]
id = "inventory"
anchor = "center"
size = [800, 400]
capture = true
inventory_panel = true
"#,
        )
        .unwrap();
        let mut hud = Hud::load(
            &ThemeSource::external(manifest.clone()),
            &mut game,
            Some(300),
        )
        .unwrap();
        std::fs::remove_file(manifest).unwrap();
        std::fs::remove_dir(package).unwrap();
        hud.inventory.view = Some(InventoryView::Inventory);
        for (size, effective) in [((936, 500), 83), ((1920, 1080), 180)] {
            hud.layout(size);
            assert_eq!(hud.effective_scale, effective);
            let [top, bottom, panel] =
                std::array::from_fn(|i| hud.placed[i].as_ref().unwrap().rect);
            assert!(top.y + top.h as i32 <= panel.y);
            assert!(panel.y + panel.h as i32 <= bottom.y);
            assert_eq!(top.w, panel.w);
            assert_eq!(bottom.w, panel.w);
            assert_eq!(hud.hit((panel.x as u32, panel.y as u32)), Hit::Panel);
            assert_eq!(hud.scale(), 300);
        }
        hud.close_inventory();
        hud.layout((936, 500));
        assert!(hud.placed[2].is_none());
        assert_eq!(hud.effective_scale, 83);
    }
}
