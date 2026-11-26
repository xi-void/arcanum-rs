//! Optional desktop presentation for the sprite diagnostic.

use arcanum_rs::art::SpriteFrame;
use arcanum_rs::gpu::OverlayDraw;

mod audio;
pub(crate) use audio::Audio;
mod classic_menu;
pub(crate) use classic_menu::Classic;
mod console;
mod development;
pub(super) use development::Developer;
mod gpu;
mod hud;
mod map_ui;
use map_ui::Arrival;
pub(super) use map_ui::MapUi;
mod menu;
mod theme_source;
use crate::app_config::ConfigFile;
use arcanum_rs::camera::{Camera, CameraScene};
use arcanum_rs::compositor::{Modulation, SpriteDraw};
use arcanum_rs::content::{
    art::LoadedFrame,
    world::{
        Destination, DoorStates, InteractiveWorld, InventoryReader, SoundEvent, WorldDraw,
        WorldStream,
    },
};
use arcanum_rs::content::{save, travel::Clock};
use classic_menu::{Command as MenuCommand, Menu};
pub(super) use hud::Hud;
pub(super) use menu::Settings;
use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{Duration, Instant},
};
pub(crate) use theme_source::ThemeSource;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalSize, Size};
use winit::event::{ElementState, MouseButton, MouseScrollDelta, StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Fullscreen, Window, WindowId};

const BACKGROUND: [u8; 3] = [32, 43, 58];
const MAX_SURFACE_PIXELS: usize = 16 * 1024 * 1024;

fn stream_background(playing: bool) -> [u8; 3] {
    if playing { [0; 3] } else { BACKGROUND }
}

pub struct Layer<'a> {
    pub frame: &'a SpriteFrame,
    /// Physical pixel offset from the center of the drawable area.
    pub relative_anchor: (i16, i16),
}

pub fn synthetic_frame() -> SpriteFrame {
    synthetic_ring([255, 168, 70], (60, 62), (-8, 5))
}

pub fn show_synthetic_overlap() -> Result<(), String> {
    let back = synthetic_frame();
    let front = synthetic_ring([64, 216, 245], (-5, 50), (7, -3));
    show_layers(
        "Arcanum-RS | Synthetic overlap".into(),
        &[
            Layer {
                frame: &back,
                relative_anchor: (-25, 10),
            },
            Layer {
                frame: &front,
                relative_anchor: (-44, -8),
            },
        ],
    )
}

pub fn show_floor(
    title: String,
    floor: &SpriteFrame,
    foreground: &SpriteFrame,
) -> Result<(), String> {
    let draws = arcanum_rs::isometric::fixed_scene(floor, foreground)
        .ok_or("floor scene requires a 78 × 40 tile with hotspot (39, 39)")?;
    let layers: Vec<_> = draws
        .map(|draw| Layer {
            frame: draw.frame,
            // The fixed 3 × 3 patch and foreground are within 80 px of center.
            relative_anchor: (draw.anchor.0 as i16, draw.anchor.1 as i16),
        })
        .collect();
    show_layers(title, &layers)
}

fn synthetic_ring([r, g, b]: [u8; 3], hotspot: (i32, i32), offset: (i32, i32)) -> SpriteFrame {
    let mut pixels = Vec::with_capacity(121 * 101);
    for y in -50i32..=50 {
        for x in -60i32..=60 {
            let distance = x.abs() + y.abs();
            pixels.push(match distance {
                0..=17 | 51.. => [0, 0, 0, 0],
                _ if x < 0 => [r, g, b, 128],
                _ => [r, g, b, 255],
            });
        }
    }
    SpriteFrame {
        width: 121,
        height: 101,
        hotspot,
        offset,
        pixels,
    }
}

pub fn show(title: String, frame: SpriteFrame) -> Result<(), String> {
    show_layers(
        title,
        &[Layer {
            frame: &frame,
            relative_anchor: (0, 0),
        }],
    )
}

pub fn show_layers(title: String, layers: &[Layer<'_>]) -> Result<(), String> {
    show_layers_with_size(title, layers, LogicalSize::new(640, 480).into())
}

pub fn show_layers_at_size(
    title: String,
    layers: &[Layer<'_>],
    size: (u32, u32),
) -> Result<(), String> {
    show_layers_with_size(title, layers, PhysicalSize::<u32>::from(size).into())
}

fn show_layers_with_size(title: String, layers: &[Layer<'_>], size: Size) -> Result<(), String> {
    let sprites = layers
        .iter()
        .map(|layer| {
            (
                hud::generated_frame("diagnostic-sprite", layer.frame.clone()),
                layer.relative_anchor,
            )
        })
        .collect();
    show_content(title, ViewContent::Fixed(sprites), size)
}

pub fn show_camera(
    title: String,
    scene: &CameraScene<'_>,
    camera: Camera,
    size: (u32, u32),
) -> Result<(), String> {
    let mut assets = BTreeMap::new();
    for frame in scene.frames() {
        assets
            .entry(std::ptr::from_ref(frame))
            .or_insert_with(|| hud::generated_frame("diagnostic-sprite", frame.clone()));
    }
    show_content(
        title,
        ViewContent::World {
            scene,
            assets,
            camera,
            shift: false,
        },
        PhysicalSize::<u32>::from(size).into(),
    )
}

pub type TravelLoader<'a> = Box<
    dyn Fn(
            &str,
            &InteractiveWorld,
            Destination,
            u8,
            (u32, u32),
            Option<u8>,
            LocationMode<'_>,
        ) -> Result<(String, WorldStream, Camera), String>
        + 'a,
>;

pub(crate) enum LocationMode<'a> {
    Travel {
        bypass: bool,
    },
    Save {
        state: &'a save::State,
        doors: DoorStates,
    },
}

struct Travel<'a> {
    map: String,
    loader: TravelLoader<'a>,
    wake: EventLoopProxy<()>,
    doors: std::collections::BTreeMap<String, DoorStates>,
}

#[derive(Default)]
pub(crate) struct Startup {
    pub fullscreen: bool,
    pub settings: Settings,
    pub preferences: Option<ConfigFile>,
    pub pause_theme: Option<Box<Classic>>,
    pub ui_scale: u16,
    pub audio: Option<Audio>,
}

pub struct StreamOptions<'a> {
    pub trace: bool,
    pub animation_ms: Option<u64>,
    pub explore: bool,
    pub debug: bool,
    pub developer: Option<Developer>,
    pub travel: Option<(String, TravelLoader<'a>)>,
    pub hud: Option<Box<Hud>>,
    pub inventory_reader: Option<Box<InventoryReader>>,
    pub map_ui: Option<Box<MapUi>>,
    pub startup: Startup,
}

pub fn show_stream(
    title: String,
    world: WorldStream,
    mut camera: Camera,
    size: (u32, u32),
    options: StreamOptions<'_>,
) -> Result<(), String> {
    let StreamOptions {
        trace,
        animation_ms,
        explore,
        debug,
        developer,
        travel,
        hud,
        inventory_reader,
        map_ui,
        startup,
    } = options;
    let event_loop = make_loop()?;
    let proxy = event_loop.create_proxy();
    let travel = travel.map(|(map, loader)| Travel {
        map,
        loader,
        wake: proxy.clone(),
        doors: std::collections::BTreeMap::new(),
    });
    let world = world
        .into_interactive(
            move || {
                let _ = proxy.send_event(());
            },
            trace,
        )
        .map_err(|error| format!("cannot start sector preparation worker: {error}"))?;
    world.follow_player(Duration::ZERO, &mut camera);
    run_content(
        title,
        ViewContent::Stream {
            world: Box::new(world),
            camera,
            shift: false,
            direction: (0, 0),
            last_presented: None,
            last_viewport: (0, 0),
            last_zoom: 1,
            waiting: false,
            explore,
            debug,
            trace,
            animation_ms,
            travel,
            hud,
            inventory_reader,
            map_ui,
        },
        PhysicalSize::<u32>::from(size).into(),
        event_loop,
        developer,
        startup,
    )
}

enum ViewContent<'a> {
    Fixed(Vec<(Arc<LoadedFrame>, (i16, i16))>),
    Stream {
        world: Box<InteractiveWorld>,
        camera: Camera,
        shift: bool,
        direction: (i32, i32),
        last_presented: Option<Camera>,
        last_viewport: (u32, u32),
        last_zoom: u8,
        waiting: bool,
        explore: bool,
        debug: bool,
        trace: bool,
        animation_ms: Option<u64>,
        travel: Option<Travel<'a>>,
        hud: Option<Box<Hud>>,
        inventory_reader: Option<Box<InventoryReader>>,
        map_ui: Option<Box<MapUi>>,
    },
    World {
        scene: &'a CameraScene<'a>,
        assets: BTreeMap<*const SpriteFrame, Arc<LoadedFrame>>,
        camera: Camera,
        shift: bool,
    },
}

fn show_content(title: String, content: ViewContent<'_>, size: Size) -> Result<(), String> {
    run_content(title, content, size, make_loop()?, None, Startup::default())
}

fn make_loop() -> Result<EventLoop<()>, String> {
    EventLoop::with_user_event()
        .build()
        .map_err(|error| format!("cannot open display: {error}"))
}

fn run_content(
    title: String,
    content: ViewContent<'_>,
    size: Size,
    event_loop: EventLoop<()>,
    mut developer: Option<Developer>,
    startup: Startup,
) -> Result<(), String> {
    if let Some(dev) = &mut developer {
        dev.start_automation(event_loop.create_proxy())?;
    }
    event_loop.set_control_flow(ControlFlow::Wait);
    let clock = match &content {
        ViewContent::Stream { world, .. } => world.game_clock(),
        _ => None,
    };
    let mut app = Viewer {
        title,
        size,
        view: None,
        content,
        failure: None,
        epoch: Instant::now(),
        animation_deadline: None,
        navigation: Navigation::default(),
        player_deadline: None,
        cursor: None,
        settings: startup.settings,
        audio: startup.audio,
        sounding_container: None,
        reset_audio: false,
        fullscreen: startup.fullscreen,
        preferences: startup.preferences,
        pause: None,
        pause_theme: startup.pause_theme,
        pause_scale: startup.ui_scale.max(50),
        saves: save::Store::user(),
        paused_for: Duration::ZERO,
        clock_paused_for: Duration::ZERO,
        map_since: None,
        clock,
        developer,
        modifiers: Default::default(),
    };
    event_loop
        .run_app(&mut app)
        .map_err(|error| format!("window event loop failed: {error}"))?;
    if let ViewContent::Stream { world, .. } = &app.content {
        eprintln!("World cache {:?}", world.stats());
    }
    if let Some(error) = app.failure {
        return Err(error);
    }
    if let Some(preferences) = app.preferences {
        preferences.save().map_err(|error| {
            format!(
                "Cannot save preferences to {}: {error}",
                preferences.path().display()
            )
        })?;
    }
    Ok(())
}

struct Viewer<'a> {
    title: String,
    size: Size,
    view: Option<WindowSurface>,
    content: ViewContent<'a>,
    failure: Option<String>,
    epoch: Instant,
    animation_deadline: Option<Instant>,
    navigation: Navigation,
    player_deadline: Option<Instant>,
    cursor: Option<(u32, u32)>,
    settings: Settings,
    audio: Option<Audio>,
    sounding_container: Option<(String, i32)>,
    reset_audio: bool,
    fullscreen: bool,
    preferences: Option<ConfigFile>,
    pause: Option<(Menu, Instant)>,
    pause_theme: Option<Box<Classic>>,
    pause_scale: u16,
    saves: Result<save::Store, String>,
    paused_for: Duration,
    clock_paused_for: Duration,
    map_since: Option<Instant>,
    clock: Option<Clock>,
    developer: Option<Developer>,
    modifiers: winit::keyboard::ModifiersState,
}

const CAMERA_TICK: Duration = Duration::from_millis(16);

#[derive(Default)]
struct Navigation {
    held: u8,
    sampled_at: Option<Instant>,
    remainder: (f64, f64),
}

impl Navigation {
    fn key(key: &Key) -> Option<u8> {
        let index = match key {
            Key::Named(NamedKey::ArrowLeft) => 0,
            Key::Named(NamedKey::ArrowRight) => 1,
            Key::Named(NamedKey::ArrowUp) => 2,
            Key::Named(NamedKey::ArrowDown) => 3,
            Key::Character(c) if c.eq_ignore_ascii_case("a") => 4,
            Key::Character(c) if c.eq_ignore_ascii_case("d") => 5,
            Key::Character(c) if c.eq_ignore_ascii_case("w") => 6,
            Key::Character(c) if c.eq_ignore_ascii_case("s") => 7,
            _ => return None,
        };
        Some(1 << index)
    }

    fn direction(&self) -> (i32, i32) {
        let held = self.held | (self.held >> 4);
        let axis = |neg, pos| i32::from(held & pos != 0) - i32::from(held & neg != 0);
        (axis(1, 2), axis(4, 8))
    }

    fn set_key(&mut self, bit: u8, pressed: bool, now: Instant) {
        let prior = self.direction();
        if pressed {
            self.held |= bit;
        } else {
            self.held &= !bit;
        }
        let after = self.direction();
        if after != prior {
            self.remainder = (0., 0.);
            self.sampled_at = (after != (0, 0)).then_some(now);
        }
    }

    fn deadline(&self) -> Option<Instant> {
        self.sampled_at.map(|last| last + CAMERA_TICK)
    }

    fn advance(&mut self, now: Instant, fast: bool, blocked: bool) -> (i32, i32) {
        let Some(last) = self.sampled_at.replace(now) else {
            return (0, 0);
        };
        if blocked {
            self.remainder = (0., 0.);
            return (0, 0);
        }
        let (x, y) = self.direction();
        let speed = if fast { 1000. } else { 300. };
        let distance = speed
            * now
                .saturating_duration_since(last)
                .min(Duration::from_millis(100))
                .as_secs_f64()
            / if x != 0 && y != 0 {
                std::f64::consts::SQRT_2
            } else {
                1.
            };
        // These quanta preserve exact integer steps in both inverse isometric axes.
        let axis = |sign, quantum: f64, remainder: &mut f64| {
            let value = *remainder + f64::from(sign) * distance;
            let step = (value / quantum).trunc() * quantum;
            *remainder = value - step;
            step as i32
        };
        (
            axis(x, 4., &mut self.remainder.0),
            axis(y, 2., &mut self.remainder.1),
        )
    }
}

struct WindowSurface {
    renderer: gpu::WindowRenderer,
    map_inset: Option<Arc<LoadedFrame>>,
    inventory_inset: Option<Arc<LoadedFrame>>,
    developer_status: Option<Arc<LoadedFrame>>,
    save_status: Option<(Arc<LoadedFrame>, Instant)>,
    last_frame_time: Duration,
    frame_presented_at: Option<Instant>,
    last_frame_interval: Option<Duration>,
    submitted_frames: u64,
    window: Arc<Window>,
    occluded: bool,
}

impl Viewer<'_> {
    fn schedule_animation(&mut self, event_loop: &ActiveEventLoop, deadline: Option<Instant>) {
        self.animation_deadline = deadline;
        self.update_control_flow(event_loop);
    }

    fn play_elapsed(&self, now: Instant) -> Duration {
        self.pause
            .as_ref()
            .map_or(self.map_since.unwrap_or(now), |(_, paused)| *paused)
            .saturating_duration_since(self.epoch)
            .saturating_sub(self.paused_for)
    }

    fn game_elapsed(&self, now: Instant) -> Duration {
        self.pause
            .as_ref()
            .map_or(now, |(_, paused)| *paused)
            .saturating_duration_since(self.epoch)
            .saturating_sub(self.clock_paused_for)
    }

    fn change_location(&mut self, target: Destination, facing: u8) -> Result<(), String> {
        self.load_location(target, facing, false)
    }

    fn load_location(
        &mut self,
        target: Destination,
        facing: u8,
        bypass: bool,
    ) -> Result<(), String> {
        self.enter_location(target, facing, LocationMode::Travel { bypass })
    }

    fn enter_location(
        &mut self,
        target: Destination,
        facing: u8,
        mode: LocationMode<'_>,
    ) -> Result<(), String> {
        let (map_hint, hour, bypass) = match &mode {
            LocationMode::Save { state, .. } => (
                Some(state.map.as_str()),
                Some((state.clock.0 % 24) as u8),
                false,
            ),
            LocationMode::Travel { bypass } => (
                None,
                self.clock
                    .as_ref()
                    .map(|clock| clock.hour(self.game_elapsed(Instant::now()))),
                *bypass,
            ),
        };
        let restoring = map_hint.is_some();
        let view = self.view.as_mut().ok_or("World window is unavailable")?;
        let ViewContent::Stream {
            world,
            camera,
            travel: Some(travel),
            trace,
            direction,
            waiting,
            last_presented,
            last_viewport,
            hud,
            ..
        } = &mut self.content
        else {
            return Err("World transition is unavailable".into());
        };
        let size = view.window.inner_size();
        let (map, next, next_camera) = (travel.loader)(
            map_hint.unwrap_or(&travel.map),
            world,
            target,
            facing,
            logical_size(size, self.settings.zoom),
            hour,
            mode,
        )?;
        let wake = travel.wake.clone();
        let mut next = next
            .into_interactive(
                move || {
                    let _ = wake.send_event(());
                },
                *trace,
            )
            .map_err(|err| err.to_string())?;
        if !restoring {
            travel
                .doors
                .insert(travel.map.clone(), world.take_door_states());
            if let Some(states) = travel.doors.remove(&map) {
                next.restore_door_states(states);
            }
            next.inherit_object_state(world);
        }
        eprintln!("Entered {map} at {:?}, game hour {hour:?}", target.tile);
        view.window.set_title(&format!("Arcanum-RS | {map} play"));
        view.developer_status = bypass.then(|| menu::developer_status("COLLISION OFF"));
        travel.map = map;
        if world.conversation().is_some()
            && let Some(audio) = &self.audio
        {
            audio.voice(None);
        }
        **world = next;
        if let Some(hud) = hud {
            hud.close_inventory();
        }
        self.sounding_container = None;
        self.reset_audio |= restoring;
        *camera = next_camera;
        world.follow_player(Duration::ZERO, camera);
        *last_presented = None;
        *last_viewport = (0, 0);
        *waiting = false;
        *direction = (0, 0);
        self.navigation = Navigation::default();
        self.player_deadline = None;
        self.animation_deadline = None;
        view.window.request_redraw();
        self.reset_developer_location();
        if let Some(dev) = &mut self.developer {
            dev.event("map_loaded", None, serde_json::json!({"target":target}));
        }
        Ok(())
    }

    fn finish_travel(&mut self, event_loop: &ActiveEventLoop, arrival: Arrival) {
        let (map, tile, cancelled) = match arrival {
            Arrival::Destination(map, tile) => (map, Some(tile), false),
            Arrival::Cancelled(map, tile) => (map, tile, true),
        };
        let facing = if let ViewContent::Stream { world, .. } = &mut self.content {
            world.cancel_player_route();
            world.player_facing().unwrap_or(4)
        } else {
            return;
        };
        let loaded = tile.map_or(Ok(()), |tile| {
            self.change_location(
                Destination {
                    map: Some(map),
                    tile,
                },
                facing,
            )
        });
        let hour = self
            .clock
            .as_ref()
            .map(|clock| clock.hour(self.game_elapsed(Instant::now())));
        if let ViewContent::Stream { world, .. } = &mut self.content
            && let Some(hour) = hour
        {
            world.set_game_hour(hour);
        }
        if let Err(error) = loaded {
            if let ViewContent::Stream {
                map_ui: Some(ui), ..
            } = &mut self.content
            {
                ui.travel_notice(&format!("Cannot arrive: {error}"));
            }
            if !cancelled {
                if let Some(view) = &self.view {
                    view.window.request_redraw();
                }
                self.update_control_flow(event_loop);
                return;
            }
            eprintln!(
                "Cancellation stopped at the departure location; the current route sector cannot be entered on foot."
            );
        }
        self.close_map(event_loop);
    }

    fn pause_game(&mut self, event_loop: &ActiveEventLoop) {
        if let ViewContent::Stream { world, shift, .. } = &mut self.content {
            world.cancel_player_route();
            *shift = false;
        }
        if let ViewContent::Stream { hud: Some(hud), .. } = &mut self.content {
            hud.cancel_inventory_drag();
        }
        self.navigation = Navigation::default();
        self.player_deadline = None;
        self.animation_deadline = None;
        if let Some(audio) = &self.audio {
            audio.pause(true);
        }
        self.pause = Some((Menu::default(), Instant::now()));
        self.menu_action(MenuCommand::Stay, event_loop);
    }

    fn record_settings(&mut self, previous: Settings) {
        let current = self.settings;
        if current.audio != previous.audio
            && let Some(audio) = &self.audio
        {
            audio.volumes(current.audio);
        }
        if let Some(preferences) = &mut self.preferences {
            for (key, value, old) in [
                ("effects", current.audio.effects, previous.audio.effects),
                ("voice", current.audio.voice, previous.audio.voice),
                ("music", current.audio.music, previous.audio.music),
            ] {
                if value != old {
                    preferences.set("audio", key, i64::from(value));
                }
            }
            if current.zoom != previous.zoom {
                preferences.set("camera", "zoom", i64::from(current.zoom));
            }
            if current.lighting != previous.lighting {
                preferences.set("display", "lighting", current.lighting);
            }
            if current.shadows != previous.shadows {
                preferences.set("display", "shadows", current.shadows);
            }
            if current.run != previous.run {
                preferences.set("input", "run", current.run);
            }
        }
    }

    fn hud_action(&mut self, action: hud::Action, event_loop: &ActiveEventLoop) {
        let previous = self.settings;
        if let Some(audio) = &self.audio {
            audio.effect(3000, false);
        }
        match action {
            hud::Action::Pause => self.pause_game(event_loop),
            hud::Action::ToggleRun => self.settings.run = !self.settings.run,
            hud::Action::ResetZoom => self.settings.zoom = 1,
            hud::Action::OpenMap => self.open_map(event_loop),
            hud::Action::ToggleInventory => {
                if let ViewContent::Stream {
                    world,
                    hud: Some(hud),
                    inventory_reader: Some(reader),
                    ..
                } = &mut self.content
                    && !hud.close_inventory()
                    && let Err(error) = hud.open_inventory(reader, world, None)
                {
                    eprintln!("Inventory: {error}");
                }
            }
            hud::Action::InventoryTakeAll | hud::Action::InventoryArrange => {
                if let ViewContent::Stream {
                    hud: Some(hud),
                    inventory_reader: Some(reader),
                    world,
                    ..
                } = &mut self.content
                {
                    hud.inventory_action(reader, world, action);
                }
            }
            hud::Action::InventoryScrollUp | hud::Action::InventoryScrollDown => {
                if let ViewContent::Stream { hud: Some(hud), .. } = &mut self.content {
                    let step = if action == hud::Action::InventoryScrollUp {
                        -1
                    } else {
                        1
                    };
                    hud.scroll_inventory(None, step);
                    hud.hover(self.cursor);
                }
            }
            _ => {}
        }
        if let Some(view) = &self.view {
            view.window.request_redraw();
        }
        self.record_settings(previous);
    }

    fn menu_action(&mut self, command: MenuCommand, event_loop: &ActiveEventLoop) {
        match command {
            MenuCommand::Continue => {
                if let Some((_, paused)) = self.pause.take() {
                    let pause = paused.elapsed();
                    self.paused_for += pause;
                    self.clock_paused_for += pause;
                    if let Some(audio) = &self.audio {
                        audio.pause(false);
                    }
                }
                if matches!(&self.content, ViewContent::Stream { world, .. } if world.player_moving())
                {
                    self.player_deadline = Some(Instant::now());
                }
            }
            MenuCommand::Quit => event_loop.exit(),
            MenuCommand::Save { id, name } => match self.save_game(id.as_deref(), name, false) {
                Ok(()) => {
                    self.save_feedback("Game Saved Successfully");
                    self.menu_action(MenuCommand::Continue, event_loop);
                }
                Err(error) => self.menu_error(error),
            },
            MenuCommand::Load(id) => match self.restore_game(&id, event_loop) {
                Ok(()) => {
                    self.save_feedback("Game Loaded Successfully");
                    self.menu_action(MenuCommand::Continue, event_loop);
                }
                Err(error) => self.menu_error(error),
            },
            MenuCommand::Delete(id) => {
                let store = self.saves.as_ref().map_err(String::as_str);
                match store.and_then(|store| {
                    store.remove(&id).map_err(|error| {
                        eprintln!("Delete save: {error}");
                        "Unable to delete saved game"
                    })
                }) {
                    Ok(()) => {
                        if let Some(((menu, _), theme)) =
                            self.pause.as_mut().zip(self.pause_theme.as_ref())
                        {
                            menu.refresh(store, theme);
                        }
                        self.save_event("save_deleted", Some(&id), None);
                    }
                    Err(error) => self.menu_error(error.into()),
                }
            }
            MenuCommand::Stay => {}
        }
        if let Some(view) = &self.view {
            view.window.request_redraw();
        }
        self.update_control_flow(event_loop);
    }

    fn ui_scale(&self) -> u16 {
        match &self.content {
            ViewContent::Stream { hud: Some(hud), .. } => hud.scale(),
            _ => self.pause_scale,
        }
    }

    fn change_pause_scale(&mut self, delta: i32) {
        let scale = (i32::from(self.ui_scale()) + delta).clamp(50, 300) as u16;
        self.pause_scale = scale;
        if let ViewContent::Stream { hud, map_ui, .. } = &mut self.content {
            if let Some(hud) = hud {
                hud.set_scale(scale);
            }
            if let Some(map) = map_ui {
                map.set_scale(scale);
            }
        }
        if let Some(dev) = &mut self.developer {
            dev.set_configured_ui_scale(scale);
        }
        if let Some(preferences) = &mut self.preferences {
            preferences.set("ui", "scale", i64::from(scale));
        }
    }

    fn save_event(&mut self, kind: &str, slot: Option<&str>, error: Option<&str>) {
        if let Some(dev) = &mut self.developer {
            dev.event(kind, None, serde_json::json!({"slot":slot,"error":error}));
        }
    }

    fn menu_error(&mut self, error: String) {
        self.save_event("save_operation_failed", None, Some(&error));
        if let Some((menu, _)) = &mut self.pause {
            menu.message = Some(error);
        } else {
            self.save_feedback(&error);
        }
    }

    fn save_feedback(&mut self, message: &str) {
        eprintln!("{message}");
        if let Some(view) = &mut self.view {
            view.save_status = Some((
                menu::developer_status(
                    &message
                        .to_ascii_uppercase()
                        .chars()
                        .take(80)
                        .collect::<String>(),
                ),
                Instant::now() + Duration::from_secs(3),
            ));
            view.window.request_redraw();
        }
    }

    fn save_game(&mut self, id: Option<&str>, name: String, quick: bool) -> Result<(), String> {
        if name.trim().is_empty() {
            return Err("Enter a save name".into());
        }
        let store = self.saves.as_ref().map_err(Clone::clone)?;
        let now = Instant::now();
        let elapsed = self.play_elapsed(now);
        let clock = self
            .clock
            .as_ref()
            .ok_or("Game clock is unavailable")?
            .for_save(self.game_elapsed(now));
        let ViewContent::Stream {
            world,
            camera,
            map_ui,
            travel: Some(travel),
            ..
        } = &mut self.content
        else {
            return Err("No active gameplay session".into());
        };
        let (tile, _) = world
            .player_map_position()
            .ok_or("Wait for the current location to finish loading")?;
        let facing = world.player_facing().ok_or("Player is unavailable")?;
        let mut doors: BTreeMap<_, _> = travel
            .doors
            .iter()
            .map(|(map, doors)| (map.clone(), doors.for_save()))
            .collect();
        doors.insert(travel.map.clone(), world.door_states().for_save());
        let mut state = save::State {
            map: travel.map.clone(),
            tile,
            facing,
            clock,
            objects: world.save_objects(),
            doors,
            discovered: Default::default(),
            explored: Default::default(),
        };
        let location = map_ui
            .as_ref()
            .and_then(|map| map.location_name(&travel.map))
            .unwrap_or(&travel.map)
            .to_owned();
        if let Some(map) = map_ui {
            map.save_discovery(&mut state);
        }
        let view = self.view.as_mut().ok_or("World window is unavailable")?;
        let mut preview_camera = camera.clone();
        if self.settings.follow {
            world.follow_player(elapsed, &mut preview_camera);
        }
        let preview = view.renderer.save_preview(scene_draws(
            world,
            &preview_camera,
            save::PREVIEW_SIZE,
            elapsed,
            self.settings,
        ))?;
        let metadata = save::Metadata {
            name,
            location,
            hours: clock.0,
            saved_at_ms: 0,
        };
        if quick {
            store.quicksave(metadata, &state, &preview)?;
        } else {
            store.write(id, metadata, &state, &preview)?;
        }
        self.save_event("save_written", id.or(quick.then_some("auto")), None);
        Ok(())
    }

    fn restore_game(&mut self, id: &str, event_loop: &ActiveEventLoop) -> Result<(), String> {
        let store = self.saves.as_ref().map_err(Clone::clone)?;
        let state = store.load(id)?;
        if let ViewContent::Stream {
            map_ui: Some(map), ..
        } = &self.content
        {
            map.validate_discovery(&state)?;
        }
        let mut doors: BTreeMap<_, _> = state
            .doors
            .iter()
            .map(|(map, states)| Ok((map.clone(), DoorStates::from_save(states)?)))
            .collect::<Result<_, String>>()?;
        let active_doors = doors.remove(&state.map).unwrap_or_default();
        self.enter_location(
            Destination {
                map: None,
                tile: state.tile,
            },
            state.facing,
            LocationMode::Save {
                state: &state,
                doors: active_doors,
            },
        )?;

        let clock_start = self
            .pause
            .as_ref()
            .map_or_else(Instant::now, |(_, paused)| *paused);
        self.clock_paused_for = clock_start.saturating_duration_since(self.epoch);
        self.clock = Some(Clock::from_save(state.clock));
        if self.map_since.is_some() {
            self.close_map(event_loop);
        }
        if let ViewContent::Stream {
            travel: Some(travel),
            map_ui,
            ..
        } = &mut self.content
        {
            travel.doors = doors;
            if let Some(map) = map_ui {
                map.restore_discovery(state);
            }
        }
        self.save_event("save_loaded", Some(id), None);
        self.update_control_flow(event_loop);
        Ok(())
    }

    fn quick_save_or_load(&mut self, key: &NamedKey, event_loop: &ActiveEventLoop) {
        let result = if *key == NamedKey::F7 {
            if self.map_since.is_some() {
                if let ViewContent::Stream {
                    map_ui: Some(map), ..
                } = &mut self.content
                    && let Some(arrival) = map.cancel_journey()
                {
                    self.finish_travel(event_loop, arrival);
                } else {
                    self.close_map(event_loop);
                }
            }
            self.save_game(None, "Auto-Save".into(), true)
                .map(|()| "Game Saved Successfully")
        } else {
            let result = self
                .saves
                .as_ref()
                .map_err(Clone::clone)
                .and_then(save::Store::recent);
            result
                .and_then(|id| self.restore_game(&id, event_loop))
                .map(|()| "Game Loaded Successfully")
        };
        match result {
            Ok(message) => self.save_feedback(message),
            Err(error) => self.menu_error(error),
        }
        self.update_control_flow(event_loop);
    }

    fn pause_event(&mut self, event_loop: &ActiveEventLoop, event: &WindowEvent) -> bool {
        let scale = self.ui_scale();
        let Some(((menu, _), theme)) = self.pause.as_mut().zip(self.pause_theme.as_ref()) else {
            return false;
        };
        let view = self
            .view
            .as_ref()
            .expect("pause input has an active window");
        let size = view.window.inner_size();
        let physical = [size.width, size.height];
        let previous = self.settings;
        let store = self.saves.as_ref().map_err(String::as_str);
        if let WindowEvent::KeyboardInput { event, .. } = event
            && event.state.is_pressed()
            && let Key::Character(key) = &event.logical_key
            && matches!(key.as_str(), "[" | "]")
        {
            self.change_pause_scale(if key == "[" { -25 } else { 25 });
            self.menu_action(MenuCommand::Stay, event_loop);
            return true;
        }
        let mut options_done = false;
        let command = if let Some(options) = &mut menu.options {
            match event {
                WindowEvent::KeyboardInput { event, .. }
                    if event.state.is_pressed() && !event.repeat =>
                {
                    options_done = options.key(&event.logical_key);
                }
                WindowEvent::CursorMoved { .. } | WindowEvent::CursorLeft { .. } => {
                    if !options.hover(self.cursor, physical, scale, theme, &mut self.settings) {
                        return true;
                    }
                }
                WindowEvent::Focused(false) => {
                    options.cancel_pointer();
                }
                WindowEvent::MouseInput {
                    state,
                    button: MouseButton::Left,
                    ..
                } => {
                    let (done, effect) = options.mouse(
                        state.is_pressed(),
                        self.cursor,
                        physical,
                        scale,
                        theme,
                        &mut self.settings,
                    );
                    options_done = done;
                    if let Some(id) = effect
                        && let Some(audio) = &self.audio
                    {
                        audio.effect(id, false);
                    }
                }
                WindowEvent::MouseInput { .. }
                | WindowEvent::KeyboardInput { .. }
                | WindowEvent::MouseWheel { .. } => return true,
                _ => return false,
            }
            if options_done {
                menu.options = None;
            }
            MenuCommand::Stay
        } else {
            match event {
                WindowEvent::KeyboardInput { event, .. } if event.state.is_pressed() => {
                    if event.repeat && !menu.editing() {
                        return true;
                    }
                    menu.key(&event.logical_key, event.text.as_deref(), theme, store)
                }
                WindowEvent::CursorMoved { .. } | WindowEvent::CursorLeft { .. } => {
                    if menu.hover(self.cursor, physical, scale, theme) {
                        view.window.request_redraw();
                    }
                    return true;
                }
                WindowEvent::MouseInput {
                    state,
                    button: MouseButton::Left,
                    ..
                } => {
                    menu.hover(self.cursor, physical, scale, theme);
                    if state.is_pressed()
                        && menu.has_hover()
                        && let Some(audio) = &self.audio
                    {
                        audio.effect(3027, false);
                    }
                    menu.mouse(
                        state.is_pressed(),
                        (self.cursor, physical, scale),
                        theme,
                        store,
                    )
                }
                WindowEvent::MouseWheel { delta, .. } => {
                    let y = match delta {
                        MouseScrollDelta::LineDelta(_, y) => f64::from(*y),
                        MouseScrollDelta::PixelDelta(point) => point.y,
                    };
                    menu.scroll(
                        if y > 0. {
                            -1
                        } else if y < 0. {
                            1
                        } else {
                            0
                        },
                        theme,
                    );
                    MenuCommand::Stay
                }
                WindowEvent::KeyboardInput { .. }
                | WindowEvent::MouseInput { .. }
                | WindowEvent::ModifiersChanged(_) => return true,
                _ => return false,
            }
        };
        self.record_settings(previous);
        if options_done
            && let Some(preferences) = &mut self.preferences
            && let Err(error) = preferences.flush()
        {
            let error = format!(
                "Cannot save preferences to {}: {error}",
                preferences.path().display()
            );
            eprintln!("{error}");
            self.menu_error(error);
        }
        self.menu_action(command, event_loop);
        true
    }

    fn open_map(&mut self, event_loop: &ActiveEventLoop) {
        if let ViewContent::Stream {
            map_ui: Some(map),
            hud,
            shift,
            world,
            trace,
            travel,
            ..
        } = &mut self.content
        {
            if let Some((tile, town)) = world.player_map_position() {
                if let Some(travel) = travel {
                    map.observe(&travel.map, tile, town);
                }
                if *trace {
                    eprintln!("Map player tile {tile:?}, town map {town}");
                }
            }
            if let Some(hud) = hud {
                hud.close_inventory();
            }
            map.open(hud.as_ref().map(|hud| hud.scale()));
            *shift = false;
            self.map_since = Some(Instant::now());
            self.navigation = Navigation::default();
            self.player_deadline = None;
            self.schedule_animation(event_loop, None);
            if let Some(view) = &mut self.view {
                view.map_inset = None;
                view.window.request_redraw();
            }
        }
    }

    fn close_map(&mut self, event_loop: &ActiveEventLoop) {
        if let ViewContent::Stream {
            map_ui: Some(map),
            world,
            hud,
            ..
        } = &mut self.content
        {
            map.close();
            if let Some(hud) = hud {
                hud.set_label(hud::Binding::InventoryHover, "");
            }
            self.paused_for += self.map_since.take().expect("open map timestamp").elapsed();
            self.player_deadline = world.player_moving().then(Instant::now);
            if let Some(view) = &mut self.view {
                view.map_inset = None;
                view.window.request_redraw();
            }
        }
        self.update_control_flow(event_loop);
    }

    fn map_event(&mut self, loop_: &ActiveEventLoop, event: &WindowEvent) -> bool {
        let now = self.epoch.elapsed();
        let elapsed = self.play_elapsed(Instant::now());
        let settings = self.settings;
        let hours = self
            .clock
            .as_ref()
            .map(|clock| clock.hours(self.game_elapsed(Instant::now())));
        let ViewContent::Stream {
            map_ui: Some(map),
            hud,
            shift,
            world,
            camera,
            last_presented,
            ..
        } = &mut self.content
        else {
            return false;
        };
        if !map.is_open() {
            return false;
        }
        let view = self.view.as_mut().unwrap();
        let mut arrival = None;
        match event {
            WindowEvent::RedrawRequested => {
                let size = view.window.inner_size();
                if size.width > 0 && size.height > 0 {
                    let viewport = (size.width, size.height);
                    let band = hud.as_deref_mut().map(|hud| {
                        if let Some(hours) = hours {
                            hud.set_game_time(hours);
                        }
                        hud.layout_with_panel(viewport, map.panel_size())
                    });
                    let map = map.prepare(viewport, self.cursor, band, hud.as_deref_mut());
                    let camera = last_presented.as_ref().unwrap_or(camera);
                    if let Some([w, h]) = map.lens_size()
                        && let Some((rect, _)) = map.bounds(hud::Binding::PlayerLens)
                    {
                        if view.map_inset.as_ref().is_none_or(|asset| {
                            (asset.decoded.frame.width, asset.decoded.frame.height)
                                != (rect.w, rect.h)
                        }) {
                            let asset = hud::generated_frame(
                                "map-player-inset",
                                SpriteFrame {
                                    width: rect.w,
                                    height: rect.h,
                                    hotspot: (0, 0),
                                    offset: (0, 0),
                                    pixels: Vec::new(),
                                },
                            );
                            let source = (w * 2, h * 2);
                            let scale = (rect.w as f32 / source.0 as f32)
                                .min(rect.h as f32 / source.1 as f32);
                            let scene = scene_draws(world, camera, source, elapsed, settings);
                            if let Err(error) = view.renderer.render_inset(
                                &asset,
                                scale,
                                stream_background(true),
                                scene,
                            ) {
                                self.fail(loop_, error);
                                return true;
                            }
                            view.map_inset = Some(asset);
                        }
                        map.set_lens_asset(
                            hud::Binding::PlayerLens,
                            Arc::clone(view.map_inset.as_ref().unwrap()),
                        );
                    }
                    let scene = scene_draws(
                        world,
                        camera,
                        logical_size(size, settings.zoom),
                        elapsed,
                        settings,
                    );
                    if let Err(error) =
                        view.redraw_map(size, settings.zoom, scene, map, hud.as_deref())
                    {
                        self.fail(loop_, error);
                    }
                }
                return true;
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = (position.x >= 0. && position.y >= 0.)
                    .then_some((position.x as u32, position.y as u32));
                map.cursor(self.cursor);
            }
            WindowEvent::CursorLeft { .. } => {
                self.cursor = None;
                map.cursor(None);
                map.cancel_drag();
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => {
                if let Some(point) = self.cursor {
                    if state.is_pressed() {
                        map.mouse_down(point);
                    } else {
                        arrival = map.mouse_up(point, now);
                    }
                } else {
                    map.cancel_drag();
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Released,
                button: MouseButton::Right,
                ..
            } => arrival = map.remove_waypoint(self.cursor),
            WindowEvent::MouseWheel { delta, .. } => {
                let delta = match delta {
                    MouseScrollDelta::LineDelta(_, y) => f64::from(*y),
                    MouseScrollDelta::PixelDelta(p) => p.y,
                };
                map.zoom(i32::from(delta > 0.) - i32::from(delta < 0.));
            }
            WindowEvent::ModifiersChanged(modifiers) => *shift = modifiers.state().shift_key(),
            WindowEvent::Focused(false) => {
                map.cancel_drag();
                *shift = false;
            }
            WindowEvent::KeyboardInput {
                event,
                is_synthetic: false,
                ..
            } if event.state.is_pressed() => match &event.logical_key {
                Key::Named(NamedKey::Escape) if !event.repeat => {
                    if let Some(arrival) = map.cancel_journey() {
                        self.finish_travel(loop_, arrival);
                    } else {
                        self.close_map(loop_);
                    }
                    return true;
                }
                Key::Character(key) if key.eq_ignore_ascii_case("m") && !event.repeat => {
                    if let Some(arrival) = map.cancel_journey() {
                        self.finish_travel(loop_, arrival);
                    } else {
                        self.close_map(loop_);
                    }
                    return true;
                }
                Key::Character(key) if key.eq_ignore_ascii_case("z") && !event.repeat => {
                    map.toggle_overview(*shift);
                }
                Key::Named(NamedKey::ArrowLeft) => map.pan_keys(-32, 0),
                Key::Named(NamedKey::ArrowRight) => map.pan_keys(32, 0),
                Key::Named(NamedKey::ArrowUp) => map.pan_keys(0, -32),
                Key::Named(NamedKey::ArrowDown) => map.pan_keys(0, 32),
                Key::Named(NamedKey::Home) => map.home(),
                Key::Named(NamedKey::Tab) => map.focus(*shift),
                Key::Named(NamedKey::Enter | NamedKey::Space) if !event.repeat => {
                    arrival = map.activate_focus(now);
                }
                Key::Character(key) if key == "[" || key == "]" => {
                    let previous = map.scale();
                    let scale = map.adjust_scale(if key == "[" { -25 } else { 25 });
                    if scale != previous
                        && let Some(preferences) = &mut self.preferences
                    {
                        preferences.set("ui", "scale", i64::from(scale));
                    }
                    if let Some(hud) = hud {
                        hud.set_scale(scale);
                    }
                }
                _ => return true,
            },
            WindowEvent::KeyboardInput { .. } | WindowEvent::MouseInput { .. } => return true,
            WindowEvent::CloseRequested | WindowEvent::Destroyed => return false,
            WindowEvent::Occluded(occluded) => {
                view.occluded = *occluded;
                if *occluded {
                    return true;
                }
            }
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {}
            _ => return true,
        }
        if let Some(arrival) = arrival {
            self.finish_travel(loop_, arrival);
        } else {
            view.window.request_redraw();
            self.update_control_flow(loop_);
        }
        true
    }

    fn update_control_flow(&self, event_loop: &ActiveEventLoop) {
        let real_now = Instant::now();
        let clock = self
            .clock
            .as_ref()
            .filter(|_| self.pause.is_none())
            .map(|clock| real_now + clock.until_next_hour(self.game_elapsed(real_now)));
        let travel = match &self.content {
            ViewContent::Stream {
                map_ui: Some(map), ..
            } if map.is_open() => map.travel_deadline().map(|time| self.epoch + time),
            _ => None,
        };
        let next = self
            .animation_deadline
            .into_iter()
            .chain(self.navigation.deadline())
            .chain(self.player_deadline)
            .chain(clock)
            .chain(travel)
            .chain(
                self.view
                    .as_ref()
                    .and_then(|view| view.save_status.as_ref().map(|(_, until)| *until)),
            )
            .min();
        event_loop.set_control_flow(next.map_or(ControlFlow::Wait, ControlFlow::WaitUntil));
    }

    fn update_light_probe(world: &mut InteractiveWorld, key: &str, trace: bool) -> bool {
        if key == "s" {
            let Some(enabled) = world.toggle_shadows() else {
                return false;
            };
            if trace {
                eprintln!("Mobile shadows enabled: {enabled}");
            }
            return true;
        }
        if key == "3" {
            let start = Instant::now();
            let Some((eligible, changed)) = world.toggle_probe_shadow(0) else {
                return false;
            };
            if trace {
                eprintln!(
                    "Probe 0 directional shadow eligible: {eligible}, changed: {changed}, shadow update {:?}",
                    start.elapsed()
                );
            }
            return changed;
        }
        let (slot, motion) = match key {
            "i" => (0, Some((0, -1))),
            "k" => (0, Some((0, 1))),
            "j" => (0, Some((-1, 0))),
            "l" => (0, Some((1, 0))),
            "1" => (0, None),
            "2" => (1, None),
            _ => return false,
        };
        let Some(mut state) = world.dynamic_light(slot) else {
            return false;
        };
        if let Some((dx, dy)) = motion {
            let (Some(x), Some(y)) = (
                state.tile.0.checked_add_signed(dx),
                state.tile.1.checked_add_signed(dy),
            ) else {
                return false;
            };
            state.tile = (x, y);
        } else {
            state.active = !state.active;
        }
        let resources = trace.then(|| world.stats().cache);
        let start = Instant::now();
        let Ok(update) = world.set_dynamic_light(slot, state) else {
            return false;
        };
        if let Some(before) = resources {
            let after = world.stats().cache;
            eprintln!(
                "Dynamic light {slot} {state:?}: update {update:?}, main {:?}, sectors +{}, VFS +{}, decodes +{}",
                start.elapsed(),
                after.sector_reads - before.sector_reads,
                after.vfs_reads - before.vfs_reads,
                after.art.decodes - before.art.decodes
            );
        }
        update.receivers != 0 || update.shadow_changed
    }

    fn open(&self, event_loop: &ActiveEventLoop) -> Result<WindowSurface, String> {
        let attributes = Window::default_attributes()
            .with_title(&self.title)
            .with_inner_size(self.size)
            .with_fullscreen(self.fullscreen.then(|| Fullscreen::Borderless(None)));
        let window = Arc::new(
            event_loop
                .create_window(attributes)
                .map_err(|error| format!("cannot create sprite window: {error}"))?,
        );
        let trace = matches!(&self.content, ViewContent::Stream { trace: true, .. });
        let capture = self
            .developer
            .as_ref()
            .is_some_and(Developer::capture_enabled);
        let renderer = gpu::WindowRenderer::new(Arc::clone(&window), trace, capture)?;
        Ok(WindowSurface {
            renderer,
            map_inset: None,
            inventory_inset: None,
            developer_status: None,
            save_status: None,
            last_frame_time: Duration::ZERO,
            frame_presented_at: None,
            last_frame_interval: None,
            submitted_frames: 0,
            window,
            occluded: false,
        })
    }

    fn fail(&mut self, event_loop: &ActiveEventLoop, error: String) {
        self.failure = Some(error);
        event_loop.exit();
    }
}

impl ApplicationHandler for Viewer<'_> {
    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: StartCause) {
        if !matches!(cause, StartCause::ResumeTimeReached { .. }) {
            return;
        }
        let now = Instant::now();
        if let Some(view) = &mut self.view
            && view
                .save_status
                .as_ref()
                .is_some_and(|(_, until)| *until <= now)
        {
            view.save_status = None;
            view.window.request_redraw();
            self.update_control_flow(event_loop);
        }
        if self.pause.is_some() {
            return;
        }
        let game_elapsed = self.game_elapsed(now);
        if self.map_since.is_some() {
            if let ViewContent::Stream {
                map_ui: Some(map),
                world,
                hud,
                ..
            } = &mut self.content
            {
                let (steps, arrival) =
                    map.advance_travel(now.saturating_duration_since(self.epoch));
                let mut redraw = steps > 0;
                if let Some(clock) = &mut self.clock {
                    clock.step_travel(steps);
                    if !map.travelling() && arrival.is_none() {
                        redraw |= world.set_game_hour(clock.hour(game_elapsed));
                    }
                    if let Some(hud) = hud {
                        redraw |= hud.set_game_time(clock.hours(game_elapsed));
                    }
                }
                if let Some(arrival) = arrival {
                    self.finish_travel(event_loop, arrival);
                } else if redraw && let Some(view) = &self.view {
                    view.window.request_redraw();
                }
            }
            self.update_control_flow(event_loop);
            return;
        }
        let elapsed = self.play_elapsed(now);
        let mut transition = None;
        let mut redraw = self
            .animation_deadline
            .is_some_and(|deadline| now >= deadline);
        if redraw {
            self.animation_deadline = None;
        }
        if let Some(view) = &self.view {
            if let ViewContent::Stream {
                world,
                camera,
                shift,
                direction,
                waiting,
                hud,
                inventory_reader,
                ..
            } = &mut self.content
            {
                if let Some(clock) = &self.clock {
                    redraw |= world.set_game_hour(clock.hour(game_elapsed));
                    if let Some(hud) = hud.as_mut() {
                        redraw |= hud.set_game_time(clock.hours(game_elapsed));
                    }
                }
                if world.playing() {
                    if self.player_deadline.is_some_and(|deadline| now >= deadline) {
                        if !*waiting && !view.occluded {
                            let before = camera.position();
                            redraw |= world.advance_player(
                                elapsed,
                                self.settings.follow.then_some(&mut *camera),
                            );
                            let after = camera.position();
                            if before != after {
                                *direction = (after.0 - before.0, after.1 - before.1);
                            }
                        }
                        if let Some(reader) = inventory_reader.as_mut()
                            && let Some(result) = reader.start_npc_interaction(world)
                        {
                            match result {
                                Ok(())
                                    if hud.as_ref().is_some_and(|hud| hud.supports_dialogue()) =>
                                {
                                    if let Some(hud) = hud.as_mut() {
                                        hud.close_inventory();
                                        hud.cancel_press();
                                    }
                                    self.navigation = Navigation::default();
                                    world.update_hover(None, (0, 0), camera, elapsed);
                                    if let Some(audio) = &self.audio {
                                        audio.voice(
                                            world
                                                .conversation()
                                                .and_then(|s| s.turn.voice.as_deref()),
                                        );
                                    }
                                }
                                Ok(()) => {
                                    world.close_dialogue("theme_has_no_dialogue_components");
                                    view.window.set_title(
                                        "Arcanum-RS | Selected UI has no dialogue components",
                                    );
                                }
                                Err(error) => {
                                    eprintln!("Dialogue: {error}");
                                    view.window.set_title(&format!("Arcanum-RS | {error}"));
                                }
                            }
                            redraw = true;
                        }
                        if let Some(target) = world.take_container_request()
                            && let (Some(hud), Some(reader)) =
                                (hud.as_mut(), inventory_reader.as_mut())
                        {
                            if let Err(error) = hud.open_inventory(reader, world, Some(target)) {
                                eprintln!("Container: {error}");
                                view.window.set_title(&format!("Arcanum-RS | {error}"));
                            }
                            redraw = true;
                        }
                        if let Some(item) = world.take_pickup_request()
                            && let Some(reader) = inventory_reader
                        {
                            if let Err(error) = reader.pickup(world, &item) {
                                eprintln!("Pickup: {error}");
                                view.window.set_title(&format!("Arcanum-RS | {error}"));
                            }
                            redraw = true;
                        }
                        transition = world.take_transition();
                        if transition.is_some() {
                            world.cancel_player_route();
                        }
                        if let Some(message) = world.take_notice() {
                            eprintln!("{message}");
                            view.window.set_title(&format!("Arcanum-RS | {message}"));
                        }
                        self.player_deadline =
                            (!view.occluded && world.player_moving()).then_some(now + CAMERA_TICK);
                    }
                } else if self
                    .navigation
                    .deadline()
                    .is_some_and(|deadline| now >= deadline)
                {
                    let delta = self
                        .navigation
                        .advance(now, *shift, *waiting || view.occluded);
                    let size = view.window.inner_size();
                    if delta != (0, 0) && camera.pan(delta, (size.width, size.height)) {
                        *direction = delta;
                        redraw = true;
                    }
                }
            }
            if redraw {
                view.window.request_redraw();
            }
        }
        if let Some((target, facing)) = transition
            && let Err(error) = self.change_location(target, facing)
        {
            eprintln!("Cannot travel to {target:?}: {error}");
            if let Some(view) = &self.view {
                view.window
                    .set_title(&format!("Arcanum-RS | Travel unavailable: {error}"));
            }
        }
        self.update_control_flow(event_loop);
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, _: ()) {
        let paste = self.developer_paste();
        if let ViewContent::Stream { world, .. } = &mut self.content {
            let received = world.receive();
            if let Some((id, accepted, duration)) = world.take_diagnostic_completion()
                && let Some(dev) = &mut self.developer
            {
                dev.event(
                    "sector_completed",
                    None,
                    serde_json::json!({
                        "sector":id,"accepted":accepted,"elapsed":duration,
                    }),
                );
            }
            if (received || paste)
                && self.map_since.is_none()
                && let Some(view) = &self.view
            {
                view.window.request_redraw();
            }
        }
        self.pump_automation(event_loop);
    }

    fn about_to_wait(&mut self, _: &ActiveEventLoop) {
        let ViewContent::Stream { world, hud, .. } = &mut self.content else {
            return;
        };
        let Some(audio) = &mut self.audio else {
            world.take_sounds().for_each(drop);
            return;
        };
        if let Some((map, sector, hour)) = world.sound_location() {
            audio.location(map, sector, hour, std::mem::take(&mut self.reset_audio));
        }
        for sound in world.take_sounds() {
            match sound {
                SoundEvent::Effect(id) => audio.effect(id, true),
                SoundEvent::Footstep { surface, armor } => audio.footstep(surface, armor),
            }
        }
        let container = hud.as_ref().and_then(|hud| hud.container_sound());
        let previous = self
            .sounding_container
            .as_ref()
            .map(|(id, sound)| (id.as_str(), *sound));
        if previous != container {
            if let Some((_, sound)) = previous {
                audio.effect(sound + 1, false);
            }
            if let Some((_, sound)) = container {
                audio.effect(sound, false);
            }
            self.sounding_container = container.map(|(id, sound)| (id.to_owned(), sound));
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.view.is_none() {
            match self.open(event_loop) {
                Ok(view) => {
                    view.window.request_redraw();
                    self.view = Some(view);
                }
                Err(error) => self.fail(event_loop, error),
            }
        }
    }

    fn suspended(&mut self, event_loop: &ActiveEventLoop) {
        self.navigation = Navigation::default();
        self.player_deadline = None;
        self.schedule_animation(event_loop, None);
        self.view = None;
    }

    fn exiting(&mut self, _: &ActiveEventLoop) {
        // Destroy EGL/Vulkan presentation resources while winit's display
        // and event loop are still alive.
        self.view = None;
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        if self.view.as_ref().is_none_or(|view| view.window.id() != id) {
            return;
        }
        if self.developer_event(event_loop, &event) {
            return;
        }
        let playing = matches!(&self.content, ViewContent::Stream { world, .. } if world.playing());
        if self.dialogue_event(&event) {
            return;
        }
        if playing
            && self.pause.is_none()
            && let WindowEvent::KeyboardInput {
                event: key,
                is_synthetic: false,
                ..
            } = &event
            && key.state.is_pressed()
            && !key.repeat
            && let Key::Named(named @ (NamedKey::F7 | NamedKey::F8)) = &key.logical_key
        {
            self.quick_save_or_load(named, event_loop);
            return;
        }
        if self.map_event(event_loop, &event) {
            return;
        }
        let elapsed = self.play_elapsed(Instant::now());
        if playing {
            match &event {
                WindowEvent::CursorMoved { position, .. } => {
                    self.cursor = (position.x >= 0. && position.y >= 0.)
                        .then_some((position.x as u32, position.y as u32));
                }
                WindowEvent::CursorLeft { .. } => self.cursor = None,
                WindowEvent::KeyboardInput {
                    event,
                    is_synthetic: false,
                    ..
                } if event.state.is_pressed()
                    && !event.repeat
                    && event.logical_key == Key::Named(NamedKey::Escape)
                    && self.pause.is_none() =>
                {
                    if let ViewContent::Stream { hud: Some(hud), .. } = &mut self.content
                        && (hud.cancel_inventory_drag() || hud.close_inventory())
                    {
                        self.view.as_ref().unwrap().window.request_redraw();
                    } else {
                        self.pause_game(event_loop);
                    }
                    return;
                }
                _ => {}
            }
            if self.pause.is_some() && self.pause_event(event_loop, &event) {
                return;
            }
            if self.pause.is_some() {
                self.render_event(event_loop, id, event, elapsed);
                return;
            }
        }
        if playing
            && let ViewContent::Stream {
                world, debug: true, ..
            } = &mut self.content
            && let WindowEvent::KeyboardInput {
                event: key,
                is_synthetic: false,
                ..
            } = &event
            && key.state.is_pressed()
            && !key.repeat
            && key.logical_key == Key::Named(NamedKey::F6)
        {
            use arcanum_rs::content::world::DeveloperAction;
            let (label, message) = match world.developer_action(DeveloperAction::ToggleCollision) {
                Ok(true) => ("COLLISION OFF   F6 TO RESTORE", "Collision disabled".into()),
                Ok(false) => ("COLLISION ON", "Collision enabled".into()),
                Err(message) => ("COLLISION OFF   MOVE TO CLEAR GROUND", message),
            };
            let view = self.view.as_mut().unwrap();
            view.developer_status = Some(menu::developer_status(label));
            eprintln!("Developer F6: {message}");
            view.window.set_title(&format!("Arcanum-RS | {message}"));
            view.window.request_redraw();
            return;
        }
        if let ViewContent::Stream {
            hud: Some(hud),
            inventory_reader: Some(reader),
            world,
            ..
        } = &mut self.content
            && hud.choosing_quantity()
        {
            if let WindowEvent::KeyboardInput {
                event: key,
                is_synthetic: false,
                ..
            } = &event
                && key.state.is_pressed()
            {
                hud.inventory_quantity_key(reader, world, &key.logical_key);
                self.view.as_ref().unwrap().window.request_redraw();
            }
            if matches!(
                event,
                WindowEvent::KeyboardInput { .. }
                    | WindowEvent::MouseInput { .. }
                    | WindowEvent::MouseWheel { .. }
            ) {
                return;
            }
        }
        if playing
            && let WindowEvent::KeyboardInput {
                event: key,
                is_synthetic: false,
                ..
            } = &event
            && key.state.is_pressed()
            && !key.repeat
            && matches!(&key.logical_key, Key::Character(ch) if ch.eq_ignore_ascii_case("m"))
        {
            self.open_map(event_loop);
            return;
        }
        if let ViewContent::Stream {
            hud: Some(hud),
            shift,
            inventory_reader,
            world,
            ..
        } = &mut self.content
        {
            let view = self.view.as_ref().unwrap();
            let size = view.window.inner_size();
            if let Some(reader) = inventory_reader {
                hud.refresh_inventory(reader, world);
            }
            hud.layout((size.width, size.height));
            if let WindowEvent::KeyboardInput {
                event: key,
                is_synthetic: false,
                ..
            } = &event
                && key.state.is_pressed()
                && !key.repeat
            {
                if key.logical_key == Key::Named(NamedKey::Tab) {
                    hud.advance_focus(*shift);
                    view.window.request_redraw();
                    return;
                }
                if matches!(
                    key.logical_key,
                    Key::Named(NamedKey::Enter | NamedKey::Space)
                ) && let Some(action) = hud.focused_action()
                {
                    self.hud_action(action, event_loop);
                    return;
                }
                if matches!(&key.logical_key, Key::Character(ch) if ch.eq_ignore_ascii_case("i"))
                    && hud.has_inventory()
                {
                    self.hud_action(hud::Action::ToggleInventory, event_loop);
                    return;
                }
                let step = match &key.logical_key {
                    Key::Character(ch) if ch == "[" => -25,
                    Key::Character(ch) if ch == "]" => 25,
                    _ => 0,
                };
                if step != 0 {
                    let previous = hud.scale();
                    hud.adjust_scale(step);
                    if hud.scale() != previous
                        && let Some(preferences) = &mut self.preferences
                    {
                        preferences.set("ui", "scale", i64::from(hud.scale()));
                    }
                    hud.hover(self.cursor);
                    view.window.request_redraw();
                    return;
                }
            }
            if hud.hover(self.cursor) {
                view.window.request_redraw();
            }
            let hit = self.cursor.map_or(hud::Hit::World, |point| hud.hit(point));
            if matches!(
                event,
                WindowEvent::Focused(false) | WindowEvent::CursorLeft { .. }
            ) {
                let cancelled = hud.cancel_inventory_drag();
                if cancelled | hud.cancel_press() {
                    view.window.request_redraw();
                }
            }
            if let WindowEvent::MouseInput { button, state, .. } = event
                && let Some(reader) = inventory_reader
                && hud.inventory_open()
            {
                if hud.dragging_inventory() {
                    if !state.is_pressed() {
                        match button {
                            MouseButton::Left => {
                                hud.finish_inventory_drag(reader, world, self.cursor)
                            }
                            MouseButton::Right => {
                                hud.cancel_inventory_drag();
                            }
                            _ => {}
                        }
                        view.window.request_redraw();
                    }
                    return;
                }
                if let Some(point) = self.cursor {
                    match (button, state) {
                        (MouseButton::Left, ElementState::Pressed) => {
                            if let Some(id) = hud.begin_inventory_drag(point) {
                                if let Some(audio) = &self.audio {
                                    audio.effect(id, false);
                                }
                                view.window.request_redraw();
                                return;
                            }
                        }
                        (MouseButton::Right, ElementState::Released) => {
                            hud.quick_inventory_transfer(reader, world, point);
                            view.window.request_redraw();
                            return;
                        }
                        _ => {}
                    }
                }
            }
            if matches!(
                event,
                WindowEvent::MouseInput {
                    state: ElementState::Released,
                    button: MouseButton::Left,
                    ..
                }
            ) && hud.is_pressed()
            {
                let action = self.cursor.and_then(|point| hud.release(point));
                hud.cancel_press();
                if let Some(action) = action {
                    self.hud_action(action, event_loop);
                } else {
                    view.window.request_redraw();
                }
                return;
            }
            if hit != hud::Hit::World {
                match event {
                    WindowEvent::MouseInput {
                        state: ElementState::Pressed,
                        button: MouseButton::Left,
                        ..
                    } => {
                        if let hud::Hit::Button(action) = hit {
                            if matches!(
                                action,
                                hud::Action::ToggleInventory
                                    | hud::Action::InventoryScrollUp
                                    | hud::Action::InventoryScrollDown
                                    | hud::Action::InventoryArrange
                                    | hud::Action::InventoryTakeAll
                            ) {
                                hud.press(self.cursor.unwrap());
                                view.window.request_redraw();
                            } else {
                                self.hud_action(action, event_loop);
                            }
                        }
                        return;
                    }
                    WindowEvent::MouseWheel { delta, .. } => {
                        let vertical = match delta {
                            MouseScrollDelta::LineDelta(_, y) => {
                                y.signum() as i32 * y.abs().ceil() as i32
                            }
                            MouseScrollDelta::PixelDelta(p) => {
                                i32::from(p.y > 0.) - i32::from(p.y < 0.)
                            }
                        };
                        if vertical != 0 && hud.scroll_inventory(self.cursor, -vertical) {
                            hud.hover(self.cursor);
                            view.window.request_redraw();
                        }
                        return;
                    }
                    _ => {}
                }
            }
            if hud.inventory_open()
                && (matches!(event, WindowEvent::MouseInput { .. })
                    || (hud.dragging_inventory()
                        && matches!(event, WindowEvent::MouseWheel { .. })))
            {
                return;
            }
        }
        self.render_event(event_loop, id, event, elapsed);
    }
}

impl Viewer<'_> {
    fn dialogue_event(&mut self, event: &WindowEvent) -> bool {
        let ViewContent::Stream {
            world,
            hud: Some(hud),
            inventory_reader: Some(reader),
            ..
        } = &mut self.content
        else {
            return false;
        };
        if world.conversation().is_none() {
            return false;
        }
        let view = self.view.as_ref().unwrap();
        let mut sync_voice = false;
        let handled = match event {
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = (position.x >= 0. && position.y >= 0.)
                    .then_some((position.x as u32, position.y as u32));
                hud.hover(self.cursor);
                true
            }
            WindowEvent::CursorLeft { .. } => {
                self.cursor = None;
                hud.hover(None);
                true
            }
            WindowEvent::KeyboardInput {
                event: key,
                is_synthetic: false,
                ..
            } if key.state.is_pressed() && !key.repeat => {
                match &key.logical_key {
                    Key::Named(NamedKey::Escape) => {
                        world.close_dialogue("cancelled");
                        sync_voice = true;
                    }
                    Key::Character(ch) if ch == "[" || ch == "]" => {
                        let previous = hud.scale();
                        hud.clear_dialogue_choices();
                        hud.adjust_scale(if ch == "[" { -25 } else { 25 });
                        if hud.scale() != previous
                            && let Some(preferences) = &mut self.preferences
                        {
                            preferences.set("ui", "scale", i64::from(hud.scale()));
                        }
                    }
                    Key::Named(NamedKey::PageUp | NamedKey::PageDown) => {
                        hud.scroll_dialogue(
                            self.cursor,
                            key.logical_key == Key::Named(NamedKey::PageUp),
                            true,
                        );
                    }
                    _ => {}
                }
                true
            }
            WindowEvent::MouseInput {
                button: MouseButton::Left,
                state,
                ..
            } if state.is_pressed() => {
                if let Some(index) = self.cursor.and_then(|point| hud.dialogue_choice(point)) {
                    sync_voice = reader.choose_dialogue_response(world, index);
                    if sync_voice {
                        hud.clear_dialogue_choices();
                    }
                }
                true
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let y = match delta {
                    MouseScrollDelta::LineDelta(_, y) => f64::from(*y),
                    MouseScrollDelta::PixelDelta(point) => point.y,
                };
                if y != 0. {
                    hud.scroll_dialogue(self.cursor, y > 0., false);
                }
                true
            }
            WindowEvent::KeyboardInput { .. } | WindowEvent::MouseInput { .. } => true,
            WindowEvent::Resized { .. } | WindowEvent::ScaleFactorChanged { .. } => {
                hud.clear_dialogue_choices();
                false
            }
            _ => false,
        };
        if sync_voice && let Some(audio) = &self.audio {
            audio.voice(world.conversation().and_then(|s| s.turn.voice.as_deref()));
        }
        if handled {
            view.window.request_redraw();
        }
        handled
    }

    fn render_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        id: WindowId,
        event: WindowEvent,
        play_elapsed: Duration,
    ) {
        let Some(view) = self.view.as_mut().filter(|view| view.window.id() == id) else {
            return;
        };
        match event {
            WindowEvent::CursorMoved { .. } | WindowEvent::CursorLeft { .. } => {
                if let ViewContent::Stream {
                    world,
                    last_presented: Some(camera),
                    last_viewport,
                    last_zoom,
                    hud,
                    ..
                } = &mut self.content
                    && world.playing()
                    && self.pause.is_none()
                    && *last_viewport
                        == (
                            view.window.inner_size().width,
                            view.window.inner_size().height,
                        )
                    && world.update_hover(
                        self.cursor
                            .filter(|&point| {
                                hud.as_ref()
                                    .is_none_or(|hud| hud.hit(point) == hud::Hit::World)
                            })
                            .map(|(x, y)| (x / u32::from(*last_zoom), y / u32::from(*last_zoom))),
                        logical_size(view.window.inner_size(), *last_zoom),
                        camera,
                        play_elapsed,
                    )
                {
                    view.window.request_redraw();
                }
            }
            WindowEvent::CloseRequested | WindowEvent::Destroyed => event_loop.exit(),
            WindowEvent::Resized(size) if size.width != 0 && size.height != 0 => {
                view.window.request_redraw();
            }
            WindowEvent::ScaleFactorChanged { .. } => view.window.request_redraw(),
            WindowEvent::Occluded(occluded) => {
                view.occluded = occluded;
                if occluded {
                    self.navigation = Navigation::default();
                    self.player_deadline = None;
                    self.schedule_animation(event_loop, None);
                } else {
                    if self.pause.is_none()
                        && matches!(&self.content, ViewContent::Stream { world, .. } if world.player_moving())
                    {
                        self.player_deadline = Some(Instant::now());
                    }
                    view.window.request_redraw();
                }
            }
            WindowEvent::MouseWheel { delta, .. } if matches!(&self.content, ViewContent::Stream { world, .. } if world.playing()) =>
            {
                let y = match delta {
                    MouseScrollDelta::LineDelta(_, y) => f64::from(y),
                    MouseScrollDelta::PixelDelta(position) => position.y,
                };
                let previous = self.settings;
                self.settings.zoom = (i32::from(self.settings.zoom) + (y > 0.) as i32
                    - (y < 0.) as i32)
                    .clamp(1, 3) as u8;
                view.window.request_redraw();
                self.record_settings(previous);
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                if let (
                    Some(cursor),
                    ViewContent::Stream {
                        world,
                        shift,
                        last_presented: Some(camera),
                        last_viewport,
                        last_zoom,
                        ..
                    },
                ) = (self.cursor, &mut self.content)
                    && world.playing()
                {
                    let size = view.window.inner_size();
                    if *last_viewport == (size.width, size.height) {
                        let scale = u32::from(*last_zoom);
                        world.set_player_destination(
                            (cursor.0 / scale, cursor.1 / scale),
                            logical_size(size, *last_zoom),
                            camera,
                            *shift ^ self.settings.run,
                            play_elapsed,
                        );
                        self.player_deadline = Some(Instant::now());
                        self.update_control_flow(event_loop);
                    }
                }
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                if let ViewContent::World { shift, .. } | ViewContent::Stream { shift, .. } =
                    &mut self.content
                {
                    *shift = modifiers.state().shift_key();
                }
            }
            WindowEvent::Focused(false) => {
                self.navigation = Navigation::default();
                self.update_control_flow(event_loop);
                if let ViewContent::World { shift, .. } | ViewContent::Stream { shift, .. } =
                    &mut self.content
                {
                    *shift = false;
                }
            }
            WindowEvent::KeyboardInput {
                ref event,
                is_synthetic: false,
                ..
            } if matches!(&self.content, ViewContent::Stream { explore: true, world, .. } if !world.playing())
                && Navigation::key(&event.logical_key).is_some() =>
            {
                if !event.repeat {
                    let now = Instant::now();
                    self.navigation.set_key(
                        Navigation::key(&event.logical_key).unwrap(),
                        event.state.is_pressed(),
                        now,
                    );
                    self.update_control_flow(event_loop);
                }
            }
            WindowEvent::KeyboardInput {
                event,
                is_synthetic: false,
                ..
            } if event.state.is_pressed() => {
                if let Key::Character(key) = &event.logical_key
                    && let ViewContent::Stream {
                        world,
                        camera,
                        trace,
                        animation_ms,
                        explore,
                        ..
                    } = &mut self.content
                {
                    let handled = match key.as_str() {
                        "w" | "s" if world.playing() => false,
                        "w" if animation_ms.is_none() && !event.repeat => {
                            let size = view.window.inner_size();
                            world.start_walk(
                                camera,
                                (size.width, size.height),
                                self.epoch.elapsed(),
                            )
                        }
                        "r" if animation_ms.is_none() && !event.repeat => world.reset_walk(),
                        "f" if animation_ms.is_none() && !event.repeat => {
                            let sampled = Instant::now();
                            let elapsed = sampled.duration_since(self.epoch);
                            if world.start_fidget(elapsed) {
                                if *trace {
                                    eprintln!("Fidget started at {}ms", elapsed.as_millis());
                                }
                                // Frame zero is already the standing pose. Schedule the
                                // first visible transition without submitting it twice.
                                let size = view.window.inner_size();
                                let deadline = world
                                    .animation_delay(camera, (size.width, size.height), elapsed)
                                    .map(|delay| sampled + delay);
                                self.schedule_animation(event_loop, deadline);
                            }
                            return;
                        }
                        "," | "." => {
                            let step = if key.as_str() == "," { -1 } else { 1 };
                            let turned = world.turn_fidget(step);
                            if let Some(direction) = turned
                                && *trace
                            {
                                eprintln!("Fidget subject direction {direction}");
                            }
                            turned.is_some()
                        }
                        "x" if *explore && !world.playing() => {
                            Self::update_light_probe(world, "s", *trace)
                        }
                        _ => Self::update_light_probe(world, key.as_str(), *trace),
                    };
                    if handled {
                        view.window.request_redraw();
                        return;
                    }
                }
                if matches!(&self.content, ViewContent::Stream { world, .. } if world.playing()) {
                    if matches!(&event.logical_key, Key::Character(key) if key == "0") {
                        let previous = self.settings;
                        self.settings.zoom = 1;
                        view.window.request_redraw();
                        self.record_settings(previous);
                    } else if !self.settings.follow
                        && event.logical_key == Key::Named(NamedKey::Home)
                        && let ViewContent::Stream { world, camera, .. } = &mut self.content
                    {
                        world.follow_player(play_elapsed, camera);
                        view.window.request_redraw();
                    }
                    return;
                }
                let size = view.window.inner_size();
                let viewport = (size.width, size.height);
                let (camera, shift, navigation) = match &mut self.content {
                    ViewContent::World { camera, shift, .. } => (camera, shift, None),
                    ViewContent::Stream {
                        camera,
                        shift,
                        direction,
                        last_presented,
                        waiting,
                        ..
                    } => (camera, shift, Some((direction, last_presented, *waiting))),
                    ViewContent::Fixed(_) => return,
                };
                let step = if *shift { 64 } else { 16 };
                let delta = match event.logical_key {
                    Key::Named(NamedKey::Escape) => {
                        event_loop.exit();
                        return;
                    }
                    Key::Named(NamedKey::Home) => (0, 0),
                    Key::Named(NamedKey::ArrowLeft) => (-step, 0),
                    Key::Named(NamedKey::ArrowRight) => (step, 0),
                    Key::Named(NamedKey::ArrowUp) => (0, -step),
                    Key::Named(NamedKey::ArrowDown) => (0, step),
                    _ => return,
                };
                let previous = camera.position();
                if let Some((direction, last, waiting)) = navigation {
                    *direction = delta;
                    if waiting && let Some(last) = last {
                        *camera = last.clone();
                    }
                }
                if delta == (0, 0) {
                    camera.reset(viewport);
                } else {
                    camera.pan(delta, viewport);
                }
                if camera.position() != previous {
                    view.window.request_redraw();
                }
            }

            WindowEvent::RedrawRequested if !view.occluded => {
                let size = view.window.inner_size();
                let clock_hours = self.clock.as_ref().map(|clock| {
                    let now = self
                        .pause
                        .as_ref()
                        .map_or_else(Instant::now, |(_, paused)| *paused);
                    clock.hours(
                        now.saturating_duration_since(self.epoch)
                            .saturating_sub(self.clock_paused_for),
                    )
                });
                let mut deadline = None;
                let result = match &mut self.content {
                    ViewContent::Fixed(layers) => {
                        let center = ((size.width / 2) as i32, (size.height / 2) as i32);
                        view.redraw(
                            size,
                            layers.iter().map(|(asset, anchor)| {
                                unlit(
                                    asset,
                                    (
                                        center.0 + i32::from(anchor.0),
                                        center.1 + i32::from(anchor.1),
                                    ),
                                )
                            }),
                        )
                    }
                    ViewContent::World {
                        scene,
                        assets,
                        camera,
                        ..
                    } => {
                        let viewport = (size.width, size.height);
                        if size.width != 0 && size.height != 0 {
                            camera.resize(viewport);
                        }
                        view.redraw(
                            size,
                            scene.visible(camera, viewport).map(|draw| {
                                unlit(&assets[&std::ptr::from_ref(draw.frame)], draw.anchor)
                            }),
                        )
                    }
                    ViewContent::Stream {
                        world,
                        camera,
                        direction,
                        last_presented,
                        last_viewport,
                        last_zoom,
                        waiting,
                        trace,
                        animation_ms,
                        hud,
                        inventory_reader,
                        map_ui,
                        travel,
                        ..
                    } => {
                        let sampled = Instant::now();
                        let elapsed = animation_ms.map_or(play_elapsed, Duration::from_millis);
                        if let (Some(hud), Some(hours)) = (hud.as_mut(), clock_hours) {
                            hud.set_game_time(hours);
                        }
                        let scale = hud.as_ref().map_or(self.pause_scale, |hud| hud.scale());
                        let menu = self.pause.as_ref().zip(self.pause_theme.as_mut()).map(
                            |((menu, _), theme)| {
                                theme.render(menu, [size.width, size.height], scale, self.settings)
                            },
                        );
                        if let (Some(map_ui), Some(travel), Some((tile, town))) = (
                            map_ui.as_mut(),
                            travel.as_ref(),
                            world.player_map_position(),
                        ) {
                            map_ui.observe(&travel.map, tile, town);
                            if let Some(hud) = hud {
                                hud.set_travel_available(map_ui.can_travel());
                            }
                        }
                        if let (Some(hud), Some(reader)) = (hud.as_mut(), inventory_reader.as_mut())
                        {
                            hud.refresh_inventory(reader, world);
                        }
                        view.redraw_stream(
                            size,
                            world,
                            camera,
                            *direction,
                            *trace,
                            FrameOptions {
                                elapsed,
                                settings: self.settings,
                                menu: menu.as_ref(),
                                cursor: self.cursor,
                                hud: hud.as_deref_mut(),
                                inventory_reader: inventory_reader.as_deref(),
                                developer: self.developer.as_mut(),
                            },
                        )
                        .map(|ready| {
                            *waiting = !ready;
                            if ready {
                                *last_presented = Some(camera.clone());
                                *last_viewport = (size.width, size.height);
                                *last_zoom = self.settings.zoom;
                                if let (Some(map_ui), Some(travel), Some((tile, town))) = (
                                    map_ui.as_mut(),
                                    travel.as_ref(),
                                    world.player_map_position(),
                                ) {
                                    map_ui.observe(&travel.map, tile, town);
                                    if let Some(hud) = hud
                                        && hud.set_travel_available(map_ui.can_travel())
                                    {
                                        view.window.request_redraw();
                                    }
                                }
                                if animation_ms.is_none() && self.pause.is_none() {
                                    deadline = world
                                        .animation_delay(
                                            camera,
                                            logical_size(size, self.settings.zoom),
                                            elapsed,
                                        )
                                        .map(|delay| sampled + delay);
                                }
                            }
                        })
                    }
                };
                self.schedule_animation(event_loop, deadline);
                if let Err(error) = result {
                    self.fail(event_loop, error);
                }
            }
            _ => {}
        }
    }
}

fn unlit(asset: &Arc<LoadedFrame>, anchor: (i32, i32)) -> WorldDraw<'_> {
    WorldDraw {
        asset,
        sprite: SpriteDraw {
            frame: &asset.decoded.frame,
            anchor,
        },
        modulation: Modulation::Unlit,
    }
}

fn scene_draws<'a>(
    world: &'a InteractiveWorld,
    camera: &Camera,
    viewport: (u32, u32),
    elapsed: Duration,
    settings: Settings,
) -> impl Iterator<Item = arcanum_rs::content::world::WorldDraw<'a>> {
    world
        .visible_assets_at(camera, viewport, elapsed)
        .filter_map(move |mut draw| {
            match draw.modulation {
                Modulation::Shadow(_) if !settings.shadows => return None,
                Modulation::Shadow(_) | Modulation::Additive | Modulation::AdditiveTint(_) => {}
                Modulation::Fade { corners, .. } if !settings.lighting => {
                    draw.modulation = Modulation::Fade {
                        corners,
                        color: [255; 3],
                        columns: &[],
                    };
                }
                _ if !settings.lighting => draw.modulation = Modulation::Unlit,
                _ => {}
            }
            Some(draw)
        })
}

fn logical_size(size: PhysicalSize<u32>, zoom: u8) -> (u32, u32) {
    let zoom = u32::from(zoom);
    (size.width.div_ceil(zoom), size.height.div_ceil(zoom))
}

fn inventory_inset(width: u32, height: u32) -> Arc<LoadedFrame> {
    hud::generated_frame(
        "inventory-world-inset",
        SpriteFrame {
            width,
            height,
            hotspot: (0, 0),
            offset: (0, 0),
            pixels: Vec::new(),
        },
    )
}

struct FrameOptions<'a> {
    elapsed: Duration,
    settings: Settings,
    menu: Option<&'a classic_menu::Frame>,
    hud: Option<&'a mut Hud>,
    inventory_reader: Option<&'a InventoryReader>,
    developer: Option<&'a mut Developer>,
    cursor: Option<(u32, u32)>,
}

impl WindowSurface {
    fn redraw_stream(
        &mut self,
        size: PhysicalSize<u32>,
        world: &mut InteractiveWorld,
        camera: &mut Camera,
        direction: (i32, i32),
        trace: bool,
        options: FrameOptions<'_>,
    ) -> Result<bool, String> {
        let FrameOptions {
            elapsed,
            settings,
            menu,
            hud,
            inventory_reader,
            mut developer,
            cursor,
        } = options;
        let zoom = settings.zoom;
        let background = stream_background(world.playing());
        let viewport = logical_size(size, zoom);
        if size.width == 0 || size.height == 0 {
            return Ok(false);
        }
        if settings.follow {
            world.follow_player(elapsed, camera);
        }
        camera.resize(viewport);
        let prepared = world.prepare(camera, viewport, direction)?;
        if !prepared.ready {
            // Keep the last complete buffer visible while the worker prepares
            // the destination. The completion event drives the next redraw.
            return Ok(false);
        }
        world.start_player()?;
        let frame_start = Instant::now();
        let mut hud = hud.filter(|_| menu.is_none());
        if let Some(hud) = hud.as_deref_mut() {
            let npc_at = world.dialogue_anchor().map(|(x, y)| {
                let (cx, cy) = camera.position();
                let zoom = i32::from(zoom);
                (
                    (x - cx) * zoom + size.width as i32 / 2,
                    (y - cy) * zoom + size.height as i32 / 2,
                )
            });
            hud.set_dialogue(world.conversation(), npc_at, (size.width, size.height));
            hud.hover(cursor);
        }
        let cursor = cursor.filter(|&point| {
            hud.as_ref()
                .is_none_or(|hud| hud.hit(point) == hud::Hit::World)
        });
        if world.playing() {
            let scale = u32::from(zoom);
            world.update_hover(
                cursor
                    .filter(|_| menu.is_none() && world.conversation().is_none())
                    .map(|(x, y)| (x / scale, y / scale)),
                viewport,
                camera,
                elapsed,
            );
        }
        if world.conversation().is_none()
            && let Some(hud) = hud.as_deref_mut()
        {
            let context = inventory_reader.and_then(|reader| reader.npc_context(world));
            hud.set_label(hud::Binding::NpcContext, context.as_deref().unwrap_or(""));
        }
        let start = trace.then(Instant::now);
        let pose = world.update_walk(elapsed);
        if let (Some(start), Some(pose)) = (start, pose) {
            eprintln!(
                "Walk at {}ms {pose:?}, update {:?}",
                elapsed.as_millis(),
                start.elapsed()
            );
        }
        let options_inset = if let Some((w, h)) = menu.and_then(classic_menu::Frame::lens_size) {
            let inset = self
                .inventory_inset
                .get_or_insert_with(|| inventory_inset(w, h));
            if (inset.decoded.frame.width, inset.decoded.frame.height) != (w, h) {
                *inset = inventory_inset(w, h);
            }
            let mut focused = camera.clone();
            world.follow_player(elapsed, &mut focused);
            self.renderer.render_inset(
                inset,
                0.5,
                background,
                scene_draws(world, &focused, (w * 2, h * 2), elapsed, settings),
            )?;
            Some(Arc::clone(inset))
        } else {
            None
        };
        if let Some(hud) = hud.as_deref_mut()
            && let Some([w, h]) = hud.native_size(hud::Binding::InventoryLens)
        {
            let inset = self
                .inventory_inset
                .get_or_insert_with(|| inventory_inset(w, h));
            if (inset.decoded.frame.width, inset.decoded.frame.height) != (w, h) {
                *inset = inventory_inset(w, h);
            }
            let source = (w * 2, h * 2);
            self.renderer.render_inset(
                inset,
                0.5,
                background,
                scene_draws(world, camera, source, elapsed, settings),
            )?;
            hud.set_lens_asset(hud::Binding::InventoryLens, Arc::clone(inset));
        }
        let hud = hud.as_deref();
        let scale = hud.map(|hud| hud.scale());
        let (console, status) = developer.as_deref_mut().map_or((None, None), |dev| {
            let scale = scale.unwrap_or(dev.ui_scale());
            (
                dev.console_frame((size.width, size.height), scale),
                dev.status_frame(
                    self.last_frame_time,
                    self.last_frame_interval,
                    self.submitted_frames,
                    world,
                    scale,
                    size.width.saturating_sub(24),
                ),
            )
        });
        let developer = developer.as_deref();
        let draws = scene_draws(world, camera, viewport, elapsed, settings).chain(
            developer
                .into_iter()
                .flat_map(|dev| dev.world_marks(world, camera, viewport)),
        );
        if trace {
            eprintln!(
                "GPU world camera {:?} animation {}ms schedule {:?} order {:?} shading {:?}",
                camera.position(),
                elapsed.as_millis(),
                prepared.scheduling,
                prepared.ordering,
                prepared.shading,
            );
        }
        self.renderer.redraw(
            ((size.width, size.height), zoom),
            background,
            draws,
            hud.into_iter()
                .flat_map(|hud| hud.draws(settings.run))
                .chain(menu.into_iter().flat_map(classic_menu::Frame::draws))
                .chain(
                    menu.into_iter()
                        .flat_map(|frame| frame.lens_draws(options_inset.as_ref())),
                )
                .chain(
                    self.save_status
                        .iter()
                        .filter(|(_, until)| menu.is_none() && Instant::now() < *until)
                        .map(|(asset, _)| OverlayDraw::at(asset, (12, 52))),
                )
                .chain(
                    self.developer_status
                        .iter()
                        .map(|asset| OverlayDraw::at(asset, (12, 52))),
                )
                .chain(status.iter().map(|asset| OverlayDraw::at(asset, (12, 85))))
                .chain(
                    console
                        .iter()
                        .map(|(asset, anchor)| OverlayDraw::at(asset, *anchor)),
                ),
            &self.window,
            trace,
        )?;
        self.last_frame_time = frame_start.elapsed();
        let now = Instant::now();
        self.last_frame_interval = self.frame_presented_at.replace(now).map(|prev| now - prev);
        self.submitted_frames += 1;
        Ok(true)
    }

    fn redraw<'a>(
        &mut self,
        size: PhysicalSize<u32>,
        draws: impl IntoIterator<Item = WorldDraw<'a>>,
    ) -> Result<(), String> {
        if size.width == 0 || size.height == 0 {
            return Ok(());
        }
        self.renderer.redraw(
            ((size.width, size.height), 1),
            BACKGROUND,
            draws,
            std::iter::empty(),
            &self.window,
            false,
        )
    }

    fn redraw_map<'a>(
        &mut self,
        size: PhysicalSize<u32>,
        zoom: u8,
        scene: impl IntoIterator<Item = arcanum_rs::content::world::WorldDraw<'a>>,
        map: &'a Hud,
        hud: Option<&'a Hud>,
    ) -> Result<(), String> {
        let layers = hud
            .into_iter()
            .flat_map(|hud| hud.draws(false))
            .chain(map.draws(false));
        self.renderer.redraw(
            ((size.width, size.height), zoom),
            stream_background(true),
            scene,
            layers,
            &self.window,
            false,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playable_viewport_clears_to_black_and_diagnostics_keep_their_clear_color() {
        assert_eq!(stream_background(true), [0, 0, 0]);
        assert_eq!(stream_background(false), [32, 43, 58]);
    }

    #[test]
    fn zoom_maps_surface_pixels_to_original_tile_under_the_same_camera() {
        let size = PhysicalSize::new(1919, 1079);
        let reference = (104, 92);
        for zoom in 1..=3 {
            let z = u32::from(zoom);
            let logical = logical_size(size, zoom);
            let point = ((logical.0 / 2 + 40) * z, (logical.1 / 2 - 20) * z);
            let offset = (
                i64::from(point.0 / z - logical.0 / 2),
                i64::from(point.1 / z) - i64::from(logical.1 / 2),
            );
            assert_eq!(
                arcanum_rs::isometric::world_tile_at_pixel(offset, reference),
                Some((103, 92))
            );
        }
    }

    #[test]
    fn held_camera_pan_integrates_elapsed_time_and_waits_without_catching_up() {
        let now = Instant::now();
        let mut motion = Navigation::default();
        assert!(motion.deadline().is_none());
        motion.set_key(1 << 5, true, now); // D
        motion.set_key(1 << 1, true, now); // Right also held
        motion.set_key(1 << 5, false, now);
        assert_eq!(motion.direction(), (1, 0));
        let divide = |ms| Duration::from_millis(ms);
        let a = motion.advance(now + divide(20), false, false);
        let b = motion.advance(now + divide(40), false, false);
        assert_eq!((a.0 + b.0, a.1 + b.1), (12, 0));
        assert_eq!(motion.advance(now + divide(80), true, true), (0, 0));
        assert_eq!(motion.advance(now + divide(100), true, false), (20, 0));
        motion.set_key(1 << 1, false, now + divide(100));
        assert!(motion.deadline().is_none());
    }
}
