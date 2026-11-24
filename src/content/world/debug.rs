//! Inspection of the same records, rasters and collision cells used during play.

use super::*;
use crate::isometric::world_tile_at_pixel_signed;

type Tile = (u32, u32);

#[derive(Clone, Copy, Debug)]
pub struct DebugObject {
    sector: u64,
    key: WorldOrder,
}

#[derive(Clone, Copy, Debug)]
pub struct DebugCollision {
    pub standable: bool,
    pub terrain_blocked: bool,
    pub edges: [bool; 4],
}

impl InteractiveWorld {
    pub fn debug_collision(&self, tile: Tile) -> Option<DebugCollision> {
        let cell = SectorCache::player_cell(&self.cache.entries, &self.cache.door_states, tile)?;
        Some(DebugCollision {
            standable: collision::can_stand(Some(cell)),
            terrain_blocked: cell.terrain_blocked,
            edges: [1, 3, 5, 7].map(|direction| cell.blocks_edge(direction)),
        })
    }

    pub fn debug_source(&self, tile: Tile) -> Option<&str> {
        self.cache
            .entries
            .get(&tile_sector_id(tile))?
            .0
            .report
            .source
            .as_deref()
    }

    pub fn debug_pick(
        &self,
        screen: (u32, u32),
        size: (u32, u32),
        camera: &Camera,
        now: Duration,
    ) -> Option<DebugObject> {
        if screen.0 >= size.0 || screen.1 >= size.1 {
            return None;
        }
        let (x, y) = camera.position();
        let point = (
            i64::from(x) + i64::from(screen.0) - i64::from(size.0 / 2),
            i64::from(y) + i64::from(screen.1) - i64::from(size.1 / 2),
        );
        let key = self.cache.pick_raster(point, self.reference, now, true)?;
        Some(DebugObject {
            sector: tile_sector_id((key.1.2, key.1.1)),
            key,
        })
    }

    pub fn debug_door_at(&self, tile: Tile) -> Option<DebugObject> {
        let sector = tile_sector_id(tile);
        let (cached, _) = self.cache.entries.get(&sector)?;
        let [key] = cached.doors_at.get(&tile)?.as_slice() else {
            return None;
        };
        Some(DebugObject { sector, key: *key })
    }

    pub fn debug_inspect(&self, object: DebugObject, now: Duration) -> Result<Vec<String>, String> {
        let (sector, _) = self
            .cache
            .entries
            .get(&object.sector)
            .ok_or("Selected sector was evicted")?;
        let raster = sector
            .draws
            .iter()
            .find(|r| r.key == object.key)
            .ok_or("Selected raster is unavailable")?;
        let (_, (_, y, x, _, _, index, portal)) = object.key;
        let record = portal.unwrap_or(index);
        let tile = (x, y);
        let asset = self.cache.door_frame(raster, now).map_or_else(
            || raster.asset_at(now, self.cache.fidget),
            |(asset, _)| asset,
        );
        let mut lines = vec![
            format!("Runtime {}:{record} at {tile:?}", object.sector),
            format!(
                "Effective ART {} palette {}",
                asset.source.virtual_path, asset.decoded.stored_palette
            ),
            format!(
                "Roof/wall visibility {}, collision {:?}",
                self.cache.raster_revealed(raster),
                self.debug_collision(tile)
            ),
        ];
        if let Some((_, text)) = sector
            .report
            .object_audit
            .iter()
            .find(|(i, _)| *i == record)
        {
            lines.push(text.clone());
        }
        if let Some(mobile) = {
            let prefix = format!("mobile runtime {record}:");
            sector
                .report
                .diagnostics
                .iter()
                .find(|line| line.starts_with(&prefix))
        } {
            lines.push(mobile.clone());
        }
        let frame_index = match &raster.animation {
            Some(RasterAnimation::Loop(clip, initial)) => Some(clip.index(*initial, now)),
            Some(RasterAnimation::Fidget(poses)) => Some(
                self.cache
                    .fidget
                    .start
                    .and_then(|start| {
                        poses
                            .sequence(self.cache.fidget.direction)
                            .one_shot_index(now.saturating_sub(start))
                    })
                    .unwrap_or(0),
            ),
            None => None,
        };
        if let Some(index) = frame_index {
            lines.push(format!(
                "Current animation frame {index}, stored direction {}",
                asset.decoded.stored_rotation
            ));
        }
        if let Some(target) = sector.interactions.get(&object.key) {
            lines.push(match &target.action {
                interaction::Action::Door(door) => {
                    self.cache.door_states.describe(door, self.cache.hour)
                }
                interaction::Action::Travel(dst) => format!("Original Use transition: {dst:?}"),
                interaction::Action::Unsupported(reason) => reason.clone(),
            });
        }
        Ok(lines)
    }

    pub fn debug_inspect_tile(&self, tile: Tile) -> Result<Vec<String>, String> {
        let id = tile_sector_id(tile);
        let (sector, _) = self
            .cache
            .entries
            .get(&id)
            .ok_or("Sector is outside the loaded area")?;
        let mut lines = vec![format!(
            "Tile {tile:?}, sector {id}, collision {:?}",
            self.debug_collision(tile)
        )];
        let location = format!("at {tile:?}");
        let stored = format!("stored tile Some({tile:?})");
        lines.extend(
            sector
                .report
                .diagnostics
                .iter()
                .filter(|s| s.contains(&location) || s.contains(&stored))
                .cloned(),
        );
        let records: Vec<_> = sector
            .draws
            .iter()
            .filter(|r| {
                r.key.0
                    && (r.key.1.2, r.key.1.1) == tile
                    && !matches!(r.object_light, ObjectLight::Roof)
            })
            .collect();
        lines.extend(
            records
                .into_iter()
                .filter_map(|r| {
                    self.debug_inspect(
                        DebugObject {
                            sector: id,
                            key: r.key,
                        },
                        Duration::ZERO,
                    )
                    .ok()
                })
                .flatten(),
        );
        if sector.report.object_list_failed {
            lines.push("Original object list incomplete".into());
            lines.extend(sector.report.diagnostics.first().cloned());
        } else if lines.len() == 1 {
            lines.push("No supported objects or reported failures at this tile".into());
        }
        Ok(lines)
    }

    pub fn debug_unlock(&mut self, object: DebugObject) -> Result<bool, String> {
        let target = self
            .cache
            .entries
            .get(&object.sector)
            .and_then(|(sector, _)| sector.interactions.get(&object.key))
            .ok_or("Selected object has no supported door action")?;
        match &target.action {
            interaction::Action::Door(door) => self.cache.door_states.unlock(door),
            _ => Err("Selected object has a scripted or unsupported action".into()),
        }
    }

    pub fn debug_tiles(
        &self,
        camera: &Camera,
        size: (u32, u32),
    ) -> impl Iterator<Item = (Tile, (i32, i32))> {
        let (cx, cy) = camera.position();
        let (cx, cy) = (i64::from(cx), i64::from(cy));
        let (w, h) = (i64::from(size.0 / 2) + 40, i64::from(size.1 / 2) + 20);
        let corners = [(-w, -h), (w, -h), (-w, h), (w, h)]
            .map(|(x, y)| world_tile_at_pixel_signed((cx + x, cy + y), self.reference));
        let bound = |range: std::ops::RangeInclusive<u32>, axis: fn((i64, i64)) -> i64| {
            let first = corners.iter().flatten().map(|&tile| axis(tile)).min();
            let last = corners.iter().flatten().map(|&tile| axis(tile)).max();
            let fallback = i64::from(*range.start());
            let start = first.unwrap_or(fallback).max(fallback) as u32;
            let end = last.unwrap_or(fallback).min(i64::from(*range.end()));
            (end >= i64::from(start)).then_some(start..=end as u32)
        };
        let xs = bound(
            self.region.first.x * 64..=self.region.last.x * 64 + 63,
            |t| t.0,
        );
        let ys = bound(
            self.region.first.y * 64..=self.region.last.y * 64 + 63,
            |t| t.1,
        );
        ys.into_iter()
            .flatten()
            .flat_map(move |y| xs.clone().into_iter().flatten().map(move |x| (x, y)))
            .filter_map(move |tile| {
                let (x, y) = project_world_tile(tile, self.reference)?;
                let (x, y) = (i64::from(x) - cx, i64::from(y) - cy);
                (x.abs() <= w && y.abs() <= h).then_some((
                    tile,
                    (
                        (x + i64::from(size.0 / 2)) as i32,
                        (y + i64::from(size.1 / 2)) as i32,
                    ),
                ))
            })
    }
}
