//! Bounded A* over the same original collision edges used by direct movement.

use super::Tile;
use crate::content::collision::{self, Cell};
use std::{
    cmp::Reverse,
    collections::{BTreeMap, BinaryHeap, VecDeque},
};

const NODE_LIMIT: usize = 8192;
const STRAIGHT: u64 = 10;
const DIAGONAL: u64 = 14;

fn estimate(from: Tile, to: Tile) -> u64 {
    let x = u64::from(from.0.abs_diff(to.0));
    let y = u64::from(from.1.abs_diff(to.1));
    DIAGONAL * x.min(y) + STRAIGHT * x.abs_diff(y)
}

pub(super) fn find(
    from: Tile,
    target: Tile,
    cell: impl Fn(Tile) -> Option<Cell>,
) -> Option<VecDeque<u8>> {
    if !collision::can_stand(cell(target)) {
        return None;
    }
    let mut nodes = BTreeMap::from([(from, (0, None))]);
    let mut open = BinaryHeap::from([Reverse((estimate(from, target), 0, from))]);
    while let Some(Reverse((_, cost, tile))) = open.pop() {
        if nodes[&tile].0 != cost {
            continue;
        }
        if tile == target {
            let mut path = VecDeque::new();
            let mut back = tile;
            while let Some((parent, direction)) = nodes[&back].1 {
                path.push_front(direction);
                back = parent;
            }
            return Some(path);
        }
        for direction in 0..8 {
            let Some(to) = collision::step(tile, direction, &cell) else {
                continue;
            };
            let next = cost
                + if direction & 1 == 0 {
                    DIAGONAL
                } else {
                    STRAIGHT
                };
            if nodes.get(&to).is_some_and(|&(prior, _)| prior <= next) {
                continue;
            }
            if nodes.len() == NODE_LIMIT && !nodes.contains_key(&to) {
                return None;
            }
            nodes.insert(to, (next, Some((tile, direction))));
            open.push(Reverse((next + estimate(to, target), next, to)));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::collision::test_cell;

    #[test]
    fn click_detours_around_a_cell_without_cutting_corners() {
        let cell = |(x, y)| (x < 5 && y < 5).then(|| test_cell((x, y) == (2, 2)));
        let mut tile = (1, 2);
        let target = (3, 2);
        let path = find(tile, target, cell).unwrap();
        assert_eq!(path.len(), 4);
        for direction in path {
            tile = collision::step(tile, direction, cell).unwrap();
        }
        assert_eq!(tile, target);
        assert!(find(tile, (2, 2), cell).is_none());
        assert!(find(tile, (5, 2), cell).is_none());
    }

    #[test]
    fn diagonal_is_cheaper_than_two_cardinal_steps() {
        assert_eq!(
            find((0, 0), (1, 1), |_| Some(test_cell(false))).unwrap(),
            VecDeque::from([4])
        );
    }
}
