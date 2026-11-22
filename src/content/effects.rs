//! Bounded import of typed operations from the legacy effect table.

use super::{Error, Source, items::ResistanceKind, load_mes};
use crate::{
    mes::{Encoding, MAX_INPUT_BYTES},
    vfs::Vfs,
};
use std::fmt;

const EFFECT_TABLE: &str = "rules/effect.mes";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LegacyEffectId(pub i32);

/// Stable native identifier, independent of effect activations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectId(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectOperation {
    AddArmourClass {
        points: i32,
    },
    AddResistance {
        channel: ResistanceKind,
        percentage_points: i32,
    },
    /// Minimum resistance value on the game's percentage scale.
    ResistanceFloor {
        channel: ResistanceKind,
        minimum_percent: i32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectDefinition {
    pub id: EffectId,
    /// Source order for inspection and diagnostics. Runtime evaluation order
    /// is independent of this sequence.
    pub operations: Vec<EffectOperation>,
}

#[derive(Debug)]
pub struct LoadedEffect {
    pub definition: EffectDefinition,
    pub legacy_id: LegacyEffectId,
    pub source: Source,
    pub encoding: Encoding,
    pub record: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeError {
    TooLarge,
    Clause {
        /// One-based clause index in the legacy record.
        index: usize,
        kind: ClauseError,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClauseError {
    MissingTarget,
    MissingOperation,
    MissingValue,
    InvalidValue,
    UnsupportedTarget,
    UnsupportedOperation,
    TrailingTokens,
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self::Clause { index, kind } = self else {
            return write!(
                f,
                "effect record exceeds the {MAX_INPUT_BYTES}-byte input limit"
            );
        };
        let message = match kind {
            ClauseError::MissingTarget => "missing effect target",
            ClauseError::MissingOperation => "missing effect operation",
            ClauseError::MissingValue => "missing effect value",
            ClauseError::InvalidValue => "expected a signed 32-bit decimal value",
            ClauseError::UnsupportedTarget => {
                "unsupported effect target (expected ac, resistfire, resistelectrical, or resistpoison)"
            }
            ClauseError::UnsupportedOperation => "unsupported operation for this effect target",
            ClauseError::TrailingTokens => {
                "expected a comma or the end of the record after the effect value"
            }
        };
        write!(f, "clause {index}: {message}")
    }
}

impl std::error::Error for DecodeError {}

/// Decode a comma-separated record in source order. Every clause must be
/// supported. The caller supplies the MES framing and source encoding.
pub fn decode_effect(record: &str) -> Result<Vec<EffectOperation>, DecodeError> {
    if record.len() > MAX_INPUT_BYTES {
        return Err(DecodeError::TooLarge);
    }
    record
        .split(',')
        .enumerate()
        .map(|(index, clause)| {
            decode_clause(clause).map_err(|kind| DecodeError::Clause {
                index: index + 1,
                kind,
            })
        })
        .collect()
}

fn decode_clause(clause: &str) -> Result<EffectOperation, ClauseError> {
    let mut tokens = clause.split_ascii_whitespace();
    let target = tokens.next().ok_or(ClauseError::MissingTarget)?;
    let operator = tokens.next().ok_or(ClauseError::MissingOperation)?;
    let operation = if target.eq_ignore_ascii_case("ac") {
        EffectOperation::AddArmourClass {
            points: decode_addition(operator)?,
        }
    } else if target.eq_ignore_ascii_case("resistelectrical") {
        EffectOperation::AddResistance {
            channel: ResistanceKind::Electrical,
            percentage_points: decode_addition(operator)?,
        }
    } else {
        let channel = if target.eq_ignore_ascii_case("resistfire") {
            ResistanceKind::Fire
        } else if target.eq_ignore_ascii_case("resistpoison") {
            ResistanceKind::Poison
        } else {
            return Err(ClauseError::UnsupportedTarget);
        };
        if !operator.eq_ignore_ascii_case("min") {
            return Err(ClauseError::UnsupportedOperation);
        }
        let minimum_percent = tokens
            .next()
            .ok_or(ClauseError::MissingValue)?
            .parse()
            .map_err(|_| ClauseError::InvalidValue)?;
        // CE reserves -1 to mean that no lower bound applies.
        if minimum_percent == -1 {
            return Err(ClauseError::UnsupportedOperation);
        }
        EffectOperation::ResistanceFloor {
            channel,
            minimum_percent,
        }
    };
    if tokens.next().is_some() {
        return Err(ClauseError::TrailingTokens);
    }
    Ok(operation)
}

fn decode_addition(token: &str) -> Result<i32, ClauseError> {
    if !token.starts_with(['+', '-']) || token.ends_with('%') {
        return Err(ClauseError::UnsupportedOperation);
    }
    token.parse().map_err(|_| ClauseError::InvalidValue)
}

#[derive(Debug)]
pub enum LoadEffectError {
    Resource(Error),
    InvalidId(LegacyEffectId),
    Missing {
        id: LegacyEffectId,
        source: Box<Source>,
    },
    Decode {
        id: LegacyEffectId,
        source: Box<Source>,
        error: DecodeError,
    },
}

impl fmt::Display for LoadEffectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Resource(error) => error.fmt(f),
            Self::InvalidId(id) => write!(
                f,
                "invalid legacy effect ID {} (expected zero or greater)",
                id.0
            ),
            Self::Missing { id, source } => write!(
                f,
                "{} from {source}: missing effect ID {}",
                source.virtual_path, id.0
            ),
            Self::Decode { id, source, error } => write!(
                f,
                "{} from {source}, effect {}: {error}",
                source.virtual_path, id.0
            ),
        }
    }
}

impl std::error::Error for LoadEffectError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Resource(error) => Some(error),
            Self::Decode { error, .. } => Some(error),
            _ => None,
        }
    }
}

/// Import one selected ID from the VFS winner, following the MES duplicate policy.
pub fn load_effect(vfs: &mut Vfs, id: LegacyEffectId) -> Result<LoadedEffect, LoadEffectError> {
    if id.0 < 0 {
        return Err(LoadEffectError::InvalidId(id));
    }
    let table = load_mes(vfs, EFFECT_TABLE).map_err(LoadEffectError::Resource)?;
    let Some(record) = table.mes.get(id.0) else {
        return Err(LoadEffectError::Missing {
            id,
            source: Box::new(table.source),
        });
    };
    let operations = match decode_effect(record) {
        Ok(operation) => operation,
        Err(error) => {
            return Err(LoadEffectError::Decode {
                id,
                source: Box::new(table.source),
                error,
            });
        }
    };
    Ok(LoadedEffect {
        definition: EffectDefinition {
            id: EffectId(format!("arcanum:effect/{}", id.0)),
            operations,
        },
        legacy_id: id,
        source: table.source,
        encoding: table.encoding,
        record: record.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_signed_armour_class_adjustments() {
        for (record, points) in [("ac +1", 1), ("AC\t-12", -12), ("ac +0", 0)] {
            assert_eq!(
                decode_effect(record).unwrap(),
                [EffectOperation::AddArmourClass { points }]
            );
        }
    }

    #[test]
    fn decodes_fire_resistance_minimum_as_its_own_operation() {
        for (record, minimum_percent) in [("  resistfire min 50 ", 50), ("RESISTFIRE MIN -5", -5)] {
            assert_eq!(
                decode_effect(record).unwrap(),
                [EffectOperation::ResistanceFloor {
                    channel: ResistanceKind::Fire,
                    minimum_percent
                }]
            );
        }
    }

    #[test]
    fn composite_preserves_order_signed_values_and_explicit_zeroes() {
        assert_eq!(
            decode_effect("resistpoison min 0, resistelectrical -8, ac +0, resistfire min 53")
                .unwrap(),
            [
                EffectOperation::ResistanceFloor {
                    channel: ResistanceKind::Poison,
                    minimum_percent: 0
                },
                EffectOperation::AddResistance {
                    channel: ResistanceKind::Electrical,
                    percentage_points: -8
                },
                EffectOperation::AddArmourClass { points: 0 },
                EffectOperation::ResistanceFloor {
                    channel: ResistanceKind::Fire,
                    minimum_percent: 53
                },
            ]
        );
    }

    #[test]
    fn reports_the_malformed_clause() {
        use ClauseError::*;
        for (record, index, kind) in [
            ("", 1, MissingTarget),
            ("ac", 1, MissingOperation),
            ("resistfire", 1, MissingOperation),
            ("resistfire min", 1, MissingValue),
            ("ac +", 1, InvalidValue),
            ("ac +2147483648", 1, InvalidValue),
            ("resistfire min 50x", 1, InvalidValue),
            ("ac +1, resistpoison min", 2, MissingValue),
            ("ac +1,, resistfire min 1", 2, MissingTarget),
            (", ac +1", 1, MissingTarget),
            ("ac +1,", 2, MissingTarget),
            ("ac +1 resistfire min 50", 1, TrailingTokens),
        ] {
            assert_eq!(
                decode_effect(record),
                Err(DecodeError::Clause { index, kind }),
                "{record:?}"
            );
        }
        assert_eq!(
            decode_effect(&" ".repeat(MAX_INPUT_BYTES + 1)),
            Err(DecodeError::TooLarge)
        );
    }

    #[test]
    fn rejects_the_entire_record_when_an_operation_is_unsupported() {
        use ClauseError::*;
        for (record, index, kind) in [
            ("ac +5%", 1, UnsupportedOperation),
            ("ac *2", 1, UnsupportedOperation),
            ("ac min 30", 1, UnsupportedOperation),
            ("resistfire +50", 1, UnsupportedOperation),
            ("resistfire max 50", 1, UnsupportedOperation),
            ("resistfire min -1", 1, UnsupportedOperation),
            ("resistmagic min 50", 1, UnsupportedTarget),
            ("ac +1, resistelectrical *2", 2, UnsupportedOperation),
            ("ac +1, resistfire min 50, speed +2", 3, UnsupportedTarget),
        ] {
            assert_eq!(
                decode_effect(record),
                Err(DecodeError::Clause { index, kind }),
                "{record:?}"
            );
        }
    }
}
