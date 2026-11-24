//! The modal atlas uses the same layout, typography and compositor as the HUD.
use super::hud::{Action, Binding, Hit, Hud, MapView as Mode, Rect};
use arcanum_rs::{
    art::SpriteFrame,
    content::map_atlas::{Atlas, Bitmap, Point, Sheet, Town, world_pixel},
    vfs::Vfs,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    path::Path,
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
    ) -> Result<Self, Box<dyn Error>> {
        let atlas = Atlas::load(&mut vfs)?;
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
        if (new_area || new_sector)
            && current.is_some_and(|map| map.id == self.atlas.start_map)
            && self.town.is_some()
        {
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
        let mode = if self.town.is_some() || !self.world_available {
            Mode::Local
        } else {
            Mode::World
        };
        self.switch(mode);
    }

    pub(super) fn lens_size(&self) -> Option<(u32, u32)> {
        self.hud.lens_size().map(|[w, h]| (w * 2, h * 2))
    }

    pub(super) fn set_lens(&mut self, frame: &SpriteFrame) {
        self.hud.set_lens(frame);
    }

    pub(super) fn toggle_overview(&mut self, detail: bool) {
        if detail {
            self.switch(Mode::World);
        } else if self.mode == Mode::Continent {
            self.action(Action::MapLocal);
        } else {
            self.switch(Mode::Continent);
        }
    }

    pub(super) fn close(&mut self) {
        self.open = false;
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
            Mode::World | Mode::Continent => world_pixel(self.world_tile?, self.world.size().0),
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

    fn action(&mut self, action: Action) {
        match action {
            Action::MapLocal if self.town.is_some() => self.switch(Mode::Local),
            Action::MapLocal | Action::MapWorld => self.switch(Mode::World),
            Action::MapZoomIn => self.zoom(1),
            Action::MapOverview => self.switch(Mode::Continent),
            Action::MapRecenter => self.recenter(),
            _ => {}
        }
    }

    pub(super) fn focus(&mut self, backwards: bool) {
        self.hud.advance_focus(backwards);
    }
    pub(super) fn activate_focus(&mut self) {
        if let Some(action) = self.hud.focused_action() {
            self.action(action);
        }
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

    pub(super) fn mouse_up(&mut self, point: Point) {
        let on_canvas = self.drag.take().is_some();
        let action = self.hud.release(point);
        if self.suppress_click {
            return;
        }
        if let Some(action) = action {
            self.action(action);
        } else if on_canvas
            && !matches!(self.hud.hit(point), Hit::Button(_))
            && let Some(id) = self.pick(point)
        {
            match self.atlas.select(id, &self.vfs) {
                Ok(()) => {
                    if let Some(selection) = &self.atlas.destination {
                        eprintln!("Map destination {selection:?}");
                    }
                    self.dirty = true;
                }
                Err(error) => eprintln!("Map destination: {error}"),
            }
        }
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

    pub(super) fn prepare(&mut self, size: Point, cursor: Option<Point>) -> &Hud {
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
        self.hud.set_label(Binding::MapStatus, status);
        &self.hud
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
            assert_eq!(transform.at((x as u32, y as u32)), Some(player));
            assert_eq!(transform.at((119, y as u32)), None);
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
