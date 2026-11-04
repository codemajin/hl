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
    /// `$XDG_CONFIG_HOME/hl`, else `%APPDATA%\hl` on Windows, else
    /// `~/.config/hl`. Unlike the config file, it does not follow
    /// `HL_CONFIG`.
    pub config: Option<PathBuf>,
    /// `$XDG_CACHE_HOME/hl`, else `%LOCALAPPDATA%\hl` on Windows, else
    /// `~/.cache/hl`.
    pub cache: Option<PathBuf>,
}

/// One of the directories: the XDG variable, the Windows variable used
/// next, and the directory under `HOME` used last.
struct Lookup {
    xdg: &'static str,
    windows: &'static str,
    home: &'static str,
}

const CONFIG: Lookup = Lookup {
    xdg: "XDG_CONFIG_HOME",
    windows: "APPDATA",
    home: ".config",
};

const CACHE: Lookup = Lookup {
    xdg: "XDG_CACHE_HOME",
    windows: "LOCALAPPDATA",
    home: ".cache",
};

/// The variable to set for a cache directory, for the message shown when
/// there is none.
pub const CACHE_VAR: &str = if cfg!(windows) { CACHE.windows } else { "HOME" };

impl Dirs {
    pub fn from_env() -> Self {
        Self::from_getenv(|name| env::var_os(name), cfg!(windows))
    }

    /// The directories as given by the environment variables that `getenv`
    /// returns; `windows` adds the Windows variables to the lookup.
    fn from_getenv(getenv: impl Fn(&str) -> Option<OsString>, windows: bool) -> Self {
        Self {
            config: lookup(&getenv, &CONFIG, windows),
            cache: lookup(&getenv, &CACHE, windows),
        }
    }
}

/// `hl` under the XDG base directory of `dir`, else (on Windows) under the
/// Windows one, else under `HOME`. Relative paths are ignored, as the XDG
/// spec says, and so are empty ones.
fn lookup(
    getenv: &impl Fn(&str) -> Option<OsString>,
    dir: &Lookup,
    windows: bool,
) -> Option<PathBuf> {
    let absolute = |var: &str| getenv(var).map(PathBuf::from).filter(|p| p.is_absolute());
    let base = absolute(dir.xdg)
        .or_else(|| windows.then(|| absolute(dir.windows)).flatten())
        .or_else(|| getenv("HOME").map(|home| Path::new(&home).join(dir.home)))?;
    Some(base.join("hl"))
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

    /// `Dirs` for an environment with only `vars` set, outside Windows.
    fn dirs_with(vars: &[(&str, String)]) -> Dirs {
        dirs_on(vars, false)
    }

    /// `Dirs` for an environment with only `vars` set; `windows` tells
    /// whether the Windows variables are looked up.
    fn dirs_on(vars: &[(&str, String)], windows: bool) -> Dirs {
        let vars: HashMap<_, _> = vars.iter().cloned().collect();
        Dirs::from_getenv(|name| vars.get(name).map(OsString::from), windows)
    }

    /// `path`, made absolute on Windows too, where a path needs a drive.
    fn abs(path: &str) -> String {
        if cfg!(windows) {
            format!("C:{path}")
        } else {
            path.to_string()
        }
    }

    fn p(path: &str) -> PathBuf {
        PathBuf::from(abs(path))
    }

    #[test]
    fn uses_absolute_xdg_dirs() {
        for windows in [false, true] {
            let dirs = dirs_on(
                &[
                    ("XDG_CONFIG_HOME", abs("/xdg/config")),
                    ("XDG_CACHE_HOME", abs("/xdg/cache")),
                    ("APPDATA", abs("/appdata")),
                    ("LOCALAPPDATA", abs("/localappdata")),
                    ("HOME", abs("/home/u")),
                ],
                windows,
            );
            assert_eq!(dirs.config, Some(p("/xdg/config/hl")), "{windows}");
            assert_eq!(dirs.cache, Some(p("/xdg/cache/hl")), "{windows}");
        }
    }

    #[test]
    fn falls_back_to_home_without_xdg_dirs() {
        let dirs = dirs_with(&[("HOME", abs("/home/u"))]);
        assert_eq!(dirs.config, Some(p("/home/u/.config/hl")));
        assert_eq!(dirs.cache, Some(p("/home/u/.cache/hl")));
    }

    #[test]
    fn uses_appdata_on_windows_only() {
        let vars = [
            ("APPDATA", abs("/appdata")),
            ("LOCALAPPDATA", abs("/localappdata")),
            ("HOME", abs("/home/u")),
        ];
        let dirs = dirs_on(&vars, true);
        assert_eq!(dirs.config, Some(p("/appdata/hl")));
        assert_eq!(dirs.cache, Some(p("/localappdata/hl")));

        let dirs = dirs_on(&vars, false);
        assert_eq!(dirs.config, Some(p("/home/u/.config/hl")));
        assert_eq!(dirs.cache, Some(p("/home/u/.cache/hl")));
    }

    /// The XDG spec says relative paths are invalid and must be ignored; the
    /// Windows variables are treated the same.
    #[test]
    fn ignores_relative_and_empty_dirs() {
        for value in ["relative", "./relative", ""] {
            let vars = [
                ("XDG_CONFIG_HOME", value.to_string()),
                ("XDG_CACHE_HOME", value.to_string()),
                ("APPDATA", value.to_string()),
                ("LOCALAPPDATA", value.to_string()),
                ("HOME", abs("/home/u")),
            ];
            for windows in [false, true] {
                let dirs = dirs_on(&vars, windows);
                assert_eq!(dirs.config, Some(p("/home/u/.config/hl")), "{value:?}");
                assert_eq!(dirs.cache, Some(p("/home/u/.cache/hl")), "{value:?}");
            }
        }
    }

    #[test]
    fn has_no_dirs_without_home_or_xdg() {
        let dirs = dirs_on(&[], true);
        assert_eq!(dirs.config, None);
        assert_eq!(dirs.cache, None);

        let rel = || "rel".to_string();
        let dirs = dirs_on(
            &[
                ("XDG_CONFIG_HOME", rel()),
                ("XDG_CACHE_HOME", rel()),
                ("APPDATA", rel()),
                ("LOCALAPPDATA", rel()),
            ],
            true,
        );
        assert_eq!(dirs.config, None);
        assert_eq!(dirs.cache, None);
    }

    /// Outside Windows the Windows variables mean nothing, even when HOME is
    /// missing.
    #[test]
    fn ignores_appdata_outside_windows() {
        let dirs = dirs_with(&[("APPDATA", abs("/appdata")), ("LOCALAPPDATA", abs("/l"))]);
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
