//! Static scenery appearance and the bounded Bates Mansion room preview.

use super::{LoadedMes, Source, art, load_mes, sector, wall};
use crate::{
    compositor::{SpriteDraw, sprite_origin},
    isometric::project_world_tile,
    mes::Mes,
    prototype::SerializedPrototypeId,
    resource_path::normalize,
    sector::{
        Coordinates,
        objects::{
            AppearanceFields, StaticCategory, StaticRecord, decode, decode_scenery_prototype,
        },
    },
    vfs::Vfs,
};
use std::{
    collections::{BTreeMap, btree_map::Entry},
    error::Error,
    sync::Arc,
};

pub const ROOM_RECORDS: [usize; 7] = [35, 40, 53, 61, 70, 78, 100];
const NAMES: &str = "art/scenery/scenery.mes";
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
        // The selected scenery uses pre-oriented files with ordinary blending.
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

pub struct ImportedScenery {
    pub record: StaticRecord,
    pub prototype_source: Source,
    pub names_source: Source,
    pub artwork: Arc<art::LoadedFrame>,
    pub art: SceneryArt,
    pub tile: (u32, u32),
    pub appearance: SceneryAppearance,
}

impl ImportedScenery {
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
}

impl SceneryAppearance {
    fn resolve(
        instance: AppearanceFields,
        parent: AppearanceFields,
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
        // Physical interaction bits and the respawnable marker leave this base draw intact.
        if scale != 100
            || flags & !(0x0040_4c30 | FLAT) != 0
            || scenery_flags & !0x21 != 0
            || blend != (Some(0), Some(0x00ff_ffff), Some(255))
        {
            return Err(format!("unsupported scenery appearance: scale {scale}, flags {flags:#x}, subtype flags {scenery_flags:#x}, blend {blend:?}").into());
        }
        Ok(Self {
            offset,
            scale,
            flags,
            scenery_flags,
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
    let mut importer = SceneryImport::new(vfs)?;
    selected
        .into_iter()
        .map(|record| importer.resolve(vfs, coordinates, record))
        .collect()
}

struct SceneryImport {
    names: LoadedMes,
    prototypes: BTreeMap<SerializedPrototypeId, (Source, AppearanceFields)>,
    frames: BTreeMap<(String, u8, u8), Arc<art::LoadedFrame>>,
}

impl SceneryImport {
    fn new(vfs: &mut Vfs) -> Result<Self, Box<dyn Error>> {
        Ok(Self {
            names: load_mes(vfs, NAMES)?,
            prototypes: BTreeMap::new(),
            frames: BTreeMap::new(),
        })
    }

    fn resolve(
        &mut self,
        vfs: &mut Vfs,
        coordinates: Coordinates,
        record: StaticRecord,
    ) -> Result<ImportedScenery, Box<dyn Error>> {
        let index = record.index;
        let (source, parent) = match self.prototypes.entry(record.prototype) {
            Entry::Occupied(e) => e.into_mut(),
            Entry::Vacant(e) => {
                let (source, bytes) = sector::read_static_prototype(vfs, record.prototype)?;
                let fields = decode_scenery_prototype(&bytes, record.prototype)
                    .map_err(|err| format!("{} from {source}: {err}", source.virtual_path))?;
                e.insert((source, fields))
            }
        };
        let appearance = SceneryAppearance::resolve(record.fields, *parent)
            .map_err(|err| format!("scenery record {index}: {err}"))?;
        let art = SceneryArt::decode(record.art_id)?;
        let path = art.resource_path(&self.names.mes)?;
        let artwork = match self.frames.entry((path.clone(), art.frame, art.palette)) {
            Entry::Occupied(e) => e.into_mut(),
            Entry::Vacant(e) => {
                let frame = art::read_art(vfs, &path)?
                    .decode_with_palette(usize::from(art.frame), art.palette)?;
                let sprite = &frame.decoded.frame;
                if sprite.width > 512 || sprite.height > 512 {
                    return Err(
                        format!("scenery frame exceeds 512 pixels per axis in {path}").into(),
                    );
                }
                e.insert(Arc::new(frame))
            }
        };
        let tile = record
            .world_tile(coordinates)
            .ok_or("invalid scenery world tile")?;
        Ok(ImportedScenery {
            record,
            prototype_source: source.clone(),
            names_source: self.names.source.clone(),
            artwork: Arc::clone(artwork),
            art,
            tile,
            appearance,
        })
    }
}

pub struct Room {
    pub floor: sector::FloorPatch,
    pub walls: Vec<wall::ImportedWall>,
    pub scenery: Vec<ImportedScenery>,
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
    })
}

/// Fixed overview of the original sector, clipped to a 1280 × 720 canvas.
pub struct RoomOverview {
    pub room: Room,
    pub issues: Vec<SceneIssue>,
    pub sector_object_counts: (usize, usize, usize),
    pub outside_floor_walls: usize,
    pub outside_floor_scenery: usize,
}

pub struct SceneIssue {
    pub record: usize,
    pub category: StaticCategory,
    pub tile: (u32, u32),
    pub art_id: u32,
    pub reason: String,
}

pub const OVERVIEW_SIZE: (u32, u32) = (1280, 720);

/// Import a contiguous 25 × 25 surface and select visible static rasters from
/// the same original sector. Raster intersection retains overhanging objects.
pub fn load_room_overview(vfs: &mut Vfs) -> Result<RoomOverview, Box<dyn Error>> {
    use sector::{DEMO_MAP, DEMO_SECTOR};
    let (source, bytes) = sector::read_base_sector(vfs, DEMO_MAP, DEMO_SECTOR)?;
    let objects =
        decode(&bytes).map_err(|err| format!("{} from {source}: {err}", source.virtual_path))?;
    if objects.count > 512 {
        return Err("room overview exceeds 512 original object records".into());
    }
    let floor = sector::load_facade_region(vfs, DEMO_MAP, DEMO_SECTOR, (25, 7), 25)?;
    let center = floor.center;
    let counts = (
        objects.walls.len(),
        objects.portal_count,
        objects.scenery_count,
    );
    let mut walls = Vec::new();
    let mut scenery = Vec::new();
    let mut issues = Vec::new();
    let mut wall_import = wall::WallImport::new(vfs, &source, objects.start, counts)?;
    let mut scenery_import = SceneryImport::new(vfs)?;
    let mut report = |record, category, tile, art_id, reason| {
        // Unresolved artwork has no reliable bounds. Report records on
        // the selected floor or with a tile anchor near the overview.
        let on_floor = floor.tiles().iter().any(|cell| cell.world == tile);
        let near_view = project_world_tile(tile, center).is_some_and(|(x, y)| {
            x.abs_diff(0) < OVERVIEW_SIZE.0 / 2 + 128 && y.abs_diff(0) < OVERVIEW_SIZE.1 / 2 + 128
        });
        if on_floor || near_view {
            issues.push(SceneIssue {
                record,
                category,
                tile,
                art_id,
                reason,
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
            .world_tile(DEMO_SECTOR)
            .ok_or("invalid overview object tile")?;
        let (index, category, art_id) = (record.index, record.category, record.art_id);
        let result = match category {
            StaticCategory::Wall => wall_import.resolve(vfs, DEMO_SECTOR, record).map(|wall| {
                if wall.draw(center).is_some_and(intersects_overview) {
                    walls.push(wall);
                }
            }),
            StaticCategory::Scenery => {
                scenery_import
                    .resolve(vfs, DEMO_SECTOR, record)
                    .map(|prop| {
                        if prop.draw(center).is_some_and(intersects_overview) {
                            scenery.push(prop);
                        }
                    })
            }
            StaticCategory::Portal => Err("portal rendering is unsupported".into()),
        };
        if let Err(error) = result {
            report(index, category, tile, art_id, error.to_string());
        }
    }
    issues.sort_unstable_by_key(|issue| issue.record);
    let outside = |tile| !floor.tiles().iter().any(|cell| cell.world == tile);
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
        },
        issues,
        sector_object_counts: counts,
    })
}

fn intersects_overview(draw: SpriteDraw<'_>) -> bool {
    let (x, y) = sprite_origin(draw.frame, draw.anchor);
    let (width, height) = (
        i64::from(OVERVIEW_SIZE.0 / 2),
        i64::from(OVERVIEW_SIZE.1 / 2),
    );
    x < width
        && y < height
        && x + i64::from(draw.frame.width) > -width
        && y + i64::from(draw.frame.height) > -height
}

type DrawOrder = (bool, u32, u32, i64, i64, usize);

fn order(tile: (u32, u32), offset: (i32, i32), flat: bool, index: usize) -> DrawOrder {
    // CE uses truncation toward zero for half-pixels in the within-tile key.
    // Normal flat objects retain insertion order within a tile.
    let horizontal = (i64::from(offset.0) - 40) / 2;
    let vertical = 2 * (i64::from(offset.1) / 2);
    let (depth, across) = if flat {
        (0, 0)
    } else {
        (vertical + horizontal, vertical - horizontal)
    };
    (!flat, tile.1, tile.0, depth, across, index)
}

impl Room {
    /// Ordered base surface followed by flat and upright object draws.
    pub fn draws(&self) -> Option<impl Iterator<Item = SpriteDraw<'_>>> {
        let center = self.floor.center;
        let mut objects = Vec::with_capacity(self.walls.len() + self.scenery.len());
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
            );
            objects.push((key, wall.draw(center)?));
        }
        for scenery in &self.scenery {
            let key = order(
                scenery.tile,
                scenery.appearance.offset,
                scenery.appearance.flags & FLAT != 0,
                scenery.record.index,
            );
            objects.push((key, scenery.draw(center)?));
        }
        objects.sort_unstable_by_key(|(key, _)| *key);
        Some(
            self.floor
                .draws()
                .chain(objects.into_iter().map(|(_, draw)| draw)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
        let effective = SceneryAppearance::resolve(instance, parent).unwrap();
        assert_eq!(effective.offset, (-7, 0));
        assert_eq!(effective.flags, 0);
        assert_eq!(effective.scenery_flags, 0x20);
        instance.scale = Some(75);
        assert!(SceneryAppearance::resolve(instance, parent).is_err());
        instance.scale = None;
        instance.flags = Some(0x200);
        assert!(SceneryAppearance::resolve(instance, parent).is_err());
    }

    #[test]
    fn flats_precede_uprights_and_ties_keep_original_insertion_order() {
        let upper_wall = (true, 80, 100, -40, 0, 4);
        let tile = (100, 80);
        assert!(order((200, 200), (0, 0), true, 9) < upper_wall);
        assert!(order(tile, (8, -3), false, 2) > upper_wall);
        assert!(order(tile, (8, -3), false, 2) < order(tile, (8, -3), false, 5));
        assert!(order((100, 81), (0, -100), false, 0) > order(tile, (8, -3), false, 2));
    }
}
