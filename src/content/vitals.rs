//! Damage channels and resistance shared by actor and object damage resolution.
use serde::Serialize;

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
        let index = match self {
            Self::Normal => 0,
            Self::Fire => 1,
            Self::Electrical => 2,
            Self::Poison => 3,
            Self::Magic => 4,
            Self::Fatigue => return amount.max(0),
        };
        let amount = i64::from(amount.max(0));
        (amount - amount * i64::from(resistances[index]) / 100).clamp(0, i64::from(i32::MAX)) as i32
    }
}
