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
        match art::decode_frame_with_palette(&self.bytes, index, palette) {
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

#[derive(Clone, Copy, Debug, Default)]
pub struct FrameCacheStats {
    pub hits: u64,
    pub rejected_hits: u64,
    pub reads: u64,
    pub decodes: u64,
    pub evictions: u64,
    pub frames: usize,
    pub pixel_bytes: usize,
    pub read_time: Duration,
    pub decode_time: Duration,
}

type FrameKey = (String, usize, u8, bool);

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

    pub(super) fn load(
        &mut self,
        vfs: &mut Vfs,
        path: &str,
        index: usize,
        palette: u8,
        mirror: bool,
    ) -> Result<Arc<LoadedFrame>, Box<dyn StdError>> {
        self.clock += 1;
        let key = (path.to_owned(), index, palette, mirror);
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
            .decode_with_palette(index, palette);
        self.stats.decode_time += start.elapsed();
        let mut frame = match result {
            Ok(frame) => frame,
            Err(error) => return Err(self.reject(key, error.to_string())),
        };
        if mirror {
            mirror_rows(&mut frame.decoded.frame);
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
