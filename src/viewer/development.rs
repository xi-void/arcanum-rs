//! Connect typed developer requests to the existing playable runtime.
use super::{console::Console, *};
use arcanum_rs::{
    content::{
        map_atlas::{Atlas, world_pixel},
        world::{DebugObject, DeveloperAction},
    },
    developer::{self, EventHistory, Failure, ObjectFilter, Output, Request, Snapshot},
    developer_ipc::{Job, Reply, Server},
    vfs::Vfs,
};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

pub(crate) struct Developer {
    console: Console,
    vfs: Vfs,
    atlas: Atlas,
    console_theme: ThemeSource,
    hud_theme: Option<ThemeSource>,
    map_theme: ThemeSource,
    scale: u16,
    selected: Option<DebugObject>,
    collision: bool,
    sectors: bool,
    fps: bool,
    marks: Marks,
    status: Option<(String, Arc<LoadedFrame>)>,
    socket: Option<PathBuf>,
    automatic_rpc: bool,
    server: Option<Server>,
    captures: Option<(PathBuf, Arc<cap_std::fs::Dir>)>,
    events: EventHistory,
    correlation: Option<u64>,
    quit: bool,
    reloads: u64,
    reload_status: Option<Value>,
}

impl Developer {
    pub(crate) fn load(
        mut vfs: Vfs,
        console_theme: ThemeSource,
        hud_theme: Option<ThemeSource>,
        map_theme: ThemeSource,
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
            socket: None,
            automatic_rpc: false,
            server: None,
            captures: None,
            events: EventHistory::default(),
            correlation: None,
            quit: false,
            reloads: 0,
            reload_status: None,
        })
    }

    pub(crate) fn with_automation(
        mut self,
        socket: Option<PathBuf>,
        automatic_rpc: bool,
        directory: Option<PathBuf>,
    ) -> std::io::Result<Self> {
        self.socket = socket;
        self.automatic_rpc = automatic_rpc;
        if let Some(path) = directory {
            let path = path.canonicalize()?;
            let directory =
                cap_std::fs::Dir::open_ambient_dir(&path, cap_std::ambient_authority())?;
            self.captures = Some((path, Arc::new(directory)));
        }
        Ok(self)
    }

    pub(super) fn start_automation(&mut self, proxy: EventLoopProxy<()>) -> Result<(), String> {
        let wake = move || {
            let _ = proxy.send_event(());
        };
        if let Some(socket) = self.socket.take() {
            self.server = Some(
                Server::bind(&socket, wake)
                    .map_err(|err| format!("Developer socket {}: {err}", socket.display()))?,
            );
        } else if self.automatic_rpc {
            let server = Server::bind_automatic(wake)
                .map_err(|err| format!("Cannot initialize automatic developer socket: {err}"))?;
            eprintln!("Developer RPC socket: {}", server.path().display());
            self.server = Some(server);
        }
        Ok(())
    }

    pub(super) fn capture_enabled(&self) -> bool {
        self.captures.is_some()
    }

    pub(super) fn event(&mut self, category: &str, id: Option<u64>, data: Value) {
        self.events.record(category, id.or(self.correlation), data);
    }

    fn mods(&self) -> Vec<String> {
        self.vfs
            .mounts()
            .filter_map(|mount| mount.mod_id.as_ref().map(ToString::to_string))
            .collect()
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

    pub(super) fn set_configured_ui_scale(&mut self, scale: u16) {
        self.scale = scale;
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
                        hud.cancel_inventory_drag();
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

    pub(super) fn pump_automation(&mut self, event_loop: &ActiveEventLoop) {
        let job = self
            .developer
            .as_ref()
            .and_then(|dev| dev.server.as_ref()?.jobs.try_recv().ok());
        if let Some(Job { call, reply }) = job {
            if let Some(dev) = &mut self.developer {
                dev.correlation = Some(call.id);
            }
            let (name, result) = developer::dispatch_args(&call.command, &call.args, self);
            let clear = matches!(&result, Ok(Output::Clear));
            let response = Reply::result(call.id, name, result);
            if let Some(dev) = &mut self.developer {
                if clear {
                    dev.console.output(Ok(Output::Clear));
                }
                dev.event(
                    "command",
                    Some(call.id),
                    json!({"command":name,"status":response.status,"error":response.error}),
                );
                dev.correlation = None;
            }
            let _ = reply.try_send(response);
            if clear && let Some(view) = &self.view {
                view.window.request_redraw();
            }
        }
        if self.developer.as_ref().is_some_and(|dev| dev.quit) {
            event_loop.exit();
        }
        self.update_control_flow(event_loop);
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

fn load_hud(
    path: Option<&ThemeSource>,
    vfs: &mut Vfs,
    scale: u16,
) -> Result<Option<Box<Hud>>, String> {
    path.map(|path| Hud::load(path, vfs, Some(scale)).map(Box::new))
        .transpose()
        .map_err(|e| e.to_string())
}

impl developer::Runtime for Viewer<'_> {
    fn execute(&mut self, request: Request) -> Result<Output, Failure> {
        let mutating = !matches!(
            &request,
            Request::Maps(_)
                | Request::Position
                | Request::Sectors
                | Request::Objects(_)
                | Request::Inventory(_)
                | Request::ItemOwner(_)
                | Request::Resource(..)
                | Request::Snapshot
                | Request::Events(_)
                | Request::Operation(_)
        );
        let result = self.execute_developer(request);
        if result.is_ok()
            && mutating
            && let Some(view) = &self.view
        {
            view.window.request_redraw();
        }
        result
    }
}

impl Viewer<'_> {
    fn execute_developer(&mut self, request: Request) -> Result<Output, Failure> {
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
            camera,
            last_presented,
            waiting,
            ..
        } = &mut self.content
        else {
            return Err("No active playable world".into());
        };
        let scale = hud.as_ref().map_or(dev.scale, |theme| theme.scale());
        let hud_theme = dev.hud_theme.as_ref().map(ThemeSource::path);
        let map_theme = dev.map_theme.path();
        let console_theme = dev.console_theme.path();
        let viewport = self
            .view
            .as_ref()
            .map(|view| {
                let size = view.window.inner_size();
                (size.width, size.height)
            })
            .unwrap_or_default();
        let logical = (
            viewport.0.div_ceil(u32::from(self.settings.zoom)),
            viewport.1.div_ceil(u32::from(self.settings.zoom)),
        );
        let map = dev.atlas.catalog.find(&travel.map)?;
        let current_hour = self.clock.as_ref().map(|clock| clock.hour(game));
        let map_state =
            json!({"id":map.id,"name":map.name,"directory":map.directory,"area_id":map.area});
        let result = match request {
            Request::Maps(filter) => {
                let query = filter.unwrap_or_default().to_lowercase();
                let maps:Vec<_> = dev.atlas.catalog.maps.iter()
                    .filter(|map|map.name.to_lowercase().contains(&query)||map.id.to_string().contains(&query))
                    .map(|map|json!({"id":map.id,"name":map.name,"directory":map.directory,"entry":map.map_list_center}))
                    .collect();
                let areas: Vec<_> = dev
                    .atlas
                    .areas
                    .values()
                    .filter(|area| area.name.to_lowercase().contains(&query))
                    .map(|area| json!({"id":area.id,"name":area.name,"tile":area.tile}))
                    .collect();
                json!({"maps":maps,"areas":areas})
            }
            Request::Load(name, tile) => {
                let (target, label) = dev.resolve(&name, tile)?;
                let facing = world.player_facing().unwrap_or(0);
                let bypass = world.collision_bypassed();
                if self.map_since.is_some() || self.pause.is_some() {
                    return Err(Failure::new(
                        "busy",
                        "Close the map or pause menu before changing locations",
                    ));
                }
                self.load_location(target, facing, bypass)?;
                json!({"message":format!("Loaded {label} at {:?}",target.tile),"map":target.map,"tile":target.tile,"view_ready":false})
            }
            Request::Teleport(tile) => {
                let facing = world.player_facing().unwrap_or(0);
                let bypass = world.collision_bypassed();
                if self.map_since.is_some() || self.pause.is_some() {
                    return Err(Failure::new(
                        "busy",
                        "Close the map or pause menu before changing locations",
                    ));
                }
                let id = map.id;
                self.load_location(Destination { map: None, tile }, facing, bypass)?;
                json!({"message":format!("Player moved to {tile:?}"),"map":id,"tile":tile,"view_ready":false})
            }
            Request::Position => {
                let position = world.player_map_position();
                let tile = position.map(|(tile, _)| tile);
                let coordinates = tile.map(|(x, y)| arcanum_rs::sector::Coordinates {
                    x: x / 64,
                    y: y / 64,
                });
                let mapped = if map.id == dev.atlas.start_map {
                    tile
                } else {
                    map.area
                        .and_then(|id| dev.atlas.areas.get(&id))
                        .map(|area| area.tile)
                };
                let source = tile.and_then(|tile| world.debug_source(tile));
                let navigation = world.debug_navigation();
                let area = map
                    .area
                    .and_then(|id| dev.atlas.areas.get(&id))
                    .or_else(|| mapped.and_then(|tile| dev.atlas.nearby_area(tile)))
                    .map(|area| json!({"id":area.id,"name":area.name,"tile":area.tile}));
                json!({
                    "map":map_state,
                    "player":{"tile":tile,"facing":world.player_facing(),"collision_bypass":world.collision_bypassed(),
                        "collision":tile.and_then(|tile|world.debug_collision(tile)),"navigation":navigation,
                        "appearance":world.player_appearance(),"statistics":world.character_statistics()},
                    "sector":{"coordinates":coordinates,"id":coordinates.and_then(|sector|sector.id()),
                        "town_map":position.map(|(_,town)|town),"terrain_source_summary":source},
                    "world":{"hour":current_hour,"world_tile":mapped,"area":area,
                        "atlas_pixel":mapped.and_then(|tile|world_pixel(tile,map_ui.as_ref()?.world_width())),
                        "travel":map_ui.as_ref().map(|map|map.diagnostic())},
                    "camera":{"projected":camera.position(),"viewport":viewport,"zoom":self.settings.zoom,"follow":self.settings.follow,
                        "view_ready":last_presented.is_some() && !*waiting},
                    "streaming":world.stats(),
                    "ui":{"hud_theme":hud_theme,"map_theme":map_theme,"console_theme":console_theme,
                        "hud":hud.as_ref().map(|hud|hud.diagnostics()),"scale_percent":scale,"console_open":dev.console.open,
                        "paused":self.pause.is_some(),
                        "pause_menu":self.pause.as_ref().zip(self.pause_theme.as_ref())
                            .map(|((menu, _), theme)| menu.diagnostic(theme, [viewport.0,viewport.1], scale)),
                        "save_directory":self.saves.as_ref().ok().map(|store| store.directory()),"reload_generation":dev.reloads,"reload_status":dev.reload_status,
                        "selected":dev.selected.map(InteractiveWorld::debug_selection_id)},
                    "audio":self.audio.as_ref().map(|audio| audio.diagnostic(self.settings.audio)),
                    "renderer":{"stats":self.view.as_ref().and_then(|view|view.renderer.stats.as_ref()),
                        "frames_presented":self.view.as_ref().map(|view|view.submitted_frames),
                        "host_frame_time":self.view.as_ref().map(|view|view.last_frame_time)},
                })
            }
            Request::Sectors => world.debug_sectors(),
            Request::Objects(filter) => {
                let mut data = world.debug_objects(&filter, camera, logical, elapsed);
                data["map"] = map_state;
                data
            }
            Request::Inventory(owner) => world
                .inspect_inventory(&mut dev.vfs, &owner)
                .map_err(|err| Failure::new("inventory_unavailable", err))?,
            Request::ItemOwner(id) => world
                .inspect_item_owner(&mut dev.vfs, &id)
                .map_err(|err| Failure::new("inventory_unavailable", err))?,
            Request::Pickup(id, slot) => world
                .pickup_item(&mut dev.vfs, &id, slot)
                .map_err(|err| Failure::new("inventory_rejected", err))?,
            Request::DropItem(id) => world
                .drop_item(&mut dev.vfs, &id)
                .map_err(|err| Failure::new("inventory_rejected", err))?,
            Request::Transfer(id, from, to, slot) => world
                .transfer_item(&mut dev.vfs, &id, &from, &to, slot, current_hour)
                .map_err(|err| Failure::new("inventory_rejected", err))?,
            Request::TransferQuantity(id, from, to, slot, quantity) => world
                .transfer_quantity(&mut dev.vfs, &id, &from, &to, slot, quantity)
                .map_err(|err| Failure::new("inventory_rejected", err))?,
            Request::Equipment(slot, item, cell) => world
                .change_equipment(&mut dev.vfs, slot, item.as_deref(), cell)
                .map_err(|err| Failure::new("inventory_rejected", err))?,
            Request::Resource(path, frame) => {
                let data = arcanum_rs::content::diagnostics::resource(&mut dev.vfs, &path, frame)
                    .map_err(|err| Failure::new("invalid_resource_path", err.to_string()))?;
                if data["winner"].is_null() {
                    let mut failure = Failure::new(
                        "missing_resource",
                        format!("No winning source for {}", data["path"]),
                    );
                    failure.details = Some(data);
                    return Err(failure);
                }
                if let Some(code) = data["decode"]["code"].as_str() {
                    let mut failure = Failure::new(
                        if code == "frame_requires_art" {
                            "unsupported_operation"
                        } else {
                            "resource_decode_failed"
                        },
                        data["decode"]["message"]
                            .as_str()
                            .unwrap_or("Frame selection requires ART"),
                    );
                    failure.details = Some(data);
                    return Err(failure);
                }
                data
            }
            Request::Snapshot => {
                let tile = world
                    .player_map_position()
                    .ok_or("Player sector is not loaded")?
                    .0;
                let loaded_sectors = world.debug_sectors()["cached"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter_map(|r| r["id"].as_u64())
                    .collect();
                return Ok(Output::Data(
                    serde_json::to_value(Snapshot {
                        version: 1,
                        map: map.id,
                        tile,
                        facing: world.player_facing().ok_or("Player unavailable")?,
                        hour: current_hour.ok_or("Clock unavailable")?,
                        collision_bypass: world.collision_bypassed(),
                        zoom: self.settings.zoom,
                        scale_percent: scale,
                        lighting: self.settings.lighting,
                        shadows: self.settings.shadows,
                        run: self.settings.run,
                        hud_theme: hud_theme.map(Path::to_path_buf),
                        map_theme: map_theme.to_path_buf(),
                        console_theme: console_theme.to_path_buf(),
                        mods: dev.mods(),
                        selected: dev.selected.map(InteractiveWorld::debug_selection_id),
                        overlays: [dev.collision, dev.sectors, dev.fps],
                        camera: camera.position(),
                        viewport,
                        loaded_sectors,
                    })
                    .expect("snapshot contains serializable fields"),
                ));
            }
            Request::Restore(text) => {
                if self.map_since.is_some() || self.pause.is_some() {
                    return Err(Failure::new(
                        "busy",
                        "Close the map or pause menu before restoring",
                    ));
                }
                let snapshot = Snapshot::parse(&text)?;
                if snapshot.mods != dev.mods()
                    || snapshot.hud_theme.as_deref() != hud_theme
                    || snapshot.map_theme != map_theme
                    || snapshot.console_theme != console_theme
                {
                    return Err(Failure::new(
                        "snapshot_environment_mismatch",
                        "Use the snapshot's UI packages and content mounts",
                    ));
                }
                let target = Destination {
                    map: Some(snapshot.map),
                    tile: snapshot.tile,
                };
                let previous = self.settings.zoom;
                self.settings.zoom = snapshot.zoom;
                if let Err(err) =
                    self.load_location(target, snapshot.facing, snapshot.collision_bypass)
                {
                    self.settings.zoom = previous;
                    return Err(err.into());
                }
                self.settings.lighting = snapshot.lighting;
                self.settings.shadows = snapshot.shadows;
                self.settings.run = snapshot.run;
                self.execute_developer(Request::Scale(snapshot.scale_percent))?;
                self.execute_developer(Request::SetTime(snapshot.hour))?;
                if let Some(dev) = &mut self.developer {
                    [dev.collision, dev.sectors, dev.fps] = snapshot.overlays;
                    if let ViewContent::Stream { world, .. } = &self.content {
                        dev.selected = snapshot.selected.and_then(|id| world.debug_select(id));
                    }
                    dev.event("snapshot_restored", None, json!({"target":target}));
                }
                json!({"map":snapshot.map,"tile":snapshot.tile,"view_ready":false})
            }
            Request::Capture(file) => {
                if !valid_capture_name(&file) {
                    return Err(Failure::new(
                        "invalid_arguments",
                        "Capture requires a simple .png filename",
                    ));
                }
                let (path, directory) = dev.captures.as_ref().ok_or_else(|| {
                    Failure::new(
                        "unsupported_operation",
                        "Set --diagnostics DIRECTORY at launch",
                    )
                })?;
                let view = self.view.as_mut().ok_or("Window unavailable")?;
                if view.occluded || viewport.0 == 0 || viewport.1 == 0 {
                    return Err(Failure::new("busy", "Capture requires a drawable window"));
                }
                return view.renderer.request_capture(
                    Arc::clone(directory),
                    path.join(&file),
                    file,
                );
            }
            Request::Operation(token) => {
                return self
                    .view
                    .as_ref()
                    .ok_or("Window unavailable")?
                    .renderer
                    .operation(token);
            }
            Request::Events(category) => dev.events.query(category.as_deref()),
            Request::Trace(enabled) => {
                dev.events.enabled = enabled;
                json!({"enabled":enabled})
            }
            Request::Quit => {
                dev.quit = true;
                json!({"message":"Engine exit requested"})
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
                json!({"collision_bypass":disabled,"message":if disabled {"Collision disabled for mouse routes and developer arrivals"} else {"Collision enabled"}})
            }
            Request::Inspect(tile) => {
                let filter = if let Some(tile) = tile {
                    dev.selected = world.debug_door_at(tile);
                    ObjectFilter {
                        tile: Some(tile),
                        ..Default::default()
                    }
                } else {
                    let object = dev
                        .selected
                        .ok_or("Click an object beneath the console or use inspect X Y")?;
                    ObjectFilter {
                        record: Some(InteractiveWorld::debug_selection_id(object)),
                        ..Default::default()
                    }
                };
                let mut data = world.debug_objects(&filter, camera, logical, elapsed);
                data["map"] = map_state;
                data
            }
            Request::Unlock => {
                let selected = dev.selected.ok_or("Select a door or container first")?;
                let changed = world
                    .debug_unlock(&mut dev.vfs, selected)
                    .map_err(|err| Failure::new("unsupported_operation", err))?;
                json!({"unlocked":true,"changed":changed,"id":InteractiveWorld::debug_selection_id(selected),
                    "message":"Click the object normally to open it."})
            }
            Request::SetTime(hour) => {
                let clock = self.clock.as_mut().ok_or("Game clock unavailable")?;
                clock.set_hour(hour, game);
                world.set_game_hour(hour);
                if let Some(hud) = hud {
                    hud.set_game_time(clock.hours(game));
                }
                dev.event("game_time", None, json!({"hour":hour}));
                json!({"hour":hour})
            }
            Request::DiscoverAll => {
                let ui = map_ui.as_mut().ok_or("World map unavailable")?;
                json!({"discovered":ui.discover_all(),"travel_eligible":ui.can_travel()})
            }
            Request::ReloadUi => {
                let loaded = (|| -> Result<_, String> {
                    let next_hud = load_hud(dev.hud_theme.as_ref(), &mut dev.vfs, scale)?;
                    let next_map = map_ui
                        .as_mut()
                        .map(|map| map.prepare_theme(&dev.map_theme, scale))
                        .transpose()?;
                    let next_console = Console::load(&dev.console_theme, &mut dev.vfs)
                        .map_err(|err| err.to_string())?;
                    let next_pause = self
                        .pause_theme
                        .as_ref()
                        .map(|theme| {
                            super::Classic::load(&theme.source, &mut dev.vfs).map(Box::new)
                        })
                        .transpose()
                        .map_err(|err| err.to_string())?;
                    Ok((next_hud, next_map, next_console, next_pause))
                })();
                let (next_hud, next_map, next_console, next_pause) = loaded.map_err(|err| {
                    dev.reload_status = Some(json!({"status":"failed","message":err}));
                    dev.event("ui_reload", None, json!({"status":"failed"}));
                    Failure::new("ui_reload_failed", err)
                })?;
                *hud = next_hud;
                self.pause_theme = next_pause;
                if let (Some(ui), Some(theme)) = (map_ui, next_map) {
                    ui.replace_theme(theme);
                }
                dev.console.replace_theme(next_console);
                dev.scale = scale;
                dev.status = None;
                dev.reloads += 1;
                let outcome = json!({"status":"completed","generation":dev.reloads});
                dev.reload_status = Some(outcome.clone());
                dev.event("ui_reload", None, outcome.clone());
                outcome
            }
            Request::Scale(scale) => {
                self.pause_scale = scale;
                dev.scale = scale;
                if let Some(hud) = hud {
                    hud.set_scale(scale);
                }
                if let Some(map) = map_ui {
                    map.set_scale(scale);
                }
                json!({"scale_percent":scale})
            }
            Request::ShowCollision => {
                dev.collision = !dev.collision;
                json!({"enabled":dev.collision})
            }
            Request::ShowSectors => {
                dev.sectors = !dev.sectors;
                json!({"enabled":dev.sectors})
            }
            Request::Fps => {
                dev.fps = !dev.fps;
                json!({"enabled":dev.fps})
            }
        };
        Ok(Output::Data(result))
    }
}

fn valid_capture_name(file: &str) -> bool {
    file.len() <= 96
        && file.ends_with(".png")
        && !file.starts_with('.')
        && file
            .bytes()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, b'.' | b'_' | b'-'))
}
