//! Custom syntaxes and themes from the config directory, merged with the
//! bundled ones, and the cache that keeps merged syntaxes fast to load.

use std::{
    fmt::Write as _,
    fs, io,
    path::{Path, PathBuf},
    process::{self, ExitCode},
    time::UNIX_EPOCH,
};

use anyhow::{Result, anyhow};
use syntect::{
    dumps,
    highlighting::{Theme, ThemeSet},
    parsing::{SyntaxDefinition, SyntaxSet},
};
use two_face::theme::LazyThemeSet;

use crate::{
    config,
    diag::{describe, warn},
    highlight::DEFAULT_THEME,
};

const SYNTAX_EXTENSION: &str = "sublime-syntax";
const THEME_EXTENSION: &str = "tmTheme";
const CACHE_FILE: &str = "syntaxes.bin";

/// The bundled syntaxes plus the custom ones. Custom syntaxes come from the
/// cache when it matches the files, and are merged on the spot otherwise,
/// which is much slower.
pub fn syntaxes() -> SyntaxSet {
    let files = syntax_files();
    if files.is_empty() {
        return two_face::syntax::extra_newlines();
    }
    let fingerprint = fingerprint(&files);
    if let Some(syntaxes) = cache_path().and_then(|path| read_cache(&path, &fingerprint)) {
        return syntaxes;
    }
    let (syntaxes, _) = merge(&files);
    eprintln!("hl: custom syntaxes are not cached, run 'hl --build-cache' to speed up startup");
    syntaxes
}

/// Merges the custom syntaxes and writes them to the cache.
pub fn build_cache() -> Result<ExitCode> {
    let path = cache_path().ok_or_else(|| anyhow!("no cache directory (set HOME)"))?;
    let files = syntax_files();
    if files.is_empty() {
        remove_cache(&path)?;
        let dir = syntax_dir().unwrap_or_default();
        println!("no custom syntaxes in {}, nothing to cache", dir.display());
        return Ok(ExitCode::SUCCESS);
    }
    let fingerprint = fingerprint(&files);
    let (syntaxes, added) = merge(&files);
    write_cache(&path, &fingerprint, syntaxes).map_err(|e| anyhow!("{}: {e}", path.display()))?;
    println!(
        "cached {added} of {} custom syntaxes in {}",
        files.len(),
        path.display()
    );
    Ok(ExitCode::SUCCESS)
}

/// Removes the cache, so that custom syntaxes are merged on every run.
pub fn clear_cache() -> Result<ExitCode> {
    if let Some(path) = cache_path() {
        remove_cache(&path)?;
    }
    Ok(ExitCode::SUCCESS)
}

fn syntax_dir() -> Option<PathBuf> {
    Some(config::dir()?.join("syntaxes"))
}

fn theme_dir() -> Option<PathBuf> {
    Some(config::dir()?.join("themes"))
}

fn cache_path() -> Option<PathBuf> {
    Some(config::cache_dir()?.join(CACHE_FILE))
}

fn syntax_files() -> Vec<PathBuf> {
    syntax_dir().map_or_else(Vec::new, |dir| files_in(&dir, SYNTAX_EXTENSION))
}

/// The files in `dir` with the given extension, sorted. A missing directory
/// has no files; any other problem reading it warns.
fn files_in(dir: &Path, extension: &str) -> Vec<PathBuf> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Vec::new(),
        Err(e) => {
            warn(dir, &describe(&e));
            return Vec::new();
        }
    };
    let mut files: Vec<_> = entries
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|p| {
            p.extension()
                .is_some_and(|e| e.eq_ignore_ascii_case(extension))
        })
        .filter(|p| p.is_file())
        .collect();
    files.sort_unstable();
    files
}

/// Identifies the custom syntaxes by their paths, sizes and modification
/// times, along with the version of `hl` whose bundled syntaxes they are
/// merged with.
fn fingerprint(files: &[PathBuf]) -> String {
    let mut fingerprint = format!("hl {}", env!("CARGO_PKG_VERSION"));
    for file in files {
        let meta = fs::metadata(file).ok();
        let len = meta.as_ref().map_or(0, fs::Metadata::len);
        let modified = meta
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_nanos());
        write!(fingerprint, "\n{}\t{len}\t{modified}", file.display())
            .expect("writing to a String cannot fail");
    }
    fingerprint
}

/// The bundled syntaxes with the custom ones added, and how many were
/// added. A custom syntax that cannot be loaded warns and is skipped.
fn merge(files: &[PathBuf]) -> (SyntaxSet, usize) {
    let mut builder = two_face::syntax::extra_newlines().into_builder();
    let mut added = 0;
    for file in files {
        match load_syntax(file) {
            Ok(syntax) => {
                builder.add(syntax);
                added += 1;
            }
            Err(e) => warn(file, &format!("cannot load syntax, skipping it: {e}")),
        }
    }
    (builder.build(), added)
}

fn load_syntax(file: &Path) -> Result<SyntaxDefinition, String> {
    let text = fs::read_to_string(file).map_err(|e| describe(&e))?;
    let name = file.file_stem().and_then(|s| s.to_str());
    SyntaxDefinition::load_from_str(&text, true, name).map_err(|e| e.to_string())
}

/// Reads the cache, or nothing if it is missing, unreadable or made from
/// other files.
fn read_cache(path: &Path, fingerprint: &str) -> Option<SyntaxSet> {
    let (cached, syntaxes): (String, SyntaxSet) = dumps::from_uncompressed_dump_file(path).ok()?;
    (cached == fingerprint).then_some(syntaxes)
}

/// Writes through a temporary file, so that a concurrent `hl` never reads
/// half a cache.
fn write_cache(path: &Path, fingerprint: &str, syntaxes: SyntaxSet) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| describe(&e))?;
    }
    let tmp = path.with_extension(format!("tmp.{}", process::id()));
    let result = dumps::dump_to_uncompressed_file(&(fingerprint, syntaxes), &tmp)
        .map_err(|e| e.to_string())
        .and_then(|()| fs::rename(&tmp, path).map_err(|e| describe(&e)));
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

fn remove_cache(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(anyhow!("{}: {}", path.display(), describe(&e))),
    }
}

/// The bundled themes and the custom ones. Custom themes are named after
/// their file and are only parsed when used, so they need no cache.
pub struct Themes {
    bundled: LazyThemeSet,
    /// `(name, path)` pairs, sorted by name.
    custom: Vec<(String, PathBuf)>,
}

enum Found<'a> {
    Bundled(&'a Theme),
    Custom(&'a Path),
}

impl Themes {
    pub fn load() -> Self {
        let files = theme_dir().map_or_else(Vec::new, |dir| files_in(&dir, THEME_EXTENSION));
        let custom = files
            .into_iter()
            .filter_map(|path| {
                let name = path.file_stem()?.to_str()?.to_string();
                Some((name, path))
            })
            .collect();
        Self {
            bundled: LazyThemeSet::from(two_face::theme::extra()),
            custom,
        }
    }

    /// The names of all themes, as listed by `--list-themes`. Custom themes
    /// that cannot be loaded warn and are left out.
    pub fn names(&self) -> Vec<&str> {
        let mut names: Vec<_> = self
            .all_names()
            .filter(|name| match self.exact(name) {
                Some(Found::Custom(path)) => load_theme(path)
                    .inspect_err(|e| warn_bad_theme(path, e))
                    .is_ok(),
                _ => true,
            })
            .collect();
        names.sort_unstable();
        names
    }

    /// The theme called `name`, or the default theme with a warning. An
    /// unknown theme is not fatal, so a typo in `HL_THEME` or the config
    /// file never hides the file contents.
    pub fn get_or_default(&self, name: &str) -> Theme {
        match self.find(name) {
            Some(Found::Bundled(theme)) => return theme.clone(),
            Some(Found::Custom(path)) => {
                match load_theme(path) {
                    Ok(theme) => return theme,
                    Err(e) => warn_bad_theme(path, &e),
                }
                eprintln!("hl: falling back to {DEFAULT_THEME}");
            }
            None => eprintln!(
                "hl: unknown theme '{name}', falling back to {DEFAULT_THEME} (see --list-themes)"
            ),
        }
        self.bundled
            .get(DEFAULT_THEME)
            .expect("default theme is bundled")
            .clone()
    }

    /// Custom themes first, then the bundled ones they do not replace.
    fn all_names(&self) -> impl Iterator<Item = &str> {
        let custom = self.custom.iter().map(|(name, _)| name.as_str());
        let bundled = self
            .bundled
            .theme_names()
            .filter(|name| !self.custom.iter().any(|(c, _)| c == name));
        custom.chain(bundled)
    }

    fn exact(&self, name: &str) -> Option<Found<'_>> {
        self.custom
            .iter()
            .find(|(c, _)| c == name)
            .map(|(_, path)| Found::Custom(path))
            .or_else(|| self.bundled.get(name).map(Found::Bundled))
    }

    /// Looks a theme up by exact name, then case-insensitively as long as
    /// the match is unambiguous (so `nord` finds `Nord`).
    fn find(&self, name: &str) -> Option<Found<'_>> {
        if let Some(found) = self.exact(name) {
            return Some(found);
        }
        let mut matches = self.all_names().filter(|n| n.eq_ignore_ascii_case(name));
        match (matches.next(), matches.next()) {
            (Some(found), None) => self.exact(found),
            _ => None,
        }
    }
}

fn load_theme(path: &Path) -> Result<Theme, String> {
    ThemeSet::get_theme(path).map_err(|e| match e {
        syntect::LoadingError::Io(e) => describe(&e),
        e => e.to_string(),
    })
}

fn warn_bad_theme(path: &Path, err: &str) {
    warn(path, &format!("cannot load theme, skipping it: {err}"));
}
