//! Camera translation and ordered raster selection for a loaded isometric region.

use crate::{
    art::SpriteFrame,
    compositor::{SpriteDraw, sprite_origin},
};
use std::collections::BTreeSet;

/// Pixel offset from the world tile used as the scene's reference center.
#[derive(Debug, Clone)]
pub struct Camera {
    position: (i32, i32),
    at_home: bool,
    // World tile bounds in the scaled inverse projection, 2Y − X and 2Y + X.
    axes: [(i64, i64); 2],
}

impl Camera {
    /// Construct a camera for an original square region of at most one sector.
    pub fn new(reference: (u32, u32), first_tile: (u32, u32), side: u8) -> Option<Self> {
        if !(1..=64).contains(&side) {
            return None;
        }
        let axis = |first: u32, center: u32| {
            let last = first.checked_add(u32::from(side) - 1)?;
            if first.abs_diff(center) > 63 || last.abs_diff(center) > 63 {
                return None;
            }
            Some((
                80 * (i64::from(first) - i64::from(center)) - 40,
                80 * (i64::from(last) - i64::from(center)) + 40,
            ))
        };
        Some(Self {
            position: (0, 0),
            at_home: true,
            axes: [
                axis(first_tile.0, reference.0)?,
                axis(first_tile.1, reference.1)?,
            ],
        })
    }

    pub fn position(&self) -> (i32, i32) {
        self.position
    }

    pub fn pan(&mut self, delta: (i32, i32), viewport: (u32, u32)) -> bool {
        let changed = self.place(
            (
                self.position.0.saturating_add(delta.0),
                self.position.1.saturating_add(delta.1),
            ),
            viewport,
        );
        if changed {
            self.at_home = false;
        }
        changed
    }

    pub fn reset(&mut self, viewport: (u32, u32)) -> bool {
        self.at_home = true;
        self.place((0, 0), viewport)
    }

    pub fn resize(&mut self, viewport: (u32, u32)) -> bool {
        self.place(if self.at_home { (0, 0) } else { self.position }, viewport)
    }

    fn place(&mut self, proposal: (i32, i32), viewport: (u32, u32)) -> bool {
        let (x, y) = (i64::from(proposal.0), i64::from(proposal.1));
        // Reserve enough room for all four viewport corners in each diagonal.
        let margin = i64::from(viewport.0.div_ceil(2)) + 2 * i64::from(viewport.1.div_ceil(2));
        let clamp = |coordinate: i64, (low, high): (i64, i64)| {
            // Multiples of four invert to integer pixel positions.
            let first = (low + margin + 3).div_euclid(4);
            let last = (high - margin).div_euclid(4);
            let units = if first <= last {
                coordinate.div_euclid(4).clamp(first, last)
            } else {
                // An oversized window shows initialized background beyond the loaded area.
                (low + high).div_euclid(8)
            };
            units * 4
        };
        let a = clamp(2 * y - x, self.axes[0]);
        let b = clamp(2 * y + x, self.axes[1]);
        let next = (((b - a) / 2) as i32, ((a + b) / 4) as i32);
        let changed = next != self.position;
        self.position = next;
        changed
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheStats {
    /// Distinct RGBA rasters retained by the loaded draw list, including transformed variants.
    pub rasters: usize,
    pub pixel_bytes: usize,
    pub repeated_references: usize,
}

struct Raster<'a> {
    draw: SpriteDraw<'a>,
    bounds: (i64, i64, i64, i64),
}

/// Prepared scene order and conservative raster rectangles. The scene borrows
/// decoded frames from the importer and does no loading or allocation on pan.
pub struct CameraScene<'a> {
    rasters: Vec<Raster<'a>>,
    cache: CacheStats,
}

impl<'a> CameraScene<'a> {
    pub fn new(draws: impl IntoIterator<Item = SpriteDraw<'a>>) -> Self {
        let mut frames: BTreeSet<*const SpriteFrame> = BTreeSet::new();
        let mut pixel_bytes = 0;
        let rasters: Vec<_> = draws
            .into_iter()
            .map(|draw| {
                if frames.insert(std::ptr::from_ref(draw.frame)) {
                    pixel_bytes += draw.frame.pixels.capacity() * size_of::<[u8; 4]>();
                }
                let (x, y) = sprite_origin(draw.frame, draw.anchor);
                Raster {
                    draw,
                    bounds: (
                        x,
                        y,
                        x + i64::from(draw.frame.width),
                        y + i64::from(draw.frame.height),
                    ),
                }
            })
            .collect();
        let cache = CacheStats {
            rasters: frames.len(),
            pixel_bytes,
            repeated_references: rasters.len() - frames.len(),
        };
        Self { rasters, cache }
    }

    pub fn cache(&self) -> CacheStats {
        self.cache
    }

    pub fn visible(
        &self,
        camera: &Camera,
        viewport: (u32, u32),
    ) -> impl Iterator<Item = SpriteDraw<'a>> + '_ {
        let (x, y) = camera.position();
        let (width, height) = viewport;
        let left = i64::from(x) - i64::from(width / 2);
        let top = i64::from(y) - i64::from(height / 2);
        let right = left + i64::from(width);
        let bottom = top + i64::from(height);
        self.rasters.iter().filter_map(move |raster| {
            let (x0, y0, x1, y1) = raster.bounds;
            if width == 0 || height == 0 || x0 >= right || x1 <= left || y0 >= bottom || y1 <= top {
                return None;
            }
            let (x, y) = raster.draw.anchor;
            Some(SpriteDraw {
                frame: raster.draw.frame,
                anchor: (
                    i32::try_from(i64::from(x) - left).ok()?,
                    i32::try_from(i64::from(y) - top).ok()?,
                ),
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compositor::blit_ordered;

    #[test]
    fn repeated_pan_and_resize_keep_viewport_inside_region() {
        let mut camera = Camera::new((101, 83), (64, 64), 64).unwrap();
        assert_eq!(camera.position(), (0, 0));
        let check = |camera: &Camera, size: (u32, u32)| {
            let (x, y) = camera.position();
            let margin = i64::from(size.0.div_ceil(2)) + 2 * i64::from(size.1.div_ceil(2));
            for (diagonal, (low, high)) in [i64::from(2 * y - x), i64::from(2 * y + x)]
                .into_iter()
                .zip(camera.axes)
            {
                assert!(diagonal - margin >= low);
                assert!(diagonal + margin <= high);
            }
        };
        let size = (1281, 721);
        for delta in [(-64, 0), (0, -64), (64, 0), (0, 64)] {
            for _ in 0..100 {
                camera.pan(delta, size);
                check(&camera, size);
            }
            assert!(!camera.pan(delta, size));
        }
        let larger = (1600, 850);
        camera.resize(larger);
        check(&camera, larger);
        assert!(camera.reset(size));
        assert_eq!(camera.position(), (0, 0));
        assert!(!camera.reset(size));
    }

    #[test]
    fn oversized_view_and_extreme_pans_stay_bounded() {
        let mut camera = Camera::new((100, 80), (96, 80), 4).unwrap();
        assert!(camera.resize((u32::MAX, u32::MAX)));
        let center = camera.position();
        assert!(!camera.pan((i32::MIN, i32::MAX), (u32::MAX, u32::MAX)));
        assert_eq!(camera.position(), center);
        assert!(Camera::new((0, 0), (u32::MAX, 0), 2).is_none());
    }

    #[test]
    fn temporary_large_viewport_does_not_move_home() {
        let mut camera = Camera::new((101, 83), (64, 64), 64).unwrap();
        assert!(camera.resize((1920, 1040)));
        assert_ne!(camera.position(), (0, 0));
        assert!(camera.resize((1280, 720)));
        assert_eq!(camera.position(), (0, 0));
    }

    #[test]
    fn raster_overhang_and_order_survive_pan_and_home() {
        let wall = SpriteFrame {
            width: 4,
            height: 1,
            hotspot: (0, 0),
            offset: (0, 0),
            pixels: vec![[120, 30, 50, 255]; 4],
        };
        let item = SpriteFrame {
            width: 4,
            height: 1,
            hotspot: (4, 0),
            offset: (0, 0),
            pixels: vec![[0; 4], [30, 110, 40, 255], [0; 4], [80, 30, 150, 255]],
        };
        let scene = CameraScene::new([
            SpriteDraw {
                frame: &wall,
                anchor: (-2, 0),
            },
            // The anchor is outside the viewport. Its raster still intersects it.
            SpriteDraw {
                frame: &item,
                anchor: (4, 0),
            },
            SpriteDraw {
                frame: &item,
                anchor: (24, 0),
            },
        ]);
        assert_eq!(scene.cache().rasters, 2);
        assert_eq!(scene.cache().repeated_references, 1);
        let mut camera = Camera::new((31, 31), (0, 0), 64).unwrap();
        let viewport = (4, 1);
        let compose = |camera: &Camera| {
            let mut pixels = [[0; 3]; 4];
            blit_ordered(&mut pixels, viewport, 4, scene.visible(camera, viewport)).unwrap();
            pixels
        };
        let original = compose(&camera);
        assert_eq!(
            original,
            [[120, 30, 50], [120, 30, 50], [120, 30, 50], [30, 110, 40]]
        );
        assert!(camera.pan((16, 0), viewport));
        assert_ne!(compose(&camera), original);
        assert!(camera.reset(viewport));
        assert_eq!(compose(&camera), original);
    }
}
