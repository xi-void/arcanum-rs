//! Connect typed developer requests to the existing playable runtime.
use super::{console::Console, *};
use arcanum_rs::{
    content::{
        map_atlas::{Atlas, world_pixel},
        world::{DebugObject, DeveloperAction},
    },
    developer::{self, Output, Request},
    vfs::Vfs,
};
use std::path::{Path, PathBuf};

pub(crate) struct Developer {
    console: Console,
    vfs: Vfs,
    atlas: Atlas,
    console_theme: PathBuf,
    hud_theme: Option<PathBuf>,
    map_theme: PathBuf,
    scale: u16,
    selected: Option<DebugObject>,
    collision: bool,
    sectors: bool,
    fps: bool,
    marks: Marks,
    status: Option<(String, Arc<LoadedFrame>)>,
}

impl Developer {
    pub(crate) fn load(
        mut vfs: Vfs,
        console_theme: PathBuf,
        hud_theme: Option<PathBuf>,
        map_theme: PathBuf,
        scale: u16,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let console = Console::load(&console_theme, &mut vfs)?;
        let atlas = Atlas::load(&mut vfs)?;
        Ok(Self {
            console,
            vfs,
            atlas,
            console_theme,
            hud_theme,
            map_theme,
            scale,
            selected: None,
            collision: false,
            sectors: false,
            fps: false,
            marks: Marks::new(),
            status: None,
        })
    }

    fn resolve(
        &mut self,
        name: &str,
        tile: Option<(u32, u32)>,
    ) -> Result<(Destination, String), String> {
        let lookup = name.trim().to_lowercase().replace('’', "'");
        let exact = self
            .atlas
            .catalog
            .maps
            .iter()
            .any(|map| map.directory == lookup || lookup.parse::<i32>() == Ok(map.id));
        let (map, authored, label) = if exact {
            let map = self.atlas.catalog.find(name)?;
            (map, None, map.name.clone())
        } else {
            let mut areas = self
                .atlas
                .areas
                .values()
                .filter(|area| area.name.to_lowercase().replace('’', "'") == lookup);
            if let Some(area) = areas.next() {
                if areas.next().is_some() {
                    return Err(
                        "Ambiguous area name; use a numeric map ID and explicit coordinates".into(),
                    );
                }
                let map = self.atlas.catalog.new_game()?;
                (
                    map,
                    Some(area.tile),
                    format!("{} (world area on map {})", area.name, map.id),
                )
            } else {
                let map = self.atlas.catalog.find(name)?;
                (map, None, map.name.clone())
            }
        };
        let files = map.files(&mut self.vfs).map_err(|err| err.to_string())?;
        let tile = files.center(tile.or(authored))?;
        Ok((
            Destination {
                map: Some(map.id),
                tile,
            },
            label,
        ))
    }

    pub(super) fn ui_scale(&self) -> u16 {
        self.scale
    }

    pub(super) fn console_frame(
        &mut self,
        size: (u32, u32),
        scale: u16,
    ) -> Option<(Arc<LoadedFrame>, (i32, i32))> {
        self.console.open.then(|| {
            (
                self.console.frame(size, scale),
                self.console.anchor(size, scale),
            )
        })
    }

    pub(super) fn status_frame(
        &mut self,
        timing: Duration,
        cadence: Option<Duration>,
        frames: u64,
        world: &InteractiveWorld,
        scale: u16,
        max_width: u32,
    ) -> Option<Arc<LoadedFrame>> {
        if !self.fps && !self.collision && !self.sectors {
            return None;
        }
        let stats = world.stats();
        let mut text = String::new();
        if self.fps {
            let rate = cadence.map_or(0., |interval| 1. / interval.as_secs_f64().max(0.000_001));
            text.push_str(&format!(
                "Last present {rate:.1} Hz  |  Vulkan host {:.2} ms  |  frames {frames}  |  sectors {}/{}  pending {}  VFS {}",
                timing.as_secs_f64() * 1000.,
                stats.cache.active_sectors,
                stats.cache.cached_sectors,
                stats.pending,
                stats.cache.vfs_reads
            ));
        }
        if self.collision {
            text.push_str(
                "  |  collision: red terrain, orange object, amber edge, violet unavailable",
            );
        }
        if self.sectors {
            text.push_str("  |  cyan sector edges");
        }
        let key = format!("{scale}:{max_width}:{text}");
        if let Some((cached, frame)) = &self.status
            && *cached == key
        {
            return Some(Arc::clone(frame));
        }
        let frame = self.console.status(&text, scale, max_width);
        self.status = Some((key, Arc::clone(&frame)));
        Some(frame)
    }

    pub(super) fn world_marks<'a>(
        &'a self,
        world: &'a InteractiveWorld,
        camera: &'a Camera,
        size: (u32, u32),
    ) -> impl Iterator<Item = WorldDraw<'a>> {
        (self.collision || self.sectors)
            .then(|| world.debug_tiles(camera, size))
            .into_iter()
            .flatten()
            .flat_map(move |(tile, (x, y))| {
                let mut marks = [None; 7];
                if self.collision {
                    match world.debug_collision(tile) {
                        None => marks[0] = Some(&self.marks.unknown),
                        Some(cell) => {
                            if !cell.standable {
                                marks[0] = Some(if cell.terrain_blocked {
                                    &self.marks.blocked
                                } else {
                                    &self.marks.object
                                });
                            }
                            for (index, blocked) in cell.edges.into_iter().enumerate() {
                                if blocked {
                                    marks[index + 1] = Some(&self.marks.edges[index]);
                                }
                            }
                        }
                    }
                }
                if self.sectors {
                    if tile.0 % 64 == 0 {
                        marks[5] = Some(&self.marks.sectors[0]);
                    }
                    if tile.1 % 64 == 0 {
                        marks[6] = Some(&self.marks.sectors[1]);
                    }
                }
                marks
                    .into_iter()
                    .flatten()
                    .map(move |asset| unlit(asset, (x - 40, y - 20)))
            })
    }
}

struct Marks {
    blocked: Arc<LoadedFrame>,
    object: Arc<LoadedFrame>,
    unknown: Arc<LoadedFrame>,
    edges: [Arc<LoadedFrame>; 4],
    sectors: [Arc<LoadedFrame>; 2],
}

impl Marks {
    fn new() -> Self {
        let marker = |name: &str, color, edge: Option<usize>| {
            let mut frame = SpriteFrame {
                width: 81,
                height: 41,
                hotspot: (0, 0),
                offset: (0, 0),
                pixels: vec![[0; 4]; 81 * 41],
            };
            let corners: [(i32, i32); 4] = [(40, 0), (80, 20), (40, 40), (0, 20)];
            if let Some(edge) = edge {
                let (x, y) = corners[edge];
                let (nx, ny) = corners[(edge + 1) % 4];
                for step in 0..=40i32 {
                    let (x, y) = (x + (nx - x) * step / 40, y + (ny - y) * step / 40);
                    frame.pixels[(y * 81 + x) as usize] = color;
                }
            } else {
                for y in 0..=40i32 {
                    for x in 0..=80i32 {
                        if (x - 40).abs() + 2 * (y - 20).abs() <= 40 {
                            frame.pixels[(y * 81 + x) as usize] = color;
                        }
                    }
                }
            }
            hud::generated_frame(name, frame)
        };
        Self {
            blocked: marker("developer/blocked", [231, 65, 77, 100], None),
            object: marker("developer/object", [224, 108, 38, 125], None),
            unknown: marker("developer/unknown", [162, 91, 219, 100], None),
            edges: std::array::from_fn(|index| {
                marker("developer/edge", [245, 178, 68, 230], Some(index))
            }),
            sectors: [0, 3].map(|edge| marker("developer/sector", [60, 229, 231, 255], Some(edge))),
        }
    }
}

impl Viewer<'_> {
    pub(super) fn developer_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        event: &WindowEvent,
    ) -> bool {
        let Some(dev) = &mut self.developer else {
            return false;
        };
        let Some(view) = &self.view else { return false };
        let ViewContent::Stream {
            world,
            camera,
            last_presented,
            last_viewport,
            last_zoom,
            travel: Some(travel),
            hud,
            shift,
            ..
        } = &mut self.content
        else {
            return false;
        };
        let size = view.window.inner_size();
        let scale = hud.as_ref().map_or(dev.scale, |hud| hud.scale());
        match event {
            WindowEvent::ModifiersChanged(value) => {
                self.modifiers = value.state();
                *shift = self.modifiers.shift_key();
            }
            WindowEvent::Focused(false) => self.modifiers = Default::default(),
            _ => {}
        }
        if let WindowEvent::KeyboardInput {
            event: key,
            is_synthetic: false,
            ..
        } = event
        {
            let toggle = dev.console.is_toggle(key.physical_key, &key.logical_key);
            if toggle && self.pause.is_none() && self.map_since.is_none() {
                if key.state.is_pressed() && !key.repeat {
                    dev.console.toggle();
                    self.navigation = Navigation::default();
                    self.player_deadline = world.player_moving().then_some(Instant::now());
                    world.cancel_player_route();
                    if let Some(hud) = hud {
                        hud.cancel_press();
                    }
                    view.window.request_redraw();
                    self.update_control_flow(event_loop);
                }
                return true;
            }
        }
        if !dev.console.open {
            return false;
        }
        let command = match event {
            WindowEvent::KeyboardInput {
                event: key,
                is_synthetic: false,
                ..
            } if key.state.is_pressed() => {
                if key.logical_key == Key::Named(NamedKey::Escape) && !key.repeat {
                    dev.console.toggle();
                } else {
                    let command = dev.console.key(
                        &key.logical_key,
                        key.text.as_deref(),
                        self.modifiers,
                        key.repeat,
                        || travel.wake.clone(),
                    );
                    view.window.request_redraw();
                    self.dispatch_console(command);
                    return true;
                }
                None
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = (position.x >= 0. && position.y >= 0.)
                    .then_some((position.x as u32, position.y as u32));
                return true;
            }
            WindowEvent::CursorLeft { .. } => {
                self.cursor = None;
                return true;
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                if let Some(point) = self.cursor
                    && !dev
                        .console
                        .panel_contains(point, (size.width, size.height), scale)
                {
                    let zoom = u32::from(*last_zoom);
                    let viewport = (
                        last_viewport.0.div_ceil(zoom),
                        last_viewport.1.div_ceil(zoom),
                    );
                    let elapsed = Instant::now()
                        .saturating_duration_since(self.epoch)
                        .saturating_sub(self.paused_for);
                    dev.selected = world.debug_pick(
                        (point.0 / zoom, point.1 / zoom),
                        viewport,
                        last_presented.as_ref().unwrap_or(camera),
                        elapsed,
                    );
                    dev.console.notice(if dev.selected.is_some() {
                        "Object selected. Type inspect or unlock."
                    } else {
                        "No visible object at the pointer."
                    });
                }
                None
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let delta = match delta {
                    MouseScrollDelta::LineDelta(_, y) => f64::from(*y),
                    MouseScrollDelta::PixelDelta(p) => p.y,
                };
                dev.console.scroll(if delta > 0. { 3 } else { -3 });
                None
            }
            WindowEvent::KeyboardInput { .. }
            | WindowEvent::MouseInput { .. }
            | WindowEvent::ModifiersChanged(_)
            | WindowEvent::Ime(_) => return true,
            _ => return false,
        };
        view.window.request_redraw();
        self.dispatch_console(command);
        true
    }

    fn dispatch_console(&mut self, command: Option<String>) {
        if let Some(command) = command {
            let result = developer::dispatch(&command, self);
            if matches!(&self.content, ViewContent::Stream { trace: true, .. }) {
                eprintln!("Developer command {command:?}: {result:?}");
            }
            if let Some(dev) = &mut self.developer {
                dev.console.output(result);
            }
        }
    }

    pub(super) fn developer_paste(&mut self) -> bool {
        self.developer
            .as_mut()
            .is_some_and(|dev| dev.console.receive_paste())
    }

    pub(super) fn reset_developer_location(&mut self) {
        if let Some(dev) = &mut self.developer {
            dev.selected = None;
        }
    }
}

fn load_hud(path: Option<&Path>, vfs: &mut Vfs, scale: u16) -> Result<Option<Box<Hud>>, String> {
    path.map(|path| Hud::load(path, vfs, Some(scale)).map(Box::new))
        .transpose()
        .map_err(|e| e.to_string())
}

impl developer::Runtime for Viewer<'_> {
    fn execute(&mut self, request: Request) -> Result<Output, String> {
        let now = Instant::now();
        let elapsed = self.play_elapsed(now);
        let game = self.game_elapsed(now);
        let dev = self
            .developer
            .as_mut()
            .ok_or("Developer mode is unavailable")?;
        let ViewContent::Stream {
            world,
            travel: Some(travel),
            hud,
            map_ui,
            ..
        } = &mut self.content
        else {
            return Err("No active playable world".into());
        };
        let scale = hud.as_ref().map_or(dev.scale, |theme| theme.scale());
        let line = match request {
            Request::Maps(filter) => {
                let query = filter.unwrap_or_default().to_lowercase();
                let mut lines: Vec<_> = dev
                    .atlas
                    .catalog
                    .maps
                    .iter()
                    .filter(|map| {
                        map.name.to_lowercase().contains(&query)
                            || map.id.to_string().contains(&query)
                    })
                    .map(|map| format!("{}  {}", map.id, map.name))
                    .collect();
                lines.extend(
                    dev.atlas
                        .areas
                        .values()
                        .filter(|area| area.name.to_lowercase().contains(&query))
                        .map(|area| format!("Area {}  {}  at {:?}", area.id, area.name, area.tile)),
                );
                if lines.is_empty() {
                    return Err(format!("No original maps or areas match {query:?}"));
                }
                return Ok(Output::Lines(lines));
            }
            Request::Load(name, tile) => {
                let (target, label) = dev.resolve(&name, tile)?;
                let facing = world.player_facing().unwrap_or(0);
                let bypass = world.collision_bypassed();
                self.load_location(target, facing, bypass)?;
                format!("Loaded {label} at {:?}", target.tile)
            }
            Request::Teleport(tile) => {
                let facing = world.player_facing().unwrap_or(0);
                let bypass = world.collision_bypassed();
                self.load_location(Destination { map: None, tile }, facing, bypass)?;
                format!("Player moved to {tile:?}")
            }
            Request::Position => {
                let map = dev.atlas.catalog.find(&travel.map)?;
                let (tile, town) = world
                    .player_map_position()
                    .ok_or("Player sector is not loaded")?;
                let sector = arcanum_rs::sector::Coordinates {
                    x: tile.0 / 64,
                    y: tile.1 / 64,
                };
                let mapped = if map.id == dev.atlas.start_map {
                    Some(tile)
                } else {
                    map.area
                        .and_then(|id| dev.atlas.areas.get(&id))
                        .map(|area| area.tile)
                };
                let world_location = mapped
                    .map(|tile| {
                        let pixels = map_ui
                            .as_ref()
                            .and_then(|ui| world_pixel(tile, ui.world_width()));
                        format!("World tile {tile:?}, atlas pixel {pixels:?}")
                    })
                    .unwrap_or_else(|| "World atlas position unavailable".into());
                return Ok(Output::Lines(vec![
                    format!(
                        "{} ({})  player {tile:?}  facing {:?}",
                        map.name,
                        map.id,
                        world.player_facing()
                    ),
                    format!(
                        "Sector ({}, {}), id {}, town map {town}",
                        sector.x,
                        sector.y,
                        sector
                            .id()
                            .map_or_else(|| "unavailable".into(), |id| id.to_string())
                    ),
                    world_location,
                    format!(
                        "Terrain {}",
                        world.debug_source(tile).unwrap_or("unavailable")
                    ),
                    format!(
                        "Collision {:?}, bypass {}",
                        world.debug_collision(tile),
                        world.collision_bypassed()
                    ),
                ]));
            }
            Request::ToggleCollision => {
                let disabled = world.developer_action(DeveloperAction::ToggleCollision)?;
                if let Some(view) = &mut self.view {
                    view.developer_status = Some(menu::developer_status(if disabled {
                        "COLLISION OFF"
                    } else {
                        "COLLISION ON"
                    }));
                }
                format!(
                    "Collision {}",
                    if disabled {
                        "disabled for keyboard and mouse"
                    } else {
                        "enabled"
                    }
                )
            }
            Request::Inspect(tile) => {
                let map = dev.atlas.catalog.find(&travel.map)?;
                let mut result = vec![format!("Map {} ({})", map.name, map.id)];
                result.extend(if let Some(tile) = tile {
                    dev.selected = world.debug_door_at(tile);
                    world.debug_inspect_tile(tile)?
                } else {
                    world.debug_inspect(
                        dev.selected
                            .ok_or("Click an object beneath the console or use inspect X Y")?,
                        elapsed,
                    )?
                });
                return Ok(Output::Lines(result));
            }
            Request::Unlock => {
                let selected = dev.selected.ok_or("Select a door first")?;
                if world.debug_unlock(selected)? {
                    "Door unlocked for this session. Use it normally to open it.".into()
                } else {
                    "Door already has the developer unlock override".into()
                }
            }
            Request::SetTime(hour) => {
                let clock = self.clock.as_mut().ok_or("Game clock unavailable")?;
                clock.set_hour(hour, game);
                world.set_game_hour(hour);
                if let Some(hud) = hud {
                    hud.set_game_time(clock.hours(game));
                }
                format!("Game hour set to {hour:02}:00")
            }
            Request::DiscoverAll => {
                let ui = map_ui.as_mut().ok_or("World map unavailable")?;
                format!(
                    "Discovered {} original destinations. Travel eligibility is unchanged.",
                    ui.discover_all()
                )
            }
            Request::ReloadUi => {
                let next_hud = load_hud(dev.hud_theme.as_deref(), &mut dev.vfs, scale)?;
                let next_map = map_ui
                    .as_mut()
                    .map(|map| map.prepare_theme(&dev.map_theme, scale))
                    .transpose()?;
                let next_console =
                    Console::load(&dev.console_theme, &mut dev.vfs).map_err(|e| e.to_string())?;
                *hud = next_hud;
                if let (Some(ui), Some(theme)) = (map_ui, next_map) {
                    ui.replace_theme(theme);
                }
                dev.console.replace_theme(next_console);
                dev.scale = scale;
                dev.status = None;
                "Reloaded available UI themes".into()
            }
            Request::Scale(scale) => {
                dev.scale = scale;
                if let Some(hud) = hud {
                    hud.set_scale(scale);
                }
                if let Some(map) = map_ui {
                    map.set_scale(scale);
                }
                format!("UI scale {scale}%")
            }
            Request::ShowCollision => {
                dev.collision = !dev.collision;
                format!("Collision overlay {}", dev.collision)
            }
            Request::ShowSectors => {
                dev.sectors = !dev.sectors;
                format!("Sector overlay {}", dev.sectors)
            }
            Request::Fps => {
                dev.fps = !dev.fps;
                format!("Vulkan frame statistics {}", dev.fps)
            }
        };
        Ok(line.into())
    }
}
