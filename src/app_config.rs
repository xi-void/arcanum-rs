//! Persistent preferences for the playable executable.
use serde::Deserialize;
use std::{
    env, fs,
    io::{self, Write},
    path::{Path, PathBuf},
};
use toml::{Table, Value};

#[derive(Default, Deserialize)]
#[serde(default)]
pub(crate) struct Config {
    pub game_data: Option<PathBuf>,
    pub window: Window,
    pub gameplay: Gameplay,
    pub display: Display,
    pub audio: Audio,
    pub ui: Ui,
    pub camera: Camera,
    pub input: Input,
    pub developer: Developer,
    pub mods: Mods,
}

#[derive(Default, Deserialize)]
#[serde(default)]
pub(crate) struct Gameplay {
    pub difficulty: arcanum_rs::content::Difficulty,
    #[serde(flatten)]
    pub combat_mode: arcanum_rs::content::world::CombatMode,
}

#[derive(Deserialize)]
#[serde(default)]
pub(crate) struct Window {
    pub width: u32,
    pub height: u32,
    pub fullscreen: bool,
}

impl Default for Window {
    fn default() -> Self {
        Self {
            width: 1280,
            height: 720,
            fullscreen: false,
        }
    }
}

#[derive(Deserialize)]
#[serde(default)]
pub(crate) struct Display {
    pub lighting: bool,
    pub shadows: bool,
}

impl Default for Display {
    fn default() -> Self {
        Self {
            lighting: true,
            shadows: true,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, serde::Serialize)]
#[serde(default)]
pub(crate) struct Audio {
    pub effects: u8,
    pub voice: u8,
    pub music: u8,
}

impl Default for Audio {
    fn default() -> Self {
        Self {
            effects: 5,
            voice: 5,
            music: 5,
        }
    }
}

impl Audio {
    pub(crate) fn normalized(self) -> Self {
        Self {
            effects: self.effects.min(10),
            voice: self.voice.min(10),
            music: self.music.min(10),
        }
    }
}

pub(crate) const DEFAULT_SCROLL_SPEED: u16 = 300;

#[derive(Deserialize)]
#[serde(default)]
pub(crate) struct Camera {
    pub zoom: u8,
    pub follow: bool,
    pub edge_scroll: bool,
    pub keyboard_pan: bool,
    pub keyboard_scroll_speed: u16,
    pub edge_scroll_speed: u16,
    #[cfg(feature = "windowed")]
    pub recenter_key: winit::keyboard::KeyCode,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            zoom: 1,
            follow: false,
            edge_scroll: true,
            keyboard_pan: true,
            keyboard_scroll_speed: DEFAULT_SCROLL_SPEED,
            edge_scroll_speed: DEFAULT_SCROLL_SPEED,
            #[cfg(feature = "windowed")]
            recenter_key: winit::keyboard::KeyCode::Home,
        }
    }
}

#[derive(Default, Deserialize)]
#[serde(default)]
pub(crate) struct Ui {
    pub hud: Option<PathBuf>,
    pub map: Option<PathBuf>,
    pub pause: Option<PathBuf>,
    pub scale: Option<u16>,
    // Read the archived M63 HUD preference when a unified scale hasn't been set.
    pub hud_scale: Option<u16>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
pub(crate) struct Input {
    pub run: bool,
}

#[derive(Default, Deserialize)]
#[serde(default)]
pub(crate) struct Developer {
    pub enabled: bool,
    pub rpc: bool,
    pub diagnostics: Option<PathBuf>,
    pub console_ui: Option<PathBuf>,
    pub map_reveal: bool,
    pub discover_all: bool,
}

#[derive(Default, Deserialize)]
#[serde(default)]
pub(crate) struct Mods {
    pub directory: Option<PathBuf>,
    pub enabled: Vec<String>,
}

pub(crate) struct ConfigFile {
    path: PathBuf,
    changes: Table,
}

impl ConfigFile {
    pub(crate) fn open() -> Result<(Self, Config), Box<dyn std::error::Error>> {
        let base = env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| {
                env::var_os("HOME")
                    .map(PathBuf::from)
                    .filter(|p| p.is_absolute())
                    .map(|home| home.join(".config"))
            })
            .ok_or("Set XDG_CONFIG_HOME or HOME to an absolute directory to locate config.toml")?;
        Self::load(base.join("arcanum-rs/config.toml"))
    }

    fn load(path: PathBuf) -> Result<(Self, Config), Box<dyn std::error::Error>> {
        let config = toml::from_str(&read(&path)?)
            .map_err(|e| format!("Configuration {}: {e}", path.display()))?;
        Ok((
            Self {
                path,
                changes: Table::new(),
            },
            config,
        ))
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn resolve(&self, path: &Path) -> PathBuf {
        self.path.parent().unwrap().join(path)
    }

    pub(crate) fn set(&mut self, section: &str, key: &str, value: impl Into<Value>) {
        section_mut(&mut self.changes, section)
            .expect("pending settings sections are tables")
            .insert(key.into(), value.into());
    }

    pub(crate) fn flush(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.save()?;
        self.changes.clear();
        Ok(())
    }

    pub(crate) fn save(&self) -> Result<(), Box<dyn std::error::Error>> {
        if self.changes.is_empty() {
            return Ok(());
        }
        let parent = self.path.parent().unwrap();
        fs::create_dir_all(parent)?;
        // Lock a stable sibling across the read, merge and atomic replacement.
        let lock = fs::File::options()
            .create(true)
            .truncate(false)
            .write(true)
            .open(parent.join("config.lock"))?;
        lock.lock()?;
        let mut document: Table = toml::from_str(&read(&self.path)?)?;
        for (section, values) in &self.changes {
            section_mut(&mut document, section)?.extend(values.as_table().unwrap().clone());
        }
        let source = toml::to_string_pretty(&document)?;
        let temporary = parent.join(format!(".config-{}.tmp", std::process::id()));
        let mut options = fs::File::options();
        options.write(true).create_new(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        let mut file = options.open(&temporary)?;
        let result = (|| {
            file.write_all(source.as_bytes())?;
            file.sync_all()?;
            drop(file);
            fs::rename(&temporary, &self.path)?;
            #[cfg(unix)]
            fs::File::open(parent)?.sync_all()?;
            Ok::<_, io::Error>(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(temporary);
        }
        Ok(result?)
    }
}

fn section_mut<'a>(document: &'a mut Table, name: &str) -> Result<&'a mut Table, String> {
    document
        .entry(name)
        .or_insert_with(|| Value::Table(Table::new()))
        .as_table_mut()
        .ok_or_else(|| format!("[{name}] must be a TOML table"))
}

fn read(path: &Path) -> io::Result<String> {
    match fs::read_to_string(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(String::new()),
        result => result.map_err(|e| io::Error::new(e.kind(), format!("{}: {e}", path.display()))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saving_only_changed_preferences_keeps_other_values_and_later_edits() {
        let dir = env::temp_dir().join(format!("arcanum-config-test-{}", std::process::id()));
        fs::create_dir(&dir).unwrap();
        let path = dir.join("config.toml");
        let (mut first, defaults) = ConfigFile::load(path.clone()).unwrap();
        first.save().unwrap();
        assert!(!path.exists());
        assert!(!defaults.developer.enabled && !defaults.developer.rpc);
        fs::write(&path, "game_data = 'game'\n[ui]\nhud_scale = 125\n").unwrap();
        let (mut second, config) = ConfigFile::load(path.clone()).unwrap();
        assert_eq!(config.ui.hud_scale, Some(125));
        assert_eq!(second.resolve(Path::new("game")), dir.join("game"));
        first.set("camera", "zoom", 2_i64);
        first.save().unwrap();
        second.set("ui", "scale", 150_i64);
        second.save().unwrap();
        let (_, merged) = ConfigFile::load(path.clone()).unwrap();
        assert_eq!(merged.game_data, Some("game".into()));
        assert_eq!(merged.ui.scale, Some(150));
        assert_eq!(merged.ui.hud_scale, Some(125));
        assert_eq!(merged.camera.zoom, 2);
        second.set("audio", "effects", 0_i64);
        second.set("audio", "voice", 7_i64);
        second.flush().unwrap();
        let (_, audio) = ConfigFile::load(path.clone()).unwrap();
        assert_eq!(
            audio.audio,
            Audio {
                effects: 0,
                voice: 7,
                music: 5
            }
        );
        assert!(second.changes.is_empty());
        fs::write(&path, "invalid = [").unwrap();
        second.set("audio", "music", 8_i64);
        assert!(second.flush().is_err());
        assert!(!second.changes.is_empty());
        assert_eq!(fs::read_to_string(path).unwrap(), "invalid = [");
        fs::remove_dir_all(dir).unwrap();
    }
}
