use arcanum_rs::content;
use arcanum_rs::content::effects::{
    EffectDefinition, EffectId, EffectOperation, LegacyEffectId, load_effect,
};
use arcanum_rs::content::schematics::DiagnosticKind;
use arcanum_rs::content::schematics::audit::{
    GroupStatus, SchematicAudit, audit_all_schematics, audit_schematics,
};
use arcanum_rs::content::schematics::readiness::{Status, audit_readiness};
use arcanum_rs::content::{items, schematics};
use arcanum_rs::dat::{Archive, Format};
use arcanum_rs::mes::EntryId;
use arcanum_rs::mods::{Catalog, Package};
use arcanum_rs::prototype::WeaponAmmoFields;
use arcanum_rs::runtime::{ItemDefinitionRef, Runtime};
use arcanum_rs::vfs::{self, Policy, Resolution, Vfs};
use std::collections::{BTreeMap, BTreeSet};
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
  arcanum-rs [--ordered] effect inspect <game-directory> <effect-id> [--mods <mods-directory> <mod-id> ...]
  arcanum-rs [--ordered] schematic inspect <game-directory> <recipe-id> [--mods <mods-directory> <mod-id> ...]
  arcanum-rs [--ordered] schematic audit <game-directory> [--all-references | [--readiness] [<recipe-id> ...]] [--mods <mods-directory> <mod-id> ...]
  arcanum-rs simulate equipment-demo";

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
        (Some("simulate"), [_, demo]) if policy == Policy::Ce && demo == "equipment-demo" => {
            equipment_demo(&mut io::stdout().lock())
        }
        (Some("mes"), [_, action, game, resource, options @ ..]) if action == "inspect" => {
            inspect_mes(Path::new(game), resource, None, options, policy)
        }
        (Some("mes"), [_, action, game, resource, entry, options @ ..]) if action == "get" => {
            inspect_mes(Path::new(game), resource, Some(entry), options, policy)
        }
        (Some("prototype"), [_, action, game, resource, options @ ..]) if action == "inspect" => {
            inspect_prototype(Path::new(game), resource, options, policy)
        }
        (Some("effect"), [_, action, game, id, options @ ..]) if action == "inspect" => {
            inspect_effect(Path::new(game), id, options, policy)
        }
        (Some("schematic"), [_, action, game, id, options @ ..]) if action == "inspect" => {
            inspect_schematic(Path::new(game), id, options, policy)
        }
        (Some("schematic"), [_, action, game, options @ ..]) if action == "audit" => {
            audit_schematic_command(Path::new(game), options, policy)
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

fn equipment_demo(out: &mut impl Write) -> Result<(), Box<dyn Error>> {
    let item = items::GenericDefinition {
        id: items::ItemId("demo:item/defence-charm".into()),
        display_name: "Defence charm".into(),
        base_weight: 1,
        base_worth: 0,
        magic_tech_complexity: 0,
    };
    let effect = EffectDefinition {
        id: EffectId("arcanum:effect/128".into()),
        operations: vec![EffectOperation::AddArmourClass { points: 1 }],
    };
    let mut runtime = Runtime::default();
    let actor = runtime.create_actor(10)?;
    let first = runtime.instantiate_item(ItemDefinitionRef::Generic(&item))?;
    let second = runtime.instantiate_item(ItemDefinitionRef::Generic(&item))?;
    writeln!(
        out,
        "{actor:?} starts at AC {}",
        runtime.armour_class(actor)?
    )?;

    runtime.equip(first, actor)?;
    let remaining = runtime.activate_effect(first, actor, &effect)?;
    writeln!(
        out,
        "Equipped {first:?}, granted {} × 1, AC {}",
        effect.id.0,
        runtime.armour_class(actor)?
    )?;
    runtime.equip(second, actor)?;
    let copies = 15;
    for _ in 0..copies {
        runtime.activate_effect(second, actor, &effect)?;
    }
    writeln!(
        out,
        "Equipped {second:?}, granted {} × {copies}, AC {}",
        effect.id.0,
        runtime.armour_class(actor)?
    )?;

    runtime.unequip(second, actor)?;
    let grant = runtime.activation(remaining)?;
    writeln!(
        out,
        "Unequipped {second:?}, {:?} from {:?} remains ({:+} AC), AC {}",
        grant.id,
        grant.source,
        grant.armour_class_points,
        runtime.armour_class(actor)?
    )?;
    runtime.unequip(first, actor)?;
    writeln!(
        out,
        "Unequipped {first:?}, AC {}",
        runtime.armour_class(actor)?
    )?;
    Ok(())
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

fn inspect_effect(
    game: &Path,
    id: &OsStr,
    options: &[OsString],
    policy: Policy,
) -> Result<(), Box<dyn Error>> {
    let id = id
        .to_str()
        .and_then(|id| id.parse().ok())
        .map(LegacyEffectId)
        .ok_or_else(|| format!("invalid effect ID {id:?}; expected a decimal integer"))?;
    let mut vfs = inspection_vfs(game, options, policy)?;
    let effect = load_effect(&mut vfs, id)?;
    let mut out = io::stdout().lock();
    write!(out, "Source: ")?;
    write_source(&mut out, &effect.source)?;
    writeln!(out, "Encoding: {}", effect.encoding)?;
    writeln!(out, "Legacy effect ID: {}", effect.legacy_id.0)?;
    writeln!(out, "Native effect ID: {}", effect.definition.id.0)?;
    writeln!(out, "Record: {}", effect.record)?;
    writeln!(out, "Operations in source order for inspection:")?;
    for operation in effect.definition.operations {
        match operation {
            EffectOperation::AddArmourClass { points } => {
                writeln!(out, "  Armour class adjustment (points): {points:+}")?;
            }
            EffectOperation::AddResistance {
                channel,
                percentage_points,
            } => {
                writeln!(
                    out,
                    "  {channel:?} resistance adjustment: {percentage_points:+} percentage points"
                )?;
            }
            EffectOperation::ResistanceFloor {
                channel,
                minimum_percent,
            } => {
                writeln!(out, "  {channel:?} resistance floor: {minimum_percent}%")?;
            }
        }
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
    let inspection = items::inspect_prototype(&mut vfs, resource)?;
    let mut out = io::stdout().lock();
    write_prototype(&mut out, &inspection.prototype)?;
    if let Some(fields) = &inspection.weapon_ammo {
        let resolved = inspection.resolve_weapon_ammo(&mut vfs);
        match resolved {
            Ok(resolved) => write_weapon_ammo(&mut out, fields, resolved)?,
            Err(error) => {
                write_weapon_ammo(&mut out, fields, None)?;
                return Err(error.into());
            }
        }
    }
    for script in inspection.resolve_wield_scripts(&vfs) {
        let label = match script.attachment {
            items::WieldAttachment::On => "Wield-on",
            items::WieldAttachment::Off => "Wield-off",
        };
        write!(out, "{label} script (legacy ID {}): ", script.legacy_id.0)?;
        match script.resolution {
            Ok(resolved) => write_source(&mut out, &resolved.source)?,
            Err(error) => writeln!(out, "unresolved ({error})")?,
        }
    }
    if let Some(fields) = &inspection.armour {
        write_armour_fields(&mut out, fields)?;
        if let Some(armour) = inspection.resolve_armour(&mut vfs)? {
            write_armour(&mut out, armour)?;
        }
    }
    if let Some(fields) = &inspection.generic {
        write_common_item_fields(&mut out, &fields.item)?;
        writeln!(out, "Legacy Generic flags: {:#010x}", fields.flags)?;
        if let Some(resolved) = inspection.resolve_generic(&mut vfs)? {
            let item = resolved.definition;
            writeln!(out, "Generic: {} ({})", item.display_name, item.id.0)?;
            write!(out, "Name: ")?;
            write_source(&mut out, &resolved.name_source)?;
        }
    }
    if let Some(fields) = &inspection.key {
        write_common_item_fields(&mut out, &fields.item)?;
        writeln!(out, "Legacy key code: {}", fields.key_id)?;
        if let Some(resolved) = inspection.resolve_key(&mut vfs)? {
            let item = resolved.definition;
            writeln!(out, "Key: {} ({})", item.display_name, item.id.0)?;
            let code = item.code.as_ref().map_or("none", |code| code.0.as_str());
            writeln!(out, "Native key code: {code}")?;
            write!(out, "Name: ")?;
            write_source(&mut out, &resolved.name_source)?;
        }
    }
    Ok(())
}

fn recipe_id(id: &OsStr) -> Result<schematics::LegacyRecipeId, Box<dyn Error>> {
    id.to_str()
        .and_then(|id| id.parse().ok())
        .map(schematics::LegacyRecipeId)
        .ok_or_else(|| format!("invalid schematic ID {id:?}; expected a decimal integer").into())
}

fn audit_schematic_command(
    game: &Path,
    options: &[OsString],
    policy: Policy,
) -> Result<(), Box<dyn Error>> {
    let split = options
        .iter()
        .position(|arg| arg == "--mods")
        .unwrap_or(options.len());
    let (selection, options) = options.split_at(split);
    let (readiness, selection) = match selection {
        [flag, rest @ ..] if flag == "--readiness" => (true, rest),
        _ => (false, selection),
    };
    let all = match selection {
        [flag] if flag == "--all-references" && !readiness => true,
        args if args.iter().any(|arg| arg == "--all-references") => {
            return Err(
                "--all-references cannot be combined with --readiness or recipe IDs".into(),
            );
        }
        _ => false,
    };
    let ids = if all {
        Vec::new()
    } else {
        selection
            .iter()
            .map(|id| recipe_id(id))
            .collect::<Result<Vec<_>, _>>()?
    };
    let mut vfs = inspection_vfs(game, options, policy)?;
    let (audit, readiness_statuses) = if readiness {
        let result = audit_readiness(&mut vfs, &ids)?;
        (result.references, Some(result.statuses))
    } else if all {
        (audit_all_schematics(&mut vfs)?, None)
    } else {
        (audit_schematics(&mut vfs, &ids)?, None)
    };
    let mut supported = 0;
    let mut variants = 0;
    let mut unsupported = 0;
    let mut invalid = 0;
    let mut issues = Vec::new();
    for group in &audit.groups {
        match &group.status {
            GroupStatus::Supported(group) => {
                supported += 1;
                variants += usize::from(group.variants.len() > 1);
            }
            GroupStatus::Unsupported(error) => {
                unsupported += 1;
                issues.push((group.id, error));
            }
            GroupStatus::MalformedOrIncomplete(error) => {
                invalid += 1;
                issues.push((group.id, error));
            }
        }
    }

    let mut out = io::stdout().lock();
    write!(out, "Rules: ")?;
    write_source(&mut out, &audit.rules_source)?;
    writeln!(
        out,
        "Discovered learned/found groups: {}",
        audit.groups.len()
    )?;
    writeln!(
        out,
        "Structurally supported: {supported} ({variants} with variants)"
    )?;
    writeln!(out, "Unsupported mappings: {unsupported}")?;
    writeln!(out, "Malformed or incomplete: {invalid}")?;
    writeln!(
        out,
        "Duplicate recipe keys (last value used): {}",
        audit.duplicate_keys.len()
    )?;
    const MAX_ISSUES: usize = 12;
    for (id, error) in issues.iter().take(MAX_ISSUES) {
        writeln!(out, "  Recipe {}: {error}", id.0)?;
    }
    if issues.len() > MAX_ISSUES {
        writeln!(
            out,
            "  {} further structural issues",
            issues.len() - MAX_ISSUES
        )?;
    }
    for key in audit.duplicate_keys.iter().take(MAX_ISSUES) {
        writeln!(out, "  Duplicate entry {key}")?;
    }

    if all || (readiness && ids.is_empty()) {
        write_full_reference_summary(&mut out, &audit)?;
    } else {
        write_selected_reference_summary(&mut out, &audit)?;
    }
    if let Some(statuses) = readiness_statuses {
        write_readiness_summary(&mut out, &statuses, ids.is_empty())?;
    }
    Ok(())
}

fn write_selected_reference_summary(
    out: &mut impl Write,
    audit: &SchematicAudit,
) -> io::Result<()> {
    let complete = audit
        .references
        .iter()
        .filter(|(_, report)| {
            report
                .as_ref()
                .is_ok_and(|report| report.definition.is_some())
        })
        .count();
    writeln!(
        out,
        "Reference validation: {complete} complete / {} selected",
        audit.references.len()
    )?;
    for (id, result) in &audit.references {
        match result {
            Ok(report) => {
                let title = report
                    .title
                    .as_ref()
                    .map_or("unresolved title", |title| &title.text);
                let status = if report.definition.is_some() {
                    "complete"
                } else {
                    "incomplete"
                };
                writeln!(out, "  {} {title}: {status}", id.0)?;
                for warning in &report.diagnostics {
                    writeln!(out, "    {}: {}", warning.field, warning.message)?;
                }
            }
            Err(error) => writeln!(out, "  {}: {error}", id.0)?,
        }
    }
    Ok(())
}

fn write_readiness_summary(
    out: &mut impl Write,
    statuses: &[(schematics::LegacyRecipeId, Status)],
    all: bool,
) -> io::Result<()> {
    let mut passed = 0;
    let mut blocked = 0;
    let mut counts = BTreeMap::new();
    for (_, status) in statuses {
        match status {
            Status::ReferencesIncomplete => blocked += 1,
            Status::Checked(issues) if issues.is_empty() => passed += 1,
            Status::Checked(issues) => {
                for issue in issues {
                    *counts.entry(issue.kind).or_insert(0) += 1;
                }
            }
        }
    }
    writeln!(
        out,
        "Content readiness checks: {passed} passed / {} audited ({} with issues, {blocked} blocked by references)",
        statuses.len(),
        statuses.len() - passed - blocked
    )?;
    for (kind, count) in counts {
        writeln!(out, "  {kind}: {count} recipe/resource occurrences")?;
    }
    let mut shown = BTreeSet::new();
    let mut examples = 0;
    for (id, status) in statuses {
        if let Status::Checked(issues) = status {
            for issue in issues {
                if all && !shown.insert(issue.source.virtual_path.as_str()) {
                    continue;
                }
                examples += 1;
                if !all || examples <= 12 {
                    write!(out, "  Recipe {} ({}): ", id.0, issue.kind)?;
                    write_source(out, &issue.source)?;
                    writeln!(out, "    {}", issue.detail)?;
                }
            }
        }
    }
    if all && examples > 12 {
        writeln!(
            out,
            "  {} further resource issues. Pass recipe IDs after --readiness for details.",
            examples - 12
        )?;
    }
    Ok(())
}

fn write_full_reference_summary(out: &mut impl Write, audit: &SchematicAudit) -> io::Result<()> {
    let mut counts = BTreeMap::new();
    let mut incomplete = Vec::new();
    for (id, result) in &audit.references {
        match result {
            Ok(report) => {
                if report.definition.is_none() {
                    incomplete.push(id.0.to_string());
                }
                for diagnostic in &report.diagnostics {
                    *counts.entry(diagnostic.kind).or_insert(0) += 1;
                }
            }
            Err(_) => {
                incomplete.push(id.0.to_string());
                *counts.entry(DiagnosticKind::MalformedMapping).or_insert(0) += 1;
            }
        }
    }
    writeln!(
        out,
        "Reference validation: {} complete / {} audited",
        audit.references.len() - incomplete.len(),
        audit.references.len()
    )?;
    writeln!(out, "Incomplete: {}", incomplete.len())?;
    if counts.is_empty() {
        writeln!(out, "Diagnostics: none")?;
    } else {
        writeln!(out, "Diagnostics by category:")?;
        for (kind, count) in counts {
            writeln!(out, "  {kind}: {count}")?;
        }
    }
    if !incomplete.is_empty() {
        writeln!(out, "Incomplete recipe IDs: {}", incomplete.join(", "))?;
        writeln!(
            out,
            "Pass recipe IDs in place of --all-references for details."
        )?;
    }
    Ok(())
}

fn inspect_schematic(
    game: &Path,
    id: &OsStr,
    options: &[OsString],
    policy: Policy,
) -> Result<(), Box<dyn Error>> {
    let id = recipe_id(id)?;
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
    if let [variant] = report.legacy.variants.as_slice() {
        for (label, id) in [
            ("Ingredient 1", variant.ingredients[0]),
            ("Ingredient 2", variant.ingredients[1]),
            ("Product", variant.product),
        ] {
            write_recipe_item(&mut out, label, id, &report.items[&id])?;
        }
    } else {
        writeln!(out, "Variants in source preference order:")?;
        let name = |id| recipe_item_name(&report.items[&id]);
        for (index, variant) in report.legacy.variants.iter().enumerate() {
            let [first, second] = variant.ingredients;
            let product = variant.product;
            writeln!(
                out,
                "  {}. {} ({}) + {} ({}) -> {} ({})",
                index + 1,
                name(first),
                first.0,
                name(second),
                second.0,
                name(product),
                product.0
            )?;
        }
        for (&id, item) in &report.items {
            write_recipe_item(&mut out, "Item", id, item)?;
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

fn recipe_item_name(item: &schematics::ItemResolution) -> &str {
    item.name.as_ref().map_or("unresolved", |name| &name.text)
}

fn write_recipe_item(
    out: &mut impl Write,
    label: &str,
    id: schematics::BasicPrototypeId,
    item: &schematics::ItemResolution,
) -> io::Result<()> {
    writeln!(
        out,
        "{label} (basic ID {}): {}",
        id.0,
        recipe_item_name(item)
    )?;
    if let Some(name) = &item.name {
        write_source(out, &name.source)?;
    }
    if let Some(prototype) = &item.prototype {
        write_prototype(out, prototype)?;
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
    } else if let Some(description) = prototype.description {
        writeln!(
            out,
            "Supported body field: description ID {}",
            description.0
        )
    } else {
        writeln!(out, "Supported fields: header only")
    }
}

fn write_weapon_ammo(
    out: &mut impl Write,
    fields: &WeaponAmmoFields,
    resolved: Option<items::ResolvedWeaponAmmo>,
) -> io::Result<()> {
    write_common_item_fields(out, fields.item())?;
    match fields {
        WeaponAmmoFields::Weapon(weapon) => {
            writeln!(
                out,
                "Speed rating: {} | range (tiles): {}",
                weapon.speed_factor, weapon.range
            )?;
            writeln!(
                out,
                "Legacy ammo type: {} | base consumption: {}",
                weapon.ammo_type, weapon.ammo_consumption
            )?;
        }
        WeaponAmmoFields::Ammo(ammo) => {
            writeln!(
                out,
                "Legacy ammo type: {} | stored prototype quantity: {}",
                ammo.ammo_type, ammo.quantity
            )?;
        }
    }

    if let Some(resolved) = resolved {
        match resolved.definition {
            items::WeaponAmmoDefinition::Weapon(weapon) => {
                writeln!(out, "Weapon: {} ({})", weapon.display_name, weapon.id.0)?;
                for damage in weapon.damage {
                    writeln!(
                        out,
                        "  {:?} damage: {}–{} points",
                        damage.kind, damage.minimum, damage.maximum
                    )?;
                }
                if let Some(ammo) = weapon.ammunition {
                    writeln!(
                        out,
                        "Ammunition: {:?} | units per attack: {}",
                        ammo.kind, ammo.units_per_attack
                    )?;
                } else {
                    writeln!(out, "Ammunition: none")?;
                }
            }
            items::WeaponAmmoDefinition::Ammo(ammo) => {
                writeln!(
                    out,
                    "Ammo: {} ({}) | kind: {:?}",
                    ammo.display_name, ammo.id.0, ammo.kind
                )?;
            }
        }
        write!(out, "Name: ")?;
        write_source(out, &resolved.name_source)?;
    }
    Ok(())
}

fn write_common_item_fields(
    out: &mut impl Write,
    common: &arcanum_rs::prototype::CommonItemFields,
) -> io::Result<()> {
    writeln!(
        out,
        "Base weight: {} stone | base worth: {} | legacy discipline: {} | signed complexity: {}",
        common.weight, common.worth, common.discipline, common.magic_tech_complexity
    )
}

fn write_armour_fields(
    out: &mut impl Write,
    fields: &arcanum_rs::prototype::ArmourFields,
) -> io::Result<()> {
    write_common_item_fields(out, &fields.item)?;
    writeln!(
        out,
        "Base armour class adjustment (points): {:+}",
        fields.armour_class_adjustment
    )?;
    writeln!(
        out,
        "Stored magical armour class adjustment (points): {:+}",
        fields.magical_armour_class_adjustment
    )?;
    writeln!(
        out,
        "Legacy armour flags: 0x{:08x} | inventory art ID: 0x{:08x}",
        fields.flags, fields.inventory_art_id
    )
}

fn write_armour(out: &mut impl Write, resolved: items::ResolvedArmour) -> io::Result<()> {
    let armour = resolved.definition;
    writeln!(out, "Armour: {} ({})", armour.display_name, armour.id.0)?;
    writeln!(out, "Coverage: {:?}", armour.coverage)?;
    if armour.sizes.is_empty() {
        writeln!(out, "Stored fit sizes: none recorded")?;
    } else {
        let sizes: Vec<_> = armour
            .sizes
            .iter()
            .map(|size| format!("{size:?}"))
            .collect();
        writeln!(out, "Stored fit sizes: {}", sizes.join(", "))?;
    }
    for resistance in armour.resistances {
        writeln!(
            out,
            "  {:?} resistance: {:+} percentage points",
            resistance.kind, resistance.percentage_points
        )?;
    }
    for resistance in armour.magical_resistances {
        writeln!(
            out,
            "  Stored magical {:?} resistance: {:+} percentage points",
            resistance.kind, resistance.percentage_points
        )?;
    }
    write!(out, "Name: ")?;
    write_source(out, &resolved.name_source)
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
