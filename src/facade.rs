//! Legacy facade references baked into sector surface cells.

use crate::{mes::Mes, resource_path::normalize};
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FacadeId(u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fields {
    pub number: u16,
    pub outdoor: bool,
    pub flippable: bool,
    pub terrain: u8,
    pub frame: u16,
    pub walkable: bool,
}

impl FacadeId {
    pub fn from_raw(raw: u32) -> Option<Self> {
        (raw >> 28 == 0xb).then_some(Self(raw))
    }

    pub fn raw(self) -> u32 {
        self.0
    }

    pub fn fields(self) -> Fields {
        let raw = self.0;
        Fields {
            number: (((raw >> 17) & 0xff) | ((raw >> 19) & 0x100)) as u16,
            outdoor: raw & (1 << 25) != 0,
            flippable: raw & (1 << 26) != 0,
            terrain: ((raw >> 11) & 0x3f) as u8,
            frame: ((raw >> 1) & 0x3ff) as u16,
            walkable: raw & 1 != 0,
        }
    }

    /// Resolve the facade number as an ordinal in the sorted name table.
    /// The resulting name already selects the artwork orientation.
    pub fn artwork_path(self, names: &Mes) -> Result<String, Error> {
        if !(1..512).contains(&names.len()) || names.get(0).is_none() {
            return Err(Error::NameTable);
        }
        let number = self.fields().number;
        let (_, stem) = names
            .entries()
            .skip_while(|(id, _)| *id < 0)
            .nth(usize::from(number))
            .ok_or(Error::MissingName(number))?;
        if stem.trim().is_empty() || matches!(stem, "." | "..") || stem.contains(['/', '\\']) {
            return Err(Error::InvalidName(number));
        }
        normalize(&format!("art/facade/{stem}.art")).map_err(|_| Error::InvalidName(number))
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    NameTable,
    MissingName(u16),
    InvalidName(u16),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NameTable => f.write_str("expected 1–511 facade names including MES key zero"),
            Self::MissingName(number) => write!(f, "missing facade name at ordinal {number}"),
            Self::InvalidName(number) => write!(f, "invalid facade filename at ordinal {number}"),
        }
    }
}

impl std::error::Error for Error {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_raw_word_and_decodes_distinct_fields() {
        assert!(FacadeId::from_raw(0xa5be_0801).is_none());
        let id = FacadeId::from_raw(0xbffe_ffff).unwrap();
        assert_eq!(id.raw(), 0xbffe_ffff);
        assert_eq!(
            id.fields(),
            Fields {
                number: 511,
                outdoor: true,
                flippable: true,
                terrain: 31,
                frame: 1023,
                walkable: true,
            }
        );
        let plain = FacadeId::from_raw(0xb000_0000).unwrap().fields();
        assert!(!plain.flippable && !plain.walkable && !plain.outdoor);
    }

    #[test]
    fn resolves_sorted_ordinal_and_requires_safe_names() {
        let id = FacadeId::from_raw(0xb402_0801).unwrap();
        for (input, result) in [
            (
                "{7}{Rug-Flip}{0}{Rug}{-1}{Other}",
                Ok("art/facade/rug-flip.art".into()),
            ),
            ("{1}{Rug}", Err(Error::NameTable)),
            ("{0}{Rug}", Err(Error::MissingName(1))),
            ("{0}{Rug}{7}{../Rug}", Err(Error::InvalidName(1))),
            ("{0}{Rug}{7}{C:Rug}", Err(Error::InvalidName(1))),
        ] {
            assert_eq!(id.artwork_path(&Mes::parse_text(input).unwrap()), result);
        }
    }
}
