//! Original quest records for the local PC and the module's global quest state.
use super::{LoadedMes, load_mes};
use crate::vfs::Vfs;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub(super) fn valid_id(id: i32) -> bool {
    (1000..2000).contains(&id)
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[repr(i32)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    #[default]
    Unknown,
    Mentioned,
    Accepted,
    Achieved,
    Completed,
    OtherCompleted,
    Botched,
}

impl Status {
    pub(super) fn decode(value: i32) -> Option<Self> {
        use Status::*;
        Some(match value {
            0 => Unknown,
            1 => Mentioned,
            2 => Accepted,
            3 => Achieved,
            4 => Completed,
            5 => OtherCompleted,
            6 => Botched,
            _ => return None,
        })
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(super) struct Origin {
    pub resource: String,
    pub mount: String,
    pub line: usize,
    pub object: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct Record {
    state: Status,
    // Botching retains the underlying state. A quest botched before it was
    // ever mentioned stays out of the player's logbook.
    botched: bool,
    minute: u64,
    origin: Origin,
}

impl Record {
    fn status(&self) -> Status {
        if self.botched {
            Status::Botched
        } else {
            self.state
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct Ledger {
    player: BTreeMap<i32, Record>,
    global: BTreeMap<i32, Status>,
}

impl Ledger {
    pub(super) fn new_completions<'a>(
        &'a self,
        previous: &'a Self,
    ) -> impl Iterator<Item = i32> + 'a {
        self.player.iter().filter_map(move |(&id, r)| {
            (r.status() == Status::Completed
                && previous.status(id, false) != Some(Status::Completed))
            .then_some(id)
        })
    }

    pub(super) fn status(&self, id: i32, global: bool) -> Option<Status> {
        valid_id(id).then(|| {
            if global {
                self.global.get(&id).copied().unwrap_or(Status::Accepted)
            } else {
                self.player.get(&id).map(Record::status).unwrap_or_default()
            }
        })
    }

    pub(super) fn set_global(&mut self, id: i32, next: Status) -> Option<()> {
        if self.status(id, true)? == Status::Accepted
            && matches!(next, Status::Completed | Status::Botched)
        {
            self.global.insert(id, next);
        }
        Some(())
    }

    pub(super) fn set_player(
        &mut self,
        id: i32,
        requested: Status,
        minute: u64,
        origin: impl FnOnce() -> Origin,
    ) -> Option<()> {
        use Status::*;
        let old = self.status(id, false)?;
        if requested <= old || matches!(old, Completed | OtherCompleted | Botched) {
            return Some(());
        }
        let next = match self.status(id, true)? {
            Completed => OtherCompleted,
            Botched => Botched,
            _ => {
                self.set_global(
                    id,
                    if requested == OtherCompleted {
                        Completed
                    } else {
                        requested
                    },
                )?;
                requested
            }
        };
        self.player.insert(
            id,
            Record {
                state: if next == Botched { old } else { next },
                botched: next == Botched,
                minute,
                origin: origin(),
            },
        );
        Some(())
    }

    pub(super) fn validate(&self) -> Result<(), String> {
        if self.player.iter().any(|(&id, record)| {
            !valid_id(id)
                || record.state == Status::Botched
                || (record.state == Status::Unknown && !record.botched)
        }) || self.global.iter().any(|(&id, state)| {
            !valid_id(id) || !matches!(state, Status::Completed | Status::Botched)
        }) {
            return Err("Invalid saved quest record".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Entry<'a> {
    pub id: i32,
    pub status: Status,
    pub minute: u64,
    pub description: Option<&'a str>,
    pub text_path: Option<&'a str>,
}

pub struct Catalog {
    normal: Option<LoadedMes>,
    dumb: Option<LoadedMes>,
    labels: Option<LoadedMes>,
    rules: Option<LoadedMes>,
    calendar: Option<LoadedMes>,
    rumors: [Option<LoadedMes>; 2],
    backgrounds: Option<LoadedMes>,
    definitions: Option<LoadedMes>,
    errors: Vec<String>,
}

impl Catalog {
    pub(super) fn load(vfs: &mut Vfs) -> Self {
        let mut errors = Vec::new();
        let mut load = |path| match load_mes(vfs, path) {
            Ok(table) => Some(table),
            Err(error) => {
                errors.push(error.to_string());
                None
            }
        };
        Self {
            normal: load("mes/gamequestlog.mes"),
            dumb: load("mes/gamequestlogdumb.mes"),
            labels: load("mes/logbk_ui.mes"),
            rules: load("rules/gamequest.mes"),
            calendar: load("rules/gameinit.mes"),
            rumors: [
                load("mes/game_rd_npc_m2m.mes"),
                load("mes/game_rd_npc_m2m_dumb.mes"),
            ],
            backgrounds: load("mes/gameback.mes"),
            definitions: load("rules/backgrnd.mes"),
            errors,
        }
    }

    pub fn entries<'a>(&'a self, ledger: &Ledger, intelligence: i32) -> Vec<Entry<'a>> {
        let mut entries: Vec<_> = ledger
            .player
            .iter()
            .filter(|(_, record)| record.state != Status::Unknown)
            .map(|(&id, record)| {
                let text = (intelligence <= 4)
                    .then_some(self.dumb.as_ref())
                    .flatten()
                    .filter(|table| table.mes.get(id).is_some())
                    .or(self.normal.as_ref());
                Entry {
                    id,
                    status: record.status(),
                    minute: record.minute,
                    description: text.and_then(|table| table.mes.get(id)),
                    text_path: text.map(|table| table.source.virtual_path.as_str()),
                }
            })
            .collect();
        entries.sort_by_key(|entry| (entry.minute, entry.id));
        entries
    }

    pub fn rumor(&self, id: i32, intelligence: i32) -> Option<&str> {
        self.rumors[usize::from(intelligence < 5)]
            .as_ref()?
            .mes
            .get(id.checked_mul(20)?)
    }
    pub fn background(&self, id: i32, authored: i32) -> Option<(&str, &str)> {
        let key = if authored > 0 {
            authored
        } else {
            self.definitions
                .as_ref()?
                .mes
                .get(id * 10)?
                .trim()
                .parse()
                .ok()?
        };
        let s = self.backgrounds.as_ref()?.mes.get(key)?;
        Some(s.split_once('\n').unwrap_or((s, "")))
    }
    pub fn label(&self, id: i32) -> Option<&str> {
        self.labels.as_ref()?.mes.get(id)
    }

    pub fn start_year(&self) -> u64 {
        self.calendar
            .as_ref()
            .and_then(|table| table.mes.get(4))
            .and_then(|value| value.trim().parse::<u32>().ok())
            .map_or(1885, u64::from)
    }

    pub(super) fn coverage(&self) -> serde_json::Value {
        let ids: Vec<_> = self
            .rules
            .iter()
            .flat_map(|table| table.mes.entries())
            .map(|(id, _)| id)
            .filter(|&id| valid_id(id))
            .collect();
        let count = |table: &Option<LoadedMes>| {
            table.as_ref().map(|table| {
                ids.iter()
                    .filter(|&&id| table.mes.get(id).is_some())
                    .count()
            })
        };
        serde_json::json!({"level": "message_records", "rules": self.rules.as_ref().map(|_| ids.len()),
            "normal_descriptions": count(&self.normal), "low_intelligence_descriptions": count(&self.dumb),
            "sources": self.sources(), "errors": self.errors})
    }

    fn sources(&self) -> BTreeMap<&str, String> {
        [
            &self.normal,
            &self.dumb,
            &self.labels,
            &self.rules,
            &self.calendar,
        ]
        .into_iter()
        .flatten()
        .map(|table| (table.source.virtual_path.as_str(), table.source.to_string()))
        .collect()
    }

    pub fn diagnostic(&self, ledger: &Ledger, intelligence: i32) -> serde_json::Value {
        use serde_json::json;
        let entries: Vec<_> = self
            .entries(ledger, intelligence)
            .into_iter()
            .map(|entry| {
                let record = &ledger.player[&entry.id];
                json!({"entry": entry, "last_change": record.origin,
                "original_rules": self.rules.as_ref().and_then(|table| table.mes.get(entry.id)),
                "unimplemented_completion_effects": (entry.status == Status::Completed)
                    .then_some(["reaction", "sound"])})
            })
            .collect();
        json!({"entries": entries, "sources": self.sources(), "errors": self.errors,
            "global_default": Status::Accepted, "player_default": Status::Unknown})
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quest_state_is_monotonic_and_global_resolution_is_distinct() {
        use Status::*;
        let mut ledger = Ledger::default();
        let origin = || Origin {
            resource: "dlg/fixture.dlg".into(),
            mount: "fixture".into(),
            line: 1,
            object: "npc".into(),
        };
        assert_eq!(ledger.status(1000, false), Some(Unknown));
        assert_eq!(ledger.status(1000, true), Some(Accepted));
        assert_eq!(ledger.set_player(999, Accepted, 0, origin), None);
        ledger.set_player(1000, Accepted, 1, origin).unwrap();
        ledger.set_player(1000, Mentioned, 2, origin).unwrap();
        assert_eq!(ledger.player[&1000].minute, 1);
        ledger.set_player(1000, Botched, 3, origin).unwrap();
        ledger.set_player(1000, Completed, 4, origin).unwrap();
        assert_eq!(ledger.status(1000, false), Some(Botched));
        assert_eq!(ledger.player[&1000].state, Accepted);
        assert_eq!(ledger.status(1000, true), Some(Botched));
        ledger.set_global(1001, Completed).unwrap();
        assert_eq!(ledger.status(1001, false), Some(Unknown));
        ledger.set_player(1001, Accepted, 4, origin).unwrap();
        assert_eq!(ledger.status(1001, false), Some(OtherCompleted));
        ledger.validate().unwrap();
    }
}
