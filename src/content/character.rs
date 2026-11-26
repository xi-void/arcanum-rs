//! Character calculations and equipment rules verified against CE's stat, critter and item code.
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

/// The relevant additive clauses of a winning rules/effect.mes entry. Other
/// clauses remain visible to diagnostics and are outside this rules slice.
#[derive(Default, Debug)]
pub(crate) struct Adjustments {
    stats: [i32; STATS.len()],
    special: [i32; SPECIAL.len()],
    resistance: [i32; 5],
    heal_percent: i32,
    picklock: i32,
    crithitchance: i32,
    critfailchance: i32,
    haggle: i32,
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
            let target = if key == "picklock" {
                Some((&mut result.picklock, value))
            } else if key == "crithitchance" {
                Some((&mut result.crithitchance, value))
            } else if key == "critfailchance" {
                Some((&mut result.critfailchance, value))
            } else if key == "haggle" {
                Some((&mut result.haggle, value))
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
    pub(crate) race: Option<Adjustments>,
    pub(crate) gender: Option<Adjustments>,
    pub(crate) background: Option<Adjustments>,
    pub(crate) encumbrance: Option<Adjustments>,
    pub(crate) terrain_flags: [Vec<(bool, bool)>; 4],
}

impl Rules {
    pub(crate) fn pick_modifiers(&self) -> [i64; 3] {
        let mut sums = [0; 3];
        for effect in self.modifiers().chain(&self.encumbrance) {
            for (sum, value) in
                sums.iter_mut()
                    .zip([effect.picklock, effect.crithitchance, effect.critfailchance])
            {
                *sum += i64::from(value);
            }
        }
        sums
    }

    pub(crate) fn haggle(&self) -> i64 {
        self.modifiers()
            .chain(&self.encumbrance)
            .map(|effect| i64::from(effect.haggle))
            .sum()
    }

    fn modifiers(&self) -> impl Iterator<Item = &Adjustments> {
        self.race.iter().chain(&self.gender).chain(&self.background)
    }

    fn stat(&self, index: usize) -> i64 {
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
    pub(crate) base: [i32; 28],
    #[serde(default)]
    pub(crate) haggle: u32,
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
            haggle: 0,
            tech_skills: [0; 4],
            colleges: [0; 16],
            mastery: None,
            fatigue_damage: 0,
            recover_at: None,
            critter_flags: 0,
            critter_flags2: 0,
            spell_flags: 0,
            background: 0,
            original_palette: 0,
            base_ac: 0,
            base_resistance: [0; 5],
            hp: [0; 2],
            fatigue: [0; 2],
            sector_aptitude: 0,
            environment: None,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct CharacterStatistics {
    pub base: [i32; 28],
    pub attributes: [i32; 8],
    pub race: i32,
    pub gender: i32,
    pub background: i32,
    pub race_effect: Option<i32>,
    pub weight: Option<i64>,
    pub carry_capacity: i32,
    pub encumbrance: Option<Encumbrance>,
    pub speed: Option<i32>,
    pub damage_bonus: i32,
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

/// CE packs investment and training into one entry in the original skill array.
#[derive(Debug, Serialize)]
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
            |value: i64, index, min, max| (value + rules.stat(index)).clamp(min, max) as i32;
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
        let level = adjusted(i64::from(self.base[17]), 17, 0, 51);
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
            let raw = i64::from(dexterity + if dexterity >= 20 { 5 } else { 0 })
                + rules.stat(11)
                + burden;
            Some(raw.clamp(1, 100) as i32)
        });
        let damage = strength - 10;
        let damage = if damage < 0 { damage / 2 } else { damage };
        let mut unresolved = Vec::new();
        if rules.race.is_none() || self.gender() == 0 && rules.gender.is_none() {
            unresolved.push("racial or gender effect entry unavailable");
        }
        if rules.modifiers().any(|m| !m.other.is_empty()) {
            unresolved.push(
                "racial, gender or background effects outside the supported statistic subset",
            );
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
        let max_fatigue = i64::from(self.fatigue[0]) * 4
            + i64::from(self.fatigue[1])
            + i64::from(willpower + 2 * (constitution + level) + 4)
            + special(1);
        CharacterStatistics {
            base: self.base,
            attributes,
            race: self.race(),
            gender: self.gender(),
            background: self.background,
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
            max_health: i64::from(self.hp[0]) * 4
                + i64::from(self.hp[1])
                + i64::from(willpower + 2 * (strength + level) + 4)
                + special(0),
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
                + rules.stat(10)
                + i64::from(adjusted(i64::from(dexterity - 10), 10, -9, 95))
                + armor)
                .clamp(0, 95) as i32,
            defense: 0,
            resistance: std::array::from_fn(|i| {
                let racial = rules
                    .modifiers()
                    .map(|m| i64::from(m.resistance[i]))
                    .sum::<i64>();
                let constitution = if i == 3 {
                    5 * (constitution - 4).max(0)
                } else {
                    0
                };
                (resistance[i]
                    + i64::from(self.base_resistance[i])
                    + i64::from(constitution)
                    + racial)
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
