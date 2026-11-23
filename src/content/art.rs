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
