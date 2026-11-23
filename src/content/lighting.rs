//! Original map ambient colors and prepared static illumination.

use super::{
    Error,
    art::{FrameCache, LoadedFrame},
    load_mes, read_bounded,
};
use crate::{
    art::SpriteFrame,
    camera::RasterBounds,
    isometric::project_world_tile,
    mes::Mes,
    resource_path::normalize,
    sector::{Coordinates, LightRecord},
    vfs,
};
use std::{collections::BTreeMap, error::Error as StdError, sync::Arc};

type Rgb = [u8; 3];
type Point = (i32, i32);

#[derive(Clone, Copy, Debug)]
pub(super) struct Ambient {
    pub indoor: Rgb,
    pub outdoor: Rgb,
}

impl Ambient {
    pub fn load(vfs: &mut vfs::Vfs, map: &str, hour: u8) -> Result<Self, Box<dyn StdError>> {
        let path = format!("maps/{map}/mapinfo.txt");
        let scheme = match read_bounded(vfs, &path, 65536) {
            Ok((source, bytes)) => {
                let text = std::str::from_utf8(&bytes)
                    .map_err(|error| format!("{path} from {source}: {error}"))?;
                text.lines()
                    .filter_map(|line| line.split_once(':'))
                    .find(|(key, _)| key.trim().eq_ignore_ascii_case("LightScheme"))
                    .map_or(Ok(1), |(_, value)| value.trim().parse::<i32>())?
            }
            Err(Error::Resource {
                error: vfs::Error::NotFound(_),
                ..
            }) => 1,
            Err(error) => return Err(error.into()),
        };
        let index = load_mes(vfs, "rules/lighting schemes.mes")?;
        let name = index
            .mes
            .get(scheme)
            .ok_or("unknown map ambient light scheme")?;
        let name = normalize(name)?;
        if name.contains('/') || scheme <= 0 {
            return Err("invalid ambient light scheme name".into());
        }
        let resource = load_mes(vfs, &format!("rules/{name}.mes"))?;
        Self::at_hour(&resource.mes, hour).map_err(|error| {
            format!(
                "{} from {}: {error}",
                resource.source.virtual_path, resource.source
            )
            .into()
        })
    }

    fn at_hour(table: &Mes, hour: u8) -> Result<Self, &'static str> {
        if hour >= 24 {
            return Err("expected an ambient hour in 0..24");
        }
        let row = |hour| {
            table
                .get(i32::from(hour))
                .filter(|value| !value.trim().is_empty())
        };
        let parse = |entry: &str| {
            let mut parts = entry.split(',').map(|channel| channel.trim().parse::<u8>());
            let mut rgb = [0; 6];
            for channel in &mut rgb {
                *channel = parts
                    .next()
                    .ok_or("missing ambient channel")?
                    .map_err(|_| "invalid ambient channel")?;
            }
            if parts.next().is_some() {
                return Err("extra ambient channel");
            }
            Ok(rgb)
        };
        let rgb = if let Some(entry) = row(hour) {
            parse(entry)?
        } else {
            let (start, a) = (0..hour)
                .rev()
                .find_map(|h| row(h).map(|v| (h, v)))
                .ok_or("missing ambient interpolation start")?;
            let (end, b) = (hour + 1..24)
                .find_map(|h| row(h).map(|v| (h, v)))
                .ok_or("missing ambient interpolation end")?;
            let mut a = parse(a)?;
            let b = parse(b)?;
            for (a, b) in a.iter_mut().zip(b) {
                let value = i16::from(*a);
                let step = (i16::from(b) - value) / i16::from(end - start);
                *a = (value + step * i16::from(hour - start)) as u8;
            }
            a
        };
        Ok(Self {
            indoor: [rgb[3], rgb[4], rgb[5]],
            outdoor: [rgb[0], rgb[1], rgb[2]],
        })
    }
}

#[derive(Clone, Copy, Default)]
struct Sample {
    add: Rgb,
    subtract: Rgb,
}

#[derive(Default)]
pub(super) struct LightGrid(BTreeMap<Point, Sample>);

impl LightGrid {
    pub fn clear(&mut self) {
        self.0.clear();
    }

    pub fn merge(&mut self, other: &Self) {
        for (&point, entry) in &other.0 {
            let target = self.0.entry(point).or_default();
            add(&mut target.add, entry.add);
            add(&mut target.subtract, entry.subtract);
        }
    }

    pub fn samples(&self) -> usize {
        self.0.len()
    }

    pub fn payload_bytes(&self) -> usize {
        self.0.len() * (size_of::<Point>() + size_of::<Sample>())
    }

    pub fn surface(&self, dynamic: &Self, (x, y): Point, ambient: Rgb) -> [Rgb; 9] {
        if self.0.is_empty() && dynamic.0.is_empty() {
            return [ambient; 9];
        }
        std::array::from_fn(|i| {
            let mut color = ambient;
            let point = (
                x.checked_add((i % 3) as i32 * 40 - 40),
                y.checked_add((i / 3) as i32 * 20 - 20),
            );
            if let (Some(x), Some(y)) = point {
                let point = (x, y);
                let fixed = self.0.get(&point).copied().unwrap_or_default();
                let moving = dynamic.0.get(&point).copied().unwrap_or_default();
                add(&mut color, fixed.add);
                add(&mut color, moving.add);
                for (channel, shade) in color.iter_mut().zip(fixed.subtract) {
                    *channel = channel.saturating_sub(shade);
                }
            }
            color
        })
    }

    pub fn insert_light(&mut self, light: &StaticLight) {
        self.insert_mask(
            &light.art.decoded.frame,
            light.art.decoded.palette_key,
            light.origin,
            light.tint,
            light.dark,
        );
    }

    fn insert_mask(
        &mut self,
        mask: &SpriteFrame,
        key: Rgb,
        (left, top): Point,
        tint: Rgb,
        dark: bool,
    ) {
        let right = left + mask.width as i32;
        let bottom = top + mask.height as i32;
        for y in (ceil_multiple(top, 20)..bottom).step_by(20) {
            for x in (ceil_multiple(left, 40)..right).step_by(40) {
                if let Some(color) =
                    mask_color(mask, key, tint, (x - left) as usize, (y - top) as usize)
                    && color != [0; 3]
                {
                    let entry = self.0.entry((x, y)).or_default();
                    add(
                        if dark {
                            &mut entry.subtract
                        } else {
                            &mut entry.add
                        },
                        color,
                    );
                }
            }
        }
    }
}

fn ceil_multiple(value: i32, interval: i32) -> i32 {
    value + (interval - value.rem_euclid(interval)) % interval
}

fn add(target: &mut Rgb, rgb: Rgb) {
    for (channel, value) in target.iter_mut().zip(rgb) {
        *channel = channel.saturating_add(value);
    }
}

fn mask_color(mask: &SpriteFrame, key: Rgb, tint: Rgb, x: usize, y: usize) -> Option<Rgb> {
    let [r, g, b, alpha] = mask.pixels[y * mask.width as usize + x];
    (alpha != 0 && [r, g, b] != key).then(|| crate::compositor::multiply([r, g, b], tint))
}

pub(super) struct StaticLight {
    tile: (u32, u32),
    origin: Point,
    art: Arc<LoadedFrame>,
    tint: Rgb,
    dark: bool,
    flags: u32,
}

impl StaticLight {
    pub fn new(
        art: Arc<LoadedFrame>,
        tile: (u32, u32),
        offset: Point,
        reference: (u32, u32),
        tint: Rgb,
        dark: bool,
    ) -> Result<Self, &'static str> {
        let origin = mask_origin(tile, offset, reference, &art.decoded.frame)?;
        Ok(Self {
            tile,
            origin,
            art,
            tint,
            dark,
            flags: if dark { 2 } else { 0 },
        })
    }

    pub fn shadow_candidate(&self) -> Option<crate::shadow::Candidate> {
        let hotspot = self.art.decoded.frame.hotspot;
        Some(crate::shadow::Candidate {
            center: (
                self.origin.0.checked_add(hotspot.0)?,
                self.origin.1.checked_add(hotspot.1)?,
            ),
            tint: self.tint,
            flags: self.flags,
            owned_by_caster: false,
        })
    }

    pub fn bounds(&self) -> RasterBounds {
        let (x, y) = (i64::from(self.origin.0), i64::from(self.origin.1));
        let mask = &self.art.decoded.frame;
        RasterBounds {
            min: (x, y),
            max: (x + i64::from(mask.width), y + i64::from(mask.height)),
        }
    }

    pub fn place(
        &mut self,
        state: DynamicLightState,
        reference: (u32, u32),
    ) -> Result<(), &'static str> {
        let origin = mask_origin(state.tile, state.offset, reference, &self.art.decoded.frame)?;
        self.tile = state.tile;
        self.origin = origin;
        Ok(())
    }

    fn intersects(&self, left: i64, top: i64, right: i64, bottom: i64) -> bool {
        let (x, y) = (i64::from(self.origin.0), i64::from(self.origin.1));
        let frame = &self.art.decoded.frame;
        x < right
            && left < x + i64::from(frame.width)
            && y < bottom
            && top < y + i64::from(frame.height)
    }

    fn apply(&self, color: &mut Rgb, (x, y): (i64, i64)) {
        let x = x - i64::from(self.origin.0);
        let y = y - i64::from(self.origin.1);
        let frame = &self.art.decoded.frame;
        if x >= 0
            && y >= 0
            && x < i64::from(frame.width)
            && y < i64::from(frame.height)
            && let Some(sample) = mask_color(
                frame,
                self.art.decoded.palette_key,
                self.tint,
                x as usize,
                y as usize,
            )
        {
            if self.dark {
                for (channel, subtract) in color.iter_mut().zip(sample) {
                    *channel = channel.saturating_sub(subtract);
                }
            } else {
                add(color, sample);
            }
        }
    }
}

/// World-space position and switch state of a prepared primary light.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DynamicLightState {
    pub tile: (u32, u32),
    pub offset: (i32, i32),
    pub active: bool,
}

pub(super) fn nocturnal_active(hour: u8, outdoors: bool) -> bool {
    !outdoors || !(6..18).contains(&hour)
}

pub(super) fn point<'a>(
    lights: impl IntoIterator<Item = &'a StaticLight>,
    pos: Point,
    mut color: Rgb,
) -> Rgb {
    for light in lights {
        light.apply(&mut color, (i64::from(pos.0), i64::from(pos.1)));
    }
    color
}

pub(super) fn portal_sample(tile: (u32, u32), rotation: u8) -> ((u32, u32), i32) {
    let (x, y) = tile;
    match rotation | 1 {
        1 => (tile, 20),
        3 => ((x, y.checked_add(1).unwrap_or(y)), -20),
        5 => ((x.checked_add(1).unwrap_or(x), y), 20),
        _ => (tile, -20),
    }
}

#[derive(Clone, Copy)]
pub(super) enum WallFace {
    NorthWest,
    NorthEast,
    SouthEast,
    SouthWest,
}

impl WallFace {
    pub fn from_rotation(rotation: u8) -> Self {
        match rotation & 6 {
            0 => Self::NorthWest,
            2 => Self::NorthEast,
            4 => Self::SouthEast,
            _ => Self::SouthWest,
        }
    }

    pub fn exterior(self) -> bool {
        matches!(self, Self::NorthEast | Self::SouthEast)
    }

    fn accepts(self, wall: (u32, u32), light: (u32, u32)) -> bool {
        match self {
            Self::NorthWest => light.0 >= wall.0,
            Self::NorthEast => light.1 > wall.1,
            Self::SouthEast => light.0 > wall.0,
            Self::SouthWest => light.1 >= wall.1,
        }
    }

    fn sampling(self, center: Point, left: i64) -> (i64, i64) {
        let (dx, dy, direction) = match self {
            Self::NorthWest => (0, -20, 1),
            Self::NorthEast => (0, 19, -1),
            Self::SouthEast => (-40, 0, 1),
            Self::SouthWest => (-40, 0, -1),
        };
        (
            i64::from(center.1) + dy + (left - i64::from(center.0) - dx) * direction,
            direction,
        )
    }

    pub fn receiver(self, center: Point, left: i64, width: usize) -> RasterBounds {
        let (start_y, direction) = self.sampling(center, left);
        let padded_width = width.next_multiple_of(2);
        let end_y = start_y + (padded_width / 2 - 1) as i64 * direction;
        RasterBounds {
            min: (left, start_y.min(end_y)),
            max: (left + padded_width as i64, start_y.max(end_y) + 1),
        }
    }

    pub fn columns(
        self,
        lights: &[&StaticLight],
        tile: (u32, u32),
        center: Point,
        left: i64,
        width: usize,
        ambient: Rgb,
    ) -> Vec<Rgb> {
        // CE's wall plane is a half-slope line beginning at the raster's
        // effective left edge, including the directional/mirrored hotspot.
        let (start_y, direction) = self.sampling(center, left);
        let padded_width = width.next_multiple_of(2);
        let mut colors = vec![ambient; padded_width];
        let end_y = start_y + (padded_width / 2 - 1) as i64 * direction;
        for light in lights.iter().filter(|light| {
            self.accepts(tile, light.tile)
                && light.intersects(
                    left,
                    start_y.min(end_y),
                    left + padded_width as i64,
                    start_y.max(end_y) + 1,
                )
        }) {
            for (column, color) in colors.iter_mut().enumerate() {
                let x = left + column as i64;
                let y = start_y + (column / 2) as i64 * direction;
                light.apply(color, (x, y));
            }
        }
        // The original software path compares through the penultimate
        // sample of the even-width array before selecting uniform tint.
        if colors[..padded_width.saturating_sub(1)]
            .iter()
            .all(|&color| color == colors[0])
        {
            let uniform = colors[0];
            colors.fill(uniform);
        }
        colors.truncate(width);
        colors
    }
}

#[derive(Default)]
pub(super) struct LightImport {
    pub grid: LightGrid,
    pub sources: Vec<StaticLight>,
    pub accepted: usize,
    pub off: usize,
    pub object_lights: usize,
    pub issues: Vec<String>,
}

#[derive(Clone, Copy)]
struct PlacementContext {
    sector: Coordinates,
    reference: (u32, u32),
    ambient: Ambient,
}

pub(super) fn prepare(
    vfs: &mut vfs::Vfs,
    frames: &mut FrameCache,
    names: &Mes,
    (records, scenery): (&[LightRecord], &[super::scenery::ImportedScenery]),
    sector: Coordinates,
    reference: (u32, u32),
    ambient: Ambient,
) -> LightImport {
    const MAX_RECORDS: usize = 256;
    let mut import = LightImport::default();
    let context = PlacementContext {
        sector,
        reference,
        ambient,
    };
    for (index, &record) in records.iter().take(MAX_RECORDS).enumerate() {
        match import_one(vfs, frames, names, record, context) {
            Ok(Some(light)) => {
                import.insert(light);
                import.accepted += 1;
            }
            Ok(None) => import.off += 1,
            Err(error) => import.issues.push(format!(
                "light {index} at {:?} ART {:#010x}: {error}",
                record.tile, record.art_id
            )),
        }
    }
    if records.len() > MAX_RECORDS {
        import.issues.push(format!(
            "{} lights exceed the {MAX_RECORDS}-record preparation limit",
            records.len() - MAX_RECORDS
        ));
    }
    for object in scenery {
        let Some(light) = object.light else { continue };
        let result = (|| {
            let flags = light.flags.ok_or("missing object light flags")?;
            if flags & 1 != 0 {
                return Ok(None);
            }
            if flags & !0x0800_0080 != 0 {
                return Err(format!("unsupported object light flags {flags:#x}").into());
            }
            let color = light.color.ok_or("missing object light color")?;
            let [blue, green, red, _] = color.to_le_bytes();
            let placement = LightRecord {
                owner: 0,
                tile: object.tile,
                offset: object.appearance.offset,
                flags: 0,
                art_id: light.art_id,
                rgb: [red, green, blue],
            };
            let mask = import_one(vfs, frames, names, placement, context)?
                .ok_or("expected an active object-owned light")?;
            if mask.art.decoded.frame_count != 1 {
                return Err("animated object-owned light masks are unsupported".into());
            }
            Ok::<_, Box<dyn StdError>>(Some(mask))
        })();
        match result {
            Ok(Some(mask)) => {
                import.insert(mask);
                import.object_lights += 1;
            }
            Ok(None) => {}
            Err(error) => import.issues.push(format!(
                "scenery record {} light {:#010x}: {error}",
                object.record.index, light.art_id
            )),
        }
    }
    import
}

impl LightImport {
    fn insert(&mut self, light: StaticLight) {
        self.grid.insert_light(&light);
        self.sources.push(light);
    }
}

pub(super) fn prepare_dynamic(
    vfs: &mut vfs::Vfs,
    frames: &mut FrameCache,
    names: &Mes,
    record: LightRecord,
    reference: (u32, u32),
    ambient: Ambient,
) -> Result<StaticLight, Box<dyn StdError>> {
    let sector = Coordinates {
        x: record.tile.0 >> 6,
        y: record.tile.1 >> 6,
    };
    let mask = import_one(
        vfs,
        frames,
        names,
        record,
        PlacementContext {
            sector,
            reference,
            ambient,
        },
    )?
    .ok_or("expected a valid dynamic light mask")?;
    if mask.art.decoded.frame_count != 1 {
        return Err("animated dynamic masks are outside the positional preview".into());
    }
    Ok(mask)
}

fn import_one(
    vfs: &mut vfs::Vfs,
    frames: &mut FrameCache,
    names: &Mes,
    record: LightRecord,
    PlacementContext {
        sector,
        reference,
        ambient,
    }: PlacementContext,
) -> Result<Option<StaticLight>, Box<dyn StdError>> {
    if record.flags & 1 != 0 {
        return Ok(None);
    }
    if record.owner != 0 || record.flags & !0x5a != 0 || record.flags & 0x18 == 0x18 {
        return Err(format!(
            "unsupported static light owner or flags {:#x}",
            record.flags
        )
        .into());
    }
    if record.art_id & 0xf007ffff != 0x90000000 {
        return Err("expected a base light mask with frame and rotation zero".into());
    }
    if (record.tile.0 >> 6, record.tile.1 >> 6) != (sector.x, sector.y) {
        return Err("light location does not belong to its base sector".into());
    }
    let name = names
        .get(((record.art_id >> 19) & 511) as i32)
        .ok_or("missing light ART name")?;
    let name = normalize(name)?;
    if name.contains('/') {
        return Err("light ART name must be a basename".into());
    }
    let art = frames.load(vfs, &format!("art/light/{name}.art"), 0, 0, false)?;
    let tint = match record.flags & 0x18 {
        8 => ambient.indoor,
        16 => ambient.outdoor,
        _ => record.rgb,
    };
    let mut light = StaticLight::new(
        art,
        record.tile,
        record.offset,
        reference,
        tint,
        record.flags & 2 != 0,
    )?;
    light.flags = record.flags;
    Ok(Some(light))
}

fn mask_origin(
    tile: (u32, u32),
    offset: Point,
    reference: (u32, u32),
    mask: &SpriteFrame,
) -> Result<Point, &'static str> {
    let (x, y) = project_world_tile(tile, reference).ok_or("light tile projection overflow")?;
    let bounds = |origin: i32, offset: i32, hot: i32, size: u32| {
        let local = i64::from(offset) - i64::from(hot);
        let end = local + i64::from(size);
        // Fits the same conservative margin used to select sector rasters.
        if local < -640 || end > 640 {
            return Err("light mask exceeds the 640px sector overhang bound");
        }
        let left = i64::from(origin) + local;
        i32::try_from(left)
            .and_then(|left| i32::try_from(left as i64 + i64::from(size) + 40).map(|_| left))
            .map_err(|_| "light mask bounds overflow")
    };
    let left = bounds(x, offset.0, mask.hotspot.0, mask.width)?;
    let top = bounds(y, offset.1, mask.hotspot.1, mask.height)?;
    Ok((left, top))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nocturnal_rule_uses_six_and_eighteen_with_indoor_exemption() {
        for hour in 0..24 {
            assert!(nocturnal_active(hour, false));
            assert_eq!(nocturnal_active(hour, true), !(6..18).contains(&hour));
        }
    }

    #[test]
    fn ambient_interpolates_per_channel_with_truncated_signed_steps() {
        let table =
            Mes::parse_text("{0}{100, 10, 20, 31, 60, 70}{1}{}{3}{108, 20, 10, 20, 60, 72}")
                .unwrap();
        let ambient = Ambient::at_hour(&table, 2).unwrap();
        assert_eq!(ambient.outdoor, [104, 16, 14]);
        assert_eq!(ambient.indoor, [25, 60, 70]);
        assert_eq!(Ambient::at_hour(&table, 3).unwrap().outdoor, [108, 20, 10]);
        assert!(Ambient::at_hour(&table, 4).is_err());
        assert!(Ambient::at_hour(&table, 24).is_err());
        let malformed = Mes::parse_text("{0}{100, -1, 20, 31, 60, 70}").unwrap();
        assert!(Ambient::at_hour(&malformed, 0).is_err());
    }

    #[test]
    fn overlapping_masks_cross_sector_edge_and_survive_grid_replacement() {
        // Tile (63,0) owns a mask whose sample lands over (64,0).
        let left = project_world_tile((63, 0), (64, 0)).unwrap();
        assert_eq!(left, (40, -20));
        let mut east = LightGrid::default();
        let mut west = LightGrid::default();
        let mask = SpriteFrame {
            width: 41,
            height: 1,
            hotspot: (0, 0),
            offset: (0, 0),
            pixels: vec![[200, 80, 100, 255]; 41],
        };
        // Left edge is at (0,0), with another lattice point at (40,0).
        west.insert_mask(
            &mask,
            [0, 0, 0],
            (left.0 - 40, left.1 + 20),
            [255, 128, 0],
            false,
        );
        east.insert_mask(&mask, [0, 0, 0], (0, 0), [255, 255, 255], false);
        east.insert_mask(&mask, [0, 0, 0], (0, 0), [0, 0, 128], true);
        let ambient = [55, 60, 90];
        let mut merged = LightGrid::default();
        merged.merge(&west);
        merged.merge(&east);
        assert_eq!(
            merged.surface(&LightGrid::default(), (0, 0), ambient)[4],
            [255, 180, 140]
        );
        merged.clear();
        merged.merge(&east);
        merged.merge(&west);
        assert_eq!(
            merged.surface(&LightGrid::default(), (0, 0), ambient)[4],
            [255, 180, 140]
        );
        merged.clear();
        merged.merge(&east);
        drop(west);
        assert_eq!(
            merged.surface(&LightGrid::default(), (0, 0), ambient)[4],
            [255, 140, 140]
        );
        assert_eq!(
            merged.surface(&LightGrid::default(), (-80, -40), ambient),
            [ambient; 9]
        );
    }

    #[test]
    fn masked_indices_and_matching_palette_key_colors_do_not_illuminate() {
        let mask = SpriteFrame {
            width: 41,
            height: 1,
            hotspot: (0, 0),
            offset: (0, 0),
            pixels: (0..41)
                .map(|x| if x == 40 { [0, 0, 255, 255] } else { [0; 4] })
                .collect(),
        };
        let mut grid = LightGrid::default();
        grid.insert_mask(&mask, [0, 0, 255], (-40, 0), [255; 3], false);
        assert_eq!(grid.samples(), 0);
        assert_eq!(ceil_multiple(-41, 40), -40);
        assert_eq!(ceil_multiple(-40, 40), -40);
    }

    #[test]
    fn point_and_wall_use_the_same_mask_but_different_receivers() {
        let mount_id = vfs::Vfs::new(vfs::Policy::Ordered)
            .mount_directory(".")
            .unwrap();
        let art = Arc::new(LoadedFrame {
            source: super::super::Source {
                virtual_path: "fixture.art".into(),
                mount_id,
                mount_path: ".".into(),
                loose_path: None,
                storage: vfs::Storage::Loose,
                mod_id: None,
            },
            decoded: crate::art::DecodedFrame {
                frame: SpriteFrame {
                    width: 4,
                    height: 2,
                    hotspot: (0, 0),
                    offset: (0, 0),
                    pixels: [10, 20, 30, 40, 50, 60, 70, 80]
                        .map(|i| [i, 0, 0, 255])
                        .to_vec(),
                },
                frame_count: 1,
                rotation_count: 1,
                stored_rotation: 0,
                stored_palette: 0,
                palette_key: [0; 3],
                frames_per_second: 0,
                action_frame: 0,
                ignored_trailing_bytes: 0,
            },
            input_bytes: 0,
        });
        let light = StaticLight {
            tile: (9, 8),
            origin: (0, -1),
            art: Arc::clone(&art),
            tint: [255; 3],
            dark: false,
            flags: 0,
        };
        let behind = StaticLight {
            tile: (9, 7),
            ..light
        };
        let front = StaticLight {
            tile: (9, 8),
            art,
            ..behind
        };
        let ambient = [1, 2, 3];
        let lights = [&front, &behind];
        assert_eq!(point(lights, (0, 0), ambient), [101, 2, 3]);
        let face = WallFace::from_rotation(7);
        assert_eq!(
            face.columns(&lights, (9, 8), (40, 0), 0, 4, ambient),
            [[51, 2, 3], [61, 2, 3], [31, 2, 3], [41, 2, 3]]
        );
        assert_eq!(portal_sample((8, 8), 2), ((8, 9), -20));
        assert_eq!(portal_sample((8, 8), 5), ((9, 8), 20));
    }

    #[test]
    fn invalid_placements_and_unsupported_records_are_reported_individually() {
        let record = LightRecord {
            owner: 0,
            tile: (128, 15),
            offset: (0, 0),
            flags: 0,
            art_id: 0x90580000,
            rgb: [255; 3],
        };
        let sector = Coordinates { x: 1, y: 0 };
        let mut vfs = vfs::Vfs::new(vfs::Policy::Ordered);
        let report = prepare(
            &mut vfs,
            &mut FrameCache::default(),
            &Mes::parse_text("{11}{largelight}").unwrap(),
            (
                &[
                    record,
                    LightRecord {
                        flags: 0x20,
                        ..record
                    },
                    LightRecord { flags: 1, ..record },
                ],
                &[],
            ),
            sector,
            (64, 0),
            Ambient {
                indoor: [74, 80, 98],
                outdoor: [255; 3],
            },
        );
        assert_eq!(
            (report.accepted, report.off, report.grid.samples()),
            (0, 1, 0)
        );
        assert_eq!(report.issues.len(), 2);
        assert!(report.issues[0].contains("does not belong"));
        assert!(report.issues[1].contains("flags 0x20"));
    }
}
