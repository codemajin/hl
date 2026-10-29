use std::{
    env, fs, io,
    path::{Path, PathBuf},
};

use toml::{Table, Value};

/// Default option values read from the config file.
pub struct Config {
    pub path: PathBuf,
    /// `(key, value)` pairs, with integers turned into strings.
    pub values: Vec<(String, String)>,
    /// `(key, values)` pairs for arrays of strings.
    pub lists: Vec<(String, Vec<String>)>,
}

impl Config {
    /// Reads the config file. A missing file at the default location is not
    /// an error; any other problem warns and yields no values, so a broken
    /// config never stops `hl` from printing.
    pub fn load() -> Option<Self> {
        let (path, explicit) = path()?;
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == io::ErrorKind::NotFound && !explicit => return None,
            Err(e) => {
                warn(&path, &crate::describe(&e));
                return None;
            }
        };
        let table: Table = match text.parse() {
            Ok(table) => table,
            Err(e) => {
                warn(
                    &path,
                    &format!("invalid TOML, ignoring the file: {}", e.message()),
                );
                return None;
            }
        };
        let mut values = Vec::new();
        let mut lists = Vec::new();
        for (key, value) in table {
            match value {
                Value::String(s) => values.push((key, s)),
                Value::Integer(n) => values.push((key, n.to_string())),
                Value::Array(items) if items.iter().all(Value::is_str) => {
                    let items = items
                        .into_iter()
                        .filter_map(|v| v.as_str().map(str::to_string))
                        .collect();
                    lists.push((key, items));
                }
                Value::Array(_) => warn(
                    &path,
                    &format!("'{key}' must be an array of strings, ignoring it"),
                ),
                _ => warn(
                    &path,
                    &format!("'{key}' must be a string or an integer, ignoring it"),
                ),
            }
        }
        Some(Self {
            path,
            values,
            lists,
        })
    }

    pub fn warn(&self, msg: &str) {
        warn(&self.path, msg);
    }
}

pub fn warn(path: &Path, msg: &str) {
    eprintln!("hl: {}: {msg}", path.display());
}

/// `HL_CONFIG`, else `$XDG_CONFIG_HOME/hl/config.toml`, else
/// `~/.config/hl/config.toml`. The flag tells whether the path was given
/// explicitly through `HL_CONFIG`.
fn path() -> Option<(PathBuf, bool)> {
    if let Some(path) = env::var_os("HL_CONFIG").filter(|p| !p.is_empty()) {
        return Some((path.into(), true));
    }
    Some((dir()?.join("config.toml"), false))
}

/// `$XDG_CONFIG_HOME/hl`, else `~/.config/hl`. Unlike the config file, it
/// does not follow `HL_CONFIG`.
pub fn dir() -> Option<PathBuf> {
    xdg_dir("XDG_CONFIG_HOME", ".config")
}

/// `$XDG_CACHE_HOME/hl`, else `~/.cache/hl`.
pub fn cache_dir() -> Option<PathBuf> {
    xdg_dir("XDG_CACHE_HOME", ".cache")
}

/// `hl` under the XDG base directory in `var`, else under `~/<fallback>`.
fn xdg_dir(var: &str, fallback: &str) -> Option<PathBuf> {
    // The XDG spec says relative paths are invalid and should be ignored.
    let dir = env::var_os(var)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| env::var_os("HOME").map(|home| Path::new(&home).join(fallback)))?;
    Some(dir.join("hl"))
}
