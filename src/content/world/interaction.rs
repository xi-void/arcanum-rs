//! Prepared interactive objects and bounded session-local door state.

use super::*;
use crate::compositor::sprite_origin;

type Tile = (u32, u32);
type Id = (u64, usize);

pub(super) enum Action {
    Travel(Destination),
    Door(Door),
    Unsupported(String),
}

pub(super) struct Target {
    pub tile: Tile,
    pub action: Action,
}

pub(super) struct Door {
    pub id: Id,
    pub originally_open: bool,
    pub direction: u8,
    flags: u32,
    frames: [(Arc<LoadedFrame>, (i32, i32)); 4],
    period: Duration,
}

#[derive(Clone, Copy)]
struct Change {
    open: bool,
    started: Duration,
}

/// Keyed by original sector and record index; never evicted with raster data.
#[derive(Default)]
pub struct DoorStates(BTreeMap<Id, Change>);

impl DoorStates {
    pub(super) fn cell(
        &self,
        sector: &CachedSector,
        tile: Tile,
        base: collision::Cell,
    ) -> collision::Cell {
        let mut cell = base;
        for key in sector.doors_at.get(&tile).into_iter().flatten() {
            let Action::Door(door) = &sector.interactions[key].action else {
                continue;
            };
            if let Some(change) = self.0.get(&door.id) {
                cell = cell.portal_state(door.direction, door.originally_open, change.open);
            }
        }
        cell
    }

    fn change(&mut self, door: &Door, tick: Duration, hour: u8) -> Result<bool, &'static str> {
        if self
            .0
            .get(&door.id)
            .is_some_and(|change| door.delay(*change, tick).is_some())
        {
            return Err("door is moving");
        }
        let open = !self
            .0
            .get(&door.id)
            .map_or(door.originally_open, |change| change.open);
        if open && barred(door.flags, hour) {
            return Err("door is locked, jammed or held");
        }
        self.0.insert(
            door.id,
            Change {
                open,
                started: tick,
            },
        );
        Ok(open)
    }
}

fn barred(flags: u32, hour: u8) -> bool {
    // OPF_JAMMED, MAGICALLY_HELD and ALWAYS_LOCKED disallow ordinary use.
    if flags & 0x16 != 0 {
        return true;
    }
    if flags & 8 != 0 {
        return false;
    } // NEVER_LOCKED
    let day = (7..=21).contains(&hour);
    flags & 1 != 0 && (flags & 0x20 == 0 || day) && (flags & 0x40 == 0 || !day)
}

impl Door {
    fn index(&self, change: Change, tick: Duration) -> usize {
        let step = (tick.saturating_sub(change.started).as_millis() / self.period.as_millis())
            .min(2) as usize;
        if change.open { step + 1 } else { 2 - step }
    }

    fn delay(&self, change: Change, tick: Duration) -> Option<Duration> {
        let n = tick.saturating_sub(change.started).as_millis();
        let p = self.period.as_millis();
        (n < 2 * p).then(|| Duration::from_millis((p - n % p) as u64))
    }

    pub(super) fn draw<'a>(
        &'a self,
        states: &DoorStates,
        tick: Duration,
    ) -> (&'a Arc<LoadedFrame>, SpriteDraw<'a>) {
        let index = states
            .0
            .get(&self.id)
            .map_or(usize::from(self.originally_open) * 3, |change| {
                self.index(*change, tick)
            });
        let (asset, anchor) = &self.frames[index];
        (
            asset,
            SpriteDraw {
                frame: &asset.decoded.frame,
                anchor: *anchor,
            },
        )
    }
}

pub(super) fn prepare(
    vfs: &mut Vfs,
    frames: &mut FrameCache,
    room: &super::super::scenery::Room,
    sector: u64,
    reference: Tile,
) -> BTreeMap<WorldOrder, Target> {
    let mut targets = BTreeMap::new();
    for portal in &room.portals {
        if portal.art.kind != super::super::portal::Kind::Door {
            continue;
        }
        let tile = portal.placement.tile;
        let action = if portal.use_script > 0 {
            use_action(vfs, portal.use_script)
        } else if portal.unlock_script > 0 {
            Action::Unsupported(format!(
                "Unlock script {} is unsupported",
                portal.unlock_script
            ))
        } else {
            let mut build = || -> Result<_, Box<dyn std::error::Error>> {
                let art = portal.art;
                let first = match art.rotation & 6 {
                    0 | 6 => 4,
                    _ => 1,
                };
                let mut sequence = Vec::with_capacity(4);
                for index in [0, first, first + 1, first + 2] {
                    let asset = frames.load(
                        vfs,
                        &portal.artwork.source.virtual_path,
                        index,
                        art.palette,
                        art.mirror_x,
                    )?;
                    let frame = &asset.decoded.frame;
                    if frame.width > 512 || frame.height > 512 {
                        return Err("animated door exceeds the supported dimensions".into());
                    }
                    let anchor = portal
                        .placement
                        .draw_with_pivot(frame, reference, art.pivot(frame))
                        .ok_or("door pivot overflow")?
                        .anchor;
                    sequence.push((asset, anchor));
                }
                let fps = sequence[0].0.decoded.frames_per_second;
                let period = Duration::from_millis((1000 / fps.max(1)).clamp(16, 800) as u64);
                Ok(Door {
                    id: (sector, portal.record.index),
                    direction: art.rotation | 1,
                    originally_open: art.frame != 0,
                    flags: portal.appearance.portal_flags,
                    frames: sequence.try_into().expect("four door poses"),
                    period,
                })
            };
            match build() {
                Ok(door) => Action::Door(door),
                Err(error) => Action::Unsupported(error.to_string()),
            }
        };
        targets.insert((true, portal.order()), Target { tile, action });
    }
    for object in &room.scenery {
        if object.use_script <= 0 {
            continue;
        }
        targets.insert(
            (true, object.order()),
            Target {
                tile: object.tile,
                action: use_action(vfs, object.use_script),
            },
        );
    }
    targets
}

fn use_action(vfs: &mut Vfs, script: i32) -> Action {
    match super::super::jumps::use_destination(vfs, script) {
        Ok(destination) => Action::Travel(destination),
        Err(error) => Action::Unsupported(format!("Use script {script}: {error}")),
    }
}

impl SectorCache {
    pub(super) fn target(&self, key: WorldOrder) -> Option<&Target> {
        let tile = (key.1.2, key.1.1);
        self.entries
            .get(&tile_sector_id(tile))?
            .0
            .interactions
            .get(&key)
    }

    pub(super) fn player_cell(
        entries: &BTreeMap<u64, (CachedSector, Option<u64>)>,
        states: &DoorStates,
        tile: Tile,
    ) -> Option<collision::Cell> {
        let (sector, _) = entries.get(&tile_sector_id(tile))?;
        Some(states.cell(sector, tile, sector.collision.as_ref()?.cell(tile)))
    }

    pub(super) fn door_frame<'a>(
        &'a self,
        raster: &Raster,
        elapsed: Duration,
    ) -> Option<(&'a Arc<LoadedFrame>, SpriteDraw<'a>)> {
        raster.key.1.6?;
        match &self.target(raster.key)?.action {
            Action::Door(door) => Some(door.draw(&self.door_states, elapsed)),
            _ => None,
        }
    }

    pub(super) fn pick(&self, point: (i64, i64), elapsed: Duration) -> Option<WorldOrder> {
        self.ordered
            .iter()
            .rev()
            .filter(|raster| raster.key.0)
            .find_map(|raster| {
                let sprite = self.door_frame(raster, elapsed).map_or_else(
                    || raster.frame_draw(raster.asset_at(elapsed, self.fidget)),
                    |(_, sprite)| sprite,
                );
                let origin = sprite_origin(sprite.frame, sprite.anchor);
                let x = usize::try_from(point.0 - origin.0).ok()?;
                let y = usize::try_from(point.1 - origin.1).ok()?;
                let width = sprite.frame.width as usize;
                (x < width
                    && y < sprite.frame.height as usize
                    && sprite.frame.pixels[y * width + x][3] != 0)
                    .then_some(raster.key)
            })
            .filter(|&key| self.target(key).is_some())
    }

    pub(super) fn approach(&mut self, key: WorldOrder, run: bool) -> Option<usize> {
        let target = self.target(key)?;
        let tile = target.tile;
        let directions: &[u8] = match &target.action {
            Action::Door(door) => std::slice::from_ref(&door.direction),
            _ => &[1, 3, 5, 7],
        };
        let candidates: Vec<_> = std::iter::once(tile)
            .chain(
                directions
                    .iter()
                    .filter_map(|&direction| neighbor(tile, direction)),
            )
            .collect();
        let entries = &self.entries;
        let states = &self.door_states;
        let player = self.player.as_mut()?;
        player.destination_near(&candidates, run, |tile| {
            Self::player_cell(entries, states, tile)
        })
    }

    pub(super) fn use_target(
        &mut self,
        key: WorldOrder,
        tick: Duration,
    ) -> Option<Result<Option<Destination>, String>> {
        let target = self.target(key)?;
        let player = self.player.as_ref()?;
        if player.moving() {
            return None;
        }
        let tile = player.tile();
        let facing = player.facing();
        let (sector, _) = self.entries.get(&tile_sector_id(target.tile))?;
        let target = &sector.interactions[&key];
        let near = match &target.action {
            Action::Door(door) => {
                target.tile == tile || neighbor(target.tile, door.direction) == Some(tile)
            }
            _ => tile.0.abs_diff(target.tile.0) + tile.1.abs_diff(target.tile.1) <= 1,
        };
        if !near {
            return Some(Err("cannot reach the object".into()));
        }
        match &target.action {
            Action::Door(door) => {
                let result = self
                    .door_states
                    .change(door, tick, self.hour)
                    .map_err(str::to_owned);
                Some(result.map(|open| {
                    eprintln!(
                        "Door {:?} record {} at {:?} {}",
                        door.id.0,
                        door.id.1,
                        target.tile,
                        if open { "opened" } else { "closed" }
                    );
                    None
                }))
            }
            Action::Travel(dst) => {
                eprintln!(
                    "Using original transition at {:?}, facing {facing}",
                    target.tile
                );
                Some(Ok(Some(*dst)))
            }
            Action::Unsupported(reason) => Some(Err(reason.clone())),
        }
    }

    pub(super) fn door_delay(&self, viewport: &Viewport, tick: Duration) -> Option<Duration> {
        if self.door_states.0.is_empty() {
            return None;
        }
        self.active
            .iter()
            .flat_map(|id| self.entries[id].0.interactions.values())
            .filter_map(|target| {
                let Action::Door(door) = &target.action else {
                    return None;
                };
                let change = *self.door_states.0.get(&door.id)?;
                let (_, sprite) = door.draw(&self.door_states, tick);
                viewport
                    .intersects(RasterBounds::new(sprite))
                    .then_some(())?;
                door.delay(change, tick)
            })
            .min()
    }
}

fn neighbor((x, y): Tile, direction: u8) -> Option<Tile> {
    let (dx, dy) = collision::DIRECTIONS[usize::from(direction)];
    Some((x.checked_add_signed(dx)?, y.checked_add_signed(dy)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires installed original data"]
    fn original_bates_door_collision_and_map_transitions() {
        let game = std::env::var_os("ARCANUM_GAME_DIR").expect("set ARCANUM_GAME_DIR");
        let mut vfs = Vfs::from_game(&game, crate::vfs::Policy::Ce).unwrap();
        let catalog = super::super::super::maps::Catalog::load(&mut vfs).unwrap();
        let bates = catalog.find("5011").unwrap();
        let files = bates.files(&mut vfs).unwrap();
        let center = files.center(None).unwrap();
        let mut camera = files.region.camera(center).unwrap();
        camera.follow((0, 0));
        let mut world = WorldStream::new(vfs, bates.directory.clone(), files.region, center)
            .unwrap()
            .with_starting_lighting()
            .unwrap()
            .with_player()
            .unwrap();
        world.prepare(&camera, (1280, 720)).unwrap();
        let bates = &world.cache.entries[&67108865].0;
        let (tile, door) = bates
            .interactions
            .values()
            .find_map(|target| {
                let Action::Door(door) = &target.action else {
                    return None;
                };
                (door.id.1 == 244).then_some((target.tile, door))
            })
            .expect("Bates authored billiard room door");
        let edge_cell = |states: &DoorStates| {
            SectorCache::player_cell(&world.cache.entries, states, tile).unwrap()
        };
        let mut states = DoorStates::default();
        let travel = |states: &DoorStates| {
            let from = neighbor(tile, door.direction).unwrap();
            collision::step(from, (door.direction + 4) % 8, |tile| {
                SectorCache::player_cell(&world.cache.entries, states, tile)
            })
        };
        let closed = edge_cell(&states);
        assert_eq!(travel(&states), None);
        assert!(states.change(door, Duration::ZERO, 15).unwrap());
        assert!(door.draw(&states, Duration::ZERO).0.decoded.frame_count > 1);
        assert!(!Arc::ptr_eq(
            door.draw(&states, Duration::ZERO).0,
            &door.frames[0].0
        ));
        let open = edge_cell(&states);
        assert_ne!(closed, open);
        assert_eq!(travel(&states), Some(tile));
        assert!(!states.change(door, Duration::from_secs(2), 15).unwrap());
        assert_eq!(closed, edge_cell(&states));
        assert_eq!(travel(&states), None);

        let dst = world.cache.jumps[&(109, 92)];
        assert_eq!(
            dst,
            Destination {
                map: Some(5000),
                tile: (61976, 65664)
            }
        );
        assert!(bates.interactions.values().any(|target| matches!(
            target.action,
            Action::Travel(Destination {
                map: Some(5012),
                tile: (99, 91)
            })
        )));
        let mut vfs = Vfs::from_game(&game, crate::vfs::Policy::Ce).unwrap();
        let exterior = catalog.find("5000").unwrap();
        let files = exterior.files(&mut vfs).unwrap();
        let center = files.center(Some(dst.tile)).unwrap();
        let region = files.region_at(center);
        let mut camera = region.camera(center).unwrap();
        camera.follow((0, 0));
        let mut outside = WorldStream::new(vfs, exterior.directory.clone(), region, center)
            .unwrap()
            .with_starting_lighting()
            .unwrap()
            .with_player()
            .unwrap();
        outside.prepare(&camera, (1280, 720)).unwrap();
        outside.validate_arrival(dst.tile).unwrap();
        assert!(outside.stats().light_samples > 0);
        assert!(
            outside.cache.entries[&tile_sector_id(dst.tile)]
                .0
                .interactions
                .values()
                .any(|target| matches!(
                    target.action,
                    Action::Travel(Destination {
                        map: Some(5011),
                        tile: (104, 92)
                    })
                ))
        );
    }

    #[test]
    fn lock_flags_have_original_time_gate() {
        assert!(barred(1, 12));
        assert!(barred(0x10, 12));
        assert!(barred(0x21, 12));
        assert!(!barred(0x21, 22));
        assert!(barred(0x41, 22));
        assert!(!barred(8 | 1, 22));
    }
}
