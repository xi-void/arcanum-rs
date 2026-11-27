use crate::resource_path;
use cap_std::fs::Dir;
use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

pub(super) struct LooseFile {
    pub relative: PathBuf,
    pub size: u64,
}

pub(super) struct LooseDirectory {
    root: Dir,
    pub files: BTreeMap<String, Vec<LooseFile>>,
}

impl LooseDirectory {
    pub fn new(root: Dir, skip_cache: bool) -> io::Result<Self> {
        let mut files = BTreeMap::<String, Vec<LooseFile>>::new();
        let mut pending = vec![(PathBuf::new(), String::new())];
        while let Some((relative, prefix)) = pending.pop() {
            let mut entries = if relative.as_os_str().is_empty() {
                root.entries()?
            } else {
                root.read_dir(&relative)?
            }
            .collect::<io::Result<Vec<_>>>()?;
            entries.sort_unstable_by_key(|entry| entry.file_name());
            for entry in entries {
                let name = entry.file_name();
                let Some(name) = name.to_str() else { continue };
                // On Unix a literal backslash in a host component is not a
                // virtual separator and must not alias a nested resource.
                if name.contains('\\') || (skip_cache && name.eq_ignore_ascii_case("TIGCache")) {
                    continue;
                }
                let Ok(component) = resource_path::normalize(name) else {
                    continue;
                };
                let kind = entry.file_type()?;
                let child = relative.join(name);
                let key = if prefix.is_empty() {
                    component
                } else {
                    format!("{prefix}/{component}")
                };
                if kind.is_dir() {
                    pending.push((child, key));
                } else if kind.is_file() {
                    let size = entry.metadata()?.len();
                    files.entry(key).or_default().push(LooseFile {
                        relative: child,
                        size,
                    });
                }
            }
        }
        for variants in files.values_mut() {
            variants.sort_unstable_by(|a, b| a.relative.cmp(&b.relative));
        }
        Ok(Self { root, files })
    }

    pub fn open(&self, relative: &Path) -> io::Result<std::fs::File> {
        if !self.root.symlink_metadata(relative)?.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "loose resource is no longer a regular file",
            ));
        }
        // All opens are relative to the directory capability, including after
        // indexing. A symlink swap cannot redirect a read outside the mount.
        let file = self.root.open(relative)?;
        if !file.metadata()?.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "loose resource is no longer a regular file",
            ));
        }
        Ok(file.into_std())
    }

    pub fn copy(&self, relative: &Path, mut output: impl Write) -> io::Result<u64> {
        io::copy(&mut self.open(relative)?, &mut output)
    }
}
