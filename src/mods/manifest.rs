use super::{Error, ModId, io_error, open_regular};
use crate::{resource_path, vfs::MountKind};
use cap_std::fs::Dir;
use semver::{Version, VersionReq};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Path, PathBuf};

pub const MANIFEST_NAME: &str = "arcanum-mod.toml";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub manifest_version: u32,
    pub id: ModId,
    pub name: String,
    pub version: Version,
    pub description: String,
    #[serde(default)]
    pub dependencies: BTreeMap<ModId, VersionReq>,
    #[serde(default)]
    pub load_after: Vec<ModId>,
    #[serde(default)]
    pub load_before: Vec<ModId>,
    #[serde(default)]
    pub mounts: Vec<Mount>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mount {
    pub kind: MountKind,
    pub path: PathBuf,
}

impl Manifest {
    pub(super) fn read(root: &Dir, package: &Path) -> Result<Self, Error> {
        const MAX_LENGTH: u64 = 1024 * 1024;
        let path = package.join(MANIFEST_NAME);
        let invalid = |reason| Error::Manifest {
            path: path.clone(),
            reason,
        };
        let mut text = String::new();
        open_regular(root, Path::new(MANIFEST_NAME))
            .and_then(|file| file.take(MAX_LENGTH + 1).read_to_string(&mut text))
            .map_err(|source| io_error(&path, source))?;
        if text.len() as u64 > MAX_LENGTH {
            return Err(invalid("manifest exceeds 1 MiB".into()));
        }

        #[derive(Deserialize)]
        struct Header {
            manifest_version: i64,
        }
        let toml_error = |source| Error::Toml {
            path: path.clone(),
            source,
        };
        let header: Header = toml::from_str(&text).map_err(toml_error)?;
        if header.manifest_version != 1 {
            return Err(invalid(format!(
                "unsupported manifest_version {}; expected 1",
                header.manifest_version
            )));
        }
        let mut manifest: Self = toml::from_str(&text).map_err(toml_error)?;
        manifest.validate().map_err(invalid)?;
        Ok(manifest)
    }

    fn validate(&mut self) -> Result<(), String> {
        if self.name.trim().is_empty() || self.name.chars().any(char::is_control) {
            return Err("name must contain a display name without control characters".into());
        }
        if self
            .description
            .chars()
            .any(|ch| ch.is_control() && !matches!(ch, '\n' | '\r' | '\t'))
        {
            return Err("description contains a control character".into());
        }
        for (field, ids) in [
            ("load_after", &self.load_after),
            ("load_before", &self.load_before),
        ] {
            let mut seen = BTreeSet::new();
            for id in ids {
                if !seen.insert(id) {
                    return Err(format!("{field} lists {id:?} more than once"));
                }
            }
        }
        let mut mounts = BTreeSet::new();
        for (index, mount) in self.mounts.iter_mut().enumerate() {
            let text = mount.path.to_str().expect("TOML paths are UTF-8");
            mount.path = mount_path(text).ok_or_else(|| format!(
                "mounts[{index}].path {text:?} must be a portable, package-relative path without parent components"
            ))?;
            if !mounts.insert(mount.path.clone()) {
                return Err(format!(
                    "mounts[{index}] repeats the mount path {}",
                    mount.path.display()
                ));
            }
        }
        Ok(())
    }
}

fn mount_path(text: &str) -> Option<PathBuf> {
    // Apply the virtual-path safety rules but preserve the host file spelling.
    resource_path::normalize(text).ok()?;
    let mut path = PathBuf::new();
    for component in text
        .split(['/', '\\'])
        .filter(|part| !matches!(*part, "" | "."))
    {
        if component.ends_with(['.', ' ']) || component.contains(['*', '?', '"', '<', '>', '|']) {
            return None;
        }
        path.push(component);
    }
    Some(path)
}
