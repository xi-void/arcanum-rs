//! Typed developer commands shared by interactive and non-visual callers.

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
}

#[derive(Debug, PartialEq, Eq)]
pub enum Output {
    Lines(Vec<String>),
    Clear,
}

impl From<String> for Output {
    fn from(line: String) -> Self {
        Self::Lines(vec![line])
    }
}

pub trait Runtime {
    fn execute(&mut self, request: Request) -> Result<Output, String>;
}

struct Command {
    name: &'static str,
    aliases: &'static [&'static str],
    syntax: &'static str,
    description: &'static str,
    handler: fn(&mut dyn Runtime, &[String]) -> Result<Output, String>,
}

fn run(
    runtime: &mut dyn Runtime,
    args: &[String],
    parse: impl FnOnce(&[String]) -> Option<Request>,
) -> Result<Output, String> {
    runtime.execute(parse(args).ok_or("Invalid arguments")?)
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

const COMMANDS: &[Command] = &[
    command!(
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
    Command {
        name: "help",
        aliases: &["?"],
        syntax: "help [command]",
        description: "List commands or show one command's usage.",
        handler: |_, a| match a {
            [] => Ok(Output::Lines(
                COMMANDS
                    .iter()
                    .map(|c| format!("{}  -  {}", c.syntax, c.description))
                    .collect(),
            )),
            [name] => find(name)
                .map(|c| format!("{}  -  {}", c.syntax, c.description).into())
                .ok_or_else(|| format!("Unknown command {name}")),
            _ => Err("Invalid arguments".into()),
        },
    },
    Command {
        name: "clear",
        aliases: &[],
        syntax: "clear",
        description: "Clear console output.",
        handler: |_, a| {
            if a.is_empty() {
                Ok(Output::Clear)
            } else {
                Err("Invalid arguments".into())
            }
        },
    },
];

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

pub fn dispatch(line: &str, runtime: &mut dyn Runtime) -> Result<Output, String> {
    let words = words(line)?;
    let (name, args) = words.split_first().ok_or("Enter a command. Try help.")?;
    let command = find(name).ok_or_else(|| format!("Unknown command {name}. Try help."))?;
    (command.handler)(runtime, args).map_err(|e| {
        if e == "Invalid arguments" {
            format!("Usage: {}", command.syntax)
        } else {
            e
        }
    })
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
        fn execute(&mut self, request: Request) -> Result<Output, String> {
            self.0 = Some(request);
            Ok("executed".to_owned().into())
        }
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
