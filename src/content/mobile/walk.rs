//! Bounded playback of an original, unmodified critter walk.

use super::{Caster, Pose};
use crate::{
    content::art::{FrameCache, LoadedFrame},
    isometric::project_world_tile,
    vfs::Vfs,
};
use std::{sync::Arc, time::Duration};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::content) struct Step {
    pub tile: (u32, u32),
    pub offset: (i32, i32),
    pub anchor: (i32, i32),
    pub direction: u8,
    pub frame: Option<usize>,
}

pub(in crate::content) struct Walk {
    pub steps: Vec<Step>,
    pub period: Duration,
    frames: [Vec<Arc<LoadedFrame>>; 2],
}

const DIRECTIONS: [u8; 2] = [3, 5];
// The diagnostic route is relative to the serialized spawn. The entries
// are CE path rotations, not pixel destinations.
const ROUTE: [u8; 4] = [3, 3, 3, 5];

impl Walk {
    pub(super) fn load(
        vfs: &mut Vfs,
        cache: &mut FrameCache,
        stand: &Pose,
        caster: Caster,
        reference: (u32, u32),
        dexterity: i32,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let mut pose = stand.clone();
        let name = pose
            .path
            .strip_suffix("uwxaa.art")
            .ok_or("expected unarmed monster stand")?;
        pose.path = format!("{name}uwxab.art");
        let mut frames = [Vec::new(), Vec::new()];
        for (frames, direction) in frames.iter_mut().zip(DIRECTIONS) {
            let facing = pose.clone().in_direction(direction);
            let first = facing.load(vfs, cache, 0)?;
            let count = first.decoded.frame_count;
            if !(2..=32).contains(&count) {
                return Err("expected bounded directional walk frames".into());
            }
            frames.push(first);
            for frame in 1..count {
                frames.push(facing.load(vfs, cache, frame)?);
            }
        }
        let period = period(dexterity, frames[0][0].decoded.frames_per_second)?;
        let steps = build_steps(caster.tile, reference, &ROUTE, |direction, frame| {
            let sequence = &frames[direction_index(direction)];
            (
                sequence.len(),
                sequence[frame % sequence.len()].decoded.frame.offset,
            )
        })?;
        Ok(Self {
            steps,
            period,
            frames,
        })
    }

    #[cfg(test)]
    pub fn fixture(tile: (u32, u32), frame: &Arc<LoadedFrame>) -> Self {
        let frames = std::array::from_fn(|_| vec![Arc::clone(frame); 6]);
        let steps = build_steps(tile, tile, &ROUTE, |dir, n| {
            let (x, y) = [(9, 4), (5, 3), (5, 3), (5, 2), (7, 4), (12, 6)][n % 6];
            (6, (if dir == 3 { x } else { -x }, y))
        })
        .unwrap();
        Self {
            steps,
            period: period(17, 8).unwrap(),
            frames,
        }
    }

    pub fn asset(&self, step: Step) -> Option<&Arc<LoadedFrame>> {
        Some(&self.frames[direction_index(step.direction)][step.frame?])
    }

    pub fn index(&self, elapsed: Duration) -> usize {
        (elapsed.as_millis() / self.period.as_millis()).min(self.steps.len() as u128 - 1) as usize
    }

    pub fn deadline(&self, index: usize) -> Duration {
        self.period * index as u32
    }

    pub fn frames(&self) -> impl Iterator<Item = &Arc<LoadedFrame>> {
        self.frames.iter().flatten()
    }
}

fn direction_index(direction: u8) -> usize {
    usize::from(direction == DIRECTIONS[1])
}

fn period(dexterity: i32, authored_fps: i32) -> Result<Duration, &'static str> {
    // CE sub_437990 / sub_42EE90 for an ordinary monster, without
    // extraordinary Dexterity, injuries, shrinking or spell modifiers.
    let fps = match dexterity {
        1..=7 => 6 + dexterity * (17 - 6) / 8,
        8 if authored_fps > 0 => authored_fps,
        9..=19 => 17 + dexterity * (30 - 17) / 30,
        _ => return Err("unsupported unmodified monster walking speed"),
    };
    Ok(Duration::from_millis((1000 / fps).clamp(30, 800) as u64))
}

fn build_steps(
    mut tile: (u32, u32),
    reference: (u32, u32),
    route: &[u8],
    frames: impl Fn(u8, usize) -> (usize, (i32, i32)),
) -> Result<Vec<Step>, &'static str> {
    let sector = crate::content::world::tile_sector_id(tile);
    let mut path = route.iter().copied();
    let mut direction = path.next().ok_or("empty walk route")?;
    let mut frame = 0;
    let mut offset = (0, 0);
    let mut crossed = false;
    let mut steps = Vec::new();
    let sample = |tile, (x, y): (i32, i32), direction, frame| {
        let (cx, cy) = project_world_tile(tile, reference).ok_or("walk projection overflow")?;
        Ok(Step {
            tile,
            offset: (x, y),
            direction,
            frame,
            anchor: (
                cx.checked_add(x).ok_or("walk X overflow")?,
                cy.checked_add(y).ok_or("walk Y overflow")?,
            ),
        })
    };
    steps.push(sample(tile, offset, direction, Some(frame))?);
    for _ in 0..256 {
        // object_inc_current_aid applies the incoming frame's movement.
        let previous_frame = frame;
        let (count, (dx, dy)) = frames(direction, frame + 1);
        frame = (frame + 1) % count;
        if dx.abs_diff(0) > 40
            || !(0..=20).contains(&dy)
            || (direction == 3 && dx < 0)
            || (direction == 5 && dx > 0)
        {
            return Err("walk frame offsets do not follow the supported path direction");
        }
        offset.0 += dx;
        offset.1 += dy;
        if crossed {
            // AGupdateAnimMove performs a center test on frames after normalization.
            if offset.1 >= 0
                && if direction == 3 {
                    offset.0 >= 0
                } else {
                    offset.0 <= 0
                }
            {
                crossed = false;
                let Some(next) = path.next() else {
                    steps.push(sample(tile, (0, 0), direction, None)?);
                    return Ok(steps);
                };
                if next != direction {
                    direction = next;
                    offset = (0, 0);
                    // CE rotates the ART ID captured before the increment.
                    frame = previous_frame;
                }
            }
        } else {
            // location_at(location_xy + (40,20) + offset), relative to the tile.
            let x = (offset.1 + 20 - (offset.0 >> 1)).div_euclid(40);
            let y = (offset.1 + 20 + (offset.0 >> 1)).div_euclid(40);
            if (x, y) != (0, 0) {
                if (x, y) != if direction == 3 { (0, 1) } else { (1, 0) } {
                    return Err("walk did not enter the requested neighbor");
                }
                tile = (
                    tile.0.checked_add_signed(x).ok_or("walk tile X overflow")?,
                    tile.1.checked_add_signed(y).ok_or("walk tile Y overflow")?,
                );
                if crate::content::world::tile_sector_id(tile) != sector {
                    return Err("diagnostic walk exceeds its source sector");
                }
                offset.0 -= 40 * (y - x);
                offset.1 -= 20 * (x + y);
                crossed = true;
            }
        }
        steps.push(sample(tile, offset, direction, Some(frame))?);
    }
    Err("walk failed to reach its target within the bounded step count")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rooted_walk_normalizes_tiles_without_teleporting_and_keeps_phase_through_turn() {
        let tile = (420, 814);
        let steps = build_steps(tile, tile, &ROUTE, |direction, frame| {
            let (x, y) = [(9, 4), (5, 3), (5, 3), (5, 2), (7, 4), (12, 6)][frame % 6];
            (6, (if direction == 3 { x } else { -x }, y))
        })
        .unwrap();
        assert_eq!(period(17, 8), Ok(Duration::from_millis(41)));
        assert_eq!(period(8, 8), Ok(Duration::from_millis(125)));
        assert_eq!((steps[1].tile, steps[1].anchor), (tile, (5, 3)));
        assert_eq!(
            (steps[4].tile, steps[4].offset, steps[4].anchor),
            ((420, 815), (-18, -8), (22, 12))
        );
        assert_eq!((steps[6].frame, steps[6].anchor), (Some(0), (43, 22)));
        assert_eq!(
            (steps[17].direction, steps[17].frame, steps[17].anchor),
            (5, Some(4), (120, 60))
        );
        assert_eq!(
            (
                steps[23].tile,
                steps[23].anchor,
                steps[23].frame,
                steps[23].direction
            ),
            ((421, 817), (80, 80), None, 5)
        );
        assert_eq!(steps.len(), 24);
    }

    #[test]
    fn walk_rejects_stationary_offsets_and_routes_leaving_the_cached_sector() {
        assert!(build_steps((420, 814), (420, 814), &[3], |_, _| (6, (0, 0))).is_err());
        assert!(build_steps((420, 831), (420, 831), &[3], |_, _| (6, (8, 4))).is_err());
    }
}
