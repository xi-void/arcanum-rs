//! Original stored roof rasters for the static roof inspection pass.

use super::{
    art::{FrameCache, LoadedFrame},
    load_mes,
};
use crate::{
    isometric::project_world_tile, resource_path::normalize, sector::Coordinates, vfs::Vfs,
};
use std::{error::Error, sync::Arc};

pub(super) struct Roof {
    pub tile: (u32, u32),
    pub anchor: (i32, i32),
    pub asset: Arc<LoadedFrame>,
}

pub(super) fn prepare(
    vfs: &mut Vfs,
    frames: &mut FrameCache,
    sector: Coordinates,
    reference: (u32, u32),
    words: &[(usize, u32)],
) -> (Vec<Roof>, Vec<String>) {
    if words.is_empty() {
        return (Vec::new(), Vec::new());
    }
    let names = match load_mes(vfs, "art/roof/roofname.mes") {
        Ok(names) => names,
        Err(err) => return (Vec::new(), vec![format!("roof names: {err}")]),
    };
    let mut roofs = Vec::new();
    let mut issues = Vec::new();
    for &(slot, id) in words {
        let mut load = || -> Result<Option<Roof>, Box<dyn Error>> {
            if id >> 28 != 10 || id & 0x0000_0fee != 0 {
                return Err("unsupported roof ART word".into());
            }
            if id & 0x2000 != 0 {
                return Ok(None); // CE fill cells have no roof draw.
            }
            if id & 0x1000 != 0 {
                return Err("stored faded roof requires CE fade interpolation".into());
            }
            let index = ((id >> 19) & 511) as usize;
            let frame = ((id >> 14) & 31) as usize;
            let mirror = id & 1 != 0;
            let palette = ((id >> 4) & 3) as u8;
            if (!mirror && (frame > 8 || palette != 0)) || (mirror && (frame > 3 || palette != 1)) {
                return Err("unsupported roof piece or palette".into());
            }
            let name = names.mes.entries().nth(index).ok_or("missing roof name")?.1;
            let name = normalize(name)?;
            if name.contains('/') {
                return Err("invalid roof basename".into());
            }
            let path = format!("art/roof/{name}.art");
            let asset = frames.load(vfs, &path, frame, palette, mirror)?;
            let raster = &asset.decoded.frame;
            if raster.width > 512 || raster.height > 512 {
                return Err("roof raster exceeds 512 pixels per axis".into());
            }
            let x = ((slot % 16) * 4 + 2) as u8;
            let y = ((slot / 16) * 4 + 2) as u8;
            let tile = sector.world_tile(x, y).ok_or("invalid roof tile")?;
            let (cx, cy) = project_world_tile(tile, reference).ok_or("roof projection overflow")?;
            // CE location_xy is 40 px left and 20 px above the tile center.
            // roof_xy subtracts another (120,200). TIG uses x hotspot zero for a flipped roof.
            let anchor = (
                cx.checked_sub(160)
                    .and_then(|x| x.checked_add(if mirror { raster.hotspot.0 } else { 0 }))
                    .ok_or("roof X overflow")?,
                cy.checked_sub(220).ok_or("roof Y overflow")?,
            );
            Ok(Some(Roof {
                tile,
                anchor,
                asset,
            }))
        };
        match load() {
            Ok(Some(roof)) => roofs.push(roof),
            Ok(None) => {}
            Err(err) => issues.push(format!("roof slot {slot} ART {id:#010x}: {err}")),
        }
    }
    (roofs, issues)
}
