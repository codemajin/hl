//! The thread that highlights the lines of one input, and the pipeline that
//! hands lines to it in batches and prints what comes back, giving up when
//! it stops finishing lines (see `STALL_TIMEOUT`).

use std::{
    collections::VecDeque,
    io::{self, Write},
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, Sender},
    },
    thread,
    time::{Duration, Instant},
};

use syntect::{easy::HighlightLines, highlighting::Theme};

use super::{GiveUp, Syntax};
use crate::{
    color::{Depth, RESET, foreground_escaped},
    encoding::UTF8_BOM,
    lines::LineState,
};

/// How many times per `STALL_TIMEOUT` the highlighting thread is checked
/// for progress.
const STALL_POLLS: u32 = 10;

/// Lines are handed to the highlighting thread in batches of up to this
/// many lines or bytes, because handing over each line on its own costs
/// more than highlighting it.
pub(super) const BATCH_LINES: usize = 64;
pub(super) const BATCH_BYTES: usize = 64 * 1024;

/// The most batches handed over and not printed yet, which bounds the
/// memory used by input read ahead.
pub(super) const MAX_IN_FLIGHT: usize = 4;

/// The escape sequences of a highlighted line, or why it could not be
/// highlighted.
type LineResult = Result<String, String>;

/// The results of a batch cut short: those of the lines highlighted before
/// the worker stopped, and why it stopped.
struct Stopped {
    done: Vec<LineResult>,
    reason: GiveUp,
}

/// Locks `mutex`, even after a panic on the worker, whose results are
/// still whole: a line is added in one push.
fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The highlighting thread of one input. Dropping it hangs up, so that the
/// thread ends after its current batch, or never if it is stuck in syntect.
pub(super) struct Worker {
    batches: Sender<Vec<String>>,
    results: Receiver<Vec<LineResult>>,
    /// The results of the batch being highlighted, so far.
    done: Arc<Mutex<Vec<LineResult>>>,
    /// The number of lines highlighted so far.
    progress: Arc<AtomicU64>,
    stall_timeout: Duration,
}

impl Worker {
    /// Starts the thread that highlights the lines of one input with
    /// `syntax`, which it keeps, with its syntax set, as long as it runs.
    /// Highlighting is given up when no line finishes for `stall_timeout`.
    pub(super) fn spawn(
        syntax: Syntax,
        theme: Arc<Theme>,
        depth: Depth,
        stall_timeout: Duration,
    ) -> io::Result<Self> {
        let (batch_tx, batch_rx) = mpsc::channel::<Vec<String>>();
        let (result_tx, result_rx) = mpsc::channel();
        let progress = Arc::new(AtomicU64::new(0));
        let done = Arc::new(Mutex::new(Vec::new()));
        let (finished, results) = (Arc::clone(&progress), Arc::clone(&done));
        thread::Builder::new()
            .name("highlight".to_string())
            .spawn(move || {
                let mut hl = HighlightLines::new(&syntax, &theme);
                for batch in batch_rx {
                    let mut failed = false;
                    for line in &batch {
                        let result = hl
                            .highlight_line(line, syntax.set())
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
        Ok(Self {
            batches: batch_tx,
            results: result_rx,
            done,
            progress,
            stall_timeout,
        })
    }

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
                        break GiveUp::Stalled(self.stall_timeout);
                    }
                }
                Err(RecvTimeoutError::Disconnected) => {
                    break GiveUp::Failed("the highlighter stopped".to_string());
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
pub(super) struct PendingLine {
    /// The line as read, including any BOM, for the line state and for
    /// printing it plain.
    pub(super) raw: Vec<u8>,
    /// Whether the line starts with a BOM, which is printed before the colors.
    pub(super) bom: bool,
}

/// What to do after printing what the worker has highlighted.
#[derive(Debug, PartialEq)]
pub(super) enum Flow {
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
pub(super) struct Pipeline<'r> {
    worker: Worker,
    /// Told why highlighting was given up.
    on_give_up: &'r mut dyn FnMut(&GiveUp),
    /// Set when the worker gets stuck (`Highlighter::has_stalled`).
    stalled: &'r AtomicBool,
    batch: Vec<String>,
    batch_bytes: usize,
    in_flight: usize,
    pending: VecDeque<PendingLine>,
}

impl<'r> Pipeline<'r> {
    pub(super) fn new(
        worker: Worker,
        on_give_up: &'r mut dyn FnMut(&GiveUp),
        stalled: &'r AtomicBool,
    ) -> Self {
        Self {
            worker,
            on_give_up,
            stalled,
            batch: Vec::new(),
            batch_bytes: 0,
            in_flight: 0,
            pending: VecDeque::new(),
        }
    }

    /// Queues `text` for highlighting, printing earlier lines when too many
    /// are waiting.
    pub(super) fn push(
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
    pub(super) fn finish(
        &mut self,
        out: &mut impl Write,
        lines: &mut LineState,
    ) -> io::Result<Flow> {
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
                    return self.give_up(&GiveUp::Failed(e), out, lines);
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
        reason: &GiveUp,
        out: &mut impl Write,
        lines: &mut LineState,
    ) -> io::Result<Flow> {
        if matches!(reason, GiveUp::Stalled(_)) {
            self.stalled.store(true, Ordering::Relaxed);
        }
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

#[cfg(test)]
mod tests {
    use super::{super::STALL_TIMEOUT, *};

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
        assert_eq!(
            reason,
            GiveUp::Failed("the highlighter stopped".to_string())
        );
    }
}
