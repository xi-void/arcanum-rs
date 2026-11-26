//! Inspection of the same records, rasters and collision cells used during play.

use super::*;
use crate::{developer::ObjectFilter, isometric::world_tile_at_pixel_signed};
use serde_json::{Value, json};

type Tile = (u32, u32);

#[derive(Clone, Copy, Debug)]
pub struct DebugObject {
    sector: u64,
    key: WorldOrder,
}

#[derive(Clone, Copy, Debug, serde::Serialize)]
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

    pub fn debug_selection_id(object: DebugObject) -> (u64, usize) {
        (object.sector, object.key.1.6.unwrap_or(object.key.1.5))
    }

    pub fn debug_select(&self, id: (u64, usize)) -> Option<DebugObject> {
        let (sector, _) = self.cache.entries.get(&id.0)?;
        sector
            .draws
            .iter()
            .filter(|raster| raster.key.0 && !matches!(raster.object_light, ObjectLight::Roof))
            .find(|r| r.key.1.6.unwrap_or(r.key.1.5) == id.1)
            .map(|r| DebugObject {
                sector: id.0,
                key: r.key,
            })
    }

    pub fn debug_sectors(&self) -> Value {
        let cached: Vec<_> = self.cache.entries.values().map(|(sector, _)| {
            let r = &sector.report;
            json!({"id":r.id,"coordinates":Coordinates::from_id(r.id),
                "active":self.cache.active.contains(&r.id),
                "source":r.source_details,"kind":r.source_details.as_ref().and_then(|source|source["path"].as_str())
                    .map(|path|if path.starts_with("terrain/") {"template"} else {"authored"}),
                "terrain_available":r.source.is_some(),"collision_available":r.collision_available,
                "surfaces":r.surfaces,"decoded_records":r.decoded_records,
                "object_preparation_bytes":r.object_preparation_bytes,
                "door_targets":sector.doors_at.values().map(Vec::len).sum::<usize>(),
                "interaction_targets":sector.interactions.len(),
                "imported":{"walls":r.wall_count,"scenery":r.scenery_count,"portals":r.portal_count},
                "resident_mobiles":sector.mobiles.len(),
                "roof_entries":r.roofs.0,"roof_rasters":r.roofs.1,
                "light_counts":r.light_counts.map(|(serialized, prepared, off)|json!({"serialized":serialized,"prepared":prepared,"switched_off":off})),
                "object_lights":r.object_lights,
                "issue_count":r.structured_issues.len(),"issues":r.structured_issues.iter().take(12).collect::<Vec<_>>()})
        }).collect();
        json!({"bounds":self.region,"cached":cached,"required":self.required,
            "speculative":self.speculative,"stats":self.stats()})
    }

    fn debug_record(
        &self,
        sector: &CachedSector,
        record: usize,
        raster: Option<&Raster>,
        camera: &Camera,
        size: (u32, u32),
        now: Duration,
    ) -> Value {
        let mobile = sector
            .mobiles
            .iter()
            .find(|entry| entry.raster == Some(record));
        let mut result = if let Some(entry) = mobile {
            entry.diagnostic()
        } else {
            sector
                .report
                .original_records
                .get(&record)
                .cloned()
                .unwrap_or_else(|| json!({"record":record}))
        };
        let id = sector.report.id;
        if mobile.is_none() {
            result["id"] = json!(format!("{id}:{record}"));
        }
        result["sector"] = json!(id);
        result["sector_source"] = sector.report.source_details.clone().unwrap_or(Value::Null);
        result["issues"] = json!(
            sector
                .report
                .structured_issues
                .iter()
                .filter(|issue| issue.record == Some(record))
                .collect::<Vec<_>>()
        );
        result["rendering"] = json!({"state":"no_raster"});
        let Some(raster) = raster else { return result };
        let key = raster.key;
        let tile = (key.1.2, key.1.1);
        let (asset, sprite) = self
            .cache
            .interaction_frame(raster, now)
            .unwrap_or_else(|| {
                let asset = raster.asset_at(now, self.cache.fidget);
                (asset, raster.frame_draw(asset))
            });
        let visible = self.cache.raster_revealed(raster);
        let intersects = Viewport::new(camera.position(), size)
            .intersects(RasterBounds::scaled(sprite, raster.scale));
        let active = self.cache.active.contains(&id);
        let frame = match &raster.animation {
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
            None => result["effective"]["authored_frame"]
                .as_u64()
                .map(|f| f as usize),
        };
        result["tile"] = json!(tile);
        result["collision"] = json!(self.debug_collision(tile));
        result["rendering"] = json!({"state": if !active {"inactive_sector"} else if !visible {"roof_or_wall_hidden"}
                else if !intersects {"outside_viewport"} else {"eligible_for_scene_submission"},
            "asset":super::super::diagnostics::source(&asset.source),"frame":frame,"stored_rotation":asset.decoded.stored_rotation,
            "palette":asset.decoded.stored_palette,"anchor":sprite.anchor,"scale":raster.scale});
        if let Some(target) = sector.interactions.get(&key) {
            result["interaction"] = match &target.action {
                interaction::Action::Npc(id) => json!({"kind":"npc", "id":id.key()}),
                interaction::Action::Door(door) => {
                    result["rendering"]["frame"] =
                        json!(door.art_frame(&self.cache.door_states, now));
                    self.cache.door_states.diagnostic(door, &self.cache)
                }
                interaction::Action::Travel(dst) => json!({"kind":"transition","destination":dst}),
                interaction::Action::Container(id) => json!({
                    "kind":"container","id":id.key(),"developer_unlock":self.container_unlocked(*id),
                }),
                interaction::Action::Pickup(id) => json!({"kind":"pickup","id":id.key()}),
                interaction::Action::Unsupported(reason) => {
                    json!({"kind":"unsupported","reason":reason})
                }
            };
        }
        result
    }

    pub fn debug_objects(
        &self,
        filter: &ObjectFilter,
        camera: &Camera,
        size: (u32, u32),
        now: Duration,
    ) -> Value {
        let mut objects = Vec::new();
        let mut truncated = false;
        let matches = |id: u64,
                       record: usize,
                       tile: Option<(u32, u32)>,
                       category: Option<&str>,
                       prototype: Option<u64>| {
            filter.record.is_none_or(|wanted| wanted == (id, record))
                && filter.tile.is_none_or(|target| {
                    tile.is_some_and(|tile| {
                        tile.0.abs_diff(target.0).max(tile.1.abs_diff(target.1)) <= filter.radius
                    })
                })
                && filter
                    .category
                    .as_deref()
                    .is_none_or(|wanted| Some(wanted) == category)
                && filter
                    .prototype
                    .is_none_or(|wanted| Some(u64::from(wanted)) == prototype)
        };
        for (sector, _) in self.cache.entries.values() {
            let id = sector.report.id;
            let rasters: BTreeMap<_, _> = sector
                .draws
                .iter()
                .filter(|r| r.key.0 && !matches!(r.object_light, ObjectLight::Roof))
                .map(|r| (r.key.1.6.unwrap_or(r.key.1.5), r))
                .collect();
            for (&record, data) in &sector.report.original_records {
                let tile = data["tile"]
                    .as_array()
                    .and_then(|t| Some((t.first()?.as_u64()? as u32, t.get(1)?.as_u64()? as u32)));
                if filter.original.is_none()
                    && matches(
                        id,
                        record,
                        tile,
                        data["category"].as_str(),
                        data["prototype"].as_u64(),
                    )
                {
                    if objects.len() == 128 {
                        truncated = true;
                        break;
                    }
                    objects.push(self.debug_record(
                        sector,
                        record,
                        rasters.get(&record).copied(),
                        camera,
                        size,
                        now,
                    ));
                }
            }
            for entry in &sector.mobiles {
                if filter.record.is_some()
                    || filter
                        .original
                        .as_ref()
                        .is_some_and(|id| *id != entry.id.key())
                    || !matches(
                        id,
                        0,
                        entry.tile,
                        if filter.category.as_deref() == Some("mobile") {
                            Some("mobile")
                        } else {
                            crate::sector::objects::category_name(entry.category)
                        },
                        Some(u64::from(entry.prototype_id)),
                    )
                {
                    continue;
                }
                if objects.len() == 128 {
                    truncated = true;
                    break;
                }
                let mut info = if let Some((record, raster)) = entry
                    .raster
                    .and_then(|record| rasters.get(&record).copied().map(|raster| (record, raster)))
                {
                    self.debug_record(sector, record, Some(raster), camera, size, now)
                } else {
                    let mut info = entry.diagnostic();
                    info["sector"] = json!(id);
                    info
                };
                if let Some(place) = self.cache.inventory.current(&self.map, entry.id) {
                    info["runtime_placement"] = place.report();
                    info["rendering"] = json!({"state":"authored_placement_replaced"});
                }
                objects.push(info);
            }
            if truncated {
                break;
            }
        }
        let issues: Vec<_> = self
            .cache
            .entries
            .values()
            .flat_map(|(sector, _)| {
                let id = sector.report.id;
                sector
                    .report
                    .structured_issues
                    .iter()
                    .filter(move |issue| {
                        filter.original.is_none()
                            && filter
                                .record
                                .is_none_or(|(s, r)| s == id && issue.record == Some(r))
                            && filter.tile.is_none_or(|target| {
                                issue.tile.is_some_and(|tile| {
                                    tile.0.abs_diff(target.0).max(tile.1.abs_diff(target.1))
                                        <= filter.radius
                                })
                            })
                            && filter
                                .category
                                .as_deref()
                                .is_none_or(|category| issue.category == Some(category))
                            && filter
                                .prototype
                                .is_none_or(|prototype| issue.prototype == Some(prototype))
                    })
                    .map(move |issue| json!({"sector":id,"issue":issue}))
            })
            .take(129)
            .collect();
        truncated |= issues.len() > 128;
        json!({"objects":objects,"issues":&issues[..issues.len().min(128)],"truncated":truncated,
            "coverage":"decoded records and available rasters in cached sectors"})
    }

    pub fn debug_unlock(&mut self, vfs: &mut Vfs, object: DebugObject) -> Result<bool, String> {
        let target = self
            .cache
            .entries
            .get(&object.sector)
            .and_then(|(sector, _)| sector.interactions.get(&object.key))
            .ok_or("Selected object has no supported lock")?;
        match &target.action {
            interaction::Action::Door(door) => self.cache.door_states.unlock(door),
            interaction::Action::Container(id) => self.unlock_container(vfs, *id),
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
