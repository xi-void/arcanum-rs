//! Character-point transactions and the original spell, degree and experience tables.
use super::{Character, CharacterStatistics, Rules, SkillValue};
use crate::content::{
    load_mes,
    schematics::{LegacyGroup, LegacyRecipeId},
};
use crate::vfs::Vfs;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub(crate) const SKILL_KEYS: [&str; 16] = [
    "bow",
    "dodge",
    "melee",
    "throwing",
    "backstab",
    "pickpocket",
    "prowling",
    "spottrap",
    "gambling",
    "haggle",
    "heal",
    "persuasion",
    "repair",
    "firearms",
    "picklock",
    "armtrap",
];
pub(super) const KEY_STATS: [usize; 16] = [1, 1, 1, 1, 1, 1, 5, 5, 4, 6, 4, 7, 4, 5, 1, 5];
pub const DEGREE_INT: [i32; 8] = [0, 5, 8, 11, 13, 15, 17, 19];
pub const SPELL_LEVEL: [i32; 5] = [1, 1, 5, 10, 15];
const BASE_WILLPOWER: [i32; 5] = [6, 9, 12, 15, 18];

pub fn recipe_id(discipline: usize, rank: u32) -> Option<LegacyRecipeId> {
    (discipline < 8 && (1..=7).contains(&rank))
        .then(|| LegacyRecipeId(1990 + 200 * discipline as i32 + 10 * rank as i32))
}
pub(super) fn expertise(degree: u32, intelligence: i32) -> i32 {
    let rank = (1..=degree.min(7) as usize)
        .take_while(|&r| intelligence >= DEGREE_INT[r])
        .last()
        .unwrap_or(0);
    [0, 10, 20, 35, 50, 65, 80, 100][rank]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Investment {
    Attribute(usize),
    Health,
    Fatigue,
    Skill(usize),
    Spell(usize),
    Degree { discipline: usize, rank: u32 },
}

#[derive(Default)]
pub struct ProgressionRules {
    thresholds: Vec<i32>,
    schemes: BTreeMap<i32, String>,
    pub(crate) ai_packets: BTreeMap<u32, [i32; 17]>,
    pub(crate) quest_levels: std::sync::Arc<BTreeMap<i32, (i32, i32)>>,
    pub(crate) worn_spells: BTreeMap<u32, Vec<std::sync::Arc<super::Adjustments>>>,
    rewards: BTreeMap<i32, i32>,
    pub spells: Vec<SpellStudy>,
    pub(crate) spell_source: Option<crate::content::Source>,
    pub(crate) item_programs:
        BTreeMap<usize, Result<std::sync::Arc<crate::content::magick::Program>, String>>,
    pub(crate) spell_effects: BTreeMap<i32, std::sync::Arc<super::Adjustments>>,
    pub stat_names: Vec<String>,
    pub sheet_labels: Vec<String>,
    pub interface: BTreeMap<i32, String>,
    pub colleges: Vec<String>,
    pub disciplines: Vec<String>,
    pub degree_titles: Vec<String>,
    pub skills: Vec<String>,
    pub training: Vec<String>,
    pub recipes: BTreeMap<i32, Result<LearnedRecipe, String>>,
    pub sources: BTreeMap<String, String>,
}

pub struct SpellStudy {
    pub(crate) program: Result<std::sync::Arc<crate::content::magick::Program>, String>,
    pub name: String,
    pub willpower: Result<i32, String>,
}
pub struct LearnedRecipe {
    pub title: String,
    pub group: LegacyGroup,
}

impl ProgressionRules {
    pub(crate) fn load(vfs: &mut Vfs) -> Result<Self, Box<dyn std::error::Error>> {
        let mut rules = Self::default();
        let mut load = |path: &str| {
            let table = load_mes(vfs, path)?;
            if path == "rules/spelllist.mes" {
                rules.spell_source = Some(table.source.clone());
            }
            rules
                .sources
                .insert(path.to_owned(), table.source.to_string());
            Ok::<_, crate::content::Error>(table.mes)
        };
        for path in ["rules/ai_params.mes", "rules/gameai.mes"] {
            if let Ok(table) = load(path) {
                for (id, row) in table.entries() {
                    if let Ok(id) = u32::try_from(id)
                        && let Ok(values) = row
                            .split_ascii_whitespace()
                            .map(str::parse::<i32>)
                            .collect::<Result<Vec<_>, _>>()
                        && let Ok(packet) = values.try_into()
                    {
                        rules.ai_packets.insert(id, packet);
                    }
                }
            }
        }
        for (path, range) in [
            ("rules/userlevel.mes", 1..50),
            ("rules/gamelevel.mes", 50..200),
        ] {
            if let Ok(table) = load(path) {
                rules.schemes.extend(
                    table
                        .entries()
                        .filter(|(n, _)| range.contains(n))
                        .map(|(n, line)| (n, line.to_owned())),
                );
            }
        }
        let xp = load("rules/xp_level.mes")?;
        rules.thresholds.push(0);
        // The final contiguous level is the XP ceiling. Installations can extend the table.
        for level in 2.. {
            let Some(value) = xp.get(level) else { break };
            let threshold = value.trim().parse()?;
            if threshold <= *rules.thresholds.last().unwrap() {
                return Err(
                    format!("Invalid rules/xp_level.mes threshold at level {level}").into(),
                );
            }
            rules.thresholds.push(threshold);
        }
        rules.quest_levels = load("rules/gamequest.mes")?
            .entries()
            .filter_map(|(k, v)| {
                let mut cols = v.split_ascii_whitespace();
                Some((k, (cols.next()?.parse().ok()?, cols.next()?.parse().ok()?)))
            })
            .collect::<BTreeMap<_, _>>()
            .into();
        rules.rewards = load("rules/xp_quest.mes")?
            .entries()
            .filter_map(|(k, v)| Some((k, v.trim().parse().ok()?)))
            .collect();
        let names = load("mes/spell.mes")?;
        let list = load("rules/spelllist.mes")?;
        let effects = load("rules/effect.mes")?;
        rules.spell_effects = effects
            .entries()
            .filter_map(|(id, line)| {
                super::Adjustments::parse(line)
                    .ok()
                    .map(|e| (id, std::sync::Arc::new(e)))
            })
            .collect();
        // CE reserves 8000..10000 for non-spell entries. Discover item programs from
        // the winning table so content overrides can add spells above the stock list.
        for (base, _) in list
            .entries()
            .filter(|(base, _)| *base >= 1000 && !(8000..10000).contains(base) && base % 50 == 0)
        {
            let spell = (base - if base < 8000 { 1000 } else { 3000 }) / 50;
            if list.get(base + 3).is_some_and(|s| {
                s.split(',').any(|clause| {
                    clause.split_once(':').is_some_and(|(k, v)| {
                        k.trim().eq_ignore_ascii_case("itemtriggers")
                            && v.trim().eq_ignore_ascii_case("item_used")
                    })
                })
            }) {
                rules.item_programs.insert(
                    spell as usize,
                    crate::content::magick::Program::decode(&list, spell).map(std::sync::Arc::new),
                );
            }
            if let Some(ids) = wear_effects(&list, base)
                && let Some(values) = ids
                    .into_iter()
                    .map(|id| {
                        super::Adjustments::parse(effects.get(id)?)
                            .ok()
                            .map(std::sync::Arc::new)
                    })
                    .collect::<Option<Vec<_>>>()
            {
                rules.worn_spells.insert(spell as u32, values);
            }
        }
        let name = |table: &crate::mes::Mes, id| table.get(id).unwrap_or("?").to_owned();
        rules.colleges = (0..16).map(|i| name(&names, 500 + i)).collect();
        rules.spells = (0..80)
            .map(|i| {
                let key = 1000 + i * 50;
                let willpower = if list.get(key).is_none() {
                    Err(format!("Missing spell definition {key}"))
                } else if let Some(value) = list.get(key + 1).and_then(|s| {
                    s.split(',')
                        .filter_map(|part| part.split_once(':'))
                        .find(|(k, _)| k.trim().eq_ignore_ascii_case("IQ"))
                        .map(|(_, v)| v)
                }) {
                    value
                        .trim()
                        .parse()
                        .map_err(|_| format!("Invalid IQ requirement at spell entry {}", key + 1))
                } else {
                    Ok(BASE_WILLPOWER[i as usize % 5])
                };
                SpellStudy {
                    program: crate::content::magick::Program::decode(&list, i)
                        .map(std::sync::Arc::new),
                    name: name(&names, i),
                    willpower,
                }
            })
            .collect();
        let stat = load("mes/stat.mes")?;
        rules.stat_names = (0..41).map(|i| name(&stat, i)).collect();
        let labels = load("mes/charedit.mes")?;
        rules.sheet_labels = (0..45).map(|i| name(&labels, i)).collect();
        rules.interface = load("art/interface/interface.mes")?
            .entries()
            .map(|(i, s)| (i, format!("art/interface/{}", s.trim())))
            .collect();
        let skills = load("mes/skill.mes")?;
        rules.skills = (0..16).map(|i| name(&skills, i)).collect();
        rules.training = (16..20).map(|i| name(&skills, i)).collect();
        let tech = load("mes/tech.mes")?;
        rules.disciplines = (0..8).map(|i| name(&tech, i)).collect();
        rules.degree_titles = (8..16).map(|i| name(&tech, i)).collect();
        let groups = load("rules/schematic.mes")?;
        let titles = load("mes/schematic_text.mes")?;
        for d in 0..8 {
            for r in 1..=7 {
                let id = recipe_id(d, r).unwrap();
                rules.recipes.insert(
                    id.0,
                    LegacyGroup::decode(id, &groups)
                        .map(|group| LearnedRecipe {
                            title: name(&titles, group.title.0),
                            group,
                        })
                        .map_err(|e| e.to_string()),
                );
            }
        }
        Ok(rules)
    }
    pub(super) fn next_threshold(&self, level: i32) -> Option<i32> {
        (level >= 1 && (level as usize) < self.thresholds.len().saturating_sub(1))
            .then(|| self.thresholds.get(level as usize).copied())
            .flatten()
    }
    pub(crate) fn quest_reward(&self, id: i32) -> Result<(i32, i32), String> {
        let &(level, alignment) = self
            .quest_levels
            .get(&id)
            .ok_or_else(|| format!("Missing reward for quest {id}"))?;
        Ok((self.reward(level)?, alignment))
    }
    pub(crate) fn reward(&self, level: i32) -> Result<i32, String> {
        self.rewards
            .get(&level)
            .copied()
            .ok_or_else(|| format!("Missing rules/xp_quest.mes entry {level}"))
    }
}

pub(crate) fn validate_training(current: &super::SkillRank, tier: u32) -> Result<(), String> {
    if tier > 3 || tier > current.training + 1 || current.level < [0, 1, 9, 18][tier as usize] {
        return Err("Skill level or prior training is insufficient".into());
    }
    Ok(())
}

impl Character {
    pub(super) fn skill(&self, i: usize) -> u32 {
        if i < 12 {
            self.basic_skills[i]
        } else {
            self.tech_skills[i - 12]
        }
    }
    fn skill_mut(&mut self, i: usize) -> &mut u32 {
        if i < 12 {
            &mut self.basic_skills[i]
        } else {
            &mut self.tech_skills[i - 12]
        }
    }
    pub(crate) fn set_skill_investment(&mut self, index: usize, rank: u32) -> Result<(), String> {
        if index >= SKILL_KEYS.len() || rank > 5 {
            return Err("Expected a skill index 0–15 and an investment of 0–5".into());
        }
        let entry = self.skill_mut(index);
        let previous = *entry & 63;
        *entry = (*entry & !63) | rank;
        if index >= 12 {
            self.adjust_aptitude_points(23, previous, rank);
        }
        Ok(())
    }

    fn adjust_aptitude_points(&mut self, stat: usize, before: u32, after: u32) {
        self.base[stat] = self.base[stat]
            .saturating_add(after as i32 - before as i32)
            .clamp(0, 210);
    }

    pub(crate) fn set_college_rank(&mut self, college: usize, rank: u32) {
        self.adjust_aptitude_points(22, self.colleges[college], rank);
        self.colleges[college] = rank;
    }

    pub(crate) fn set_degree_rank(&mut self, discipline: usize, rank: u32) {
        self.adjust_aptitude_points(23, self.degrees[discipline], rank);
        self.degrees[discipline] = rank;
    }

    pub(crate) fn set_base_stat(&mut self, index: usize, value: i32) -> Result<(), String> {
        let range =
            match index {
                0..8 => 1..=self.primary_max(index),
                19 => -1000..=1000,
                20 => 0..=100,
                25 => 20..=1000,
                _ => return Err(
                    "Writable stats are the eight base attributes, alignment, fate points and age"
                        .into(),
                ),
            };
        if !range.contains(&value) {
            return Err(format!("Base stat {index} requires a value in {range:?}"));
        }
        self.base[index] = value;
        Ok(())
    }
    pub(crate) fn invest(
        &mut self,
        purchase: Investment,
        rules: &ProgressionRules,
        stats: &CharacterStatistics,
        undo: bool,
    ) -> Result<(), String> {
        let delta = if undo { -1 } else { 1 };
        let cost = match purchase {
            Investment::Attribute(i) if i < 8 => {
                i32::from(i64::from(self.base[i]) + i64::from(!undo) > 1)
            }
            _ => 1,
        };
        if !undo && self.base[21] < cost {
            return Err("No character points available".into());
        }
        match purchase {
            Investment::Attribute(i) if i < 8 => {
                if !undo
                    && (stats.attributes[i] >= self.primary_max(i)
                        || self.base[i] >= self.primary_max(i))
                {
                    return Err("Attribute is at its maximum".into());
                }
                if undo {
                    let lower = stats.attributes[i] - 1;
                    if KEY_STATS.iter().enumerate().any(|(s, &key)| {
                        key == i && (self.skill(s) & 63) * 4 > SkillValue::level(5, 0, lower) as u32
                    }) || i == 4
                        && (self.degrees.iter().any(|&d| DEGREE_INT[d as usize] > lower)
                            || lower < 5 && self.colleges.iter().any(|&c| c > 0))
                        || i == 6
                            && self.colleges.iter().enumerate().any(|(c, &rank)| {
                                (0..rank as usize).any(|r| {
                                    rules.spells[c * 5 + r]
                                        .willpower
                                        .as_ref()
                                        .is_ok_and(|wp| *wp > lower)
                                })
                            })
                    {
                        return Err("An existing investment requires this attribute".into());
                    }
                }
                self.base[i] += delta;
            }
            Investment::Health | Investment::Fatigue => {
                let points = if purchase == Investment::Health {
                    &mut self.hp[0]
                } else {
                    &mut self.fatigue[0]
                };
                *points = points
                    .checked_add(delta)
                    .filter(|n| *n >= 0)
                    .ok_or("Invalid vital point count")?;
            }
            Investment::Skill(i) if i < 16 => {
                let points = (self.skill(i) & 63) as i32 + delta;
                if !(0..=5).contains(&points)
                    || (!undo
                        && 4 * points > SkillValue::level(5, 0, stats.attributes[KEY_STATS[i]]))
                {
                    return Err(format!(
                        "Skill rank requires a higher governing attribute (rank {points})"
                    ));
                }
                self.set_skill_investment(i, points as u32)?;
            }
            Investment::Spell(i) if i < 80 => {
                let rank = i % 5;
                if self.colleges[i / 5] as usize != rank + usize::from(undo) {
                    return Err("Spells must be learned in college order".into());
                }
                if !undo {
                    let required = *rules
                        .spells
                        .get(i)
                        .ok_or("Spell unavailable")?
                        .willpower
                        .as_ref()
                        .map_err(Clone::clone)?;
                    if stats.level < SPELL_LEVEL[rank]
                        || stats.attributes[4] < 5
                        || stats.attributes[6] < required
                    {
                        return Err(format!(
                            "Requires level {}, IN 5 and WP {required}",
                            SPELL_LEVEL[rank]
                        ));
                    }
                }
                self.set_college_rank(i / 5, (rank + usize::from(!undo)) as u32);
            }
            Investment::Degree { discipline, rank } if recipe_id(discipline, rank).is_some() => {
                if self.degrees[discipline] != rank - u32::from(!undo) {
                    return Err("Degrees must be learned in discipline order".into());
                }
                if !undo && stats.attributes[4] < DEGREE_INT[rank as usize] {
                    return Err(format!("Requires IN {}", DEGREE_INT[rank as usize]));
                }
                self.set_degree_rank(discipline, rank - u32::from(undo));
            }
            _ => return Err("Unknown investment".into()),
        }
        self.base[21] = self.base[21].saturating_sub(delta * cost);
        Ok(())
    }

    pub(crate) fn train(
        &mut self,
        skill: usize,
        tier: u32,
        stats: &CharacterStatistics,
    ) -> Result<(), String> {
        let current = stats.skills.get(skill).ok_or("Unknown training skill")?;
        validate_training(current, tier)?;
        let entry = self.skill_mut(skill);
        *entry = (*entry & !(3 << 6)) | (tier << 6);
        Ok(())
    }

    pub(crate) fn gain_experience(&mut self, amount: i32, rules: &Rules) {
        if amount == 0 {
            return;
        }
        let (mut flat, mut percent) = (0i64, 100i64);
        for effect in rules.modifiers() {
            flat += i64::from(effect.xp_flat);
            percent += i64::from(effect.xp_percent);
        }
        let cap = rules
            .progression
            .thresholds
            .last()
            .copied()
            .unwrap_or(i32::MAX);
        self.base[18] = (i64::from(self.base[18]) + flat + i64::from(amount) * percent / 100)
            .clamp(0, i64::from(cap.max(self.base[18]))) as i32;
        while rules
            .progression
            .next_threshold(self.base[17])
            .is_some_and(|n| self.base[18] >= n)
        {
            self.base[17] += 1;
            self.base[21] = self.base[21]
                .saturating_add(1 + i32::from(self.base[17] % 5 == 0))
                .min(56);
        }
    }
}

/// CE dispatches each item spell to the wearer. Only unconditional, paired
/// Add/Remove components can be derived entirely from equipment ownership.
fn wear_effects(list: &crate::mes::Mes, base: i32) -> Option<Vec<i32>> {
    let flags = list.get(base + 3)?.to_ascii_lowercase();
    let triggers = flags
        .split(',')
        .map(str::trim)
        .find_map(|s| s.strip_prefix("itemtriggers:"))?;
    let mut events = triggers.split('|').map(str::trim).collect::<Vec<_>>();
    events.sort_unstable();
    if events != ["unwear", "wear"] {
        return None;
    }
    let mut wear = Vec::new();
    let mut unwear = Vec::<i32>::new();
    for (_, component) in list
        .entries()
        .filter(|(k, _)| (base + 5..base + 50).contains(k))
    {
        let component = component.to_ascii_lowercase();
        let words: Vec<_> = component.split(',').map(str::trim).collect();
        match words.as_slice() {
            ["[begin]", "itemtriggers: wear", "type: effect", id, "add"] => {
                wear.push(id.parse().ok()?)
            }
            [
                "[begin]",
                "itemtriggers: unwear",
                "type: effect",
                id,
                "remove",
            ] => unwear.push(id.parse().ok()?),
            ["[begin]", _, "type: eyecandy", ..] => {}
            _ => return None,
        }
    }
    wear.sort_unstable();
    unwear.sort_unstable();
    (!wear.is_empty() && wear == unwear).then_some(wear)
}

/// Original DLG AL supports relative changes, assignment and one-sided limits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Alignment {
    Add(i32),
    Set(i32),
    AtLeast(i32),
    AtMost(i32),
}
impl Alignment {
    pub(crate) fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        Some(match s.as_bytes().first()? {
            b'+' | b'-' => Self::Add(s.parse().ok()?),
            b'>' => Self::AtLeast(s[1..].trim().parse().ok()?),
            b'<' => Self::AtMost(s[1..].trim().parse().ok()?),
            _ => Self::Set(s.parse().ok()?),
        })
    }
    pub(crate) fn apply(self, base: i32) -> i32 {
        self.value(base).clamp(-1000, 1000)
    }
    pub(crate) fn value(self, base: i32) -> i32 {
        match self {
            Self::Add(n) => base.saturating_add(n),
            Self::Set(n) => n,
            Self::AtLeast(n) => base.max(n),
            Self::AtMost(n) => base.min(n),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn level_thresholds_bonus_points_and_terminal_ceiling() {
        let mut rules = Rules::default();
        rules.progression.thresholds = (0..=50).map(|n| n * 100).collect();
        let mut pc = Character::default();
        pc.gain_experience(399, &rules);
        assert_eq!((pc.base[17], pc.base[21]), (4, 8));
        pc.gain_experience(1, &rules);
        assert_eq!((pc.base[17], pc.base[21]), (5, 10));
        pc.gain_experience(i32::MAX, &rules);
        assert_eq!((pc.base[17], pc.base[18], pc.base[21]), (50, 5000, 56));
        rules.progression.thresholds.extend([5100, 5200]);
        pc.gain_experience(150, &rules);
        assert_eq!((pc.base[17], pc.base[18]), (52, 5150));
        pc.base[18] = 7000;
        pc.gain_experience(30, &rules);
        assert_eq!(pc.base[18], 7000);
    }
    #[test]
    fn alignment_uses_base_for_choices_and_effect_limits_for_checks() {
        let rules = Rules {
            background: Some(
                super::super::Adjustments::parse("alignment -200, alignment max -200").unwrap(),
            ),
            ..Default::default()
        };
        let mut pc = Character::default();
        for (directive, before, after) in [
            ("+150", 0, 150),
            ("<100", 150, 100),
            (">190", 100, 190),
            ("-2000", 190, -1000),
            ("500", -1000, 500),
        ] {
            assert_eq!(pc.base[19], before);
            pc.base[19] = Alignment::parse(directive).unwrap().apply(before);
            assert_eq!(pc.base[19], after);
            assert_eq!(pc.rules_alignment(&rules), (after - 200).clamp(-1000, -200));
        }
    }
    #[test]
    fn advancement_rejection_preserves_points_and_prerequisite() {
        let mut rules = Rules::default();
        rules.progression.spells = (0..80)
            .map(|i| SpellStudy {
                program: Err("no fixture".into()),
                name: String::new(),
                willpower: Ok(BASE_WILLPOWER[i % 5]),
            })
            .collect();
        let mut pc = Character::default();
        for (entry, allowed) in [
            (Investment::Spell(1), false),
            (Investment::Spell(0), true),
            (Investment::Spell(1), false),
            (Investment::Attribute(6), true),
            (Investment::Spell(1), true),
        ] {
            let old = pc.base;
            let stats = pc.statistics(&rules, None, 0, [0; 5], false);
            assert_eq!(
                pc.invest(entry, &rules.progression, &stats, false).is_ok(),
                allowed
            );
            if !allowed {
                assert_eq!(old, pc.base);
            }
        }
        let stats = pc.statistics(&rules, None, 0, [0; 5], false);
        assert_eq!(
            (stats.colleges[0], stats.aptitude, stats.base[21]),
            (2, 10, 2)
        );
        assert!(
            pc.invest(Investment::Attribute(6), &rules.progression, &stats, true)
                .is_err()
        );
    }
}

/// Purchased statistics are independent of the actor's current injuries, effects and inventory.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct NpcAdvancement {
    attributes: [i32; 8],
    level: i32,
    points: [i32; 3],
    basic: [u32; 12],
    technological: [u32; 4],
    colleges: [u32; 16],
    degrees: [u32; 8],
    vital_points: [i32; 2],
}

impl NpcAdvancement {
    pub(crate) fn apply(&self, pc: &mut Character) {
        pc.base[..8].copy_from_slice(&self.attributes);
        pc.base[17] = self.level;
        pc.base[21..24].copy_from_slice(&self.points);
        pc.basic_skills = self.basic;
        pc.tech_skills = self.technological;
        pc.colleges = self.colleges;
        pc.degrees = self.degrees;
        pc.hp[0] = self.vital_points[0];
        pc.fatigue[0] = self.vital_points[1];
    }
}

impl Character {
    pub(crate) fn advance_as_follower(
        &mut self,
        scheme: i32,
        old: i32,
        new: i32,
        rules: &Rules,
    ) -> Option<NpcAdvancement> {
        let current = self.base[17];
        let level = if current <= old {
            current.saturating_add(new - old)
        } else {
            new
        };
        let level = level.min(rules.progression.thresholds.len() as i32);
        if level <= current {
            return None;
        }
        self.base[17] = level;
        self.base[21] = self.base[21].saturating_add(level - current + level / 5 - current / 5);
        if scheme != 0
            && let Some(rule) = rules.progression.schemes.get(&scheme)
        {
            let fallback = rules.progression.schemes.get(&70).filter(|_| scheme != 70);
            for clause in std::iter::once(rule)
                .chain(fallback)
                .flat_map(|s| s.split(','))
            {
                let mut parts = clause.split_ascii_whitespace();
                let (Some(key), Some(score)) = (
                    parts.next(),
                    parts.next().and_then(|s| s.parse::<i32>().ok()),
                ) else {
                    break;
                };
                let key = key.to_ascii_lowercase();
                while self.base[21] > 0 {
                    let stats = self.statistics(&Rules::default(), Some(0), 0, [0; 5], false);
                    let locate = |names: &[&str]| names.iter().position(|&s| s == key);
                    let (value, purchase, prereq) = if let Some(i) =
                        locate(&["st", "dx", "cn", "be", "in", "pe", "wp", "ch"])
                    {
                        (self.base[i], Investment::Attribute(i), None)
                    } else if let Some(i) = locate(&SKILL_KEYS) {
                        (
                            stats.skills[i].level,
                            Investment::Skill(i),
                            Some(KEY_STATS[i]),
                        )
                    } else if let Some(i) = locate(&[
                        "conveyance",
                        "divination",
                        "air",
                        "earth",
                        "fire",
                        "water",
                        "force",
                        "mental",
                        "meta",
                        "morph",
                        "nature",
                        "necro_evil",
                        "necro_good",
                        "phantasm",
                        "summoning",
                        "temporal",
                    ]) {
                        let rank = self.colleges[i] as usize;
                        if rank >= 5 || SPELL_LEVEL[rank] > level {
                            break;
                        }
                        (
                            rank as i32,
                            Investment::Spell(i * 5 + rank),
                            Some(if stats.attributes[4] < 5 { 4 } else { 6 }),
                        )
                    } else if let Some(i) = locate(&[
                        "anatomical",
                        "chemistry",
                        "electric",
                        "explosives",
                        "gun_smithy",
                        "mechanical",
                        "smithy",
                        "therapeutics",
                    ]) {
                        (
                            self.degrees[i] as i32,
                            Investment::Degree {
                                discipline: i,
                                rank: self.degrees[i] + 1,
                            },
                            Some(4),
                        )
                    } else if key == "maxhps" {
                        (stats.max_health as i32, Investment::Health, None)
                    } else if key == "maxfatigue" {
                        (stats.max_fatigue as i32, Investment::Fatigue, None)
                    } else {
                        break;
                    };
                    if value >= score {
                        break;
                    }
                    if self
                        .invest(purchase, &rules.progression, &stats, false)
                        .is_err()
                        && !prereq.is_some_and(|i| {
                            self.invest(Investment::Attribute(i), &rules.progression, &stats, false)
                                .is_ok()
                        })
                    {
                        break;
                    }
                }
                if self.base[21] == 0 {
                    break;
                }
            }
        }
        Some(NpcAdvancement {
            attributes: self.base[..8].try_into().unwrap(),
            level,
            points: self.base[21..24].try_into().unwrap(),
            basic: self.basic_skills,
            technological: self.tech_skills,
            colleges: self.colleges,
            degrees: self.degrees,
            vital_points: [self.hp[0], self.fatigue[0]],
        })
    }
}
