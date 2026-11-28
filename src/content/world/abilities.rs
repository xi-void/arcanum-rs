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
    #[serde(default)]
    pub held: Option<bool>,
    pub locked: Option<bool>,
    pub jammed: Option<bool>,
    // CE TIMEEVENT_TYPE_LOCK runs even if another method has since touched the lock.
    pub relock_at: BTreeSet<u64>,
}

impl LockChange {
    pub fn flags(&self, mut flags: u32) -> u32 {
        for (bit, value) in [(1, self.locked), (2, self.jammed), (4, self.held)] {
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
    Consumable(OriginalId),
    Spell(usize),
}
impl Method {
    pub fn is_skill(self) -> bool {
        matches!(self, Self::PickLocks(_) | Self::Consumable(_))
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) enum Aim {
    Player,
    Object(WorldOrder),
    Tile((u32, u32)),
    Item(OriginalId),
    Destination(crate::content::jumps::Destination),
}

#[derive(Default)]
pub(super) struct UseState {
    pub selected: Option<Method>,
    pub pending: Option<Method>,
    pub request: Option<(Aim, Method)>,
    pub finish_at: Option<Duration>,
    pub casting_actor: Option<OriginalId>,
    pub casting: Option<(Aim, Method, Duration)>,
    pub last: Option<Value>,
    pub message: String,
}
impl UseState {
    pub fn select(&mut self, method: Method) {
        self.message = "Pick Locks — select a portal or container".into();
        self.selected = Some(method);
    }
    pub fn cancel(&mut self) -> bool {
        let method = self
            .selected
            .take()
            .or(self.pending.take())
            .or(self.request.take().map(|r| r.1))
            .or(self.casting.take().map(|(_, m, _)| m));
        self.finish_at = None;
        self.casting_actor = None;
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
        json!({"selected":self.selected,"pending":self.pending.or(self.casting.map(|(_,m,_)|m)),"message":self.message,"last":self.last})
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
    pub fn selecting_skill(&self) -> bool {
        self.ability.selected.is_some_and(Method::is_skill)
    }
    pub fn combat_ready(&self) -> bool {
        self.cache.inventory.combat.armed
    }
    pub fn selected_spell(&self) -> Option<usize> {
        if let Some(Method::Spell(id)) = self.ability.selected {
            Some(id)
        } else {
            None
        }
    }
    pub fn reject_selected_spell(&mut self, reason: &str) {
        if let Some(m) = self.ability.selected {
            self.ability.reject(m, reason)
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
    pub fn select_spell(&mut self, id: usize) {
        self.cancel_player_route();
        let method = Method::Spell(id);
        let Some(study) = self.progression_rules().spells.get(id) else {
            self.ability.reject(method, "Unknown spell");
            return;
        };
        if self.college_levels()[id / 5] as usize <= id % 5 {
            self.ability.reject(method, "Spell has not been learned");
            return;
        }
        let program = match &study.program {
            Ok(p) => p,
            Err(e) => {
                let reason = e.clone();
                self.ability.reject(method, &reason);
                return;
            }
        };
        let auto = program.targets.iter().any(|s| s == "tgt_self")
            && !program
                .targets
                .iter()
                .any(|s| s.starts_with("tgt_tile") || s == "tgt_cone");
        let immediate=auto && !program.components.iter().any(|c|matches!(&c.operation,crate::content::magick::Operation::Move(mode) if mode=="teleport_tile"));
        self.ability.message = format!("{} — select a target", study.name);
        self.ability.selected = Some(method);
        if immediate {
            self.ability.request = Some((Aim::Player, method))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_expiry_preserves_authored_restrictions_and_other_deadlines() {
        let mut change = LockChange {
            held: None,
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
