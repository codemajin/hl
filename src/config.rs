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
        let values = table
            .into_iter()
            .filter_map(|(key, value)| match value {
                Value::String(s) => Some((key, s)),
                Value::Integer(n) => Some((key, n.to_string())),
                _ => {
                    warn(
                        &path,
                        &format!("'{key}' must be a string or an integer, ignoring it"),
                    );
                    None
                }
            })
            .collect();
        Some(Self { path, values })
    }

    pub fn warn(&self, msg: &str) {
        warn(&self.path, msg);
    }
}

fn warn(path: &Path, msg: &str) {
    eprintln!("hl: {}: {msg}", path.display());
}

/// `HL_CONFIG`, else `$XDG_CONFIG_HOME/hl/config.toml`, else
/// `~/.config/hl/config.toml`. The flag tells whether the path was given
/// explicitly through `HL_CONFIG`.
fn path() -> Option<(PathBuf, bool)> {
    if let Some(path) = env::var_os("HL_CONFIG").filter(|p| !p.is_empty()) {
        return Some((path.into(), true));
    }
    // The XDG spec says relative paths are invalid and should be ignored.
    let dir = env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| env::var_os("HOME").map(|home| Path::new(&home).join(".config")))?;
    Some((dir.join("hl").join("config.toml"), false))
}
