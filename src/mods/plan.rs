use super::{Error, ModId, Mount, Package, io_error, open_directory, open_regular};
use crate::vfs::{MountKind, PackageSource, Policy, Vfs};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

#[derive(Debug)]
pub struct LoadOrder<'a> {
    packages: Vec<&'a Package>,
}

impl<'a> LoadOrder<'a> {
    /// Packages from lowest to highest resource precedence.
    pub fn packages(&self) -> &[&'a Package] {
        &self.packages
    }

    /// Verify package mount paths, directory indexes, and DAT/PATCH metadata.
    pub fn validate_mounts(&self) -> Result<(), Error> {
        self.add_packages(&mut Vfs::new(Policy::Ordered))
    }

    /// Layer the planned mods over an ordered VFS supplied by the caller.
    pub fn apply(&self, mut base: Vfs) -> Result<Vfs, Error> {
        if base.policy() != Policy::Ordered {
            return Err(Error::RequiresOrderedVfs);
        }
        self.add_packages(&mut base)?;
        Ok(base)
    }

    fn add_packages(&self, vfs: &mut Vfs) -> Result<(), Error> {
        for package in &self.packages {
            for mount in &package.manifest().mounts {
                let path = package.path().join(&mount.path);
                let source = open_mount(package, mount)?;
                vfs.mount_package_source(&package.manifest().id, path, source)
                    .map_err(|source| Error::Mount {
                        id: package.manifest().id.clone(),
                        source,
                    })?;
            }
        }
        Ok(())
    }
}

fn open_mount(package: &Package, mount: &Mount) -> Result<PackageSource, Error> {
    let mut parts = mount.path.iter();
    let leaf = parts
        .next_back()
        .expect("validated mount paths have a leaf");
    let mut dir = package
        .root
        .try_clone()
        .map_err(|source| io_error(package.path(), source))?;
    let mut path = package.path().to_owned();
    for part in parts {
        path.push(part);
        dir = open_directory(&dir, Path::new(part)).map_err(|source| io_error(&path, source))?;
    }
    path.push(leaf);
    let source = match mount.kind {
        MountKind::Archive => {
            open_regular(&dir, Path::new(leaf)).map(|file| PackageSource::Archive(file.into_std()))
        }
        MountKind::Directory => open_directory(&dir, Path::new(leaf)).map(PackageSource::Directory),
    };
    source.map_err(|source| io_error(&path, source))
}

#[derive(Clone, Copy)]
struct Constraint<'a> {
    author: &'a ModId,
    field: &'static str,
}

type Incoming<'a> = BTreeMap<&'a ModId, BTreeMap<&'a ModId, Constraint<'a>>>;

pub(super) fn resolve<'a>(
    packages: &'a BTreeMap<ModId, Package>,
    selected: &[&str],
) -> Result<LoadOrder<'a>, Error> {
    let active = with_dependencies(packages, selected)?;
    let mut incoming: Incoming<'a> = active.iter().map(|&id| (id, BTreeMap::new())).collect();

    for &id in &active {
        let manifest = packages[id].manifest();
        for dependency in manifest.dependencies.keys() {
            add_edge(
                &mut incoming,
                dependency,
                id,
                Constraint {
                    author: id,
                    field: "dependencies",
                },
            );
        }
        for (field, targets) in [
            ("load_after", &manifest.load_after),
            ("load_before", &manifest.load_before),
        ] {
            for target in targets {
                if !packages.contains_key(target) {
                    return Err(dependency_error(
                        id,
                        format!("{field} references mod {target:?}, which is not installed"),
                    ));
                }
                if active.contains(target) {
                    let (before, after) = if field == "load_after" {
                        (target, id)
                    } else {
                        (id, target)
                    };
                    add_edge(
                        &mut incoming,
                        before,
                        after,
                        Constraint { author: id, field },
                    );
                }
            }
        }
    }

    let mut outgoing: BTreeMap<&ModId, Vec<&ModId>> = BTreeMap::new();
    let mut ready = BTreeSet::new();
    for (&id, prerequisites) in &incoming {
        if prerequisites.is_empty() {
            ready.insert(id);
        }
        for &before in prerequisites.keys() {
            outgoing.entry(before).or_default().push(id);
        }
    }
    let mut order = Vec::with_capacity(active.len());
    while let Some(id) = ready.pop_first() {
        order.push(&packages[id]);
        incoming.remove(id);
        if let Some(dependents) = outgoing.remove(id) {
            for dependent in dependents {
                let prereqs = incoming
                    .get_mut(dependent)
                    .expect("a dependent still has incoming edges");
                prereqs.remove(id);
                if prereqs.is_empty() {
                    ready.insert(dependent);
                }
            }
        }
    }
    if !incoming.is_empty() {
        return Err(Error::Cycle(describe_cycle(&incoming)));
    }
    Ok(LoadOrder { packages: order })
}

fn with_dependencies<'a>(
    packages: &'a BTreeMap<ModId, Package>,
    selected: &[&str],
) -> Result<BTreeSet<&'a ModId>, Error> {
    let mut pending = BTreeSet::new();
    if selected.is_empty() {
        pending.extend(packages.keys());
    } else {
        for &id in selected {
            let Some((key, _)) = packages.get_key_value(id) else {
                return Err(Error::UnknownMod(id.into()));
            };
            if !pending.insert(key) {
                return Err(Error::DuplicateSelection(id.into()));
            }
        }
    }
    let mut active = BTreeSet::new();
    while let Some(id) = pending.pop_first() {
        active.insert(id);
        for (dependency, requirement) in &packages[id].manifest().dependencies {
            let Some((key, package)) = packages.get_key_value(dependency) else {
                return Err(dependency_error(
                    id,
                    format!("requires {dependency:?} ({requirement}), which is not installed"),
                ));
            };
            let version = &package.manifest().version;
            if !requirement.matches(version) {
                return Err(dependency_error(
                    id,
                    format!(
                        "requires {dependency:?} ({requirement}), but installed version is {version}"
                    ),
                ));
            }
            if !active.contains(key) {
                pending.insert(key);
            }
        }
    }
    Ok(active)
}

fn add_edge<'a>(
    incoming: &mut Incoming<'a>,
    before: &'a ModId,
    after: &'a ModId,
    constraint: Constraint<'a>,
) {
    incoming
        .get_mut(after)
        .expect("every active mod has a graph node")
        .entry(before)
        .or_insert(constraint);
}

fn describe_cycle(incoming: &Incoming<'_>) -> String {
    let mut path = Vec::new();
    let mut seen = BTreeMap::new();
    let mut id = *incoming.first_key_value().unwrap().0;
    // Every remaining node has at least one remaining prerequisite.
    while !seen.contains_key(id) {
        seen.insert(id, path.len());
        path.push(id);
        id = *incoming[id].first_key_value().unwrap().0;
    }
    let cycle = &mut path[seen[id]..];
    cycle.reverse();
    let mut report = cycle[0].to_string();
    for (&before, &after) in cycle.iter().zip(cycle.iter().cycle().skip(1)) {
        let rule = incoming[after][before];
        use std::fmt::Write;
        write!(report, " -> {after} ({} in {})", rule.field, rule.author).unwrap();
    }
    report
}

fn dependency_error(id: &ModId, reason: String) -> Error {
    Error::Dependency {
        id: id.clone(),
        reason,
    }
}
