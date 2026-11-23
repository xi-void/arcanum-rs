//! Optional desktop presentation for the sprite diagnostic.

use arcanum_rs::art::SpriteFrame;
use arcanum_rs::camera::{Camera, CameraScene};
use arcanum_rs::compositor::{SpriteDraw, blit_ordered};
use arcanum_rs::content::world::WorldStream;
use softbuffer::{Context, Surface};
use std::{
    num::NonZeroU32,
    rc::Rc,
    time::{Duration, Instant},
};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalSize, Size};
use winit::event::WindowEvent;
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
) -> Result<(), String> {
    show_content(
        title,
        ViewContent::Stream {
            world: Box::new(world),
            camera,
            shift: false,
            trace,
        },
        PhysicalSize::<u32>::from(size).into(),
    )
}

enum ViewContent<'a> {
    Fixed(&'a [Layer<'a>]),
    Stream {
        world: Box<WorldStream>,
        camera: Camera,
        shift: bool,
        trace: bool,
    },
    World {
        scene: &'a CameraScene<'a>,
        camera: Camera,
        shift: bool,
    },
}

fn show_content(title: String, content: ViewContent<'_>, size: Size) -> Result<(), String> {
    let event_loop = EventLoop::new().map_err(|error| format!("cannot open display: {error}"))?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let context = Context::new(event_loop.owned_display_handle())
        .map_err(|error| format!("cannot initialize software presentation: {error}"))?;
    let mut app = Viewer {
        title,
        size,
        context,
        view: None,
        content,
        failure: None,
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
    context: Context<OwnedDisplayHandle>,
    view: Option<WindowSurface>,
    content: ViewContent<'a>,
    failure: Option<String>,
}

struct WindowSurface {
    surface: Surface<OwnedDisplayHandle, Rc<Window>>,
    window: Rc<Window>,
    pixels: Vec<[u8; 3]>,
}

impl Viewer<'_> {
    fn open(&self, event_loop: &ActiveEventLoop) -> Result<WindowSurface, String> {
        let attributes = Window::default_attributes()
            .with_title(&self.title)
            .with_inner_size(self.size);
        let window = Rc::new(
            event_loop
                .create_window(attributes)
                .map_err(|error| format!("cannot create sprite window: {error}"))?,
        );
        let surface = Surface::new(&self.context, Rc::clone(&window))
            .map_err(|error| format!("cannot create sprite surface: {error}"))?;
        Ok(WindowSurface {
            surface,
            window,
            pixels: Vec::new(),
        })
    }

    fn fail(&mut self, event_loop: &ActiveEventLoop, error: String) {
        self.failure = Some(error);
        event_loop.exit();
    }
}

impl ApplicationHandler for Viewer<'_> {
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

    fn suspended(&mut self, _: &ActiveEventLoop) {
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
            WindowEvent::ScaleFactorChanged { .. } | WindowEvent::Occluded(false) => {
                view.window.request_redraw();
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
                if let ViewContent::World { camera, shift, .. }
                | ViewContent::Stream { camera, shift, .. } = &mut self.content
                {
                    let size = view.window.inner_size();
                    let viewport = (size.width, size.height);
                    let step = if *shift { 64 } else { 16 };
                    let changed = match event.logical_key {
                        Key::Named(NamedKey::Escape) => {
                            event_loop.exit();
                            return;
                        }
                        Key::Named(NamedKey::Home) => camera.reset(viewport),
                        Key::Named(NamedKey::ArrowLeft) => camera.pan((-step, 0), viewport),
                        Key::Named(NamedKey::ArrowRight) => camera.pan((step, 0), viewport),
                        Key::Named(NamedKey::ArrowUp) => camera.pan((0, -step), viewport),
                        Key::Named(NamedKey::ArrowDown) => camera.pan((0, step), viewport),
                        _ => false,
                    };
                    if changed {
                        view.window.request_redraw();
                    }
                }
            }
            WindowEvent::RedrawRequested => {
                let size = view.window.inner_size();
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
                        view.redraw(size, draws).map(|_| ())
                    }
                    ViewContent::World { scene, camera, .. } => {
                        let viewport = (size.width, size.height);
                        if size.width != 0 && size.height != 0 {
                            camera.resize(viewport);
                        }
                        view.redraw(size, scene.visible(camera, viewport))
                            .map(|_| ())
                    }
                    ViewContent::Stream {
                        world,
                        camera,
                        trace,
                        ..
                    } => view.redraw_stream(size, world, camera, *trace),
                };
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
}

impl WindowSurface {
    fn redraw_stream(
        &mut self,
        size: PhysicalSize<u32>,
        world: &mut WorldStream,
        camera: &mut Camera,
        trace: bool,
    ) -> Result<(), String> {
        let viewport = (size.width, size.height);
        if size.width == 0 || size.height == 0 {
            return Ok(());
        }
        camera.resize(viewport);
        let before = world.stats().sector_reads;
        let prepared = world.prepare(camera, viewport)?;
        if world.stats().sector_reads != before {
            for sector in world.reports() {
                let note = sector.diagnostics.first().map_or("", String::as_str);
                eprintln!(
                    "Sector {}: {} surfaces, {} walls, {} scenery, {} portals, {} diagnostics {note}",
                    sector.id,
                    sector.surfaces,
                    sector.wall_count,
                    sector.scenery_count,
                    sector.portal_count,
                    sector.diagnostics.len()
                );
            }
        }
        let frame = self.redraw(size, world.visible(camera, viewport))?;
        if trace
            || prepared.load
                + prepared.ordering
                + frame.composition
                + frame.packing
                + frame.submission
                > Duration::from_millis(30)
        {
            eprintln!(
                "Frame {:?} {}x{} load {:?} (ART read {:?}, ART decode {:?}) order {:?} compose {:?} pack {:?} submit {:?}",
                camera.position(),
                size.width,
                size.height,
                prepared.load,
                prepared.art_read,
                prepared.art_decode,
                prepared.ordering,
                frame.composition,
                frame.packing,
                frame.submission
            );
        }
        Ok(())
    }

    fn redraw<'a>(
        &mut self,
        size: PhysicalSize<u32>,
        draws: impl IntoIterator<Item = SpriteDraw<'a>>,
    ) -> Result<FrameTiming, String> {
        let (Some(width), Some(height)) =
            (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
        else {
            return Ok(FrameTiming::default());
        };
        let start = Instant::now();
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

        blit_ordered(
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
        self.window.pre_present_notify();
        buffer
            .present()
            .map_err(|error| format!("cannot present sprite: {error}"))?;
        Ok(FrameTiming {
            composition,
            packing,
            submission: start.elapsed(),
        })
    }
}
