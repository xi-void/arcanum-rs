use arcanum_rs::dat::{Archive, Format};
use arcanum_rs::mods::{Catalog, Package};
use arcanum_rs::vfs::{self, Policy, Resolution, Vfs};
use std::error::Error;
use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::Path;
use std::process::ExitCode;

const USAGE: &str = "Usage:
  arcanum-rs list <archive>
  arcanum-rs info <archive> <resource-path>
  arcanum-rs extract <archive> <resource-path> <output-file>
  arcanum-rs [--ordered] mounts <game-directory>
  arcanum-rs [--ordered] resolve <game-directory> <resource-path>
  arcanum-rs [--ordered] conflicts <game-directory>
  arcanum-rs mods list <mods-directory>
  arcanum-rs mods info <package-directory>
  arcanum-rs mods validate <mods-directory> [<mod-id> ...]
  arcanum-rs mods order <mods-directory> [<mod-id> ...]
  arcanum-rs mods resolve <game-directory> <mods-directory> <resource-path> <mod-id> ...";

fn main() -> ExitCode {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let (policy, args) = match args.as_slice() {
        [flag, rest @ ..] if flag == "--ordered" => (Policy::Ordered, rest),
        args => (Policy::Ce, args),
    };
    let command = args.first().and_then(|arg| arg.to_str());
    let result = match (command, args) {
        (Some("-h" | "--help"), [_]) => writeln!(io::stdout(), "{USAGE}").map_err(Into::into),
        (Some("list"), [_, archive]) if policy == Policy::Ce => list(Path::new(archive)),
        (Some("info"), [_, archive, resource]) if policy == Policy::Ce => {
            info(Path::new(archive), resource)
        }
        (Some("extract"), [_, archive, resource, output]) if policy == Policy::Ce => {
            extract(Path::new(archive), resource, Path::new(output))
        }
        (Some("mounts"), [_, game]) => mounts(Path::new(game), policy),
        (Some("resolve"), [_, game, resource]) => resolve(Path::new(game), policy, resource),
        (Some("conflicts"), [_, game]) => conflicts(Path::new(game), policy),
        (Some("mods"), [_, subcommand, directory])
            if policy == Policy::Ce && subcommand == "list" =>
        {
            list_mods(Path::new(directory))
        }
        (Some("mods"), [_, subcommand, directory])
            if policy == Policy::Ce && subcommand == "info" =>
        {
            mod_info(Path::new(directory))
        }
        (Some("mods"), [_, subcommand, directory, selected @ ..])
            if policy == Policy::Ce && (subcommand == "validate" || subcommand == "order") =>
        {
            mod_order(Path::new(directory), selected, subcommand == "order")
        }
        (Some("mods"), [_, subcommand, game, directory, resource, selected @ ..])
            if policy == Policy::Ce && subcommand == "resolve" && !selected.is_empty() =>
        {
            mod_resolve(Path::new(game), Path::new(directory), resource, selected)
        }
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("arcanum-rs: {err}");
            ExitCode::FAILURE
        }
    }
}

fn open_archive(path: &Path) -> Result<Archive<File>, Box<dyn Error>> {
    Ok(Archive::open(path).map_err(|err| format!("{}: {err}", path.display()))?)
}

fn resource_name(path: &OsStr) -> Result<&str, Box<dyn Error>> {
    path.to_str()
        .ok_or_else(|| "resource path must be UTF-8".into())
}

fn list(path: &Path) -> Result<(), Box<dyn Error>> {
    let archive = open_archive(path)?;
    let mut stdout = io::BufWriter::new(io::stdout().lock());
    for entry in archive.entries() {
        writeln!(stdout, "{}\t{}\t{}", entry.kind, entry.size, entry.path)?;
    }
    stdout.flush()?;
    Ok(())
}

fn mounts(game: &Path, policy: Policy) -> Result<(), Box<dyn Error>> {
    let vfs = Vfs::from_game(game, policy)?;
    let mut stdout = io::stdout().lock();
    writeln!(stdout, "Policy: {} (highest priority first)", vfs.policy())?;
    for mount in vfs.mounts() {
        writeln!(
            stdout,
            "{}\t{}\t{}",
            mount.id,
            mount.kind,
            mount.path.display()
        )?;
    }
    Ok(())
}

fn resolve(game: &Path, policy: Policy, resource: &OsStr) -> Result<(), Box<dyn Error>> {
    let resource = resource_name(resource)?;
    let vfs = Vfs::from_game(game, policy)?;
    print_resolution(&vfs, resource)
}

fn print_resolution(vfs: &Vfs, resource: &str) -> Result<(), Box<dyn Error>> {
    let resolution = vfs.resolve(resource)?;
    write_resolution(&mut io::stdout().lock(), &resolution)?;
    if resolution.winner().is_none() {
        return Err(vfs::Error::NotFound(resolution.path).into());
    }
    Ok(())
}

fn list_mods(directory: &Path) -> Result<(), Box<dyn Error>> {
    let catalog = Catalog::discover(directory)?;
    let mut out = io::stdout().lock();
    for package in catalog.packages() {
        write_package(&mut out, package)?;
    }
    Ok(())
}

fn mod_info(directory: &Path) -> Result<(), Box<dyn Error>> {
    let package = Package::open(directory)?;
    let manifest = package.manifest();
    let mut out = io::stdout().lock();
    writeln!(out, "Package: {}", package.path().display())?;
    writeln!(out, "Manifest version: {}", manifest.manifest_version)?;
    writeln!(out, "ID: {}", manifest.id)?;
    writeln!(out, "Name: {}", manifest.name)?;
    writeln!(out, "Version: {}", manifest.version)?;
    writeln!(out, "Description: {}", manifest.description)?;
    for (id, requirement) in &manifest.dependencies {
        writeln!(out, "Requires: {id} {requirement}")?;
    }
    for id in &manifest.load_after {
        writeln!(out, "Load after: {id}")?;
    }
    for id in &manifest.load_before {
        writeln!(out, "Load before: {id}")?;
    }
    for mount in &manifest.mounts {
        writeln!(out, "Mount: {} {}", mount.kind, mount.path.display())?;
    }
    Ok(())
}

fn selected_mods(ids: &[std::ffi::OsString]) -> Result<Vec<&str>, Box<dyn Error>> {
    ids.iter()
        .map(|id| id.to_str().ok_or_else(|| "mod ID must be UTF-8".into()))
        .collect()
}

fn mod_order(
    directory: &Path,
    selected: &[std::ffi::OsString],
    print_order: bool,
) -> Result<(), Box<dyn Error>> {
    let catalog = Catalog::discover(directory)?;
    let plan = catalog.load_order(&selected_mods(selected)?)?;
    plan.validate_mounts()?;
    let mut out = io::stdout().lock();
    if print_order {
        for (index, package) in plan.packages().iter().enumerate() {
            write!(out, "{}\t", index + 1)?;
            write_package(&mut out, package)?;
        }
    } else {
        writeln!(out, "Validated {} mod packages", plan.packages().len())?;
    }
    Ok(())
}

fn mod_resolve(
    game: &Path,
    directory: &Path,
    resource: &OsStr,
    selected: &[std::ffi::OsString],
) -> Result<(), Box<dyn Error>> {
    let resource = resource_name(resource)?;
    let catalog = Catalog::discover(directory)?;
    let plan = catalog.load_order(&selected_mods(selected)?)?;
    let base = Vfs::from_game(game, Policy::Ordered)?;
    print_resolution(&plan.apply(base)?, resource)
}

fn write_package(mut out: impl Write, package: &Package) -> io::Result<()> {
    let manifest = package.manifest();
    writeln!(
        out,
        "{}\t{}\t{}\t{}",
        manifest.id,
        manifest.version,
        manifest.name,
        package.path().display()
    )
}

fn conflicts(game: &Path, policy: Policy) -> Result<(), Box<dyn Error>> {
    let vfs = Vfs::from_game(game, policy)?;
    let mut stdout = io::BufWriter::new(io::stdout().lock());
    for resolution in vfs.conflicts() {
        write_resolution(&mut stdout, &resolution)?;
    }
    stdout.flush()?;
    Ok(())
}

fn write_resolution(mut out: impl Write, resolution: &Resolution<'_>) -> io::Result<()> {
    writeln!(out, "{}", resolution.path)?;
    let winner = resolution.winner();
    for &candidate in resolution.candidates() {
        if let Some(reason) = candidate.exclusion {
            write!(out, "  excluded ({reason})")?;
        } else if winner == Some(candidate) {
            write!(out, "  winner")?;
        } else {
            write!(out, "  shadowed")?;
        }
        write!(
            out,
            "\tmount {}\t{} {}",
            candidate.mount.id,
            candidate.mount.kind,
            candidate.mount.path.display()
        )?;
        if let Some(relative) = candidate.loose_path {
            write!(out, " / {}", relative.display())?;
        }
        write!(out, "\t{}\t{} bytes", candidate.storage, candidate.size)?;
        if let Some(id) = &candidate.mount.mod_id {
            write!(out, "\tmod {id}")?;
        }
        writeln!(out)?;
    }
    Ok(())
}

fn info(path: &Path, resource: &OsStr) -> Result<(), Box<dyn Error>> {
    let archive = open_archive(path)?;
    let entry = archive.entry(resource_name(resource)?)?;
    let mut stdout = io::stdout().lock();
    match archive.metadata().format {
        Format::Dat => writeln!(stdout, "Format: DAT")?,
        Format::Dat1 { guid } => {
            writeln!(stdout, "Format: DAT1")?;
            write!(stdout, "GUID bytes: ")?;
            for byte in guid {
                write!(stdout, "{byte:02x}")?;
            }
            writeln!(stdout)?;
        }
    }
    writeln!(stdout, "Path: {}", entry.path)?;
    writeln!(stdout, "Kind: {}", entry.kind)?;
    writeln!(stdout, "Flags: {:#x}", entry.flags)?;
    writeln!(stdout, "Size: {}", entry.size)?;
    writeln!(stdout, "Stored size field: {}", entry.stored_size)?;
    if let Some(offset) = entry.data_offset {
        writeln!(stdout, "Data offset: {offset}")?;
    }
    Ok(())
}

fn extract(path: &Path, resource: &OsStr, destination: &Path) -> Result<(), Box<dyn Error>> {
    let resource = resource_name(resource)?;
    let mut archive = open_archive(path)?;
    let entry = archive.entry(resource)?;
    if entry.data_offset.is_none() {
        return Err(arcanum_rs::dat::Error::UnreadableEntry {
            path: entry.path.clone(),
            kind: entry.kind,
            flags: entry.flags,
        }
        .into());
    }

    // The destination comes exclusively from the command line. Exclusive creation
    // also rejects existing symlinks, even if their targets do not exist.
    let mut output = File::create_new(destination)
        .map_err(|err| format!("cannot create {}: {err}", destination.display()))?;
    let result = archive
        .extract(resource, &mut output)
        .and_then(|_| Ok(output.sync_all()?));
    drop(output);
    if let Err(err) = result {
        if let Err(cleanup) = fs::remove_file(destination) {
            return Err(format!(
                "{err}; cannot remove incomplete {}: {cleanup}",
                destination.display()
            )
            .into());
        }
        return Err(format!("{}: {err}", destination.display()).into());
    }
    Ok(())
}
