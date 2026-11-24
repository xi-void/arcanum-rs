//! Original map metadata and winning sector namespaces.

use super::{Source, load_mes, read_bounded, world::SectorRegion};
use crate::{resource_path::normalize, sector::Coordinates, vfs::Vfs};
use std::{collections::BTreeSet, error::Error};

pub struct Catalog {
    pub source: Source,
    pub maps: Vec<Map>,
}

pub struct Map {
    pub id: i32,
    pub name: String,
    pub directory: String,
    pub map_list_center: (u32, u32),
    start_map: bool,
    pub area: Option<i32>,
    pub worldmap: Option<i32>,
}

pub struct MapFiles {
    pub start: Option<((u32, u32), Source)>,
    game_entry: (u32, u32),
    pub region: SectorRegion,
    pub sectors: BTreeSet<u64>,
}

impl Catalog {
    pub fn load(vfs: &mut Vfs) -> Result<Self, Box<dyn Error>> {
        let table = load_mes(vfs, "rules/maplist.mes")?;
        let mut maps = Vec::new();
        for (id, value) in table.mes.entries().filter(|(id, _)| *id >= 5000) {
            let malformed = || format!("invalid map metadata at row {id} in {}", table.source);
            let mut fields = value.split(',').map(str::trim);
            let name = fields.next().ok_or_else(malformed)?;
            let directory = normalize(name)?;
            if directory.contains('/') {
                return Err(malformed().into());
            }
            let x = fields
                .next()
                .ok_or_else(malformed)?
                .parse()
                .map_err(|_| malformed())?;
            let y = fields
                .next()
                .ok_or_else(malformed)?
                .parse()
                .map_err(|_| malformed())?;
            let mut start_map = false;
            let mut area = None;
            let mut worldmap = None;
            for field in fields {
                if let Some((key, value)) = field.split_once(':') {
                    let value = value.trim();
                    match key.trim().to_ascii_lowercase().as_str() {
                        "type" => start_map = value.eq_ignore_ascii_case("START_MAP"),
                        "area" => area = Some(value.parse().map_err(|_| malformed())?),
                        "worldmap" => worldmap = Some(value.parse().map_err(|_| malformed())?),
                        _ => {}
                    }
                }
            }
            maps.push(Map {
                id,
                name: name.into(),
                directory,
                map_list_center: (x, y),
                start_map,
                area,
                worldmap,
            });
        }
        Ok(Self {
            source: table.source,
            maps,
        })
    }

    /// CE's new-game action resolves MAP_TYPE_START_MAP from MapList.mes.
    pub fn new_game(&self) -> Result<&Map, String> {
        let mut starts = self.maps.iter().filter(|map| map.start_map);
        let start = starts.next().ok_or("original map list has no START_MAP")?;
        if starts.next().is_some() {
            return Err("original map list has multiple START_MAP entries".into());
        }
        Ok(start)
    }

    pub fn find(&self, input: &str) -> Result<&Map, String> {
        let normalized = input.trim().to_ascii_lowercase().replace('’', "'");
        let exact: Vec<_> = self
            .maps
            .iter()
            .filter(|map| map.directory == normalized || normalized.parse::<i32>() == Ok(map.id))
            .collect();
        if let [map] = exact.as_slice() {
            return Ok(map);
        }
        let mut candidates: Vec<_> = if exact.is_empty() && !normalized.is_empty() {
            self.maps
                .iter()
                .filter(|map| map.directory.starts_with(&normalized))
                .collect()
        } else {
            exact
        };
        // Descendant scenarios are named “Parent - …”. A unique parent
        // can be selected by its prefix without guessing between peers.
        if let Some(parent) = candidates
            .iter()
            .min_by_key(|map| map.directory.len())
            .copied()
            && candidates.iter().all(|map| {
                std::ptr::eq(*map, parent)
                    || map
                        .directory
                        .strip_prefix(&parent.directory)
                        .is_some_and(|suffix| suffix.starts_with(" - "))
            })
        {
            return Ok(parent);
        }
        candidates.sort_unstable_by_key(|map| map.id);
        if candidates.is_empty() {
            let hints = self
                .maps
                .iter()
                .filter(|map| !normalized.is_empty() && map.directory.contains(&normalized))
                .take(8)
                .map(|map| map.name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            return Err(if hints.is_empty() {
                format!("unknown map {input:?}; use art maps list")
            } else {
                format!("unknown map {input:?}; matching names: {hints}")
            });
        }
        let names = candidates
            .iter()
            .take(12)
            .map(|map| format!("{} ({})", map.name, map.id))
            .collect::<Vec<_>>()
            .join(", ");
        Err(format!("ambiguous map {input:?}: {names}"))
    }
}

impl Map {
    pub fn files(&self, vfs: &mut Vfs) -> Result<MapFiles, Box<dyn Error>> {
        let prefix = format!("maps/{}/", self.directory);
        let mut sectors = BTreeSet::new();
        let mut first = Coordinates {
            x: u32::MAX,
            y: u32::MAX,
        };
        let mut last = Coordinates { x: 0, y: 0 };
        for path in vfs.paths_with_prefix(&prefix) {
            let Some(suffix) = path
                .strip_prefix(&prefix)
                .and_then(|name| name.strip_suffix(".sec"))
            else {
                continue;
            };
            let Ok(id) = suffix.parse::<u64>() else {
                continue;
            };
            let Some(coordinates) = Coordinates::from_id(id) else {
                continue;
            };
            if suffix != id.to_string() || vfs.resolve(path)?.winner().is_none() {
                continue;
            }
            first.x = first.x.min(coordinates.x);
            first.y = first.y.min(coordinates.y);
            last.x = last.x.max(coordinates.x);
            last.y = last.y.max(coordinates.y);
            sectors.insert(id);
        }
        if sectors.is_empty() {
            return Err(format!(
                "{} has no available base sector resources under {prefix}",
                self.name
            )
            .into());
        }
        let path = format!("{prefix}startloc.txt");
        let start = if vfs.resolve(&path)?.winner().is_some() {
            let (source, data) = read_bounded(vfs, &path, 256)?;
            let text = std::str::from_utf8(&data)?.trim_start_matches('\u{feff}');
            let mut lines = text.lines();
            let parse = |line: Option<&str>| -> Result<u32, Box<dyn Error>> {
                line.and_then(|line| line.trim().parse().ok())
                    .ok_or_else(|| format!("invalid author start coordinates in {source}").into())
            };
            Some(((parse(lines.next())?, parse(lines.next())?), source))
        } else {
            None
        };
        Ok(MapFiles {
            start,
            game_entry: self.map_list_center,
            region: SectorRegion { first, last },
            sectors,
        })
    }
}

impl MapFiles {
    /// Large maps use a local streaming rectangle around the arrival point.
    pub fn region_at(&self, center: (u32, u32)) -> SectorRegion {
        if self.region.camera(center).is_some() {
            return self.region;
        }
        let x = center.0 / 64;
        let y = center.1 / 64;
        SectorRegion {
            first: Coordinates {
                x: self.region.first.x.max(x.saturating_sub(15)),
                y: self.region.first.y.max(y.saturating_sub(15)),
            },
            last: Coordinates {
                x: self.region.last.x.min(x.saturating_add(15)),
                y: self.region.last.y.min(y.saturating_add(15)),
            },
        }
    }

    pub fn center(&self, center: Option<(u32, u32)>) -> Result<(u32, u32), String> {
        // CE map_open_in_game overrides the map_open startloc.txt value
        // with the map-list entry before setting the camera origin.
        let point = center.unwrap_or(self.game_entry);
        let sector = Coordinates {
            x: point.0 / 64,
            y: point.1 / 64,
        };
        if !sector.id().is_some_and(|id| self.sectors.contains(&id)) {
            return Err(format!(
                "world tile {point:?} is in unavailable sector {sector:?}; specify --center in a discovered sector"
            ));
        }
        Ok(point)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vfs::Policy;
    use std::{fs, path::PathBuf};

    struct Fixture(PathBuf);

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn resolves_named_map_descendants_and_winning_authored_start() {
        let fixture =
            Fixture(std::env::temp_dir().join(format!("arcanum-map-test-{}", std::process::id())));
        let base = fixture.0.join("base");
        let patch = fixture.0.join("patch");
        fs::create_dir_all(base.join("rules")).unwrap();
        fs::create_dir_all(base.join("maps/tulla interior")).unwrap();
        fs::create_dir_all(patch.join("maps/tulla interior")).unwrap();
        fs::write(
            base.join("rules/maplist.mes"),
            concat!(
                "{5031}{Tulla Interior, 96, 96, Type: START_MAP, Area: 11}",
                "{5032}{Tulla Interior - Battle, 100, 100}",
                "{5067}{K'na Tha, 20, 30}",
                "{5120}{Other Keep, 1, 2}",
                "{5121}{Other Cave, 3, 4}",
            ),
        )
        .unwrap();
        fs::write(base.join("maps/tulla interior/startloc.txt"), "64\n64\n").unwrap();
        fs::write(base.join("maps/tulla interior/67108865.sec"), [0]).unwrap();
        fs::write(
            patch.join("maps/tulla interior/startloc.txt"),
            "100\r\n100\r\n",
        )
        .unwrap();
        let mut vfs = Vfs::new(Policy::Ordered);
        vfs.mount_directory(&base).unwrap();
        vfs.mount_directory(&patch).unwrap();
        let catalog = Catalog::load(&mut vfs).unwrap();
        assert_eq!(catalog.new_game().unwrap().id, 5031);
        assert_eq!(catalog.find("TULLA").unwrap().id, 5031);
        assert_eq!(
            catalog.find("5032").unwrap().name,
            "Tulla Interior - Battle"
        );
        assert_eq!(catalog.find("K’na Tha").unwrap().directory, "k'na tha");
        assert!(catalog.find("other").err().unwrap().contains("ambiguous"));
        assert!(
            catalog
                .find("interior")
                .err()
                .unwrap()
                .contains("matching names")
        );
        let files = catalog.find("tulla").unwrap().files(&mut vfs).unwrap();
        assert_eq!(files.start.as_ref().unwrap().0, (100, 100));
        assert_eq!(files.center(None), Ok((96, 96)));
        assert_eq!(files.center(Some((64, 65))), Ok((64, 65)));
        assert!(files.center(Some((0, 0))).is_err());
    }
}
