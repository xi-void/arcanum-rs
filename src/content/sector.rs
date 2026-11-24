//! Resolve bounded base sector surface patches through the existing VFS.

use super::{LoadedMes, Source, art, load_mes, read_bounded};
use crate::{
    compositor::SpriteDraw,
    facade::FacadeId,
    isometric::surface_draw,
    sector::{self, ArtError, Coordinates, TileArtId, TileResolution},
    vfs::Vfs,
};
use std::collections::{BTreeMap, BTreeSet};
use std::{error::Error, sync::Arc};

pub const PATCH_SIDE: u8 = 5;
pub const WIDE_SIDE: u8 = 9;
const MAX_FACADE_SIDE: u8 = sector::SIDE as u8;
pub const WIDE_ORIGIN: (u8, u8) = (20, 13);
pub const DEMO_MAP: &str = "bates mansion lev 1";
pub const DEMO_SECTOR: Coordinates = Coordinates { x: 1, y: 1 };
pub const DEMO_ORIGIN: (u8, u8) = (29, 32);
pub const FACADE_ORIGIN: (u8, u8) = (35, 14);

pub struct PatchTile {
    pub world: (u32, u32),
    pub id: TileArtId,
    pub artwork: String,
    pub flip_x: bool,
    pub facade: Option<FacadeId>,
    anchor: (i32, i32),
}

impl PatchTile {
    pub fn frame_index(&self) -> usize {
        self.facade.map_or(0, |id| usize::from(id.fields().frame))
    }
}

pub struct FloorPatch {
    pub source: Source,
    pub names_source: Source,
    pub facade_names_source: Option<Source>,
    pub light_count: u32,
    pub lights: Vec<sector::LightRecord>,
    pub tile_offset: usize,
    pub center: (u32, u32),
    pub(super) outdoor_rows: [u64; 64],
    tiles: Vec<PatchTile>,
    frames: BTreeMap<String, BTreeMap<usize, Arc<art::LoadedFrame>>>,
    flipped: BTreeMap<String, Arc<art::LoadedFrame>>,
    pub surface_issues: Vec<String>,
}

impl FloorPatch {
    pub fn tiles(&self) -> &[PatchTile] {
        &self.tiles
    }

    pub fn artwork(&self) -> impl Iterator<Item = &art::LoadedFrame> {
        self.frames
            .values()
            .flat_map(|frames| frames.values().map(Arc::as_ref))
    }

    pub fn flipped_frame_count(&self) -> usize {
        self.flipped.len()
    }

    /// Surface draws in serialized row order, relative to the patch center.
    /// A baked facade piece occupies its own cell in this pass.
    pub fn draws(&self) -> impl Iterator<Item = SpriteDraw<'_>> {
        self.shared_draws().map(|(_, frame, anchor)| SpriteDraw {
            frame: &frame.decoded.frame,
            anchor,
        })
    }

    pub(super) fn shared_draws(
        &self,
    ) -> impl Iterator<Item = ((u32, u32), &Arc<art::LoadedFrame>, (i32, i32))> {
        self.tiles.iter().map(|tile| {
            let frame = if tile.flip_x {
                &self.flipped[&tile.artwork]
            } else {
                &self.frames[&tile.artwork][&tile.frame_index()]
            };
            (tile.world, frame, tile.anchor)
        })
    }
}

/// Import a 5 × 5 region contained in one explicit base map sector.
/// Whole-file overrides follow the supplied VFS's policy.
pub fn load_patch(
    vfs: &mut Vfs,
    map: &str,
    coordinates: Coordinates,
    origin: (u8, u8),
) -> Result<FloorPatch, Box<dyn Error>> {
    load_region(vfs, map, coordinates, origin, PATCH_SIDE)
}

/// Import a square region, up to 9 × 9, contained in one base sector.
pub fn load_region(
    vfs: &mut Vfs,
    map: &str,
    coordinates: Coordinates,
    origin: (u8, u8),
    side: u8,
) -> Result<FloorPatch, Box<dyn Error>> {
    load_surface(vfs, map, coordinates, origin, side, false)
}

/// Import a bounded region containing floor tiles and baked facade pieces.
/// Facade previews defined by standalone walkmasks are outside this path.
pub fn load_facade_region(
    vfs: &mut Vfs,
    map: &str,
    coordinates: Coordinates,
    origin: (u8, u8),
    side: u8,
) -> Result<FloorPatch, Box<dyn Error>> {
    load_surface(vfs, map, coordinates, origin, side, true)
}

fn load_surface(
    vfs: &mut Vfs,
    map: &str,
    coordinates: Coordinates,
    origin: (u8, u8),
    side: u8,
    include_facades: bool,
) -> Result<FloorPatch, Box<dyn Error>> {
    let (source, bytes) = read_base_sector(vfs, map, coordinates)?;
    let floor = parse_surface(&source, &bytes)?;
    resolve_surface(
        vfs,
        &mut art::FrameCache::default(),
        (source, floor),
        coordinates,
        SurfaceView {
            origin,
            side,
            include_facades,
            center: None,
            partial: false,
        },
    )
}

pub(super) struct SurfaceView {
    pub origin: (u8, u8),
    pub side: u8,
    pub include_facades: bool,
    pub center: Option<(u32, u32)>,
    pub partial: bool,
}

pub(super) fn parse_surface(
    source: &Source,
    bytes: &[u8],
) -> Result<sector::Floor, Box<dyn Error>> {
    sector::decode_floor(bytes)
        .map_err(|err| format!("{} from {source}: {err}", source.virtual_path).into())
}

pub(super) fn resolve_surface(
    vfs: &mut Vfs,
    cache: &mut art::FrameCache,
    (source, floor): (Source, sector::Floor),
    coordinates: Coordinates,
    SurfaceView {
        origin,
        side,
        include_facades,
        center,
        partial,
    }: SurfaceView,
) -> Result<FloorPatch, Box<dyn Error>> {
    let max_side = if include_facades {
        MAX_FACADE_SIDE
    } else {
        WIDE_SIDE
    };
    if !(1..=max_side).contains(&side) {
        return Err(format!("sector region side must be between 1 and {max_side}").into());
    }
    let limit = sector::SIDE as u8 - side;
    if origin.0 > limit || origin.1 > limit {
        return Err("floor patch crosses the sector boundary".into());
    }
    let names = load_mes(vfs, "art/tile/tilename.mes")?;
    let midpoint = (origin.0 + side / 2, origin.1 + side / 2);
    let center = center.unwrap_or_else(|| {
        coordinates
            .world_tile(midpoint.0, midpoint.1)
            .expect("validated sector and midpoint")
    });
    if (center.0 >> 6, center.1 >> 6) != (coordinates.x, coordinates.y) {
        return Err("scene center must be in the selected sector".into());
    }
    let mut tiles = Vec::with_capacity(usize::from(side).pow(2));
    let mut facade_names: Option<LoadedMes> = None;
    let mut surface_issues = Vec::new();
    let mut resolved = BTreeMap::new();
    for y in origin.1..origin.1 + side {
        for x in origin.0..origin.0 + side {
            let world = coordinates
                .world_tile(x, y)
                .expect("validated sector and local tile");
            let id = floor.tile(x, y).expect("validated local tile");
            let facade = FacadeId::from_raw(id.0).filter(|_| include_facades);
            let value = resolved.entry(id).or_insert_with(|| {
                cache.resolve(|| {
                    if let Some(facade) = facade {
                        if facade_names.is_none() {
                            match load_mes(vfs, "art/facade/facadename.mes") {
                                Ok(names) => facade_names = Some(names),
                                Err(error) => return Err(error.to_string()),
                            }
                        }
                        facade
                            .artwork_path(&facade_names.as_ref().unwrap().mes)
                            .map(|path| (path, false))
                            .map_err(|e| e.to_string())
                    } else {
                        id.resolve(&names.mes)
                            .map(|tile| (tile.path, tile.fields.flip_x))
                            .map_err(|e| e.to_string())
                    }
                })
            });
            let (artwork, flip_x) = match value {
                Ok((path, flip)) => (path.clone(), *flip),
                Err(error) if partial => {
                    let message = format!("tile ART {:#010x}: {error}", id.0);
                    if !surface_issues.contains(&message) {
                        surface_issues.push(message);
                    }
                    continue;
                }
                Err(error) => {
                    return Err(format!(
                        "{} tile ({x}, {y}) art {:#010x}: {error}",
                        source.virtual_path, id.0
                    )
                    .into());
                }
            };
            tiles.push(PatchTile {
                world,
                id,
                artwork,
                flip_x,
                facade,
                anchor: (0, 0),
            });
        }
    }
    let mut requests = BTreeSet::new();
    for tile in &tiles {
        requests.insert((tile.artwork.as_str(), tile.frame_index(), false));
        if tile.flip_x {
            requests.insert((tile.artwork.as_str(), tile.frame_index(), true));
        }
    }
    let mut frames: BTreeMap<String, BTreeMap<usize, Arc<art::LoadedFrame>>> = BTreeMap::new();
    let mut flipped = BTreeMap::new();
    for (path, index, mirror) in requests {
        let result = cache.load(vfs, path, index, 0, mirror).and_then(|frame| {
            if surface_draw(&frame.decoded.frame, 0, 0).is_none() {
                return Err(format!(
                    "{path} frame {index} from {} requires a 78 × 40 surface",
                    frame.source
                )
                .into());
            }
            Ok(frame)
        });
        match result {
            Ok(frame) if mirror => {
                flipped.insert(path.to_owned(), frame);
            }
            Ok(frame) => {
                frames
                    .entry(path.to_owned())
                    .or_default()
                    .insert(index, frame);
            }
            Err(error) if partial => surface_issues.push(error.to_string()),
            Err(error) => return Err(error),
        }
    }
    tiles.retain_mut(|tile| {
        let Some(frame) = frames
            .get(&tile.artwork)
            .and_then(|frames| frames.get(&tile.frame_index()))
        else {
            return false;
        };
        if tile.flip_x && !flipped.contains_key(&tile.artwork) {
            return false;
        }
        let x = (i64::from(tile.world.0) - i64::from(center.0)) as i32;
        let y = (i64::from(tile.world.1) - i64::from(center.1)) as i32;
        tile.anchor = surface_draw(&frame.decoded.frame, x, y)
            .expect("validated surface frame and bounded region")
            .anchor;
        true
    });
    let mut outdoor_rows = [0; 64];
    for (index, id) in floor.ids().enumerate() {
        let outdoor = match id.0 >> 28 {
            0 => id.0 & (1 << 8) != 0,
            11 => id.0 & (1 << 25) != 0,
            _ => false,
        };
        outdoor_rows[index / 64] |= u64::from(outdoor) << (index % 64);
    }
    Ok(FloorPatch {
        source,
        names_source: names.source,
        facade_names_source: facade_names.map(|names| names.source),
        light_count: floor.light_count,
        lights: floor.lights,
        tile_offset: floor.tile_offset,
        center,
        outdoor_rows,
        tiles,
        frames,
        flipped,
        surface_issues,
    })
}

fn load_floor_and_names(
    vfs: &mut Vfs,
    map: &str,
    coordinates: Coordinates,
) -> Result<(Source, sector::Floor, LoadedMes), Box<dyn Error>> {
    let (source, bytes) = read_base_sector(vfs, map, coordinates)?;
    let floor = sector::decode_floor(&bytes)
        .map_err(|error| format!("{} from {source}: {error}", source.virtual_path))?;
    let names = load_mes(vfs, "art/tile/tilename.mes")?;
    Ok((source, floor, names))
}

pub(super) fn read_base_sector(
    vfs: &mut Vfs,
    map: &str,
    coordinates: Coordinates,
) -> Result<(Source, Vec<u8>), Box<dyn Error>> {
    if map.is_empty() || matches!(map, "." | "..") || map.contains(['/', '\\']) {
        return Err("expected a single map directory name".into());
    }
    let id = coordinates
        .id()
        .ok_or("sector coordinates exceed 26 bits")?;
    let path = format!("maps/{map}/{id}.sec");
    Ok(read_bounded(vfs, &path, sector::MAX_INPUT_BYTES)?)
}

pub(super) fn read_static_prototype(
    vfs: &mut Vfs,
    id: crate::prototype::SerializedPrototypeId,
) -> Result<(Source, Vec<u8>), Box<dyn Error>> {
    let prefix = format!("proto/{:06} - ", id.0);
    let candidates: Vec<_> = vfs
        .paths_with_prefix(&prefix)
        .into_iter()
        .filter(|path| path.ends_with(".pro") && !path[prefix.len()..].contains('/'))
        .collect();
    let [path] = candidates.as_slice() else {
        return Err(format!(
            "expected one static prototype for {}, found {candidates:?}",
            id.0
        )
        .into());
    };
    let path = (*path).to_owned();
    Ok(read_bounded(
        vfs,
        &path,
        crate::prototype::MAX_PROTOTYPE_BYTES,
    )?)
}

// Floor rectangles keep their origin. A mirrored frame reverses only the
// pixels within each row, including its transparent edge.

pub struct AuditEntry {
    pub id: TileArtId,
    pub occurrences: usize,
    pub resolution: Result<TileResolution, ArtError>,
}

pub enum ArtworkCheck {
    Ready(Source),
    Missing,
    Invalid(String),
}

pub struct FloorAudit {
    pub source: Source,
    pub names_source: Source,
    pub entries: Vec<AuditEntry>,
    pub artwork: BTreeMap<String, ArtworkCheck>,
}

pub fn audit_floor(
    vfs: &mut Vfs,
    map: &str,
    coordinates: Coordinates,
) -> Result<FloorAudit, Box<dyn Error>> {
    let (source, floor, names) = load_floor_and_names(vfs, map, coordinates)?;
    let mut counts = BTreeMap::new();
    for id in floor.ids() {
        *counts.entry(id).or_insert(0) += 1;
    }
    let mut artwork = BTreeMap::new();
    let mut entries = Vec::with_capacity(counts.len());
    for (id, occurrences) in counts {
        let resolution = id.resolve(&names.mes);
        if let Ok(tile) = &resolution {
            artwork.entry(tile.path.clone()).or_insert_with(|| {
                match art::load_frame(vfs, &tile.path, 0) {
                    Ok(frame) if surface_draw(&frame.decoded.frame, 0, 0).is_some() => {
                        ArtworkCheck::Ready(frame.source)
                    }
                    Ok(_) => ArtworkCheck::Invalid(
                        "unsupported floor frame dimensions or hotspot".into(),
                    ),
                    Err(super::Error::Resource {
                        error: crate::vfs::Error::NotFound(_),
                        ..
                    }) => ArtworkCheck::Missing,
                    Err(error) => ArtworkCheck::Invalid(error.to_string()),
                }
            });
        }
        entries.push(AuditEntry {
            id,
            occurrences,
            resolution,
        });
    }
    Ok(FloorAudit {
        source,
        names_source: names.source,
        entries,
        artwork,
    })
}

#[cfg(test)]
mod tests {
    use super::art::mirror_rows;
    use crate::art::SpriteFrame;

    #[test]
    fn mirrors_individual_rows_without_changing_anchor_or_alpha_values() {
        let frame = SpriteFrame {
            width: 3,
            height: 2,
            hotspot: (1, 1),
            offset: (2, 4),
            pixels: vec![
                [1, 0, 0, 255],
                [2, 0, 0, 128],
                [0, 0, 0, 0],
                [3, 0, 0, 255],
                [4, 0, 0, 255],
                [5, 0, 0, 255],
            ],
        };
        let mut flipped = SpriteFrame {
            pixels: frame.pixels.clone(),
            ..frame
        };
        mirror_rows(&mut flipped);
        assert_eq!(
            flipped.pixels,
            [
                frame.pixels[2],
                frame.pixels[1],
                frame.pixels[0],
                frame.pixels[5],
                frame.pixels[4],
                frame.pixels[3]
            ]
        );
        assert_eq!(flipped.hotspot, frame.hotspot);
        assert_eq!(flipped.offset, frame.offset);
        mirror_rows(&mut flipped);
        assert_eq!(flipped, frame);
    }
}
