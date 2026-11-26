//! Built-in package manifests and external packages share the same loaders.
use std::{
    borrow::Cow,
    io::{self, Read},
    path::{Path, PathBuf},
};

#[derive(Clone)]
pub(crate) struct ThemeSource {
    path: PathBuf,
    embedded: Option<&'static str>,
}

impl ThemeSource {
    pub(crate) fn external(path: PathBuf) -> Self {
        Self {
            path,
            embedded: None,
        }
    }

    pub(crate) fn hud() -> Self {
        Self::builtin(
            "classic/theme.toml",
            include_str!("../../ui/classic/theme.toml"),
        )
    }

    pub(crate) fn map() -> Self {
        Self::builtin(
            "classic-map/theme.toml",
            include_str!("../../ui/classic-map/theme.toml"),
        )
    }

    pub(crate) fn pause() -> Self {
        let embedded = Self::embedded_pause();
        Self::builtin("classic-pause/theme.toml", embedded.embedded.unwrap())
    }

    pub(super) fn embedded_pause() -> Self {
        Self {
            path: PathBuf::from("ui/classic-pause/theme.toml"),
            embedded: Some(include_str!("../../ui/classic-pause/theme.toml")),
        }
    }

    pub(crate) fn console() -> Self {
        Self::builtin(
            "developer-console.toml",
            include_str!("../../ui/developer-console.toml"),
        )
    }

    fn builtin(name: &str, embedded: &'static str) -> Self {
        let path = Path::new("ui").join(name);
        if let Ok(executable) = std::env::current_exe()
            && let Some(directory) = executable.parent()
        {
            let candidate = directory.join(&path);
            if candidate.exists() {
                return Self::external(candidate);
            }
        }
        Self {
            path,
            embedded: Some(embedded),
        }
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(super) fn directory(&self) -> Option<&Path> {
        self.embedded.is_none().then(|| {
            self.path
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new("."))
        })
    }

    pub(super) fn read(&self, limit: u64) -> io::Result<Cow<'static, str>> {
        if let Some(source) = self.embedded {
            return Ok(Cow::Borrowed(source));
        }
        let mut source = String::new();
        std::fs::File::open(&self.path)?
            .take(limit)
            .read_to_string(&mut source)?;
        Ok(Cow::Owned(source))
    }
}
