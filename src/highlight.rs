use std::{
    io::{self, BufRead, Read, Write},
    path::{self, Path, PathBuf},
};

use globset::{GlobBuilder, GlobMatcher};
use syntect::{
    easy::HighlightLines,
    highlighting::{Color, Theme},
    parsing::{SyntaxReference, SyntaxSet},
    util::as_24_bit_terminal_escaped,
};

use crate::{
    color::{Depth, as_256_terminal_escaped, background_sgr, blend},
    diag,
    encoding::UTF8_BOM,
    lines::{LineState, RESET, Visibility, write_plain},
};

pub const DEFAULT_THEME: &str = "base16-ocean.dark";

/// A language that inputs can be highlighted as.
pub type Syntax = SyntaxReference;

/// How much of the foreground tints the background for highlighted lines
/// when the theme has no `lineHighlight` color.
const FALLBACK_HIGHLIGHT_ALPHA: u8 = 0x33;

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

/// A `PATTERN:LANG` pair from `--map-syntax` or the config file.
#[derive(Debug, Clone)]
pub struct SyntaxMapping {
    glob: GlobMatcher,
    /// Whether the pattern contains `/` and so applies to the whole path
    /// rather than the file name.
    by_path: bool,
    /// The mapping as written, for messages.
    pub spec: String,
    pub language: String,
    /// The config file the mapping comes from, or `None` for the command line.
    pub origin: Option<PathBuf>,
}

impl SyntaxMapping {
    /// Parses `PATTERN:LANG`, splitting at the last colon. The language is
    /// only checked once the syntaxes are loaded.
    pub fn parse(s: &str) -> Result<Self, String> {
        let (pattern, language) = s
            .rsplit_once(':')
            .filter(|(p, l)| !p.is_empty() && !l.is_empty())
            .ok_or_else(|| format!("'{s}' is not PATTERN:LANG (e.g. '*.conf:INI')"))?;
        let glob = GlobBuilder::new(pattern)
            .literal_separator(true)
            .build()
            .map_err(|e| e.kind().to_string())?
            .compile_matcher();
        Ok(Self {
            glob,
            by_path: pattern.contains('/'),
            spec: s.to_string(),
            language: language.to_string(),
            origin: None,
        })
    }

    fn matches(&self, path: &Path) -> bool {
        if self.by_path {
            path::absolute(path).is_ok_and(|p| self.glob.is_match(p))
        } else {
            path.file_name().is_some_and(|n| self.glob.is_match(n))
        }
    }
}

pub struct Highlighter<'a> {
    syntaxes: &'a SyntaxSet,
    theme: Theme,
    depth: Depth,
    /// User mappings with their languages resolved; the last match wins.
    mappings: Vec<(SyntaxMapping, &'a Syntax)>,
}

impl<'a> Highlighter<'a> {
    pub fn new(
        syntaxes: &'a SyntaxSet,
        theme: Theme,
        depth: Depth,
        mappings: Vec<(SyntaxMapping, &'a Syntax)>,
    ) -> Self {
        Self {
            syntaxes,
            theme,
            depth,
            mappings,
        }
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
    pub fn detect(&self, path: Option<&Path>, first_line: &str) -> &'a Syntax {
        let by_path = path.and_then(|p| {
            let mapped = self.mappings.iter().rev().find(|(m, _)| m.matches(p));
            let ext = p.extension().and_then(|e| e.to_str());
            let name = p.file_name().and_then(|n| n.to_str());
            mapped
                .map(|&(_, syntax)| syntax)
                .or_else(|| ext.and_then(|e| self.syntaxes.find_syntax_by_extension(e)))
                .or_else(|| name.and_then(|n| self.syntaxes.find_syntax_by_extension(n)))
        });
        by_path
            .or_else(|| self.syntaxes.find_syntax_by_first_line(first_line))
            .unwrap_or_else(|| self.syntaxes.find_syntax_plain_text())
    }

    /// Writes `reader` highlighted with `syntax`, one line at a time. Lines
    /// that are not printed (outside the line range, squeezed) are still
    /// highlighted, so that the printed ones get the right colors.
    ///
    /// Once the input exceeds `limits.max_size` or a line exceeds
    /// `limits.max_line`, the rest is copied as is: syntect's state carries
    /// over between lines, so highlighting cannot resume after a skipped line.
    /// A line that is not valid UTF-8 switches to plain output the same way,
    /// and so does an error in the syntax, after a warning about `name`.
    pub fn write(
        &self,
        reader: &mut impl BufRead,
        name: &Path,
        out: &mut impl Write,
        syntax: &Syntax,
        lines: &mut LineState,
        limits: Limits,
    ) -> io::Result<()> {
        let mut hl = HighlightLines::new(syntax, &self.theme);
        let max_line = limits.max_line.unwrap_or(u64::MAX);
        let mut total: u64 = 0;
        let mut buf = Vec::new();
        loop {
            if lines.is_done() {
                return Ok(());
            }
            buf.clear();
            // Reading at most one byte past the limit keeps a huge line out of memory.
            let n = reader
                .by_ref()
                .take(max_line.saturating_add(1))
                .read_until(b'\n', &mut buf)?;
            if n == 0 {
                return Ok(());
            }
            let n = u64::try_from(n).expect("a line read into memory fits in u64");
            total += n;
            let too_long = !buf.ends_with(b"\n") && n > max_line;
            // Input that is not UTF-8 (Shift_JIS, ...) is copied unchanged
            // rather than replacing its bytes with U+FFFD.
            let text = std::str::from_utf8(&buf).ok();
            let Some(mut line) = text.filter(|_| !too_long && !limits.exceeds_size(total)) else {
                if lines.begin_line(&buf, out)? {
                    lines.write_raw(&buf, out)?;
                }
                return write_plain(reader, out, lines, Visibility::default());
            };
            let print = lines.begin_line(&buf, out)?;
            if total == n
                && let Some(rest) = line.strip_prefix('\u{FEFF}')
            {
                // Keep the BOM in the output, but out of syntect's way.
                if print {
                    out.write_all(UTF8_BOM)?;
                }
                line = rest;
            }
            let newline = if line.ends_with('\n') { "\n" } else { "" };
            // Highlight including the newline so syntect's state stays correct,
            // but print the newline after the reset sequence.
            let ranges = match hl.highlight_line(line, self.syntaxes) {
                Ok(ranges) => ranges,
                Err(e) => {
                    diag::warn(
                        name,
                        &format!("cannot highlight: {e}, printing the rest as plain text"),
                    );
                    if print {
                        lines.write_raw(line.as_bytes(), out)?;
                    }
                    return write_plain(reader, out, lines, Visibility::default());
                }
            };
            if !print {
                continue;
            }
            let escaped = match self.depth {
                Depth::TrueColor => as_24_bit_terminal_escaped(&ranges, false),
                Depth::Ansi256 => as_256_terminal_escaped(&ranges),
            };
            let escaped = escaped.strip_suffix('\n').unwrap_or(&escaped);
            write!(out, "{escaped}{RESET}{newline}")?;
        }
    }
}

/// Resolves an explicit `--language` value by name or extension.
pub fn find_language<'a>(syntaxes: &'a SyntaxSet, lang: &str) -> Option<&'a Syntax> {
    syntaxes
        .find_syntax_by_token(lang)
        .or_else(|| syntaxes.find_syntax_by_extension(lang))
}

/// Looks up the language of each syntax mapping. An unknown language is an
/// error on the command line, but only a warning in the config file, which
/// never stops `hl` from printing.
pub fn resolve_mappings<'a, 'm>(
    syntaxes: &'a SyntaxSet,
    mappings: impl IntoIterator<Item = &'m SyntaxMapping>,
) -> Result<Vec<(SyntaxMapping, &'a Syntax)>, String> {
    let mut resolved = Vec::new();
    for mapping in mappings {
        let Some(syntax) = find_language(syntaxes, &mapping.language) else {
            let msg = format!(
                "unknown language '{}' in map-syntax '{}'",
                mapping.language, mapping.spec
            );
            match &mapping.origin {
                Some(path) => diag::warn(path, &format!("{msg}, ignoring it")),
                None => return Err(format!("{msg} (see --list-languages)")),
            }
            continue;
        };
        resolved.push((mapping.clone(), syntax));
    }
    Ok(resolved)
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
    use std::io::Cursor;

    use super::*;
    use crate::lines::{LineHighlight, LineRange, Numbering};

    /// The bundled syntaxes only, so the user's custom ones stay out.
    fn syntaxes() -> SyntaxSet {
        two_face::syntax::extra_newlines()
    }

    fn highlighter<'a>(syntaxes: &'a SyntaxSet, mappings: &[&str]) -> Highlighter<'a> {
        let mappings: Vec<_> = mappings
            .iter()
            .map(|m| SyntaxMapping::parse(m).unwrap())
            .collect();
        let mappings = resolve_mappings(syntaxes, &mappings).unwrap();
        Highlighter::new(syntaxes, Theme::default(), Depth::TrueColor, mappings)
    }

    /// The name of the syntax detected for `path` and `first_line`.
    fn detect(mappings: &[&str], path: Option<&str>, first_line: &str) -> String {
        let syntaxes = syntaxes();
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

        let syntaxes = syntaxes();
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

    #[test]
    fn finds_a_language_by_name_or_extension() {
        let syntaxes = syntaxes();
        let name = |lang| find_language(&syntaxes, lang).map(|s| s.name.as_str());
        assert_eq!(name("Rust"), Some("Rust"));
        assert_eq!(name("rust"), Some("Rust"));
        assert_eq!(name("py"), Some("Python"));
        assert_eq!(name("no-such-language"), None);
        assert_eq!(name(""), None);
    }

    #[test]
    fn parses_a_mapping_at_the_last_colon() {
        let mapping = SyntaxMapping::parse("*.conf:INI").unwrap();
        assert_eq!(mapping.spec, "*.conf:INI");
        assert_eq!(mapping.language, "INI");
        assert!(mapping.origin.is_none());
        assert_eq!(SyntaxMapping::parse("a:b:INI").unwrap().language, "INI");
    }

    #[test]
    fn rejects_malformed_mappings() {
        for spec in ["*.conf", ":INI", "*.conf:"] {
            let err = SyntaxMapping::parse(spec).unwrap_err();
            assert!(err.contains("is not PATTERN:LANG"), "{spec}: {err}");
        }
        assert!(SyntaxMapping::parse("[:INI").is_err());
    }

    #[test]
    fn unknown_language_is_an_error_on_the_command_line() {
        let syntaxes = syntaxes();
        let mapping = SyntaxMapping::parse("*.x:no-such-language").unwrap();
        let err = resolve_mappings(&syntaxes, [&mapping]).unwrap_err();
        assert!(err.contains("see --list-languages"), "{err}");
    }

    #[test]
    fn unknown_language_in_the_config_is_skipped() {
        let syntaxes = syntaxes();
        let mut unknown = SyntaxMapping::parse("*.x:no-such-language").unwrap();
        unknown.origin = Some(PathBuf::from("config.toml"));
        let known = SyntaxMapping::parse("*.y:Rust").unwrap();
        let resolved = resolve_mappings(&syntaxes, [&unknown, &known]).unwrap();
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].0.spec, "*.y:Rust");
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
        let syntaxes = syntaxes();
        let mut hl = highlighter(&syntaxes, &[]);
        hl.depth = depth;
        let mut out = Vec::new();
        let syntax = syntaxes.find_syntax_plain_text();
        hl.write(
            &mut Cursor::new(input),
            Path::new("-"),
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

    fn strip_ansi(bytes: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut rest = bytes;
        while let Some((&b, tail)) = rest.split_first() {
            if b == 0x1b {
                let end = tail.iter().position(|&c| c == b'm').unwrap();
                rest = &tail[end + 1..];
            } else {
                out.push(b);
                rest = tail;
            }
        }
        out
    }

    fn has_escape(bytes: &[u8]) -> bool {
        bytes.contains(&0x1b)
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
        let syntaxes = syntaxes();
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
        let syntax = syntaxes.find_syntax_plain_text();
        hl.write(
            &mut Cursor::new(input),
            Path::new("-"),
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
        let syntaxes = syntaxes();
        let sgr = highlighter(&syntaxes, &[]).line_highlight_sgr();
        let out = write_highlighted(b"a\nbbbbbbbbbb\nc\n", max_line(4), Depth::TrueColor);
        let lines: Vec<_> = out.split_inclusive('\n').collect();
        assert_eq!(lines[1], format!("{sgr}bbbbbbbbbb\x1b[0m\n"), "{out:?}");
        assert_eq!(lines[2], "c\n", "{out:?}");
    }

    #[test]
    fn tints_the_background_without_a_line_highlight_color() {
        let syntaxes = syntaxes();
        let mut hl = highlighter(&syntaxes, &[]);
        hl.theme.settings.background = Some(Color::BLACK);
        hl.theme.settings.foreground = Some(Color::WHITE);
        assert_eq!(hl.line_highlight_sgr(), "\x1b[48;2;51;51;51m");
        hl.theme.settings.line_highlight = Some(Color {
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
}
