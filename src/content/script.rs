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

const STEP_BUDGET: usize = 1000;
const GLOBAL_FLAGS: i32 = 3200;
const GLOBAL_VARIABLES: i32 = 2000;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, Ord, PartialOrd)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Event {
    Unlock,
    Use,
    BuyObject,
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
    flags: BTreeMap<i32, bool>,
    variables: BTreeMap<i32, i32>,
    attachments: BTreeMap<String, BTreeMap<Event, Local>>,
    #[serde(default)]
    pub quests: Ledger,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    random: Option<Random>,
    #[serde(skip)]
    pub(super) game_minute: u64,
    #[serde(skip)]
    pub(super) interaction: Option<Interaction>,
}

/// The staged read view for a single SCR/DLG invocation. Operations commit with its state writes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Interaction {
    pub reaction: Option<i32>,
    pub aptitude: i32,
    pub spell_flags: u32,
    pub gold: [i64; 2],
    pub items: Vec<(u8, u32, String)>,
    pub substitute: Option<String>,
    pub operations: Vec<Transfer>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Transfer {
    Gold { actor: u8, delta: i64 },
    Item { id: String, from: u8, to: u8 },
}

impl Interaction {
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

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Actor {
    Player,
    Item(u32),
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
    otherwise: Operation,
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

fn path(vfs: &Vfs, id: i32) -> Result<String, String> {
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
        Ok(Self { source, code })
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
                if focus == 0 && matches!(input.triggerer, Actor::Item(_)) {
                    None
                } else {
                    actor(focus)
                }
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
                    Actor::Player => None,
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
                10..=12 => {
                    let rhs = value(action, 1).ok_or_else(unresolved)?;
                    let assigned = match action.opcode {
                        11 => rhs.wrapping_add(value(action, 2).ok_or_else(unresolved)?),
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
                        state
                            .quests
                            .set_player(id, status, state.game_minute, || Origin {
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
                29 | 33 => {
                    let facts = view.state().interaction.as_ref().ok_or_else(unresolved)?;
                    let result = match action.opcode {
                        29 if actor(action.focus(0)) == Some(1)
                            && actor(action.focus(1)) == Some(0) =>
                        {
                            facts.reaction
                        }
                        33 if actor(action.focus(1)) == Some(0) && value(action, 0) == Some(16) => {
                            Some(facts.aptitude)
                        }
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
