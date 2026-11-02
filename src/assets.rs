//! Custom syntaxes and themes from the config directory, merged with the
//! bundled ones, and the cache that keeps merged syntaxes fast to load.

use std::{
    fmt::Write as _,
    fs,
    hash::{DefaultHasher, Hash, Hasher},
    io,
    path::{Path, PathBuf},
    process,
    time::UNIX_EPOCH,
};

use anyhow::{Context, Result, anyhow};
use syntect::{
    dumps,
    highlighting::{Theme, ThemeSet},
    parsing::{SyntaxDefinition, SyntaxSet},
};
use two_face::theme::LazyThemeSet;

use crate::{
    config::Dirs,
    diag::{describe, note, warn},
};

/// The theme used when none is chosen, or when the one chosen cannot be used.
pub const DEFAULT_THEME: &str = "base16-ocean.dark";

const SYNTAX_EXTENSION: &str = "sublime-syntax";
const THEME_EXTENSION: &str = "tmTheme";
const CACHE_FILE: &str = "syntaxes.bin";

/// The syntaxes of two-face with the fix from `build.rs`.
const BUNDLED_SYNTAXES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/syntaxes.bin"));

/// The bundled syntaxes: two-face's, with JavaScript fixed so that it no
/// longer hangs on `x /*` (see `build.rs`).
pub fn bundled_syntaxes() -> SyntaxSet {
    dumps::from_uncompressed_data(BUNDLED_SYNTAXES).expect("the bundled syntaxes are valid")
}

/// The bundled syntaxes plus the custom ones. Custom syntaxes come from the
/// cache when it matches the files, and are merged on the spot otherwise,
/// which is much slower.
pub fn syntaxes(dirs: &Dirs) -> SyntaxSet {
    let files = syntax_files(dirs);
    if files.is_empty() {
        return bundled_syntaxes();
    }
    let fingerprint = fingerprint(&files);
    if let Some(syntaxes) = cache_path(dirs).and_then(|path| read_cache(&path, &fingerprint)) {
        return syntaxes;
    }
    let (syntaxes, _) = merge(&files);
    note("custom syntaxes are not cached, run 'hl --build-cache' to speed up startup");
    syntaxes
}

/// What `build_cache` did.
pub enum CacheReport {
    /// There are no custom syntaxes in `dir`, so any old cache was removed.
    Empty { dir: PathBuf },
    /// `added` of `total` custom syntaxes were cached in `path`.
    Cached {
        added: usize,
        total: usize,
        path: PathBuf,
    },
}

/// Merges the custom syntaxes and writes them to the cache.
pub fn build_cache(dirs: &Dirs) -> Result<CacheReport> {
    let path = cache_path(dirs).ok_or_else(|| anyhow!("no cache directory (set HOME)"))?;
    let files = syntax_files(dirs);
    if files.is_empty() {
        remove_cache(&path)?;
        let dir = syntax_dir(dirs).unwrap_or_default();
        return Ok(CacheReport::Empty { dir });
    }
    let fingerprint = fingerprint(&files);
    let (syntaxes, added) = merge(&files);
    write_cache(&path, &fingerprint, syntaxes).with_context(|| path.display().to_string())?;
    Ok(CacheReport::Cached {
        added,
        total: files.len(),
        path,
    })
}

/// Removes the cache, so that custom syntaxes are merged on every run.
pub fn clear_cache(dirs: &Dirs) -> Result<()> {
    if let Some(path) = cache_path(dirs) {
        remove_cache(&path)?;
    }
    Ok(())
}

fn syntax_dir(dirs: &Dirs) -> Option<PathBuf> {
    Some(dirs.config.as_ref()?.join("syntaxes"))
}

/// The directory of the custom themes, for `Themes::load`.
pub fn theme_dir(dirs: &Dirs) -> Option<PathBuf> {
    Some(dirs.config.as_ref()?.join("themes"))
}

fn cache_path(dirs: &Dirs) -> Option<PathBuf> {
    Some(dirs.cache.as_ref()?.join(CACHE_FILE))
}

fn syntax_files(dirs: &Dirs) -> Vec<PathBuf> {
    syntax_dir(dirs).map_or_else(Vec::new, |dir| files_in(&dir, SYNTAX_EXTENSION))
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
/// times, along with the version of `hl` and the bundled syntaxes they are
/// merged with. The hash of the bundled syntaxes catches a change of them
/// within the same version.
fn fingerprint(files: &[PathBuf]) -> String {
    let mut hasher = DefaultHasher::new();
    BUNDLED_SYNTAXES.hash(&mut hasher);
    let mut fingerprint = format!("hl {} {:016x}", env!("CARGO_PKG_VERSION"), hasher.finish());
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
    let mut builder = bundled_syntaxes().into_builder();
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
/// other files. The fingerprint is checked before the syntaxes are decoded:
/// decoding a file that `hl` did not write, such as a broken one, can ask
/// for any amount of memory and abort.
fn read_cache(path: &Path, fingerprint: &str) -> Option<SyntaxSet> {
    let data = fs::read(path).ok()?;
    if !data.starts_with(&cache_header(fingerprint)) {
        return None;
    }
    let (_, syntaxes): (String, SyntaxSet) = dumps::from_uncompressed_data(&data).ok()?;
    Some(syntaxes)
}

/// How a cache made with `fingerprint` starts: `write_cache` dumps it first,
/// and bincode writes a string as its length (a little-endian `u64`)
/// followed by its bytes.
fn cache_header(fingerprint: &str) -> Vec<u8> {
    let len = u64::try_from(fingerprint.len()).expect("a length fits in 64 bits");
    let mut header = len.to_le_bytes().to_vec();
    header.extend_from_slice(fingerprint.as_bytes());
    header
}

/// Writes through a temporary file, so that a concurrent `hl` never reads
/// half a cache.
fn write_cache(path: &Path, fingerprint: &str, syntaxes: SyntaxSet) -> Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension(format!("tmp.{}", process::id()));
    let result = dumps::dump_to_uncompressed_file(&(fingerprint, syntaxes), &tmp)
        .map_err(anyhow::Error::from)
        .and_then(|()| Ok(fs::rename(&tmp, path)?));
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

fn remove_cache(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e).with_context(|| path.display().to_string()),
    }
}

/// The bundled themes and the custom ones. Custom themes are named after
/// their file and are only parsed when used, so they need no cache.
pub struct Themes {
    bundled: LazyThemeSet,
    /// `(name, path)` pairs, sorted by name.
    custom: Vec<(String, PathBuf)>,
}

/// Why `Themes::get` found no theme to use.
#[derive(Debug)]
pub enum ThemeFallback {
    /// No theme has the name.
    Unknown,
    /// The custom theme at `path` cannot be loaded.
    Broken { path: PathBuf, error: String },
}

enum Found<'a> {
    Bundled(&'a Theme),
    Custom(&'a Path),
}

impl Themes {
    /// The bundled themes and the custom ones in `theme_dir`.
    pub fn load(theme_dir: Option<&Path>) -> Self {
        let files = theme_dir.map_or_else(Vec::new, |dir| files_in(dir, THEME_EXTENSION));
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

    /// The theme called `name`, or why there is none to use. The caller
    /// falls back to `default_theme`: an unknown theme is not fatal, so a
    /// typo in `HL_THEME` or the config file never hides the file contents.
    pub fn get(&self, name: &str) -> Result<Theme, ThemeFallback> {
        match self.find(name) {
            Some(Found::Bundled(theme)) => Ok(theme.clone()),
            Some(Found::Custom(path)) => load_theme(path).map_err(|error| ThemeFallback::Broken {
                path: path.to_path_buf(),
                error,
            }),
            None => Err(ThemeFallback::Unknown),
        }
    }

    /// The theme used when the one asked for cannot be.
    pub fn default_theme(&self) -> Theme {
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

/// Warns that the custom theme at `path` is skipped because of `err`.
pub fn warn_bad_theme(path: &Path, err: &str) {
    warn(path, &format!("cannot load theme, skipping it: {err}"));
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    const HLTEST_SYNTAX: &str = "%YAML 1.2
---
name: HlTest
file_extensions: [hltest]
scope: source.hltest
contexts:
  main:
    - match: \\bkw\\b
      scope: keyword.control.hltest
";

    const RED_THEME: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>name</key><string>Red</string>
<key>settings</key><array>
<dict><key>settings</key><dict><key>foreground</key><string>#FF0000</string></dict></dict>
</array></dict></plist>
"#;

    fn tempdir() -> TempDir {
        tempfile::tempdir().unwrap()
    }

    /// Writes `content` to `name` in `dir`, creating the directories in the
    /// name, and returns its path.
    fn write(dir: &Path, name: &str, content: &str) -> PathBuf {
        let path = dir.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, content).unwrap();
        path
    }

    /// `Dirs` with the config and cache directories under `root`.
    fn dirs_in(root: &Path) -> Dirs {
        Dirs {
            config: Some(root.join("config")),
            cache: Some(root.join("cache")),
        }
    }

    #[test]
    fn lists_the_files_with_the_extension_sorted() {
        let dir = tempdir();
        let b = write(dir.path(), "b.tmTheme", "");
        let a = write(dir.path(), "a.TMTHEME", "");
        write(dir.path(), "c.sublime-syntax", "");
        write(dir.path(), "tmTheme", "");
        fs::create_dir(dir.path().join("x.tmTheme")).unwrap();
        assert_eq!(files_in(dir.path(), THEME_EXTENSION), [a, b]);
    }

    #[test]
    fn missing_directory_has_no_files() {
        let dir = tempdir();
        assert_eq!(
            files_in(&dir.path().join("missing"), THEME_EXTENSION),
            Vec::<PathBuf>::new()
        );
    }

    #[test]
    fn fingerprint_follows_the_files() {
        let dir = tempdir();
        let a = write(dir.path(), "a.sublime-syntax", "a");
        let files = vec![a.clone()];
        let first = fingerprint(&files);
        assert_eq!(first, fingerprint(&files));
        assert!(first.starts_with(&format!("hl {} ", env!("CARGO_PKG_VERSION"))));

        fs::write(&a, "longer").unwrap();
        let resized = fingerprint(&files);
        assert_ne!(first, resized);

        let b = write(dir.path(), "b.sublime-syntax", "b");
        assert_ne!(resized, fingerprint(&[a, b]));
    }

    #[test]
    fn reads_the_cache_only_for_the_same_fingerprint() {
        let dir = tempdir();
        let path = dir.path().join("sub/syntaxes.bin");
        assert!(read_cache(&path, "fp").is_none());

        let syntaxes = SyntaxSet::load_defaults_newlines();
        let count = syntaxes.syntaxes().len();
        write_cache(&path, "fp", syntaxes).unwrap();
        assert_eq!(read_cache(&path, "fp").unwrap().syntaxes().len(), count);
        assert!(read_cache(&path, "other").is_none());

        // A broken file must not be decoded, which could abort.
        for garbage in [&b"not a cache"[..], &[0xff; 64], b""] {
            fs::write(&path, garbage).unwrap();
            assert!(read_cache(&path, "fp").is_none());
        }
    }

    #[test]
    fn merges_again_when_the_cache_is_broken() {
        let root = tempdir();
        let dirs = dirs_in(root.path());
        write(
            root.path(),
            "config/syntaxes/HlTest.sublime-syntax",
            HLTEST_SYNTAX,
        );
        write(root.path(), "cache/syntaxes.bin", "not a cache");
        assert!(syntaxes(&dirs).find_syntax_by_name("HlTest").is_some());
    }

    #[test]
    fn uses_the_cache_built_for_the_files() {
        let root = tempdir();
        let dirs = dirs_in(root.path());
        write(
            root.path(),
            "config/syntaxes/HlTest.sublime-syntax",
            HLTEST_SYNTAX,
        );
        let Ok(CacheReport::Cached {
            added: 1,
            total: 1,
            path,
        }) = build_cache(&dirs)
        else {
            panic!("the syntax must be cached");
        };
        let files = syntax_files(&dirs);
        let cached = read_cache(&path, &fingerprint(&files)).unwrap();
        assert!(cached.find_syntax_by_name("HlTest").is_some());

        clear_cache(&dirs).unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn build_cache_needs_a_cache_directory() {
        let dirs = Dirs {
            config: None,
            cache: None,
        };
        let err = build_cache(&dirs).err().unwrap();
        assert_eq!(err.to_string(), "no cache directory (set HOME)");
        clear_cache(&dirs).unwrap();
    }

    /// The themes with custom ones named `names` in a temporary directory.
    fn themes_with(dir: &TempDir, names: &[&str]) -> Themes {
        for name in names {
            write(dir.path(), &format!("{name}.tmTheme"), RED_THEME);
        }
        Themes::load(Some(dir.path()))
    }

    /// The path of the custom theme that `name` finds, if it finds one.
    fn custom_path<'a>(themes: &'a Themes, name: &str) -> Option<&'a Path> {
        match themes.find(name) {
            Some(Found::Custom(path)) => Some(path),
            _ => None,
        }
    }

    #[test]
    fn finds_themes_ignoring_case_when_unambiguous() {
        let dir = tempdir();
        let themes = themes_with(&dir, &["Red"]);
        let red = dir.path().join("Red.tmTheme");
        assert_eq!(custom_path(&themes, "Red"), Some(red.as_path()));
        assert_eq!(custom_path(&themes, "red"), Some(red.as_path()));
        assert!(matches!(themes.find("nord"), Some(Found::Bundled(_))));
        assert!(themes.find("no-such-theme").is_none());
    }

    #[test]
    fn ambiguous_case_insensitive_match_is_unknown() {
        let dir = tempdir();
        let themes = themes_with(&dir, &["NORD"]);
        let custom = dir.path().join("NORD.tmTheme");
        assert_eq!(custom_path(&themes, "NORD"), Some(custom.as_path()));
        assert!(matches!(themes.find("Nord"), Some(Found::Bundled(_))));
        assert!(themes.find("nord").is_none());
        assert!(matches!(themes.get("nord"), Err(ThemeFallback::Unknown)));
    }

    #[test]
    fn custom_theme_replaces_the_bundled_one_of_the_same_name() {
        let dir = tempdir();
        let themes = themes_with(&dir, &["Nord"]);
        assert!(custom_path(&themes, "Nord").is_some());
        assert!(custom_path(&themes, "nord").is_some());
        assert_eq!(themes.names().iter().filter(|&&n| n == "Nord").count(), 1);
    }

    #[test]
    fn broken_custom_theme_is_reported() {
        let dir = tempdir();
        write(dir.path(), "Broken.tmTheme", "not a theme\n");
        let themes = Themes::load(Some(dir.path()));
        assert!(matches!(
            themes.get("Broken"),
            Err(ThemeFallback::Broken { path, .. }) if path == dir.path().join("Broken.tmTheme")
        ));
        assert!(!themes.names().contains(&"Broken"));
    }

    #[test]
    fn loads_only_bundled_themes_without_a_directory() {
        let themes = Themes::load(None);
        assert_eq!(themes.custom.len(), 0);
        assert!(themes.names().contains(&DEFAULT_THEME));
    }
}
