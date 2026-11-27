//! Character calculations and equipment rules verified against CE's stat, critter and item code.
pub mod progression;
use progression::{KEY_STATS, ProgressionRules, SKILL_KEYS};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EquipmentSlot {
    Helmet,
    Ring1,
    Ring2,
    Medallion,
    Weapon,
    Shield,
    Armor,
    Gauntlet,
    Boots,
}

impl EquipmentSlot {
    pub const ALL: [Self; 9] = [
        Self::Helmet,
        Self::Ring1,
        Self::Ring2,
        Self::Medallion,
        Self::Weapon,
        Self::Shield,
        Self::Armor,
        Self::Gauntlet,
        Self::Boots,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Helmet => "helmet",
            Self::Ring1 => "ring1",
            Self::Ring2 => "ring2",
            Self::Medallion => "medallion",
            Self::Weapon => "weapon",
            Self::Shield => "shield",
            Self::Armor => "armor",
            Self::Gauntlet => "gauntlet",
            Self::Boots => "boots",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|slot| slot.name().eq_ignore_ascii_case(value))
    }

    pub(crate) fn accepts(self, category: u32, icon: Option<u32>, flags: u32) -> bool {
        let original = match category {
            5 => Some(Self::Weapon),
            7 => match icon.filter(|id| id >> 28 == 6).map(|id| (id >> 14) & 7) {
                Some(0) => Some(Self::Armor),
                Some(1) => Some(Self::Shield),
                Some(2) => Some(Self::Helmet),
                Some(3) => Some(Self::Gauntlet),
                Some(4) => Some(Self::Boots),
                Some(5) => Some(Self::Ring1),
                Some(6) => Some(Self::Medallion),
                _ => None,
            },
            14 if flags & 1 != 0 => Some(Self::Shield),
            _ => None,
        };
        original
            == Some(if self == Self::Ring2 {
                Self::Ring1
            } else {
                self
            })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Encumbrance {
    None,
    Light,
    Moderate,
    Medium,
    Significant,
    Heavy,
    Severe,
}

impl Encumbrance {
    pub const fn name(self) -> &'static str {
        [
            "None",
            "Light",
            "Moderate",
            "Medium",
            "Significant",
            "Heavy",
            "Severe",
        ][self as usize]
    }

    fn at(weight: i64, capacity: i32) -> Self {
        use Encumbrance::*;
        [None, Light, Moderate, Medium, Significant, Heavy, Severe]
            .into_iter()
            .zip([15, 30, 45, 60, 75, 90, 100])
            .find(|&(_, ratio)| weight < i64::from(capacity) * ratio / 100)
            .map_or(Severe, |(tier, _)| tier)
    }

    pub fn threshold(self, capacity: i32) -> i32 {
        capacity * [15, 30, 45, 60, 75, 90, 100][self as usize] / 100
    }

    fn effects(self) -> i32 {
        self as i32 + if self == Self::Severe { 2 } else { 0 }
    }
}

// CE effect_stat_lookup_tbl, in serialized STAT order.
const STATS: [&str; 26] = [
    "st",
    "dx",
    "cn",
    "be",
    "in",
    "pe",
    "wp",
    "ch",
    "carry",
    "damage",
    "ac",
    "speed",
    "healrate",
    "poisonrate",
    "beautyreaction",
    "maxfollowers",
    "aptitude",
    "level",
    "xpsUNUSED",
    "alignment",
    "fateUNUSED",
    "unspentUNUSED",
    "magicpts",
    "techpts",
    "poisonlevel",
    "age",
];
const SPECIAL: [&str; 3] = ["maxhps", "maxfatigue", "reaction"];
const RESISTANCE: [&str; 5] = [
    "resistdamage",
    "resistfire",
    "resistelectrical",
    "resistpoison",
    "resistmagic",
];

pub(super) fn service_price_percent(reaction: i32) -> i32 {
    let r = reaction.clamp(0, 100);
    if r < 50 {
        2 * (100 - r)
    } else {
        120 - 2 * r / 5
    }
}

/// The relevant additive clauses of a winning rules/effect.mes entry. Other
/// clauses remain visible to diagnostics and are outside this rules slice.
#[derive(Default, Debug)]
pub(crate) struct Adjustments {
    stats: [i32; STATS.len()],
    special: [i32; SPECIAL.len()],
    resistance: [i32; 5],
    heal_percent: i32,
    skills: [i32; 16],
    expertise: [i32; 8],
    social: [Vec<Modifier>; 3],
    xp_percent: i32,
    xp_flat: i32,
    crithitchance: i32,
    critfailchance: i32,
    critical_severity: [i32; 2],

    operations: Vec<(usize, Modifier)>,
    pub(crate) other: Vec<String>,
}

impl Adjustments {
    pub(crate) fn parse(text: &str) -> Result<Self, String> {
        let mut result = Self::default();
        for clause in text
            .split(',')
            .map(str::trim)
            .filter(|part| !part.is_empty())
        {
            let Some((key, value)) = clause.split_once(char::is_whitespace) else {
                result.other.push(clause.to_owned());
                continue;
            };
            let key = key.to_ascii_lowercase();
            let value = value.trim();
            if let Some(i) = ["alignment", "badreactionadj", "goodreactionadj"]
                .iter()
                .position(|&s| key == s)
            {
                if let Some(op) = Modifier::parse(value) {
                    result.social[i].push(op)
                } else {
                    result.other.push(clause.to_owned())
                }
                continue;
            }
            let key_index = STATS
                .iter()
                .position(|k| k.eq_ignore_ascii_case(&key))
                .or_else(|| RESISTANCE.iter().position(|k| *k == key).map(|i| 26 + i))
                .or_else(|| SKILL_KEYS.iter().position(|k| *k == key).map(|i| 31 + i));
            if let Some(i) = key_index
                && let Some(op) = Modifier::parse(value)
                && !matches!(op, Modifier::Add(_))
                && !(key == "healrate" && value.ends_with('%'))
            {
                result.operations.push((i, op));
                continue;
            }
            let target = if let Some(i) = SKILL_KEYS.iter().position(|&name| name == key) {
                Some((&mut result.skills[i], value))
            } else if let Some(i) = [
                "expertiseanatomical",
                "expertisechemistry",
                "expertiseelectric",
                "expertiseexplosives",
                "expertisegun_smithy",
                "expertisemechanical",
                "expertisesmithy",
                "expertisetherapeutics",
            ]
            .iter()
            .position(|&s| s == key)
            {
                Some((&mut result.expertise[i], value))
            } else if key == "crithiteffect" {
                Some((&mut result.critical_severity[0], value))
            } else if key == "critfaileffect" {
                Some((&mut result.critical_severity[1], value))
            } else if key == "crithitchance" {
                Some((&mut result.crithitchance, value))
            } else if key == "critfailchance" {
                Some((&mut result.critfailchance, value))
            } else if key == "xpgain" {
                if value.ends_with('%') {
                    Some((&mut result.xp_percent, value.trim_end_matches('%')))
                } else {
                    Some((&mut result.xp_flat, value))
                }
            } else if key == "healrate" && value.ends_with('%') {
                Some((&mut result.heal_percent, value.trim_end_matches('%')))
            } else {
                STATS
                    .iter()
                    .position(|candidate| candidate.eq_ignore_ascii_case(&key))
                    .map(|index| &mut result.stats[index])
                    .or_else(|| {
                        RESISTANCE
                            .iter()
                            .position(|&candidate| candidate == key)
                            .map(|index| &mut result.resistance[index])
                    })
                    .or_else(|| {
                        SPECIAL
                            .iter()
                            .position(|&candidate| candidate == key)
                            .map(|index| &mut result.special[index])
                    })
                    .map(|target| (target, value))
            };
            if let Some((target, value)) = target {
                if let Some(amount) = value
                    .parse::<i32>()
                    .ok()
                    .filter(|_| value.starts_with(['+', '-']))
                {
                    *target = target
                        .checked_add(amount)
                        .ok_or("Effect adjustment overflow")?;
                } else {
                    result.other.push(clause.to_owned());
                }
            } else {
                result.other.push(clause.to_owned());
            }
        }
        Ok(result)
    }
}

#[derive(Default)]
pub(crate) struct Rules {
    pub(crate) progression: ProgressionRules,
    pub(crate) tempo: i32,
    pub(crate) temporary_critter: u32,
    pub(crate) temporary: Vec<(std::sync::Arc<Adjustments>, i32)>,
    pub(crate) worn: Vec<std::sync::Arc<Adjustments>>,
    pub(crate) race: Option<Adjustments>,
    pub(crate) gender: Option<Adjustments>,
    pub(crate) background: Option<Adjustments>,
    pub(crate) encumbrance: Option<Adjustments>,
    pub(crate) terrain_flags: [Vec<(bool, bool)>; 4],
}

impl Rules {
    pub(crate) fn social(&self, i: usize) -> ModifierSum {
        let mut result = ModifierSum::default();
        for op in self.modifiers().flat_map(|m| m.social[i].iter()) {
            result.include(*op)
        }
        result
    }
    pub(crate) fn pick_modifiers(&self) -> [i64; 3] {
        let mut sums = [0; 3];
        for effect in self.modifiers().chain(&self.encumbrance) {
            for (sum, value) in sums.iter_mut().zip([
                effect.skills[14],
                effect.crithitchance,
                effect.critfailchance,
            ]) {
                *sum += i64::from(value);
            }
        }
        sums
    }

    pub(crate) fn skill_modifier(&self, skill: usize) -> i64 {
        self.evaluate(31 + skill, 0)
            + self
                .encumbrance
                .as_ref()
                .map_or(0, |effect| i64::from(effect.skills[skill]))
    }

    fn modifiers(&self) -> impl Iterator<Item = &Adjustments> {
        self.race
            .iter()
            .chain(&self.gender)
            .chain(&self.background)
            .chain(self.worn.iter().map(AsRef::as_ref))
    }

    fn evaluate(&self, index: usize, base: i64) -> i64 {
        let mut sum = ModifierSum::default();
        let additive = |a: &Adjustments| match index {
            0..26 => a.stats[index],
            26..31 => a.resistance[index - 26],
            _ => a.skills[index - 31],
        };
        for (a, count) in self
            .modifiers()
            .map(|a| (a, 1))
            .chain(self.temporary.iter().map(|(a, n)| (a.as_ref(), *n)))
        {
            sum.add = sum
                .add
                .saturating_add(i64::from(additive(a)) * i64::from(count));
            for (_, op) in a.operations.iter().filter(|(k, _)| *k == index) {
                match *op {
                    Modifier::Percent(n) => {
                        sum.percent = sum.percent.saturating_add(i64::from(n) * i64::from(count))
                    }
                    Modifier::Multiply(n) => {
                        sum.mul = sum
                            .mul
                            .saturating_mul(i64::from(n).saturating_pow(count as u32))
                    }
                    Modifier::Divide(n) => {
                        sum.div = sum
                            .div
                            .saturating_mul(i64::from(n).saturating_pow(count as u32))
                    }
                    op => sum.include(op),
                }
            }
        }
        i64::from(sum.apply(base.clamp(i32::MIN.into(), i32::MAX.into()) as i32))
    }
    pub(crate) fn stat(&self, index: usize) -> i64 {
        self.modifiers()
            .map(|effect| i64::from(effect.stats[index]))
            .sum()
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) struct Environment {
    pub daylight: bool,
    pub outdoors: bool,
    pub natural: bool,
    pub sinkable: bool,
    pub dark: bool,
}

#[derive(Clone, Serialize, serde::Deserialize)]
pub(crate) struct Character {
    #[serde(default)]
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) background_text: i32,
    pub(crate) base: [i32; 28],
    #[serde(default)]
    #[serde(skip_serializing, rename = "haggle")]
    pub(crate) legacy_haggle: Option<u32>,
    #[serde(default)]
    pub(crate) basic_skills: [u32; 12],
    #[serde(default)]
    pub(crate) degrees: [u32; 8],
    #[serde(default)]
    pub(crate) health_damage: i32,
    #[serde(default)]
    pub(crate) tech_skills: [u32; 4],
    #[serde(default)]
    pub(crate) colleges: [u32; 16],
    #[serde(default)]
    pub(crate) mastery: Option<u32>,
    #[serde(default)]
    pub(crate) fatigue_damage: i32,
    #[serde(default)]
    pub(crate) recover_at: Option<u64>,
    #[serde(default)]
    pub(crate) critter_flags: u32,
    #[serde(default)]
    pub(crate) critter_flags2: u32,
    #[serde(default)]
    pub(crate) spell_flags: u32,
    pub(crate) background: i32,
    #[serde(default)]
    pub(crate) portrait: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) sound_effect: Option<i32>,
    pub(crate) original_palette: u32,
    pub(crate) base_ac: i32,
    pub(crate) base_resistance: [i32; 5],
    pub(crate) hp: [i32; 2],
    pub(crate) fatigue: [i32; 2],
    #[serde(skip)]
    pub(crate) sector_aptitude: i32,
    #[serde(skip)]
    pub(crate) environment: Option<Environment>,
}

impl Default for Character {
    fn default() -> Self {
        // CE stat_set_defaults; the existing playable character is a human male.
        let mut base = [0; 28];
        base[..8].fill(8);
        base[17] = 1;
        base[21] = 5;
        base[25] = 20;
        base[26] = 1;
        Self {
            base,
            name: String::new(),
            background_text: 0,
            legacy_haggle: None,
            basic_skills: [0; 12],
            degrees: [0; 8],
            health_damage: 0,
            tech_skills: [0; 4],
            colleges: [0; 16],
            mastery: None,
            fatigue_damage: 0,
            recover_at: None,
            critter_flags: 0,
            critter_flags2: 0,
            spell_flags: 0,
            background: 0,
            sound_effect: None,
            original_palette: 0,
            portrait: 0,
            base_ac: 0,
            base_resistance: [0; 5],
            hp: [0; 2],
            fatigue: [0; 2],
            sector_aptitude: 0,
            environment: None,
        }
    }
}

// CE dialog.c sub_414F50 and script.c SAT_GET_STAT share this rule.
fn speech_intelligence(intelligence: i32, background: i32) -> i32 {
    if intelligence >= 5 && matches!(background, 55 | 74 | 75) {
        1
    } else {
        intelligence
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct CharacterStatistics {
    pub name: String,
    pub speech_intelligence: i32,
    pub alignment: i32,
    pub portrait: i32,
    pub skills: [SkillRank; 16],
    pub colleges: [u32; 16],
    pub degrees: [u32; 8],
    pub expertise: [i32; 8],
    pub next_level_xp: Option<i32>,
    pub current_health: i64,
    pub base: [i32; 28],
    pub attributes: [i32; 8],
    pub race: i32,
    pub gender: i32,
    pub background: i32,
    pub background_text: i32,
    pub race_effect: Option<i32>,
    pub weight: Option<i64>,
    pub carry_capacity: i32,
    pub encumbrance: Option<Encumbrance>,
    pub speed: Option<i32>,
    pub damage_bonus: i32,
    pub critical_severity: [i32; 2],
    pub ac_adjustment: i32,
    pub reaction_modifier: i32,
    pub heal_rate: i32,
    pub poison_recovery: i32,
    pub max_followers: i32,
    pub aptitude: i32,
    pub age: i32,
    pub level: i32,
    pub max_health: i64,
    pub max_fatigue: i64,
    pub current_fatigue: i64,
    pub pick_locks: SkillValue,
    pub reaction_adjustment: i64,
    pub armor_class: i32,
    pub defense: i32,
    pub resistance: [i32; 5],
    pub unresolved: Vec<&'static str>,
}

impl CharacterStatistics {
    pub(crate) fn script_values(&self) -> [Option<i32>; 28] {
        let mut values = self.base.map(Some);
        for (slot, attribute) in values.iter_mut().zip(self.attributes) {
            *slot = Some(attribute)
        }
        values[8..17].copy_from_slice(&[
            Some(self.carry_capacity),
            Some(self.damage_bonus),
            Some(self.ac_adjustment),
            self.speed,
            Some(self.heal_rate),
            Some(self.poison_recovery),
            Some(self.reaction_modifier),
            Some(self.max_followers),
            Some(self.aptitude),
        ]);
        values[4] = Some(self.speech_intelligence);
        values[19] = Some(self.alignment);
        values
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct SkillRank {
    pub points: u32,
    pub training: u32,
    pub level: i32,
}

/// CE packs investment and training into one entry in the original skill array.
#[derive(Clone, Debug, Serialize)]
pub struct SkillValue {
    pub points: u32,
    pub training: u32,
    pub level: i32,
    pub effectiveness: i32,
}

impl SkillValue {
    pub(crate) fn level(packed: u32, modifier: i64, key_stat: i32) -> i32 {
        let cap = match key_stat {
            ..=5 => 3,
            6..=8 => 7,
            9..=11 => 11,
            12..=14 => 15,
            15..=17 => 19,
            _ => 20,
        };
        (i64::from(4 * (packed & 63)) + modifier).clamp(0, cap) as i32
    }

    fn pick_locks(packed: u32, modifier: i64, dexterity: i32, intelligence: i32) -> Self {
        let level = Self::level(packed, modifier, dexterity);
        Self {
            points: packed & 63,
            training: (packed >> 6) & 3,
            level,
            effectiveness: (6 * level + if intelligence >= 20 { 10 } else { 0 }).min(95),
        }
    }
}

impl Character {
    pub(crate) fn validate_progression(&self) -> Result<(), String> {
        if self.health_damage < 0
            || self.degrees.iter().any(|&d| d > 7)
            || self
                .basic_skills
                .iter()
                .chain(&self.tech_skills)
                .any(|&v| v & 63 > 5)
            || self.base[18] < 0
            || self.base[21] < 0
            || self.fatigue_damage < 0
            || self.colleges.iter().any(|&level| level > 5)
            || self.mastery.is_some_and(|n| n >= 16)
        {
            return Err("Invalid character ability state".into());
        }
        Ok(())
    }

    fn rules_alignment(&self, rules: &Rules) -> i32 {
        rules.social(0).apply(self.base[19]).clamp(-1000, 1000)
    }
    pub(crate) fn race(&self) -> i32 {
        self.base[27]
    }

    pub(crate) fn race_effect(&self) -> Option<i32> {
        (0..=10).contains(&self.race()).then(|| 64 + self.race())
    }

    pub(crate) fn aptitude(&self, rules: &Rules) -> i32 {
        let magic_adjustment = self.environment.map_or(0, |e| match self.background {
            62 => {
                if e.daylight {
                    4
                } else {
                    -4
                }
            }
            63 => {
                if e.daylight {
                    -4
                } else {
                    4
                }
            }
            64 => {
                if e.outdoors {
                    4
                } else {
                    -4
                }
            }
            65 => {
                if e.natural {
                    4
                } else {
                    -4
                }
            }
            _ => 0,
        });
        let points = |index| {
            (i64::from(self.base[index])
                + rules.stat(index)
                + if index == 22 { magic_adjustment } else { 0 })
            .clamp(0, 210)
        };
        ((50 * points(22) - 55 * points(23)) / 10
            + i64::from(self.sector_aptitude)
            + rules.stat(16))
        .clamp(-100, 100) as i32
    }

    pub(crate) fn gender(&self) -> i32 {
        self.base[26]
    }

    pub(super) fn body_art(&self, armor_fragment: Option<u32>) -> Result<u32, &'static str> {
        // CE mainmenu_ui's playable race table, with non-player races from
        // proto.c's race_to_body_type_tbl. Half-elves use a human player body.
        let body = *[0, 1, 4, 0, 2, 2, 0, 3, 4, 3, 0]
            .get(self.race() as usize)
            .ok_or("Unresolved character body type")?;
        let gender = u32::try_from(self.gender())
            .ok()
            .filter(|&value| value < 2)
            .ok_or("Unresolved character gender")?;
        let (armor, palette) = match armor_fragment {
            None => (0, self.original_palette),
            Some(fragment) if fragment >> 28 == 6 => ((fragment >> 6) & 15, (fragment >> 4) & 3),
            _ => return Err("Invalid armor use ART fragment"),
        };
        Ok(0x2000_2000 | gender << 27 | body << 24 | armor << 20 | palette << 4)
    }

    pub(crate) fn armor_size(&self) -> Option<u32> {
        [2, 1, 2, 2, 1, 1, 2, 4, 2, 4, 2]
            .get(self.race() as usize)
            .copied()
    }

    fn primary_max(&self, index: usize) -> i32 {
        match (self.race(), index) {
            (1 | 6, 0 | 2) | (2 | 8, 1 | 3 | 6) | (3, 1) => 21,
            (4, 6) | (5, 1) | (10, 0 | 2) => 22,
            (7, 0) => 24,
            (9, 0) => 26,
            _ => 20,
        }
    }

    pub(crate) fn statistics(
        &self,
        rules: &Rules,
        weight: Option<i64>,
        armor: i64,
        resistance: [i64; 5],
        incomplete_equipment: bool,
    ) -> CharacterStatistics {
        let adjusted =
            |value: i64, index, min, max| rules.evaluate(index, value).clamp(min, max) as i32;
        let environmental_attribute = |i| {
            self.environment.map_or(0, |e| {
                if !matches!(i, 0 | 1 | 4 | 6) {
                    return 0;
                }
                if self.background == 66 && !e.outdoors {
                    return if i == 4 { 2 } else { 0 };
                }
                if (self.background == 66 && e.outdoors)
                    || (self.background == 67 && e.sinkable)
                    || (self.background == 68 && e.dark)
                {
                    return if i == 0 { 2 } else { -2 };
                }
                0
            })
        };
        let attributes: [i32; 8] = std::array::from_fn(|i| {
            adjusted(
                i64::from(self.base[i]) + environmental_attribute(i)
                    - if matches!(i, 0 | 1) {
                        ((i64::from(self.base[24]) + rules.stat(24)).max(0) / 100).min(3)
                    } else {
                        0
                    },
                i,
                1,
                i64::from(self.primary_max(i)),
            )
        });
        let [
            strength,
            dexterity,
            constitution,
            beauty,
            _,
            _,
            willpower,
            charisma,
        ] = attributes;
        let carry_capacity = adjusted(i64::from(500 * strength), 8, 300, 10_000);
        let level = adjusted(i64::from(self.base[17]), 17, 0, i64::from(i32::MAX));
        let special = |index: usize| {
            rules
                .modifiers()
                .map(|effect| i64::from(effect.special[index]))
                .sum::<i64>()
        };
        let encumbrance = weight.map(|w| Encumbrance::at(w, carry_capacity));
        let speed = encumbrance.and_then(|tier| {
            let burden = if tier == Encumbrance::None {
                0
            } else {
                i64::from(rules.encumbrance.as_ref()?.stats[11]) * i64::from(tier.effects())
            };
            let raw = rules.evaluate(
                11,
                i64::from(
                    dexterity + if dexterity >= 20 { 5 } else { 0 } + rules.tempo
                        - if (self.critter_flags | rules.temporary_critter) & 0x400 != 0 {
                            5
                        } else {
                            0
                        },
                ) + burden,
            );
            Some(raw.clamp(1, 100) as i32)
        });
        let damage = strength - 10;
        let damage = if damage < 0 { damage / 2 } else { damage };
        let mut unresolved = Vec::new();
        if rules.race.is_none() || self.gender() == 0 && rules.gender.is_none() {
            unresolved.push("racial or gender effect entry unavailable");
        }
        if rules.modifiers().any(|m| !m.other.is_empty()) {
            unresolved
                .push("character or equipment effects outside the supported statistic subset");
        }
        if (62..=68).contains(&self.background) && self.environment.is_none() {
            unresolved.push("location and time dependent background modifiers are unavailable");
        }
        if self.background != 0 && rules.background.is_none() {
            unresolved.push("background effect entry unavailable");
        }
        if encumbrance.is_some() && rules.encumbrance.is_none() {
            unresolved.push("encumbrance speed effect unavailable");
        }
        if incomplete_equipment {
            unresolved.push("equipment results exclude triggered spells or item lighting");
        }
        let max_health = i64::from(self.hp[0]) * 4
            + i64::from(self.hp[1])
            + i64::from(willpower + 2 * strength + 4)
            + 2 * i64::from(level)
            + special(0);
        let max_fatigue = i64::from(self.fatigue[0]) * 4
            + i64::from(self.fatigue[1])
            + i64::from(willpower + 2 * constitution + 4)
            + 2 * i64::from(level)
            + special(1);
        CharacterStatistics {
            name: self.name.clone(),
            critical_severity: std::array::from_fn(|index| {
                rules
                    .modifiers()
                    .fold(0i32, |n, e| n.saturating_add(e.critical_severity[index]))
            }),
            speech_intelligence: speech_intelligence(attributes[4], self.background),
            portrait: self.portrait,
            alignment: self.rules_alignment(rules),
            skills: std::array::from_fn(|i| {
                let packed = self.skill(i);
                SkillRank {
                    points: packed & 63,
                    training: (packed >> 6) & 3,
                    level: SkillValue::level(
                        packed,
                        rules.evaluate(31 + i, i64::from(4 * (packed & 63)))
                            - i64::from(4 * (packed & 63))
                            + rules
                                .encumbrance
                                .as_ref()
                                .map_or(0, |effect| i64::from(effect.skills[i])),
                        attributes[KEY_STATS[i]],
                    ),
                }
            }),
            colleges: self.colleges,
            degrees: self.degrees,
            expertise: std::array::from_fn(|i| {
                (i64::from(progression::expertise(self.degrees[i], attributes[4]))
                    + rules
                        .modifiers()
                        .map(|e| i64::from(e.expertise[i]))
                        .sum::<i64>())
                .clamp(i32::MIN.into(), i32::MAX.into()) as i32
            }),
            next_level_xp: rules.progression.next_threshold(level),
            current_health: max_health - i64::from(self.health_damage),
            base: self.base,
            attributes,
            race: self.race(),
            gender: self.gender(),
            background: self.background,
            background_text: self.background_text,
            race_effect: self.race_effect(),
            weight,
            carry_capacity,
            encumbrance,
            speed,
            damage_bonus: adjusted(
                i64::from(if strength >= 20 { damage * 2 } else { damage }),
                9,
                -50,
                50,
            ),
            ac_adjustment: adjusted(i64::from(dexterity - 10), 10, -9, 95),
            reaction_modifier: adjusted(
                i64::from(if beauty >= 20 {
                    2 * (5 * beauty - 50)
                } else {
                    // CE stat_reaction_tbl. Extraordinary beauty uses a distinct formula.
                    [
                        -65, -52, -42, -33, -25, -18, -12, -7, -3, 0, 3, 7, 12, 18, 25, 33, 42, 52,
                        65,
                    ][(beauty - 1) as usize]
                }),
                14,
                -65,
                200,
            ),
            heal_rate: {
                let value = i64::from((constitution + 1) / 3);
                let percent = rules
                    .modifiers()
                    .map(|effect| i64::from(effect.heal_percent))
                    .sum::<i64>();
                adjusted(value * (100 + percent) / 100, 12, 0, 6)
            },
            poison_recovery: adjusted(i64::from(constitution), 13, 1, 20),
            max_followers: adjusted(i64::from(charisma / 4), 15, 1, 7),
            aptitude: self.aptitude(rules),
            age: adjusted(i64::from(self.base[25]), 25, 20, 1000),
            level,
            max_health,
            max_fatigue,
            current_fatigue: max_fatigue - i64::from(self.fatigue_damage),
            pick_locks: SkillValue::pick_locks(
                self.tech_skills[2],
                rules.pick_modifiers()[0],
                dexterity,
                attributes[4],
            ),
            reaction_adjustment: special(2),
            // CE object_get_ac applies the AC effect to armor and again through STAT_AC_ADJUSTMENT.
            armor_class: (i64::from(self.base_ac)
                + rules.evaluate(10, 0)
                + i64::from(adjusted(i64::from(dexterity - 10), 10, -9, 95))
                + armor)
                .clamp(0, 95) as i32,
            defense: 0,
            resistance: std::array::from_fn(|i| {
                let constitution = if i == 3 {
                    5 * (constitution - 4).max(0)
                } else {
                    0
                };
                rules
                    .evaluate(
                        26 + i,
                        resistance[i]
                            + i64::from(self.base_resistance[i])
                            + i64::from(constitution),
                    )
                    .clamp(0, 95) as i32
            }),
            unresolved,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pick_locks_aptitude_and_current_fatigue_use_adjusted_character_values() {
        let mut pc = Character::default();
        pc.base[22] = 6;
        pc.base[23] = 2;
        pc.tech_skills[2] = 5 | (3 << 6);
        pc.fatigue_damage = 10;
        pc.fatigue = [2, -1];
        let rules = Rules {
            background: Some(
                Adjustments::parse("dx +1, wp +1, aptitude -3, maxfatigue +2").unwrap(),
            ),
            ..Default::default()
        };
        let stats = pc.statistics(&rules, None, 0, [0; 5], false);
        assert_eq!(stats.aptitude, 16);
        assert_eq!((stats.max_fatigue, stats.current_fatigue), (40, 30));
        assert_eq!(
            (
                stats.pick_locks.points,
                stats.pick_locks.training,
                stats.pick_locks.level,
                stats.pick_locks.effectiveness
            ),
            (5, 3, 11, 66)
        );
        pc.sector_aptitude = -25;
        pc.base[24] = 200;
        pc.background = 62;
        pc.environment = Some(Environment {
            daylight: true,
            outdoors: true,
            natural: true,
            sinkable: false,
            dark: false,
        });
        let stats = pc.statistics(&rules, None, 0, [0; 5], false);
        assert_eq!(stats.aptitude, 11);
        assert_eq!(stats.pick_locks.level, 7);
        assert_eq!(SkillValue::pick_locks(0, 2, 8, 20).effectiveness, 22);
    }

    #[test]
    fn slot_compatibility_uses_original_item_type_and_armor_coverage() {
        use EquipmentSlot::*;
        for (coverage, slot) in [Armor, Shield, Helmet, Gauntlet, Boots, Ring1, Medallion]
            .into_iter()
            .enumerate()
        {
            let art = Some(0x6000_0000 | ((coverage as u32) << 14));
            for current in EquipmentSlot::ALL {
                assert_eq!(
                    current.accepts(7, art, 0),
                    current == slot || (slot == Ring1 && current == Ring2)
                );
            }
        }
        assert!(Weapon.accepts(5, None, 0));
        assert!(Shield.accepts(14, None, 1));
        assert!(!Shield.accepts(14, None, 0));
        assert!(!Armor.accepts(7, Some(0x2000_0000), 0));
    }

    #[test]
    fn racial_adjustments_and_strict_encumbrance_thresholds() {
        let rules = Rules {
            race: Some(Adjustments::parse("st +1, dx -1, repair +2").unwrap()),
            encumbrance: Some(Adjustments::parse("speed -2").unwrap()),
            ..Rules::default()
        };
        let mut pc = Character::default();
        pc.base[27] = 1;
        let stats = |weight| pc.statistics(&rules, Some(weight), 5, [3, 0, 0, 0, 0], false);
        assert_eq!(stats(674).carry_capacity, 4500);
        assert_eq!(stats(674).speed, Some(7));
        assert_eq!(stats(675).encumbrance, Some(Encumbrance::Light));
        assert_eq!(stats(675).speed, Some(5));
        assert_eq!(stats(4050).encumbrance, Some(Encumbrance::Severe));
        assert_eq!(stats(4050).speed, Some(1));
        assert_eq!(stats(0).armor_class, 2);
        assert_eq!(stats(0).resistance, [3, 0, 0, 20, 0]);
    }
}

#[derive(Clone, Copy, Debug)]
enum Modifier {
    Add(i32),
    Multiply(i32),
    Divide(i32),
    Percent(i32),
    Min(i32),
    Max(i32),
}
impl Modifier {
    fn parse(s: &str) -> Option<Self> {
        Some(if let Some(n) = s.strip_prefix("min ") {
            Self::Min(n.trim().parse().ok()?)
        } else if let Some(n) = s.strip_prefix("max ") {
            Self::Max(n.trim().parse().ok()?)
        } else if let Some(n) = s.strip_suffix('%') {
            Self::Percent(n.parse().ok()?)
        } else if let Some(n) = s.strip_prefix('*') {
            Self::Multiply(n.parse().ok()?)
        } else if let Some(n) = s.strip_prefix('/') {
            Self::Divide(n.parse().ok().filter(|&n| n != 0)?)
        } else if s.starts_with(['+', '-']) {
            Self::Add(s.parse().ok()?)
        } else {
            return None;
        })
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ModifierSum {
    add: i64,
    percent: i64,
    mul: i64,
    div: i64,
    min: i32,
    max: i32,
}
impl Default for ModifierSum {
    fn default() -> Self {
        Self {
            add: 0,
            percent: 100,
            mul: 1,
            div: 1,
            min: i32::MIN,
            max: i32::MAX,
        }
    }
}
impl ModifierSum {
    fn include(&mut self, op: Modifier) {
        match op {
            Modifier::Add(n) => self.add += i64::from(n),
            Modifier::Percent(n) => self.percent += i64::from(n),
            Modifier::Multiply(n) => self.mul = self.mul.saturating_mul(n.into()),
            Modifier::Divide(n) => self.div = self.div.saturating_mul(n.into()),
            Modifier::Min(n) => self.min = self.min.max(n),
            Modifier::Max(n) => self.max = self.max.min(n),
        }
    }
    /// CE effect_adjust_func applies percentage, product, division, addition and limits in that order.
    pub(crate) fn apply(&self, n: i32) -> i32 {
        (i64::from(n).saturating_mul(self.percent) / 100)
            .saturating_mul(self.mul)
            .saturating_div(self.div)
            .saturating_add(self.add)
            .max(self.min.into())
            .min(self.max.into())
            .clamp(i32::MIN.into(), i32::MAX.into()) as i32
    }
}
