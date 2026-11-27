//! Run the original dying attachment once at the live-to-dead transition.
use super::*;
use crate::content::script::{Actor as Trigger, Attachment, Event, Invocation, Script};

impl InventoryReader {
    pub(super) fn run_dying_scripts(&mut self, w: &mut InteractiveWorld) {
        for (id, killer) in std::mem::take(&mut w.cache.inventory.combat.deaths) {
            match self.dying_script(w, id, killer) {
                Ok(Some(event)) => w.cache.inventory.combat.log(event),
                Ok(None) => {}
                Err(reason) => w
                    .cache
                    .inventory
                    .combat
                    .log(json!({"event":"dying_script_rejected","actor":id,"reason":reason})),
            }
        }
    }
    fn dying_script(
        &mut self,
        w: &mut InteractiveWorld,
        id: OriginalId,
        killer: Actor,
    ) -> Result<Option<Value>, String> {
        let Some(Aim::Object(key)) = Self::subject_aim(w, &Subject::Mobile(id)) else {
            return Err("Dying actor is outside the loaded map".into());
        };
        let (resident, tile) = w
            .cache
            .npc(key)
            .ok_or("Original dying actor is unavailable")?;
        let object = resident.instance();
        let props = mobile::population::Effective::new(&object, resident.prototype.as_deref());
        let Some((script, initial)) = props.appearance.dying.filter(|(n, _)| *n > 0) else {
            return Ok(None);
        };
        let attachment = Attachment {
            owner: id.key(),
            event: Event::Dying,
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
        let loc = match killer {
            None => w.cache.player.as_ref().map(Player::tile),
            Some(k) => Self::subject_aim(w, &Subject::Mobile(k)).and_then(|a| {
                if let Aim::Object(key) = a {
                    w.cache.target(key).map(|t| t.tile)
                } else {
                    None
                }
            }),
        };
        let input = Invocation {
            attachment: &attachment,
            triggerer: if killer.is_none() {
                Trigger::Player
            } else {
                Trigger::Mobile
            },
            position: 0,
            hour: w.cache.hour,
            follows_player: None,
            npc_flags,
            locations: [loc, Some(tile)],
            met: Some(w.met_npcs.contains(&(w.map.clone(), id.key()))),
        };
        let result = program
            .execute(&input, &mut staged)
            .map_err(|e| format!("{}: {e}", program.source))?;
        w.commit_script_inventory(&mut self.vfs, &id.key(), &mut staged)?;
        w.cache.scripts = staged;
        Ok(Some(
            json!({"event":"dying_script","actor":id,"killer":subject(killer),"source":crate::content::diagnostics::source(&program.source),"execution":result}),
        ))
    }
}
