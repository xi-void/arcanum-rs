//! Read-only resource resolution over ordered archive and directory mounts.

mod game;
mod ignore;
mod loose;

use crate::{dat, mod_id::ModId, resource_path};
use cap_std::{ambient_authority, fs::Dir};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use dat::{Archive, EntryKind};
use ignore::IgnoreRules;
use loose::LooseDirectory;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Policy {
    /// Stable relative-path binary lookup under CE's TIG repository rules.
    Ce,
    /// Later mounts override earlier ones. Sidecars and directory records have
    /// no effect on file precedence.
    Ordered,
}

impl fmt::Display for Policy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Ce => "CE",
            Self::Ordered => "ordered",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MountId(usize);

impl MountId {
    /// Provenance marker for generated render assets, which have no VFS mount.
    pub const GENERATED: Self = Self(usize::MAX);
}

impl fmt::Display for MountId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MountKind {
    Archive,
    Directory,
}

impl fmt::Display for MountKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Archive => "archive",
            Self::Directory => "loose directory",
        })
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct MountInfo {
    pub id: MountId,
    pub kind: MountKind,
    pub path: PathBuf,
    pub mod_id: Option<ModId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Storage {
    Archive(EntryKind),
    Loose,
}

impl fmt::Display for Storage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Archive(kind) => kind.fmt(f),
            Self::Loose => f.write_str("loose"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exclusion {
    IgnoreSidecar,
    IgnoredEntry,
    DirectoryRecord,
    UnsupportedEntry,
}

impl fmt::Display for Exclusion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::IgnoreSidecar => "archive .ignore sidecar",
            Self::IgnoredEntry => "archive ignored flag",
            Self::DirectoryRecord => "directory metadata",
            Self::UnsupportedEntry => "unsupported archive flags",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Candidate<'a> {
    pub mount: &'a MountInfo,
    pub storage: Storage,
    pub size: u64,
    pub loose_path: Option<&'a Path>,
    pub exclusion: Option<Exclusion>,
}

#[derive(Debug)]
pub struct Resolution<'a> {
    pub path: String,
    candidates: Vec<Candidate<'a>>,
    winner: Option<usize>,
}

impl<'a> Resolution<'a> {
    /// All matching records, ordered from highest to lowest mount priority.
    pub fn candidates(&self) -> &[Candidate<'a>] {
        &self.candidates
    }

    /// In CE mode a winning archive directory record identifies a zero-byte
    /// TIG handle. Use `copy` to read file payloads.
    pub fn winner(&self) -> Option<Candidate<'a>> {
        self.winner.map(|index| self.candidates[index])
    }
}

#[derive(Debug)]
pub enum Error {
    InvalidPath(String),
    NotFound(String),
    NotAFile(String),
    Io { path: PathBuf, source: io::Error },
    Archive { path: PathBuf, source: dat::Error },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPath(path) => write!(
                f,
                "invalid virtual resource path {path:?}: {}",
                resource_path::InvalidResourcePath
            ),
            Self::NotFound(path) => write!(f, "resource {path:?} not found"),
            Self::NotAFile(path) => write!(
                f,
                "resource {path:?} resolves to an archive directory record"
            ),
            Self::Io { path, source } => write!(f, "{}: {source}", path.display()),
            Self::Archive { path, source } => write!(f, "{}: {source}", path.display()),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Archive { source, .. } => Some(source),
            _ => None,
        }
    }
}

struct Mount {
    info: MountInfo,
    contents: Contents,
}

enum Contents {
    Archive {
        archive: Archive<File>,
        ignore: IgnoreRules,
    },
    Directory(LooseDirectory),
}

pub(crate) enum PackageSource {
    Archive(File),
    Directory(Dir),
}

pub struct Vfs {
    policy: Policy,
    // The newest mount is last. Re-registering preserves its identifier.
    mounts: Vec<Mount>,
    next_id: usize,
    read_attempts: u64,
}

impl Vfs {
    pub fn new(policy: Policy) -> Self {
        Self {
            policy,
            mounts: Vec::new(),
            next_id: 1,
            read_attempts: 0,
        }
    }

    pub fn policy(&self) -> Policy {
        self.policy
    }

    /// Mounts in the order in which the resolver considers them.
    pub fn mounts(&self) -> impl Iterator<Item = &MountInfo> {
        self.mounts.iter().rev().map(|mount| &mount.info)
    }

    /// Add a DAT/PATCH file supplied by the caller, independently of its suffix.
    pub fn mount_archive(&mut self, path: impl AsRef<Path>) -> Result<MountId, Error> {
        let path = canonicalize(path.as_ref())?;
        let Some((parent, filename)) = path.parent().zip(path.file_name()) else {
            return Err(io_error(
                &path,
                io::Error::new(io::ErrorKind::InvalidInput, "expected an archive file"),
            ));
        };
        let dir = ambient_dir(parent)?;
        self.add_archive(&dir, parent, Path::new(filename))
    }

    /// Add a snapshot of the regular files under a loose mount root.
    pub fn mount_directory(&mut self, path: impl AsRef<Path>) -> Result<MountId, Error> {
        let path = canonicalize(path.as_ref())?;
        let dir = ambient_dir(&path)?;
        self.add_directory(dir, path)
    }

    pub(crate) fn mount_package_source(
        &mut self,
        mod_id: &ModId,
        path: PathBuf,
        source: PackageSource,
    ) -> Result<(), Error> {
        debug_assert_eq!(self.policy, Policy::Ordered);
        let (kind, contents) = match source {
            PackageSource::Archive(file) => (
                MountKind::Archive,
                Contents::Archive {
                    archive: read_archive(&path, file)?,
                    ignore: IgnoreRules::default(),
                },
            ),
            PackageSource::Directory(root) => {
                (MountKind::Directory, self.read_directory(&path, root)?)
            }
        };
        self.push(path, kind, contents, Some(mod_id.clone()));
        Ok(())
    }

    /// Inspect a virtual path, including entries suppressed by the active policy.
    pub fn resolve(&self, path: &str) -> Result<Resolution<'_>, Error> {
        let path = resource_path::normalize(path).map_err(|_| Error::InvalidPath(path.into()))?;
        Ok(self.resolve_normalized(path))
    }

    /// Indexed names under a normalized prefix. Call `resolve` to select a source.
    pub(crate) fn paths_with_prefix(&self, prefix: &str) -> BTreeSet<&str> {
        let mut paths = BTreeSet::new();
        for mount in &self.mounts {
            match &mount.contents {
                Contents::Archive { archive, .. } => paths.extend(
                    archive
                        .entries_with_prefix(prefix)
                        .map(|entry| entry.path.as_str()),
                ),
                Contents::Directory(directory) => paths.extend(
                    directory
                        .files
                        .range(prefix.to_owned()..)
                        .map(|(path, _)| path.as_str())
                        .take_while(|path| path.starts_with(prefix)),
                ),
            }
        }
        paths
    }

    /// Inspect paths with multiple entries in normalized lexical order.
    pub fn conflicts(&self) -> impl Iterator<Item = Resolution<'_>> + '_ {
        let mut counts = BTreeMap::<&str, usize>::new();
        for mount in &self.mounts {
            match &mount.contents {
                Contents::Archive { archive, .. } => {
                    for entry in archive.entries() {
                        *counts.entry(&entry.path).or_default() += 1;
                    }
                }
                Contents::Directory(directory) => {
                    for (path, files) in &directory.files {
                        *counts.entry(path).or_default() += files.len();
                    }
                }
            }
        }
        counts
            .into_iter()
            .filter(|(_, count)| *count > 1)
            .map(|(path, _)| self.resolve_normalized(path.to_owned()))
    }

    /// Number of resource stream attempts since this VFS was created.
    pub fn read_attempts(&self) -> u64 {
        self.read_attempts
    }

    /// Open the winning loose resource for seekable streaming. Packed resources
    /// remain accessible through `copy`, under the same precedence rules.
    pub fn open_loose(&self, path: &str) -> Result<Option<std::fs::File>, Error> {
        let resolution = self.resolve(path)?;
        let winner = resolution
            .winner()
            .ok_or_else(|| Error::NotFound(resolution.path.clone()))?;
        let Some(relative) = winner.loose_path else {
            return Ok(None);
        };
        let mount = self
            .mounts
            .iter()
            .find(|m| m.info.id == winner.mount.id)
            .unwrap();
        let Contents::Directory(directory) = &mount.contents else {
            unreachable!()
        };
        directory
            .open(relative)
            .map(Some)
            .map_err(|e| io_error(&mount.info.path.join(relative), e))
    }

    /// Stream the winning regular resource to a writer.
    pub fn copy(&mut self, path: &str, output: impl Write) -> Result<u64, Error> {
        self.read_attempts += 1;
        let resolution = self.resolve(path)?;
        let winner = resolution
            .winner()
            .ok_or_else(|| Error::NotFound(resolution.path.clone()))?;
        if winner.storage == Storage::Archive(EntryKind::Directory) {
            return Err(Error::NotAFile(resolution.path));
        }
        let id = winner.mount.id;
        let relative = winner.loose_path.map(Path::to_owned);
        let path = resolution.path;
        let mount = self
            .mounts
            .iter_mut()
            .find(|mount| mount.info.id == id)
            .expect("a resolution always references a mounted source");
        match &mut mount.contents {
            Contents::Archive { archive, .. } => archive
                .extract(&path, output)
                .map_err(|source| archive_error(&mount.info.path, source)),
            Contents::Directory(dir) => {
                let relative =
                    relative.expect("a loose candidate always carries its relative path");
                dir.copy(&relative, output)
                    .map_err(|source| io_error(&mount.info.path.join(relative), source))
            }
        }
    }

    fn resolve_normalized(&self, path: String) -> Resolution<'_> {
        let mut candidates = Vec::new();
        for mount in self.mounts.iter().rev() {
            match &mount.contents {
                Contents::Archive { archive, ignore } => {
                    if let Some(entry) = archive.entry_normalized(&path) {
                        let exclusion = match entry.kind {
                            EntryKind::Ignored => Some(Exclusion::IgnoredEntry),
                            _ if ignore.matches(&path) => Some(Exclusion::IgnoreSidecar),
                            EntryKind::Directory if self.policy == Policy::Ordered => {
                                Some(Exclusion::DirectoryRecord)
                            }
                            EntryKind::Unsupported => Some(Exclusion::UnsupportedEntry),
                            _ => None,
                        };
                        candidates.push(Candidate {
                            mount: &mount.info,
                            storage: Storage::Archive(entry.kind),
                            size: entry.size.into(),
                            loose_path: None,
                            exclusion,
                        });
                    }
                }
                Contents::Directory(dir) => {
                    if let Some(files) = dir.files.get(&path) {
                        candidates.extend(files.iter().map(|file| Candidate {
                            mount: &mount.info,
                            storage: Storage::Loose,
                            size: file.size,
                            loose_path: Some(&file.relative),
                            exclusion: None,
                        }));
                    }
                }
            }
        }
        // For stable files the highest eligible archive anchors CE's search.
        // Only loose directories ahead of it can override it. With no archive,
        // the first loose file wins. Both reduce to this first eligible record.
        let winner = candidates
            .iter()
            .position(|candidate| candidate.exclusion.is_none());
        Resolution {
            path,
            candidates,
            winner,
        }
    }

    fn add_archive(&mut self, root: &Dir, base: &Path, relative: &Path) -> Result<MountId, Error> {
        let path = base.join(relative);
        if let Some(id) = self.promote(&path) {
            return Ok(id);
        }
        if !root
            .symlink_metadata(relative)
            .map_err(|source| io_error(&path, source))?
            .is_file()
        {
            return Err(io_error(
                &path,
                io::Error::new(io::ErrorKind::InvalidInput, "expected an archive file"),
            ));
        }
        let file = root
            .open(relative)
            .map_err(|source| io_error(&path, source))?;
        let archive = read_archive(&path, file.into_std())?;
        let ignore = if self.policy == Policy::Ce {
            IgnoreRules::load(root, relative)
                .map_err(|source| io_error(&base.join(ignore::sidecar(relative)), source))?
        } else {
            IgnoreRules::default()
        };
        Ok(self.push(
            path,
            MountKind::Archive,
            Contents::Archive { archive, ignore },
            None,
        ))
    }

    fn add_directory(&mut self, root: Dir, path: PathBuf) -> Result<MountId, Error> {
        if let Some(id) = self.promote(&path) {
            return Ok(id);
        }
        let contents = self.read_directory(&path, root)?;
        Ok(self.push(path, MountKind::Directory, contents, None))
    }

    fn read_directory(&self, path: &Path, root: Dir) -> Result<Contents, Error> {
        LooseDirectory::new(root, self.policy == Policy::Ce)
            .map(Contents::Directory)
            .map_err(|source| io_error(path, source))
    }

    fn promote(&mut self, path: &Path) -> Option<MountId> {
        let index = self.mounts.iter().position(|mount| {
            mount.info.mod_id.is_none()
                && (mount.info.path == path
                    || (self.policy == Policy::Ce
                        && mount
                            .info
                            .path
                            .as_os_str()
                            .eq_ignore_ascii_case(path.as_os_str())))
        })?;
        let mount = self.mounts.remove(index);
        let id = mount.info.id;
        self.mounts.push(mount);
        Some(id)
    }

    fn push(
        &mut self,
        path: PathBuf,
        kind: MountKind,
        contents: Contents,
        mod_id: Option<ModId>,
    ) -> MountId {
        let id = MountId(self.next_id);
        self.next_id += 1;
        self.mounts.push(Mount {
            info: MountInfo {
                id,
                kind,
                path,
                mod_id,
            },
            contents,
        });
        id
    }
}

fn read_archive(path: &Path, file: File) -> Result<Archive<File>, Error> {
    Archive::new(file).map_err(|source| archive_error(path, source))
}

fn canonicalize(path: &Path) -> Result<PathBuf, Error> {
    fs::canonicalize(path).map_err(|source| io_error(path, source))
}

fn ambient_dir(path: &Path) -> Result<Dir, Error> {
    Dir::open_ambient_dir(path, ambient_authority()).map_err(|source| io_error(path, source))
}

fn io_error(path: &Path, source: io::Error) -> Error {
    Error::Io {
        path: path.to_owned(),
        source,
    }
}

fn archive_error(path: &Path, source: dat::Error) -> Error {
    Error::Archive {
        path: path.to_owned(),
        source,
    }
}

#[cfg(test)]
mod tests;
