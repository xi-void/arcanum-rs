//! Encounter scheduling, weapon attacks and corpse access over session actor and inventory state.
use super::casting::{Kind, Subject, Target};
use super::*;
use crate::content::world::abilities::Aim;
use crate::content::{player::path, vitals::Channel};
use serde::{Deserialize, Serialize};
mod gear;
mod missile;
mod movement;
mod presentation;
mod resolution;
mod scripts;

type Actor = Option<OriginalId>;

/// CE stores these switches independently. Space changes only the first one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CombatMode {
    pub turn_based: bool,
    pub fast_turn_based: bool,
}

impl CombatMode {
    pub fn choice(self) -> usize {
        if self.turn_based {
            1 + usize::from(self.fast_turn_based)
        } else {
            0
        }
    }

    fn name(self) -> &'static str {
        ["real_time", "turn_based", "fast_turn_based"][self.choice()]
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct Participant {
    actor: Actor,
    ally: bool,
    #[serde(default)]
    faction: i32,
    speed: i32,
    wait_ms: u64,
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub(in crate::content::world) struct Combat {
    pub(in crate::content::world) armed: bool,
    #[serde(flatten)]
    mode: CombatMode,
    party: Vec<Participant>,
    turn: usize,
    round: u64,
    #[serde(skip)]
    elapsed_rounds: u64,
    #[serde(default)]
    deaths: Vec<(OriginalId, Actor)>,
    ap: i32,
    focus: Actor,
    #[serde(default)]
    pub(super) provoked: Actor,
    #[serde(skip)]
    pub(in crate::content::world) request: Actor,
    #[serde(default)]
    shots: Vec<missile::Shot>,
    pools: BTreeMap<String, [i32; 2]>,
    pub(super) prepared: BTreeSet<OriginalId>,
    #[serde(skip)]
    last_tick: Option<Duration>,
    #[serde(skip)]
    next_scan: Duration,
    #[serde(skip)]
    movement_due: bool,
    #[serde(skip)]
    message: String,
    #[serde(skip)]
    history: std::collections::VecDeque<Value>,
    #[serde(skip)]
    poses: BTreeMap<Actor, presentation::Motion>,
    #[serde(skip)]
    pub(super) falls: BTreeSet<OriginalId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    fall_progress: Vec<(OriginalId, u64)>,
    #[serde(skip)]
    settled_bodies: Vec<OriginalId>,
    #[serde(skip)]
    next_ordinary: Duration,
    #[serde(skip)]
    sounds: Vec<(Duration, i32, i32)>,
    #[serde(skip)]
    ordinary_due: Option<Duration>,
    #[serde(skip)]
    indicator: Option<Result<Arc<presentation::Indicator>, String>>,
    #[serde(skip)]
    scan_context: Option<(Tile, usize, usize)>,
}

fn distance(a: Tile, b: Tile) -> u32 {
    a.0.abs_diff(b.0).max(a.1.abs_diff(b.1))
}
fn subject(actor: Actor) -> Subject {
    actor.map_or(Subject::Player, Subject::Mobile)
}
fn actor_at(cache: &SectorCache, key: WorldOrder) -> Actor {
    match cache.target(key)?.action {
        interaction::Action::Npc(id) | interaction::Action::Summon(id) => Some(id),
        _ => None,
    }
}
fn reachable(w: &InteractiveWorld, from: Tile, to: Tile, range: u32) -> bool {
    let cell = |tile| SectorCache::player_cell(&w.cache.entries, &w.cache.door_states, tile);
    distance(from, to) <= range
        && if range <= 1 {
            collision::can_reach_container(from, to, cell)
        } else {
            collision::sight(from, to, false, cell)
        }
}
impl SectorCache {
    // Sector activation also runs when loading a saved player turn. A resting corpse
    // must obtain its final ART frame even if the encounter clock is waiting for input.
    pub(in crate::content::world) fn restore_visible_corpses(&mut self) {
        if !self.inventory.magick.has_corpses() {
            self.inventory.combat.settled_bodies.clear();
            return;
        }
        let combat = &self.inventory.combat;
        self.inventory.combat.settled_bodies = self
            .ordered
            .iter()
            .filter_map(|r| {
                let id = actor_at(self, r.key)?;
                (self.inventory.magick.down(id)
                    && !self.inventory.magick.destroying(id)
                    && !combat.poses.get(&Some(id)).is_some_and(|p| p.dead()))
                .then_some(id)
            })
            .collect();
    }
}

impl Combat {
    pub(super) fn validate(&self) -> Result<(), String> {
        let ids: BTreeSet<_> = self.party.iter().map(|p| p.actor).collect();
        if self
            .fall_progress
            .iter()
            .any(|(_, ms)| *ms > u64::from(u32::MAX))
            || self.shots.iter().any(|s| !s.valid())
            || self.ap < 0
            || ids.len() != self.party.len()
            || (!self.party.is_empty()
                && (!ids.contains(&None) || self.turn >= self.party.len() || !self.armed))
            || self.pools.values().any(|pair| pair.iter().any(|n| *n < 0))
        {
            return Err("Invalid saved combat encounter".into());
        }
        Ok(())
    }
    pub(super) fn log(&mut self, event: Value) {
        if self.history.len() == 32 {
            self.history.pop_front();
        }
        self.history.push_back(event);
    }
    pub(in crate::content::world) fn leave(&mut self) {
        self.movement_due = false;
        self.armed = false;
        self.provoked = None;
        self.focus = None;
        self.request = None;
        self.party.clear();
        self.turn = 0;
        self.ap = 0;
        self.round = 0;
    }
    pub(super) fn rescan(&mut self) {
        self.scan_context = None;
        self.next_scan = Duration::ZERO;
    }
    pub(super) fn new_map(&mut self) {
        self.leave();
        self.poses.clear();
        self.settled_bodies.clear();
        self.sounds.clear();
        self.falls.clear();
        self.fall_progress.clear();
        self.next_ordinary = Duration::ZERO;
        self.ordinary_due = None;
        self.shots.clear();
        self.deaths.clear();
        self.scan_context = None;
        self.next_scan = Duration::ZERO;
        self.message.clear();
    }
    pub(in crate::content::world) fn timed(&self) -> bool {
        self.mode.turn_based && !self.party.is_empty()
    }
    // CE sub_4B8040 permits the current actor to skip turning and gait.
    // Attacks, spell action frames and projectile flights use their own clocks.
    pub(in crate::content::world) fn instant_movement(&self, actor: Actor) -> bool {
        self.timed() && self.mode.fast_turn_based && self.party[self.turn].actor == actor
    }

    pub(in crate::content::world) fn check(
        &self,
        actor: Actor,
        cost: i32,
        fatigue: i64,
    ) -> Result<(), String> {
        if self.timed() {
            if self.party[self.turn].actor != actor {
                return Err("Wait for your turn".into());
            }
            if self.ap < cost && !(actor.is_none() && self.ap > 0 && fatigue > 1) {
                return Err(format!("Requires {cost} AP ({} available)", self.ap));
            }
        }
        Ok(())
    }
    pub(in crate::content::world) fn spend(&mut self, cost: i32) {
        if self.timed() {
            self.ap = self.ap.saturating_sub(cost).max(0)
        }
    }
    pub(super) fn active(&self) -> bool {
        !self.party.is_empty()
    }
    fn next_turn(&mut self) {
        if !self.timed() {
            return;
        }
        self.turn = (self.turn + 1) % self.party.len();
        if self.turn == 0 {
            self.elapsed_rounds = self.elapsed_rounds.saturating_add(1);
            self.round = self.round.saturating_add(1)
        }
        self.begin_subturn();
    }
    fn begin_subturn(&mut self) {
        self.request = None;
        self.ap = self.party[self.turn].speed.max(5);
        self.log(json!({"event":"turn_started","actor":subject(self.party[self.turn].actor),"round":self.round,"ap":self.ap}));
    }
    fn join(
        &mut self,
        id: OriginalId,
        speed: i32,
        ally: bool,
        faction: i32,
        pc_speed: i32,
        initiator: Actor,
    ) {
        let fresh = self.party.is_empty();
        if fresh {
            self.armed = true;
            self.party.push(Participant {
                actor: None,
                ally: true,
                faction: 0,
                speed: pc_speed,
                wait_ms: 0,
            });
            self.round = 1;
        }
        if !self.party.iter().any(|p| p.actor == Some(id)) {
            // Stock sort_combat_list places all NPCs before the player. Insertion
            // must preserve the current actor, including midway through a round.
            let index = self
                .party
                .iter()
                .position(|p| p.actor.is_none())
                .unwrap_or(self.party.len());
            if !fresh && index <= self.turn {
                self.turn += 1;
            }
            self.party.insert(
                index,
                Participant {
                    actor: Some(id),
                    ally,
                    faction,
                    speed,
                    wait_ms: 350,
                },
            );
        }
        if fresh {
            // CE puts NPCs before the local player in its turn list and gives an
            // opening attacker the active subturn when entering combat.
            self.turn = self
                .party
                .iter()
                .position(|p| p.actor == initiator)
                .unwrap_or(0);
            self.ap = self.party[self.turn].speed.max(5);
            self.log(json!({"event":"engaged","initiator":initiator,"target":id}));
        }
    }
}

impl InteractiveWorld {
    pub fn set_difficulty(&mut self, value: crate::content::Difficulty) {
        self.cache.inventory.difficulty = value;
    }
    pub fn combat_mode(&self) -> CombatMode {
        self.cache.inventory.combat.mode
    }

    pub fn set_combat_mode(&mut self, mode: CombatMode) -> bool {
        let previous = self.combat_mode();
        if mode == previous {
            return true;
        }
        if previous.turn_based != mode.turn_based {
            // CE cannot interrupt a priority-four casting goal on the player.
            // Released weapon missiles already own their AP and ammunition.
            if self.ability.casting.is_some() {
                self.cache.inventory.combat.message =
                    "Finish the current action before switching combat mode".into();
                return false;
            }
            self.cancel_route_keep_spell();
        }
        let c = &mut self.cache.inventory.combat;
        c.mode = mode;
        if !previous.turn_based && c.timed() {
            // CE combat_turn_based_start begins round one at the head of the
            // initiative list, even when the encounter began in real time.
            c.round = 1;
            c.turn = 0;
            c.begin_subturn();
        }
        c.message = [
            "Real-time combat",
            "Turn-based combat",
            "Fast turn-based combat",
        ][mode.choice()]
        .into();
        true
    }
    pub fn set_turn_based(&mut self, enabled: bool) {
        self.set_combat_mode(CombatMode {
            turn_based: enabled,
            ..self.combat_mode()
        });
    }

    pub fn toggle_turn_based(&mut self) {
        let mode = self.combat_mode();
        if mode.turn_based || !self.unconscious() {
            self.set_turn_based(!mode.turn_based);
        }
    }

    pub fn combat_report(&self) -> Value {
        let c = &self.cache.inventory.combat;
        json!({"difficulty":self.cache.inventory.difficulty,"armed":c.armed,"mode":c.mode.name(),"fast_turn_based":c.mode.fast_turn_based,"participants":c.party,"round":c.round,"turn":c.party.get(c.turn).map(|p|subject(p.actor)), "ap":c.ap,"target":c.focus.map(OriginalId::key),"message":c.message,"history":c.history,"poses":c.pose_report(self.cache.effect_time),"projectiles":c.shots,"experience_pools":c.pools})
    }
    pub fn take_elapsed_combat_rounds(&mut self) -> u64 {
        std::mem::take(&mut self.cache.inventory.combat.elapsed_rounds)
    }
    pub fn combat_ap(&self) -> Option<i32> {
        let c = &self.cache.inventory.combat;
        c.timed().then(|| {
            if c.party[c.turn].actor.is_none() {
                c.ap
            } else {
                0
            }
        })
    }
    pub fn combat_blocks_travel(&self) -> bool {
        !self.cache.inventory.combat.party.is_empty() || self.unconscious()
    }
    pub fn combat_status(&self) -> String {
        let c = &self.cache.inventory.combat;
        if c.timed() {
            format!(
                "{}  {} AP  {}",
                if c.party[c.turn].actor.is_none() {
                    "Your turn"
                } else {
                    "Enemy turn"
                },
                c.ap,
                c.message
            )
        } else {
            c.message.clone()
        }
    }
    pub fn toggle_combat(&mut self) {
        if self.unconscious() {
            return;
        }
        self.cancel_route_keep_spell();
        let c = &mut self.cache.inventory.combat;
        if c.party.is_empty() {
            c.armed = !c.armed;
            c.message = if c.armed {
                "Combat ready"
            } else {
                "Combat ended"
            }
            .into();
        } else {
            c.message = "You cannot exit combat-mode when under attack".into();
        }
    }
    fn turn_action_pending(&self, actor: Actor) -> bool {
        self.cache
            .inventory
            .combat
            .shots
            .iter()
            .any(|s| s.from == actor)
            || self.cache.inventory.magick.animation_callback_pending()
            || (actor.is_none()
                && (self.ability.casting.is_some()
                    || self.cache.player.as_ref().is_some_and(Player::in_stride)))
    }

    pub fn end_combat_turn(&mut self) -> bool {
        let c = &self.cache.inventory.combat;
        if !c.timed() || c.party[c.turn].actor.is_some() || self.ability.casting.is_some() {
            return false;
        }
        if self.turn_action_pending(None) {
            // Yield the remaining points now. The scheduler hands off after the
            // paid stride, missile impact or spell callback completes.
            self.cache.inventory.combat.ap = 0;
        } else {
            self.cache.inventory.combat.next_turn();
        }
        self.cancel_route_keep_spell();
        true
    }
    pub fn combat_wait(&self, now: Duration) -> Option<Duration> {
        let c = &self.cache.inventory.combat;
        if c.movement_due {
            return Some(Duration::ZERO);
        }
        let tick = c
            .last_tick
            .map_or(Duration::ZERO, |last| last + Duration::from_millis(80));
        if !c.settled_bodies.is_empty()
            || !c.sounds.is_empty()
            || !c.deaths.is_empty()
            || !c.shots.is_empty()
            || c.request.is_some()
            || c.provoked.is_some()
        {
            return Some(tick.saturating_sub(now));
        }
        if self.cache.inventory.magick.animation_callback_pending()
            || !c.falls.is_empty()
            || c.poses.values().any(|p| p.awaiting_frame(now))
        {
            return Some((self.cache.effect_time + Duration::from_millis(40)).saturating_sub(now));
        }
        if self.dialogue.is_some() || self.trade.is_some() {
            return None;
        }
        if !c.party.is_empty() && (!c.timed() || c.party[c.turn].actor.is_some() || c.ap == 0) {
            return Some(tick.saturating_sub(now));
        }
        if c.active() {
            None
        } else {
            c.ordinary_due.map(|due| due.saturating_sub(now))
        }
    }
    pub(in crate::content::world) fn combat_click(&mut self, key: WorldOrder) -> bool {
        if !self.cache.inventory.combat.armed {
            return false;
        }
        let Some(id) = actor_at(&self.cache, key) else {
            return false;
        };
        if self.corpse_tile(id).is_some() {
            return false;
        }
        self.cache.inventory.combat.request = Some(id);
        true
    }
    pub(in crate::content::world) fn corpse_tile(&self, id: OriginalId) -> Option<Tile> {
        let Aim::Object(key) = InventoryReader::subject_aim(self, &Subject::Mobile(id))? else {
            return None;
        };
        if self.cache.inventory.magick.destroying(id) || self.cache.inventory.magick.has_spawn(id) {
            return None;
        }
        if let Some((r, tile)) = self.cache.npc(key) {
            let (_, s) =
                self.cache
                    .inventory
                    .npc_vitals(id, &r.prototype.as_ref()?.defaults, &r.instance());
            return (s.current_health <= 0).then_some(tile);
        }
        self.cache
            .inventory
            .magick
            .down(id)
            .then(|| self.cache.target(key).map(|t| t.tile))
            .flatten()
    }
    pub(super) fn check_corpse(&self, id: OriginalId) -> Result<(), String> {
        if self
            .cache
            .inventory
            .combat
            .falling(id, self.cache.effect_time)
        {
            return Err("The creature is still falling".into());
        }
        let tile = self
            .corpse_tile(id)
            .ok_or("Only a dead actor can be looted")?;
        let pc = self
            .cache
            .player
            .as_ref()
            .filter(|p| !p.moving())
            .ok_or("Stand beside the corpse")?;
        if !reachable(self, pc.tile(), tile, 1) || self.unconscious() {
            return Err("Corpse is out of reach".into());
        }
        Ok(())
    }
}

impl InventoryReader {
    pub(in crate::content::world) fn script_kill(
        &mut self,
        w: &mut InteractiveWorld,
        id: OriginalId,
        now: Duration,
    ) -> Result<(), String> {
        let mut target = self.fighter(w, Some(id))?;
        let before = target.hp;
        if before >= target.maximum {
            return Ok(());
        }
        w.cache
            .inventory
            .change_vitals(&mut target, Channel::Normal, 32000 - before);
        w.cache.inventory.magick.set_death_animation(id, 7);
        self.downed_pose(w, Some(id), now, true);
        // SAT_KILL invokes the dying attachment with a null killer.
        self.dispatch_dying_script(w, id, None);
        w.cache.inventory.revision += 1;
        Ok(())
    }

    fn fighter(&mut self, w: &InteractiveWorld, id: Actor) -> Result<Target, String> {
        let aim = Self::subject_aim(w, &subject(id)).ok_or("Actor is outside the active map")?;
        let t = self.target(w, aim)?;
        if t.kind != Kind::Actor {
            return Err("Select a living actor".into());
        }
        Ok(t)
    }
    pub(in crate::content::world::inventory) fn engage(
        &mut self,
        w: &mut InteractiveWorld,
        id: OriginalId,
        ally: bool,
        initiator: Actor,
    ) -> Result<(), String> {
        self.prepare_gear(w, id)?;
        let t = self.fighter(w, Some(id))?;
        if t.hp >= t.maximum {
            return Err("Target is dead".into());
        }
        let speed = t.stats.as_ref().and_then(|s| s.speed).unwrap_or(5);
        let faction = t
            .key
            .and_then(|k| w.cache.npc(k))
            .and_then(|(r, _)| {
                r.instance()
                    .field(140)
                    .and_then(FieldValue::scalar)
                    .map(|v| v as i32)
                    .or_else(|| r.prototype.as_ref().map(|p| p.defaults.faction))
            })
            .unwrap_or(0);
        let pc = w.character_statistics().speed.unwrap_or(5);
        w.cache
            .inventory
            .combat
            .join(id, speed, ally, faction, pc, initiator);
        Ok(())
    }
    fn scan_combat(&mut self, w: &mut InteractiveWorld, now: Duration, at: Tile) {
        let c = &mut w.cache.inventory.combat;
        let context = (at, w.cache.ordered.len(), c.party.len());
        if now < c.next_scan && !(c.timed() && c.scan_context != Some(context)) {
            return;
        }
        c.next_scan = now + Duration::from_millis(300);
        if c.party.is_empty() && c.scan_context == Some(context) {
            return;
        }
        c.scan_context = Some(context);
        if w.character_statistics().current_health <= 0 {
            return;
        }
        let invisible = w.player_flags().0 & 1 != 0;
        let perception_range = (w.character_statistics().attributes[5] / 2 + 5).max(10) as u32;
        let mut candidates = Vec::new();
        for raster in &w.cache.ordered {
            let key = &raster.key;
            let Some((r, tile)) = w.cache.npc(*key) else {
                continue;
            };
            let id = r.id;
            if distance(at, tile) > perception_range
                || w.cache
                    .inventory
                    .combat
                    .party
                    .iter()
                    .any(|p| p.actor == Some(id))
            {
                continue;
            }
            let Some(d) = r.prototype.as_ref().map(|p| &p.defaults) else {
                continue;
            };
            let o = r.instance();
            let flags = o
                .field(128)
                .and_then(FieldValue::scalar)
                .or(d.npc_flags)
                .unwrap_or(0);
            let (character, s) = w.cache.inventory.npc_vitals(id, d, &o);
            let spell = character.spell_flags;
            let ally = spell & 0x08000000 != 0;
            if s.current_health <= 0 {
                // A placed flat corpse already supplies its exact ART pose and frame.
                if o.appearance().flags.or(d.appearance.flags).unwrap_or(0) & 4 == 0 {
                    candidates.push((id, false, true));
                }
                continue;
            }
            let faction = o
                .field(140)
                .and_then(FieldValue::scalar)
                .map_or(d.faction, |n| n as i32);
            let protects = faction != 0
                && w.cache
                    .inventory
                    .combat
                    .party
                    .iter()
                    .any(|p| !p.ally && p.faction == faction);
            if !ally
                && ((!protects && flags & 0x900 != 0x100)
                    || spell & 0x00800000 != 0
                    || invisible && spell & 0x200 == 0
                    || distance(at, tile) > s.attributes[5].max(1) as u32
                    || !reachable(w, at, tile, perception_range))
            {
                continue;
            }
            if !ally || !w.cache.inventory.combat.party.is_empty() {
                candidates.push((id, ally, false))
            }
        }
        for (id, ally, dead) in candidates {
            if dead {
                self.downed_pose(w, Some(id), now, true);
                continue;
            }
            if let Err(e) = self.engage(w, id, ally, Some(id)) {
                w.cache.inventory.combat.message = e
            }
        }
        if !w.cache.inventory.combat.party.is_empty() {
            for id in w
                .cache
                .inventory
                .magick
                .combat_summons(&w.cache.inventory.state, &w.map, at)
            {
                if !w
                    .cache
                    .inventory
                    .combat
                    .party
                    .iter()
                    .any(|p| p.actor == Some(id))
                {
                    let _ = self.engage(w, id, true, None);
                }
            }
        }
    }
    pub fn tick_combat(&mut self, w: &mut InteractiveWorld, now: Duration) -> bool {
        let mut changed = self.tick_combat_once(w, now);
        // CE's AGbeginAnimMove finishes the affordable path before presentation.
        // Re-enter the ordinary scheduler at the same instant after each paid NPC
        // step, so occupancy, target selection and AP are checked at every tile.
        while w.cache.inventory.combat.movement_due
            && !w.cache.inventory.combat.instant_movement(None)
            && w.cache.inventory.combat.ap > 0
        {
            changed |= self.tick_combat_once(w, now);
        }
        changed
    }

    pub fn continue_fast_player_route(&mut self, w: &mut InteractiveWorld, now: Duration) -> bool {
        let c = &w.cache.inventory.combat;
        if w.transition.is_some() || !c.movement_due || !c.instant_movement(None) || c.ap == 0 {
            return false;
        }
        self.tick_combat(w, now);
        w.cache.inventory.combat.instant_movement(None)
            && w.cache
                .player
                .as_ref()
                .is_some_and(Player::has_planned_steps)
    }

    fn tick_combat_once(&mut self, w: &mut InteractiveWorld, now: Duration) -> bool {
        let mut ready = Vec::new();
        w.cache.inventory.combat.sounds.retain(|(due, s, f)| {
            if *due <= now {
                ready.push((*s, *f));
                false
            } else {
                true
            }
        });
        for (s, f) in ready {
            w.queue_sound(SoundEvent::EffectVariant {
                preferred: s,
                fallback: f,
            });
        }
        let redraw_motion = w
            .cache
            .inventory
            .combat
            .poses
            .values()
            .any(|p| p.needs_redraw(w.cache.inventory.combat.last_tick, now));
        let previous = w.cache.inventory.combat.last_tick;
        let movement_due = std::mem::take(&mut w.cache.inventory.combat.movement_due);
        if !movement_due
            && previous.is_some_and(|n| now.saturating_sub(n) < Duration::from_millis(60))
            && w.cache.inventory.combat.request.is_none()
        {
            return redraw_motion;
        }
        let Some(pc) = w.cache.player.as_ref() else {
            return false;
        };
        let at = pc.tile();
        if let Some(id) = w.cache.inventory.combat.provoked.take() {
            w.close_dialogue("combat");
            if let Err(e) = self.engage(w, id, false, Some(id)) {
                w.cache.inventory.combat.message = e
            }
        }
        let mut settled = false;
        for p in w.cache.inventory.combat.poses.values_mut() {
            settled |= p.settle(now)
        }
        if settled {
            w.cache.activate(&w.cache.active.clone());
        }
        for (id, progress) in std::mem::take(&mut w.cache.inventory.combat.fall_progress) {
            if Self::subject_aim(w, &Subject::Mobile(id)).is_some() {
                self.start_fall(
                    w,
                    Some(id),
                    now,
                    true,
                    presentation::FallStart::Resume(Duration::from_millis(progress)),
                );
            } else {
                w.cache.inventory.combat.fall_progress.push((id, progress))
            }
        }
        let falls = std::mem::take(&mut w.cache.inventory.combat.falls);
        for id in falls {
            self.start_fall(w, Some(id), now, true, presentation::FallStart::New);
        }
        let settled_bodies = std::mem::take(&mut w.cache.inventory.combat.settled_bodies);
        let restored = !settled_bodies.is_empty();
        for id in settled_bodies {
            self.downed_pose(w, Some(id), now, true);
        }
        self.run_dying_scripts(w);
        let cleanup = self.finish_bodies(w, now) || settled || restored;
        if w.dialogue.is_some() || w.trade.is_some() {
            w.cache.inventory.combat.last_tick = Some(now);
            return redraw_motion || cleanup;
        }
        self.scan_combat(w, now, at);
        let c = &mut w.cache.inventory.combat;
        let dt = c.last_tick.replace(now).map_or(0, |last| {
            now.saturating_sub(last).as_millis().min(u64::MAX as u128) as u64
        });
        for p in &mut c.party {
            p.wait_ms = p.wait_ms.saturating_sub(dt)
        }
        c.poses.retain(|actor, p| {
            if p.dead() {
                return actor.is_none_or(|id| {
                    !w.cache.inventory.magick.revived_or_decayed(id)
                        && (id.kind != u16::MAX || w.cache.inventory.magick.owns_spawn(&w.map, id))
                });
            }
            p.hold() || p.animating(now) || p.adjoining_step(now)
        });
        let request = c.request.take();
        let impact = self.advance_shots(w, dt, now);
        if let Some(id) = request
            && let Err(e) = self.order_attack(w, id, now)
        {
            let c = &mut w.cache.inventory.combat;
            c.focus = None;
            c.message = e.clone();
            c.log(json!({"event":"rejected","reason":e}));
        }
        if w.combat_ap().is_some() && w.cache.inventory.combat.indicator.is_none() {
            let loaded = presentation::Indicator::load(&mut self.vfs, &mut self.frames);
            if let Err(reason) = &loaded {
                w.cache
                    .inventory
                    .combat
                    .log(json!({"event":"combat_indicator_unavailable","reason":reason}));
            }
            w.cache.inventory.combat.indicator = Some(loaded);
        }
        let c = &w.cache.inventory.combat;
        if c.party.is_empty() {
            if w.character_statistics().current_health <= 0 {
                self.downed_pose(w, None, now, true)
            }
            let moved = self.ordinary_movement(w, now, at);
            return redraw_motion || cleanup || moved || impact || request.is_some();
        }
        let was_turn = (c.round, c.turn, c.party.len());
        let old_revision = w.cache.inventory.revision;
        let mut acted = false;
        let order = c.party.iter().map(|p| p.actor).collect::<Vec<_>>();
        for actor in order {
            let Ok(t) = self.fighter(w, actor) else {
                self.remove_fighter(w, actor);
                continue;
            };
            if t.hp >= t.maximum {
                self.downed_pose(w, actor, now, true);
                if actor.is_none() {
                    w.cancel_player_route();
                    w.cache.inventory.combat.leave();
                    w.cache.inventory.combat.message = "You have died".into();
                    return true;
                }
                w.cache.activate(&w.cache.active.clone());
                self.remove_fighter(w, actor);
                continue;
            }
            if let Some(p) = w
                .cache
                .inventory
                .combat
                .party
                .iter_mut()
                .find(|p| p.actor == actor)
            {
                p.speed = t.stats.as_ref().and_then(|s| s.speed).unwrap_or(5);
                if p.ally && actor.is_some_and(|id| id.kind == 2) && t.spell & 0x08000000 == 0 {
                    self.remove_fighter(w, actor);
                    continue;
                }
            }
            let unable = t.unable();
            if unable && t.stats.as_ref().is_some_and(|s| s.current_fatigue <= 0) {
                self.downed_pose(w, actor, now, false);
            } else if !unable
                && w.cache
                    .inventory
                    .combat
                    .poses
                    .get(&actor)
                    .is_some_and(|p| p.hold())
            {
                w.cache.inventory.combat.poses.remove(&actor);
            }
            let c = &w.cache.inventory.combat;
            if c.timed() && c.party[c.turn].actor != actor {
                continue;
            }
            if (c.timed() && w.turn_action_pending(actor)
                || actor.is_none() && w.ability.casting.is_some())
                && !unable
            {
                continue;
            }
            if c.timed() && (c.ap <= 0 || unable) {
                w.cache.inventory.combat.next_turn();
                continue;
            }
            let Some(id) = actor else { continue };
            if distance(at, t.tile) > 24 {
                self.remove_fighter(w, actor);
                continue;
            }
            let member = c.party.iter().find(|p| p.actor == actor).unwrap();
            if member.wait_ms > 0 || unable {
                continue;
            }
            let ally = member.ally || t.spell & 0x08000000 != 0;
            let target = c
                .party
                .iter()
                .filter(|p| {
                    let other_ally = p.ally
                        || w.cache.inventory.magick.flags(&subject(p.actor)).0 & 0x08000000 != 0;
                    other_ally != ally && p.actor != actor
                })
                .filter_map(|p| {
                    let tile = match p.actor {
                        None => at,
                        Some(id) => match Self::subject_aim(w, &Subject::Mobile(id))? {
                            Aim::Object(k) => w.cache.target(k)?.tile,
                            _ => return None,
                        },
                    };
                    Some((distance(t.tile, tile), p.actor))
                })
                .min_by_key(|v| v.0)
                .map(|v| v.1);
            acted = true;
            let result = target
                .ok_or_else(|| "No hostile target".to_owned())
                .and_then(|victim| self.act(w, actor, victim, now));
            if let Err(e) = result {
                let c = &mut w.cache.inventory.combat;
                if c.timed() {
                    c.next_turn()
                }
                if let Some(p) = c.party.iter_mut().find(|p| p.actor == actor) {
                    p.wait_ms = 800
                }
                if target.is_some() {
                    c.log(json!({"event":"ai_wait","actor":id,"reason":e}));
                }
            }
        }
        let c = &mut w.cache.inventory.combat;
        if !c.party.iter().any(|p| {
            p.actor.is_some()
                && !p.ally
                && w.cache.inventory.magick.flags(&subject(p.actor)).0 & 0x08000000 == 0
        }) {
            c.leave();
            c.message = "Combat ended".into();
        }
        redraw_motion
            || cleanup
            || impact
            || request.is_some()
            || acted
            || old_revision != w.cache.inventory.revision
            || (c.round, c.turn, c.party.len()) != was_turn
    }
    fn remove_fighter(&self, w: &mut InteractiveWorld, id: Actor) {
        let c = &mut w.cache.inventory.combat;
        if let Some(i) = c.party.iter().position(|p| p.actor == id) {
            c.party.remove(i);
            if c.focus == id {
                c.focus = None;
                c.request = None
            }
            if i < c.turn {
                c.turn -= 1
            } else if i == c.turn && !c.party.is_empty() {
                c.turn %= c.party.len();
                c.ap = c.party[c.turn].speed.max(5)
            }
        }
    }
    fn order_attack(
        &mut self,
        w: &mut InteractiveWorld,
        id: OriginalId,
        now: Duration,
    ) -> Result<(), String> {
        let t = self.fighter(w, Some(id))?;
        if t.hp >= t.maximum {
            return Err("Target is dead".into());
        }
        self.act(w, None, Some(id), now)
    }
    fn act(
        &mut self,
        w: &mut InteractiveWorld,
        source: Actor,
        victim: Actor,
        now: Duration,
    ) -> Result<(), String> {
        let mut attacker = self.fighter(w, source)?;
        let mut target = self.fighter(w, victim)?;
        if target.hp >= target.maximum || target.object_flags & 0x00400003 != 0 {
            return Err("Target is unavailable".into());
        }
        if attacker.unable() {
            return Err("Attacker is incapacitated".into());
        }
        let weapon = w.cache.inventory.weapon(source, &attacker);
        let c = &w.cache.inventory.combat;
        let fatigue = attacker.stats.as_ref().unwrap().current_fatigue;
        c.check(source, 1, fatigue)?;
        if c.party.iter().any(|p| p.actor == source && p.wait_ms > 0) && !c.timed() {
            if source.is_none() {
                w.cache.inventory.combat.request = victim;
            }
            return Ok(());
        }
        let flee = source.is_some() && attacker.critter & 0x10 != 0;
        let approach = flee || !reachable(w, attacker.tile, target.tile, weapon.range);
        let cost = if approach {
            if source.is_some() { 2 } else { 1 }
        } else {
            weapon.cost
        };
        c.check(source, cost, fatigue)?;
        let ammo = self.ammo_plan(w, source, &weapon)?;
        let direction = if approach {
            if attacker.spell & 0x01000000 != 0 {
                return Err("Entangle prevents movement".into());
            }
            Some(
                combat_step(w, attacker.tile, target.tile, flee)
                    .ok_or("No reachable attack position")?,
            )
        } else {
            None
        };
        if source.is_none() {
            let id = victim.ok_or("Select an opponent")?;
            if !w
                .cache
                .inventory
                .combat
                .party
                .iter()
                .any(|p| p.actor == victim)
            {
                self.engage(w, id, false, None)?;
                target = self.fighter(w, victim)?;
            }
            w.cache.inventory.combat.focus = victim;
            w.cache.inventory.combat.check(source, cost, fatigue)?;
        }
        if let Some((dir, to)) = direction {
            if let Some(id) = source {
                let duration = self.walk_actor(w, &attacker, id, dir, to, now)?;
                if let Some(p) = w
                    .cache
                    .inventory
                    .combat
                    .party
                    .iter_mut()
                    .find(|p| p.actor == source)
                {
                    p.wait_ms = duration.as_millis().min(u128::from(u64::MAX)) as u64;
                }
                w.cache
                    .inventory
                    .pay_movement_ap(source, attacker.tile, to, cost);
            } else {
                let p = w.cache.player.as_mut().unwrap();
                if !p.moving() {
                    p.follow_route([dir].into(), true);
                }
                w.cache.inventory.combat.request = victim;
            }
            if source.is_none() {
                self.delay(w, source, 220);
            }
            return Ok(());
        }
        if source.is_none() && w.cache.player.as_ref().is_some_and(Player::moving) {
            w.cache.inventory.combat.request = victim;
            return Ok(());
        }
        let facing = npc::facing(attacker.tile, target.tile).unwrap_or(4);
        if source.is_none() {
            w.cache.player.as_mut().unwrap().set_facing(facing)
        }
        self.strike_pose(w, source, weapon.art, facing, now);
        let wait = 3000 * cost as u64
            / attacker
                .stats
                .as_ref()
                .and_then(|s| s.speed)
                .unwrap_or(8)
                .max(1) as u64;
        let inv = &mut w.cache.inventory;
        inv.pay_combat_ap(source, cost);
        let ammo_used = inv.consume_ammo(ammo);
        if weapon.missile.is_some() && weapon.range > 1 {
            self.release(w, &attacker, &target, weapon, ammo_used, now);
        } else {
            let event = inv.strike(&mut attacker, &mut target, &weapon, ammo_used);
            self.attack_feedback(w, &attacker, &target, &weapon, event, now);
        }
        self.delay(w, source, wait);
        Ok(())
    }
    fn delay(&self, w: &mut InteractiveWorld, actor: Actor, ms: u64) {
        let c = &mut w.cache.inventory.combat;
        let turn = c.timed();
        if let Some(p) = c.party.iter_mut().find(|p| p.actor == actor) {
            p.wait_ms = if turn { 240 } else { ms }
        }
    }
}

fn combat_step(w: &InteractiveWorld, from: Tile, dest: Tile, flee: bool) -> Option<(u8, Tile)> {
    let pc = w.cache.player.as_ref()?.tile();
    let cell = |tile| {
        let c = SectorCache::player_cell(&w.cache.entries, &w.cache.door_states, tile)?;
        Some(if tile == pc && from != pc {
            c.occupied()
        } else {
            c
        })
    };
    if flee {
        return (0..8)
            .filter_map(|d| Some((d, collision::step(from, d, cell)?)))
            .filter(|(_, t)| distance(*t, dest) > distance(from, dest))
            .max_by_key(|(_, t)| distance(*t, dest));
    }
    approach_step(from, dest, cell)
}

fn approach_step(
    from: Tile,
    dest: Tile,
    cell: impl Fn(Tile) -> Option<collision::Cell> + Copy,
) -> Option<(u8, Tile)> {
    // Plan up to the actor using interaction reach on the final edge. The actor
    // still occupies its tile during movement and diagonal corner checks.
    let route = path::find_with(from, dest, |tile, dir| {
        let (dx, dy) = collision::DIRECTIONS[usize::from(dir)];
        let next = (
            tile.0.checked_add_signed(dx)?,
            tile.1.checked_add_signed(dy)?,
        );
        if next == dest && collision::can_reach_container(tile, dest, cell) {
            return Some((dest, 0));
        }
        collision::step(tile, dir, cell).map(|t| (t, 0))
    })?;
    let dir = *route.front()?;
    Some((dir, collision::step(from, dir, cell)?))
}
impl Target {
    fn unable(&self) -> bool {
        self.hp >= self.maximum
            || self.critter & 0x60 != 0
            || self.spell & 0x10000 != 0
            || self.critter & 4 == 0 && self.stats.as_ref().is_some_and(|s| s.current_fatigue <= 0)
    }
}

impl Session {
    pub(in crate::content::world) fn pay_movement_ap(
        &mut self,
        actor: Actor,
        from: Tile,
        to: Tile,
        cost: i32,
    ) {
        if self.combat.timed() {
            self.pay_combat_ap(actor, cost);
            let instant = self.combat.instant_movement(actor);
            self.combat.movement_due = instant;
            self.combat.log(json!({"event":"move","actor":subject(actor),"from":from,"to":to,"cost":cost,"ap":self.combat.ap,"instant":instant}));
        }
    }

    pub(in crate::content::world) fn pay_combat_ap(&mut self, actor: Actor, cost: i32) {
        if self.combat.timed() && actor.is_none() && self.combat.ap < cost {
            self.character.fatigue_damage = self.character.fatigue_damage.saturating_add(2);
            self.character
                .recover_at
                .get_or_insert(self.game_second.saturating_add(80));
        }
        self.combat.spend(cost);
    }
    pub(super) fn note_death(&mut self, target: &Target, before: i32, source: Actor) {
        if before < target.maximum
            && target.hp >= target.maximum
            && let Subject::Mobile(id) = target.subject
            && (id.kind == 2 || id.kind == u16::MAX && !self.magick.has_spawn(id))
        {
            self.combat.deaths.push((id, source));
        }
    }
    fn record_attack(&mut self, event: Value) {
        self.combat.message = format!(
            "{} · {} HP damage",
            event["outcome"].as_str().unwrap_or("Attack"),
            if event["outcome"] == "Critical failure" {
                &event["attacker_health_damage"]
            } else {
                &event["health_damage"]
            }
        );
        self.combat.log(event);
        self.revision += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn opponent() -> OriginalId {
        OriginalId {
            kind: 2,
            payload: [3; 16],
        }
    }
    #[test]
    fn approach_reaches_an_occupied_enemy_without_stepping_on_it() {
        let dest = (3, 1);
        let cell = |tile: (u32, u32)| {
            (tile.0 < 5 && tile.1 < 3).then(|| collision::test_cell(tile == dest))
        };
        assert_eq!(approach_step((1, 1), dest, cell), Some((5, (2, 1))));
        assert!(collision::can_reach_container((2, 1), dest, cell));
        assert!(approach_step((2, 1), dest, cell).is_none());
    }
    #[test]
    fn fast_gait_belongs_only_to_the_active_actor_and_survives_reload() {
        let mut combat = Combat::default();
        let mut legacy = serde_json::to_value(&combat).unwrap();
        legacy["turn_based"] = json!(true);
        legacy.as_object_mut().unwrap().remove("fast_turn_based");
        combat = serde_json::from_value(legacy).unwrap();
        assert_eq!(combat.mode.name(), "turn_based");

        let foe = Some(opponent());
        combat.mode.fast_turn_based = true;
        assert!(!combat.instant_movement(None));
        combat.join(opponent(), 7, false, 0, 9, None);
        combat.spend(2);
        assert!(combat.instant_movement(None));
        assert!(!combat.instant_movement(foe));
        combat.mode.turn_based = false;
        let mut restored: Combat =
            serde_json::from_value(serde_json::to_value(&combat).unwrap()).unwrap();
        assert_eq!(restored.mode.name(), "real_time");
        assert!(!restored.instant_movement(None));
        restored.mode.turn_based = true;
        assert_eq!((restored.mode.name(), restored.ap), ("fast_turn_based", 7));
        restored.next_turn();
        assert!(restored.instant_movement(foe));
        assert!(!restored.instant_movement(None));
        restored.leave();
        assert!(!restored.instant_movement(foe));
    }

    #[test]
    fn opening_actor_turn_reload_and_fatigue_overspend() {
        let mut inv = Session::default();
        let c = &mut inv.combat;
        let id = opponent();
        let foe = Some(id);
        c.mode.turn_based = true;
        c.join(id, 7, false, 0, 9, None);
        assert_eq!(c.party[c.turn].actor, None);
        assert!(c.check(foe, 1, 50).is_err());
        inv.pay_combat_ap(None, 8);
        assert!(inv.combat.check(None, 5, 1).is_err());
        inv.combat.check(None, 5, 2).unwrap();
        inv.pay_combat_ap(None, 5);
        assert_eq!((inv.combat.ap, inv.character.fatigue_damage), (0, 2));
        assert!(inv.combat.check(None, 1, 50).is_err());
        let mut restored: Combat =
            serde_json::from_value(serde_json::to_value(&inv.combat).unwrap()).unwrap();
        restored.validate().unwrap();
        restored.next_turn();
        assert_eq!(restored.party[restored.turn].actor, foe);
        assert_eq!(restored.ap, 7);
        assert!(restored.check(foe, 8, 50).is_err());
        restored.leave();
        assert!(!restored.armed && !restored.timed() && restored.ap == 0);
    }
    #[test]
    fn reinforcements_preserve_the_current_subturn_and_act_before_the_next_player_turn() {
        let mut c = Combat {
            mode: CombatMode {
                turn_based: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let first = opponent();
        let second = OriginalId {
            payload: [4; 16],
            ..first
        };
        c.join(first, 8, false, 0, 12, None);
        c.spend(5);
        c.join(second, 9, false, 0, 12, Some(second));
        assert_eq!((c.party[c.turn].actor, c.ap), (None, 7));
        for (actor, ap) in [(Some(first), 8), (Some(second), 9), (None, 12)] {
            c.next_turn();
            assert_eq!((c.party[c.turn].actor, c.ap), (actor, ap));
        }
        assert_eq!(c.round, 2);
    }

    #[test]
    fn invalid_saved_turn_or_duplicate_actor_is_rejected() {
        let mut c = Combat::default();
        c.join(opponent(), 8, false, 0, 8, Some(opponent()));
        assert_eq!(c.party[c.turn].actor, Some(opponent()));
        c.turn = c.party.len();
        assert!(c.validate().is_err());
        c.turn = 0;
        c.party.push(c.party[0].clone());
        assert!(c.validate().is_err());
    }
}
