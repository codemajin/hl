use std::io::{self, BufRead, Write};
use std::path::Path;

use anyhow::{Result, anyhow};
use syntect::easy::HighlightLines;
use syntect::highlighting::Theme;
use syntect::parsing::{SyntaxReference, SyntaxSet};
use syntect::util::as_24_bit_terminal_escaped;
use two_face::theme::LazyThemeSet;

use crate::lines::{LineState, Visibility, write_visible};

const RESET: &str = "\x1b[0m";

pub struct Highlighter {
    syntaxes: SyntaxSet,
    theme: Theme,
}

impl Highlighter {
    pub fn new(theme_name: &str) -> Result<Self> {
        let themes = LazyThemeSet::from(two_face::theme::extra());
        let theme = themes
            .get(theme_name)
            .ok_or_else(|| anyhow!("unknown theme '{theme_name}' (see --list-themes)"))?
            .clone();
        Ok(Self {
            syntaxes: two_face::syntax::extra_newlines(),
            theme,
        })
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
    pub fn write(
        &self,
        reader: &mut impl BufRead,
        out: &mut impl Write,
        syntax: &SyntaxReference,
        lines: &mut LineState,
    ) -> io::Result<()> {
        let mut hl = HighlightLines::new(syntax, &self.theme);
        let mut buf = Vec::new();
        loop {
            buf.clear();
            if reader.read_until(b'\n', &mut buf)? == 0 {
                return Ok(());
            }
            let print = lines.begin_line(&buf, out)?;
            let line = String::from_utf8_lossy(&buf);
            let newline = if line.ends_with('\n') { "\n" } else { "" };
            // Highlight including the newline so syntect's state stays correct,
            // but print the newline after the reset sequence.
            let ranges = hl
                .highlight_line(&line, &self.syntaxes)
                .map_err(io::Error::other)?;
            if !print {
                continue;
            }
            let escaped = as_24_bit_terminal_escaped(&ranges, false);
            let escaped = escaped.strip_suffix('\n').unwrap_or(&escaped);
            write!(out, "{escaped}{RESET}{newline}")?;
        }
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
