//! CE sleep_ui advances one game hour per callback. Waiting only runs game-time events.
use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RestMode {
    Wait,
    Sleep,
    Bed,
}
impl RestMode {
    pub fn waiting(self) -> bool {
        self == Self::Wait
    }
}
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub(in crate::content::world) struct Rest {
    mode: RestMode,
    option: usize,
    hours: u64,
    started: u64,
    #[serde(skip)]
    next: Duration,
}
impl InteractiveWorld {
    pub fn check_rest(&self) -> Result<(), String> {
        if let Some(e) = self.incapacity() {
            return Err(e.into());
        }
        if self.cache.inventory.combat.active() {
            return Err("You cannot sleep with enemies near.".into());
        }
        if self.trading() || self.conversation().is_some() {
            return Err("Finish the conversation first".into());
        }
        if self.ability.casting.is_some()
            || self.cache.inventory.magick.animation_callback_pending()
        {
            return Err("An action is still in progress".into());
        }
        Ok(())
    }

    pub fn rest_deadline(&self) -> Option<Duration> {
        self.rest.map(|r| r.next)
    }
    pub fn rest_report(&self) -> Value {
        json!({"active":self.rest,"last":self.last_rest,"encounters":self.cache.inventory.encounter})
    }
    pub fn finish_rest(&mut self, reason: &str) {
        if let Some(r) = self.rest.take() {
            self.last_rest = Some(
                json!({"mode":r.mode,"option":r.option,"hours":(self.game_second.saturating_sub(r.started) / 3600).max(r.hours),"start_second":r.started,"end_second":self.game_second,"reason":reason}),
            );
        }
    }
    pub fn rest_feedback(&mut self, message: String) {
        self.last_rest = Some(json!({"reason":message,"outcome":"rejected"}));
        self.ability.message = message;
    }

    pub fn finish_rest_hour(&mut self, hour: u8, elapsed: Duration) {
        let Some(r) = self.rest.as_mut() else { return };
        r.hours += 1;
        let stats = self.cache.inventory.statistics();
        if stats.current_health <= 0 {
            self.finish_rest("death");
            return;
        }
        let pc = &mut self.cache.inventory.character;
        let rate = stats
            .heal_rate
            .saturating_mul(if r.mode == RestMode::Bed { 2 } else { 1 });
        if !r.mode.waiting() {
            pc.health_damage = pc.health_damage.saturating_sub(rate).max(0);
            pc.fatigue_damage = pc
                .fatigue_damage
                .saturating_sub(rate.saturating_mul(3))
                .max(0);
            if pc.fatigue_damage == 0 {
                pc.recover_at = None
            }
            self.cache.inventory.revision += 1;
        }
        let complete = match r.option {
            i @ 0..=4 => r.hours >= [1, 2, 4, 8, 24][i],
            5 => hour == 7,
            6 => hour == 20,
            _ => pc.health_damage == 0 || rate == 0,
        };
        if complete {
            self.finish_rest("finished")
        } else {
            r.next = elapsed + Duration::from_millis(200)
        }
    }
}

impl InventoryReader {
    pub fn tick_script_calls(&mut self, w: &mut InteractiveWorld, now: u64) -> bool {
        w.cache.scripts.tick_calls(&mut self.vfs, now, |new, old| {
            w.cache.inventory.apply_quest_rewards(new, old)
        })
    }
    pub fn rest_mode(&mut self, w: &mut InteractiveWorld, bed: bool) -> Result<RestMode, String> {
        w.check_rest()?;
        if bed {
            return w.validate_bed(&mut self.vfs, false).map(|()| RestMode::Bed);
        }
        let (at, town) = w
            .player_map_position()
            .ok_or("The player sector is still loading")?;
        if !w.cache.entries[&tile_sector_id(at)]
            .0
            .report
            .collision_available
        {
            return Err("Player sector collision is unavailable".into());
        }
        if town == 0 {
            return Ok(RestMode::Sleep);
        }
        let towns = load_mes(&mut self.vfs, "rules/townmap.mes").map_err(|e| e.to_string())?;
        let row = towns
            .mes
            .get(town as i32)
            .ok_or_else(|| format!("Unknown town map {town}"))?;
        if super::super::super::map_atlas::town_rule(row).1 {
            Ok(RestMode::Wait)
        } else {
            Err("You cannot wait here.".into())
        }
    }
    pub fn begin_rest(
        &mut self,
        w: &mut InteractiveWorld,
        mode: RestMode,
        option: usize,
        elapsed: Duration,
    ) -> Result<(), String> {
        w.check_rest()?;
        if option >= if mode.waiting() { 7 } else { 8 }
            || (mode != RestMode::Bed && self.rest_mode(w, false)? != mode)
        {
            return Err("This rest option is unavailable".into());
        }
        if option == 7
            && w.character_statistics().heal_rate == 0
            && w.cache.inventory.character.health_damage > 0
        {
            return Err("Rest cannot heal this character".into());
        }
        if mode == RestMode::Bed {
            w.validate_bed(&mut self.vfs, true)?;
        }
        w.cancel_player_route();
        if w.combat_ready() {
            w.toggle_combat()
        }
        while w.maintained_spells().next().is_some() {
            self.dismiss_spell(w, 0, elapsed)
        }
        w.rest = Some(Rest {
            mode,
            option,
            hours: 0,
            started: w.game_second,
            next: elapsed + Duration::from_millis(50),
        });
        Ok(())
    }
}
