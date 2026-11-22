//! Resolve a bounded base sector floor patch through the existing VFS.

use super::{Source, art, load_mes, read_bounded};
use crate::{
    compositor::SpriteDraw,
    isometric::floor_draw,
    sector::{self, Coordinates, TileArtId},
    vfs::Vfs,
};
use std::collections::BTreeMap;
use std::error::Error;

pub const PATCH_SIDE: u8 = 5;
pub const DEMO_MAP: &str = "bates mansion lev 1";
pub const DEMO_SECTOR: Coordinates = Coordinates { x: 1, y: 1 };
pub const DEMO_ORIGIN: (u8, u8) = (29, 32);

pub struct PatchTile {
    pub world: (u32, u32),
    pub id: TileArtId,
    pub artwork: String,
    anchor: (i32, i32),
}

pub struct FloorPatch {
    pub source: Source,
    pub names_source: Source,
    pub light_count: u32,
    pub tile_offset: usize,
    pub center: (u32, u32),
    tiles: Vec<PatchTile>,
    frames: BTreeMap<String, art::LoadedFrame>,
}

impl FloorPatch {
    pub fn tiles(&self) -> &[PatchTile] {
        &self.tiles
    }

    pub fn artwork(&self) -> impl Iterator<Item = &art::LoadedFrame> {
        self.frames.values()
    }

    /// Floor-only draws in serialized row order, relative to the patch center.
    pub fn draws(&self) -> impl Iterator<Item = SpriteDraw<'_>> {
        self.tiles.iter().map(|tile| SpriteDraw {
            frame: &self.frames[&tile.artwork].decoded.frame,
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
    if map.is_empty() || matches!(map, "." | "..") || map.contains(['/', '\\']) {
        return Err("expected a single map directory name".into());
    }
    let id = coordinates
        .id()
        .ok_or("sector coordinates exceed 26 bits")?;
    let limit = sector::SIDE as u8 - PATCH_SIDE;
    if origin.0 > limit || origin.1 > limit {
        return Err("floor patch crosses the sector boundary".into());
    }
    let midpoint = (origin.0 + PATCH_SIDE / 2, origin.1 + PATCH_SIDE / 2);
    let center = coordinates
        .world_tile(midpoint.0, midpoint.1)
        .expect("validated sector and midpoint");
    let path = format!("maps/{map}/{id}.sec");
    let (source, bytes) = read_bounded(vfs, &path, sector::MAX_INPUT_BYTES)?;
    let floor = sector::decode_floor(&bytes)
        .map_err(|error| format!("{} from {source}: {error}", source.virtual_path))?;
    drop(bytes);
    let names = load_mes(vfs, "art/tile/tilename.mes")?;
    let mut tiles = Vec::with_capacity(usize::from(PATCH_SIDE).pow(2));
    let mut frames = BTreeMap::new();
    for y in origin.1..origin.1 + PATCH_SIDE {
        for x in origin.0..origin.0 + PATCH_SIDE {
            let world = coordinates
                .world_tile(x, y)
                .expect("validated sector and local tile");
            let id = floor.tile(x, y).expect("validated local tile");
            let artwork = id.base_path(&names.mes).map_err(|error| {
                format!(
                    "{} tile ({x}, {y}) art {:#010x}: {error}",
                    source.virtual_path, id.0
                )
            })?;
            let loaded = match frames.entry(artwork.clone()) {
                std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(art::load_frame(vfs, &artwork, 0)?)
                }
            };
            let draw = floor_draw(
                &loaded.decoded.frame,
                i32::from(x) - i32::from(midpoint.0),
                i32::from(y) - i32::from(midpoint.1),
            )
            .ok_or_else(|| {
                format!("{artwork} requires a 78 × 40 floor frame with hotspot (39, 39)")
            })?;
            tiles.push(PatchTile {
                world,
                id,
                artwork,
                anchor: draw.anchor,
            });
        }
    }
    Ok(FloorPatch {
        source,
        names_source: names.source,
        light_count: floor.light_count,
        tile_offset: floor.tile_offset,
        center,
        tiles,
        frames,
    })
}
