//! Loading of the winning ART resource through the existing VFS.

use super::{Error, Source, read_bounded};
use crate::art::{self, DecodedFrame};
use crate::vfs::Vfs;

#[derive(Debug)]
pub struct LoadedFrame {
    pub source: Source,
    pub decoded: DecodedFrame,
    pub input_bytes: usize,
}

pub fn load_frame(vfs: &mut Vfs, path: &str, index: usize) -> Result<LoadedFrame, Error> {
    read_art(vfs, path)?.decode(index)
}

pub(super) struct ArtResource {
    source: Source,
    bytes: Vec<u8>,
}

pub(super) fn read_art(vfs: &mut Vfs, path: &str) -> Result<ArtResource, Error> {
    let (source, bytes) = read_bounded(vfs, path, art::MAX_INPUT_BYTES)?;
    Ok(ArtResource { source, bytes })
}

impl ArtResource {
    pub(super) fn decode(&self, index: usize) -> Result<LoadedFrame, Error> {
        self.decode_with_palette(index, 0)
    }

    pub(super) fn decode_with_palette(
        &self,
        index: usize,
        palette: u8,
    ) -> Result<LoadedFrame, Error> {
        self.decode_direction(index, 0, palette)
    }

    pub(super) fn decode_direction(
        &self,
        index: usize,
        rotation: u8,
        palette: u8,
    ) -> Result<LoadedFrame, Error> {
        match art::decode_direction(&self.bytes, index, rotation, palette) {
            Ok(decoded) => Ok(LoadedFrame {
                source: self.source.clone(),
                decoded,
                input_bytes: self.bytes.len(),
            }),
            Err(error) => Err(Error::Art {
                provenance: Box::new(self.source.clone()),
                error,
            }),
        }
    }
}

use std::{
    collections::BTreeMap,
    error::Error as StdError,
    sync::Arc,
    time::{Duration, Instant},
};

const FRAME_CACHE_BYTES: usize = 96 * 1024 * 1024;
const FRAME_CACHE_ENTRIES: usize = 4096;

#[derive(Clone, Copy, Debug, Default, serde::Serialize)]
pub struct FrameCacheStats {
    pub hits: u64,
    pub rejected_hits: u64,
    pub reads: u64,
    pub decodes: u64,
    pub evictions: u64,
    pub frames: usize,
    pub pixel_bytes: usize,
    pub resolution_time: Duration,
    pub read_time: Duration,
    pub decode_time: Duration,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Reflection {
    None,
    Rows,
    Critter,
}

type FrameKey = (String, usize, u8, Reflection, u8);

/// Scope this cache to one immutable set of VFS mounts.
#[derive(Default)]
pub(super) struct FrameCache {
    frames: BTreeMap<FrameKey, (Arc<LoadedFrame>, u64)>,
    last_input: Option<(String, ArtResource)>,
    rejected: BTreeMap<FrameKey, String>,
    clock: u64,
    stats: FrameCacheStats,
}

impl FrameCache {
    pub(super) fn stats(&self) -> FrameCacheStats {
        self.stats
    }

    pub(super) fn finish_sector(&mut self) {
        self.last_input = None;
    }

    pub(super) fn resolve<T>(&mut self, resolve: impl FnOnce() -> T) -> T {
        let start = Instant::now();
        let value = resolve();
        self.stats.resolution_time += start.elapsed();
        value
    }

    pub(super) fn load(
        &mut self,
        vfs: &mut Vfs,
        path: &str,
        index: usize,
        palette: u8,
        mirror: bool,
    ) -> Result<Arc<LoadedFrame>, Box<dyn StdError>> {
        self.load_rotated(vfs, path, index, palette, mirror, 0)
    }

    pub(super) fn load_rotated(
        &mut self,
        vfs: &mut Vfs,
        path: &str,
        index: usize,
        palette: u8,
        mirror: bool,
        rotation: u8,
    ) -> Result<Arc<LoadedFrame>, Box<dyn StdError>> {
        let reflection = if mirror {
            Reflection::Rows
        } else {
            Reflection::None
        };
        self.load_with_reflection(vfs, path, index, palette, rotation, reflection)
    }

    pub(super) fn load_with_reflection(
        &mut self,
        vfs: &mut Vfs,
        path: &str,
        index: usize,
        palette: u8,
        rotation: u8,
        reflection: Reflection,
    ) -> Result<Arc<LoadedFrame>, Box<dyn StdError>> {
        self.clock += 1;
        let mut key = (path.to_owned(), index, palette, reflection, rotation);
        if rotation != 0 && rotation < 8 {
            key.4 = 0;
            if !self
                .frames
                .get(&key)
                .is_some_and(|(frame, _)| frame.decoded.rotation_count == 1)
            {
                key.4 = rotation;
            }
        }
        if let Some((frame, stamp)) = self.frames.get_mut(&key) {
            self.stats.hits += 1;
            *stamp = self.clock;
            return Ok(Arc::clone(frame));
        }
        if let Some(error) = self.rejected.get(&key) {
            self.stats.rejected_hits += 1;
            return Err(error.clone().into());
        }
        if self
            .last_input
            .as_ref()
            .is_none_or(|(name, _)| name != path)
        {
            self.stats.reads += 1;
            let start = Instant::now();
            let input = read_art(vfs, path);
            self.stats.read_time += start.elapsed();
            let input = match input {
                Ok(input) => input,
                Err(error) => return Err(self.reject(key, error.to_string())),
            };
            self.last_input = Some((path.to_owned(), input));
        }
        self.stats.decodes += 1;
        let start = Instant::now();
        let result = self
            .last_input
            .as_ref()
            .expect("ART input loaded")
            .1
            .decode_direction(index, rotation, palette);
        self.stats.decode_time += start.elapsed();
        let mut frame = match result {
            Ok(frame) => frame,
            Err(error) => return Err(self.reject(key, error.to_string())),
        };
        if reflection != Reflection::None {
            let frame = &mut frame.decoded.frame;
            if reflection == Reflection::Critter {
                frame.hotspot.0 = (frame.width as i32 - 1)
                    .checked_sub(frame.hotspot.0)
                    .ok_or("reflected critter hotspot overflow")?;
                frame.offset.0 = frame
                    .offset
                    .0
                    .checked_neg()
                    .ok_or("reflected critter offset overflow")?;
            }
            mirror_rows(frame);
        }
        let bytes = frame.decoded.frame.pixels.capacity() * size_of::<[u8; 4]>();
        while self.stats.pixel_bytes + bytes > FRAME_CACHE_BYTES
            || self.frames.len() >= FRAME_CACHE_ENTRIES
        {
            let key = self
                .frames
                .iter()
                .filter(|(_, (frame, _))| Arc::strong_count(frame) == 1)
                .min_by_key(|(_, (_, stamp))| *stamp)
                .map(|(key, _)| key.clone())
                .ok_or("decoded world ART cache budget exhausted by retained frames")?;
            let (old, _) = self.frames.remove(&key).expect("selected cached frame");
            self.stats.pixel_bytes -= old.decoded.frame.pixels.capacity() * size_of::<[u8; 4]>();
            self.stats.evictions += 1;
        }
        key.4 = frame.decoded.stored_rotation;
        let frame = Arc::new(frame);
        self.frames.insert(key, (Arc::clone(&frame), self.clock));
        self.stats.frames = self.frames.len();
        self.stats.pixel_bytes += bytes;
        Ok(frame)
    }

    fn reject(&mut self, key: FrameKey, error: String) -> Box<dyn StdError> {
        if self.rejected.len() < FRAME_CACHE_ENTRIES {
            self.rejected.insert(key, error.clone());
        }
        error.into()
    }
}

pub(super) fn mirror_rows(frame: &mut art::SpriteFrame) {
    for row in frame.pixels.chunks_exact_mut(frame.width as usize) {
        row.reverse();
    }
}

/// Prepared indexed ART loop. Time remains independent of resource loading.
#[derive(Debug)]
pub(super) struct FrameLoop {
    pub(super) frames: Box<[Arc<LoadedFrame>]>,
    period_ms: u32,
}

impl FrameLoop {
    pub(super) fn new(frames: Vec<Arc<LoadedFrame>>, fps: u32) -> Result<Self, &'static str> {
        if !(1..=1000).contains(&fps) || !(2..=32).contains(&frames.len()) {
            return Err("invalid animation length or ART frame rate");
        }
        if frames
            .iter()
            .any(|frame| frame.decoded.frame.offset != (0, 0))
        {
            return Err("stationary animation with ART movement offsets is unsupported");
        }
        Ok(Self {
            frames: frames.into_boxed_slice(),
            period_ms: 1000 / fps,
        })
    }

    pub(super) fn period(&self) -> Duration {
        Duration::from_millis(u64::from(self.period_ms))
    }

    pub(super) fn index(&self, initial: usize, elapsed: Duration) -> usize {
        let step = elapsed.as_millis() / u128::from(self.period_ms);
        ((step + initial as u128) % self.frames.len() as u128) as usize
    }

    /// A one-shot sequence returns to its authored standing frame after
    /// consuming the last frame interval.
    pub(super) fn one_shot_index(&self, elapsed: Duration) -> Option<usize> {
        let step = elapsed.as_millis() / u128::from(self.period_ms);
        usize::try_from(step)
            .ok()
            .filter(|&index| index < self.frames.len())
    }

    /// Delay to the next boundary, or a later one when intervening frames
    /// are completely outside the viewport.
    pub(super) fn delay(&self, elapsed: Duration, steps: usize) -> Duration {
        let period = u128::from(self.period_ms) * 1_000_000;
        Duration::from_nanos((period * steps as u128 - elapsed.as_nanos() % period) as u64)
    }
}
