//! Read-only interpretation of original DLG and dialogue entry statements.

use super::{Source, character::CharacterStatistics, load_mes, read_bounded};
use crate::{mes, resource_path, vfs::Vfs};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Serialize)]
struct Line {
    id: i32,
    text: String,
    alternate: String,
    iq: i32,
    test: String,
    next: i32,
    effect: String,
}

pub(super) struct Dialogue {
    source: Source,
    lines: BTreeMap<i32, Line>,
}

fn number(field: &str) -> Result<i32, String> {
    if field.is_empty() {
        return Ok(0);
    }
    field
        .parse()
        .map_err(|_| format!("invalid number {field:?}"))
}

impl Dialogue {
    pub(super) fn load(vfs: &mut Vfs, path: &str) -> Result<Self, String> {
        let (source, bytes) =
            read_bounded(vfs, path, mes::MAX_INPUT_BYTES).map_err(|e| e.to_string())?;
        Self::decode(source, &bytes).map_err(|error| format!("{path}: {error}"))
    }

    pub(super) fn structure(&self) -> serde_json::Value {
        let speaker_lines = self.lines.values().filter(|line| line.iq == 0).count();
        serde_json::json!({"status":"decoded","records":self.lines.len(),"speaker_lines":speaker_lines,
            "response_lines":self.lines.len()-speaker_lines})
    }

    fn decode(source: Source, bytes: &[u8]) -> Result<Self, String> {
        let mut fields = mes::fields(bytes).map_err(|e| e.to_string())?;
        let mut lines = BTreeMap::new();
        while let Some(id) = fields.next() {
            let id = number(id.map_err(|e| e.to_string())?.trim())?;
            let mut field = || {
                fields
                    .next()
                    .ok_or_else(|| format!("incomplete line {id}"))?
                    .map(|s| s.trim().to_owned())
                    .map_err(|e| e.to_string())
            };
            let text = field()?;
            let alternate = field()?;
            let iq = number(&field()?)?;
            let test = field()?;
            let next = number(&field()?)?;
            let effect = field()?;
            if id <= 0
                || lines
                    .insert(
                        id,
                        Line {
                            id,
                            text,
                            alternate,
                            iq,
                            test,
                            next,
                            effect,
                        },
                    )
                    .is_some()
            {
                return Err(format!("invalid or duplicate line {id}"));
            }
        }
        if lines.is_empty() {
            return Err("empty dialogue resource".into());
        }
        Ok(Self { source, lines })
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
pub(crate) struct EntryContext {
    pub script: i32,
    pub local_flags: Option<u32>,
    pub follows_player: Option<bool>,
    pub npc_flags: Option<u32>,
    pub hour: u8,
}

fn word(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn entry(bytes: &[u8], context: EntryContext) -> Result<(usize, i32), String> {
    let header = bytes.get(..64).ok_or("truncated script header")?;
    let (rows, tail) = bytes[64..].as_chunks::<132>();
    if !tail.is_empty() || rows.len() != word(header, 52) as usize {
        return Err("invalid dialogue script length".into());
    }
    if word(header, 48) != 0 {
        return Err("unsupported dialogue script flags".into());
    }
    let mut pc = 0;
    for _ in 0..rows.len() {
        let row = rows
            .get(pc)
            .ok_or("dialogue script branch outside resource")?;
        let op = word(row, 0);
        let test = match op {
            0 => Some(true),                            // SCT_TRUE
            1 => Some((6..18).contains(&context.hour)), // SCT_DAYTIME
            3 if row[4] == 3 => local_flag(context.local_flags, word(row, 12)),
            18 if row[4..6] == [1, 0] => context.follows_player, // SCT_OBJ_FOLLOWING_PC
            39 if row[4] == 1 => context.npc_flags.map(|flags| flags & 0x20 != 0), // SCT_OBJ_JILTED
            _ => None,
        }
        .ok_or_else(|| format!("unresolved script condition {op} at statement {pc}"))?;
        let action = if test { &row[44..88] } else { &row[88..] };
        match word(action, 0) {
            0 => pc += 1,
            1 | 2 => {
                return Err(format!(
                    "script returned without starting dialogue at statement {pc}"
                ));
            }
            3 if action[4] == 3 => pc = word(action, 12) as usize,
            4 if action[4] == 3 => return Ok((pc, word(action, 12) as i32)), // SAT_DIALOG, literal
            op => return Err(format!("unresolved script action {op} at statement {pc}")),
        }
    }
    Err("dialogue script did not reach an entry point".into())
}

#[derive(Clone, Debug, Serialize)]
pub struct Response {
    pub id: Option<i32>,
    pub text: String,
    pub next: i32,
    pub condition: String,
    pub effect: String,
    pub available: bool,
}

#[derive(Debug, Serialize)]
pub struct Dependency {
    pub line: i32,
    pub kind: &'static str,
    pub value: String,
}

#[derive(Serialize)]
pub struct Turn {
    pub line: i32,
    pub text: String,
    pub responses: Vec<Response>,
    pub dependencies: Vec<Dependency>,
    pub voice: Option<String>,
    pub speech_id: Option<u16>,
    #[serde(skip)]
    conditions: Conditions,
}

pub struct Conversation {
    pub speaker: String,
    pub target: String,
    pub tile: (u32, u32),
    pub facing: u8,
    pub turn: Turn,
    dialogue: Dialogue,
    script: Source,
    script_id: i32,
    entry_statement: usize,
    entry_line: i32,
    npc_gender: Option<i32>,
    npc_intelligence: Option<i32>,
    npc_race: Option<i32>,
    social_class: Option<i32>,
    player_armor: Option<u32>,
    state: Conditions,

    override_script: i32,
    generic: Option<mes::Mes>,
    voice_map: Option<mes::Mes>,
    voice_map_error: Option<String>,
}

pub(crate) struct Context {
    pub target: String,
    pub speaker: String,
    pub tile: (u32, u32),
    pub facing: u8,
    pub npc_gender: Option<i32>,
    pub npc_intelligence: Option<i32>,
    pub npc_race: Option<i32>,
    pub social_class: Option<i32>,
    pub player_armor: Option<u32>,
    pub met: bool,
    pub stats: CharacterStatistics,
    pub gold: u64,
    pub reaction: Option<i32>,
    pub override_script: i32,
    pub entry: EntryContext,
}

impl Conversation {
    pub(crate) fn open(vfs: &mut Vfs, context: &Context) -> Result<Self, String> {
        let script_id = context.entry.script;
        let path = super::jumps::script_path(vfs, script_id)?;
        let (script, bytes) =
            read_bounded(vfs, &path, mes::MAX_INPUT_BYTES).map_err(|e| e.to_string())?;
        let (entry_statement, entry_line) = entry(&bytes, context.entry)
            .map_err(|error| format!("{path} from {script}: {error}"))?;
        let stem = path
            .strip_prefix("scr/")
            .unwrap()
            .strip_suffix(".scr")
            .unwrap();
        let path = format!("dlg/{stem}.dlg");
        let dialogue = Dialogue::load(vfs, &path)?;
        let map_path = format!("dlg/{stem}.voice.mes");
        let (voice_map, voice_map_error) = if exists(vfs, &map_path) {
            match load_mes(vfs, &map_path) {
                Ok(map) => (Some(map.mes), None),
                Err(error) => (None, Some(error.to_string())),
            }
        } else {
            (None, None)
        };
        let generic = context.npc_gender.and_then(|gender| {
            let gender = if gender == 1 { 'm' } else { 'f' };
            let dumb = if context.stats.attributes[4] <= 4 {
                "dumb_"
            } else {
                ""
            };
            load_mes(vfs, &format!("mes/gd_{dumb}pc2{gender}.mes"))
                .ok()
                .map(|table| table.mes)
        });
        let mut session = Self {
            speaker: context.speaker.clone(),
            target: context.target.clone(),
            tile: context.tile,
            facing: context.facing,
            turn: Turn {
                line: 0,
                text: String::new(),
                responses: Vec::new(),
                dependencies: Vec::new(),
                voice: None,
                speech_id: None,
                conditions: Conditions::default(),
            },
            dialogue,
            script,
            script_id,
            entry_statement,
            entry_line,
            npc_gender: context.npc_gender,
            npc_intelligence: context.npc_intelligence,
            npc_race: context.npc_race,
            social_class: context.social_class,
            player_armor: context.player_armor,
            state: Conditions {
                reaction: context.reaction,
                flags: context.entry.local_flags,
                met: context.met,
            },
            override_script: context.override_script,
            generic,
            voice_map,
            voice_map_error,
        };
        session.turn = session.build_turn(vfs, entry_line, &context.stats, context.gold)?;
        session.state = session.turn.conditions;
        Ok(session)
    }

    pub(crate) fn choose(
        &mut self,
        vfs: &mut Vfs,
        index: usize,
        stats: &CharacterStatistics,
        gold: u64,
    ) -> Result<bool, String> {
        let response = self
            .turn
            .responses
            .get(index)
            .filter(|r| r.available)
            .ok_or("response unavailable")?;
        if response.next <= 0 {
            return Ok(false);
        }
        let state = self.state;
        if !response.effect.is_empty() {
            self.state.reaction = None;
        }
        match self.build_turn(vfs, response.next, stats, gold) {
            Ok(turn) => {
                self.state = turn.conditions;
                self.turn = turn;
            }
            Err(error) => {
                self.state = state;
                return Err(error);
            }
        }
        Ok(true)
    }

    pub fn diagnostic(&self) -> serde_json::Value {
        serde_json::json!({"target":self.target,"speaker":self.speaker,"tile":self.tile,"facing":self.facing,
            "npc_gender":self.npc_gender,"reaction":self.state.reaction,"met_before":self.state.met,"override_script":self.override_script,"turn":self.turn,"script_id":self.script_id,
            "script":self.script.virtual_path,"script_mount":self.script.to_string(),"entry_statement":self.entry_statement,
            "entry_line":self.entry_line,"exit_script":"unsupported","resource":self.dialogue.source.virtual_path,
            "mount":self.dialogue.source.to_string()})
    }

    fn build_turn(
        &self,
        vfs: &mut Vfs,
        id: i32,
        stats: &CharacterStatistics,
        gold: u64,
    ) -> Result<Turn, String> {
        let line = self
            .dialogue
            .lines
            .get(&id)
            .filter(|l| l.iq == 0)
            .ok_or_else(|| format!("missing NPC dialogue line {id}"))?;
        let raw = if stats.gender == 1 {
            &line.text
        } else {
            &line.alternate
        };
        let generated = line.text.eq_ignore_ascii_case("g:");
        let raw = if generated {
            self.greeting(vfs, stats).ok_or_else(|| {
                format!("NPC dialogue line {id} needs an unresolved greeting resource or condition")
            })?
        } else if raw.is_empty() || matches!(directive(raw), Some('g' | 'i' | 'm' | 'r')) {
            return Err(format!(
                "NPC dialogue line {id} requires an unsupported text directive or has no speaker text"
            ));
        } else {
            raw.clone()
        };
        let mut condition_state = self.state;
        let mut block_progression = false;
        for effect in line
            .effect
            .split(',')
            .map(str::trim)
            .filter(|effect| !effect.is_empty())
        {
            match effect.get(..2).map(str::to_ascii_lowercase).as_deref() {
                Some("lf") => condition_state.flags = None,
                Some("re") => condition_state.reaction = None,
                // These affect gameplay state, leaving the authored next-line selection intact.
                Some("gf" | "gv" | "qu" | "ru" | "rq" | "in" | "lc" | "mm") => {}
                _ => block_progression = true,
            }
        }
        let mut turn = Turn {
            line: id,
            text: String::new(),
            responses: vec![],
            dependencies: vec![],
            voice: None,
            speech_id: line.test.parse().ok(),
            conditions: condition_state,
        };
        turn.text = substitute(&raw, &self.speaker, line.id, &mut turn.dependencies);
        if !line.effect.is_empty() {
            turn.dependencies.push(Dependency {
                line: id,
                kind: if block_progression {
                    "speaker_flow"
                } else {
                    "speaker_effect"
                },
                value: line.effect.clone(),
            });
        }
        if let Some(error) = &self.voice_map_error {
            turn.dependencies.push(Dependency {
                line: id,
                kind: "voice_map",
                value: error.clone(),
            });
        }
        turn.voice = self.voice(vfs, line, stats.gender, &mut turn.dependencies);
        for (_, response) in self
            .dialogue
            .lines
            .range((std::ops::Bound::Excluded(id), std::ops::Bound::Unbounded))
            .take_while(|(_, response)| response.iq != 0)
        {
            let iq = i64::from(stats.attributes[4]);
            let gender_matches = match response.alternate.as_str() {
                "" => true,
                "0" => stats.gender == 0,
                "1" => stats.gender == 1,
                _ => {
                    turn.dependencies.push(Dependency {
                        line: response.id,
                        kind: "gender",
                        value: response.alternate.clone(),
                    });
                    false
                }
            };
            let (passes, unknown) = conditions(&response.test, stats, gold, condition_state);
            for value in unknown {
                turn.dependencies.push(Dependency {
                    line: response.id,
                    kind: "condition",
                    value,
                });
            }
            if !gender_matches || !threshold(iq, response.iq) || !passes {
                continue;
            }
            let text = match directive(&response.text) {
                Some(code) => {
                    let start = match code {
                        'y' => 1,
                        'n' => 100,
                        's' => 200,
                        'e' => 400,
                        'f' => 800,
                        'k' => 1500,
                        'w' => 1800,
                        _ => 0,
                    };
                    let Some(text) = (start > 0).then(|| self.generic(start)).flatten() else {
                        turn.dependencies.push(Dependency {
                            line: response.id,
                            kind: "response_directive",
                            value: response.text.clone(),
                        });
                        continue;
                    };
                    text
                }
                None => &response.text,
            };
            if turn.responses.len() == 5 {
                continue;
            }
            // RE only adjusts reaction. Within a text-only conversation, retain the
            // response and diagnose the skipped adjustment. Other effects can alter control flow.
            let available = !block_progression
                && response.next >= 0
                && (response.effect.is_empty()
                    || response
                        .effect
                        .get(..2)
                        .is_some_and(|op| op.eq_ignore_ascii_case("re"))
                        && response.effect[2..].trim().parse::<i32>().is_ok());
            if !response.effect.is_empty() {
                turn.dependencies.push(Dependency {
                    line: response.id,
                    kind: "response_effect",
                    value: response.effect.clone(),
                });
            }
            if response.next < 0 {
                turn.dependencies.push(Dependency {
                    line: response.id,
                    kind: "script_continuation",
                    value: response.next.to_string(),
                });
            }
            turn.responses.push(Response {
                id: Some(response.id),
                text: substitute(text, &self.speaker, response.id, &mut turn.dependencies),
                next: response.next,
                condition: response.test.clone(),
                effect: response.effect.clone(),
                available,
            });
        }
        if !block_progression
            && turn.responses.is_empty()
            && let Some(text) = self.generic(400)
        {
            turn.responses.push(Response {
                id: None,
                text: text.to_owned(),
                next: 0,
                condition: String::new(),
                effect: String::new(),
                available: true,
            });
        }
        Ok(turn)
    }

    fn greeting(&self, vfs: &mut Vfs, stats: &CharacterStatistics) -> Option<String> {
        // CE sub_419260: ordinary G: greetings in the current, spell-free
        // exploration runtime. Numeric greeting directives remain unresolved.
        let reaction = self.state.reaction?;
        // A dialogue override can change the selected greeting.
        if self.override_script != 0 {
            return None;
        }
        let genders = |gender| if gender == 1 { 'm' } else { 'f' };
        let dumb = self.npc_intelligence? <= 4;
        let mut start = match self.player_armor? {
            0 => 16000,
            7 => 17000,
            _ => match (self.state.met, reaction) {
                (false, 101..) => 8000,
                (false, 61..=100) => 9000,
                (false, 41..=60) => 10000,
                (true, 101..) => 11000,
                (true, 61..=100) => 12000,
                (true, 41..=60) => 13000,
                (false, 1..=40) => 2000,
                (false, ..=0) => 3000,
                (true, 1..=40) => 4000,
                (true, ..=0) => 5000,
            },
        };
        let family = if start >= 8000 {
            if !dumb {
                start += 50 * self.social_class?.clamp(0, 18);
            }
            "cls"
        } else {
            if !dumb && stats.race != self.npc_race? {
                start += 50 * (stats.race + 1);
            }
            "rce"
        };
        let path = format!(
            "mes/gd_{family}_{}{}2{}.mes",
            if dumb { "dumb_" } else { "" },
            genders(self.npc_gender?),
            genders(stats.gender)
        );
        let table = load_mes(vfs, &path).ok()?.mes;
        table
            .entries()
            .find(|(id, text)| (start..start + 50).contains(id) && !text.trim().is_empty())
            .map(|(_, text)| text.to_owned())
    }

    fn generic(&self, start: i32) -> Option<&str> {
        self.generic
            .as_ref()?
            .entries()
            .find(|(id, text)| (start..start + 100).contains(id) && !text.trim().is_empty())
            .map(|(_, text)| text)
    }

    fn voice(
        &self,
        vfs: &Vfs,
        line: &Line,
        gender: i32,
        dependencies: &mut Vec<Dependency>,
    ) -> Option<String> {
        if let Some(path) = self.voice_map.as_ref().and_then(|map| map.get(line.id)) {
            let path = path.trim();
            if path.is_empty() {
                return None;
            }
            if let Ok(path) = resource_path::normalize(path)
                && (path.ends_with(".mp3") || path.ends_with(".wav"))
                && exists(vfs, &path)
            {
                return Some(path);
            }
            dependencies.push(Dependency {
                line: line.id,
                kind: "voice_resource",
                value: path.to_owned(),
            });
            return None;
        }
        let speech = line.test.parse::<u16>().ok()?;
        let original = |gender| {
            format!(
                "sound/speech/{:05}/v{speech}_{gender}.mp3",
                self.script_id & 0x7fff
            )
        };
        [if gender == 1 { 'm' } else { 'f' }, 'm']
            .into_iter()
            .map(original)
            .find(|path| exists(vfs, path))
    }
}

fn exists(vfs: &Vfs, path: &str) -> bool {
    vfs.resolve(path).is_ok_and(|res| res.winner().is_some())
}

fn substitute(raw: &str, speaker: &str, line: i32, dependencies: &mut Vec<Dependency>) -> String {
    let mut chunks = raw.split('@');
    let mut text = chunks.next().unwrap_or_default().to_owned();
    while let Some(token) = chunks.next() {
        let Some(tail) = chunks.next() else {
            text.push('@');
            text.push_str(token);
            break;
        };
        if token.eq_ignore_ascii_case("npcname") {
            text.push_str(speaker);
        } else {
            dependencies.push(Dependency {
                line,
                kind: "text_token",
                value: token.to_owned(),
            });
        }
        text.push_str(tail);
    }
    text
}

fn directive(text: &str) -> Option<char> {
    let mut chars = text.chars();
    let code = chars.next()?.to_ascii_lowercase();
    (code.is_ascii_alphabetic() && chars.next() == Some(':')).then_some(code)
}

fn threshold(value: i64, limit: i32) -> bool {
    if limit < 0 {
        value <= -i64::from(limit)
    } else {
        value >= i64::from(limit)
    }
}

#[derive(Clone, Copy, Default)]
struct Conditions {
    reaction: Option<i32>,
    flags: Option<u32>,
    met: bool,
}

fn local_flag(flags: Option<u32>, index: u32) -> Option<bool> {
    Some(flags? & 1u32.checked_shl(index)? != 0)
}

fn conditions(
    test: &str,
    stats: &CharacterStatistics,
    gold: u64,
    state: Conditions,
) -> (bool, Vec<String>) {
    let mut passes = true;
    let mut unknown = Vec::new();
    let mut rest = test.trim();
    // Each condition begins with a two-character opcode. The following operands
    // extend to the next opcode. GF and similar tests retain both operands.
    while !rest.is_empty() {
        let Some(code) = rest
            .get(..2)
            .filter(|code| code.bytes().all(|c| c.is_ascii_alphabetic() || c == b'$'))
        else {
            unknown.push(rest.to_owned());
            break;
        };
        let end = rest[2..]
            .find(|ch: char| ch.is_ascii_alphabetic() || ch == '$')
            .map_or(rest.len(), |i| i + 2);
        let raw = rest[..end].trim_end_matches(|ch: char| ch == ',' || ch.is_ascii_whitespace());
        let code = code.to_ascii_lowercase();
        let args = raw[2..].trim();
        let result = if code == "lf" {
            let mut pair = args.split_ascii_whitespace();
            pair.next()
                .and_then(|index| local_flag(state.flags, index.parse().ok()?))
                .zip(pair.next().filter(|&value| value == "0" || value == "1"))
                .filter(|_| pair.next().is_none())
                .map(|(actual, expected)| actual == (expected == "1"))
        } else {
            args.parse::<i32>().ok().and_then(|n| {
                let stat = match code.as_str() {
                    "ch" => stats.attributes[7],
                    "re" => state.reaction?,
                    "me" if matches!(n, 0 | 1) => return Some(state.met == (n == 1)),
                    "pe" => stats.attributes[5],
                    "al" => stats.base[19],
                    "ma" => stats.aptitude,
                    "ta" => -stats.aptitude,
                    "le" => -stats.level,
                    "ra" => {
                        return Some((i64::from(stats.race) + 1 == i64::from(n).abs()) == (n > 0));
                    }
                    "$$" => return Some(n >= 0 && gold >= n as u64),
                    _ => return None,
                };
                Some(threshold(i64::from(stat), n))
            })
        };
        match result {
            Some(value) => passes &= value,
            None => unknown.push(raw.to_owned()),
        }
        rest = &rest[end..];
    }
    (passes && unknown.is_empty(), unknown)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        content::character::{Character, Rules},
        vfs::{MountId, Storage},
    };

    fn source() -> Source {
        Source {
            virtual_path: "dlg/00001fixture.dlg".into(),
            mount_id: MountId::GENERATED,
            mount_path: "test".into(),
            loose_path: None,
            storage: Storage::Loose,
            mod_id: None,
        }
    }

    #[test]
    fn original_field_order_distinguishes_speaker_and_responses() {
        let dialogue = Dialogue::decode(
            source(),
            b"// fixture\r\n{10}{Ca\xe9}{Hello ma'am}{}{7}{}{gf4 1}\r\n{11}{Hello}{1}{5}{ch6}{22}{}\n{22}{Bye}{Bye}{}{}{}{}",
        ).unwrap();
        let speaker = &dialogue.lines[&10];
        assert_eq!(
            (&*speaker.text, &*speaker.alternate, &*speaker.effect),
            ("Caé", "Hello ma'am", "gf4 1")
        );
        assert_eq!(
            (&*dialogue.lines[&11].test, dialogue.lines[&11].next),
            ("ch6", 22)
        );
        assert!(Dialogue::decode(source(), b"{1}{Incomplete}").is_err());
        assert!(Dialogue::decode(source(), b"{1}{}{}{}{}{}{}{1}{}{}{}{}{}{}").is_err());
    }

    #[test]
    fn entry_follows_literal_jump_and_rejects_unresolved_condition() {
        let mut script = vec![0; 64 + 2 * 132];
        script[52] = 2;
        script[64 + 44] = 3;
        script[64 + 48] = 3;
        script[64 + 56] = 1;
        script[196 + 44] = 4;
        script[196 + 48] = 3;
        script[196 + 56] = 42;
        let context = EntryContext {
            script: 1,
            local_flags: Some(0),
            follows_player: Some(false),
            npc_flags: Some(0),
            hour: 12,
        };
        assert_eq!(entry(&script, context).unwrap(), (1, 42));
        script[196] = 43;
        assert!(
            entry(&script, context)
                .unwrap_err()
                .contains("condition 43 at statement 1")
        );
        script[196] = 0;
        script[196 + 48] = 4;
        assert!(entry(&script, context).unwrap_err().contains("action 4"));
    }

    #[test]
    fn supported_conditions_combine_and_unknowns_fail_closed() {
        let stats = Character::default().statistics(&Rules::default(), None, 0, [0; 5], false);
        assert_eq!(
            conditions(
                "ch8, pe -9 ra1 $$5 re-61",
                &stats,
                5,
                Conditions {
                    reaction: Some(60),
                    ..Conditions::default()
                }
            ),
            (true, vec![])
        );
        assert_eq!(
            conditions("ch9, pe -9, $$5", &stats, 5, Conditions::default()),
            (false, vec![])
        );
        assert_eq!(
            conditions("ch8gf2010 0, qu1020 2", &stats, 5, Conditions::default()),
            (false, vec!["gf2010 0".into(), "qu1020 2".into()])
        );
    }
}
