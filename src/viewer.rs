//! Optional desktop presentation for the sprite diagnostic.

use crate::RenderBackend;
use arcanum_rs::art::SpriteFrame;

#[cfg(feature = "gpu")]
mod gpu;
use arcanum_rs::camera::{Camera, CameraScene};
use arcanum_rs::compositor::{Modulation, SpriteDraw, blit_modulated};
use arcanum_rs::content::world::{InteractiveWorld, WorldStream};
use softbuffer::{Context, Surface};
use std::{
    num::NonZeroU32,
    sync::Arc,
    time::{Duration, Instant},
};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalSize, Size};
use winit::event::{StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, OwnedDisplayHandle};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

const BACKGROUND: [u8; 3] = [32, 43, 58];
const MAX_SURFACE_PIXELS: usize = 16 * 1024 * 1024;

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

pub fn show_stream(
    title: String,
    world: WorldStream,
    camera: Camera,
    size: (u32, u32),
    trace: bool,
    renderer: RenderBackend,
    animation_ms: Option<u64>,
) -> Result<(), String> {
    #[cfg(not(feature = "gpu"))]
    if renderer != RenderBackend::Cpu {
        return Err("GPU renderer requires the gpu feature".into());
    }
    let event_loop = make_loop()?;
    let proxy = event_loop.create_proxy();
    let world = world
        .into_interactive(
            move || {
                let _ = proxy.send_event(());
            },
            trace,
        )
        .map_err(|error| format!("cannot start sector preparation worker: {error}"))?;
    run_content(
        title,
        ViewContent::Stream {
            world: Box::new(world),
            camera,
            shift: false,
            direction: (0, 0),
            last_presented: None,
            waiting: false,
            trace,
            animation_ms,
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
        waiting: bool,
        trace: bool,
        animation_ms: Option<u64>,
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
        event_loop.set_control_flow(deadline.map_or(ControlFlow::Wait, ControlFlow::WaitUntil));
    }

    fn update_light_probe(world: &mut InteractiveWorld, key: &str, trace: bool) -> bool {
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
        update.receivers != 0
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
        if matches!(cause, StartCause::ResumeTimeReached { .. })
            && self
                .animation_deadline
                .is_some_and(|deadline| Instant::now() >= deadline)
        {
            self.schedule_animation(event_loop, None);
            if let Some(view) = &self.view {
                view.window.request_redraw();
            }
        }
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
        self.schedule_animation(event_loop, None);
        self.view = None;
    }

    fn exiting(&mut self, _: &ActiveEventLoop) {
        // Destroy EGL/Vulkan presentation resources while winit's display
        // and event loop are still alive.
        self.view = None;
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some(view) = self.view.as_mut().filter(|view| view.window.id() == id) else {
            return;
        };
        match event {
            WindowEvent::CloseRequested | WindowEvent::Destroyed => event_loop.exit(),
            WindowEvent::Resized(size) if size.width != 0 && size.height != 0 => {
                view.window.request_redraw();
            }
            WindowEvent::ScaleFactorChanged { .. } => view.window.request_redraw(),
            WindowEvent::Occluded(occluded) => {
                view.occluded = occluded;
                if occluded {
                    self.schedule_animation(event_loop, None);
                } else {
                    view.window.request_redraw();
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
                if let ViewContent::World { shift, .. } | ViewContent::Stream { shift, .. } =
                    &mut self.content
                {
                    *shift = false;
                }
            }
            WindowEvent::KeyboardInput {
                event,
                is_synthetic: false,
                ..
            } if event.state.is_pressed() => {
                if let Key::Character(key) = &event.logical_key
                    && let ViewContent::Stream { world, trace, .. } = &mut self.content
                    && Self::update_light_probe(world, key.as_str(), *trace)
                {
                    view.window.request_redraw();
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
                        waiting,
                        trace,
                        animation_ms,
                        ..
                    } => {
                        let sampled = Instant::now();
                        let elapsed = animation_ms.map_or_else(
                            || sampled.duration_since(self.epoch),
                            Duration::from_millis,
                        );
                        view.redraw_stream(size, world, camera, *direction, *trace, elapsed)
                            .map(|ready| {
                                *waiting = !ready;
                                if ready {
                                    *last_presented = Some(camera.clone());
                                    if animation_ms.is_none() {
                                        deadline = world
                                            .animation_delay(
                                                camera,
                                                (size.width, size.height),
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

impl WindowSurface {
    fn redraw_stream(
        &mut self,
        size: PhysicalSize<u32>,
        world: &mut InteractiveWorld,
        camera: &mut Camera,
        direction: (i32, i32),
        trace: bool,
        elapsed: Duration,
    ) -> Result<bool, String> {
        let viewport = (size.width, size.height);
        if size.width == 0 || size.height == 0 {
            return Ok(false);
        }
        camera.resize(viewport);
        let prepared = world.prepare(camera, viewport, direction)?;
        if !prepared.ready {
            // Keep the last complete buffer visible while the worker prepares
            // the destination. The completion event drives the next redraw.
            return Ok(false);
        }
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
                viewport,
                world.visible_assets_at(camera, viewport, elapsed),
                &self.window,
                trace,
            )?;
            return Ok(true);
        }
        let frame = self.redraw(size, world.visible_modulated_at(camera, viewport, elapsed))?;
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
        match &mut self.backend {
            Presentation::Cpu(cpu) => cpu.redraw(size, draws, &self.window),
            #[cfg(feature = "gpu")]
            Presentation::Gpu(_) => Err("GPU presentation requires a streamed world".into()),
        }
    }
}

impl CpuSurface {
    fn redraw<'a>(
        &mut self,
        size: PhysicalSize<u32>,
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
        self.pixels
            .try_reserve(count.saturating_sub(self.pixels.len()))
            .map_err(|error| format!("cannot allocate sprite framebuffer: {error}"))?;
        self.pixels.truncate(count);
        self.pixels.fill(BACKGROUND);
        self.pixels.resize(count, BACKGROUND);

        blit_modulated(
            &mut self.pixels,
            (size.width, size.height),
            size.width as usize,
            draws,
        )
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
        for (output, &[r, g, b]) in buffer.iter_mut().zip(&self.pixels) {
            *output = u32::from_be_bytes([0, r, g, b]);
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
