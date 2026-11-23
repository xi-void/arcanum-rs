//! Resolve bounded base sector surface patches through the existing VFS.

use super::{LoadedMes, Source, art, load_mes, read_bounded};
use crate::{
    art::SpriteFrame,
    compositor::SpriteDraw,
    facade::FacadeId,
    isometric::floor_draw,
    sector::{self, ArtError, Coordinates, TileArtId, TileResolution},
    vfs::Vfs,
};
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;

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
    pub tile_offset: usize,
    pub center: (u32, u32),
    tiles: Vec<PatchTile>,
    frames: BTreeMap<String, BTreeMap<usize, art::LoadedFrame>>,
    flipped: BTreeMap<String, SpriteFrame>,
}

impl FloorPatch {
    pub fn tiles(&self) -> &[PatchTile] {
        &self.tiles
    }

    pub fn artwork(&self) -> impl Iterator<Item = &art::LoadedFrame> {
        self.frames.values().flat_map(|frames| frames.values())
    }

    pub fn flipped_frame_count(&self) -> usize {
        self.flipped.len()
    }

    /// Surface draws in serialized row order, relative to the patch center.
    /// A baked facade piece occupies its own cell in this pass.
    pub fn draws(&self) -> impl Iterator<Item = SpriteDraw<'_>> {
        self.tiles.iter().map(|tile| SpriteDraw {
            frame: if tile.flip_x {
                &self.flipped[&tile.artwork]
            } else {
                &self.frames[&tile.artwork][&tile.frame_index()]
                    .decoded
                    .frame
            },
            anchor: tile.anchor,
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
    load_surface(vfs, map, coordinates, origin, side, false, None)
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
    load_surface(vfs, map, coordinates, origin, side, true, None)
}

pub(super) fn load_centered_facade_region(
    vfs: &mut Vfs,
    map: &str,
    coordinates: Coordinates,
    origin: (u8, u8),
    side: u8,
    center: (u32, u32),
) -> Result<FloorPatch, Box<dyn Error>> {
    load_surface(vfs, map, coordinates, origin, side, true, Some(center))
}

fn load_surface(
    vfs: &mut Vfs,
    map: &str,
    coordinates: Coordinates,
    origin: (u8, u8),
    side: u8,
    include_facades: bool,
    center: Option<(u32, u32)>,
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
    let (source, floor, names) = load_floor_and_names(vfs, map, coordinates)?;
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
    for y in origin.1..origin.1 + side {
        for x in origin.0..origin.0 + side {
            let world = coordinates
                .world_tile(x, y)
                .expect("validated sector and local tile");
            let id = floor.tile(x, y).expect("validated local tile");
            let facade = FacadeId::from_raw(id.0).filter(|_| include_facades);
            let (artwork, flip_x) = if let Some(facade) = facade {
                if facade_names.is_none() {
                    facade_names = Some(load_mes(vfs, "art/facade/facadename.mes")?);
                }
                let names = facade_names.as_ref().expect("facade names loaded");
                let path = facade.artwork_path(&names.mes).map_err(|error| {
                    format!(
                        "{} tile ({x}, {y}) art {:#010x}: {error} in {} from {}",
                        source.virtual_path, id.0, names.source.virtual_path, names.source
                    )
                })?;
                (path, false)
            } else {
                let resolved = id.resolve(&names.mes).map_err(|error| {
                    format!(
                        "{} tile ({x}, {y}) art {:#010x}: {error}",
                        source.virtual_path, id.0
                    )
                })?;
                (resolved.path, resolved.fields.flip_x)
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
    let mut requests = BTreeMap::<_, BTreeSet<_>>::new();
    for tile in &tiles {
        requests
            .entry(tile.artwork.as_str())
            .or_default()
            .insert(tile.frame_index());
    }
    let mut frames = BTreeMap::new();
    for (path, indices) in requests {
        let resource = art::read_art(vfs, path)?;
        let mut decoded = BTreeMap::new();
        for index in indices {
            let frame = resource.decode(index)?;
            if floor_draw(&frame.decoded.frame, 0, 0).is_none() {
                return Err(format!(
                    "{path} frame {index} from {} requires a 78 × 40 surface with hotspot (39, 39)",
                    frame.source
                )
                .into());
            }
            decoded.insert(index, frame);
        }
        frames.insert(path.to_owned(), decoded);
    }
    let mut flipped = BTreeMap::new();
    for tile in &mut tiles {
        let frame = &frames[&tile.artwork][&tile.frame_index()].decoded.frame;
        let x = (i64::from(tile.world.0) - i64::from(center.0)) as i32;
        let y = (i64::from(tile.world.1) - i64::from(center.1)) as i32;
        tile.anchor = floor_draw(frame, x, y)
            .expect("validated surface frame and bounded region")
            .anchor;
        if tile.flip_x {
            flipped
                .entry(tile.artwork.clone())
                .or_insert_with(|| mirror_rows(frame));
        }
    }
    Ok(FloorPatch {
        source,
        names_source: names.source,
        facade_names_source: facade_names.map(|names| names.source),
        light_count: floor.light_count,
        tile_offset: floor.tile_offset,
        center,
        tiles,
        frames,
        flipped,
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
fn mirror_rows(frame: &SpriteFrame) -> SpriteFrame {
    let mut pixels = frame.pixels.clone();
    for row in pixels.chunks_exact_mut(frame.width as usize) {
        row.reverse();
    }
    SpriteFrame { pixels, ..*frame }
}

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
                    Ok(frame) if floor_draw(&frame.decoded.frame, 0, 0).is_some() => {
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
    use super::*;

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
        let flipped = mirror_rows(&frame);
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
        assert_eq!(mirror_rows(&flipped), frame);
    }
}
