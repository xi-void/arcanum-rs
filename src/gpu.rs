//! Experimental sprite compositor using wgpu and the existing ordered world draws.

use crate::{
    compositor::{Modulation, sprite_origin},
    content::{art::LoadedFrame, world::WorldDraw},
};
use bytemuck::{Pod, Zeroable, bytes_of, cast_slice};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, Weak},
    time::{Duration, Instant},
};
use wgpu::*;

const MAX_DRAWS: usize = 32_768;
const MAX_COLUMNS: usize = 524_288;
const CACHE_BYTES: usize = 96 * 1024 * 1024;
const CACHE_TEXTURES: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Blend {
    SourceOver,
    Add,
    Subtract,
}

impl Blend {
    fn for_modulation(modulation: Modulation<'_>) -> Self {
        match modulation {
            Modulation::Additive | Modulation::AdditiveTint(_) => Self::Add,
            Modulation::Shadow(_) => Self::Subtract,
            _ => Self::SourceOver,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct InstanceData {
    rect: [f32; 4],
    shade: [u32; 4],
    nodes_a: [u32; 4],
    nodes_b: [u32; 4],
}

type TextureId = usize;

struct SpriteTexture {
    // Holding the weak allocation prevents pointer reuse while this key exists.
    asset: Weak<LoadedFrame>,
    bind: BindGroup,
    bytes: usize,
    used: u64,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Preparation {
    pub commands: Duration,
    pub uploads: Duration,
    pub uploaded: usize,
    pub uploaded_bytes: usize,
    pub reused: usize,
    pub retired: usize,
    pub evicted: usize,
    pub texture_count: usize,
    pub texture_bytes: usize,
    pub draws: usize,
    pub draw_calls: usize,
}

pub struct Compositor {
    pub device: Device,
    pub queue: Queue,
    over: RenderPipeline,
    add: RenderPipeline,
    subtract: RenderPipeline,
    globals: BindGroup,
    texture_layout: BindGroupLayout,
    viewport: Buffer,
    instances: Buffer,
    column_buffer: Buffer,
    draws: Vec<InstanceData>,
    commands: Vec<(TextureId, Blend, std::ops::Range<u32>)>,
    columns: Vec<u32>,
    textures: BTreeMap<TextureId, SpriteTexture>,
    texture_bytes: usize,
    texture_budget: (usize, usize), // bytes and distinct textures
    frame: u64,
    prepared: bool,
    timestamp_query: Option<Timestamps>,
    failure: Arc<Mutex<Option<String>>>,
}

pub fn instance(backends: Backends) -> Instance {
    Instance::new(&InstanceDescriptor {
        backends,
        ..Default::default()
    })
}

/// Keep hardware identification explicit, including for offscreen tests.
pub fn hardware_adapter(
    instance: &Instance,
    backends: Backends,
    surface: Option<&Surface<'_>>,
) -> Result<Adapter, String> {
    let adapters = instance.enumerate_adapters(backends);
    let mut diagnostics = Vec::new();
    for adapter in adapters {
        let info = adapter.get_info();
        let downlevel = adapter.get_downlevel_capabilities();
        let format = adapter.get_texture_format_features(TextureFormat::Rgba8Unorm);
        let presentation = surface.is_none_or(|target| adapter.is_surface_supported(target));
        eprintln!(
            "GPU candidate {:?}: {} ({:?}, vendor {:#06x}, device {:#06x}), {} {}, shader {:?}, RGBA8 {:?}, present={} timestamp={}",
            info.backend,
            info.name,
            info.device_type,
            info.vendor,
            info.device,
            info.driver,
            info.driver_info,
            downlevel.shader_model,
            format,
            presentation,
            adapter.features().contains(Features::TIMESTAMP_QUERY),
        );
        diagnostics.push(info.name.clone());
        if matches!(
            info.device_type,
            DeviceType::IntegratedGpu | DeviceType::DiscreteGpu
        ) && presentation
            && downlevel.flags.contains(DownlevelFlags::FRAGMENT_STORAGE)
            && format.flags.contains(TextureFormatFeatureFlags::BLENDABLE)
        {
            return Ok(adapter);
        }
    }
    Err(format!(
        "no compatible hardware GPU for {backends:?} (found: {}); select --renderer cpu for software presentation",
        diagnostics.join(", ")
    ))
}

impl Compositor {
    pub fn new(adapter: &Adapter, format: TextureFormat, timing: bool) -> Result<Self, String> {
        let features = if timing {
            adapter.features() & Features::TIMESTAMP_QUERY
        } else {
            Features::empty()
        };
        let limits = Limits {
            max_storage_buffers_per_shader_stage: 1,
            max_storage_buffer_binding_size: (MAX_COLUMNS * 4) as u32,
            ..Limits::downlevel_defaults().using_resolution(adapter.limits())
        };
        if !limits.check_limits(&adapter.limits()) {
            return Err("GPU adapter does not meet sprite compositor limits".into());
        }
        let (device, queue) = pollster::block_on(adapter.request_device(&DeviceDescriptor {
            label: Some("Arcanum sprite device"),
            required_features: features,
            required_limits: limits,
            memory_hints: MemoryHints::MemoryUsage,
            ..Default::default()
        }))
        .map_err(|error| format!("cannot initialize GPU device: {error}"))?;
        let failure = Arc::new(Mutex::new(None));
        let lost = failure.clone();
        device.set_device_lost_callback(move |reason, message| {
            *lost.lock().unwrap() = Some(format!("GPU device lost ({reason:?}): {message}"));
        });
        let uncaptured = failure.clone();
        device.on_uncaptured_error(Box::new(move |error| {
            uncaptured
                .lock()
                .unwrap()
                .get_or_insert_with(|| format!("GPU error: {error}"));
        }));
        device.push_error_scope(ErrorFilter::Validation);
        let viewport = buffer(
            &device,
            "sprite viewport",
            16,
            BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        );
        let instances = buffer(
            &device,
            "sprite instances",
            (MAX_DRAWS * size_of::<InstanceData>()) as u64,
            BufferUsages::VERTEX | BufferUsages::COPY_DST,
        );
        let column_buffer = buffer(
            &device,
            "wall column tints",
            (MAX_COLUMNS * 4) as u64,
            BufferUsages::STORAGE | BufferUsages::COPY_DST,
        );
        let globals_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("sprite globals"),
            entries: &[
                buffer_binding(0, ShaderStages::VERTEX_FRAGMENT, BufferBindingType::Uniform),
                buffer_binding(
                    1,
                    ShaderStages::FRAGMENT,
                    BufferBindingType::Storage { read_only: true },
                ),
            ],
        });
        let globals = device.create_bind_group(&BindGroupDescriptor {
            label: None,
            layout: &globals_layout,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: viewport.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: column_buffer.as_entire_binding(),
                },
            ],
        });
        let texture_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("original sprite pixels"),
            entries: &[BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Texture {
                    sample_type: TextureSampleType::Float { filterable: false },
                    view_dimension: TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[&globals_layout, &texture_layout],
            push_constant_ranges: &[],
        });
        let shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("sprite modulation"),
            source: ShaderSource::Wgsl(include_str!("gpu/sprites.wgsl").into()),
        });
        let over = pipeline(
            &device,
            &pipeline_layout,
            &shader,
            format,
            BlendState::ALPHA_BLENDING,
        );
        let indexed_blend = |operation| BlendState {
            color: BlendComponent {
                src_factor: BlendFactor::One,
                dst_factor: BlendFactor::One,
                operation,
            },
            alpha: BlendComponent {
                src_factor: BlendFactor::Zero,
                dst_factor: BlendFactor::One,
                operation: BlendOperation::Add,
            },
        };
        let add = pipeline(
            &device,
            &pipeline_layout,
            &shader,
            format,
            indexed_blend(BlendOperation::Add),
        );
        let subtract = pipeline(
            &device,
            &pipeline_layout,
            &shader,
            format,
            indexed_blend(BlendOperation::ReverseSubtract),
        );
        let timestamp_query = features
            .contains(Features::TIMESTAMP_QUERY)
            .then(|| Timestamps::new(&device));
        if let Some(error) = pollster::block_on(device.pop_error_scope()) {
            return Err(format!("invalid sprite GPU pipeline: {error}"));
        }
        let compositor = Self {
            device,
            queue,
            over,
            add,
            subtract,
            globals,
            texture_layout,
            viewport,
            instances,
            column_buffer,
            draws: Vec::new(),
            commands: Vec::new(),
            columns: Vec::new(),
            textures: BTreeMap::new(),
            texture_bytes: 0,
            texture_budget: (CACHE_BYTES, CACHE_TEXTURES),
            frame: 0,
            prepared: false,
            timestamp_query,
            failure,
        };
        compositor.check_health()?;
        Ok(compositor)
    }

    pub fn check_health(&self) -> Result<(), String> {
        match self.failure.lock().unwrap().as_ref() {
            Some(message) => Err(message.clone()),
            None => Ok(()),
        }
    }

    pub fn prepare<'a>(
        &mut self,
        size: (u32, u32),
        input: impl IntoIterator<Item = WorldDraw<'a>>,
    ) -> Result<Preparation, String> {
        self.prepare_scaled(size, 1, input)
    }

    /// Rasterize original pixels into integer-sized surface blocks.
    pub fn prepare_scaled<'a>(
        &mut self,
        size: (u32, u32),
        zoom: u8,
        input: impl IntoIterator<Item = WorldDraw<'a>>,
    ) -> Result<Preparation, String> {
        self.prepare_with_overlay(size, zoom, input, std::iter::empty())
    }

    /// The overlay is already laid out in physical pixels.
    pub fn prepare_with_overlay<'a>(
        &mut self,
        size: (u32, u32),
        zoom: u8,
        input: impl IntoIterator<Item = WorldDraw<'a>>,
        overlay: impl IntoIterator<Item = WorldDraw<'a>>,
    ) -> Result<Preparation, String> {
        if !(1..=3).contains(&zoom) {
            return Err("zoom must be 1, 2 or 3".into());
        }
        self.prepared = false;
        self.check_health()?;
        let start = Instant::now();
        self.frame += 1;
        self.commands.clear();
        self.draws.clear();
        self.columns.clear();
        let mut stats = Preparation::default();
        self.textures.retain(|_, texture| {
            if texture.asset.strong_count() != 0 {
                true
            } else {
                self.texture_bytes -= texture.bytes;
                stats.retired += 1;
                false
            }
        });
        let mut missing = BTreeMap::new();
        let mut visible_bytes = 0;
        let limit = self.device.limits().max_texture_dimension_2d;
        for (draw, overlay) in input
            .into_iter()
            .map(|draw| (draw, false))
            .chain(overlay.into_iter().map(|draw| (draw, true)))
        {
            let blend = Blend::for_modulation(draw.modulation);
            if self.draws.len() == MAX_DRAWS {
                return Err("GPU frame exceeds the draw limit".into());
            }
            let id = Arc::as_ptr(draw.asset) as usize;
            if let Some(entry) = self.textures.get_mut(&id) {
                if entry.used != self.frame {
                    visible_bytes += entry.bytes;
                    stats.reused += 1;
                    entry.used = self.frame;
                }
            } else if let std::collections::btree_map::Entry::Vacant(entry) = missing.entry(id) {
                let bytes = sprite_bytes(draw.asset, limit)?;
                visible_bytes += bytes;
                stats.uploaded_bytes += bytes;
                entry.insert(draw.asset);
            }
            let mut data = instance_data(&draw, &mut self.columns)?;
            if overlay {
                // The shader uses the sign to bypass camera zoom without
                // adding another field to every sprite instance.
                data.rect[3] = -data.rect[3];
            }

            let index = self.draws.len() as u32;
            self.draws.push(data);
            match self.commands.last_mut() {
                Some((previous, previous_blend, range))
                    if *previous == id && *previous_blend == blend =>
                {
                    range.end += 1
                }
                _ => self.commands.push((id, blend, index..index + 1)),
            }
        }
        stats.uploaded = missing.len();
        if visible_bytes > self.texture_budget.0
            || stats.reused + stats.uploaded > self.texture_budget.1
        {
            return Err("visible GPU textures exceed the cache budget".into());
        }

        // Protect every visible hit before making room for any misses.
        stats.evicted = self.make_room(stats.uploaded, stats.uploaded_bytes);
        stats.commands = start.elapsed();
        let start = Instant::now();
        for (id, asset) in missing {
            self.upload(id, asset);
        }
        self.queue.write_buffer(
            &self.viewport,
            0,
            bytes_of(&[size.0 as f32, size.1 as f32, f32::from(zoom), 0.]),
        );
        if !self.draws.is_empty() {
            self.queue
                .write_buffer(&self.instances, 0, cast_slice(&self.draws));
        }
        if !self.columns.is_empty() {
            self.queue
                .write_buffer(&self.column_buffer, 0, cast_slice(&self.columns));
        }
        stats.uploads = start.elapsed();
        stats.texture_count = self.textures.len();
        stats.texture_bytes = self.texture_bytes;
        stats.draws = self.draws.len();
        stats.draw_calls = self.commands.len();
        self.check_health()?;
        self.prepared = true;
        Ok(stats)
    }

    fn make_room(&mut self, count: usize, bytes: usize) -> usize {
        if self.texture_bytes + bytes <= self.texture_budget.0
            && self.textures.len() + count <= self.texture_budget.1
        {
            return 0;
        }
        let mut inactive: Vec<_> = self
            .textures
            .iter()
            .filter(|(_, entry)| entry.used != self.frame)
            .map(|(&id, entry)| (entry.used, id))
            .collect();
        inactive.sort_unstable();
        let mut evicted = 0;
        for (_, id) in inactive {
            self.texture_bytes -= self.textures.remove(&id).unwrap().bytes;
            evicted += 1;
            if self.texture_bytes + bytes <= self.texture_budget.0
                && self.textures.len() + count <= self.texture_budget.1
            {
                break;
            }
        }
        evicted
    }

    fn upload(&mut self, id: TextureId, asset: &Arc<LoadedFrame>) {
        let frame = &asset.decoded.frame;
        let bytes = size_of_val(frame.pixels.as_slice());
        let texture = self.device.create_texture(&TextureDescriptor {
            label: Some(&asset.source.virtual_path),
            size: extent((frame.width, frame.height)),
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8Unorm,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.queue.write_texture(
            texture.as_image_copy(),
            cast_slice(&frame.pixels),
            TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(frame.width * 4),
                rows_per_image: None,
            },
            extent((frame.width, frame.height)),
        );
        let view = texture.create_view(&TextureViewDescriptor::default());
        let bind = self.device.create_bind_group(&BindGroupDescriptor {
            label: None,
            layout: &self.texture_layout,
            entries: &[BindGroupEntry {
                binding: 0,
                resource: BindingResource::TextureView(&view),
            }],
        });
        self.textures.insert(
            id,
            SpriteTexture {
                asset: Arc::downgrade(asset),
                bind,
                bytes,
                used: self.frame,
            },
        );
        self.texture_bytes += bytes;
    }

    pub fn encode(
        &self,
        encoder: &mut CommandEncoder,
        target: &TextureView,
        background: [u8; 3],
    ) -> Result<(), String> {
        if !self.prepared {
            return Err("GPU encode requires a successful frame preparation".into());
        }
        let [r, g, b] = background.map(|channel| f64::from(channel) / 255.);
        {
            let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("Arcanum sprites"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: target,
                    resolve_target: None,
                    depth_slice: None,
                    ops: Operations {
                        load: LoadOp::Clear(Color { r, g, b, a: 1. }),
                        store: StoreOp::Store,
                    },
                })],
                timestamp_writes: self.timestamp_query.as_ref().map(|timing| {
                    RenderPassTimestampWrites {
                        query_set: &timing.query,
                        beginning_of_pass_write_index: Some(0),
                        end_of_pass_write_index: Some(1),
                    }
                }),
                ..Default::default()
            });
            pass.set_bind_group(0, &self.globals, &[]);
            pass.set_vertex_buffer(0, self.instances.slice(..));
            let mut active = None;
            for (id, blend, range) in &self.commands {
                if active != Some(*blend) {
                    pass.set_pipeline(match blend {
                        Blend::SourceOver => &self.over,
                        Blend::Add => &self.add,
                        Blend::Subtract => &self.subtract,
                    });
                    active = Some(*blend);
                }
                pass.set_bind_group(1, &self.textures[id].bind, &[]);
                pass.draw(0..6, range.clone());
            }
        }
        if let Some(timing) = &self.timestamp_query {
            encoder.resolve_query_set(&timing.query, 0..2, &timing.resolve, 0);
            encoder.copy_buffer_to_buffer(&timing.resolve, 0, &timing.read, 0, 16);
        }
        Ok(())
    }

    /// Explicitly waits for completion. Only used by traced benchmark frames.
    pub fn complete(&self, submission: SubmissionIndex) -> Result<Option<Duration>, String> {
        let Some(timing) = &self.timestamp_query else {
            self.device
                .poll(PollType::wait_for(submission))
                .map_err(|e| e.to_string())?;
            return Ok(None);
        };
        let data = read_buffer(&self.device, &timing.read, submission)?;
        let begin = u64::from_ne_bytes(data[..8].try_into().unwrap());
        let end = u64::from_ne_bytes(data[8..].try_into().unwrap());
        let nanos = end.saturating_sub(begin) as f64 * f64::from(self.queue.get_timestamp_period());
        Ok((nanos > 0.).then(|| Duration::from_secs_f64(nanos / 1e9)))
    }
}

fn sprite_bytes(asset: &LoadedFrame, limit: u32) -> Result<usize, String> {
    let frame = &asset.decoded.frame;
    if frame.width == 0
        || frame.height == 0
        || frame.width > limit
        || frame.height > limit
        || u64::from(frame.width) * u64::from(frame.height) != frame.pixels.len() as u64
    {
        return Err(format!(
            "invalid GPU sprite dimensions for {}",
            asset.source.virtual_path
        ));
    }
    Ok(size_of_val(frame.pixels.as_slice()))
}

fn instance_data(draw: &WorldDraw<'_>, columns: &mut Vec<u32>) -> Result<InstanceData, String> {
    let frame = draw.sprite.frame;
    if !std::ptr::eq(frame, &draw.asset.decoded.frame) {
        return Err("GPU sprite identity does not match its decoded frame".into());
    }
    let (x, y) = sprite_origin(frame, draw.sprite.anchor);
    let mut data = InstanceData {
        rect: [x as f32, y as f32, frame.width as f32, frame.height as f32],
        ..InstanceData::zeroed()
    };
    let pack = |[r, g, b]: [u8; 3]| u32::from_le_bytes([r, g, b, 0]);
    match draw.modulation {
        Modulation::Unlit | Modulation::Additive => {}
        Modulation::Shadow(gray) => data.shade = [1, pack([gray; 3]), 0, 0],
        Modulation::Uniform(rgb) | Modulation::AdditiveTint(rgb) => {
            data.shade = [1, pack(rgb), 0, 0]
        }
        Modulation::Floor(nodes) => {
            if (frame.width, frame.height) != (78, 40) {
                return Err("floor modulation requires a 78 × 40 sprite".into());
            }
            let nodes = nodes.map(pack);
            data.shade = [2, 0, nodes[8], 0];
            data.nodes_a.copy_from_slice(&nodes[..4]);
            data.nodes_b.copy_from_slice(&nodes[4..8]);
        }
        Modulation::Fade {
            corners,
            color,
            columns: colors,
        } => {
            data.shade = [4, pack(color), u32::from_le_bytes(corners), 0];
            if !colors.is_empty() {
                if colors.len() != frame.width as usize
                    || columns.len() + colors.len() > MAX_COLUMNS
                {
                    return Err("invalid GPU fade column modulation".into());
                }
                data.shade[1] = columns.len() as u32;
                data.shade[3] = 1;
                columns.extend(colors.iter().copied().map(pack));
            }
        }
        Modulation::Columns(colors) => {
            if colors.len() != frame.width as usize || columns.len() + colors.len() > MAX_COLUMNS {
                return Err("invalid or oversized GPU column modulation".into());
            }
            data.shade = [3, columns.len() as u32, 0, 0];
            columns.extend(colors.iter().copied().map(pack));
        }
    }
    Ok(data)
}

fn pipeline(
    device: &Device,
    layout: &PipelineLayout,
    shader: &ShaderModule,
    format: TextureFormat,
    blend: BlendState,
) -> RenderPipeline {
    let attributes =
        vertex_attr_array![0 => Float32x4, 1 => Uint32x4, 2 => Uint32x4, 3 => Uint32x4];
    device.create_render_pipeline(&RenderPipelineDescriptor {
        label: Some("ordered sprite quads"),
        layout: Some(layout),
        vertex: VertexState {
            module: shader,
            entry_point: Some("vs"),
            compilation_options: Default::default(),
            buffers: &[VertexBufferLayout {
                array_stride: size_of::<InstanceData>() as u64,
                step_mode: VertexStepMode::Instance,
                attributes: &attributes,
            }],
        },
        primitive: PrimitiveState::default(),
        depth_stencil: None,
        multisample: MultisampleState::default(),
        fragment: Some(FragmentState {
            module: shader,
            entry_point: Some("fs"),
            compilation_options: Default::default(),
            targets: &[Some(ColorTargetState {
                format,
                blend: Some(blend),
                write_mask: ColorWrites::COLOR,
            })],
        }),
        multiview: None,
        cache: None,
    })
}

fn buffer_binding(
    binding: u32,
    visibility: ShaderStages,
    ty: BufferBindingType,
) -> BindGroupLayoutEntry {
    BindGroupLayoutEntry {
        binding,
        visibility,
        count: None,
        ty: BindingType::Buffer {
            ty,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
    }
}

fn extent((width, height): (u32, u32)) -> Extent3d {
    Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    }
}

fn buffer(device: &Device, label: &str, size: u64, usage: BufferUsages) -> Buffer {
    device.create_buffer(&BufferDescriptor {
        label: Some(label),
        size,
        usage,
        mapped_at_creation: false,
    })
}

struct Timestamps {
    query: QuerySet,
    resolve: Buffer,
    read: Buffer,
}

impl Timestamps {
    fn new(device: &Device) -> Self {
        Self {
            query: device.create_query_set(&QuerySetDescriptor {
                label: Some("GPU sprite pass timestamps"),
                ty: QueryType::Timestamp,
                count: 2,
            }),
            resolve: buffer(
                device,
                "timestamp resolve",
                16,
                BufferUsages::QUERY_RESOLVE | BufferUsages::COPY_SRC,
            ),
            read: buffer(
                device,
                "timestamp readback",
                16,
                BufferUsages::COPY_DST | BufferUsages::MAP_READ,
            ),
        }
    }
}

fn read_buffer(
    device: &Device,
    buffer: &Buffer,
    submission: SubmissionIndex,
) -> Result<Vec<u8>, String> {
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    buffer.slice(..).map_async(MapMode::Read, move |result| {
        let _ = tx.send(result);
    });
    device
        .poll(PollType::wait_for(submission))
        .map_err(|error| error.to_string())?;
    rx.recv()
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())?;
    let bytes = buffer.slice(..).get_mapped_range().to_vec();
    buffer.unmap();
    Ok(bytes)
}

#[cfg(test)]
mod tests;
