//! Wayland surface presentation for the experimental sprite compositor.

use crate::RenderBackend;
use arcanum_rs::{
    content::world::WorldDraw,
    gpu::{self, Blend, Compositor},
};
use std::{sync::Arc, time::Instant};
use wgpu::{
    Backends, CommandEncoderDescriptor, CompositeAlphaMode, Device, PresentMode, Surface,
    SurfaceConfiguration, SurfaceError, TextureFormat, TextureUsages, TextureViewDescriptor,
};
use winit::{event_loop::OwnedDisplayHandle, window::Window};

pub(super) struct WindowRenderer {
    surface: Surface<'static>,
    config: SurfaceConfiguration,
    compositor: Compositor,
    configured: bool,
    // EGL termination may issue Wayland calls even after the surface drops.
    // Keep the display alive through destruction of all wgpu resources.
    _display: OwnedDisplayHandle,
}

impl WindowRenderer {
    pub fn new(
        window: Arc<Window>,
        display: OwnedDisplayHandle,
        backend: RenderBackend,
        trace: bool,
    ) -> Result<Self, String> {
        let start = Instant::now();
        let backends = match backend {
            RenderBackend::Vulkan => Backends::VULKAN,
            RenderBackend::Gles => Backends::GL,
            RenderBackend::Cpu => return Err("expected a GPU renderer".into()),
        };
        let instance = gpu::instance(backends);
        let surface = instance
            .create_surface(window)
            .map_err(|e| format!("cannot create GPU surface: {e}"))?;
        let adapter = gpu::hardware_adapter(&instance, backends, Some(&surface))?;
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
            usage: TextureUsages::RENDER_ATTACHMENT,
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
            _display: display,
        })
    }

    pub fn redraw<'a>(
        &mut self,
        size: (u32, u32),
        draws: impl IntoIterator<Item = WorldDraw<'a>>,
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
        let stats = self.compositor.prepare(
            size,
            draws.into_iter().map(|draw| {
                let blend = Blend::for_modulation(draw.modulation);
                (draw, blend)
            }),
        )?;
        let begin = Instant::now();
        let target = frame.texture.create_view(&TextureViewDescriptor::default());
        let mut encoder = self
            .compositor
            .device
            .create_command_encoder(&CommandEncoderDescriptor::default());
        self.compositor
            .encode(&mut encoder, &target, super::BACKGROUND);
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
        if trace {
            let host = start.elapsed();
            let gpu = self.compositor.complete(submission)?;
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
