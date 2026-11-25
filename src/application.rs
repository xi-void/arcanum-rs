//! Persistent preferences for the playable executable.
use serde::{Deserialize, Serialize};
use std::{
    error::Error,
    ffi::OsString,
    fs::{self, DirBuilder, OpenOptions},
    io::{self, Write},
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Default, Deserialize)]
#[serde(default)]
pub(crate) struct Preferences {
    pub game_data: Option<PathBuf>,
    pub mods_directory: Option<PathBuf>,
    pub mods: Vec<String>,
    pub display: Display,
    pub camera: Camera,
    pub ui: Ui,
    pub developer: Developer,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(default)]
pub(crate) struct Display {
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub fullscreen: bool,
    pub lighting: bool,
    pub shadows: bool,
    pub run: bool,
}
impl Default for Display {
    fn default() -> Self {
        Self {
            width: None,
            height: None,
            fullscreen: false,
            lighting: true,
            shadows: true,
            run: false,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub(crate) struct Camera {
    pub zoom: u8,
    pub follow: bool,
    pub edge_scroll: bool,
    pub speed: u16,
    pub recenter: String,
    pub follow_toggle: String,
}
impl Default for Camera {
    fn default() -> Self {
        Self {
            zoom: 1,
            follow: false,
            edge_scroll: true,
            speed: 1200,
            recenter: "Home".into(),
            follow_toggle: "F7".into(),
        }
    }
}

#[derive(Default, Deserialize)]
#[serde(default)]
pub(crate) struct Ui {
    pub hud: Option<PathBuf>,
    pub inventory: Option<PathBuf>,
    pub map: Option<PathBuf>,
    pub console: Option<PathBuf>,
    pub hud_scale: Option<u16>,
    pub inventory_scale: Option<u16>,
    pub map_scale: Option<u16>,
    pub console_scale: Option<u16>,
    pub menu_scale: Option<u16>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
pub(crate) struct Developer {
    pub enabled: bool,
    pub rpc: bool,
    pub diagnostics: Option<PathBuf>,
    pub map: Option<String>,
    pub center: Option<[u32; 2]>,
    pub map_reveal: bool,
    pub discover_all: bool,
}

pub(crate) struct Application {
    pub source: PathBuf,
    pub prefs: Preferences,
    socket_directory: Option<PathBuf>,
}

fn absolute_env(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
}

impl Application {
    pub fn load() -> Result<Self, Box<dyn Error>> {
        let source = if let Some(path) = absolute_env("XDG_CONFIG_HOME") {
            path
        } else {
            absolute_env("HOME")
                .ok_or("Set XDG_CONFIG_HOME or HOME to an absolute path")?
                .join(".config")
        }
        .join("arcanum-rs/config.toml");
        let prefs = match fs::read_to_string(&source) {
            Ok(text) => Self::parse(&text).map_err(|e| format!("{}: {e}", source.display()))?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => Preferences::default(),
            Err(e) => return Err(format!("{}: {e}", source.display()).into()),
        };
        Ok(Self {
            source,
            prefs,
            socket_directory: None,
        })
    }

    fn parse(text: &str) -> Result<Preferences, Box<dyn Error>> {
        let prefs: Preferences = toml::from_str(text)?;
        let d = prefs.display;
        if d.width.is_some() != d.height.is_some()
            || d.width
                .into_iter()
                .chain(d.height)
                .any(|n| !(200..=4096).contains(&n))
        {
            return Err(
                "display.width and display.height must both be set, each from 200 to 4096".into(),
            );
        }
        let c = &prefs.camera;
        let valid_key = |key: &str| {
            key.eq_ignore_ascii_case("Home")
                || key
                    .strip_prefix('F')
                    .and_then(|n| n.parse::<u8>().ok())
                    .is_some_and(|n| (7..=12).contains(&n))
        };
        if !(1..=3).contains(&c.zoom)
            || !(40..=2400).contains(&c.speed)
            || !valid_key(&c.recenter)
            || !valid_key(&c.follow_toggle)
            || c.recenter.eq_ignore_ascii_case(&c.follow_toggle)
        {
            return Err(
                "camera requires zoom 1–3, speed 40–2400 and distinct Home or F7–F12 bindings"
                    .into(),
            );
        }
        let ui = &prefs.ui;
        if [
            ui.hud_scale,
            ui.inventory_scale,
            ui.map_scale,
            ui.console_scale,
            ui.menu_scale,
        ]
        .into_iter()
        .flatten()
        .any(|n| !(50..=300).contains(&n))
        {
            return Err("interface scale must be from 50 to 300 percent".into());
        }
        let dev = &prefs.developer;
        if !dev.enabled
            && (dev.rpc
                || dev.diagnostics.is_some()
                || dev.map.is_some()
                || dev.center.is_some()
                || dev.map_reveal
                || dev.discover_all)
        {
            return Err("configured developer options require developer.enabled = true".into());
        }
        if prefs.mods.is_empty() != prefs.mods_directory.is_none() {
            return Err("set both mods_directory and a nonempty mods list".into());
        }
        Ok(prefs)
    }

    pub fn resolve(&self, path: &Path) -> PathBuf {
        self.source.parent().unwrap().join(path)
    }

    pub fn game_data(&self) -> Result<PathBuf, String> {
        let data = self.prefs.game_data.as_deref().map(|p| self.resolve(p)).ok_or_else(|| {
            format!("Set game_data in {} to the directory containing your original game installation. Example: game_data = \"/path/to/installed/game\"", self.source.display())
        })?;
        if !data.is_dir() {
            return Err(format!(
                "{}: game_data directory {} does not exist",
                self.source.display(),
                data.display()
            ));
        }
        Ok(data)
    }

    pub fn launch_overrides(
        &mut self,
        explicit: &[OsString],
    ) -> Result<Vec<OsString>, Box<dyn Error>> {
        let mut remaining = Vec::new();
        let mut args = explicit.iter();
        while let Some(flag) = args.next() {
            let number =
                |arg: Option<&OsString>, range: std::ops::RangeInclusive<u32>, name: &str| {
                    arg.and_then(|v| v.to_str())
                        .and_then(|v| v.parse::<u32>().ok())
                        .filter(|n| range.contains(n))
                        .ok_or_else(|| format!("Invalid or missing value for {name}"))
                };
            match flag.to_str() {
                Some("--window-size") => {
                    self.prefs.display.width =
                        Some(number(args.next(), 200..=4096, "--window-size width")?);
                    self.prefs.display.height =
                        Some(number(args.next(), 200..=4096, "--window-size height")?);
                }
                Some("--fullscreen") => self.prefs.display.fullscreen = true,
                Some("--windowed") => self.prefs.display.fullscreen = false,
                Some("--zoom") => {
                    self.prefs.camera.zoom = number(args.next(), 1..=3, "--zoom")? as u8
                }
                Some("--follow") => self.prefs.camera.follow = true,
                Some("--no-follow") => self.prefs.camera.follow = false,
                Some("--edge-scroll") => self.prefs.camera.edge_scroll = true,
                Some("--no-edge-scroll") => self.prefs.camera.edge_scroll = false,
                Some("--pan-speed") => {
                    self.prefs.camera.speed = number(args.next(), 40..=2400, "--pan-speed")? as u16
                }
                Some("--mods") => {
                    remaining.push(flag.clone());
                    remaining.extend(args.cloned());
                    break;
                }
                _ => remaining.push(flag.clone()),
            }
        }
        Ok(remaining)
    }

    pub fn flags(&mut self, explicit: &[OsString]) -> Result<Vec<OsString>, Box<dyn Error>> {
        let mut flags = Vec::new();
        let mut add = |flag: &str, value: Option<OsString>| {
            if !explicit
                .iter()
                .take_while(|arg| *arg != "--mods")
                .any(|arg| arg == flag)
            {
                flags.push(flag.into());
                flags.extend(value);
            }
        };
        let ui = &self.prefs.ui;
        for (flag, path) in [
            ("--ui", &ui.hud),
            ("--map-ui", &ui.map),
            ("--inventory-ui", &ui.inventory),
            ("--console-ui", &ui.console),
        ] {
            if let Some(path) = path {
                let selected = if flag == "--ui"
                    && (path == Path::new("off") || path == Path::new("classic"))
                {
                    path.clone()
                } else {
                    self.resolve(path)
                };
                add(flag, Some(selected.into_os_string()));
            }
        }
        let dev = &self.prefs.developer;
        for (flag, enabled) in [
            ("--debug", dev.enabled),
            ("--discover-all", dev.discover_all),
            ("--map-reveal", dev.map_reveal),
        ] {
            if enabled {
                add(flag, None);
            }
        }
        if let Some(path) = &dev.diagnostics {
            add("--diagnostics", Some(self.resolve(path).into_os_string()));
        }
        if let Some(map) = &dev.map {
            add("--map", Some(map.into()));
        }
        if dev.rpc
            && !explicit
                .iter()
                .take_while(|arg| *arg != "--mods")
                .any(|arg| arg == "--rpc")
        {
            let preferred = absolute_env("XDG_RUNTIME_DIR").unwrap_or_else(std::env::temp_dir);
            let dir = unique_directory(&preferred, "arcanum-rs-rpc")?;
            let socket = dir.join("engine.sock");
            self.socket_directory = Some(dir);
            add("--rpc", Some(socket.into_os_string()));
        }
        if let Some([x, y]) = self.prefs.developer.center
            && !explicit
                .iter()
                .take_while(|arg| *arg != "--mods")
                .any(|arg| arg == "--center" || arg == "--map")
        {
            flags.extend([
                "--center".into(),
                x.to_string().into(),
                y.to_string().into(),
            ]);
        }
        flags.extend_from_slice(explicit);
        if let Some(path) = &self.prefs.mods_directory
            && !explicit.iter().any(|arg| arg == "--mods")
        {
            flags.extend(["--mods".into(), self.resolve(path).into_os_string()]);
            flags.extend(self.prefs.mods.iter().map(OsString::from));
        }
        Ok(flags)
    }

    pub fn persist_values<'a>(
        &self,
        section: &str,
        values: impl IntoIterator<Item = (&'a str, toml::Value)>,
    ) -> Result<(), Box<dyn Error>> {
        let mut table: toml::Table = match fs::read_to_string(&self.source) {
            Ok(text) => {
                Self::parse(&text)?;
                toml::from_str(&text)?
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => toml::Table::new(),
            Err(e) => return Err(e.into()),
        };
        table
            .entry(section)
            .or_insert_with(|| toml::Value::Table(toml::Table::new()))
            .as_table_mut()
            .ok_or("Configuration section must be a table")?
            .extend(values.into_iter().map(|(key, value)| (key.into(), value)));
        let text = toml::to_string_pretty(&table)?;
        Self::parse(&text)?;
        let parent = self.source.parent().unwrap();
        fs::create_dir_all(parent)?;
        let dir = unique_directory(parent, ".config-write")?;
        let path = dir.join("config.toml");
        let write = || -> io::Result<()> {
            let mut file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .mode(0o600)
                .open(&path)?;
            file.write_all(text.as_bytes())?;
            file.sync_all()?;
            fs::rename(&path, &self.source)
        };
        let result = write();
        let _ = fs::remove_file(path);
        let _ = fs::remove_dir(dir);
        result?;
        Ok(())
    }
}
impl Drop for Application {
    fn drop(&mut self) {
        if let Some(dir) = &self.socket_directory {
            let _ = fs::remove_dir(dir);
        }
    }
}
fn unique_directory(parent: &Path, prefix: &str) -> io::Result<PathBuf> {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let path = parent.join(format!("{prefix}-{}-{nonce:x}", std::process::id()));
    DirBuilder::new().mode(0o700).create(&path)?;
    Ok(path)
}

pub(crate) fn ui_resource(relative: &str) -> PathBuf {
    let executable = std::env::current_exe().unwrap_or_default();
    for parent in executable.ancestors().skip(1) {
        for base in [parent.join("ui"), parent.join("share/arcanum-rs/ui")] {
            let candidate = base.join(relative);
            if candidate.exists() {
                return candidate;
            }
        }
    }
    executable.with_file_name("ui").join(relative)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_invalid_values_are_explicit() {
        let prefs = Application::parse("").unwrap();
        assert!(!prefs.camera.follow);
        assert!(prefs.camera.edge_scroll);
        assert!(!prefs.developer.enabled && !prefs.developer.rpc);
        for invalid in [
            "[camera]\nzoom = 4",
            "[camera]\nfollow_toggle = 'Home'",
            "[developer]\nrpc = true",
            "[display]\nwidth = 800",
            "[ui]\nmap_scale = 301",
            "[ui",
        ] {
            assert!(Application::parse(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn flag_overrides_and_atomic_writes_preserve_unrelated_settings() {
        let dir = unique_directory(&std::env::temp_dir(), "arcanum-config-test").unwrap();
        let source = dir.join("config.toml");
        let input = "game_data = 'game'\n[ui]\nhud_scale = 125\nmap_scale = 200\nhud = 'classic'\n[developer]\nenabled = true\nmap = '5011'\ncenter = [80, 103]\n[extension]\nkey = 'retained'\n";
        fs::write(&source, input).unwrap();
        let mut app = Application {
            source: source.clone(),
            prefs: Application::parse(input).unwrap(),
            socket_directory: None,
        };
        let flags = app
            .flags(&["--ui".into(), "off".into(), "--map".into(), "5031".into()])
            .unwrap();
        assert_eq!(
            flags,
            ["--debug", "--ui", "off", "--map", "5031"].map(OsString::from)
        );
        assert_eq!(fs::read_to_string(&source).unwrap(), input);
        app.persist_values(
            "ui",
            [
                ("hud_scale", 175_i64.into()),
                ("inventory_scale", 175_i64.into()),
            ],
        )
        .unwrap();
        let saved = fs::read_to_string(&source).unwrap();
        let prefs = Application::parse(&saved).unwrap();
        assert_eq!(prefs.ui.map_scale, Some(200));
        assert_eq!(prefs.ui.inventory_scale, Some(175));
        assert_eq!(prefs.ui.hud_scale, Some(175));
        assert_eq!(
            toml::from_str::<toml::Table>(&saved).unwrap()["extension"]["key"].as_str(),
            Some("retained")
        );
        assert!(
            app.persist_values("camera", [("zoom", 4_i64.into())])
                .is_err()
        );
        assert_eq!(fs::read_to_string(&source).unwrap(), saved);
        fs::remove_dir_all(dir).unwrap();
    }
}
