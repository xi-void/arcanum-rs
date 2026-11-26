//! Session ownership and placement for original items.
use super::{
    Source,
    art::FrameCache,
    diagnostics, load_mes,
    mobile::{item_art_entry, population::Effective},
};
use crate::{sector::objects::OriginalId, vfs::Vfs};
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub(super) type Tile = (u32, u32);
pub(super) const COLUMNS: usize = 10;
pub(super) const PLAYER_ROWS: usize = 12;
pub(super) const CONTAINER_ROWS: usize = 96;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Owner {
    Player,
    Container(String, OriginalId),
    OriginalObject(String, OriginalId),
}

impl Owner {
    pub fn rows(&self) -> usize {
        match self {
            Self::Player => PLAYER_ROWS,
            Self::Container(..) => CONTAINER_ROWS,
            Self::OriginalObject(..) => 0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Place {
    Ground(String, Tile),
    Owned(Owner),
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
}

impl Item {
    pub fn report(&self, place: &Place, slot: Option<usize>) -> Value {
        json!({
            "id":self.id.key(),"category":crate::sector::objects::category_name(self.category),
            "prototype":self.prototype,"origin_map":self.origin_map,
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

pub(super) struct Change {
    pub item: Item,
    pub place: Place,
    pub slot: Option<usize>,
    pub order: usize,
}

#[derive(Default)]
pub(super) struct State {
    pub changes: BTreeMap<OriginalId, Change>,
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
        let order = self
            .changes
            .get(&item.id)
            .map_or(self.changes.len(), |change| change.order);
        let result = json!({
            "id":item.id.key(),"from":self.place(&item).report(),"to":destination.report(),
            "slot":slot,"footprint":item.footprint.cells,
        });
        self.changes.insert(
            item.id,
            Change {
                item,
                place: destination,
                slot,
                order,
            },
        );
        Ok(result)
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
