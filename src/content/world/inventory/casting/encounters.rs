//! CE wmap_rnd uses regional frequency, power and table overrides on the starting map.
use super::*;
use crate::content::maps::Catalog;
use crate::random::Random;

#[derive(Clone, Default, Serialize, Deserialize)]
pub(in crate::content::world::inventory) struct Schedule {
    pub(in crate::content::world::inventory) due: Option<u64>,
    random: Random,
    #[serde(default)]
    counts: BTreeMap<i32, u32>,
    #[serde(default, skip_deserializing)]
    last: Option<Value>,
}
pub(in crate::content::world::inventory) struct Ruleset {
    start_map: String,
    source: String,
    charts: [Vec<(Tile, u32, i32)>; 3],
    tables: BTreeMap<i32, Vec<Entry>>,
}
struct Entry {
    key: i32,
    weight: u32,
    groups: Vec<(u32, u16, u16)>,
    min: i32,
    max: i32,
    flag: Option<i32>,
    limit: Option<u32>,
}
fn number<T: std::str::FromStr>(s: &str) -> Result<T, String> {
    s.trim()
        .trim_end_matches('%')
        .parse()
        .map_err(|_| format!("Invalid encounter number {s:?}"))
}
impl Entry {
    fn parse(key: i32, line: &str) -> Result<Self, String> {
        let mut fields = line.split(',').map(str::trim);
        let mut e = Self {
            key,
            weight: number(fields.next().unwrap_or(""))?,
            groups: vec![],
            min: 0,
            max: 32000,
            flag: None,
            limit: None,
        };
        while let Some(f) = fields.next().filter(|s| !s.is_empty()) {
            let (label, v) = f
                .split_once(':')
                .ok_or_else(|| format!("Invalid encounter clause {f}"))?;
            match label.trim().to_ascii_lowercase().as_str() {
                "first" | "second" | "third" | "fourth" | "fifth" => {
                    let proto = number(v)?;
                    let range = fields.next().ok_or("Missing encounter count")?;
                    let (a, b) = range.split_once('-').unwrap_or((range, range));
                    // TIG reads comma-delimited numeric ranges with atoi. In shipped tables
                    // an omitted comma can leave the next keyword inside the upper bound.
                    let digits = b
                        .trim_start()
                        .bytes()
                        .take_while(u8::is_ascii_digit)
                        .count();
                    let (lo, hi) = (number(a)?, number(&b.trim_start()[..digits])?);
                    if lo > hi {
                        return Err("Reversed encounter range".into());
                    }
                    e.groups.push((proto, lo, hi));
                    if digits < b.trim().len() {
                        break;
                    }
                }
                "minlevel" => e.min = number(v)?,
                "maxlevel" => e.max = number(v)?,
                "globalflag" => e.flag = Some(number(v)?),
                "triggercount" => e.limit = Some(number(v)?),
                _ => return Err(format!("Unknown encounter field {label}")),
            }
        }
        if e.groups.is_empty() {
            return Err("No encounter prototypes".into());
        }
        Ok(e)
    }
}
impl Ruleset {
    fn load(vfs: &mut Vfs) -> Result<Self, String> {
        let catalog = Catalog::load(vfs).map_err(|e| e.to_string())?;
        let start_map = catalog.new_game()?.directory.clone();
        let table = load_mes(vfs, "rules/wmap_rnd.mes").map_err(|e| e.to_string())?;
        let mut charts: [Vec<(Tile, u32, i32)>; 3] = Default::default();
        let mut tables: BTreeMap<i32, Vec<Entry>> = BTreeMap::new();
        for (key, row) in table.mes.entries() {
            match key {
                10000..40000 => {
                    let fields = row.split(',').map(str::trim).collect::<Vec<_>>();
                    let [x, y, r, v] = fields.as_slice() else {
                        return Err(format!("Invalid region {key}"));
                    };
                    let i = (key / 10000 - 1) as usize;
                    let value = if i == 1 {
                        ["none", "easy", "average", "powerful"]
                            .iter()
                            .position(|s| v.eq_ignore_ascii_case(s))
                            .ok_or_else(|| format!("Unknown encounter power {v}"))?
                            as i32
                    } else {
                        number(v)?
                    };
                    charts[i].push(((number(x)?, number(y)?), number(r)?, value));
                }
                50000.. => {
                    tables
                        .entry((key - 50000) / 100)
                        .or_default()
                        .push(Entry::parse(key, row).map_err(|e| format!("{key}: {e}"))?);
                }
                _ => (),
            }
        }
        for chart in &mut charts {
            chart.sort_by_key(|v| v.1)
        }
        Ok(Self {
            start_map,
            source: table.source.to_string(),
            charts,
            tables,
        })
    }
    fn roll(
        &self,
        w: &InteractiveWorld,
        at: Tile,
        word: u16,
        now: u64,
        rng: &mut Random,
    ) -> Option<&Entry> {
        let region = |i: usize| {
            self.charts[i]
                .iter()
                .find(|(o, r, _)| at.0.abs_diff(o.0).max(at.1.abs_diff(o.1)) <= *r)
                .map(|v| v.2)
        };
        let frequency = region(0)
            .unwrap_or(5)
            .saturating_mul(if w.rest.is_some() { 5 } else { 1 });
        if rng.below(100) as i32 >= frequency {
            return None;
        }
        let power = region(1).unwrap_or(2);
        if power == 0 {
            return None;
        }
        // Indexed by original TDF terrain type. Zero prohibits encounters.
        let kind = |n: u16| {
            [1, 3, 0, 2, 7, 6, 9, 2, 6, 0, 4, 2, 8, 5]
                .get(n as usize)
                .copied()
                .unwrap_or(0)
        };
        let terrain = kind(word >> 11);
        if terrain == 0 || kind((word >> 6) & 31) == 0 {
            return None;
        }
        let night = !(6..18).contains(&((now / 3600) % 24));
        let table = region(2).unwrap_or(i32::from(night) + 2 * power + 6 * terrain - 8);
        let level = w.character_statistics().level;
        let eligible = self.tables.get(&table)?.iter().filter(|e| {
            (e.min..=e.max).contains(&level)
                && e.flag.is_none_or(|f| w.cache.scripts.global_flag(f))
                && e.limit.is_none_or(|max| {
                    w.cache
                        .inventory
                        .encounter
                        .counts
                        .get(&e.key)
                        .copied()
                        .unwrap_or(0)
                        < max
                })
        });
        let total = eligible.clone().map(|e| u64::from(e.weight)).sum::<u64>();
        if total == 0 {
            return None;
        }
        let mut roll = rng.below(total) + 1;
        let mut last = None;
        for entry in eligible {
            last = Some(entry);
            if roll < u64::from(entry.weight) {
                break;
            }
            roll -= u64::from(entry.weight);
        }
        last
    }
}

impl InventoryReader {
    pub fn tick_wilderness_encounter(&mut self, w: &mut InteractiveWorld, now: u64) -> bool {
        let schedule = &mut w.cache.inventory.encounter;
        let delay = |rng: &mut Random| 60 * (300 + rng.below(401));
        let due = *schedule
            .due
            .get_or_insert_with(|| now.saturating_add(delay(&mut schedule.random)));
        if now < due {
            return false;
        }
        let Some((at, town)) = w.player_map_position() else {
            return false;
        };
        let schedule = &mut w.cache.inventory.encounter;
        let mut rng = schedule.random;
        schedule.due = Some(now.saturating_add(delay(&mut rng)));
        schedule.random = rng;
        if town != 0 {
            return false;
        }
        let Some(word) = w
            .cache
            .entries
            .get(&tile_sector_id(at))
            .and_then(|e| e.0.terrain_word)
        else {
            return false;
        };
        let rules = self
            .encounter_rules
            .get_or_insert_with(|| Ruleset::load(&mut self.vfs));
        let result: Result<Option<Value>, String> = (|| {
            let rules = rules.as_ref().map_err(Clone::clone)?;
            if rules.start_map != w.map {
                return Ok(None);
            }
            let Some(e) = rules.roll(w, at, word, now, &mut rng) else {
                return Ok(None);
            };
            let index = e.key;
            let groups = e.groups.clone();
            let source = rules.source.clone();
            let spawned = spawn_group(&mut self.vfs, &mut self.frames, w, at, &groups, &mut rng)?;
            let count = w.cache.inventory.encounter.counts.entry(index).or_default();
            *count = count.saturating_add(1);
            Ok(Some(
                json!({"entry":index,"source":source,"actors":spawned,"second":now}),
            ))
        })();
        w.cache.inventory.encounter.random = rng;
        match result {
            Ok(Some(report)) => {
                if report["actors"].as_array().is_some_and(|a| !a.is_empty()) {
                    w.finish_rest("wilderness_encounter");
                    w.refresh_spell_state();
                }
                w.cache.inventory.encounter.last = Some(report);
                true
            }
            Ok(None) => false,
            Err(e) => {
                w.cache.inventory.encounter.last = Some(json!({"error":e,"second":now}));
                false
            }
        }
    }
}

fn spawn_group(
    vfs: &mut Vfs,
    frames: &mut FrameCache,
    w: &mut InteractiveWorld,
    at: Tile,
    groups: &[(u32, u16, u16)],
    rng: &mut Random,
) -> Result<Vec<Value>, String> {
    let mut slots = Vec::new();
    let mut prototypes = Prototypes::default();
    let mut occupied = BTreeSet::new();
    let mut seq = w.cache.inventory.state.next_generated;
    let counts: Vec<_> = groups
        .iter()
        .map(|&(n, lo, hi)| {
            (
                n,
                if lo == hi {
                    lo
                } else {
                    lo + rng.below(u64::from(hi - lo) + 1) as u16
                },
            )
        })
        .collect();
    let origin = if rng.below(100) < 50 {
        (at.0.saturating_sub(6), at.1)
    } else {
        (at.0, at.1.saturating_sub(6))
    };
    for (proto, count) in counts {
        let serial = crate::prototype::BasicPrototypeId(proto)
            .serialized()
            .ok_or("Invalid encounter prototype")?;
        for k in 0..u32::from(count) {
            let ideal = (
                origin.0.saturating_add(u32::from(k != 0)),
                origin.1.saturating_add(if k == 0 { 1 } else { k / 3 }),
            );
            if occupied.contains(&ideal) {
                continue;
            }
            let free = |t: Tile| {
                t != at
                    && !occupied.contains(&t)
                    && collision::can_stand(SectorCache::player_cell(
                        &w.cache.entries,
                        &w.cache.door_states,
                        t,
                    ))
            };
            let tile = if free(ideal) {
                ideal
            } else {
                let candidates = (-6i32..=6).flat_map(|x| (-6i32..=6).map(move |y| (x, y)));
                let Some(t) = candidates
                    .filter_map(|(x, y)| {
                        Some((at.0.checked_add_signed(x)?, at.1.checked_add_signed(y)?))
                    })
                    .filter(|&t| free(t))
                    .min_by_key(|t| t.0.abs_diff(ideal.0).max(t.1.abs_diff(ideal.1)))
                else {
                    continue;
                };
                t
            };
            let mut id = OriginalId {
                kind: u16::MAX,
                payload: [0; 16],
            };
            id.payload[..8].copy_from_slice(&seq.to_le_bytes());
            let r = mobile::population::Resident::generated_npc(
                id,
                serial.0,
                tile,
                vfs,
                &mut prototypes,
            )?;
            let facing = npc::facing(tile, at).unwrap_or(0);
            let art = r
                .current_art
                .value()
                .ok_or("Encounter actor has no original art")?;
            let frame = standing(vfs, frames, (art & !(7 << 11)) | u32::from(facing) << 11)?;
            let gear = loadout::Loadout::prepare(
                vfs,
                frames,
                &r.prototype.as_ref().unwrap().defaults,
                rng,
            )?;
            seq = seq
                .checked_add(1 + gear.count() as u64)
                .filter(|n| *n < (usize::MAX / 2 - 2) as u64)
                .ok_or("Runtime identity overflow")?;
            occupied.insert(tile);
            slots.push((r, tile, facing, frame, gear));
        }
    }
    let inv = &mut w.cache.inventory;
    let mut report = Vec::new();
    for (r, tile, facing, frame, gear) in slots {
        let id = r.id;
        let sequence = inv.state.next_generated;
        inv.state.next_generated += 1;
        gear.commit(inv, &w.map, id);
        report.push(json!({"id":id.key(),"prototype":r.prototype_id,"tile":tile}));
        inv.magick.offsets.insert(
            id.key(),
            Placement {
                prototype: Some(r.prototype_id),
                origin_map: None,
                map: w.map.clone(),
                source: tile,
                tile,
                sequence,
            },
        );
        inv.magick.moved.insert(id, (Arc::new(r), frame));
        inv.magick.face(id, facing);
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encounter_groups_preserve_counts_and_eligibility() {
        let row = Entry::parse(50001,"30%, First: 11001, 1, Second: 11002, 2-4, MinLevel: 3, MaxLevel: 12, GlobalFlag: 300, TriggerCount: 1").unwrap();
        assert_eq!(row.groups, [(11001, 1, 1), (11002, 2, 4)]);
        assert_eq!(
            (row.weight, row.min, row.max, row.flag, row.limit),
            (30, 3, 12, Some(300), Some(1))
        );
        assert!(Entry::parse(50001, "30%, First: 11001, 4-2").is_err());
    }

    #[test]
    #[ignore = "requires ARCANUM_GAME_DIR with the original encounter and item tables"]
    fn installed_recovery_programs_and_wilderness_groups() {
        let root = std::env::var_os("ARCANUM_GAME_DIR").unwrap();
        let mut vfs = Vfs::from_game(root, crate::vfs::Policy::Ce).unwrap();
        let rules = Ruleset::load(&mut vfs).unwrap();
        assert!(rules.charts.iter().all(|c| !c.is_empty()));
        assert!(!rules.tables[&0].is_empty());
        let list = load_mes(&mut vfs, "rules/spelllist.mes").unwrap();
        for (id, expected, amount) in [
            (150, Channel::Normal, 20),
            (151, Channel::Fatigue, 20),
            (152, Channel::Poison, 80),
        ] {
            let p = Program::decode(&list.mes, id).unwrap();
            assert!(p.components.iter().any(|c|matches!(c.operation,Operation::Vital{heal:true,channel,min,max,..} if channel==expected && min==amount && max==amount)));
        }
    }
}
