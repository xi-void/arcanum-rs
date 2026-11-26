//! Static scenery appearance and the bounded Bates Mansion room preview.

use super::{LoadedMes, Source, art, load_mes, portal, sector, wall};
use crate::{
    compositor::{SpriteDraw, sprite_origin},
    isometric::project_world_tile,
    mes::Mes,
    prototype::SerializedPrototypeId,
    resource_path::normalize,
    sector::{
        Coordinates,
        objects::{
            AppearanceFields, IncompleteScenery, ListEvidence, SceneryPrototype, StaticCategory,
            StaticRecord, decode, decode_scenery_prototype, decode_with_evidence,
        },
    },
    vfs::Vfs,
    wall::WallArtId,
};
use std::{
    collections::{BTreeMap, btree_map::Entry},
    error::Error,
    sync::Arc,
};

pub const ROOM_RECORDS: [usize; 7] = [35, 40, 53, 61, 70, 78, 100];
const NAMES: &str = "art/scenery/scenery.mes";
// Object metadata has its own per-sector budget. Input bytes and decoded ART
// are bounded by their readers and cache independently of this charge.
const OBJECT_PREPARATION_BUDGET: usize = 8 * 1024 * 1024;
const FLAT: u32 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SceneryArt {
    pub number: u16,
    pub family: u8,
    pub rotation: u8,
    pub frame: u8,
    pub palette: u8,
}

impl SceneryArt {
    pub fn decode(raw: u32) -> Result<Self, Box<dyn Error>> {
        // Filename selection is independent of stored direction and palette.
        if raw >> 28 != 4 || raw & 15 != 0 {
            return Err(format!("unsupported scenery ART ID {raw:#010x}").into());
        }
        Ok(Self {
            number: ((raw >> 19) & 511) as u16,
            family: ((raw >> 6) & 31) as u8,
            rotation: ((raw >> 11) & 7) as u8,
            frame: ((raw >> 14) & 31) as u8,
            palette: ((raw >> 4) & 3) as u8,
        })
    }

    pub fn resource_path(self, names: &Mes) -> Result<String, Box<dyn Error>> {
        let key = i32::from(self.family) * 1000 + i32::from(self.number);
        let name = names
            .get(key)
            .ok_or_else(|| format!("no scenery name for key {key}"))?;
        let name = normalize(name)?;
        if name.contains('/') || !name.ends_with(".art") {
            return Err(format!("invalid scenery ART filename at key {key}").into());
        }
        Ok(format!("art/scenery/{name}"))
    }
}

#[derive(Clone, Copy)]
pub(super) struct SceneryLight {
    pub(super) art_id: u32,
    pub(super) flags: Option<u32>,
    pub(super) color: Option<u32>,
}

pub struct ImportedScenery {
    pub record: StaticRecord,
    pub prototype_source: Source,
    pub prototype_art_id: Option<u32>,
    /// Present only when the original instance overrides the current ART ID.
    pub stored_art_id: Option<u32>,
    pub names_source: Source,
    pub artwork: Arc<art::LoadedFrame>,
    pub art: SceneryArt,
    pub tile: (u32, u32),
    pub appearance: SceneryAppearance,
    pub(super) animation: Option<Arc<art::FrameLoop>>,
    pub(super) light: Option<SceneryLight>,
    pub(super) use_script: i32,
}

impl ImportedScenery {
    pub(super) fn order(&self) -> DrawOrder {
        let mut key = order(
            self.tile,
            self.appearance.offset,
            self.appearance.flags & FLAT != 0,
            self.record.index,
        );
        if self.appearance.scenery_flags & 0x200 != 0 {
            // CE inserts OSCF_UNDER_ALL at the head of its tile's flat list.
            key.3 = -1;
            key.4 = -(self.record.index as i64);
        }
        key
    }

    pub fn draw(&self, center: (u32, u32)) -> Option<SpriteDraw<'_>> {
        let (x, y) = project_world_tile(self.tile, center)?;
        let (dx, dy) = self.appearance.offset;
        Some(SpriteDraw {
            frame: &self.artwork.decoded.frame,
            anchor: (x.checked_add(dx)?, y.checked_add(dy)?),
        })
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct SceneryAppearance {
    pub offset: (i32, i32),
    pub scale: i32,
    pub flags: u32,
    pub scenery_flags: u32,
    pub additive: bool,
}

impl SceneryAppearance {
    fn resolve_for_world(
        instance: AppearanceFields,
        parent: AppearanceFields,
        effects: bool,
    ) -> Result<Self, Box<dyn Error>> {
        let offset = (
            instance
                .offset_x
                .or(parent.offset_x)
                .ok_or("missing scenery X offset")?,
            instance
                .offset_y
                .or(parent.offset_y)
                .ok_or("missing scenery Y offset")?,
        );
        let scale = instance
            .scale
            .or(parent.scale)
            .ok_or("missing scenery scale")?;
        let flags = instance
            .flags
            .or(parent.flags)
            .ok_or("missing scenery flags")?;
        let scenery_flags = instance
            .scenery_flags
            .or(parent.scenery_flags)
            .ok_or("missing scenery subtype flags")?;
        let blend = (
            instance.blit_flags.or(parent.blit_flags),
            instance.blit_color.or(parent.blit_color),
            instance.alpha.or(parent.alpha),
        );
        let additive = effects && blend == (Some(0x10), Some(0x00ff_ffff), Some(255));
        let (object_mask, scenery_mask) = if additive {
            // IS_FIRE and sound size leave the stored additive draw intact.
            (0x0010_0000, 0xd0)
        } else {
            (0, 0)
        };
        // Physical interaction bits and the respawnable marker leave this base draw intact.
        if scale != 100
            || flags & !(0x0040_4c30 | FLAT | object_mask) != 0
            || scenery_flags & !(0x221 | scenery_mask) != 0
            || scenery_flags & 0x200 != 0 && flags & FLAT == 0
            || (!additive && blend != (Some(0), Some(0x00ff_ffff), Some(255)))
        {
            let disallowed_subtype_flags = scenery_flags & !(0x221 | scenery_mask);
            return Err(Box::new(super::AppearanceRejection {
                scale,
                flags,
                subtype_flags: scenery_flags,
                blend,
                disallowed_flags: flags & !(0x0040_4c30 | FLAT | object_mask),
                disallowed_subtype_flags,
                requires_flat: scenery_flags & 0x200 != 0 && flags & FLAT == 0,
                blend_rejected: !additive && blend != (Some(0), Some(0x00ff_ffff), Some(255)),
                effective_art: None,
            }));
        }
        Ok(Self {
            offset,
            scale,
            flags,
            scenery_flags,
            additive,
        })
    }
}

pub(super) fn resolve_scenery(
    vfs: &mut Vfs,
    coordinates: Coordinates,
    records: Vec<StaticRecord>,
    indices: &[usize],
) -> Result<Vec<ImportedScenery>, Box<dyn Error>> {
    if indices.len() > 64 {
        return Err("scenery selection exceeds 64 records".into());
    }
    let selected: Vec<_> = records
        .into_iter()
        .filter(|record| indices.contains(&record.index))
        .collect();
    if selected.len() != indices.len() {
        return Err(format!(
            "scenery selection must identify distinct drawable records, requested {indices:?}"
        )
        .into());
    }
    if selected.is_empty() {
        return Ok(Vec::new());
    }
    let mut importer = SceneryImport::new(vfs, false)?;
    let mut frames = art::FrameCache::default();
    selected
        .into_iter()
        .map(|record| importer.resolve(vfs, &mut frames, coordinates, record))
        .collect()
}

struct SceneryImport {
    effects: bool,
    loops: BTreeMap<(String, u8, u8), Arc<art::FrameLoop>>,
    names: LoadedMes,
    prototypes: BTreeMap<SerializedPrototypeId, (Source, SceneryPrototype)>,
}

impl SceneryImport {
    fn new(vfs: &mut Vfs, effects: bool) -> Result<Self, Box<dyn Error>> {
        Ok(Self {
            effects,
            loops: BTreeMap::new(),
            names: load_mes(vfs, NAMES)?,
            prototypes: BTreeMap::new(),
        })
    }

    fn prototype(
        &mut self,
        vfs: &mut Vfs,
        id: SerializedPrototypeId,
    ) -> Result<&(Source, SceneryPrototype), Box<dyn Error>> {
        let entry = match self.prototypes.entry(id) {
            Entry::Occupied(e) => e.into_mut(),
            Entry::Vacant(e) => {
                let (source, bytes) = sector::read_static_prototype(vfs, id)?;
                let fields = decode_scenery_prototype(&bytes, id)
                    .map_err(|err| format!("{} from {source}: {err}", source.virtual_path))?;
                e.insert((source, fields))
            }
        };
        Ok(entry)
    }

    fn resolve_inherited(
        &mut self,
        vfs: &mut Vfs,
        frames: &mut art::FrameCache,
        coordinates: Coordinates,
        record: IncompleteScenery,
        stored_tile: (u32, u32),
    ) -> Result<ImportedScenery, Box<dyn Error>> {
        let id = self
            .prototype(vfs, record.prototype)?
            .1
            .art_id
            .ok_or("scenery prototype has no current ART ID")?;
        let mut imported = self.resolve(
            vfs,
            frames,
            coordinates,
            StaticRecord {
                category: StaticCategory::Scenery,
                index: record.index,
                span: record.span,
                prototype: record.prototype,
                instance_id: record.instance_id,
                art_id: id,
                stored_tile,
                fields: record.fields,
            },
        )?;
        imported.stored_art_id = None;
        Ok(imported)
    }

    fn resolve(
        &mut self,
        vfs: &mut Vfs,
        frames: &mut art::FrameCache,
        coordinates: Coordinates,
        record: StaticRecord,
    ) -> Result<ImportedScenery, Box<dyn Error>> {
        let (source, parent) = self.prototype(vfs, record.prototype)?;
        let source = source.clone();
        let parent = *parent;
        let appearance =
            SceneryAppearance::resolve_for_world(record.fields, parent.appearance, self.effects)
                .map_err(|mut err| {
                    if let Some(rejection) = err.downcast_mut::<super::AppearanceRejection>() {
                        rejection.effective_art = Some(record.art_id);
                    }
                    err
                })?;
        let art = SceneryArt::decode(record.art_id).map_err(super::ObjectArtFailure::from_error)?;
        let path = frames
            .resolve(|| art.resource_path(&self.names.mes))
            .map_err(super::ObjectArtFailure::from_error)?;
        let artwork = frames
            .load_rotated(
                vfs,
                &path,
                usize::from(art.frame),
                art.palette,
                false,
                art.rotation,
            )
            .map_err(super::ObjectArtFailure::from_error)?;
        let frame = &artwork.decoded.frame;
        if frame.width > 512 || frame.height > 512 {
            return Err(format!("scenery frame exceeds 512 pixels per axis in {path}").into());
        }
        let animation = if appearance.additive
            && appearance.scenery_flags & 1 == 0
            && artwork.decoded.frame_count > 1
        {
            let count = artwork.decoded.frame_count;
            if count > 32 {
                return Err("scenery animation exceeds ART ID frame capacity".into());
            }
            let key = (path.clone(), art.rotation, art.palette);
            let animation = match self.loops.entry(key) {
                Entry::Occupied(entry) => Arc::clone(entry.get()),
                Entry::Vacant(entry) => {
                    let sequence = (0..count)
                        .map(|i| {
                            let frame = frames
                                .load_rotated(vfs, &path, i, art.palette, false, art.rotation)
                                .map_err(super::ObjectArtFailure::from_error)?;
                            if frame.decoded.frame.width > 512 || frame.decoded.frame.height > 512 {
                                return Err(format!(
                                    "animated scenery frame exceeds 512 pixels per axis in {path}"
                                )
                                .into());
                            }
                            Ok(frame)
                        })
                        .collect::<Result<_, Box<dyn Error>>>()?;
                    let fps = u32::try_from(artwork.decoded.frames_per_second)
                        .map_err(|_| "invalid scenery ART frame rate")?;
                    let clip = Arc::new(art::FrameLoop::new(sequence, fps)?);
                    Arc::clone(entry.insert(clip))
                }
            };
            Some(animation)
        } else {
            None
        };
        let tile = record
            .world_tile(coordinates)
            .ok_or("invalid scenery world tile")?;
        let light = record
            .fields
            .light_art
            .or(parent.appearance.light_art)
            .filter(|&id| id != u32::MAX)
            .map(|art_id| SceneryLight {
                art_id,
                flags: record.fields.light_flags.or(parent.appearance.light_flags),
                color: record.fields.light_color.or(parent.appearance.light_color),
            });
        Ok(ImportedScenery {
            use_script: record
                .fields
                .use_script
                .or(parent.appearance.use_script)
                .unwrap_or(0),
            stored_art_id: Some(record.art_id),
            record,
            prototype_source: source,
            prototype_art_id: parent.art_id,
            names_source: self.names.source.clone(),
            artwork,
            art,
            tile,
            appearance,
            animation,
            light,
        })
    }
}

pub struct Room {
    pub floor: sector::FloorPatch,
    pub walls: Vec<wall::ImportedWall>,
    pub scenery: Vec<ImportedScenery>,
    pub portals: Vec<portal::ImportedPortal>,
}

/// The preview selects base-sector records and uses their original coordinates.
pub fn load_room(vfs: &mut Vfs) -> Result<Room, Box<dyn Error>> {
    use sector::{DEMO_MAP, DEMO_SECTOR, WIDE_SIDE};
    let (source, bytes) = sector::read_base_sector(vfs, DEMO_MAP, DEMO_SECTOR)?;
    let mut objects =
        decode(&bytes).map_err(|err| format!("{} from {source}: {err}", source.virtual_path))?;
    let scenery = resolve_scenery(
        vfs,
        DEMO_SECTOR,
        std::mem::take(&mut objects.scenery),
        &ROOM_RECORDS,
    )?;
    let walls = wall::resolve_many(vfs, DEMO_SECTOR, &source, objects, &wall::CONNECTED_RECORDS)?;
    let floor = sector::load_facade_region(
        vfs,
        DEMO_MAP,
        DEMO_SECTOR,
        wall::CONNECTED_ORIGIN,
        WIDE_SIDE,
    )?;
    Ok(Room {
        floor,
        walls,
        scenery,
        portals: Vec::new(),
    })
}

/// A bounded base-sector scene and import diagnostics.
pub struct RoomOverview {
    pub room: Room,
    pub issues: Vec<SceneIssue>,
    pub culled_portals: Vec<CulledPortal>,
    pub sector_object_counts: (usize, usize, usize),
    pub outside_floor_walls: usize,
    pub outside_floor_scenery: usize,
    pub object_issue: Option<String>,
    pub(super) object_rejection: Option<(&'static str, ListEvidence)>,
    pub(super) object_preparation_bytes: usize,
    pub(super) object_ids: std::collections::BTreeSet<crate::sector::objects::OriginalId>,
    pub(super) traps: Vec<crate::sector::objects::TrapRecord>,
    pub(super) decoded_records: Option<usize>,
    pub(super) original_records: Vec<serde_json::Value>,
    pub object_list_failed: bool,
    pub(super) townmap: u32,
    pub(super) aptitude: i32,
    pub(super) sound_schemes: [i32; 2],
    pub(super) roof_words: Vec<(usize, u32)>,
    pub(super) collision: Option<super::collision::Collision>,
}

fn object_preparation_bytes(list: &crate::sector::objects::ObjectList, audit: bool) -> usize {
    use std::mem::size_of;
    let render_candidates =
        list.walls.len() + list.portals.len() + list.scenery.len() + list.incomplete_scenery.len();
    let imported = list.walls.len() * size_of::<wall::ImportedWall>()
        + list.portals.len() * size_of::<portal::ImportedPortal>()
        + (list.scenery.len() + list.incomplete_scenery.len()) * size_of::<ImportedScenery>();
    let decoded = (list.walls.capacity() + list.portals.capacity() + list.scenery.capacity())
        * size_of::<StaticRecord>()
        + list.incomplete_scenery.capacity() * size_of::<IncompleteScenery>()
        + list.traps.capacity() * size_of::<crate::sector::objects::TrapRecord>();
    // Include sector and merged draw descriptors and the alternative failure
    // path. Debug mode also retains JSON values and human-readable provenance.
    let diagnostics = if audit { 2048 * list.count } else { 0 };
    decoded
        + imported
        + render_candidates
            * (super::world::OBJECT_RENDER_DESCRIPTOR_BYTES + size_of::<SceneIssue>())
        + diagnostics
}

pub struct CulledPortal {
    pub record: usize,
    pub tile: (u32, u32),
    pub art_id: u32,
    pub art: portal::PortalArt,
    pub state: portal::PortalState,
}

pub struct SceneIssue {
    pub record: usize,
    pub category: StaticCategory,
    pub tile: (u32, u32),
    pub art_id: u32,
    pub reason: String,
    pub art_failure: bool,
    pub(super) prototype: u32,
    pub(super) appearance: Option<super::AppearanceRejection>,
    pub(super) opening: Option<OpeningRejection>,
}

pub const OVERVIEW_SIZE: (u32, u32) = (1280, 720);

/// Import a contiguous 25 × 25 surface and select visible static rasters from
/// the same original sector. Raster intersection retains overhanging objects.
pub fn load_room_overview(vfs: &mut Vfs) -> Result<RoomOverview, Box<dyn Error>> {
    load_overview(
        vfs,
        &mut art::FrameCache::default(),
        WorldRegion::BATES_MANSION,
        LoadMode::RoomPreview,
    )
}

#[derive(Clone, Copy)]
pub struct WorldRegion<'a> {
    pub map: &'a str,
    pub sector: Coordinates,
    pub origin: (u8, u8),
    pub side: u8,
    pub center: (u32, u32),
    pub viewport: (u32, u32),
}

impl WorldRegion<'static> {
    pub const BATES_MANSION: Self = Self {
        map: sector::DEMO_MAP,
        sector: sector::DEMO_SECTOR,
        origin: (25, 7),
        side: 25,
        center: (101, 83),
        viewport: OVERVIEW_SIZE,
    };
}

/// Import supported surfaces and static objects into one draw pass.
pub fn load_world(vfs: &mut Vfs, region: WorldRegion<'_>) -> Result<RoomOverview, Box<dyn Error>> {
    load_overview(
        vfs,
        &mut art::FrameCache::default(),
        region,
        LoadMode::WorldPreview,
    )
}

/// Load and retain the region and supported sector objects for subsequent panning.
pub fn load_navigation_world(
    vfs: &mut Vfs,
    region: WorldRegion<'_>,
) -> Result<RoomOverview, Box<dyn Error>> {
    load_overview(
        vfs,
        &mut art::FrameCache::default(),
        region,
        LoadMode::Navigation,
    )
}

pub(super) fn load_streamed_sector(
    vfs: &mut Vfs,
    frames: &mut art::FrameCache,
    region: WorldRegion<'_>,
    playing: bool,
    audit: bool,
    terrain: Result<Option<&super::terrain::TerrainMap>, &str>,
    template_path: Option<&str>,
) -> (Result<RoomOverview, Box<dyn Error>>, LoadStages) {
    let mut timing = LoadStages::default();
    let mode = if audit {
        LoadMode::Audited
    } else if playing {
        LoadMode::Playable
    } else {
        LoadMode::Streamed
    };
    let result = load_overview_timed(
        vfs,
        frames,
        region,
        mode,
        terrain,
        template_path,
        &mut timing,
    );
    frames.finish_sector();
    (result, timing)
}

#[derive(Clone, Copy, Debug, Default)]
pub struct LoadStages {
    pub sector_read: std::time::Duration,
    pub floor_parse: std::time::Duration,
    pub surfaces: std::time::Duration,
    pub object_parse: std::time::Duration,
    pub object_import: std::time::Duration,
    pub rasters: std::time::Duration,
    pub lighting: std::time::Duration,
    pub mobiles: std::time::Duration,
}

#[derive(Clone, Copy)]
enum LoadMode {
    RoomPreview,
    WorldPreview,
    Navigation,
    Streamed,
    Playable,
    Audited,
}

fn load_overview(
    vfs: &mut Vfs,
    frames: &mut art::FrameCache,
    region: WorldRegion<'_>,
    mode: LoadMode,
) -> Result<RoomOverview, Box<dyn Error>> {
    load_overview_timed(
        vfs,
        frames,
        region,
        mode,
        Ok(None),
        None,
        &mut LoadStages::default(),
    )
}

fn load_overview_timed(
    vfs: &mut Vfs,
    frames: &mut art::FrameCache,
    region: WorldRegion<'_>,
    mode: LoadMode,
    terrain: Result<Option<&super::terrain::TerrainMap>, &str>,
    template_path: Option<&str>,
    timing: &mut LoadStages,
) -> Result<RoomOverview, Box<dyn Error>> {
    use std::time::Instant;

    let include_portals = !matches!(mode, LoadMode::RoomPreview);
    let initial_view_only = matches!(mode, LoadMode::RoomPreview | LoadMode::WorldPreview);
    let partial = matches!(
        mode,
        LoadMode::Streamed | LoadMode::Playable | LoadMode::Audited
    );
    let WorldRegion {
        map,
        sector: coordinates,
        origin,
        side,
        center,
        viewport,
    } = region;
    let (width, height) = viewport;
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > 16 * 1024 * 1024 {
        return Err("invalid world preview viewport".into());
    }
    let start = Instant::now();
    let base = if let Some(path) = template_path {
        super::read_bounded(vfs, path, crate::sector::MAX_INPUT_BYTES).map_err(Into::into)
    } else {
        sector::read_base_sector(vfs, map, coordinates).or_else(|error| {
            let missing = matches!(
                error.downcast_ref::<super::Error>(),
                Some(super::Error::Resource {
                    error: crate::vfs::Error::NotFound(_),
                    ..
                })
            );
            if !missing {
                return Err(error);
            }
            match terrain {
                Ok(Some(terrain)) => {
                    let path = terrain
                        .template(coordinates)
                        .map_err(|reason| format!("{error}; {reason}"))?;
                    super::read_bounded(vfs, &path, crate::sector::MAX_INPUT_BYTES).map_err(
                        |reason| {
                            format!("{error}; original terrain template {path}: {reason}").into()
                        },
                    )
                }
                Ok(None) => Err(error),
                Err(reason) => Err(format!("{error}; terrain lookup unavailable: {reason}").into()),
            }
        })
    };
    timing.sector_read = start.elapsed();
    let (source, bytes) = base?;
    let start = Instant::now();
    let mut floor = sector::parse_surface(&source, &bytes)?;
    let template_lights = if source.virtual_path.starts_with("terrain/") {
        let count = floor.lights.len();
        floor.lights.clear();
        count
    } else {
        0
    };
    let crate::sector::objects::SectorProperties {
        townmap,
        aptitude,
        sound_schemes,
    } = crate::sector::objects::sector_properties(&bytes, floor.tile_offset).unwrap_or_default();
    let aptitude = if source.virtual_path.starts_with("terrain/") {
        0
    } else {
        aptitude
    };
    let roof_words = crate::sector::objects::roof_words(&bytes, floor.tile_offset)
        .unwrap_or_default()
        .iter()
        .enumerate()
        .map(|(slot, word)| (slot, u32::from_le_bytes(*word)))
        .filter(|(_, id)| *id != u32::MAX)
        .collect();
    timing.floor_parse = start.elapsed();
    let terrain = if matches!(mode, LoadMode::Playable | LoadMode::Audited) {
        let names = load_mes(vfs, "art/tile/tilename.mes")?;
        Some(super::collision::Collision::terrain(&floor, &names.mes))
    } else {
        None
    };
    let start = Instant::now();
    let mut floor = sector::resolve_surface(
        vfs,
        frames,
        (source.clone(), floor),
        coordinates,
        sector::SurfaceView {
            origin,
            side,
            include_facades: true,
            center: Some(center),
            partial,
        },
    )?;
    if template_lights > 0 {
        floor.surface_issues.push(format!(
            "{template_lights} terrain-template light records require unsupported relocation"
        ));
    }
    timing.surfaces = start.elapsed();
    let start = Instant::now();
    let (objects, mut evidence) = decode_with_evidence(&bytes);
    let mut preparation_bytes = 0;
    let objects = objects
        .map_err(|err| {
            (
                err.code,
                format!("{} from {source}: {err}", source.virtual_path),
            )
        })
        .and_then(|objects| {
            preparation_bytes = object_preparation_bytes(&objects, matches!(mode, LoadMode::Audited));
            if preparation_bytes > OBJECT_PREPARATION_BUDGET {
                evidence.preparation_bytes = Some(preparation_bytes);
                evidence.preparation_budget_bytes = Some(OBJECT_PREPARATION_BUDGET);
                Err((
                    "sector_preparation_memory_budget",
                    format!("object preparation charge {preparation_bytes} exceeds the {OBJECT_PREPARATION_BUDGET}-byte metadata budget"),
                ))
            } else {
                Ok(objects)
            }
        });
    timing.object_parse = start.elapsed();
    let start = Instant::now();
    let objects = match objects {
        Ok(objects) => objects,
        Err((code, error)) if partial => {
            return Ok(RoomOverview {
                room: Room {
                    floor,
                    walls: Vec::new(),
                    scenery: Vec::new(),
                    portals: Vec::new(),
                },
                issues: Vec::new(),
                culled_portals: Vec::new(),
                sector_object_counts: (0, 0, 0),
                outside_floor_walls: 0,
                outside_floor_scenery: 0,
                object_issue: Some(error),
                object_rejection: Some((code, evidence)),
                object_ids: Default::default(),
                object_preparation_bytes: preparation_bytes,
                traps: Vec::new(),
                decoded_records: None,
                original_records: Vec::new(),
                object_list_failed: true,
                townmap,
                aptitude,
                sound_schemes,
                roof_words,
                collision: None,
            });
        }
        Err((_, error)) => return Err(error.into()),
    };
    let object_ids = objects
        .walls
        .iter()
        .chain(&objects.scenery)
        .chain(&objects.portals)
        .map(|record| record.instance_id)
        .chain(
            objects
                .incomplete_scenery
                .iter()
                .map(|record| record.instance_id),
        )
        .map(crate::sector::objects::OriginalId::from_wire)
        .filter(|id| id.kind == 2)
        .collect();
    let decoded_records = objects.count;
    let original_records = if matches!(mode, LoadMode::Audited) {
        use serde_json::json;
        objects.walls.iter().chain(&objects.portals).chain(&objects.scenery)
            .map(|r| json!({"record":r.index,"category":r.category,"tile":r.world_tile(coordinates),
                "stored_tile":r.stored_tile,"raw_art":r.art_id,"prototype":r.prototype.0,
                "instance_id":r.instance_id,"bytes":[r.span.start,r.span.end],"overrides":r.fields}))
            .chain(objects.incomplete_scenery.iter().map(|r| json!({"record":r.index,"category":"scenery",
                "stored_tile":r.stored_tile,"tile":r.stored_tile.and_then(|(x,y)|coordinates.world_tile((x&63) as u8,(y&63) as u8)),
                "raw_art":r.art_id,"prototype":r.prototype.0,"instance_id":r.instance_id,
                "bytes":[r.span.start,r.span.end],"overrides":r.fields})))
            .chain(objects.traps.iter().map(|r| json!({"record":r.index,"category":"trap","prototype":r.prototype.0,
                "stored_tile": r.stored_tile, "tile":r.stored_tile.and_then(|(x,y)|coordinates.world_tile((x&63) as u8,(y&63) as u8))})))
            .collect()
    } else {
        Vec::new()
    };
    let collision = terrain.map(|terrain| terrain.with_objects(vfs, &objects));
    let counts = (
        objects.walls.len(),
        objects.portal_count,
        objects.scenery_count,
    );
    let framing_only = (
        counts.1 - objects.portals.len(),
        objects
            .incomplete_scenery
            .iter()
            .filter(|record| record.stored_tile.is_none())
            .count(),
    );
    let mut walls = Vec::new();
    let mut scenery = Vec::new();
    let mut issues = Vec::new();
    let mut portals = Vec::new();
    let mut culled_portals = Vec::new();
    let mut portal_import = (include_portals && !objects.portals.is_empty())
        .then(|| portal::PortalImport::new(vfs))
        .transpose()?;
    // The supported portal configuration has exactly one same-tile wall opening.
    let mut openings = BTreeMap::new();
    if portal_import.is_some() {
        for wall in &objects.walls {
            let tile = wall.world_tile(coordinates).ok_or("invalid wall tile")?;
            openings
                .entry(tile)
                .and_modify(|(count, _, _)| *count += 1)
                .or_insert((1usize, wall.index, wall.art_id));
        }
    }
    let mut wall_import = wall::WallImport::new(vfs, &source, objects.start, counts)?;
    let mut scenery_import = SceneryImport::new(vfs, partial)?;
    let first = coordinates
        .world_tile(origin.0, origin.1)
        .ok_or("invalid floor origin")?;
    let inside_floor = |tile: (u32, u32)| {
        tile.0
            .checked_sub(first.0)
            .is_some_and(|x| x < u32::from(side))
            && tile
                .1
                .checked_sub(first.1)
                .is_some_and(|y| y < u32::from(side))
    };
    let mut report =
        |(record, category, art_id, prototype), tile, inherited, error: Box<dyn Error>| {
            // Unresolved artwork has no reliable bounds. Report records on
            // the selected floor or with a tile anchor near the overview.
            let on_floor = inside_floor(tile);
            let near_view = project_world_tile(tile, center).is_some_and(|(x, y)| {
                x.abs_diff(0) < width / 2 + 128 && y.abs_diff(0) < height / 2 + 128
            });
            if !initial_view_only
                || on_floor
                || near_view
                || (include_portals && category == StaticCategory::Portal)
            {
                issues.push(SceneIssue {
                    record,
                    category,
                    tile,
                    art_id,
                    reason: if inherited {
                        format!("inherited scenery ART: {error}")
                    } else {
                        error.to_string()
                    },
                    art_failure: error.is::<super::ObjectArtFailure>(),
                    prototype,
                    appearance: error.downcast_ref::<super::AppearanceRejection>().copied(),
                    opening: error.downcast_ref::<OpeningRejection>().copied(),
                });
            }
        };
    for record in objects
        .walls
        .into_iter()
        .chain(objects.scenery)
        .chain(objects.portals)
    {
        let tile = record
            .world_tile(coordinates)
            .ok_or("invalid overview object tile")?;
        let (index, category, art_id, prototype) = (
            record.index,
            record.category,
            record.art_id,
            record.prototype.0,
        );
        let result = match category {
            StaticCategory::Wall => {
                wall_import
                    .resolve(vfs, frames, coordinates, record)
                    .map(|wall| {
                        if !initial_view_only
                            || wall
                                .draw(center)
                                .is_some_and(|draw| intersects_viewport(draw, viewport))
                        {
                            walls.push(wall);
                        }
                    })
            }
            StaticCategory::Scenery => scenery_import
                .resolve(vfs, frames, coordinates, record)
                .map(|prop| {
                    if !initial_view_only
                        || prop
                            .draw(center)
                            .is_some_and(|draw| intersects_viewport(draw, viewport))
                    {
                        scenery.push(prop);
                    }
                }),
            StaticCategory::Portal => match &mut portal_import {
                Some(importer) => matched_opening(openings.get(&tile).copied(), art_id)
                    .and_then(|wall| importer.resolve(vfs, frames, coordinates, record, wall))
                    .map(|leaf| {
                        if !initial_view_only
                            || leaf
                                .draw(center)
                                .is_some_and(|draw| intersects_viewport(draw, viewport))
                        {
                            portals.push(leaf);
                        } else {
                            culled_portals.push(CulledPortal {
                                record: index,
                                tile,
                                art_id,
                                art: leaf.art,
                                state: leaf.state,
                            });
                        }
                    }),
                None => Err("portal rendering is disabled in this preview".into()),
            },
        };
        if let Err(error) = result {
            report((index, category, art_id, prototype), tile, false, error);
        }
    }
    for record in objects.incomplete_scenery {
        let Some(stored_tile) = record.stored_tile else {
            continue;
        };
        let tile = coordinates
            .world_tile((stored_tile.0 & 63) as u8, (stored_tile.1 & 63) as u8)
            .ok_or("invalid inherited scenery tile")?;
        let (index, prototype) = (record.index, record.prototype.0);
        match scenery_import.resolve_inherited(vfs, frames, coordinates, record, stored_tile) {
            Ok(prop)
                if !initial_view_only
                    || prop
                        .draw(center)
                        .is_some_and(|draw| intersects_viewport(draw, viewport)) =>
            {
                scenery.push(prop)
            }
            Ok(_) => {}
            Err(error) => report(
                (index, StaticCategory::Scenery, 0, prototype),
                tile,
                true,
                error,
            ),
        }
    }
    issues.sort_unstable_by_key(|issue| issue.record);
    timing.object_import = start.elapsed();
    let outside = |tile: (u32, u32)| !inside_floor(tile);
    Ok(RoomOverview {
        outside_floor_walls: walls
            .iter()
            .filter(|wall| outside(wall.placement.tile))
            .count(),
        outside_floor_scenery: scenery.iter().filter(|prop| outside(prop.tile)).count(),
        room: Room {
            floor,
            walls,
            scenery,
            portals,
        },
        issues,
        culled_portals,
        sector_object_counts: counts,
        object_list_failed: false,
        townmap,
        aptitude,
        sound_schemes,
        roof_words,
        collision,
        traps: objects.traps,
        decoded_records: Some(decoded_records),
        original_records,
        object_rejection: None,
        object_ids,
        object_preparation_bytes: preparation_bytes,
        object_issue: (partial && framing_only != (0, 0)).then(|| {
            format!(
                "{} portals lack a stored ART ID or location, {} scenery records lack an instance location",
                framing_only.0, framing_only.1
            )
        }),
    })
}

fn intersects_viewport(draw: SpriteDraw<'_>, viewport: (u32, u32)) -> bool {
    let (x, y) = sprite_origin(draw.frame, draw.anchor);
    let (width, height) = (i64::from(viewport.0 / 2), i64::from(viewport.1 / 2));
    x < i64::from(viewport.0) - width
        && y < i64::from(viewport.1) - height
        && x + i64::from(draw.frame.width) > -width
        && y + i64::from(draw.frame.height) > -height
}

#[derive(Clone, Copy, Debug, serde::Serialize)]
pub(super) struct OpeningRejection {
    pub code: &'static str,
    wall_count: usize,
    wall_record: Option<usize>,
    wall_art: Option<u32>,
    wall_piece: Option<u8>,
    wall_rotation: Option<u8>,
    portal_rotation: Option<u8>,
}

impl std::fmt::Display for OpeningRejection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(if self.wall_count == 1 {
            "portal does not match the wall opening's piece and rotation"
        } else {
            "expected one same-tile wall opening for portal"
        })
    }
}

impl Error for OpeningRejection {}

fn matched_opening(
    wall: Option<(usize, usize, u32)>,
    raw_portal: u32,
) -> Result<Option<usize>, Box<dyn Error>> {
    let (count, index, raw_wall) = wall.unwrap_or_default();
    let portal_art = portal::PortalArt::decode(raw_portal)?;
    // CE registers doors as objects in their own right. A paired wall is
    // useful for ordering built-in doors, but free-standing doors have none.
    if count == 0 && portal_art.kind == portal::Kind::Door {
        return Ok(None);
    }
    let mut evidence = OpeningRejection {
        code: if count == 0 {
            "portal_without_same_tile_wall"
        } else {
            "portal_with_multiple_same_tile_walls"
        },
        wall_count: count,
        wall_record: (count == 1).then_some(index),
        wall_art: (count == 1).then_some(raw_wall),
        wall_piece: None,
        wall_rotation: None,
        portal_rotation: None,
    };
    if count != 1 {
        return Err(Box::new(evidence));
    }
    let wall_art = WallArtId(raw_wall).fields()?;
    let piece_matches = (21..=33).contains(&wall_art.piece);
    let rotation_matches = wall_art.rotation == portal_art.rotation;
    if !piece_matches || !rotation_matches {
        evidence.code = match (piece_matches, rotation_matches) {
            (false, false) => "portal_wall_piece_and_rotation_mismatch",
            (false, true) => "portal_wall_piece_outside_range",
            (true, false) => "portal_wall_rotation_mismatch",
            (true, true) => unreachable!(),
        };
        evidence.wall_piece = Some(wall_art.piece);
        evidence.wall_rotation = Some(wall_art.rotation);
        evidence.portal_rotation = Some(portal_art.rotation);
        return Err(Box::new(evidence));
    }
    Ok(Some(index))
}

pub(super) type DrawOrder = (bool, u32, u32, i64, i64, usize, Option<usize>);

pub(super) fn order(tile: (u32, u32), offset: (i32, i32), flat: bool, index: usize) -> DrawOrder {
    // CE uses truncation toward zero for half-pixels in the within-tile key.
    // Normal flat objects retain insertion order within a tile.
    let horizontal = (i64::from(offset.0) - 40) / 2;
    let vertical = 2 * (i64::from(offset.1) / 2);
    let (depth, across) = if flat {
        (0, 0)
    } else {
        (vertical + horizontal, vertical - horizontal)
    };
    (!flat, tile.1, tile.0, depth, across, index, None)
}

#[derive(Clone, Copy)]
pub(super) enum ObjectLight {
    Point,
    Wall(u8),
    Portal(u8),
    Unlit,
    Additive,
    Roof,
}

pub(super) struct ObjectAuditSource<'a> {
    pub record: &'a StaticRecord,
    pub prototype: &'a Source,
    pub stored_art: Option<u32>,
    pub rotation: u8,
    pub frame: u8,
    pub offset: (i32, i32),
    pub flags: u32,
    pub use_script: Option<i32>,
}

type SharedObject<'a> = (
    DrawOrder,
    SpriteDraw<'a>,
    &'a Arc<art::LoadedFrame>,
    ObjectLight,
    Option<(&'a Arc<art::FrameLoop>, usize)>,
);

impl Room {
    pub(super) fn audit_sources(&self) -> BTreeMap<usize, ObjectAuditSource<'_>> {
        self.walls
            .iter()
            .map(|wall| ObjectAuditSource {
                record: &wall.record,
                prototype: &wall.prototype_source,
                stored_art: Some(wall.record.art_id),
                rotation: wall.orientation.rotation,
                frame: 0,
                offset: wall.placement.pixel_offset,
                flags: wall.flags,
                use_script: wall.record.fields.use_script,
            })
            .chain(self.portals.iter().map(|portal| ObjectAuditSource {
                record: &portal.record,
                prototype: &portal.prototype_source,
                stored_art: Some(portal.record.art_id),
                rotation: portal.art.rotation,
                frame: portal.art.frame,
                offset: portal.placement.pixel_offset,
                flags: portal.appearance.flags,
                use_script: Some(portal.use_script),
            }))
            .chain(self.scenery.iter().map(|scenery| ObjectAuditSource {
                record: &scenery.record,
                prototype: &scenery.prototype_source,
                stored_art: scenery.stored_art_id,
                rotation: scenery.art.rotation,
                frame: scenery.art.frame,
                offset: scenery.appearance.offset,
                flags: scenery.appearance.flags,
                use_script: Some(scenery.use_script),
            }))
            .map(|source| (source.record.index, source))
            .collect()
    }

    pub(super) fn shared_objects(&self) -> Option<Vec<SharedObject<'_>>> {
        let center = self.floor.center;
        let mut objects =
            Vec::with_capacity(self.walls.len() + self.scenery.len() + self.portals.len());
        for wall in &self.walls {
            let tile = wall.placement.tile;
            let (depth, across) = wall.orientation.order_key();
            let key = (
                true,
                tile.1,
                tile.0,
                i64::from(depth),
                i64::from(across),
                wall.record.index,
                None,
            );
            objects.push((
                key,
                wall.draw(center)?,
                &wall.artwork,
                ObjectLight::Wall(wall.orientation.rotation),
                None,
            ));
        }
        for scenery in &self.scenery {
            let key = scenery.order();
            let light = if scenery.appearance.additive {
                ObjectLight::Additive
            } else if scenery.appearance.flags & 0x00100000 != 0 {
                ObjectLight::Unlit
            } else {
                ObjectLight::Point
            };
            objects.push((
                key,
                scenery.draw(center)?,
                &scenery.artwork,
                light,
                scenery
                    .animation
                    .as_ref()
                    .map(|clip| (clip, scenery.art.frame.into())),
            ));
        }
        for portal in &self.portals {
            let key = portal.order();
            objects.push((
                key,
                portal.draw(center)?,
                &portal.artwork,
                ObjectLight::Portal(portal.art.rotation),
                None,
            ));
        }
        Some(objects)
    }

    /// Ordered base surface followed by flat and upright object draws.
    pub fn draws(&self) -> Option<impl Iterator<Item = SpriteDraw<'_>>> {
        let mut objects = self.shared_objects()?;
        objects.sort_unstable_by_key(|(key, _, _, _, _)| *key);
        Some(
            self.floor
                .draws()
                .chain(objects.into_iter().map(|(_, draw, _, _, _)| draw)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preparation_budget_admits_dense_original_scale_but_bounds_metadata() {
        let mut record = vec![0u8; 70];
        record[0..4].copy_from_slice(&119u32.to_le_bytes());
        record[4] = 1;
        record[12..16].copy_from_slice(&4000u32.to_le_bytes());
        record[52] = 3; // Scenery.
        record[56] = 2;
        record[58] = 3; // Explicit current ART and location.
        record.extend_from_slice(&0x4000_0000u32.to_le_bytes());
        record.extend_from_slice(&[1, 0, 0, 0, 0, 0, 0, 0, 0]);
        let list = |count: u32| {
            let mut bytes = vec![0; 4 + 64 * 64 * 4];
            bytes.extend(1u32.to_le_bytes()); // No roof words.
            bytes.extend(0xaa0000u32.to_le_bytes());
            for _ in 0..count {
                bytes.extend(&record);
            }
            bytes.extend(count.to_le_bytes());
            decode(&bytes).unwrap()
        };
        let dense = list(2_000);
        assert_eq!(dense.scenery.len(), 2_000);
        assert!(object_preparation_bytes(&dense, true) < OBJECT_PREPARATION_BUDGET);
        assert!(object_preparation_bytes(&list(25_000), false) > OBJECT_PREPARATION_BUDGET);
    }

    #[test]
    fn standalone_doors_skip_wall_association_but_windows_still_require_it() {
        assert_eq!(matched_opening(None, 0x32a8_2c00).unwrap(), None);
        let error = matched_opening(None, 928524288).unwrap_err();
        let evidence = error.downcast_ref::<OpeningRejection>().unwrap();
        assert_eq!(evidence.code, "portal_without_same_tile_wall");
    }

    #[test]
    fn portal_opening_rejection_preserves_window_piece_evidence() {
        // Serialized window and its same-tile piece 17 wall, both rotation 5.
        let error = matched_opening(Some((1, 128, 300181760)), 928524288).unwrap_err();
        let evidence = error.downcast_ref::<OpeningRejection>().unwrap();
        assert_eq!(evidence.code, "portal_wall_piece_outside_range");
        assert_eq!(evidence.wall_piece, Some(17));
        assert_eq!(evidence.wall_rotation, evidence.portal_rotation);
        assert_eq!(evidence.wall_record, Some(128));
    }
    use crate::mes::parse_source;

    #[test]
    fn scenery_names_use_family_and_number_and_keep_the_frame_distinct() {
        let raw = 0x4120_0380 | (3 << 14) | (5 << 11) | (1 << 4);
        let art = SceneryArt::decode(raw).unwrap();
        assert_eq!(
            (art.family, art.number, art.rotation, art.frame, art.palette),
            (14, 36, 5, 3, 1)
        );
        let names = parse_source(b"{14036}{Couch_Flip.ART}\n{36}{Other.ART}").unwrap();
        assert_eq!(
            art.resource_path(&names.mes).unwrap(),
            "art/scenery/couch_flip.art"
        );
        let bad = parse_source(b"{14036}{../Couch.ART}").unwrap();
        assert!(art.resource_path(&bad.mes).is_err());
        assert!(SceneryArt::decode(raw | 1).is_err());
        assert!(SceneryArt::decode(0x1120_0380).is_err());
    }

    #[test]
    fn instance_presence_wins_over_prototype_even_for_zero() {
        let parent = AppearanceFields {
            offset_x: Some(-7),
            offset_y: Some(9),
            scale: Some(100),
            flags: Some(0x30),
            scenery_flags: Some(0x20),
            blit_flags: Some(0),
            blit_color: Some(0xff_ffff),
            alpha: Some(255),
            ..AppearanceFields::default()
        };
        let mut instance = AppearanceFields {
            offset_y: Some(0),
            flags: Some(0),
            ..AppearanceFields::default()
        };
        let effective = SceneryAppearance::resolve_for_world(instance, parent, false).unwrap();
        assert_eq!(effective.offset, (-7, 0));
        assert_eq!(effective.flags, 0);
        assert_eq!(effective.scenery_flags, 0x20);
        instance.scale = Some(75);
        assert!(SceneryAppearance::resolve_for_world(instance, parent, false).is_err());
        instance.scale = None;
        instance.flags = Some(0x200);
        assert!(SceneryAppearance::resolve_for_world(instance, parent, false).is_err());
    }

    #[test]
    fn flats_precede_uprights_and_ties_keep_original_insertion_order() {
        let upper_wall = (true, 80, 100, -40, 0, 4, None);
        let tile = (100, 80);
        assert!(order((200, 200), (0, 0), true, 9) < upper_wall);
        assert!(order(tile, (8, -3), false, 2) > upper_wall);
        assert!(order(tile, (8, -3), false, 2) < order(tile, (8, -3), false, 5));
        assert!(order((100, 81), (0, -100), false, 0) > order(tile, (8, -3), false, 2));
    }
}
