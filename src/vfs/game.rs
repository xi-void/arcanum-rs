use super::{Error, Policy, Vfs, ambient_dir, canonicalize, io_error};
use cap_std::fs::{Dir, FileType};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::Path;

struct Child {
    name: OsString,
    kind: FileType,
}

impl Vfs {
    /// Mount the installed base game and the stock Arcanum campaign.
    /// Missing optional sources are skipped without creating any directories.
    pub fn from_game(path: impl AsRef<Path>, policy: Policy) -> Result<Self, Error> {
        let path = canonicalize(path.as_ref())?;
        let root = ambient_dir(&path)?;
        let children = children(&root, &path)?;
        let mut vfs = Self::new(policy);

        if let Some(tig) = children.get("tig.dat").filter(|child| child.kind.is_file()) {
            vfs.add_archive(&root, &path, Path::new(&tig.name))?;
        }
        vfs.add_directory(
            root.try_clone().map_err(|source| io_error(&path, source))?,
            path.clone(),
        )?;
        for (key, child) in &children {
            if key.starts_with("arcanum") && key.ends_with(".dat") && child.kind.is_file() {
                vfs.add_archive(&root, &path, Path::new(&child.name))?;
            }
        }
        if let Some(data) = children.get("data").filter(|child| child.kind.is_dir()) {
            let data_path = path.join(&data.name);
            let data = root
                .open_dir(&data.name)
                .map_err(|source| io_error(&data_path, source))?;
            vfs.add_directory(data, data_path)?;
        }
        if let Some(modules) = children.get("modules").filter(|child| child.kind.is_dir()) {
            let modules_path = path.join(&modules.name);
            let modules = root
                .open_dir(&modules.name)
                .map_err(|source| io_error(&modules_path, source))?;
            vfs.add_stock_module(&modules, &modules_path)?;
        }
        Ok(vfs)
    }

    fn add_stock_module(&mut self, root: &Dir, path: &Path) -> Result<(), Error> {
        let children = children(root, path)?;
        if let Some(base) = children
            .get("arcanum.dat")
            .filter(|child| child.kind.is_file())
        {
            self.add_archive(root, path, Path::new(&base.name))?;
            for number in 0..10 {
                if let Some(patch) = children
                    .get(&format!("arcanum.patch{number}"))
                    .filter(|child| child.kind.is_file())
                {
                    self.add_archive(root, path, Path::new(&patch.name))?;
                }
            }
        }
        if let Some(loose) = children.get("arcanum").filter(|child| child.kind.is_dir()) {
            let loose_path = path.join(&loose.name);
            let loose = root
                .open_dir(&loose.name)
                .map_err(|source| io_error(&loose_path, source))?;
            self.add_directory(loose, loose_path)?;
        }
        Ok(())
    }
}

fn children(dir: &Dir, path: &Path) -> Result<BTreeMap<String, Child>, Error> {
    let mut entries = dir
        .entries()
        .map_err(|source| io_error(path, source))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|source| io_error(path, source))?;
    entries.sort_unstable_by_key(|entry| entry.file_name());
    let mut children = BTreeMap::new();
    for entry in entries {
        let name = entry.file_name();
        let Some(key) = name.to_str().map(str::to_ascii_lowercase) else {
            continue;
        };
        let kind = entry
            .file_type()
            .map_err(|source| io_error(&path.join(&name), source))?;
        if !kind.is_symlink() {
            children.entry(key).or_insert(Child { name, kind });
        }
    }
    Ok(children)
}
