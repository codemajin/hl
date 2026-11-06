# hl design

[English](DESIGN.md) | [日本語](DESIGN_ja.md) · [README](../README.md)

This document describes how the code of `hl` is organized and why. What `hl` does is described in [SPEC.md](SPEC.md), and how to build, test and release it in [DEV.md](DEV.md).

## Overview

`hl` is a single binary crate. `src/main.rs` reads the settings, picks where the output goes, and hands the inputs to `print::print_all`, which prints each of them highlighted or plain.

```mermaid
flowchart TD
    start(["hl starts"]) --> check["stdout::check<br/>Was stdout open at startup?"]
    check --> settings["config::Dirs::from_env, Config::load,<br/>cli::parse_with<br/>Settings: command line, environment, config file"]
    settings --> info{"--list-themes, --man,<br/>--build-cache, ...?"}
    info -->|yes| infoout["Print it and exit"]
    info -->|no| color["Args::use_color<br/>Colored output?"]
    color --> assets["assets::syntaxes<br/>Bundled and custom syntaxes,<br/>loaded only when needed"]
    assets --> printer["build_printer, build_line_state<br/>Highlighter (only when colored), language,<br/>mappings, limits, line numbering"]
    printer --> out["start_pager<br/>Pager or stdout"]
    out --> printall["print::print_all<br/>Each input in turn"]
    printall --> finish["Flush, wait for the pager,<br/>exit status"]
```

Most of the work is done once per run before the first input: the syntaxes and the theme are loaded, the `--map-syntax` mappings are resolved, and a `Printer` is built. `print_all` then only reads, decides and writes.

## Modules

| File | Role |
| --- | --- |
| `src/main.rs` | Entry point: runs the commands that print no inputs (`--list-themes`, ...), builds the printer and the line state from the arguments and config, picks the pager or stdout, handles errors and the exit code. Decides what the problems found on the way become: an unknown language in `--map-syntax` is an error and one in the config file a warning, an unknown or broken theme falls back to the default with a warning, and a problem with an input from `src/print.rs` gets a warning. The only place that reads the environment (`PAGER`, `LESS`, the color depth). Resolves the config and cache directories once (`config::Dirs::from_env`) and passes them to the config file and `src/assets.rs` |
| `src/assets.rs` | The default theme, the bundled syntaxes, custom syntaxes and themes from the config directory, merged with the bundled ones, and the syntax cache (`--build-cache`/`--clear-cache`). Takes the directories as arguments, so it reads no environment variables |
| `src/cli.rs` | Command-line argument definitions (clap) and the config file applied to them (option defaults, flags, `map-syntax`). The rest of the code reads the arguments through methods that combine related options (`numbering()`, `visibility()`, `limits()`, ...), not through the raw fields |
| `src/color.rs` | Terminal color depth detection, the 256-color fallback, and the escape sequences for colors, bold and reset |
| `src/config.rs` | Config file loading (`HL_CONFIG` or the config directory) and the config/cache directories (`Dirs`, from `XDG_CONFIG_HOME`/`XDG_CACHE_HOME`, `APPDATA`/`LOCALAPPDATA` on Windows, and `HOME`) |
| `src/console.rs` | Turning on escape sequence processing in the Windows console, so that colors work in cmd.exe and PowerShell outside Windows Terminal |
| `src/diag.rs` | Warning and error messages on stderr. Everything written to stderr goes through it, so the `hl: ` format is set in one place; an error is shown with its causes (`path: reason`) |
| `src/encoding.rs` | UTF-8/UTF-16 BOM detection and UTF-16 decoding |
| `src/escape.rs` | Removing the escape sequences of the input (CSI, OSC and the other strings, C1 controls in UTF-8) and the C0 controls that act on the terminal (BEL, BS, SO, ...) as a reader, also when a sequence is split across reads |
| `src/highlight.rs` | Language detection, `--map-syntax` mappings, size limits, and highlighted output (syntect, on a thread of its own with a time limit). Writes no warnings: the reason highlighting was given up goes to the caller |
| `src/lines.rs` | Line numbering, blank-line squeezing, and `cat -v`/`-E`/`-T` notation |
| `src/pager.rs` | Building the pager command from `$PAGER` (split like the shell, or by Windows rules on Windows), starting the pager and waiting for it to exit. A missing default `less` is reported apart from other failures, so that `src/main.rs` can print to stdout instead |
| `src/print.rs` | Printing each input in turn, highlighted or plain, with headers and encoding handling. Writes no warnings: problems with an input (unreadable, highlighting given up) go to a callback, and `print_all` returns a `PrintOutcome` (whether an input failed, the write error that ended the output) |
| `src/stdout.rs` | Telling whether stdout was closed at startup, before std reopens it on `/dev/null` (Unix), or whether there is a stdout handle at all (Windows) |
| `src/test_support.rs` | Helpers for the unit tests only: a reader that returns its input in chunks of the given sizes, and a proptest strategy for those sizes |
| `build.rs` | Fixes the JavaScript and Scala syntaxes of two-face and bundles the syntaxes |

## Module dependencies

The arrows show which module uses which (`use crate::...`, not counting the tests). `src/main.rs` uses all of them and is left out.

```mermaid
flowchart LR
    cli --> assets
    cli --> config
    cli --> highlight
    cli --> lines
    cli --> color
    cli --> diag
    assets --> config
    assets --> diag
    config --> diag
    pager --> diag
    print --> highlight
    print --> lines
    print --> escape
    print --> encoding
    print --> color
    print --> diag
    highlight --> lines
    highlight --> encoding
    highlight --> color
    lines --> color
```

There are no cycles. `color`, `diag`, `encoding`, `escape`, `console` and `stdout` depend on no other module of `hl`, and `highlight` and `lines` do not depend on `diag`: they write nothing to stderr.

## Printing an input

`Printer::print` (`src/print.rs`) decides how one input is printed, following the second diagram in [Behavior details](SPEC.md#behavior-details):

1. When there is nothing to highlight and the escape sequences are kept, the input goes straight to `lines::write_plain`, which copies it like `cat`. This is the path of every run without colors, as when the output is a pipe.
2. Otherwise the first 8 KB (`PEEK_LEN`) are read in full. A pipe can return a few bytes per read, so deciding from the first read alone would give different output for the same bytes from a file and from a pipe.
3. From those bytes `encoding::utf16_bom` finds a UTF-16 BOM, and a NUL byte or invalid UTF-8 makes the input binary. The syntax is chosen from the path and the first line (`Printer::choose_syntax`, `Highlighter::detect`).
4. The bytes already read are put back in front of the rest of the input (`Cursor::chain`), so nothing is lost. UTF-16 is decoded by `encoding::Utf16Decoder`, and the escape sequences are removed by `escape::StripEscapes` when asked. Both are readers, so the input is never held in memory as a whole.
5. The result goes to `Highlighter::write`, or to `write_plain`.

`lines::LineState` carries line numbering, squeezing of blank lines and `--line-range` across the inputs, and both the highlighted and the plain path print each line through it.

## Highlighting pipeline

syntect can loop forever on some syntaxes and inputs, and a call into it cannot be interrupted. So `Highlighter::write` (`src/highlight.rs`) reads and prints the lines on the main thread, and highlights them on a thread of its own:

```mermaid
sequenceDiagram
    participant M as Main thread<br/>(Highlighter::write, Pipeline)
    participant W as Highlight thread<br/>(Worker)
    participant O as Output
    loop Until the input ends
        M->>M: Read a line (up to --max-line-length + 1 bytes)
        M->>W: A batch of up to 64 lines or 64 KB<br/>(at most 4 batches in flight)
        W->>W: Highlight each line,<br/>counting the lines done
        W-->>M: The escape sequences of the batch
        M->>O: Print the lines through LineState
    end
    alt No line finished for 3 seconds, or an error in the syntax
        M->>M: on_give_up(reason)
        M->>O: The pending lines and the rest of the input, plain
        Note over W: A stuck thread is left behind<br/>and keeps a CPU core busy until hl exits
    end
```

- Lines are handed over in batches (`BATCH_LINES`, `BATCH_BYTES`) because handing over each line costs more than highlighting it. `MAX_IN_FLIGHT` bounds how far the reading runs ahead, and so the memory used.
- The main thread waits for a batch with `recv_timeout`, ten times per `STALL_TIMEOUT` (3 seconds), and compares the count of lines done each time. Highlighting is given up only when no line has finished for the whole timeout, so a big batch of slow but progressing lines is not cut short.
- The syntaxes are `'static` (leaked by `main`) and the theme is shared through an `Arc`, because a stuck thread outlives the input it was started for.
- syntect's state carries over from line to line, so highlighting cannot resume after a line that was not highlighted. A line over `--max-line-length`, an input over `--max-size`, a line that is not UTF-8, an error in the syntax and a stuck line all switch the rest of the input to plain output.
- Lines that are not printed (before the range, squeezed) are still highlighted, so that the printed ones get the same colors. Reading stops at the end of the range.

## Problems, messages and exit status

All output on stderr goes through `src/diag.rs`, so the `hl: ` prefix and the escaping of control characters (`diag::escape_controls`, `diag::display_safe`, and `diag::warn`, which escapes the path and the message, as it may quote a key or a value of the config file) are done in one place, and a failure to write stderr is ignored. The modules that do the printing do not write messages themselves: they return what went wrong, and `src/main.rs` decides what it becomes.

```mermaid
flowchart TD
    giveup["highlight: on_give_up(reason)"] --> cannot["print: Problem::CannotHighlight"]
    unreadable["print: an input cannot be<br/>opened or read"] --> unread["print: Problem::Unreadable"]
    cannot --> report["main: report<br/>(callback of print_all)"]
    unread --> report
    report --> warn["diag::warn<br/>hl: PATH: reason"]
    unread --> failed["PrintOutcome.input_failed"]
    failed --> one["Exit status 1"]
    write["A write error"] --> outcome["PrintOutcome.write_error"]
    outcome --> check["main: check_write"]
    check -->|broken pipe| zero["Exit status 0"]
    check -->|other| note["diag::note<br/>hl: write error: reason"]
    note --> one
    usage["An invalid command line<br/>or HL_* variable"] --> clap["clap error"]
    clap --> two["Exit status 2"]
    config["An invalid entry in the<br/>config file, an unknown theme"] --> fallback["Warning and a fallback"]
    fallback --> zero2["Exit status 0"]
```

`print::TrackedWriter` remembers whether writing failed, so that an error coming out of `print_all` can be told apart from an error reading an input: after a write error there is no point in reading more inputs, while after a read error the next input is printed.

## Design rules

- **The environment is read at startup only.** `config::Dirs::from_env`, `config::Config::load` (`HL_CONFIG`), the `HL_*` variables in `cli` (through clap), `color::detect_from_env`, and `PAGER` and `LESS` in `main.rs` are all called from `main.rs` before printing starts. Everything else takes the values as arguments, so the tests can give any environment, and the developer's own environment does not leak into them (`cli::parse_without_env`, `Dirs::from_getenv`).
- **Plain output is `cat`'s output.** Without colors, and with the escape sequences kept, the bytes are copied as they are. No byte is replaced, not even invalid UTF-8 (the input goes through `String::from_utf8_lossy` only to detect the syntax from the first line). This is what makes `alias cat=hl` safe.
- **Modules return problems instead of printing them.** `highlight` and `print` pass the reason to a callback or return it in `PrintOutcome`; `main.rs` decides whether it is a warning or an error. For example, an unknown language in `--map-syntax` is an error, and in the configuration file a warning (`highlight::Origin`).
- **The arguments are read through methods.** The rest of the code does not read the raw fields of `cli::Args` but methods that combine related options, such as `numbering()`, `visibility()`, `limits()`, `use_color()` and `strip_escapes()`, so that a rule such as "`-b` overrides `-n`" or "`-v` turns off colors" is written once.
- **Only what is needed is loaded.** The syntaxes are loaded only when the output is colored or a language is named (`Args::needs_syntaxes`), and the theme only when the output is highlighted. A plain run, such as into a pipe, with no language named loads neither.
- **The bundled syntaxes are fixed at build time.** `build.rs` patches the JavaScript and Scala syntaxes of two-face and writes the syntax set that `assets::bundled_syntaxes` embeds. If an update of two-face changes them so that a fix no longer applies, `build.rs` fails the build with the name of the syntax, instead of silently shipping the hang again (see [DEV.md](DEV.md#dependencies)).
- **The cache of custom syntaxes knows when it is stale.** Its fingerprint (`assets::fingerprint`) holds the version of `hl`, a hash of the bundled syntaxes, and the paths, sizes and modification times of the custom syntaxes. A cache that does not match is not used, and a reminder to run `hl --build-cache` is printed.
- **A closed stdout is detected before `main`.** On Unix, std reopens a closed stdout on `/dev/null` before `main` runs, which would hide the error. `src/stdout.rs` checks fd 1 from a function run by the loader before that, and on Windows asks for the stdout handle (see the module documentation).
