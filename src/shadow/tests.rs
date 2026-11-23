use super::*;
use crate::{
    art::SpriteFrame,
    compositor::{SpriteDraw, blit_modulated},
};

fn bitmap(rows: &[[u8; 7]; 2]) -> Vec<u8> {
    let offset = DIB_END + 256 * 4;
    let mut bytes = vec![0; offset];
    bytes[..2].copy_from_slice(b"BM");
    for (at, word) in [
        (2, offset + 16),
        (10, offset),
        (14, 40),
        (18, 7),
        (22, 2),
        (26, 0x0008_0001),
        (34, 16),
    ] {
        bytes[at..at + 4].copy_from_slice(&(word as u32).to_le_bytes());
    }
    for row in rows.iter().rev() {
        bytes.extend_from_slice(row);
        bytes.push(36); // Row padding is never a lookup sample.
    }
    bytes
}

#[test]
fn directional_bin_order_cap_tint_and_eligibility() {
    let bytes = bitmap(&[[38, 36, 64, 45, 34, 33, 32], [0; 7]]);
    let map = Lookup::decode(&bytes).unwrap();
    let light = |x, flags, owned_by_caster, tint| Candidate {
        center: (x, -1),
        tint,
        flags,
        owned_by_caster,
    };
    let white = [255; 3];
    let candidates = [
        light(2, 0x40, false, white),
        light(2, 0x01, false, white),
        light(2, 0x02, false, white),
        light(2, 0, true, white),
        light(2, 0, false, [100; 3]),
        light(4, 0, false, white),
        light(-3, 0, false, white),
        light(-2, 0, false, white),
        light(-1, 0, false, white),
        light(0, 0, false, white),
        light(1, 0, false, white), // Fifth eligible light loses before sorting.
    ];
    assert_eq!(map.dimensions(), (7, 2));
    let shadows = select(&map, (0, 0), [100; 3], 7, true, candidates).unwrap();
    assert_eq!(
        shadows
            .iter()
            .map(|m| (m.direction, m.frame, m.gray))
            .collect::<Vec<_>>(),
        [
            (2, 7, 40),
            (16, 4, 30),
            (20, 4, 30),
            (16, 6, 14),
            (17, 6, 14)
        ]
    );
    assert_eq!(
        select(&map, (0, 0), [100; 3], 7, false, candidates).unwrap(),
        shadows[..1]
    );
    assert_eq!(
        select(&map, (0, 0), [100; 3], 7, true, [light(2, 0, false, white)]).unwrap(),
        [Mask {
            direction: 16,
            frame: 1,
            gray: 54
        }]
    );
}

#[test]
fn moving_and_toggling_use_world_delta_and_do_not_mutate_lookup() {
    let bytes = bitmap(&[[0; 7], [0, 34, 0, 46, 0, 31, 0]]);
    let map = Lookup::decode(&bytes).unwrap();
    let mut emitter = Candidate {
        center: (-2, 0),
        flags: 0,
        tint: [230, 128, 30],
        owned_by_caster: false,
    };
    let sample = |caster, candidate| select(&map, caster, [30; 3], 0, true, [candidate]);
    let left = sample((0, 0), emitter).unwrap();
    assert_eq!(
        left[1],
        Mask {
            direction: 16,
            frame: 2,
            gray: 35
        }
    );
    emitter.center = (0, 0);
    assert_eq!(
        sample((0, 0), emitter).unwrap(),
        [Mask {
            direction: 18,
            frame: 0,
            gray: 47
        }]
    );
    emitter.center = (500, -250);
    assert_eq!(
        sample((500, -250), emitter).unwrap(),
        sample(
            (0, 0),
            Candidate {
                center: (0, 0),
                ..emitter
            }
        )
        .unwrap()
    );
    emitter.flags = 1;
    assert_eq!(sample((500, -250), emitter).unwrap(), left[..1]);
    emitter.center = (502, -250);
    emitter.flags = 0;
    assert_eq!(sample((500, -250), emitter), Err(Error::InvalidBin(31)));
    assert_eq!(map.index((i32::MIN, 0), (i32::MAX, 0)), 0);
}

#[test]
fn rejects_unsound_bitmap_layouts() {
    let clean = bitmap(&[[0; 7]; 2]);
    for (at, value) in [
        (10, 54u32),
        (18, u32::MAX),
        (22, u32::MAX),
        (30, 1),
        (34, 15),
        (46, 257),
    ] {
        let mut bytes = clean.clone();
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
        assert!(matches!(Lookup::decode(&bytes), Err(Error::InvalidBitmap)));
    }
    assert!(Lookup::decode(&clean[..clean.len() - 1]).is_err());
    assert!(Lookup::decode(&vec![0; MAX_LOOKUP_BYTES + 1]).is_err());
}

#[test]
fn extended_art_uses_mes_name_and_eight_rotations_per_file() {
    let names = Mes::parse_text("{6}{Med_Critter}\n{7}{nested/escape}").unwrap();
    let art = Art::decode(0x9030_5001).unwrap();
    assert_eq!(art.base_frame, 5);
    assert_eq!(
        art.resource(&names, 31).unwrap(),
        ("art/light/med_critter_s3.art".into(), 7)
    );
    assert_eq!(
        art.resource(&names, 8).unwrap(),
        ("art/light/med_critter_s1.art".into(), 0)
    );
    assert_eq!(art.resource(&names, 32), Err(Error::InvalidArtName));
    assert_eq!(
        Art::decode(0x9038_0001).unwrap().resource(&names, 0),
        Err(Error::InvalidArtName)
    );
    assert!(Art::decode(0x9030_0000).is_err());
    assert!(Art::decode(u32::MAX).is_err());
}

#[test]
fn shadow_subtraction_preserves_transparency_and_later_draws() {
    let frame = SpriteFrame {
        width: 4,
        height: 1,
        hotspot: (1, 0),
        offset: (0, 0),
        pixels: vec![
            [200, 200, 200, 255],
            [255; 4],
            [255, 255, 255, 0],
            [100, 25, 255, 1],
        ],
    };
    let mask = SpriteDraw {
        frame: &frame,
        anchor: (0, 0),
    };
    let upright = SpriteFrame {
        width: 1,
        pixels: vec![[20, 40, 60, 255]],
        height: 1,
        hotspot: (1, 0),
        offset: (0, 0),
    };
    let upright = SpriteDraw {
        frame: &upright,
        anchor: (4, 0),
    };
    let mut dest = [[180, 75, 210]; 4];
    blit_modulated(
        &mut dest,
        (4, 1),
        4,
        [
            (mask, Modulation::Shadow(102)),
            (mask, Modulation::Shadow(102)),
            (upright, Modulation::Unlit),
        ],
    )
    .unwrap();
    assert_eq!(
        dest,
        [[0, 0, 6], [180, 75, 210], [100, 55, 6], [20, 40, 60]]
    );
}

/// Verified base NPC prefix and prototype prefix for this installed fixture.
/// A world mobile importer will need to read the full records and inheritance.
pub(crate) fn bates_fixture(
    vfs: &mut crate::vfs::Vfs,
) -> ((u32, u32), [(Mask, crate::content::art::LoadedFrame); 2]) {
    use crate::{content, isometric::project_world_tile, sector};
    let mut read = |path: &str| {
        let mut data = Vec::new();
        vfs.copy(path, &mut data).unwrap();
        data
    };
    let mobile = read("maps/bates mansion lev 1/g_0a68bdf7_1d83_4ec4_b208_b4c36b06bf87.mob");
    let word = |bytes: &[u8], at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
    // Version 119 NPC. Its five common/subtype bitmap words precede the
    // current ART, location present byte, location and pixel offsets.
    assert_eq!(word(&mobile, 0), 119);
    assert_eq!(word(&mobile, 12), 17088);
    assert_eq!(word(&mobile, 52), 16);
    assert_eq!(word(&mobile, 58) & 0x1f, 0xf);
    assert_eq!(word(&mobile, 78), 0x2880_2820);
    assert_eq!(mobile[82], 1);
    let caster = (word(&mobile, 83), word(&mobile, 87));
    assert_eq!(caster, (80, 81));
    assert_eq!((word(&mobile, 91), word(&mobile, 95)), (0, 0));
    let proto = read("proto/017088 - npc.pro");
    let fields = sector::objects::fixture_npc_prototype_appearance(
        &proto,
        crate::prototype::SerializedPrototypeId(17088),
    )
    .unwrap();
    let art = Art::decode(fields.shadow_art.unwrap()).unwrap();
    assert_eq!(fields.shadow_art, Some(0x9030_0001));
    assert_eq!(fields.scale, Some(100));
    assert_eq!(word(&mobile, 99) & 0x0006_0387, 0); // Visible, upright, dry and unshrunk.
    let sector = read("maps/bates mansion lev 1/67108865.sec");
    let floor = sector::decode_floor(&sector).unwrap();
    assert_eq!(floor.tile(16, 17).unwrap().0 & 0x100, 0); // Indoor.
    let lights = &floor.lights;
    assert_eq!(lights[2].tile, (81, 82));
    assert_eq!(lights[2].offset, (13, -6));
    assert_eq!(lights[2].rgb, [254, 213, 163]);
    assert_eq!(lights[7].flags, 0x40);
    let lookup = content::shadow::LoadedLookup::load(vfs).unwrap();
    assert_eq!(lookup.lookup().dimensions(), (383, 183));
    let map = lookup.lookup();
    let candidates = lights.iter().rev().map(|light| {
        let projected = project_world_tile(light.tile, caster).unwrap();
        Candidate {
            center: (projected.0 + light.offset.0, projected.1 + light.offset.1),
            tint: light.rgb,
            flags: light.flags,
            owned_by_caster: false, // All are map-owned.
        }
    });
    let masks = select(&map, (0, 0), [74, 80, 98], art.base_frame, true, candidates).unwrap();
    assert_eq!(
        masks,
        [
            Mask {
                direction: 2,
                frame: 0,
                gray: 32
            },
            Mask {
                direction: 31,
                frame: 2,
                gray: 41
            },
        ]
    );
    let names = content::load_mes(vfs, "art/light/light.mes").unwrap();
    let frames = [masks[0], masks[1]].map(|mask| {
        (
            mask,
            content::shadow::load_mask(vfs, &names.mes, art, mask).unwrap(),
        )
    });
    (caster, frames)
}

#[test]
#[ignore = "requires ARCANUM_GAME_DIR with original Bates Mansion data"]
fn original_bates_light_selects_authored_mobile_shadow_masks() {
    use crate::vfs::{Policy, Vfs};
    let game = std::env::var_os("ARCANUM_GAME_DIR").expect("set ARCANUM_GAME_DIR");
    let mut vfs = Vfs::from_game(game, Policy::Ce).unwrap();
    let (_, [(_, ambient), (_, directional)]) = bates_fixture(&mut vfs);
    assert_eq!(ambient.source.virtual_path, "art/light/med_critter_s0.art");
    assert_eq!(
        directional.source.virtual_path,
        "art/light/med_critter_s3.art"
    );
    assert_eq!(ambient.decoded.stored_rotation, 2);
    assert_eq!(directional.decoded.stored_rotation, 7);
    assert_eq!(directional.decoded.stored_palette, 0);
    assert!(directional.decoded.frame.pixels.iter().any(|p| p[3] == 0));
    assert!(directional.decoded.frame.pixels.iter().any(|p| p[3] == 255));
}
