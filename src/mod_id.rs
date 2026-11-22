use serde::Deserialize;
use std::{borrow::Borrow, fmt};

/// Stable package identity, independent of its directory and display name.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
#[serde(try_from = "String")]
pub struct ModId(String);

impl ModId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for ModId {
    type Error = String;

    fn try_from(id: String) -> Result<Self, Self::Error> {
        let alphanumeric = |byte: u8| byte.is_ascii_lowercase() || byte.is_ascii_digit();
        if id.bytes().next().is_some_and(alphanumeric)
            && id
                .bytes()
                .all(|byte| alphanumeric(byte) || matches!(byte, b'-' | b'_' | b'.'))
        {
            Ok(Self(id))
        } else {
            Err(format!(
                "invalid mod ID {id:?}; use lowercase ASCII letters, digits, '.', '_' or '-', starting with a letter or digit"
            ))
        }
    }
}

impl Borrow<str> for ModId {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Debug for ModId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}

impl fmt::Display for ModId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
