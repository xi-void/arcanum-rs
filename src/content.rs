//! Bounded loading of resolved content from a caller-owned virtual filesystem.

use crate::mes::{self, Encoding, Mes};
use crate::mod_id::ModId;
use crate::vfs::{self, MountId, Storage, Vfs};
use std::fmt;
use std::io::{self, Write};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    pub virtual_path: String,
    pub mount_id: MountId,
    pub mount_path: PathBuf,
    /// Actual on-disk spelling relative to a loose mount.
    pub loose_path: Option<PathBuf>,
    pub storage: Storage,
    pub mod_id: Option<ModId>,
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(relative) = &self.loose_path {
            write!(f, "{}", self.mount_path.join(relative).display())?;
        } else {
            write!(f, "{}", self.mount_path.display())?;
        }
        write!(f, " (mount {}, {})", self.mount_id, self.storage)
    }
}

#[derive(Debug)]
pub struct LoadedMes {
    pub source: Source,
    pub mes: Mes,
    pub encoding: Encoding,
    pub duplicates: Vec<mes::Duplicate>,
}

#[derive(Debug)]
pub enum Error {
    Resource {
        path: String,
        error: vfs::Error,
    },
    TooLarge {
        provenance: Box<Source>,
        limit: usize,
    },
    Mes {
        provenance: Box<Source>,
        error: mes::Error,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Resource { path, error } => write!(f, "loading {path:?}: {error}"),
            Self::TooLarge { provenance, limit } => write!(
                f,
                "{} from {provenance} exceeds the {limit}-byte MES limit",
                provenance.virtual_path
            ),
            Self::Mes { provenance, error } => {
                write!(f, "{} from {provenance}", provenance.virtual_path)?;
                if let Some(id) = &provenance.mod_id {
                    write!(f, " [mod {id}]")?;
                }
                write!(f, ": {error}")
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Resource { error, .. } => Some(error),
            Self::Mes { error, .. } => Some(error),
            Self::TooLarge { .. } => None,
        }
    }
}

/// Parse the winning resource with automatic encoding selection.
pub fn load_mes(vfs: &mut Vfs, path: &str) -> Result<LoadedMes, Error> {
    load(vfs, path, None)
}

/// Parse the winning resource with an explicit text encoding.
pub fn load_mes_with_encoding(
    vfs: &mut Vfs,
    path: &str,
    encoding: Encoding,
) -> Result<LoadedMes, Error> {
    load(vfs, path, Some(encoding))
}

fn load(vfs: &mut Vfs, path: &str, encoding: Option<Encoding>) -> Result<LoadedMes, Error> {
    let resolution = vfs.resolve(path).map_err(|error| Error::Resource {
        path: path.into(),
        error,
    })?;
    let winner = resolution.winner().ok_or_else(|| Error::Resource {
        path: resolution.path.clone(),
        error: vfs::Error::NotFound(resolution.path.clone()),
    })?;
    let source = Source {
        virtual_path: resolution.path,
        mount_id: winner.mount.id,
        mount_path: winner.mount.path.clone(),
        loose_path: winner.loose_path.map(PathBuf::from),
        storage: winner.storage,
        mod_id: winner.mount.mod_id.clone(),
    };
    let too_large = |source| Error::TooLarge {
        provenance: Box::new(source),
        limit: mes::MAX_INPUT_BYTES,
    };
    if winner.size > mes::MAX_INPUT_BYTES as u64 {
        return Err(too_large(source));
    }

    let mut input = BoundedBytes::default();
    // The mounted indexes remain fixed while copying. VFS performs the same
    // lookup and opens only the selected resource, including across mod mounts.
    if let Err(error) = vfs.copy(&source.virtual_path, &mut input) {
        if input.exceeded {
            return Err(too_large(source));
        }
        return Err(Error::Resource {
            path: source.virtual_path,
            error,
        });
    }
    match mes::parse_source_with_encoding(&input.bytes, encoding) {
        Ok(parsed) => Ok(LoadedMes {
            source,
            mes: parsed.mes,
            encoding: parsed.encoding,
            duplicates: parsed.duplicates,
        }),
        Err(error) => Err(Error::Mes {
            provenance: Box::new(source),
            error,
        }),
    }
}

#[derive(Default)]
struct BoundedBytes {
    bytes: Vec<u8>,
    exceeded: bool,
}

impl Write for BoundedBytes {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > mes::MAX_INPUT_BYTES - self.bytes.len() {
            self.exceeded = true;
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "MES input limit exceeded",
            ));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests;
