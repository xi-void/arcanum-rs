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
fn installed_world_matches_cpu_across_maps_resizing_and_eviction() {
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
        (
            "k'na tha",
            (12, 12),
            (12, 12),
            (798, 786),
            (64, 16),
            Some(0),
        ),
        (
            "arcanum1-024-fixed",
            (1434, 613),
            (1436, 615),
            (91897, 39313),
            (64, 16),
            None,
        ),
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
        let (home, _) = compare_world_frame(&mut gpu, &mut world, &camera, size);
        if first == (12, 12) {
            let decodes = world.stats().art.decodes;
            let reads = world.stats().vfs_reads;
            let (next, different) = compare_world_tick(
                &mut gpu,
                &mut world,
                &camera,
                size,
                Duration::from_millis(90),
            );
            assert!(different.uploaded > 0);
            assert_ne!(home, next);
            let (same, warm) = compare_world_tick(
                &mut gpu,
                &mut world,
                &camera,
                size,
                Duration::from_millis(1890),
            );
            assert_eq!(next, same);
            assert_eq!(warm.uploaded, 0);
            assert_eq!(world.stats().art.decodes, decodes);
            assert_eq!(world.stats().vfs_reads, reads);
        }

        camera.pan(offset, size);
        compare_world_frame(&mut gpu, &mut world, &camera, size);
        if first == (5, 14) {
            assert_eq!(world.reports().count(), 2);
        }
        if map.starts_with("bates") {
            assert_eq!(world.reports().next().unwrap().portal_count, 19);
        }
        let small = (641, 407);
        camera.resize(small);
        compare_world_frame(&mut gpu, &mut world, &camera, small);
        camera.reset(size);
        assert_eq!(
            home,
            compare_world_frame(&mut gpu, &mut world, &camera, size).0
        );
    }

    let region = SectorRegion {
        first: Coordinates { x: 5, y: 14 },
        last: Coordinates { x: 18, y: 14 },
    };
    let center = (341, 928);
    let size = (1280, 720);
    let mut camera = region.camera(center).unwrap();
    let mut world = WorldStream::new(
        Vfs::from_game(&game, Policy::Ce).unwrap(),
        "k'na tha".into(),
        region,
        center,
    )
    .unwrap();
    let (home, _) = compare_world_frame(&mut gpu, &mut world, &camera, size);
    for _ in 5..18 {
        camera.pan((-2560, 1280), size);
        compare_world_frame(&mut gpu, &mut world, &camera, size);
    }
    let stats = world.stats();
    assert_eq!(stats.cached_sectors, 12);
    assert!(stats.sector_evictions >= 2, "{stats:?}");
    camera.reset(size);
    let reads = stats.sector_reads;
    let (returned, gpu_stats) = compare_world_frame(&mut gpu, &mut world, &camera, size);
    assert_eq!(home, returned);
    let returned_stats = world.stats();
    assert!(returned_stats.sector_reads > reads);
    assert_eq!(returned_stats.art.decodes, stats.art.decodes);
    assert_eq!(gpu_stats.uploaded, 0);
    assert!(gpu_stats.reused > 0);

    // Reference (341, 928) plus 234 tiles in X puts the camera at (575, 928),
    // where the floor runs up to the original missing sector (9, 14).
    camera.pan((-40 * 234, 20 * 234), size);
    assert_eq!(camera.position(), (-9360, 4680));
    let (edge, _) = compare_world_frame(&mut gpu, &mut world, &camera, size);
    let reports: BTreeMap<_, _> = world
        .reports()
        .map(|report| (Coordinates::from_id(report.id).unwrap().x, report))
        .collect();
    assert_eq!(reports[&8].surfaces, 4096);
    assert!(reports[&8].source.is_some());
    assert_eq!(reports[&9].surfaces, 0);
    assert!(reports[&9].source.is_none());
    assert!(
        reports[&9]
            .diagnostics
            .iter()
            .any(|line| line.contains("resource \"maps/k'na tha/939524105.sec\" not found"))
    );
    assert_eq!(edge[360 * 1280 + 198], [32, 43, 58]);
    assert_ne!(edge[360 * 1280 + 1100], [32, 43, 58]);
}

fn compare_world_frame(
    gpu: &mut Compositor,
    world: &mut crate::content::world::WorldStream,
    camera: &crate::camera::Camera,
    size: (u32, u32),
) -> (Vec<[u8; 3]>, Preparation) {
    compare_world_tick(gpu, world, camera, size, Duration::ZERO)
}

fn compare_world_tick(
    gpu: &mut Compositor,
    world: &mut crate::content::world::WorldStream,
    camera: &crate::camera::Camera,
    size: (u32, u32),
    tick: Duration,
) -> (Vec<[u8; 3]>, Preparation) {
    world.prepare(camera, size).unwrap();
    let background = [32, 43, 58];
    let mut cpu = vec![background; (size.0 * size.1) as usize];
    blit_modulated(
        &mut cpu,
        size,
        size.0 as usize,
        world
            .visible_assets_at(camera, size, tick)
            .map(|d| (d.sprite, d.modulation)),
    )
    .unwrap();
    let (rendered, stats) = read_frame(
        gpu,
        size,
        background,
        world.visible_assets_at(camera, size, tick).map(|d| {
            let blend = Blend::for_modulation(d.modulation);
            (d, blend)
        }),
    );
    assert!(
        cpu == rendered,
        "{size:?} camera {:?}: {} different pixels",
        camera.position(),
        cpu.iter().zip(&rendered).filter(|(a, b)| a != b).count()
    );
    (rendered, stats)
}

#[test]
#[ignore = "requires a hardware GPU adapter"]
fn texture_pressure_protects_visible_hits_and_retires_dead_assets() {
    let backend = test_backend();
    let instance = instance(backend);
    let adapter = hardware_adapter(&instance, backend, None).unwrap();
    let mut gpu = Compositor::new(&adapter, TextureFormat::Rgba8Unorm, false).unwrap();
    gpu.texture_budget = (8, 2);
    let size = (3, 1);
    let lingering = fixture(1, 1, vec![[255, 0, 0, 255]]);
    let dormant = fixture(1, 1, vec![[0, 255, 0, 255]]);
    let arriving = fixture(1, 1, vec![[0, 0, 255, 255]]);
    fn at(x: i32, asset: &Arc<LoadedFrame>) -> (WorldDraw<'_>, Blend) {
        (draw(asset, (x, 1), Modulation::Unlit), Blend::SourceOver)
    }
    read_frame(&mut gpu, size, [0; 3], [at(1, &lingering)]);
    read_frame(&mut gpu, size, [0; 3], [at(2, &dormant)]);

    // The oldest texture is needed by a later draw in this frame.
    let (pixels, stats) = read_frame(
        &mut gpu,
        size,
        [0; 3],
        [at(3, &arriving), at(1, &lingering), at(1, &lingering)],
    );
    assert_eq!(pixels, [[255, 0, 0], [0; 3], [0, 0, 255]]);
    assert_eq!((stats.uploaded, stats.reused, stats.evicted), (1, 1, 1));
    let error = gpu
        .prepare(size, [at(1, &lingering), at(2, &dormant), at(3, &arriving)])
        .unwrap_err();
    assert!(error.contains("visible GPU textures exceed"));
    let (_, stats) = read_frame(
        &mut gpu,
        size,
        [0; 3],
        [at(1, &lingering), at(3, &arriving)],
    );
    assert_eq!((stats.uploaded, stats.reused), (0, 2));

    drop(lingering);
    let (_, stats) = read_frame(&mut gpu, size, [0; 3], [at(3, &arriving)]);
    assert_eq!(
        (
            stats.retired,
            stats.evicted,
            stats.texture_count,
            stats.texture_bytes
        ),
        (1, 0, 1, 4)
    );
}

#[test]
#[ignore = "requires a hardware GPU adapter"]
fn deliberate_device_destruction_reports_loss() {
    let backend = test_backend();
    let instance = instance(backend);
    let adapter = hardware_adapter(&instance, backend, None).unwrap();
    let mut gpu = Compositor::new(&adapter, TextureFormat::Rgba8Unorm, false).unwrap();
    gpu.device.destroy();
    let _ = gpu.device.poll(PollType::Wait);
    let error = gpu.prepare((1, 1), []).unwrap_err();
    assert!(error.contains("GPU device lost"), "{error}");
}
