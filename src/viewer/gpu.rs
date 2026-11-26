//! Native presentation for the Vulkan sprite compositor.

use arcanum_rs::developer::{Failure, Output};
use arcanum_rs::{
    content::{art::LoadedFrame, world::WorldDraw},
    gpu::{self, Compositor},
};
use serde_json::{Value, json};
use std::{collections::VecDeque, path::PathBuf, sync::Arc, time::Instant};
use wgpu::{
    CommandEncoderDescriptor, CompositeAlphaMode, Device, PresentMode, Surface,
    SurfaceConfiguration, SurfaceError, TextureFormat, TextureUsages, TextureViewDescriptor,
};
use winit::window::Window;

pub(super) struct WindowRenderer {
    surface: Surface<'static>,
    config: SurfaceConfiguration,
    compositor: Compositor,
    configured: bool,
    pending: Option<Capture>,
    captures: VecDeque<(u64, Result<Value, String>)>,
    sequence: u64,
    pub(super) stats: Option<FrameStats>,
}

#[derive(serde::Serialize)]
pub(super) struct FrameStats {
    backend: &'static str,
    viewport: (u32, u32),
    acquire: std::time::Duration,
    encode: std::time::Duration,
    submit: std::time::Duration,
    present: std::time::Duration,
    host: std::time::Duration,
    gpu: Option<std::time::Duration>,
    preparation: gpu::Preparation,
}

struct Capture {
    token: u64,
    directory: Arc<cap_std::fs::Dir>,
    file: String,
    path: PathBuf,
}

impl WindowRenderer {
    pub fn new(window: Arc<Window>, trace: bool, allow_capture: bool) -> Result<Self, String> {
        let start = Instant::now();
        let instance = gpu::instance();
        let surface = instance
            .create_surface(window)
            .map_err(|e| format!("cannot create GPU surface: {e}"))?;
        let adapter = gpu::hardware_adapter(&instance, Some(&surface))?;
        let caps = surface.get_capabilities(&adapter);
        let format = [TextureFormat::Bgra8Unorm, TextureFormat::Rgba8Unorm]
            .into_iter()
            .find(|format| caps.formats.contains(format))
            .ok_or_else(|| {
                format!(
                    "surface lacks a byte-domain RGBA/BGRA format: {:?}",
                    caps.formats
                )
            })?;
        let alpha_mode = if caps.alpha_modes.contains(&CompositeAlphaMode::Opaque) {
            CompositeAlphaMode::Opaque
        } else {
            caps.alpha_modes[0]
        };
        let config = SurfaceConfiguration {
            usage: TextureUsages::RENDER_ATTACHMENT
                | if allow_capture && caps.usages.contains(TextureUsages::COPY_SRC) {
                    TextureUsages::COPY_SRC
                } else {
                    TextureUsages::empty()
                },
            format,
            width: 1,
            height: 1,
            present_mode: PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode,
            view_formats: vec![],
        };
        let compositor = Compositor::new(&adapter, format, trace)?;
        eprintln!(
            "GPU surface {:?}, {:?}, {:?} in {:?}; offered {:?}",
            config.format,
            config.present_mode,
            config.alpha_mode,
            start.elapsed(),
            caps.formats
        );
        Ok(Self {
            surface,
            config,
            compositor,
            configured: false,
            pending: None,
            captures: VecDeque::new(),
            sequence: 0,
            stats: None,
        })
    }

    pub fn request_capture(
        &mut self,
        directory: Arc<cap_std::fs::Dir>,
        path: PathBuf,
        file: String,
    ) -> Result<Output, Failure> {
        if !self.config.usage.contains(TextureUsages::COPY_SRC) {
            return Err(Failure::new(
                "unsupported_operation",
                "The active Vulkan surface does not support capture",
            ));
        }
        if self.pending.is_some() {
            return Err(Failure::new("busy", "A capture is already queued"));
        }
        self.sequence += 1;
        self.pending = Some(Capture {
            token: self.sequence,
            directory,
            path: path.clone(),
            file,
        });
        Ok(Output::Accepted(json!({"token":self.sequence,"path":path})))
    }

    pub fn operation(&self, token: u64) -> Result<Output, Failure> {
        if let Some(request) = self.pending.as_ref().filter(|r| r.token == token) {
            return Ok(Output::Accepted(json!({"token":token,"path":request.path})));
        }
        match self.captures.iter().find(|(id, _)| *id == token) {
            Some((_, Ok(result))) => Ok(Output::Data(result.clone())),
            Some((_, Err(error))) => Err(Failure::new("capture_failed", error)),
            None => Err(Failure::new(
                "unknown_operation",
                "Capture token is unavailable",
            )),
        }
    }

    pub fn render_inset<'a>(
        &mut self,
        asset: &Arc<LoadedFrame>,
        scale: f32,
        background: [u8; 3],
        draws: impl IntoIterator<Item = WorldDraw<'a>>,
    ) -> Result<(), String> {
        self.compositor
            .render_sprite(asset, scale, background, draws)
    }

    pub fn save_preview<'a>(
        &mut self,
        draws: impl IntoIterator<Item = WorldDraw<'a>>,
    ) -> Result<Vec<u8>, String> {
        let size = arcanum_rs::content::save::PREVIEW_SIZE;
        self.compositor
            .prepare_with_overlay(size, 1, draws, std::iter::empty())?;
        let device = &self.compositor.device;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Savegame world preview"),
            size: wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.config.format,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor::default());
        self.compositor.encode(
            &mut encoder,
            &texture.create_view(&Default::default()),
            [0; 3],
        )?;
        let (buffer, stride) = copy_pixels(device, &mut encoder, &texture, size);
        let submission = self.compositor.queue.submit([encoder.finish()]);
        let bytes = gpu::read_buffer(device, &buffer, submission)?;
        Ok(rgb_pixels(&bytes, stride, size, self.config.format))
    }

    pub fn redraw<'a>(
        &mut self,
        (size, zoom): ((u32, u32), u8),
        background: [u8; 3],
        draws: impl IntoIterator<Item = WorldDraw<'a>>,
        hud: impl IntoIterator<Item = gpu::OverlayDraw<'a>>,
        window: &Window,
        trace: bool,
    ) -> Result<(), String> {
        self.compositor.check_health()?;
        let start = Instant::now();
        let limit = self.compositor.device.limits().max_texture_dimension_2d;
        if size.0 == 0
            || size.1 == 0
            || size.0 > limit
            || size.1 > limit
            || u64::from(size.0) * u64::from(size.1) > super::MAX_SURFACE_PIXELS as u64
        {
            return Err(format!("invalid GPU surface dimensions {size:?}"));
        }
        if !self.configured || (self.config.width, self.config.height) != size {
            (self.config.width, self.config.height) = size;
            self.surface
                .configure(&self.compositor.device, &self.config);
            self.configured = true;
        }
        self.compositor.check_health()?;
        let frame = match self.surface.get_current_texture() {
            Ok(frame) => frame,
            Err(SurfaceError::Lost | SurfaceError::Outdated) => {
                self.surface
                    .configure(&self.compositor.device, &self.config);
                self.surface
                    .get_current_texture()
                    .map_err(|e| format!("cannot recover GPU surface: {e}"))?
            }
            Err(error) => return Err(format!("cannot acquire GPU frame: {error}")),
        };
        let acquire = start.elapsed();
        let stats = self
            .compositor
            .prepare_with_overlay(size, zoom, draws, hud)?;
        let begin = Instant::now();
        let target = frame.texture.create_view(&TextureViewDescriptor::default());
        let mut encoder = self
            .compositor
            .device
            .create_command_encoder(&CommandEncoderDescriptor::default());
        self.compositor.encode(&mut encoder, &target, background)?;
        let capture = self.pending.take().map(|request| {
            let (buffer, stride) =
                copy_pixels(&self.compositor.device, &mut encoder, &frame.texture, size);
            (request, buffer, stride)
        });
        let commands = encoder.finish();
        self.compositor.check_health()?;
        let encode = begin.elapsed();
        let report_residency =
            trace && (stats.uploaded != 0 || stats.retired != 0 || stats.evicted != 0);
        if report_residency {
            report_allocations(&self.compositor.device, "before submit");
        }
        let begin = Instant::now();
        let submission = self.compositor.queue.submit([commands]);
        let submit = begin.elapsed();
        self.compositor.check_health()?;
        let begin = Instant::now();
        window.pre_present_notify();
        frame.present();
        let present = begin.elapsed();
        let host = start.elapsed();
        let gpu = if trace {
            self.compositor.complete(submission.clone())?
        } else {
            None
        };
        if let Some((request, buffer, stride)) = capture {
            let token = request.token;
            let outcome = gpu::read_buffer(&self.compositor.device, &buffer, submission)
                .and_then(|bytes| write_capture(request, &bytes, stride, size, self.config.format));
            if self.captures.len() == 16 {
                self.captures.pop_front();
            }
            self.captures.push_back((token, outcome));
        }
        self.stats = Some(FrameStats {
            backend: "vulkan",
            viewport: size,
            acquire,
            encode,
            submit,
            present,
            host,
            gpu,
            preparation: stats,
        });
        if trace {
            eprintln!(
                "GPU frame {size:?} acquire {acquire:?} encode {encode:?} submit {submit:?} present {present:?} host {host:?} complete {:?} gpu {gpu:?} {stats:?}",
                start.elapsed()
            );
            if report_residency {
                report_allocations(&self.compositor.device, "after completion");
            }
        }
        self.compositor.check_health()
    }
}

fn copy_pixels(
    device: &Device,
    encoder: &mut wgpu::CommandEncoder,
    texture: &wgpu::Texture,
    size: (u32, u32),
) -> (wgpu::Buffer, u32) {
    let stride = (size.0 * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Requested image readback"),
        size: u64::from(stride) * u64::from(size.1),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(stride),
                rows_per_image: None,
            },
        },
        wgpu::Extent3d {
            width: size.0,
            height: size.1,
            depth_or_array_layers: 1,
        },
    );
    (buffer, stride)
}

fn rgb_pixels(bytes: &[u8], stride: u32, size: (u32, u32), format: TextureFormat) -> Vec<u8> {
    bytes
        .chunks_exact(stride as usize)
        .flat_map(|row| row[..size.0 as usize * 4].as_chunks::<4>().0)
        .flat_map(|&[a, g, b, _]| {
            if format == TextureFormat::Bgra8Unorm {
                [b, g, a]
            } else {
                [a, g, b]
            }
        })
        .collect()
}

fn write_capture(
    request: Capture,
    bytes: &[u8],
    stride: u32,
    size: (u32, u32),
    format: TextureFormat,
) -> Result<Value, String> {
    let rgb = rgb_pixels(bytes, stride, size, format);
    let file = request
        .directory
        .open_with(
            &request.file,
            cap_std::fs::OpenOptions::new().create_new(true).write(true),
        )
        .map_err(|error| error.to_string())?;
    let result = (|| {
        let mut encoder = png::Encoder::new(file, size.0, size.1);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(&rgb)?;
        writer.finish()
    })();
    if let Err(error) = result {
        let _ = request.directory.remove_file(request.file);
        return Err(error.to_string());
    }
    Ok(
        json!({"token":request.token,"path":request.path,"viewport":size,"format":"png","backend":"vulkan"}),
    )
}

fn report_allocations(device: &Device, stage: &str) {
    // Nonzero on Vulkan builds with wgpu's optional resource counters.
    let hal = device.get_internal_counters().hal;
    let textures = hal.textures.read();
    if textures != 0 {
        eprintln!(
            "GPU allocator {stage}: textures={textures} texture_bytes={} buffers={} buffer_bytes={} memory_allocations={}",
            hal.texture_memory.read(),
            hal.buffers.read(),
            hal.buffer_memory.read(),
            hal.memory_allocations.read(),
        );
    }
}
