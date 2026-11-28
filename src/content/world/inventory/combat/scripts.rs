//! Run original death, resurrection and party kill attachments over the actor transaction.
use super::*;
use crate::content::script::{Actor as Trigger, Attachment, Event, Invocation, Script};

impl InventoryReader {
    pub(super) fn run_dying_scripts(&mut self, w: &mut InteractiveWorld) {
        for (id, killer) in std::mem::take(&mut w.cache.inventory.combat.deaths) {
            self.dispatch_dying_script(w, id, Some(subject(killer)));
        }
        for victim in std::mem::take(&mut w.cache.inventory.combat.leader_kills) {
            let followers: Vec<_> = w
                .cache
                .scripts
                .followers
                .keys()
                .filter(|k| w.cache.scripts.following(k))
                .filter_map(|k| parse_id(k).ok())
                .collect();
            for actor in followers {
                self.dispatch_actor_event(
                    w,
                    actor,
                    Some(Subject::Mobile(victim)),
                    Event::LeaderKilling,
                );
            }
        }
    }

    pub(super) fn dispatch_dying_script(
        &mut self,
        w: &mut InteractiveWorld,
        id: OriginalId,
        killer: Option<Subject>,
    ) {
        self.dispatch_actor_event(w, id, killer, Event::Dying);
    }
    pub(in crate::content::world::inventory) fn dispatch_actor_event(
        &mut self,
        w: &mut InteractiveWorld,
        id: OriginalId,
        source: Option<Subject>,
        event: Event,
    ) -> bool {
        let label = match event {
            Event::Dying => "dying_script",
            Event::Resurrect => "resurrect_script",
            Event::LeaderKilling => "leader_killing_script",
            _ => return false,
        };
        match self.actor_script(w, id, source, event, label) {
            Ok(Some((entry, allowed))) => {
                w.cache.inventory.combat.log(entry);
                allowed
            }
            Ok(None) => true,
            Err(reason) => {
                w.cache
                    .inventory
                    .combat
                    .log(json!({"event":format!("{label}_rejected"),"actor":id,"reason":reason}));
                false
            }
        }
    }
    fn actor_script(
        &mut self,
        w: &mut InteractiveWorld,
        id: OriginalId,
        killer: Option<Subject>,
        event: Event,
        label: &str,
    ) -> Result<Option<(Value, bool)>, String> {
        let Some(Aim::Object(key)) = Self::subject_aim(w, &Subject::Mobile(id)) else {
            return Err("Scripted actor is outside the loaded map".into());
        };
        let (resident, tile) = w
            .cache
            .npc(key)
            .ok_or("Original scripted actor is unavailable")?;
        let object = resident.instance();
        let props = mobile::population::Effective::new(&object, resident.prototype.as_deref());
        let Some((script, initial)) = (match event {
            Event::Dying => props.appearance.dying,
            Event::Resurrect => props.appearance.party_events.map(|a| a[0]),
            Event::LeaderKilling => props.appearance.party_events.map(|a| a[1]),
            _ => None,
        })
        .filter(|(n, _)| *n > 0) else {
            return Ok(None);
        };
        let attachment = Attachment {
            owner: id.key(),
            event,
            script,
            initial,
        };
        let npc_flags = object
            .field(128)
            .and_then(FieldValue::scalar)
            .or_else(|| resident.prototype.as_ref()?.defaults.npc_flags);
        let program = Script::load(&mut self.vfs, script)?;
        let mut staged = w.cache.scripts.clone();
        staged.interaction = Some(w.script_inventory(&mut self.vfs, &id.key(), None)?);
        let loc = match &killer {
            Some(Subject::Player) => w.cache.player.as_ref().map(Player::tile),
            Some(k) => Self::subject_aim(w, k).and_then(|a| {
                if let Aim::Object(key) = a {
                    w.cache.target(key).map(|t| t.tile)
                } else {
                    None
                }
            }),
            None => None,
        };
        let input = Invocation {
            attachment: &attachment,
            triggerer: match &killer {
                Some(Subject::Player) => Trigger::Player,
                Some(Subject::Mobile(_)) => Trigger::Mobile,
                _ => Trigger::None,
            },
            position: 0,
            hour: w.cache.hour,
            follows_player: Some(w.cache.scripts.followers.contains_key(&id.key())),
            npc_flags,
            locations: [loc, Some(tile)],
            met: Some(w.met_npcs.contains(&(w.map.clone(), id.key()))),
        };
        let result = program
            .execute(&input, &mut staged)
            .map_err(|e| format!("{}: {e}", program.source))?;
        w.commit_script_inventory(&mut self.vfs, &id.key(), &mut staged)?;
        w.cache.scripts = staged;
        let allow = matches!(
            result.outcome,
            crate::content::script::Outcome::Return { run_default: true }
        );
        Ok(Some((
            json!({"event":label,"actor":id,"killer":killer,"source":crate::content::diagnostics::source(&program.source),"execution":result}),
            allow,
        )))
    }
}
