//! Targeted player actions. Lock flags are deltas over the original record.
use super::*;
use crate::sector::objects::OriginalId;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum LockId {
    Door((u64, usize)),
    Container(OriginalId),
}

impl LockId {
    pub(super) fn label(self) -> String {
        match self {
            Self::Door((sector, record)) => format!("{sector}:{record}"),
            Self::Container(id) => id.key(),
        }
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub(super) struct LockChange {
    pub locked: Option<bool>,
    pub jammed: Option<bool>,
    // CE TIMEEVENT_TYPE_LOCK runs even if another method has since touched the lock.
    pub relock_at: BTreeSet<u64>,
}

impl LockChange {
    pub fn flags(&self, mut flags: u32) -> u32 {
        for (bit, value) in [(1, self.locked), (2, self.jammed)] {
            if let Some(on) = value {
                flags = (flags & !bit) | if on { bit } else { 0 };
            }
        }
        flags
    }

    pub fn expire(&mut self, now: u64) -> bool {
        if self.relock_at.first().is_none_or(|due| *due > now) {
            return false;
        }
        self.relock_at.retain(|due| *due > now);
        self.locked = Some(true);
        self.jammed = Some(false);
        true
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Method {
    PickLocks(OriginalId),
    UnlockingCantrip,
}
impl Method {
    pub fn is_skill(self) -> bool {
        matches!(self, Self::PickLocks(_))
    }
}

#[derive(Default)]
pub(super) struct UseState {
    pub selected: Option<Method>,
    pub pending: Option<Method>,
    pub request: Option<(WorldOrder, Method)>,
    pub finish_at: Option<Duration>,
    pub casting: Option<(WorldOrder, Duration)>,
    pub last: Option<Value>,
    pub message: String,
}
impl UseState {
    pub fn select(&mut self, method: Method) {
        self.message = if method.is_skill() {
            "Pick Locks"
        } else {
            "Unlocking Cantrip"
        }
        .to_owned()
            + " — select a portal or container";
        self.selected = Some(method);
    }
    pub fn cancel(&mut self) -> bool {
        let method = self
            .selected
            .take()
            .or(self.pending.take())
            .or(self.request.take().map(|r| r.1))
            .or(self.casting.take().map(|_| Method::UnlockingCantrip));
        self.finish_at = None;
        if let Some(method) = method {
            self.last = Some(json!({"method":method,"outcome":"cancelled","fatigue_cost":0}));
            self.message = "Cancelled".into();
        }
        method.is_some()
    }
    pub fn reject(&mut self, method: Method, reason: &str) {
        self.message = reason.into();
        self.last =
            Some(json!({"method":method,"outcome":"rejected","reason":reason,"fatigue_cost":0}));
    }
    pub fn clear_feedback(&mut self) {
        self.message.clear();
    }
    pub fn diagnostic(&self) -> Value {
        json!({"selected":self.selected,"pending":self.pending.or(self.casting.map(|_|Method::UnlockingCantrip)),"message":self.message,"last":self.last})
    }
}

pub(super) struct Spell {
    pub cost: i32,
    pub willpower: i32,
    pub range: u32,
    pub target_effect: Option<i32>,
    pub targets: [bool; 2],
    pub clears: [bool; 2],
    pub source: super::super::Source,
}
impl Spell {
    pub fn load(vfs: &mut Vfs) -> Result<Self, String> {
        let table = load_mes(vfs, "rules/spelllist.mes").map_err(|e| e.to_string())?;
        Self::decode(&table).map_err(|e| format!("Unlocking Cantrip in {}: {e}", table.source))
    }
    fn decode(table: &super::super::LoadedMes) -> Result<Self, String> {
        let mut spell = Self {
            cost: 0,
            willpower: 9,
            range: 99,
            target_effect: None,
            targets: [false; 2],
            clears: [false; 2],
            source: table.source.clone(),
        };
        let kinds = ["tgt_obj_t_portal", "tgt_obj_t_container"];
        for id in 1050..1100 {
            let Some(line) = table.mes.get(id) else {
                continue;
            };
            let line = line.to_ascii_lowercase();
            let parts: Vec<_> = line.split(',').map(str::trim).collect();
            if id == 1050 {
                let targets = line
                    .trim()
                    .strip_prefix("aoe:")
                    .ok_or("expected target flags")?;
                for target in targets.split('|').map(str::trim) {
                    let i = kinds
                        .iter()
                        .position(|&s| s == target)
                        .ok_or("unsupported target flag")?;
                    spell.targets[i] = true;
                }
            } else if id == 1051 {
                for part in parts {
                    let (key, value) = part.split_once(':').ok_or("expected spell parameter")?;
                    let n: i32 = value
                        .trim()
                        .parse()
                        .map_err(|_| "invalid spell parameter")?;
                    if n < 0 {
                        return Err("negative spell parameter".into());
                    }
                    match key.trim() {
                        "cost" => spell.cost = n,
                        "iq" => spell.willpower = n,
                        "range" => spell.range = n as u32,
                        _ => return Err(format!("unsupported parameter {key}")),
                    }
                }
            } else {
                match parts.as_slice() {
                    ["[begin]", target, "type: objflag", field, flag, "flag_off"] => {
                        let i = kinds
                            .iter()
                            .position(|&s| Some(s) == target.strip_prefix("aoe:").map(str::trim))
                            .ok_or("unsupported component target")?;
                        if (*field, *flag)
                            != [
                                ("obj_f_portal_flags", "opf_locked"),
                                ("obj_f_container_flags", "ocof_locked"),
                            ][i]
                        {
                            return Err("unsupported lock flag component".into());
                        }
                        spell.clears[i] = true;
                    }
                    ["[begin]", "type: eyecandy", index, "add"] => {
                        let index = index
                            .parse::<i32>()
                            .map_err(|_| "invalid EyeCandy component index")?;
                        if !(0..6).contains(&index) || spell.target_effect.is_some() {
                            return Err("unsupported EyeCandy component sequence".into());
                        }
                        spell.target_effect = Some(index);
                    }
                    _ => return Err(format!("unsupported spell clause {id}: {line}")),
                }
            }
        }
        if !spell.targets.contains(&true) {
            return Err("missing target definition".into());
        }
        Ok(spell)
    }
}

impl InteractiveWorld {
    pub(super) fn lock_id(&self, key: WorldOrder) -> Option<LockId> {
        match &self.cache.target(key)?.action {
            interaction::Action::Door(door) => Some(LockId::Door(door.id)),
            interaction::Action::Window { id, .. } => Some(LockId::Door(*id)),
            interaction::Action::Container(id) => Some(LockId::Container(*id)),
            _ => None,
        }
    }
    pub fn ability_message(&self) -> &str {
        &self.ability.message
    }
    pub fn cancel_ability(&mut self) -> bool {
        if self.ability.cancel() {
            self.cancel_player_route();
            true
        } else {
            false
        }
    }
    pub fn college_levels(&self) -> [u32; 16] {
        self.cache.inventory.character.colleges
    }
    pub fn conveyance_level(&self) -> u32 {
        self.cache.inventory.character.colleges[0]
    }
    pub fn select_cantrip(&mut self) {
        let method = Method::UnlockingCantrip;
        self.cancel_player_route();
        if self.conveyance_level() < 2 {
            self.ability.reject(method, "Unlocking Cantrip is unknown");
        } else {
            self.ability.select(method);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_expiry_preserves_authored_restrictions_and_other_deadlines() {
        let mut change = LockChange {
            locked: Some(false),
            jammed: Some(true),
            relock_at: [3600, 86400].into(),
        };
        assert_eq!(change.flags(0x25), 0x26);
        assert!(!change.expire(3599));
        assert!(change.expire(3600));
        assert_eq!(change.flags(0x25), 0x25);
        assert_eq!(change.relock_at.first(), Some(&86400));
    }
}
