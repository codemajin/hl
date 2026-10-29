use std::{
    io::{self, BufRead, Read, Write},
    path::{self, Path, PathBuf},
};

use globset::{GlobBuilder, GlobMatcher};
use syntect::{
    easy::HighlightLines,
    highlighting::Theme,
    parsing::{SyntaxReference, SyntaxSet},
    util::as_24_bit_terminal_escaped,
};

use crate::{
    color::{Depth, as_256_terminal_escaped},
    encoding::UTF8_BOM,
    lines::{LineState, Visibility, write_visible},
};

pub const DEFAULT_THEME: &str = "base16-ocean.dark";

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
    mappings: Vec<(SyntaxMapping, &'a SyntaxReference)>,
}

impl<'a> Highlighter<'a> {
    pub fn new(
        syntaxes: &'a SyntaxSet,
        theme: Theme,
        depth: Depth,
        mappings: Vec<(SyntaxMapping, &'a SyntaxReference)>,
    ) -> Self {
        Self {
            syntaxes,
            theme,
            depth,
            mappings,
        }
    }

    /// Detects the syntax from the user mappings, then the file name
    /// (extension, then full name) and finally the first line (shebang,
    /// modelines). Falls back to plain text.
    pub fn detect(&self, path: Option<&Path>, first_line: &str) -> &'a SyntaxReference {
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
                    out.write_all(&buf)?;
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
            let ranges = hl
                .highlight_line(line, self.syntaxes)
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

/// Resolves an explicit `--language` value by name or extension.
pub fn find_language<'a>(syntaxes: &'a SyntaxSet, lang: &str) -> Option<&'a SyntaxReference> {
    syntaxes
        .find_syntax_by_token(lang)
        .or_else(|| syntaxes.find_syntax_by_extension(lang))
}

/// Copies `reader` without highlighting, applying numbering, squeezing, the
/// line range and `cat -v` style notation as requested.
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
        if lines.is_done() {
            return Ok(());
        }
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
