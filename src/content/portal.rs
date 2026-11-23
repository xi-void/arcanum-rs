//! Static appearances of original doors in supported wall openings.

use super::{LoadedMes, Source, art, load_mes, sector::read_static_prototype};
use crate::{
    art::SpriteFrame,
    compositor::SpriteDraw,
    mes::Mes,
    prototype::SerializedPrototypeId,
    resource_path::normalize,
    sector::{
        Coordinates,
        objects::{
            AppearanceFields, PortalPrototype, StaticCategory, StaticRecord,
            decode_portal_prototype,
        },
    },
    vfs::Vfs,
    wall::{Placement, opening_pivot},
};
use std::{
    collections::{BTreeMap, btree_map::Entry},
    error::Error,
    sync::Arc,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Window,
    Door,
}

/// Verified visual state of a stored door frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortalState {
    Closed,
    Open,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PortalArt {
    pub kind: Kind,
    pub number: u16,
    pub frame: u8,
    pub rotation: u8,
    pub palette: u8,
    pub mirror_x: bool,
    pub damaged: bool,
}

impl PortalArt {
    pub fn decode(raw: u32) -> Result<Self, Box<dyn Error>> {
        if raw >> 28 != 3 || raw & 0x1ce != 0 {
            return Err(format!("unsupported portal ART word {raw:#010x}").into());
        }
        Ok(Self {
            kind: if raw & 0x400 != 0 {
                Kind::Door
            } else {
                Kind::Window
            },
            number: ((raw >> 19) & 511) as u16,
            frame: ((raw >> 14) & 31) as u8,
            rotation: ((raw >> 11) & 7) as u8,
            palette: ((raw >> 4) & 3) as u8,
            mirror_x: raw & 1 != 0,
            damaged: raw & 0x200 != 0,
        })
    }

    pub fn resource_path(self, names: &Mes) -> Result<String, Box<dyn Error>> {
        if self.damaged {
            return Err("damaged portal artwork is unsupported".into());
        }
        let key = i32::from(self.number) + if self.kind == Kind::Window { 1001 } else { 0 };
        let name = names
            .get(key)
            .and_then(|value| value.split_ascii_whitespace().next())
            .ok_or_else(|| format!("no portal name for key {key}"))?;
        let name = normalize(name)?;
        if name.contains('/') || !name.ends_with(".art") {
            return Err(format!("invalid portal ART filename at key {key}").into());
        }
        Ok(format!("art/portal/{name}"))
    }

    pub fn visual_state(self) -> Result<PortalState, Box<dyn Error>> {
        if self.kind == Kind::Door && !self.damaged {
            match (self.frame, self.rotation, self.palette, self.mirror_x) {
                (0, 1, 0, false) | (0, 7, 1, true) => return Ok(PortalState::Closed),
                // Rotations 3 and 5 use the 1–3 opening sequence.
                (3, 3, 1, true) | (3, 5, 0, false) => return Ok(PortalState::Open),
                _ => {}
            }
        }
        Err(format!("unsupported static portal appearance {self:?}").into())
    }

    pub fn pivot(self, frame: &SpriteFrame) -> (i64, i64) {
        opening_pivot(frame, self.rotation, self.mirror_x)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PortalAppearance {
    pub offset: (i32, i32),
    pub scale: i32,
    pub flags: u32,
    pub portal_flags: u32,
}

impl PortalAppearance {
    fn resolve(
        instance: AppearanceFields,
        prototype: AppearanceFields,
    ) -> Result<Self, Box<dyn Error>> {
        let offset = (
            instance
                .offset_x
                .or(prototype.offset_x)
                .ok_or("missing portal X offset")?,
            instance
                .offset_y
                .or(prototype.offset_y)
                .ok_or("missing portal Y offset")?,
        );
        let scale = instance
            .scale
            .or(prototype.scale)
            .ok_or("missing portal scale")?;
        let flags = instance
            .flags
            .or(prototype.flags)
            .ok_or("missing portal object flags")?;
        let portal_flags = instance
            .portal_flags
            .or(prototype.portal_flags)
            .ok_or("missing portal subtype flags")?;
        let blit = (
            instance.blit_flags.or(prototype.blit_flags),
            instance.blit_color.or(prototype.blit_color),
            instance.alpha.or(prototype.alpha),
        );
        // Lock bits do not choose graphics. The busted bit and unknown flags are outside this slice.
        if scale != 100
            || flags & !0x4430 != 0
            || portal_flags & !0x17f != 0
            || blit != (Some(0), Some(0xff_ffff), Some(255))
        {
            return Err(format!("unsupported portal appearance: scale {scale}, flags {flags:#x}, portal flags {portal_flags:#x}, blend {blit:?}").into());
        }
        Ok(Self {
            offset,
            scale,
            flags,
            portal_flags,
        })
    }
}

pub struct ImportedPortal {
    pub record: StaticRecord,
    pub prototype_source: Source,
    pub prototype_art_id: u32,
    pub names_source: Source,
    pub artwork: Arc<art::LoadedFrame>,
    pub art: PortalArt,
    pub state: PortalState,
    pub appearance: PortalAppearance,
    pub placement: Placement,
    /// The same-tile wall that precedes this leaf in the supported room pass.
    pub paired_wall_index: usize,
}

impl ImportedPortal {
    pub fn draw(&self, center: (u32, u32)) -> Option<SpriteDraw<'_>> {
        let frame = &self.artwork.decoded.frame;
        self.placement
            .draw_with_pivot(frame, center, self.art.pivot(frame))
    }
}

pub(super) struct PortalImport {
    names: LoadedMes,
    prototypes: BTreeMap<SerializedPrototypeId, (Source, PortalPrototype)>,
}

impl PortalImport {
    pub(super) fn new(vfs: &mut Vfs) -> Result<Self, Box<dyn Error>> {
        Ok(Self {
            names: load_mes(vfs, "art/portal/portal.mes")?,
            prototypes: BTreeMap::new(),
        })
    }

    pub(super) fn resolve(
        &mut self,
        vfs: &mut Vfs,
        frames: &mut art::FrameCache,
        sector: Coordinates,
        record: StaticRecord,
        paired_wall_index: usize,
    ) -> Result<ImportedPortal, Box<dyn Error>> {
        if record.category != StaticCategory::Portal {
            return Err("expected a portal record".into());
        }
        let art = PortalArt::decode(record.art_id)?;
        let state = art.visual_state()?;
        let (prototype_source, prototype) = match self.prototypes.entry(record.prototype) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => {
                let (source, bytes) = read_static_prototype(vfs, record.prototype)?;
                let fields = decode_portal_prototype(&bytes, record.prototype)
                    .map_err(|error| format!("{} from {source}: {error}", source.virtual_path))?;
                entry.insert((source, fields))
            }
        };
        let appearance = PortalAppearance::resolve(record.fields, prototype.appearance)?;
        let path = frames.resolve(|| art.resource_path(&self.names.mes))?;
        let artwork = frames.load(
            vfs,
            &path,
            usize::from(art.frame),
            art.palette,
            art.mirror_x,
        )?;
        let frame = &artwork.decoded.frame;
        if frame.width > 512 || frame.height > 512 {
            return Err(format!("portal frame exceeds 512 pixels per axis in {path}").into());
        }
        let placement = Placement {
            tile: record.world_tile(sector).ok_or("invalid portal tile")?,
            pixel_offset: appearance.offset,
        };
        Ok(ImportedPortal {
            record,
            prototype_source: prototype_source.clone(),
            prototype_art_id: prototype.art_id,
            names_source: self.names.source.clone(),
            artwork,
            art,
            state,
            appearance,
            placement,
            paired_wall_index,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closed_mirrored_leaf_uses_the_stored_palette_and_door_name() {
        let door = PortalArt::decode(0x3210_3c11).unwrap();
        assert_eq!(door.kind, Kind::Door);
        assert_eq!(
            (door.number, door.frame, door.rotation, door.palette),
            (66, 0, 7, 1)
        );
        assert!(door.mirror_x);
        assert_eq!(door.visual_state().unwrap(), PortalState::Closed);
        let names =
            Mes::parse_text("{66}{IN4E4aU0.ART 2008}\n{1067}{TestWindow.ART 2008}").unwrap();
        assert_eq!(
            door.resource_path(&names).unwrap(),
            "art/portal/in4e4au0.art"
        );

        let window = PortalArt::decode(0x3210_3811).unwrap();
        assert_eq!(window.kind, Kind::Window);
        assert_eq!(
            window.resource_path(&names).unwrap(),
            "art/portal/testwindow.art"
        );
        assert!(window.visual_state().is_err());
        let open = PortalArt::decode(0x3178_ec00).unwrap();
        assert_eq!((open.frame, open.rotation), (3, 5));
        assert_eq!(open.visual_state().unwrap(), PortalState::Open);
        let mirrored = PortalArt::decode(0x3348_dc11).unwrap();
        assert!(mirrored.mirror_x);
        assert_eq!(mirrored.visual_state().unwrap(), PortalState::Open);
        for unsupported in [
            0x3178_6c00,
            0x3178_ac00,
            0x3178_ec01,
            0x3178_cc00,
            0x3178_fc11,
        ] {
            assert!(
                PortalArt::decode(unsupported)
                    .unwrap()
                    .visual_state()
                    .is_err()
            );
        }
        assert!(
            PortalArt::decode(0x3210_3e11)
                .unwrap()
                .resource_path(&names)
                .is_err()
        );
        assert!(PortalArt::decode(0x3210_3c13).is_err());

        let frame = SpriteFrame {
            width: 33,
            height: 96,
            hotspot: (29, 79),
            offset: (4, 5),
            pixels: Vec::new(),
        };
        assert_eq!(door.pivot(&frame), (42, 99));
        assert_eq!(
            PortalArt::decode(0x3210_0c00).unwrap().pivot(&frame),
            (-11, 99)
        );
    }

    #[test]
    fn explicit_zero_overrides_prototype_fields_and_busted_is_rejected() {
        let defaults = AppearanceFields {
            offset_x: Some(-4),
            offset_y: Some(3),
            scale: Some(100),
            flags: Some(0x4000),
            portal_flags: Some(9),
            blit_flags: Some(0),
            blit_color: Some(0xff_ffff),
            alpha: Some(255),
            ..AppearanceFields::default()
        };
        let mut instance = AppearanceFields {
            offset_y: Some(0),
            portal_flags: Some(0),
            ..AppearanceFields::default()
        };
        let effective = PortalAppearance::resolve(instance, defaults).unwrap();
        assert_eq!(effective.offset, (-4, 0));
        assert_eq!(effective.portal_flags, 0);
        assert_eq!(effective.flags, 0x4000);
        instance.portal_flags = Some(0x80);
        assert!(PortalAppearance::resolve(instance, defaults).is_err());
    }

    #[test]
    fn portal_draw_preserves_the_wall_through_transparent_and_clipped_pixels() {
        use crate::compositor::{blit_ordered, sprite_origin};
        let art = PortalArt::decode(0x3210_0c00).unwrap();
        let placement = Placement {
            tile: (12, 12),
            pixel_offset: (-2, 0),
        };
        let frame = SpriteFrame {
            width: 3,
            height: 1,
            hotspot: (39, -20),
            offset: (91, 23),
            pixels: vec![[200, 80, 10, 255], [255, 0, 0, 0], [200, 80, 10, 255]],
        };
        let draw = placement
            .draw_with_pivot(&frame, placement.tile, art.pivot(&frame))
            .unwrap();
        assert_eq!(sprite_origin(draw.frame, draw.anchor), (-1, 0));
        let mut wall = [[20, 50, 80]; 3];
        blit_ordered(&mut wall, (3, 1), 3, [draw]).unwrap();
        assert_eq!(wall, [[20, 50, 80], [200, 80, 10], [20, 50, 80]]);
    }
}
