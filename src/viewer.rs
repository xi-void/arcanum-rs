//! Optional desktop presentation for the sprite diagnostic.

use arcanum_rs::art::SpriteFrame;
use arcanum_rs::compositor::{SpriteDraw, blit_ordered};
use softbuffer::{Context, Surface};
use std::{num::NonZeroU32, rc::Rc};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, OwnedDisplayHandle};
use winit::window::{Window, WindowId};

const BACKGROUND: [u8; 3] = [32, 43, 58];
const MAX_SURFACE_PIXELS: usize = 16 * 1024 * 1024;

pub struct Layer {
    pub frame: SpriteFrame,
    /// Physical pixel offset from the center of the drawable area.
    pub relative_anchor: (i16, i16),
}

pub fn synthetic_frame() -> SpriteFrame {
    synthetic_ring([255, 168, 70], (60, 62), (-8, 5))
}

pub fn synthetic_overlap() -> Vec<Layer> {
    vec![
        Layer {
            frame: synthetic_frame(),
            relative_anchor: (-25, 10),
        },
        Layer {
            frame: synthetic_ring([64, 216, 245], (-5, 50), (7, -3)),
            relative_anchor: (-44, -8),
        },
    ]
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
        vec![Layer {
            frame,
            relative_anchor: (0, 0),
        }],
    )
}

pub fn show_layers(title: String, layers: Vec<Layer>) -> Result<(), String> {
    let event_loop = EventLoop::new().map_err(|error| format!("cannot open display: {error}"))?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let context = Context::new(event_loop.owned_display_handle())
        .map_err(|error| format!("cannot initialize software presentation: {error}"))?;
    let mut app = Viewer {
        title,
        context,
        view: None,
        layers,
        failure: None,
    };
    event_loop
        .run_app(&mut app)
        .map_err(|error| format!("window event loop failed: {error}"))?;
    app.failure.map_or(Ok(()), Err)
}

struct Viewer {
    title: String,
    context: Context<OwnedDisplayHandle>,
    view: Option<WindowSurface>,
    layers: Vec<Layer>,
    failure: Option<String>,
}

struct WindowSurface {
    surface: Surface<OwnedDisplayHandle, Rc<Window>>,
    window: Rc<Window>,
    pixels: Vec<[u8; 3]>,
}

impl Viewer {
    fn open(&self, event_loop: &ActiveEventLoop) -> Result<WindowSurface, String> {
        let attributes = Window::default_attributes()
            .with_title(&self.title)
            .with_inner_size(LogicalSize::new(640, 480));
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

impl ApplicationHandler for Viewer {
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
            WindowEvent::RedrawRequested => {
                if let Err(error) = view.redraw(&self.layers) {
                    self.fail(event_loop, error);
                }
            }
            _ => {}
        }
    }
}

impl WindowSurface {
    fn redraw(&mut self, layers: &[Layer]) -> Result<(), String> {
        let size = self.window.inner_size();
        let (Some(width), Some(height)) =
            (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
        else {
            return Ok(());
        };
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

        // The checked surface area bounds both center coordinates.
        let center = ((size.width / 2) as i32, (size.height / 2) as i32);
        let draws = layers.iter().map(|layer| SpriteDraw {
            frame: &layer.frame,
            anchor: (
                center.0 + i32::from(layer.relative_anchor.0),
                center.1 + i32::from(layer.relative_anchor.1),
            ),
        });
        blit_ordered(
            &mut self.pixels,
            (size.width, size.height),
            size.width as usize,
            draws,
        )
        .map_err(|error| format!("cannot composite sprite: {error}"))?;

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
        self.window.pre_present_notify();
        buffer
            .present()
            .map_err(|error| format!("cannot present sprite: {error}"))
    }
}
