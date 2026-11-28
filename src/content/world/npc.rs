//! World-side NPC selection, reach, facing and dialogue lifecycle.

use super::*;
use crate::{
    content::{
        dialogue::{Context, Conversation, EntryContext},
        script::{Actor, Attachment, Event, Local, Outcome, Script, State},
    },
    sector::objects::FieldValue,
};
use serde_json::json;

#[derive(Clone, Copy)]
pub struct HoveredVitals {
    pub percent: [u32; 2],
    pub poisoned: bool,
    pub allied: bool,
    pub values: [i64; 4],
}

type Tile = (u32, u32);

pub(super) fn in_range(a: Tile, b: Tile) -> bool {
    // CE AG_TALK uses location_dist < ai_max_dialog_distance(local PC).
    a.0.abs_diff(b.0).max(a.1.abs_diff(b.1)) < 5
}

pub(super) fn facing(a: Tile, b: Tile) -> Option<u8> {
    if a == b {
        return None;
    }
    let dx = i64::from(b.0) - i64::from(a.0);
    let dy = i64::from(b.1) - i64::from(a.1);
    // CE location_rot switches to a cardinal facing past a 2:1 ratio.
    let direction = (
        if dx.abs() * 2 < dy.abs() {
            0
        } else {
            dx.signum() as i32
        },
        if dy.abs() * 2 < dx.abs() {
            0
        } else {
            dy.signum() as i32
        },
    );
    collision::DIRECTIONS
        .iter()
        .position(|&dir| dir == direction)
        .map(|dir| dir as u8)
}

impl SectorCache {
    pub(super) fn npc(&self, key: WorldOrder) -> Option<(&mobile::population::Resident, Tile)> {
        let target = self.target(key)?;
        let interaction::Action::Npc(id) = target.action else {
            return None;
        };
        if let Some(resident) = self.inventory.displaced_actor(id) {
            return Some((resident, target.tile));
        }
        let sector = &self.entries.get(&tile_sector_id(target.tile))?.0;
        let index = key.1.5.checked_sub(mobile_draw_slot(0))?;
        let resident = sector.mobiles.get(index)?;
        (resident.id == id && !resident.conflict).then_some((resident, target.tile))
    }
}

impl InteractiveWorld {
    pub fn hovered_npc(&self) -> Option<String> {
        match self.cache.target(self.cache.hover?)?.action {
            interaction::Action::Npc(id) | interaction::Action::Summon(id) => Some(id.key()),
            _ => None,
        }
    }

    pub fn conversation(&self) -> Option<&Conversation> {
        self.dialogue.as_ref().filter(|_| self.trade.is_none())
    }

    pub fn dialogue_diagnostic(&self) -> serde_json::Value {
        json!({"active":self.conversation().map(Conversation::diagnostic),"trade":self.trade_diagnostic(),
            "last_result":self.dialogue_status})
    }

    pub fn script_state(&self) -> &State {
        &self.cache.scripts
    }

    pub fn set_quest_time(&mut self, hours: u64, minute: u64) {
        self.cache.scripts.game_minute = hours.saturating_mul(60).saturating_add(minute);
    }

    pub fn restore_script_state(&mut self, mut state: State) {
        state.start_hour = self.cache.scripts.start_hour;
        state.sector_map = self.cache.scripts.sector_map.take();
        state.initialize_random();
        self.cache.scripts = state;
    }

    pub fn dialogue_anchor(&self) -> Option<(i32, i32)> {
        let (key, asset) = self.cache.speaking_pose.as_ref()?;
        let index = self
            .cache
            .ordered
            .binary_search_by_key(key, |raster| raster.key)
            .ok()?;
        let bounds = self.cache.ordered[index].frame_bounds(asset);
        Some((
            i32::try_from((bounds.min.0 + bounds.max.0) / 2).ok()?,
            i32::try_from(bounds.min.1).ok()?,
        ))
    }

    pub fn close_dialogue(&mut self, reason: &str) {
        self.trade = None;
        if let Some(session) = self.dialogue.take() {
            if matches!(reason, "completed" | "cancelled") {
                self.met_npcs
                    .insert((self.map.clone(), session.target.clone()));
            }
            self.dialogue_status =
                Some(json!({"status":reason,"conversation":session.diagnostic()}));
        }
        // Keep the actors in their last facing while this world cache is active.
    }
}

impl super::inventory::InventoryReader {
    pub fn npc_context(&mut self, world: &InteractiveWorld) -> Option<String> {
        let (resident, _) = world.cache.npc(world.cache.hover?)?;
        let name = self.npc_name(
            resident,
            world
                .met_npcs
                .contains(&(world.map.clone(), resident.id.key())),
        );
        if let Some(combat) = self.combat_target_hint(world, world.cache.hover?) {
            return Some(format!("{name}  {combat}"));
        }
        if world.player_flags().0 & 0x80 != 0
            && let Some(proto) = resident.prototype.as_ref()
        {
            let (_, stats) = world.cache.inventory.npc_vitals(
                resident.id,
                &proto.defaults,
                &resident.instance(),
            );
            return Some(format!("{name}  Alignment {}", stats.alignment));
        }
        Some(name)
    }

    pub(in crate::content::world) fn npc_name(
        &self,
        resident: &mobile::population::Resident,
        known: bool,
    ) -> String {
        let object = resident.instance();
        let description = object
            .field(22)
            .and_then(FieldValue::scalar)
            .or_else(|| resident.prototype.as_ref()?.defaults.description);
        let unknown = object.field(86).and_then(FieldValue::scalar).or_else(|| {
            resident
                .prototype
                .as_ref()?
                .defaults
                .npc_unknown_description
        });
        [
            if known {
                description
            } else {
                unknown.filter(|&id| id != 0)
            },
            description,
        ]
        .into_iter()
        .flatten()
        .find_map(|id| {
            self.descriptions
                .iter()
                .find_map(|table| table.mes.get(id as i32))
        })
        .unwrap_or("NPC")
        .to_owned()
    }

    pub fn start_npc_interaction(
        &mut self,
        world: &mut InteractiveWorld,
    ) -> Option<Result<(), String>> {
        let key = world.npc_request.take()?;
        if let Some(interaction::Action::Npc(id) | interaction::Action::Summon(id)) =
            world.cache.target(key).map(|t| &t.action)
            && world.corpse_tile(*id).is_some()
        {
            return Some(self.open_corpse(world, *id));
        }
        let result = self.open_npc_dialogue(world, key, None);
        if let Err(error) = &result
            && world.dialogue_status.is_none()
        {
            world.dialogue_status = Some(
                json!({"status":"rejected","target":world.cache.npc(key).map(|(npc, _)| npc.id.key()),"error":error}),
            );
        }
        Some(result)
    }

    pub(super) fn open_npc_dialogue(
        &mut self,
        world: &mut InteractiveWorld,
        key: WorldOrder,
        script_start: Option<usize>,
    ) -> Result<(), String> {
        if world.dialogue.is_some() {
            return Err("Dialogue already active".into());
        }
        let (resident, tile) = world.cache.npc(key).ok_or("NPC is unavailable")?;
        let player = world.cache.player.as_ref().ok_or("Player is unavailable")?;
        if script_start.is_none() && (player.moving() || !in_range(player.tile(), tile)) {
            return Err("NPC is out of dialogue range".into());
        }
        let object = resident.instance();
        let prototype = resident
            .prototype
            .as_ref()
            .ok_or("NPC prototype unavailable")?;
        let defaults = &prototype.defaults;
        let effective = mobile::population::Effective::new(&object, Some(prototype));
        let scalar = |index| object.field(index).and_then(FieldValue::scalar);
        let npc_flags = scalar(128).or(defaults.npc_flags);
        let leader = object
            .field(129)
            .map_or(defaults.npc_leader, FieldValue::reference);
        let mut follows_player = leader.is_none().then_some(false);
        let script = effective
            .appearance
            .dialog_script
            .filter(|&n| n > 0)
            .ok_or("NPC has no dialogue attachment")?;
        let art = effective
            .current_art
            .value()
            .ok_or("NPC standing ART unavailable")?;
        let original_facing = ((art >> 11) & 7) as u8;
        let npc_facing = facing(tile, player.tile()).unwrap_or(original_facing);
        let player_facing = facing(player.tile(), tile).unwrap_or_else(|| player.facing());
        let attribute = |index| {
            object.field(66).map_or_else(
                || defaults.character.as_ref().and_then(|stats| stats[index]),
                |field| {
                    field
                        .array_entry(index)
                        .map(|bytes| i32::from_le_bytes(bytes.try_into().unwrap()))
                },
            )
        };
        let (actor, vitals) = world
            .cache
            .inventory
            .npc_vitals(resident.id, defaults, &object);
        let critter = actor.critter_flags;
        let spell = actor.spell_flags;
        let spirit = spell & 0x02000000 != 0;
        let controlled = spell & 0x08000000 != 0;
        if controlled {
            follows_player = Some(true)
        }
        let npc_flags = npc_flags.map(|f| {
            if controlled || spell & 0x00800000 != 0 {
                f | 0x800
            } else {
                f
            }
        });
        let (hp, fatigue) = (Some(vitals.max_health), Some(vitals.max_fatigue));
        let (damage, fatigue_damage) = (actor.health_damage, actor.fatigue_damage);
        if critter & 0x3070 != 0
            || spell & 0x10000 != 0
            || npc_flags.is_some_and(|flags| flags & 0x80001 != 0 || flags & 0x900 == 0x100)
            || !spirit && (damage >= 32_000 || hp.is_some_and(|max| i64::from(damage) >= max))
            || critter & 4 == 0 && fatigue.is_some_and(|max| i64::from(fatigue_damage) >= max)
        {
            return Err(format!(
                "NPC cannot speak in its current state (critter flags {critter:#x}, NPC flags {npc_flags:?}, HP {hp:?}/{damage}, fatigue {fatigue:?}/{fatigue_damage})"
            ));
        }
        let stats = world.character_statistics();
        let reaction = initial_reaction(
            scalar(143).map(|n| n as i32).or(defaults.reaction_base),
            npc_flags,
            attribute(27),
            critter,
            &stats,
        );
        let target = resident.id.key();
        let met = world
            .met_npcs
            .contains(&(world.map.clone(), target.clone()));
        let magick_reaction = if controlled {
            -50
        } else if spell & 0x00800000 != 0 && critter & 8 == 0 {
            30
        } else {
            0
        };
        let reaction = reaction.map(|r| {
            r.saturating_add(world.reaction_delta(&target))
                .saturating_add(magick_reaction)
        });
        let context = Context {
            target,
            speaker: self.npc_name(resident, true),
            tile,
            facing: npc_facing,
            npc_gender: attribute(26),
            npc_intelligence: Some(vitals.attributes[4]),
            npc_race: attribute(27),
            social_class: scalar(144).map(|n| n as i32).or(defaults.social_class),
            player_armor: world.player_appearance().map(|art| (art >> 20) & 15),
            met,
            stats,
            gold: world.inventory_counters()[0],
            reaction,
            override_script: effective
                .appearance
                .dialog_override_script
                .unwrap_or_default(),
            entry: EntryContext {
                attachment: Attachment {
                    // These NPCs have persistent original GUIDs independent of the loaded sector.
                    owner: resident.id.key(),
                    event: Event::Dialogue,
                    script,
                    initial: Local::new(
                        effective.appearance.dialog_local_flags.unwrap_or_default(),
                        effective
                            .appearance
                            .dialog_local_counters
                            .unwrap_or_default(),
                    ),
                },
                follows_player,
                npc_flags,
                locations: [player.tile(), tile],
                met,
                hour: world.cache.hour,
            },
        };
        let script = Script::load(&mut self.vfs, script)?;
        let mut staged = world.cache.scripts.clone();
        staged.interaction =
            Some(world.script_inventory(&mut self.vfs, &context.target, reaction)?);
        let invocation = context.entry.invocation(script_start.unwrap_or(0));
        let execution = script.execute(&invocation, &mut staged).map_err(|fault| {
            let error = format!(
                "{} from {}: {fault}",
                script.source.virtual_path, script.source
            );
            world.dialogue_status = Some(json!({
                "status":"rejected","target":context.target,"attachment":invocation.attachment,
                "triggerer":Actor::Player,"script":script.source.virtual_path,
                "script_mount":script.source.to_string(),"error":error,"script_fault":fault
            }));
            error
        })?;
        let Outcome::Dialogue { line } = execution.outcome else {
            world.commit_script_inventory(&mut self.vfs, &context.target, &mut staged)?;
            world.cache.scripts = staged;
            world.dialogue_status = Some(json!({
                "status":"script_returned","target":context.target,"attachment":invocation.attachment,
                "triggerer":Actor::Player,"script":script.source.virtual_path,
                "script_mount":script.source.to_string(),"execution":execution
            }));
            return Ok(());
        };
        let conversation = Conversation::open(
            &mut self.vfs,
            &context,
            script,
            execution.statement,
            line,
            &mut staged,
        )?;
        let index = world
            .cache
            .ordered
            .binary_search_by_key(&key, |raster| raster.key)
            .map_err(|_| "NPC raster unavailable")?;
        let frame = &world.cache.ordered[index].frame;
        let asset = if original_facing == npc_facing {
            Arc::clone(frame)
        } else {
            mobile::Pose::new(art, frame.source.virtual_path.clone())
                .in_direction(npc_facing)
                .load(&mut self.vfs, &mut self.frames, 0)
                .map_err(|e| format!("Cannot face NPC: {e}"))?
        };
        world.commit_script_inventory(&mut self.vfs, &context.target, &mut staged)?;
        world
            .cache
            .player
            .as_mut()
            .unwrap()
            .set_facing(player_facing);
        world.cache.speaking_pose = Some((key, asset));
        world.cache.scripts = staged;
        world.dialogue = Some(conversation);
        world.dialogue_status = None;
        Ok(())
    }

    pub fn choose_dialogue_response(&mut self, world: &mut InteractiveWorld, index: usize) -> bool {
        self.advance_dialogue(world, index, None)
    }

    pub fn finish_trade(&mut self, world: &mut InteractiveWorld) -> bool {
        let Some(trade) = world.trade.take() else {
            return false;
        };
        let reaction = trade
            .reaction
            .saturating_add(world.reaction_delta(&trade.npc));
        if let Some(dialogue) = &mut world.dialogue {
            dialogue.set_reaction(reaction);
        }
        self.advance_dialogue(world, trade.response, Some(trade.destination))
    }

    fn advance_dialogue(
        &mut self,
        world: &mut InteractiveWorld,
        index: usize,
        after_barter: Option<i32>,
    ) -> bool {
        use crate::content::dialogue::{ResponseRejection, Selection};
        let Some(mut session) = world.dialogue.take() else {
            return false;
        };
        let stats = world.character_statistics();
        let mut staged = world.cache.scripts.clone();
        let npc = session.target.clone();
        let result = world
            .script_inventory(&mut self.vfs, &npc, session.reaction())
            .map_err(ResponseRejection::Effect)
            .and_then(|facts| {
                staged.interaction = Some(facts);
                let selection = if let Some(next) = after_barter {
                    Selection::AfterBarter(next)
                } else if session.turn.responses.get(index).is_some_and(|r| r.barter) {
                    if staged.following(&npc) {
                        let id = crate::sector::objects::OriginalId::parse_key(&npc)
                            .ok_or(ResponseRejection::Selection)?;
                        session.select(index, &stats, &mut staged)?;
                        world
                            .check_actor_inventory(id)
                            .map_err(ResponseRejection::Effect)?;
                        self.prepare_gear(world, id)
                            .map_err(ResponseRejection::Effect)?;
                        world
                            .commit_script_inventory(&mut self.vfs, &npc, &mut staged)
                            .map_err(ResponseRejection::Effect)?;
                        world.container_request = Some(id);
                        return Ok(false);
                    }
                    let reaction = session.reaction().ok_or_else(|| {
                        ResponseRejection::Effect("Merchant reaction unavailable".into())
                    })?;
                    let destination = session.select(index, &stats, &mut staged)?;
                    world
                        .exchange(|world| {
                            world.commit_script_inventory(&mut self.vfs, &npc, &mut staged)?;
                            world.open_trade(
                                &mut self.vfs,
                                &npc,
                                session.speaker.clone(),
                                reaction,
                                (index, destination),
                            )
                        })
                        .map_err(ResponseRejection::Effect)?;
                    return Ok(true);
                } else {
                    Selection::Response(index)
                };
                let hour = world.cache.hour;
                session.choose(
                    &mut self.vfs,
                    selection,
                    &stats,
                    &mut staged,
                    |vfs, scripts| world.commit_script_inventory(vfs, &npc, scripts),
                    hour,
                )
            });
        match result {
            Ok(open) => {
                world.cache.scripts = staged;
                world.dialogue = Some(session);
                world.dialogue_status = None;
                if !open {
                    world.close_dialogue("completed");
                }
                true
            }
            Err(error) => {
                world.dialogue_status = Some(session.record_rejection(index, error));
                world.dialogue = Some(session);
                false
            }
        }
    }
}

pub(super) fn initial_reaction(
    base: Option<i32>,
    flags: Option<u32>,
    race: Option<i32>,
    critter: u32,
    player: &super::super::character::CharacterStatistics,
) -> Option<i32> {
    // CE sub_4C1500, before reaction history, reputation and temporary effects.
    const RACIAL: [[i32; 8]; 11] = [
        [0, 0, 5, 5, 0, 10, -5, -5],
        [-5, 10, -10, -5, 0, 5, -10, -15],
        [-10, -20, 0, 0, -10, 10, -15, -10],
        [0, 0, 0, 0, 0, 5, -5, -5],
        [0, 0, 0, 0, 5, 5, -10, 5],
        [0, 0, 20, 15, 0, 10, -10, -10],
        [0, -20, -15, -10, -10, 0, 10, 0],
        [0, -20, -15, -5, 20, 10, 0, 5],
        [-30, -30, 5, -20, -30, -5, -30, -30],
        [-10, -30, -20, -15, -20, -20, 0, -10],
        [-20, -30, -40, -30, -20, -20, 10, 0],
    ];
    let base = base?;
    if flags? & 0x20000 != 0 || critter & 0x2000_800c != 0 {
        return Some(base);
    }
    let race = RACIAL.get(usize::try_from(race?).ok()?)?;
    let adjustment = race.get(usize::try_from(player.race).ok()?)?;
    Some(
        base.saturating_add(*adjustment)
            .saturating_add(player.reaction_modifier)
            .clamp(0, 100),
    )
}
