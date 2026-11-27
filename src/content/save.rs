//! Versioned native saves. Display metadata can be listed without loading world state.
use super::world::{Objects, SavedDoors};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

const MAGIC: [u8; 8] = *b"ARCRSAVE";
const VERSION: u32 = 1;
const MAX_METADATA: usize = 16 * 1024;
const MAX_STATE: usize = 64 * 1024 * 1024;
pub const PREVIEW_SIZE: (u32, u32) = (468, 300);
const PREVIEW_BYTES: usize = (PREVIEW_SIZE.0 * PREVIEW_SIZE.1 * 3) as usize;
const QUICK: &str = "auto";
static SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Serialize, Deserialize)]
pub struct Metadata {
    pub name: String,
    pub location: String,
    pub hours: u64,
    pub saved_at_ms: u64,
}

#[derive(Serialize, Deserialize)]
pub struct State {
    pub map: String,
    pub tile: (u32, u32),
    #[serde(default)]
    pub camera_offset: Option<(i32, i32)>,
    pub facing: u8,
    pub clock: (u64, u64),
    pub objects: Objects,
    #[serde(default)]
    pub scripts: super::script::State,
    pub doors: BTreeMap<String, SavedDoors>,
    pub discovered: BTreeSet<i32>,
    pub explored: BTreeMap<u32, BTreeSet<u32>>,
}

#[derive(Clone)]
pub struct Slot {
    pub id: String,
    pub metadata: Result<Metadata, String>,
}

pub struct Store(PathBuf);

impl Store {
    pub fn user() -> Result<Self, String> {
        let base = env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| {
                env::var_os("HOME")
                    .map(PathBuf::from)
                    .filter(|p| p.is_absolute())
                    .map(|p| p.join(".local/share"))
            })
            .ok_or("Set XDG_DATA_HOME or HOME to locate the save directory")?;
        Ok(Self(base.join("arcanum-rs/saves")))
    }

    pub fn directory(&self) -> &Path {
        &self.0
    }

    fn path(&self, id: &str) -> Result<PathBuf, String> {
        if !valid_id(id) {
            return Err("Invalid save slot ID".into());
        }
        Ok(self.0.join(format!("{id}.ars")))
    }

    pub fn slots(&self) -> Result<Vec<Slot>, String> {
        let entries = match fs::read_dir(&self.0) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.to_string()),
        };
        let mut slots = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|err| err.to_string())?;
            if !entry.file_type().map_err(|err| err.to_string())?.is_file() {
                continue;
            }
            let filename = entry.file_name();
            let Some(id) = filename
                .to_str()
                .and_then(|name| name.strip_suffix(".ars"))
                .filter(|id| valid_id(id))
            else {
                continue;
            };
            let metadata = self.reader(id).and_then(|mut reader| reader.metadata());
            let modified = entry
                .metadata()
                .ok()
                .and_then(|meta| meta.modified().ok())
                .unwrap_or(UNIX_EPOCH);
            slots.push((
                modified,
                Slot {
                    id: id.into(),
                    metadata,
                },
            ));
        }
        // CE looks up the newest save by file date, including named slots.
        slots.sort_unstable_by(|a, b| (b.0, &b.1.id).cmp(&(a.0, &a.1.id)));
        Ok(slots.into_iter().map(|(_, slot)| slot).collect())
    }

    pub fn recent(&self) -> Result<String, String> {
        self.slots()?
            .into_iter()
            .next()
            .map(|slot| slot.id)
            .ok_or("No saved games".into())
    }

    fn reader(&self, id: &str) -> Result<Reader, String> {
        let mut file =
            File::open(self.path(id)?).map_err(|error| format!("Save slot {id}: {error}"))?;
        let mut header = [0; 28];
        file.read_exact(&mut header)
            .map_err(|error| format!("Invalid save header: {error}"))?;
        let word = |at| u32::from_le_bytes(header[at..at + 4].try_into().unwrap());
        if header[..8] != MAGIC {
            return Err("Invalid Arcanum-RS save signature".into());
        }
        if word(8) != VERSION {
            return Err(format!("Unsupported Arcanum-RS save version {}", word(8)));
        }
        let (metadata, preview, state) = (word(12) as usize, word(16) as usize, word(20) as usize);
        if metadata > MAX_METADATA
            || !matches!(preview, 0 | PREVIEW_BYTES)
            || state > MAX_STATE
            || file.metadata().map_err(|err| err.to_string())?.len()
                != (header.len() + metadata + preview + state) as u64
        {
            return Err("Invalid save section sizes".into());
        }
        Ok(Reader {
            file,
            lengths: [metadata, preview, state],
            checksum: word(24),
        })
    }

    pub fn load(&self, id: &str) -> Result<State, String> {
        let mut reader = self.reader(id)?;
        let [metadata, preview, state] = reader.lengths;
        let bytes = reader.read(metadata + preview + state)?;
        if checksum(&[&bytes]) != reader.checksum {
            return Err("Save checksum mismatch".into());
        }
        decode_metadata(&bytes[..metadata])?;
        let state: State = serde_json::from_slice(&bytes[metadata + preview..])
            .map_err(|error| format!("Invalid saved gameplay state: {error}"))?;
        if state.facing >= 8 || !super::travel::Clock::valid_phase(state.clock.1) {
            return Err("Invalid saved facing or clock phase".into());
        }
        state.scripts.validate()?;
        Ok(state)
    }

    pub fn preview(&self, id: &str) -> Result<Vec<u8>, String> {
        let mut reader = self.reader(id)?;
        reader.metadata()?;
        reader.read(reader.lengths[1])
    }

    pub fn remove(&self, id: &str) -> Result<(), String> {
        fs::remove_file(self.path(id)?).map_err(|error| error.to_string())
    }

    pub fn quicksave(
        &self,
        metadata: Metadata,
        state: &State,
        preview: &[u8],
    ) -> Result<(), String> {
        self.write(Some(QUICK), metadata, state, preview)
    }

    pub fn write(
        &self,
        replace: Option<&str>,
        mut metadata: Metadata,
        state: &State,
        preview: &[u8],
    ) -> Result<(), String> {
        metadata.name = metadata.name.trim().into();
        metadata.saved_at_ms = now().as_millis() as u64;
        let metadata = serde_json::to_vec(&metadata).map_err(|error| error.to_string())?;
        decode_metadata(&metadata)?;
        let state = serde_json::to_vec(state).map_err(|error| error.to_string())?;
        if metadata.len() > MAX_METADATA
            || !matches!(preview.len(), 0 | PREVIEW_BYTES)
            || state.len() > MAX_STATE
        {
            return Err("Save exceeds the native format bounds".into());
        }

        fs::create_dir_all(&self.0).map_err(|error| error.to_string())?;
        let unique = format!(
            "{:032x}{:08x}{:08x}",
            now().as_nanos(),
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        );
        let path = self.path(replace.unwrap_or(&unique))?;
        let temporary = self.0.join(format!(".{unique}.tmp"));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        let mut file = options
            .open(&temporary)
            .map_err(|error| error.to_string())?;
        let result = (|| -> io::Result<()> {
            file.write_all(&MAGIC)?;
            for word in [
                VERSION,
                metadata.len() as u32,
                preview.len() as u32,
                state.len() as u32,
                checksum(&[&metadata, preview, &state]),
            ] {
                file.write_all(&word.to_le_bytes())?;
            }
            for bytes in [metadata.as_slice(), preview, &state] {
                file.write_all(bytes)?;
            }
            file.sync_all()?;
            drop(file);
            if replace.is_some() {
                fs::rename(&temporary, &path)?;
            } else {
                fs::hard_link(&temporary, &path)?;
                fs::remove_file(&temporary)?;
            }
            #[cfg(unix)]
            File::open(&self.0)?.sync_all()?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(temporary);
        }
        result.map_err(|error| format!("Save {}: {error}", path.display()))
    }
}

fn valid_id(id: &str) -> bool {
    id == QUICK
        || (id.len() == 48
            && id
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()))
}

fn now() -> std::time::Duration {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
}

fn checksum(parts: &[&[u8]]) -> u32 {
    let mut crc = flate2::Crc::new();
    for bytes in parts {
        crc.update(bytes);
    }
    crc.sum()
}

fn decode_metadata(bytes: &[u8]) -> Result<Metadata, String> {
    let metadata: Metadata =
        serde_json::from_slice(bytes).map_err(|error| format!("Invalid save metadata: {error}"))?;
    if metadata.name.trim().is_empty()
        || metadata.name.chars().count() > 80
        || metadata.name.chars().any(char::is_control)
    {
        return Err("Enter a save name between 1 and 80 characters".into());
    }
    Ok(metadata)
}

struct Reader {
    file: File,
    lengths: [usize; 3],
    checksum: u32,
}

impl Reader {
    fn read(&mut self, length: usize) -> Result<Vec<u8>, String> {
        let mut bytes = vec![0; length];
        self.file
            .read_exact(&mut bytes)
            .map_err(|error| error.to_string())?;
        Ok(bytes)
    }

    fn metadata(&mut self) -> Result<Metadata, String> {
        decode_metadata(&self.read(self.lengths[0])?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::character::Character;

    #[test]
    fn slots_keep_complete_versions_and_reject_damaged_files() {
        let store = Store(env::temp_dir().join(format!(
            "arcanum-save-test-{}-{}",
            std::process::id(),
            now().as_nanos()
        )));
        use crate::content::script::{Attachment, Event, Slot};
        let attachment = Attachment {
            owner: "fixture".into(),
            event: Event::Dialogue,
            script: 1,
            initial: Default::default(),
        };
        let mut scripts = crate::content::script::State::default();
        scripts.initialize_random();
        scripts.write(&attachment, Slot::LocalFlag(7), 1).unwrap();
        scripts.write(&attachment, Slot::GlobalFlag(4), 1).unwrap();
        scripts
            .write(&attachment, Slot::GlobalVariable(5), -19)
            .unwrap();
        scripts
            .quests
            .set_player(1009, crate::content::quest::Status::Completed, 1683, || {
                crate::content::quest::Origin {
                    resource: "dlg/fixture.dlg".into(),
                    mount: "fixture".into(),
                    line: 10,
                    object: attachment.owner.clone(),
                }
            })
            .unwrap();
        let state = State {
            map: "fixture".into(),
            tile: (32, 32),
            camera_offset: Some((80, 20)),
            facing: 4,
            clock: (29, 3_000),
            scripts,
            objects: serde_json::from_value(serde_json::json!({
                "character":Character::default(),"next_generated":0,"items":[],"unlocked":{},
            }))
            .unwrap(),
            doors: Default::default(),
            discovered: BTreeSet::from([3, 7]),
            explored: BTreeMap::from([(2, BTreeSet::from([15]))]),
        };
        let describe = |name: &str| Metadata {
            name: name.into(),
            location: "Fixture".into(),
            hours: 29,
            saved_at_ms: 0,
        };
        let preview = vec![42; PREVIEW_BYTES];
        assert!(store.recent().is_err());
        store
            .write(None, describe("First"), &state, &preview)
            .unwrap();
        let id = store.recent().unwrap();
        let loaded = store.load(&id).unwrap();
        assert_eq!(loaded.scripts, state.scripts);
        let mut legacy = serde_json::to_value(&state).unwrap();
        let scripts = legacy["scripts"].as_object_mut().unwrap();
        assert!(scripts.remove("random").is_some());
        scripts.remove("quests");
        let prior: State = serde_json::from_value(legacy.clone()).unwrap();
        assert_eq!(prior.scripts.quests, Default::default());
        assert_eq!(
            prior.scripts.read(&attachment, Slot::GlobalFlag(4)),
            Some(1)
        );
        legacy.as_object_mut().unwrap().remove("scripts");
        let legacy: State = serde_json::from_value(legacy).unwrap();
        assert_eq!(legacy.scripts, crate::content::script::State::default());
        assert_eq!(loaded.discovered, state.discovered);
        assert_eq!(loaded.explored, state.explored);
        assert_eq!(loaded.clock, state.clock);
        assert_eq!(store.preview(&id).unwrap(), preview);

        let updated = State {
            tile: (33, 34),
            ..state
        };
        assert!(
            store
                .write(Some(&id), describe(""), &updated, &preview)
                .is_err()
        );
        assert_eq!(store.load(&id).unwrap().tile, (32, 32));
        store
            .write(Some(&id), describe("Overwritten"), &updated, &preview)
            .unwrap();
        assert_eq!(store.slots().unwrap().len(), 1);
        assert_eq!(store.load(&id).unwrap().tile, (33, 34));
        store
            .quicksave(describe("Auto-Save"), &updated, &preview)
            .unwrap();
        assert_eq!(store.slots().unwrap().len(), 2);
        assert_eq!(store.load(QUICK).unwrap().tile, (33, 34));

        let path = store.path(&id).unwrap();
        let good = fs::read(&path).unwrap();
        let mut damaged = good.clone();
        let last = damaged.last_mut().unwrap();
        *last ^= 1;
        fs::write(&path, damaged).unwrap();
        assert_eq!(store.load(&id).err().unwrap(), "Save checksum mismatch");
        let mut unsupported = good.clone();
        unsupported[8..12].copy_from_slice(&2u32.to_le_bytes());
        fs::write(&path, unsupported).unwrap();
        assert!(store.load(&id).err().unwrap().contains("version 2"));
        fs::write(&path, &good[..good.len() - 1]).unwrap();
        assert!(store.load(&id).is_err());
        store.remove(&id).unwrap();
        assert_eq!(store.slots().unwrap().len(), 1);
        fs::remove_dir_all(store.directory()).unwrap();
    }
}
