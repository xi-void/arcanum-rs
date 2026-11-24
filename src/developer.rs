//! Typed developer commands shared by interactive and non-visual callers.

use serde::Serialize;
use serde_json::{Value, json};

#[derive(Debug, PartialEq, Eq)]
pub enum Request {
    Maps(Option<String>),
    Load(String, Option<(u32, u32)>),
    Position,
    Teleport((u32, u32)),
    ToggleCollision,
    Inspect(Option<(u32, u32)>),
    Unlock,
    SetTime(u8),
    DiscoverAll,
    ReloadUi,
    Scale(u16),
    ShowCollision,
    ShowSectors,
    Fps,
    Sectors,
    Objects(ObjectFilter),
    Resource(String, Option<usize>),
    Snapshot,
    Restore(String),
    Capture(String),
    Operation(u64),
    Events(Option<String>),
    Trace(bool),
    Inventory,
    Owner(String),
    OpenInventory(Option<String>),
    CloseInventory,
    ApproachObject(String),
    Transfer(String, String, Option<String>),
    Quit,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct ObjectFilter {
    pub tile: Option<(u32, u32)>,
    pub radius: u32,
    pub record: Option<(u64, usize)>,
    pub original: Option<String>,
    pub category: Option<String>,
    pub prototype: Option<u32>,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct Failure {
    pub code: &'static str,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

impl Failure {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            details: None,
        }
    }
}

impl From<String> for Failure {
    fn from(message: String) -> Self {
        Self::new("runtime_error", message)
    }
}

impl From<&str> for Failure {
    fn from(message: &str) -> Self {
        message.to_owned().into()
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Output {
    Clear,
    Data(Value),
    Accepted(Value),
}

impl From<String> for Output {
    fn from(line: String) -> Self {
        Self::Data(json!({ "message": line }))
    }
}

pub trait Runtime {
    fn execute(&mut self, request: Request) -> Result<Output, Failure>;
}

struct Command {
    name: &'static str,
    aliases: &'static [&'static str],
    syntax: &'static str,
    description: &'static str,
    mutates: bool,
    handler: fn(&mut dyn Runtime, &[String]) -> Result<Output, Failure>,
}

fn run(
    runtime: &mut dyn Runtime,
    args: &[String],
    parse: impl FnOnce(&[String]) -> Option<Request>,
) -> Result<Output, Failure> {
    runtime
        .execute(parse(args).ok_or_else(|| Failure::new("invalid_arguments", "Invalid arguments"))?)
}

fn tile(args: &[String]) -> Option<(u32, u32)> {
    let [x, y] = args else { return None };
    Some((x.parse().ok()?, y.parse().ok()?))
}

fn optional_tile(args: &[String]) -> Option<Option<(u32, u32)>> {
    if args.is_empty() {
        Some(None)
    } else {
        tile(args).map(Some)
    }
}

macro_rules! command {
    ($name:literal, $syntax:literal, $desc:literal, $parse:expr) => {
        Command {
            name: $name,
            aliases: &[],
            syntax: $syntax,
            mutates: true,
            description: $desc,
            handler: |rt, args| run(rt, args, $parse),
        }
    };
}

macro_rules! simple {
    ($name:literal, $desc:literal, $request:ident) => {
        command!($name, $name, $desc, |a| a
            .is_empty()
            .then_some(Request::$request))
    };
}

macro_rules! query {
    ($name:literal, $syntax:literal, $desc:literal, $parse:expr) => {
        Command {
            mutates: false,
            ..command!($name, $syntax, $desc, $parse)
        }
    };
}

fn object_filter(args: &[String]) -> Option<ObjectFilter> {
    let mut filter = ObjectFilter::default();
    let mut args = args.iter();
    while let Some(key) = args.next() {
        let value = args.next()?;
        match key.as_str() {
            "near" if filter.tile.is_none() => {
                filter.tile = Some((value.parse().ok()?, args.next()?.parse().ok()?));
                filter.radius = args.next()?.parse().ok()?;
                if filter.radius > 8 {
                    return None;
                }
            }
            "id" if filter.record.is_none() => {
                let (sector, record) = value.split_once(':')?;
                filter.record = Some((sector.parse().ok()?, record.parse().ok()?));
            }
            "original" if filter.original.is_none() => {
                let payload = value.strip_prefix("2:")?;
                if payload.len() != 32 || !payload.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                    return None;
                }
                filter.original = Some(value.to_ascii_lowercase());
            }
            "category" if filter.category.is_none() => {
                if value.is_empty()
                    || value.len() > 32
                    || !value
                        .bytes()
                        .all(|byte| byte.is_ascii_lowercase() || byte == b'_')
                {
                    return None;
                }
                filter.category = Some(value.clone());
            }
            "prototype" if filter.prototype.is_none() => {
                filter.prototype = Some(value.parse().ok()?)
            }
            _ => return None,
        }
    }
    Some(filter)
}

const COMMANDS: &[Command] = &[
    command!(
        "approachobject",
        "approachobject ORIGINAL_ID",
        "Start normal approach and use of a resident item or container. Observe navigation and inventory for the outcome.",
        |a| match a {
            [id] => Some(Request::ApproachObject(id.clone())),
            _ => None,
        }
    ),
    query!(
        "inventory",
        "inventory",
        "Inspect current player and open container contents.",
        |a| a.is_empty().then_some(Request::Inventory)
    ),
    query!(
        "owner",
        "owner ORIGINAL_ID",
        "Inspect current and original placement, including unloaded modified items.",
        |a| match a {
            [id] => Some(Request::Owner(id.clone())),
            _ => None,
        }
    ),
    command!(
        "openinventory",
        "openinventory [ORIGINAL_ID]",
        "Open player inventory or a nearby accessible container.",
        |a| (a.len() <= 1).then(|| Request::OpenInventory(a.first().cloned()))
    ),
    simple!(
        "closeinventory",
        "Close the inventory window.",
        CloseInventory
    ),
    command!(
        "transfer",
        "transfer take|put|drop ORIGINAL_ID [CONTAINER_ID]",
        "Transfer one whole object using gameplay access checks.",
        |a| {
            let [op, id, rest @ ..] = a else { return None };
            let valid_id = |s: &str| crate::sector::objects::OriginalId::parse(s).is_some();
            (matches!(
                (op.as_str(), rest),
                ("take" | "drop", []) | ("take" | "put", [_])
            ) && valid_id(id)
                && rest.iter().all(|s| valid_id(s)))
            .then(|| Request::Transfer(op.clone(), id.clone(), rest.first().cloned()))
        }
    ),
    query!(
        "maps",
        "maps [filter]",
        "List original maps and matching world areas.",
        |a| { (a.len() <= 1).then(|| Request::Maps(a.first().cloned())) }
    ),
    command!(
        "coc",
        "coc <map-or-area> [X Y]",
        "Load an original map or authored world area.",
        |a| {
            let (name, rest) = a.split_first()?;
            Some(Request::Load(name.clone(), optional_tile(rest)?))
        }
    ),
    Command {
        aliases: &["position"],
        mutates: false,
        ..simple!(
            "pos",
            "Show current map, coordinates and collision.",
            Position
        )
    },
    command!(
        "teleport",
        "teleport X Y",
        "Move within the current map.",
        |a| tile(a).map(Request::Teleport)
    ),
    simple!(
        "tcl",
        "Toggle collision for keyboard and mouse movement.",
        ToggleCollision
    ),
    command!(
        "inspect",
        "inspect [X Y]",
        "Inspect the clicked object or records at a tile.",
        |a| optional_tile(a).map(Request::Inspect)
    ),
    simple!(
        "unlock",
        "Unlock the selected ordinary door for this session.",
        Unlock
    ),
    command!(
        "settime",
        "settime <0..23>",
        "Set the shared game clock hour.",
        |a| {
            let [hour] = a else { return None };
            let hour = hour.parse::<u8>().ok()?;
            (hour < 24).then_some(Request::SetTime(hour))
        }
    ),
    simple!(
        "discoverall",
        "Discover original destinations while retaining travel eligibility.",
        DiscoverAll
    ),
    simple!(
        "reloadui",
        "Validate and reload the selected HUD and map themes.",
        ReloadUi
    ),
    command!(
        "setuiscale",
        "setuiscale <50..300>",
        "Set UI scale in percent.",
        |a| {
            let [scale] = a else { return None };
            let scale = scale.parse::<u16>().ok()?;
            (50..=300).contains(&scale).then_some(Request::Scale(scale))
        }
    ),
    simple!(
        "showcollision",
        "Toggle the original collision overlay.",
        ShowCollision
    ),
    simple!(
        "showsectors",
        "Toggle visible sector boundaries.",
        ShowSectors
    ),
    simple!("fps", "Toggle frame and streaming statistics.", Fps),
    query!(
        "sectors",
        "sectors",
        "Inspect cached and requested sectors.",
        |a| a.is_empty().then_some(Request::Sectors)
    ),
    query!(
        "objects",
        "objects [near X Y RADIUS] [id SECTOR:RECORD] [original ORIGINAL_ID] [category CATEGORY] [prototype PROTOTYPE]",
        "Inspect up to 128 objects from loaded sectors.",
        |a| object_filter(a).map(Request::Objects)
    ),
    query!(
        "resource",
        "resource PATH [FRAME]",
        "Resolve winning resource and optionally decode an ART frame.",
        |a| match a {
            [path] => Some(Request::Resource(path.clone(), None)),
            [path, frame] => Some(Request::Resource(path.clone(), Some(frame.parse().ok()?))),
            _ => None,
        }
    ),
    query!(
        "snapshot",
        "snapshot",
        "Export the supported reproduction context.",
        |a| a.is_empty().then_some(Request::Snapshot)
    ),
    command!(
        "restore",
        "restore JSON",
        "Restore a snapshot in the current game installation.",
        |a| match a {
            [snapshot] => Some(Request::Restore(snapshot.clone())),
            _ => None,
        }
    ),
    command!(
        "capture",
        "capture FILE",
        "Request a PNG of the presented Vulkan frame.",
        |a| match a {
            [file] => Some(Request::Capture(file.clone())),
            _ => None,
        }
    ),
    query!(
        "operation",
        "operation TOKEN",
        "Inspect screenshot request completion.",
        |a| match a {
            [id] => Some(Request::Operation(id.parse().ok()?)),
            _ => None,
        }
    ),
    query!(
        "events",
        "events [CATEGORY]",
        "Read the bounded diagnostic event history.",
        |a| (a.len() <= 1).then(|| Request::Events(a.first().cloned()))
    ),
    command!(
        "trace",
        "trace <on|off>",
        "Toggle structured runtime event recording.",
        |a| match a {
            [arg] if arg == "on" => Some(Request::Trace(true)),
            [arg] if arg == "off" => Some(Request::Trace(false)),
            _ => None,
        }
    ),
    simple!("quit", "Close the running engine cleanly.", Quit),
    Command {
        name: "help",
        aliases: &["?"],
        syntax: "help [command]",
        description: "List commands or show one command's usage.",
        mutates: false,
        handler: |_, a| match a {
            [] => Ok(Output::Data(
                json!({"commands": COMMANDS.iter().map(Command::metadata).collect::<Vec<_>>() }),
            )),
            [name] => find(name)
                .map(|c| Output::Data(c.metadata()))
                .ok_or_else(|| Failure::new("invalid_command", format!("Unknown command {name}"))),
            _ => Err(Failure::new(
                "invalid_arguments",
                "Expected zero or one argument",
            )),
        },
    },
    Command {
        name: "commands",
        aliases: &[],
        syntax: "commands",
        description: "Discover command metadata and protocol capabilities.",
        mutates: false,
        handler: |_, args| {
            if !args.is_empty() {
                return Err(Failure::new("invalid_arguments", "Expected no arguments"));
            }
            Ok(Output::Data(
                json!({"commands": COMMANDS.iter().map(Command::metadata).collect::<Vec<_>>(),
                "protocol":{"version":1,"transport":"newline-delimited JSON over Unix socket",
                    "request_limit_bytes":16384,"statuses":["completed","accepted","failed"],
                    "capture_completion":"operation TOKEN"}}),
            ))
        },
    },
    Command {
        name: "clear",
        aliases: &[],
        syntax: "clear",
        description: "Clear console output.",
        mutates: true,
        handler: |_, a| {
            if a.is_empty() {
                Ok(Output::Clear)
            } else {
                Err(Failure::new("invalid_arguments", "Expected no arguments"))
            }
        },
    },
];

impl Command {
    fn metadata(&self) -> Value {
        let mut optional = false;
        let arguments: Vec<_> = self
            .syntax
            .split_ascii_whitespace()
            .skip(1)
            .map(|word| {
                let required = !optional && !word.starts_with('[');
                optional = (optional || word.starts_with('[')) && !word.ends_with(']');
                let name = word.trim_matches(['<', '>', '[', ']']);
                let (kind, constraint) = match name {
                    "X" | "Y" | "FRAME" | "PROTOTYPE" | "TOKEN" => ("integer", "unsigned"),
                    "RADIUS" => ("integer", "0..8"),
                    "near" | "id" | "original" | "category" | "prototype" => ("literal", name),
                    "JSON" => ("string", "version-1 snapshot JSON object"),
                    "0..23" | "50..300" => ("integer", name),
                    "on|off" | "take|put|drop" => ("enum", name),
                    "ID" | "SECTOR:RECORD" => ("string", "sector:record"),
                    "ORIGINAL_ID" | "CONTAINER_ID" => ("string", "2: followed by 32 hexadecimal digits in serialized GUID-byte order"),
                    "CATEGORY" if self.name == "objects" => {
                        ("string", "mobile or a canonical category such as npc, container, armor, or wall")
                    }
                    _ => ("string", ""),
                };
                json!({"name":match name {"0..23"=>"hour","50..300"=>"percent","on|off"=>"mode",_=>name},"type":kind,"required":required,"constraint":constraint})
            })
            .collect();
        let fields: &[&str] = match self.name {
            "pos" | "snapshot" => &[
                "map",
                "player",
                "sector",
                "streaming",
                "camera",
                "ui",
                "world",
                "renderer",
            ],
            "inventory" | "openinventory" | "closeinventory" => {
                &["open", "container", "container_name", "player", "contents"]
            }
            "transfer" => &["id", "from", "to"],
            "approachobject" => &["id", "route_steps", "interaction_pending"],
            "owner" => &[
                "id",
                "source",
                "original",
                "current",
                "quantity",
                "icon",
                "slot",
                "footprint",
            ],
            "sectors" => &["bounds", "cached", "required", "speculative", "stats"],
            "objects" | "inspect" => &["map", "objects", "issues", "truncated"],
            "maps" => &["maps", "areas"],
            "resource" => &["path", "winner", "candidates", "decode"],
            "capture" | "operation" => &["token", "path", "viewport"],
            "events" => &["events", "dropped", "enabled"],
            "commands" => &["commands", "protocol"],
            "help" => &["commands"],
            "coc" | "teleport" | "restore" => &["map", "tile", "view_ready"],
            "tcl" => &["collision_bypass"],
            "settime" => &["hour"],
            "setuiscale" => &["scale_percent"],
            "discoverall" => &["discovered", "travel_eligible"],
            "unlock" => &["id", "unlocked", "changed"],
            "trace" | "fps" | "showsectors" | "showcollision" => &["enabled"],
            "reloadui" => &["status", "generation"],
            "clear" => &["clear"],
            _ => &["message"],
        };
        json!({"name":self.name,"aliases":self.aliases,"syntax":self.syntax,"description":self.description,
            "arguments":arguments,"mutates":self.mutates,
            "requires_world":!matches!(self.name,"help"|"commands"|"clear"),
            "result_fields":fields})
    }
}

fn find(name: &str) -> Option<&'static Command> {
    COMMANDS.iter().find(|c| {
        c.name.eq_ignore_ascii_case(name) || c.aliases.iter().any(|a| a.eq_ignore_ascii_case(name))
    })
}

pub fn complete(prefix: &str) -> impl Iterator<Item = &'static str> {
    let prefix = prefix.to_ascii_lowercase();
    COMMANDS
        .iter()
        .map(|c| c.name)
        .filter(move |name| name.starts_with(&prefix))
}

pub fn dispatch_args<'a>(
    name: &'a str,
    args: &[String],
    runtime: &mut dyn Runtime,
) -> (&'a str, Result<Output, Failure>) {
    let Some(command) = find(name) else {
        return (
            name,
            Err(Failure::new(
                "invalid_command",
                format!("Unknown command {name}. Try help."),
            )),
        );
    };
    let result = (command.handler)(runtime, args).map_err(|mut error| {
        if error.code == "invalid_arguments" {
            error.message = format!("Usage: {}", command.syntax);
        }
        error
    });
    (command.name, result)
}

pub fn dispatch(line: &str, runtime: &mut dyn Runtime) -> Result<Output, String> {
    let words = words(line)?;
    let (name, args) = words.split_first().ok_or("Enter a command. Try help.")?;
    dispatch_args(name, args, runtime)
        .1
        .map_err(|error| error.message)
}

fn words(line: &str) -> Result<Vec<String>, String> {
    let mut result = Vec::new();
    let mut input = line.chars().peekable();
    while let Some(first) = input.next() {
        if first.is_whitespace() {
            continue;
        }
        let mut word = String::new();
        if matches!(first, '\'' | '"') {
            loop {
                match input.next() {
                    Some(ch) if ch == first => break,
                    Some('\\') if input.peek().is_some_and(|&ch| ch == first || ch == '\\') => {
                        word.push(input.next().unwrap())
                    }
                    Some(ch) => word.push(ch),
                    None => return Err("Unclosed quote".into()),
                }
            }
            if input.peek().is_some_and(|ch| !ch.is_whitespace()) {
                return Err("Expected a space after quoted argument".into());
            }
        } else {
            word.push(first);
            while let Some(ch) = input.next_if(|ch| !ch.is_whitespace()) {
                word.push(ch);
            }
        }
        result.push(word);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Recording(Option<Request>);
    impl Runtime for Recording {
        fn execute(&mut self, request: Request) -> Result<Output, Failure> {
            self.0 = Some(request);
            Ok("executed".to_owned().into())
        }
    }

    #[test]
    fn registry_discovery_and_machine_dispatch_share_validators() {
        let mut runtime = Recording::default();
        let (name, result) = dispatch_args("commands", &[], &mut runtime);
        assert_eq!(name, "commands");
        let Output::Data(data) = result.unwrap() else {
            panic!("expected metadata")
        };
        assert_eq!(data["protocol"]["version"], 1);
        let pos = data["commands"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["name"] == "pos")
            .unwrap();
        assert_eq!(pos["mutates"], false);
        let settime = data["commands"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["name"] == "settime")
            .unwrap();
        assert_eq!(settime["arguments"][0]["constraint"], "0..23");
        assert_eq!(
            dispatch_args("settime", &["24".into()], &mut runtime)
                .1
                .unwrap_err()
                .code,
            "invalid_arguments"
        );
        let (name, result) = dispatch_args("position", &[], &mut runtime);
        assert_eq!(name, "pos");
        assert!(result.is_ok());
        assert_eq!(runtime.0, Some(Request::Position));
    }

    #[test]
    fn inventory_registry_rejects_incomplete_transfers_before_execution() {
        let mut runtime = Recording::default();
        let id = "2:00000000000000000000000000000001";
        for command in [
            format!("transfer put {id}"),
            format!("transfer drop {id} {id}"),
            "transfer take invalid".into(),
        ] {
            assert!(dispatch(&command, &mut runtime).is_err());
            assert_eq!(runtime.0, None);
        }
        dispatch(&format!("transfer take {id}"), &mut runtime).unwrap();
        assert_eq!(
            runtime.0,
            Some(Request::Transfer("take".into(), id.into(), None))
        );
    }
    #[test]
    fn object_registry_checks_combined_filters() {
        let mut runtime = Recording::default();
        dispatch(
            "objects near 10 20 2 prototype 1021 category wall",
            &mut runtime,
        )
        .unwrap();
        assert_eq!(
            runtime.0,
            Some(Request::Objects(ObjectFilter {
                tile: Some((10, 20)),
                radius: 2,
                original: None,
                prototype: Some(1021),
                category: Some("wall".into()),
                record: None,
            }))
        );
        assert!(dispatch("objects near 10 20 9", &mut runtime).is_err());
        assert!(dispatch("objects id 1:4 id 2:8", &mut runtime).is_err());
    }

    #[test]
    fn resolves_commands_and_validates_arguments_before_dispatch() {
        let mut rt = Recording::default();
        dispatch(r#"CoC "K'na Tha" 10 20"#, &mut rt).unwrap();
        assert_eq!(rt.0, Some(Request::Load("K'na Tha".into(), Some((10, 20)))));
        dispatch("coc K'na", &mut rt).unwrap();
        assert_eq!(rt.0, Some(Request::Load("K'na".into(), None)));
        dispatch("POSITION", &mut rt).unwrap();
        assert_eq!(rt.0, Some(Request::Position));
        assert!(
            dispatch("teleport -1 4", &mut rt)
                .unwrap_err()
                .starts_with("Usage:")
        );
        assert!(dispatch("settime 24", &mut rt).is_err());
        assert!(dispatch("setuiscale 301", &mut rt).is_err());
        assert!(dispatch("coc 'unclosed", &mut rt).is_err());
        assert!(dispatch("unknown", &mut rt).is_err());
        assert_eq!(rt.0, Some(Request::Position));
        assert_eq!(
            complete("SHOW").collect::<Vec<_>>(),
            ["showcollision", "showsectors"]
        );
        assert_eq!(dispatch("clear", &mut rt), Ok(Output::Clear));
    }
}

/// Diagnostic events are retained in memory with a fixed upper bound.
#[derive(Default)]
pub struct EventHistory {
    pub enabled: bool,
    next: u64,
    dropped: u64,
    events: std::collections::VecDeque<Value>,
}

impl EventHistory {
    pub fn record(&mut self, category: &str, id: Option<u64>, data: Value) {
        if !self.enabled {
            return;
        }
        if self.events.len() == 128 {
            self.events.pop_front();
            self.dropped += 1;
        }
        self.next += 1;
        self.events.push_back(
            json!({"sequence":self.next,"category":category,"request_id":id,"data":data}),
        );
    }

    pub fn query(&self, category: Option<&str>) -> Value {
        let events: Vec<_> = self
            .events
            .iter()
            .filter(|event| category.is_none_or(|name| event["category"] == name))
            .collect();
        json!({"enabled":self.enabled,"events":events,"dropped":self.dropped})
    }
}

#[derive(Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub version: u16,
    pub map: i32,
    pub tile: (u32, u32),
    pub facing: u8,
    pub hour: u8,
    pub collision_bypass: bool,
    pub zoom: u8,
    pub scale_percent: u16,
    pub lighting: bool,
    pub shadows: bool,
    pub run: bool,
    pub hud_theme: Option<std::path::PathBuf>,
    pub map_theme: std::path::PathBuf,
    pub console_theme: std::path::PathBuf,
    pub mods: Vec<String>,
    pub selected: Option<(u64, usize)>,
    pub overlays: [bool; 3],
    pub camera: (i32, i32),
    pub viewport: (u32, u32),
    pub loaded_sectors: Vec<u64>,
}

impl Snapshot {
    pub fn parse(text: &str) -> Result<Self, Failure> {
        let snapshot: Self = serde_json::from_str(text)
            .map_err(|err| Failure::new("invalid_snapshot", err.to_string()))?;
        if snapshot.version != 1
            || snapshot.facing > 7
            || snapshot.hour > 23
            || !(1..=3).contains(&snapshot.zoom)
            || !(50..=300).contains(&snapshot.scale_percent)
            || snapshot.loaded_sectors.len() > 12
            || snapshot.mods.len() > 128
        {
            return Err(Failure::new(
                "invalid_snapshot",
                "Unsupported version or out-of-range snapshot fields",
            ));
        }
        Ok(snapshot)
    }
}

#[cfg(test)]
mod diagnostics_tests {
    use super::*;

    #[test]
    fn event_history_is_bounded_and_filterable() {
        let mut events = EventHistory::default();
        events.record("disabled", Some(0), json!({}));
        assert_eq!(events.query(None)["events"], json!([]));
        events.enabled = true;
        for id in 1..=130 {
            events.record(
                if id % 2 == 0 { "sector" } else { "command" },
                Some(id),
                json!({}),
            );
        }
        let data = events.query(Some("sector"));
        assert_eq!(data["dropped"], 2);
        let matching = data["events"].as_array().unwrap();
        assert_eq!(matching.len(), 64);
        assert_eq!(matching[0]["request_id"], 4);
    }

    #[test]
    fn snapshots_round_trip_and_reject_invalid_reproduction_state() {
        let snapshot = Snapshot {
            version: 1,
            map: 5000,
            tile: (28320, 31168),
            facing: 6,
            hour: 23,
            collision_bypass: true,
            zoom: 2,
            scale_percent: 125,
            lighting: true,
            shadows: true,
            run: false,
            hud_theme: None,
            map_theme: "theme.toml".into(),
            console_theme: "console.toml".into(),
            mods: vec![],
            selected: Some((32682017210, 42)),
            overlays: [false, true, false],
            camera: (0, 0),
            viewport: (1280, 720),
            loaded_sectors: vec![32682017210],
        };
        let json = serde_json::to_value(&snapshot).unwrap();
        assert_eq!(Snapshot::parse(&json.to_string()).unwrap(), snapshot);
        for (key, value) in [
            ("version", json!(2)),
            ("zoom", json!(0)),
            ("hour", json!(24)),
            ("scale_percent", json!(301)),
        ] {
            let mut invalid = json.clone();
            invalid[key] = value;
            assert_eq!(
                Snapshot::parse(&invalid.to_string()).unwrap_err().code,
                "invalid_snapshot"
            );
        }
    }
}
