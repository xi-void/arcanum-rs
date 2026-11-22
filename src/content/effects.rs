//! Bounded import of single-operation records from the legacy effect table.

use super::{Error, Source, items::ResistanceKind, load_mes};
use crate::{mes::Encoding, vfs::Vfs};
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
    /// Minimum resistance value on the game's percentage scale.
    ResistanceFloor {
        channel: ResistanceKind,
        minimum_percent: i32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectDefinition {
    pub id: EffectId,
    pub operation: EffectOperation,
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
    MissingTarget,
    MissingOperation,
    MissingValue,
    InvalidValue,
    UnsupportedTarget,
    UnsupportedOperation,
    MultipleOperations,
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::MissingTarget => "missing effect target",
            Self::MissingOperation => "missing effect operation",
            Self::MissingValue => "missing effect value",
            Self::InvalidValue => "expected a signed 32-bit decimal value",
            Self::UnsupportedTarget => "only ac and resistfire targets are supported",
            Self::UnsupportedOperation => "unsupported operation for this effect target",
            Self::MultipleOperations => "expected a single operation in the effect record",
        })
    }
}

impl std::error::Error for DecodeError {}

/// Decode either a signed AC addition or a Fire resistance minimum.
/// The caller is responsible for the MES framing and source encoding.
pub fn decode_effect(record: &str) -> Result<EffectOperation, DecodeError> {
    if record.contains(',') {
        return Err(DecodeError::MultipleOperations);
    }
    let mut tokens = record.split_ascii_whitespace();
    let target = tokens.next().ok_or(DecodeError::MissingTarget)?;
    if !target.eq_ignore_ascii_case("ac") && !target.eq_ignore_ascii_case("resistfire") {
        return Err(DecodeError::UnsupportedTarget);
    }
    let operator = tokens.next().ok_or(DecodeError::MissingOperation)?;
    let operation = if target.eq_ignore_ascii_case("ac") {
        if !operator.starts_with(['+', '-']) || operator.ends_with('%') {
            return Err(DecodeError::UnsupportedOperation);
        }
        let points = operator.parse().map_err(|_| DecodeError::InvalidValue)?;
        EffectOperation::AddArmourClass { points }
    } else {
        if !operator.eq_ignore_ascii_case("min") {
            return Err(DecodeError::UnsupportedOperation);
        }
        let minimum_percent = tokens
            .next()
            .ok_or(DecodeError::MissingValue)?
            .parse()
            .map_err(|_| DecodeError::InvalidValue)?;
        // CE reserves -1 to mean that no lower bound applies.
        if minimum_percent == -1 {
            return Err(DecodeError::UnsupportedOperation);
        }
        EffectOperation::ResistanceFloor {
            channel: ResistanceKind::Fire,
            minimum_percent,
        }
    };
    if tokens.next().is_some() {
        return Err(DecodeError::MultipleOperations);
    }
    Ok(operation)
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
    let operation = match decode_effect(record) {
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
            operation,
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
                decode_effect(record),
                Ok(EffectOperation::AddArmourClass { points })
            );
        }
    }

    #[test]
    fn decodes_fire_resistance_minimum_as_its_own_operation() {
        for (record, minimum_percent) in [("  resistfire min 50 ", 50), ("RESISTFIRE MIN -5", -5)] {
            assert_eq!(
                decode_effect(record),
                Ok(EffectOperation::ResistanceFloor {
                    channel: ResistanceKind::Fire,
                    minimum_percent,
                })
            );
        }
    }

    #[test]
    fn reports_missing_and_invalid_tokens() {
        for (record, error) in [
            ("", DecodeError::MissingTarget),
            ("ac", DecodeError::MissingOperation),
            ("resistfire", DecodeError::MissingOperation),
            ("resistfire min", DecodeError::MissingValue),
            ("ac +", DecodeError::InvalidValue),
            ("ac +2147483648", DecodeError::InvalidValue),
            ("resistfire min 50x", DecodeError::InvalidValue),
        ] {
            assert_eq!(decode_effect(record), Err(error), "{record:?}");
        }
    }

    #[test]
    fn rejects_effect_language_outside_the_verified_slice() {
        for (record, error) in [
            ("ac +5%", DecodeError::UnsupportedOperation),
            ("ac *2", DecodeError::UnsupportedOperation),
            ("ac min 30", DecodeError::UnsupportedOperation),
            ("resistfire +50", DecodeError::UnsupportedOperation),
            ("resistfire max 50", DecodeError::UnsupportedOperation),
            ("resistfire min -1", DecodeError::UnsupportedOperation),
            ("resistmagic min 50", DecodeError::UnsupportedTarget),
            ("ac +1, resistfire min 50", DecodeError::MultipleOperations),
            ("ac +1 ac +2", DecodeError::MultipleOperations),
        ] {
            assert_eq!(decode_effect(record), Err(error), "{record:?}");
        }
    }
}
