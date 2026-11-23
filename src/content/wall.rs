//! Read-only wall placement and source resolution through the VFS.

use super::{
    LoadedMes, Source, art, load_mes,
    sector::{read_base_sector, read_static_prototype},
};
use crate::{
    compositor::SpriteDraw,
    prototype::SerializedPrototypeId,
    sector::{
        Coordinates,
        objects::{AppearanceFields, ObjectList, StaticRecord, decode, decode_wall_prototype},
    },
    vfs::Vfs,
    wall::{ArtFields, Placement, WallArtId},
};
use std::{
    collections::{BTreeMap, BTreeSet, btree_map::Entry},
    error::Error,
    sync::Arc,
};

pub const DEMO_RECORD: usize = 52;
pub const DEMO_ORIGIN: (u8, u8) = (34, 14);
pub const CONNECTED_ORIGIN: (u8, u8) = (33, 13);
pub const CONNECTED_RECORDS: [usize; 17] = [
    30, 31, 32, 33, 34, 36, 37, 38, 39, 52, 60, 69, 77, 87, 93, 99, 107,
];

pub struct ImportedWall {
    pub sector_source: Source,
    pub object_list_offset: usize,
    pub object_counts: (usize, usize, usize),
    pub record: StaticRecord,
    pub prototype_source: Source,
    pub structure_source: Source,
    pub names_source: Source,
    /// Decoded pixels in draw orientation. The ART hotspot remains unchanged.
    pub artwork: Arc<art::LoadedFrame>,
    pub orientation: ArtFields,
    pub placement: Placement,
    pub scale: i32,
    pub flags: u32,
    pub wall_flags: u32,
}

impl ImportedWall {
    pub fn draw(&self, center: (u32, u32)) -> Option<SpriteDraw<'_>> {
        let frame = &self.artwork.decoded.frame;
        self.placement
            .draw_with_pivot(frame, center, self.orientation.pivot(frame))
    }
}

/// Load one serialized wall by record index, retaining its VFS provenance.
pub fn load(
    vfs: &mut Vfs,
    map: &str,
    coordinates: Coordinates,
    record_index: usize,
) -> Result<ImportedWall, Box<dyn Error>> {
    Ok(load_many(vfs, map, coordinates, &[record_index])?
        .pop()
        .expect("selected one wall"))
}

/// Load selected base-sector walls in the isometric upright object's draw order.
/// Selection is independent of viewport clipping. Equal order keys retain
/// serialized insertion order, even if the caller supplies IDs out of order.
pub fn load_many(
    vfs: &mut Vfs,
    map: &str,
    coordinates: Coordinates,
    indices: &[usize],
) -> Result<Vec<ImportedWall>, Box<dyn Error>> {
    if indices.is_empty() {
        return Ok(Vec::new());
    }
    if indices.len() > 64 {
        return Err("wall selection exceeds 64 records".into());
    }
    let (source, bytes) = read_base_sector(vfs, map, coordinates)?;
    let list = decode(&bytes)
        .map_err(|error| format!("{} from {source}: {error}", source.virtual_path))?;
    resolve_many(vfs, coordinates, &source, list, indices)
}

pub(super) fn resolve_many(
    vfs: &mut Vfs,
    coordinates: Coordinates,
    source: &Source,
    list: ObjectList,
    indices: &[usize],
) -> Result<Vec<ImportedWall>, Box<dyn Error>> {
    let mut requested: BTreeSet<_> = indices.iter().copied().collect();
    if requested.len() != indices.len() {
        return Err("duplicate wall record selection".into());
    }
    let records: Vec<_> = list
        .walls
        .into_iter()
        .filter(|record| requested.remove(&record.index))
        .collect();
    if !requested.is_empty() {
        return Err(format!(
            "selection {requested:?} does not refer to walls in {}",
            source.virtual_path
        )
        .into());
    }
    let mut import = WallImport::new(
        vfs,
        source,
        list.start,
        (
            list.count - list.portal_count - list.scenery_count,
            list.portal_count,
            list.scenery_count,
        ),
    )?;
    let mut walls = Vec::with_capacity(records.len());
    for record in records {
        walls.push(import.resolve(vfs, coordinates, record)?);
    }
    walls.sort_by_key(|wall| {
        (
            wall.placement.tile.1,
            wall.placement.tile.0,
            wall.orientation.order_key(),
        )
    });
    Ok(walls)
}

pub(super) struct WallImport {
    source: Source,
    start: usize,
    counts: (usize, usize, usize),
    structure: LoadedMes,
    names: LoadedMes,
    prototypes: BTreeMap<SerializedPrototypeId, (Source, AppearanceFields)>,
    artwork: BTreeMap<(String, u8, bool), Arc<art::LoadedFrame>>,
}

impl WallImport {
    pub(super) fn new(
        vfs: &mut Vfs,
        source: &Source,
        start: usize,
        counts: (usize, usize, usize),
    ) -> Result<Self, Box<dyn Error>> {
        Ok(Self {
            source: source.clone(),
            start,
            counts,
            structure: load_mes(vfs, "art/wall/structure.mes")?,
            names: load_mes(vfs, "art/wall/wallname.mes")?,
            prototypes: BTreeMap::new(),
            artwork: BTreeMap::new(),
        })
    }

    pub(super) fn resolve(
        &mut self,
        vfs: &mut Vfs,
        coordinates: Coordinates,
        record: StaticRecord,
    ) -> Result<ImportedWall, Box<dyn Error>> {
        let record_index = record.index;
        let (prototype_source, parent) = match self.prototypes.entry(record.prototype) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => entry.insert(load_prototype(vfs, record.prototype)?),
        };
        let fields = record.fields;
        let offset_x = fields
            .offset_x
            .or(parent.offset_x)
            .ok_or("missing wall X offset")?;
        let offset_y = fields
            .offset_y
            .or(parent.offset_y)
            .ok_or("missing wall Y offset")?;
        let scale = fields.scale.or(parent.scale).ok_or("missing wall scale")?;
        let flags = fields.flags.or(parent.flags).ok_or("missing wall flags")?;
        let wall_flags = fields
            .wall_flags
            .or(parent.wall_flags)
            .ok_or("missing wall subtype flags")?;
        let blit_flags = fields
            .blit_flags
            .or(parent.blit_flags)
            .ok_or("missing wall blit flags")?;
        let blit_color = fields
            .blit_color
            .or(parent.blit_color)
            .ok_or("missing wall blit color")?;
        let alpha = fields.alpha.or(parent.alpha).ok_or("missing wall alpha")?;
        // Only ordinary visible, non-flat, unscaled, opaque walls are drawn.
        // Cover, shoot-through, see-through and collision bits do not affect this pass.
        if scale != 100
            || flags & !0x4430 != 0
            || wall_flags != 0
            || blit_flags != 0
            || blit_color != 0x00ff_ffff
            || alpha != 255
        {
            return Err(format!("unsupported wall appearance at record {record_index}: scale {scale}, object flags {flags:#x}, wall flags {wall_flags:#x}, blit flags {blit_flags:#x}, color {blit_color:#x}, alpha {alpha}").into());
        }
        let art_id = WallArtId(record.art_id);
        let orientation = art_id.fields()?;
        let path = art_id
            .resolve(&self.structure.mes, &self.names.mes)
            .map_err(|error| {
                format!(
                    "wall record {record_index}, ART {:#010x}: {error}",
                    art_id.0
                )
            })?;
        let mirror = orientation.mirror_x();
        let artwork = match self
            .artwork
            .entry((path.clone(), orientation.palette, mirror))
        {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => {
                let mut frame =
                    art::read_art(vfs, &path)?.decode_with_palette(0, orientation.palette)?;
                if frame.decoded.frame_count != 1 {
                    return Err(format!("expected one static wall frame in {path}").into());
                }
                let sprite = &mut frame.decoded.frame;
                if sprite.width > 512 || sprite.height > 512 {
                    return Err(format!("wall frame exceeds 512 pixels per axis in {path}").into());
                }
                if mirror {
                    for row in sprite.pixels.chunks_exact_mut(sprite.width as usize) {
                        row.reverse();
                    }
                }
                entry.insert(Arc::new(frame))
            }
        };
        let placement = Placement {
            tile: record
                .world_tile(coordinates)
                .ok_or("invalid sector tile coordinate")?,
            pixel_offset: (offset_x, offset_y),
        };
        Ok(ImportedWall {
            sector_source: self.source.clone(),
            object_list_offset: self.start,
            object_counts: self.counts,
            record,
            prototype_source: prototype_source.clone(),
            structure_source: self.structure.source.clone(),
            names_source: self.names.source.clone(),
            artwork: Arc::clone(artwork),
            orientation,
            placement,
            scale,
            flags,
            wall_flags,
        })
    }
}

fn load_prototype(
    vfs: &mut Vfs,
    id: SerializedPrototypeId,
) -> Result<(Source, AppearanceFields), Box<dyn Error>> {
    let (source, bytes) = read_static_prototype(vfs, id)?;
    let fields = decode_wall_prototype(&bytes, id)
        .map_err(|error| format!("{} from {source}: {error}", source.virtual_path))?;
    Ok((source, fields))
}
