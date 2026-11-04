use std::{
    collections::VecDeque,
    io::{self, BufRead, Read, Write},
    path::{self, Path, PathBuf},
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, Sender},
    },
    thread,
    time::{Duration, Instant},
};

use globset::{GlobBuilder, GlobMatcher};
use syntect::{
    easy::HighlightLines,
    highlighting::{Color, Theme},
    parsing::{SyntaxReference, SyntaxSet},
};

use crate::{
    color::{Depth, RESET, background_sgr, blend, foreground_escaped},
    encoding::UTF8_BOM,
    lines::{LineState, Visibility, write_plain},
};

/// A language that inputs can be highlighted as.
pub type Syntax = SyntaxReference;

/// How much of the foreground tints the background for highlighted lines
/// when the theme has no `lineHighlight` color.
const FALLBACK_HIGHLIGHT_ALPHA: u8 = 0x33;

/// How long highlighting may go without finishing a line before it is given
/// up. syntect can loop forever on some syntaxes and inputs, and cannot be
/// interrupted, so it runs on a thread of its own that is left behind.
const STALL_TIMEOUT: Duration = Duration::from_secs(3);

/// How many times per `STALL_TIMEOUT` the highlighting thread is checked
/// for progress.
const STALL_POLLS: u32 = 10;

/// Lines are handed to the highlighting thread in batches of up to this
/// many lines or bytes, because handing over each line on its own costs
/// more than highlighting it.
const BATCH_LINES: usize = 64;
const BATCH_BYTES: usize = 64 * 1024;

/// The most batches handed over and not printed yet, which bounds the
/// memory used by input read ahead.
const MAX_IN_FLIGHT: usize = 4;

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

/// Where a syntax mapping comes from, which decides whether an unknown
/// language in it is an error or a warning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    /// `--map-syntax`.
    CommandLine,
    /// The `map-syntax` key of the config file at this path.
    ConfigFile(PathBuf),
}

/// A `PATTERN:LANG` pair from `--map-syntax` or the config file.
#[derive(Debug, Clone)]
pub struct SyntaxMapping {
    glob: GlobMatcher,
    /// Whether the pattern contains a path separator (`/`, or also `\` on
    /// Windows) and so applies to the whole path rather than the file name.
    by_path: bool,
    /// The mapping as written, for messages.
    pub spec: String,
    pub language: String,
    origin: Origin,
}

impl SyntaxMapping {
    /// Parses `PATTERN:LANG`, splitting at the last colon. The language is
    /// only checked once the syntaxes are loaded.
    pub fn parse(s: &str, origin: Origin) -> Result<Self, String> {
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
            by_path: pattern.contains(path::is_separator),
            spec: s.to_string(),
            language: language.to_string(),
            origin,
        })
    }

    pub fn origin(&self) -> &Origin {
        &self.origin
    }

    fn matches(&self, path: &Path) -> bool {
        if self.by_path {
            path::absolute(path).is_ok_and(|p| self.glob.is_match(p))
        } else {
            path.file_name().is_some_and(|n| self.glob.is_match(n))
        }
    }
}

/// Highlights inputs. The syntaxes are `'static` because they are shared
/// with the highlighting thread, which may outlive the input it was started
/// for (see `STALL_TIMEOUT`).
pub struct Highlighter {
    syntaxes: &'static SyntaxSet,
    theme: Arc<Theme>,
    depth: Depth,
    /// User mappings with their languages resolved; the last match wins.
    mappings: Vec<(SyntaxMapping, &'static Syntax)>,
    stall_timeout: Duration,
}

impl Highlighter {
    pub fn new(
        syntaxes: &'static SyntaxSet,
        theme: Theme,
        depth: Depth,
        mappings: Vec<(SyntaxMapping, &'static Syntax)>,
    ) -> Self {
        Self {
            syntaxes,
            theme: Arc::new(theme),
            depth,
            mappings,
            stall_timeout: STALL_TIMEOUT,
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
    pub fn detect(&self, path: Option<&Path>, first_line: &str) -> &'static Syntax {
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
    /// highlighted, so that the printed ones get the right colors. Nothing
    /// is read past the end of the line range.
    ///
    /// The lines are read and printed here, and highlighted on another
    /// thread a few batches ahead (see `Pipeline`).
    ///
    /// Once the input exceeds `limits.max_size` or a line exceeds
    /// `limits.max_line`, the rest is copied as is: syntect's state carries
    /// over between lines, so highlighting cannot resume after a skipped line.
    /// A line that is not valid UTF-8 switches to plain output the same way,
    /// and so does an error in the syntax or a line that takes longer than
    /// `STALL_TIMEOUT` to highlight, after passing the reason to `on_give_up`.
    pub fn write(
        &self,
        reader: &mut impl BufRead,
        on_give_up: &mut dyn FnMut(&str),
        out: &mut impl Write,
        syntax: &'static Syntax,
        lines: &mut LineState,
        limits: Limits,
    ) -> io::Result<()> {
        let mut pipeline = Pipeline::new(self.spawn_worker(syntax)?, on_give_up);
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
                    Flow::Plain => write_plain(reader, out, lines, Visibility::default()),
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
                return write_plain(reader, out, lines, Visibility::default());
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
                Flow::Plain => return write_plain(reader, out, lines, Visibility::default()),
            }
        }
    }

    /// Starts the thread that highlights the lines of one input with `syntax`.
    fn spawn_worker(&self, syntax: &'static Syntax) -> io::Result<Worker> {
        let (batch_tx, batch_rx) = mpsc::channel::<Vec<String>>();
        let (result_tx, result_rx) = mpsc::channel();
        let progress = Arc::new(AtomicU64::new(0));
        let done = Arc::new(Mutex::new(Vec::new()));
        let syntaxes = self.syntaxes;
        let theme = Arc::clone(&self.theme);
        let depth = self.depth;
        let (finished, results) = (Arc::clone(&progress), Arc::clone(&done));
        thread::Builder::new()
            .name("highlight".to_string())
            .spawn(move || {
                let mut hl = HighlightLines::new(syntax, &theme);
                for batch in batch_rx {
                    let mut failed = false;
                    for line in &batch {
                        let result = hl
                            .highlight_line(line, syntaxes)
                            .map(|ranges| foreground_escaped(&ranges, depth))
                            .map_err(|e| e.to_string());
                        failed = result.is_err();
                        lock(&results).push(result);
                        finished.fetch_add(1, Ordering::Relaxed);
                        if failed {
                            break;
                        }
                    }
                    let results = std::mem::take(&mut *lock(&results));
                    // syntect's state is broken after an error, and the other
                    // end hangs up once it stops highlighting.
                    if result_tx.send(results).is_err() || failed {
                        return;
                    }
                }
            })?;
        Ok(Worker {
            batches: batch_tx,
            results: result_rx,
            done,
            progress,
            stall_timeout: self.stall_timeout,
        })
    }
}

/// The escape sequences of a highlighted line, or why it could not be
/// highlighted.
type LineResult = Result<String, String>;

/// The results of a batch cut short: those of the lines highlighted before
/// the worker stopped, and why it stopped.
struct Stopped {
    done: Vec<LineResult>,
    reason: String,
}

/// Locks `mutex`, even after a panic on the worker, whose results are
/// still whole: a line is added in one push.
fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The highlighting thread of one input. Dropping it hangs up, so that the
/// thread ends after its current batch, or never if it is stuck in syntect.
struct Worker {
    batches: Sender<Vec<String>>,
    results: Receiver<Vec<LineResult>>,
    /// The results of the batch being highlighted, so far.
    done: Arc<Mutex<Vec<LineResult>>>,
    /// The number of lines highlighted so far.
    progress: Arc<AtomicU64>,
    stall_timeout: Duration,
}

impl Worker {
    fn send(&self, batch: Vec<String>) {
        // A thread that has ended is noticed when receiving.
        let _ = self.batches.send(batch);
    }

    /// The results of the oldest batch not received yet. Waits as long as
    /// lines keep being highlighted.
    ///
    /// When the worker is stuck on a line, it is still on that batch, since
    /// the earlier ones were received, so the results it has so far belong
    /// to it.
    fn recv(&self) -> Result<Vec<LineResult>, Stopped> {
        // Checking for progress a few times per timeout keeps the wait for
        // a stuck line close to the timeout.
        let poll = self.stall_timeout / STALL_POLLS;
        let mut seen = self.progress.load(Ordering::Relaxed);
        let mut since = Instant::now();
        let reason = loop {
            match self.results.recv_timeout(poll) {
                Ok(results) => return Ok(results),
                Err(RecvTimeoutError::Timeout) => {
                    let now = self.progress.load(Ordering::Relaxed);
                    if now != seen {
                        seen = now;
                        since = Instant::now();
                    } else if since.elapsed() >= self.stall_timeout {
                        break format!("a line took longer than {:?}", self.stall_timeout);
                    }
                }
                Err(RecvTimeoutError::Disconnected) => {
                    break "the highlighter stopped".to_string();
                }
            }
        };
        // The batch may have been finished right after the timeout.
        if let Ok(results) = self.results.try_recv() {
            return Ok(results);
        }
        let done = std::mem::take(&mut *lock(&self.done));
        Err(Stopped { done, reason })
    }
}

/// A line handed to the worker and not printed yet.
struct PendingLine {
    /// The line as read, including any BOM, for the line state and for
    /// printing it plain.
    raw: Vec<u8>,
    /// Whether the line starts with a BOM, which is printed before the colors.
    bom: bool,
}

/// What to do after printing what the worker has highlighted.
#[derive(Debug, PartialEq)]
enum Flow {
    /// Keep reading.
    Continue,
    /// The line range has ended.
    Done,
    /// Highlighting failed and the pending lines were printed plain: copy the
    /// rest of the input.
    Plain,
}

/// Lines on their way through the worker, in input order: those in the
/// batch being filled, then those of the batches handed over.
struct Pipeline<'r> {
    worker: Worker,
    /// Told why highlighting was given up.
    on_give_up: &'r mut dyn FnMut(&str),
    batch: Vec<String>,
    batch_bytes: usize,
    in_flight: usize,
    pending: VecDeque<PendingLine>,
}

impl<'r> Pipeline<'r> {
    fn new(worker: Worker, on_give_up: &'r mut dyn FnMut(&str)) -> Self {
        Self {
            worker,
            on_give_up,
            batch: Vec::new(),
            batch_bytes: 0,
            in_flight: 0,
            pending: VecDeque::new(),
        }
    }

    /// Queues `text` for highlighting, printing earlier lines when too many
    /// are waiting.
    fn push(
        &mut self,
        text: String,
        line: PendingLine,
        out: &mut impl Write,
        lines: &mut LineState,
    ) -> io::Result<Flow> {
        self.batch_bytes += text.len();
        self.batch.push(text);
        self.pending.push_back(line);
        if self.batch.len() >= BATCH_LINES || self.batch_bytes >= BATCH_BYTES {
            self.send_batch();
        }
        while self.in_flight > MAX_IN_FLIGHT {
            match self.print_batch(out, lines)? {
                Flow::Continue => {}
                flow => return Ok(flow),
            }
        }
        Ok(Flow::Continue)
    }

    /// Prints all the queued lines.
    fn finish(&mut self, out: &mut impl Write, lines: &mut LineState) -> io::Result<Flow> {
        self.send_batch();
        while self.in_flight > 0 {
            match self.print_batch(out, lines)? {
                Flow::Continue => {}
                flow => return Ok(flow),
            }
        }
        Ok(Flow::Continue)
    }

    fn send_batch(&mut self) {
        if self.batch.is_empty() {
            return;
        }
        self.worker.send(std::mem::take(&mut self.batch));
        self.batch_bytes = 0;
        self.in_flight += 1;
    }

    /// Prints the oldest batch handed over, once it is highlighted.
    fn print_batch(&mut self, out: &mut impl Write, lines: &mut LineState) -> io::Result<Flow> {
        let (results, stopped) = match self.worker.recv() {
            Ok(results) => (results, None),
            Err(Stopped { done, reason }) => (done, Some(reason)),
        };
        self.in_flight -= 1;
        for result in results {
            let line = self
                .pending
                .pop_front()
                .expect("each result is for a pending line");
            if lines.is_done() {
                return Ok(Flow::Done);
            }
            let print = lines.begin_line(&line.raw, out)?;
            match result {
                Ok(escaped) if print => {
                    if line.bom {
                        out.write_all(UTF8_BOM)?;
                    }
                    let newline = if line.raw.ends_with(b"\n") { "\n" } else { "" };
                    write!(out, "{escaped}{RESET}{newline}")?;
                }
                Ok(_) => {}
                Err(e) => {
                    if print {
                        lines.write_raw(&line.raw, out)?;
                    }
                    return self.give_up(&e, out, lines);
                }
            }
        }
        match stopped {
            Some(reason) => self.give_up(&reason, out, lines),
            None => Ok(Flow::Continue),
        }
    }

    /// Reports why highlighting failed and prints the pending lines plain.
    fn give_up(
        &mut self,
        reason: &str,
        out: &mut impl Write,
        lines: &mut LineState,
    ) -> io::Result<Flow> {
        (self.on_give_up)(reason);
        while let Some(line) = self.pending.pop_front() {
            if lines.is_done() {
                return Ok(Flow::Done);
            }
            if lines.begin_line(&line.raw, out)? {
                lines.write_raw(&line.raw, out)?;
            }
        }
        Ok(Flow::Plain)
    }
}

/// Resolves an explicit `--language` value by name or extension.
pub fn find_language<'a>(syntaxes: &'a SyntaxSet, lang: &str) -> Option<&'a Syntax> {
    syntaxes
        .find_syntax_by_token(lang)
        .or_else(|| syntaxes.find_syntax_by_extension(lang))
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
    use std::{fmt::Write as _, io::Cursor, sync::LazyLock};

    use super::*;
    use crate::lines::{LineHighlight, LineRange, Numbering};

    /// The bundled syntaxes only, so the user's custom ones stay out.
    fn syntaxes() -> &'static SyntaxSet {
        static SYNTAXES: LazyLock<SyntaxSet> = LazyLock::new(crate::assets::bundled_syntaxes);
        &SYNTAXES
    }

    fn highlighter(syntaxes: &'static SyntaxSet, mappings: &[&str]) -> Highlighter {
        let mappings = mappings
            .iter()
            .map(|m| {
                let mapping = SyntaxMapping::parse(m, Origin::CommandLine).unwrap();
                let syntax = find_language(syntaxes, &mapping.language).unwrap();
                (mapping, syntax)
            })
            .collect();
        Highlighter::new(syntaxes, Theme::default(), Depth::TrueColor, mappings)
    }

    /// The name of the syntax detected for `path` and `first_line`.
    fn detect(mappings: &[&str], path: Option<&str>, first_line: &str) -> String {
        let syntaxes = syntaxes();
        let hl = highlighter(syntaxes, mappings);
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
        let hl = highlighter(syntaxes, &[]);
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
        let syntaxes = syntaxes();
        let name = |lang| find_language(syntaxes, lang).map(|s| s.name.as_str());
        assert_eq!(name("Rust"), Some("Rust"));
        assert_eq!(name("rust"), Some("Rust"));
        assert_eq!(name("py"), Some("Python"));
        assert_eq!(name("no-such-language"), None);
        assert_eq!(name(""), None);
    }

    #[test]
    fn parses_a_mapping_at_the_last_colon() {
        let mapping = SyntaxMapping::parse("*.conf:INI", Origin::CommandLine).unwrap();
        assert_eq!(mapping.spec, "*.conf:INI");
        assert_eq!(mapping.language, "INI");
        assert_eq!(mapping.origin(), &Origin::CommandLine);
        let origin = Origin::ConfigFile(PathBuf::from("config.toml"));
        let mapping = SyntaxMapping::parse("a:b:INI", origin.clone()).unwrap();
        assert_eq!(mapping.language, "INI");
        assert_eq!(mapping.origin(), &origin);
    }

    #[test]
    fn rejects_malformed_mappings() {
        for spec in ["*.conf", ":INI", "*.conf:"] {
            let err = SyntaxMapping::parse(spec, Origin::CommandLine).unwrap_err();
            assert!(err.contains("is not PATTERN:LANG"), "{spec}: {err}");
        }
        assert!(SyntaxMapping::parse("[:INI", Origin::CommandLine).is_err());
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
        let mut hl = highlighter(syntaxes, &[]);
        hl.depth = depth;
        let mut out = Vec::new();
        let syntax = syntaxes.find_syntax_plain_text();
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

    #[test]
    fn stops_reading_after_the_line_range() {
        let range = LineRange {
            start: 1,
            end: Some(2),
        };
        let mut lines = LineState::new(Numbering::None, false, Some(range));
        let syntaxes = syntaxes();
        let hl = highlighter(syntaxes, &[]);
        let mut reader = Cursor::new(&b"a\nb\nc\nd\n"[..]);
        let mut out = Vec::new();
        hl.write(
            &mut reader,
            &mut |reason| panic!("gave up: {reason}"),
            &mut out,
            syntaxes.find_syntax_plain_text(),
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
        let mut hl = highlighter(syntaxes, &[]);
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
        let syntaxes = syntaxes();
        let sgr = highlighter(syntaxes, &[]).line_highlight_sgr();
        let out = write_highlighted(b"a\nbbbbbbbbbb\nc\n", max_line(4), Depth::TrueColor);
        let lines: Vec<_> = out.split_inclusive('\n').collect();
        assert_eq!(lines[1], format!("{sgr}bbbbbbbbbb\x1b[0m\n"), "{out:?}");
        assert_eq!(lines[2], "c\n", "{out:?}");
    }

    #[test]
    fn tints_the_background_without_a_line_highlight_color() {
        let syntaxes = syntaxes();
        let mut hl = highlighter(syntaxes, &[]);
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

    /// Whether parsing `text` as `language` ends within a few seconds. The
    /// parser runs on a thread of its own, left behind if it hangs.
    fn parses_in_time(language: &str, text: &str) -> bool {
        let syntaxes = syntaxes();
        let syntax = syntaxes.find_syntax_by_name(language).unwrap();
        let text = text.to_string();
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let mut state = syntect::parsing::ParseState::new(syntax);
            for line in text.split_inclusive('\n') {
                state.parse_line(line, syntaxes).unwrap();
            }
            let _ = tx.send(());
        });
        rx.recv_timeout(Duration::from_secs(5)).is_ok()
    }

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
        let syntaxes = syntaxes();
        let js = syntaxes.find_syntax_by_name("JavaScript").unwrap();
        let scopes = |text: &str| {
            let mut state = syntect::parsing::ParseState::new(js);
            let mut stack = syntect::parsing::ScopeStack::new();
            let mut all = Vec::new();
            for line in text.split_inclusive('\n') {
                for (_, op) in state.parse_line(line, syntaxes).unwrap() {
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
        let syntaxes = syntaxes();
        let scala = syntaxes.find_syntax_by_name("Scala").unwrap();
        let scopes = |text: &str| {
            let mut state = syntect::parsing::ParseState::new(scala);
            let mut stack = syntect::parsing::ScopeStack::new();
            let mut all = Vec::new();
            for (_, op) in state.parse_line(text, syntaxes).unwrap() {
                stack.apply(&op).unwrap();
                all.push(format!("{stack:?}"));
            }
            all.join("\n")
        };
        assert!(scopes("def f(x: Int @unchecked) = x\n").contains("variable.annotation"));
        assert!(scopes("type L = A @uncheckedVariance\n").contains("variable.annotation"));
        assert!(scopes("class A @Inject() (a: Int)\n").contains("variable.annotation"));
    }

    /// A syntax whose two contexts hand `x` back and forth without
    /// consuming it, which makes syntect loop forever.
    const LOOPING_SYNTAX: &str = r"%YAML 1.2
---
name: Loop
scope: source.loop
contexts:
  main:
    - match: (?=x)
      set: other
  other:
    - match: (?=x)
      set: main
";

    /// A syntax that fails on `x`: it pushes a syntax that does not exist.
    const FAILING_SYNTAX: &str = r"%YAML 1.2
---
name: Fail
scope: source.fail
contexts:
  main:
    - match: \bx\b
      push: scope:source.nonexistent
";

    /// A stall timeout short enough to keep the tests fast. A busy machine
    /// may take longer than this to start the worker, so the tests using it
    /// do not check which lines before `x` were highlighted.
    const SHORT_STALL: Duration = Duration::from_millis(200);

    /// The syntax defined by `yaml`, in a syntax set of its own.
    fn custom_syntax(yaml: &str) -> (&'static SyntaxSet, &'static Syntax) {
        let definition =
            syntect::parsing::SyntaxDefinition::load_from_str(yaml, true, None).unwrap();
        let mut builder = syntect::parsing::SyntaxSetBuilder::new();
        builder.add(definition);
        let set: &'static SyntaxSet = Box::leak(Box::new(builder.build()));
        (set, &set.syntaxes()[0])
    }

    /// Highlights `input` with the syntax defined by `yaml`, and returns the
    /// output and the reasons highlighting was given up.
    fn write_custom(
        yaml: &str,
        input: &[u8],
        lines: &mut LineState,
        limits: Limits,
        stall_timeout: Duration,
    ) -> (Vec<u8>, Vec<String>) {
        let (set, syntax) = custom_syntax(yaml);
        let mut hl = highlighter(syntaxes(), &[]);
        hl.syntaxes = set;
        hl.stall_timeout = stall_timeout;
        let mut out = Vec::new();
        let mut reasons = Vec::new();
        hl.write(
            &mut Cursor::new(input),
            &mut |reason| reasons.push(reason.to_string()),
            &mut out,
            syntax,
            lines,
            limits,
        )
        .unwrap();
        (out, reasons)
    }

    /// The syntaxes that give up on `x`, by stalling or by failing.
    const GIVING_UP: [&str; 2] = [LOOPING_SYNTAX, FAILING_SYNTAX];

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
        assert_eq!(reasons, ["a line took longer than 2s"]);
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
        for yaml in GIVING_UP {
            for (input, limits) in [not_utf8, too_long, too_big] {
                let mut lines = LineState::new(Numbering::None, false, None);
                let (out, reasons) = write_custom(yaml, input, &mut lines, limits, SHORT_STALL);
                assert_eq!(reasons.len(), 1, "{input:?}: {reasons:?}");
                assert_eq!(strip_ansi(&out), input, "{input:?}: {out:?}");
                assert!(out.ends_with(&input[2..]), "{input:?}: {out:?}");
            }
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
        for yaml in GIVING_UP {
            let mut lines = LineState::new(Numbering::None, false, None);
            let (out, reasons) =
                write_custom(yaml, input.as_bytes(), &mut lines, NO_LIMITS, SHORT_STALL);
            assert_eq!(reasons.len(), 1, "{reasons:?}");
            assert_eq!(strip_ansi(&out), input.as_bytes());
            assert!(out.ends_with(plain.as_bytes()));
        }
    }

    #[test]
    fn gives_up_on_a_line_before_the_range() {
        let range = LineRange {
            start: 3,
            end: None,
        };
        for yaml in GIVING_UP {
            for (numbering, expected) in [
                (Numbering::None, &b"c\nd\n"[..]),
                (Numbering::All, b"     3\tc\n     4\td\n"),
            ] {
                let mut lines = LineState::new(numbering, false, Some(range));
                let (out, reasons) =
                    write_custom(yaml, b"a\nx\nc\nd\n", &mut lines, NO_LIMITS, SHORT_STALL);
                assert_eq!(reasons.len(), 1, "{reasons:?}");
                assert_eq!(out, expected);
            }
        }
    }

    #[test]
    fn squeezes_the_lines_printed_plain_after_giving_up() {
        for yaml in GIVING_UP {
            let mut lines = LineState::new(Numbering::None, true, None);
            let (out, reasons) =
                write_custom(yaml, b"a\nx\n\n\n\nc\n", &mut lines, NO_LIMITS, SHORT_STALL);
            assert_eq!(reasons.len(), 1, "{reasons:?}");
            assert_eq!(strip_ansi(&out), b"a\nx\n\nc\n");
            assert!(out.ends_with(b"x\n\nc\n"), "{out:?}");
        }
    }

    #[test]
    fn reports_a_worker_that_hung_up() {
        let (batches, _) = mpsc::channel();
        let (results_tx, results) = mpsc::channel();
        drop(results_tx);
        let worker = Worker {
            batches,
            results,
            done: Arc::default(),
            progress: Arc::default(),
            stall_timeout: STALL_TIMEOUT,
        };
        let Err(Stopped { done, reason }) = worker.recv() else {
            panic!("received results from a worker that hung up");
        };
        assert_eq!(done, Vec::<LineResult>::new());
        assert_eq!(reason, "the highlighter stopped");
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
}
