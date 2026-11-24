//! The modal atlas uses the same layout, typography and compositor as the HUD.
use super::hud::{Action, Binding, Hit, Hud, MapView as Mode, Rect};
use arcanum_rs::{
    art::SpriteFrame,
    content::map_atlas::{Atlas, Bitmap, Point, Sheet, Town, world_pixel},
    content::travel::{self, Journey, TravelMap},
    vfs::Vfs,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    path::Path,
    time::Duration,
};

#[derive(Clone, Copy)]
struct Transform {
    rect: Rect,
    center: (f64, f64),
    scale: f64,
    ui_scale: u32,
}

impl Transform {
    fn origin(self) -> (f64, f64) {
        (
            self.center.0 - f64::from(self.rect.w) / (2. * self.scale),
            self.center.1 - f64::from(self.rect.h) / (2. * self.scale),
        )
    }

    fn at(self, point: Point) -> Option<Point> {
        if !self.rect.contains(point) {
            return None;
        }
        let origin = self.origin();
        let x = origin.0 + (f64::from(point.0) + 0.5 - f64::from(self.rect.x)) / self.scale;
        let y = origin.1 + (f64::from(point.1) + 0.5 - f64::from(self.rect.y)) / self.scale;
        (x >= 0. && y >= 0.).then_some((x as u32, y as u32))
    }

    fn screen(self, point: Point) -> (i32, i32) {
        let origin = self.origin();
        (
            (f64::from(point.0) + 0.5 - origin.0)
                .mul_add(self.scale, f64::from(self.rect.x))
                .floor() as i32,
            (f64::from(point.1) + 0.5 - origin.1)
                .mul_add(self.scale, f64::from(self.rect.y))
                .floor() as i32,
        )
    }
}

struct Waypoint {
    tile: Point,
    area: Option<i32>,
    steps: Vec<Point>,
}

pub(super) enum Arrival {
    Destination(i32, Point),
    Cancelled(i32, Option<Point>),
}

pub(crate) struct MapUi {
    hud: Hud,
    atlas: Atlas,
    vfs: Vfs,
    town: Option<Town>,
    town_id: u32,
    explored: BTreeMap<u32, BTreeSet<u32>>,
    world: Sheet,
    continent: Option<Bitmap>,
    world_id: i32,
    world_available: bool,
    travel_map: Result<TravelMap, String>,
    can_travel: bool,
    waypoints: Vec<Waypoint>,
    active_waypoint: Option<usize>,
    journey: Option<Journey>,
    travel_message: Option<String>,
    tile: Option<Point>,
    world_tile: Option<Point>,
    location: String,
    town_error: Option<String>,
    world_error: Option<String>,
    mode: Mode,
    inspect_all: bool,
    open: bool,
    origin_map: Option<(String, Point, u32)>,
    center: (f64, f64),
    readout: Point,
    zoom: usize,
    bounds: Option<(Rect, u32)>,
    dirty: bool,
    drag: Option<(Point, Point)>,
    suppress_click: bool,
    bitmaps: BTreeMap<u32, Option<Bitmap>>,
    bitmap_bytes: usize,
}

const ZOOMS: [f64; 4] = [0.25, 0.5, 1., 2.];
const BITMAP_BUDGET: usize = 24 * 1024 * 1024;
const MAX_VISIBLE_CHUNKS: u32 = 4096;

impl MapUi {
    pub(crate) fn load(
        path: &Path,
        mut vfs: Vfs,
        scale: Option<u16>,
        inspect_all: bool,
        discover_all: bool,
    ) -> Result<Self, Box<dyn Error>> {
        let mut atlas = Atlas::load(&mut vfs)?;
        if discover_all {
            atlas.discover_all();
        }
        let travel_map = TravelMap::load(&mut vfs, &atlas.catalog.new_game()?.directory)
            .map_err(|err| err.to_string());
        let world = atlas.world(&mut vfs, 0)?;
        let hud = Hud::load(path, &mut vfs, scale)?;
        if !hud.has_canvas() {
            return Err("map theme needs a visible map-canvas binding".into());
        }
        Ok(Self {
            hud,
            atlas,
            vfs,
            world,
            continent: None,
            world_id: 0,
            world_available: false,
            travel_map,
            can_travel: false,
            waypoints: Vec::new(),
            active_waypoint: None,
            journey: None,
            travel_message: None,
            town: None,
            town_id: 0,
            explored: BTreeMap::new(),
            tile: None,
            world_tile: None,
            location: String::new(),
            town_error: None,
            world_error: None,
            mode: Mode::Local,
            inspect_all,
            open: false,
            origin_map: None,
            center: (0., 0.),
            readout: (0, 0),
            zoom: 2,
            bounds: None,
            dirty: true,
            drag: None,
            suppress_click: false,
            bitmaps: BTreeMap::new(),
            bitmap_bytes: 0,
        })
    }

    pub(super) fn diagnostic(&self) -> serde_json::Value {
        serde_json::json!({"open":self.open,"mode":match self.mode { Mode::Local=>"local",Mode::World=>"world",Mode::Continent=>"continent" },"travel_eligible":self.can_travel,
            "journey":self.journey.as_ref().map(Journey::diagnostic),
            "waypoints":self.waypoints.iter().map(|point|point.tile).collect::<Vec<_>>(),
            "hud":self.hud.diagnostics()})
    }

    pub(super) fn world_width(&self) -> u32 {
        self.world.size().0
    }

    pub(super) fn discover_all(&mut self) -> usize {
        self.atlas.discover_all();
        self.dirty = true;
        self.atlas.discovered.len()
    }

    pub(super) fn prepare_theme(&mut self, path: &Path, scale: u16) -> Result<Hud, String> {
        let hud = Hud::load(path, &mut self.vfs, Some(scale)).map_err(|e| e.to_string())?;
        if !hud.has_canvas() {
            return Err("map theme needs a visible map-canvas binding".into());
        }
        Ok(hud)
    }

    pub(super) fn replace_theme(&mut self, mut hud: Hud) {
        hud.set_map_view(self.mode);
        self.hud = hud;
        self.invalidate_sheet();
    }

    pub(super) fn set_scale(&mut self, scale: u16) {
        self.hud.set_scale(scale);
        self.dirty = true;
    }

    pub(super) fn observe(&mut self, map: &str, tile: Point, town_id: u32) {
        if self
            .origin_map
            .as_ref()
            .is_some_and(|(name, pos, id)| name == map && *pos == tile && *id == town_id)
        {
            return;
        }
        let new_area = self
            .origin_map
            .as_ref()
            .is_none_or(|(name, _, id)| name != map || *id != town_id);
        let new_sector = self
            .origin_map
            .as_ref()
            .is_none_or(|(_, old, _)| (old.0 / 64, old.1 / 64) != (tile.0 / 64, tile.1 / 64));
        self.origin_map = Some((map.to_owned(), tile, town_id));
        self.tile = Some(tile);
        let current = self.atlas.catalog.find(map).ok();
        self.location = current.map_or(map, |meta| &meta.name).into();
        self.world_tile = current.and_then(|meta| {
            if meta.id == self.atlas.start_map {
                Some(tile)
            } else {
                self.atlas.areas.get(&meta.area?).map(|area| area.tile)
            }
        });
        if new_area && let Some(meta) = current {
            let world_id = meta.worldmap.unwrap_or(0).max(0);
            if world_id != self.world_id {
                self.world_id = world_id;
                self.continent = None;
                match self.atlas.world(&mut self.vfs, world_id) {
                    Ok(world) => {
                        self.world = world;
                        self.world_error = None;
                    }
                    Err(error) => {
                        eprintln!("World map: {error}");
                        self.world_error = Some(error.to_string());
                    }
                }
            }
            self.world_available = self.world_error.is_none();
        }
        if new_area && town_id != self.town_id {
            self.town_id = town_id;
            self.town = match self.atlas.local(&mut self.vfs, town_id) {
                Ok(town) => {
                    self.town_error = None;
                    town
                }
                Err(error) => {
                    eprintln!("Local map: {error}");
                    self.town_error = Some(error.to_string());
                    None
                }
            };
        }
        let overworld = current.is_some_and(|map| map.id == self.atlas.start_map);
        self.can_travel = current.is_some_and(|map| {
            self.atlas
                .travel_available_from(self.world_id, map.id, town_id)
        }) && self
            .travel_map
            .as_ref()
            .is_ok_and(|data| data.can_depart(tile));
        if (new_area || new_sector) && overworld && self.town.is_some() {
            // The overworld sector origin is in the same coordinates as GameArea.
            self.atlas.discover_near((tile.0 & !63, tile.1 & !63));
        }
        if let Some(town) = &self.town
            && let Some(index) = town.pixel(tile).and_then(|p| town.sheet.index(p))
        {
            self.explored.entry(town_id).or_default().insert(index);
        }
        if new_area {
            self.invalidate_sheet();
        }
        self.dirty = true;
    }

    fn invalidate_sheet(&mut self) {
        self.bitmaps.clear();
        self.bitmap_bytes = 0;
        self.dirty = true;
    }

    pub(super) fn is_open(&self) -> bool {
        self.open
    }

    pub(super) fn open(&mut self, scale: Option<u16>) {
        if let Some(scale) = scale {
            self.hud.set_scale(scale);
        }
        self.open = true;
        self.waypoints.clear();
        self.active_waypoint = None;
        self.atlas.destination = None;
        self.travel_message = None;
        let mode = if self.town.is_some() || !self.world_available {
            Mode::Local
        } else {
            Mode::World
        };
        self.switch(mode);
    }

    pub(super) fn toggle_overview(&mut self, detail: bool) {
        if self.travelling() {
            return;
        }
        if detail {
            self.switch(Mode::World);
        } else if self.mode == Mode::Continent {
            self.switch(if self.town.is_some() {
                Mode::Local
            } else {
                Mode::World
            });
        } else {
            self.switch(Mode::Continent);
        }
    }

    pub(super) fn close(&mut self) {
        self.open = false;
        self.journey = None;
        self.cancel_drag();
        self.hud.clear_layout();
    }

    fn sheet(&self) -> Option<&Sheet> {
        match self.mode {
            Mode::Local => Some(&self.town.as_ref()?.sheet),
            Mode::World | Mode::Continent => self.world_available.then_some(&self.world),
        }
    }

    fn player_pixel(&self) -> Option<Point> {
        match self.mode {
            Mode::Local => self.town.as_ref()?.pixel(self.tile?),
            Mode::World | Mode::Continent => {
                let tile = self
                    .journey
                    .as_ref()
                    .and_then(|ride| ride.traversed().last().copied())
                    .or(self.world_tile)?;
                world_pixel(tile, self.world.size().0)
            }
        }
    }

    fn switch(&mut self, mode: Mode) {
        if self.mode != mode {
            self.invalidate_sheet();
        }
        if mode == Mode::Continent && self.continent.is_none() && self.world_available {
            match self.atlas.overview(&mut self.vfs, self.world_id) {
                Ok(overview) => self.continent = Some(overview),
                Err(error) => eprintln!("Continent overview: {error}"),
            }
        }
        self.mode = mode;
        self.hud.set_map_view(mode);
        self.hud.set_selected_action(if mode == Mode::Continent {
            Action::MapOverview
        } else {
            Action::MapLocal
        });
        self.zoom = 2;
        self.recenter();
    }

    fn recenter(&mut self) {
        let extent = self.sheet().map_or((0, 0), Sheet::size);
        let center = (self.mode != Mode::Continent)
            .then(|| self.player_pixel())
            .flatten()
            .unwrap_or((extent.0 / 2, extent.1 / 2));
        self.center = (f64::from(center.0), f64::from(center.1));
        self.dirty = true;
    }

    fn transform(&self) -> Option<Transform> {
        let (rect, percent) = self.bounds?;
        let scale = if self.mode == Mode::Continent {
            let (w, h) = self.world.size();
            (f64::from(rect.w) / f64::from(w)).min(f64::from(rect.h) / f64::from(h))
        } else {
            f64::from(percent) / 100. * ZOOMS[self.zoom]
        };
        Some(Transform {
            rect,
            center: self.center,
            scale,
            ui_scale: percent,
        })
    }

    fn pan(&mut self, dx: f64, dy: f64) {
        if self.mode == Mode::Continent {
            return;
        }
        if let Some(transform) = self.transform()
            && let Some(sheet) = self.sheet()
        {
            let (w, h) = sheet.size();
            let clamp = |value: f64, extent: u32, view: u32| {
                let half = (f64::from(view) / (2. * transform.scale)).min(f64::from(extent) / 2.);
                value.clamp(half, f64::from(extent) - half)
            };
            self.center = (
                clamp(self.center.0 + dx / transform.scale, w, transform.rect.w),
                clamp(self.center.1 + dy / transform.scale, h, transform.rect.h),
            );
            self.dirty = true;
        }
    }

    pub(super) fn pan_keys(&mut self, dx: i32, dy: i32) {
        self.pan(f64::from(dx), f64::from(dy));
    }
    pub(super) fn home(&mut self) {
        self.recenter();
    }

    pub(super) fn zoom(&mut self, step: i32) {
        if self.mode == Mode::Continent {
            if step > 0 {
                self.switch(Mode::World);
            }
            return;
        }

        let zoom = (self.zoom as i32 + step).clamp(0, (ZOOMS.len() - 1) as i32) as usize;
        if self.zoom != zoom {
            self.zoom = zoom;
            self.pan(0., 0.);
        }
    }

    pub(super) fn adjust_scale(&mut self, step: i16) -> u16 {
        self.hud.adjust_scale(step);
        self.hud.scale()
    }

    fn action(&mut self, action: Action, now: Duration) -> Option<Arrival> {
        if self.journey.is_some() && action != Action::MapTravel {
            return None;
        }
        match action {
            Action::MapLocal if self.town.is_some() => self.switch(Mode::Local),
            Action::MapLocal | Action::MapWorld => self.switch(Mode::World),
            Action::MapZoomIn => self.zoom(1),
            Action::MapOverview => self.switch(Mode::Continent),
            Action::MapRecenter => self.recenter(),
            Action::MapTravel => {
                if self.journey.is_some() {
                    return self.cancel_journey();
                }
                self.start_journey(now);
            }
            _ => {}
        }
        None
    }

    pub(super) fn can_travel(&self) -> bool {
        self.can_travel
    }

    pub(super) fn travelling(&self) -> bool {
        self.journey.is_some()
    }

    pub(super) fn travel_deadline(&self) -> Option<Duration> {
        self.journey.as_ref().map(Journey::deadline)
    }

    fn arrival_waypoint(&self) -> Option<&Waypoint> {
        let map = self.travel_map.as_ref().ok()?;
        self.waypoints
            .last()
            .filter(|point| map.has_sector(point.tile))
    }

    fn start_journey(&mut self, now: Duration) {
        if !self.can_travel || self.mode != Mode::World {
            return;
        }
        let Some(last) = self.arrival_waypoint() else {
            return;
        };
        let steps: Vec<_> = self
            .waypoints
            .iter()
            .flat_map(|point| point.steps.iter().copied())
            .collect();
        eprintln!(
            "World travel: {} waypoint(s), {} sector steps, arrival {:?}",
            self.waypoints.len(),
            steps.len(),
            last.tile
        );
        self.journey = Journey::new(steps, now);
        self.travel_message = None;
        self.dirty = true;
    }

    pub(super) fn advance_travel(&mut self, now: Duration) -> (usize, Option<Arrival>) {
        let Some(journey) = &mut self.journey else {
            return (0, None);
        };
        let old_count = journey.traversed().len();
        let steps = journey.advance(now);
        for &tile in &journey.traversed()[old_count..] {
            self.atlas.discover_near(tile);
        }
        let finished = journey.finished();
        if steps > 0 {
            self.recenter();
        }
        let arrival = finished.then(|| {
            self.journey = None;
            Arrival::Destination(self.atlas.start_map, self.waypoints.last().unwrap().tile)
        });
        (steps, arrival)
    }

    pub(super) fn cancel_journey(&mut self) -> Option<Arrival> {
        let journey = self.journey.take()?;
        let stop = journey.traversed().last().copied();
        self.dirty = true;
        eprintln!(
            "World travel cancelled after {} sector steps",
            journey.traversed().len()
        );
        Some(Arrival::Cancelled(self.atlas.start_map, stop))
    }

    pub(super) fn travel_notice(&mut self, message: &str) {
        eprintln!("{message}");
        self.travel_message = Some(message.into());
    }

    fn append_waypoint(&mut self, tile: Point, area: Option<i32>) -> Result<(), String> {
        if !self.can_travel {
            return Err(self
                .travel_map
                .as_ref()
                .err()
                .cloned()
                .unwrap_or_else(|| "World travel is unavailable from this location.".into()));
        }
        if self.waypoints.len() == 30 {
            return Err("The route already has 30 waypoints.".into());
        }
        let from = self
            .waypoints
            .last()
            .map(|point| point.tile)
            .or(self.world_tile)
            .ok_or("Player has no world-map position.")?;
        let steps = self
            .travel_map
            .as_ref()
            .map_err(String::as_str)?
            .route(from, tile)?;
        if steps.is_empty() {
            return Err("That waypoint is in the current route sector.".into());
        }
        if self
            .waypoints
            .iter()
            .map(|point| point.steps.len())
            .sum::<usize>()
            + steps.len()
            > 5000
        {
            return Err("The route exceeds 5000 sector steps.".into());
        }
        self.waypoints.push(Waypoint { tile, area, steps });
        self.active_waypoint = None;
        self.travel_message = None;
        self.dirty = true;
        Ok(())
    }

    fn replace_waypoints(&mut self, old: Vec<Waypoint>, changed: Vec<(Point, Option<i32>)>) {
        let previous_selection = self.atlas.destination.clone();
        let result = changed
            .into_iter()
            .try_for_each(|(tile, area)| self.append_waypoint(tile, area));
        if let Err(error) = result {
            self.waypoints = old;
            self.atlas.destination = previous_selection;
            self.travel_notice(&error);
        } else {
            self.atlas.destination = None;
            if let Some(id) = self.waypoints.last().and_then(|point| point.area) {
                let _ = self.atlas.select(id, &self.vfs);
            }
            self.travel_message = None;
            self.dirty = true;
        }
    }

    pub(super) fn remove_waypoint(&mut self, point: Option<Point>) -> Option<Arrival> {
        if self.travelling() {
            return self.cancel_journey();
        }
        if let Some(index) = point
            .and_then(|p| self.pick_waypoint(p))
            .or(self.active_waypoint.take())
            .or_else(|| self.waypoints.len().checked_sub(1))
        {
            let old = std::mem::take(&mut self.waypoints);
            let changed = old
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != index)
                .map(|(_, point)| (point.tile, point.area))
                .collect();
            self.replace_waypoints(old, changed);
        }
        None
    }

    fn pick_waypoint(&self, point: Point) -> Option<usize> {
        let transform = self.transform()?;
        if self.mode != Mode::World || !transform.rect.contains(point) {
            return None;
        }
        let radius = i64::from((6 * transform.ui_scale / 100).max(4));
        self.waypoints
            .iter()
            .enumerate()
            .filter_map(|(i, waypoint)| {
                let (x, y) = transform.screen(world_pixel(waypoint.tile, self.world.size().0)?);
                let d = (i64::from(x) - i64::from(point.0))
                    .abs()
                    .max((i64::from(y) - i64::from(point.1)).abs());
                (d <= radius).then_some((d, i))
            })
            .min()
            .map(|(_, i)| i)
    }

    fn place_waypoint(&mut self, point: Point) {
        if self.active_waypoint.is_none()
            && let Some(i) = self.pick_waypoint(point)
        {
            self.active_waypoint = Some(i);
            self.dirty = true;
            return;
        }
        let area = self.pick(point);
        let previous_selection = self.atlas.destination.clone();
        let tile = if let Some(id) = area {
            match self.atlas.select(id, &self.vfs) {
                Ok(()) => self.atlas.areas[&id].tile,
                Err(error) => {
                    self.travel_notice(&error.to_string());
                    return;
                }
            }
        } else {
            let Some(tile) = self
                .transform()
                .and_then(|t| t.at(point))
                .filter(|&p| self.world.index(p).is_some())
                .and_then(|p| travel::sector_center(p, self.world.size().0))
            else {
                return;
            };
            tile
        };
        if let Some(index) = self.active_waypoint.take() {
            self.atlas.destination = previous_selection;
            let old = std::mem::take(&mut self.waypoints);
            let changed = old
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    if i == index {
                        (tile, area)
                    } else {
                        (p.tile, p.area)
                    }
                })
                .collect();
            self.replace_waypoints(old, changed);
        } else if let Err(error) = self.append_waypoint(tile, area) {
            self.atlas.destination = previous_selection;
            self.travel_notice(&error);
        } else if let Some(id) = area {
            eprintln!("Map destination {:?}", self.atlas.areas[&id]);
        } else {
            self.atlas.destination = None;
        }
    }

    pub(super) fn focus(&mut self, backwards: bool) {
        self.hud.advance_focus(backwards);
    }
    pub(super) fn activate_focus(&mut self, now: Duration) -> Option<Arrival> {
        self.action(self.hud.focused_action()?, now)
    }

    pub(super) fn mouse_down(&mut self, point: Point) {
        self.suppress_click = false;
        if self.hud.press(point) {
            return;
        }
        self.drag = self
            .bounds
            .filter(|(rect, _)| rect.contains(point))
            .map(|_| (point, point));
    }

    pub(super) fn mouse_up(&mut self, point: Point, now: Duration) -> Option<Arrival> {
        let on_canvas = self.drag.take().is_some();
        let action = self.hud.release(point);
        if self.suppress_click {
            return None;
        }
        if let Some(action) = action {
            return self.action(action, now);
        }
        if on_canvas && !matches!(self.hud.hit(point), Hit::Button(_)) && !self.travelling() {
            if self.mode == Mode::World {
                self.place_waypoint(point);
            } else if self.mode == Mode::Continent
                && let Some(pixel) = self
                    .transform()
                    .and_then(|t| t.at(point))
                    .filter(|&p| self.world.index(p).is_some())
            {
                self.switch(Mode::World);
                self.center = (f64::from(pixel.0), f64::from(pixel.1));
            }
        }
        None
    }

    pub(super) fn cursor(&mut self, point: Option<Point>) {
        if let Some(point) = point
            && let Some((start, last)) = self.drag
            && (self.suppress_click || start.0.abs_diff(point.0).max(start.1.abs_diff(point.1)) > 4)
        {
            self.pan(
                f64::from(last.0) - f64::from(point.0),
                f64::from(last.1) - f64::from(point.1),
            );
            self.suppress_click = true;
            self.drag = Some((start, point));
        }
        self.hud.hover(point);
    }

    pub(super) fn cancel_drag(&mut self) {
        self.drag = None;
        self.hud.cancel_press();
    }

    fn pick(&self, point: Point) -> Option<i32> {
        let transform = self.transform()?;
        if self.mode == Mode::Local || !self.world_available || !transform.rect.contains(point) {
            return None;
        }
        let marker = if self.mode == Mode::Continent {
            Binding::ContinentPlaceMarker
        } else {
            Binding::PlaceMarker
        };
        let marker = self.hud.marker(marker)?;
        let radius = u64::from((marker.width.max(marker.height) * transform.ui_scale / 200).max(4));
        self.atlas
            .areas
            .values()
            .filter_map(|area| {
                if !self.inspect_all && !self.atlas.discovered.contains(&area.id) {
                    return None;
                }
                let pixel = area.pixel(self.world.size().0)?;
                self.world.index(pixel)?;
                let (x, y) = transform.screen(pixel);
                let pos = (i64::from(x), i64::from(y));
                let distance = pos
                    .0
                    .abs_diff(point.0.into())
                    .max(pos.1.abs_diff(point.1.into()));
                (distance <= radius).then_some((distance, area.id))
            })
            .min()
            .map(|(_, id)| id)
    }

    pub(super) fn prepare(&mut self, size: Point, cursor: Option<Point>) -> &mut Hud {
        self.hud.set_unavailable_action(
            (!self.travelling() && (!self.can_travel || self.arrival_waypoint().is_none()))
                .then_some(Action::MapTravel),
        );
        self.hud
            .set_active_action(self.travelling().then_some(Action::MapTravel));
        self.hud.layout(size);
        let bounds = self.hud.bounds(Binding::MapCanvas);
        if self.bounds != bounds {
            self.bounds = bounds;
            self.dirty = true;
        }
        if let Some(t) = self.transform()
            && self.dirty
        {
            self.paint(t);
            self.dirty = false;
        }
        let title = match self.mode {
            Mode::World | Mode::Continent => "World map",
            Mode::Local => self.town.as_ref().map_or(&self.location, |town| &town.name),
        };
        self.hud.set_label(Binding::MapTitle, title);
        let cursor_pixel =
            cursor
                .and_then(|point| self.transform()?.at(point))
                .filter(|&(x, y)| {
                    self.sheet()
                        .is_some_and(|s| x < s.size().0 && y < s.size().1)
                });
        // CE leaves the last world readout in place when pointing into a town map.
        if self.mode != Mode::Local
            && let Some(point) = cursor_pixel
        {
            self.readout = point;
        }
        let (x, y) = format_world_readout(self.world.size().0, self.readout, self.atlas.offset);
        self.hud.set_label(Binding::MapWest, &x);
        self.hud.set_label(Binding::MapSouth, &y);
        let hover = cursor.and_then(|point| self.pick(point));
        let status = hover.and_then(|id| self.atlas.areas.get(&id)).map_or_else(
            || {
                self.town_error
                    .as_deref()
                    .or(self.world_error.as_deref())
                    .unwrap_or("")
            },
            |area| &area.description,
        );
        let arrival_status = (!self.waypoints.is_empty() && self.arrival_waypoint().is_none())
            .then_some("No on-foot terrain is available at the final waypoint.");
        self.hud.set_label(
            Binding::MapStatus,
            self.travel_message
                .as_deref()
                .or(arrival_status)
                .unwrap_or(status),
        );
        &mut self.hud
    }

    fn paint(&mut self, t: Transform) {
        let mut canvas = SpriteFrame {
            width: t.rect.w,
            height: t.rect.h,
            hotspot: (0, 0),
            offset: (0, 0),
            pixels: vec![[0, 0, 0, 255]; (t.rect.w * t.rect.h) as usize],
        };
        if let Some(sheet) = self.sheet().cloned() {
            if self.mode == Mode::Continent {
                if let Some(overview) = &self.continent {
                    paint_overview(&mut canvas, t, sheet.size(), overview);
                }
            } else {
                self.paint_sheet(t, &sheet, &mut canvas);
            }
            if self.mode != Mode::Local {
                for area in self
                    .atlas
                    .areas
                    .values()
                    .filter(|a| self.inspect_all || self.atlas.discovered.contains(&a.id))
                {
                    if let Some(pixel) = area.pixel(sheet.size().0)
                        && sheet.index(pixel).is_some()
                    {
                        let selected = self
                            .atlas
                            .destination
                            .as_ref()
                            .is_some_and(|d| d.area == area.id);
                        let marker = match (self.mode, selected) {
                            (Mode::Continent, true) => Binding::ContinentSelectedMarker,
                            (Mode::Continent, false) => Binding::ContinentPlaceMarker,
                            (_, true) => Binding::SelectedMarker,
                            (_, false) => Binding::PlaceMarker,
                        };
                        draw_marker(&mut canvas, t, pixel, self.hud.marker(marker));
                        if self.mode == Mode::World {
                            let (x, y) = t.screen(pixel);
                            let offset = |n: i32| {
                                (i64::from(n) * i64::from(t.ui_scale) / 100)
                                    .clamp(-100_000, 100_000) as i32
                            };
                            self.hud.draw_map_label(
                                &mut canvas,
                                (
                                    x - t.rect.x + offset(area.label_offset.0),
                                    y - t.rect.y + offset(area.label_offset.1),
                                ),
                                t.ui_scale,
                                &area.name,
                            );
                        }
                    }
                }
            }
            if self.mode != Mode::Local {
                self.paint_route(&mut canvas, t);
            }
            if let Some(pixel) = self.player_pixel()
                && sheet.index(pixel).is_some()
            {
                let marker = if self.mode == Mode::Continent {
                    Binding::ContinentPlayerMarker
                } else {
                    Binding::PlayerMarker
                };
                draw_marker(&mut canvas, t, pixel, self.hud.marker(marker));
            }
        }
        self.hud.set_canvas(canvas);
    }

    fn paint_route(&self, canvas: &mut SpriteFrame, t: Transform) {
        let width = self.world.size().0;
        let passed = self
            .journey
            .as_ref()
            .map_or(0, |travel| travel.traversed().len());
        let points = || {
            let mut cumulative = 0;
            self.waypoints
                .iter()
                .enumerate()
                .filter_map(move |(index, waypoint)| {
                    cumulative += waypoint.steps.len();
                    (cumulative > passed)
                        .then(|| world_pixel(waypoint.tile, width))
                        .flatten()
                        .map(|pixel| (index, pixel))
                })
        };
        if let Some(start) = self.player_pixel() {
            let mut previous = t.screen(start);
            for (index, pixel) in points() {
                let next = t.screen(pixel);
                if let Some(color) = self.hud.route_color(self.active_waypoint == Some(index)) {
                    dashed_segment(canvas, t.rect, previous, next, color);
                }
                previous = next;
            }
        }
        for (index, pixel) in points() {
            let binding = if self.active_waypoint == Some(index) {
                Binding::SelectedMarker
            } else {
                Binding::WaypointMarker
            };
            draw_marker(canvas, t, pixel, self.hud.marker(binding));
        }
    }

    fn paint_sheet(&mut self, t: Transform, sheet: &Sheet, canvas: &mut SpriteFrame) {
        let origin = t.origin();
        let size = sheet.size();
        let (tw, th) = sheet.chunk;
        let limits = |start: f64, pixels: u32, extent: u32, chunk: u32| {
            let first = (start.max(0.) as u32).min(extent) / chunk;
            let last =
                (((start + f64::from(pixels) / t.scale).ceil() as u32).min(extent)).div_ceil(chunk);
            first..last
        };
        let columns = limits(origin.0, t.rect.w, size.0, tw);
        let rows = limits(origin.1, t.rect.h, size.1, th);
        if columns.len().saturating_mul(rows.len()) > MAX_VISIBLE_CHUNKS as usize {
            return;
        }
        // A sheet-local bounded cache. Decoded pixels are independent of
        // UI scale, scrolling, world streaming and GPU resources.
        let visited = self.explored.get(&self.town_id);
        let reveal_all =
            self.mode == Mode::World || self.town.as_ref().is_some_and(|town| town.waitable);
        for row in rows {
            for col in columns.clone() {
                let index = row * sheet.grid.0 + col;
                let flags = if reveal_all {
                    15
                } else {
                    visible_corners(visited, sheet.grid, (col, row))
                };
                if flags == 0 {
                    continue;
                }
                if !self.bitmaps.contains_key(&index) {
                    let tile_bytes = (tw * th * 4) as usize;
                    while self.bitmap_bytes + tile_bytes > BITMAP_BUDGET {
                        if let Some((_, Some(tile))) = self.bitmaps.pop_first() {
                            self.bitmap_bytes -= tile.pixels.len() * 4;
                        }
                    }
                }
                let tile = self.bitmaps.entry(index).or_insert_with(|| {
                    match sheet.load(&mut self.vfs, index) {
                        Ok(bitmap) if bitmap.size == sheet.chunk => {
                            self.bitmap_bytes += bitmap.pixels.len() * 4;
                            Some(bitmap)
                        }
                        Ok(_) => {
                            eprintln!("Atlas tile {index} does not match declared dimensions");
                            None
                        }
                        Err(error) => {
                            // Local bitmaps are sparsely authored. Preserve empty
                            // cells and report other format or read failures.
                            if self.mode == Mode::World || !error.to_string().contains("not found")
                            {
                                eprintln!("Atlas tile {index}: {error}");
                            }
                            None
                        }
                    }
                });
                if let Some(tile) = tile {
                    paint_tile(canvas, t, (col * tw, row * th), tile, flags);
                }
            }
        }
    }
}

fn dashed_segment(
    canvas: &mut SpriteFrame,
    rect: Rect,
    a: (i32, i32),
    b: (i32, i32),
    color: [u8; 4],
) {
    // TIG starts at the upper (or leftmost) endpoint and uses one pixel of width.
    let (a, b) = if (a.1, a.0) <= (b.1, b.0) {
        (a, b)
    } else {
        (b, a)
    };
    let (mut x, mut y) = a;
    let (dx, dy) = ((b.0 - a.0).abs(), b.1 - a.1);
    let sx = (b.0 - a.0).signum();
    let (major, minor) = (dx.max(dy), dx.min(dy));
    let mut error = 2 * minor - major;
    for i in 0..=major {
        let (column, row) = (x - rect.x, y - rect.y);
        if (i == 0 || (i - 1) % 6 < 3)
            && column >= 0
            && row >= 0
            && column < rect.w as i32
            && row < rect.h as i32
        {
            canvas.pixels[row as usize * rect.w as usize + column as usize] = color;
        }
        let diagonal = error > 0;
        error += 2 * minor;
        if diagonal {
            error -= 2 * major;
        }
        if dx >= dy || diagonal {
            x += sx;
        }
        if dy > dx || diagonal {
            y += 1;
        }
    }
}

fn format_world_readout(width: u32, (x, y): Point, offset: (i32, i32)) -> (String, String) {
    (
        format!(
            "{} W",
            i64::from(width) - i64::from(x) - i64::from(offset.0)
        ),
        format!("{} S", i64::from(y) + i64::from(offset.1)),
    )
}

fn paint_overview(canvas: &mut SpriteFrame, t: Transform, extent: Point, image: &Bitmap) {
    let (width, height) = extent;
    for y in 0..canvas.height {
        for x in 0..canvas.width {
            let point = (t.rect.x as u32 + x, t.rect.y as u32 + y);
            if let Some((mx, my)) = t.at(point)
                && mx < width
                && my < height
            {
                let (sx, sy) = (mx * image.size.0 / width, my * image.size.1 / height);
                canvas.pixels[(y * canvas.width + x) as usize] =
                    image.pixels[(sy * image.size.0 + sx) as usize];
            }
        }
    }
}

fn visible_corners(visited: Option<&BTreeSet<u32>>, (w, h): Point, (x, y): Point) -> u8 {
    let Some(visited) = visited else {
        return 0;
    };
    if visited.contains(&(y * w + x)) {
        return 15;
    }
    let mut flags = 0;
    for (dy, weights) in [(-1, [8, 12, 4]), (0, [9, 0, 6]), (1, [1, 3, 2])] {
        for (dx, weight) in [-1, 0, 1].into_iter().zip(weights) {
            if let (Some(x), Some(y)) = (x.checked_add_signed(dx), y.checked_add_signed(dy))
                && x < w
                && y < h
                && visited.contains(&(y * w + x))
            {
                flags |= weight;
            }
        }
    }
    flags
}

fn paint_tile(
    canvas: &mut SpriteFrame,
    transform: Transform,
    pos: Point,
    tile: &Bitmap,
    flags: u8,
) {
    let origin = transform.origin();
    let edge = |coordinate: u32, cam: f64| {
        ((f64::from(coordinate) - cam) * transform.scale - 0.5).ceil() as i32
    };
    let x0 = edge(pos.0, origin.0).clamp(0, canvas.width as i32);
    let y0 = edge(pos.1, origin.1).clamp(0, canvas.height as i32);
    let x1 = edge(pos.0 + tile.size.0, origin.0).clamp(0, canvas.width as i32);
    let y1 = edge(pos.1 + tile.size.1, origin.1).clamp(0, canvas.height as i32);
    let (w, h) = tile.size;
    for y in y0..y1 {
        let py = ((origin.1 + (f64::from(y) + 0.5) / transform.scale).floor() as u32)
            .saturating_sub(pos.1)
            .min(h - 1);
        for x in x0..x1 {
            let px = ((origin.0 + (f64::from(x) + 0.5) / transform.scale).floor() as u32)
                .saturating_sub(pos.0)
                .min(w - 1);
            let mut color = tile.pixels[(py * w + px) as usize];
            if flags != 15 {
                let z = |bit| u32::from(flags & bit != 0);
                let factor = (w - px) * (h - py) * z(8)
                    + px * (h - py) * z(4)
                    + px * py * z(2)
                    + (w - px) * py * z(1);
                for c in &mut color[..3] {
                    *c = (u32::from(*c) * factor / (w * h)) as u8;
                }
            }
            canvas.pixels[(y as u32 * canvas.width + x as u32) as usize] = color;
        }
    }
}

fn draw_marker(canvas: &mut SpriteFrame, t: Transform, pixel: Point, frame: Option<&SpriteFrame>) {
    let Some(frame) = frame else {
        return;
    };
    let w = (frame.width * t.ui_scale / 100).max(1);
    let h = (frame.height * t.ui_scale / 100).max(1);
    let (x, y) = t.screen(pixel);
    let x = x - t.rect.x - w as i32 / 2;
    let y = y - t.rect.y - h as i32 / 2;
    for sy in 0..h {
        for sx in 0..w {
            let px = x + sx as i32;
            let py = y + sy as i32;
            if px >= 0 && py >= 0 && px < canvas.width as i32 && py < canvas.height as i32 {
                let color = frame.pixels
                    [((sy * frame.height / h) * frame.width + sx * frame.width / w) as usize];
                if color[3] != 0 {
                    canvas.pixels[(py as u32 * canvas.width + px as u32) as usize] = color;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coordinate_readouts_use_original_west_and_south_format() {
        assert_eq!(
            format_world_readout(2000, (0, 0), (0, 0)),
            ("2000 W".into(), "0 S".into())
        );
        assert_eq!(
            format_world_readout(2000, (548, 1290), (2, 3)),
            ("1450 W".into(), "1293 S".into())
        );
    }

    #[test]
    fn map_zoom_and_ui_scale_share_the_same_marker_and_mouse_transform() {
        for (ui_scale, scale) in [(100, 1.), (125, 1.25), (150, 1.5), (200, 2.)] {
            let transform = Transform {
                rect: Rect {
                    x: 120,
                    y: 56,
                    w: 500,
                    h: 400,
                },
                center: (548., 1290.),
                scale,
                ui_scale,
            };
            let player = (548, 1290);
            let (x, y) = transform.screen(player);
            let picked = transform.at((x as u32, y as u32)).unwrap();
            assert_eq!(picked, player);
            let tile = travel::sector_center(picked, 2000).unwrap();
            assert_eq!(world_pixel(tile, 2000), Some(player));
            assert_eq!(transform.at((119, y as u32)), None);
        }
    }

    #[test]
    fn route_marks_are_clipped_to_the_scaled_canvas() {
        let rect = Rect {
            x: 20,
            y: 30,
            w: 10,
            h: 3,
        };
        let mut canvas = SpriteFrame {
            width: rect.w,
            height: rect.h,
            hotspot: (0, 0),
            offset: (0, 0),
            pixels: vec![[0; 4]; 30],
        };
        let red = [180, 50, 50, 255];
        dashed_segment(&mut canvas, rect, (14, 31), (36, 31), red);
        assert!(canvas.pixels[10..20].contains(&red));
        assert!(
            canvas.pixels[..10]
                .iter()
                .chain(&canvas.pixels[20..])
                .all(|&p| p == [0; 4])
        );
    }

    #[test]
    fn waypoint_segments_follow_pan_and_zoom_at_fractional_ui_scales() {
        let rect = Rect {
            x: 120,
            y: 70,
            w: 100,
            h: 80,
        };
        let red = [160, 50, 50, 255];
        let blue = [50, 50, 110, 255];
        for (ui_scale, scale, center) in [(100, 1., (50., 50.)), (125, 2.5, (51., 49.))] {
            let transform = Transform {
                rect,
                center,
                scale,
                ui_scale,
            };
            let mut canvas = SpriteFrame {
                width: rect.w,
                height: rect.h,
                hotspot: (0, 0),
                offset: (0, 0),
                pixels: vec![[0; 4]; (rect.w * rect.h) as usize],
            };
            let [a, b, c] = [(40, 45), (58, 45), (58, 55)].map(|p| transform.screen(p));
            dashed_segment(&mut canvas, rect, a, b, red);
            dashed_segment(&mut canvas, rect, b, c, blue);
            let pixel = |(x, y): (i32, i32)| {
                canvas.pixels[((y - rect.y) * rect.w as i32 + x - rect.x) as usize]
            };
            assert_eq!(pixel(a), red);
            assert_eq!(pixel((a.0 + 3, a.1)), red);
            assert_eq!(pixel((a.0 + 4, a.1)), [0; 4]);
            assert_eq!(pixel(b), blue);
            assert_eq!(pixel((b.0, b.1 + 3)), blue);
            assert_eq!(pixel((b.0 - 1, b.1 + 3)), [0; 4]);
        }
    }

    #[test]
    fn visited_tiles_light_only_their_original_neighbor_corners() {
        let visited = BTreeSet::from([0u32]);
        assert_eq!(visible_corners(Some(&visited), (3, 3), (0, 0)), 15);
        assert_eq!(visible_corners(Some(&visited), (3, 3), (1, 0)), 9);
        assert_eq!(visible_corners(Some(&visited), (3, 3), (1, 1)), 8);
        assert_eq!(visible_corners(Some(&visited), (3, 3), (2, 0)), 0);
    }
}
