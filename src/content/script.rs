//! Original SCR framing, execution and persistent attachment state.
use super::{Source, read_bounded};
use crate::vfs::Vfs;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fmt};

const STEP_BUDGET: usize = 1000;
const GLOBAL_FLAGS: i32 = 3200;
const GLOBAL_VARIABLES: i32 = 2000;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, Ord, PartialOrd)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Event {
    Unlock,
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
    flags: BTreeMap<i32, bool>,
    variables: BTreeMap<i32, i32>,
    attachments: BTreeMap<String, BTreeMap<Event, Local>>,
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
    GlobalFlag(i32),
    GlobalVariable(i32),
    LocalFlag(i32),
    Counter(i32),
}

impl Slot {
    pub(crate) fn dlg(code: &str, index: i32) -> Option<Self> {
        Some(match code {
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
            Self::Counter(_) => (0..=255).contains(&value),
            Self::GlobalVariable(_) => true,
        }
    }
}

impl State {
    fn local(&self, attachment: &Attachment) -> Local {
        self.attachments
            .get(&attachment.owner)
            .and_then(|slots| slots.get(&attachment.event))
            .copied()
            .unwrap_or(attachment.initial)
    }

    pub(crate) fn read(&self, attachment: &Attachment, slot: Slot) -> Option<i32> {
        match slot {
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
        Ok(())
    }
}

pub(crate) struct Invocation<'a> {
    pub attachment: &'a Attachment,
    pub triggerer: Actor,
    pub position: usize,
    pub hour: u8,
    pub follows_player: Option<bool>,
    pub npc_flags: Option<u32>,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Actor {
    Player,
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

#[derive(Debug)]
pub(crate) struct Fault {
    pub statement: Option<usize>,
    pub reason: String,
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
        self.code.run(invocation, View::Writable(state))
    }

    pub(crate) fn inspect(
        &self,
        invocation: &Invocation<'_>,
        state: &State,
    ) -> Result<Execution, Fault> {
        self.code.run(invocation, View::ReadOnly(state))
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
    fn run(&self, input: &Invocation<'_>, mut view: View<'_>) -> Result<Execution, Fault> {
        self.check_flags()?;
        let mut pc = input.position;
        let attachment = input.attachment;
        let mut temporaries = [0; 10];
        for _ in 0..STEP_BUDGET {
            let fault = |reason| Fault {
                statement: Some(pc),
                reason,
            };
            let row = self
                .statements
                .get(pc)
                .ok_or_else(|| fault("script branch outside resource".into()))?;
            let value =
                |op: &Operation, index| read(op.operand(index), &view, attachment, &temporaries);
            let cond = &row.condition;
            let test = match cond.opcode {
                0 => Some(true), // SCT_TRUE
                1 => Some((6..18).contains(&input.hour)),
                3 => value(cond, 0)
                    .and_then(|index| view.state().read(attachment, Slot::LocalFlag(index)))
                    .map(|v| v != 0),
                4 | 5 => value(cond, 0)
                    .zip(value(cond, 1))
                    .map(|(a, b)| if cond.opcode == 4 { a == b } else { a <= b }),
                18 if [cond.focus(0), cond.focus(1)] == [1, 0] => input.follows_player,
                39 if cond.focus(0) == 1 => input.npc_flags.map(|flags| flags & 0x20 != 0),
                43 => value(cond, 0)
                    .and_then(|index| view.state().read(attachment, Slot::GlobalFlag(index)))
                    .map(|v| v != 0),
                _ => None,
            }
            .ok_or_else(|| fault(format!("unresolved script condition {}", cond.opcode)))?;
            let action = if test { &row.action } else { &row.otherwise };
            let unresolved = || fault(format!("unresolved script action {}", action.opcode));
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
                    match action.operand(0).ok_or_else(unresolved)? {
                        Operand::State(slot) => view
                            .write(attachment, slot, assigned)
                            .ok_or_else(unresolved)?,
                        Operand::Temporary(index) => temporaries[index] = assigned,
                        Operand::Literal(_) => return Err(unresolved()),
                    }
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
            reason: "script execution budget exhausted".into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let run = |state: &State| script.run(&input, View::ReadOnly(state)).unwrap();
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
            .run(&invocation(&first), View::Writable(&mut state))
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
    fn malformed_scripts_report_flags_branches_and_budget() {
        let attachment = attachment("fixture", Event::Dialogue);
        let input = invocation(&attachment);
        let state = State::default();
        let run = |bytes: &[u8]| {
            Code::decode(bytes)
                .unwrap()
                .run(&input, View::ReadOnly(&state))
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
