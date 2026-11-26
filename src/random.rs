//! Independent pseudorandom streams for runtime systems.

use serde::{Deserialize, Serialize};
use std::{
    num::NonZeroU64,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct Random(NonZeroU64);

impl Random {
    pub fn fresh() -> Self {
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(1, |time| time.as_nanos() as u64 | 1);
        Self(NonZeroU64::new(seed).unwrap())
    }

    /// Advances the stream and returns a value in 0..limit.
    pub fn below(&mut self, limit: u64) -> u64 {
        let mut seed = self.0.get();
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        self.0 = NonZeroU64::new(seed).expect("xorshift preserves nonzero state");
        seed % limit
    }
}
