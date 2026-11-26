//! Overland movement uses 64-tile sectors, independently of on-foot collision.
use super::{Error, load_mes, read_bounded, terrain::TerrainMap};
use crate::{
    sector::Coordinates,
    vfs::{self, Vfs},
};
use std::{
    cmp::Reverse,
    collections::{BTreeSet, BinaryHeap},
    error::Error as StdError,
    time::Duration,
};

type Point = (u32, u32);
type Sector = (i32, i32);
const DIRECTIONS: [Sector; 8] = [
    (-1, -1),
    (-1, 0),
    (-1, 1),
    (0, 1),
    (1, 1),
    (1, 0),
    (1, -1),
    (0, -1),
];
const LIMIT: usize = 5000;
pub const INITIAL_DELAY: Duration = Duration::from_millis(50);
pub const STEP_DELAY: Duration = Duration::from_millis(62);
const REAL_HOUR: u64 = 3_600_000 / 8;

pub fn sector_center(pixel: Point, width: u32) -> Option<Point> {
    Some((
        width
            .checked_sub(pixel.0)?
            .checked_mul(64)?
            .checked_add(32)?,
        pixel.1.checked_mul(64)?.checked_add(32)?,
    ))
}

pub fn starting_hour(vfs: &mut Vfs) -> Result<u8, Box<dyn StdError>> {
    match load_mes(vfs, "rules/gameinit.mes") {
        Ok(table) => table.mes.get(5).map_or(Ok(10), |value| {
            let hour = value.trim().parse::<u8>()?;
            if hour >= 24 {
                return Err("invalid original starting hour".into());
            }
            Ok(hour)
        }),
        Err(Error::Resource {
            error: vfs::Error::NotFound(_),
            ..
        }) => Ok(15),
        Err(error) => Err(error.into()),
    }
}

pub struct Clock {
    start_hour: u64,
    phase_ms: u64,
    travel_hours: u64,
    hour_offset: i64,
}

impl Clock {
    pub fn new(start_hour: u8) -> Self {
        Self {
            start_hour: u64::from(start_hour),
            phase_ms: 0,
            travel_hours: 0,
            hour_offset: 0,
        }
    }

    pub fn set_hour(&mut self, hour: u8, play: Duration) {
        let offset = i64::from(hour % 24) - i64::from(self.hour(play));
        self.hour_offset = self.hour_offset.saturating_add(offset);
    }

    pub fn step_travel(&mut self, steps: usize) {
        self.travel_hours = self.travel_hours.saturating_add(steps as u64);
    }

    pub fn hours(&self, play: Duration) -> u64 {
        let normal = self
            .start_hour
            .saturating_add(self.travel_hours)
            .saturating_add((self.elapsed_ms(play) / u128::from(REAL_HOUR)) as u64);
        normal.saturating_add_signed(self.hour_offset)
    }

    pub fn hour(&self, play: Duration) -> u8 {
        (self.hours(play) % 24) as u8
    }

    fn elapsed_ms(&self, play: Duration) -> u128 {
        play.as_millis() + u128::from(self.phase_ms)
    }

    pub fn minute(&self, play: Duration) -> u64 {
        (self.elapsed_ms(play) % u128::from(REAL_HOUR)) as u64 * 60 / REAL_HOUR
    }

    pub fn for_save(&self, play: Duration) -> (u64, u64) {
        (
            self.hours(play),
            (self.elapsed_ms(play) % u128::from(REAL_HOUR)) as u64,
        )
    }

    pub fn from_save((hours, phase_ms): (u64, u64)) -> Self {
        Self {
            start_hour: hours,
            phase_ms,
            travel_hours: 0,
            hour_offset: 0,
        }
    }

    pub(crate) fn valid_phase(phase_ms: u64) -> bool {
        phase_ms < REAL_HOUR
    }

    /// Time to an absolute game minute, bounded by the next hourly clock update.
    pub fn until_minute(&self, target: u64, play: Duration) -> Duration {
        let (hours, phase) = self.for_save(play);
        let minute = target.saturating_sub(hours.saturating_mul(60)).min(60);
        Duration::from_millis((minute * REAL_HOUR).div_ceil(60).saturating_sub(phase))
    }

    pub fn game_second(&self, play: Duration) -> u64 {
        let (hour, phase) = self.for_save(play);
        hour.saturating_mul(3600)
            .saturating_add(phase * 3600 / REAL_HOUR)
    }

    pub fn until_second(&self, target: u64, play: Duration) -> Duration {
        let (hour, phase) = self.for_save(play);
        let seconds = target.saturating_sub(hour.saturating_mul(3600)).min(3600);
        Duration::from_millis((seconds * REAL_HOUR).div_ceil(3600).saturating_sub(phase))
    }

    pub fn until_next_hour(&self, play: Duration) -> Duration {
        Duration::from_millis(REAL_HOUR - self.for_save(play).1)
    }
}

pub(super) fn load_blocks(vfs: &mut Vfs, map: &str) -> Result<BTreeSet<u64>, Box<dyn StdError>> {
    let block_path = format!("maps/{map}/map.sbf");
    match read_bounded(vfs, &block_path, 256 * 1024) {
        Ok((source, bytes)) => {
            parse_blocks(&bytes).map_err(|e| format!("{block_path} from {source}: {e}").into())
        }
        Err(Error::Resource {
            error: vfs::Error::NotFound(_),
            ..
        }) => Ok(BTreeSet::new()),
        Err(error) => Err(error.into()),
    }
}

pub struct TravelMap {
    map: String,
    toggled: BTreeSet<u64>,
    terrain: TerrainMap,
    blocked_sectors: BTreeSet<u64>,
    authored_sectors: BTreeSet<u64>,
    terrain_sectors: BTreeSet<String>,
}

impl TravelMap {
    pub fn load(vfs: &mut Vfs, map: &str) -> Result<Self, Box<dyn StdError>> {
        let terrain = TerrainMap::load(vfs, map)?.ok_or("original map has no terrain.tdf")?;
        let blocked_sectors = load_blocks(vfs, map)?;
        let prefix = format!("maps/{map}/");
        let authored_sectors = vfs
            .paths_with_prefix(&prefix)
            .into_iter()
            .filter_map(|path| {
                path.strip_prefix(&prefix)?
                    .strip_suffix(".sec")?
                    .parse()
                    .ok()
            })
            .collect();
        let terrain_sectors = vfs
            .paths_with_prefix("terrain/")
            .into_iter()
            .filter(|path| path.ends_with(".sec"))
            .map(str::to_owned)
            .collect();
        Ok(Self {
            terrain,
            map: map.to_owned(),
            toggled: BTreeSet::new(),
            terrain_sectors,
            blocked_sectors,
            authored_sectors,
        })
    }

    pub fn observe_blocks(&mut self, scripts: &super::script::State) -> bool {
        let toggled = scripts.sector_toggles.get(&self.map);
        if toggled.map_or(self.toggled.is_empty(), |set| set == &self.toggled) {
            return false;
        }
        self.toggled = toggled.cloned().unwrap_or_default();
        true
    }

    pub fn sector_blocked(&self, map: &str, tile: Point) -> Option<bool> {
        if map != self.map {
            return None;
        }
        sector_id(sector(tile)).map(|id| self.blocked(id))
    }

    fn blocked(&self, id: u64) -> bool {
        self.blocked_sectors.contains(&id) ^ self.toggled.contains(&id)
    }

    pub fn can_depart(&self, tile: Point) -> bool {
        self.passable(sector(tile)) && self.has_sector(tile)
    }

    pub fn has_sector(&self, tile: Point) -> bool {
        let coordinates = Coordinates {
            x: tile.0 / 64,
            y: tile.1 / 64,
        };
        coordinates
            .id()
            .is_some_and(|id| self.authored_sectors.contains(&id))
            || self
                .terrain
                .template(coordinates)
                .is_ok_and(|path| self.terrain_sectors.contains(&path))
    }

    fn passable(&self, at: Sector) -> bool {
        let Some(id) = sector_id(at) else {
            return false;
        };
        if self.blocked(id) {
            return false;
        }
        self.authored_sectors.contains(&id) || self.terrain.passable(at)
    }

    pub fn route(&self, from: Point, to: Point) -> Result<Vec<Point>, &'static str> {
        if !self.passable(sector(from)) {
            return Err("World travel is unavailable in this sector.");
        }
        if !self.passable(sector(to)) {
            return Err("The waypoint sector is blocked.");
        }
        route(sector(from), sector(to), |p| self.passable(p))
            .map(|steps| {
                steps
                    .into_iter()
                    .map(|(x, y)| (x as u32 * 64 + 32, y as u32 * 64 + 32))
                    .collect()
            })
            .ok_or("The route is blocked. Look for a bridge or pass.")
    }
}

fn parse_blocks(data: &[u8]) -> Result<BTreeSet<u64>, &'static str> {
    let count = u32::from_le_bytes(
        data.get(..4)
            .ok_or("missing sector block count")?
            .try_into()
            .unwrap(),
    ) as usize;
    let data = &data[4..];
    if data.len() / 8 != count || !data.len().is_multiple_of(8) {
        return Err("invalid sector block list length");
    }
    data.as_chunks::<8>()
        .0
        .iter()
        .map(|&row| {
            let id = u64::from_le_bytes(row);
            Coordinates::from_id(id)
                .map(|_| id)
                .ok_or("invalid blocked sector ID")
        })
        .collect()
}

fn sector((x, y): Point) -> Sector {
    ((x / 64) as i32, (y / 64) as i32)
}

fn sector_id((x, y): Sector) -> Option<u64> {
    Coordinates {
        x: u32::try_from(x).ok()?,
        y: u32::try_from(y).ok()?,
    }
    .id()
}

fn distance(a: Sector, b: Sector) -> u32 {
    a.0.abs_diff(b.0).max(a.1.abs_diff(b.1))
}

fn direct(a: Sector, b: Sector) -> Sector {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let (x, y) = (dx.signum(), dy.signum());
    if dx.abs() > 2 * dy.abs() {
        (a.0 + x, a.1)
    } else if dy.abs() > 2 * dx.abs() {
        (a.0, a.1 + y)
    } else {
        (a.0 + x, a.1 + y)
    }
}

// CE walks directly toward the target, detouring to the first passable sector
// behind an obstruction. Its local search fits in a 16x16 grid.
fn route(mut at: Sector, goal: Sector, passable: impl Fn(Sector) -> bool) -> Option<Vec<Sector>> {
    let mut result = Vec::new();
    while at != goal && result.len() < LIMIT {
        let next = direct(at, goal);
        if passable(next) {
            result.push(next);
            at = next;
            continue;
        }
        let mut rejoin = next;
        while distance(at, rejoin) < 8 && rejoin != goal && !passable(rejoin) {
            rejoin = direct(rejoin, goal);
        }
        if distance(at, rejoin) >= 8 || !passable(rejoin) {
            return None;
        }
        result.extend(detour(at, rejoin, &passable)?);
        at = rejoin;
    }
    (at == goal && result.len() <= LIMIT).then_some(result)
}

fn detour(from: Sector, to: Sector, passable: &impl Fn(Sector) -> bool) -> Option<Vec<Sector>> {
    let origin = (
        from.0.min(to.0) - (16 - from.0.abs_diff(to.0) as i32) / 2,
        from.1.min(to.1) - (16 - from.1.abs_diff(to.1) as i32) / 2,
    );
    let index = |(x, y): Sector| {
        let (x, y) = (x - origin.0, y - origin.1);
        ((0..16).contains(&x) && (0..16).contains(&y)).then_some((x + y * 16) as usize)
    };
    let point = |i: usize| (origin.0 + (i % 16) as i32, origin.1 + (i / 16) as i32);
    let mut path = [None; 256];
    let mut costs = [u32::MAX; 256];
    let start = index(from)?;
    let mut open = BinaryHeap::new();
    costs[start] = 0;
    open.push(Reverse((10 * distance(from, to), start, 0)));
    while let Some(Reverse((_, i, cost))) = open.pop() {
        let at = point(i);
        if cost != costs[i] {
            continue;
        }
        if at == to {
            let mut route = Vec::new();
            let mut i = i;
            while let Some(parent) = path[i] {
                route.push(point(i));
                i = parent;
            }
            route.reverse();
            return Some(route);
        }
        for (dx, dy) in DIRECTIONS {
            let next = (at.0 + dx, at.1 + dy);
            let Some(j) = index(next).filter(|_| passable(next)) else {
                continue;
            };
            let turns = path[i].is_some_and(|p| {
                let p = point(p);
                (at.0 - p.0, at.1 - p.1) != (dx, dy)
            });
            let next_cost = cost + 10 + u32::from(turns);
            if next_cost < costs[j] {
                costs[j] = next_cost;
                path[j] = Some(i);
                open.push(Reverse((next_cost + 10 * distance(next, to), j, next_cost)));
            }
        }
    }
    None
}

/// A fixed-step journey. The consumer supplies wall time and handles arrivals.
pub struct Journey {
    steps: Vec<Point>,
    done: usize,
    deadline: Duration,
}

impl Journey {
    pub fn new(steps: Vec<Point>, now: Duration) -> Option<Self> {
        (!steps.is_empty()).then_some(Self {
            steps,
            done: 0,
            deadline: now + INITIAL_DELAY,
        })
    }

    pub fn deadline(&self) -> Duration {
        self.deadline
    }

    pub fn advance(&mut self, now: Duration) -> usize {
        let mut advanced = 0;
        while self.done < self.steps.len() && now >= self.deadline {
            self.done += 1;
            advanced += 1;
            self.deadline += STEP_DELAY;
        }
        advanced
    }

    pub fn finished(&self) -> bool {
        self.done == self.steps.len()
    }

    pub fn diagnostic(&self) -> serde_json::Value {
        serde_json::json!({"steps_total":self.steps.len(),"steps_traversed":self.done,
            "current_sector":self.traversed().last(),"destination_sector":self.steps.last()})
    }

    pub fn traversed(&self) -> &[Point] {
        &self.steps[..self.done]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::map_atlas::Atlas;

    #[test]
    fn scheduled_minutes_include_overdue_events_and_saved_fractional_hours() {
        let clock = Clock::from_save((23, REAL_HOUR - 10));
        assert_eq!(clock.until_minute(23 * 60, Duration::ZERO), Duration::ZERO);
        assert_eq!(
            clock.until_minute(24 * 60, Duration::ZERO),
            Duration::from_millis(10)
        );
        assert_eq!(
            clock.until_minute(24 * 60, Duration::from_millis(10)),
            Duration::ZERO
        );
        assert_eq!(
            clock.until_minute(24 * 60 + 1, Duration::from_millis(10)),
            Duration::from_millis(REAL_HOUR / 60)
        );
    }

    #[test]
    fn set_time_keeps_day_and_elapsed_progression() {
        let mut clock = Clock::new(15);
        clock.step_travel(25);
        let elapsed = Duration::from_millis(REAL_HOUR);
        assert_eq!(clock.hours(elapsed), 41);
        clock.set_hour(2, elapsed);
        assert_eq!(clock.hours(elapsed), 26);
        assert_eq!(clock.hour(elapsed * 2), 3);
    }

    #[test]
    fn routing_honors_terrain_blocks_and_local_detours() {
        let (a, b) = ((10, 10), (18, 10));
        let road = route(a, b, |p| p != (13, 10)).unwrap();
        assert_eq!(road.last(), Some(&b));
        assert!(!road.contains(&(13, 10)));
        assert!(
            std::iter::once(&a)
                .chain(&road)
                .zip(&road)
                .all(|(&a, &b)| distance(a, b) == 1)
        );
        assert!(route(a, b, |p| !(11..=17).contains(&p.0)).is_none());
        assert!(parse_blocks(&[1, 0, 0, 0]).is_err());
    }

    #[test]
    fn journey_accumulates_each_step_at_ce_deadlines() {
        let mut clock = Clock::new(15);
        let start = Duration::from_millis(100);
        let mut journey = Journey::new(vec![(32, 32), (96, 32), (160, 32)], start).unwrap();
        assert_eq!(journey.advance(start), 0);
        clock.step_travel(journey.advance(start + INITIAL_DELAY));
        assert_eq!(journey.traversed(), &[(32, 32)]);
        assert_eq!(clock.hour(Duration::ZERO), 16);
        assert!(!journey.finished());
        clock.step_travel(journey.advance(start + INITIAL_DELAY + STEP_DELAY * 2));
        assert!(journey.finished());
        assert!(journey.finished());
        assert_eq!(clock.hour(Duration::ZERO), 18);
        assert_eq!(clock.hour(Duration::from_secs(6 * 450)), 0);
        assert_eq!(
            Clock::new(0).until_next_hour(Duration::from_secs(449)),
            Duration::from_secs(1)
        );
    }

    #[test]
    #[ignore = "requires installed Arcanum data"]
    fn original_overland_routes_and_authored_arrivals() {
        let root = std::env::var_os("ARCANUM_GAME_DIR").expect("set ARCANUM_GAME_DIR");
        let mut vfs = Vfs::from_game(root, vfs::Policy::Ce).unwrap();
        let mut atlas = Atlas::load(&mut vfs).unwrap();
        let map = atlas.catalog.new_game().unwrap();
        let start = map.map_list_center;
        let map_id = map.id;
        let directory = map.directory.clone();
        let terrain = TravelMap::load(&mut vfs, &directory).unwrap();
        let hills = atlas
            .areas
            .values()
            .find(|a| a.name == "Shrouded Hills")
            .unwrap();
        let (hills_id, hills_tile) = (hills.id, hills.tile);
        assert!(atlas.select(hills_id, &vfs).is_err());
        atlas.discovered.insert(hills_id);
        atlas.select(hills_id, &vfs).unwrap();
        assert_eq!(atlas.destination.as_ref().unwrap().tile, hills_tile);
        assert_eq!(hills_tile, (90226, 84176));
        assert!(terrain.can_depart(start));
        let road = terrain.route(start, hills_tile).unwrap();
        let return_road = terrain.route(hills_tile, start).unwrap();
        assert_eq!((road.len(), return_road.len()), (43, 44));
        assert!(atlas.travel_available_from(0, map_id, 0));
        assert!(!atlas.travel_available_from(0, map_id, 2));
        assert!(!atlas.travel_available_from(0, 5078, 0));
        for (from, destination) in [
            ((28320, 31168), "Vendigroth Ruins"),
            ((91902, 39305), "Qintarra"),
            ((92960, 82692), "Shrouded Hills"),
        ] {
            let area = atlas
                .areas
                .values()
                .find(|area| area.name == destination)
                .unwrap();
            let dst = area.tile;
            assert!(terrain.can_depart(from), "departure sector at {from:?}");
            assert!(terrain.has_sector(dst), "arrival sector at {dst:?}");
            let route = terrain.route(from, dst);
            eprintln!(
                "{from:?} to {destination} {dst:?}: {:?}",
                route.as_ref().map(Vec::len)
            );
            assert!(route.is_ok(), "{destination} from {from:?}: {route:?}");
        }
        assert_eq!(sector(*road.last().unwrap()), sector(hills_tile));
        let (first, last) = road.split_at(10);
        assert!(terrain.passable(sector(*first.last().unwrap())));
        assert!(
            terrain
                .route(*first.last().unwrap(), *last.last().unwrap())
                .is_ok()
        );
    }
}
