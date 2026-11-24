use super::*;
use crate::{
    art::{DecodedFrame, SpriteFrame},
    compositor::SpriteDraw,
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

#[test]
#[ignore = "requires a hardware GPU adapter"]
fn roof_fade_leaves_the_background_transparent() {
    let instance = instance();
    let adapter = hardware_adapter(&instance, None).unwrap();
    let mut gpu = Compositor::new(&adapter, TextureFormat::Rgba8Unorm, false).unwrap();
    let asset = fixture(
        4,
        4,
        (0..16)
            .map(|i| [125, 16 * i, 200, if i == 2 { 0 } else { 255 }])
            .collect(),
    );
    let bg = [40, 24, 62];
    let size = (6, 6);
    let draws = || {
        [
            draw(&asset, (1, 1), Modulation::AdditiveTint([200; 3])),
            draw(
                &asset,
                (2, 2),
                Modulation::Fade {
                    corners: [80, 255, 128, 200],
                    color: [255; 3],
                    columns: &[[210, 232, 190]; 4],
                },
            ),
        ]
    };
    let (pixels, _) = read_frame(&mut gpu, size, bg, draws());
    assert_eq!(pixels[35], bg);
    assert_ne!(pixels[0], bg);
    assert_ne!(pixels[1], bg);
    assert_ne!(pixels[7], pixels[8]);
}

#[test]
#[ignore = "requires hardware GPU"]
fn integer_zoom_matches_palette_and_clipping_at_odd_viewport_sizes() {
    let context = instance();
    let adapter = hardware_adapter(&context, None).unwrap();
    let mut gpu = Compositor::new(&adapter, TextureFormat::Rgba8Unorm, false).unwrap();
    let asset = fixture(
        3,
        2,
        vec![
            [11, 77, 25, 255],
            [122, 51, 120, 255],
            [0; 4],
            [17, 27, 25, 255],
            [84, 51, 85, 255],
            [45, 12, 98, 255],
        ],
    );
    let background = [33, 40, 59];
    let size = (11u32, 7u32);
    for zoom in [1u8, 2, 3] {
        let z = u32::from(zoom);
        let draw = || draw(&asset, (0, 2), Modulation::Unlit);
        let (gpu_pixels, _) = read_frame_zoom(&mut gpu, size, zoom, background, [draw()]);
        for y in 0..size.1 {
            for x in 0..size.0 {
                assert_eq!(
                    gpu_pixels[(y * size.0 + x) as usize],
                    match (x / z, y / z) {
                        (0, 1) => [122, 51, 120],
                        (0, 2) => [84, 51, 85],
                        (1, 2) => [45, 12, 98],
                        _ => background,
                    },
                    "zoom {zoom} at ({x}, {y})"
                );
            }
        }
    }
}

fn read_target(gpu: &Compositor, size: (u32, u32)) -> Texture {
    gpu.device.create_texture(&TextureDescriptor {
        label: Some("GPU verification target"),
        size: extent(size),
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: TextureFormat::Rgba8Unorm,
        usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

fn read_frame<'a>(
    gpu: &mut Compositor,
    size: (u32, u32),
    bg: [u8; 3],
    draws: impl IntoIterator<Item = WorldDraw<'a>>,
) -> (Vec<[u8; 3]>, Preparation) {
    read_frame_zoom(gpu, size, 1, bg, draws)
}

fn read_frame_zoom<'a>(
    gpu: &mut Compositor,
    size: (u32, u32),
    zoom: u8,
    bg: [u8; 3],
    draws: impl IntoIterator<Item = WorldDraw<'a>>,
) -> (Vec<[u8; 3]>, Preparation) {
    read_frame_with_overlay(gpu, size, zoom, bg, draws, std::iter::empty())
}

fn read_frame_with_overlay<'a>(
    gpu: &mut Compositor,
    size: (u32, u32),
    zoom: u8,
    bg: [u8; 3],
    draws: impl IntoIterator<Item = WorldDraw<'a>>,
    overlay: impl IntoIterator<Item = WorldDraw<'a>>,
) -> (Vec<[u8; 3]>, Preparation) {
    let stats = gpu
        .prepare_with_overlay(size, zoom, draws, overlay)
        .unwrap();
    let target = read_target(gpu, size);
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
    )
    .unwrap();
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
fn screen_overlay_keeps_screen_coordinates_at_each_world_zoom() {
    let context = instance();
    let adapter = hardware_adapter(&context, None).unwrap();
    let mut gpu = Compositor::new(&adapter, TextureFormat::Rgba8Unorm, false).unwrap();
    let world = fixture(3, 3, vec![[80, 100, 120, 255]; 9]);
    let hud = fixture(2, 1, vec![[40, 80, 160, 255], [0; 4]]);
    let size = (8u32, 6u32);
    let bg = [10, 20, 30];
    for zoom in [1, 2, 3] {
        let (world, hud) = (
            draw(&world, (1, 1), Modulation::Unlit),
            draw(&hud, (3, 2), Modulation::Unlit),
        );
        let z = u32::from(zoom);
        let expected: Vec<_> = (0..size.1)
            .flat_map(|y| {
                (0..size.0).map(move |x| {
                    if (x, y) == (2, 1) {
                        [40, 80, 160]
                    } else if x < 3 * z && y < 3 * z {
                        [80, 100, 120]
                    } else {
                        bg
                    }
                })
            })
            .collect();
        let (actual, _) = read_frame_with_overlay(&mut gpu, size, zoom, bg, [world], [hud]);
        assert_eq!(actual, expected, "world zoom {zoom}");
    }
}

#[test]
#[ignore = "requires Vulkan hardware"]
fn map_inset_stays_resident_and_scales_without_readback() {
    let instance = instance();
    let adapter = hardware_adapter(&instance, None).unwrap();
    let mut gpu = Compositor::new(&adapter, TextureFormat::Rgba8Unorm, false).unwrap();
    let surface = fixture(4, 4, vec![[25, 75, 180, 255]; 16]);
    let inset = fixture(2, 2, Vec::new());
    gpu.render_sprite(
        &inset,
        0.5,
        [0; 3],
        [draw(&surface, (1, 1), Modulation::Unlit)],
    )
    .unwrap();
    let (pixels, stats) = read_frame_with_overlay(
        &mut gpu,
        (4, 4),
        2,
        [0; 3],
        [],
        [draw(&inset, (2, 2), Modulation::Unlit)],
    );
    assert_eq!(stats.uploaded, 0);
    assert_eq!(pixels[5..7], [[25, 75, 180]; 2]);
    assert_eq!(pixels[9..11], [[25, 75, 180]; 2]);
    assert_eq!(pixels[0], [0; 3]);
}

#[test]
#[ignore = "requires a hardware GPU adapter"]
fn vulkan_shades_and_saturates_additive_blending() {
    let instance = instance();
    let adapter = hardware_adapter(&instance, None).unwrap();
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
    let (rendered, cold) = read_frame(&mut gpu, size, bg, draws());
    assert_eq!(cold.uploaded, 2);
    assert_eq!(rendered[80], [4, 40, 100]);
    assert_eq!(rendered[79], bg);
    assert_ne!(rendered[4 * 80 + 7], bg);
    let fire = fixture(
        3,
        1,
        vec![[75, 140, 30, 255], [250, 250, 250, 0], [30, 100, 200, 255]],
    );
    let fire_draws =
        || std::array::from_fn::<_, 2, _>(|_| draw(&fire, (1, 1), Modulation::Additive));
    let background = [200, 100, 80];
    let (additive, hot) = read_frame(&mut gpu, (3, 1), background, fire_draws());
    assert_eq!(hot.uploaded, 1);
    assert_eq!(additive, [[255, 255, 140], [200, 100, 80], [255, 255, 255]]);
    let shadow_draws = || {
        [
            draw(&fire, (1, 1), Modulation::Shadow(171)),
            draw(&fire, (1, 1), Modulation::Shadow(171)),
        ]
    };
    let (subtractive, warm) = read_frame(&mut gpu, (3, 1), [200, 100, 80], shadow_draws());
    assert_eq!(warm.uploaded, 0);
    assert_eq!(subtractive, [[100, 0, 40], [200, 100, 80], [160, 0, 0]]);
    let (_, hot) = read_frame(&mut gpu, size, bg, draws());
    assert_eq!(hot.uploaded, 0);
}

#[test]
#[ignore = "requires installed Arcanum data and a hardware GPU adapter"]
fn installed_world_is_stable_across_maps_resizing_and_eviction() {
    use crate::{
        content::world::{SectorRegion, WorldStream},
        sector::Coordinates,
    };
    let game = std::path::PathBuf::from(
        std::env::var_os("ARCANUM_GAME_DIR").expect("set ARCANUM_GAME_DIR"),
    );
    let instance = instance();
    let adapter = hardware_adapter(&instance, None).unwrap();
    let mut gpu = Compositor::new(&adapter, TextureFormat::Rgba8Unorm, false).unwrap();
    for (label, map, first, last, center, offset, hour) in [
        (
            "Bates lit interior",
            "bates mansion lev 1",
            (1, 1),
            (1, 1),
            (101, 83),
            (64, 16),
            Some(12),
        ),
        (
            "Bates unlit interior",
            "bates mansion lev 1",
            (1, 1),
            (1, 1),
            (101, 83),
            (64, 16),
            None,
        ),
        (
            "K’na Tha populated seam",
            "k'na tha",
            (5, 14),
            (6, 14),
            (341, 928),
            (-1532, 934),
            None,
        ),
        (
            "K’na Tha brazier court",
            "k'na tha",
            (12, 12),
            (12, 12),
            (798, 786),
            (64, 16),
            Some(0),
        ),
        (
            "Forest Passage",
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
        let (home, _) = capture_world_frame(&mut gpu, &mut world, &camera, size);
        if first == (12, 12) {
            let decodes = world.stats().art.decodes;
            let reads = world.stats().vfs_reads;
            let (next, different) = capture_world_tick(
                &mut gpu,
                &mut world,
                &camera,
                size,
                Duration::from_millis(90),
            );
            assert!(different.uploaded > 0);
            assert_ne!(home, next);
            let (same, warm) = capture_world_tick(
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
        capture_world_frame(&mut gpu, &mut world, &camera, size);
        if first == (5, 14) {
            assert_eq!(world.reports().count(), 2);
        }
        if map.starts_with("bates") {
            assert_eq!(world.reports().next().unwrap().portal_count, 19);
        }
        let small = (641, 407);
        camera.resize(small);
        capture_world_frame(&mut gpu, &mut world, &camera, small);
        camera.reset(size);
        assert_eq!(
            home,
            capture_world_frame(&mut gpu, &mut world, &camera, size).0,
            "{label} home frame changed",
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
    let (home, _) = capture_world_frame(&mut gpu, &mut world, &camera, size);
    for _ in 5..18 {
        camera.pan((-2560, 1280), size);
        capture_world_frame(&mut gpu, &mut world, &camera, size);
    }
    let stats = world.stats();
    assert_eq!(stats.cached_sectors, 12);
    assert!(stats.sector_evictions >= 2, "{stats:?}");
    camera.reset(size);
    let reads = stats.sector_reads;
    let (returned, gpu_stats) = capture_world_frame(&mut gpu, &mut world, &camera, size);
    assert_eq!(home, returned);
    let returned_stats = world.stats();
    assert!(returned_stats.sector_reads > reads);
    assert_eq!(returned_stats.art.decodes, stats.art.decodes);
    assert_eq!(gpu_stats.uploaded, 0);
    assert!(gpu_stats.reused > 0);

    // Pan across the authored-to-template seam. The missing map .sec is
    // supplied by the original forest template encoded in terrain.tdf.
    camera.pan((-40 * 234, 20 * 234), size);
    assert_eq!(camera.position(), (-9360, 4680));
    let (edge, _) = capture_world_frame(&mut gpu, &mut world, &camera, size);
    let reports: BTreeMap<_, _> = world
        .reports()
        .map(|report| (Coordinates::from_id(report.id).unwrap().x, report))
        .collect();
    assert_eq!(reports[&8].surfaces, 4096);
    assert_eq!(reports[&9].surfaces, 4096);
    assert!(
        reports[&9]
            .source
            .as_deref()
            .unwrap()
            .starts_with("terrain/forest/67108864.sec")
    );
    assert_ne!(edge[360 * 1280 + 198], [32, 43, 58]);
    assert_ne!(edge[360 * 1280 + 1100], [32, 43, 58]);
}

#[test]
#[ignore = "requires installed Arcanum data and a hardware GPU adapter"]
fn dynamic_lights_change_pixels_and_reuse_warm_resources() {
    use crate::{
        content::world::{SectorRegion, WorldStream},
        sector::Coordinates,
    };
    let game = std::path::PathBuf::from(
        std::env::var_os("ARCANUM_GAME_DIR").expect("set ARCANUM_GAME_DIR"),
    );
    let context = instance();
    let adapter = hardware_adapter(&context, None).unwrap();
    let mut gpu = Compositor::new(&adapter, TextureFormat::Rgba8Unorm, false).unwrap();
    let sector = Coordinates { x: 1, y: 1 };
    let region = SectorRegion {
        first: sector,
        last: sector,
    };
    let center = (101, 83);
    let mut camera = region.camera(center).unwrap();
    let size = (1280, 720);
    let tick = Duration::from_millis(90);
    let mut world = WorldStream::new(
        Vfs::from_game(&game, Policy::Ce).unwrap(),
        "bates mansion lev 1".into(),
        region,
        center,
    )
    .unwrap()
    .with_lighting(12)
    .unwrap();
    world.add_light_probe((101, 83), false).unwrap();
    world.add_light_probe((102, 83), false).unwrap();
    let first = world.dynamic_light(0).unwrap();
    let second = world.dynamic_light(1).unwrap();
    let mut second_off = second;
    second_off.active = false;
    world.set_dynamic_light(1, second_off).unwrap();
    let (single, _) = capture_world_tick(&mut gpu, &mut world, &camera, size, tick);
    let warm = world.stats();
    let update = world.set_dynamic_light(1, second).unwrap();
    assert!((1..1000).contains(&update.receivers));
    let (overlap, stats) = capture_world_tick(&mut gpu, &mut world, &camera, size, tick);
    assert_ne!(single, overlap);
    assert_eq!(stats.uploaded, 0);
    let mut moved = first;
    moved.tile = (104, 84);
    assert!(world.set_dynamic_light(0, moved).unwrap().receivers > 0);
    let (relocated, stats) = capture_world_tick(&mut gpu, &mut world, &camera, size, tick);
    assert_ne!(overlap, relocated);
    assert_eq!(stats.uploaded, 0);
    let after = world.stats();
    assert_eq!(
        (after.sector_reads, after.vfs_reads, after.art.decodes),
        (warm.sector_reads, warm.vfs_reads, warm.art.decodes)
    );
    let later = Duration::from_millis(125);
    let (advancing, _) = capture_world_tick(&mut gpu, &mut world, &camera, size, later);
    assert_ne!(relocated, advancing);
    let wrapped = Duration::from_millis(2625);
    let (wrapped_pixels, warm_animation) =
        capture_world_tick(&mut gpu, &mut world, &camera, size, wrapped);
    assert_eq!(advancing, wrapped_pixels);
    assert_eq!(warm_animation.uploaded, 0);
    camera.pan((64, 32), size);
    capture_world_tick(&mut gpu, &mut world, &camera, size, tick);
    camera.reset(size);
    assert_eq!(
        relocated,
        capture_world_tick(&mut gpu, &mut world, &camera, size, tick).0
    );
    world.set_dynamic_light(0, first).unwrap();
    world.set_dynamic_light(1, second_off).unwrap();
    assert_eq!(
        single,
        capture_world_tick(&mut gpu, &mut world, &camera, size, tick).0
    );
}

#[test]
#[ignore = "requires installed original data and a hardware GPU"]
fn playable_avatar_draws_original_shadows() {
    use crate::{
        content::world::{SectorRegion, WorldStream},
        sector::Coordinates,
    };
    let game = std::env::var_os("ARCANUM_GAME_DIR").expect("set ARCANUM_GAME_DIR");
    let context = instance();
    let adapter = hardware_adapter(&context, None).unwrap();
    let mut gpu = Compositor::new(&adapter, TextureFormat::Rgba8Unorm, false).unwrap();
    let sector = Coordinates { x: 1, y: 1 };
    let region = SectorRegion {
        first: sector,
        last: sector,
    };
    let center = (86, 85);
    let camera = region.camera(center).unwrap();
    let mut world = WorldStream::new(
        Vfs::from_game(game, Policy::Ce).unwrap(),
        "bates mansion lev 1".into(),
        region,
        center,
    )
    .unwrap()
    .with_starting_lighting()
    .unwrap()
    .with_player()
    .unwrap();
    let size = (1280, 720);
    let tick = Duration::from_millis(246);
    let lit = capture_world_tick(&mut gpu, &mut world, &camera, size, tick).0;
    assert_eq!(world.toggle_shadows(), Some(false));
    let without = capture_world_tick(&mut gpu, &mut world, &camera, size, tick).0;
    assert_ne!(lit, without);
    assert_eq!(world.toggle_shadows(), Some(true));
    assert_eq!(
        lit,
        capture_world_tick(&mut gpu, &mut world, &camera, size, tick).0
    );
}

fn capture_world_frame(
    gpu: &mut Compositor,
    world: &mut crate::content::world::WorldStream,
    camera: &crate::camera::Camera,
    size: (u32, u32),
) -> (Vec<[u8; 3]>, Preparation) {
    capture_world_tick(gpu, world, camera, size, Duration::ZERO)
}

#[test]
#[ignore = "requires installed original data and a hardware GPU"]
fn playable_tarant_gate_and_roofs_survive_pan_and_resize() {
    use crate::{
        content::world::{SectorRegion, WorldStream},
        sector::Coordinates,
    };
    let game = std::env::var_os("ARCANUM_GAME_DIR").expect("set ARCANUM_GAME_DIR");
    let context = instance();
    let adapter = hardware_adapter(&context, None).unwrap();
    let mut gpu = Compositor::new(&adapter, TextureFormat::Rgba8Unorm, false).unwrap();
    let region = SectorRegion {
        first: Coordinates { x: 967, y: 1025 },
        last: Coordinates { x: 970, y: 1027 },
    };
    let center = (61997, 65660);
    let mut camera = region.camera(center).unwrap();
    let mut world = WorldStream::new(
        Vfs::from_game(game, Policy::Ce).unwrap(),
        "arcanum1-024-fixed".into(),
        region,
        center,
    )
    .unwrap()
    .with_starting_lighting()
    .unwrap()
    .with_player()
    .unwrap();
    let size = (1280, 720);
    let tick = Duration::from_millis(246);
    let first = capture_world_tick(&mut gpu, &mut world, &camera, size, tick).0;
    camera.follow((150, 320));
    capture_world_tick(&mut gpu, &mut world, &camera, (853, 521), tick);
    camera.follow((0, 0));
    assert_eq!(
        first,
        capture_world_tick(&mut gpu, &mut world, &camera, size, tick).0
    );
}

fn capture_world_tick(
    gpu: &mut Compositor,
    world: &mut crate::content::world::WorldStream,
    camera: &crate::camera::Camera,
    size: (u32, u32),
    tick: Duration,
) -> (Vec<[u8; 3]>, Preparation) {
    world.prepare(camera, size).unwrap();
    world.update_walk(tick);
    let background = [32, 43, 58];
    let rendered = read_frame(
        gpu,
        size,
        background,
        world.visible_assets_at(camera, size, tick),
    );
    assert!(rendered.1.draws > 0);
    rendered
}

#[test]
#[ignore = "requires a hardware GPU adapter"]
fn texture_pressure_protects_visible_hits_and_retires_dead_assets() {
    let instance = instance();
    let adapter = hardware_adapter(&instance, None).unwrap();
    let mut gpu = Compositor::new(&adapter, TextureFormat::Rgba8Unorm, false).unwrap();
    gpu.texture_budget = (8, 2);
    let size = (3, 1);
    let lingering = fixture(1, 1, vec![[255, 0, 0, 255]]);
    let dormant = fixture(1, 1, vec![[0, 255, 0, 255]]);
    let arriving = fixture(1, 1, vec![[0, 0, 255, 255]]);
    fn at(x: i32, asset: &Arc<LoadedFrame>) -> WorldDraw<'_> {
        draw(asset, (x, 1), Modulation::Unlit)
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
fn failed_frame_cannot_encode_partial_commands_and_next_frame_recovers() {
    let instance = instance();
    let adapter = hardware_adapter(&instance, None).unwrap();
    let mut gpu = Compositor::new(&adapter, TextureFormat::Rgba8Unorm, false).unwrap();
    let size = (2, 1);
    let bg = [0; 3];
    let cached = fixture(1, 1, vec![[255, 0, 0, 255]]);
    let pending = fixture(1, 1, vec![[0, 0, 255, 255]]);
    let target = read_target(&gpu, size).create_view(&TextureViewDescriptor::default());
    let mut encoder = gpu
        .device
        .create_command_encoder(&CommandEncoderDescriptor::default());
    assert!(gpu.encode(&mut encoder, &target, bg).is_err());
    read_frame(
        &mut gpu,
        size,
        bg,
        [draw(&cached, (1, 1), Modulation::Unlit)],
    );

    let invalid = [
        draw(&pending, (1, 1), Modulation::Unlit),
        draw(&cached, (2, 1), Modulation::Columns(&[[255; 3]; 2])),
    ];
    assert!(
        gpu.prepare(size, invalid)
            .unwrap_err()
            .contains("column modulation")
    );
    let error = gpu.encode(&mut encoder, &target, bg).unwrap_err();
    assert!(error.contains("successful frame preparation"));
    let (pixels, stats) = read_frame(
        &mut gpu,
        size,
        bg,
        [
            draw(&cached, (1, 1), Modulation::Unlit),
            draw(&pending, (2, 1), Modulation::Unlit),
        ],
    );
    assert_eq!(pixels, [[255, 0, 0], [0, 0, 255]]);
    assert_eq!((stats.reused, stats.uploaded), (1, 1));
}

#[test]
#[ignore = "requires a hardware GPU adapter"]
fn deliberate_device_destruction_reports_loss() {
    let instance = instance();
    let adapter = hardware_adapter(&instance, None).unwrap();
    let mut gpu = Compositor::new(&adapter, TextureFormat::Rgba8Unorm, false).unwrap();
    gpu.device.destroy();
    let _ = gpu.device.poll(PollType::Wait);
    let error = gpu.prepare((1, 1), []).unwrap_err();
    assert!(error.contains("GPU device lost"), "{error}");
}

#[test]
#[ignore = "requires installed Arcanum data and a hardware GPU adapter"]
fn installed_bates_shadow_pass_survives_clipping() {
    use crate::{
        content::{
            shadow::PreparedMask,
            world::{SectorRegion, WorldStream},
        },
        sector::Coordinates,
        shadow::tests::bates_fixture,
    };
    let game = std::env::var_os("ARCANUM_GAME_DIR").expect("set ARCANUM_GAME_DIR");
    let mut vfs = Vfs::from_game(game, Policy::Ce).unwrap();
    let (caster, masks) = bates_fixture(&mut vfs);
    let shadows = masks.map(|(mask, frame)| PreparedMask {
        frame: Arc::new(frame),
        anchor: (0, 0),
        mask,
    });
    let region = SectorRegion {
        first: Coordinates { x: 1, y: 1 },
        last: Coordinates { x: 1, y: 1 },
    };
    let mut camera = region.camera(caster).unwrap();
    let mut world = WorldStream::new(vfs, "bates mansion lev 1".into(), region, caster)
        .unwrap()
        .with_lighting(12)
        .unwrap();
    let instance = instance();
    let adapter = hardware_adapter(&instance, None).unwrap();
    let mut gpu = Compositor::new(&adapter, TextureFormat::Rgba8Unorm, true).unwrap();
    let time = Duration::from_millis(90);
    let bg = [32, 43, 58];
    let mut home = None;
    for (size, offset) in [
        ((1280, 720), (0, 0)),
        ((64, 48), (25, 9)),
        ((1280, 720), (0, 0)),
    ] {
        camera.reset(size);
        camera.pan(offset, size);
        world.prepare(&camera, size).unwrap();
        let draws = || world.visible_assets_with_shadows(&camera, size, time, &shadows);
        let (actual, _) = read_frame(&mut gpu, size, bg, draws());
        let (without, _) = read_frame(
            &mut gpu,
            size,
            bg,
            world.visible_assets_with_shadows(&camera, size, time, &[]),
        );
        assert_ne!(actual, without);
        if size == (1280, 720) {
            if let Some(pixels) = &home {
                assert_eq!(pixels, &actual);
            } else {
                home = Some(actual.clone());
            }
        }

        let (warm, stats) = read_frame(&mut gpu, size, bg, draws());
        assert_eq!(warm, actual);
        assert_eq!(stats.uploaded, 0);
    }
}

#[test]
#[ignore = "requires installed Arcanum data and a hardware GPU adapter"]
fn original_mobile_shadow_tracks_probe_without_reloading_art() {
    use crate::{
        content::world::{SectorRegion, WorldStream},
        sector::Coordinates,
    };
    let game = std::env::var_os("ARCANUM_GAME_DIR").expect("set ARCANUM_GAME_DIR");
    let context = instance();
    let adapter = hardware_adapter(&context, None).unwrap();
    let mut gpu = Compositor::new(&adapter, TextureFormat::Rgba8Unorm, false).unwrap();
    let sector = Coordinates { x: 1, y: 1 };
    let region = SectorRegion {
        first: sector,
        last: sector,
    };
    let center = (91, 79);
    let size = (1280, 720);
    let time = Duration::from_millis(90);
    let mut camera = region.camera(center).unwrap();
    camera.resize(size);
    let mut world = WorldStream::new(
        Vfs::from_game(game, Policy::Ce).unwrap(),
        "bates mansion lev 1".into(),
        region,
        center,
    )
    .unwrap()
    .with_lighting(12)
    .unwrap();
    world.add_light_probe((90, 79), false).unwrap();
    let start = world.dynamic_light(0).unwrap();
    let (initial, _) = capture_world_tick(&mut gpu, &mut world, &camera, size, time);
    let expected_anchor = (
        size.0 as i32 / 2 - camera.position().0,
        size.1 as i32 / 2 - camera.position().1,
    );
    let draws: Vec<_> = world.visible_assets_at(&camera, size, time).collect();
    let mobile = draws
        .iter()
        .position(|d| {
            d.asset.source.virtual_path == "art/critter/hmm/hmmcdxaa.art"
                && d.sprite.anchor == expected_anchor
        })
        .expect("verified dining-room mobile");
    assert_eq!(draws[mobile].sprite.frame.hotspot, (12, 74));
    let shadow = draws
        .iter()
        .rposition(|d| matches!(d.modulation, Modulation::Shadow(_)))
        .unwrap();
    assert!(shadow < mobile);
    let cold = world.stats();
    assert_eq!(world.toggle_probe_shadow(0), Some((false, true)));
    let (excluded, _) = capture_world_tick(&mut gpu, &mut world, &camera, size, time);
    assert_ne!(initial, excluded);
    assert_eq!(world.toggle_probe_shadow(0), Some((true, true)));
    assert_eq!(
        initial,
        capture_world_tick(&mut gpu, &mut world, &camera, size, time).0
    );
    assert_eq!(world.toggle_shadows(), Some(false));
    let (unshadowed, _) = capture_world_tick(&mut gpu, &mut world, &camera, size, time);
    assert_ne!(initial, unshadowed);
    assert_eq!(world.toggle_shadows(), Some(true));
    let moved = crate::content::world::DynamicLightState {
        tile: (91, 78),
        ..start
    };
    assert!(world.set_dynamic_light(0, moved).unwrap().shadow_changed);
    let (relocated, _) = capture_world_tick(&mut gpu, &mut world, &camera, size, time);
    assert_ne!(relocated, initial);
    let off = crate::content::world::DynamicLightState {
        active: false,
        ..moved
    };
    assert!(world.set_dynamic_light(0, off).unwrap().shadow_changed);
    assert_ne!(
        relocated,
        capture_world_tick(&mut gpu, &mut world, &camera, size, time).0
    );
    world.set_dynamic_light(0, moved).unwrap();
    let (again, warm) = capture_world_tick(&mut gpu, &mut world, &camera, size, time);
    assert_eq!(relocated, again);
    assert_eq!(warm.uploaded, 0);
    camera.pan((64, 32), size);
    capture_world_tick(&mut gpu, &mut world, &camera, (960, 560), time);
    camera.reset(size);
    assert_eq!(
        relocated,
        capture_world_tick(&mut gpu, &mut world, &camera, size, time).0
    );
    let after = world.stats();
    assert_eq!(
        (after.sector_reads, after.vfs_reads, after.art.decodes),
        (cold.sector_reads, cold.vfs_reads, cold.art.decodes)
    );
}

#[test]
#[ignore = "requires installed Arcanum data and a hardware GPU adapter"]
fn original_rabbit_fidget_is_finite_and_pivoted() {
    use crate::{
        content::world::{SectorRegion, WorldStream},
        sector::Coordinates,
    };
    let game = std::env::var_os("ARCANUM_GAME_DIR").expect("set ARCANUM_GAME_DIR");
    let context = instance();
    let adapter = hardware_adapter(&context, None).unwrap();
    let mut gpu = Compositor::new(&adapter, TextureFormat::Rgba8Unorm, false).unwrap();
    let sector = Coordinates { x: 6, y: 12 };
    let region = SectorRegion {
        first: sector,
        last: sector,
    };
    let center = (420, 814);
    let size = (1280, 720);
    let mut camera = region.camera(center).unwrap();
    camera.resize(size);
    let mut world = WorldStream::new(
        Vfs::from_game(&game, Policy::Ce).unwrap(),
        "k'na tha".into(),
        region,
        center,
    )
    .unwrap()
    .with_lighting(12)
    .unwrap();
    world.add_light_probe((419, 814), false).unwrap();
    let (standing, _) = capture_world_tick(&mut gpu, &mut world, &camera, size, Duration::ZERO);
    assert_eq!(world.animation_delay(&camera, size, Duration::ZERO), None);
    assert!(world.start_fidget(Duration::ZERO));
    let before = world.stats();
    let tick = Duration::from_millis(300);
    let (fidget, _) = capture_world_tick(&mut gpu, &mut world, &camera, size, tick);
    assert_ne!(fidget, standing);
    let anchor = (
        size.0 as i32 / 2 - camera.position().0,
        size.1 as i32 / 2 - camera.position().1,
    );
    {
        let mut draws = world.visible_assets_at(&camera, size, tick);
        assert!(
            draws.any(|draw| matches!(draw.modulation, Modulation::Shadow(_))
                && draw.sprite.anchor == anchor)
        );
        let rabbit = draws
            .find(|draw| draw.asset.source.virtual_path == "art/monster/bun/bunuwxaa.art")
            .unwrap();
        assert_eq!(rabbit.sprite.anchor, anchor);
    }
    assert_eq!(world.turn_fidget(1), Some(4));
    let (turned, _) = capture_world_tick(&mut gpu, &mut world, &camera, size, tick);
    assert_ne!(turned, fidget);
    assert_eq!(world.turn_fidget(-1), Some(3));
    let (restored, warm) = capture_world_tick(&mut gpu, &mut world, &camera, size, tick);
    assert_eq!(restored, fidget);
    assert_eq!(warm.uploaded, 0);
    let state = world.dynamic_light(0).unwrap();
    world
        .set_dynamic_light(
            0,
            crate::content::world::DynamicLightState {
                tile: (420, 813),
                ..state
            },
        )
        .unwrap();
    assert_eq!(
        world.animation_delay(&camera, size, tick),
        Some(Duration::from_millis(100))
    );
    capture_world_tick(&mut gpu, &mut world, &camera, size, tick);
    world.set_dynamic_light(0, state).unwrap();
    let end = Duration::from_secs(1);
    let (finished, _) = capture_world_tick(&mut gpu, &mut world, &camera, size, end);
    assert_eq!(finished, standing);
    assert_eq!(world.animation_delay(&camera, size, end), None);
    camera.pan((64, 32), size);
    capture_world_tick(&mut gpu, &mut world, &camera, (960, 560), tick);
    camera.reset(size);
    assert_eq!(
        fidget,
        capture_world_tick(&mut gpu, &mut world, &camera, size, tick).0
    );
    let after = world.stats();
    assert_eq!(
        (after.sector_reads, after.vfs_reads, after.art.decodes),
        (before.sector_reads, before.vfs_reads, before.art.decodes)
    );
}

#[test]
#[ignore = "requires installed Arcanum data and a hardware GPU adapter"]
fn original_rabbit_walk_keeps_shadow_and_art_warm() {
    use crate::{
        content::world::{SectorRegion, WorldStream},
        sector::Coordinates,
    };
    let game = std::env::var_os("ARCANUM_GAME_DIR").expect("set ARCANUM_GAME_DIR");
    let context = instance();
    let adapter = hardware_adapter(&context, None).unwrap();
    let mut gpu = Compositor::new(&adapter, TextureFormat::Rgba8Unorm, false).unwrap();
    let sector = Coordinates { x: 6, y: 12 };
    let region = SectorRegion {
        first: sector,
        last: sector,
    };
    let center = (420, 814);
    let size = (320, 220);
    let mut camera = region.camera(center).unwrap();
    let mut world = WorldStream::new(
        Vfs::from_game(game, Policy::Ce).unwrap(),
        "k'na tha".into(),
        region,
        center,
    )
    .unwrap()
    .with_lighting(12)
    .unwrap()
    .with_walk_preview(true)
    .unwrap();
    world.add_light_probe((420, 815), false).unwrap();
    let time = |ms| Duration::from_millis(ms);
    let (first, _) = capture_world_tick(&mut gpu, &mut world, &camera, size, time(0));
    let warm = world.stats();
    let moved = world.update_walk(time(246)).unwrap();
    assert_eq!(moved.tile, (420, 815));
    let draws: Vec<_> = world.visible_assets_at(&camera, size, time(246)).collect();
    let mobile_index = draws
        .iter()
        .position(|draw| draw.asset.source.virtual_path == "art/monster/bun/bunuwxab.art")
        .unwrap();
    let rabbit = &draws[mobile_index];
    assert_eq!(
        rabbit.sprite.anchor,
        (size.0 as i32 / 2 + 43, size.1 as i32 / 2 + 22)
    );
    let shadows: Vec<_> = draws
        .iter()
        .enumerate()
        .filter(|(_, draw)| matches!(draw.modulation, Modulation::Shadow(_)))
        .collect();
    assert!(!shadows.is_empty());
    for (i, shadow) in shadows {
        assert!(i < mobile_index);
        assert_eq!(shadow.sprite.anchor, rabbit.sprite.anchor);
    }
    let (middle, _) = capture_world_tick(&mut gpu, &mut world, &camera, size, time(246));
    let (turn, _) = capture_world_tick(&mut gpu, &mut world, &camera, size, time(697));
    let (stop, _) = capture_world_tick(&mut gpu, &mut world, &camera, size, time(943));
    assert_ne!(first, middle);
    assert_ne!(middle, turn);
    assert_ne!(turn, stop);
    assert_eq!(world.animation_delay(&camera, size, time(943)), None);
    let before = world.dynamic_light(0).unwrap();
    let mut shifted = before;
    shifted.tile = (421, 817);
    world.set_dynamic_light(0, shifted).unwrap();
    capture_world_tick(&mut gpu, &mut world, &camera, size, time(246));
    world.set_dynamic_light(0, before).unwrap();
    assert_eq!(
        capture_world_tick(&mut gpu, &mut world, &camera, size, time(246)).0,
        middle
    );
    for step in 0..=23 {
        capture_world_tick(&mut gpu, &mut world, &camera, size, time(step * 41));
    }
    for step in 0..=23 {
        let (_, stats) = capture_world_tick(&mut gpu, &mut world, &camera, size, time(step * 41));
        assert_eq!(stats.uploaded, 0, "warm walk frame {step}");
    }
    camera.pan((200, 120), size);
    capture_world_tick(&mut gpu, &mut world, &camera, (321, 201), time(697));
    camera.reset(size);
    let (restored, _) = capture_world_tick(&mut gpu, &mut world, &camera, size, time(943));
    assert_eq!(restored, stop);
    let hot = world.stats();
    assert_eq!(
        (hot.sector_reads, hot.vfs_reads, hot.art.decodes),
        (warm.sector_reads, warm.vfs_reads, warm.art.decodes)
    );
}
