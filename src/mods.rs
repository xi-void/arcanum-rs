//! Local mod discovery, dependency ordering, and VFS mounting.

mod manifest;
mod plan;

use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::{
    ambient_authority,
    fs::{Dir, File, OpenOptions},
};
use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub use crate::mod_id::ModId;
use crate::vfs;
pub use manifest::{MANIFEST_NAME, Manifest, Mount};
pub use plan::LoadOrder;

#[derive(Debug)]
pub enum Error {
    Io {
        path: PathBuf,
        source: io::Error,
    },
    Toml {
        path: PathBuf,
        source: toml::de::Error,
    },
    Manifest {
        path: PathBuf,
        reason: String,
    },
    DuplicateId {
        id: ModId,
        first: PathBuf,
        second: PathBuf,
    },
    Dependency {
        id: ModId,
        reason: String,
    },
    UnknownMod(String),
    DuplicateSelection(String),
    Cycle(String),
    Mount {
        id: ModId,
        source: vfs::Error,
    },
    RequiresOrderedVfs,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => write!(f, "{}: {source}", path.display()),
            Self::Toml { path, source } => write!(f, "{}: {source}", path.display()),
            Self::Manifest { path, reason } => write!(f, "{}: {reason}", path.display()),
            Self::DuplicateId { id, first, second } => write!(
                f,
                "duplicate mod ID {id:?} in {} and {}",
                first.display(),
                second.display()
            ),
            Self::Dependency { id, reason } => write!(f, "mod {id:?}: {reason}"),
            Self::UnknownMod(id) => write!(f, "selected mod {id:?} is not installed"),
            Self::DuplicateSelection(id) => write!(f, "mod {id:?} was selected more than once"),
            Self::Cycle(chain) => write!(f, "load-order cycle: {chain}"),
            Self::Mount { id, source } => write!(f, "mod {id:?}: {source}"),
            Self::RequiresOrderedVfs => f.write_str("mod packages require an ordered VFS"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Toml { source, .. } => Some(source),
            Self::Mount { source, .. } => Some(source),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub struct Package {
    path: PathBuf,
    root: Dir,
    manifest: Manifest,
}

impl Package {
    /// Read the manifest in an explicitly chosen package directory.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, Error> {
        let (path, root) = open_root(path.as_ref())?;
        Self::read(path, root)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn read(path: PathBuf, root: Dir) -> Result<Self, Error> {
        let manifest = Manifest::read(&root, &path)?;
        Ok(Self {
            path,
            root,
            manifest,
        })
    }
}

#[derive(Debug)]
pub struct Catalog {
    packages: BTreeMap<ModId, Package>,
}

impl Catalog {
    /// Discover immediate, non-symlink package directories containing a manifest.
    pub fn discover(path: impl AsRef<Path>) -> Result<Self, Error> {
        let (path, root) = open_root(path.as_ref())?;
        let mut packages = BTreeMap::<ModId, Package>::new();
        let mut entries = root
            .entries()
            .map_err(|source| io_error(&path, source))?
            .collect::<io::Result<Vec<_>>>()
            .map_err(|source| io_error(&path, source))?;
        entries.sort_unstable_by_key(|entry| entry.file_name());
        for entry in entries {
            let name = entry.file_name();
            let child = path.join(&name);
            if !entry
                .file_type()
                .map_err(|source| io_error(&child, source))?
                .is_dir()
            {
                continue;
            }
            let dir = root
                .open_dir_nofollow(&name)
                .map_err(|source| io_error(&child, source))?;
            match dir.symlink_metadata(MANIFEST_NAME) {
                Ok(_) => {}
                Err(err) if err.kind() == io::ErrorKind::NotFound => continue,
                Err(err) => return Err(io_error(&child.join(MANIFEST_NAME), err)),
            }
            let package = Package::read(child, dir)?;
            let id = &package.manifest.id;
            if let Some(first) = packages.get(id) {
                return Err(Error::DuplicateId {
                    id: id.clone(),
                    first: first.path.clone(),
                    second: package.path,
                });
            }
            packages.insert(id.clone(), package);
        }
        Ok(Self { packages })
    }

    /// Installed packages ordered by mod ID.
    pub fn packages(&self) -> impl Iterator<Item = &Package> {
        self.packages.values()
    }

    /// Order every installed package when selection is empty. Otherwise include
    /// the selected IDs and their transitive dependencies.
    pub fn load_order(&self, selected: &[&str]) -> Result<LoadOrder<'_>, Error> {
        plan::resolve(&self.packages, selected)
    }
}

fn open_root(path: &Path) -> Result<(PathBuf, Dir), Error> {
    let absolute = fs::canonicalize(path).map_err(|source| io_error(path, source))?;
    let dir = Dir::open_ambient_dir(&absolute, ambient_authority())
        .map_err(|source| io_error(&absolute, source))?;
    Ok((absolute, dir))
}

// Callers supply one component relative to a capability. Reject links at the
// actual open as well as at the type check.
fn open_regular(dir: &Dir, name: &Path) -> io::Result<File> {
    let invalid = || {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "expected a regular file; symlinks are forbidden",
        )
    };
    if !dir.symlink_metadata(name)?.is_file() {
        return Err(invalid());
    }
    let file = dir.open_with(
        name,
        OpenOptions::new().read(true).follow(FollowSymlinks::No),
    )?;
    if !file.metadata()?.is_file() {
        return Err(invalid());
    }
    Ok(file)
}

fn open_directory(dir: &Dir, name: &Path) -> io::Result<Dir> {
    if !dir.symlink_metadata(name)?.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "expected a directory; symlinks are forbidden",
        ));
    }
    dir.open_dir_nofollow(name)
}

fn io_error(path: &Path, source: io::Error) -> Error {
    Error::Io {
        path: path.into(),
        source,
    }
}

#[cfg(test)]
mod tests;
