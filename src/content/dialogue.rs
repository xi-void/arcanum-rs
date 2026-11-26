//! Original DLG conditions, effects and dialogue lifecycle.

use super::{
    Source,
    character::CharacterStatistics,
    load_mes, read_bounded,
    script::{Actor, Attachment, Invocation, Outcome, Script, Slot, State},
};
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

#[derive(Clone)]
pub(crate) struct EntryContext {
    pub attachment: Attachment,
    pub follows_player: Option<bool>,
    pub npc_flags: Option<u32>,
    pub hour: u8,
}

impl EntryContext {
    pub(crate) fn invocation(&self, position: usize) -> Invocation<'_> {
        Invocation {
            attachment: &self.attachment,
            triggerer: Actor::Player,
            position,
            hour: self.hour,
            follows_player: self.follows_player,
            npc_flags: self.npc_flags,
        }
    }
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
    script: Script,
    context: EntryContext,
    entry_statement: usize,
    entry_line: i32,
    continuation: Option<super::script::Execution>,
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
    pub(crate) fn open(
        vfs: &mut Vfs,
        context: &Context,
        script: Script,
        entry_statement: usize,
        entry_line: i32,
        scripts: &mut State,
    ) -> Result<Self, String> {
        let path = &script.source.virtual_path;
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
            context: context.entry.clone(),
            entry_statement,
            entry_line,
            continuation: None,
            npc_gender: context.npc_gender,
            npc_intelligence: context.npc_intelligence,
            npc_race: context.npc_race,
            social_class: context.social_class,
            player_armor: context.player_armor,
            state: Conditions {
                reaction: context.reaction,
                met: context.met,
                ..Conditions::default()
            },
            override_script: context.override_script,
            generic,
            voice_map,
            voice_map_error,
        };
        session.turn =
            session.build_turn(vfs, entry_line, &context.stats, context.gold, scripts)?;
        session.state = session.turn.conditions.clone();
        Ok(session)
    }

    pub(crate) fn choose(
        &mut self,
        vfs: &mut Vfs,
        index: usize,
        stats: &CharacterStatistics,
        gold: u64,
        scripts: &mut State,
        hour: u8,
    ) -> Result<bool, String> {
        let response = self
            .turn
            .responses
            .get(index)
            .filter(|r| r.available)
            .ok_or("response unavailable")?;
        let attachment = &self.context.attachment;
        if !conditions(
            &response.condition,
            stats,
            gold,
            &self.state,
            scripts,
            attachment,
        )
        .0
        {
            return Err("response conditions no longer pass".into());
        }
        let mut staged = scripts.clone();
        let previous = self.state.clone();
        match effects(&response.effect) {
            Some(effects) => {
                apply_effects(&effects, &mut staged, attachment)?;
                self.state
                    .unresolved
                    .retain(|slot| !effects.iter().any(|(written, _)| written == slot));
            }
            None if legacy_reaction(&response.effect) && response.next >= 0 => {
                self.state.reaction = None
            }
            None => return Err("unresolved response effect".into()),
        }
        let destination = response.next;
        let result = (|| {
            let mut continuation = None;
            let next = match destination {
                ..0 => {
                    let mut context = self.context.invocation(
                        continuation_position(destination).ok_or("invalid script continuation")?,
                    );
                    context.hour = hour;
                    let execution =
                        self.script
                            .execute(&context, &mut staged)
                            .map_err(|error| {
                                format!(
                                    "{} from {}: {error}",
                                    self.script.source.virtual_path, self.script.source
                                )
                            })?;
                    let line = match execution.outcome {
                        Outcome::Return { .. } => 0,
                        Outcome::Dialogue { line } => line,
                    };
                    continuation = Some(execution);
                    line
                }
                next => next,
            };
            let turn = if next == 0 {
                None
            } else {
                Some(self.build_turn(vfs, next, stats, gold, &mut staged)?)
            };
            self.continuation = continuation;
            Ok(turn)
        })();
        match result {
            Ok(Some(turn)) => {
                self.state = turn.conditions.clone();
                self.turn = turn;
                *scripts = staged;
                Ok(true)
            }
            Ok(None) => {
                *scripts = staged;
                Ok(false)
            }
            Err(error) => {
                self.state = previous;
                Err(error)
            }
        }
    }

    pub fn diagnostic(&self) -> serde_json::Value {
        serde_json::json!({"target":self.target,"speaker":self.speaker,"tile":self.tile,"facing":self.facing,
            "npc_gender":self.npc_gender,"reaction":self.state.reaction,"met_before":self.state.met,"override_script":self.override_script,"turn":self.turn,"script_id":self.context.attachment.script,
            "attachment":self.context.attachment,"triggerer":Actor::Player,"continuation":self.continuation,
            "script":self.script.source.virtual_path,"script_mount":self.script.source.to_string(),"entry_statement":self.entry_statement,
            "entry_line":self.entry_line,"exit_script":"unsupported","resource":self.dialogue.source.virtual_path,
            "mount":self.dialogue.source.to_string()})
    }

    fn build_turn(
        &self,
        vfs: &mut Vfs,
        id: i32,
        stats: &CharacterStatistics,
        gold: u64,
        scripts: &mut State,
    ) -> Result<Turn, String> {
        let line = self
            .dialogue
            .lines
            .get(&id)
            .filter(|l| l.iq == 0)
            .ok_or_else(|| format!("missing NPC dialogue line {id}"))?;
        let mut condition_state = self.state.clone();
        let attachment = &self.context.attachment;
        let mut block_progression = false;
        let speaker_effects = effects(&line.effect);
        if let Some(effects) = &speaker_effects {
            apply_effects(effects, scripts, attachment)?;
            condition_state
                .unresolved
                .retain(|slot| !effects.iter().any(|(written, _)| written == slot));
        } else {
            // Preserve visibility for the existing text-only fallback. Conditions
            // affected by a skipped write stay unresolved until a supported write.
            for effect in line
                .effect
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                match effect.get(..2).map(str::to_ascii_lowercase).as_deref() {
                    Some("gf" | "gv" | "lf" | "lc") => {
                        if let Some(slot) = state_slot(effect).map(|(slot, _)| slot) {
                            condition_state.unresolved.push(slot);
                        }
                    }
                    Some("re") => condition_state.reaction = None,
                    Some("qu" | "ru" | "rq" | "in" | "mm") => {}
                    _ => block_progression = true,
                }
            }
        }
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
        if speaker_effects.is_none() {
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
            let (passes, unknown) = conditions(
                &response.test,
                stats,
                gold,
                &turn.conditions,
                scripts,
                attachment,
            );
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
            let supported_effect = effects(&response.effect).is_some();
            let supported_link = response.next >= 0
                || continuation_position(response.next)
                    .is_some_and(|pc| pc < self.script.code().statements.len());
            let available = !block_progression
                && supported_link
                && (supported_effect || response.next >= 0 && legacy_reaction(&response.effect));
            if !supported_effect {
                turn.dependencies.push(Dependency {
                    line: response.id,
                    kind: "response_effect",
                    value: response.effect.clone(),
                });
            }
            if !supported_link {
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
                self.context.attachment.script & 0x7fff
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

#[derive(Clone, Default)]
struct Conditions {
    reaction: Option<i32>,
    met: bool,
    unresolved: Vec<Slot>,
}

fn continuation_position(next: i32) -> Option<usize> {
    usize::try_from(next.checked_neg()?).ok()
}

fn state_slot(raw: &str) -> Option<(Slot, i32)> {
    let code = raw.get(..2)?.to_ascii_lowercase();
    let mut args = raw[2..].split_ascii_whitespace();
    let slot = Slot::dlg(&code, args.next()?.parse().ok()?)?;
    let value = args.next()?.parse().ok()?;
    args.next().is_none().then_some((slot, value))
}

fn effects(raw: &str) -> Option<Vec<(Slot, i32)>> {
    if raw.is_empty() {
        return Some(vec![]);
    }
    raw.split(',')
        .map(|effect| {
            let (slot, value) = state_slot(effect.trim())?;
            slot.accepts(value).then_some((slot, value))
        })
        .collect()
}

fn apply_effects(
    writes: &[(Slot, i32)],
    scripts: &mut State,
    attachment: &Attachment,
) -> Result<(), String> {
    for &(slot, value) in writes {
        scripts
            .write(attachment, slot, value)
            .ok_or("invalid dialogue state write")?;
    }
    Ok(())
}

fn legacy_reaction(raw: &str) -> bool {
    raw.get(..2).is_some_and(|op| op.eq_ignore_ascii_case("re"))
        && raw[2..].trim().parse::<i32>().is_ok()
}

fn conditions(
    test: &str,
    stats: &CharacterStatistics,
    gold: u64,
    state: &Conditions,
    scripts: &State,
    attachment: &Attachment,
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
        let result = if matches!(code.as_str(), "gf" | "gv" | "lf" | "lc") {
            state_slot(raw)
                .filter(|(slot, expected)| {
                    (!matches!(slot, Slot::GlobalFlag(_) | Slot::LocalFlag(_))
                        || slot.accepts(*expected))
                        && !state.unresolved.contains(slot)
                })
                .and_then(|(slot, expected)| {
                    scripts
                        .read_dialogue(attachment, slot)
                        .map(|actual| actual == expected)
                })
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
    fn response_writes_precede_script_and_speaker_effects_precede_conditions() {
        use crate::vfs::Policy;
        use std::{
            fs,
            time::{SystemTime, UNIX_EPOCH},
        };

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("arcanum-dialogue-{}-{unique}", std::process::id()));
        for directory in ["scr", "dlg"] {
            fs::create_dir_all(root.join(directory)).unwrap();
        }
        let mut bytes = vec![0; 64 + 3 * 132];
        for (at, value) in [
            (52, 3u32),
            (108, 4),
            (112, 3),
            (120, 10), // entry: DLG 10
            (196, 4),
            (200, 0x0304),
            (208, 7),
            (212, 1), // when gf7 == 1
            (240, 4),
            (244, 3),
            (252, 20), // DLG 20
            (284, 2),  // otherwise return with default enabled
            (372, 23), // unsupported action at statement 2
        ] {
            bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        fs::write(root.join("scr/00001fixture.scr"), bytes).unwrap();
        fs::write(
            root.join("dlg/00001fixture.dlg"),
            concat!(
                "{10}{Hello}{Hello}{}{}{}{lf2 1}
",
                "{11}{Invalid continuation}{}{1}{lf2 1}{-2}{gv3 90}
",
                "{12}{Continue}{}{1}{lf2 1}{-1}{gf7 1}
",
                "{20}{Continue again}{Continue again}{}{}{}{gv3 10}
",
                "{21}{Done}{}{1}{gv3 10, gf7 1}{}{lf2 0}
"
            ),
        )
        .unwrap();
        let mut vfs = Vfs::new(Policy::Ordered);
        vfs.mount_directory(&root).unwrap();
        let mut scripts = State::default();
        let context = Context {
            target: "fixture".into(),
            speaker: "NPC".into(),
            tile: (0, 0),
            facing: 0,
            npc_gender: None,
            npc_intelligence: None,
            npc_race: None,
            social_class: None,
            player_armor: None,
            met: false,
            stats: Character::default().statistics(&Rules::default(), None, 0, [0; 5], false),
            gold: 0,
            reaction: None,
            override_script: 0,
            entry: EntryContext {
                attachment: Attachment {
                    owner: "fixture".into(),
                    event: super::super::script::Event::Dialogue,
                    script: 1,
                    initial: Default::default(),
                },
                follows_player: None,
                npc_flags: None,
                hour: 12,
            },
        };
        let script = Script::load(&mut vfs, 1).unwrap();
        let entry = script
            .execute(&context.entry.invocation(0), &mut scripts)
            .unwrap();
        assert_eq!(entry.outcome, Outcome::Dialogue { line: 10 });
        let mut conversation = Conversation::open(
            &mut vfs,
            &context,
            script,
            entry.statement,
            10,
            &mut scripts,
        )
        .unwrap();
        let local = &context.entry.attachment;
        assert_eq!(scripts.read(local, Slot::LocalFlag(2)), Some(1));
        assert_eq!(conversation.turn.responses[0].id, Some(11));
        assert!(
            conversation
                .choose(&mut vfs, 0, &context.stats, 0, &mut scripts, 12)
                .unwrap_err()
                .contains("statement 2")
        );
        assert_eq!(conversation.turn.line, 10);
        assert_eq!(scripts.read(local, Slot::GlobalVariable(3)), Some(0));
        assert!(
            conversation
                .choose(&mut vfs, 1, &context.stats, 0, &mut scripts, 12)
                .unwrap()
        );
        assert_eq!(conversation.turn.line, 20);
        assert_eq!(conversation.turn.responses[0].id, Some(21));
        assert!(
            !conversation
                .choose(&mut vfs, 0, &context.stats, 0, &mut scripts, 12)
                .unwrap()
        );
        assert_eq!(scripts.read(local, Slot::LocalFlag(2)), Some(0));
        drop(vfs);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn supported_conditions_combine_and_unknowns_fail_closed() {
        let stats = Character::default().statistics(&Rules::default(), None, 0, [0; 5], false);
        let attachment = Attachment {
            owner: "fixture".into(),
            event: super::super::script::Event::Dialogue,
            script: 1,
            initial: Default::default(),
        };
        let mut scripts = State::default();
        let state = Conditions {
            reaction: Some(60),
            ..Conditions::default()
        };
        let check =
            |text, scripts: &State| conditions(text, &stats, 5, &state, scripts, &attachment);
        assert_eq!(check("ch8, pe -9 ra1 $$5 re-61", &scripts), (true, vec![]));
        assert_eq!(check("ch9, pe -9, $$5", &scripts), (false, vec![]));
        assert_eq!(
            check("ch8gf2010 0, qu1020 2", &scripts),
            (false, vec!["qu1020 2".into()])
        );
        let writes = effects("GF2010 1, lf3 0, lf3 1, gv4 -5, lc2 3").unwrap();
        apply_effects(&writes, &mut scripts, &attachment).unwrap();
        assert_eq!(
            check("gf2010 1 lf3 1 gv4 -5 lc2 3", &scripts),
            (true, vec![])
        );
        assert!(effects("gf2010 0, qu1020 2, lf3 0").is_none());
        assert!(effects("gv2000 -5").is_none());
    }
}
