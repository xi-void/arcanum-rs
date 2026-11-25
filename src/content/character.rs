//! Character calculations and equipment rules verified against CE's stat, critter and item code.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
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

    fn effects(self) -> i32 {
        self as i32 + if self == Self::Severe { 2 } else { 0 }
    }
}

const PRIMARY: [&str; 8] = ["st", "dx", "cn", "be", "in", "pe", "wp", "ch"];
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
    primary: [i32; 8],
    resistance: [i32; 5],
    speed: i32,
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
            let target = match key.as_str() {
                "speed" => Some(&mut result.speed),
                _ => {
                    if let Some(index) = PRIMARY.iter().position(|&candidate| candidate == key) {
                        Some(&mut result.primary[index])
                    } else {
                        RESISTANCE
                            .iter()
                            .position(|&candidate| candidate == key)
                            .map(|index| &mut result.resistance[index])
                    }
                }
            };
            if let Some(target) = target {
                let value = value.trim();
                let amount: i32 = value
                    .parse()
                    .ok()
                    .filter(|_| value.starts_with(['+', '-']))
                    .ok_or_else(|| format!("Unsupported effect adjustment: {clause}"))?;
                *target = target
                    .checked_add(amount)
                    .ok_or("Effect adjustment overflow")?;
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
    pub(crate) encumbrance: Option<Adjustments>,
}

pub(crate) struct Character {
    pub(crate) base: [i32; 28],
    pub(crate) original_palette: u32,
    pub(crate) base_ac: i32,
    pub(crate) base_resistance: [i32; 5],
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
            original_palette: 0,
            base_ac: 0,
            base_resistance: [0; 5],
        }
    }
}

#[derive(Debug, Serialize)]
pub struct CharacterStatistics {
    pub base: [i32; 28],
    pub attributes: [i32; 8],
    pub race: i32,
    pub gender: i32,
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
    pub armor_class: i32,
    pub resistance: [i32; 5],
    pub unresolved: Vec<&'static str>,
}

impl Character {
    pub(crate) fn race(&self) -> i32 {
        self.base[27]
    }

    pub(crate) fn race_effect(&self) -> Option<i32> {
        (0..=10).contains(&self.race()).then(|| 64 + self.race())
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
        Ok(super::mobile::Pose::standing_critter_art(
            gender, body, armor, palette,
        ))
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
        let modifiers = || rules.race.iter().chain(rules.gender.iter());
        let attributes: [i32; 8] = std::array::from_fn(|i| {
            let value =
                i64::from(self.base[i]) + modifiers().map(|m| i64::from(m.primary[i])).sum::<i64>();
            value.clamp(1, i64::from(self.primary_max(i))) as i32
        });
        let [strength, dexterity, constitution, beauty, _, _, _, charisma] = attributes;
        let carry_capacity = (500 * strength).clamp(300, 10_000);
        let encumbrance = weight.map(|w| Encumbrance::at(w, carry_capacity));
        let speed = encumbrance.and_then(|tier| {
            let burden = if tier == Encumbrance::None {
                0
            } else {
                i64::from(rules.encumbrance.as_ref()?.speed) * i64::from(tier.effects())
            };
            let raw = i64::from(dexterity + if dexterity >= 20 { 5 } else { 0 })
                + modifiers().map(|m| i64::from(m.speed)).sum::<i64>()
                + burden;
            Some(raw.clamp(1, 100) as i32)
        });
        let damage = strength - 10;
        let damage = if damage < 0 { damage / 2 } else { damage };
        let mut unresolved = Vec::new();
        if rules.race.is_none() || self.gender() == 0 && rules.gender.is_none() {
            unresolved.push("racial or gender effect entry unavailable");
        }
        if modifiers().any(|m| !m.other.is_empty()) {
            unresolved.push("racial or gender effects outside the supported statistic subset");
        }
        if weight.is_none() {
            unresolved
                .push("item weight requires unsupported aptitude adjustment or unresolved fields");
        }
        if encumbrance.is_some() && rules.encumbrance.is_none() {
            unresolved.push("encumbrance speed effect unavailable");
        }
        if incomplete_equipment {
            unresolved.push(
                "equipment results exclude unsupported aptitude modifiers, triggered spells or item lighting",
            );
        }
        CharacterStatistics {
            base: self.base,
            attributes,
            race: self.race(),
            gender: self.gender(),
            race_effect: self.race_effect(),
            weight,
            carry_capacity,
            encumbrance,
            speed,
            damage_bonus: if strength >= 20 { damage * 2 } else { damage },
            ac_adjustment: dexterity - 10,
            reaction_modifier: if beauty >= 20 {
                2 * (5 * beauty - 50)
            } else {
                // CE stat_reaction_tbl. Extraordinary beauty uses a distinct formula.
                [
                    -65, -52, -42, -33, -25, -18, -12, -7, -3, 0, 3, 7, 12, 18, 25, 33, 42, 52, 65,
                ][(beauty - 1) as usize]
            },
            heal_rate: ((constitution + 1) / 3).clamp(0, 6),
            poison_recovery: constitution.clamp(1, 20),
            max_followers: (charisma / 4).clamp(1, 7),
            armor_class: (i64::from(self.base_ac) + i64::from(dexterity - 10) + armor).clamp(0, 95)
                as i32,
            resistance: std::array::from_fn(|i| {
                let racial = modifiers().map(|m| i64::from(m.resistance[i])).sum::<i64>();
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
