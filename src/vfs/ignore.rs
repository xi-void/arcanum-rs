//! Archive-local TIG sidecar patterns.

use cap_std::fs::Dir;
use std::collections::BTreeMap;
use std::io::{self, BufRead, BufReader};
use std::path::{Path, PathBuf};

#[derive(Default)]
pub(super) struct IgnoreRules(BTreeMap<usize, Vec<Pattern>>);

pub(super) fn sidecar(archive: &Path) -> PathBuf {
    let mut path = archive.as_os_str().to_owned();
    path.push(".ignore");
    path.into()
}

impl IgnoreRules {
    pub fn load(root: &Dir, archive: &Path) -> io::Result<Self> {
        let path = sidecar(archive);
        match root.symlink_metadata(&path) {
            Ok(info) if info.is_file() => {}
            Ok(_) => return Ok(Self::default()),
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(err) => return Err(err),
        }
        let mut rules = BTreeMap::<usize, Vec<Pattern>>::new();
        for line in BufReader::new(root.open(&path)?).split(b'\n') {
            let mut line = line?;
            if cfg!(windows) && line.ends_with(b"\r") {
                line.pop();
            }
            line.truncate(
                line.iter()
                    .position(|&byte| byte == 0)
                    .unwrap_or(line.len()),
            );
            for byte in &mut line {
                *byte = if *byte == b'\\' {
                    b'/'
                } else {
                    byte.to_ascii_lowercase()
                };
            }
            if let Some(pattern) = Pattern::parse(&line) {
                rules
                    .entry(separator_count(&line))
                    .or_default()
                    .push(pattern);
            }
        }
        Ok(Self(rules))
    }

    pub fn matches(&self, path: &str) -> bool {
        !self.0.is_empty()
            && self
                .0
                .get(&separator_count(path.as_bytes()))
                .is_some_and(|rules| rules.iter().any(|rule| rule.matches(path.as_bytes())))
    }
}

fn separator_count(path: &[u8]) -> usize {
    path.iter().filter(|&&byte| byte == b'/').count()
}

enum Token {
    Literal(u8),
    Any,
    Star,
    UntilDot,
    Set {
        ranges: Vec<(u8, u8)>,
        negated: bool,
    },
    NegateSuffix,
}

enum Pattern {
    Literal(Vec<u8>),
    Wildcard(Vec<Token>),
}

impl Pattern {
    fn parse(text: &[u8]) -> Option<Self> {
        if text.is_empty() {
            return None;
        }
        // TIG converts backslashes to separators on Linux before matching.
        // Windows fpattern uses a backtick to quote pattern metacharacters.
        if !text.iter().any(|byte| {
            matches!(byte, b'?' | b'*' | b'[' | b'!' | 0x1a) || (cfg!(windows) && *byte == b'`')
        }) {
            return Some(Self::Literal(text.to_vec()));
        }
        let mut bytes = text.iter().copied().peekable();
        let set_bound = |bytes: &mut std::iter::Peekable<_>| {
            let byte = bytes.next()?;
            if cfg!(windows) && byte == b'`' {
                bytes.next()
            } else {
                Some(byte)
            }
        };
        let mut tokens = Vec::new();
        while let Some(byte) = bytes.next() {
            let token = match byte {
                b'?' => Token::Any,
                b'*' => Token::Star,
                0x1a => Token::UntilDot,
                b'!' if bytes.peek().is_some() => Token::NegateSuffix,
                b'!' => return None,
                b'`' if cfg!(windows) => Token::Literal(bytes.next()?),
                b'[' => {
                    let negated = bytes.next_if_eq(&b'!').is_some();
                    let mut ranges = Vec::new();
                    while bytes.next_if_eq(&b']').is_none() {
                        let low = set_bound(&mut bytes)?;
                        let high = if bytes.next_if_eq(&b'-').is_some() {
                            set_bound(&mut bytes)?
                        } else {
                            low
                        };
                        ranges.push((low, high));
                    }
                    Token::Set { ranges, negated }
                }
                _ => Token::Literal(byte),
            };
            tokens.push(token);
        }
        Some(Self::Wildcard(tokens))
    }

    fn matches(&self, path: &[u8]) -> bool {
        let Self::Wildcard(tokens) = self else {
            return matches!(self, Self::Literal(literal) if literal == path);
        };
        // Each cell says whether the remaining tokens match that path suffix.
        // Work backwards to avoid recursive backtracking for repeated stars.
        let mut suffix = vec![false; path.len() + 1];
        let mut current = suffix.clone();
        suffix[path.len()] = true;
        for token in tokens.iter().rev() {
            current[path.len()] = match token {
                Token::Star | Token::UntilDot => suffix[path.len()],
                Token::NegateSuffix => !suffix[path.len()],
                _ => false,
            };
            for i in (0..path.len()).rev() {
                let byte = path[i];
                current[i] = match token {
                    Token::Literal(expected) => *expected == byte && suffix[i + 1],
                    Token::Any => suffix[i + 1],
                    Token::Star => suffix[i] || current[i + 1],
                    Token::UntilDot => suffix[i] || (byte != b'.' && current[i + 1]),
                    Token::Set { ranges, negated } => {
                        (ranges.iter().any(|&(a, b)| (a..=b).contains(&byte)) != *negated)
                            && suffix[i + 1]
                    }
                    Token::NegateSuffix => !suffix[i],
                };
            }
            std::mem::swap(&mut suffix, &mut current);
        }
        suffix[0]
    }
}
