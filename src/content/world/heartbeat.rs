//! First and recurring NPC callbacks are scheduled on active original mobiles.
//! The script, its local state and any requested object action commit together.
use super::*;
use crate::content::script::{
    Actor, ActorAction, Attachment, Event, HeartbeatContext, Invocation, Local, Outcome, Script,
};
use crate::sector::objects::OriginalId;
use serde_json::{Value, json};

struct Pending {
    key: WorldOrder,
    scripts: [(i32, Local); 2],
    first: bool,
    due: Duration,
}

#[derive(Default)]
pub(super) struct Scheduler {
    generation: Option<u64>,
    random: crate::random::Random,
    activated: BTreeSet<OriginalId>,
    pending: BTreeMap<OriginalId, Pending>,
    programs: BTreeMap<i32, Result<Arc<Script>, String>>,
    history: std::collections::VecDeque<Value>,
}
impl InteractiveWorld {
    pub fn heartbeat_report(&self) -> Value {
        json!({"scheduled":self.heartbeats.pending.len(),"recent":self.heartbeats.history})
    }
    pub fn heartbeat_wait(&self, now: Duration) -> Option<Duration> {
        if self.cache.player.is_none() || self.cache.inventory.combat.timed() {
            return None;
        }
        if self.heartbeats.generation != Some(self.cache.step) {
            return Some(Duration::ZERO);
        }
        self.heartbeats
            .pending
            .values()
            .map(|p| p.due.saturating_sub(now))
            .min()
    }
    fn register_heartbeats(&mut self, now: Duration) {
        if self.heartbeats.generation == Some(self.cache.step) {
            return;
        }
        self.heartbeats.generation = Some(self.cache.step);
        let mut present = BTreeSet::new();
        for sector in &self.cache.active {
            for (&key, target) in &self.cache.entries[sector].0.interactions {
                let interaction::Action::Npc(id) = target.action else {
                    continue;
                };
                if self.cache.scripts.switched_off.contains(&id.key())
                    || self.cache.inventory.magick.down(id)
                {
                    continue;
                }
                let Some((resident, _)) = self.cache.npc(key) else {
                    continue;
                };
                let obj = resident.instance();
                let Some(scripts) =
                    mobile::population::Effective::new(&obj, resident.prototype.as_deref())
                        .appearance
                        .heartbeat
                else {
                    continue;
                };
                let removed = self.cache.scripts.detached.get(&id.key());
                present.insert(id);
                let first = self.heartbeats.activated.insert(id);
                if ![Event::FirstHeartbeat, Event::Heartbeat]
                    .into_iter()
                    .zip(scripts)
                    .any(|(event, (s, _))| {
                        s > 0
                            && (event == Event::Heartbeat || first)
                            && removed.is_none_or(|d| !d.contains(&event))
                    })
                {
                    continue;
                }
                self.heartbeats.pending.entry(id).or_insert(Pending {
                    key,
                    scripts,
                    first,
                    due: now,
                });
            }
        }
        self.heartbeats.activated.retain(|id| present.contains(id));
        self.heartbeats.pending.retain(|id, _| present.contains(id));
    }
}

impl InventoryReader {
    pub fn tick_heartbeats(
        &mut self,
        w: &mut InteractiveWorld,
        now: Duration,
        viewport: (u32, u32),
    ) -> bool {
        if w.heartbeat_wait(now) != Some(Duration::ZERO) {
            return false;
        }
        w.register_heartbeats(now);
        let pc = w.cache.player.as_ref().unwrap().tile();
        let due: Vec<_> = w
            .heartbeats
            .pending
            .iter()
            .filter(|(_, p)| p.due <= now)
            .map(|(&id, _)| id)
            .collect();
        let mut changed = false;
        for id in due {
            let p = w.heartbeats.pending.get_mut(&id).unwrap();
            let key = p.key;
            let scripts = p.scripts;
            let first = std::mem::take(&mut p.first);
            let Some((r, tile)) = w.cache.npc(key) else {
                w.heartbeats.pending.remove(&id);
                continue;
            };
            let dist = pc.0.abs_diff(tile.0).max(pc.1.abs_diff(tile.1));
            let jitter = if first {
                w.heartbeats.random.below(5001)
            } else {
                0
            };
            let delay = Duration::from_millis(250 + 4750 * u64::from(dist.min(30)) / 30 + jitter);
            let dead = r.prototype.as_ref().is_some_and(|p| {
                w.cache
                    .inventory
                    .npc_vitals(id, &p.defaults, &r.instance())
                    .1
                    .current_health
                    <= 0
            });
            if dead {
                w.heartbeats.pending.remove(&id);
                continue;
            }
            w.heartbeats.pending.get_mut(&id).unwrap().due = now + delay;
            for (index, event) in [Event::FirstHeartbeat, Event::Heartbeat]
                .into_iter()
                .enumerate()
            {
                let (script, initial) = scripts[index];
                if script <= 0
                    || (index == 0 && !first)
                    || (index == 1 && dist > 30)
                    || w.cache
                        .scripts
                        .detached
                        .get(&id.key())
                        .is_some_and(|s| s.contains(&event))
                {
                    continue;
                }
                let attachment = Attachment {
                    owner: id.key(),
                    event,
                    script,
                    initial,
                };
                let program = w
                    .heartbeats
                    .programs
                    .entry(script)
                    .or_insert_with(|| Script::load(&mut self.vfs, script).map(Arc::new))
                    .clone();
                let result = program
                    .as_ref()
                    .map_err(Clone::clone)
                    .and_then(|p| self.run_heartbeat(w, key, p, &attachment, now, viewport));
                let outcome = match &result {
                    Ok((run_default, acted)) => {
                        json!({"status":"completed","run_default":run_default,"actor_changed":acted})
                    }
                    Err(reason) => json!({"status":"rejected","reason":reason}),
                };
                let record = json!({"actor":id.key(),"event":event,"script":script,
                    "source":program.as_ref().ok().map(|p|super::super::diagnostics::source(&p.source)),"result":outcome});
                if !w.heartbeats.history.contains(&record) {
                    if w.heartbeats.history.len() >= 64 {
                        w.heartbeats.history.pop_front();
                    }
                    w.heartbeats.history.push_back(record);
                }
                match result {
                    Ok((run_default, acted)) => {
                        changed |= acted;
                        if !run_default {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
            if scripts[1].0 <= 0
                || w.cache.inventory.magick.down(id)
                || w.cache.scripts.switched_off.contains(&id.key())
                || w.cache
                    .scripts
                    .detached
                    .get(&id.key())
                    .is_some_and(|s| s.contains(&Event::Heartbeat))
            {
                w.heartbeats.pending.remove(&id);
            }
        }
        changed
    }

    fn run_heartbeat(
        &mut self,
        w: &mut InteractiveWorld,
        key: WorldOrder,
        program: &Script,
        attachment: &Attachment,
        now: Duration,
        viewport: (u32, u32),
    ) -> Result<(bool, bool), String> {
        let (r, tile) = w.cache.npc(key).ok_or("NPC callback owner unavailable")?;
        let id = r.id;
        let pc = w.cache.player.as_ref().ok_or("Player unavailable")?.tile();
        let mut vicinity = Vec::new();
        if program
            .code()
            .statements
            .iter()
            .any(|r| r.action.opcode == 18 || r.otherwise.opcode == 18)
        {
            // CE enumerates the tile-aligned bounding rectangle of the projected window.
            let extent = u64::from(viewport.0.div_ceil(2)) + 2 * u64::from(viewport.1.div_ceil(2));
            let radius = (extent + 40) / 80;
            let inside =
                |t: (u32, u32)| u64::from(t.0.abs_diff(tile.0).max(t.1.abs_diff(tile.1))) <= radius;
            for sector in &w.cache.active {
                for target in w.cache.entries[sector].0.interactions.values() {
                    if let interaction::Action::Npc(id) = target.action
                        && inside(target.tile)
                        && !w.cache.scripts.switched_off.contains(&id.key())
                    {
                        vicinity.push(Actor::Mobile)
                    }
                }
            }
            if inside(pc) {
                vicinity.push(Actor::Player)
            }
        }
        let mut staged = w.cache.scripts.clone();
        let needs_party = program.code().statements.iter().any(|r| {
            matches!(r.action.opcode, 21 | 22 | 67)
                || matches!(r.otherwise.opcode, 21 | 22 | 67)
                || matches!(r.condition.opcode, 21 | 23)
        });
        if needs_party {
            staged.interaction = Some(w.script_inventory(&mut self.vfs, &id.key(), None)?);
        }
        staged.heartbeat = Some(HeartbeatContext {
            vicinity,
            action: None,
        });
        let npc_flags = r
            .instance()
            .field(128)
            .and_then(crate::sector::objects::FieldValue::scalar)
            .or_else(|| r.prototype.as_ref()?.defaults.npc_flags);
        let input = Invocation {
            attachment,
            triggerer: Actor::Mobile,
            position: 0,
            hour: w.cache.hour,
            follows_player: None,
            npc_flags,
            locations: [Some(tile), Some(tile)],
            met: Some(w.met_npcs.contains(&(w.map.clone(), id.key()))),
        };
        let execution = program
            .execute(&input, &mut staged)
            .map_err(|e| e.to_string())?;
        let Outcome::Return { run_default } = execution.outcome else {
            return Err("Unexpected NPC callback outcome".into());
        };
        let action = staged.heartbeat.take().unwrap().action;
        staged.interaction = None;
        let party_changed = staged.followers != w.cache.scripts.followers;
        let off = staged.switched_off != w.cache.scripts.switched_off;
        let saved = std::mem::replace(&mut w.cache.scripts, staged);
        let applied = match action {
            Some(ActorAction::Kill) => self.script_kill(w, id, now),
            Some(ActorAction::CallDialogue(line)) => self.open_npc_dialogue(w, key, Some(line)),
            None => Ok(()),
        };
        if let Err(e) = applied {
            w.cache.scripts = saved;
            return Err(e);
        }
        if off {
            w.cache.activate(&w.cache.active.clone());
        }
        Ok((run_default, off || party_changed || action.is_some()))
    }
}
