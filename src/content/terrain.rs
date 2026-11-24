//! Original sector terrain grid and template lookup.
use super::{Error, Source, load_mes, read_bounded};
use crate::{
    resource_path::normalize,
    sector::Coordinates,
    vfs::{self, Vfs},
};
use flate2::bufread::ZlibDecoder;
use std::{error::Error as StdError, io::Read};

pub(super) struct Terrain {
    pub(super) width: usize,
    pub(super) cells: Vec<u16>,
}

impl Terrain {
    pub(super) fn parse(data: &[u8]) -> Result<Self, &'static str> {
        let word = |i| {
            data.get(i..i + 4)
                .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
        };
        if word(0) != Some(1.2f32.to_bits()) {
            return Err("expected terrain TDF version 1.2");
        }
        let flags = word(4).ok_or("missing terrain flags")?;
        let dimension = |i| {
            (word(i + 4) == Some(0))
                .then(|| word(i))
                .flatten()
                .filter(|n| (1..=4096).contains(n))
                .map(|n| n as usize)
                .ok_or("invalid terrain dimensions")
        };
        let width = dimension(8)?;
        let height = dimension(16)?;
        let mut tail = data.get(32..).ok_or("truncated terrain header")?;
        let pitch = width * 2;
        let mut cells = Vec::with_capacity(width * height);
        let mut row = vec![0; pitch];
        for _ in 0..height {
            match flags {
                0 => {
                    row.copy_from_slice(tail.get(..pitch).ok_or("truncated terrain row")?);
                    tail = &tail[pitch..];
                }
                1 => {
                    let size = u32::from_le_bytes(
                        tail.get(..4)
                            .ok_or("missing terrain row size")?
                            .try_into()
                            .unwrap(),
                    ) as usize;
                    tail = &tail[4..];
                    let compressed = tail.get(..size).ok_or("truncated compressed terrain row")?;
                    tail = &tail[size..];
                    let mut decoder = ZlibDecoder::new(compressed);
                    decoder
                        .read_exact(&mut row)
                        .map_err(|_| "invalid compressed terrain row")?;
                    if decoder
                        .read(&mut [0])
                        .map_err(|_| "invalid terrain row checksum")?
                        != 0
                        || decoder.total_in() != size as u64
                    {
                        return Err("unexpected compressed terrain row size");
                    }
                }
                _ => return Err("unsupported terrain flags"),
            }
            cells.extend(
                row.as_chunks::<2>()
                    .0
                    .iter()
                    .copied()
                    .map(u16::from_le_bytes),
            );
        }
        if !tail.is_empty() {
            return Err("trailing terrain data");
        }
        Ok(Self { width, cells })
    }

    pub(super) fn at(&self, (x, y): (i32, i32)) -> Option<u16> {
        let x = usize::try_from(x).ok()?;
        let y = usize::try_from(y).ok()?;
        (x < self.width && y < self.cells.len() / self.width)
            .then(|| self.cells[y * self.width + x])
    }
}

pub(super) struct TerrainMap {
    pub grid: Terrain,
    pub source: Source,
    names: Vec<String>,
    blocked_types: u32,
}

impl TerrainMap {
    pub fn load(vfs: &mut Vfs, map: &str) -> Result<Option<Self>, Box<dyn StdError>> {
        let path = format!("maps/{map}/terrain.tdf");
        let (source, bytes) = match read_bounded(vfs, &path, 64 * 1024 * 1024) {
            Ok(value) => value,
            Err(Error::Resource {
                error: vfs::Error::NotFound(_),
                ..
            }) => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let grid = Terrain::parse(&bytes).map_err(|err| format!("{path} from {source}: {err}"))?;
        let table = load_mes(vfs, "terrain/terrain.mes")?;
        let mut names = Vec::new();
        let mut blocked_types = 0;
        for (index, entry) in (0..32).map_while(|i| table.mes.get(i).map(|entry| (i, entry))) {
            let (name, blocked) = entry
                .split_once("/b")
                .map_or((entry, false), |(name, _)| (name, true));
            let name = normalize(name.trim())?;
            if name.contains('/') {
                return Err(format!(
                    "invalid terrain basename at entry {index} in {}",
                    table.source
                )
                .into());
            }
            names.push(name);
            if blocked {
                blocked_types |= 1 << index;
            }
        }
        Ok(Some(Self {
            grid,
            source,
            names,
            blocked_types,
        }))
    }

    pub(super) fn unique_template_references(
        &self,
    ) -> impl Iterator<Item = (u16, Result<String, &'static str>)> + '_ {
        self.grid
            .cells
            .iter()
            .copied()
            .filter(|&word| word != u16::MAX)
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .map(|word| (word, template(word, &self.names)))
    }

    pub fn passable(&self, sector: (i32, i32)) -> bool {
        self.grid.at(sector).is_some_and(|word| {
            word != 0xffff
                && self.blocked_types & ((1 << (word >> 11)) | (1 << ((word >> 6) & 31))) == 0
        })
    }

    pub fn template(&self, sector: Coordinates) -> Result<String, String> {
        let word = self
            .grid
            .at((sector.x as i32, sector.y as i32))
            .ok_or_else(|| {
                format!(
                    "sector ({}, {}) is outside {}",
                    sector.x, sector.y, self.source.virtual_path
                )
            })?;
        template(word, &self.names).map_err(|reason| {
            format!(
                "{} from {}, terrain word {word:#06x}: {reason}",
                self.source.virtual_path, self.source
            )
        })
    }
}

// CE terrain_sector_path and sub_4E8E60. The inverse transition switches
// both terrain names and the transition index without mirroring the raster.
fn template(word: u16, names: &[String]) -> Result<String, &'static str> {
    const TRANSITIONS: [Option<u64>; 16] = [
        None,
        Some(1),
        Some((2 << 26) | 1),
        Some((1 << 26) | 1),
        Some((2 << 26) | 3),
        Some(0),
        Some((2 << 26) | 2),
        None,
        Some(3),
        Some(2),
        None,
        None,
        Some((1 << 26) | 3),
        None,
        None,
        None,
    ];
    let name = |index| names.get(index as usize).ok_or("invalid terrain type");
    let base = word >> 11;
    let neighbor = (word >> 6) & 31;
    let transition = usize::from((word >> 2) & 15);
    if base == neighbor || transition == 0 {
        let variant = u64::from(word & 3);
        return Ok(format!(
            "terrain/{}/{}.sec",
            name(base)?,
            (variant % 2) | ((variant / 2) << 26)
        ));
    }
    let (from, to, index) = TRANSITIONS[transition]
        .map_or((neighbor, base, 15 - transition), |_| {
            (base, neighbor, transition)
        });
    let id = TRANSITIONS[index].ok_or("unsupported terrain transition")?;
    Ok(format!("terrain/{} to {}/{id}.sec", name(from)?, name(to)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    #[test]
    fn compressed_and_uncompressed_terrain_rows_match() {
        let mut header = vec![0; 32];
        header[..4].copy_from_slice(&1.2f32.to_le_bytes());
        header[8] = 2;
        header[16] = 1;
        let row = [0x00, 0x08, 0xff, 0xff];
        let plain = [header.as_slice(), &row].concat();
        header[4] = 1;
        let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
        encoder.write_all(&row).unwrap();
        let compressed = encoder.finish().unwrap();
        header.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
        header.extend_from_slice(&compressed);
        let terrain = Terrain::parse(&header).unwrap();
        assert_eq!(terrain.cells, Terrain::parse(&plain).unwrap().cells);
        assert_eq!(terrain.at((1, 0)), Some(0xffff));
        assert_eq!(terrain.at((2, 0)), None);
        assert!(Terrain::parse(&header[..header.len() - 1]).is_err());
    }

    #[test]
    fn original_variant_and_reverse_transition_templates() {
        let names = vec!["grass".into(), "water".into()];
        assert_eq!(template(3, &names).unwrap(), "terrain/grass/67108865.sec");
        assert_eq!(
            template((1 << 6) | (2 << 2), &names).unwrap(),
            "terrain/grass to water/134217729.sec"
        );
        assert_eq!(
            template((1 << 11) | (13 << 2), &names).unwrap(),
            "terrain/grass to water/134217729.sec"
        );
        assert!(template(0xffff, &names).is_err());
    }
}
