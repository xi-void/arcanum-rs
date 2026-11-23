//! Wall-run topology in CE wallcheck_recalc, independent of render coordinates.

use super::Tile;
use std::collections::BTreeMap;

#[derive(Clone, Copy)]
pub(in crate::content) struct Wall<K> {
    pub key: K,
    pub rotation: u8,
}

struct Run {
    crosses_opening: bool,
    from: usize,
    to: usize,
    sides: [u8; 2],
    step: (i32, i32),
    exclude: i64,
    quadrants: [i64; 4],
}

const QUADRANTS: [[u8; 4]; 4] = [[3, 1, 1, 9], [2, 3, 9, 8], [2, 6, 12, 8], [6, 4, 4, 12]];
const OFFSETS: [(i32, i32); 27] = [
    (8, -8),
    (4, -8),
    (8, -4),
    (4, -4),
    (0, -4),
    (12, 0),
    (8, 0),
    (4, 0),
    (0, 0),
    (-4, 0),
    (16, 4),
    (12, 4),
    (8, 4),
    (4, 4),
    (0, 4),
    (-4, 4),
    (-8, 4),
    (12, 8),
    (8, 8),
    (4, 8),
    (0, 8),
    (-4, 8),
    (-8, 8),
    (8, 12),
    (4, 12),
    (0, 12),
    (4, 16),
];

// The canonical wall-run exclusions are bitsets of other run indices.
#[rustfmt::skip]
const RUNS: [Run; 34] = [
    Run { crosses_opening: true, from: 7, to: 13, sides: [2, 1], step: (0, 1), exclude: 0x154105450, quadrants: [0x280000800, 0x000000000, 0x000440000, 0x280440800] },
    Run { crosses_opening: true, from: 13, to: 14, sides: [32, 16], step: (-1, 0), exclude: 0x2a820a8a0, quadrants: [0x000000000, 0x140000400, 0x140880400, 0x000880000] },
    Run { crosses_opening: false, from: 7, to: 8, sides: [0, 0], step: (-1, 0), exclude: 0x001460100, quadrants: [0x004000000, 0x004000000, 0x000000000, 0x000000000] },
    Run { crosses_opening: false, from: 8, to: 14, sides: [0, 0], step: (0, 1), exclude: 0x002890200, quadrants: [0x008000000, 0x008000000, 0x000000000, 0x000000000] },
    Run { crosses_opening: true, from: 12, to: 13, sides: [32, 16], step: (-1, 0), exclude: 0x140000400, quadrants: [0x280000800, 0x000000000, 0x000000000, 0x280000800] },
    Run { crosses_opening: true, from: 13, to: 19, sides: [2, 1], step: (0, 1), exclude: 0x280000000, quadrants: [0x000000000, 0x140000c00, 0x140000c00, 0x000000000] },
    Run { crosses_opening: true, from: 6, to: 12, sides: [2, 1], step: (0, 1), exclude: 0x010104000, quadrants: [0x000000000, 0x000000000, 0x000000000, 0x000000000] },
    Run { crosses_opening: true, from: 19, to: 20, sides: [32, 16], step: (-1, 0), exclude: 0x020208000, quadrants: [0x000000000, 0x000000000, 0x000000000, 0x000000000] },
    Run { crosses_opening: true, from: 3, to: 7, sides: [2, 1], step: (0, 1), exclude: 0x000440000, quadrants: [0x004000000, 0x004000000, 0x000000000, 0x000000000] },
    Run { crosses_opening: true, from: 14, to: 15, sides: [32, 16], step: (-1, 0), exclude: 0x000880000, quadrants: [0x008000000, 0x008000000, 0x000000000, 0x000000000] },
    Run { crosses_opening: true, from: 12, to: 18, sides: [2, 1], step: (0, 1), exclude: 0x140000000, quadrants: [0x000000000, 0x000000000, 0x000000000, 0x000000000] },
    Run { crosses_opening: true, from: 18, to: 19, sides: [32, 16], step: (-1, 0), exclude: 0x280000000, quadrants: [0x000000000, 0x000000000, 0x000000000, 0x000000000] },
    Run { crosses_opening: false, from: 6, to: 7, sides: [0, 0], step: (-1, 0), exclude: 0x004000000, quadrants: [0x000000000, 0x000000000, 0x000400000, 0x000400000] },
    Run { crosses_opening: false, from: 14, to: 20, sides: [0, 0], step: (0, 1), exclude: 0x008000000, quadrants: [0x000000000, 0x000000000, 0x000800000, 0x000800000] },
    Run { crosses_opening: true, from: 5, to: 11, sides: [2, 1], step: (0, 1), exclude: 0x010000000, quadrants: [0x000000000, 0x000000000, 0x000000000, 0x000000000] },
    Run { crosses_opening: true, from: 24, to: 25, sides: [32, 16], step: (-1, 0), exclude: 0x020000000, quadrants: [0x000000000, 0x000000000, 0x000000000, 0x000000000] },
    Run { crosses_opening: false, from: 9, to: 15, sides: [0, 0], step: (0, 1), exclude: 0x002000000, quadrants: [0x000000000, 0x000000000, 0x000000000, 0x000000000] },
    Run { crosses_opening: false, from: 3, to: 4, sides: [0, 0], step: (-1, 0), exclude: 0x001000000, quadrants: [0x000000000, 0x000000000, 0x000000000, 0x000000000] },
    Run { crosses_opening: false, from: 2, to: 3, sides: [0, 0], step: (-1, 0), exclude: 0x000400000, quadrants: [0x000000000, 0x000000000, 0x000000000, 0x000000000] },
    Run { crosses_opening: false, from: 15, to: 21, sides: [0, 0], step: (0, 1), exclude: 0x000800000, quadrants: [0x000000000, 0x000000000, 0x000000000, 0x000000000] },
    Run { crosses_opening: true, from: 11, to: 12, sides: [32, 16], step: (-1, 0), exclude: 0x000000000, quadrants: [0x040000000, 0x000000000, 0x000000000, 0x040000000] },
    Run { crosses_opening: true, from: 19, to: 24, sides: [2, 1], step: (0, 1), exclude: 0x000000000, quadrants: [0x000000000, 0x080000000, 0x080000000, 0x000000000] },
    Run { crosses_opening: true, from: 0, to: 2, sides: [2, 1], step: (0, 1), exclude: 0x000000000, quadrants: [0x000000000, 0x000000000, 0x000000000, 0x000000000] },
    Run { crosses_opening: true, from: 21, to: 22, sides: [32, 16], step: (-1, 0), exclude: 0x000000000, quadrants: [0x000000000, 0x000000000, 0x000000000, 0x000000000] },
    Run { crosses_opening: true, from: 1, to: 3, sides: [2, 1], step: (0, 1), exclude: 0x000000000, quadrants: [0x000000000, 0x000000000, 0x000000000, 0x000000000] },
    Run { crosses_opening: true, from: 15, to: 16, sides: [32, 16], step: (-1, 0), exclude: 0x000000000, quadrants: [0x000000000, 0x000000000, 0x000000000, 0x000000000] },
    Run { crosses_opening: true, from: 2, to: 6, sides: [2, 1], step: (0, 1), exclude: 0x000000000, quadrants: [0x000000000, 0x000000000, 0x000000000, 0x000000000] },
    Run { crosses_opening: true, from: 20, to: 21, sides: [32, 16], step: (-1, 0), exclude: 0x000000000, quadrants: [0x000000000, 0x000000000, 0x000000000, 0x000000000] },
    Run { crosses_opening: true, from: 10, to: 11, sides: [32, 16], step: (-1, 0), exclude: 0x000000000, quadrants: [0x000000000, 0x000000000, 0x000000000, 0x000000000] },
    Run { crosses_opening: true, from: 24, to: 26, sides: [2, 1], step: (0, 1), exclude: 0x000000000, quadrants: [0x000000000, 0x000000000, 0x000000000, 0x000000000] },
    Run { crosses_opening: true, from: 11, to: 17, sides: [2, 1], step: (0, 1), exclude: 0x000000000, quadrants: [0x000000000, 0x000000000, 0x000000000, 0x000000000] },
    Run { crosses_opening: true, from: 23, to: 24, sides: [32, 16], step: (-1, 0), exclude: 0x000000000, quadrants: [0x000000000, 0x000000000, 0x000000000, 0x000000000] },
    Run { crosses_opening: true, from: 17, to: 18, sides: [32, 16], step: (-1, 0), exclude: 0x000000000, quadrants: [0x000000000, 0x000000000, 0x000000000, 0x000000000] },
    Run { crosses_opening: true, from: 18, to: 23, sides: [2, 1], step: (0, 1), exclude: 0x000000000, quadrants: [0x000000000, 0x000000000, 0x000000000, 0x000000000] },
];

pub(in crate::content) fn select<'a, K: Copy + Ord + 'a>(
    (x, y): Tile,
    walls: impl Fn(Tile) -> &'a [Wall<K>],
) -> BTreeMap<K, (Tile, u8)> {
    let mut result = BTreeMap::new();
    let player = (x, y);
    let (mut x, mut y) = (i64::from(x) - 2, i64::from(y) - 2);
    let (mut col, mut row) = (x.rem_euclid(4), y.rem_euclid(4));
    if col == 0 || row == 0 {
        let x_aligned = col == 0;
        for wall in walls(player) {
            match wall.rotation {
                4 | 5 if x_aligned => {
                    col = 3;
                    x -= 1;
                }
                2 | 3 => {
                    row = 3;
                    y -= 1;
                }
                _ => {}
            }
        }
    }
    let base = (x - col + 2, y - row + 2);
    let quadrant = QUADRANTS[row as usize][col as usize];
    let at = |(dx, dy): (i32, i32)| {
        let tile = (
            u32::try_from(base.0 + i64::from(dx)).ok()?,
            u32::try_from(base.1 + i64::from(dy)).ok()?,
        );
        Some((tile, walls(tile)))
    };
    let mut exclusions = 0;
    let mut samples = [[None; 3]; 34];
    for (index, run) in RUNS.iter().enumerate() {
        if exclusions & (1 << index) != 0 {
            continue;
        }
        let (dx, dy) = OFFSETS[run.from];
        for (step, sample) in samples[index].iter_mut().enumerate() {
            let step = step as i32 + 1;
            *sample = at((dx + step * run.step.0, dy + step * run.step.1))
                .and_then(|(tile, walls)| Some((tile, *walls.first()?)));
            if index < 22 && sample.is_none() && !run.crosses_opening {
                break;
            }
        }
        if index < 22 && samples[index].iter().all(Option::is_some) {
            exclusions |= run.exclude;
            for (side, exclude) in run.quadrants.iter().enumerate() {
                if quadrant & (1 << side) != 0 {
                    exclusions |= exclude;
                }
            }
        }
    }
    let mut sides = [0; OFFSETS.len()];
    for (index, run) in RUNS.iter().enumerate() {
        if exclusions & (1 << index) != 0 || !run.crosses_opening {
            continue;
        }
        for (tile, wall) in samples[index].iter().flatten() {
            result.insert(wall.key, (*tile, 3));
            sides[run.from] |= run.sides[0];
            sides[run.to] |= run.sides[1];
        }
    }
    for (offset, side) in OFFSETS
        .into_iter()
        .zip(sides)
        .filter(|(_, side)| *side != 0)
    {
        if let Some((tile, walls)) = at(offset) {
            for wall in walls {
                let fade = if matches!(wall.rotation & 6, 0 | 4) {
                    side & 3
                } else {
                    side >> 4
                };
                if fade != 0 {
                    result.insert(wall.key, (tile, fade));
                }
            }
        }
    }
    result
}
