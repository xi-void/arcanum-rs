//! Authored world and town atlas metadata and indexed BMP tiles.
use super::{load_mes, maps::Catalog, read_bounded};
use crate::{
    isometric::project_world_tile, resource_path::normalize, sector::Coordinates, vfs::Vfs,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
};

pub type Point = (u32, u32);

#[derive(Debug)]
pub struct Area {
    pub id: i32,
    pub name: String,
    pub description: String,
    pub tile: Point,
    pub label_offset: (i32, i32),
    radius: Option<u32>,
}

impl Area {
    pub fn pixel(&self, width: u32) -> Option<Point> {
        world_pixel(self.tile, width)
    }
}

pub fn world_pixel(tile: Point, width: u32) -> Option<Point> {
    Some((width.checked_sub(tile.0 / 64)?, tile.1 / 64))
}

/// A selection identifies an authored destination. The sector flag describes an explicit authored resource. Route eligibility and arrival collision belong to travel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    pub area: i32,
    pub map: i32,
    pub tile: Point,
    pub sector_available: bool,
}

pub struct Atlas {
    pub catalog: Catalog,
    pub areas: BTreeMap<i32, Area>,
    pub discovered: BTreeSet<i32>,
    pub destination: Option<Selection>,
    towns: BTreeMap<u32, (String, bool)>,
    keyed_maps: BTreeMap<i32, i32>,
    pub offset: (i32, i32),
    pub start_map: i32,
}

impl Atlas {
    pub fn load(vfs: &mut Vfs) -> Result<Self, Box<dyn Error>> {
        let catalog = Catalog::load(vfs)?;
        let start_map = catalog.new_game()?.id;
        let table = load_mes(vfs, "mes/gamearea.mes")?;
        let mut areas = BTreeMap::new();
        for (id, raw) in table.mes.entries().filter(|(id, _)| *id > 0) {
            let area =
                parse_area(id, raw).map_err(|err| format!("{} row {id}: {err}", table.source))?;
            areas.insert(id, area);
        }
        let towns = if vfs.resolve("rules/townmap.mes")?.winner().is_some() {
            load_mes(vfs, "rules/townmap.mes")?
                .mes
                .entries()
                .filter_map(|(id, raw)| {
                    let id = u32::try_from(id).ok().filter(|id| *id > 0)?;
                    let lower = raw.to_ascii_lowercase();
                    let name = raw[..lower.find("[w:").unwrap_or(raw.len())].trim();
                    Some((id, (name.to_owned(), lower.contains("[w:1]"))))
                })
                .collect()
        } else {
            BTreeMap::new()
        };
        let world = load_mes(vfs, "worldmap/worldmap.mes")?;
        let offset = world
            .mes
            .get(20)
            .ok_or("missing world atlas offsets")?
            .split_once(',')
            .ok_or("invalid world atlas offsets")?;
        // CE's MapKeyedTo is a one-based index into consecutive MapList
        // rows starting at 5000. The native catalog exposes the MES IDs.
        let keyed_maps = world
            .mes
            .entries()
            .filter_map(|(row, fields)| {
                let index = row.checked_sub(50).filter(|&i| i >= 0)?;
                let keyed = fields
                    .split(',')
                    .filter_map(|field| field.split_once(':'))
                    .find(|(key, _)| key.trim().eq_ignore_ascii_case("MapKeyedTo"))?;
                let ce_index = keyed.1.trim().parse::<i32>().ok().filter(|&i| i > 0)?;
                let id = 4999i32.checked_add(ce_index)?;
                let valid = catalog
                    .maps
                    .iter()
                    .map(|map| map.id)
                    .zip(5000..=id)
                    .take_while(|(found, expected)| found == expected)
                    .count()
                    == ce_index as usize;
                valid.then_some((index, id))
            })
            .collect();
        let discovered = BTreeSet::new();
        let mut atlas = Self {
            catalog,
            areas,
            discovered,
            destination: None,
            towns,
            keyed_maps,
            offset: (offset.0.trim().parse()?, offset.1.trim().parse()?),
            start_map,
        };
        let start = atlas.catalog.new_game()?.map_list_center;
        atlas.discover_near(start);
        Ok(atlas)
    }

    pub fn travel_available_from(&self, world_id: i32, map_id: i32, town_id: u32) -> bool {
        // The original travel action arrives in the module's START_MAP.
        town_id == 0 && map_id == self.start_map && self.keyed_maps.get(&world_id) == Some(&map_id)
    }

    pub fn discover_all(&mut self) {
        self.discovered.extend(self.areas.keys().copied());
    }

    pub fn nearby_area(&self, tile: Point) -> Option<&Area> {
        self.areas
            .values()
            .filter_map(|area| {
                let distance = area
                    .tile
                    .0
                    .abs_diff(tile.0)
                    .max(area.tile.1.abs_diff(tile.1));
                (distance <= area.radius?).then_some((distance, area))
            })
            .min_by_key(|(distance, area)| (*distance, area.id))
            .map(|(_, area)| area)
    }

    pub fn discover_near(&mut self, tile: Point) {
        if let Some(area) = self.nearby_area(tile) {
            self.discovered.insert(area.id);
        }
    }

    /// CE AntiTeleport.mes uses map numbers and contiguous sector-radius regions.
    pub fn check_teleport(
        &self,
        vfs: &mut Vfs,
        map: i32,
        from: Option<Point>,
        to: Option<Point>,
    ) -> Result<(), Box<dyn Error>> {
        let path = "rules/antiteleport.mes";
        if vfs.resolve(path)?.winner().is_none() {
            return Ok(());
        }
        let rules = super::load_mes(vfs, path)?.mes;
        for line in (1000..).map_while(|i| rules.get(i)) {
            let n: i32 = line.trim().parse()?;
            let id = if n > 5000 {
                n
            } else {
                n.checked_add(5000)
                    .ok_or("AntiTeleport map number overflow")?
            };
            if id == map {
                return Err("Teleportation is restricted on this map".into());
            }
        }
        let from = from.ok_or("The current map has no teleportable world area")?;
        for line in (100..).map_while(|i| rules.get(i)) {
            let mut parts = line.split(',').map(str::trim);
            let x: u32 = parts
                .next()
                .ok_or("Missing anti-teleport region X")?
                .parse()?;
            let y: u32 = parts
                .next()
                .ok_or("Missing anti-teleport region Y")?
                .parse()?;
            let r: u32 = parts
                .next()
                .ok_or("Missing anti-teleport radius")?
                .parse()?;
            let radius = r.checked_mul(64).ok_or("Anti-teleport radius overflow")?;
            if [Some(from), to]
                .into_iter()
                .flatten()
                .any(|p| x.abs_diff(p.0).max(y.abs_diff(p.1)) < radius)
            {
                return Err("Teleportation is restricted in this region".into());
            }
        }
        Ok(())
    }

    pub fn select(&mut self, id: i32, vfs: &Vfs) -> Result<(), Box<dyn Error>> {
        if !self.discovered.contains(&id) {
            return Err("destination is undiscovered".into());
        }
        let area = self.areas.get(&id).ok_or("unknown destination")?;
        let from = self.catalog.new_game()?;
        let sector = Coordinates {
            x: area.tile.0 / 64,
            y: area.tile.1 / 64,
        };
        let path = format!(
            "maps/{}/{}.sec",
            from.directory,
            sector
                .id()
                .ok_or("destination sector outside original format")?
        );
        self.destination = Some(Selection {
            area: id,
            map: from.id,
            tile: area.tile,
            sector_available: vfs.resolve(&path)?.winner().is_some(),
        });
        Ok(())
    }

    pub fn local(&self, vfs: &mut Vfs, id: u32) -> Result<Option<Town>, Box<dyn Error>> {
        let Some((name, waitable)) = self.towns.get(&id) else {
            return if id == 0 {
                Ok(None)
            } else {
                Err(format!("town map {id} has no rules/townmap.mes entry").into())
            };
        };
        let dir = normalize(name)?;
        let path = format!("townmap/{dir}/{dir}.tmi");
        let (source, data) = read_bounded(vfs, &path, 48)?;
        Town::parse(id, name, dir.clone(), *waitable, &data)
            .map(Some)
            .map_err(|err| format!("{path} from {source}: {err}").into())
    }

    pub fn overview(&self, vfs: &mut Vfs, index: i32) -> Result<Bitmap, Box<dyn Error>> {
        let table = load_mes(vfs, "worldmap/worldmap.mes")?;
        let row = 50i32
            .checked_add(index.max(0))
            .ok_or("world atlas index overflow")?;
        let basename = table
            .mes
            .get(row)
            .ok_or("missing continent map definition")?
            .split(',')
            .map(str::trim)
            .find_map(|field| field.strip_prefix("ZoomedName:"))
            .ok_or("continent map has no ZoomedName")?;
        let path = format!("worldmap/{}.bmp", normalize(basename.trim())?);
        let (source, bytes) = read_bounded(vfs, &path, 2 * 1024 * 1024)?;
        decode_bmp(&bytes).map_err(|error| format!("{path} from {source}: {error}").into())
    }

    pub fn world(&self, vfs: &mut Vfs, index: i32) -> Result<Sheet, Box<dyn Error>> {
        let table = load_mes(vfs, "worldmap/worldmap.mes")?;
        let row = 50i32
            .checked_add(index.max(0))
            .ok_or("world atlas index overflow")?;
        let mut fields = table
            .mes
            .get(row)
            .ok_or_else(|| format!("missing world atlas row {row}"))?
            .split(',')
            .map(str::trim);
        let columns = fields
            .next()
            .ok_or("missing world atlas columns")?
            .parse()?;
        let rows = fields.next().ok_or("missing world atlas rows")?.parse()?;
        let basename = normalize(fields.next().ok_or("missing world atlas basename")?)?;
        let mut sheet = Sheet::new(
            (columns, rows),
            (1, 1),
            format!("worldmap/{basename}"),
            3,
            1,
        )?;
        let bmp = sheet.load(vfs, 0)?;
        sheet.chunk = bmp.size;
        Ok(sheet)
    }
}

fn parse_area(id: i32, raw: &str) -> Result<Area, Box<dyn Error>> {
    let mut sections = raw.split('/');
    let mut fields = sections
        .next()
        .ok_or("missing coordinates")?
        .split(',')
        .map(str::trim);
    let mut next = || fields.next().ok_or("missing coordinate or label offset");
    let tile = (next()?.parse()?, next()?.parse()?);
    let label_offset = (next()?.parse()?, next()?.parse()?);
    let name = sections
        .next()
        .filter(|s| !s.trim().is_empty())
        .ok_or("missing area name")?
        .trim()
        .to_owned();
    let description = sections.next().unwrap_or_default().trim().to_owned();
    let radius = sections
        .find_map(|s| s.trim().strip_prefix("Radius:"))
        .map(|s| s.trim().parse::<i32>())
        .transpose()?
        .unwrap_or(5)
        .checked_mul(64)
        .ok_or("invalid area radius")?;
    let radius = u32::try_from(radius).ok();
    Ok(Area {
        id,
        name,
        description,
        tile,
        label_offset,
        radius,
    })
}

#[derive(Debug)]
pub struct Town {
    pub name: String,
    pub center: Point,
    pub waitable: bool,
    unscaled: Point,
    scale: f32,
    pub sheet: Sheet,
}

impl Town {
    fn parse(
        id: u32,
        name: &str,
        dir: String,
        waitable: bool,
        data: &[u8],
    ) -> Result<Self, Box<dyn Error>> {
        if data.len() != 48 || word(data, 0)? != 2 || word(data, 8)? != id {
            return Err("invalid town map version, ID or length".into());
        }
        let unscaled = (word(data, 12)?, word(data, 16)?);
        let grid = (word(data, 20)?, word(data, 24)?);
        let center = (word(data, 32)?, word(data, 36)?);
        let scale = f32::from_bits(word(data, 40)?);
        if !(0.01..=8.).contains(&scale)
            || grid.0 == 0
            || grid.1 == 0
            || unscaled.0 > 1_000_000
            || unscaled.1 > 1_000_000
        {
            return Err("invalid town map dimensions or scale".into());
        }
        let chunk = (
            ((unscaled.0 / grid.0) as f32 * scale) as u32,
            ((unscaled.1 / grid.1) as f32 * scale) as u32,
        );
        let sheet = Sheet::new(grid, chunk, format!("townmap/{dir}/{dir}"), 6, 0)?;
        Ok(Self {
            name: name.into(),
            center,
            waitable,
            unscaled,
            scale,
            sheet,
        })
    }

    pub fn pixel(&self, tile: Point) -> Option<Point> {
        let (dx, dy) = project_world_tile(tile, self.center)?;
        let x = i64::from(dx) + i64::from(self.unscaled.0 / 2);
        let y = i64::from(dy) + i64::from(self.unscaled.1 / 2);
        if x < 0 || y < 0 || x >= i64::from(self.unscaled.0) || y >= i64::from(self.unscaled.1) {
            return None;
        }
        Some((
            (x as f32 * self.scale) as u32,
            (y as f32 * self.scale) as u32,
        ))
    }
}

#[derive(Debug, Clone)]
pub struct Sheet {
    pub grid: Point,
    pub chunk: Point,
    prefix: String,
    digits: usize,
    first: u32,
}

impl Sheet {
    fn new(
        grid: Point,
        chunk: Point,
        prefix: String,
        digits: usize,
        first: u32,
    ) -> Result<Self, &'static str> {
        if grid.0 == 0
            || grid.1 == 0
            || grid.0.saturating_mul(grid.1) > 16_384
            || chunk.0 == 0
            || chunk.1 == 0
            || chunk.0 > 1024
            || chunk.1 > 1024
        {
            return Err("invalid atlas tile grid");
        }
        Ok(Self {
            grid,
            chunk,
            prefix,
            digits,
            first,
        })
    }

    pub fn size(&self) -> Point {
        (self.grid.0 * self.chunk.0, self.grid.1 * self.chunk.1)
    }

    pub fn index(&self, (x, y): Point) -> Option<u32> {
        (x < self.size().0 && y < self.size().1)
            .then(|| y / self.chunk.1 * self.grid.0 + x / self.chunk.0)
    }

    pub fn load(&self, vfs: &mut Vfs, index: u32) -> Result<Bitmap, Box<dyn Error>> {
        if index >= self.grid.0 * self.grid.1 {
            return Err("atlas tile outside grid".into());
        }
        let path = format!(
            "{}{:0width$}.bmp",
            self.prefix,
            self.first + index,
            width = self.digits
        );
        let (source, bytes) = read_bounded(vfs, &path, 2 * 1024 * 1024)?;
        decode_bmp(&bytes).map_err(|err| format!("{path} from {source}: {err}").into())
    }
}

pub struct Bitmap {
    pub size: Point,
    pub pixels: Vec<[u8; 4]>,
}

fn word(bytes: &[u8], offset: usize) -> Result<u32, &'static str> {
    bytes
        .get(offset..offset + 4)
        .ok_or("truncated field")
        .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
}

pub(super) fn decode_bmp(bytes: &[u8]) -> Result<Bitmap, &'static str> {
    if !bytes.starts_with(b"BM")
        || word(bytes, 14)? != 40
        || word(bytes, 26)? != 0x0008_0001
        || word(bytes, 30)? != 0
    {
        return Err("unsupported atlas BMP (expected 8-bit uncompressed Windows bitmap)");
    }
    let offset = word(bytes, 10)? as usize;
    let width = word(bytes, 18)?;
    let signed_height = word(bytes, 22)? as i32;
    let height = signed_height.unsigned_abs();
    let colors = match word(bytes, 46)? {
        0 => 256,
        n @ 1..=256 => n,
        _ => return Err("invalid bitmap palette size"),
    };
    if !(1..=1024).contains(&width) || !(1..=1024).contains(&height) {
        return Err("invalid bitmap dimensions");
    }
    let palette_end = (54 + colors * 4) as usize;
    if offset < palette_end || offset > bytes.len() {
        return Err("invalid bitmap pixel offset");
    }
    let stride = width.next_multiple_of(4) as usize;
    let data = bytes[offset..]
        .get(..stride * height as usize)
        .ok_or("truncated bitmap pixels")?;
    let mut pixels = Vec::with_capacity((width * height) as usize);
    for y in 0..height {
        let row = if signed_height < 0 { y } else { height - 1 - y } as usize * stride;
        for &index in &data[row..row + width as usize] {
            let i = usize::from(index) * 4 + 54;
            let &[b, g, r, _] = bytes
                .get(i..i + 4)
                .filter(|_| i < palette_end)
                .ok_or("bitmap palette index out of bounds")?
            else {
                unreachable!()
            };
            pixels.push([r, g, b, 255]);
        }
    }
    Ok(Bitmap {
        size: (width, height),
        pixels,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vfs::Policy;

    #[test]
    fn area_coordinates_and_script_only_discovery_are_independent() {
        let area = parse_area(7, "92960,82592,0,-5 /Crash Site/example/Radius:-1").unwrap();
        assert_eq!(area.id, 7);
        assert_eq!(area.label_offset, (0, -5));
        assert_eq!(area.pixel(2000), Some((548, 1290)));
        assert_eq!(area.radius, None);
        assert!(parse_area(7, "927,abc /Invalid/example").is_err());
    }

    #[test]
    fn town_projection_uses_relative_isometric_geometry_and_bounds() {
        let mut input = [0; 48];
        for (offset, word) in [
            (0, 2u32),
            (8, 37),
            (12, 5120),
            (16, 2560),
            (20, 20),
            (24, 20),
            (32, 96),
            (36, 96),
            (40, 0.25f32.to_bits()),
        ] {
            input[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
        }
        let town = Town::parse(37, "Bates", "bates".into(), false, &input).unwrap();
        assert_eq!(town.sheet.chunk, (64, 32));
        assert_eq!(town.pixel((96, 96)), Some((640, 320)));
        assert_eq!(town.pixel((97, 96)), Some((630, 325)));
        assert_eq!(town.pixel((0, 0)), None);
        assert!(Town::parse(37, "Bates", "bates".into(), false, &input[..47]).is_err());
    }

    #[test]
    fn indexed_bmp_is_bounded_and_uses_bottom_up_padded_rows() {
        let mut bytes = vec![0; 70];
        bytes[..2].copy_from_slice(b"BM");
        for (offset, word) in [
            (10, 62u32),
            (14, 40),
            (18, 2),
            (22, 2),
            (26, 0x80001),
            (46, 2),
        ] {
            bytes[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
        }
        bytes[58..62].copy_from_slice(&[10, 20, 30, 0]);
        bytes[62..].copy_from_slice(&[0, 1, 0, 0, 1, 0, 0, 0]);
        let bitmap = decode_bmp(&bytes).unwrap();
        assert_eq!(bitmap.size, (2, 2));
        assert_eq!(
            bitmap.pixels,
            [
                [30, 20, 10, 255],
                [0, 0, 0, 255],
                [0, 0, 0, 255],
                [30, 20, 10, 255]
            ]
        );
        assert!(decode_bmp(&bytes[..69]).is_err());
        bytes[62] = 2;
        assert!(decode_bmp(&bytes).is_err());
    }

    #[test]
    #[ignore = "requires original installed Arcanum data"]
    fn original_map_art_and_named_destinations() {
        let mut vfs =
            Vfs::from_game(std::env::var_os("ARCANUM_GAME_DIR").unwrap(), Policy::Ce).unwrap();
        let mut atlas = Atlas::load(&mut vfs).unwrap();
        let world = atlas.world(&mut vfs, 0).unwrap();
        assert_eq!(world.size(), (2000, 2000));
        assert_eq!(world.load(&mut vfs, 63).unwrap().size, (250, 250));
        assert_eq!(atlas.overview(&mut vfs, 0).unwrap().size, (365, 365));
        for (id, name, tile) in [
            (1, "Shrouded Hills", (90226, 84176)),
            (2, "Crash Site", (92960, 82592)),
            (6, "Qintarra", (98922, 42009)),
            (11, "Tulla", (28455, 31171)),
            (21, "Tarant", (62182, 65662)),
            (22, "Vendigroth Ruins", (19704, 37452)),
        ] {
            assert_eq!(
                (&*atlas.areas[&id].name, atlas.areas[&id].tile),
                (name, tile)
            );
            assert!(
                atlas.areas[&id]
                    .pixel(world.size().0)
                    .and_then(|p| world.index(p))
                    .is_some()
            );
        }
        assert_eq!(atlas.discovered.iter().copied().collect::<Vec<_>>(), [2]);
        assert!(atlas.select(21, &vfs).is_err());
        atlas.select(2, &vfs).unwrap();
        assert_eq!(
            atlas.destination,
            Some(Selection {
                area: 2,
                map: 5000,
                tile: (92960, 82592),
                sector_available: true
            })
        );
        for (id, tile, expected) in [
            (2, (92958, 82592), (1940, 950)),
            (141, (2087, 2018), (590, 365)),
            (37, (104, 92), (520, 340)),
            (12, (28455, 31171), (920, 850)),
        ] {
            let town = atlas.local(&mut vfs, id).unwrap().unwrap();
            assert_eq!(town.pixel(tile), Some(expected), "town {id}");
            assert_eq!(town.sheet.load(&mut vfs, 0).unwrap().size, (64, 32));
        }
    }
}
