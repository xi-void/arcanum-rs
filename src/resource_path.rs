//! Virtual resource names, independent of the host filesystem.

use std::error::Error;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidResourcePath;

impl fmt::Display for InvalidResourcePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("expected a relative resource path without parent components, colons, or control characters")
    }
}

impl Error for InvalidResourcePath {}

/// Normalize a Windows-style resource path to slash separators and ASCII lowercase.
///
/// Empty components and single dots are skipped. Absolute paths and parent
/// components are rejected, so the result is always a relative virtual name.
pub fn normalize(path: &str) -> Result<String, InvalidResourcePath> {
    if path.starts_with(['/', '\\']) || path.contains(|ch: char| ch == ':' || ch.is_control()) {
        return Err(InvalidResourcePath);
    }

    let mut normalized = String::with_capacity(path.len());
    for component in path.split(['/', '\\']) {
        match component {
            "" | "." => continue,
            ".." => return Err(InvalidResourcePath),
            _ => {}
        }
        if !normalized.is_empty() {
            normalized.push('/');
        }
        normalized.push_str(component);
    }
    if normalized.is_empty() {
        return Err(InvalidResourcePath);
    }
    normalized.make_ascii_lowercase();
    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_relative_windows_paths() {
        assert_eq!(
            normalize(r".\Art\\UI/./Button.ART").unwrap(),
            "art/ui/button.art"
        );
        assert_eq!(normalize("Maps/Café").unwrap(), "maps/café");
    }

    #[test]
    fn rejects_unsafe_or_empty_names() {
        for path in [
            "",
            ".",
            "/art",
            r"\art",
            r"\\host\share",
            "C:art",
            "x:y",
            "art/../map",
            "a\0b",
        ] {
            assert_eq!(normalize(path), Err(InvalidResourcePath), "{path:?}");
        }
    }
}
