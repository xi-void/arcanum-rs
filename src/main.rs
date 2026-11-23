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

#[cfg(feature = "windowed")]
mod viewer;

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
  arcanum-rs [--ordered] art inspect <game-directory> <resource-path> <frame> [--mods <mods-directory> <mod-id> ...]
  arcanum-rs [--ordered] art export <game-directory> <resource-path> <frame> <output.pam> [--mods <mods-directory> <mod-id> ...]
  arcanum-rs art demo                          (requires --features windowed)
  arcanum-rs [--ordered] art sector-audit <game-directory> [--mods <mods-directory> <mod-id> ...]
  arcanum-rs [--ordered] art sector-wide <game-directory> [--mods <mods-directory> <mod-id> ...]  (requires --features windowed)
  arcanum-rs [--ordered] art sector-facade <game-directory> [--mods <mods-directory> <mod-id> ...]  (requires --features windowed)
  arcanum-rs [--ordered] art sector-wall-inspect <game-directory> [--mods <mods-directory> <mod-id> ...]
  arcanum-rs [--ordered] art world-inspect <game-directory> [--view <local-x> <local-y> <side> <center-x> <center-y> <width> <height>] [--mods <mods-directory> <mod-id> ...]
  arcanum-rs [--ordered] art world <game-directory> [--view <local-x> <local-y> <side> <center-x> <center-y> <width> <height>] [--mods <mods-directory> <mod-id> ...]  (requires --features windowed)
  arcanum-rs [--ordered] art world <game-directory> --sectors <map> <min-x> <min-y> <max-x> <max-y> <world-x> <world-y> <width> <height> [--trace] [--lighting <hour>] [--animation-ms <elapsed>] [--fidget] [--light-probe X Y | --nocturnal-probe X Y] [--renderer cpu|vulkan|gles] [--mods …]
  arcanum-rs [--ordered] art sector-room-wide-inspect <game-directory> [--mods <mods-directory> <mod-id> ...]
  arcanum-rs [--ordered] art sector-room-wide <game-directory> [--mods <mods-directory> <mod-id> ...]  (requires --features windowed)
  arcanum-rs [--ordered] art sector-room-inspect <game-directory> [--mods <mods-directory> <mod-id> ...]
  arcanum-rs [--ordered] art sector-room <game-directory> [--mods <mods-directory> <mod-id> ...]  (requires --features windowed)
  arcanum-rs [--ordered] art sector-walls-inspect <game-directory> [--mods <mods-directory> <mod-id> ...]
  arcanum-rs [--ordered] art sector-walls <game-directory> [--mods <mods-directory> <mod-id> ...]  (requires --features windowed)
  arcanum-rs [--ordered] art sector-wall <game-directory> [--mods <mods-directory> <mod-id> ...]  (requires --features windowed)
  arcanum-rs [--ordered] art sector <game-directory> [--mods <mods-directory> <mod-id> ...]  (requires --features windowed)
  arcanum-rs art floor-demo                    (requires --features windowed)
  arcanum-rs [--ordered] art floor <game-directory> [--mods <mods-directory> <mod-id> ...]  (requires --features windowed)
  arcanum-rs art overlap-demo                  (requires --features windowed)
  arcanum-rs [--ordered] art overlap <game-directory> [--mods <mods-directory> <mod-id> ...]  (requires --features windowed)
  arcanum-rs [--ordered] art view <game-directory> <resource-path> <frame> [--mods <mods-directory> <mod-id> ...]  (requires --features windowed)
  arcanum-rs [--ordered] prototype inspect <game-directory> <resource-path> [--mods <mods-directory> <mod-id> ...]
  arcanum-rs [--ordered] effect inspect <game-directory> <effect-id> [--mods <mods-directory> <mod-id> ...]
  arcanum-rs [--ordered] schematic inspect <game-directory> <recipe-id> [--mods <mods-directory> <mod-id> ...]
  arcanum-rs [--ordered] schematic audit <game-directory> [--all-references | [--readiness] [<recipe-id> ...]] [--mods <mods-directory> <mod-id> ...]
  arcanum-rs simulate equipment-demo
  arcanum-rs simulate resistance-demo
  arcanum-rs simulate electrical-demo
  arcanum-rs simulate composite-demo
  arcanum-rs simulate same-channel-demo";

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
        (Some("simulate"), [_, demo]) if policy == Policy::Ce && demo == "resistance-demo" => {
            resistance_demo(&mut io::stdout().lock())
        }
        (Some("simulate"), [_, demo]) if policy == Policy::Ce && demo == "electrical-demo" => {
            electrical_demo(&mut io::stdout().lock())
        }
        (Some("simulate"), [_, demo]) if policy == Policy::Ce && demo == "composite-demo" => {
            composite_demo(&mut io::stdout().lock())
        }
        (Some("simulate"), [_, demo]) if policy == Policy::Ce && demo == "same-channel-demo" => {
            same_channel_demo(&mut io::stdout().lock())
        }
        (Some("mes"), [_, action, game, resource, options @ ..]) if action == "inspect" => {
            inspect_mes(Path::new(game), resource, None, options, policy)
        }
        (Some("mes"), [_, action, game, resource, entry, options @ ..]) if action == "get" => {
            inspect_mes(Path::new(game), resource, Some(entry), options, policy)
        }
        #[cfg(feature = "windowed")]
        (Some("art"), [_, action]) if action == "demo" && policy == Policy::Ce => sprite_demo(),
        #[cfg(feature = "windowed")]
        (Some("art"), [_, action]) if action == "overlap-demo" && policy == Policy::Ce => {
            viewer::show_synthetic_overlap().map_err(Into::into)
        }
        (Some("art"), [_, action, game, options @ ..]) if action == "sector-audit" => {
            report_sector_audit(Path::new(game), options, policy)
        }
        (Some("art"), [_, action, game, options @ ..]) if action == "sector-wall-inspect" => {
            inspect_demo_wall(Path::new(game), options, policy)
        }
        (Some("art"), [_, action, game, options @ ..]) if action == "sector-walls-inspect" => {
            inspect_connected_walls(Path::new(game), options, policy)
        }
        (Some("art"), [_, action, game, options @ ..]) if action == "sector-room-inspect" => {
            inspect_room(Path::new(game), options, policy)
        }
        (Some("art"), [_, action, game, options @ ..]) if action == "sector-room-wide-inspect" => {
            inspect_room_overview(Path::new(game), options, policy)
        }
        (Some("art"), [_, action, game, options @ ..]) if action == "world-inspect" => {
            inspect_world(Path::new(game), options, policy)
        }
        #[cfg(feature = "windowed")]
        (Some("art"), [_, action, game, options @ ..]) if action == "world" => {
            show_world(Path::new(game), options, policy)
        }
        #[cfg(feature = "windowed")]
        (Some("art"), [_, action, game, options @ ..])
            if action == "sector-room" || action == "sector-room-wide" =>
        {
            show_room(
                Path::new(game),
                options,
                policy,
                action == "sector-room-wide",
            )
        }
        #[cfg(feature = "windowed")]
        (Some("art"), [_, action, game, options @ ..])
            if action == "sector"
                || action == "sector-wide"
                || action == "sector-facade"
                || action == "sector-wall"
                || action == "sector-walls" =>
        {
            show_sector_floor(Path::new(game), options, policy, action)
        }
        #[cfg(feature = "windowed")]
        (Some("art"), [_, action]) if action == "floor-demo" && policy == Policy::Ce => {
            viewer::show_floor(
                "Arcanum-RS | Synthetic isometric floor".into(),
                &arcanum_rs::isometric::synthetic_floor(),
                &viewer::synthetic_frame(),
            )
            .map_err(Into::into)
        }
        #[cfg(feature = "windowed")]
        (Some("art"), [_, action, game, options @ ..]) if action == "floor" => {
            show_art_floor(Path::new(game), options, policy)
        }
        #[cfg(feature = "windowed")]
        (Some("art"), [_, action, game, options @ ..]) if action == "overlap" => {
            show_art_overlap(Path::new(game), options, policy)
        }
        #[cfg(feature = "windowed")]
        (Some("art"), [_, action, game, resource, frame, options @ ..]) if action == "view" => {
            inspect_art(
                Path::new(game),
                resource,
                frame,
                ArtAction::View,
                options,
                policy,
            )
        }
        #[cfg(not(feature = "windowed"))]
        (Some("art"), [_, action, ..])
            if action == "view"
                || action == "demo"
                || action == "overlap"
                || action == "overlap-demo"
                || action == "floor"
                || action == "floor-demo"
                || action == "sector"
                || action == "sector-wide"
                || action == "sector-wall"
                || action == "sector-walls"
                || action == "sector-room"
                || action == "sector-room-wide"
                || action == "world"
                || action == "sector-facade" =>
        {
            Err("sprite presentation requires a build with --features windowed".into())
        }
        (Some("art"), [_, action, game, resource, frame, options @ ..]) if action == "inspect" => {
            inspect_art(
                Path::new(game),
                resource,
                frame,
                ArtAction::Inspect,
                options,
                policy,
            )
        }
        (Some("art"), [_, action, game, resource, frame, output, options @ ..])
            if action == "export" =>
        {
            inspect_art(
                Path::new(game),
                resource,
                frame,
                ArtAction::Export(Path::new(output)),
                options,
                policy,
            )
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

fn resistance_demo(out: &mut impl Write) -> Result<(), Box<dyn Error>> {
    let item = items::GenericDefinition {
        id: items::ItemId("demo:item/fire-charm".into()),
        display_name: "Fire charm".into(),
        base_weight: 1,
        base_worth: 0,
        magic_tech_complexity: 0,
    };
    let effect = EffectDefinition {
        id: EffectId("arcanum:effect/53".into()),
        operations: vec![EffectOperation::ResistanceFloor {
            channel: items::ResistanceKind::Fire,
            minimum_percent: 50,
        }],
    };
    let mut runtime = Runtime::default();
    let actor = runtime.create_actor(10)?;
    runtime.set_underlying_fire_resistance(actor, 20)?;
    writeln!(
        out,
        "{actor:?} starts with underlying Fire resistance {}%",
        runtime.underlying_fire_resistance(actor)?
    )?;
    let first = runtime.instantiate_item(ItemDefinitionRef::Generic(&item))?;
    let second = runtime.instantiate_item(ItemDefinitionRef::Generic(&item))?;
    let mut grant = |source| -> Result<_, Box<dyn Error>> {
        runtime.equip(source, actor)?;
        let id = runtime.activate_effect(source, actor, &effect)?;
        writeln!(
            out,
            "Equipped {source:?}, granted {} as {id:?}, Fire resistance {}%",
            effect.id.0,
            runtime.fire_resistance(actor)?
        )?;
        Ok(id)
    };
    grant(first)?;
    let remaining = grant(second)?;
    runtime.unequip(first, actor)?;
    let surviving = runtime.activation(remaining)?;
    writeln!(
        out,
        "Unequipped {first:?}, {:?} from {:?} remains, Fire resistance {}%",
        surviving.id,
        surviving.source,
        runtime.fire_resistance(actor)?
    )?;
    runtime.unequip(second, actor)?;
    writeln!(
        out,
        "Unequipped {second:?}, Fire resistance {}%",
        runtime.fire_resistance(actor)?
    )?;
    Ok(())
}

fn electrical_demo(out: &mut impl Write) -> Result<(), Box<dyn Error>> {
    let item = items::GenericDefinition {
        id: items::ItemId("demo:item/electrical-charm".into()),
        display_name: "Electrical charm".into(),
        base_weight: 1,
        base_worth: 0,
        magic_tech_complexity: 0,
    };
    let effect = EffectDefinition {
        id: EffectId("demo:effect/electrical-adjustment".into()),
        operations: vec![EffectOperation::AddResistance {
            channel: items::ResistanceKind::Electrical,
            percentage_points: 8,
        }],
    };
    let mut runtime = Runtime::default();
    let actor = runtime.create_actor(10)?;
    runtime.set_underlying_electrical_resistance(actor, 30)?;
    writeln!(
        out,
        "{actor:?} starts with underlying Electrical resistance {}%",
        runtime.underlying_electrical_resistance(actor)?
    )?;
    let first = runtime.instantiate_item(ItemDefinitionRef::Generic(&item))?;
    let second = runtime.instantiate_item(ItemDefinitionRef::Generic(&item))?;
    let mut grant = |source| -> Result<_, Box<dyn Error>> {
        runtime.equip(source, actor)?;
        let id = runtime.activate_effect(source, actor, &effect)?;
        writeln!(
            out,
            "Equipped {source:?}, granted {id:?} ({:+} points), intermediate {}%",
            runtime.activation(id)?.electrical_resistance_points,
            runtime.electrical_resistance_after_effects(actor)?
        )?;
        Ok(id)
    };
    grant(first)?;
    let remaining = grant(second)?;
    runtime.unequip(first, actor)?;
    let surviving = runtime.activation(remaining)?;
    writeln!(
        out,
        "Unequipped {first:?}, {:?} remains, intermediate {}%",
        surviving.id,
        runtime.electrical_resistance_after_effects(actor)?
    )?;
    runtime.set_underlying_electrical_resistance(actor, 92)?;
    writeln!(
        out,
        "Underlying changed to {}%, intermediate {}%, ordinary query {}%",
        runtime.underlying_electrical_resistance(actor)?,
        runtime.electrical_resistance_after_effects(actor)?,
        runtime.electrical_resistance(actor)?
    )?;
    runtime.unequip(second, actor)?;
    writeln!(
        out,
        "Unequipped {second:?}, intermediate {}%, ordinary query {}%",
        runtime.electrical_resistance_after_effects(actor)?,
        runtime.electrical_resistance(actor)?
    )?;
    Ok(())
}

fn composite_demo(out: &mut impl Write) -> Result<(), Box<dyn Error>> {
    use items::ResistanceKind::{Electrical, Fire, Poison};

    let item = items::GenericDefinition {
        id: items::ItemId("demo:item/composite-charm".into()),
        display_name: "Composite charm".into(),
        base_weight: 1,
        base_worth: 0,
        magic_tech_complexity: 0,
    };
    let effect = EffectDefinition {
        id: EffectId("arcanum:effect/54".into()),
        operations: vec![
            EffectOperation::ResistanceFloor {
                channel: Fire,
                minimum_percent: 95,
            },
            EffectOperation::AddResistance {
                channel: Electrical,
                percentage_points: 8,
            },
            EffectOperation::ResistanceFloor {
                channel: Poison,
                minimum_percent: 97,
            },
        ],
    };
    let mut runtime = Runtime::default();
    let actor = runtime.create_actor(10)?;
    runtime.set_underlying_fire_resistance(actor, 20)?;
    runtime.set_underlying_electrical_resistance(actor, 30)?;
    runtime.set_underlying_poison_resistance(actor, 10)?;
    let show_resistance = |out: &mut dyn Write,
                           runtime: &Runtime<'_>|
     -> Result<(), Box<dyn Error>> {
        writeln!(
            out,
            "  Intermediate / ordinary query: Fire {}% / {}%, Electrical {}% / {}%, Poison {}% / {}%",
            runtime.fire_resistance_after_effects(actor)?,
            runtime.fire_resistance(actor)?,
            runtime.electrical_resistance_after_effects(actor)?,
            runtime.electrical_resistance(actor)?,
            runtime.poison_resistance_after_effects(actor)?,
            runtime.poison_resistance(actor)?
        )?;
        Ok(())
    };
    writeln!(
        out,
        "{actor:?} underlying: Fire {}%, Electrical {}%, Poison {}%",
        runtime.underlying_fire_resistance(actor)?,
        runtime.underlying_electrical_resistance(actor)?,
        runtime.underlying_poison_resistance(actor)?
    )?;
    let first = runtime.instantiate_item(ItemDefinitionRef::Generic(&item))?;
    let second = runtime.instantiate_item(ItemDefinitionRef::Generic(&item))?;
    let mut grant = |source| -> Result<_, Box<dyn Error>> {
        runtime.equip(source, actor)?;
        let id = runtime.activate_effect(source, actor, &effect)?;
        writeln!(
            out,
            "Equipped {source:?}, granted {} as {id:?}",
            effect.id.0
        )?;
        show_resistance(out, &runtime)?;
        Ok(id)
    };
    let first_grant = grant(first)?;
    let remaining = grant(second)?;
    runtime.remove_activation(first_grant)?;
    let surviving = runtime.activation(remaining)?;
    writeln!(
        out,
        "Removed {first_grant:?}, {:?} from {:?} remains",
        surviving.id, surviving.source
    )?;
    show_resistance(out, &runtime)?;

    runtime.set_underlying_fire_resistance(actor, 98)?;
    runtime.set_underlying_electrical_resistance(actor, 92)?;
    runtime.set_underlying_poison_resistance(actor, 90)?;
    writeln!(
        out,
        "Underlying changed: Fire {}%, Electrical {}%, Poison {}%",
        runtime.underlying_fire_resistance(actor)?,
        runtime.underlying_electrical_resistance(actor)?,
        runtime.underlying_poison_resistance(actor)?
    )?;
    show_resistance(out, &runtime)?;
    runtime.unequip(second, actor)?;
    writeln!(out, "Unequipped {second:?}, removed {remaining:?}")?;
    show_resistance(out, &runtime)
}

fn same_channel_demo(out: &mut impl Write) -> Result<(), Box<dyn Error>> {
    use items::ResistanceKind::Electrical;

    let item = items::GenericDefinition {
        id: items::ItemId("demo:item/grounding-charm".into()),
        display_name: "Grounding charm".into(),
        base_weight: 1,
        base_worth: 0,
        magic_tech_complexity: 0,
    };
    let addition = EffectDefinition {
        id: EffectId("demo:effect/electrical-addition".into()),
        operations: vec![EffectOperation::AddResistance {
            channel: Electrical,
            percentage_points: 8,
        }],
    };
    let floor = EffectDefinition {
        id: EffectId("demo:effect/electrical-floor".into()),
        operations: vec![EffectOperation::ResistanceFloor {
            channel: Electrical,
            minimum_percent: 50,
        }],
    };
    let mut runtime = Runtime::default();
    let actor = runtime.create_actor(10)?;
    let show = |out: &mut dyn Write, label, runtime: &Runtime<'_>| -> Result<(), Box<dyn Error>> {
        writeln!(
            out,
            "{label}: underlying {}%, intermediate {}%, queried {}%",
            runtime.underlying_electrical_resistance(actor)?,
            runtime.electrical_resistance_after_effects(actor)?,
            runtime.electrical_resistance(actor)?
        )?;
        Ok(())
    };
    writeln!(
        out,
        "Synthetic Electrical scenario with +8 points and a 50% floor"
    )?;
    runtime.set_underlying_electrical_resistance(actor, 20)?;
    let first = runtime.instantiate_item(ItemDefinitionRef::Generic(&item))?;
    let second = runtime.instantiate_item(ItemDefinitionRef::Generic(&item))?;
    runtime.equip(first, actor)?;
    runtime.activate_effect(first, actor, &addition)?;
    show(out, "Addition source equipped", &runtime)?;
    runtime.equip(second, actor)?;
    runtime.activate_effect(second, actor, &floor)?;
    show(out, "Floor source equipped", &runtime)?;
    for value in [45, 92, 45] {
        runtime.set_underlying_electrical_resistance(actor, value)?;
        show(out, "Underlying changed", &runtime)?;
    }
    runtime.unequip(first, actor)?;
    show(out, "Addition source removed", &runtime)?;
    runtime.unequip(second, actor)?;
    show(out, "Floor source removed", &runtime)
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

#[cfg(feature = "windowed")]
fn sprite_demo() -> Result<(), Box<dyn Error>> {
    let frame = viewer::synthetic_frame();
    writeln!(
        io::stdout(),
        "Synthetic sprite {} × {}, hotspot {:?}, frame offset {:?}",
        frame.width,
        frame.height,
        frame.hotspot,
        frame.offset
    )?;
    Ok(viewer::show("Arcanum-RS | Synthetic sprite".into(), frame)?)
}

enum ArtAction<'a> {
    Inspect,
    Export(&'a Path),
    #[cfg(feature = "windowed")]
    View,
}

fn inspect_art(
    game: &Path,
    resource: &OsStr,
    frame: &OsStr,
    action: ArtAction<'_>,
    options: &[OsString],
    policy: Policy,
) -> Result<(), Box<dyn Error>> {
    let resource = resource_name(resource)?;
    let index = frame
        .to_str()
        .and_then(|frame| frame.parse::<usize>().ok())
        .ok_or_else(|| format!("invalid ART frame {frame:?}; expected a zero-based index"))?;
    let loaded =
        content::art::load_frame(&mut inspection_vfs(game, options, policy)?, resource, index)?;
    let mut out = io::stdout().lock();
    report_art(&mut out, &loaded, index)?;
    match action {
        ArtAction::Inspect => {}
        ArtAction::Export(destination) => {
            export_pam(destination, &loaded.decoded.frame)?;
            writeln!(out, "Exported: {}", destination.display())?;
        }
        #[cfg(feature = "windowed")]
        ArtAction::View => {
            drop(out);
            viewer::show(
                format!("Arcanum-RS | {} frame {index}", loaded.source.virtual_path),
                loaded.decoded.frame,
            )?;
        }
    }
    Ok(())
}

#[cfg(feature = "windowed")]
fn show_art_overlap(
    game: &Path,
    options: &[OsString],
    policy: Policy,
) -> Result<(), Box<dyn Error>> {
    let samples = [
        ("art/item/bread.art", 0, (-12, 8)),
        ("art/interface/align.art", 10, (12, 17)),
    ];
    let mut vfs = inspection_vfs(game, options, policy)?;
    let mut frames = Vec::with_capacity(samples.len());
    let mut out = io::stdout().lock();
    for (order, (path, index, relative_anchor)) in samples.into_iter().enumerate() {
        let loaded = content::art::load_frame(&mut vfs, path, index)?;
        writeln!(
            out,
            "Draw {} (back to front), anchor offset {relative_anchor:?}",
            order + 1
        )?;
        report_art(&mut out, &loaded, index)?;
        frames.push((loaded.decoded.frame, relative_anchor));
    }
    drop(out);
    drop(vfs);
    let layers: Vec<_> = frames
        .iter()
        .map(|(frame, relative_anchor)| viewer::Layer {
            frame,
            relative_anchor: *relative_anchor,
        })
        .collect();
    Ok(viewer::show_layers(
        "Arcanum-RS | Original ART overlap".into(),
        &layers,
    )?)
}

#[cfg(feature = "windowed")]
fn show_art_floor(game: &Path, options: &[OsString], policy: Policy) -> Result<(), Box<dyn Error>> {
    let (floor, foreground) = {
        let mut vfs = inspection_vfs(game, options, policy)?;
        (
            content::art::load_frame(&mut vfs, "art/tile/fl1bse0a.art", 0)?,
            content::art::load_frame(&mut vfs, "art/item/bread.art", 0)?,
        )
    };
    let mut out = io::stdout().lock();
    writeln!(out, "Floor (3 × 3, increasing tile y then x)")?;
    report_art(&mut out, &floor, 0)?;
    writeln!(out, "Foreground at tile (0, 0)")?;
    report_art(&mut out, &foreground, 0)?;
    drop(out);
    Ok(viewer::show_floor(
        "Arcanum-RS | Original isometric floor".into(),
        &floor.decoded.frame,
        &foreground.decoded.frame,
    )?)
}

#[cfg(feature = "windowed")]
fn show_sector_floor(
    game: &Path,
    options: &[OsString],
    policy: Policy,
    action: &OsStr,
) -> Result<(), Box<dyn Error>> {
    use content::sector::{
        DEMO_MAP, DEMO_ORIGIN, DEMO_SECTOR, FACADE_ORIGIN, WIDE_ORIGIN, WIDE_SIDE,
        load_facade_region, load_patch, load_region,
    };

    let mut vfs = inspection_vfs(game, options, policy)?;
    let walls = if action == "sector-walls" {
        load_connected_walls(&mut vfs)?
    } else if action == "sector-wall" {
        vec![load_demo_wall(&mut vfs)?]
    } else {
        Vec::new()
    };
    let patch = if action == "sector-walls" {
        load_facade_region(
            &mut vfs,
            DEMO_MAP,
            DEMO_SECTOR,
            content::wall::CONNECTED_ORIGIN,
            WIDE_SIDE,
        )?
    } else if action == "sector-wall" {
        load_facade_region(
            &mut vfs,
            DEMO_MAP,
            DEMO_SECTOR,
            content::wall::DEMO_ORIGIN,
            WIDE_SIDE,
        )?
    } else if action == "sector-facade" {
        load_facade_region(&mut vfs, DEMO_MAP, DEMO_SECTOR, FACADE_ORIGIN, WIDE_SIDE)?
    } else if action == "sector-wide" {
        load_region(&mut vfs, DEMO_MAP, DEMO_SECTOR, WIDE_ORIGIN, WIDE_SIDE)?
    } else {
        load_patch(&mut vfs, DEMO_MAP, DEMO_SECTOR, DEMO_ORIGIN)?
    };
    drop(vfs);
    let mut out = io::stdout().lock();
    writeln!(
        out,
        "Sector: {} from {}",
        patch.source.virtual_path, patch.source
    )?;
    writeln!(
        out,
        "Terrain names: {} from {}",
        patch.names_source.virtual_path, patch.names_source
    )?;
    if let Some(source) = &patch.facade_names_source {
        writeln!(out, "Facade names: {} from {source}", source.virtual_path)?;
    }
    writeln!(
        out,
        "Lights: {}, tile array at byte {}",
        patch.light_count, patch.tile_offset
    )?;
    writeln!(
        out,
        "Patch center: {:?}, {} surface cells ({} facade pieces)",
        patch.center,
        patch.tiles().len(),
        patch
            .tiles()
            .iter()
            .filter(|tile| tile.facade.is_some())
            .count()
    )?;
    let mut seen = std::collections::BTreeSet::new();
    for frame in patch.artwork() {
        if seen.insert(frame.source.virtual_path.as_str()) {
            writeln!(
                out,
                "ART: {} from {}",
                frame.source.virtual_path, frame.source
            )?;
        }
    }
    let mut layers: Vec<_> = patch
        .draws()
        .map(|draw| viewer::Layer {
            frame: draw.frame,
            // The bounded sector region fits within 320 pixels of center.
            relative_anchor: (draw.anchor.0 as i16, draw.anchor.1 as i16),
        })
        .collect();
    if action == "sector-walls" {
        report_connected_walls(&mut out, &walls)?;
    } else if let [wall] = walls.as_slice() {
        report_wall(&mut out, wall)?;
    }
    for wall in &walls {
        let draw = wall
            .draw(patch.center)
            .ok_or("wall anchor exceeds the drawable range")?;
        layers.push(viewer::Layer {
            frame: draw.frame,
            relative_anchor: (i16::try_from(draw.anchor.0)?, i16::try_from(draw.anchor.1)?),
        });
    }
    drop(out);
    Ok(viewer::show_layers(
        if action == "sector-walls" {
            "Arcanum-RS | Bates Mansion wall run"
        } else if action == "sector-wall" {
            "Arcanum-RS | Bates Mansion wall"
        } else if action == "sector-facade" {
            "Arcanum-RS | Bates Mansion facade"
        } else {
            "Arcanum-RS | Bates Mansion floor"
        }
        .into(),
        &layers,
    )?)
}

#[cfg(feature = "windowed")]
fn show_room(
    game: &Path,
    options: &[OsString],
    policy: Policy,
    wide: bool,
) -> Result<(), Box<dyn Error>> {
    let mut vfs = inspection_vfs(game, options, policy)?;
    let room = if wide {
        let overview = content::scenery::load_room_overview(&mut vfs)?;
        report_room_overview(&mut io::stdout().lock(), &overview)?;
        overview.room
    } else {
        let room = content::scenery::load_room(&mut vfs)?;
        report_room(&mut io::stdout().lock(), &room)?;
        room
    };
    drop(vfs);
    if wide {
        present_room(
            &room,
            "Arcanum-RS | Bates Mansion Level 1 overview",
            Some(content::scenery::OVERVIEW_SIZE),
        )
    } else {
        present_room(&room, "Arcanum-RS | Bates Mansion room", None)
    }
}

#[cfg(feature = "windowed")]
fn show_world(game: &Path, options: &[OsString], policy: Policy) -> Result<(), Box<dyn Error>> {
    if let Some((world, camera, size, trace, map, renderer, animation_ms)) =
        streaming_world(game, options, policy)?
    {
        return viewer::show_stream(
            format!("Arcanum-RS | {map} world"),
            world,
            camera,
            size,
            trace,
            renderer,
            animation_ms,
        )
        .map_err(Into::into);
    }
    use arcanum_rs::camera::{Camera, CameraScene};
    use content::scenery::{WorldRegion, load_navigation_world};
    let default = WorldRegion {
        origin: (0, 0),
        side: arcanum_rs::sector::SIDE as u8,
        ..WorldRegion::BATES_MANSION
    };
    let (region, options) = world_options(default, options)?;
    let mut vfs = inspection_vfs(game, options, policy)?;
    let world = load_navigation_world(&mut vfs, region)?;
    drop(vfs);
    let first = region
        .sector
        .world_tile(region.origin.0, region.origin.1)
        .ok_or("invalid camera floor origin")?;
    let camera = Camera::new(region.center, first, region.side).ok_or("invalid camera region")?;
    let room = &world.room;
    let scene = CameraScene::new(
        room.draws()
            .ok_or("world anchor exceeds the drawable range")?,
    );
    let cache = scene.cache();
    println!(
        "Navigation: {} from {}; {} tiles, {} walls, {} scenery, {} portals, {} unsupported records",
        room.floor.source.virtual_path,
        room.floor.source,
        room.floor.tiles().len(),
        room.walls.len(),
        room.scenery.len(),
        room.portals.len(),
        world.issues.len(),
    );
    println!(
        "Reference tile {:?}; {} cached rasters ({} RGBA bytes), {} reused draw references",
        region.center, cache.rasters, cache.pixel_bytes, cache.repeated_references,
    );
    for issue in &world.issues {
        println!(
            "Skipped record {} {:?} at {:?}: {}",
            issue.record, issue.category, issue.tile, issue.reason
        );
    }
    viewer::show_camera(
        "Arcanum-RS | Bates Mansion world".into(),
        &scene,
        camera,
        region.viewport,
    )
    .map_err(Into::into)
}

fn inspect_world(game: &Path, options: &[OsString], policy: Policy) -> Result<(), Box<dyn Error>> {
    if let Some((mut world, mut camera, size, _, _, _, _)) = streaming_world(game, options, policy)?
    {
        camera.resize(size);
        let timing = world.prepare(&camera, size)?;
        let mut out = io::stdout().lock();
        for report in world.reports() {
            writeln!(
                out,
                "Sector {}: {}",
                report.id,
                report.source.as_deref().unwrap_or("unavailable")
            )?;
            let (walls, portals, scenery) = report.original_object_counts;
            writeln!(
                out,
                "{} surfaces ({} facades), original {walls} walls / {scenery} scenery / {portals} portals, retained {} / {} / {}",
                report.surfaces,
                report.facades,
                report.wall_count,
                report.scenery_count,
                report.portal_count
            )?;
            if let Some((submitted, culled)) = world.object_visibility(report.id, &camera, size) {
                writeln!(
                    out,
                    "  Object rasters: {submitted} submitted, {culled} viewport-culled"
                )?;
            }
            if let Some((total, prepared, off)) = report.light_counts {
                writeln!(
                    out,
                    "  Sector lights: {total} original, {prepared} prepared, {off} off, {} unsupported",
                    total - prepared - off
                )?;
            }
            if report.object_lights != 0 {
                writeln!(
                    out,
                    "  Prepared object-owned lights: {}",
                    report.object_lights
                )?;
            }
            for diagnostic in &report.diagnostics {
                writeln!(out, "  {diagnostic}")?;
            }
        }
        writeln!(
            out,
            "World cache {:?}, preparation {:?}",
            world.stats(),
            timing
        )?;
        return Ok(());
    }
    use content::scenery::{WorldRegion, load_world};
    let (region, options) = world_options(WorldRegion::BATES_MANSION, options)?;
    let mut vfs = inspection_vfs(game, options, policy)?;
    let overview = load_world(&mut vfs, region)?;
    report_room_overview(&mut io::stdout().lock(), &overview)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum RenderBackend {
    #[default]
    Cpu,
    Vulkan,
    Gles,
}

type StreamPreview = (
    content::world::WorldStream,
    arcanum_rs::camera::Camera,
    (u32, u32),
    bool,
    String,
    RenderBackend,
    Option<u64>,
);

fn streaming_world(
    game: &Path,
    options: &[OsString],
    policy: Policy,
) -> Result<Option<StreamPreview>, Box<dyn Error>> {
    use arcanum_rs::sector::Coordinates;
    use content::world::{SectorRegion, WorldStream};
    let [flag, rest @ ..] = options else {
        return Ok(None);
    };
    if flag != "--sectors" {
        return Ok(None);
    }
    let [
        map,
        min_x,
        min_y,
        max_x,
        max_y,
        start_x,
        start_y,
        width,
        height,
        rest @ ..,
    ] = rest
    else {
        return Err(
            "expected --sectors MAP MIN_X MIN_Y MAX_X MAX_Y START_X START_Y WIDTH HEIGHT".into(),
        );
    };
    let map = resource_name(map)?;
    let region = SectorRegion {
        first: Coordinates {
            x: parse_view_number(min_x, "first sector X")?,
            y: parse_view_number(min_y, "first sector Y")?,
        },
        last: Coordinates {
            x: parse_view_number(max_x, "last sector X")?,
            y: parse_view_number(max_y, "last sector Y")?,
        },
    };
    let reference = (
        parse_view_number(start_x, "world X")?,
        parse_view_number(start_y, "world Y")?,
    );
    let size = (
        parse_view_number::<u32>(width, "width")?,
        parse_view_number::<u32>(height, "height")?,
    );
    if size.0 == 0 || size.1 == 0 || u64::from(size.0) * u64::from(size.1) > 16 * 1024 * 1024 {
        return Err("invalid world viewport size".into());
    }
    let camera = region
        .camera(reference)
        .ok_or("invalid sector rectangle or world camera position")?;
    let mut trace = false;
    let mut hour = None;
    let mut animation_ms = None;
    let mut fidget = false;
    let mut probes = Vec::new();
    let mut renderer = RenderBackend::Cpu;
    let mut options = rest;
    loop {
        options = match options {
            [flag, rest @ ..] if flag == "--trace" => {
                trace = true;
                rest
            }
            [flag, hour_arg, rest @ ..] if flag == "--lighting" => {
                hour = Some(parse_view_number(hour_arg, "lighting hour")?);
                rest
            }
            [flag, rest @ ..] if flag == "--fidget" => {
                fidget = true;
                rest
            }
            [flag, elapsed, rest @ ..] if flag == "--animation-ms" => {
                animation_ms = Some(parse_view_number(elapsed, "animation milliseconds")?);
                rest
            }
            [flag, x, y, rest @ ..] if flag == "--light-probe" || flag == "--nocturnal-probe" => {
                if probes.len() == 2 {
                    return Err("at most two light probes are supported".into());
                }
                probes.push((
                    (
                        parse_view_number(x, "light probe X")?,
                        parse_view_number(y, "light probe Y")?,
                    ),
                    flag == "--nocturnal-probe",
                ));
                rest
            }
            [flag, backend, rest @ ..] if flag == "--renderer" => {
                renderer = match backend.to_str() {
                    Some("cpu") => RenderBackend::Cpu,
                    Some("vulkan") => RenderBackend::Vulkan,
                    Some("gles") => RenderBackend::Gles,
                    _ => return Err("expected renderer cpu, vulkan or gles".into()),
                };
                rest
            }
            [flag] if flag == "--renderer" || flag == "--lighting" || flag == "--animation-ms" => {
                return Err(format!("missing value after {}", flag.to_string_lossy()).into());
            }
            _ => break,
        };
    }
    let vfs = inspection_vfs(game, options, policy)?;
    let mut world = WorldStream::new(vfs, map.to_owned(), region, reference)?;
    if let Some(hour) = hour {
        world = world.with_lighting(hour)?;
    }
    if fidget {
        world = world.with_fidget_preview();
    }
    for (tile, nocturnal) in probes {
        world.add_light_probe(tile, nocturnal)?;
    }
    Ok(Some((
        world,
        camera,
        size,
        trace,
        map.to_owned(),
        renderer,
        animation_ms,
    )))
}

fn world_options<'a>(
    mut region: content::scenery::WorldRegion<'static>,
    options: &'a [OsString],
) -> Result<(content::scenery::WorldRegion<'static>, &'a [OsString]), Box<dyn Error>> {
    let options = match options {
        [
            flag,
            x,
            y,
            side,
            center_x,
            center_y,
            width,
            height,
            rest @ ..,
        ] if flag == "--view" => {
            region.origin = (
                parse_view_number(x, "local X")?,
                parse_view_number(y, "local Y")?,
            );
            region.side = parse_view_number(side, "side")?;
            region.center = (
                parse_view_number(center_x, "center X")?,
                parse_view_number(center_y, "center Y")?,
            );
            region.viewport = (
                parse_view_number(width, "width")?,
                parse_view_number(height, "height")?,
            );
            rest
        }
        [flag, ..] if flag == "--view" => return Err("expected seven numbers after --view".into()),
        options => options,
    };
    Ok((region, options))
}

fn parse_view_number<T: std::str::FromStr>(
    value: &OsStr,
    field: &str,
) -> Result<T, Box<dyn Error>> {
    value
        .to_str()
        .and_then(|text| text.parse().ok())
        .ok_or_else(|| format!("invalid world preview {field}: {value:?}").into())
}

#[cfg(feature = "windowed")]
fn present_room(
    room: &content::scenery::Room,
    title: &str,
    size: Option<(u32, u32)>,
) -> Result<(), Box<dyn Error>> {
    let layers = room
        .draws()
        .ok_or("room anchor exceeds the drawable range")?
        .map(|draw| {
            Ok(viewer::Layer {
                frame: draw.frame,
                relative_anchor: (i16::try_from(draw.anchor.0)?, i16::try_from(draw.anchor.1)?),
            })
        })
        .collect::<Result<Vec<_>, std::num::TryFromIntError>>()?;
    if let Some(size) = size {
        Ok(viewer::show_layers_at_size(title.into(), &layers, size)?)
    } else {
        Ok(viewer::show_layers(title.into(), &layers)?)
    }
}

fn inspect_room(game: &Path, options: &[OsString], policy: Policy) -> Result<(), Box<dyn Error>> {
    let room = content::scenery::load_room(&mut inspection_vfs(game, options, policy)?)?;
    report_room(&mut io::stdout().lock(), &room)
}

fn inspect_room_overview(
    game: &Path,
    options: &[OsString],
    policy: Policy,
) -> Result<(), Box<dyn Error>> {
    let mut vfs = inspection_vfs(game, options, policy)?;
    let overview = content::scenery::load_room_overview(&mut vfs)?;
    report_room_overview(&mut io::stdout().lock(), &overview)
}

fn report_room_overview(
    out: &mut impl Write,
    overview: &content::scenery::RoomOverview,
) -> Result<(), Box<dyn Error>> {
    let room = &overview.room;
    let (walls, portals, scenery) = overview.sector_object_counts;
    writeln!(
        out,
        "Base sector contains {walls} walls, {portals} portals and {scenery} scenery records"
    )?;
    writeln!(
        out,
        "Raster intersects preview outside the floor patch: {} walls, {} scenery",
        overview.outside_floor_walls, overview.outside_floor_scenery
    )?;
    report_room(out, room)?;
    for leaf in &overview.culled_portals {
        writeln!(
            out,
            "Culled record {} {:?} (Portal type 1) at {:?}, ART {:#010x}, state {:?}, frame {}, rotation {}, palette {}, mirror {}: outside viewport",
            leaf.record,
            leaf.art.kind,
            leaf.tile,
            leaf.art_id,
            leaf.state,
            leaf.art.frame,
            leaf.art.rotation,
            leaf.art.palette,
            leaf.art.mirror_x,
        )?;
    }
    for issue in &overview.issues {
        writeln!(
            out,
            "Skipped record {} {:?} at {:?} ART {:#010x}: {}",
            issue.record, issue.category, issue.tile, issue.art_id, issue.reason
        )?;
    }
    Ok(())
}

fn report_room(out: &mut impl Write, room: &content::scenery::Room) -> Result<(), Box<dyn Error>> {
    let floor = &room.floor;
    writeln!(
        out,
        "Sector {} from {}",
        floor.source.virtual_path, floor.source
    )?;
    writeln!(
        out,
        "Center {:?}, {} surfaces, {} walls, {} scenery records",
        floor.center,
        floor.tiles().len(),
        room.walls.len(),
        room.scenery.len()
    )?;
    for scenery in &room.scenery {
        let record = &scenery.record;
        let art = &scenery.artwork;
        let fields = &scenery.appearance;
        let draw = scenery
            .draw(floor.center)
            .ok_or("room anchor exceeds the drawable range")?;
        let top_left = arcanum_rs::compositor::sprite_origin(draw.frame, draw.anchor);
        writeln!(
            out,
            "Record {} Scenery (type 3), bytes {:?}, tile {:?}, prototype {} from {}, ART {:#010x}: {} from {}, rotation {}, frame {}, palette {}, offset {:?}, scale {}%, hotspot {:?}, relative raster origin {:?}, object flags {:#x}, scenery flags {:#x}",
            record.index,
            record.span,
            scenery.tile,
            record.prototype.0,
            scenery.prototype_source,
            record.art_id,
            art.source.virtual_path,
            art.source,
            scenery.art.rotation,
            scenery.art.frame,
            scenery.art.palette,
            fields.offset,
            fields.scale,
            draw.frame.hotspot,
            top_left,
            fields.flags,
            fields.scenery_flags
        )?;
    }
    for portal in &room.portals {
        let path = &portal.artwork.source;
        let draw = portal
            .draw(floor.center)
            .ok_or("portal anchor exceeds the drawable range")?;
        writeln!(
            out,
            "Record {} {:?} (Portal type 1) bytes {:?}, tile {:?}, wall {}, prototype {} ART {:#010x} from {}, stored ART {:#010x}: {} from {}, frame {}, rotation {}, palette {}, mirror {}, state {:?}, offset {:?}, scale {}%, flags {:#x}, portal flags {:#x}, hotspot {:?}, pivot {:?}, relative raster origin {:?}",
            portal.record.index,
            portal.art.kind,
            portal.record.span,
            portal.placement.tile,
            portal.paired_wall_index,
            portal.record.prototype.0,
            portal.prototype_art_id,
            portal.prototype_source,
            portal.record.art_id,
            path.virtual_path,
            path,
            portal.art.frame,
            portal.art.rotation,
            portal.art.palette,
            portal.art.mirror_x,
            portal.state,
            portal.appearance.offset,
            portal.appearance.scale,
            portal.appearance.flags,
            portal.appearance.portal_flags,
            draw.frame.hotspot,
            portal.art.pivot(draw.frame),
            arcanum_rs::compositor::sprite_origin(draw.frame, draw.anchor),
        )?;
    }
    Ok(())
}

fn load_demo_wall(vfs: &mut Vfs) -> Result<content::wall::ImportedWall, Box<dyn Error>> {
    let sector = content::sector::DEMO_SECTOR;
    content::wall::load(
        vfs,
        content::sector::DEMO_MAP,
        sector,
        content::wall::DEMO_RECORD,
    )
}

fn inspect_demo_wall(
    game: &Path,
    options: &[OsString],
    policy: Policy,
) -> Result<(), Box<dyn Error>> {
    let mut vfs = inspection_vfs(game, options, policy)?;
    report_wall(&mut io::stdout().lock(), &load_demo_wall(&mut vfs)?)
}

fn load_connected_walls(vfs: &mut Vfs) -> Result<Vec<content::wall::ImportedWall>, Box<dyn Error>> {
    content::wall::load_many(
        vfs,
        content::sector::DEMO_MAP,
        content::sector::DEMO_SECTOR,
        &content::wall::CONNECTED_RECORDS,
    )
}

fn inspect_connected_walls(
    game: &Path,
    options: &[OsString],
    policy: Policy,
) -> Result<(), Box<dyn Error>> {
    let mut vfs = inspection_vfs(game, options, policy)?;
    report_connected_walls(&mut io::stdout().lock(), &load_connected_walls(&mut vfs)?)
}

fn report_connected_walls(
    out: &mut impl Write,
    walls: &[content::wall::ImportedWall],
) -> Result<(), Box<dyn Error>> {
    if let Some(wall) = walls.first() {
        writeln!(
            out,
            "Sector {} from {}",
            wall.sector_source.virtual_path, wall.sector_source
        )?;
    }
    for wall in walls {
        let art = &wall.artwork;
        writeln!(
            out,
            "Record {} Wall (type 0) at {:?} ART {:#010x}, rotation {}, palette {}, mirrored {}, {} frame 0, offset {:?}, scale {}%, pivot {:?}, from {}",
            wall.record.index,
            wall.placement.tile,
            wall.record.art_id,
            wall.orientation.rotation,
            wall.orientation.palette,
            wall.orientation.mirror_x(),
            art.source.virtual_path,
            wall.placement.pixel_offset,
            wall.scale,
            wall.orientation.pivot(&art.decoded.frame),
            art.source
        )?;
    }
    Ok(())
}

fn report_wall(
    out: &mut impl Write,
    wall: &content::wall::ImportedWall,
) -> Result<(), Box<dyn Error>> {
    let (walls, portals, scenery) = wall.object_counts;
    let record = &wall.record;
    writeln!(
        out,
        "Sector: {} from {}",
        wall.sector_source.virtual_path, wall.sector_source
    )?;
    writeln!(
        out,
        "Object list byte {}: {walls} walls, {portals} portals, {scenery} scenery records",
        wall.object_list_offset
    )?;
    writeln!(
        out,
        "Record {}: Wall (type 0), bytes {:?}",
        record.index, record.span
    )?;
    writeln!(
        out,
        "Current ART: {:#010x} {:?}",
        record.art_id,
        arcanum_rs::wall::WallArtId(record.art_id).fields()?
    )?;
    writeln!(
        out,
        "World tile {:?}, serialized tile {:?}, offsets {:?}, scale {}%",
        wall.placement.tile, record.stored_tile, wall.placement.pixel_offset, wall.scale
    )?;
    writeln!(
        out,
        "Object flags {:#x}, wall flags {:#x}; stored overrides {:?}",
        wall.flags, wall.wall_flags, record.fields
    )?;
    writeln!(
        out,
        "Prototype {}: {} from {}",
        record.prototype.0, wall.prototype_source.virtual_path, wall.prototype_source
    )?;
    writeln!(
        out,
        "Structure table from {}; wall names from {}",
        wall.structure_source, wall.names_source
    )?;
    report_art(out, &wall.artwork, 0)?;
    writeln!(
        out,
        "Effective wall pivot: {:?}; original palette slot {}, without lighting",
        wall.orientation.pivot(&wall.artwork.decoded.frame),
        wall.orientation.palette
    )?;
    Ok(())
}

fn report_sector_audit(
    game: &Path,
    options: &[OsString],
    policy: Policy,
) -> Result<(), Box<dyn Error>> {
    use content::sector::{ArtworkCheck, DEMO_MAP, DEMO_SECTOR, audit_floor};
    let audit = audit_floor(
        &mut inspection_vfs(game, options, policy)?,
        DEMO_MAP,
        DEMO_SECTOR,
    )?;
    let mut out = io::stdout().lock();
    writeln!(
        out,
        "Sector: {} from {}",
        audit.source.virtual_path, audit.source
    )?;
    writeln!(out, "Terrain names: {}", audit.names_source)?;
    let (mut ready, mut missing, mut invalid, mut unsupported) = (0, 0, 0, 0);
    for entry in &audit.entries {
        let id = entry.id.0;
        let count = entry.occurrences;
        write!(out, "{id:#010x} × {count:4}  ")?;
        match &entry.resolution {
            Ok(tile) => {
                let [first, second] = &tile.families;
                writeln!(
                    out,
                    "{first}/{second} mask {:2} variant {} flip={} transition={} {} [{}]",
                    tile.fields.mask,
                    char::from(b'a' + tile.fields.variant),
                    tile.fields.flip_x,
                    tile.transition,
                    tile.path,
                    match &audit.artwork[&tile.path] {
                        ArtworkCheck::Ready(source) => {
                            ready += count;
                            source.to_string()
                        }
                        ArtworkCheck::Missing => {
                            missing += count;
                            "missing artwork".into()
                        }
                        ArtworkCheck::Invalid(reason) => {
                            invalid += count;
                            reason.clone()
                        }
                    }
                )?;
            }
            Err(error) => {
                unsupported += count;
                writeln!(out, "unsupported type {:#x} ({error})", id >> 28)?;
            }
        }
    }
    writeln!(
        out,
        "{} distinct IDs, {} distinct artwork paths",
        audit.entries.len(),
        audit.artwork.len()
    )?;
    writeln!(
        out,
        "Tiles: {ready} ready, {missing} missing artwork, {invalid} invalid artwork, {unsupported} unsupported IDs"
    )?;
    Ok(())
}

fn report_art(
    out: &mut impl Write,
    loaded: &content::art::LoadedFrame,
    index: usize,
) -> io::Result<()> {
    let frame = &loaded.decoded.frame;
    writeln!(out, "Path: {}", loaded.source.virtual_path)?;
    writeln!(out, "Source: {}", loaded.source)?;
    if let Some(id) = &loaded.source.mod_id {
        writeln!(out, "Mod: {id}")?;
    }
    writeln!(
        out,
        "Frame: {index} (zero-based, {} total)",
        loaded.decoded.frame_count
    )?;
    writeln!(out, "Dimensions: {} × {}", frame.width, frame.height)?;
    writeln!(out, "Hotspot: {}, {}", frame.hotspot.0, frame.hotspot.1)?;
    writeln!(out, "Offset: {}, {}", frame.offset.0, frame.offset.1)?;
    writeln!(
        out,
        "Stored FPS: {}, action frame: {}",
        loaded.decoded.frames_per_second, loaded.decoded.action_frame
    )?;
    writeln!(
        out,
        "ART input: {} bytes, RGBA8: {} bytes",
        loaded.input_bytes,
        frame.pixels.as_flattened().len()
    )?;
    if loaded.decoded.ignored_trailing_bytes != 0 {
        writeln!(
            out,
            "Ignored trailing ART bytes: {}",
            loaded.decoded.ignored_trailing_bytes
        )?;
    }
    Ok(())
}

fn export_pam(destination: &Path, frame: &arcanum_rs::art::SpriteFrame) -> io::Result<()> {
    let mut output = File::create_new(destination).map_err(|err| {
        io::Error::new(
            err.kind(),
            format!("cannot create {}: {err}", destination.display()),
        )
    })?;
    let result = write!(
        output,
        "P7\nWIDTH {}\nHEIGHT {}\nDEPTH 4\nMAXVAL 255\nTUPLTYPE RGB_ALPHA\nENDHDR\n",
        frame.width, frame.height
    )
    .and_then(|()| output.write_all(frame.pixels.as_flattened()))
    .and_then(|()| output.sync_all());
    drop(output);
    if let Err(err) = result {
        let message = match fs::remove_file(destination) {
            Ok(()) => format!("cannot export {}: {err}", destination.display()),
            Err(cleanup) => format!(
                "cannot export {}: {err}; cannot remove incomplete export: {cleanup}",
                destination.display()
            ),
        };
        return Err(io::Error::new(err.kind(), message));
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
