//! Original SCR framing, execution and persistent attachment state.
use super::{
    Source,
    quest::{self, Ledger, Origin, Status},
    read_bounded,
};
use crate::{random::Random, vfs::Vfs};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    sync::Arc,
};

mod timers;

const STEP_BUDGET: usize = 1000;
const GLOBAL_FLAGS: i32 = 3200;
const GLOBAL_VARIABLES: i32 = 2000;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, Ord, PartialOrd)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Event {
    Unlock,
    Use,
    BuyObject,
    Dying,
    FirstHeartbeat,
    Heartbeat,
    Dialogue,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct Local {
    flags: u32,
    counters: u32,
}

impl Local {
    pub(crate) fn new(flags: u32, counters: u32) -> Self {
        Self { flags, counters }
    }

    pub(crate) fn from_attachment(bytes: &[u8]) -> Self {
        Self::new(word(bytes, 0), word(bytes, 4))
    }

    fn flag(self, index: i32) -> Option<i32> {
        Some(i32::from(self.flags & 1u32.checked_shl(index as u32)? != 0))
    }

    fn counter(self, index: i32) -> Option<i32> {
        (0..4)
            .contains(&index)
            .then(|| ((self.counters >> (8 * index)) & 255) as i32)
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct State {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(super) sector_toggles: BTreeMap<String, BTreeSet<u64>>,
    #[serde(skip)]
    pub(super) sector_map: Option<Arc<(String, BTreeSet<u64>)>>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(super) detached: BTreeMap<String, BTreeSet<Event>>,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub(super) switched_off: BTreeSet<String>,
    #[serde(skip)]
    pub(super) heartbeat: Option<HeartbeatContext>,
    flags: BTreeMap<i32, bool>,
    variables: BTreeMap<i32, i32>,
    attachments: BTreeMap<String, BTreeMap<Event, Local>>,
    #[serde(default)]
    pub quests: Ledger,
    #[serde(default)]
    pub rumors: BTreeMap<i32, u64>,
    #[serde(skip)]
    slides: Option<Vec<i32>>,
    #[serde(default)]
    pub quelled_rumors: BTreeSet<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    random: Option<Random>,
    #[serde(skip)]
    pub(in crate::content) print_lines: Option<Vec<i32>>,
    #[serde(skip)]
    pub(super) game_minute: u64,
    #[serde(skip)]
    pub(super) game_second: u64,
    #[serde(skip)]
    pub(super) start_hour: u8,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    calls: BTreeMap<u64, std::collections::VecDeque<timers::Call>>,
    #[serde(default, skip_deserializing, skip_serializing_if = "Option::is_none")]
    last_call: Option<serde_json::Value>,
    #[serde(skip)]
    pub(super) interaction: Option<Interaction>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct HeartbeatContext {
    pub vicinity: Vec<Actor>,
    pub action: Option<ActorAction>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ActorAction {
    Kill,
    CallDialogue(usize),
}

/// The staged read view for a single SCR/DLG invocation. Operations commit with its state writes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Interaction {
    pub item_use: Option<ItemUse>,
    pub reaction: Option<i32>,
    pub aptitude: i32,
    pub alignment_base: i32,
    pub alignment_modifier: crate::content::character::ModifierSum,
    pub reaction_modifiers: [crate::content::character::ModifierSum; 2],
    pub quest_levels: Option<Arc<BTreeMap<i32, (i32, i32)>>>,
    pub skills: [crate::content::character::SkillRank; 16],
    pub stats: [Option<i32>; 28],
    pub vitals: [[i32; 2]; 2],
    pub spell_flags: u32,
    pub gold: [i64; 2],
    pub items: Vec<(u8, u32, String)>,
    pub substitute: Option<String>,
    pub operations: Vec<Transfer>,
}

/// Staged extra-object context for an item Use attachment.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct ItemUse {
    pub target_stats: [Option<i32>; 28],
    pub power_ratio: i32,
    pub destroy: bool,
    pub poison_heal: i32,
    pub visuals: Vec<i32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Transfer {
    Attack,
    Training(usize, u32),
    QuestExperience(i32),
    Reaction(i32),
    Heal { fatigue: bool, amount: i32 },
    Alignment(crate::content::character::progression::Alignment),
    Gold { actor: u8, delta: i64 },
    Item { id: String, from: u8, to: u8 },
}

impl Interaction {
    pub fn align(&mut self, change: crate::content::character::progression::Alignment) {
        self.alignment_base = change.apply(self.alignment_base);
        self.stats[19] = Some(
            self.alignment_modifier
                .apply(self.alignment_base)
                .clamp(-1000, 1000),
        );
        self.operations.push(Transfer::Alignment(change));
    }
    pub fn react(
        &mut self,
        change: crate::content::character::progression::Alignment,
    ) -> Result<(), String> {
        let before = self
            .reaction
            .ok_or("Trainer or speaker reaction unavailable")?;
        let delta = change.value(before).saturating_sub(before);
        if delta != 0 {
            let delta = self.reaction_modifiers[usize::from(delta > 0)].apply(delta);
            self.reaction = Some(before.saturating_add(delta));
            self.operations.push(Transfer::Reaction(delta));
        }
        Ok(())
    }
    pub fn train(&mut self, skill: usize, tier: u32) -> Result<(), String> {
        let rank = self.skills.get_mut(skill).ok_or("Unknown training skill")?;
        crate::content::character::progression::validate_training(rank, tier)?;
        rank.training = tier;
        self.operations.push(Transfer::Training(skill, tier));
        Ok(())
    }
    pub fn adjust(&mut self, actor: u8, delta: i64) -> Result<(), String> {
        let balance = self.gold[usize::from(actor)] + delta;
        if !(0..=i64::from(i32::MAX)).contains(&balance) {
            return Err("Insufficient gold or currency limit exceeded".into());
        }
        self.gold[usize::from(actor)] = balance;
        self.operations.push(Transfer::Gold { actor, delta });
        Ok(())
    }
    pub fn give(&mut self, name: u32, from: u8, to: u8, include_stock: bool) -> Result<(), String> {
        let index = self
            .items
            .iter()
            .position(|(side, n, _)| *side == from && *n == name)
            .or_else(|| {
                include_stock
                    .then(|| {
                        self.items
                            .iter()
                            .position(|(side, n, _)| *side == 2 && *n == name)
                    })
                    .flatten()
            })
            .ok_or_else(|| format!("Source does not own item named {name}"))?;
        let entry = &mut self.items[index];
        self.operations.push(Transfer::Item {
            id: entry.2.clone(),
            from: entry.0,
            to,
        });
        entry.0 = to;
        Ok(())
    }
}

fn actor(focus: u8) -> Option<u8> {
    match focus {
        0 | 21 => Some(0),
        1 => Some(1),
        _ => None,
    }
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct Attachment {
    pub owner: String,
    pub event: Event,
    pub script: i32,
    #[serde(skip)]
    pub initial: Local,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Slot {
    Quest(i32),
    GlobalFlag(i32),
    GlobalVariable(i32),
    LocalFlag(i32),
    Counter(i32),
}

impl Slot {
    pub(crate) fn dlg(code: &str, index: i32) -> Option<Self> {
        Some(match code {
            "qu" if quest::valid_id(index) => Self::Quest(index),
            "gf" if (0..GLOBAL_FLAGS).contains(&index) => Self::GlobalFlag(index),
            "gv" if (0..GLOBAL_VARIABLES).contains(&index) => Self::GlobalVariable(index),
            "lf" if (0..32).contains(&index) => Self::LocalFlag(index),
            "lc" if (0..4).contains(&index) => Self::Counter(index),
            _ => return None,
        })
    }

    pub(crate) fn accepts(self, value: i32) -> bool {
        match self {
            Self::GlobalFlag(_) | Self::LocalFlag(_) => matches!(value, 0 | 1),
            Self::Quest(_) => Status::decode(value).is_some(),
            Self::Counter(_) => (0..=255).contains(&value),
            Self::GlobalVariable(_) => true,
        }
    }
}

impl State {
    pub(super) fn set_player_quest(
        &mut self,
        id: i32,
        requested: Status,
        origin: impl FnOnce() -> Origin,
    ) -> Option<()> {
        let old = self.quests.status(id, false)?;
        self.quests
            .set_player(id, requested, self.game_minute, origin)?;
        if old < Status::Completed
            && requested == Status::Completed
            && let Some(facts) = self.interaction.as_mut()
            && let Some(levels) = &facts.quest_levels
        {
            let &(level, adjustment) = levels.get(&id)?;
            facts.operations.push(Transfer::QuestExperience(level));
            if self.quests.status(id, false) == Some(Status::Completed) {
                facts.align(crate::content::character::progression::Alignment::Add(
                    adjustment,
                ));
            }
        }
        Some(())
    }
    pub(super) fn learn_rumor(&mut self, id: i32, quell: bool) -> Option<()> {
        if !(1000..3000).contains(&id) {
            return None;
        }
        if quell {
            self.quelled_rumors.insert(id);
        } else {
            self.rumors.entry(id).or_insert(self.game_minute);
        }
        Some(())
    }
    pub(super) fn choose_rumor(&mut self, mut ids: Vec<i32>) -> Option<i32> {
        ids.retain(|id| !self.rumors.contains_key(id) && !self.quelled_rumors.contains(id));
        if ids.is_empty() {
            return None;
        }
        let index = self
            .random
            .get_or_insert_with(Random::fresh)
            .below(ids.len() as u64);
        Some(ids[index as usize])
    }
    pub(super) fn initialize_random(&mut self) {
        self.random.get_or_insert_with(Random::fresh);
    }

    pub fn validate_maps(&self, catalog: &super::maps::Catalog) -> Result<(), String> {
        for map in self.sector_toggles.keys() {
            if !catalog.maps.iter().any(|entry| &entry.directory == map) {
                return Err(format!("Saved sector block map {map:?} is unavailable"));
            }
        }
        Ok(())
    }

    fn local(&self, attachment: &Attachment) -> Local {
        self.attachments
            .get(&attachment.owner)
            .and_then(|slots| slots.get(&attachment.event))
            .copied()
            .unwrap_or(attachment.initial)
    }

    pub(crate) fn read(&self, attachment: &Attachment, slot: Slot) -> Option<i32> {
        match slot {
            Slot::Quest(id) => self.quests.status(id, false).map(|status| status as i32),
            Slot::GlobalFlag(index) if (0..GLOBAL_FLAGS).contains(&index) => {
                Some(i32::from(self.flags.get(&index).copied().unwrap_or(false)))
            }
            Slot::GlobalVariable(index) if (0..GLOBAL_VARIABLES).contains(&index) => {
                Some(self.variables.get(&index).copied().unwrap_or(0))
            }
            Slot::LocalFlag(index) => self.local(attachment).flag(index),
            Slot::Counter(index) => self.local(attachment).counter(index),
            _ => None,
        }
    }

    pub(crate) fn read_dialogue(&self, attachment: &Attachment, slot: Slot) -> Option<i32> {
        if let Slot::Counter(index) = slot {
            // CE's DLG accessor shifts the packed counters without the SCR byte mask.
            return (0..4)
                .contains(&index)
                .then(|| (self.local(attachment).counters >> (8 * index)) as i32);
        }
        self.read(attachment, slot)
    }

    pub(in crate::content) fn global_flag(&self, index: i32) -> bool {
        self.flags.get(&index).copied().unwrap_or(false)
    }

    pub(crate) fn set_global_flag(&mut self, index: i32) -> Result<(), String> {
        if !(0..GLOBAL_FLAGS).contains(&index) {
            return Err("Invalid starting global flag".into());
        }
        self.flags.insert(index, true);
        Ok(())
    }

    pub(crate) fn write(&mut self, attachment: &Attachment, slot: Slot, value: i32) -> Option<()> {
        self.read(attachment, slot)?;
        if !slot.accepts(value) {
            return None;
        }
        match slot {
            Slot::Quest(_) => return None,
            Slot::GlobalFlag(index) => {
                if value == 0 {
                    self.flags.remove(&index);
                } else {
                    self.flags.insert(index, true);
                }
            }
            Slot::GlobalVariable(index) => {
                if value == 0 {
                    self.variables.remove(&index);
                } else {
                    self.variables.insert(index, value);
                }
            }
            Slot::LocalFlag(index) => {
                let mut local = self.local(attachment);
                local.flags = (local.flags & !(1 << index)) | ((value as u32) << index);
                self.set_local(attachment, local);
            }
            Slot::Counter(index) => {
                let mut local = self.local(attachment);
                let shift = 8 * index;
                local.counters = (local.counters & !(255 << shift)) | ((value as u32) << shift);
                self.set_local(attachment, local);
            }
        }
        Some(())
    }

    fn write_scr(&mut self, attachment: &Attachment, slot: Slot, value: i32) -> Option<()> {
        // CE's SCR setter replaces the addressed field and ORs the shifted,
        // unmasked value into its containing word.
        match slot {
            Slot::Counter(index) if (0..4).contains(&index) => {
                let mut local = self.local(attachment);
                let shift = 8 * index;
                local.counters = (local.counters & !(255 << shift)) | ((value as u32) << shift);
                self.set_local(attachment, local);
                Some(())
            }
            Slot::GlobalFlag(index) if (0..GLOBAL_FLAGS).contains(&index) => {
                let bits = (value as u32) << (index % 32);
                self.write(attachment, slot, value & 1)?;
                // The rest of the word retains old flags and gains any bits
                // set by the value. Bits outside the word are discarded.
                let base = index / 32 * 32;
                let mut extra = bits & !(1u32 << (index % 32));
                while extra != 0 {
                    self.flags
                        .insert(base + extra.trailing_zeros() as i32, true);
                    extra &= extra - 1;
                }
                Some(())
            }
            _ => self.write(attachment, slot, value),
        }
    }

    fn set_local(&mut self, attachment: &Attachment, local: Local) {
        self.attachments
            .entry(attachment.owner.clone())
            .or_default()
            .insert(attachment.event, local);
    }

    pub(super) fn validate(&self) -> Result<(), String> {
        if self
            .calls
            .values()
            .any(|batch| batch.is_empty() || batch.iter().any(|c| c.script <= 0))
        {
            return Err("Invalid saved script timer".into());
        }
        if self.flags.keys().any(|&i| !(0..GLOBAL_FLAGS).contains(&i))
            || self
                .variables
                .keys()
                .any(|&i| !(0..GLOBAL_VARIABLES).contains(&i))
        {
            return Err("Invalid saved script global index".into());
        }
        if self.sector_toggles.iter().any(|(map, ids)| {
            map.is_empty()
                || map.contains(['/', '\\'])
                || ids
                    .iter()
                    .any(|&id| crate::sector::Coordinates::from_id(id).is_none())
        }) {
            return Err("Invalid saved sector block identity".into());
        }
        self.quests.validate()
    }
}

pub(crate) struct Invocation<'a> {
    pub attachment: &'a Attachment,
    pub triggerer: Actor,
    pub position: usize,
    pub hour: u8,
    pub follows_player: Option<bool>,
    pub npc_flags: Option<u32>,
    pub locations: [Option<(u32, u32)>; 2],
    pub met: Option<bool>,
}

impl Invocation<'_> {
    fn location(&self, focus: u8) -> Option<(u32, u32)> {
        match focus {
            0 | 21 => self.locations[0],
            1 => self.locations[1],
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Actor {
    Player,
    Item(u32),
    Mobile,
    None,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum Outcome {
    Return { run_default: bool },
    Dialogue { line: i32 },
}

#[derive(Debug, Serialize)]
pub(crate) struct Execution {
    pub statement: usize,
    pub outcome: Outcome,
}

#[derive(Debug, Serialize)]
pub(crate) struct Fault {
    pub statement: Option<usize>,
    pub reason: String,
    #[serde(flatten)]
    operation: Option<OperationFault>,
}

#[derive(Debug, Serialize)]
struct OperationFault {
    opcode: u32,
    operation: &'static str,
    name: Option<&'static str>,
}

impl fmt::Display for Fault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.statement {
            Some(index) => write!(f, "{} at statement {index}", self.reason),
            None => f.write_str(&self.reason),
        }
    }
}

pub(crate) struct Script {
    initial: Local,
    pub source: Source,
    code: Code,
}

pub(super) struct Code {
    flags: u32,
    pub statements: Vec<Statement>,
}

pub(super) struct Statement {
    pub condition: Operation,
    pub action: Operation,
    pub(super) otherwise: Operation,
}

pub(super) struct Operation {
    pub opcode: u32,
    types: [u8; 8],
    values: [i32; 8],
}

impl Operation {
    fn decode(bytes: &[u8]) -> Self {
        Self {
            opcode: word(bytes, 0),
            types: bytes[4..12].try_into().unwrap(),
            values: std::array::from_fn(|i| word(bytes, 12 + i * 4) as i32),
        }
    }

    pub fn literal(&self, index: usize) -> Option<i32> {
        (self.types[index] == 3).then_some(self.values[index])
    }

    pub fn focus(&self, index: usize) -> u8 {
        self.types[index]
    }

    fn operand(&self, index: usize) -> Option<Operand> {
        let value = self.values[index];
        Some(match self.types[index] {
            0 => Operand::State(Slot::Counter(value)),
            1 => Operand::State(Slot::GlobalVariable(value)),
            2 if (0..10).contains(&value) => Operand::Temporary(value as usize),
            3 => Operand::Literal(value),
            4 => Operand::State(Slot::GlobalFlag(value)),
            _ => return None,
        })
    }
}

fn sector_operand(x: Option<i32>, y: Option<i32>) -> Option<u64> {
    crate::sector::Coordinates {
        x: u32::try_from(x?).ok()? / 64,
        y: u32::try_from(y?).ok()? / 64,
    }
    .id()
}

enum Operand {
    State(Slot),
    Temporary(usize),
    Literal(i32),
}

fn word(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

impl Code {
    pub fn decode(bytes: &[u8]) -> Result<Self, &'static str> {
        let header = bytes.get(..64).ok_or("truncated script header")?;
        let (rows, tail) = bytes[64..].as_chunks::<132>();
        if !tail.is_empty() || rows.len() != word(header, 52) as usize {
            return Err("invalid script length");
        }
        Ok(Self {
            flags: word(header, 48),
            statements: rows
                .iter()
                .map(|row| Statement {
                    condition: Operation::decode(&row[..44]),
                    action: Operation::decode(&row[44..88]),
                    otherwise: Operation::decode(&row[88..]),
                })
                .collect(),
        })
    }

    pub fn check_flags(&self) -> Result<(), Fault> {
        // Trap, auto-removal, speech eligibility and movement-trigger flags
        // have effects outside this interpreter's supported event lifecycle.
        if self.flags != 0 {
            return Err(Fault {
                statement: None,
                operation: None,
                reason: format!("unsupported script flags {:#x}", self.flags),
            });
        }
        Ok(())
    }
}

pub(super) fn path(vfs: &Vfs, id: i32) -> Result<String, String> {
    let prefix = format!("scr/{id:05}");
    let paths: Vec<_> = vfs
        .paths_with_prefix(&prefix)
        .into_iter()
        .filter(|path| path.ends_with(".scr") && !path[prefix.len()..].contains('/'))
        .collect();
    let [path] = paths.as_slice() else {
        return Err(format!("missing or ambiguous script {id}"));
    };
    Ok((*path).to_owned())
}

impl Script {
    pub(crate) fn load(vfs: &mut Vfs, id: i32) -> Result<Self, String> {
        Self::load_path(vfs, &path(vfs, id)?)
    }

    pub(super) fn load_path(vfs: &mut Vfs, path: &str) -> Result<Self, String> {
        let (source, bytes) =
            read_bounded(vfs, path, crate::mes::MAX_INPUT_BYTES).map_err(|e| e.to_string())?;
        let code =
            Code::decode(&bytes).map_err(|error| format!("{path} from {source}: {error}"))?;
        Ok(Self {
            source,
            code,
            initial: Local::from_attachment(&bytes),
        })
    }

    pub(super) fn structure(&self) -> serde_json::Value {
        serde_json::json!({
            "status": "decoded",
            "statements": self.code.statements.len(),
            "flags": self.code.flags,
        })
    }

    pub(super) fn code(&self) -> &Code {
        &self.code
    }

    /// The caller supplies staged state and publishes it after the enclosing
    /// interaction (including any requested dialogue transition) succeeds.
    pub(crate) fn execute(
        &self,
        invocation: &Invocation<'_>,
        state: &mut State,
    ) -> Result<Execution, Fault> {
        self.code
            .run(invocation, View::Writable(state), &self.source)
    }

    pub(crate) fn inspect(
        &self,
        invocation: &Invocation<'_>,
        state: &State,
    ) -> Result<Execution, Fault> {
        self.code
            .run(invocation, View::ReadOnly(state), &self.source)
    }
}

enum View<'a> {
    Writable(&'a mut State),
    ReadOnly(&'a State),
}

impl View<'_> {
    fn state(&self) -> &State {
        match self {
            Self::Writable(state) => state,
            Self::ReadOnly(state) => state,
        }
    }

    fn write(&mut self, attachment: &Attachment, slot: Slot, value: i32) -> Option<()> {
        match self {
            Self::Writable(state) => state.write(attachment, slot, value),
            Self::ReadOnly(_) => None,
        }
    }
    fn assign(
        &mut self,
        attachment: &Attachment,
        target: Operand,
        value: i32,
        temporaries: &mut [i32; 10],
    ) -> Option<()> {
        match target {
            Operand::State(slot) => match self {
                Self::Writable(state) => state.write_scr(attachment, slot, value),
                Self::ReadOnly(_) => None,
            },
            Operand::Temporary(index) => {
                temporaries[index] = value;
                Some(())
            }
            // CE ignores writes to an immediate operand.
            Operand::Literal(_) => Some(()),
        }
    }

    fn draw_below(&mut self, limit: u64) -> Option<u64> {
        match self {
            Self::Writable(state) => Some(state.random.as_mut()?.below(limit)),
            Self::ReadOnly(_) => None,
        }
    }
}

fn read(
    operand: Option<Operand>,
    view: &View<'_>,
    attachment: &Attachment,
    temporaries: &[i32; 10],
) -> Option<i32> {
    match operand? {
        Operand::State(slot) => view.state().read(attachment, slot),
        Operand::Temporary(index) => Some(temporaries[index]),
        Operand::Literal(value) => Some(value),
    }
}

impl Code {
    fn run(
        &self,
        input: &Invocation<'_>,
        mut view: View<'_>,
        source: &Source,
    ) -> Result<Execution, Fault> {
        self.check_flags()?;
        let mut pc = input.position;
        let attachment = input.attachment;
        let mut temporaries = [0; 10];
        let mut loop_state: Option<(usize, usize)> = None;
        for _ in 0..STEP_BUDGET {
            let fault = |reason| Fault {
                statement: Some(pc),
                operation: None,
                reason,
            };
            let operation_fault = |operation, opcode, name| Fault {
                operation: Some(OperationFault {
                    opcode,
                    operation,
                    name,
                }),
                ..fault(match name {
                    Some(name) => format!("unsupported SCR {operation} {name} (opcode {opcode})"),
                    None => format!("unresolved script {operation} {opcode}"),
                })
            };
            let row = self
                .statements
                .get(pc)
                .ok_or_else(|| fault("script branch outside resource".into()))?;
            let value =
                |op: &Operation, index| read(op.operand(index), &view, attachment, &temporaries);
            // Buy-object predicates receive the offered item as triggerer.
            let actor = |focus| {
                if focus == 0 && !matches!(input.triggerer, Actor::Player) {
                    None
                } else {
                    actor(focus)
                }
            };
            let extra_target = |focus| {
                focus == 12
                    && view
                        .state()
                        .interaction
                        .as_ref()
                        .is_some_and(|i| i.item_use.is_some())
            };
            let cond = &row.condition;
            let test = match cond.opcode {
                0 => Some(true), // SCT_TRUE
                1 => Some((6..18).contains(&input.hour)),
                2 => actor(cond.focus(0))
                    .zip(value(cond, 1))
                    .and_then(|(side, n)| {
                        Some(
                            view.state().interaction.as_ref()?.gold[usize::from(side)]
                                >= i64::from(n),
                        )
                    }),
                11..=15 if actor(cond.focus(0)) == Some(0) => view
                    .state()
                    .interaction
                    .as_ref()
                    .map(|s| match cond.opcode {
                        // The current player has no summoned followers.
                        11 => false,
                        12 => s.spell_flags & 0x20000 != 0,
                        13 => s.spell_flags & 0x80000 != 0,
                        14 => s.spell_flags & 0x3c != 0,
                        _ => s.spell_flags & 1 != 0,
                    }),
                17 => {
                    actor(cond.focus(0))
                        .zip(value(cond, 1))
                        .and_then(|(side, n)| {
                            Some(
                                view.state().interaction.as_ref()?.items.iter().any(
                                    |(s, name, _)| *s == side && i64::from(*name) == i64::from(n),
                                ),
                            )
                        })
                }
                3 => value(cond, 0)
                    .and_then(|index| view.state().read(attachment, Slot::LocalFlag(index)))
                    .map(|v| v != 0),
                4 | 5 => value(cond, 0)
                    .zip(value(cond, 1))
                    .map(|(a, b)| if cond.opcode == 4 { a == b } else { a <= b }),
                6 if actor(cond.focus(0)).is_some() => {
                    // Every currently supported event is triggered by the local PC.
                    let global = cond.focus(0) == 1;
                    value(cond, 1)
                        .zip(value(cond, 2))
                        .and_then(|(id, expected)| {
                            view.state()
                                .quests
                                .status(id, global)
                                .map(|status| status as i32 == expected)
                        })
                }
                7 => value(cond, 0)
                    .zip(value(cond, 1))
                    .and_then(|(id, expected)| {
                        view.state()
                            .quests
                            .status(id, true)
                            .map(|status| status as i32 == expected)
                    }),
                10 if cond.focus(0) == 1 && matches!(cond.focus(1), 0 | 21) => input.met,
                20 if cond.focus(0) == 0 => match input.triggerer {
                    Actor::Item(name) => value(cond, 1).map(|n| i64::from(n) == i64::from(name)),
                    Actor::Player | Actor::Mobile | Actor::None => None,
                },
                18 if [cond.focus(0), cond.focus(1)] == [1, 0] => input.follows_player,
                45 => sector_operand(value(cond, 0), value(cond, 1)).and_then(|id| {
                    let (map, base) = view.state().sector_map.as_deref()?;
                    Some(
                        base.contains(&id)
                            ^ view
                                .state()
                                .sector_toggles
                                .get(map)
                                .is_some_and(|set| set.contains(&id)),
                    )
                }),
                34 => input
                    .location(cond.focus(0))
                    .zip(value(cond, 1))
                    .zip(value(cond, 2).zip(value(cond, 3)))
                    .map(|(((obj_x, obj_y), range), (x, y))| {
                        let dx = (i64::from(obj_x) - i64::from(x)).abs();
                        let dy = (i64::from(obj_y) - i64::from(y)).abs();
                        dx.max(dy) <= i64::from(range)
                    }),
                40 if actor(cond.focus(0)) == Some(0) => {
                    value(cond, 1).map(|id| view.state().rumors.contains_key(&id))
                }
                41 => value(cond, 0).map(|id| view.state().quelled_rumors.contains(&id)),
                39 if cond.focus(0) == 1 => input.npc_flags.map(|flags| flags & 0x20 != 0),
                43 => value(cond, 0)
                    .and_then(|index| view.state().read(attachment, Slot::GlobalFlag(index)))
                    .map(|v| v != 0),
                _ => None,
            }
            .ok_or_else(|| {
                let name = match cond.opcode {
                    10 => Some("SCT_OBJ_MET_PC_BEFORE"),
                    34 => Some("SCT_OBJ_WITHIN_RANGE"),
                    _ => None,
                };
                operation_fault("condition", cond.opcode, name)
            })?;
            let action = if test { &row.action } else { &row.otherwise };
            let unresolved = || {
                let name = match action.opcode {
                    21 => Some("SAT_CRITTER_FOLLOW"),
                    29 => Some("SAT_GET_REACTION"),
                    33 => Some("SAT_GET_STAT"),
                    _ => None,
                };
                operation_fault("action", action.opcode, name)
            };
            let outcome = match action.opcode {
                0 => None,
                1 | 2 => Some(Outcome::Return {
                    run_default: action.opcode == 2,
                }),
                3 => {
                    pc = usize::try_from(value(action, 0).ok_or_else(unresolved)?)
                        .map_err(|_| fault("negative script branch".into()))?;
                    continue;
                }
                4 if attachment.event == Event::Dialogue
                    && matches!(input.triggerer, Actor::Player) =>
                {
                    let line = value(action, 0).filter(|&n| n > 0).ok_or_else(unresolved)?;
                    Some(Outcome::Dialogue { line })
                }
                5 if view.state().heartbeat.is_some() => {
                    let View::Writable(state) = &mut view else {
                        return Err(unresolved());
                    };
                    state
                        .detached
                        .entry(attachment.owner.clone())
                        .or_default()
                        .insert(attachment.event);
                    None
                }
                18 if action.focus(0) == 8 && loop_state.is_none() => {
                    let actors = &view
                        .state()
                        .heartbeat
                        .as_ref()
                        .ok_or_else(unresolved)?
                        .vicinity;
                    if actors.is_empty() {
                        pc = self
                            .statements
                            .iter()
                            .enumerate()
                            .skip(pc + 1)
                            .find(|(_, r)| r.condition.opcode == 0 && r.action.opcode == 19)
                            .map(|(n, _)| n)
                            .ok_or_else(unresolved)?;
                    } else {
                        loop_state = Some((pc + 1, 0));
                    }
                    None
                }
                19 => {
                    let (start, index) = loop_state.take().ok_or_else(unresolved)?;
                    let actors = &view
                        .state()
                        .heartbeat
                        .as_ref()
                        .ok_or_else(unresolved)?
                        .vicinity;
                    if index + 1 < actors.len() {
                        loop_state = Some((start, index + 1));
                        pc = start;
                        continue;
                    }
                    None
                }
                34 if action.focus(0) == 10 => {
                    let (_, i) = loop_state.ok_or_else(unresolved)?;
                    let actor = view
                        .state()
                        .heartbeat
                        .as_ref()
                        .and_then(|h| h.vicinity.get(i))
                        .ok_or_else(unresolved)?;
                    let kind = match actor {
                        Actor::Player => 15,
                        Actor::Mobile => 16,
                        _ => return Err(unresolved()),
                    };
                    view.assign(
                        attachment,
                        action.operand(1).ok_or_else(unresolved)?,
                        kind,
                        &mut temporaries,
                    )
                    .ok_or_else(unresolved)?;
                    None
                }
                56 if action.focus(0) == 1 && view.state().heartbeat.is_some() => {
                    let View::Writable(state) = &mut view else {
                        return Err(unresolved());
                    };
                    if !state.switched_off.remove(&attachment.owner) {
                        state.switched_off.insert(attachment.owner.clone());
                    }
                    None
                }
                58 | 81 if action.focus(0) == 1 && view.state().heartbeat.is_some() => {
                    let effect = if action.opcode == 58 {
                        ActorAction::Kill
                    } else {
                        let trigger = match action.focus(3) {
                            21 => Some(&Actor::Player),
                            10 => loop_state.and_then(|(_, i)| {
                                view.state().heartbeat.as_ref()?.vicinity.get(i)
                            }),
                            _ => None,
                        };
                        if value(action, 1) != Some(9) || trigger != Some(&Actor::Player) {
                            return Err(unresolved());
                        }
                        ActorAction::CallDialogue(
                            usize::try_from(value(action, 2).ok_or_else(unresolved)?)
                                .map_err(|_| unresolved())?,
                        )
                    };
                    let View::Writable(state) = &mut view else {
                        return Err(unresolved());
                    };
                    let action = &mut state.heartbeat.as_mut().unwrap().action;
                    if action.replace(effect).is_some() {
                        return Err(fault(
                            "Multiple actor action requests in one heartbeat are unsupported"
                                .into(),
                        ));
                    }
                    None
                }
                8 | 9 | 100 | 101 => {
                    let index = value(action, 0).ok_or_else(unresolved)?;
                    let slot = if action.opcode < 10 {
                        Slot::LocalFlag(index)
                    } else {
                        Slot::GlobalFlag(index)
                    };
                    view.write(
                        attachment,
                        slot,
                        i32::from(matches!(action.opcode, 8 | 100)),
                    )
                    .ok_or_else(unresolved)?;
                    None
                }
                131 => {
                    let id = value(action, 0).ok_or_else(unresolved)?;
                    let View::Writable(state) = &mut view else {
                        return Err(unresolved());
                    };
                    state.slides.as_mut().ok_or_else(unresolved)?.push(id);
                    None
                }
                50 | 51 => {
                    let id = value(action, 0).ok_or_else(unresolved)?;
                    if actor(action.focus(1)) != Some(0) {
                        return Err(unresolved());
                    }
                    let View::Writable(state) = &mut view else {
                        return Err(unresolved());
                    };
                    state
                        .learn_rumor(id, action.opcode == 51)
                        .ok_or_else(unresolved)?;
                    None
                }
                24 if view.state().print_lines.is_some() => {
                    let line = value(action, 0).ok_or_else(unresolved)?;
                    let View::Writable(state) = &mut view else {
                        return Err(unresolved());
                    };
                    state.print_lines.as_mut().unwrap().push(line);
                    None
                }
                96..=98 | 105 => {
                    let minutes = view.state().game_minute;
                    let n = match action.opcode {
                        96 => minutes / 1440 + 1,
                        97 => u64::from(input.hour),
                        98 => minutes % 60,
                        _ => minutes / 60,
                    };
                    view.assign(
                        attachment,
                        action.operand(0).ok_or_else(unresolved)?,
                        n.min(i32::MAX as u64) as i32,
                        &mut temporaries,
                    )
                    .ok_or_else(unresolved)?;
                    None
                }
                54 | 55 if action.focus(3) == 1 => {
                    let script = value(action, 0).filter(|&n| n > 0).ok_or_else(unresolved)?;
                    let line = value(action, 1)
                        .and_then(|n| usize::try_from(n).ok())
                        .ok_or_else(unresolved)?;
                    let seconds = value(action, 4)
                        .and_then(|n| u64::try_from(n).ok())
                        .ok_or_else(unresolved)?;
                    let triggerer = match action.focus(2) {
                        21 => Actor::Player,
                        0 if matches!(input.triggerer, Actor::Player | Actor::None) => {
                            input.triggerer
                        }
                        _ => return Err(unresolved()),
                    };
                    let View::Writable(state) = &mut view else {
                        return Err(unresolved());
                    };
                    let now = state.game_second;
                    let delay = if action.opcode == 54 {
                        seconds.saturating_mul(8)
                    } else {
                        (seconds % 86400 + u64::from(state.start_hour) * 3600 + 86400 - now % 86400)
                            % 86400
                    };
                    state
                        .calls
                        .entry(now.saturating_add(delay.max(1)))
                        .or_default()
                        .push_back(timers::Call {
                            script,
                            line,
                            owner: attachment.owner.clone(),
                            triggerer,
                            map: state.sector_map.as_ref().map(|pair| pair.0.clone()),
                        });
                    None
                }
                125 if action.focus(1) == 1 => {
                    let id = value(action, 0).ok_or_else(unresolved)?;
                    let View::Writable(state) = &mut view else {
                        return Err(unresolved());
                    };
                    if let Some((due, index)) = state.calls.iter().find_map(|(&due, b)| {
                        b.iter()
                            .position(|c| c.script == id && c.owner == attachment.owner)
                            .map(|i| (due, i))
                    }) {
                        let batch = state.calls.get_mut(&due).unwrap();
                        batch.remove(index);
                        if batch.is_empty() {
                            state.calls.remove(&due);
                        }
                    }
                    None
                }
                10..=13 => {
                    let rhs = value(action, 1).ok_or_else(unresolved)?;
                    let assigned = match action.opcode {
                        11 => rhs.wrapping_add(value(action, 2).ok_or_else(unresolved)?),
                        13 => rhs.wrapping_mul(value(action, 2).ok_or_else(unresolved)?),
                        12 => rhs.wrapping_sub(value(action, 2).ok_or_else(unresolved)?),
                        _ => rhs,
                    };
                    let target = action.operand(0).ok_or_else(unresolved)?;
                    view.assign(attachment, target, assigned, &mut temporaries)
                        .ok_or_else(unresolved)?;
                    None
                }
                16 | 17 => {
                    let pc_quest = action.opcode == 16;
                    if pc_quest && !matches!(action.focus(0), 0 | 21) {
                        return Err(unresolved());
                    }
                    let i = usize::from(pc_quest);
                    let id = value(action, i)
                        .filter(|&id| quest::valid_id(id))
                        .ok_or_else(unresolved)?;
                    let status = value(action, i + 1)
                        .and_then(Status::decode)
                        .ok_or_else(unresolved)?;
                    let View::Writable(state) = &mut view else {
                        return Err(unresolved());
                    };
                    if pc_quest {
                        state.set_player_quest(id, status, || Origin {
                            resource: source.virtual_path.clone(),
                            mount: source.to_string(),
                            line: pc,
                            object: attachment.owner.clone(),
                        });
                    } else {
                        state.quests.set_global(id, status);
                    }
                    None
                }
                29 | 33 | 47 => {
                    let facts = view.state().interaction.as_ref().ok_or_else(unresolved)?;
                    let result = match action.opcode {
                        29 if actor(action.focus(0)) == Some(1)
                            && actor(action.focus(1)) == Some(0) =>
                        {
                            facts.reaction
                        }
                        33 if extra_target(action.focus(1)) => value(action, 0).and_then(|i| {
                            facts
                                .item_use
                                .as_ref()?
                                .target_stats
                                .get(i as usize)
                                .copied()
                                .flatten()
                        }),
                        33 if actor(action.focus(1)) == Some(0) && value(action, 0) == Some(16) => {
                            Some(facts.aptitude)
                        }
                        47 if actor(action.focus(1)) == Some(0) => value(action, 0)
                            .and_then(|i| facts.skills.get(i as usize).map(|s| s.level)),
                        33 if actor(action.focus(1)) == Some(0) => value(action, 0)
                            .and_then(|i| facts.stats.get(i as usize).copied().flatten()),
                        _ => None,
                    }
                    .ok_or_else(unresolved)?;
                    view.assign(
                        attachment,
                        action.operand(2).ok_or_else(unresolved)?,
                        result,
                        &mut temporaries,
                    )
                    .ok_or_else(unresolved)?;
                    None
                }
                91 if action.focus(1) == 1 && extra_target(action.focus(2)) => {
                    let ratio = view
                        .state()
                        .interaction
                        .as_ref()
                        .unwrap()
                        .item_use
                        .as_ref()
                        .unwrap()
                        .power_ratio;
                    let amount = value(action, 0).ok_or_else(unresolved)?;
                    view.assign(
                        attachment,
                        action.operand(3).ok_or_else(unresolved)?,
                        ((i64::from(amount) * i64::from(ratio) + 50) / 100) as i32,
                        &mut temporaries,
                    )
                    .ok_or_else(unresolved)?;
                    None
                }
                68 | 120 | 128 => {
                    let amount = match action.opcode {
                        68 if action.focus(0) == 1 => 0,
                        120 if extra_target(action.focus(1)) => {
                            value(action, 0).ok_or_else(unresolved)?
                        }
                        128 if extra_target(action.focus(0)) => {
                            value(action, 1).ok_or_else(unresolved)?
                        }
                        _ => return Err(unresolved()),
                    };
                    let View::Writable(state) = &mut view else {
                        return Err(unresolved());
                    };
                    let item = state
                        .interaction
                        .as_mut()
                        .and_then(|i| i.item_use.as_mut())
                        .ok_or_else(unresolved)?;
                    match action.opcode {
                        68 => item.destroy = true,
                        120 => item.visuals.push(amount),
                        _ => {
                            item.poison_heal = item.poison_heal.saturating_add(amount);
                            if let Some(n) = &mut item.target_stats[24] {
                                *n = n.saturating_sub(amount).max(0);
                            }
                        }
                    }
                    None
                }
                107 | 108
                    if extra_target(action.focus(0))
                        || (actor(action.focus(0)) == Some(0)
                            && view
                                .state()
                                .interaction
                                .as_ref()
                                .is_none_or(|i| i.item_use.is_none())) =>
                {
                    let pair = view
                        .state()
                        .interaction
                        .as_ref()
                        .ok_or_else(unresolved)?
                        .vitals[usize::from(action.opcode == 108)];
                    for (i, n) in pair.into_iter().enumerate() {
                        view.assign(
                            attachment,
                            action.operand(i + 1).ok_or_else(unresolved)?,
                            n,
                            &mut temporaries,
                        )
                        .ok_or_else(unresolved)?;
                    }
                    None
                }
                75 | 76
                    if extra_target(action.focus(0))
                        || (actor(action.focus(0)) == Some(0)
                            && view
                                .state()
                                .interaction
                                .as_ref()
                                .is_none_or(|i| i.item_use.is_none())) =>
                {
                    let amount = value(action, 1)
                        .filter(|&n| n >= 0)
                        .ok_or_else(unresolved)?;
                    let View::Writable(state) = &mut view else {
                        return Err(unresolved());
                    };
                    let facts = state.interaction.as_mut().ok_or_else(unresolved)?;
                    let fatigue = action.opcode == 76;
                    let pool = &mut facts.vitals[usize::from(fatigue)];
                    pool[0] = pool[0].saturating_add(amount).min(pool[1]);
                    facts.operations.push(Transfer::Heal { fatigue, amount });
                    None
                }
                137 if actor(action.focus(1)) == Some(0) && value(action, 0) == Some(19) => {
                    let n = value(action, 2).ok_or_else(unresolved)?;
                    let View::Writable(state) = &mut view else {
                        return Err(unresolved());
                    };
                    state
                        .interaction
                        .as_mut()
                        .ok_or_else(unresolved)?
                        .align(crate::content::character::progression::Alignment::Add(n));
                    None
                }
                63 if actor(action.focus(0)) == Some(0) => {
                    let amount = value(action, 1).ok_or_else(unresolved)?;
                    let View::Writable(state) = &mut view else {
                        return Err(unresolved());
                    };
                    state
                        .interaction
                        .as_mut()
                        .ok_or_else(unresolved)?
                        .operations
                        .push(Transfer::QuestExperience(amount));
                    None
                }
                36 if matches!(
                    (actor(action.focus(0)), actor(action.focus(1))),
                    (Some(0), Some(1)) | (Some(1), Some(0))
                ) =>
                {
                    let View::Writable(state) = &mut view else {
                        return Err(unresolved());
                    };
                    state
                        .interaction
                        .as_mut()
                        .ok_or_else(unresolved)?
                        .operations
                        .push(Transfer::Attack);
                    None
                }
                35 | 41 => {
                    let a =
                        value(action, usize::from(action.opcode == 35)).ok_or_else(unresolved)?;
                    let side = actor(action.focus(usize::from(action.opcode == 41)))
                        .ok_or_else(unresolved)?;
                    let View::Writable(state) = &mut view else {
                        return Err(unresolved());
                    };
                    let facts = state.interaction.as_mut().ok_or_else(unresolved)?;
                    if action.opcode == 35 {
                        facts.adjust(side, i64::from(a))
                    } else {
                        facts.give(
                            u32::try_from(a).map_err(|_| unresolved())?,
                            side,
                            actor(action.focus(2)).ok_or_else(unresolved)?,
                            false,
                        )
                    }
                    .map_err(fault)?;
                    None
                }
                106 => {
                    let id = sector_operand(value(action, 0), value(action, 1))
                        .ok_or_else(unresolved)?;
                    let View::Writable(state) = &mut view else {
                        return Err(unresolved());
                    };
                    let (map, _) = state.sector_map.as_deref().ok_or_else(unresolved)?;
                    let set = state.sector_toggles.entry(map.clone()).or_default();
                    if !set.remove(&id) {
                        set.insert(id);
                    }
                    if set.is_empty() {
                        state.sector_toggles.remove(map);
                    }
                    None
                }
                37 => {
                    let random_fault = |reason: &str| Fault {
                        reason: format!("SAT_RANDOM (opcode 37): {reason}"),
                        ..operation_fault("action", action.opcode, Some("SAT_RANDOM"))
                    };
                    let a = value(action, 0).ok_or_else(|| random_fault("invalid first bound"))?;
                    let b = value(action, 1).ok_or_else(|| random_fault("invalid second bound"))?;
                    let target = action
                        .operand(2)
                        .ok_or_else(|| random_fault("unsupported destination operand"))?;
                    // CE accepts either order and skips the draw for a singleton.
                    let low = i64::from(a.min(b));
                    let span = i64::from(a.max(b)) - low + 1;
                    let offset = if span == 1 {
                        0
                    } else {
                        view.draw_below(span as u64)
                            .ok_or_else(|| random_fault("no writable script random stream"))?
                    };
                    view.assign(
                        attachment,
                        target,
                        (low + offset as i64) as i32,
                        &mut temporaries,
                    )
                    .ok_or_else(|| random_fault("unwritable destination"))?;
                    None
                }
                _ => return Err(unresolved()),
            };
            if let Some(outcome) = outcome {
                return Ok(Execution {
                    statement: pc,
                    outcome,
                });
            }
            if pc + 1 == self.statements.len() {
                return Ok(Execution {
                    statement: pc,
                    outcome: Outcome::Return { run_default: false },
                });
            }
            pc += 1;
        }
        Err(Fault {
            statement: Some(pc),
            operation: None,
            reason: "script execution budget exhausted".into(),
        })
    }
}

/// A reversible equipment SCR loop. Decode both entry points and operands
/// before representing its Add/Remove multiplicity as a derived modifier.
pub(super) struct WieldEffect {
    pub id: i32,
    strength: i32,
    power_ratio: bool,
    pub sources: [Source; 2],
}
/// Original paired insert/remove SCRs store the effective stat in local
/// counter zero on insert. Removal divides that stored value again.
#[derive(Clone)]
pub(crate) struct CarryEffect {
    pub stat: usize,
    pub divisor: u8,
    pub adjustment: Arc<crate::content::character::Adjustments>,
}
impl CarryEffect {
    pub(crate) fn load(vfs: &mut Vfs, attachments: [(i32, Local); 2]) -> Result<Self, String> {
        let [(insert, locals), (remove, _)] = attachments;
        let enter = Script::load(vfs, insert)?;
        let exit = Script::load(vfs, remove)?;
        let invalid = || {
            format!(
                "Unsupported carry scripts {} and {}",
                enter.source, exit.source
            )
        };
        let [
            get,
            div,
            stop,
            add,
            dec,
            jump,
            again,
            empty,
            sub,
            decr,
            back,
        ] = enter.code.statements.as_slice()
        else {
            return Err(invalid());
        };
        let [call, ret] = exit.code.statements.as_slice() else {
            return Err(invalid());
        };
        let instruction = |s: &Statement, condition, action| {
            s.condition.opcode == condition && s.action.opcode == action && s.otherwise.opcode == 0
        };
        let arg = |op: &Operation, i: usize, t, v| op.types[i] == t && op.values[i] == v;
        let divide = |s: &Statement| {
            instruction(s, 0, 14)
                && arg(&s.action, 0, 2, 0)
                && arg(&s.action, 1, 0, 0)
                && arg(&s.action, 2, 0, 2)
        };
        let done = |s: &Statement| {
            instruction(s, 4, 2) && arg(&s.condition, 0, 2, 0) && s.condition.literal(1) == Some(0)
        };
        let decrement = |s: &Statement| {
            instruction(s, 0, 12)
                && arg(&s.action, 0, 2, 0)
                && arg(&s.action, 1, 2, 0)
                && s.action.literal(2) == Some(1)
        };
        if enter.code.check_flags().is_err()
            || exit.code.check_flags().is_err()
            || !instruction(get, 0, 33)
            || !arg(&get.action, 0, 0, 3)
            || get.action.focus(1) != 0
            || !arg(&get.action, 2, 0, 0)
            || !divide(div)
            || !divide(again)
            || !done(stop)
            || !done(empty)
            || !decrement(dec)
            || !decrement(decr)
            || !instruction(add, 0, 77)
            || add.action.focus(0) != 0
            || !arg(&add.action, 1, 0, 1)
            || add.action.literal(2) != Some(5)
            || !instruction(sub, 0, 78)
            || sub.action.focus(0) != 0
            || !arg(&sub.action, 1, 0, 1)
            || !instruction(jump, 0, 3)
            || jump.action.literal(0) != Some(2)
            || !instruction(back, 0, 3)
            || back.action.literal(0) != Some(7)
            || !instruction(call, 0, 81)
            || call.action.focus(0) != 1
            || call.action.literal(1) != Some(21)
            || call.action.literal(2) != Some(6)
            || !instruction(ret, 0, 2)
        {
            return Err(invalid());
        }
        let stat = locals.counter(3).unwrap() as usize;
        let divisor = locals.counter(2).unwrap() as u8;
        if stat >= 28 || divisor == 0 {
            return Err(invalid());
        }
        let effects =
            crate::content::load_mes(vfs, "rules/effect.mes").map_err(|e| e.to_string())?;
        let effect = effects
            .mes
            .get(locals.counter(1).unwrap())
            .ok_or_else(invalid)?;
        Ok(Self {
            stat,
            divisor,
            adjustment: Arc::new(crate::content::character::Adjustments::parse(effect)?),
        })
    }
}

/// CE slide_ui_prepare invokes a SAP_USE entry with null objects.
pub fn slide_sequence(vfs: &mut Vfs, script: i32) -> Result<Vec<i32>, String> {
    let program = Script::load(vfs, script)?;
    let attachment = Attachment {
        owner: String::new(),
        event: Event::Use,
        script,
        initial: Local::default(),
    };
    let invocation = Invocation {
        attachment: &attachment,
        triggerer: Actor::None,
        position: 0,
        hour: 0,
        follows_player: None,
        npc_flags: None,
        locations: [None, None],
        met: None,
    };
    let mut state = State {
        slides: Some(Vec::new()),
        ..State::default()
    };
    program
        .execute(&invocation, &mut state)
        .map_err(|e| format!("{}: {e}", program.source))?;
    Ok(state.slides.unwrap())
}

impl WieldEffect {
    pub fn load(vfs: &mut Vfs, attachments: [(i32, Local); 2]) -> Result<Self, String> {
        let [(on, initial), (off, _)] = attachments;
        let wear = Script::load(vfs, on)?;
        let unwear = Script::load(vfs, off)?;
        let unsupported = || {
            format!(
                "Unsupported wield behavior in {} or {}",
                wear.source, unwear.source
            )
        };
        let decode = || {
            wear.code.check_flags().ok()?;
            unwear.code.check_flags().ok()?;
            let [
                scale,
                save,
                done,
                add,
                decrement,
                jump,
                restore,
                empty,
                remove,
                reduce,
                again,
            ] = wear.code.statements.as_slice()
            else {
                return None;
            };
            let [call, ret] = unwear.code.statements.as_slice() else {
                return None;
            };
            if wear
                .code
                .statements
                .iter()
                .chain(&unwear.code.statements)
                .any(|s| s.otherwise.opcode != 0)
            {
                return None;
            }
            let op = |s: &Statement, condition: u32, action: u32| {
                s.condition.opcode == condition && s.action.opcode == action
            };
            let var = |op: &Operation, idx: usize, kind: u8, n: i32| {
                op.types[idx] == kind && op.values[idx] == n
            };
            let val = |op: &Operation, idx| match op.operand(idx)? {
                Operand::Literal(v) => Some(v),
                Operand::State(Slot::Counter(i)) => initial.counter(i),
                _ => None,
            };
            let power_ratio = scale.action.opcode == 91;
            if !op(scale, 0, if power_ratio { 91 } else { 80 })
                || scale.action.focus(1) != 1
                || scale.action.focus(2) != 0
                || (!power_ratio && scale.action.focus(3) != 0)
                || !var(&scale.action, if power_ratio { 3 } else { 4 }, 0, 0)
            {
                return None;
            }
            let strength = val(&scale.action, 0)?;
            let id = val(&add.action, 1)?;
            let copy =
                |s: &Statement| op(s, 0, 10) && var(&s.action, 0, 2, 0) && var(&s.action, 1, 0, 0);
            let stop = |s: &Statement| {
                op(s, 4, 2) && var(&s.condition, 0, 2, 0) && s.condition.literal(1) == Some(0)
            };
            let dec = |s: &Statement| {
                op(s, 0, 12)
                    && var(&s.action, 0, 2, 0)
                    && var(&s.action, 1, 2, 0)
                    && s.action.literal(2) == Some(1)
            };
            let branch = |s: &Statement, to| op(s, 0, 3) && s.action.literal(0) == Some(to);
            if !(0..=255).contains(&strength)
                || !copy(save)
                || !copy(restore)
                || !stop(done)
                || !stop(empty)
                || !dec(decrement)
                || !dec(reduce)
                || !branch(jump, 2)
                || !branch(again, 7)
                || !op(add, 0, 77)
                || add.action.focus(0) != 0
                || add.action.literal(2) != Some(5)
                || !op(remove, 0, 78)
                || remove.action.focus(0) != 0
                || val(&remove.action, 1) != Some(id)
                || !op(call, 0, 81)
                || call.action.focus(0) != 1
                || call.action.literal(1) != Some(24)
                || call.action.literal(2) != Some(6)
                || !op(ret, 0, 2)
            {
                return None;
            }
            Some((id, strength, power_ratio))
        };
        let (id, strength, power_ratio) = decode().ok_or_else(unsupported)?;
        Ok(Self {
            id,
            strength,
            power_ratio,
            sources: [wear.source, unwear.source],
        })
    }
    pub fn count(&self, complexity: i32, aptitude: i32) -> usize {
        let c = i64::from(complexity);
        let a = i64::from(aptitude);
        let ratio = if self.power_ratio {
            if c == 0 {
                100
            } else {
                100 * ((c + a) / 2).clamp(c.min(0), c.max(0)) / c
            }
        } else {
            100 - if c.signum() * a.signum() < 0 {
                a.abs()
            } else {
                0
            }
        };
        ((i64::from(self.strength) * ratio + if self.power_ratio { 50 } else { 0 }) / 100) as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dialogue_conditions_use_invocation_locations_and_meeting_history() {
        let bytes = original_rows(
            2,
            &[
                (64, 34),
                (68, 0x03030301),
                (80, 2),
                (84, 102),
                (88, 102),
                (152, 1), // Outside the attachee's range: return immediately.
                (196, 10),
                (200, 0x1501),
                (240, 4),
                (244, 3),
                (252, 205),
                (284, 4),
                (288, 3),
                (296, 1),
            ],
        );
        let code = Code::decode(&bytes).unwrap();
        let attachment = attachment("npc", Event::Dialogue);
        let mut input = invocation(&attachment);
        input.locations = [Some((104, 104)), Some((100, 100))];
        let state = State::default();
        let run = |input: &Invocation<'_>| code.run(input, View::ReadOnly(&state), &source());
        input.met = Some(false);
        assert_eq!(run(&input).unwrap().outcome, Outcome::Dialogue { line: 1 });
        input.met = Some(true);
        assert_eq!(
            run(&input).unwrap().outcome,
            Outcome::Dialogue { line: 205 }
        );
        input.locations[1] = Some((99, 100));
        assert_eq!(
            run(&input).unwrap().outcome,
            Outcome::Return { run_default: false }
        );
        input.locations[1] = None;
        assert_eq!(run(&input).unwrap_err().statement, Some(0));
    }

    #[test]
    fn sector_use_toggles_only_the_target_map_and_survives_serialization() {
        let code = Code::decode(&original_rows(
            2,
            &[
                (64, 45),
                (68, 0x0303),
                (76, 127),
                (80, 64),
                (108, 106),
                (112, 0x0303),
                (120, 65),
                (124, 100),
                (240, 2),
            ],
        ))
        .unwrap();
        let owner = attachment("door", Event::Use);
        let input = invocation(&owner);
        let sector = sector_operand(Some(127), Some(64)).unwrap();
        let mut state = State::default();
        let map = Arc::new(("exterior".to_owned(), BTreeSet::from([sector])));
        state.sector_map = Some(map.clone());
        for _ in 0..2 {
            code.run(&input, View::Writable(&mut state), &source())
                .unwrap();
            assert_eq!(state.sector_toggles["exterior"], BTreeSet::from([sector]));
            state = serde_json::from_slice(&serde_json::to_vec(&state).unwrap()).unwrap();
            state.sector_map = Some(map.clone());
        }
        state.sector_map = Some(Arc::new(("interior".to_owned(), BTreeSet::new())));
        code.run(&input, View::Writable(&mut state), &source())
            .unwrap();
        assert_eq!(state.sector_toggles.len(), 1);
    }

    #[test]
    fn original_quest_opcodes_use_distinct_pc_and_global_state() {
        let attachment = attachment("npc", Event::Dialogue);
        let input = Invocation {
            attachment: &attachment,
            triggerer: Actor::Player,
            position: 0,
            hour: 12,
            follows_player: None,
            npc_flags: None,
            locations: [None; 2],
            met: None,
        };
        let mut state = State::default();
        // Write PC Accepted, then test it on the triggerer and set global
        // Botched. A subsequent write resolves to Botched for the PC.
        let bytes = original_rows(
            3,
            &[
                (108, 16),
                (112, 0x030300),
                (124, 1009),
                (128, 2),
                (196, 6),
                (200, 0x030300),
                (212, 1009),
                (216, 2),
                (240, 17),
                (244, 0x0303),
                (252, 1009),
                (256, 6),
                (328, 7),
                (332, 0x0303),
                (340, 1009),
                (344, 6),
                (372, 16),
                (376, 0x030315),
                (388, 1009),
                (392, 3),
            ],
        );
        Code::decode(&bytes)
            .unwrap()
            .run(&input, View::Writable(&mut state), &source())
            .unwrap();
        assert_eq!(state.quests.status(1009, false), Some(Status::Botched));
        assert_eq!(state.quests.status(1009, true), Some(Status::Botched));
        assert_eq!(state.read(&attachment, Slot::GlobalFlag(1009)), Some(0));
    }

    #[test]
    fn script_item_and_gold_actions_update_the_staged_owners() {
        // Transfer the named item from the attachee to the triggerer, debit the
        // triggerer's purse, then evaluate has-item-named on its new owner.
        let bytes = original_rows(
            3,
            &[
                (108, 41),
                (112, 0x00000103),
                (120, 4402),
                (240, 35),
                (244, 0x0300),
                (256, (-100i32) as u32),
                (328, 17),
                (332, 0x0300),
                (344, 4402),
                (372, 2),
                (416, 1),
            ],
        );
        let owner = attachment("npc", Event::Dialogue);
        let context = invocation(&owner);
        let mut state = State {
            interaction: Some(Interaction {
                gold: [100, 0],
                items: vec![(1, 4402, "key".into())],
                ..Interaction::default()
            }),
            ..State::default()
        };
        let code = Code::decode(&bytes).unwrap();
        let result = code
            .run(&context, View::Writable(&mut state), &source())
            .unwrap();
        assert_eq!(result.outcome, Outcome::Return { run_default: true });
        let facts = state.interaction.unwrap();
        assert_eq!(facts.gold, [0, 0]);
        assert_eq!(facts.items[0].0, 0);
        assert_eq!(
            facts.operations,
            vec![
                Transfer::Item {
                    id: "key".into(),
                    from: 1,
                    to: 0
                },
                Transfer::Gold {
                    actor: 0,
                    delta: -100
                }
            ]
        );
    }

    fn source() -> Source {
        Source {
            virtual_path: "scr/fixture.scr".into(),
            mount_id: crate::vfs::MountId::GENERATED,
            mount_path: "test".into(),
            loose_path: None,
            storage: crate::vfs::Storage::Loose,
            mod_id: None,
        }
    }

    fn original_rows(count: usize, fields: &[(usize, u32)]) -> Vec<u8> {
        let mut bytes = vec![0; 64 + count * 132];
        bytes[52..56].copy_from_slice(&(count as u32).to_le_bytes());
        for &(offset, word) in fields {
            bytes[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
        }
        bytes
    }

    fn attachment(owner: &str, event: Event) -> Attachment {
        Attachment {
            owner: owner.into(),
            event,
            script: 10,
            initial: Local::new(1 << 2, 15 << 16),
        }
    }

    fn invocation(attachment: &Attachment) -> Invocation<'_> {
        Invocation {
            attachment,
            triggerer: Actor::Player,
            position: 0,
            hour: 12,
            follows_player: None,
            npc_flags: None,
            locations: [None; 2],
            met: None,
        }
    }

    #[test]
    #[ignore = "requires ARCANUM_GAME_DIR with the original Lloyd Gurloes dialogue and delayed script"]
    fn installed_delayed_dialogue_call_survives_save_and_executes_once() {
        let mut vfs = Vfs::from_game(
            std::env::var_os("ARCANUM_GAME_DIR").unwrap(),
            crate::vfs::Policy::Ce,
        )
        .unwrap();
        let script = Script::load(&mut vfs, 1140).unwrap();
        let mut a = attachment("timer-test", Event::Dialogue);
        a.script = 1140;
        let mut input = invocation(&a);
        input.position = 2;
        let mut state = State {
            game_second: 6000,
            ..State::default()
        };
        assert!(matches!(
            script.execute(&input, &mut state).unwrap().outcome,
            Outcome::Dialogue { .. }
        ));
        assert_eq!(state.next_call(), Some(7200));
        let mut state: State =
            serde_json::from_value(serde_json::to_value(state).unwrap()).unwrap();
        assert!(!state.tick_calls(&mut vfs, 7199, |_, _| Ok(())));
        assert!(!state.global_flag(2069));
        assert!(state.tick_calls(&mut vfs, 10000, |_, _| Ok(())));
        assert!(state.global_flag(2069));
        assert_eq!(state.next_call(), None);
        assert!(!state.tick_calls(&mut vfs, 11000, |_, _| Ok(())));
    }

    #[test]
    #[ignore = "requires installed Crash Site scripts"]
    fn installed_heartbeat_vicinity_and_starting_flags_drive_original_actor_actions() {
        let root = std::env::var_os("ARCANUM_GAME_DIR").expect("set ARCANUM_GAME_DIR");
        let mut vfs = Vfs::from_game(root, crate::vfs::Policy::Ce).unwrap();
        let mut owner = attachment("placed NPC", Event::Heartbeat);
        owner.initial = Local::default();
        owner.script = 1100;
        let program = Script::load(&mut vfs, owner.script).unwrap();
        let invoke = |program: &Script, owner: &Attachment, state: &mut State, nearby| {
            state.heartbeat = Some(HeartbeatContext {
                vicinity: nearby,
                action: None,
            });
            let mut input = invocation(owner);
            input.triggerer = Actor::Mobile;
            program.execute(&input, state).unwrap();
            state.heartbeat.take().unwrap().action
        };
        let mut state = State::default();
        assert_eq!(invoke(&program, &owner, &mut state, vec![]), None);
        assert_eq!(
            invoke(&program, &owner, &mut state, vec![Actor::Mobile]),
            None
        );
        assert!(state.detached.is_empty());
        assert_eq!(
            invoke(
                &program,
                &owner,
                &mut state,
                vec![Actor::Mobile, Actor::Player]
            ),
            Some(ActorAction::CallDialogue(4))
        );
        assert!(state.detached[&owner.owner].contains(&owner.event));

        owner.script = 1781;
        let program = Script::load(&mut vfs, owner.script).unwrap();
        let mut state = State::default();
        assert_eq!(
            invoke(&program, &owner, &mut state, vec![]),
            Some(ActorAction::Kill)
        );
        assert!(state.switched_off.is_empty());
        state.set_global_flag(2350).unwrap();
        assert_eq!(invoke(&program, &owner, &mut state, vec![]), None);
        let loaded: State = serde_json::from_slice(&serde_json::to_vec(&state).unwrap()).unwrap();
        assert!(loaded.switched_off.contains(&owner.owner));
        assert!(loaded.detached[&owner.owner].contains(&owner.event));
    }

    #[test]
    fn unlock_uses_live_global_conditions_and_returns() {
        let bytes = original_rows(
            3,
            &[
                (64, 4),      // SCT_EQ
                (68, 0x0304), // global flag, literal
                (76, 7),
                (80, 1),
                (64 + 88, 3), // false: GOTO literal row 2
                (64 + 92, 3),
                (64 + 100, 2),
                (64 + 132 + 44, 1),
                (64 + 264 + 44, 2),
            ],
        );
        let script = Code::decode(&bytes).unwrap();
        let attachment = attachment("fixture", Event::Unlock);
        let input = invocation(&attachment);
        let mut live = State::default();
        let run = |state: &State| {
            script
                .run(&input, View::ReadOnly(state), &source())
                .unwrap()
        };
        let before = run(&live);
        assert_eq!(before.statement, 2);
        assert_eq!(before.outcome, Outcome::Return { run_default: true });
        live.write(&attachment, Slot::GlobalFlag(7), 1).unwrap();
        let after = run(&live);
        assert_eq!(after.statement, 1);
        assert_eq!(after.outcome, Outcome::Return { run_default: false });
    }

    #[test]
    fn attachment_writes_and_temporary_reads_feed_dialogue_branches() {
        let bytes = original_rows(
            5,
            &[
                (64 + 44, 8),
                (64 + 48, 3),
                (64 + 56, 3), // set local flag 3
                (196 + 44, 10),
                (196 + 48, 0x0102),
                (196 + 60, 9), // temp 0 = gv9
                (328 + 44, 11),
                (328 + 48, 0x030201),
                (328 + 56, 9),
                (328 + 64, 1), // gv9 = temp 0 + 1
                (460, 4),
                (460 + 4, 0x0301),
                (460 + 12, 9),
                (460 + 16, 1), // gv9 == 1
                (460 + 44, 3),
                (460 + 48, 3),
                (460 + 56, 4),
                (460 + 88, 1),
                (592, 3),
                (592 + 4, 3),
                (592 + 12, 3), // local flag 3
                (592 + 44, 4),
                (592 + 48, 3),
                (592 + 56, 42),
            ],
        );
        let script = Code::decode(&bytes).unwrap();
        let first = attachment("first", Event::Dialogue);
        let second = attachment("second", Event::Dialogue);
        let other_slot = attachment("first", Event::Unlock);
        let mut state = State::default();
        let execution = script
            .run(&invocation(&first), View::Writable(&mut state), &source())
            .unwrap();
        assert_eq!(execution.statement, 4);
        assert_eq!(execution.outcome, Outcome::Dialogue { line: 42 });
        assert_eq!(state.read(&first, Slot::LocalFlag(2)), Some(1));
        assert_eq!(state.read(&first, Slot::LocalFlag(3)), Some(1));
        assert_eq!(state.read(&second, Slot::LocalFlag(3)), Some(0));
        assert_eq!(state.read(&other_slot, Slot::LocalFlag(3)), Some(0));
        assert_eq!(state.read(&first, Slot::Counter(2)), Some(15));
        assert_eq!(state.read(&second, Slot::GlobalVariable(9)), Some(1));
    }

    #[test]
    fn random_reads_operands_and_assigns_through_temporary_and_persistent_slots() {
        let attachment = attachment("fixture", Event::Dialogue);
        let input = invocation(&attachment);
        let bytes = original_rows(
            2,
            &[
                (108, 37),
                (112, 0x020301),
                (120, 7),
                (124, 2), // roll gv7, 2 into temp 0
                (240, 10),
                (244, 0x0201),
                (252, 8), // copy temp 0 into gv8
            ],
        );
        let code = Code::decode(&bytes).unwrap();
        let mut state = State {
            random: serde_json::from_value(1.into()).unwrap(),
            ..State::default()
        };
        state
            .write(&attachment, Slot::GlobalVariable(7), -2)
            .unwrap();
        let mut restored: State =
            serde_json::from_value(serde_json::to_value(&state).unwrap()).unwrap();
        for expected in [-1, -2, 0, -2, 1, 2] {
            code.run(&input, View::Writable(&mut state), &source())
                .unwrap();
            code.run(&input, View::Writable(&mut restored), &source())
                .unwrap();
            assert_eq!(state, restored);
            let result = state.read(&attachment, Slot::GlobalVariable(8)).unwrap();
            assert_eq!(result, expected);
        }
        let error = code
            .run(&input, View::ReadOnly(&state), &source())
            .unwrap_err();
        assert!(error.reason.contains("no writable script random stream"));
    }

    #[test]
    fn random_handles_reversed_singleton_and_full_signed_ranges() {
        let owner = attachment("fixture", Event::Dialogue);
        let input = invocation(&owner);
        let mut initial = State::default();
        initial.initialize_random();
        let roll = |a: i32, b: i32| {
            let bytes = original_rows(
                1,
                &[
                    (108, 37),
                    (112, 0x010303),
                    (120, a as u32),
                    (124, b as u32),
                    (128, 10),
                ],
            );
            let mut state = initial.clone();
            Code::decode(&bytes)
                .unwrap()
                .run(&input, View::Writable(&mut state), &source())
                .unwrap();
            state
        };
        assert_eq!(roll(5, -9), roll(-9, 5));
        let equal = roll(i32::MIN, i32::MIN);
        assert_eq!(equal.read(&owner, Slot::GlobalVariable(10)), Some(i32::MIN));
        assert_eq!(equal.random, initial.random);
        assert_eq!(roll(i32::MIN, i32::MAX), roll(i32::MAX, i32::MIN));
        assert_ne!(roll(i32::MIN, i32::MAX).random, initial.random);
    }

    #[test]
    fn random_assigns_counter_and_flag_words_and_ignores_immediate_destination() {
        let attachment = attachment("fixture", Event::Dialogue);
        let input = invocation(&attachment);
        let bytes = original_rows(
            3,
            &[
                (108, 37),
                (112, 0x000303),
                (120, 0x1002),
                (124, 0x1002),
                (128, 1),
                (240, 37),
                (244, 0x040303),
                (252, 6),
                (256, 6),
                (260, 3),
                (372, 37),
                (376, 0x030303),
                (384, 1),
                (388, 1),
                (392, 50),
            ],
        );
        let mut state = State::default();
        state.write(&attachment, Slot::GlobalFlag(3), 1).unwrap();
        Code::decode(&bytes)
            .unwrap()
            .run(&input, View::Writable(&mut state), &source())
            .unwrap();
        assert_eq!(state.read(&attachment, Slot::Counter(1)), Some(2));
        assert_eq!(state.read(&attachment, Slot::Counter(2)), Some(31));
        assert_eq!(state.read(&attachment, Slot::GlobalFlag(3)), Some(0));
        assert_eq!(state.read(&attachment, Slot::GlobalFlag(4)), Some(1));
        assert_eq!(state.read(&attachment, Slot::GlobalFlag(5)), Some(1));
        assert_eq!(state.random, None);
    }

    #[test]
    fn malformed_scripts_report_flags_branches_and_budget() {
        let attachment = attachment("fixture", Event::Dialogue);
        let input = invocation(&attachment);
        let state = State::default();
        let run = |bytes: &[u8]| {
            Code::decode(bytes)
                .unwrap()
                .run(&input, View::ReadOnly(&state), &source())
                .unwrap_err()
        };
        let branch = [(108, 3), (112, 3)];
        let mut bytes = original_rows(1, &branch);
        let error = run(&bytes);
        assert_eq!(error.statement, Some(0));
        assert!(error.reason.contains("budget exhausted"));
        bytes[120] = 2;
        assert_eq!(run(&bytes).statement, Some(2));
        bytes[48] = 4; // SF_AUTO_REMOVING is a resource flag.
        assert_eq!(run(&bytes).statement, None);
        bytes.pop();
        assert!(Code::decode(&bytes).is_err());
    }
}
