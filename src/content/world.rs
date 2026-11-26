//! Bounded streaming of original base sectors for the read-only world viewer.

use super::{
    LoadedMes,
    art::{FrameCache, FrameCacheStats, FrameLoop, LoadedFrame},
    collision::{self, Collision},
    lighting::{self, Ambient, LightGrid, LightImport, StaticLight, WallFace},
    load_mes,
    mobile::{self, Caster, DirectionalFidget, FidgetPlayback, MobileShadows, Walk, WalkStep},
    player::Player,
    roof,
    scenery::{
        DrawOrder, LoadStages, ObjectLight, RoomOverview, WorldRegion, load_streamed_sector,
    },
    shadow::PreparedMask,
};
use crate::{
    camera::{Camera, RasterBounds, Viewport},
    compositor::{Modulation, SpriteDraw},
    isometric::project_world_tile,
    sector::{Coordinates, SIDE},
    shadow,
    vfs::Vfs,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    ops::Range,
    sync::Arc,
    time::{Duration, Instant},
};

mod debug;
mod scan;
pub use scan::{ScanOptions, compatibility_scan};
mod interaction;
mod inventory;
pub use debug::{DebugCollision, DebugObject};
mod prefetch;
pub use super::jumps::Destination;
pub use interaction::DoorStates;
pub use inventory::{InventoryContents, InventoryEntry, InventoryReader};
pub use lighting::DynamicLightState;
pub use prefetch::{DeveloperAction, InteractiveWorld, PrefetchStats};

const MAX_SECTORS: usize = 12;
const OVERHANG: i64 = 640;

#[derive(Clone, Copy, Debug, serde::Serialize)]
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
    pub source_details: Option<serde_json::Value>,
    pub decoded_records: Option<usize>,
    pub structured_issues: Vec<scan::Issue>,
    pub original_records: BTreeMap<usize, serde_json::Value>,

    pub id: u64,
    pub source: Option<String>,
    pub collision_available: bool,
    pub surfaces: usize,
    pub facades: usize,
    pub wall_count: usize,
    pub scenery_count: usize,
    pub portal_count: usize,
    pub original_object_counts: (usize, usize, usize),
    /// When set, serialized object counts are unknown and the diagnostics explain why.
    pub object_list_failed: bool,
    pub object_raster_rejections: usize,
    pub object_preparation_bytes: usize,
    pub object_art_failures: usize,
    pub object_audit: Vec<(usize, String)>,
    pub diagnostics: Vec<String>,
    /// Total, prepared and switched-off original light records.
    pub light_counts: Option<(usize, usize, usize)>,
    pub object_lights: usize,
    /// Original non-invalid entries, prepared diagnostic draws.
    pub roofs: (usize, usize),
}

#[derive(Clone, Copy, Debug, serde::Serialize)]
pub struct StreamStats {
    pub sector_reads: u64,
    pub sector_hits: u64,
    pub sector_evictions: u64,
    pub cached_sectors: usize,
    pub active_sectors: usize,
    pub vfs_reads: u64,
    /// Reserved storage for sector and merged raster descriptors, excluding shared RGBA.
    pub raster_bytes: usize,
    /// Nonzero world-grid entries in the active static surface lighting.
    pub light_samples: usize,
    /// Entry data in the active and cached light grids, excluding tree overhead.
    pub light_grid_payload_bytes: usize,
    pub draw_lighting_bytes: usize,
    pub active_dynamic_lights: usize,
    pub art: FrameCacheStats,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct PrepareTiming {
    pub load: Duration,
    pub stages: LoadStages,
    pub art_resolution: Duration,
    pub art_read: Duration,
    pub art_decode: Duration,
    pub ordering: Duration,
    pub shading: Duration,
}

#[derive(Debug, Default)]
pub struct LightUpdate {
    pub grid: Duration,
    pub shading: Duration,
    pub receivers: usize,
    pub shadows: Duration,
    pub shadow_changed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WalkStatus {
    pub step: usize,
    pub tile: (u32, u32),
    pub offset: (i32, i32),
    pub anchor: (i32, i32),
    pub direction: u8,
    pub frame: Option<usize>,
}

struct DynamicLight {
    mask: StaticLight,
    state: DynamicLightState,
    nocturnal: bool,
    shadow_excluded: bool,
}

pub(super) fn tile_sector_id((x, y): (u32, u32)) -> u64 {
    u64::from(x >> 6) | (u64::from(y >> 6) << 26)
}

fn overlaps(a: RasterBounds, b: RasterBounds) -> bool {
    a.min.0 < b.max.0 && b.min.0 < a.max.0 && a.min.1 < b.max.1 && b.min.1 < a.max.1
}

/// One ordered, clipped draw with its shared decoded texture identity.
pub struct WorldDraw<'a> {
    pub asset: &'a Arc<LoadedFrame>,
    pub sprite: SpriteDraw<'a>,
    pub modulation: Modulation<'a>,
}

type WorldOrder = (bool, DrawOrder);

fn roof_key((x, y): (u32, u32)) -> WorldOrder {
    (true, (true, u32::MAX, y, i64::from(x), 0, 0, None))
}

#[derive(Clone)]
enum RasterAnimation {
    Loop(Arc<FrameLoop>, usize),
    Fidget(Arc<DirectionalFidget>),
}

#[derive(Clone)]
struct Raster {
    key: WorldOrder,
    frame: Arc<LoadedFrame>,
    anchor: (i32, i32),
    bounds: RasterBounds,
    object_light: ObjectLight,
    animation: Option<RasterAnimation>,
    click_through: bool,
}

pub(super) const OBJECT_RENDER_DESCRIPTOR_BYTES: usize =
    2 * size_of::<Raster>() + size_of::<PreparedShade>();

impl Raster {
    fn new(key: WorldOrder, frame: &Arc<LoadedFrame>, anchor: (i32, i32)) -> Self {
        Self {
            key,
            frame: Arc::clone(frame),
            anchor,
            object_light: ObjectLight::Unlit,
            animation: None,
            click_through: false,
            bounds: RasterBounds::new(SpriteDraw {
                frame: &frame.decoded.frame,
                anchor,
            }),
        }
    }

    fn include_frame(&mut self, asset: &LoadedFrame) {
        let frame = RasterBounds::new(self.frame_draw(asset));
        self.bounds.min.0 = self.bounds.min.0.min(frame.min.0);
        self.bounds.min.1 = self.bounds.min.1.min(frame.min.1);
        self.bounds.max.0 = self.bounds.max.0.max(frame.max.0);
        self.bounds.max.1 = self.bounds.max.1.max(frame.max.1);
    }

    fn set_animation(&mut self, clip: &Arc<FrameLoop>, initial: usize) {
        // The sector overhang must cover all potential frames.
        for frame in &clip.frames {
            self.include_frame(frame);
        }
        self.animation = Some(RasterAnimation::Loop(Arc::clone(clip), initial));
    }

    fn set_fidget(&mut self, poses: Arc<DirectionalFidget>) {
        for frame in poses.frames() {
            self.include_frame(frame);
        }
        self.animation = Some(RasterAnimation::Fidget(poses));
    }

    fn asset_at(&self, elapsed: Duration, playback: FidgetPlayback) -> &Arc<LoadedFrame> {
        match &self.animation {
            Some(RasterAnimation::Loop(clip, initial)) => {
                &clip.frames[clip.index(*initial, elapsed)]
            }
            Some(RasterAnimation::Fidget(poses)) => {
                let clip = poses.sequence(playback.direction);
                let index = playback
                    .start
                    .and_then(|start| clip.one_shot_index(elapsed.saturating_sub(start)))
                    .unwrap_or(0);
                &clip.frames[index]
            }
            None => &self.frame,
        }
    }

    fn frame_draw<'a>(&self, frame: &'a LoadedFrame) -> SpriteDraw<'a> {
        SpriteDraw {
            frame: &frame.decoded.frame,
            anchor: self.anchor,
        }
    }

    fn visible_at(
        &self,
        viewport: &Viewport,
        elapsed: Duration,
        playback: FidgetPlayback,
    ) -> Option<(&Arc<LoadedFrame>, SpriteDraw<'_>)> {
        let (asset, bounds) = if self.animation.is_some() {
            if !viewport.intersects(self.bounds) {
                return None;
            }
            let asset = self.asset_at(elapsed, playback);
            (asset, RasterBounds::new(self.frame_draw(asset)))
        } else {
            (&self.frame, self.bounds)
        };
        Some((asset, viewport.place(self.frame_draw(asset), bounds)?))
    }

    fn next_change(
        &self,
        viewport: &Viewport,
        elapsed: Duration,
        playback: FidgetPlayback,
    ) -> Option<Duration> {
        if !viewport.intersects(self.bounds) {
            return None;
        }
        let (clip, current, relative, wait, remaining) = match self.animation.as_ref()? {
            RasterAnimation::Loop(clip, initial) => (
                &**clip,
                clip.index(*initial, elapsed),
                elapsed,
                Duration::ZERO,
                clip.frames.len() - 1,
            ),
            RasterAnimation::Fidget(poses) => {
                let start = playback.start?;
                let clip = poses.sequence(playback.direction);
                let relative = elapsed.saturating_sub(start);
                let current = clip.one_shot_index(relative)?;
                // The final boundary restores the standing frame.
                (
                    clip,
                    current,
                    relative,
                    start.saturating_sub(elapsed),
                    clip.frames.len() - current,
                )
            }
        };
        let visible = |index: usize| {
            viewport.intersects(RasterBounds::new(self.frame_draw(&clip.frames[index])))
        };
        if visible(current) {
            return Some(wait.saturating_add(clip.delay(relative, 1)));
        }
        (1..=remaining)
            .find(|steps| visible((current + steps) % clip.frames.len()))
            .map(|steps| wait.saturating_add(clip.delay(relative, steps)))
    }
}

struct CachedSector {
    mobiles: Vec<mobile::population::Resident>,
    townmap: u32,
    report: SectorReport,
    draws: Vec<Raster>,
    lights: LightGrid,
    emitters: Vec<StaticLight>,
    outdoor_rows: [u64; 64],
    casters: Vec<Caster>,
    mobile_shadows: Option<MobileShadows>,
    walk: Option<(WorldOrder, Arc<Walk>)>,
    collision: Option<Collision>,
    interactions: BTreeMap<WorldOrder, interaction::Target>,
    doors_at: BTreeMap<(u32, u32), Vec<WorldOrder>>,
    roofs: BTreeMap<(u32, u32), roof::Cell>,
    roof_shades: Option<[u8; 3]>,
    wall_tiles: BTreeMap<(u32, u32), Vec<roof::wallcheck::Wall<WorldOrder>>>,
}

impl CachedSector {
    fn retint_lights(&mut self, ambient: Ambient) {
        let mut changed = false;
        for light in &mut self.emitters {
            changed |= light.retint(ambient);
        }
        if changed {
            self.lights.clear();
            for light in &self.emitters {
                self.lights.insert_light(light);
            }
        }
    }

    fn missing(id: u64, error: String) -> Self {
        Self {
            townmap: 0,
            mobiles: Vec::new(),
            report: SectorReport {
                id,
                source_details: None,
                decoded_records: None,
                structured_issues: vec![scan::Issue::sector_failure(&error)],
                original_records: BTreeMap::new(),
                source: None,
                collision_available: false,
                surfaces: 0,
                facades: 0,
                wall_count: 0,
                scenery_count: 0,
                portal_count: 0,
                original_object_counts: (0, 0, 0),
                object_list_failed: true,
                object_raster_rejections: 0,
                object_preparation_bytes: 0,
                object_art_failures: 0,
                object_audit: Vec::new(),
                diagnostics: vec![error],
                light_counts: None,
                object_lights: 0,
                roofs: (0, 0),
            },
            draws: Vec::new(),
            lights: LightGrid::default(),
            emitters: Vec::new(),
            outdoor_rows: [0; 64],
            casters: Vec::new(),
            mobile_shadows: None,
            walk: None,
            collision: None,
            interactions: BTreeMap::new(),
            doors_at: BTreeMap::new(),
            roofs: BTreeMap::new(),
            roof_shades: None,
            wall_tiles: BTreeMap::new(),
        }
    }

    fn from_room(
        id: u64,
        world: RoomOverview,
        reference: (u32, u32),
        lights: Option<LightImport>,
        audit: bool,
    ) -> Self {
        let room = &world.room;
        let mut report = SectorReport {
            id,
            source_details: Some(super::diagnostics::source(&room.floor.source)),
            decoded_records: world.decoded_records,
            structured_issues: Vec::new(),
            original_records: world
                .original_records
                .into_iter()
                .map(|record| {
                    (
                        record["record"].as_u64().expect("record index") as usize,
                        record,
                    )
                })
                .collect(),
            source: Some(format!(
                "{} from {}",
                room.floor.source.virtual_path, room.floor.source
            )),
            collision_available: world.collision.is_some(),
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
            object_list_failed: world.object_list_failed,
            object_raster_rejections: 0,
            object_preparation_bytes: world.object_preparation_bytes,
            object_art_failures: world
                .issues
                .iter()
                .filter(|issue| issue.art_failure)
                .count(),
            object_audit: Vec::new(),
            diagnostics: room.floor.surface_issues.clone(),
            light_counts: lights
                .as_ref()
                .map(|lights| (room.floor.lights.len(), lights.accepted, lights.off)),
            object_lights: lights.as_ref().map_or(0, |lights| lights.object_lights),
            roofs: (world.roof_words.len(), 0),
        };
        if let Some(error) = world.object_issue {
            let issue = match &world.object_rejection {
                Some((code, evidence)) => scan::Issue::list_rejection(&error, code, evidence),
                None => scan::Issue::new("object_placement_incomplete", &error),
            };
            report.structured_issues.push(issue);
            report.diagnostics.insert(0, error);
        }
        report.structured_issues.extend(
            room.floor
                .surface_issues
                .iter()
                .map(|issue| scan::Issue::new("surface_unavailable", issue)),
        );
        report
            .structured_issues
            .extend(world.issues.iter().map(scan::Issue::scene));
        report.structured_issues.extend(
            world
                .traps
                .iter()
                .map(|record| scan::Issue::trap(record.index, record.prototype.0)),
        );

        report
            .diagnostics
            .extend(world.issues.into_iter().map(|issue| {
                format!(
                    "record {} {:?} at {:?} ART {:#010x}: {}{}",
                    issue.record,
                    issue.category,
                    issue.tile,
                    issue.art_id,
                    if issue.art_failure {
                        "ART resolution: "
                    } else {
                        ""
                    },
                    issue.reason
                )
            }));
        report.diagnostics.extend(world.traps.iter().map(|trap| {
            format!(
                "record {} Trap, prototype {}, stored tile {:?}: parsed, trap detection and gameplay are unsupported",
                trap.index, trap.prototype.0, trap.stored_tile
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
        let audit_sources = audit.then(|| room.audit_sources());
        let click_through: BTreeSet<_> = room
            .scenery
            .iter()
            .filter(|object| object.appearance.flags & 0x800 != 0)
            .map(|object| object.record.index)
            .collect();
        if let Some(objects) = room.shared_objects() {
            for (key, draw, frame, light, animation) in objects {
                let tile =
                    project_world_tile((key.2, key.1), reference).expect("bounded object tile");
                if let Some(mut raster) = shift(draw.anchor)
                    .map(|anchor| {
                        let mut raster = Raster::new((true, key), frame, anchor);
                        if let Some((clip, initial)) = animation {
                            raster.set_animation(clip, initial);
                        }
                        raster
                    })
                    .filter(|raster| within_overhang(raster.bounds, tile))
                {
                    raster.object_light = light;
                    raster.click_through = key.6.is_none() && click_through.contains(&key.5);
                    if let Some(sources) = &audit_sources {
                        let source = &sources[&key.6.unwrap_or(key.5)];
                        let record = source.record;
                        let frame_info = &frame.decoded;
                        let (x, y) =
                            crate::compositor::sprite_origin(&frame_info.frame, raster.anchor);
                        let pivot = (
                            i64::from(tile.0) + i64::from(source.offset.0) - x,
                            i64::from(tile.1) + i64::from(source.offset.1) - y,
                        );
                        let details = serde_json::json!({
                            "prototype_source": super::diagnostics::source(source.prototype),
                            "asset_source": super::diagnostics::source(&frame.source),
                            "effective_art":record.art_id,"direction":source.rotation,"authored_frame":source.frame,
                            "palette":frame_info.stored_palette,"offset":source.offset,"pivot":pivot,
                            "flags":source.flags,"use_script":source.use_script,
                        });
                        if let Some(original) = report.original_records.get_mut(&record.index) {
                            original["effective"] = details;
                        }
                        report.object_audit.push((record.index, format!(
                            "record {} {:?} bytes {:?}, identity {:02x?}, prototype {} from {}, tile {:?} (stored {:?}), ART {:#010x} (stored {:x?}), {} from {}, direction {} (stored {}), frame {}, palette {}, offset {:?}, pivot {pivot:?}, order {key:?}; effective object flags {:#010x}, Use script {:?}; instance overrides {:?}",
                            record.index, record.category, record.span, record.instance_id,
                            record.prototype.0, source.prototype, (key.2, key.1), record.stored_tile,
                            record.art_id, source.stored_art, frame.source.virtual_path, frame.source,
                            source.rotation, frame_info.stored_rotation, source.frame,
                            frame_info.stored_palette, source.offset, source.flags, source.use_script, record.fields,
                        )));
                    }
                    draws.push(raster);
                } else {
                    report.object_raster_rejections += 1;
                    report.structured_issues.push(scan::Issue::record(
                        "raster_overhang",
                        key.6.unwrap_or(key.5),
                        (key.2, key.1),
                        "Raster exceeds supported overhang",
                    ));
                    report.diagnostics.push(format!("object at ({}, {}) record {} exceeds the {OVERHANG}px raster overhang bound", key.2, key.1, key.6.unwrap_or(key.5)));
                }
            }
        } else {
            report.object_raster_rejections =
                room.walls.len() + room.portals.len() + room.scenery.len();
            report
                .diagnostics
                .push("static object projection overflow".into());
        }
        // The unfiltered counts remain available in the sector diagnostics.
        report.wall_count = room.walls.len();
        report.scenery_count = room.scenery.len();
        report.portal_count = room.portals.len();
        let lights = lights.unwrap_or_default();
        report.diagnostics.extend(lights.issues);
        Self {
            townmap: world.townmap,
            mobiles: Vec::new(),
            report,
            draws,
            lights: lights.grid,
            emitters: lights.sources,
            outdoor_rows: room.floor.outdoor_rows,
            casters: Vec::new(),
            mobile_shadows: None,
            walk: None,
            collision: world.collision,
            interactions: BTreeMap::new(),
            doors_at: BTreeMap::new(),
            roofs: BTreeMap::new(),
            roof_shades: None,
            wall_tiles: BTreeMap::new(),
        }
    }

    fn add_mobiles(
        &mut self,
        mobiles: mobile::PreparedMobiles,
        reference: (u32, u32),
        playing: bool,
    ) {
        self.mobile_shadows = mobiles.shadows;
        self.report.diagnostics.extend(mobiles.issues);
        self.mobiles = mobiles.objects;
        for entry in &mut self.mobiles {
            if entry.blocks
                && let (Some(tile), Some(collision)) = (entry.tile, &mut self.collision)
            {
                collision.occupy(tile);
            } else {
                entry.blocks = false;
            }
        }
        for mobile in mobiles.frames {
            let caster = mobile.caster;
            let index = mobile.object_index;
            let record = mobile_draw_slot(index);
            let key = super::scenery::order(caster.tile, mobile.offset, mobile.flat, record);
            let mut raster = Raster::new((true, key), &mobile.asset, caster.anchor);
            if let Some(fidget) = mobile.fidget {
                raster.set_fidget(fidget);
            }
            let tile =
                project_world_tile(caster.tile, reference).expect("prepared mobile position");
            if !within_overhang(raster.bounds, tile) {
                self.mobiles[index].render_issue = Some("mobile exceeds sector overhang".into());
                self.report.diagnostics.push(format!(
                    "{}; mobile exceeds sector overhang",
                    mobile.diagnostic
                ));
                continue;
            }
            self.report
                .diagnostics
                .push(format!("mobile runtime {}: {}", key.5, mobile.diagnostic));
            raster.object_light = ObjectLight::Point;
            let walk = mobile.walk.filter(|walk| {
                let fits = walk.steps.iter().all(|&step| {
                    let asset = walk.asset(step).unwrap_or(&mobile.asset);
                    let bounds = if step.frame.is_some() {
                        RasterBounds::new(SpriteDraw {
                            frame: &asset.decoded.frame,
                            anchor: step.anchor,
                        })
                    } else {
                        let (dx, dy) = (
                            i64::from(step.anchor.0) - i64::from(caster.anchor.0),
                            i64::from(step.anchor.1) - i64::from(caster.anchor.1),
                        );
                        RasterBounds {
                            min: (raster.bounds.min.0 + dx, raster.bounds.min.1 + dy),
                            max: (raster.bounds.max.0 + dx, raster.bounds.max.1 + dy),
                        }
                    };
                    within_overhang(bounds, tile)
                });
                if !fits {
                    self.report
                        .diagnostics
                        .push("walk raster exceeds the sector overhang bound".into());
                }
                fits
            });
            if let Some(walk) = walk {
                self.walk = Some((raster.key, walk));
            }
            self.mobiles[index].raster = Some(record);
            let entry = &self.mobiles[index];
            let action = match entry.category {
                2 => Some(interaction::Action::Container(entry.id)),
                5..=14 => Some(interaction::Action::Pickup(entry.id)),
                _ => None,
            };
            if playing
                && entry.id.kind == 2
                && let Some(action) = action
            {
                self.interactions.insert(
                    raster.key,
                    interaction::Target {
                        tile: caster.tile,
                        action,
                    },
                );
            }
            self.draws.push(raster);
            if mobile.casts_shadow {
                self.casters.push(caster);
            }
        }
    }
}

fn within_overhang(bounds: RasterBounds, (x, y): (i32, i32)) -> bool {
    let (x, y) = (i64::from(x), i64::from(y));
    bounds.min.0 >= x - OVERHANG
        && bounds.max.0 <= x + OVERHANG
        && bounds.min.1 >= y - OVERHANG
        && bounds.max.1 <= y + OVERHANG
}

fn light_receiver(raster: &Raster, reference: (u32, u32)) -> Option<RasterBounds> {
    let point = |x: i32, y: i32| RasterBounds {
        min: (i64::from(x), i64::from(y)),
        max: (i64::from(x) + 1, i64::from(y) + 1),
    };
    let (x, y) = raster.anchor;
    if !raster.key.0 {
        return Some(RasterBounds {
            min: (raster.bounds.min.0 - 1, raster.bounds.min.1),
            max: (raster.bounds.max.0 + 2, raster.bounds.max.1 + 1),
        });
    }
    let tile = (raster.key.1.2, raster.key.1.1);
    match raster.object_light {
        ObjectLight::Point => Some(point(x, y)),
        ObjectLight::Portal(rotation) => {
            let (tile, dx) = lighting::portal_sample(tile, rotation);
            let (x, y) = project_world_tile(tile, reference)?;
            Some(point(x.checked_add(dx)?, y.checked_sub(10)?))
        }
        ObjectLight::Wall(rotation) => Some(WallFace::from_rotation(rotation).receiver(
            project_world_tile(tile, reference)?,
            raster.bounds.min.0,
            raster.frame.decoded.frame.width as usize,
        )),
        ObjectLight::Unlit | ObjectLight::Additive | ObjectLight::Roof => None,
    }
}

enum PreparedShade {
    Tint(Modulation<'static>),
    Columns(Box<[[u8; 3]]>),
}

impl PreparedShade {
    fn modulation(&self) -> Modulation<'_> {
        match self {
            Self::Tint(shade) => *shade,
            Self::Columns(colors) => Modulation::Columns(colors),
        }
    }

    fn uniform(color: [u8; 3]) -> Self {
        Self::Tint(if color == [255; 3] {
            Modulation::Unlit
        } else {
            Modulation::Uniform(color)
        })
    }
}

#[derive(Default)]
struct SectorCache {
    entries: BTreeMap<u64, (CachedSector, Option<u64>)>,
    active: Vec<u64>,
    ordered: Vec<Raster>,
    animated: Vec<usize>,
    step: u64,
    loads: u64,
    hits: u64,
    evictions: u64,
    ambient: Option<(Ambient, (u32, u32))>,
    light_schedule: Option<[Ambient; 24]>,
    lights: LightGrid,
    shades: Vec<PreparedShade>,
    dynamic: Vec<DynamicLight>,
    dynamic_grid: LightGrid,
    hour: u8,
    shadows_disabled: bool,
    shadows: Vec<PreparedMask>,
    shadow_error: Option<shadow::Error>,
    fidget: FidgetPlayback,
    walk_start: Option<Duration>,
    walk_subject: Option<(usize, Arc<Walk>)>,
    walk_facing: Option<u8>,
    walking: Option<MovingDraw>,
    player: Option<Player>,
    collision_disabled: bool,
    player_shadows: Option<MobileShadows>,
    jumps: super::jumps::Jumps,
    door_states: DoorStates,
    inventory: inventory::Session,
    dropped_targets: BTreeMap<WorldOrder, interaction::Target>,
    inventory_context: Option<(String, (u32, u32))>,
    roof_state: Option<roof::State>,
    hover: Option<WorldOrder>,
    faded_roofs: BTreeSet<(u32, u32)>,
    faded_walls: BTreeMap<WorldOrder, u8>,
}

struct MovingDraw {
    raster: Raster,
    origin: Caster,
    step: usize,
    insertion: usize,
    shade: PreparedShade,
    standing_direction: Option<u8>,
    standing_since: Duration,
}

impl MovingDraw {
    fn playback(&self, fidget: FidgetPlayback) -> FidgetPlayback {
        FidgetPlayback {
            direction: self.standing_direction,
            start: fidget.start.filter(|start| *start >= self.standing_since),
        }
    }
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
                *touched = Some(self.step);
                self.hits += 1;
                continue;
            }
            if self.entries.len() == capacity {
                self.evict(capacity, required, false);
            }
            self.entries.insert(id, (load(id), Some(self.step)));
            self.loads += 1;
        }
        let load = start.elapsed();
        let start = Instant::now();
        let shading = self.activate(required);
        PrepareTiming {
            load,
            ordering: start.elapsed().saturating_sub(shading),
            shading,
            ..PrepareTiming::default()
        }
    }

    fn evict(&mut self, capacity: usize, required: &[u64], keep_active: bool) -> bool {
        if self.entries.len() < capacity {
            return true;
        }
        let victim = self
            .entries
            .iter()
            .filter(|(id, _)| !required.contains(id) && (!keep_active || !self.active.contains(id)))
            .min_by_key(|(_, (_, touched))| *touched)
            .map(|(&id, _)| id);
        if let Some(victim) = victim {
            self.entries.remove(&victim);
            self.evictions += 1;
        }
        victim.is_some()
    }

    fn activate(&mut self, required: &[u64]) -> Duration {
        self.step += 1;
        self.ordered.clear();
        self.walk_subject = None;
        self.walking = None;
        self.active.clear();
        self.active.extend_from_slice(required);
        for id in required {
            let (sector, touched) = self.entries.get_mut(id).expect("activate prepared sector");
            *touched = Some(self.step);
            self.ordered.extend(sector.draws.iter().cloned());
        }
        self.project_inventory();
        self.ordered.sort_unstable_by_key(|draw| draw.key);
        self.walk_subject = required.iter().find_map(|id| {
            let (key, walk) = self.entries[id].0.walk.as_ref()?;
            let index = self
                .ordered
                .binary_search_by_key(key, |raster| raster.key)
                .ok()?;
            Some((index, Arc::clone(walk)))
        });
        self.animated.clear();
        self.animated.extend(
            self.ordered
                .iter()
                .enumerate()
                .filter_map(|(index, raster)| raster.animation.is_some().then_some(index)),
        );
        self.refresh_roofs();
        if self.ambient.is_none() {
            return Duration::ZERO;
        }
        let start = Instant::now();
        self.lights.clear();
        for id in self.active.iter().rev() {
            self.lights.merge(&self.entries[id].0.lights);
        }
        self.refresh_dynamic_grid();
        self.refresh_shades(None);
        self.refresh_shadows();
        start.elapsed()
    }

    fn set_game_hour(&mut self, hour: u8) -> bool {
        let Some((ambient, reference)) = self.ambient else {
            return false;
        };
        let Some(next) = self
            .light_schedule
            .and_then(|hours| hours.get(usize::from(hour)).copied())
        else {
            return false;
        };
        if self.hour == hour {
            return false;
        }
        let nocturnal_changed = self.dynamic.iter().any(|light| {
            light.nocturnal
                && light.state.active
                && lighting::nocturnal_active(self.hour, self.outdoor(light.state.tile))
                    != lighting::nocturnal_active(hour, self.outdoor(light.state.tile))
        });
        self.hour = hour;
        if ambient != next {
            self.ambient = Some((next, reference));
            for (sector, _) in self.entries.values_mut() {
                sector.retint_lights(next);
            }
            self.lights.clear();
            for id in self.active.iter().rev() {
                self.lights.merge(&self.entries[id].0.lights);
            }
        }
        if ambient == next && !nocturnal_changed {
            return false;
        }
        self.refresh_dynamic_grid();
        self.refresh_shades(None);
        self.refresh_shadows();
        true
    }

    fn outdoor(&self, tile: (u32, u32)) -> bool {
        let id = tile_sector_id(tile);
        self.active.contains(&id)
            && self.entries[&id].0.outdoor_rows[(tile.1 & 63) as usize] & (1 << (tile.0 & 63)) != 0
    }

    fn refresh_roofs(&mut self) {
        if let (Some(state), Some(player)) = (&mut self.roof_state, &self.player) {
            state.recalc(player.tile(), |tile| {
                let (sector, _) = self.entries.get(&tile_sector_id(tile))?;
                Some(sector.roofs.get(&tile).copied())
            });
            self.faded_roofs.clear();
            self.faded_walls.clear();
            let selected = roof::wallcheck::select(player.tile(), |tile| {
                self.entries
                    .get(&tile_sector_id(tile))
                    .and_then(|(sector, _)| sector.wall_tiles.get(&tile))
                    .map_or(&[], Vec::as_slice)
            });
            for (key, (tile, side)) in selected {
                self.faded_roofs.insert(roof::cell_center(tile));
                self.faded_walls.insert(key, side);
            }
        }
    }

    fn roof_faded_at(&self, tile: (u32, u32)) -> bool {
        let tile = roof::cell_center(tile);
        let Some(cell) = self
            .entries
            .get(&tile_sector_id(tile))
            .and_then(|(sector, _)| sector.roofs.get(&tile))
        else {
            return false;
        };
        (self.faded_roofs.contains(&tile) || cell.authored_fade())
            && self
                .roof_state
                .as_ref()
                .is_none_or(|state| !state.filled(tile, *cell))
    }

    fn roof_covered(&self, tile: (u32, u32)) -> bool {
        self.roof_state.as_ref().is_some_and(|state| {
            !self.outdoor(tile)
                && state.covered(tile, |center| {
                    self.entries
                        .get(&tile_sector_id(center))?
                        .0
                        .roofs
                        .get(&center)
                        .copied()
                })
        })
    }

    fn raster_revealed(&self, raster: &Raster) -> bool {
        let Some(state) = &self.roof_state else {
            return true;
        };
        if matches!(raster.object_light, ObjectLight::Roof) {
            let tile = (raster.key.1.3 as u32, raster.key.1.2);
            return self
                .entries
                .get(&tile_sector_id(tile))
                .and_then(|(sector, _)| sector.roofs.get(&tile))
                .is_some_and(|&cell| !state.filled(tile, cell));
        }
        let tile = (raster.key.1.2, raster.key.1.1);
        !raster.key.0
            || (!self.roof_covered(tile)
                && !(matches!(raster.object_light, ObjectLight::Wall(0 | 1 | 6 | 7))
                    && self.faded_walls.contains_key(&raster.key)
                    && self.roof_faded_at(tile)))
    }

    fn fade(&self, raster: &Raster) -> Option<[u8; 4]> {
        if matches!(raster.object_light, ObjectLight::Roof) {
            let tile = (raster.key.1.3 as u32, raster.key.1.2);
            let sector = &self.entries.get(&tile_sector_id(tile))?.0;
            return sector
                .roofs
                .get(&tile)?
                .opacity(self.faded_roofs.contains(&tile), sector.roof_shades?);
        }
        Some(match self.faded_walls.get(&raster.key)? {
            1 => [128, 255, 255, 128],
            2 => [255, 128, 128, 255],
            _ => [64; 4],
        })
    }

    fn light_active(&self, source: &DynamicLight) -> bool {
        source.state.active
            && self.active.contains(&tile_sector_id(source.state.tile))
            && self.entries[&tile_sector_id(source.state.tile)]
                .0
                .report
                .source
                .is_some()
            && (!source.nocturnal
                || lighting::nocturnal_active(self.hour, self.outdoor(source.state.tile)))
    }

    fn refresh_dynamic_grid(&mut self) {
        // The dynamic grid contains only the footprints of the bounded probes.
        let mut grid = LightGrid::default();
        for source in self.dynamic.iter().filter(|light| self.light_active(light)) {
            grid.insert_light(&source.mask);
        }
        self.dynamic_grid = grid;
    }

    fn set_dynamic(
        &mut self,
        slot: usize,
        state: DynamicLightState,
    ) -> Result<LightUpdate, &'static str> {
        let (_, reference) = self.ambient.ok_or("enable world lighting first")?;
        let source = self.dynamic.get(slot).ok_or("unknown dynamic light slot")?;
        if state == source.state {
            return Ok(LightUpdate::default());
        }
        let old = self.light_active(source).then(|| source.mask.bounds());
        let source = &mut self.dynamic[slot];
        source.mask.place(state, reference)?;
        source.state = state;
        let source = &self.dynamic[slot];
        let new = self.light_active(source).then(|| source.mask.bounds());
        if old.is_none() && new.is_none() {
            return Ok(LightUpdate::default());
        }
        let start = Instant::now();
        self.refresh_dynamic_grid();
        let grid = start.elapsed();
        let start = Instant::now();
        let receivers = self.refresh_shades(Some([old, new]));
        let shading = start.elapsed();
        let start = Instant::now();
        let shadow_changed = self.refresh_shadows();
        Ok(LightUpdate {
            grid,
            shading,
            receivers,
            shadows: start.elapsed(),
            shadow_changed,
        })
    }

    fn start_walk(&mut self, elapsed: Duration) -> bool {
        if self.walk_subject.is_none() || self.walk_start.is_some() {
            return false;
        }
        self.walk_start = Some(elapsed);
        true
    }

    fn reset_walk(&mut self) -> bool {
        self.walk_facing = None;
        self.walk_start.take().is_some()
    }

    fn update_walk(&mut self, elapsed: Duration) -> Option<WalkStatus> {
        let Some(((source, walk), start)) = self.walk_subject.as_ref().zip(self.walk_start) else {
            if self.walking.take().is_some() {
                self.refresh_shadows();
            }
            return None;
        };
        let original = &self.ordered[*source];
        let index = walk.index(elapsed.saturating_sub(start));
        if self.walking.as_ref().is_some_and(|draw| draw.step == index) {
            return None;
        }
        let sample = walk.steps[index];
        let key = (
            true,
            super::scenery::order(sample.tile, sample.offset, false, original.key.1.5),
        );
        let asset = walk.asset(sample).unwrap_or(&original.frame);
        let mut raster = Raster::new(key, asset, sample.anchor);
        if sample.frame.is_none()
            && let Some(RasterAnimation::Fidget(poses)) = &original.animation
        {
            raster.set_fidget(Arc::clone(poses));
        }
        raster.object_light = ObjectLight::Point;
        let origin = Caster {
            tile: (original.key.1.2, original.key.1.1),
            anchor: original.anchor,
        };
        self.walking = Some(MovingDraw {
            raster,
            origin,
            step: index,
            insertion: self.ordered.partition_point(|draw| draw.key < key),
            shade: PreparedShade::Tint(Modulation::Unlit),
            standing_since: start.saturating_add(walk.deadline(walk.steps.len() - 1)),
            standing_direction: sample
                .frame
                .is_none()
                .then_some(self.walk_facing.unwrap_or(sample.direction)),
        });
        self.refresh_walk_shade();
        self.refresh_shadows();
        Some(WalkStatus {
            step: index,
            tile: sample.tile,
            offset: sample.offset,
            anchor: sample.anchor,
            direction: sample.direction,
            frame: sample.frame,
        })
    }

    fn refresh_walk_shade(&mut self) {
        let Some((ambient, moving)) = self.ambient.zip(self.walking.as_ref()) else {
            return;
        };
        let raster = &moving.raster;
        let tile = (raster.key.1.2, raster.key.1.1);
        let base = if self.outdoor(tile) {
            ambient.0.outdoor
        } else {
            ambient.0.indoor
        };
        let color = lighting::point(self.light_emitters(), raster.anchor, base);
        self.walking.as_mut().unwrap().shade = PreparedShade::uniform(color);
    }

    fn light_emitters(&self) -> impl Iterator<Item = &StaticLight> {
        // CE adds object lights at the head of the owning sector's list.
        self.active.iter().rev().flat_map(move |&id| {
            self.dynamic
                .iter()
                .rev()
                .filter(move |light| {
                    tile_sector_id(light.state.tile) == id && self.light_active(light)
                })
                .map(|light| &light.mask)
                .chain(self.entries[&id].0.emitters.iter().rev())
        })
    }

    fn fidget_poses(&self) -> Option<&DirectionalFidget> {
        self.animated
            .iter()
            .find_map(|&index| match &self.ordered[index].animation {
                Some(RasterAnimation::Fidget(poses)) => Some(&**poses),
                _ => None,
            })
    }

    fn start_fidget(&mut self, elapsed: Duration) -> bool {
        if self
            .walking
            .as_ref()
            .is_some_and(|moving| moving.standing_direction.is_none())
        {
            return false; // The same subject cannot fidget through a walk pose.
        }
        let Some(poses) = self.fidget_poses() else {
            return false;
        };
        let playback = self
            .walking
            .as_ref()
            .map_or(self.fidget, |moving| moving.playback(self.fidget));
        let clip = poses.sequence(playback.direction);
        if playback
            .start
            .is_some_and(|start| clip.one_shot_index(elapsed.saturating_sub(start)).is_some())
        {
            return false;
        }
        self.fidget.start = Some(elapsed);
        true
    }

    fn turn_fidget(&mut self, step: i8) -> Option<u8> {
        let poses = self.fidget_poses()?;
        let direction = match &self.walking {
            Some(moving) => moving.standing_direction?,
            None => self.fidget.direction.unwrap_or(poses.original_direction),
        };
        let direction = (i16::from(direction) + i16::from(step)).rem_euclid(8) as u8;
        if let Some(moving) = &mut self.walking {
            self.walk_facing = Some(direction);
            moving.standing_direction = Some(direction);
        } else {
            self.fidget.direction = Some(direction);
        }
        Some(direction)
    }

    fn toggle_shadows(&mut self) -> Option<bool> {
        if self.player_shadows.is_none()
            && !self
                .active
                .iter()
                .any(|id| self.entries[id].0.mobile_shadows.is_some())
        {
            return None;
        }
        self.shadows_disabled = !self.shadows_disabled;
        self.refresh_shadows();
        Some(!self.shadows_disabled)
    }

    fn toggle_probe_shadow(&mut self, slot: usize) -> Option<(bool, bool)> {
        let light = self.dynamic.get_mut(slot)?;
        light.shadow_excluded = !light.shadow_excluded;
        let eligible = !light.shadow_excluded;
        Some((eligible, self.refresh_shadows()))
    }

    fn shadow_candidates(&self) -> impl Iterator<Item = shadow::Candidate> {
        self.active.iter().rev().flat_map(move |id| {
            let probes = self
                .dynamic
                .iter()
                .rev()
                .filter(move |light| {
                    tile_sector_id(light.state.tile) == *id && self.light_active(light)
                })
                .filter_map(|light| {
                    let mut candidate = light.mask.shadow_candidate()?;
                    if light.shadow_excluded {
                        candidate.flags |= 0x40;
                    }
                    Some(candidate)
                });
            probes.chain(
                self.entries[id]
                    .0
                    .emitters
                    .iter()
                    .rev()
                    .filter_map(StaticLight::shadow_candidate),
            )
        })
    }

    fn refresh_shadows(&mut self) -> bool {
        let Some((ambient, _)) = self.ambient else {
            return false;
        };
        let mut prepared = std::mem::take(&mut self.shadows);
        let mut count = 0;
        let mut changed = false;
        let mut error = None;
        if !self.shadows_disabled {
            for id in &self.active {
                let sector = &self.entries[id].0;
                let Some(resources) = &sector.mobile_shadows else {
                    continue;
                };
                for &original in &sector.casters {
                    let caster = self
                        .walking
                        .as_ref()
                        .filter(|moving| moving.origin == original)
                        .map_or(original, |moving| Caster {
                            anchor: moving.raster.anchor,
                            tile: (moving.raster.key.1.2, moving.raster.key.1.1),
                        });
                    let (masks, invalid) = self.caster_masks(resources, caster, ambient);
                    error = error.or(invalid);
                    for mask in masks {
                        let frame = resources.frame(mask);
                        let draw = PreparedMask {
                            anchor: caster.anchor,
                            frame,
                            mask,
                        };
                        if let Some(old) = prepared.get_mut(count) {
                            changed |= old.anchor != draw.anchor
                                || old.mask != draw.mask
                                || !Arc::ptr_eq(&old.frame, &draw.frame);
                            *old = draw;
                        } else {
                            changed = true;
                            prepared.push(draw);
                        }
                        count += 1;
                    }
                }
            }
        }
        changed |= prepared.len() != count;
        prepared.truncate(count);
        self.shadows = prepared;
        if error != self.shadow_error {
            if let Some(error) = error {
                eprintln!(
                    "Original directional shadow lookup rejected a sample: {error}; using ambient mask for the affected caster"
                );
            }
            self.shadow_error = error;
        }
        changed
    }

    fn caster_masks(
        &self,
        resources: &MobileShadows,
        caster: Caster,
        ambient: Ambient,
    ) -> (Vec<shadow::Mask>, Option<shadow::Error>) {
        let base = if self.outdoor(caster.tile) {
            ambient.outdoor
        } else {
            ambient.indoor
        };
        let lookup = resources.lookup.lookup();
        match shadow::select(
            &lookup,
            caster.anchor,
            base,
            0,
            true,
            self.shadow_candidates(),
        ) {
            Ok(masks) => (masks, None),
            Err(invalid) => (
                shadow::select(&lookup, caster.anchor, base, 0, false, [])
                    .expect("ambient shadow selection"),
                Some(invalid),
            ),
        }
    }

    fn refresh_shades(&mut self, dirty: Option<[Option<RasterBounds>; 2]>) -> usize {
        let Some((ambient, reference)) = self.ambient else {
            return 0;
        };
        let mut shades = std::mem::take(&mut self.shades);
        let emitters: Vec<_> = self.light_emitters().collect();
        let ambient_at = |tile| {
            if self.outdoor(tile) {
                ambient.outdoor
            } else {
                ambient.indoor
            }
        };
        let mut updated = 0;
        if let Some(dirty) = dirty {
            for (index, raster) in self.ordered.iter().enumerate() {
                let receiver = light_receiver(raster, reference);
                if receiver.is_some_and(|r| dirty.iter().flatten().any(|&d| overlaps(r, d))) {
                    shades[index] = self.shade(raster, reference, ambient, &emitters, &ambient_at);
                    updated += 1;
                }
            }
        } else {
            shades.clear();
            shades.extend(
                self.ordered
                    .iter()
                    .map(|raster| self.shade(raster, reference, ambient, &emitters, &ambient_at)),
            );
            updated = shades.len();
        }
        self.shades = shades;
        self.refresh_walk_shade();
        updated
    }

    fn shade(
        &self,
        raster: &Raster,
        reference: (u32, u32),
        ambient: Ambient,
        emitters: &[&StaticLight],
        ambient_at: &impl Fn((u32, u32)) -> [u8; 3],
    ) -> PreparedShade {
        let tile = (raster.key.1.2, raster.key.1.1);
        let base = ambient_at(tile);
        if !raster.key.0 {
            let center = project_world_tile(tile, reference).expect("prepared surface projection");
            return PreparedShade::Tint(Modulation::floor(self.lights.surface(
                &self.dynamic_grid,
                center,
                base,
            )));
        }
        match raster.object_light {
            ObjectLight::Point => PreparedShade::uniform(lighting::point(
                emitters.iter().copied(),
                raster.anchor,
                base,
            )),
            ObjectLight::Portal(rotation) => {
                let (sample_tile, offset) = lighting::portal_sample(tile, rotation);
                let color = project_world_tile(sample_tile, reference)
                    .and_then(|(x, y)| Some((x.checked_add(offset)?, y.checked_sub(10)?)))
                    .map_or(base, |sample| {
                        lighting::point(emitters.iter().copied(), sample, ambient_at(sample_tile))
                    });
                PreparedShade::uniform(color)
            }
            ObjectLight::Wall(rotation) => {
                let face = WallFace::from_rotation(rotation);
                let base = if face.exterior() {
                    ambient.outdoor
                } else {
                    base
                };
                let center = project_world_tile(tile, reference).expect("prepared wall tile");
                let colors = face.columns(
                    emitters,
                    tile,
                    center,
                    raster.bounds.min.0,
                    raster.frame.decoded.frame.width as usize,
                    base,
                );
                if colors.iter().all(|&color| color == colors[0]) {
                    PreparedShade::uniform(colors[0])
                } else {
                    PreparedShade::Columns(colors.into_boxed_slice())
                }
            }
            ObjectLight::Roof => PreparedShade::uniform(ambient.outdoor),
            ObjectLight::Unlit => PreparedShade::Tint(Modulation::Unlit),
            ObjectLight::Additive => PreparedShade::Tint(Modulation::Additive),
        }
    }

    fn stats(&self, (reads, art): (u64, FrameCacheStats)) -> StreamStats {
        StreamStats {
            sector_reads: self.loads,
            sector_hits: self.hits,
            sector_evictions: self.evictions,
            cached_sectors: self.entries.len(),
            active_sectors: self.active.len(),
            vfs_reads: reads,
            raster_bytes: (self.ordered.capacity()
                + self
                    .entries
                    .values()
                    .map(|(sector, _)| sector.draws.capacity())
                    .sum::<usize>())
                * size_of::<Raster>(),
            art,
            light_samples: self.lights.samples() + self.dynamic_grid.samples(),
            active_dynamic_lights: self
                .dynamic
                .iter()
                .filter(|light| self.light_active(light))
                .count(),
            draw_lighting_bytes: self.shades.capacity() * size_of::<PreparedShade>()
                + self
                    .shades
                    .iter()
                    .map(|shade| match shade {
                        PreparedShade::Columns(colors) => size_of_val(colors.as_ref()),
                        _ => 0,
                    })
                    .sum::<usize>(),
            light_grid_payload_bytes: self.lights.payload_bytes()
                + self.dynamic_grid.payload_bytes()
                + self
                    .entries
                    .values()
                    .map(|(sector, _)| sector.lights.payload_bytes())
                    .sum::<usize>(),
        }
    }

    fn visible(&self, camera: &Camera, size: (u32, u32)) -> impl Iterator<Item = SpriteDraw<'_>> {
        self.visible_assets(camera, size, Duration::ZERO)
            .map(|draw| draw.sprite)
    }

    fn animation_delay(
        &self,
        camera: &Camera,
        size: (u32, u32),
        elapsed: Duration,
    ) -> Option<Duration> {
        let viewport = Viewport::new(camera.position(), size);
        self.animated
            .iter()
            .copied()
            .filter(|index| {
                self.walking.is_none()
                    || Some(*index) != self.walk_subject.as_ref().map(|&(index, _)| index)
            })
            .filter_map(|index| {
                let raster = &self.ordered[index];
                self.raster_revealed(raster)
                    .then(|| raster.next_change(&viewport, elapsed, self.fidget))
                    .flatten()
            })
            .chain(self.walk_delay(&viewport, elapsed))
            .chain(self.door_delay(&viewport, elapsed))
            .min()
    }

    fn walk_delay(&self, viewport: &Viewport, elapsed: Duration) -> Option<Duration> {
        let ((source, walk), start) = self.walk_subject.as_ref().zip(self.walk_start)?;
        let original = &self.ordered[*source];
        let index = walk.index(elapsed.saturating_sub(start));
        if index + 1 == walk.steps.len() {
            let moving = self.walking.as_ref()?;
            return moving
                .raster
                .next_change(viewport, elapsed, moving.playback(self.fidget));
        }
        let visible = |&step: &WalkStep| {
            let asset = walk
                .asset(step)
                .unwrap_or_else(|| match &original.animation {
                    Some(RasterAnimation::Fidget(poses)) => {
                        &poses.sequence(Some(step.direction)).frames[0]
                    }
                    _ => &original.frame,
                });
            let anchor = step.anchor;
            if viewport.intersects(RasterBounds::new(SpriteDraw {
                frame: &asset.decoded.frame,
                anchor,
            })) {
                return true;
            }
            let (x, y) = (i64::from(anchor.0), i64::from(anchor.1));
            if self.shadows_disabled
                || !viewport.intersects(RasterBounds {
                    min: (x - OVERHANG, y - OVERHANG),
                    max: (x + OVERHANG, y + OVERHANG),
                })
            {
                return false;
            }
            let Some(resources) = self.entries[&tile_sector_id(step.tile)]
                .0
                .mobile_shadows
                .as_ref()
            else {
                return false;
            };
            let Some((ambient, _)) = self.ambient else {
                return false;
            };
            let (masks, _) = self.caster_masks(
                resources,
                Caster {
                    tile: step.tile,
                    anchor,
                },
                ambient,
            );
            masks.into_iter().any(|mask| {
                viewport.intersects(RasterBounds::new(SpriteDraw {
                    frame: &resources.asset(mask).decoded.frame,
                    anchor,
                }))
            })
        };
        let next = if visible(&walk.steps[index]) {
            index + 1
        } else {
            index + 1 + walk.steps[index + 1..].iter().position(visible)?
        };
        Some(
            start
                .saturating_add(walk.deadline(next))
                .saturating_sub(elapsed),
        )
    }

    fn visible_assets(
        &self,
        camera: &Camera,
        size: (u32, u32),
        elapsed: Duration,
    ) -> impl Iterator<Item = WorldDraw<'_>> {
        self.visible_with_shadows(camera, size, elapsed, &self.shadows)
    }

    fn visible_rasters(
        &self,
        range: Range<usize>,
        viewport: Viewport,
        elapsed: Duration,
    ) -> impl Iterator<Item = WorldDraw<'_>> {
        let replaced = self
            .walking
            .as_ref()
            .and(self.walk_subject.as_ref())
            .map(|&(source, _)| source);
        range.filter_map(move |index| {
            if replaced == Some(index) {
                return None;
            }
            let raster = &self.ordered[index];
            let (asset, sprite) = if let Some((asset, draw)) = self.door_frame(raster, elapsed) {
                (asset, viewport.place(draw, RasterBounds::new(draw))?)
            } else {
                raster.visible_at(&viewport, elapsed, self.fidget)?
            };
            if !self.raster_revealed(raster) {
                return None;
            }
            let mut modulation = self.shades.get(index).map_or_else(
                || {
                    if matches!(raster.object_light, ObjectLight::Additive) {
                        Modulation::Additive
                    } else {
                        Modulation::Unlit
                    }
                },
                PreparedShade::modulation,
            );
            if let Some(corners) = self.fade(raster) {
                let (color, columns) = match modulation {
                    Modulation::Uniform(color) => (color, &[][..]),
                    Modulation::Columns(columns) => ([255; 3], columns),
                    _ => ([255; 3], &[][..]),
                };
                modulation = Modulation::Fade {
                    corners,
                    color,
                    columns,
                };
            }
            Some(WorldDraw {
                asset,
                sprite,
                modulation,
            })
        })
    }

    fn player_draw(
        &self,
        viewport: Viewport,
        elapsed: Duration,
    ) -> Option<(WorldOrder, WorldDraw<'_>)> {
        let player = self.player.as_ref()?;
        let pose = player.sample(elapsed);
        if self.roof_covered(pose.tile) {
            return None;
        }
        let draw = SpriteDraw {
            frame: &pose.asset.decoded.frame,
            anchor: pose.anchor,
        };
        let sprite = viewport.place(draw, RasterBounds::new(draw))?;
        let modulation = self.ambient.map_or(Modulation::Unlit, |(ambient, _)| {
            let base = if self.outdoor(pose.tile) {
                ambient.outdoor
            } else {
                ambient.indoor
            };
            Modulation::Uniform(lighting::point(self.light_emitters(), pose.anchor, base))
        });
        let key = (
            true,
            super::scenery::order(pose.tile, pose.offset, false, usize::MAX),
        );
        Some((
            key,
            WorldDraw {
                asset: pose.asset,
                sprite,
                modulation,
            },
        ))
    }

    fn collision_cell(
        entries: &BTreeMap<u64, (CachedSector, Option<u64>)>,
        tile: (u32, u32),
    ) -> Option<collision::Cell> {
        let (sector, _) = entries.get(&tile_sector_id(tile))?;
        Some(sector.collision.as_ref()?.cell(tile))
    }

    fn validate_arrival(&self, tile: (u32, u32)) -> Result<(), String> {
        if self.collision_disabled && Self::available_tile(&self.entries, tile) {
            return Ok(());
        }
        let cell = Self::collision_cell(&self.entries, tile);
        if collision::can_stand(cell) {
            return Ok(());
        }
        let detail = if cell.is_none() {
            self.entries
                .get(&tile_sector_id(tile))
                .and_then(|(sector, _)| sector.report.diagnostics.first())
                .map_or("original collision data is unavailable", String::as_str)
        } else {
            "original terrain or an object blocks this tile"
        };
        Err(format!("cannot enter tile {tile:?}: {detail}"))
    }

    fn available_tile(
        entries: &BTreeMap<u64, (CachedSector, Option<u64>)>,
        tile: (u32, u32),
    ) -> bool {
        entries
            .get(&tile_sector_id(tile))
            .is_some_and(|(sector, _)| sector.report.source.is_some())
    }

    fn unrestricted_step(
        entries: &BTreeMap<u64, (CachedSector, Option<u64>)>,
        (x, y): (u32, u32),
        facing: u8,
    ) -> Option<(u32, u32)> {
        let (dx, dy) = collision::DIRECTIONS[usize::from(facing)];
        let next = (x.checked_add_signed(dx)?, y.checked_add_signed(dy)?);
        Self::available_tile(entries, next).then_some(next)
    }

    fn update_player(&mut self, now: Duration) -> Option<(i32, i32)> {
        // Borrow the cache entries only while considering the next tile.
        let entries = &self.entries;
        let states = &self.door_states;
        let player = self.player.as_mut()?;
        player.update(now, |tile, facing| {
            if self.collision_disabled {
                Self::unrestricted_step(entries, tile, facing)
            } else {
                collision::step(tile, facing, |tile| {
                    Self::player_cell(entries, states, tile)
                })
            }
        });
        Some(player.sample(now).anchor)
    }

    fn visible_player_shadows(
        &self,
        viewport: Viewport,
        elapsed: Duration,
    ) -> impl Iterator<Item = WorldDraw<'_>> {
        let bank = self.player_shadows.as_ref();
        let player = self
            .player
            .as_ref()
            .zip(bank)
            .zip(self.ambient)
            .filter(|_| !self.shadows_disabled)
            .map(|((player, bank), (ambient, _))| {
                let pose = player.sample(elapsed);
                let caster = Caster {
                    tile: pose.tile,
                    anchor: pose.anchor,
                };
                let (masks, _) = self.caster_masks(bank, caster, ambient);
                (caster.anchor, masks)
            });
        player.into_iter().flat_map(move |(anchor, masks)| {
            masks.into_iter().filter_map(move |mask| {
                let asset = bank?.asset(mask);
                let sprite = SpriteDraw {
                    frame: &asset.decoded.frame,
                    anchor,
                };
                Some(WorldDraw {
                    asset,
                    sprite: viewport.place(sprite, RasterBounds::new(sprite))?,
                    modulation: mask.modulation(),
                })
            })
        })
    }

    fn visible_with_shadows<'a>(
        &'a self,
        camera: &Camera,
        size: (u32, u32),
        elapsed: Duration,
        shadows: &'a [PreparedMask],
    ) -> impl Iterator<Item = WorldDraw<'a>> {
        let viewport = Viewport::new(camera.position(), size);
        let split = if shadows.is_empty() && self.walking.is_none() && self.player.is_none() {
            self.ordered.len()
        } else {
            self.ordered
                .partition_point(|raster| !raster.key.0 || !raster.key.1.0)
        };
        let player = self.player_draw(viewport, elapsed);
        let hover = self.hover.and_then(|key| {
            let index = self
                .ordered
                .binary_search_by_key(&key, |raster| raster.key)
                .ok()?;
            let mut draw = self
                .visible_rasters(index..index + 1, viewport, elapsed)
                .next()?;
            draw.modulation = Modulation::AdditiveTint([200; 3]);
            Some(draw)
        });
        let roof_start = if hover.is_some() {
            self.ordered
                .partition_point(|raster| raster.key < roof_key((0, 0)))
        } else {
            self.ordered.len()
        };
        let insertion = player.as_ref().map_or_else(
            || {
                self.walking
                    .as_ref()
                    .map_or(split, |moving| moving.insertion)
            },
            |(key, _)| self.ordered.partition_point(|draw| draw.key < *key),
        );
        let moving = self.walking.as_ref().and_then(|moving| {
            let (asset, sprite) =
                moving
                    .raster
                    .visible_at(&viewport, elapsed, moving.playback(self.fidget))?;
            Some(WorldDraw {
                asset,
                sprite,
                modulation: moving.shade.modulation(),
            })
        });
        self.visible_rasters(0..split, viewport, elapsed)
            .chain(shadows.iter().filter_map(move |draw| {
                let sprite = SpriteDraw {
                    frame: &draw.frame.decoded.frame,
                    anchor: draw.anchor,
                };
                Some(WorldDraw {
                    asset: &draw.frame,
                    sprite: viewport.place(sprite, RasterBounds::new(sprite))?,
                    modulation: draw.mask.modulation(),
                })
            }))
            .chain(self.visible_player_shadows(viewport, elapsed))
            .chain(self.visible_rasters(split..insertion, viewport, elapsed))
            .chain(moving)
            .chain(player.map(|(_, draw)| draw))
            .chain(self.visible_rasters(insertion..roof_start, viewport, elapsed))
            .chain(hover)
            .chain(self.visible_rasters(roof_start..self.ordered.len(), viewport, elapsed))
    }

    fn visible_modulated(
        &self,
        camera: &Camera,
        size: (u32, u32),
        elapsed: Duration,
    ) -> impl Iterator<Item = (SpriteDraw<'_>, Modulation<'_>)> {
        self.visible_assets(camera, size, elapsed)
            .map(|draw| (draw.sprite, draw.modulation))
    }
}

/// A selected map rectangle, one immutable set of mounts and a bounded cache.
/// Sector loading takes place in prepare, before immutable raster iteration.
pub struct WorldStream {
    region: SectorRegion,
    loader: SectorLoader,
    cache: SectorCache,
}

impl WorldStream {
    pub fn new(
        mut vfs: Vfs,
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
        let terrain =
            super::terrain::TerrainMap::load(&mut vfs, &map).map_err(|error| error.to_string());
        Ok(Self {
            region,
            loader: SectorLoader {
                mobile_index: Default::default(),
                mobile_prototypes: Default::default(),
                terrain,
                map,
                reference,
                vfs,
                frames: FrameCache::default(),
                lighting: None,
                walking: false,
                playing: false,
                audit: false,
                roofs: false,
            },
            cache: SectorCache::default(),
        })
    }

    /// Spawn the first-playable character at the resolved original map entry.
    pub fn with_player(mut self) -> Result<Self, Box<dyn std::error::Error>> {
        if self.loader.walking || !self.cache.entries.is_empty() {
            return Err("player mode must be selected before loading sectors and without the rabbit preview".into());
        }
        self.cache.jumps = super::jumps::load(&mut self.loader.vfs, &self.loader.map)?;
        let reference = self.loader.reference;
        let player = Player::load(&mut self.loader.vfs, &mut self.loader.frames, reference)?;
        if self.loader.lighting.is_some() {
            self.cache.light_schedule =
                Some(Ambient::schedule(&mut self.loader.vfs, &self.loader.map)?);
            // CE proto.c uses light ART (6,0,0,1) for its generic biped PC too.
            let art = shadow::Art::decode(mobile::VERIFIED_SHADOW)?;
            self.cache.player_shadows = Some(MobileShadows::load(
                &mut self.loader.vfs,
                &mut self.loader.frames,
                art,
            )?);
        }
        self.loader.frames.finish_sector();
        self.cache.player = Some(player);
        self.loader.playing = true;
        self.loader.roofs = true;
        self.cache.roof_state = Some(roof::State::default());
        Ok(self)
    }

    /// Reuse immutable discovery when replacing the active view of the same
    /// map with the same mounted content. Resident state remains sector-local.
    pub fn reuse_mobile_discovery(&mut self, previous: &InteractiveWorld) {
        if self.loader.map == previous.map {
            self.loader.mobile_index.clone_from(&previous.mobile_index);
        }
    }

    /// Start at the original game's authored hour.
    pub fn with_starting_lighting(mut self) -> Result<Self, Box<dyn std::error::Error>> {
        let hour = super::travel::starting_hour(&mut self.loader.vfs)?;
        self.with_lighting(hour)
    }

    pub fn lighting_hour(&self) -> Option<u8> {
        self.cache.ambient.map(|_| self.cache.hour)
    }

    pub fn set_arrival_facing(&mut self, facing: u8) {
        if let Some(player) = &mut self.cache.player {
            player.set_facing(facing & 7);
        }
    }

    pub fn with_collision_bypass(mut self, enabled: bool) -> Self {
        self.cache.collision_disabled = enabled;
        self
    }

    pub fn validate_arrival(&self, tile: (u32, u32)) -> Result<(), String> {
        self.cache.validate_arrival(tile)
    }

    /// Opt in to static surface lighting at a fixed original scheme hour.
    pub fn with_lighting(mut self, hour: u8) -> Result<Self, Box<dyn std::error::Error>> {
        if !self.cache.entries.is_empty() {
            return Err("select world lighting before loading a sector".into());
        }
        let ambient = Ambient::load(&mut self.loader.vfs, &self.loader.map, hour)?;
        let names = load_mes(&mut self.loader.vfs, "art/light/light.mes")?;
        self.loader.lighting = Some((ambient, names));
        self.cache.ambient = Some((ambient, self.loader.reference));
        self.cache.hour = hour;
        Ok(self)
    }

    /// Prepare a diagnostic emitter using the original court brazier or torch.
    /// The caller chooses the placement independently of the source record.
    pub fn add_light_probe(
        &mut self,
        tile: (u32, u32),
        nocturnal: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        use crate::sector::{LightRecord, objects};
        if !self.cache.entries.is_empty() || self.cache.dynamic.len() == 2 {
            return Err("prepare at most two light probes before sector loading".into());
        }
        if self.region.camera(tile).is_none() {
            return Err("light probe is outside the world region".into());
        }
        let (ambient, names) = self
            .loader
            .lighting
            .as_ref()
            .ok_or("light probes require --lighting")?;
        let (_, bytes) = super::sector::read_base_sector(
            &mut self.loader.vfs,
            "k'na tha",
            Coordinates { x: 12, y: 12 },
        )?;
        let scenery = objects::decode(&bytes)?.scenery;
        let index = if nocturnal { 38 } else { 103 };
        let fields = scenery
            .iter()
            .find(|record| record.index == index && record.prototype.0 == 4040)
            .ok_or("missing verified probe template scenery")?
            .fields;
        let flags = fields.light_flags.ok_or("missing probe light flags")?;
        if flags & !0x0800_0080 != 0
            || fields
                .scenery_flags
                .is_none_or(|f| (f & 4 != 0) != nocturnal)
        {
            return Err("unsupported probe template light state".into());
        }
        let offset = (
            fields.offset_x.ok_or("missing probe X offset")?,
            fields.offset_y.ok_or("missing probe Y offset")?,
        );
        let [blue, green, red, _] = fields
            .light_color
            .ok_or("missing probe tint")?
            .to_le_bytes();
        let record = LightRecord {
            owner: 0,
            tile,
            offset,
            flags: 0,
            art_id: fields.light_art.ok_or("missing probe light ART")?,
            rgb: [red, green, blue],
        };
        let mask = lighting::prepare_dynamic(
            &mut self.loader.vfs,
            &mut self.loader.frames,
            &names.mes,
            record,
            self.loader.reference,
            *ambient,
        )?;
        self.cache.dynamic.push(DynamicLight {
            mask,
            state: DynamicLightState {
                tile,
                offset,
                active: true,
            },
            nocturnal,
            shadow_excluded: false,
        });
        self.loader.frames.finish_sector();
        Ok(())
    }

    pub fn dynamic_light(&self, slot: usize) -> Option<DynamicLightState> {
        self.cache.dynamic.get(slot).map(|light| light.state)
    }

    pub fn set_dynamic_light(
        &mut self,
        slot: usize,
        state: DynamicLightState,
    ) -> Result<LightUpdate, &'static str> {
        if self.region.camera(state.tile).is_none() {
            return Err("light position is outside the world region");
        }
        self.cache.set_dynamic(slot, state)
    }

    /// Draw the stored, unfaded roof pass for diagnostic inspection.
    pub fn with_roofs(mut self) -> Self {
        self.loader.roofs = true;
        self
    }

    pub fn reference(&self) -> (u32, u32) {
        self.loader.reference
    }

    /// Retain static raster provenance for headless inspection.
    pub fn with_object_audit(mut self) -> Self {
        self.loader.audit = true;
        self
    }

    /// Prepare the authored rabbit walk in the loaded reference region.
    pub fn with_walk_preview(mut self, start_at_epoch: bool) -> Result<Self, &'static str> {
        if !self.cache.entries.is_empty() {
            return Err("select the walk preview before loading a sector");
        }
        let Coordinates { x, y } =
            Coordinates::from_id(mobile::RABBIT_SECTOR).expect("rabbit sector");
        if !mobile::supports_walk(&self.loader.map, mobile::RABBIT_SECTOR)
            || !(self.region.first.x..=self.region.last.x).contains(&x)
            || !(self.region.first.y..=self.region.last.y).contains(&y)
        {
            return Err("walk preview requires the K'na Tha rabbit sector (6,12)");
        }
        self.loader.walking = true;
        self.cache.walk_start = start_at_epoch.then_some(Duration::ZERO);
        Ok(self)
    }

    pub fn update_walk(&mut self, elapsed: Duration) -> Option<WalkStatus> {
        self.cache.update_walk(elapsed)
    }

    /// Start one original fidget at the shared animation epoch for deterministic previews.
    pub fn with_fidget_preview(mut self) -> Self {
        self.cache.fidget.start = Some(Duration::ZERO);
        self
    }

    pub fn start_fidget(&mut self, elapsed: Duration) -> bool {
        self.cache.start_fidget(elapsed)
    }

    pub fn turn_fidget(&mut self, step: i8) -> Option<u8> {
        self.cache.turn_fidget(step)
    }

    pub fn animation_delay(
        &self,
        camera: &Camera,
        size: (u32, u32),
        elapsed: Duration,
    ) -> Option<Duration> {
        self.cache.animation_delay(camera, size, elapsed)
    }

    pub fn toggle_shadows(&mut self) -> Option<bool> {
        self.cache.toggle_shadows()
    }

    pub fn toggle_probe_shadow(&mut self, slot: usize) -> Option<(bool, bool)> {
        self.cache.toggle_probe_shadow(slot)
    }

    /// Synchronous inspection and controlled benchmarking. The viewer uses into_interactive.
    pub fn prepare(&mut self, camera: &Camera, size: (u32, u32)) -> Result<PrepareTiming, String> {
        let required = self
            .region
            .needed(self.loader.reference, camera.position(), size)?;
        let before = self.loader.frames.stats();
        let mut stages = LoadStages::default();
        let mut timing = self.cache.select(required.as_slice(), MAX_SECTORS, |id| {
            let (sector, slice) = self.loader.load(id);
            stages.sector_read += slice.sector_read;
            stages.floor_parse += slice.floor_parse;
            stages.surfaces += slice.surfaces;
            stages.object_parse += slice.object_parse;
            stages.object_import += slice.object_import;
            stages.rasters += slice.rasters;
            stages.lighting += slice.lighting;
            stages.mobiles += slice.mobiles;
            sector
        });
        timing.stages = stages;
        let after = self.loader.frames.stats();
        timing.art_resolution = after.resolution_time - before.resolution_time;
        timing.art_read = after.read_time - before.read_time;
        timing.art_decode = after.decode_time - before.decode_time;
        Ok(timing)
    }

    pub fn into_interactive(
        self,
        wake: impl Fn() + Send + 'static,
        trace: bool,
    ) -> std::io::Result<InteractiveWorld> {
        prefetch::InteractiveWorld::new(self, wake, trace)
    }

    pub fn reports(&self) -> impl Iterator<Item = &SectorReport> {
        self.cache
            .active
            .iter()
            .map(|id| &self.cache.entries[id].0.report)
    }

    /// Counts accepted object rasters submitted and culled at this viewport.
    /// Surface rasters and rejected imports are excluded.
    pub fn object_visibility(
        &self,
        id: u64,
        camera: &Camera,
        size: (u32, u32),
    ) -> Option<(usize, usize)> {
        let (sector, _) = self.cache.entries.get(&id)?;
        let viewport = Viewport::new(camera.position(), size);
        let mut submitted = 0;
        let mut culled = 0;
        for raster in sector
            .draws
            .iter()
            .filter(|raster| raster.key.0 && !matches!(raster.object_light, ObjectLight::Roof))
        {
            if self.cache.raster_revealed(raster)
                && raster
                    .visible_at(&viewport, Duration::ZERO, self.cache.fidget)
                    .is_some()
            {
                submitted += 1;
            } else {
                culled += 1;
            }
        }
        Some((submitted, culled))
    }

    /// The original records whose accepted rasters intersect the current viewport at tick zero.
    pub fn visible_object_records(
        &self,
        id: u64,
        camera: &Camera,
        size: (u32, u32),
    ) -> impl Iterator<Item = usize> + '_ {
        let viewport = Viewport::new(camera.position(), size);
        self.cache
            .entries
            .get(&id)
            .into_iter()
            .flat_map(move |(sector, _)| {
                sector.draws.iter().filter_map(move |raster| {
                    let (is_object, key) = raster.key;
                    (is_object
                        && !matches!(raster.object_light, ObjectLight::Roof)
                        && raster
                            .visible_at(&viewport, Duration::ZERO, self.cache.fidget)
                            .is_some())
                    .then(|| key.6.unwrap_or(key.5))
                })
            })
    }

    pub fn stats(&self) -> StreamStats {
        self.cache.stats(self.loader.stats())
    }

    pub fn visible(
        &self,
        camera: &Camera,
        size: (u32, u32),
    ) -> impl Iterator<Item = SpriteDraw<'_>> {
        self.cache.visible(camera, size)
    }

    pub fn visible_assets(
        &self,
        camera: &Camera,
        size: (u32, u32),
    ) -> impl Iterator<Item = WorldDraw<'_>> {
        self.visible_assets_at(camera, size, Duration::ZERO)
    }

    pub fn visible_assets_at(
        &self,
        camera: &Camera,
        size: (u32, u32),
        elapsed: Duration,
    ) -> impl Iterator<Item = WorldDraw<'_>> {
        self.cache.visible_assets(camera, size, elapsed)
    }

    /// Draw prepared original shadows after surfaces and flat objects,
    /// before upright objects. Draw order within the supplied masks is preserved.
    pub fn visible_assets_with_shadows<'a>(
        &'a self,
        camera: &Camera,
        size: (u32, u32),
        elapsed: Duration,
        shadows: &'a [PreparedMask],
    ) -> impl Iterator<Item = WorldDraw<'a>> {
        self.cache
            .visible_with_shadows(camera, size, elapsed, shadows)
    }

    pub fn visible_modulated(
        &self,
        camera: &Camera,
        size: (u32, u32),
    ) -> impl Iterator<Item = (SpriteDraw<'_>, Modulation<'_>)> {
        self.cache.visible_modulated(camera, size, Duration::ZERO)
    }
}

// Internal ordering only. The persistent identity is held by Resident.
fn mobile_draw_slot(index: usize) -> usize {
    usize::MAX / 2 + index
}

struct SectorLoader {
    mobile_index: Arc<std::sync::OnceLock<mobile::population::Index>>,
    mobile_prototypes: mobile::population::Prototypes,
    terrain: Result<Option<super::terrain::TerrainMap>, String>,
    map: String,
    reference: (u32, u32),
    vfs: Vfs,
    frames: FrameCache,
    lighting: Option<(Ambient, LoadedMes)>,
    walking: bool,
    playing: bool,
    audit: bool,
    roofs: bool,
}

impl SectorLoader {
    fn load(&mut self, id: u64) -> (CachedSector, LoadStages) {
        self.load_source(id, None)
    }

    fn load_source(&mut self, id: u64, template_path: Option<&str>) -> (CachedSector, LoadStages) {
        let sector = Coordinates::from_id(id).expect("validated sector coordinates");
        let region = WorldRegion {
            map: &self.map,
            sector,
            origin: (0, 0),
            side: SIDE as u8,
            center: sector
                .world_tile(0, 0)
                .expect("validated sector coordinates"),
            // Full-sector import is independent of the current viewport.
            viewport: (1, 1),
        };
        let (room, mut timing) = load_streamed_sector(
            &mut self.vfs,
            &mut self.frames,
            region,
            self.playing,
            self.audit,
            self.terrain
                .as_ref()
                .map(Option::as_ref)
                .map_err(String::as_str),
            template_path,
        );
        let sector = match room {
            Ok(room) => {
                let start = Instant::now();
                let lights = self.lighting.as_ref().map(|(ambient, names)| {
                    lighting::prepare(
                        &mut self.vfs,
                        &mut self.frames,
                        &names.mes,
                        (&room.room.floor.lights, &room.room.scenery),
                        sector,
                        self.reference,
                        *ambient,
                    )
                });
                timing.lighting = start.elapsed();
                let start = Instant::now();
                let diagnostic_walk = mobile::supports_walk(&self.map, id);
                let mobiles =
                    (template_path.is_none() && (self.playing || diagnostic_walk)).then(|| {
                        let index = self.mobile_index.get_or_init(|| {
                            mobile::population::Index::load(&mut self.vfs, &self.map)
                        });
                        mobile::prepare(
                            &mut self.vfs,
                            &mut self.frames,
                            mobile::Prepare {
                                index,
                                prototypes: &mut self.mobile_prototypes,
                                excluded: &room.object_ids,
                                sector: id,
                                reference: self.reference,
                                lighting: self.lighting.is_some(),
                                fidget: diagnostic_walk,
                                walking: self.walking && diagnostic_walk,
                            },
                        )
                    });
                let roofs = if self.roofs {
                    super::roof::prepare(
                        &mut self.vfs,
                        &mut self.frames,
                        sector,
                        self.reference,
                        &room.roof_words,
                        self.playing,
                    )
                } else {
                    roof::Prepared::default()
                };
                let interactions = if self.playing {
                    interaction::prepare(
                        &mut self.vfs,
                        &mut self.frames,
                        &room.room,
                        id,
                        self.reference,
                    )
                } else {
                    BTreeMap::new()
                };
                self.frames.finish_sector();
                timing.mobiles = start.elapsed();
                let start = Instant::now();
                let mut cached =
                    CachedSector::from_room(id, room, self.reference, lights, self.audit);
                if self.playing {
                    for raster in &cached.draws {
                        if let ObjectLight::Wall(rotation) = raster.object_light {
                            cached
                                .wall_tiles
                                .entry((raster.key.1.2, raster.key.1.1))
                                .or_default()
                                .push(roof::wallcheck::Wall {
                                    key: raster.key,
                                    rotation,
                                });
                        }
                    }
                    for walls in cached.wall_tiles.values_mut() {
                        walls.sort_unstable_by_key(|wall| wall.key);
                    }
                }
                for (&key, target) in &interactions {
                    if let interaction::Action::Unsupported(reason) = &target.action {
                        cached.report.structured_issues.push(scan::Issue::record(
                            "interaction_unsupported",
                            key.1.6.unwrap_or(key.1.5),
                            target.tile,
                            reason,
                        ));
                    }
                    if let interaction::Action::Travel(dst) = target.action {
                        cached.report.diagnostics.push(format!(
                            "Use transition {:?} record {} -> {dst:?}",
                            target.tile,
                            key.1.6.unwrap_or(key.1.5)
                        ));
                    }
                    if matches!(target.action, interaction::Action::Door(_)) {
                        cached.doors_at.entry(target.tile).or_default().push(key);
                    }
                }
                cached.interactions = interactions;
                cached.report.structured_issues.extend(
                    roofs
                        .issues
                        .iter()
                        .map(|detail| scan::Issue::new("roof_unavailable", detail)),
                );
                cached.report.diagnostics.extend(roofs.issues);
                cached.roofs = roofs.cells;
                cached.roof_shades = roofs.shades;
                for roof in roofs.rasters {
                    // CE's separate pass is globally ordered by roof row, then column.
                    let key = roof_key(roof.tile);
                    let mut raster = Raster::new(key, &roof.asset, roof.anchor);
                    raster.object_light = ObjectLight::Roof;
                    if project_world_tile(roof.tile, self.reference)
                        .is_some_and(|center| within_overhang(raster.bounds, center))
                    {
                        cached.report.roofs.1 += 1;
                        cached.draws.push(raster);
                    } else {
                        cached.report.diagnostics.push(format!(
                            "roof at {:?} exceeds the sector overhang bound",
                            roof.tile
                        ));
                    }
                }
                if let Some(mobiles) = mobiles {
                    cached.add_mobiles(mobiles, self.reference, self.playing);
                }
                timing.rasters = start.elapsed();
                cached
            }
            Err(error) => CachedSector::missing(id, error.to_string()),
        };
        (sector, timing)
    }

    fn stats(&self) -> (u64, FrameCacheStats) {
        (self.vfs.read_attempts(), self.frames.stats())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires the installed original game"]
    fn dense_original_sectors_enter_the_normal_scene_and_collision_paths() {
        let root = std::env::var_os("ARCANUM_GAME_DIR").expect("set ARCANUM_GAME_DIR");
        let mut loader = SectorLoader {
            mobile_index: Default::default(),
            mobile_prototypes: Default::default(),
            terrain: Ok(None),
            map: "castle s'nel n'fa".into(),
            reference: (0, 0),
            vfs: Vfs::from_game(root, crate::vfs::Policy::Ce).unwrap(),
            frames: FrameCache::default(),
            lighting: None,
            walking: false,
            playing: true,
            audit: true,
            roofs: true,
        };
        for (path, id, count, doors) in [
            (None, 67_108_871, 1_915, 21),
            (
                Some("terrain/plains to broad leaf forest/3.sec"),
                3,
                1_845,
                0,
            ),
        ] {
            loader.reference = Coordinates::from_id(id)
                .unwrap()
                .world_tile(32, 32)
                .unwrap();
            let (sector, _) = loader.load_source(id, path);
            let report = &sector.report;
            assert!(!report.object_list_failed, "{:?}", report.diagnostics);
            assert_eq!(report.decoded_records, Some(count));
            assert!(sector.collision.is_some());
            assert!(
                sector
                    .draws
                    .iter()
                    .any(|draw| draw.key.0 && draw.key.1.6.unwrap_or(draw.key.1.5) >= 1024),
                "late original records must produce scene rasters in {id}"
            );
            assert!(report.original_records.keys().any(|&record| record >= 1024));
            assert_eq!(sector.doors_at.values().map(Vec::len).sum::<usize>(), doors);
        }
    }

    #[test]
    #[ignore = "requires the installed original game"]
    fn original_terrain_templates_supply_neighboring_world_sectors_and_collision() {
        let root = std::env::var_os("ARCANUM_GAME_DIR").expect("set ARCANUM_GAME_DIR");
        let open = || Vfs::from_game(&root, crate::vfs::Policy::Ce).unwrap();
        let mut atlas_vfs = open();
        let atlas = super::super::map_atlas::Atlas::load(&mut atlas_vfs).unwrap();
        let map = atlas.catalog.new_game().unwrap();
        let bounds = map.files(&mut atlas_vfs).unwrap().bounds();
        assert_eq!(bounds.last, Coordinates { x: 1999, y: 1999 });
        for (center, sector, template) in [
            (
                (28320, 31168),
                Coordinates { x: 441, y: 487 },
                "terrain/desert island/67108864.sec",
            ),
            (
                (19704, 37452),
                Coordinates { x: 308, y: 585 },
                "terrain/desert island/0.sec",
            ),
        ] {
            let mut camera = bounds.camera(center).unwrap();
            camera.follow((0, 0));
            let mut world = WorldStream::new(open(), map.directory.clone(), bounds, center)
                .unwrap()
                .with_player()
                .unwrap();
            world.prepare(&camera, (1280, 720)).unwrap();
            world.validate_arrival(center).unwrap();
            let report = world
                .reports()
                .find(|report| Some(report.id) == sector.id())
                .unwrap();
            assert!(report.source.as_deref().unwrap().starts_with(template));
            assert_eq!(report.surfaces, 4096);
            assert!(report.facades > 0 && report.scenery_count > 0);
            assert!(report.collision_available && !report.object_list_failed);
            let corner = sector.world_tile(0, 0).unwrap();
            let walkable = (0..64)
                .flat_map(|y| (0..64).map(move |x| (corner.0 + x, corner.1 + y)))
                .any(|tile| world.validate_arrival(tile).is_ok());
            assert!(
                walkable,
                "{template} should include original walkable tiles"
            );
        }

        let roseborough = atlas
            .areas
            .values()
            .find(|area| area.name.eq_ignore_ascii_case("roseborough"))
            .unwrap();
        let center = roseborough.tile;
        let mut camera = bounds.camera(center).unwrap();
        camera.follow((0, 0));
        let mut world = WorldStream::new(open(), map.directory.clone(), bounds, center)
            .unwrap()
            .with_object_audit();
        world.prepare(&camera, (1280, 720)).unwrap();
        assert!(world.reports().any(|sector| {
            sector
                .source
                .as_deref()
                .unwrap_or_default()
                .starts_with("terrain/")
                && sector.surfaces > 0
        }));
    }

    #[test]
    #[ignore = "requires the installed original game"]
    fn overland_arrival_has_collision_and_hourly_lighting_without_asset_reads() {
        let root = std::env::var_os("ARCANUM_GAME_DIR").expect("set ARCANUM_GAME_DIR");
        let mut vfs = Vfs::from_game(root, crate::vfs::Policy::Ce).unwrap();
        let atlas = super::super::map_atlas::Atlas::load(&mut vfs).unwrap();
        let map = atlas.catalog.new_game().unwrap();
        let entry = atlas
            .areas
            .values()
            .find(|area| area.name == "Qintarra")
            .unwrap()
            .tile;
        let region = map.files(&mut vfs).unwrap().bounds();
        let mut camera = region.camera(entry).unwrap();
        camera.follow((0, 0));
        let size = (1280, 720);
        let mut scene = WorldStream::new(vfs, map.directory.clone(), region, entry)
            .unwrap()
            .with_lighting(15)
            .unwrap()
            .with_player()
            .unwrap();
        scene.prepare(&camera, size).unwrap();
        scene.validate_arrival(entry).unwrap();

        let reads = scene.loader.stats();
        let geometry = scene.cache.ordered.as_ptr();
        let pixels = |scene: &WorldStream| {
            let mut target = vec![[0; 3]; size.0 as usize * size.1 as usize];
            crate::compositor::blit_modulated(
                &mut target,
                size,
                size.0 as usize,
                scene
                    .visible_assets_at(&camera, size, Duration::ZERO)
                    .map(|draw| (draw.sprite, draw.modulation)),
            )
            .unwrap();
            target
        };
        let day = pixels(&scene);
        assert!(scene.cache.set_game_hour(5));
        assert!(pixels(&scene) != day, "night must change visible pixels");
        assert!(scene.cache.set_game_hour(15));
        assert!(
            pixels(&scene) == day,
            "returning to the same hour must reproduce pixels"
        );
        assert_eq!(scene.cache.ordered.as_ptr(), geometry);
        let after = scene.loader.stats();
        assert_eq!(
            (after.0, after.1.reads, after.1.decodes),
            (reads.0, reads.1.reads, reads.1.decodes)
        );
    }

    #[test]
    #[ignore = "requires the installed original game"]
    fn original_new_game_entry_and_crash_site_content() {
        let game = std::env::var_os("ARCANUM_GAME_DIR").expect("set ARCANUM_GAME_DIR");
        let mut vfs = Vfs::from_game(game, crate::vfs::Policy::Ce).unwrap();
        let catalog = super::super::maps::Catalog::load(&mut vfs).unwrap();
        let map = catalog.new_game().unwrap();
        let files = map.files(&mut vfs).unwrap();
        let center = files.center(None).unwrap();
        assert_eq!((map.id, center), (5000, (92958, 82592)));
        let region = files.bounds();
        let camera = region.camera(center).unwrap();
        let mut world = WorldStream::new(vfs, map.directory.clone(), region, center)
            .unwrap()
            .with_starting_lighting()
            .unwrap()
            .with_player()
            .unwrap();
        let size = (1280, 720);
        world.prepare(&camera, size).unwrap();
        world.validate_arrival(center).unwrap();
        let assets = |path: &'static str| {
            world
                .cache
                .ordered
                .iter()
                .filter(move |r| r.frame.source.virtual_path.ends_with(path))
        };
        assert!(assets("campfire.art").any(|r| r.animation.is_some()));
        let wreck = assets("small_plane_crashed.art").next().unwrap();
        let tile = (wreck.key.1.2, wreck.key.1.1);
        assert!(!collision::can_stand(SectorCache::collision_cell(
            &world.cache.entries,
            tile
        )));
        let cave = assets("cave_entrance-clickable.art").next().unwrap();
        let target = world
            .cache
            .target(cave.key)
            .expect("original cave entrance has a Use event");
        assert_eq!(target.tile, (92910, 82598));
        let interaction::Action::Travel(dst) = &target.action else {
            if let interaction::Action::Unsupported(reason) = &target.action {
                panic!("{reason}");
            }
            panic!("cave entrance is not a travel target");
        };
        assert_eq!(dst.map, Some(5078));
        assert_eq!(dst.tile, (2087, 2018));
        let entry = (92913, 82598);
        let cell =
            |tile| SectorCache::player_cell(&world.cache.entries, &world.cache.door_states, tile);
        let walk = super::super::player::path::find_with(center, entry, |tile, direction| {
            collision::step(tile, direction, cell).map(|tile| (tile, 0))
        })
        .expect("original collision permits a path from the new-game tile to the cave");
        let mut at = center;
        for (index, direction) in walk.into_iter().enumerate() {
            at = collision::step(at, direction, cell).unwrap();
            if (index + 1).is_multiple_of(6) || at == entry {
                eprintln!("Crash Site route waypoint {at:?}");
            }
        }
        assert_eq!(at, entry);
        let mut vfs = Vfs::from_game(
            std::env::var_os("ARCANUM_GAME_DIR").unwrap(),
            crate::vfs::Policy::Ce,
        )
        .unwrap();
        let map = catalog.find("5078").unwrap();
        let files = map.files(&mut vfs).unwrap();
        let region = files.bounds();
        let camera = region.camera(dst.tile).unwrap();
        let mut cave = WorldStream::new(vfs, map.directory.clone(), region, dst.tile)
            .unwrap()
            .with_starting_lighting()
            .unwrap()
            .with_player()
            .unwrap();
        cave.prepare(&camera, size).unwrap();
        cave.validate_arrival(dst.tile).unwrap();
        let exit = cave
            .cache
            .ordered
            .iter()
            .find(|r| r.frame.source.virtual_path.ends_with("/cave_entrance.art"))
            .unwrap();
        let target = cave
            .cache
            .target(exit.key)
            .expect("the cave exit has a Use event");
        let interaction::Action::Travel(back) = &target.action else {
            if let interaction::Action::Unsupported(reason) = &target.action {
                panic!("{reason}");
            }
            panic!("cave exit is not a travel target");
        };
        assert_eq!(back.map, Some(5000));
        eprintln!("original cave return {back:?}");
    }

    #[test]
    #[ignore = "requires the installed original game"]
    fn tulla_authored_arrival_retains_wall_end_caps() {
        let game = std::env::var_os("ARCANUM_GAME_DIR").expect("set ARCANUM_GAME_DIR");
        let mut vfs = Vfs::from_game(game, crate::vfs::Policy::Ce).unwrap();
        let atlas = super::super::map_atlas::Atlas::load(&mut vfs).unwrap();
        let entry = atlas.areas[&11].tile;
        assert_eq!(entry, (28455, 31171));
        let map = atlas.catalog.find("5000").unwrap();
        let region = map.files(&mut vfs).unwrap().bounds();
        let mut world = WorldStream::new(vfs, map.directory.clone(), region, entry)
            .unwrap()
            .with_player()
            .unwrap();
        world
            .prepare(&region.camera(entry).unwrap(), (1280, 720))
            .unwrap();
        world.validate_arrival(entry).unwrap();
        for tile in [(28438, 31162), (28438, 31178)] {
            assert!(
                world.cache.ordered.iter().any(|raster| {
                    (raster.key.1.2, raster.key.1.1) == tile
                        && raster.frame.source.virtual_path.ends_with("uecu0.art")
                }),
                "missing wall end cap at {tile:?}"
            );
        }
    }

    #[test]
    #[ignore = "requires the installed original game"]
    fn vendigroth_entrance_uses_original_transition_and_loads_past_the_trap() {
        let game = std::env::var_os("ARCANUM_GAME_DIR").expect("set ARCANUM_GAME_DIR");
        let open = || Vfs::from_game(&game, crate::vfs::Policy::Ce).unwrap();
        let mut vfs = open();
        let catalog = super::super::maps::Catalog::load(&mut vfs).unwrap();
        let map = catalog.find("5000").unwrap();
        let region = map.files(&mut vfs).unwrap().bounds();
        let start = (19689, 37487);
        let viewport = (1280, 720);
        let mut world = WorldStream::new(vfs, map.directory.clone(), region, start)
            .unwrap()
            .with_player()
            .unwrap();
        world
            .prepare(&region.camera(start).unwrap(), viewport)
            .unwrap();
        let raster = world
            .cache
            .ordered
            .iter()
            .find(|raster| {
                raster.frame.source.virtual_path == "art/scenery/vendigroth-belltower-door.art"
            })
            .unwrap();
        assert_eq!(raster.key.1.5, 256);
        let origin = crate::compositor::sprite_origin(&raster.frame.decoded.frame, raster.anchor);
        assert!(
            (0..raster.frame.decoded.frame.height).any(|y| (0..raster.frame.decoded.frame.width)
                .any(|x| {
                    world.cache.pick(
                        (origin.0 + i64::from(x), origin.1 + i64::from(y)),
                        start,
                        Duration::ZERO,
                    ) == Some(raster.key)
                })),
            "the serialized entrance must be pickable"
        );
        let key = raster.key;
        assert_eq!(world.cache.approach(key, false), Some(0));
        let destination = world
            .cache
            .use_target(key, Duration::ZERO)
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(
            (destination.map, destination.tile),
            (Some(5032), (2051, 2038))
        );

        let mut vfs = open();
        let map = catalog.find("5032").unwrap();
        let region = map.files(&mut vfs).unwrap().bounds();
        let mut underground =
            WorldStream::new(vfs, map.directory.clone(), region, destination.tile)
                .unwrap()
                .with_player()
                .unwrap();
        underground
            .prepare(&region.camera(destination.tile).unwrap(), viewport)
            .unwrap();
        underground.validate_arrival(destination.tile).unwrap();
        let report = underground
            .reports()
            .find(|report| report.id == 2080374816)
            .unwrap();
        assert!(!report.object_list_failed && report.collision_available);
        assert!(
            report
                .diagnostics
                .iter()
                .any(|text| text.contains("record 91 Trap, prototype 26021"))
        );
        assert!(underground.cache.ordered.iter().any(|raster| {
            raster.key.0
                && raster.key.1.5 > 91
                && tile_sector_id((raster.key.1.2, raster.key.1.1)) == report.id
        }));
    }

    #[test]
    fn developer_step_requires_available_world_coordinates() {
        let mut sector = CachedSector::missing(0, "fixture".into());
        sector.report.source = Some("fixture".into());
        let entries = BTreeMap::from([(0, (sector, None))]);
        assert_eq!(SectorCache::unrestricted_step(&entries, (0, 0), 1), None);
        assert_eq!(SectorCache::unrestricted_step(&entries, (63, 63), 5), None);
        let start = (10, 10);
        assert!(SectorCache::unrestricted_step(&entries, start, 1).is_some());
        assert_eq!(
            collision::step(start, 1, |tile| SectorCache::collision_cell(&entries, tile)),
            None
        );
    }

    #[test]
    #[ignore = "requires the installed original game"]
    fn tarant_pedestrian_gate_and_both_gatehouses_load_in_play() {
        let game = std::env::var_os("ARCANUM_GAME_DIR").expect("set ARCANUM_GAME_DIR");
        let mut vfs = Vfs::from_game(game, crate::vfs::Policy::Ce).unwrap();
        let catalog = super::super::maps::Catalog::load(&mut vfs).unwrap();
        let map = catalog.find("5000").unwrap();
        let files = map.files(&mut vfs).unwrap();
        let (source, bytes) = super::super::read_bounded(
            &mut vfs,
            "maps/arcanum1-024-fixed/68786586568.sec",
            crate::sector::MAX_INPUT_BYTES,
        )
        .unwrap();
        assert!(source.mount_path.ends_with("Arcanum.PATCH1"), "{source}");
        let records = crate::sector::objects::decode(&bytes).unwrap();
        let original = records.portals.iter().find(|r| r.index == 646).unwrap();
        assert_eq!(original.art_id, 0x32c0_2c00);
        assert_eq!(original.fields.unlock_script, Some(1151));
        assert_eq!(original.fields.portal_flags, Some(0x11));
        assert_eq!(original.stored_tile, (61998, 65660));
        let start = (61997, 65660);
        let size = (1280, 720);
        let region = files.bounds();
        let camera = region.camera(start).unwrap();
        let mut world = WorldStream::new(vfs, map.directory.clone(), region, start)
            .unwrap()
            .with_player()
            .unwrap();
        world.prepare(&camera, size).unwrap();

        let tile = (61998, 65660);
        let sector = &world.cache.entries[&tile_sector_id(tile)].0;
        let (&key, target) = sector
            .interactions
            .iter()
            .find(|(key, _)| key.1.6 == Some(646))
            .expect("original pedestrian gate must register for use");
        assert_eq!(target.tile, tile);
        let interaction::Action::Door(door) = &target.action else {
            panic!("original pedestrian gate must use the ordinary door rules");
        };
        assert!(!door.originally_open);
        assert_eq!(door.direction, 5);
        let crosses_gate = |cache: &SectorCache| {
            collision::step(tile, 5, |cell| {
                SectorCache::player_cell(&cache.entries, &cache.door_states, cell)
            })
        };
        assert_eq!(crosses_gate(&world.cache), None);
        let raster = world
            .cache
            .ordered
            .iter()
            .find(|raster| raster.key == key)
            .unwrap();
        assert_eq!(raster.frame.source.virtual_path, "art/portal/rire3au0.art");
        let sprite = raster.frame_draw(&raster.frame);
        let origin = crate::compositor::sprite_origin(sprite.frame, sprite.anchor);
        assert!(
            (0..sprite.frame.height).any(|y| (0..sprite.frame.width).any(|x| {
                world.cache.pick(
                    (origin.0 + i64::from(x), origin.1 + i64::from(y)),
                    start,
                    Duration::ZERO,
                ) == Some(key)
            })),
            "the original gate must be pickable from this view"
        );

        for (y, slots) in [(65654, [218, 219, 234, 235]), (65670, [26, 27, 42, 43])] {
            for (index, (x, y)) in [(61994, y), (61998, y), (61994, y + 4), (61998, y + 4)]
                .into_iter()
                .enumerate()
            {
                let roof = world
                    .cache
                    .ordered
                    .iter()
                    .find(|raster| raster.key == roof_key((x, y)))
                    .unwrap_or_else(|| panic!("missing gatehouse roof slot {}", slots[index]));
                assert!(world.cache.raster_revealed(roof));
                assert_eq!(roof.frame.source.virtual_path, "art/roof/gb0.art");
            }
        }
        assert_eq!(world.cache.approach(key, false), Some(1));
        world.cache.update_player(Duration::ZERO);
        let arrived = Duration::from_secs(1);
        world.cache.update_player(arrived);
        assert_eq!(world.cache.player.as_ref().unwrap().tile(), tile);
        assert_eq!(
            world.cache.use_target(key, arrived).unwrap(),
            Err("door is locked".into())
        );
        assert_eq!(crosses_gate(&world.cache), None);
    }

    #[test]
    #[ignore = "requires the installed original game"]
    fn player_uses_original_movement_assets_and_collision_across_sectors() {
        let game = std::env::var_os("ARCANUM_GAME_DIR").expect("set ARCANUM_GAME_DIR");
        let mut vfs = Vfs::from_game(game, crate::vfs::Policy::Ce).unwrap();
        let catalog = super::super::maps::Catalog::load(&mut vfs).unwrap();
        let map = catalog.find("k'na tha").unwrap();
        let files = map.files(&mut vfs).unwrap();
        let start = files.center(Some((383, 928))).unwrap();
        let mut camera = files.region.camera(start).unwrap();
        camera.follow((0, 0));
        let mut world = WorldStream::new(vfs, map.directory.clone(), files.region, start)
            .unwrap()
            .with_player()
            .unwrap();
        let size = (1280, 720);
        world.prepare(&camera, size).unwrap();
        let cell = |tile| SectorCache::collision_cell(&world.cache.entries, tile);
        assert!(
            collision::can_stand(cell(start)),
            "original grass spawn should be walkable"
        );
        assert_eq!(collision::step(start, 5, cell), Some((384, 928)));
        assert_ne!(tile_sector_id(start), tile_sector_id((384, 928)));

        let still = world.cache.player.as_ref().unwrap().sample(Duration::ZERO);
        assert_eq!(
            still.asset.source.virtual_path,
            "art/critter/hmm/hmmv1xaa.art"
        );
        assert_eq!(still.anchor, (0, 0));
        world.cache.plan_route((384, 928), false).unwrap();
        world.cache.update_player(Duration::ZERO);
        let middle = world
            .cache
            .update_player(Duration::from_millis(60))
            .unwrap();
        assert!((-39..0).contains(&middle.0) && (1..20).contains(&middle.1));
        let stepping = world
            .cache
            .player
            .as_ref()
            .unwrap()
            .sample(Duration::from_millis(60));
        assert_eq!(
            stepping.asset.source.virtual_path,
            "art/critter/hmm/hmmv1xab.art"
        );
        let destination = world.cache.update_player(Duration::from_secs(2)).unwrap();
        assert_eq!(destination, (-40, 20));
        let player = world.cache.player.as_ref().unwrap();
        assert_eq!(player.tile(), (384, 928));
        assert!(!player.moving());
        camera.follow(destination);
        camera.resize((1024, 576));
        assert_eq!(camera.position(), destination);
        world.cache.plan_route(start, true).unwrap();
        world.cache.update_player(Duration::from_secs(3));
        let running = world
            .cache
            .player
            .as_ref()
            .unwrap()
            .sample(Duration::from_millis(3050));
        assert_eq!(
            running.asset.source.virtual_path,
            "art/critter/hmm/hmmv1xag.art"
        );
        assert_ne!(running.anchor, destination);
        world.cache.plan_route((384, 928), false).unwrap();
        world.cache.update_player(Duration::from_secs(5));
        let resumed = world
            .cache
            .player
            .as_ref()
            .unwrap()
            .sample(Duration::from_millis(5050));
        assert_eq!(
            resumed.asset.source.virtual_path,
            "art/critter/hmm/hmmv1xab.art"
        );
    }

    #[test]
    #[ignore = "requires the installed original game"]
    fn playable_lighting_matches_world_at_the_original_hour_and_shadows_reach_pixels() {
        let game = std::env::var_os("ARCANUM_GAME_DIR").expect("set ARCANUM_GAME_DIR");
        let open = || Vfs::from_game(&game, crate::vfs::Policy::Ce).unwrap();
        let mut vfs = open();
        let catalog = super::super::maps::Catalog::load(&mut vfs).unwrap();
        let map = catalog.find("5011").unwrap();
        let files = map.files(&mut vfs).unwrap();
        let start = files.center(Some((86, 85))).unwrap();
        let mut camera = files.region.camera(start).unwrap();
        camera.follow((0, 0));
        let mut play = WorldStream::new(vfs, map.directory.clone(), files.region, start)
            .unwrap()
            .with_starting_lighting()
            .unwrap()
            .with_player()
            .unwrap();
        let mut world = WorldStream::new(open(), map.directory.clone(), files.region, start)
            .unwrap()
            .with_lighting(15)
            .unwrap();
        let size = (1280, 720);
        play.prepare(&camera, size).unwrap();
        world.prepare(&camera, size).unwrap();
        assert_eq!(play.cache.hour, 15);
        assert!(play.reports().any(|report| report.object_lights > 0));
        assert!(!play.cache.shadows.is_empty());
        let frame = |world: &WorldStream| {
            let mut target = vec![[32, 43, 58]; (size.0 * size.1) as usize];
            crate::compositor::blit_modulated(
                &mut target,
                size,
                size.0 as usize,
                world
                    .visible_assets_at(&camera, size, Duration::from_millis(246))
                    .map(|draw| (draw.sprite, draw.modulation)),
            )
            .unwrap();
            target
        };
        let with_shadows = frame(&play);
        play.toggle_shadows();
        let without_shadows = frame(&play);
        let changed = with_shadows
            .iter()
            .zip(without_shadows)
            .filter(|(a, b)| **a != *b)
            .count();
        assert!(changed > 0, "shadows must change composed pixels");
        play.toggle_shadows();
        let player = play.cache.player.take();
        // Match visibility state for this lighting comparison. The diagnostic
        // viewer has no player tile from which to select wall runs.
        let fades = std::mem::take(&mut play.cache.faded_walls);
        assert!(!fades.is_empty(), "Bates interior should select wall fades");
        assert!(
            frame(&play) == frame(&world),
            "the lit world must compose identically at the same camera and frame tick"
        );
        play.cache.faded_walls = fades;
        play.cache.player = player;
        let base = frame(&play);
        play.cache.player_shadows = None;
        let player_shadow_pixels = base
            .iter()
            .zip(frame(&play))
            .filter(|(a, b)| **a != *b)
            .count();
        assert!(
            player_shadow_pixels > 0,
            "the generic human shadow must change composed pixels"
        );
        eprintln!(
            "Bates 15:00, fixed 246 ms: {changed} shadow pixels, including {player_shadow_pixels} attributable to the player shadow"
        );
    }

    #[test]
    #[ignore = "requires the installed original game"]
    fn player_follows_a_detour_and_cancels_routes_in_original_bates() {
        let game = std::env::var_os("ARCANUM_GAME_DIR").expect("set ARCANUM_GAME_DIR");
        let sector = Coordinates { x: 1, y: 1 };
        let region = SectorRegion {
            first: sector,
            last: sector,
        };
        let spawn = (104u32, 92u32);
        let camera = region.camera(spawn).unwrap();
        let mut world = WorldStream::new(
            Vfs::from_game(game, crate::vfs::Policy::Ce).unwrap(),
            "bates mansion lev 1".into(),
            region,
            spawn,
        )
        .unwrap()
        .with_player()
        .unwrap();
        world.prepare(&camera, (1280, 720)).unwrap();
        let entries = &world.cache.entries;
        let cell = |tile| SectorCache::collision_cell(entries, tile);
        let player = world.cache.player.as_mut().unwrap();
        let (target, planned) = (98..=109)
            .flat_map(|x| (86..=97).map(move |y| (x, y)))
            .find_map(|tile: (u32, u32)| {
                let straight = tile.0.abs_diff(spawn.0).max(tile.1.abs_diff(spawn.1));
                let steps = player.destination(tile, true, cell)?;
                (steps > straight as usize && steps < 13).then_some((tile, steps))
            })
            .expect("a real obstacle must force a detour");
        eprintln!("Bates authored entry {spawn:?} to {target:?}: detour has {planned} steps");
        let mut visited = vec![spawn];
        for tick in 0..=planned {
            player.update(Duration::from_secs(tick as u64 * 2), |tile, dir| {
                collision::step(tile, dir, cell)
            });
            if tick != 0 {
                visited.push(player.tile());
            }
        }
        assert_eq!(visited.len(), planned + 1);
        assert_eq!(player.tile(), target);
        assert!(!player.moving());
        assert_eq!(player.destination(spawn, false, cell), Some(planned));
        assert_eq!(player.destination(target, true, cell), Some(0));
        assert!(!player.moving());
        assert_eq!(player.destination(spawn, false, cell), Some(planned));
        player.cancel_route();
        assert!(!player.moving());
        assert_eq!(player.tile(), target);
    }

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
    fn sector_selection_covers_floor_rasters_and_overhang_at_viewport_corners() {
        let region = SectorRegion {
            first: Coordinates { x: 0, y: 0 },
            last: Coordinates { x: 1, y: 1 },
        };
        let reference = (63, 63);
        let frame = crate::isometric::synthetic_floor();
        for (position, size) in [
            ((0, 0), (1, 1)),
            ((-39, 20), (319, 201)),
            ((63, -33), (1280, 720)),
            ((-759, 574), (1281, 719)),
        ] {
            let required = region.needed(reference, position, size).unwrap();
            let viewport = Viewport::new(position, size);
            let mut visible = 0;
            for y in 0..128 {
                for x in 0..128 {
                    let floor = crate::isometric::floor_draw(&frame, x - 63, y - 63).unwrap();
                    let bounds = RasterBounds::new(floor);
                    let (cx, cy) = project_world_tile((x as u32, y as u32), reference).unwrap();
                    let cx = i64::from(cx);
                    let cy = i64::from(cy);
                    let overhang = RasterBounds {
                        min: (cx - OVERHANG, cy - OVERHANG),
                        max: (cx + OVERHANG, cy + OVERHANG),
                    };
                    let id = Coordinates {
                        x: x as u32 / 64,
                        y: y as u32 / 64,
                    }
                    .id()
                    .unwrap();
                    if viewport.place(floor, bounds).is_some() {
                        visible += 1;
                        assert!(required.as_slice().contains(&id), "floor ({x}, {y})");
                    }
                    if viewport.place(floor, overhang).is_some() {
                        assert!(required.as_slice().contains(&id), "overhang ({x}, {y})");
                    }
                }
            }
            assert!(visible > 0);
        }
    }

    #[test]
    fn walk_inserts_one_moving_draw_in_depth_order_and_preserves_other_deadlines() {
        use crate::{
            art::{DecodedFrame, SpriteFrame},
            content::Source,
            vfs::{Policy, Storage},
        };
        let mount_id = Vfs::new(Policy::Ordered).mount_directory(".").unwrap();
        let asset = |name: &str| {
            Arc::new(LoadedFrame {
                source: Source {
                    virtual_path: name.into(),
                    mount_id,
                    mount_path: ".".into(),
                    loose_path: None,
                    storage: Storage::Loose,
                    mod_id: None,
                },
                decoded: DecodedFrame {
                    frame: SpriteFrame {
                        width: 3,
                        height: 3,
                        hotspot: (1, 2),
                        offset: (0, 0),
                        pixels: vec![
                            if name == "scenery" {
                                [7, 91, 16, 255]
                            } else {
                                [31, 20, 10, 255]
                            };
                            9
                        ],
                    },
                    frame_count: 1,
                    rotation_count: 1,
                    stored_rotation: 0,
                    stored_palette: 0,
                    palette_key: [0; 3],
                    frames_per_second: 10,
                    action_frame: 0,
                    ignored_trailing_bytes: 0,
                },
                input_bytes: 0,
            })
        };
        let tile = (34, 32);
        let camera = Camera::rectangle(tile, (0, 0), (64, 64)).unwrap();
        let size = (320, 240);
        let standing = asset("stand");
        let walk = Arc::new(Walk::fixture(tile, &asset("walk")));
        let retained = Arc::downgrade(&walk);
        let key = (true, super::super::scenery::order(tile, (0, 0), false, 0));
        let fidget = Arc::new(DirectionalFidget::fixture(std::array::from_fn(|_| {
            FrameLoop::new(vec![Arc::clone(&standing), asset("fidget")], 10).unwrap()
        })));
        let mut rabbit = Raster::new(key, &standing, (0, 0));
        rabbit.set_fidget(fidget);
        let mut flame = Raster::new(
            (
                true,
                super::super::scenery::order((34, 33), (0, 0), false, 1),
            ),
            &asset("flame"),
            (40, 20),
        );
        flame.set_animation(
            &Arc::new(FrameLoop::new(vec![asset("flame0"), asset("flame1")], 11).unwrap()),
            0,
        );
        flame.object_light = ObjectLight::Additive;
        let scenery = Raster::new(
            (
                true,
                super::super::scenery::order((34, 33), (4, 2), false, 2),
            ),
            &asset("scenery"),
            (44, 22),
        );
        let mut cache = SectorCache::default();
        let mut sector = CachedSector::missing(0, String::new());
        sector.draws = vec![rabbit, flame, scenery];
        sector.walk = Some((key, walk));
        cache.entries.insert(0, (sector, None));
        cache.activate(&[0]);
        assert!(cache.start_fidget(Duration::ZERO));
        assert!(cache.start_walk(Duration::ZERO));
        cache.update_walk(Duration::ZERO);
        assert_eq!(
            cache.animation_delay(&camera, size, Duration::ZERO),
            Some(Duration::from_millis(41))
        );
        let tick = Duration::from_millis(246);
        let pose = cache.update_walk(tick).unwrap();
        assert_eq!(pose.anchor, (43, 22));
        let ordered: Vec<_> = cache
            .visible_assets(&camera, size, tick)
            .map(|draw| (draw.asset.source.virtual_path.as_str(), draw.sprite.anchor))
            .collect();
        assert_eq!(
            ordered,
            [
                ("flame0", (200, 140)),
                ("walk", (203, 142)),
                ("scenery", (204, 142))
            ]
        );
        assert_eq!(
            cache.animation_delay(&camera, size, tick),
            Some(Duration::from_millis(24))
        );
        let mut pixels = vec![[0; 3]; (size.0 * size.1) as usize];
        crate::compositor::blit_modulated(
            &mut pixels,
            size,
            size.0 as usize,
            cache.visible_modulated(&camera, size, tick),
        )
        .unwrap();
        assert_eq!(pixels[141 * 320 + 202], [31, 20, 10]);
        assert_eq!(pixels[141 * 320 + 203], [7, 91, 16]);
        assert_eq!(cache.update_walk(tick), None);
        assert_eq!(cache.fidget.start, Some(Duration::ZERO));
        let mut away = camera.clone();
        away.pan((-1000, 0), size);
        assert_eq!(cache.animation_delay(&away, size, tick), None);
        cache.activate(&[]);
        assert!(retained.upgrade().is_some());
        cache.activate(&[0]);
        assert_eq!(cache.update_walk(tick), Some(pose));
        let stop = Duration::from_millis(943);
        assert_eq!(cache.update_walk(stop).unwrap().frame, None);
        assert!(cache.start_fidget(stop));
        assert!(cache.turn_fidget(1).is_some());
        assert!(cache.reset_walk());
        cache.update_walk(stop);
        assert!(cache.walking.is_none());
        cache.activate(&[]);
        assert!(cache.evict(1, &[], false));
        assert!(retained.upgrade().is_none());
    }

    #[test]
    fn animated_raster_schedules_only_intersecting_frames_and_survives_shared_sector_lifetime() {
        use crate::{
            art::DecodedFrame,
            content::Source,
            vfs::{Policy, Storage},
        };
        let mount_id = Vfs::new(Policy::Ordered).mount_directory(".").unwrap();
        let fixture = |hotspot_x| {
            Arc::new(LoadedFrame {
                source: Source {
                    virtual_path: "effect.art".into(),
                    mount_id,
                    mount_path: ".".into(),
                    loose_path: None,
                    storage: Storage::Loose,
                    mod_id: None,
                },
                decoded: DecodedFrame {
                    frame: crate::art::SpriteFrame {
                        width: 2,
                        height: 1,
                        hotspot: (hotspot_x, 0),
                        offset: (0, 0),
                        pixels: vec![[30, 20, 10, 255]; 2],
                    },
                    frame_count: 3,
                    rotation_count: 1,
                    stored_rotation: 0,
                    stored_palette: 0,
                    palette_key: [0; 3],
                    frames_per_second: 11,
                    action_frame: 0,
                    ignored_trailing_bytes: 0,
                },
                input_bytes: 0,
            })
        };
        let clip = Arc::new(FrameLoop::new(vec![fixture(0), fixture(1), fixture(40)], 11).unwrap());
        let retained = Arc::downgrade(&clip.frames[2]);
        let mut raster = Raster::new(
            (true, (true, 50, 64, 0, 0, 0, None)),
            &clip.frames[0],
            (40, 0),
        );
        raster.set_animation(&clip, 0);
        raster.object_light = ObjectLight::Additive;
        assert_eq!(clip.index(1, Duration::from_millis(89)), 1);
        assert_eq!(clip.index(1, Duration::from_millis(90)), 2);
        assert_eq!(clip.index(1, Duration::from_millis(270)), 1);
        assert!(FrameLoop::new(clip.frames.to_vec(), 1001).is_err());
        drop(clip);

        let camera = Camera::rectangle((63, 50), (0, 0), (128, 64)).unwrap();
        let view = (8, 4);
        let mut cache = SectorCache::default();
        let empty = |id| CachedSector::missing(id, String::new());
        cache.select(&[0, 1], 2, |id| {
            let mut sector = empty(id);
            if id == 1 {
                sector.draws.push(raster.clone());
            }
            sector
        });
        drop(raster);
        assert_eq!(
            cache.animation_delay(&camera, view, Duration::ZERO),
            Some(Duration::from_millis(180))
        );
        let tick = Duration::from_millis(180);
        let draw = cache.visible_assets(&camera, view, tick).next().unwrap();
        assert_eq!(draw.sprite.anchor, (44, 2));
        assert!(matches!(draw.modulation, Modulation::Additive));
        assert_eq!(
            cache.animation_delay(&camera, view, tick),
            Some(Duration::from_millis(90))
        );
        assert!(
            cache
                .visible_assets(&camera, view, Duration::from_millis(179))
                .next()
                .is_none()
        );
        let mut away = camera.clone();
        away.pan((-128, 0), view);
        assert_eq!(cache.animation_delay(&away, view, tick), None);
        cache.select(&[0], 2, empty);
        assert!(retained.upgrade().is_some());
        assert_eq!(cache.animation_delay(&camera, view, tick), None);
        cache.select(&[0, 2], 2, empty);
        assert!(retained.upgrade().is_none());

        let mut fidget = Raster::new((true, (true, 50, 63, 0, 0, 1, None)), &fixture(0), (0, 0));
        let poses = Arc::new(DirectionalFidget::fixture(std::array::from_fn(|dir| {
            FrameLoop::new(vec![fixture(0), fixture(dir as i32 % 2 + 1)], 10).unwrap()
        })));
        fidget.set_fidget(poses);
        let mut flame = Raster::new((true, (true, 50, 63, 0, 0, 2, None)), &fixture(0), (0, 0));
        flame.set_animation(
            &Arc::new(FrameLoop::new(vec![fixture(0), fixture(1)], 11).unwrap()),
            0,
        );
        let mut cache = SectorCache::default();
        cache.select(&[0], 1, |id| {
            let mut sector = empty(id);
            sector.draws.extend([fidget.clone(), flame.clone()]);
            sector
        });
        let end = Duration::from_millis(200);
        assert_eq!(
            cache.animation_delay(&camera, view, Duration::ZERO),
            Some(Duration::from_millis(90))
        );
        assert!(cache.start_fidget(Duration::ZERO));
        assert_eq!(
            cache.animation_delay(&camera, view, Duration::from_millis(91)),
            Some(Duration::from_millis(9))
        );
        assert!(!cache.start_fidget(Duration::from_millis(99)));
        let elapsed = Duration::from_millis(100);
        assert_eq!(cache.turn_fidget(1), Some(4));
        let draw = cache.visible_assets(&camera, view, elapsed).next().unwrap();
        assert_eq!(draw.sprite.anchor, (4, 2));
        assert_eq!(draw.sprite.frame.hotspot, (1, 0));
        let stopped = cache.visible_assets(&camera, view, end).next().unwrap();
        assert_eq!(stopped.sprite.frame.hotspot, (0, 0));
        let viewport = Viewport::new(camera.position(), view);
        assert_eq!(fidget.next_change(&viewport, end, cache.fidget), None);
        assert_eq!(cache.animation_delay(&away, view, elapsed), None);
        assert!(cache.start_fidget(end));
    }

    #[test]
    fn moving_and_toggling_reuses_rasters_and_replaces_the_old_footprint() {
        use crate::{
            art::{DecodedFrame, SpriteFrame},
            content::Source,
            vfs::{Policy, Storage},
        };
        let mount_id = Vfs::new(Policy::Ordered).mount_directory(".").unwrap();
        let frame = |pixels| {
            Arc::new(LoadedFrame {
                source: Source {
                    virtual_path: "fixture.art".into(),
                    mount_id,
                    mount_path: ".".into(),
                    loose_path: None,
                    storage: Storage::Loose,
                    mod_id: None,
                },
                decoded: DecodedFrame {
                    frame: pixels,
                    frame_count: 1,
                    rotation_count: 1,
                    stored_rotation: 0,
                    stored_palette: 0,
                    palette_key: [0; 3],
                    frames_per_second: 8,
                    action_frame: 0,
                    ignored_trailing_bytes: 0,
                },
                input_bytes: 0,
            })
        };
        let mask = frame(SpriteFrame {
            width: 1,
            height: 1,
            hotspot: (0, 0),
            offset: (0, 0),
            pixels: vec![[255; 4]],
        });
        let floor = frame(crate::isometric::synthetic_floor());
        let reference = (63, 32);
        let from = DynamicLightState {
            tile: reference,
            offset: (0, 0),
            active: true,
        };
        let to = DynamicLightState {
            tile: (64, 32),
            ..from
        };
        let flame_frames: Vec<_> = [100, 200]
            .into_iter()
            .map(|red| {
                frame(SpriteFrame {
                    width: 1,
                    height: 1,
                    hotspot: (0, 0),
                    offset: (0, 0),
                    pixels: vec![[red, 10, 10, 255]],
                })
            })
            .collect();
        let clip = Arc::new(FrameLoop::new(flame_frames, 11).unwrap());
        let mut flame = Raster::new(
            (true, (true, reference.1, reference.0, 0, 0, 10, None)),
            &clip.frames[0],
            (0, 0),
        );
        flame.object_light = ObjectLight::Additive;
        flame.set_animation(&clip, 0);
        let shadows = [PreparedMask {
            frame: Arc::clone(&mask),
            anchor: (0, 0),
            mask: crate::shadow::Mask {
                direction: 2,
                frame: 0,
                gray: 10,
            },
        }];
        let sample_time = Duration::from_millis(95);
        let camera = Camera::rectangle(reference, (0, 0), (128, 64)).unwrap();
        let size = (64, 64);
        let check_animation = |cache: &SectorCache| {
            assert_eq!(
                cache.animation_delay(&camera, size, sample_time),
                Some(Duration::from_millis(85))
            );
            let mut draws = cache.visible_with_shadows(&camera, size, sample_time, &shadows);
            assert!(draws.any(|draw| matches!(draw.modulation, Modulation::Shadow(10))));
            let current = draws.next().unwrap();
            assert!(matches!(current.modulation, Modulation::Additive));
            assert!(Arc::ptr_eq(current.asset, &clip.frames[1]));
        };
        let mut cache = SectorCache {
            ambient: Some((
                Ambient {
                    indoor: [10; 3],
                    outdoor: [10; 3],
                },
                reference,
            )),
            hour: 12,
            ..Default::default()
        };
        cache.dynamic.push(DynamicLight {
            mask: StaticLight::new(
                mask.clone(),
                from.tile,
                from.offset,
                reference,
                [60, 20, 10],
                false,
            )
            .unwrap(),
            state: from,
            nocturnal: false,
            shadow_excluded: false,
        });
        cache.select(&[0, 1], 2, |id| {
            let mut sector = CachedSector::missing(id, String::new());
            sector.report.source = Some("synthetic base sector".into());
            let tile = (reference.0 + id as u32, reference.1);
            let (x, y) = project_world_tile(tile, reference).unwrap();
            sector.draws.push(Raster::new(
                (false, (false, tile.1, tile.0, 0, 0, 0, None)),
                &floor,
                (x, y + 19),
            ));
            if id == 0 {
                sector.draws.push(flame.clone());
                sector.lights.insert_light(
                    &StaticLight::new(
                        mask.clone(),
                        from.tile,
                        from.offset,
                        reference,
                        [200, 30, 20],
                        false,
                    )
                    .unwrap(),
                );
                sector.outdoor_rows[32] = 1 << 63;
            }
            sector
        });
        let original_geometry = cache.ordered.as_ptr();
        let centers = |cache: &SectorCache| {
            cache
                .shades
                .iter()
                .zip(&cache.ordered)
                .filter(|(_, raster)| !raster.key.0)
                .map(|(shade, _)| {
                    let Modulation::Floor(nodes) = shade.modulation() else {
                        assert!(matches!(
                            shade.modulation(),
                            Modulation::Uniform([10, 10, 10])
                        ));
                        return [10; 3];
                    };
                    nodes[4]
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(centers(&cache), [[255, 60, 40], [10; 3]]);
        check_animation(&cache);
        let moved = cache.set_dynamic(0, to).unwrap();
        check_animation(&cache);
        assert_eq!(moved.receivers, 2);
        assert_eq!(centers(&cache), [[210, 40, 30], [70, 30, 20]]);
        assert_eq!(cache.ordered.as_ptr(), original_geometry);
        let moved_shades = centers(&cache);
        assert_eq!(cache.toggle_probe_shadow(0), Some((false, false)));
        assert!(cache.dynamic[0].state.active);
        assert!(
            cache
                .shadow_candidates()
                .any(|candidate| candidate.flags & 0x40 != 0)
        );
        assert_eq!(centers(&cache), moved_shades);
        check_animation(&cache);
        assert_eq!(cache.toggle_probe_shadow(0), Some((true, false)));
        cache.refresh_shades(None);
        assert_eq!(centers(&cache), moved_shades);
        assert_eq!(cache.set_dynamic(0, to).unwrap().receivers, 0);

        let mut camera = Camera::rectangle(reference, (0, 0), (128, 64)).unwrap();
        let size = (64, 64);
        let snapshot = |c: &Camera| {
            let mut pixels = vec![[0; 3]; 64 * 64];
            crate::compositor::blit_modulated(
                &mut pixels,
                size,
                64,
                cache
                    .visible_with_shadows(c, size, sample_time, &shadows)
                    .map(|draw| (draw.sprite, draw.modulation)),
            )
            .unwrap();
            pixels
        };
        let original = snapshot(&camera);
        camera.pan((-24, 12), size);
        assert_ne!(snapshot(&camera), original);
        camera.reset(size);
        assert_eq!(snapshot(&camera), original);

        cache
            .set_dynamic(
                0,
                DynamicLightState {
                    active: false,
                    ..to
                },
            )
            .unwrap();
        assert_eq!(centers(&cache), [[210, 40, 30], [10; 3]]);
        check_animation(&cache);
        cache.dynamic[0].nocturnal = true;
        assert_eq!(cache.set_dynamic(0, from).unwrap().receivers, 0);
        assert_eq!(centers(&cache), [[210, 40, 30], [10; 3]]);
        cache.set_dynamic(0, to).unwrap(); // Indoors stays eligible by day.
        assert_eq!(centers(&cache), moved_shades);
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
