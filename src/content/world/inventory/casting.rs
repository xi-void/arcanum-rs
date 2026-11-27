//! Resolve college programs against the same PC, original mobiles and object transactions
//! used by movement, dialogue and inventory. Only durable deltas go into a save.
use super::super::abilities::{Aim, LockId, Method};
use super::super::spell_fx::Cue;
use super::*;
use crate::content::magick::{Channel, Component, Field, Operation, Phase, Program};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::content::world) enum Subject {
    Player,
    Mobile(OriginalId),
    Lock(String, LockId),
    Tile(String, Tile),
    Item(OriginalId),
}
#[derive(Default, Clone, Debug, Serialize, Deserialize)]
struct VitalChanges {
    #[serde(default)]
    dead: bool,
    #[serde(default)]
    facing: Option<u8>,
    #[serde(default)]
    death_anim: Option<u8>,
    #[serde(default)]
    decay_at: Option<u64>,
    #[serde(default)]
    decayed: bool,
    #[serde(default)]
    waypoint: Option<usize>,
    #[serde(default)]
    heal_rate: i32,
    #[serde(default)]
    recover_at: Option<u64>,
    health: Option<i32>,
    fatigue: Option<i32>,
    poison: Option<i32>,
    critter_on: u32,
    critter_off: u32,
    spell_on: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Binding {
    target: Subject,
    effects: Vec<(i32, i32)>,
    spell: u32,
    critter: u32,
    portal: u32,
    form: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    equipment: Vec<(EquipmentSlot, OriginalId)>,
    #[serde(default)]
    form_palette: u8,
    resistance: i32,
    #[serde(default)]
    maximum: i32,
    #[serde(default)]
    stun: Option<u32>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    overlays: Vec<i32>,
    #[serde(default)]
    destroy: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    visuals: Vec<Attachment>,
}
impl Binding {
    fn new(target: Subject) -> Self {
        Self {
            target,
            effects: vec![],
            spell: 0,
            critter: 0,
            portal: 0,
            form: None,
            equipment: Vec::new(),
            form_palette: 0,
            resistance: 0,
            maximum: 0,
            stun: None,
            overlays: vec![],
            destroy: false,
            visuals: vec![],
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Spawn {
    id: OriginalId,
    map: String,
    tile: Tile,
    #[serde(default)]
    blocks: bool,
    prototype: u32,
    #[serde(default)]
    palette: Option<u8>,
    #[serde(default)]
    restore_art: bool,
    #[serde(default)]
    facing: u8,
    #[serde(default)]
    hidden: bool,
    #[serde(default)]
    visuals: Vec<Attachment>,
    spell_flags: u32,
    critter_flags: u32,
    npc_flags: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Active {
    spell: usize,
    bindings: Vec<Binding>,
    summons: Vec<Spawn>,
    due: Option<u64>,
    ticks: Option<u32>,
    aptitude: i32,
    #[serde(default)]
    callback: Option<u64>,
    #[serde(default)]
    callback_left: Option<Duration>,
    #[serde(default)]
    ending: bool,
    #[serde(default)]
    center: Option<(String, Tile)>,
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub(in crate::content::world) struct State {
    #[serde(default)]
    offsets: BTreeMap<String, Placement>,
    #[serde(skip)]
    moved: BTreeMap<OriginalId, (Arc<mobile::population::Resident>, Arc<LoadedFrame>)>,
    actors: BTreeMap<String, VitalChanges>,
    active: Vec<Active>,
    #[serde(skip)]
    animation_tick: Option<Duration>,
    #[serde(skip)]
    art: BTreeMap<ModelKey, Model>,
    #[serde(skip)]
    stun: BTreeMap<u32, Arc<FrameLoop>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Attachment {
    entry: i32,
    start: u64,
    until: Option<u64>,
    reverse: bool,
    initial: usize,
    #[serde(default)]
    hazard: Option<Hazard>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Hazard {
    fire: bool,
    next: u64,
    frame: usize,
    count: usize,
    period_ms: u64,
}
#[derive(Clone, Serialize, Deserialize)]
struct Placement {
    map: String,
    source: Tile,
    tile: Tile,
    sequence: u64,
}
pub(super) struct Target {
    pub(super) stats: Option<CharacterStatistics>,
    pub(super) worth: [i32; 2],
    pub(super) critter2: u32,
    pub(super) natural: [i32; 10],
    pub(super) subject: Subject,
    pub(super) tile: Tile,
    pub(super) key: Option<WorldOrder>,
    pub(super) kind: Kind,
    pub(super) critter: u32,
    pub(super) spell: u32,
    pub(super) hp: i32,
    pub(super) fatigue: i32,
    pub(super) poison: i32,
    pub(super) maximum: i32,
    attributes: [i32; 8],
    pub(super) resistance: [i32; 5],
    aptitude: i32,
    weight: Option<i32>,
    pub(super) material: Option<u32>,
    item: Option<Item>,
    pub(super) object_flags: u32,
    flags: u32,
    difficulty: i32,
    notify: i32,
    pub(super) alignment: i32,
    pub(super) sound: Option<i32>,
}
#[derive(Clone, Copy, PartialEq)]
pub(super) enum Kind {
    Actor,
    Portal,
    Container,
    Tile,
    Item,
}
impl Target {
    pub(super) fn new(subject: Subject, tile: Tile, key: Option<WorldOrder>, kind: Kind) -> Self {
        Self {
            critter2: 0,
            stats: None,
            worth: [0; 2],
            natural: [0; 10],
            subject,
            tile,
            key,
            kind,
            critter: 0,
            spell: 0,
            hp: 0,
            fatigue: 0,
            poison: 0,
            maximum: 0,
            attributes: [0; 8],
            resistance: [0; 5],
            aptitude: 0,
            weight: None,
            material: None,
            item: None,
            object_flags: 0,
            flags: 0,
            difficulty: -1,
            notify: 0,
            alignment: 0,
            sound: None,
        }
    }
    fn actor(&mut self, pc: &Character, stats: &CharacterStatistics) {
        self.critter2 = pc.critter_flags2;
        self.stats = Some(stats.clone());
        self.hp = pc.health_damage;
        self.fatigue = pc.fatigue_damage;
        self.poison = pc.base[24];
        self.maximum = stats.max_health.clamp(0, i32::MAX.into()) as i32;
        self.attributes = stats.attributes;
        self.resistance = stats.resistance;
        self.aptitude = stats.aptitude;
        self.critter = pc.critter_flags;
        self.spell = pc.spell_flags;
        self.alignment = stats.alignment;
    }
    fn with_item(mut self, item: Item) -> Self {
        self.maximum = item.stats.hp[0].saturating_add(item.stats.hp[1]);
        self.hp = item.stats.hp[2];
        self.weight = item.stats.weight;
        self.material = item.effective.appearance.material;
        self.object_flags = item.effective.flags.value().unwrap_or(0);
        if let Some(r) = item.stats.resistance {
            self.resistance = r.map(|n| n.unwrap_or(0));
        }
        self.item = Some(item);
        self
    }
    fn matches(&self, flags: &[String]) -> Result<bool, String> {
        let mut categories = false;
        let mut matched = false;
        for flag in flags {
            let allowed = match flag.as_str() {
                "tgt_obj_t_critter" | "tgt_obj_t_portal" | "tgt_obj_t_container" => {
                    categories = true;
                    matched |= matches!(
                        (self.kind, flag.as_str()),
                        (Kind::Actor, "tgt_obj_t_critter")
                            | (Kind::Portal, "tgt_obj_t_portal")
                            | (Kind::Container, "tgt_obj_t_container")
                    );
                    true
                }
                "tgt_obj_t_pc" => self.subject == Subject::Player,
                "tgt_non_party_critters" => {
                    self.subject != Subject::Player && self.spell & 0x08000000 == 0
                }
                "tgt_obj_weight_below_5" => {
                    self.kind == Kind::Item && self.weight.is_some_and(|n| n < 5)
                }
                "tgt_obj_no_m_stone" => self.material != Some(0),
                "tgt_obj_m_stone" => {
                    self.material == Some(0)
                        || self.kind == Kind::Actor
                            && flags.iter().any(|s| s == "tgt_obj_t_critter")
                }
                "tgt_obj_no_self" | "tgt_obj_no_t_pc" => self.subject != Subject::Player,
                "tgt_obj_no_st_critter_dead" => self.kind != Kind::Actor || self.hp < self.maximum,
                "tgt_obj_st_critter_dead" => self.kind == Kind::Actor && self.hp >= self.maximum,
                "tgt_obj_st_critter_animal" => self.critter & 8 != 0,
                "tgt_obj_no_st_critter_animal" => self.critter & 8 == 0,
                "tgt_obj_no_st_critter_mechanical" => self.critter & 0x20000000 == 0,
                "tgt_obj_no_st_critter_undead" => self.critter & 4 == 0,
                "tgt_obj_no_st_critter_unrevivifiable" => self.critter & 0x800000 == 0,
                "tgt_obj_no_st_magically_held" => self.flags & 4 == 0,
                "tgt_obj_damaged" => self.hp > 0,
                "tgt_obj_inven" => self.kind == Kind::Item && self.key.is_none(),
                "tgt_object" | "tgt_obj_radius" => self.kind != Kind::Tile,
                "tgt_tile_empty"
                | "tgt_tile"
                | "tgt_tile_radius"
                | "tgt_tile_empty_immobiles"
                | "tgt_tile_radius_wall"
                | "tgt_tile_pathable_to" => self.kind == Kind::Tile,
                "tgt_self" | "tgt_all_party_critters" => true,
                "tgt_obj_no_inven" => {
                    self.kind != Kind::Tile && (self.kind != Kind::Item || self.key.is_some())
                }
                "tgt_obj_no_invulnerable" => self.object_flags & 0x00400000 == 0,
                "tgt_obj_no_t_wall" => true,
                s => {
                    return Err(format!(
                        "Target rule {s} requires additional world properties"
                    ));
                }
            };
            if !allowed {
                return Ok(false);
            }
        }
        Ok(!categories || matched)
    }
}

impl State {
    fn bindings<'a>(&'a self, target: &'a Subject) -> impl Iterator<Item = &'a Binding> {
        self.active
            .iter()
            .flat_map(|r| &r.bindings)
            .filter(move |b| b.target == *target)
    }
    pub(in crate::content::world) fn flags(&self, target: &Subject) -> (u32, u32) {
        self.bindings(target)
            .fold((0, 0), |(s, c), b| (s | b.spell, c | b.critter))
    }
    fn effects(
        &self,
        target: &Subject,
        rules: &Rules,
    ) -> Vec<(Arc<crate::content::character::Adjustments>, i32)> {
        self.bindings(target)
            .flat_map(|b| &b.effects)
            .filter_map(|(id, count)| {
                Some((rules.progression.spell_effects.get(id)?.clone(), *count))
            })
            .collect()
    }
    pub(super) fn revived_or_decayed(&self, id: OriginalId) -> bool {
        self.actors
            .get(&id.key())
            .is_some_and(|d| d.decayed || !d.dead && d.health.is_some())
    }
    pub(super) fn has_spawn(&self, id: OriginalId) -> bool {
        self.spawn(id).is_some()
    }
    fn spawn(&self, id: OriginalId) -> Option<&Spawn> {
        self.active
            .iter()
            .flat_map(|a| &a.summons)
            .find(|s| s.id == id)
    }
    pub(super) fn owns_spawn(&self, map: &str, id: OriginalId) -> bool {
        self.spawn(id).is_some_and(|s| s.map == map)
    }
    fn form(&self, target: &Subject) -> Option<u32> {
        self.bindings(target).find_map(|b| b.form)
    }
    pub(in crate::content::world) fn report(&self) -> Value {
        json!({"active":self.active,"actors":self.actors,"positions":self.offsets})
    }
}

fn spawn_defaults(vfs: &mut Vfs, n: u32) -> Result<(u32, objects::PrototypeDefaults), String> {
    let id = crate::prototype::BasicPrototypeId(n)
        .serialized()
        .ok_or("invalid basic summon prototype")?;
    let (_, bytes) =
        crate::content::sector::read_static_prototype(vfs, id).map_err(|e| e.to_string())?;
    let category = crate::prototype::decode_header(&bytes)
        .map_err(|e| e.to_string())?
        .object_type;
    Ok((
        category,
        objects::prototype_defaults(&bytes, id, category).map_err(|e| e.to_string())?,
    ))
}
pub(super) fn motion_clip(
    vfs: &mut Vfs,
    frames: &mut FrameCache,
    pose: &mobile::Pose,
) -> Result<Arc<FrameLoop>, String> {
    appearance::clip(vfs, frames, pose, true)
}
impl State {
    pub(super) fn facing(&self, id: OriginalId) -> Option<u8> {
        self.actors.get(&id.key())?.facing
    }
    pub(super) fn face(&mut self, id: OriginalId, dir: u8) {
        self.actors.entry(id.key()).or_default().facing = Some(dir);
        if let Some(spawn) = self
            .active
            .iter_mut()
            .flat_map(|r| &mut r.summons)
            .find(|s| s.id == id)
        {
            spawn.facing = dir
        }
    }
    pub(super) fn set_resident_frame(&mut self, id: OriginalId, frame: Arc<LoadedFrame>) {
        if let Some((_, current)) = self.moved.get_mut(&id) {
            *current = frame;
        }
    }
    pub(super) fn waypoint(&self, id: OriginalId) -> Option<usize> {
        self.actors.get(&id.key())?.waypoint
    }
    pub(super) fn set_waypoint(&mut self, id: OriginalId, index: usize) {
        self.actors.entry(id.key()).or_default().waypoint = Some(index)
    }
    pub(super) fn animation_callback_pending(&self) -> bool {
        self.active.iter().any(|r| r.callback_left.is_some())
    }
    pub(in crate::content::world) fn destroying(&self, id: OriginalId) -> bool {
        self.bindings(&Subject::Mobile(id)).any(|b| b.destroy)
    }
    pub(super) fn has_corpses(&self) -> bool {
        self.actors.values().any(|a| a.dead && !a.decayed)
    }
    pub(super) fn death_anim(&self, id: OriginalId) -> u8 {
        self.actors
            .get(&id.key())
            .and_then(|d| d.death_anim)
            .unwrap_or(7)
    }
    pub(in crate::content::world) fn down(&self, id: OriginalId) -> bool {
        self.actors.get(&id.key()).is_some_and(|d| d.dead)
    }
    pub(super) fn next_recovery(&self) -> Option<u64> {
        self.actors.values().filter_map(|d| d.recover_at).min()
    }
    pub(super) fn recover(&mut self, now: u64) -> bool {
        let mut changed = false;
        for d in self
            .actors
            .values_mut()
            .filter(|d| d.recover_at.is_some_and(|due| due <= now))
        {
            let due = d.recover_at.unwrap();
            let ticks = (now - due) / 80 + 1;
            let heal = ticks
                .saturating_mul(d.heal_rate.max(1) as u64)
                .min(i32::MAX as u64) as i32;
            let left = d.fatigue.unwrap_or(0).saturating_sub(heal).max(0);
            d.fatigue = Some(left);
            d.recover_at =
                (left > 0 && !d.dead).then(|| due.saturating_add(ticks.saturating_mul(80)));
            changed = true;
        }
        changed
    }
    pub(super) fn combat_summons(
        &self,
        inventory: &super::State,
        map: &str,
        at: Tile,
    ) -> Vec<OriginalId> {
        self.active
            .iter()
            .flat_map(|r| &r.summons)
            .filter(|s| {
                s.map == map
                    && !s.hidden
                    && s.tile.0.abs_diff(at.0).max(s.tile.1.abs_diff(at.1)) <= 12
                    && self
                        .art
                        .get(&s.model_key(inventory))
                        .is_some_and(|m| m.actor)
                    && !self.down(s.id)
            })
            .map(|s| s.id)
            .collect()
    }
}
pub(super) fn standing(
    vfs: &mut Vfs,
    frames: &mut FrameCache,
    art: u32,
) -> Result<Arc<LoadedFrame>, String> {
    appearance::standing_pose(vfs, art)?
        .load(vfs, frames, 0)
        .map_err(|e| e.to_string())
}
fn stun_clip(vfs: &mut Vfs, frames: &mut FrameCache, art: u32) -> Result<Arc<FrameLoop>, String> {
    let stand = standing(vfs, frames, art)?;
    let path = &stand.source.virtual_path;
    let stem = path
        .strip_suffix("xaa.art")
        .or_else(|| path.strip_suffix("xba.art"))
        .ok_or("Unrecognized critter ART for stun")?;
    let requested = format!("{stem}xbx.art");
    let path = if vfs
        .resolve(&requested)
        .map_err(|e| e.to_string())?
        .winner()
        .is_some()
    {
        requested
    } else {
        path.clone()
    };
    let mut pose = mobile::Pose::new(art, path);
    let load = |f: &mut FrameCache, v: &mut Vfs, p: &mut mobile::Pose, index| {
        p.load(v, f, index).map_err(|e| e.to_string())
    };
    let first = load(frames, vfs, &mut pose, 0)?;
    let count = first.decoded.frame_count;
    let fps = first.decoded.frames_per_second as u32;
    let mut seq = vec![first];
    for i in 1..count {
        seq.push(load(frames, vfs, &mut pose, i)?)
    }
    Ok(Arc::new(FrameLoop::timed(seq, fps)?))
}
mod actors;
mod appearance;
mod lifecycle;
mod loadout;
mod projection;
mod targeting;
mod transformation;
use actors::npc_character;
use appearance::{Model, ModelKey};
// magictech.c summon tables 3–5 choose a basic prototype by caster aptitude.
fn summon_proto(prototype: u32, list: Option<usize>, apt: i32) -> Result<u32, String> {
    if prototype != 0 {
        return Ok(prototype);
    }
    let table: &[(i32, u32)] = match list {
        Some(3) => &[(25, 27371), (50, 27372), (75, 27373), (1000, 27374)],
        Some(4) => &[
            (20, 28319),
            (40, 28332),
            (55, 28334),
            (70, 28330),
            (95, 28329),
            (1000, 28360),
        ],
        Some(5) => &[(20, 28468), (60, 28469), (85, 28470), (1000, 28471)],
        _ => return Err("Unknown summon table".into()),
    };
    Ok(table
        .iter()
        .find(|(max, _)| apt <= *max)
        .ok_or("Aptitude outside summon table")?
        .1)
}

impl InventoryReader {
    fn spell_eye(
        &mut self,
        w: &mut InteractiveWorld,
        id: i32,
        key: Option<WorldOrder>,
        now: Duration,
        cue: Cue,
    ) -> (Duration, Value) {
        let def = self.spell_fx.entry(id).or_insert_with(|| {
            spell_fx::Definition::load(&mut self.vfs, id).map_err(|e| format!("EyeCandy {id}: {e}"))
        });
        match def {
            Ok(Some(d)) => w
                .play_spell_effect(&mut self.vfs, &mut self.frames, d, key, now, cue)
                .unwrap_or_else(|e| (Duration::ZERO, json!({"entry":id,"error":e.to_string()}))),
            Ok(None) => (Duration::ZERO, Value::Null),
            Err(e) => (Duration::ZERO, json!({"entry":id,"error":e})),
        }
    }
}

impl InteractiveWorld {
    pub fn selecting_spell_destination(&self) -> bool {
        matches!(self.ability.selected,Some(Method::Spell(id)) if self.cache.inventory.program(id).is_ok_and(|p|p.components.iter().any(|c|matches!(&c.operation,Operation::Move(mode) if mode=="teleport_tile"))))
    }
    pub fn select_spell_destination(&mut self, dest: crate::content::jumps::Destination) {
        if self.selecting_spell_destination() {
            self.ability.request = self
                .ability
                .selected
                .map(|method| (Aim::Destination(dest), method))
        }
    }
    pub(in crate::content::world) fn aims_at_ground(&self, id: usize) -> bool {
        self.cache
            .inventory
            .program(id)
            .is_ok_and(|p| p.targets.iter().any(|s| s.starts_with("tgt_tile")))
    }
    pub fn selecting_spell_item(&self) -> bool {
        matches!(self.ability.selected,Some(Method::Spell(id)) if self.cache.inventory.program(id).is_ok_and(|p|has(&p.targets,"tgt_obj_inven")))
    }
    pub fn select_spell_item(&mut self, item: &str) -> bool {
        if !self.selecting_spell_item() {
            return false;
        }
        let Ok(id) = parse_id(item) else { return false };
        self.ability.request = self.ability.selected.map(|m| (Aim::Item(id), m));
        true
    }
    pub(in crate::content::world) fn player_flags(&self) -> (u32, u32) {
        let inv = &self.cache.inventory;
        let (s, c) = inv.magick.flags(&Subject::Player);
        (
            s | inv.character.spell_flags,
            c | inv.character.critter_flags,
        )
    }
}
fn has(flags: &[String], name: &str) -> bool {
    flags.iter().any(|s| s == name)
}
fn saving_throw(p: &Program, t: &Target, rng: &mut crate::random::Random) -> (bool, i32) {
    if t.subject == Subject::Player || t.kind != Kind::Actor {
        return (true, 0);
    }
    let mut resistance = t.resistance[4];
    if t.aptitude < 0 {
        resistance = 100 - (100 - resistance) * (100 + t.aptitude) / 100
    }
    let roll = 1 + rng.below(100) as i32;
    let mut resisted = (resistance - roll).max(0);
    let damage = p
        .components
        .iter()
        .any(|c| matches!(c.operation, Operation::Vital { heal: false, .. }));
    let Some((stat, modifier)) = p.save else {
        return (damage || p.maintain.is_some() || resisted == 0, resisted);
    };
    if stat == 6 && t.attributes[6] >= 20 {
        return (false, 100);
    }
    let adjustment = if damage || p.maintain.is_some() {
        0
    } else {
        resisted / 10
    };
    let difficulty = t.attributes[stat]
        .saturating_add(modifier)
        .saturating_sub(adjustment);
    if difficulty > 0 {
        let roll = 1 + rng.below(20) as i32;
        if roll <= difficulty {
            resisted = resisted.saturating_add(difficulty - roll);
            if damage {
                resisted = resisted.max(50)
            } else if p.maintain.is_none() {
                return (false, 100);
            }
        }
    }
    (true, resisted.clamp(0, 100))
}

impl InventoryReader {
    pub(super) fn cast_spell(
        &mut self,
        w: &mut InteractiveWorld,
        id: usize,
        aim: Aim,
        now: Duration,
        impact: bool,
    ) -> Result<(String, Value), String> {
        let program = w.cache.inventory.program(id)?;
        let study = &w.progression_rules().spells[id];
        let name = study.name.clone();
        let wp = study.willpower.clone()?;
        let stats = w.character_statistics();
        if stats.colleges[id / 5] as usize <= id % 5 {
            return Err("Spell has not been learned".into());
        }
        if w.unconscious() || stats.current_health <= 0 {
            return Err("Caster is incapacitated".into());
        }
        let (caster_flags, critter) = w.cache.inventory.magick.flags(&Subject::Player);
        let caster_flags = caster_flags | w.cache.inventory.character.spell_flags;
        if stats.attributes[4] < 5
            || stats.attributes[6] < wp
            || caster_flags & (0x1800 | program.disallowed_caster) != 0
            || (critter | w.cache.inventory.character.critter_flags) & 0x60 != 0
        {
            return Err("You lose your concentration".into());
        }
        let mut targets = self.spell_targets(w, &program, aim)?;
        // Resolve authored armor before taking spell damage/resistance snapshots.
        let mut prepared_actor = false;
        for target in &targets {
            if let Subject::Mobile(id) = target.subject
                && target.kind == Kind::Actor
                && id.kind == 2
                && !w.cache.inventory.combat.prepared.contains(&id)
            {
                self.prepare_gear(w, id)?;
                prepared_actor = true;
            }
        }
        if prepared_actor {
            targets = self.spell_targets(w, &program, aim)?;
        }
        let mut run = Active {
            spell: id,
            bindings: vec![],
            summons: vec![],
            aptitude: stats.aptitude,
            callback: None,
            callback_left: None,
            ending: false,
            center: Some((w.map.clone(), self.target(w, aim)?.tile)),
            due: program
                .maintain
                .map(|(_, n)| n)
                .or(program.duration.map(|n| {
                    if w.cache.inventory.character.mastery == Some((id / 5) as u32) {
                        n.saturating_mul(2)
                    } else {
                        n
                    }
                }))
                .map(|n| w.game_second.saturating_add(n)),
            ticks: program.triggers,
        };
        let active = &w.cache.inventory.magick.active;
        if program.maintain.is_some()
            && active
                .iter()
                .filter(|r| {
                    !r.ending
                        && w.cache
                            .inventory
                            .program(r.spell)
                            .is_ok_and(|p| p.maintain.is_some())
                })
                .count()
                >= (stats.attributes[4] / 4) as usize
        {
            return Err("All maintained spell slots are in use".into());
        }
        let mut preparations = BTreeMap::new();
        let mut blocking = BTreeMap::new();
        let mut stuns = BTreeMap::new();
        let mut moves = BTreeMap::new();
        let mut drops = BTreeMap::new();
        let mut weapons = BTreeMap::new();
        let mut information = None;
        let mut loadouts = BTreeMap::new();
        let mut source_random = w.cache.inventory.skill_random;
        for t in &targets {
            if t.spell & program.disallowed_target != 0
                || t.critter & program.disallowed_critter != 0
                || program.no_stack
                    && active
                        .iter()
                        .any(|r| r.spell == id && r.bindings.iter().any(|b| b.target == t.subject))
            {
                return Err("Target already has an incompatible spell".into());
            }
            for c in &program.components {
                match &c.operation {
                    Operation::Form(n) if *n >= 0 => {
                        let key = ModelKey::Form(*n as u32, c.palette.unwrap_or(0));
                        if !w.cache.inventory.magick.art.contains_key(&key)
                            && !preparations.contains_key(&key)
                        {
                            preparations.insert(key, key.load(&mut self.vfs, &mut self.frames)?);
                        }
                    }
                    Operation::Summon { prototype, list } => {
                        let n = summon_proto(*prototype, *list, stats.aptitude)?;
                        let (category, d) = spawn_defaults(&mut self.vfs, n)?;
                        blocking
                            .entry(n)
                            .or_insert(d.appearance.flags.unwrap_or(0) & 0x403 == 0);
                        let gear = if category == 16 {
                            loadout::Loadout::prepare(
                                &mut self.vfs,
                                &mut self.frames,
                                &d,
                                &mut source_random,
                            )?
                        } else {
                            loadout::Loadout::default()
                        };
                        let key = ModelKey::summon(
                            n,
                            c.palette,
                            program.restores_summoned_art(),
                            gear.armor(),
                        );
                        loadouts.insert((t.tile, n), gear);
                        if !w.cache.inventory.magick.art.contains_key(&key)
                            && !preparations.contains_key(&key)
                        {
                            preparations.insert(key, key.load(&mut self.vfs, &mut self.frames)?);
                        }
                    }
                    Operation::Effect { id, .. } => {
                        if !w
                            .cache
                            .inventory
                            .rules
                            .progression
                            .spell_effects
                            .contains_key(id)
                        {
                            return Err(format!("Missing effect {id}"));
                        }
                    }
                    Operation::Identify => {
                        information = Some(match t.subject {
                            Subject::Lock(_, LockId::Container(n)) => {
                                let v = w.inspect_inventory(&mut self.vfs, &n.key())?;
                                let list = v["items"]
                                    .as_array()
                                    .ok_or("Missing container inventory")?
                                    .iter()
                                    .map(|i| i["name"].as_str().unwrap_or("Item"))
                                    .collect::<Vec<_>>();
                                if list.is_empty() {
                                    "Empty container".into()
                                } else {
                                    list.join(", ")
                                }
                            }
                            Subject::Mobile(_) => format!(
                                "HP {}/{}  Alignment {}  Aptitude {}",
                                t.maximum - t.hp,
                                t.maximum,
                                t.alignment,
                                t.aptitude
                            ),
                            Subject::Item(_) => "Item identified".into(),
                            _ => return Err("Select a container, item or NPC".into()),
                        })
                    }
                    Operation::Vital { flags, .. } if has(flags, "stunned") => {
                        let art = match t.subject {
                            Subject::Player => w.player_appearance(),
                            Subject::Mobile(id) => {
                                if let Some(s) = w.cache.inventory.magick.spawn(id) {
                                    let (_, d) = spawn_defaults(&mut self.vfs, s.prototype)?;
                                    d.current_art.or(d.original_art)
                                } else {
                                    t.key
                                        .and_then(|k| w.cache.npc(k))
                                        .and_then(|(r, _)| r.current_art.value())
                                }
                            }
                            _ => None,
                        }
                        .ok_or("Missing actor ART for stun")?;
                        let clip = stun_clip(&mut self.vfs, &mut self.frames, art)?;
                        stuns.insert(t.key, (art, clip));
                    }
                    Operation::Vital { .. }
                        if t.kind != Kind::Actor
                            && t.kind != Kind::Tile
                            && t.item.is_none()
                            && c.targets.is_empty() =>
                    {
                        return Err("Structural damage to this object is unavailable".into());
                    }
                    Operation::Goal(goal)
                        if goal == "anim_goal_knockback" && t.matches(&c.targets)? =>
                    {
                        let from = w.cache.player.as_ref().unwrap().tile();
                        let direction = npc::facing(from, t.tile).unwrap_or(4);
                        let mut to = t.tile;
                        for _ in 0..4 {
                            let Some(next) = collision::step(to, direction, |tile| {
                                SectorCache::player_cell(
                                    &w.cache.entries,
                                    &w.cache.door_states,
                                    tile,
                                )
                            }) else {
                                break;
                            };
                            to = next
                        }
                        if to != t.tile {
                            moves.insert(t.key, to);
                            if let Subject::Item(id) = t.subject {
                                let mut records = w.records(&mut self.vfs);
                                let item = records.item(id, &w.cache.inventory.state)?;
                                check_item(&item, &w.cache.inventory.state)?;
                                let art = records.ground_art(&item, to, w.reference)?;
                                drops.insert(id, (item, art));
                            }
                        }
                    }
                    Operation::Goal(goal) if goal != "anim_goal_knockback" => {
                        return Err(format!("Unknown movement goal {goal}"));
                    }
                    Operation::Move(s)
                        if s == "teleport_tile" && !matches!(aim, Aim::Destination(_)) =>
                    {
                        return Err(
                            "Teleportation requires a discovered destination selection".into()
                        );
                    }
                    Operation::Vital { flags, .. } if has(flags, "weap_dropped") => {
                        if let Subject::Mobile(id) = t.subject {
                            let inv = &w.cache.inventory;
                            let mut records = w.records(&mut self.vfs);
                            let item = if inv.magick.spawn(id).is_some() {
                                inv.state.changes.values().find(|c|matches!(c.place, Place::WornBy(_,actor,EquipmentSlot::Weapon) if actor==id)).map(|c|c.item.clone())
                            } else {
                                let owner = Owner::OriginalObject(w.map.clone(), id);
                                records
                                    .owner_layout(&inv.state, &owner)?
                                    .items
                                    .into_values()
                                    .find(|i| {
                                        i.authored_slot == Some(1004)
                                            && !inv.state.changes.contains_key(&i.id)
                                    })
                            };
                            if let Some(item) = item {
                                check_removal(&item)?;
                                if item.scripted {
                                    return Err("Disarm requires an equipment script".into());
                                }
                                let art = records.ground_art(&item, t.tile, w.reference)?;
                                weapons.insert(id, (item, art));
                            }
                        }
                    }
                    _ => (),
                }
                if c.apply.is_empty()
                    && c.phase == Phase::Begin
                    && !matches!(c.operation, Operation::Summon { .. })
                {
                    t.matches(&c.targets)?;
                }
            }
        }
        let potential = targets
            .len()
            .checked_mul(
                program
                    .components
                    .iter()
                    .filter(|c| {
                        matches!(c.operation, Operation::Summon { .. } | Operation::Goal(_))
                    })
                    .count(),
            )
            .and_then(|n| {
                loadouts
                    .values()
                    .try_fold(n, |n, g| n.checked_add(g.count()))
            })
            .ok_or("Summon count overflow")?;
        if w.cache
            .inventory
            .state
            .next_generated
            .checked_add(potential as u64)
            .is_none_or(|n| n > usize::MAX as u64 / 2 - 2)
        {
            return Err("Generated object identities exhausted".into());
        }
        let reflects = !program.no_reflect && targets.iter().any(|t| t.spell & 0x2000 != 0);
        let mut equipment = if impact
            && (reflects || targets.iter().any(|t| t.subject == Subject::Player))
            && program.components.iter().any(|c| {
                c.phase == Phase::Begin && matches!(c.operation, Operation::Form(n) if n >= 0)
            }) {
            Some(transformation::FormEquipment::prepare(w, &mut self.vfs)?)
        } else {
            None
        };
        w.cache
            .inventory
            .combat
            .check(None, 4, stats.current_fatigue)?;
        let cost = program.cost;
        if impact {
            w.cache.inventory.charge_spell(
                (id / 5) as u32,
                cost,
                stats.current_fatigue,
                w.game_second,
                false,
            )?;
            if program.aggressive
                || program
                    .components
                    .iter()
                    .any(|c| matches!(c.operation, Operation::Vital { heal: false, .. }))
            {
                for t in &targets {
                    if let Subject::Mobile(actor) = t.subject
                        && t.kind == Kind::Actor
                        && t.hp < t.maximum
                    {
                        self.engage(w, actor, false, None)?;
                    }
                }
            }
            w.cache.inventory.pay_combat_ap(None, 4);
        }
        let xp_before = w.cache.inventory.character.base[18];
        let (fatigue, mana) = w.cache.inventory.charge_spell(
            (id / 5) as u32,
            cost,
            stats.current_fatigue,
            w.game_second,
            impact,
        )?;
        let mut result = json!({"method":Method::Spell(id),"spell":id,"name":name,"aptitude":stats.aptitude,
            "remaining_dependencies":program.remaining_dependencies(),"spell_source":w.progression_rules().spell_source.as_ref().map(crate::content::diagnostics::source),"fatigue_cost":0,"target":targets.first().map(|t|&t.subject)});
        if !impact {
            let tile = targets[0].tile;
            if let Some(p) = w.cache.player.as_mut()
                && let Some(f) = npc::facing(p.tile(), tile)
            {
                p.set_facing(f)
            }
            let (mut delay, fx) = self.spell_eye(w, id as i32 * 10, None, now, Cue::Forward);
            if program.missile.is_some()
                && let Some(anchor) = project_world_tile(self.target(w, aim)?.tile, w.reference)
            {
                let entry = id as i32 * 10 + 1;
                let (_, mut projectile) = self.spell_eye(w, entry, None, now + delay, Cue::Forward);
                if let Some(Ok(Some(def))) = self.spell_fx.get(&entry) {
                    let travel = w.cache.launch_missile(entry, anchor, def.projectile_speed);
                    projectile["travel_ms"] = json!(travel.as_millis());
                    delay += travel;
                }
                projectile["impact_delay_ms"] = json!(delay.as_millis());
                result["projectile"] = projectile;
            }
            w.ability.casting_actor = if let Aim::Object(key) = aim {
                match &w.cache.target(key).map(|t| &t.action) {
                    Some(interaction::Action::Npc(id) | interaction::Action::Summon(id)) => {
                        Some(*id)
                    }
                    _ => None,
                }
            } else {
                None
            };
            w.ability.casting = Some((aim, Method::Spell(id), now + delay));
            result["casting_effect"] = fx;
            result["outcome"] = json!("casting");
            return Ok((format!("Casting {name}"), result));
        }
        if let Some(last) = &w.ability.last
            && let Some(projectile) = last.get("projectile")
        {
            result["projectile"] = projectile.clone();
        }
        result["casting_effect"] = w
            .ability
            .last
            .as_ref()
            .map(|v| v["casting_effect"].clone())
            .unwrap_or_default();
        result["fatigue_cost"] = json!(fatigue);
        result["item_mana_cost"] = json!(mana);
        w.cache.inventory.revision += 1;
        if stats.current_fatigue < i64::from(fatigue) {
            result["outcome"] = json!("exhausted");
            return Ok(("Not enough energy — exhausted".into(), result));
        }
        w.cache.inventory.skill_random = source_random;
        w.cache.inventory.magick.art.extend(preparations);
        let mut applied = vec![];
        let mut effects = vec![];
        let mut attachments = vec![];
        let mut failed = false;
        for t in &mut targets {
            if t.subject != Subject::Player && t.spell & 0x2000 != 0 && !program.no_reflect {
                if caster_flags & 0x2000 != 0 {
                    applied.push(json!({"target":t.subject,"outcome":"reflected_twice"}));
                    continue;
                }
                result["reflected_by"] = json!(t.subject);
                *t = self.target(w, Aim::Player)?;
            }
            let (passed, resistance) =
                saving_throw(&program, t, &mut w.cache.inventory.skill_random);
            if !passed || t.spell & 0x800 != 0 {
                applied
                    .push(json!({"target":t.subject,"outcome":"resisted","resistance":resistance}));
                continue;
            }
            let mut binding = Binding::new(t.subject.clone());
            binding.resistance = resistance;
            binding.maximum = t.maximum;
            if let Some((art, clip)) = stuns.remove(&t.key) {
                let cycles = ((20 - t.attributes[2]) / 2).max(1) as u32;
                let duration = clip
                    .period()
                    .saturating_mul(clip.frames.len() as u32)
                    .saturating_mul(cycles);
                binding.stun = Some(art);
                binding.critter |= 0x20;
                run.due = Some(
                    w.game_second
                        .saturating_add((duration.as_millis() * 8).div_ceil(1000) as u64),
                );
                w.cache.inventory.magick.stun.insert(art, clip);
            }
            let hp_before = t.hp;
            let start_summons = run.summons.len();
            for c in program
                .components
                .iter()
                .filter(|c| c.phase == Phase::Begin)
            {
                if !c.apply.is_empty() {
                    if has(&c.apply, "tgt_summoned") {
                        if let Some(last) = run.summons[start_summons..].last_mut() {
                            match &c.operation {
                                Operation::Flag { field, mask, on } => {
                                    let f = match field {
                                        Field::Critter => Some(&mut last.critter_flags),
                                        Field::Spell => Some(&mut last.spell_flags),
                                        Field::Npc => Some(&mut last.npc_flags),
                                        _ => None,
                                    };
                                    if let Some(f) = f {
                                        if *on { *f |= *mask } else { *f &= !mask }
                                    }
                                }
                                Operation::Eye {
                                    index,
                                    add: true,
                                    play,
                                } => attachments.push((
                                    last.id,
                                    id as i32 * 10 + index,
                                    play.clone(),
                                )),
                                _ => (),
                            }
                        }
                        continue;
                    }
                    if has(&c.apply, "tgt_self")
                        && let Operation::Flag { field, mask, on } = c.operation
                    {
                        if field == Field::Spell && on && !run.summons.is_empty() {
                            let mut caster = Binding::new(Subject::Player);
                            caster.spell = mask;
                            run.bindings.push(caster);
                        } else {
                            w.cache
                                .inventory
                                .permanent_flags(&Subject::Player, field, mask, on);
                        }
                        continue;
                    }
                }
                if (c.targets.iter().any(|s| s == "tgt_self") || has(&c.apply, "tgt_self"))
                    && t.subject != Subject::Player
                {
                    continue;
                }
                if !t.matches(&c.targets)? || c.spell_flags != 0 && t.spell & c.spell_flags == 0 {
                    continue;
                }
                match &c.operation {
                    Operation::Eye {
                        index,
                        add: true,
                        play,
                    } if t.kind != Kind::Tile => {
                        let entry = id as i32 * 10 + index;
                        let (attachment, fx) = self.start_attachment(
                            w,
                            t.key,
                            entry,
                            play,
                            now,
                            binding.visuals.last(),
                        );
                        let until = attachment.as_ref().and_then(|v| v.until);
                        if let Some(v) = attachment {
                            binding.visuals.push(v);
                        }
                        binding.destroy |= has(play, "destroy")
                            && matches!(t.subject, Subject::Mobile(_))
                            && t.hp >= t.maximum;
                        if has(play, "callback")
                            && (binding.destroy
                                || program
                                    .components
                                    .iter()
                                    .any(|c| c.phase == Phase::Callback))
                        {
                            let due = until.unwrap_or(w.game_second);
                            run.callback = Some(due);
                            run.callback_left = Some(Duration::from_millis(
                                fx["duration_ms"].as_u64().unwrap_or(0),
                            ));
                        }
                        if !fx.is_null() {
                            effects.push(fx)
                        }
                    }
                    Operation::Flag {
                        field: Field::Container | Field::Portal,
                        mask,
                        on,
                    } => {
                        if let Subject::Lock(_, lock) = t.subject {
                            result["flags_before"] = json!(t.flags);
                            result["difficulty"] = json!(t.difficulty);
                            if *mask == 1 {
                                if t.notify != 0 {
                                    result["guard_notice"] =
                                        json!({"loudness":"loud","failed":false})
                                }
                                if t.difficulty < 0 || stats.aptitude >= t.difficulty {
                                    w.cache.door_states.locks.entry(lock).or_default().locked =
                                        Some(*on)
                                } else {
                                    failed = true
                                }
                            } else if *mask == 4 && *on {
                                binding.portal |= 4
                            }
                        }
                    }
                    Operation::Flag {
                        field: Field::Item,
                        mask,
                        on: true,
                    } => {
                        if let Subject::Item(item) = t.subject
                            && let Some(change) = w.cache.inventory.state.changes.get_mut(&item)
                        {
                            change.item.flags |= mask
                        }
                    }
                    Operation::Flag { field, mask, on } => {
                        if run.due.is_some() || run.callback.is_some() {
                            match (field, on) {
                                (Field::Spell, true) => binding.spell |= mask,
                                (Field::Critter, true) => binding.critter |= mask,
                                _ => (),
                            }
                        } else {
                            w.cache
                                .inventory
                                .permanent_flags(&t.subject, *field, *mask, *on)
                        }
                    }
                    Operation::Effect {
                        id,
                        count,
                        scaled,
                        add: true,
                    } => binding.effects.push((
                        *id,
                        count
                            .saturating_add(scaled.saturating_mul(stats.aptitude) / 100)
                            .max(0),
                    )),
                    Operation::Form(n) if *n >= 0 => {
                        if t.subject == Subject::Player
                            && let Some(items) = equipment.take()
                        {
                            items.apply(w, t.tile, &mut binding);
                        }
                        binding.form = Some(*n as u32);
                        binding.form_palette = c.palette.unwrap_or(0);
                    }
                    Operation::Vital { flags, .. } if has(flags, "weap_dropped") => {
                        if let Subject::Mobile(id) = t.subject
                            && let Some((item, art)) = weapons.remove(&id)
                        {
                            let owned = item.id;
                            let inv = &mut w.cache.inventory;
                            result["disarm"] = inv.state.relocate(
                                item,
                                Place::Ground(w.map.clone(), t.tile),
                                None,
                            )?;
                            inv.ground.insert(owned, art);
                        } else {
                            result["disarm"] = json!({"outcome":"unarmed"})
                        }
                    }
                    Operation::Vital { .. } => {
                        apply_vital(&mut w.cache.inventory, t, c, stats.aptitude, resistance)
                    }
                    Operation::Dispel => {
                        while let Some(i) = w.cache.inventory.magick.active.iter().position(|r| {
                            !r.ending
                                && (r.bindings.iter().any(|b| b.target == t.subject)
                                    || r.summons.iter().any(|s| Subject::Mobile(s.id) == t.subject))
                        }) {
                            self.end_spell(w, i, now)
                        }
                        if let Subject::Lock(_, lock) = t.subject {
                            w.cache.door_states.locks.entry(lock).or_default().held = Some(false)
                        }
                    }
                    Operation::Goal(_) => {
                        if let Some(to) = moves.remove(&t.key) {
                            match t.subject {
                                Subject::Mobile(id) => {
                                    w.relocate_actor(id, t.key, t.tile, to);
                                }
                                Subject::Item(id) => {
                                    if let Some((item, art)) = drops.remove(&id) {
                                        let inv = &mut w.cache.inventory;
                                        inv.state.relocate(
                                            item,
                                            Place::Ground(w.map.clone(), to),
                                            None,
                                        )?;
                                        inv.ground.insert(id, art);
                                    }
                                }
                                Subject::Player => {
                                    if let Some(p) = w.cache.player.as_mut() {
                                        p.relocate(to)
                                    }
                                }
                                _ => (),
                            }
                            result["displacement"] = json!({"from":t.tile,"to":to});
                            t.tile = to;
                        }
                    }
                    Operation::Move(s) if s == "teleport_tile" => {
                        if let Aim::Destination(dest) = aim {
                            w.spell_transition(dest)
                        }
                    }
                    Operation::Move(s) if s == "target_tile" => {
                        if let Some(p) = w.cache.player.as_mut() {
                            p.relocate(t.tile)
                        }
                        w.cache.refresh_roofs();
                        w.cache.update_character_sector()
                    }
                    Operation::Summon { prototype, list } => {
                        let cell = SectorCache::player_cell(
                            &w.cache.entries,
                            &w.cache.door_states,
                            t.tile,
                        );
                        if has(&c.targets, "tgt_tile_empty_immobiles")
                            && !collision::can_stand(cell.map(|c| c.vacate(u32::MAX)))
                        {
                            continue;
                        }
                        if has(&c.targets, "tgt_tile_empty")
                            && (!collision::can_stand(cell)
                                || Some(t.tile) == w.cache.player.as_ref().map(|p| p.tile()))
                        {
                            continue;
                        }
                        let prototype = summon_proto(*prototype, *list, stats.aptitude)?;
                        let inv = &mut w.cache.inventory;
                        let restore_art = program.restores_summoned_art();
                        let gear = loadouts.remove(&(t.tile, prototype)).unwrap_or_default();
                        let key = ModelKey::summon(prototype, c.palette, restore_art, gear.armor());
                        let sid = inv.state.generated_id();
                        inv.state.next_generated += 1;
                        gear.commit(inv, &w.map, sid);
                        run.summons.push(Spawn {
                            id: sid,
                            map: w.map.clone(),
                            tile: t.tile,
                            blocks: blocking[&prototype],
                            prototype,
                            palette: c.palette,
                            restore_art,
                            facing: if inv.magick.art[&key].actor {
                                npc::facing(t.tile, w.cache.player.as_ref().unwrap().tile())
                                    .unwrap_or(0)
                            } else {
                                inv.magick.art[&key].facing()
                            },
                            visuals: vec![],
                            hidden: inv.magick.art[&key].actor || inv.magick.art[&key].hidden,
                            spell_flags: 0,
                            critter_flags: 0,
                            npc_flags: 0,
                        })
                    }
                    _ => (),
                }
            }
            applied.push(json!({"target":t.subject,"tile":t.tile,"health_damage_before":hp_before,"health_damage":t.hp,"max_hp":t.maximum,"remaining_hp":t.maximum.saturating_sub(t.hp).max(0),"poison":t.poison,"resistance":resistance}));
            if program.cancels != 0 {
                while let Some(i) = w.cache.inventory.magick.active.iter().position(|r| {
                    !r.ending
                        && r.bindings
                            .iter()
                            .any(|b| b.target == t.subject && b.spell & program.cancels != 0)
                }) {
                    self.end_spell(w, i, now);
                }
            }
            run.bindings.push(binding);
        }
        let all_resisted = run.bindings.is_empty();
        if !all_resisted && (run.due.is_some() || run.callback.is_some() || !run.summons.is_empty())
        {
            w.cache.inventory.magick.active.push(run)
        }
        if !all_resisted {
            w.refresh_spell_state();
            for (owner, entry, play) in attachments {
                let fx = self.attach_eye(w, owner, entry, &play, now);
                if !fx.is_null() {
                    effects.push(fx)
                }
            }
        }
        result["targets"] = json!(applied);
        if let Some(fx) = effects.first() {
            result["target_effect"] = fx.clone()
        }
        result["effects"] = json!(effects);
        result["outcome"] = json!(if all_resisted {
            "resisted"
        } else if failed {
            "failed"
        } else {
            "success"
        });
        if let Some(Subject::Lock(_, lock)) = targets.first().map(|t| &t.subject) {
            result["flags_after"] = json!(w.cache.door_states.flags(*lock, targets[0].flags));
            result["target"] = json!(lock.label())
        }
        if w.cache.inventory.combat.active() {
            let xp = w.cache.inventory.character.base[18].saturating_sub(xp_before);
            w.cache.inventory.combat.log(
                json!({"event":"spell", "caster":"player", "spell":id, "outcome":result["outcome"],
                "targets":applied, "cost":4, "fatigue":fatigue, "item_mana":mana, "xp":xp}),
            );
        }
        let message = if all_resisted {
            format!("{name} resisted")
        } else if failed {
            format!("{name} failed")
        } else {
            information.unwrap_or_else(|| format!("{name} succeeded"))
        };
        Ok((message, result))
    }
}

fn apply_vital(inv: &mut Session, t: &mut Target, c: &Component, apt: i32, resisted: i32) {
    let Operation::Vital {
        heal,
        channel,
        min,
        max,
        flags,
    } = &c.operation
    else {
        return;
    };
    if t.kind != Kind::Actor
        && (t.item.is_none()
            || t.maximum <= 0
            || matches!(channel, Channel::Poison | Channel::Fatigue))
    {
        return;
    }
    let mut amount = if has(flags, "full") {
        if *heal { i32::MAX } else { t.maximum }
    } else if has(flags, "scaled") {
        min.saturating_add(((i64::from(apt.max(0)) * i64::from(max - min)) / 100) as i32)
    } else {
        min.saturating_add(inv.skill_random.below((i64::from(max - min) + 1) as u64) as i32)
    };
    if !heal {
        if resisted > 0 {
            amount = amount
                .saturating_sub((amount.saturating_mul(resisted) / 100).max(1))
                .max(0)
        }
        amount = channel.after_resistance(amount, &t.resistance);
        if !matches!(channel, Channel::Fatigue | Channel::Poison) {
            amount = inv.difficulty.adjust(amount);
        }
    }
    let before = t.hp;
    inv.change_vitals(t, *channel, if *heal { -amount } else { amount });
    if !heal {
        inv.note_death(t, before, None);
        inv.combat_reward(t, before);
    }
    if *heal && has(flags, "full") {
        inv.change_vitals(t, Channel::Fatigue, -i32::MAX);
        inv.permanent_flags(&t.subject, Field::Critter, 0x780, false)
    }
    if *heal && has(flags, "crippling") {
        inv.permanent_flags(&t.subject, Field::Critter, 0x700, false)
    }
}

impl InteractiveWorld {
    fn refresh_spell_state(&mut self) {
        self.cache.inventory.combat.rescan();
        self.cache.inventory.refresh_magick();
        self.cache.update_character_movement();
        if self.cache.player_spell_flags() & 0x01000000 != 0
            && let Some(player) = &mut self.cache.player
        {
            player.cancel_route();
        }
        self.cache.activate(&self.cache.active.clone());
    }
    pub fn maintained_spells(&self) -> impl Iterator<Item = usize> + '_ {
        self.cache
            .inventory
            .magick
            .active
            .iter()
            .filter(|r| {
                !r.ending
                    && self
                        .cache
                        .inventory
                        .program(r.spell)
                        .is_ok_and(|p| p.maintain.is_some())
            })
            .map(|r| r.spell)
    }
}

impl InteractiveWorld {
    pub(in crate::content::world) fn relocate_actor(
        &mut self,
        id: OriginalId,
        key: Option<WorldOrder>,
        tile: Tile,
        to: Tile,
    ) {
        if let Some(s) = self
            .cache
            .inventory
            .magick
            .active
            .iter_mut()
            .flat_map(|r| &mut r.summons)
            .find(|s| s.id == id)
        {
            s.tile = to
        } else if let Some(key) = key {
            if !self.cache.inventory.magick.moved.contains_key(&id) {
                let (r, _) = self.cache.npc(key).expect("validated mobile target");
                let frame = self
                    .cache
                    .ordered
                    .iter()
                    .find(|r| r.key == key)
                    .unwrap()
                    .frame
                    .clone();
                let r = Arc::new(r.clone());
                self.cache.inventory.magick.moved.insert(id, (r, frame));
            }
            let inv = &mut self.cache.inventory;
            if let Some(p) = inv.magick.offsets.get_mut(&id.key()) {
                p.tile = to
            } else {
                let sequence = inv.state.next_generated;
                inv.state.next_generated += 1;
                inv.magick.offsets.insert(
                    id.key(),
                    Placement {
                        map: self.map.clone(),
                        source: tile,
                        tile: to,
                        sequence,
                    },
                );
            }
        }
    }
}
