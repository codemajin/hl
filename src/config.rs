use std::{
    env,
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
};

use toml::{Table, Value};

use crate::diag::{describe, warn};

/// The directories `hl` reads its files from, resolved once at startup and
/// handed to the code that needs them.
pub struct Dirs {
    /// `$XDG_CONFIG_HOME/hl`, else `~/.config/hl`. Unlike the config file,
    /// it does not follow `HL_CONFIG`.
    pub config: Option<PathBuf>,
    /// `$XDG_CACHE_HOME/hl`, else `~/.cache/hl`.
    pub cache: Option<PathBuf>,
}

impl Dirs {
    pub fn from_env() -> Self {
        Self::from_getenv(|name| env::var_os(name))
    }

    /// The directories as given by the environment variables that `getenv`
    /// returns.
    fn from_getenv(getenv: impl Fn(&str) -> Option<OsString>) -> Self {
        Self {
            config: xdg_dir(&getenv, "XDG_CONFIG_HOME", ".config"),
            cache: xdg_dir(&getenv, "XDG_CACHE_HOME", ".cache"),
        }
    }
}

/// `hl` under the XDG base directory in `var`, else under `~/<fallback>`.
fn xdg_dir(
    getenv: &impl Fn(&str) -> Option<OsString>,
    var: &str,
    fallback: &str,
) -> Option<PathBuf> {
    // The XDG spec says relative paths are invalid and should be ignored.
    let dir = getenv(var)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| getenv("HOME").map(|home| Path::new(&home).join(fallback)))?;
    Some(dir.join("hl"))
}

/// Default option values read from the config file.
pub struct Config {
    pub path: PathBuf,
    /// `(key, value)` pairs as written; their keys and types are checked
    /// against the options by the caller.
    pub entries: Vec<(String, Value)>,
}

impl Config {
    /// Reads the config file from `HL_CONFIG`, else from the config
    /// directory in `dirs`. A missing file at the default location is not
    /// an error; any other problem warns and yields no values, so a broken
    /// config never stops `hl` from printing.
    pub fn load(dirs: &Dirs) -> Option<Self> {
        Self::read(path(env::var_os("HL_CONFIG"), dirs.config.as_deref())?)
    }

    /// Reads the file at `path`; `explicit` tells whether the user named it,
    /// in which case a missing file warns.
    fn read((path, explicit): (PathBuf, bool)) -> Option<Self> {
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == io::ErrorKind::NotFound && !explicit => return None,
            Err(e) => {
                warn(&path, &describe(&e));
                return None;
            }
        };
        Self::parse(path.clone(), &text)
            .inspect_err(|msg| warn(&path, msg))
            .ok()
    }

    /// The config in `text`, read from `path`, or the message to warn with.
    fn parse(path: PathBuf, text: &str) -> Result<Self, String> {
        let table: Table = text.parse().map_err(|e: toml::de::Error| {
            format!("invalid TOML, ignoring the file: {}", e.message())
        })?;
        Ok(Self {
            path,
            entries: table.into_iter().collect(),
        })
    }

    pub fn warn(&self, msg: &str) {
        warn(&self.path, msg);
    }
}

/// The config file and whether it was given explicitly: `hl_config` (the
/// value of `HL_CONFIG`) unless it is empty, else `config.toml` in
/// `config_dir`.
fn path(hl_config: Option<OsString>, config_dir: Option<&Path>) -> Option<(PathBuf, bool)> {
    if let Some(path) = hl_config.filter(|p| !p.is_empty()) {
        return Some((path.into(), true));
    }
    Some((config_dir?.join("config.toml"), false))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    /// `Dirs` for an environment with only `vars` set.
    fn dirs_with(vars: &[(&str, &str)]) -> Dirs {
        let vars: HashMap<_, _> = vars.iter().copied().collect();
        Dirs::from_getenv(|name| vars.get(name).map(OsString::from))
    }

    #[test]
    fn uses_absolute_xdg_dirs() {
        let dirs = dirs_with(&[
            ("XDG_CONFIG_HOME", "/xdg/config"),
            ("XDG_CACHE_HOME", "/xdg/cache"),
            ("HOME", "/home/u"),
        ]);
        assert_eq!(dirs.config.as_deref(), Some(Path::new("/xdg/config/hl")));
        assert_eq!(dirs.cache.as_deref(), Some(Path::new("/xdg/cache/hl")));
    }

    #[test]
    fn falls_back_to_home_without_xdg_dirs() {
        let dirs = dirs_with(&[("HOME", "/home/u")]);
        assert_eq!(
            dirs.config.as_deref(),
            Some(Path::new("/home/u/.config/hl"))
        );
        assert_eq!(dirs.cache.as_deref(), Some(Path::new("/home/u/.cache/hl")));
    }

    /// The XDG spec says relative paths are invalid and must be ignored.
    #[test]
    fn ignores_relative_and_empty_xdg_dirs() {
        for value in ["relative", "./relative", ""] {
            let dirs = dirs_with(&[
                ("XDG_CONFIG_HOME", value),
                ("XDG_CACHE_HOME", value),
                ("HOME", "/home/u"),
            ]);
            assert_eq!(
                dirs.config.as_deref(),
                Some(Path::new("/home/u/.config/hl")),
                "{value:?}"
            );
            assert_eq!(
                dirs.cache.as_deref(),
                Some(Path::new("/home/u/.cache/hl")),
                "{value:?}"
            );
        }
    }

    #[test]
    fn has_no_dirs_without_home_or_xdg() {
        let dirs = dirs_with(&[]);
        assert_eq!(dirs.config, None);
        assert_eq!(dirs.cache, None);

        let dirs = dirs_with(&[("XDG_CONFIG_HOME", "rel"), ("XDG_CACHE_HOME", "rel")]);
        assert_eq!(dirs.config, None);
        assert_eq!(dirs.cache, None);
    }

    #[test]
    fn hl_config_names_the_file_explicitly() {
        let dir = Path::new("/c/hl");
        assert_eq!(
            path(Some("/x/my.toml".into()), Some(dir)),
            Some((PathBuf::from("/x/my.toml"), true))
        );
        assert_eq!(
            path(Some("/x/my.toml".into()), None),
            Some((PathBuf::from("/x/my.toml"), true))
        );
    }

    #[test]
    fn empty_hl_config_counts_as_unset() {
        let dir = Path::new("/c/hl");
        for hl_config in [None, Some(OsString::new())] {
            assert_eq!(
                path(hl_config, Some(dir)),
                Some((PathBuf::from("/c/hl/config.toml"), false))
            );
        }
        assert_eq!(path(Some(OsString::new()), None), None);
    }

    #[test]
    fn parses_the_entries() {
        let config = Config::parse("c.toml".into(), "color = \"always\"\ntab-width = 4\n").unwrap();
        assert_eq!(config.path, PathBuf::from("c.toml"));
        let keys: Vec<_> = config.entries.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["color", "tab-width"]);
        assert_eq!(config.entries[0].1.as_str(), Some("always"));
    }

    #[test]
    fn rejects_invalid_toml() {
        for text in ["color = ", "[unclosed", "a = 1\na = 2\n"] {
            let msg = Config::parse("c.toml".into(), text).err().unwrap();
            assert!(
                msg.starts_with("invalid TOML, ignoring the file: "),
                "{msg}"
            );
        }
    }

    #[test]
    fn reads_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("config.toml");

        assert!(Config::read((file.clone(), false)).is_none());
        // Warns, but still yields nothing.
        assert!(Config::read((file.clone(), true)).is_none());

        fs::write(&file, "color = \"never\"\n").unwrap();
        let config = Config::read((file.clone(), false)).unwrap();
        assert_eq!(config.path, file);
        assert_eq!(config.entries.len(), 1);

        fs::write(&file, "color = \n").unwrap();
        assert!(Config::read((file, false)).is_none());
    }
}
