//! One local, controllable character with original directional critter ART.

use super::{
    art::{FrameCache, LoadedFrame},
    collision::DIRECTIONS,
    mobile::Pose,
};
use crate::{isometric::project_world_tile, vfs::Vfs};
use std::{collections::VecDeque, error::Error, sync::Arc, time::Duration};

pub(super) mod path;

type Tile = (u32, u32);

pub(super) struct Appearance {
    pub art: u32,
    directions: [Direction; 8],
}

pub(super) struct Player {
    pub appearance: Arc<Appearance>,
    pub speed: i32,
    pub can_run: bool,
    reference: Tile,
    tile: Tile,
    facing: u8,
    step: Option<Step>,
    phase_start: Duration,
    phase_cycles: f64,
    cycle_period: Duration,
    route: VecDeque<u8>,
    route_runs: bool,
    running: bool,
}

struct Direction {
    stand: Arc<LoadedFrame>,
    walk: Stride,
    run: Stride,
}

struct Stride {
    frames: Vec<Arc<LoadedFrame>>,
    frame_period: Duration,
    tile_period: Duration,
}

#[derive(Clone, Copy)]
struct Step {
    to: Tile,
    start: Duration,
    duration: Duration,
}

pub(super) struct Sample<'a> {
    pub tile: Tile,
    pub offset: (i32, i32),
    pub anchor: (i32, i32),
    pub asset: &'a Arc<LoadedFrame>,
}

fn tile_period(
    direction: u8,
    offsets: impl Iterator<Item = (i32, i32)>,
    frame_period: Duration,
) -> Option<Duration> {
    let (dx, dy) = DIRECTIONS[usize::from(direction)];
    let axis = (i64::from(40 * (dy - dx)), i64::from(20 * (dx + dy)));
    let (count, distance) = offsets.fold((0, 0), |(count, distance), (x, y)| {
        (
            count + 1,
            distance + axis.0 * i64::from(x) + axis.1 * i64::from(y),
        )
    });
    if distance <= 0 {
        return None;
    }
    let duration = frame_period
        .mul_f64(f64::from(count) * (axis.0 * axis.0 + axis.1 * axis.1) as f64 / distance as f64);
    ((Duration::from_millis(16)..=Duration::from_secs(2)).contains(&duration)).then_some(duration)
}

impl Stride {
    fn load(
        vfs: &mut Vfs,
        cache: &mut FrameCache,
        stand: &Pose,
        animation: u8,
    ) -> Result<[Self; 8], Box<dyn Error>> {
        let mut gait = stand.clone();
        let stem = gait
            .path
            .strip_suffix("xaa.art")
            .ok_or("invalid player standing path")?;
        // CE critter unarmed animation suffix is 'a' + the original animation ID.
        gait.path = format!("{stem}xa{}.art", char::from(b'a' + animation));
        let directions = (0..8)
            .map(|direction| {
                let pose = gait.clone().in_direction(direction);
                let first = pose.load(vfs, cache, 0)?;
                let (count, fps) = (first.decoded.frame_count, first.decoded.frames_per_second);
                if !(2..=32).contains(&count) || !(1..=100).contains(&fps) {
                    return Err("invalid player movement ART frame count or cadence".into());
                }
                let mut frames = Vec::with_capacity(count);
                frames.push(first);
                for index in 1..count {
                    frames.push(pose.load(vfs, cache, index)?);
                }
                // CE sub_437990 keeps authored cadence at normal speed (8).
                let frame_period = Duration::from_millis(1000 / fps as u64);
                let tile_period = tile_period(
                    direction,
                    frames.iter().map(|frame| frame.decoded.frame.offset),
                    frame_period,
                )
                .ok_or_else(|| {
                    format!(
                        "{} has an invalid stride in direction {direction}",
                        gait.path
                    )
                })?;
                Ok(Self {
                    frames,
                    frame_period,
                    tile_period,
                })
            })
            .collect::<Result<Vec<_>, Box<dyn Error>>>()?;
        Ok(directions
            .try_into()
            .ok()
            .expect("eight movement directions"))
    }
}

impl Direction {
    fn gait(&self, running: bool) -> &Stride {
        if running { &self.run } else { &self.walk }
    }
}

impl Appearance {
    pub(super) fn load(
        vfs: &mut Vfs,
        frames: &mut FrameCache,
        art: u32,
    ) -> Result<Self, Box<dyn Error>> {
        let stand = Pose::decode(art, None)?;
        let standing = (0..8)
            .map(|dir| stand.clone().in_direction(dir).load(vfs, frames, 0))
            .collect::<Result<Vec<_>, _>>()?;
        let walk = Stride::load(vfs, frames, &stand, 1)?; // TIG_ART_ANIM_WALK
        let run = Stride::load(vfs, frames, &stand, 6)?; // TIG_ART_ANIM_RUN
        let mut standing = standing.into_iter();
        let mut strides = walk.into_iter().zip(run);
        Ok(Self {
            art,
            directions: std::array::from_fn(|_| {
                let (walk, run) = strides.next().expect("eight movement directions");
                Direction {
                    stand: standing.next().expect("eight standing directions"),
                    walk,
                    run,
                }
            }),
        })
    }
}

impl Player {
    pub(super) fn new(appearance: Arc<Appearance>, start: Tile) -> Self {
        Self {
            appearance,
            speed: 8,
            can_run: true,
            reference: start,
            tile: start,
            facing: 4,
            step: None,
            phase_start: Duration::ZERO,
            phase_cycles: 0.0,
            cycle_period: Duration::from_millis(100),
            route: VecDeque::new(),
            route_runs: false,
            running: false,
        }
    }

    pub(super) fn facing(&self) -> u8 {
        self.facing
    }

    pub(super) fn set_facing(&mut self, facing: u8) {
        self.facing = facing;
    }

    pub(super) fn tile(&self) -> Tile {
        self.tile
    }

    pub(super) fn moving(&self) -> bool {
        self.step.is_some() || !self.route.is_empty()
    }

    #[cfg(test)]
    pub(super) fn destination(
        &mut self,
        tile: Tile,
        run: bool,
        cell: impl Fn(Tile) -> Option<super::collision::Cell>,
    ) -> Option<usize> {
        let from = self.route_origin();
        let path = path::find(from, tile, cell);
        let count = path.as_ref().map(VecDeque::len);
        self.route = path.unwrap_or_default();
        self.route_runs = run;
        count
    }

    pub(super) fn route_destination(&self) -> Option<Tile> {
        if !self.moving() {
            return None;
        }
        self.route
            .iter()
            .try_fold(self.route_origin(), |(x, y), &direction| {
                let (dx, dy) = DIRECTIONS[usize::from(direction)];
                Some((x.checked_add_signed(dx)?, y.checked_add_signed(dy)?))
            })
    }

    pub(super) fn route_origin(&self) -> Tile {
        self.step.map_or(self.tile, |step| step.to)
    }

    pub(super) fn follow_route(&mut self, route: VecDeque<u8>, run: bool) {
        self.route = route;
        self.route_runs = run;
    }

    pub(super) fn destination_near(
        &mut self,
        candidates: &[Tile],
        run: bool,
        cell: impl Fn(Tile) -> Option<super::collision::Cell>,
    ) -> Option<usize> {
        let from = self.step.map_or(self.tile, |step| step.to);
        let choice = candidates
            .iter()
            .filter_map(|&tile| path::find(from, tile, &cell))
            .min_by_key(VecDeque::len);
        let length = choice.as_ref().map(VecDeque::len);
        self.route = choice.unwrap_or_default();
        self.route_runs = run;
        length
    }

    pub(super) fn cancel_route(&mut self) {
        self.route.clear();
    }

    pub(super) fn update(&mut self, now: Duration, next: impl FnOnce(Tile, u8) -> Option<Tile>) {
        let mut start = now;
        let mut continuing = false;
        if let Some(step) = self.step {
            let end = step.start + step.duration;
            if now < end {
                return;
            }
            self.tile = step.to;
            self.step = None;
            continuing = true;
            // Absorb ordinary timer jitter without jumping through obstacles after a stall.
            start = end.max(now.saturating_sub(Duration::from_millis(16)));
        }
        if let Some(direction) = self.route.pop_front() {
            let running = self.route_runs && self.can_run;
            let gait = self.appearance.directions[usize::from(direction)].gait(running);
            // CE anim.c sub_437990 adjusts authored cadence only when speed differs from 8.
            let offset = if matches!((self.appearance.art >> 24) & 7, 1 | 2) {
                4
            } else {
                0
            };
            let (normal, low, high) = if running { (20, 8, 30) } else { (17, 6, 30) };
            let period = match self.speed {
                8 => gait.frame_period,
                speed => {
                    let fps = if speed < 8 {
                        low + speed * (normal - low) / 8
                    } else {
                        normal + speed.min(30) * (high - normal) / 30
                    };
                    Duration::from_millis(1000 / (fps + offset) as u64)
                }
            };
            if !continuing || direction != self.facing || running != self.running {
                self.phase_cycles = 0.0;
            } else {
                self.phase_cycles += start.saturating_sub(self.phase_start).as_secs_f64()
                    / self.cycle_period.as_secs_f64();
            }
            self.phase_start = start;
            self.cycle_period = period * gait.frames.len() as u32;
            self.facing = direction;
            self.running = running;
            if let Some(to) = next(self.tile, direction) {
                self.step = Some(Step {
                    to,
                    start,
                    duration: gait
                        .tile_period
                        .mul_f64(period.as_secs_f64() / gait.frame_period.as_secs_f64()),
                });
            } else {
                self.cancel_route();
            }
        }
    }

    pub(super) fn sample(&self, now: Duration) -> Sample<'_> {
        let direction = &self.appearance.directions[usize::from(self.facing)];
        let center =
            |tile| project_world_tile(tile, self.reference).expect("bounded player projection");
        let origin = center(self.tile);
        let Some(step) = self.step else {
            return Sample {
                asset: &direction.stand,
                tile: self.tile,
                offset: (0, 0),
                anchor: origin,
            };
        };
        let to = center(step.to);
        let elapsed = now.saturating_sub(step.start).min(step.duration);
        let tile = if elapsed >= step.duration / 2 {
            step.to
        } else {
            self.tile
        };
        let ratio = elapsed.as_secs_f64() / step.duration.as_secs_f64();
        let anchor = (
            origin.0 + (f64::from(to.0 - origin.0) * ratio).round() as i32,
            origin.1 + (f64::from(to.1 - origin.1) * ratio).round() as i32,
        );
        let c = center(tile);
        let gait = direction.gait(self.running);
        let phase = self.phase_cycles
            + now.saturating_sub(self.phase_start).as_secs_f64() / self.cycle_period.as_secs_f64();
        Sample {
            asset: &gait.frames
                [(phase.fract() * gait.frames.len() as f64) as usize % gait.frames.len()],
            tile,
            offset: (anchor.0 - c.0, anchor.1 - c.1),
            anchor,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authored_stride_defines_tile_duration_and_rejects_zero_or_reverse_motion() {
        let period = Duration::from_millis(100);
        assert_eq!(
            tile_period(3, [(10, 5); 4].into_iter(), period),
            Some(period * 4)
        );
        assert_eq!(tile_period(3, [(0, 0)].into_iter(), period), None);
        assert_eq!(tile_period(3, [(-10, -5)].into_iter(), period), None);
    }
}
