use super::*;
use crate::{
    art::{DecodedFrame, SpriteFrame},
    compositor::{SpriteDraw, blit_modulated},
    content::Source,
    vfs::{Policy, Storage, Vfs},
};

fn fixture(width: u32, height: u32, pixels: Vec<[u8; 4]>) -> Arc<LoadedFrame> {
    let mount_id = Vfs::new(Policy::Ordered).mount_directory(".").unwrap();
    Arc::new(LoadedFrame {
        source: Source {
            virtual_path: "synthetic.art".into(),
            mount_id,
            mount_path: ".".into(),
            loose_path: None,
            storage: Storage::Loose,
            mod_id: None,
        },
        decoded: DecodedFrame {
            frame: SpriteFrame {
                width,
                height,
                hotspot: (1, 1),
                offset: (0, 0),
                pixels,
            },
            frame_count: 1,
            rotation_count: 1,
            stored_rotation: 0,
            stored_palette: 0,
            palette_key: [0; 3],
            frames_per_second: 0,
            action_frame: 0,
            ignored_trailing_bytes: 0,
        },
        input_bytes: 0,
    })
}

fn draw<'a>(
    asset: &'a Arc<LoadedFrame>,
    anchor: (i32, i32),
    modulation: Modulation<'a>,
) -> WorldDraw<'a> {
    WorldDraw {
        asset,
        sprite: SpriteDraw {
            frame: &asset.decoded.frame,
            anchor,
        },
        modulation,
    }
}

#[test]
fn uploads_screen_column_tints_and_floor_nodes_in_byte_order() {
    let asset = fixture(3, 1, vec![[0; 4]; 3]);
    let mut columns = vec![42];
    let draw = draw(&asset, (-1, 4), Modulation::Columns(&[[1, 2, 3]; 3]));
    let data = instance_data(&draw, &mut columns).unwrap();
    assert_eq!(data.rect, [-2., 3., 3., 1.]);
    assert_eq!(data.shade, [3, 1, 0, 0]);
    assert_eq!(columns, [42, 0x030201, 0x030201, 0x030201]);
}

fn test_backend() -> Backends {
    match std::env::var("ARCANUM_GPU_BACKEND").as_deref() {
        Ok("gles") => Backends::GL,
        _ => Backends::VULKAN,
    }
}

fn read_frame<'a>(
    gpu: &mut Compositor,
    size: (u32, u32),
    bg: [u8; 3],
    draws: impl IntoIterator<Item = (WorldDraw<'a>, Blend)>,
) -> (Vec<[u8; 3]>, Preparation) {
    let stats = gpu.prepare(size, draws).unwrap();
    let target = gpu.device.create_texture(&TextureDescriptor {
        label: Some("GPU verification target"),
        size: extent(size),
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: TextureFormat::Rgba8Unorm,
        usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let stride = (size.0 * 4).next_multiple_of(COPY_BYTES_PER_ROW_ALIGNMENT);
    let copy = buffer(
        &gpu.device,
        "GPU verification read",
        u64::from(stride) * u64::from(size.1),
        BufferUsages::COPY_DST | BufferUsages::MAP_READ,
    );
    let mut encoder = gpu
        .device
        .create_command_encoder(&CommandEncoderDescriptor::default());
    gpu.encode(
        &mut encoder,
        &target.create_view(&TextureViewDescriptor::default()),
        bg,
    );
    encoder.copy_texture_to_buffer(
        target.as_image_copy(),
        TexelCopyBufferInfo {
            buffer: &copy,
            layout: TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(stride),
                rows_per_image: None,
            },
        },
        extent(size),
    );
    let index = gpu.queue.submit([encoder.finish()]);
    let bytes = read_buffer(&gpu.device, &copy, index).unwrap();
    let pixels = bytes
        .chunks_exact(stride as usize)
        .flat_map(|row| {
            row[..size.0 as usize * 4]
                .as_chunks::<4>()
                .0
                .iter()
                .map(|&[r, g, b, _]| [r, g, b])
        })
        .collect();
    (pixels, stats)
}

#[test]
#[ignore = "requires a hardware GPU adapter"]
fn hardware_matches_cpu_shading_and_saturating_additive_blending() {
    let backend = test_backend();
    let instance = instance(backend);
    let adapter = hardware_adapter(&instance, backend, None).unwrap();
    let mut gpu = Compositor::new(&adapter, TextureFormat::Rgba8Unorm, false).unwrap();
    let size = (80, 44);
    let bg = [11, 42, 67];
    let surface = fixture(
        78,
        40,
        (0..3120)
            .map(|i| [(i % 256) as u8, 55, 240, if i % 3 == 0 { 0 } else { 255 }])
            .collect(),
    );
    let front = fixture(
        3,
        2,
        vec![
            [9, 80, 200, 255],
            [33, 14, 100, 128],
            [0, 0, 0, 0],
            [128, 20, 6, 255],
            [255; 4],
            [0; 4],
        ],
    );
    let nodes = [
        [55, 70, 90],
        [110, 255, 89],
        [70; 3],
        [128; 3],
        [38, 90, 170],
        [10; 3],
        [94; 3],
        [140; 3],
        [250; 3],
    ];
    let columns = [[128; 3], [0, 128, 255], [255; 3]];
    let draws = || {
        [
            draw(&surface, (-2, 2), Modulation::Floor(nodes)),
            draw(&front, (1, 2), Modulation::Columns(&columns)),
            draw(&front, (79, 44), Modulation::Uniform([88, 140, 210])),
        ]
    };
    let mut cpu = vec![bg; (size.0 * size.1) as usize];
    blit_modulated(
        &mut cpu,
        size,
        size.0 as usize,
        draws().into_iter().map(|d| (d.sprite, d.modulation)),
    )
    .unwrap();
    let (rendered, cold) = read_frame(
        &mut gpu,
        size,
        bg,
        draws().into_iter().map(|d| (d, Blend::SourceOver)),
    );
    assert_eq!(cold.uploaded, 2);
    for (pos, (actual, reference)) in rendered.iter().zip(&cpu).enumerate() {
        let alpha_edge = matches!(pos, 81 | 3519);
        for (&actual, &reference) in actual.iter().zip(reference) {
            assert!(
                actual.abs_diff(reference) <= u8::from(alpha_edge),
                "pixel {pos}, GPU {actual}, CPU {reference}"
            );
        }
    }
    let fire = fixture(
        3,
        1,
        vec![[75, 140, 30, 255], [250, 250, 250, 0], [30, 100, 200, 255]],
    );
    let (additive, hot) = read_frame(
        &mut gpu,
        (3, 1),
        [200, 100, 80],
        [
            (draw(&fire, (1, 1), Modulation::Unlit), Blend::Add),
            (draw(&fire, (1, 1), Modulation::Unlit), Blend::Add),
        ],
    );
    assert_eq!(hot.uploaded, 1);
    assert_eq!(additive, [[255, 255, 140], [200, 100, 80], [255, 255, 255]]);
    let (_, hot) = read_frame(
        &mut gpu,
        size,
        bg,
        draws().into_iter().map(|d| (d, Blend::SourceOver)),
    );
    assert_eq!(hot.uploaded, 0);
}

#[test]
#[ignore = "requires installed Arcanum data and a hardware GPU adapter"]
fn installed_world_matches_cpu_at_bates_and_the_kna_tha_seam() {
    use crate::{
        content::world::{SectorRegion, WorldStream},
        sector::Coordinates,
    };
    let game = std::path::PathBuf::from(
        std::env::var_os("ARCANUM_GAME_DIR").expect("set ARCANUM_GAME_DIR"),
    );
    let backend = test_backend();
    let instance = instance(backend);
    let adapter = hardware_adapter(&instance, backend, None).unwrap();
    let mut gpu = Compositor::new(&adapter, TextureFormat::Rgba8Unorm, false).unwrap();
    for (map, first, last, center, offset, hour) in [
        (
            "bates mansion lev 1",
            (1, 1),
            (1, 1),
            (101, 83),
            (64, 16),
            Some(12),
        ),
        (
            "bates mansion lev 1",
            (1, 1),
            (1, 1),
            (101, 83),
            (64, 16),
            None,
        ),
        ("k'na tha", (5, 14), (6, 14), (341, 928), (-1532, 934), None),
    ] {
        let region = SectorRegion {
            first: Coordinates {
                x: first.0,
                y: first.1,
            },
            last: Coordinates {
                x: last.0,
                y: last.1,
            },
        };
        let mut camera = region.camera(center).unwrap();
        let size = (1280, 720);
        let mut world = WorldStream::new(
            Vfs::from_game(&game, Policy::Ce).unwrap(),
            map.into(),
            region,
            center,
        )
        .unwrap();
        if let Some(hour) = hour {
            world = world.with_lighting(hour).unwrap();
        }
        for pan in [(0, 0), offset, (-offset.0, -offset.1)] {
            camera.pan(pan, size);
            world.prepare(&camera, size).unwrap();
            if map == "k'na tha" && pan == offset {
                assert_eq!(world.reports().count(), 2);
            }
            if map.starts_with("bates") {
                assert_eq!(world.reports().next().unwrap().portal_count, 19);
            }
            let mut cpu = vec![[32, 43, 58]; (size.0 * size.1) as usize];
            blit_modulated(
                &mut cpu,
                size,
                size.0 as usize,
                world.visible_modulated(&camera, size),
            )
            .unwrap();
            let (rendered, _) = read_frame(
                &mut gpu,
                size,
                [32, 43, 58],
                world
                    .visible_assets(&camera, size)
                    .map(|d| (d, Blend::SourceOver)),
            );
            assert!(
                cpu == rendered,
                "{map} {hour:?} {pan:?} mismatched pixels: {}",
                cpu.iter().zip(&rendered).filter(|(a, b)| a != b).count()
            );
        }
    }
}
