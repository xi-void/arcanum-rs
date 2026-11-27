//! Damage channels and resistance shared by actor and object damage resolution.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Difficulty {
    Easy,
    #[default]
    Normal,
    Hard,
}

impl Difficulty {
    // CE skill.c and combat_dmg use integer adjustments to the player's
    // effectiveness and total HP damage, after channel-specific resistance.
    pub(crate) fn adjust(self, value: i32) -> i32 {
        match self {
            Self::Easy => value.saturating_add(value / 2),
            Self::Normal => value,
            Self::Hard => value - value / 4,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug, Serialize)]
pub(crate) enum Channel {
    Normal,
    Fire,
    Electrical,
    Poison,
    Magic,
    Fatigue,
}

impl Channel {
    pub(crate) fn after_resistance(self, amount: i32, resistances: &[i32; 5]) -> i32 {
        let resistance = match self {
            Self::Normal => resistances[0],
            Self::Fire => resistances[1],
            Self::Electrical => resistances[2],
            Self::Poison => resistances[3],
            Self::Magic => resistances[4],
            Self::Fatigue => (3 * i64::from(resistances[0]) / 4) as i32,
        };
        let amount = i64::from(amount.max(0));
        (amount - amount * i64::from(resistance.clamp(0, 100)) / 100) as i32
    }
}
