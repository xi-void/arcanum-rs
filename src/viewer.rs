//! Optional desktop presentation for the sprite diagnostic.

use crate::RenderBackend;
use arcanum_rs::art::SpriteFrame;

#[cfg(feature = "gpu")]
mod gpu;
mod menu;
use arcanum_rs::camera::{Camera, CameraScene};
use arcanum_rs::compositor::{Modulation, SpriteDraw, blit_modulated};
use arcanum_rs::content::world::{Destination, DoorStates, InteractiveWorld, WorldStream};
use menu::{Command as MenuCommand, Menu, Settings};
use softbuffer::{Context, Surface};
use std::{
    num::NonZeroU32,
    sync::Arc,
    time::{Duration, Instant},
};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalSize, Size};
use winit::event::{ElementState, MouseButton, MouseScrollDelta, StartCause, WindowEvent};
use winit::event_loop::{
    ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy, OwnedDisplayHandle,
};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

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
    show_content(title, ViewContent::Fixed(layers), size)
}

pub fn show_camera(
    title: String,
    scene: &CameraScene<'_>,
    camera: Camera,
    size: (u32, u32),
) -> Result<(), String> {
    show_content(
        title,
        ViewContent::World {
            scene,
            camera,
            shift: false,
        },
        PhysicalSize::<u32>::from(size).into(),
    )
}

pub type TravelLoader<'a> = Box<
    dyn Fn(&str, Destination, u8, (u32, u32)) -> Result<(String, WorldStream, Camera), String> + 'a,
>;

struct Travel<'a> {
    map: String,
    loader: TravelLoader<'a>,
    wake: EventLoopProxy<()>,
    doors: std::collections::BTreeMap<String, DoorStates>,
}

pub struct StreamOptions<'a> {
    pub trace: bool,
    pub renderer: RenderBackend,
    pub animation_ms: Option<u64>,
    pub explore: bool,
    pub travel: Option<(String, TravelLoader<'a>)>,
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
        renderer,
        animation_ms,
        explore,
        travel,
    } = options;
    #[cfg(not(feature = "gpu"))]
    if renderer != RenderBackend::Cpu {
        return Err("GPU renderer requires the gpu feature".into());
    }
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
            trace,
            animation_ms,
            travel,
        },
        PhysicalSize::<u32>::from(size).into(),
        event_loop,
        renderer,
    )
}

enum ViewContent<'a> {
    Fixed(&'a [Layer<'a>]),
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
        trace: bool,
        animation_ms: Option<u64>,
        travel: Option<Travel<'a>>,
    },
    World {
        scene: &'a CameraScene<'a>,
        camera: Camera,
        shift: bool,
    },
}

fn show_content(title: String, content: ViewContent<'_>, size: Size) -> Result<(), String> {
    run_content(title, content, size, make_loop()?, RenderBackend::Cpu)
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
    renderer: RenderBackend,
) -> Result<(), String> {
    event_loop.set_control_flow(ControlFlow::Wait);
    let context = (renderer == RenderBackend::Cpu)
        .then(|| Context::new(event_loop.owned_display_handle()))
        .transpose()
        .map_err(|error| format!("cannot initialize software presentation: {error}"))?;
    let mut app = Viewer {
        title,
        size,
        context,
        renderer,
        view: None,
        content,
        failure: None,
        epoch: Instant::now(),
        animation_deadline: None,
        navigation: Navigation::default(),
        player_deadline: None,
        cursor: None,
        settings: Settings::default(),
        pause: None,
        paused_for: Duration::ZERO,
    };
    event_loop
        .run_app(&mut app)
        .map_err(|error| format!("window event loop failed: {error}"))?;
    if let ViewContent::Stream { world, .. } = &app.content {
        eprintln!("World cache {:?}", world.stats());
    }
    app.failure.map_or(Ok(()), Err)
}

struct Viewer<'a> {
    title: String,
    size: Size,
    context: Option<Context<OwnedDisplayHandle>>,
    renderer: RenderBackend,
    view: Option<WindowSurface>,
    content: ViewContent<'a>,
    failure: Option<String>,
    epoch: Instant,
    animation_deadline: Option<Instant>,
    navigation: Navigation,
    player_deadline: Option<Instant>,
    cursor: Option<(u32, u32)>,
    settings: Settings,
    pause: Option<(Menu, Instant)>,
    paused_for: Duration,
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
    backend: Presentation,
    window: Arc<Window>,
    occluded: bool,
}

enum Presentation {
    Cpu(CpuSurface),
    #[cfg(feature = "gpu")]
    Gpu(Box<gpu::WindowRenderer>),
}

struct CpuSurface {
    surface: Surface<OwnedDisplayHandle, Arc<Window>>,
    pixels: Vec<[u8; 3]>,
}

impl Viewer<'_> {
    fn schedule_animation(&mut self, event_loop: &ActiveEventLoop, deadline: Option<Instant>) {
        self.animation_deadline = deadline;
        self.update_control_flow(event_loop);
    }

    fn play_elapsed(&self, now: Instant) -> Duration {
        self.pause
            .as_ref()
            .map_or(now, |(_, paused)| *paused)
            .saturating_duration_since(self.epoch)
            .saturating_sub(self.paused_for)
    }

    fn menu_action(&mut self, command: MenuCommand, event_loop: &ActiveEventLoop) {
        match command {
            MenuCommand::Continue => {
                if let Some((_, paused)) = self.pause.take() {
                    self.paused_for += paused.elapsed();
                }
                if matches!(&self.content, ViewContent::Stream { world, .. } if world.player_moving())
                {
                    self.player_deadline = Some(Instant::now());
                }
            }
            MenuCommand::Quit => event_loop.exit(),
            MenuCommand::Stay => {}
        }
        if let Some(view) = &self.view {
            view.window.request_redraw();
        }
        self.update_control_flow(event_loop);
    }

    fn update_control_flow(&self, event_loop: &ActiveEventLoop) {
        let next = self
            .animation_deadline
            .into_iter()
            .chain(self.navigation.deadline())
            .chain(self.player_deadline)
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
            .with_inner_size(self.size);
        let window = Arc::new(
            event_loop
                .create_window(attributes)
                .map_err(|error| format!("cannot create sprite window: {error}"))?,
        );
        let backend = match self.renderer {
            RenderBackend::Cpu => {
                let surface = Surface::new(
                    self.context.as_ref().expect("CPU context"),
                    Arc::clone(&window),
                )
                .map_err(|error| format!("cannot create sprite surface: {error}"))?;
                Presentation::Cpu(CpuSurface {
                    surface,
                    pixels: Vec::new(),
                })
            }
            #[cfg(feature = "gpu")]
            backend => {
                let trace = matches!(&self.content, ViewContent::Stream { trace: true, .. });
                Presentation::Gpu(Box::new(gpu::WindowRenderer::new(
                    Arc::clone(&window),
                    event_loop.owned_display_handle(),
                    backend,
                    trace,
                )?))
            }
            #[cfg(not(feature = "gpu"))]
            _ => return Err("GPU renderer requires the gpu feature".into()),
        };
        Ok(WindowSurface {
            backend,
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
        if self.pause.is_some() {
            return;
        }
        let elapsed = self.play_elapsed(now);
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
                trace,
                last_presented,
                travel,
                ..
            } = &mut self.content
            {
                if world.playing() {
                    if self.player_deadline.is_some_and(|deadline| now >= deadline) {
                        let input = self.navigation.direction();
                        if !*waiting && !view.occluded {
                            let before = camera.position();
                            redraw |= world.move_player(
                                input,
                                *shift ^ self.settings.run,
                                elapsed,
                                camera,
                            );
                            let after = camera.position();
                            if before != after {
                                *direction = (after.0 - before.0, after.1 - before.1);
                            }
                        }
                        if let Some((target, facing)) = world.take_transition() {
                            world.cancel_player_route();
                            if let Some(travel) = travel {
                                let size = view.window.inner_size();
                                let loaded = (travel.loader)(
                                    &travel.map,
                                    target,
                                    facing,
                                    (size.width, size.height),
                                )
                                .and_then(
                                    |(map, next, next_camera)| {
                                        let wake = travel.wake.clone();
                                        let next = next
                                            .into_interactive(
                                                move || {
                                                    let _ = wake.send_event(());
                                                },
                                                *trace,
                                            )
                                            .map_err(|error| error.to_string())?;
                                        Ok((map, next, next_camera))
                                    },
                                );
                                match loaded {
                                    Ok((map, mut next, next_camera)) => {
                                        eprintln!("Entered {map} at {:?}", target.tile);
                                        view.window.set_title(&format!("Arcanum-RS | {map} play"));
                                        travel
                                            .doors
                                            .insert(travel.map.clone(), world.take_door_states());
                                        if let Some(states) = travel.doors.remove(&map) {
                                            next.restore_door_states(states);
                                        }
                                        travel.map = map;
                                        **world = next;
                                        *camera = next_camera;
                                        *last_presented = None;
                                        *waiting = false;
                                        *direction = (0, 0);
                                        self.navigation = Navigation::default();
                                        self.player_deadline = None;
                                        view.window.request_redraw();
                                        self.update_control_flow(event_loop);
                                        return;
                                    }
                                    Err(error) => {
                                        eprintln!("Cannot travel to {target:?}: {error}");
                                        view.window.set_title(&format!(
                                            "Arcanum-RS | Travel unavailable: {error}"
                                        ));
                                    }
                                }
                            }
                        }
                        if let Some(message) = world.take_notice() {
                            eprintln!("{message}");
                            view.window.set_title(&format!("Arcanum-RS | {message}"));
                        }
                        self.player_deadline = (!view.occluded
                            && (input != (0, 0) || world.player_moving()))
                        .then_some(now + CAMERA_TICK);
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
        self.update_control_flow(event_loop);
    }

    fn user_event(&mut self, _: &ActiveEventLoop, _: ()) {
        if let ViewContent::Stream { world, .. } = &mut self.content
            && world.receive()
            && let Some(view) = &self.view
        {
            view.window.request_redraw();
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
        let playing = matches!(&self.content, ViewContent::Stream { world, .. } if world.playing());
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
                    && event.logical_key == Key::Named(NamedKey::Escape) =>
                {
                    if let Some((menu, _)) = &mut self.pause {
                        let command = menu.back();
                        self.menu_action(command, event_loop);
                    } else {
                        if let ViewContent::Stream { world, shift, .. } = &mut self.content {
                            world.cancel_player_route();
                            *shift = false;
                        }
                        self.navigation = Navigation::default();
                        self.player_deadline = None;
                        self.animation_deadline = None;
                        self.pause = Some((Menu::new(), Instant::now()));
                        self.menu_action(MenuCommand::Stay, event_loop);
                    }
                    return;
                }
                _ => {}
            }
            if let Some((menu, _)) = &mut self.pause {
                let command = match &event {
                    WindowEvent::KeyboardInput { event, .. } => {
                        if !event.state.is_pressed() || event.repeat {
                            return;
                        }
                        match event.logical_key {
                            Key::Named(NamedKey::ArrowUp) => {
                                menu.adjust(-1);
                                MenuCommand::Stay
                            }
                            Key::Named(NamedKey::ArrowDown) => {
                                menu.adjust(1);
                                MenuCommand::Stay
                            }
                            Key::Named(NamedKey::Enter) => menu.activate(&mut self.settings),
                            _ => return,
                        }
                    }
                    WindowEvent::MouseInput {
                        state: ElementState::Pressed,
                        button: MouseButton::Left,
                        ..
                    } => {
                        let Some(cursor) = self.cursor else { return };
                        let size = self.view.as_ref().unwrap().window.inner_size();
                        let scale = u32::from(self.settings.zoom);
                        menu.click(
                            (cursor.0 / scale, cursor.1 / scale),
                            logical_size(size, self.settings.zoom),
                            &mut self.settings,
                        )
                    }
                    WindowEvent::MouseInput { .. }
                    | WindowEvent::MouseWheel { .. }
                    | WindowEvent::ModifiersChanged(_) => return,
                    _ => {
                        self.render_event(event_loop, id, event, elapsed);
                        return;
                    }
                };
                self.menu_action(command, event_loop);
                return;
            }
        }
        self.render_event(event_loop, id, event, elapsed);
    }
}

impl Viewer<'_> {
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
                self.settings.zoom = (i32::from(self.settings.zoom) + (y > 0.) as i32
                    - (y < 0.) as i32)
                    .clamp(1, 3) as u8;
                view.window.request_redraw();
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
                    && self.navigation.held == 0
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
            } if matches!(self.content, ViewContent::Stream { explore: true, .. })
                && Navigation::key(&event.logical_key).is_some() =>
            {
                if !event.repeat {
                    let now = Instant::now();
                    self.navigation.set_key(
                        Navigation::key(&event.logical_key).unwrap(),
                        event.state.is_pressed(),
                        now,
                    );
                    if let ViewContent::Stream { world, .. } = &mut self.content
                        && world.playing()
                    {
                        if event.state.is_pressed() {
                            world.cancel_player_route();
                        }
                        self.navigation.sampled_at = None; // Gameplay uses its own tick, including after release.
                        self.player_deadline = Some(now);
                    }
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
                        self.settings.zoom = 1;
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
                let mut deadline = None;
                let result = match &mut self.content {
                    ViewContent::Fixed(layers) => {
                        let center = ((size.width / 2) as i32, (size.height / 2) as i32);
                        let draws = layers.iter().map(|layer| SpriteDraw {
                            frame: layer.frame,
                            anchor: (
                                center.0 + i32::from(layer.relative_anchor.0),
                                center.1 + i32::from(layer.relative_anchor.1),
                            ),
                        });
                        view.redraw(size, draws.map(|draw| (draw, Modulation::Unlit)))
                            .map(|_| ())
                    }
                    ViewContent::World { scene, camera, .. } => {
                        let viewport = (size.width, size.height);
                        if size.width != 0 && size.height != 0 {
                            camera.resize(viewport);
                        }
                        view.redraw(
                            size,
                            scene
                                .visible(camera, viewport)
                                .map(|draw| (draw, Modulation::Unlit)),
                        )
                        .map(|_| ())
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
                        ..
                    } => {
                        let sampled = Instant::now();
                        let elapsed = animation_ms.map_or(play_elapsed, Duration::from_millis);
                        let menu = self.pause.as_ref().map(|(menu, _)| {
                            menu.frame(
                                self.settings,
                                match self.renderer {
                                    RenderBackend::Cpu => "CPU",
                                    RenderBackend::Vulkan => "VULKAN",
                                    RenderBackend::Gles => "OPENGL",
                                },
                            )
                        });
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
                            },
                        )
                        .map(|ready| {
                            *waiting = !ready;
                            if ready {
                                *last_presented = Some(camera.clone());
                                *last_viewport = (size.width, size.height);
                                *last_zoom = self.settings.zoom;
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

#[derive(Default)]
struct FrameTiming {
    composition: Duration,
    packing: Duration,
    submission: Duration,
    host: Duration,
}

fn logical_size(size: PhysicalSize<u32>, zoom: u8) -> (u32, u32) {
    let zoom = u32::from(zoom);
    (size.width.div_ceil(zoom), size.height.div_ceil(zoom))
}

struct FrameOptions<'a> {
    elapsed: Duration,
    settings: Settings,
    menu: Option<&'a Arc<arcanum_rs::content::art::LoadedFrame>>,
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
            cursor,
        } = options;
        let zoom = settings.zoom;
        let background = stream_background(world.playing());
        let viewport = logical_size(size, zoom);
        if size.width == 0 || size.height == 0 {
            return Ok(false);
        }
        world.follow_player(elapsed, camera);
        camera.resize(viewport);
        let prepared = world.prepare(camera, viewport, direction)?;
        if !prepared.ready {
            // Keep the last complete buffer visible while the worker prepares
            // the destination. The completion event drives the next redraw.
            return Ok(false);
        }
        world.start_player()?;
        if world.playing() {
            let scale = u32::from(zoom);
            world.update_hover(
                cursor
                    .filter(|_| menu.is_none())
                    .map(|(x, y)| (x / scale, y / scale)),
                viewport,
                camera,
                elapsed,
            );
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
        let draws = world
            .visible_assets_at(camera, viewport, elapsed)
            .filter_map(|mut draw| {
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
            .chain(menu.map(|asset| arcanum_rs::content::world::WorldDraw {
                asset,
                sprite: menu::layer(asset, viewport),
                modulation: Modulation::Unlit,
            }));
        #[cfg(feature = "gpu")]
        if let Presentation::Gpu(renderer) = &mut self.backend {
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
            renderer.redraw(
                (size.width, size.height),
                zoom,
                background,
                draws,
                &self.window,
                trace,
            )?;
            return Ok(true);
        }
        let frame = self.redraw_scaled(
            size,
            zoom,
            background,
            draws.map(|draw| (draw.sprite, draw.modulation)),
        )?;
        if trace
            || prepared.scheduling
                + prepared.ordering
                + prepared.shading
                + frame.composition
                + frame.packing
                + frame.submission
                > Duration::from_millis(30)
        {
            eprintln!(
                "Frame {:?} {}x{} sync load wait 0ns schedule {:?} order {:?} shading {:?} compose {:?} pack {:?} submit {:?} host {:?} animation {}ms",
                camera.position(),
                size.width,
                size.height,
                prepared.scheduling,
                prepared.ordering,
                prepared.shading,
                frame.composition,
                frame.packing,
                frame.submission,
                frame.host,
                elapsed.as_millis()
            );
        }
        Ok(true)
    }

    fn redraw<'a>(
        &mut self,
        size: PhysicalSize<u32>,
        draws: impl IntoIterator<Item = (SpriteDraw<'a>, Modulation<'a>)>,
    ) -> Result<FrameTiming, String> {
        self.redraw_scaled(size, 1, BACKGROUND, draws)
    }

    fn redraw_scaled<'a>(
        &mut self,
        size: PhysicalSize<u32>,
        zoom: u8,
        background: [u8; 3],
        draws: impl IntoIterator<Item = (SpriteDraw<'a>, Modulation<'a>)>,
    ) -> Result<FrameTiming, String> {
        match &mut self.backend {
            Presentation::Cpu(cpu) => cpu.redraw(size, zoom, background, draws, &self.window),
            #[cfg(feature = "gpu")]
            Presentation::Gpu(_) => Err("GPU presentation requires a streamed world".into()),
        }
    }
}

impl CpuSurface {
    fn redraw<'a>(
        &mut self,
        size: PhysicalSize<u32>,
        zoom: u8,
        background: [u8; 3],
        draws: impl IntoIterator<Item = (SpriteDraw<'a>, Modulation<'a>)>,
        window: &Window,
    ) -> Result<FrameTiming, String> {
        let (Some(width), Some(height)) =
            (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
        else {
            return Ok(FrameTiming::default());
        };
        let frame_start = Instant::now();
        let start = frame_start;
        let count = (width.get() as usize)
            .checked_mul(height.get() as usize)
            .filter(|&count| count <= MAX_SURFACE_PIXELS)
            .ok_or_else(|| {
                format!(
                    "sprite surface {} × {} exceeds {MAX_SURFACE_PIXELS} pixels",
                    width, height
                )
            })?;
        let viewport = logical_size(size, zoom);
        let pixels = (viewport.0 * viewport.1) as usize;
        self.pixels
            .try_reserve(pixels.saturating_sub(self.pixels.len()))
            .map_err(|error| format!("cannot allocate sprite framebuffer: {error}"))?;
        self.pixels.resize(pixels, background);
        self.pixels.fill(background);
        blit_modulated(&mut self.pixels, viewport, viewport.0 as usize, draws)
            .map_err(|error| format!("cannot composite sprite: {error}"))?;
        let composition = start.elapsed();
        let start = Instant::now();
        self.surface
            .resize(width, height)
            .map_err(|error| format!("cannot resize sprite surface: {error}"))?;
        let mut buffer = self
            .surface
            .buffer_mut()
            .map_err(|error| format!("cannot access presentation buffer: {error}"))?;
        if buffer.len() != count {
            return Err(format!(
                "presentation buffer has {} pixels, expected {count}",
                buffer.len()
            ));
        }
        if zoom == 1 {
            for (output, &[r, g, b]) in buffer.iter_mut().zip(&self.pixels) {
                *output = u32::from_be_bytes([0, r, g, b]);
            }
        } else {
            let zoom = usize::from(zoom);
            for (y, row) in buffer.chunks_exact_mut(size.width as usize).enumerate() {
                let start = y / zoom * viewport.0 as usize;
                for (x, output) in row.iter_mut().enumerate() {
                    let [r, g, b] = self.pixels[start + x / zoom];
                    *output = u32::from_be_bytes([0, r, g, b]);
                }
            }
        }
        let packing = start.elapsed();
        let start = Instant::now();
        window.pre_present_notify();
        buffer
            .present()
            .map_err(|error| format!("cannot present sprite: {error}"))?;
        Ok(FrameTiming {
            composition,
            packing,
            submission: start.elapsed(),
            host: frame_start.elapsed(),
        })
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
    fn held_navigation_integrates_elapsed_time_and_waits_without_catching_up() {
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
