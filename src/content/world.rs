//! Bounded streaming of original base sectors for the read-only world viewer.

use super::{
    art::{FrameCache, FrameCacheStats, LoadedFrame},
    scenery::{DrawOrder, RoomOverview, WorldRegion, load_streamed_sector},
};
use crate::{
    camera::{Camera, RasterBounds, Viewport},
    compositor::SpriteDraw,
    isometric::project_world_tile,
    sector::{Coordinates, SIDE},
    vfs::Vfs,
};
use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{Duration, Instant},
};

const MAX_SECTORS: usize = 12;
const OVERHANG: i64 = 640;

#[derive(Clone, Copy, Debug)]
pub struct SectorRegion {
    pub first: Coordinates,
    pub last: Coordinates,
}

impl SectorRegion {
    pub fn camera(self, reference: (u32, u32)) -> Option<Camera> {
        let (x, y) = self.first.world_tile(0, 0)?;
        let (right, bottom) = self.last.world_tile(63, 63)?;
        if !(x..=right).contains(&reference.0) || !(y..=bottom).contains(&reference.1) {
            return None;
        }
        Camera::rectangle(
            reference,
            (x, y),
            (
                right.checked_sub(x)?.checked_add(1)?,
                bottom.checked_sub(y)?.checked_add(1)?,
            ),
        )
    }

    fn needed(
        self,
        reference: (u32, u32),
        (x, y): (i32, i32),
        (width, height): (u32, u32),
    ) -> Result<Required, String> {
        // Inverse projection in units of 1/80 tile. One overhang allowance
        // on each screen axis becomes three allowances on each diagonal.
        let extent = i64::from(width.div_ceil(2)) + 2 * i64::from(height.div_ceil(2));
        let margin = extent + 3 * OVERHANG;
        let (x, y) = (i64::from(x), i64::from(y));
        let coords = |tile, axis, first, last| {
            let center = 80 * i64::from(tile) + axis;
            let sector = |q: i64| (q + 40).div_euclid(80 * SIDE as i64);
            let lower = sector(center - margin).max(i64::from(first));
            let upper = sector(center + margin).min(i64::from(last));
            lower..=upper
        };
        let cols = coords(reference.0, 2 * y - x, self.first.x, self.last.x);
        let rows = coords(reference.1, 2 * y + x, self.first.y, self.last.y);
        let len = |range: &std::ops::RangeInclusive<i64>| (range.end() - range.start() + 1).max(0);
        if len(&cols) * len(&rows) > MAX_SECTORS as i64 {
            return Err(format!(
                "viewport needs more than {MAX_SECTORS} original sectors"
            ));
        }
        let mut result = Required {
            ids: [0; MAX_SECTORS],
            len: 0,
        };
        for y in rows {
            for x in cols.clone() {
                result.ids[result.len] = (x as u64) | ((y as u64) << 26);
                result.len += 1;
            }
        }
        Ok(result)
    }
}

struct Required {
    ids: [u64; MAX_SECTORS],
    len: usize,
}

impl Required {
    fn as_slice(&self) -> &[u64] {
        &self.ids[..self.len]
    }
}

#[derive(Debug)]
pub struct SectorReport {
    pub id: u64,
    pub source: Option<String>,
    pub surfaces: usize,
    pub facades: usize,
    pub wall_count: usize,
    pub scenery_count: usize,
    pub portal_count: usize,
    pub original_object_counts: (usize, usize, usize),
    pub diagnostics: Vec<String>,
}

#[derive(Clone, Copy, Debug)]
pub struct StreamStats {
    pub sector_reads: u64,
    pub sector_hits: u64,
    pub sector_evictions: u64,
    pub cached_sectors: usize,
    pub active_sectors: usize,
    pub vfs_reads: u64,
    /// Reserved storage for sector and merged raster descriptors, excluding shared RGBA.
    pub raster_bytes: usize,
    pub art: FrameCacheStats,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct PrepareTiming {
    pub load: Duration,
    pub art_read: Duration,
    pub art_decode: Duration,
    pub ordering: Duration,
}

type WorldOrder = (bool, DrawOrder);

#[derive(Clone)]
struct Raster {
    key: WorldOrder,
    frame: Arc<LoadedFrame>,
    anchor: (i32, i32),
    bounds: RasterBounds,
}

impl Raster {
    fn new(key: WorldOrder, frame: &Arc<LoadedFrame>, anchor: (i32, i32)) -> Self {
        Self {
            key,
            frame: Arc::clone(frame),
            anchor,
            bounds: RasterBounds::new(SpriteDraw {
                frame: &frame.decoded.frame,
                anchor,
            }),
        }
    }

    fn draw(&self) -> SpriteDraw<'_> {
        SpriteDraw {
            frame: &self.frame.decoded.frame,
            anchor: self.anchor,
        }
    }
}

struct CachedSector {
    report: SectorReport,
    draws: Vec<Raster>,
}

impl CachedSector {
    fn missing(id: u64, error: String) -> Self {
        Self {
            report: SectorReport {
                id,
                source: None,
                surfaces: 0,
                facades: 0,
                wall_count: 0,
                scenery_count: 0,
                portal_count: 0,
                original_object_counts: (0, 0, 0),
                diagnostics: vec![error],
            },
            draws: Vec::new(),
        }
    }

    fn from_room(id: u64, world: RoomOverview, reference: (u32, u32)) -> Self {
        let room = &world.room;
        let mut report = SectorReport {
            id,
            source: Some(format!(
                "{} from {}",
                room.floor.source.virtual_path, room.floor.source
            )),
            surfaces: room.floor.tiles().len(),
            facades: room
                .floor
                .tiles()
                .iter()
                .filter(|tile| tile.facade.is_some())
                .count(),
            wall_count: 0,
            scenery_count: 0,
            portal_count: 0,
            original_object_counts: world.sector_object_counts,
            diagnostics: room.floor.surface_issues.clone(),
        };
        report.diagnostics.extend(world.object_issue);
        report
            .diagnostics
            .extend(world.issues.into_iter().map(|issue| {
                format!(
                    "record {} {:?} at {:?} ART {:#010x}: {}",
                    issue.record, issue.category, issue.tile, issue.art_id, issue.reason
                )
            }));
        let (dx, dy) =
            project_world_tile(room.floor.center, reference).expect("bounded sector projection");
        let shift = |(x, y): (i32, i32)| Some((x.checked_add(dx)?, y.checked_add(dy)?));
        let mut draws: Vec<_> = room
            .floor
            .shared_draws()
            .map(|((x, y), frame, anchor)| {
                Raster::new(
                    (false, (false, y, x, 0, 0, 0, None)),
                    frame,
                    shift(anchor).expect("bounded floor raster"),
                )
            })
            .collect();
        if let Some(objects) = room.shared_objects() {
            for (key, draw, frame) in objects {
                let tile =
                    project_world_tile((key.2, key.1), reference).expect("bounded object tile");
                if let Some(raster) = shift(draw.anchor)
                    .map(|anchor| Raster::new((true, key), frame, anchor))
                    .filter(|raster| within_overhang(raster.bounds, tile))
                {
                    draws.push(raster);
                } else {
                    report.diagnostics.push(format!("object at ({}, {}) record {} exceeds the {OVERHANG}px raster overhang bound", key.2, key.1, key.6.unwrap_or(key.5)));
                }
            }
        } else {
            report
                .diagnostics
                .push("static object projection overflow".into());
        }
        // The unfiltered counts remain available in the sector diagnostics.
        report.wall_count = room.walls.len();
        report.scenery_count = room.scenery.len();
        report.portal_count = room.portals.len();
        Self { report, draws }
    }
}

fn within_overhang(bounds: RasterBounds, (x, y): (i32, i32)) -> bool {
    let (x, y) = (i64::from(x), i64::from(y));
    bounds.min.0 >= x - OVERHANG
        && bounds.max.0 <= x + OVERHANG
        && bounds.min.1 >= y - OVERHANG
        && bounds.max.1 <= y + OVERHANG
}

#[derive(Default)]
struct SectorCache {
    entries: BTreeMap<u64, (CachedSector, u64)>,
    active: Vec<u64>,
    ordered: Vec<Raster>,
    step: u64,
    loads: u64,
    hits: u64,
    evictions: u64,
}

impl SectorCache {
    fn select(
        &mut self,
        required: &[u64],
        capacity: usize,
        mut load: impl FnMut(u64) -> CachedSector,
    ) -> PrepareTiming {
        if self.active == required {
            return PrepareTiming::default();
        }
        let start = Instant::now();
        self.step += 1;
        self.ordered.clear();
        for &id in required {
            if let Some((_, touched)) = self.entries.get_mut(&id) {
                *touched = self.step;
                self.hits += 1;
                continue;
            }
            if self.entries.len() == capacity {
                let (&oldest, _) = self
                    .entries
                    .iter()
                    .filter(|(id, _)| !required.contains(id))
                    .min_by_key(|(_, (_, touched))| *touched)
                    .expect("at least one inactive sector at capacity");
                self.entries.remove(&oldest);
                self.evictions += 1;
            }
            self.entries.insert(id, (load(id), self.step));
            self.loads += 1;
        }
        let load = start.elapsed();
        let start = Instant::now();
        self.active.clear();
        self.active.extend_from_slice(required);
        for id in &self.active {
            self.ordered
                .extend(self.entries[id].0.draws.iter().cloned());
        }
        self.ordered.sort_unstable_by_key(|draw| draw.key);
        PrepareTiming {
            load,
            ordering: start.elapsed(),
            ..PrepareTiming::default()
        }
    }

    fn visible(&self, camera: &Camera, size: (u32, u32)) -> impl Iterator<Item = SpriteDraw<'_>> {
        let viewport = Viewport::new(camera.position(), size);
        self.ordered
            .iter()
            .filter_map(move |draw| viewport.place(draw.draw(), draw.bounds))
    }
}

/// A selected map rectangle, one immutable set of mounts and a bounded cache.
/// Sector loading takes place in prepare, before immutable raster iteration.
pub struct WorldStream {
    map: String,
    region: SectorRegion,
    reference: (u32, u32),
    vfs: Vfs,
    frames: FrameCache,
    cache: SectorCache,
}

impl WorldStream {
    pub fn new(
        vfs: Vfs,
        map: String,
        region: SectorRegion,
        reference: (u32, u32),
    ) -> Result<Self, String> {
        if map.is_empty() || matches!(map.as_str(), "." | "..") || map.contains(['/', '\\']) {
            return Err("expected a single map directory name".into());
        }
        region
            .camera(reference)
            .ok_or("invalid world sector bounds or camera reference")?;
        Ok(Self {
            map,
            region,
            reference,
            vfs,
            frames: FrameCache::default(),
            cache: SectorCache::default(),
        })
    }

    pub fn prepare(&mut self, camera: &Camera, size: (u32, u32)) -> Result<PrepareTiming, String> {
        let required = self
            .region
            .needed(self.reference, camera.position(), size)?;
        let before = self.frames.stats();
        let mut timing = self.cache.select(required.as_slice(), MAX_SECTORS, |id| {
            let sector = Coordinates::from_id(id).expect("validated sector coordinates");
            let region = WorldRegion {
                map: &self.map,
                sector,
                origin: (0, 0),
                side: SIDE as u8,
                center: sector
                    .world_tile(0, 0)
                    .expect("validated sector coordinates"),
                viewport: size,
            };
            match load_streamed_sector(&mut self.vfs, &mut self.frames, region) {
                Ok(room) => CachedSector::from_room(id, room, self.reference),
                Err(error) => CachedSector::missing(id, error.to_string()),
            }
        });
        let after = self.frames.stats();
        timing.art_read = after.read_time - before.read_time;
        timing.art_decode = after.decode_time - before.decode_time;
        Ok(timing)
    }

    pub fn reports(&self) -> impl Iterator<Item = &SectorReport> {
        self.cache
            .active
            .iter()
            .map(|id| &self.cache.entries[id].0.report)
    }

    pub fn stats(&self) -> StreamStats {
        StreamStats {
            sector_reads: self.cache.loads,
            sector_hits: self.cache.hits,
            sector_evictions: self.cache.evictions,
            cached_sectors: self.cache.entries.len(),
            active_sectors: self.cache.active.len(),
            vfs_reads: self.vfs.read_attempts(),
            raster_bytes: (self.cache.ordered.capacity()
                + self
                    .cache
                    .entries
                    .values()
                    .map(|(sector, _)| sector.draws.capacity())
                    .sum::<usize>())
                * size_of::<Raster>(),
            art: self.frames.stats(),
        }
    }

    pub fn visible(
        &self,
        camera: &Camera,
        size: (u32, u32),
    ) -> impl Iterator<Item = SpriteDraw<'_>> {
        self.cache.visible(camera, size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sector_prefetch_covers_sources_outside_the_visible_tile_interval() {
        let region = SectorRegion {
            first: Coordinates { x: 0, y: 0 },
            last: Coordinates { x: 2, y: 0 },
        };
        // Even though the visible tile interval ends at X=61, a sprite
        // anchored at X=64 may overlap it.
        let view = (320, 200);
        let reference = (57, 32);
        let needed = region.needed(reference, (0, 0), view).unwrap();
        assert_eq!(needed.as_slice(), [0, 1]);
        let tile = project_world_tile((64, 32), reference).unwrap();
        assert!(within_overhang(
            RasterBounds {
                min: (-100, -60),
                max: (20, 90)
            },
            tile
        ));
        assert!(!within_overhang(
            RasterBounds {
                min: (-100, -600),
                max: (20, 90)
            },
            tile
        ));
    }

    #[test]
    fn eviction_is_lru_and_keeps_sectors_needed_by_the_current_view() {
        let mut cache = SectorCache::default();
        let fixture = |id| CachedSector::missing(id, "synthetic empty sector".into());
        cache.select(&[1], 2, fixture);
        cache.select(&[1, 2], 2, fixture);
        cache.select(&[2, 3], 2, fixture);
        assert!(!cache.entries.contains_key(&1));
        assert_eq!(cache.entries.len(), 2);
        assert_eq!((cache.loads, cache.hits, cache.evictions), (3, 2, 1));

        cache.select(&[2], 2, fixture);
        cache.select(&[1], 2, fixture);
        assert!(cache.entries.contains_key(&2));
        assert!(!cache.entries.contains_key(&3));
        assert_eq!((cache.loads, cache.hits, cache.evictions), (4, 3, 2));
        assert_eq!(
            cache.select(&[1], 2, |_| panic!("warm reload")).load,
            Duration::ZERO
        );
    }
}
