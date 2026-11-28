//! CE standalone script calls use a fresh SCR header and the scheduled game time.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Call {
    pub(super) script: i32,
    pub(super) line: usize,
    pub(super) owner: String,
    pub(super) triggerer: Actor,
    pub(super) map: Option<String>,
}

impl State {
    pub(in crate::content) fn next_call(&self) -> Option<u64> {
        self.calls.first_key_value().map(|(&second, _)| second)
    }

    pub(in crate::content) fn tick_calls(
        &mut self,
        vfs: &mut Vfs,
        now: u64,
        mut rewards: impl FnMut(
            &crate::content::quest::Ledger,
            &crate::content::quest::Ledger,
        ) -> Result<(), String>,
    ) -> bool {
        self.game_second = now;
        let mut changed = false;
        // Limit work per presentation iteration; due calls retain their deadlines and order.
        for _ in 0..64 {
            let Some(mut entry) = self.calls.first_entry().filter(|e| *e.key() <= now) else {
                break;
            };
            let due = *entry.key();
            let call = entry.get_mut().pop_front().expect("validated script queue");
            if entry.get().is_empty() {
                entry.remove();
            }
            let result = Script::load(vfs, call.script).and_then(|script| {
                let mut staged = self.clone();
                let sector = staged.sector_map.take();
                if sector.as_deref().map(|(map, _)| map) == call.map.as_ref() {
                    staged.sector_map = sector.clone();
                }
                staged.game_second = due;
                staged.game_minute = due / 60;
                let locals = staged
                    .attachments
                    .get_mut(&call.owner)
                    .and_then(|events| events.remove(&Event::Use));
                let attachment = Attachment {
                    owner: call.owner.clone(),
                    event: Event::Use,
                    script: call.script,
                    initial: script.initial,
                };
                let input = Invocation {
                    attachment: &attachment,
                    triggerer: call.triggerer,
                    position: call.line,
                    hour: ((due / 3600) % 24) as u8,
                    locations: [None, None],
                    npc_flags: None,
                    follows_player: None,
                    met: None,
                };
                let execution = script
                    .execute(&input, &mut staged)
                    .map_err(|e| format!("{}: {e}", script.source))?;
                if !matches!(execution.outcome, Outcome::Return { .. }) {
                    return Err("Unsupported timer outcome".into());
                }
                if let Some(local) = locals {
                    staged.set_local(&attachment, local)
                } else if let Some(events) = staged.attachments.get_mut(&call.owner) {
                    events.remove(&Event::Use);
                    if events.is_empty() {
                        staged.attachments.remove(&call.owner);
                    }
                }
                staged.game_second = now;
                staged.game_minute = self.game_minute;
                staged.sector_map = sector;
                rewards(&staged.quests, &self.quests)?;
                *self = staged;
                Ok(script.source.to_string())
            });
            self.last_call = Some(serde_json::json!({"due":due,"call":call,"result":result}));
            changed = true;
        }
        changed
    }
}
