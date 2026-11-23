//! Original movement blockers, independent of sprite resolution.

use super::sector::read_static_prototype;
use crate::{
    facade::FacadeId,
    mes::Mes,
    prototype::SerializedPrototypeId,
    sector::{
        Floor, SIDE, TileArtId,
        objects::{self, AppearanceFields, ObjectList, StaticCategory},
    },
    vfs::Vfs,
};
use std::{collections::BTreeMap, error::Error};

type Tile = (u32, u32);

const INACTIVE: u32 = 0x3; // OF_DESTROYED | OF_OFF
const NO_BLOCK: u32 = 0x400;

#[derive(Clone, Copy, Default)]
pub(super) struct Cell {
    blocked: bool,
    edges: u8,
}

pub(super) struct Collision {
    cells: Box<[Cell; SIDE * SIDE]>,
}

// CE location_in_dir rotation order.
pub(super) const DIRECTIONS: [(i32, i32); 8] = [
    (-1, -1),
    (-1, 0),
    (-1, 1),
    (0, 1),
    (1, 1),
    (1, 0),
    (1, -1),
    (0, -1),
];

fn index((x, y): Tile) -> usize {
    (y as usize % SIDE) * SIDE + x as usize % SIDE
}

impl Collision {
    pub(super) fn terrain(floor: &Floor, names: &Mes) -> Self {
        let groups = [0, 100, 200, 300].map(|start| {
            let mut blocked = [true; 64];
            for (slot, (_, entry)) in blocked.iter_mut().zip(
                names
                    .entries()
                    .skip_while(|&(id, _)| id < start)
                    .take_while(|&(id, _)| id < start + 100),
            ) {
                // a_name.c reads the flags between the slash and the first space.
                *slot = entry.split_once('/').is_some_and(|(_, flags)| {
                    flags
                        .split(' ')
                        .next()
                        .is_some_and(|flags| flags.contains(['b', 'f']))
                });
            }
            blocked
        });
        let mut cells = Box::new([Cell::default(); SIDE * SIDE]);
        for (cell, TileArtId(raw)) in cells.iter_mut().zip(floor.ids()) {
            cell.blocked = if let Some(facade) = FacadeId::from_raw(raw) {
                !facade.fields().walkable
            } else if raw >> 28 == 0 {
                let group = usize::from(raw & 0x100 == 0) * 2 + usize::from(raw & 0x80 == 0);
                groups[group][(raw >> 22) as usize & 63]
            } else {
                true
            };
        }
        Self { cells }
    }

    pub(super) fn with_objects(mut self, vfs: &mut Vfs, list: &ObjectList) -> Self {
        for (i, cell) in self.cells.iter_mut().enumerate() {
            cell.blocked |= list.tile_blocks[i / 8] & (1 << (i % 8)) != 0;
        }
        let mut prototypes = BTreeMap::new();
        for record in list.walls.iter().chain(&list.portals).chain(&list.scenery) {
            let mut fields = record.fields;
            if fields.flags.is_none() || record.category == StaticCategory::Wall {
                let parent = prototype(vfs, &mut prototypes, record.prototype, record.category);
                fields.flags = fields.flags.or(parent.flags);
                fields.spell_flags = fields.spell_flags.or(parent.spell_flags);
            }
            self.apply(record.stored_tile, record.category, record.art_id, fields);
        }
        for record in &list.incomplete_scenery {
            if let Some(tile) = record.stored_tile {
                let flags = record.fields.flags.or_else(|| {
                    prototype(
                        vfs,
                        &mut prototypes,
                        record.prototype,
                        StaticCategory::Scenery,
                    )
                    .flags
                });
                self.apply(
                    tile,
                    StaticCategory::Scenery,
                    0,
                    AppearanceFields {
                        flags,
                        ..record.fields
                    },
                );
            }
        }
        self
    }

    fn apply(&mut self, tile: Tile, category: StaticCategory, art: u32, fields: AppearanceFields) {
        if fields.flags.unwrap_or(0) & INACTIVE != 0 {
            return;
        }
        let cell = &mut self.cells[index(tile)];
        let edge = 1 << (((art >> 11) & 7) | 1);
        match category {
            StaticCategory::Wall if art >> 28 == 1 => {
                let piece = (art >> 14) & 63;
                let opening = matches!(piece, 22 | 25 | 26 | 29 | 30 | 31 | 32);
                if !opening && fields.spell_flags.unwrap_or(0) & 0x0010_0000 == 0 {
                    cell.edges |= edge;
                }
            }
            // CE's portal_is_open uses the current ART frame, not the lock flags.
            StaticCategory::Portal if art >> 28 == 3 => {
                if (art >> 14) & 31 == 0 && art & 0x200 == 0 {
                    cell.edges |= edge;
                }
            }
            StaticCategory::Scenery => {
                cell.blocked |= fields.flags.unwrap_or(0) & NO_BLOCK == 0;
            }
            _ => cell.blocked = true,
        }
    }

    pub(super) fn cell(&self, tile: Tile) -> Cell {
        self.cells[index(tile)]
    }
}

fn prototype(
    vfs: &mut Vfs,
    cache: &mut BTreeMap<SerializedPrototypeId, AppearanceFields>,
    id: SerializedPrototypeId,
    category: StaticCategory,
) -> AppearanceFields {
    *cache.entry(id).or_insert_with(|| {
        fn read(
            vfs: &mut Vfs,
            id: SerializedPrototypeId,
            category: StaticCategory,
        ) -> Result<AppearanceFields, Box<dyn Error>> {
            let (_, bytes) = read_static_prototype(vfs, id)?;
            Ok(match category {
                StaticCategory::Wall => objects::decode_wall_prototype(&bytes, id)?,
                StaticCategory::Portal => objects::decode_portal_prototype(&bytes, id)?.appearance,
                StaticCategory::Scenery => {
                    objects::decode_scenery_prototype(&bytes, id)?.appearance
                }
            })
        }
        // Unknown flags cannot grant passage through an object.
        read(vfs, id, category).unwrap_or_default()
    })
}

pub(super) fn can_stand(cell: Option<Cell>) -> bool {
    cell.is_some_and(|cell| !cell.blocked)
}

pub(super) fn step(tile: Tile, direction: u8, cell: impl Fn(Tile) -> Option<Cell>) -> Option<Tile> {
    let neighbor = |(x, y): Tile, direction: u8| {
        let (dx, dy) = DIRECTIONS[usize::from(direction)];
        Some((x.checked_add_signed(dx)?, y.checked_add_signed(dy)?))
    };
    let cardinal = |from, direction| {
        let to = neighbor(from, direction)?;
        let source = cell(from)?;
        let target = cell(to)?;
        (!target.blocked
            && source.edges & (1 << direction) == 0
            && target.edges & (1 << ((direction + 4) % 8)) == 0)
            .then_some(to)
    };
    if direction & 1 != 0 {
        return cardinal(tile, direction);
    }
    // Check both paths round the corner as object_calc_traversal_cost_func does.
    let a = (direction + 7) % 8;
    let b = direction + 1;
    let to = cardinal(cardinal(tile, a)?, b)?;
    cardinal(cardinal(tile, b)?, a)?;
    Some(to)
}

#[cfg(test)]
pub(super) fn test_cell(blocked: bool) -> Cell {
    Cell {
        blocked,
        ..Cell::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terrain_facade_and_sector_mask_each_block_a_tile() {
        let names = Mes::parse_text("{0}{grs/n 0}{1}{wat/f 0}").unwrap();
        let mut bytes = vec![0; 4];
        for raw in [0x180u32, 0x0040_0180, 0xb000_0000, 0xb000_0001] {
            bytes.extend(raw.to_le_bytes());
        }
        bytes.resize(4 + SIDE * SIDE * 4, 0);
        bytes.extend(1u32.to_le_bytes()); // Empty roof list.
        bytes.extend(0xaa0004u32.to_le_bytes());
        bytes.extend([0; 4 + 12 + 24]); // No tile scripts, then fixed metadata.
        bytes.push(1 << 3);
        bytes.extend([0; SIDE * SIDE / 8 - 1]);
        bytes.extend([0; 4]); // No object records.
        let floor = crate::sector::decode_floor(&bytes).unwrap();
        let terrain = Collision::terrain(&floor, &names);
        let open =
            |collision: &Collision| [0, 1, 2, 3].map(|x| can_stand(Some(collision.cell((x, 0)))));
        assert_eq!(open(&terrain), [true, false, false, true]);
        let list = objects::decode(&bytes).unwrap();
        let mut vfs = Vfs::new(crate::vfs::Policy::Ce);
        assert_eq!(
            open(&terrain.with_objects(&mut vfs, &list)),
            [true, false, false, false]
        );
    }

    #[test]
    fn paths_respect_wall_edges_both_ways_and_do_not_cut_corners() {
        let a = (63, 10);
        let b = (64, 10);
        let blocked = |tile| {
            Some(Cell {
                blocked: false,
                edges: if tile == b { 1 << 1 } else { 0 },
            })
        };
        assert_eq!(step(a, 5, blocked), None);
        assert_eq!(step(b, 1, blocked), None);
        assert_eq!(step(a, 4, blocked), None);
        assert_eq!(step(a, 3, blocked), Some((63, 11)));
        assert_eq!(step(a, 5, |tile| (tile == a).then(Cell::default)), None);
        assert_eq!(step(a, 5, |_| Some(Cell::default())), Some(b));
    }

    #[test]
    fn wall_opening_takes_its_door_collision_from_the_current_art_frame() {
        let mut collision = Collision {
            cells: Box::new([Cell::default(); SIDE * SIDE]),
        };
        let tile = (10, 10);
        let fields = AppearanceFields::default();
        collision.apply(
            tile,
            StaticCategory::Wall,
            0x1000_0000 | (22 << 14) | (5 << 11),
            fields,
        );
        assert_eq!(collision.cell(tile).edges, 0);
        collision.apply(
            tile,
            StaticCategory::Portal,
            0x3000_0000 | (3 << 14) | (5 << 11),
            fields,
        );
        assert_eq!(collision.cell(tile).edges, 0);
        collision.apply(
            tile,
            StaticCategory::Portal,
            0x3000_0000 | (5 << 11),
            fields,
        );
        assert_eq!(collision.cell(tile).edges, 1 << 5);
    }
}
