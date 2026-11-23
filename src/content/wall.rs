//! Read-only wall placement and source resolution through the VFS.

use super::{Source, art, load_mes, read_bounded, sector::read_base_sector};
use crate::{
    compositor::SpriteDraw,
    prototype::MAX_PROTOTYPE_BYTES,
    sector::{
        Coordinates,
        objects::{WallRecord, decode, decode_wall_prototype},
    },
    vfs::Vfs,
    wall::{Placement, WallArtId},
};
use std::error::Error;

pub const DEMO_RECORD: usize = 52;
pub const DEMO_ORIGIN: (u8, u8) = (34, 14);

pub struct ImportedWall {
    pub sector_source: Source,
    pub object_list_offset: usize,
    pub object_counts: (usize, usize, usize),
    pub record: WallRecord,
    pub prototype_source: Source,
    pub structure_source: Source,
    pub names_source: Source,
    pub artwork: art::LoadedFrame,
    pub placement: Placement,
    pub scale: i32,
    pub flags: u32,
    pub wall_flags: u32,
}

impl ImportedWall {
    pub fn draw(&self, center: (u32, u32)) -> Option<SpriteDraw<'_>> {
        self.placement.draw(&self.artwork.decoded.frame, center)
    }
}

/// Load one serialized wall by record index, retaining its VFS provenance.
pub fn load(
    vfs: &mut Vfs,
    map: &str,
    coordinates: Coordinates,
    record_index: usize,
) -> Result<ImportedWall, Box<dyn Error>> {
    let (source, bytes) = read_base_sector(vfs, map, coordinates)?;
    let list = decode(&bytes)
        .map_err(|error| format!("{} from {source}: {error}", source.virtual_path))?;
    let object_counts = (list.walls.len(), list.portal_count, list.scenery_count);
    let record = list
        .walls
        .into_iter()
        .find(|record| record.index == record_index)
        .ok_or_else(|| {
            format!(
                "no wall at object record {record_index} in {}",
                source.virtual_path
            )
        })?;

    let prefix = format!("proto/{:06} - ", record.prototype.0);
    let candidates: Vec<_> = vfs
        .paths_with_prefix(&prefix)
        .into_iter()
        .filter(|path| path.ends_with(".pro") && !path[prefix.len()..].contains('/'))
        .collect();
    let [path] = candidates.as_slice() else {
        return Err(format!(
            "expected one wall prototype for {}, found {candidates:?}",
            record.prototype.0
        )
        .into());
    };
    let path = (*path).to_owned();
    let (prototype_source, bytes) = read_bounded(vfs, &path, MAX_PROTOTYPE_BYTES)?;
    let parent = decode_wall_prototype(&bytes, record.prototype).map_err(|error| {
        format!(
            "{} from {prototype_source}: {error}",
            prototype_source.virtual_path
        )
    })?;
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
    let structures = load_mes(vfs, "art/wall/structure.mes")?;
    let names = load_mes(vfs, "art/wall/wallname.mes")?;
    let path = WallArtId(record.art_id).resolve(&structures.mes, &names.mes)?;
    let artwork = art::load_frame(vfs, &path, 0)?;
    if artwork.decoded.frame_count != 1 {
        return Err(format!("expected one static wall frame in {path}").into());
    }
    let placement = Placement {
        tile: record
            .world_tile(coordinates)
            .ok_or("invalid sector tile coordinate")?,
        pixel_offset: (offset_x, offset_y),
    };
    Ok(ImportedWall {
        sector_source: source,
        object_list_offset: list.start,
        object_counts,
        record,
        prototype_source,
        structure_source: structures.source,
        names_source: names.source,
        artwork,
        placement,
        scale,
        flags,
        wall_flags,
    })
}
