use arcanum_rs::content;
use arcanum_rs::content::{items, schematics};
use arcanum_rs::dat::{Archive, Format};
use arcanum_rs::mes::EntryId;
use arcanum_rs::mods::{Catalog, Package};
use arcanum_rs::vfs::{self, Policy, Resolution, Vfs};
use std::error::Error;
use std::ffi::{OsStr, OsString};
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
  arcanum-rs mods resolve <game-directory> <mods-directory> <resource-path> <mod-id> ...
  arcanum-rs [--ordered] mes inspect <game-directory> <resource-path> [--mods <mods-directory> <mod-id> ...]
  arcanum-rs [--ordered] mes get <game-directory> <resource-path> <entry-id> [--mods <mods-directory> <mod-id> ...]
  arcanum-rs [--ordered] prototype inspect <game-directory> <resource-path> [--mods <mods-directory> <mod-id> ...]
  arcanum-rs [--ordered] schematic inspect <game-directory> <recipe-id> [--mods <mods-directory> <mod-id> ...]";

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
        (Some("mes"), [_, action, game, resource, options @ ..]) if action == "inspect" => {
            inspect_mes(Path::new(game), resource, None, options, policy)
        }
        (Some("mes"), [_, action, game, resource, entry, options @ ..]) if action == "get" => {
            inspect_mes(Path::new(game), resource, Some(entry), options, policy)
        }
        (Some("prototype"), [_, action, game, resource, options @ ..]) if action == "inspect" => {
            inspect_prototype(Path::new(game), resource, options, policy)
        }
        (Some("schematic"), [_, action, game, id, options @ ..]) if action == "inspect" => {
            inspect_schematic(Path::new(game), id, options, policy)
        }
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

fn selected_mods(ids: &[OsString]) -> Result<Vec<&str>, Box<dyn Error>> {
    ids.iter()
        .map(|id| id.to_str().ok_or_else(|| "mod ID must be UTF-8".into()))
        .collect()
}

fn mod_order(
    directory: &Path,
    selected: &[OsString],
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
    selected: &[OsString],
) -> Result<(), Box<dyn Error>> {
    let resource = resource_name(resource)?;
    print_resolution(&modded_vfs(game, directory, selected)?, resource)
}

fn modded_vfs(game: &Path, directory: &Path, selected: &[OsString]) -> Result<Vfs, Box<dyn Error>> {
    let catalog = Catalog::discover(directory)?;
    let plan = catalog.load_order(&selected_mods(selected)?)?;
    let base = Vfs::from_game(game, Policy::Ordered)?;
    Ok(plan.apply(base)?)
}

fn inspection_vfs(
    game: &Path,
    options: &[OsString],
    policy: Policy,
) -> Result<Vfs, Box<dyn Error>> {
    match options {
        [] => Ok(Vfs::from_game(game, policy)?),
        [flag, directory, selected @ ..] if flag == "--mods" && !selected.is_empty() => {
            modded_vfs(game, Path::new(directory), selected)
        }
        _ => Err("expected --mods <mods-directory> <mod-id> ...".into()),
    }
}

fn inspect_mes(
    game: &Path,
    resource: &OsStr,
    entry: Option<&OsStr>,
    options: &[OsString],
    policy: Policy,
) -> Result<(), Box<dyn Error>> {
    let resource = resource_name(resource)?;
    let id = entry
        .map(|entry| {
            entry
                .to_str()
                .and_then(|entry| entry.parse::<EntryId>().ok())
                .ok_or_else(|| {
                    format!(
                        "invalid MES entry ID {entry:?}; expected a signed 32-bit decimal integer"
                    )
                })
        })
        .transpose()?;
    let mut vfs = inspection_vfs(game, options, policy)?;
    let loaded = content::load_mes(&mut vfs, resource)?;
    let mut out = io::stdout().lock();
    writeln!(out, "Path: {}", loaded.source.virtual_path)?;
    writeln!(out, "Source: {}", loaded.source)?;
    if let Some(id) = &loaded.source.mod_id {
        writeln!(out, "Mod: {id}")?;
    }
    writeln!(out, "Encoding: {}", loaded.encoding)?;
    writeln!(out, "Entries: {}", loaded.mes.len())?;
    if let Some(first) = loaded.duplicates.first() {
        writeln!(
            out,
            "Duplicates: {} (last record wins; first replacement is ID {} at {})",
            loaded.duplicates.len(),
            first.id,
            first.replacement
        )?;
    }
    if let Some(id) = id {
        let value = loaded
            .mes
            .get(id)
            .ok_or_else(|| format!("MES entry {id} not found in {}", loaded.source.virtual_path))?;
        writeln!(out, "Entry ID: {id}")?;
        writeln!(out, "Value: {value}")?;
    }
    Ok(())
}

fn inspect_prototype(
    game: &Path,
    resource: &OsStr,
    options: &[OsString],
    policy: Policy,
) -> Result<(), Box<dyn Error>> {
    let resource = resource_name(resource)?;
    let mut vfs = inspection_vfs(game, options, policy)?;
    let prototype = items::load_prototype(&mut vfs, resource)?;
    write_prototype(&mut io::stdout().lock(), &prototype)?;
    Ok(())
}

fn inspect_schematic(
    game: &Path,
    id: &OsStr,
    options: &[OsString],
    policy: Policy,
) -> Result<(), Box<dyn Error>> {
    let id = id
        .to_str()
        .and_then(|id| id.parse().ok())
        .map(schematics::LegacyRecipeId)
        .ok_or_else(|| format!("invalid schematic ID {id:?}; expected a decimal integer"))?;
    let mut vfs = inspection_vfs(game, options, policy)?;
    let report = schematics::load_schematic(&mut vfs, id)?;
    let mut out = io::stdout().lock();
    writeln!(out, "Recipe ID: {}", id.0)?;
    write!(out, "Rules: ")?;
    write_source(&mut out, &report.rules_source)?;
    for (label, key, entry) in [
        ("Title", report.legacy.title.0, &report.title),
        (
            "Description",
            report.legacy.description.0,
            &report.description,
        ),
    ] {
        write!(out, "{label} (MES key {key}): ")?;
        if let Some(entry) = entry {
            writeln!(out, "{}", entry.text)?;
            write_source(&mut out, &entry.source)?;
        } else {
            writeln!(out, "unresolved")?;
        }
    }
    write!(
        out,
        "Illustration (interface ID {}): ",
        report.legacy.illustration.0
    )?;
    if let Some(art) = &report.illustration {
        write_source(&mut out, &art.asset_source)?;
        write!(out, "Illustration table: ")?;
        write_source(&mut out, &art.table_source)?;
    } else {
        writeln!(out, "unresolved")?;
    }
    for (label, id, item) in [
        (
            "Ingredient 1",
            report.legacy.ingredients[0],
            &report.ingredients[0],
        ),
        (
            "Ingredient 2",
            report.legacy.ingredients[1],
            &report.ingredients[1],
        ),
        ("Product", report.legacy.product, &report.product),
    ] {
        write!(out, "{label} (basic ID {}): ", id.0)?;
        if let Some(name) = &item.name {
            writeln!(out, "{}", name.text)?;
            write_source(&mut out, &name.source)?;
        } else {
            writeln!(out, "unresolved")?;
        }
        if let Some(prototype) = &item.prototype {
            write_prototype(&mut out, prototype)?;
        }
    }
    writeln!(out, "Quantity: {}", report.legacy.quantity)?;
    if let Some(definition) = &report.definition {
        writeln!(out, "Native recipe: {}", definition.id.0)?;
    } else {
        writeln!(out, "Native recipe: unresolved")?;
    }
    for diagnostic in &report.diagnostics {
        writeln!(
            out,
            "Warning ({}): {}",
            diagnostic.field, diagnostic.message
        )?;
    }
    Ok(())
}

fn write_prototype(out: &mut impl Write, prototype: &items::LoadedPrototype) -> io::Result<()> {
    write!(out, "Prototype: ")?;
    write_source(out, &prototype.source)?;
    let header = prototype.header;
    writeln!(
        out,
        "Version: {} | serialized ID: {} | object type: {}",
        header.version, header.id.0, header.object_type
    )?;
    if let Some(food) = prototype.food {
        writeln!(
            out,
            "Food fields: description ID {}, weight {}, worth {}, discipline {}, signed complexity {}",
            food.description.0,
            food.weight,
            food.worth,
            food.discipline,
            food.magic_tech_complexity
        )
    } else {
        writeln!(out, "Supported fields: header only")
    }
}

fn write_source(out: &mut impl Write, source: &content::Source) -> io::Result<()> {
    write!(out, "{} from {source}", source.virtual_path)?;
    if let Some(id) = &source.mod_id {
        write!(out, " [mod {id}]")?;
    }
    writeln!(out)
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
