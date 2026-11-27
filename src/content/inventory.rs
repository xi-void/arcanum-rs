//! Session ownership and placement for original items.
use super::{
    Source,
    art::FrameCache,
    character::EquipmentSlot,
    diagnostics, load_mes,
    mobile::{item_art_entry, population::Effective},
};
use crate::{
    sector::objects::{ItemStats, OriginalId},
    vfs::Vfs,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub(super) type Tile = (u32, u32);
pub(super) const COLUMNS: usize = 10;
pub(super) const PLAYER_ROWS: usize = 12;
pub(super) const CONTAINER_ROWS: usize = 96;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub(super) enum Owner {
    Player,
    Container(String, OriginalId),
    OriginalObject(String, OriginalId),
}

impl Owner {
    pub fn key(&self) -> String {
        match self {
            Self::Player => "player".into(),
            Self::Container(_, id) | Self::OriginalObject(_, id) => id.key(),
        }
    }

    pub fn rows(&self) -> usize {
        match self {
            Self::Player => PLAYER_ROWS,
            Self::Container(..) => CONTAINER_ROWS,
            Self::OriginalObject(..) => PLAYER_ROWS,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(super) enum Place {
    Ground(String, Tile),
    Owned(Owner),
    Equipped(EquipmentSlot),
    Merged(OriginalId),
    Spent,
    Consumed,
    Retired,
}

impl Place {
    pub fn report(&self) -> Value {
        match self {
            Self::Owned(Owner::Player) => json!({"kind":"player"}),
            Self::Owned(Owner::Container(map, id)) => {
                json!({"kind":"container","map":map,"id":id.key()})
            }
            Self::Owned(Owner::OriginalObject(map, id)) => {
                json!({"kind":"original_object","map":map,"id":id.key()})
            }
            Self::Equipped(slot) => json!({"kind":"equipped","slot":slot}),
            Self::Retired => json!({"kind":"restocked"}),
            Self::Spent => json!({"kind":"spent"}),
            Self::Consumed => json!({"kind":"consumed"}),
            Self::Merged(id) => json!({"kind":"merged","into":id.key()}),
            Self::Ground(map, tile) => json!({"kind":"world","map":map,"tile":tile}),
        }
    }
}

#[derive(Clone)]
pub(super) struct Footprint {
    pub cells: [usize; 2],
    pub source: Option<Source>,
    pub fallback_reason: Option<String>,
}

impl Footprint {
    pub fn load(vfs: &mut Vfs, cache: &mut FrameCache, art: Option<u32>) -> Self {
        let frame = inventory_frame(vfs, cache, art);
        match frame {
            Ok(frame) => Self {
                cells: [
                    frame.decoded.frame.width.div_ceil(32) as usize,
                    frame.decoded.frame.height.div_ceil(32) as usize,
                ],
                source: Some(frame.source.clone()),
                fallback_reason: None,
            },
            // CE item_inv_icon_size uses a single cell when ART lookup fails.
            Err(error) => Self {
                cells: [1, 1],
                source: None,
                fallback_reason: Some(error.to_string()),
            },
        }
    }
}

pub(super) fn inventory_frame(
    vfs: &mut Vfs,
    cache: &mut FrameCache,
    art: Option<u32>,
) -> Result<std::sync::Arc<super::art::LoadedFrame>, Box<dyn std::error::Error>> {
    let id = art
        .filter(|id| id >> 28 == 6)
        .ok_or("Missing or unsupported item inventory ART ID")?;
    let table = match (id >> 12) & 3 {
        0 => "art/item/item_ground.mes",
        1 => "art/item/item_inven.mes",
        2 => "art/item/item_paper.mes",
        _ => "art/item/item_schematic.mes",
    };
    let names = load_mes(vfs, table)?;
    let name = names
        .mes
        .get(item_art_entry(id))
        .ok_or("Missing item ART table entry")?;
    let name = crate::resource_path::normalize(name.trim())?;
    if name.contains('/') || !name.ends_with(".art") {
        return Err("Invalid item ART name".into());
    }
    cache.load(
        vfs,
        &format!("art/item/{name}"),
        0,
        ((id >> 4) & 3) as u8,
        false,
    )
}

#[derive(Clone)]
pub(super) struct Item {
    pub id: OriginalId,
    pub category: u32,
    pub description: Option<u32>,
    pub prototype: u32,
    pub instance_source: Source,
    pub prototype_source: Source,
    pub origin_map: String,
    pub original: Place,
    pub authored_slot: Option<i32>,
    pub inventory_art: Option<u32>,
    pub footprint: Footprint,
    pub effective: Effective,
    pub flags: u32,
    pub scripted: bool,
    pub stats: ItemStats,
    pub generated: bool,
}

impl Item {
    /// CE sfx_item_sound maps object materials to pickup and drop sound IDs.
    pub fn handling_sound(&self, pickup: bool, aptitude: i32) -> i32 {
        let offset = if self.stats.unknown_description == Some(9056) {
            8
        } else {
            match self.effective.appearance.material {
                Some(0 | 1) => 3,
                Some(2 | 3) => 4,
                Some(5) if self.weight(aptitude) > 2000 => 2,
                Some(5 | 11) => 1,
                Some(6) => 5,
                Some(7) => 6,
                Some(9) => 7,
                _ => 0,
            }
        };
        (if pickup { 5950 } else { 5960 }) + offset
    }

    pub fn weight(&self, aptitude: i32) -> i64 {
        if self.category == 8 {
            return 0;
        }
        let quantity = if self.category == 6 {
            self.stats.quantity.unwrap_or_default() / 4
        } else {
            1
        };
        // Bound malformed negative weights and keep even extreme wire values representable.
        (i64::from(self.stats.weight.unwrap_or_default())
            + self.stats.adjust_magic(self.stats.magic_weight, aptitude))
        .clamp(0, i64::from(i32::MAX))
            * i64::from(quantity)
    }

    pub fn detail(&self, aptitude: i32) -> String {
        let mut detail = self
            .stats
            .quantity
            .map_or_else(String::new, |quantity| format!("×{quantity}  "));
        use std::fmt::Write;
        let _ = write!(detail, "Weight: {} st", self.weight(aptitude));
        if self.category == 7 {
            let armor = i64::from(self.stats.armor_class.unwrap_or_default())
                + self
                    .stats
                    .adjust_magic(self.stats.magic_armor_class, aptitude);
            let _ = write!(detail, "  AC: {armor:+}");
        }
        detail
    }

    pub fn same_stack(&self, other: &Self) -> bool {
        matches!(self.category, 6 | 8)
            && self.category == other.category
            && self.prototype == other.prototype
            && self.stats.ammo_type == other.stats.ammo_type
            && self.stats.quantity.is_some()
            && other.stats.quantity.is_some()
    }

    pub fn report(&self, place: &Place, slot: Option<usize>) -> Value {
        json!({
            "id":self.id.key(),"category":crate::sector::objects::category_name(self.category),
            "prototype":self.prototype,"origin_map":self.origin_map,"generated":self.generated,
            "quantity":self.stats.quantity, "ammo_type":self.stats.ammo_type,
            "item_statistics":self.stats,
            "instance_source":diagnostics::source(&self.instance_source),
            "prototype_source":diagnostics::source(&self.prototype_source),
            "original":self.original.report(),"current":place.report(),
            "authored_slot":self.authored_slot,"slot":slot,
            "footprint":self.footprint.cells,"inventory_art":self.inventory_art,
            "footprint_source":self.footprint.source.as_ref().map(diagnostics::source),
            "footprint_fallback":self.footprint.fallback_reason,
        })
    }
}

#[derive(Default)]
pub(super) struct Grid {
    pub items: BTreeMap<OriginalId, (usize, [usize; 2])>,
}

impl Grid {
    pub fn contains_cell(&self, id: OriginalId, slot: usize) -> bool {
        self.items
            .get(&id)
            .is_some_and(|&(start, [width, height])| {
                (start / COLUMNS..start / COLUMNS + height).contains(&(slot / COLUMNS))
                    && (start % COLUMNS..start % COLUMNS + width).contains(&(slot % COLUMNS))
            })
    }

    pub fn fit(
        &self,
        id: OriginalId,
        [w, h]: [usize; 2],
        rows: usize,
        requested: Option<usize>,
    ) -> Option<usize> {
        self.fit_ordered(id, [w, h], rows, requested, false)
    }

    pub fn fit_ordered(
        &self,
        id: OriginalId,
        [w, h]: [usize; 2],
        rows: usize,
        requested: Option<usize>,
        vertical: bool,
    ) -> Option<usize> {
        if w == 0 || h == 0 || w > COLUMNS || h > rows {
            return None;
        }
        let free = |slot: usize| {
            let (x, y) = (slot % COLUMNS, slot / COLUMNS);
            x + w <= COLUMNS
                && y <= rows - h
                && self.items.iter().all(|(other, &(occupied, [ow, oh]))| {
                    let (ox, oy) = (occupied % COLUMNS, occupied / COLUMNS);
                    *other == id || x + w <= ox || ox + ow <= x || y + h <= oy || oy + oh <= y
                })
        };
        match requested {
            Some(slot) => free(slot).then_some(slot),
            None => (0..rows * COLUMNS)
                .map(|index| {
                    if vertical {
                        index % rows * COLUMNS + index / rows
                    } else {
                        index
                    }
                })
                .find(|&slot| free(slot)),
        }
    }
}

#[derive(Clone)]
pub(super) struct Change {
    pub item: Item,
    pub place: Place,
    pub slot: Option<usize>,
    pub order: usize,
}

#[derive(Clone, Default)]
pub(super) struct State {
    pub changes: BTreeMap<OriginalId, Change>,
    pub next_generated: u64,
}

impl State {
    pub fn place<'a>(&'a self, item: &'a Item) -> &'a Place {
        self.changes
            .get(&item.id)
            .map_or(&item.original, |change| &change.place)
    }

    pub fn relocate(
        &mut self,
        item: Item,
        destination: Place,
        grid: Option<(&Grid, Option<usize>)>,
    ) -> Result<Value, &'static str> {
        let slot = match (&destination, grid) {
            (Place::Owned(owner), Some((grid, requested))) => Some(
                grid.fit(item.id, item.footprint.cells, owner.rows(), requested)
                    .ok_or("Item does not fit in the requested destination")?,
            ),
            (Place::Ground(..), None) => None,
            _ => return Err("Invalid inventory destination"),
        };
        let result = json!({
            "id":item.id.key(),"from":self.place(&item).report(),"to":destination.report(),
            "slot":slot,"footprint":item.footprint.cells,"quantity":item.stats.quantity,
        });
        self.put(item, destination, slot);
        Ok(result)
    }

    pub fn generated_id(&self) -> OriginalId {
        let mut payload = [0; 16];
        payload[..8].copy_from_slice(&self.next_generated.to_le_bytes());
        OriginalId {
            kind: u16::MAX,
            payload,
        }
    }

    pub fn put(&mut self, item: Item, destination: Place, slot: Option<usize>) {
        let order = self
            .changes
            .get(&item.id)
            .map_or(self.changes.len(), |change| change.order);
        self.changes.insert(
            item.id,
            Change {
                item,
                place: destination,
                slot,
                order,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: u8) -> OriginalId {
        OriginalId {
            kind: 2,
            payload: [n; 16],
        }
    }

    #[test]
    fn rectangular_placement_checks_edges_overlap_and_capacity() {
        let mut grid = Grid::default();
        let rows = PLAYER_ROWS;
        assert_eq!(grid.fit(id(1), [2, 2], rows, Some(9)), None);
        assert_eq!(grid.fit(id(1), [2, 2], rows, Some(110)), None);
        assert_eq!(grid.fit(id(1), [2, 2], rows, Some(108)), Some(108));
        grid.items.insert(id(1), (0, [2, 2]));
        assert_eq!(grid.fit_ordered(id(2), [2, 2], rows, None, true), Some(20));
        assert_eq!(grid.fit(id(2), [1, 1], rows, Some(11)), None);
        assert_eq!(grid.fit(id(2), [2, 2], rows, None), Some(2));
        assert_eq!(grid.fit(id(1), [2, 2], rows, Some(1)), Some(1));
        grid.items.insert(id(1), (0, [COLUMNS, rows]));
        assert_eq!(grid.fit(id(2), [1, 1], rows, None), None);
        assert_eq!(grid.fit(id(2), [1, 1], CONTAINER_ROWS, None), Some(120));
    }
}
