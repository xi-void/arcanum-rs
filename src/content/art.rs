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
    let (source, bytes) = read_bounded(vfs, path, art::MAX_INPUT_BYTES)?;
    match art::decode_frame(&bytes, index) {
        Ok(decoded) => Ok(LoadedFrame {
            source,
            decoded,
            input_bytes: bytes.len(),
        }),
        Err(error) => Err(Error::Art {
            provenance: Box::new(source),
            error,
        }),
    }
}
