use std::io::{self, BufRead, Read, Write};
use std::path::Path;

use syntect::easy::HighlightLines;
use syntect::highlighting::Theme;
use syntect::parsing::{SyntaxReference, SyntaxSet};
use syntect::util::as_24_bit_terminal_escaped;
use two_face::theme::LazyThemeSet;

use crate::cli::DEFAULT_THEME;
use crate::color::{Depth, as_256_terminal_escaped};
use crate::encoding::UTF8_BOM;
use crate::lines::{LineState, Visibility, write_visible};

const RESET: &str = "\x1b[0m";

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

pub struct Highlighter {
    syntaxes: SyntaxSet,
    theme: Theme,
    depth: Depth,
}

impl Highlighter {
    /// An unknown theme is not fatal: it warns and falls back to the default,
    /// so a typo in `HL_THEME` never hides the file contents.
    pub fn new(theme_name: &str, depth: Depth) -> Self {
        let themes = LazyThemeSet::from(two_face::theme::extra());
        let theme = resolve_theme(&themes, theme_name)
            .unwrap_or_else(|| {
                eprintln!(
                    "hl: unknown theme '{theme_name}', falling back to {DEFAULT_THEME} (see --list-themes)"
                );
                themes.get(DEFAULT_THEME).expect("default theme is bundled")
            })
            .clone();
        Self {
            syntaxes: two_face::syntax::extra_newlines(),
            theme,
            depth,
        }
    }

    pub fn syntaxes(&self) -> &SyntaxSet {
        &self.syntaxes
    }

    /// Resolves an explicit `--language` value by name or extension.
    pub fn find_language(&self, lang: &str) -> Option<&SyntaxReference> {
        self.syntaxes
            .find_syntax_by_token(lang)
            .or_else(|| self.syntaxes.find_syntax_by_extension(lang))
    }

    /// Detects the syntax from the file name (extension, then full name) and
    /// finally the first line (shebang, modelines). Falls back to plain text.
    pub fn detect(&self, path: Option<&Path>, first_line: &str) -> &SyntaxReference {
        let by_path = path.and_then(|p| {
            let ext = p.extension().and_then(|e| e.to_str());
            let name = p.file_name().and_then(|n| n.to_str());
            ext.and_then(|e| self.syntaxes.find_syntax_by_extension(e))
                .or_else(|| name.and_then(|n| self.syntaxes.find_syntax_by_extension(n)))
        });
        by_path
            .or_else(|| self.syntaxes.find_syntax_by_first_line(first_line))
            .unwrap_or_else(|| self.syntaxes.find_syntax_plain_text())
    }

    /// Writes `reader` highlighted with `syntax`, one line at a time.
    ///
    /// Once the input exceeds `limits.max_size` or a line exceeds
    /// `limits.max_line`, the rest is copied as is: syntect's state carries
    /// over between lines, so highlighting cannot resume after a skipped line.
    /// A line that is not valid UTF-8 switches to plain output the same way.
    pub fn write(
        &self,
        reader: &mut impl BufRead,
        out: &mut impl Write,
        syntax: &SyntaxReference,
        lines: &mut LineState,
        limits: Limits,
    ) -> io::Result<()> {
        let mut hl = HighlightLines::new(syntax, &self.theme);
        let max_line = limits.max_line.unwrap_or(u64::MAX);
        let mut total: u64 = 0;
        let mut buf = Vec::new();
        loop {
            buf.clear();
            // Reading at most one byte past the limit keeps a huge line out of memory.
            let n = reader
                .by_ref()
                .take(max_line.saturating_add(1))
                .read_until(b'\n', &mut buf)?;
            if n == 0 {
                return Ok(());
            }
            total += n as u64;
            let too_long = !buf.ends_with(b"\n") && n as u64 > max_line;
            // Input that is not UTF-8 (Shift_JIS, ...) is copied unchanged
            // rather than replacing its bytes with U+FFFD.
            let text = std::str::from_utf8(&buf).ok();
            let Some(mut line) = text.filter(|_| !too_long && !limits.exceeds_size(total)) else {
                if lines.begin_line(&buf, out)? {
                    out.write_all(&buf)?;
                }
                return write_plain(reader, out, lines, Visibility::default());
            };
            let print = lines.begin_line(&buf, out)?;
            if total == n as u64
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
            let ranges = hl
                .highlight_line(line, &self.syntaxes)
                .map_err(io::Error::other)?;
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

/// Looks a theme up by exact name, then case-insensitively as long as the
/// match is unambiguous (so `nord` finds `Nord`).
fn resolve_theme<'a>(themes: &'a LazyThemeSet, name: &str) -> Option<&'a Theme> {
    if let Some(theme) = themes.get(name) {
        return Some(theme);
    }
    let mut matches = themes.theme_names().filter(|n| n.eq_ignore_ascii_case(name));
    match (matches.next(), matches.next()) {
        (Some(found), None) => themes.get(found),
        _ => None,
    }
}

/// Copies `reader` without highlighting, applying numbering, squeezing and
/// `cat -v` style notation as requested.
pub fn write_plain(
    reader: &mut impl BufRead,
    out: &mut impl Write,
    lines: &mut LineState,
    vis: Visibility,
) -> io::Result<()> {
    if lines.is_passthrough() && !vis.any() {
        io::copy(reader, out)?;
        return Ok(());
    }
    let mut buf = Vec::new();
    loop {
        buf.clear();
        if reader.read_until(b'\n', &mut buf)? == 0 {
            return Ok(());
        }
        if !lines.begin_line(&buf, out)? {
            continue;
        }
        if vis.any() {
            write_visible(&buf, vis, out)?;
        } else {
            out.write_all(&buf)?;
        }
    }
}
