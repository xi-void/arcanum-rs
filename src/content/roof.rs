//! Original roof ART and CE's tile-driven roof fill rules.

use super::{
    art::{FrameCache, LoadedFrame},
    load_mes,
};
use crate::{
    isometric::project_world_tile, resource_path::normalize, sector::Coordinates, vfs::Vfs,
};
use std::{collections::BTreeMap, error::Error, sync::Arc};

type Tile = (u32, u32);

pub(super) mod wallcheck;

#[derive(Clone, Copy)]
pub(super) struct Cell {
    piece: u8,
    filled: bool,
    faded: bool,
}

impl Cell {
    pub fn opacity(self, wall_fades: bool, shades: [u8; 3]) -> Option<[u8; 4]> {
        if !(self.faded || wall_fades) {
            return None;
        }
        let [full, partial, edge] = shades;
        Some(match self.piece {
            0 => [edge, partial, partial, edge],
            1 => [partial, full, partial, edge],
            2 => [edge, partial, full, partial],
            3 => [partial, full, full, partial],
            4 => [partial, partial, edge, edge],
            5 => [full, full, partial, partial],
            6 => [partial, partial, full, full],
            7 => [edge, edge, partial, partial],
            8 | 12 => [full; 4],
            9 => [partial, edge, edge, partial],
            10 => [full, full, edge, partial],
            11 => [partial, edge, partial, full],
            _ => unreachable!("validated roof piece"),
        })
    }

    pub fn authored_fade(self) -> bool {
        self.faded
    }

    fn interior(self, (x, y): Tile) -> bool {
        // roof_recalc and the coverage table in CE's roof.c.
        let (x, y) = (x & 3, y & 3);
        match self.piece {
            0 => x < 3 && y > 1,
            1 => x < 3,
            2 => y > 1,
            3 => x < 3 || y > 1,
            4 => x < 3 && y < 3,
            5 => x < 3 || y < 3,
            6 => x > 1 || y > 1,
            7 => x > 1 && y > 1,
            8 => true,
            9 => x > 1 && y < 3,
            10 => y < 3,
            11 => x > 1,
            12 => x > 1 || y < 3,
            _ => unreachable!("validated roof piece"),
        }
    }
}

pub(super) fn cell_center(tile: Tile) -> Tile {
    (tile.0 & !3 | 2, tile.1 & !3 | 2)
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum From {
    West,
    East,
    North,
    South,
}

impl From {
    fn accepts(self, piece: u8) -> bool {
        !match self {
            Self::West => matches!(piece, 7 | 9 | 11),
            Self::East => matches!(piece, 0 | 1 | 4),
            Self::North => matches!(piece, 0 | 2 | 7),
            Self::South => matches!(piece, 4 | 9 | 10),
        }
    }

    fn source(self, (x, y): Tile) -> Option<Tile> {
        match self {
            Self::West => Some((x.checked_sub(4)?, y)),
            Self::East => Some((x.checked_add(4)?, y)),
            Self::North => Some((x, y.checked_sub(4)?)),
            Self::South => Some((x, y.checked_add(4)?)),
        }
    }

    fn opposite(self) -> Self {
        match self {
            Self::West => Self::East,
            Self::East => Self::West,
            Self::North => Self::South,
            Self::South => Self::North,
        }
    }
}

/// A lookup distinguishes absent authored cells from sectors still being streamed.
type Lookup = Option<Option<Cell>>;

#[derive(Default)]
pub(super) struct State {
    changes: BTreeMap<Tile, bool>,
    pending: BTreeMap<(Tile, From), bool>,
}

impl State {
    pub fn filled(&self, tile: Tile, original: Cell) -> bool {
        self.changes.get(&tile).copied().unwrap_or(original.filled)
    }

    pub fn covered(&self, tile: Tile, lookup: impl FnOnce(Tile) -> Option<Cell>) -> bool {
        let shifted = (tile.0.saturating_add(3), tile.1.saturating_add(3));
        let center = cell_center(shifted);
        lookup(center).is_some_and(|cell| !self.filled(center, cell) && cell.interior(shifted))
    }

    pub fn recalc(&mut self, tile: Tile, lookup: impl Fn(Tile) -> Lookup) {
        // Resume only edges whose source is still in the state that reached them.
        let mut work = std::mem::take(&mut self.pending)
            .into_iter()
            .filter_map(|((tile, from), fill)| {
                let source = from.source(tile)?;
                (self.changes.get(&source) == Some(&fill)).then_some((tile, Some(from), fill))
            })
            .collect::<Vec<_>>();
        self.flood(&mut work, &lookup);
        let center = cell_center(tile);
        if let Some(Some(cell)) = lookup(center) {
            work.push((center, None, cell.interior(tile)));
            self.flood(&mut work, &lookup);
        }
    }

    fn flood(
        &mut self,
        work: &mut Vec<(Tile, Option<From>, bool)>,
        lookup: &impl Fn(Tile) -> Lookup,
    ) {
        while let Some((tile, from, fill)) = work.pop() {
            match lookup(tile) {
                None => {
                    if let Some(from) = from {
                        self.pending.insert((tile, from), fill);
                    }
                }
                Some(Some(cell))
                    if self.filled(tile, cell) != fill
                        && from.is_none_or(|dir| dir.accepts(cell.piece)) =>
                {
                    self.changes.insert(tile, fill);
                    // Stack order yields CE's +X, -X, +Y, -Y traversal.
                    for dir in [From::South, From::North, From::East, From::West] {
                        if let Some(next) = dir.opposite().source(tile) {
                            work.push((next, Some(dir), fill));
                        }
                    }
                }
                Some(_) => {}
            }
        }
    }
}

pub(super) struct Roof {
    pub tile: (u32, u32),
    pub anchor: (i32, i32),
    pub asset: Arc<LoadedFrame>,
}

#[derive(Default)]
pub(super) struct Prepared {
    pub cells: BTreeMap<Tile, Cell>,
    pub rasters: Vec<Roof>,
    pub issues: Vec<String>,
    pub shades: Option<[u8; 3]>,
}

pub(super) fn prepare(
    vfs: &mut Vfs,
    frames: &mut FrameCache,
    sector: Coordinates,
    reference: (u32, u32),
    words: &[(usize, u32)],
    player: bool,
) -> Prepared {
    let mut result = Prepared::default();
    if words.is_empty() {
        return result;
    }
    let names = match load_mes(vfs, "art/roof/roofname.mes") {
        Ok(names) => names,
        Err(err) => {
            result.issues.push(format!("roof names: {err}"));
            return result;
        }
    };
    match load_mes(vfs, "art/roof/roofshade.mes").map(|table| {
        let shade = |key| table.mes.get(key).and_then(|s| s.trim().parse::<u8>().ok());
        shade(0)
            .zip(shade(1))
            .zip(shade(2))
            .map(|((a, b), c)| [a, b, c])
    }) {
        Ok(Some(shades)) => result.shades = Some(shades),
        Ok(None) => result
            .issues
            .push("invalid roofshade.mes opacity values".into()),
        Err(error) => result.issues.push(format!("roof opacity: {error}")),
    }
    for &(slot, id) in words {
        let mut load = || -> Result<Option<Roof>, Box<dyn Error>> {
            if id >> 28 != 10 || id & 0x0000_0fee != 0 {
                return Err("unsupported roof ART word".into());
            }
            let index = ((id >> 19) & 511) as usize;
            let frame = ((id >> 14) & 31) as usize;
            let mirror = id & 1 != 0;
            let palette = ((id >> 4) & 3) as u8;
            if (!mirror && (frame > 8 || palette != 0)) || (mirror && (frame > 3 || palette != 1)) {
                return Err("unsupported roof piece or palette".into());
            }
            let x = ((slot % 16) * 4 + 2) as u8;
            let y = ((slot / 16) * 4 + 2) as u8;
            let tile = sector.world_tile(x, y).ok_or("invalid roof tile")?;
            let filled = id & 0x2000 != 0;
            if filled && !player {
                return Ok(None); // Stored diagnostic draws only visible authored cells.
            }
            let name = names.mes.entries().nth(index).ok_or("missing roof name")?.1;
            let name = normalize(name)?;
            if name.contains('/') {
                return Err("invalid roof basename".into());
            }
            let path = format!("art/roof/{name}.art");
            let asset = frames.load(vfs, &path, frame, palette, mirror)?;
            let raster = &asset.decoded.frame;
            if raster.width > 512 || raster.height > 512 {
                return Err("roof raster exceeds 512 pixels per axis".into());
            }
            let (cx, cy) = project_world_tile(tile, reference).ok_or("roof projection overflow")?;
            // CE location_xy is 40 px left and 20 px above the tile center.
            // roof_xy subtracts another (120,200). TIG uses x hotspot zero for a flipped roof.
            let anchor = (
                cx.checked_sub(160)
                    .and_then(|x| x.checked_add(if mirror { raster.hotspot.0 } else { 0 }))
                    .ok_or("roof X overflow")?,
                cy.checked_sub(220).ok_or("roof Y overflow")?,
            );
            result.cells.insert(
                tile,
                Cell {
                    piece: frame as u8 + if mirror { 9 } else { 0 },
                    filled,
                    faded: id & 0x1000 != 0,
                },
            );
            Ok(Some(Roof {
                tile,
                anchor,
                asset,
            }))
        };
        match load() {
            Ok(Some(roof)) => result.rasters.push(roof),
            Ok(None) => {}
            Err(err) => result
                .issues
                .push(format!("roof slot {slot} ART {id:#010x}: {err}")),
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roof_fill_crosses_streamed_seam_and_stops_at_the_next_roof_edge() {
        let left = (62, 6);
        let right = (66, 6);
        let independent = (70, 6);
        let cell = |piece| Cell {
            piece,
            filled: false,
            faded: false,
        };
        let lookup = |east_ready: bool, tile| match tile {
            t if t == left => Some(Some(cell(11))),
            t if t == right => east_ready.then_some(Some(cell(1))),
            t if t == independent => Some(Some(cell(11))),
            _ => Some(None),
        };
        let mut state = State::default();
        assert!(state.covered((59, 3), |tile| lookup(true, tile).flatten()));
        state.recalc((62, 6), |tile| lookup(false, tile));
        assert!(state.filled(left, cell(11)));
        assert!(!state.covered((59, 3), |tile| lookup(true, tile).flatten()));
        state.recalc((62, 6), |tile| lookup(true, tile));
        assert!(state.filled(right, cell(1)));
        assert!(!state.filled(independent, cell(11)));

        // Crossing the left piece's authored cut line restores both cells.
        state.recalc((61, 6), |tile| lookup(true, tile));
        assert!(!state.filled(left, cell(11)));
        assert!(!state.filled(right, cell(1)));
        assert!(state.covered((59, 3), |tile| lookup(true, tile).flatten()));
    }
}
