mod worker;

use std::{
    fmt,
    io::{self, BufRead, Read, Write},
    ops::Deref,
    path::{self, Path},
    ptr,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use globset::{GlobBuilder, GlobMatcher};
use syntect::{
    highlighting::{Color, Theme},
    parsing::{SyntaxReference, SyntaxSet},
};

use self::worker::{Flow, PendingLine, Pipeline, Worker};
use crate::{
    color::{Depth, background_sgr, blend},
    lines::{LineState, Visibility, write_plain},
};

/// A language that inputs can be highlighted as: one syntax of a syntax
/// set, which it keeps alive. The highlighting thread holds one, because it
/// may outlive the input it was started for (see `STALL_TIMEOUT`).
#[derive(Clone)]
pub struct Syntax {
    set: Arc<SyntaxSet>,
    /// Where the syntax is in `set.syntaxes()`.
    index: usize,
}

impl Syntax {
    /// `syntax`, which must be one of the syntaxes of `set`.
    fn new(set: &Arc<SyntaxSet>, syntax: &SyntaxReference) -> Self {
        let index = set
            .syntaxes()
            .iter()
            .position(|s| ptr::eq(s, syntax))
            .expect("the syntax is one of the set");
        Self {
            set: Arc::clone(set),
            index,
        }
    }

    /// The syntax set the syntax belongs to, which syntect needs to
    /// highlight with it.
    fn set(&self) -> &SyntaxSet {
        &self.set
    }
}

impl fmt::Debug for Syntax {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Syntax")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

impl Deref for Syntax {
    type Target = SyntaxReference;

    fn deref(&self) -> &SyntaxReference {
        &self.set.syntaxes()[self.index]
    }
}

/// How much of the foreground tints the background for highlighted lines
/// when the theme has no `lineHighlight` color.
const FALLBACK_HIGHLIGHT_ALPHA: u8 = 0x33;

/// How long highlighting may go without finishing a line before it is given
/// up. syntect can loop forever on some syntaxes and inputs, and cannot be
/// interrupted, so it runs on a thread of its own that is left behind. No
/// input is highlighted after that (see `Highlighter::has_stalled`), so a
/// run waits this long and leaves a thread behind once at most.
const STALL_TIMEOUT: Duration = Duration::from_secs(3);

/// Inputs beyond these limits are printed without highlighting, because
/// syntect gets very slow on huge files and very long lines.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Total bytes of one input.
    pub max_size: Option<u64>,
    /// Bytes of one line, excluding the newline.
    pub max_line: Option<u64>,
}

impl Limits {
    pub fn exceeds_size(self, len: u64) -> bool {
        self.max_size.is_some_and(|max| len > max)
    }
}

/// The glob pattern of a user mapping, which tells the inputs it applies to.
/// Its language is resolved by the caller, and paired with it in
/// `Highlighter::new`.
#[derive(Debug, Clone)]
pub struct SyntaxMapping {
    glob: GlobMatcher,
    /// Whether the pattern contains a path separator (`/`, or also `\` on
    /// Windows) and so applies to the whole path rather than the file name.
    by_path: bool,
}

impl SyntaxMapping {
    /// Compiles the glob `pattern`, or returns why it is invalid. `*` does
    /// not match a separator; `**` does.
    pub fn new(pattern: &str) -> Result<Self, String> {
        let glob = GlobBuilder::new(pattern)
            .literal_separator(true)
            .build()
            .map_err(|e| e.kind().to_string())?
            .compile_matcher();
        Ok(Self {
            glob,
            by_path: pattern.contains(path::is_separator),
        })
    }

    /// Whether the input at `path` matches: its file name, or `absolute`,
    /// the absolute form of `path`, for a pattern with a separator. Without
    /// `absolute` (it could not be found) only a file name can match.
    fn matches(&self, path: &Path, absolute: Option<&Path>) -> bool {
        if self.by_path {
            absolute.is_some_and(|p| self.glob.is_match(p))
        } else {
            path.file_name().is_some_and(|n| self.glob.is_match(n))
        }
    }
}

/// Why highlighting an input was given up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GiveUp {
    /// An error in the syntax, or the highlighting thread ended.
    Failed(String),
    /// No line finished for this long. The thread is stuck and left behind,
    /// so no later input is highlighted either.
    Stalled(Duration),
}

impl fmt::Display for GiveUp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Failed(reason) => f.write_str(reason),
            Self::Stalled(timeout) => write!(f, "a line took longer than {timeout:?}"),
        }
    }
}

/// Highlights inputs.
pub struct Highlighter {
    syntaxes: Arc<SyntaxSet>,
    theme: Arc<Theme>,
    depth: Depth,
    /// User mappings with their languages resolved; the last match wins.
    mappings: Vec<(SyntaxMapping, Syntax)>,
    stall_timeout: Duration,
    /// Whether highlighting got stuck on an input.
    stalled: AtomicBool,
}

/// Leaves out the syntax set and the theme's styles, which are large.
impl fmt::Debug for Highlighter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Highlighter")
            .field("theme", &self.theme.name)
            .field("depth", &self.depth)
            .field("mappings", &self.mappings)
            .field("stall_timeout", &self.stall_timeout)
            .field("stalled", &self.stalled)
            .finish_non_exhaustive()
    }
}

impl Highlighter {
    pub fn new(
        syntaxes: Arc<SyntaxSet>,
        theme: Theme,
        depth: Depth,
        mappings: Vec<(SyntaxMapping, Syntax)>,
    ) -> Self {
        Self {
            syntaxes,
            theme: Arc::new(theme),
            depth,
            mappings,
            stall_timeout: STALL_TIMEOUT,
            stalled: AtomicBool::new(false),
        }
    }

    /// Gives up highlighting a line after `timeout` instead of
    /// `STALL_TIMEOUT`.
    #[cfg(test)]
    pub fn with_stall_timeout(mut self, timeout: Duration) -> Self {
        self.stall_timeout = timeout;
        self
    }

    /// Whether highlighting got stuck on an input, which leaves a thread
    /// spinning until the process exits. The caller prints the later inputs
    /// without highlighting, so that each of them does not wait
    /// `STALL_TIMEOUT` and leave one more thread behind.
    pub fn has_stalled(&self) -> bool {
        self.stalled.load(Ordering::Relaxed)
    }

    /// The escape sequence for the background of `--highlight-line`: the
    /// theme's `lineHighlight` color, or the background tinted with the
    /// foreground when the theme has none.
    pub fn line_highlight_sgr(&self) -> String {
        let settings = &self.theme.settings;
        let background = settings.background.unwrap_or(Color::BLACK);
        let color = settings.line_highlight.unwrap_or_else(|| Color {
            a: FALLBACK_HIGHLIGHT_ALPHA,
            ..settings.foreground.unwrap_or(Color::WHITE)
        });
        background_sgr(blend(color, background), self.depth)
    }

    /// Detects the syntax from the user mappings, then the file name
    /// (extension, then full name) and finally the first line (shebang,
    /// modelines). Falls back to plain text.
    pub fn detect(&self, path: Option<&Path>, first_line: &str) -> Syntax {
        let mapped = path.and_then(|p| {
            let absolute = path::absolute(p).ok();
            let absolute = absolute.as_deref();
            self.mappings
                .iter()
                .rev()
                .find(|(m, _)| m.matches(p, absolute))
        });
        if let Some((_, syntax)) = mapped {
            return syntax.clone();
        }
        let syntaxes = &*self.syntaxes;
        let by_path = path.and_then(|p| {
            let ext = p.extension().and_then(|e| e.to_str());
            let name = p.file_name().and_then(|n| n.to_str());
            ext.and_then(|e| syntaxes.find_syntax_by_extension(e))
                .or_else(|| name.and_then(|n| syntaxes.find_syntax_by_extension(n)))
        });
        let found = by_path
            .or_else(|| syntaxes.find_syntax_by_first_line(first_line))
            .unwrap_or_else(|| syntaxes.find_syntax_plain_text());
        Syntax::new(&self.syntaxes, found)
    }

    /// Writes `reader` highlighted with `syntax`, one line at a time. Lines
    /// that are not printed (outside the line range, squeezed) are still
    /// highlighted, so that the printed ones get the right colors. Nothing
    /// is read past the end of the line range.
    ///
    /// The lines are read and printed here, and highlighted on another
    /// thread a few batches ahead (see `worker::Pipeline`).
    ///
    /// Once the input exceeds `limits.max_size` or a line exceeds
    /// `limits.max_line`, the rest is copied as is: syntect's state carries
    /// over between lines, so highlighting cannot resume after a skipped line.
    /// A line that is not valid UTF-8 switches to plain output the same way,
    /// and so does an error in the syntax or a line that takes longer than
    /// `STALL_TIMEOUT` to highlight, after passing the reason to `on_give_up`.
    /// The latter also makes `has_stalled` true.
    pub fn write(
        &self,
        reader: &mut impl BufRead,
        on_give_up: &mut dyn FnMut(&GiveUp),
        out: &mut impl Write,
        syntax: Syntax,
        lines: &mut LineState,
        limits: Limits,
    ) -> io::Result<()> {
        let theme = Arc::clone(&self.theme);
        let worker = Worker::spawn(syntax, theme, self.depth, self.stall_timeout)?;
        let mut pipeline = Pipeline::new(worker, on_give_up, &self.stalled);
        let max_line = limits.max_line.unwrap_or(u64::MAX);
        let range_end = lines.range_end();
        let mut total: u64 = 0;
        // Whole lines read from this input. The lines wait in the pipeline
        // before they are printed, so `lines.is_done()` comes too late to
        // keep the lines past the range from being read.
        let mut read_lines: u64 = 0;
        loop {
            if lines.is_done() {
                return Ok(());
            }
            let mut buf = Vec::new();
            let n = if range_end.is_some_and(|end| read_lines >= end) {
                0
            } else {
                // Reading at most one byte past the limit keeps a huge line out of memory.
                reader
                    .by_ref()
                    .take(max_line.saturating_add(1))
                    .read_until(b'\n', &mut buf)?
            };
            if n == 0 {
                return match pipeline.finish(out, lines)? {
                    Flow::Continue | Flow::Done => Ok(()),
                    Flow::Plain => plain_rest(reader, out, lines),
                };
            }
            let n = u64::try_from(n).expect("a line read into memory fits in u64");
            total += n;
            read_lines += u64::from(buf.ends_with(b"\n"));
            let too_long = !buf.ends_with(b"\n") && n > max_line;
            // Input that is not UTF-8 (Shift_JIS, ...) is copied unchanged
            // rather than replacing its bytes with U+FFFD.
            let text = std::str::from_utf8(&buf).ok();
            let Some(line) = text.filter(|_| !too_long && !limits.exceeds_size(total)) else {
                // The pending lines are printed, highlighted or, if
                // highlighting was given up on the way, plain. Either way
                // `buf` comes next.
                if pipeline.finish(out, lines)? == Flow::Done {
                    return Ok(());
                }
                if lines.begin_line(&buf, out)? {
                    lines.write_raw(&buf, out)?;
                }
                return plain_rest(reader, out, lines);
            };
            // Keep the BOM in the output, but out of syntect's way.
            let bom = total == n && line.starts_with('\u{FEFF}');
            let text = if bom {
                &line['\u{FEFF}'.len_utf8()..]
            } else {
                line
            };
            // Highlight including the newline so syntect's state stays correct.
            let text = text.to_owned();
            match pipeline.push(text, PendingLine { raw: buf, bom }, out, lines)? {
                Flow::Continue => {}
                Flow::Done => return Ok(()),
                Flow::Plain => return plain_rest(reader, out, lines),
            }
        }
    }
}

/// Copies the rest of the input as is, once highlighting is given up. There
/// is no `-v`, `-E` or `-T` to apply: `Printer` does not highlight with them.
fn plain_rest(
    reader: &mut impl BufRead,
    out: &mut impl Write,
    lines: &mut LineState,
) -> io::Result<()> {
    write_plain(reader, out, lines, Visibility::default())
}

/// Resolves an explicit `--language` value by name or extension.
pub fn find_language(syntaxes: &Arc<SyntaxSet>, lang: &str) -> Option<Syntax> {
    syntaxes
        .find_syntax_by_token(lang)
        .or_else(|| syntaxes.find_syntax_by_extension(lang))
        .map(|syntax| Syntax::new(syntaxes, syntax))
}

/// The names and file extensions of the languages listed by
/// `--list-languages`, sorted by name.
pub fn languages(syntaxes: &SyntaxSet) -> Vec<(&str, &[String])> {
    let mut languages: Vec<_> = syntaxes
        .syntaxes()
        .iter()
        .filter(|s| !s.hidden)
        .map(|s| (s.name.as_str(), s.file_extensions.as_slice()))
        .collect();
    languages.sort_unstable_by_key(|(name, _)| name.to_lowercase());
    languages
}

#[cfg(test)]
mod tests {
    use std::{fmt::Write as _, io::Cursor, sync::mpsc, thread};

    use syntect::parsing::{SyntaxDefinition, SyntaxSetBuilder};

    use super::{
        worker::{BATCH_BYTES, BATCH_LINES, MAX_IN_FLIGHT},
        *,
    };
    use crate::{
        encoding::UTF8_BOM,
        lines::{LineHighlight, LineRange, Numbering},
        test_support::{LOOPING_SYNTAX, bundled, custom_syntax, has_escape, strip_ansi},
    };

    fn highlighter(syntaxes: &Arc<SyntaxSet>, mappings: &[&str]) -> Highlighter {
        let mappings = mappings
            .iter()
            .map(|m| {
                let (pattern, language) = m.rsplit_once(':').unwrap();
                let mapping = SyntaxMapping::new(pattern).unwrap();
                (mapping, find_language(syntaxes, language).unwrap())
            })
            .collect();
        Highlighter::new(
            Arc::clone(syntaxes),
            Theme::default(),
            Depth::TrueColor,
            mappings,
        )
    }

    /// The plain text syntax of `syntaxes`.
    fn plain_text(syntaxes: &Arc<SyntaxSet>) -> Syntax {
        Syntax::new(syntaxes, syntaxes.find_syntax_plain_text())
    }

    /// The name of the syntax detected for `path` and `first_line`.
    fn detect(mappings: &[&str], path: Option<&str>, first_line: &str) -> String {
        let syntaxes = bundled();
        let hl = highlighter(&syntaxes, mappings);
        hl.detect(path.map(Path::new), first_line).name.clone()
    }

    #[test]
    fn detects_by_extension() {
        assert_eq!(detect(&[], Some("src/main.rs"), ""), "Rust");
        assert_eq!(detect(&[], Some("MAIN.RS"), ""), "Rust");
    }

    #[test]
    fn detects_by_file_name() {
        assert_eq!(detect(&[], Some("Makefile"), ""), "Makefile");
        assert_eq!(
            detect(&[], Some(".bashrc"), ""),
            "Bourne Again Shell (bash)"
        );
    }

    #[test]
    fn detects_by_shebang() {
        let python = "#!/usr/bin/env python3";
        assert_eq!(detect(&[], Some("script"), python), "Python");
        assert_eq!(detect(&[], None, "#!/bin/sh"), "Bourne Again Shell (bash)");
    }

    #[test]
    fn extension_beats_shebang() {
        let python = "#!/usr/bin/env python3";
        assert_eq!(detect(&[], Some("main.rs"), python), "Rust");
    }

    #[test]
    fn falls_back_to_plain_text() {
        assert_eq!(detect(&[], Some("notes.unknownext"), "hello"), "Plain Text");
        assert_eq!(detect(&[], None, ""), "Plain Text");
    }

    #[cfg(unix)]
    #[test]
    fn detects_by_shebang_when_the_name_is_not_utf8() {
        use std::{ffi::OsStr, os::unix::ffi::OsStrExt};

        let syntaxes = bundled();
        let hl = highlighter(&syntaxes, &[]);
        let path = Path::new(OsStr::from_bytes(b"a.\xff"));
        let syntax = hl.detect(Some(path), "#!/bin/sh");
        assert_eq!(syntax.name, "Bourne Again Shell (bash)");
    }

    #[test]
    fn mapping_beats_extension() {
        assert_eq!(detect(&["*.rs:Python"], Some("main.rs"), ""), "Python");
    }

    #[test]
    fn maps_an_exact_file_name() {
        assert_eq!(
            detect(&["Settings:Ruby"], Some("conf/Settings"), ""),
            "Ruby"
        );
        assert_ne!(
            detect(&["Settings:Ruby"], Some("conf/MySettings"), ""),
            "Ruby"
        );
    }

    #[test]
    fn later_mapping_wins() {
        let mappings = ["*.rs:Python", "main.*:Ruby"];
        assert_eq!(detect(&mappings, Some("main.rs"), ""), "Ruby");
    }

    #[test]
    fn unmatched_mapping_falls_through() {
        assert_eq!(detect(&["*.py:Ruby"], Some("main.rs"), ""), "Rust");
    }

    #[test]
    fn mappings_do_not_apply_to_stdin() {
        assert_eq!(
            detect(&["*:Ruby"], None, "#!/usr/bin/env python3"),
            "Python"
        );
    }

    #[test]
    fn pattern_with_a_slash_matches_the_absolute_path() {
        let path = Some("conf/app.cfg");
        assert_eq!(detect(&["**/conf/*.cfg:Ruby"], path, ""), "Ruby");
        assert_ne!(detect(&["conf/*.cfg:Ruby"], path, ""), "Ruby");
    }

    /// On Windows a backslash is a separator in the pattern as in the path,
    /// and the drive colon stays in the pattern.
    #[cfg(windows)]
    #[test]
    fn windows_patterns_match_the_absolute_path() {
        let path = Some(r"conf\app.cfg");
        assert_eq!(detect(&[r"**\conf\*.cfg:Ruby"], path, ""), "Ruby");
        assert_eq!(detect(&["**/conf/*.cfg:Ruby"], path, ""), "Ruby");
        let absolute = std::path::absolute("conf/app.cfg").unwrap();
        let mapping = format!("{}:Ruby", absolute.display());
        assert_eq!(detect(&[mapping.as_str()], path, ""), "Ruby");
    }

    #[test]
    fn finds_a_language_by_name_or_extension() {
        let syntaxes = bundled();
        let name = |lang| find_language(&syntaxes, lang).map(|s| s.name.clone());
        assert_eq!(name("Rust"), Some("Rust".to_string()));
        assert_eq!(name("rust"), Some("Rust".to_string()));
        assert_eq!(name("py"), Some("Python".to_string()));
        assert_eq!(name("no-such-language"), None);
        assert_eq!(name(""), None);
    }

    /// `matches` reads nothing but the paths it is given: the current
    /// directory does not matter.
    #[test]
    fn matches_only_the_given_paths() {
        let path = Path::new("conf/app.cfg");
        let absolute = Some(Path::new("proj/conf/app.cfg"));
        let by_path = SyntaxMapping::new("proj/conf/*.cfg").unwrap();
        assert!(by_path.matches(path, absolute));
        assert!(!by_path.matches(path, None));
        assert!(!by_path.matches(path, Some(Path::new("other/conf/app.cfg"))));
        let by_name = SyntaxMapping::new("*.cfg").unwrap();
        assert!(by_name.matches(path, None));
        assert!(by_name.matches(path, absolute));
        assert!(!by_name.matches(Path::new("conf/app.ini"), None));
    }

    #[test]
    fn rejects_an_invalid_glob() {
        assert!(SyntaxMapping::new("[").is_err());
    }

    #[test]
    fn size_limit_is_exclusive() {
        let limits = |max_size| Limits {
            max_size,
            max_line: None,
        };
        assert!(!limits(None).exceeds_size(u64::MAX));
        assert!(!limits(Some(10)).exceeds_size(10));
        assert!(limits(Some(10)).exceeds_size(11));
    }

    const NO_LIMITS: Limits = Limits {
        max_size: None,
        max_line: None,
    };

    /// Highlights `input` as plain text.
    fn write(input: &[u8], lines: &mut LineState, limits: Limits, depth: Depth) -> Vec<u8> {
        let syntaxes = bundled();
        let mut hl = highlighter(&syntaxes, &[]);
        hl.depth = depth;
        let mut out = Vec::new();
        let syntax = plain_text(&syntaxes);
        hl.write(
            &mut Cursor::new(input),
            &mut |reason| panic!("gave up: {reason}"),
            &mut out,
            syntax,
            lines,
            limits,
        )
        .unwrap();
        out
    }

    fn write_simple(input: &[u8], numbering: Numbering, limits: Limits) -> Vec<u8> {
        let mut lines = LineState::new(numbering, false, None);
        write(input, &mut lines, limits, Depth::TrueColor)
    }

    #[test]
    fn resets_colors_before_each_newline() {
        let out = write_simple(b"a\nb\n", Numbering::All, NO_LIMITS);
        assert_eq!(strip_ansi(&out), b"     1\ta\n     2\tb\n");
        let out = String::from_utf8(out).unwrap();
        assert_eq!(out.matches("\x1b[0m\n").count(), 2, "{out:?}");
    }

    #[test]
    fn keeps_a_missing_trailing_newline() {
        let out = write_simple(b"a\nb", Numbering::None, NO_LIMITS);
        assert!(out.ends_with(b"b\x1b[0m"), "{out:?}");
    }

    #[test]
    fn writes_nothing_for_empty_input() {
        assert_eq!(write_simple(b"", Numbering::All, NO_LIMITS), b"");
    }

    #[test]
    fn keeps_the_bom_in_front_of_the_colors() {
        let out = write_simple("\u{FEFF}a\n".as_bytes(), Numbering::None, NO_LIMITS);
        assert!(out.starts_with(UTF8_BOM), "{out:?}");
        assert_eq!(strip_ansi(&out), "\u{FEFF}a\n".as_bytes());
    }

    /// Only a BOM at the start of the input is kept out of syntect: one at
    /// the start of a later line is text, highlighted with the rest.
    #[test]
    fn highlights_a_bom_at_the_start_of_a_later_line() {
        let input = "a\n\u{FEFF}b\n".as_bytes();
        let out = write_simple(input, Numbering::None, NO_LIMITS);
        assert_eq!(strip_ansi(&out), input);
        let second = &out[out.iter().position(|&b| b == b'\n').unwrap() + 1..];
        assert!(second.starts_with(b"\x1b["), "{out:?}");
        let bom = second
            .windows(UTF8_BOM.len())
            .position(|w| w == UTF8_BOM)
            .unwrap();
        assert!(has_escape(&second[..bom]), "{out:?}");
    }

    #[test]
    fn prints_only_the_line_range() {
        let range = LineRange {
            start: 2,
            end: Some(2),
        };
        let mut lines = LineState::new(Numbering::All, false, Some(range));
        let out = write(b"a\nb\nc\n", &mut lines, NO_LIMITS, Depth::TrueColor);
        assert_eq!(strip_ansi(&out), b"     2\tb\n");
    }

    #[test]
    fn stops_reading_after_the_line_range() {
        let range = LineRange {
            start: 1,
            end: Some(2),
        };
        let mut lines = LineState::new(Numbering::None, false, Some(range));
        let syntaxes = bundled();
        let hl = highlighter(&syntaxes, &[]);
        let mut reader = Cursor::new(&b"a\nb\nc\nd\n"[..]);
        let mut out = Vec::new();
        hl.write(
            &mut reader,
            &mut |reason| panic!("gave up: {reason}"),
            &mut out,
            plain_text(&syntaxes),
            &mut lines,
            NO_LIMITS,
        )
        .unwrap();
        assert_eq!(strip_ansi(&out), b"a\nb\n");
        assert_eq!(reader.position(), 4, "read past the range");
    }

    fn max_line(max: u64) -> Limits {
        Limits {
            max_size: None,
            max_line: Some(max),
        }
    }

    #[test]
    fn highlights_a_line_at_the_length_limit() {
        let out = write_simple(b"abcd\nxy\n", Numbering::None, max_line(4));
        assert!(out.starts_with(b"\x1b["), "{out:?}");
        assert!(out.ends_with(b"xy\x1b[0m\n"), "{out:?}");
    }

    /// A last line without a newline is held to the limit the same way.
    #[test]
    fn applies_the_length_limit_to_a_last_line_without_newline() {
        let out = write_simple(b"abcd", Numbering::None, max_line(4));
        assert!(has_escape(&out), "{out:?}");
        assert_eq!(strip_ansi(&out), b"abcd");
        let out = write_simple(b"abcde", Numbering::None, max_line(4));
        assert_eq!(out, b"abcde");
    }

    #[test]
    fn copies_the_rest_from_a_line_over_the_limit() {
        let out = write_simple(b"abcde\nxy\n", Numbering::None, max_line(4));
        assert_eq!(out, b"abcde\nxy\n");
    }

    #[test]
    fn keeps_numbering_after_switching_to_plain() {
        let out = write_simple(b"abcde\nxy\n", Numbering::All, max_line(4));
        assert_eq!(out, b"     1\tabcde\n     2\txy\n");
    }

    #[test]
    fn copies_the_rest_once_the_size_limit_is_exceeded() {
        let limits = Limits {
            max_size: Some(2),
            max_line: None,
        };
        let out = write_simple(b"a\nb\n", Numbering::None, limits);
        let (first, rest) = out.split_at(out.len() - 2);
        assert!(has_escape(first), "{out:?}");
        assert_eq!(rest, b"b\n");
    }

    #[test]
    fn copies_the_rest_from_a_line_that_is_not_utf8() {
        let out = write_simple(b"a\n\xff\nc\n", Numbering::None, NO_LIMITS);
        let (first, rest) = out.split_at(out.len() - 4);
        assert!(has_escape(first), "{out:?}");
        assert_eq!(rest, b"\xff\nc\n");
    }

    /// Highlights `input` with line 2 shown with the background.
    fn write_highlighted(input: &[u8], limits: Limits, depth: Depth) -> String {
        let syntaxes = bundled();
        let mut hl = highlighter(&syntaxes, &[]);
        hl.depth = depth;
        let highlight = LineHighlight {
            lines: vec![LineRange {
                start: 2,
                end: Some(2),
            }],
            sgr: hl.line_highlight_sgr(),
        };
        let mut lines = LineState::new(Numbering::None, false, None).with_highlight(highlight);
        let mut out = Vec::new();
        let syntax = plain_text(&syntaxes);
        hl.write(
            &mut Cursor::new(input),
            &mut |reason| panic!("gave up: {reason}"),
            &mut out,
            syntax,
            &mut lines,
            limits,
        )
        .unwrap();
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn highlights_only_the_given_line() {
        let out = write_highlighted(b"a\nb\nc\n", NO_LIMITS, Depth::TrueColor);
        let lines: Vec<_> = out.lines().collect();
        assert!(!lines[0].contains("\x1b[48;"), "{out:?}");
        assert!(lines[1].starts_with("\x1b[48;2;"), "{out:?}");
        assert!(lines[1].ends_with("b\x1b[0m"), "{out:?}");
        assert!(!lines[2].contains("\x1b[48;"), "{out:?}");
    }

    #[test]
    fn highlights_with_256_colors() {
        let out = write_highlighted(b"a\nb\n", NO_LIMITS, Depth::Ansi256);
        assert!(out.contains("\x1b[48;5;"), "{out:?}");
        assert!(!out.contains("48;2;"), "{out:?}");
    }

    #[test]
    fn highlights_after_switching_to_plain() {
        let out = write_highlighted(b"abcde\nxy\n", max_line(4), Depth::TrueColor);
        assert!(out.starts_with("abcde\n\x1b[48;2;"), "{out:?}");
        assert!(out.ends_with("xy\x1b[0m\n"), "{out:?}");
    }

    #[test]
    fn highlights_a_long_line_in_one_piece() {
        let syntaxes = bundled();
        let sgr = highlighter(&syntaxes, &[]).line_highlight_sgr();
        let out = write_highlighted(b"a\nbbbbbbbbbb\nc\n", max_line(4), Depth::TrueColor);
        let lines: Vec<_> = out.split_inclusive('\n').collect();
        assert_eq!(lines[1], format!("{sgr}bbbbbbbbbb\x1b[0m\n"), "{out:?}");
        assert_eq!(lines[2], "c\n", "{out:?}");
    }

    #[test]
    fn tints_the_background_without_a_line_highlight_color() {
        let syntaxes = bundled();
        let mut hl = highlighter(&syntaxes, &[]);
        let theme = Arc::make_mut(&mut hl.theme);
        theme.settings.background = Some(Color::BLACK);
        theme.settings.foreground = Some(Color::WHITE);
        assert_eq!(hl.line_highlight_sgr(), "\x1b[48;2;51;51;51m");
        Arc::make_mut(&mut hl.theme).settings.line_highlight = Some(Color {
            r: 10,
            g: 20,
            b: 30,
            a: 255,
        });
        assert_eq!(hl.line_highlight_sgr(), "\x1b[48;2;10;20;30m");
    }

    #[test]
    fn uses_256_colors_when_asked() {
        let mut lines = LineState::new(Numbering::None, false, None);
        let out = write(b"a\n", &mut lines, NO_LIMITS, Depth::Ansi256);
        let out = String::from_utf8(out).unwrap();
        assert!(out.contains("\x1b[38;5;"), "{out:?}");
        assert!(!out.contains("38;2;"), "{out:?}");
    }

    /// Whether parsing `text` as `language` ends within `HANG_TIMEOUT`. The
    /// parser runs on a thread of its own, left behind if it hangs.
    fn parses_in_time(language: &str, text: &str) -> bool {
        let syntaxes = bundled();
        let syntax = Syntax::new(&syntaxes, syntaxes.find_syntax_by_name(language).unwrap());
        let text = text.to_string();
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let mut state = syntect::parsing::ParseState::new(&syntax);
            for line in text.split_inclusive('\n') {
                state.parse_line(line, syntax.set()).unwrap();
            }
            let _ = tx.send(());
        });
        rx.recv_timeout(HANG_TIMEOUT).is_ok()
    }

    /// How long `parses_in_time` waits. A hang never ends, so this only has
    /// to be well above the time a slow CI runner takes: compiling the
    /// regexes of a big syntax such as Markdown in a debug build takes
    /// seconds there, more so while other tests keep its cores busy.
    const HANG_TIMEOUT: Duration = Duration::from_secs(30);

    #[test]
    fn javascript_does_not_hang_on_a_comment_after_an_expression() {
        let cases = [
            ("JavaScript", "x /*"),
            ("JavaScript", "1 /*\n"),
            ("JavaScript", "foo /* comment */\n"),
            ("JavaScript (Rails)", "x /*\n"),
            ("QML", "1 /*\n"),
            ("HTML", "<script>\nx /*\n</script>\n"),
            ("Markdown", "```js\nx /*\n```\n"),
            ("Vue Component", "<script>\nx /*\n</script>\n"),
            ("Svelte", "<script>\nx /*\n</script>\n"),
            ("Svelte", "{x /*\n"),
        ];
        for (language, text) in cases {
            assert!(parses_in_time(language, text), "{language}: {text:?}");
        }
    }

    #[test]
    fn javascript_still_continues_an_expression_on_the_next_line() {
        let syntaxes = bundled();
        let js = syntaxes.find_syntax_by_name("JavaScript").unwrap();
        let scopes = |text: &str| {
            let mut state = syntect::parsing::ParseState::new(js);
            let mut stack = syntect::parsing::ScopeStack::new();
            let mut all = Vec::new();
            for line in text.split_inclusive('\n') {
                for (_, op) in state.parse_line(line, &syntaxes).unwrap() {
                    stack.apply(&op).unwrap();
                    all.push(format!("{stack:?}"));
                }
            }
            all.join("\n")
        };
        assert!(scopes("a\n/ 2\n").contains("keyword.operator"));
        assert!(scopes("a\n// c\n").contains("comment.line"));
    }

    #[test]
    fn scala_does_not_hang_on_an_operator_after_at_in_a_type() {
        let cases = [
            ":a@<",
            ":a@+",
            "x:\na@<",
            ":\na @<",
            "type L = A @> B\n",
            "x match { case y: A @> B => 1 }\n",
            "class A @> \n",
        ];
        for text in cases {
            assert!(parses_in_time("Scala", text), "{text:?}");
        }
    }

    #[test]
    fn scala_still_reads_annotations_in_a_type() {
        let syntaxes = bundled();
        let scala = syntaxes.find_syntax_by_name("Scala").unwrap();
        let scopes = |text: &str| {
            let mut state = syntect::parsing::ParseState::new(scala);
            let mut stack = syntect::parsing::ScopeStack::new();
            let mut all = Vec::new();
            for (_, op) in state.parse_line(text, &syntaxes).unwrap() {
                stack.apply(&op).unwrap();
                all.push(format!("{stack:?}"));
            }
            all.join("\n")
        };
        assert!(scopes("def f(x: Int @unchecked) = x\n").contains("variable.annotation"));
        assert!(scopes("type L = A @uncheckedVariance\n").contains("variable.annotation"));
        assert!(scopes("class A @Inject() (a: Int)\n").contains("variable.annotation"));
    }

    /// A syntax that fails on `x`: it pushes a syntax that does not exist.
    ///
    /// The tests of what happens after giving up use it rather than
    /// `LOOPING_SYNTAX`, which leaves a thread spinning for each input. It
    /// takes the same path from `Pipeline::give_up` on.
    const FAILING_SYNTAX: &str = r"%YAML 1.2
---
name: Fail
scope: source.fail
contexts:
  main:
    - match: \bx\b
      push: scope:source.nonexistent
";

    /// Highlights `input` with the syntax defined by `yaml`, and returns the
    /// output and the reasons highlighting was given up. Checks that only a
    /// stall makes `has_stalled` true.
    fn write_custom(
        yaml: &str,
        input: &[u8],
        lines: &mut LineState,
        limits: Limits,
        stall_timeout: Duration,
    ) -> (Vec<u8>, Vec<GiveUp>) {
        let syntax = custom_syntax(yaml);
        let mut hl = highlighter(&bundled(), &[]);
        hl.stall_timeout = stall_timeout;
        let mut out = Vec::new();
        let mut reasons = Vec::new();
        hl.write(
            &mut Cursor::new(input),
            &mut |reason| reasons.push(reason.clone()),
            &mut out,
            syntax,
            lines,
            limits,
        )
        .unwrap();
        // Only a stuck line keeps the later inputs from being highlighted.
        let stalled = reasons.iter().any(|r| matches!(r, GiveUp::Stalled(_)));
        assert_eq!(hl.has_stalled(), stalled, "{reasons:?}");
        (out, reasons)
    }

    #[test]
    fn prints_the_rest_plain_when_a_line_takes_too_long() {
        // Long enough for the worker to start and highlight `a` and `b` on
        // a busy machine.
        let stall = Duration::from_secs(2);
        let mut lines = LineState::new(Numbering::All, false, None);
        let (out, reasons) = write_custom(
            LOOPING_SYNTAX,
            b"a\nb\nx\nc\n",
            &mut lines,
            NO_LIMITS,
            stall,
        );
        assert_eq!(reasons, [GiveUp::Stalled(stall)]);
        assert_eq!(reasons[0].to_string(), "a line took longer than 2s");
        let (highlighted, plain) = out.split_at(out.len() - "     3\tx\n     4\tc\n".len());
        assert_eq!(plain, b"     3\tx\n     4\tc\n", "{out:?}");
        assert!(has_escape(highlighted), "{out:?}");
        assert_eq!(strip_ansi(highlighted), b"     1\ta\n     2\tb\n");
    }

    #[test]
    fn keeps_the_line_that_ends_highlighting_after_giving_up() {
        let not_utf8 = (&b"a\nx\n\xff\nc\n"[..], NO_LIMITS);
        let too_long = (&b"a\nx\nyyyyyyyyyyyyyyyy\nc\n"[..], max_line(4));
        let too_big = (
            &b"a\nx\nb\nc\n"[..],
            Limits {
                max_size: Some(5),
                max_line: None,
            },
        );
        for (input, limits) in [not_utf8, too_long, too_big] {
            let mut lines = LineState::new(Numbering::None, false, None);
            let (out, reasons) =
                write_custom(FAILING_SYNTAX, input, &mut lines, limits, STALL_TIMEOUT);
            assert_eq!(reasons.len(), 1, "{input:?}: {reasons:?}");
            assert_eq!(strip_ansi(&out), input, "{input:?}: {out:?}");
            assert!(out.ends_with(&input[2..]), "{input:?}: {out:?}");
        }
    }

    #[test]
    fn gives_up_with_too_many_batches_in_flight() {
        // `x` is in the first batch, which is printed once more than
        // `MAX_IN_FLIGHT` batches are handed over, long before the end.
        let mut input = "a\n".repeat(5) + "x\n";
        input += &"b\n".repeat(BATCH_LINES * (MAX_IN_FLIGHT + 2));
        input += "c\n";
        let plain = &input[input.find('x').unwrap()..];
        let mut lines = LineState::new(Numbering::None, false, None);
        let (out, reasons) = write_custom(
            FAILING_SYNTAX,
            input.as_bytes(),
            &mut lines,
            NO_LIMITS,
            STALL_TIMEOUT,
        );
        assert_eq!(reasons.len(), 1, "{reasons:?}");
        assert_eq!(strip_ansi(&out), input.as_bytes());
        assert!(out.ends_with(plain.as_bytes()));
    }

    #[test]
    fn gives_up_on_a_line_before_the_range() {
        let range = LineRange {
            start: 3,
            end: None,
        };
        for (numbering, expected) in [
            (Numbering::None, &b"c\nd\n"[..]),
            (Numbering::All, b"     3\tc\n     4\td\n"),
        ] {
            let mut lines = LineState::new(numbering, false, Some(range));
            let (out, reasons) = write_custom(
                FAILING_SYNTAX,
                b"a\nx\nc\nd\n",
                &mut lines,
                NO_LIMITS,
                STALL_TIMEOUT,
            );
            assert_eq!(reasons.len(), 1, "{reasons:?}");
            assert_eq!(out, expected);
        }
    }

    #[test]
    fn squeezes_the_lines_printed_plain_after_giving_up() {
        let mut lines = LineState::new(Numbering::None, true, None);
        let (out, reasons) = write_custom(
            FAILING_SYNTAX,
            b"a\nx\n\n\n\nc\n",
            &mut lines,
            NO_LIMITS,
            STALL_TIMEOUT,
        );
        assert_eq!(reasons.len(), 1, "{reasons:?}");
        assert_eq!(strip_ansi(&out), b"a\nx\n\nc\n");
        assert!(out.ends_with(b"x\n\nc\n"), "{out:?}");
    }

    #[test]
    fn highlights_across_batches() {
        let line = "abcdefghij\n";
        let input = line.repeat(BATCH_LINES * (MAX_IN_FLIGHT + 2) + 3);
        let out = write_simple(input.as_bytes(), Numbering::None, NO_LIMITS);
        assert_eq!(strip_ansi(&out), input.as_bytes());
        let out = String::from_utf8(out).unwrap();
        assert_eq!(out.matches("\x1b[0m\n").count(), input.lines().count());
    }

    #[test]
    fn highlights_lines_that_fill_a_batch_by_size() {
        let line = format!("{}\n", "a".repeat(BATCH_BYTES / 2));
        let input = line.repeat(MAX_IN_FLIGHT * 3);
        let out = write_simple(input.as_bytes(), Numbering::All, NO_LIMITS);
        let expected = (1..=MAX_IN_FLIGHT * 3).fold(String::new(), |mut s, n| {
            write!(s, "{n:>6}\t{line}").unwrap();
            s
        });
        assert_eq!(strip_ansi(&out), expected.as_bytes());
    }

    #[test]
    fn stops_at_the_end_of_the_line_range_within_a_batch() {
        let range = LineRange {
            start: 70,
            end: Some(72),
        };
        let mut lines = LineState::new(Numbering::All, false, Some(range));
        let input = (1..=200).fold(String::new(), |mut s, n| {
            writeln!(s, "{n}").unwrap();
            s
        });
        let out = write(input.as_bytes(), &mut lines, NO_LIMITS, Depth::TrueColor);
        assert_eq!(strip_ansi(&out), b"    70\t70\n    71\t71\n    72\t72\n");
    }

    #[test]
    fn switches_to_plain_after_a_batch_is_queued() {
        let mut input = "a\n".repeat(BATCH_LINES * 2 + 5).into_bytes();
        input.extend_from_slice(b"\xff\nz\n");
        let out = write_simple(&input, Numbering::None, NO_LIMITS);
        assert!(out.ends_with(b"a\x1b[0m\n\xff\nz\n"), "{out:?}");
        assert_eq!(strip_ansi(&out), input);
    }

    /// `--list-languages` sorts the names ignoring case, so `beta` comes
    /// between `Alpha` and `Delta`, and leaves out the hidden syntaxes.
    #[test]
    fn lists_languages_sorted_ignoring_case_without_hidden_ones() {
        let mut builder = SyntaxSetBuilder::new();
        for (name, hidden) in [
            ("beta", false),
            ("Gamma", true),
            ("Delta", false),
            ("Alpha", false),
        ] {
            let yaml = format!(
                "%YAML 1.2\n---\nname: {name}\nscope: source.{scope}\nfile_extensions: [{scope}]\n\
                 hidden: {hidden}\ncontexts:\n  main: []\n",
                scope = name.to_lowercase(),
            );
            builder.add(SyntaxDefinition::load_from_str(&yaml, true, None).unwrap());
        }
        let set = builder.build();
        let listed: Vec<_> = languages(&set)
            .into_iter()
            .map(|(name, extensions)| (name, extensions.to_vec()))
            .collect();
        assert_eq!(
            listed,
            [
                ("Alpha", vec!["alpha".to_string()]),
                ("beta", vec!["beta".to_string()]),
                ("Delta", vec!["delta".to_string()]),
            ]
        );
    }
}
