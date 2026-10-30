# hl

[![CI](https://github.com/codemajin/hl/actions/workflows/ci.yml/badge.svg)](https://github.com/codemajin/hl/actions/workflows/ci.yml)

English | [日本語](README_ja.md)

`hl` is a `cat` replacement that detects the file type and prints the file with syntax highlighting. It is written in Rust.

- Detects the language from the file extension, the file name (`Makefile`, `Cargo.lock`, etc.), and the shebang.
- Uses the same syntax definitions and themes as [bat](https://github.com/sharkdp/bat) (via [two-face](https://crates.io/crates/two-face)), so TOML, TypeScript, Dockerfile and many more are supported.
- Writes the input unmodified when stdout is not a terminal, so aliasing `cat` to `hl` doesn't change what pipes and redirects produce.

## Requirements

- A Rust toolchain (Rust 1.88 or later, for edition 2024)
- `less` for the pager options (`--paging`, `-p`). If `$PAGER` is set, that pager is used instead.
- A terminal with 24-bit color (Windows Terminal, iTerm2, GNOME Terminal, WezTerm, etc.) or 256 color support

## Installation

```sh
git clone https://github.com/codemajin/hl.git
cd hl
cargo install --path .
```

By default this installs `hl` to `~/.cargo/bin/hl`. Make sure `~/.cargo/bin` is on your `PATH`.

To install somewhere else, pass `--root`. The binary goes to `<root>/bin/`.

```sh
cargo install --path . --root ~/.local      # → ~/.local/bin/hl
```

To update, pull the latest changes and run the same command again. It overwrites the existing binary.

### Uninstallation

```sh
cargo uninstall hl                  # add the same --root if you used one
```

## Usage

```sh
hl src/main.rs Cargo.toml        # highlight with automatic language detection
hl -n file.py                    # number lines (continuous across files)
hl --header src/*.rs             # print a header with the file name before each file
hl -n -r 120:160 src/main.rs     # print only lines 120 to 160, with their line numbers
cat script | hl -l bash          # read stdin with an explicit language
hl --map-syntax '*.conf:INI' app.conf  # highlight *.conf files as INI
hl --theme Nord file.rs          # choose a theme
hl --paging=auto big.rs          # use a pager only if the output does not fit on one screen
hl -p big.rs                     # always view through a pager
hl -A file.txt                   # show tabs, line ends and control characters, like cat -A
hl --list-themes                 # list themes
hl --list-languages              # list languages and their extensions
hl --build-cache                 # cache custom syntaxes so that they load fast
```

### Options

| Option | Description |
| --- | --- |
| `-n`, `--number` | Number output lines. Like `cat -n`, numbering continues across files |
| `-b`, `--number-nonblank` | Number non-empty output lines. Overrides `-n` |
| `-s`, `--squeeze-blank` | Squeeze repeated empty lines into one |
| `-r`, `--line-range <N:M>` | Print only lines N to M of each input. `N:` runs to the end, `:M` starts at line 1, and `N` prints a single line |
| `--header` | Print a `==> FILE <==` header before each input |
| `-v`, `--show-nonprinting` | Show control characters and bytes 128 and above in `^` and `M-` notation (except newline and tab) |
| `-E`, `--show-ends` | Print `$` at the end of each line |
| `-T`, `--show-tabs` | Print tabs as `^I` |
| `-A`, `--show-all` | Same as `-vET` |
| `-e` | Same as `-vE` |
| `-t` | Same as `-vT` |
| `-u` | Accepted for `cat` compatibility and ignored |
| `-l`, `--language <LANG>` | Use this language instead of auto-detection, given as a name or an extension (e.g. `rust`, `py`) |
| `--map-syntax <PATTERN:LANG>` | Use language LANG for files matching the glob PATTERN (e.g. `'*.conf:INI'`). LANG is a name or an extension, as with `-l`. A pattern without `/` matches the file name; a pattern with `/` matches the absolute path (`*` stays within one directory, `**` spans any number). Can be given more than once; when several mappings match, the last one wins. Standard input is never mapped |
| `--theme <THEME>` | Color theme (case-insensitive), bundled or custom. Defaults to `base16-ocean.dark`, which is also used with a warning if the theme is not found |
| `--max-size <SIZE>` | Print inputs larger than this without highlighting. Default `512K`. Accepts `K`, `M` and `G` (powers of 1024); `0` removes the limit |
| `--max-line-length <LEN>` | Stop highlighting at the first line longer than this many bytes (excluding the newline). Default `16K`; `0` removes the limit |
| `--paging <WHEN>` | When to show output in a pager: `auto`, `always`, or `never` (default). `auto` pages only when the output does not fit on one screen. The pager is `$PAGER`, or `less -RFX` (`auto`) / `less -RX` (`always`) if unset. No pager is started when stdout is not a terminal |
| `-p` | Same as `--paging=always` |
| `--color <WHEN>` | When to use colors: `auto`, `always`, or `never`. The default `auto` colors output only when stdout is a terminal |
| `--color-depth <DEPTH>` | Color depth: `auto`, `truecolor` (alias `24bit`), or `256`. The default `auto` uses 24-bit color when `COLORTERM` is `truecolor` or `24bit`, `TERM` ends in `-direct`, or `WT_SESSION` is set (Windows Terminal), and approximates colors to the 256 color palette otherwise |
| `--list-themes` | List available themes |
| `--list-languages` | List supported languages |
| `--build-cache` | Cache the custom syntaxes (see below) so that they load fast |
| `--clear-cache` | Remove the cache made by `--build-cache` |

With no file, or when the file is `-`, `hl` reads from standard input.

### Environment variables

| Variable | Description |
| --- | --- |
| `HL_THEME` | Default theme. `--theme` takes precedence |
| `HL_MAX_SIZE` | Default for `--max-size` |
| `HL_MAX_LINE_LENGTH` | Default for `--max-line-length` |
| `HL_COLOR_DEPTH` | Default for `--color-depth` |
| `HL_CONFIG` | Path of the configuration file |
| `PAGER` | Pager used by `--paging` and `-p` |

### Configuration file

Defaults for some options can be set in `$XDG_CONFIG_HOME/hl/config.toml` (`~/.config/hl/config.toml` when `XDG_CONFIG_HOME` is not set). Set `HL_CONFIG` to use another file. Keys are the long option names:

```toml
theme = "Monokai Extended"
color = "auto"            # auto, always, never
color-depth = "truecolor" # auto, truecolor, 256
paging = "auto"           # auto, always, never
max-size = "1M"           # integers such as 0 work too
max-line-length = "16K"
map-syntax = ["*.conf:INI", "Jenkinsfile:Groovy"]
```

Settings apply in this order, highest first: command-line options, environment variables, the configuration file, then the built-in defaults. `hl --help` shows the defaults with the configuration file applied. Without a configuration file, `hl` behaves as before. If the file can't be read or isn't valid TOML, `hl` prints a warning and ignores the whole file. Unknown keys and invalid values get a warning and are skipped one by one. Flags such as `-n` and `--header` can't be set here, because they could not be turned off from the command line.

`map-syntax` takes a list of `--map-syntax` values. Unlike the other keys, it does not supply a default that the command line replaces: `--map-syntax` adds to these mappings, and takes precedence over them when both match. An entry that is malformed or names an unknown language gets a warning and is skipped.

### Custom syntaxes and themes

Besides the bundled ones, `hl` uses Sublime Text syntaxes and TextMate themes placed in the config directory (`$XDG_CONFIG_HOME/hl`, or `~/.config/hl` when `XDG_CONFIG_HOME` is not set; `HL_CONFIG` does not move it):

```text
~/.config/hl/
├── syntaxes/   # *.sublime-syntax
└── themes/     # *.tmTheme
```

- A custom syntax is used like a bundled one: by its `file_extensions`, its `first_line_match`, `-l`, and `--map-syntax`. When a custom and a bundled syntax claim the same extension, the custom one wins.
- A theme is named after its file without the extension: `themes/My Theme.tmTheme` is `--theme 'My Theme'`. A custom theme replaces a bundled one with the same name.
- `--list-languages` and `--list-themes` include the custom ones.
- A file that can't be loaded gets a warning and is skipped. If the theme in use can't be loaded, `hl` falls back to `base16-ocean.dark`.

Merging custom syntaxes with the bundled ones takes a few hundred milliseconds, so cache the result:

```sh
hl --build-cache   # writes $XDG_CACHE_HOME/hl/syntaxes.bin (~/.cache/hl/syntaxes.bin)
hl --clear-cache   # removes it
```

Run `hl --build-cache` again after adding, changing or removing a syntax, and after upgrading `hl`. Until then, `hl` notices that the cache is out of date, merges the syntaxes on every run, and prints a reminder to stderr. Themes are read only when used and need no cache. Without custom syntaxes, `hl` neither reads nor needs the cache.

### Behavior details

- The language is determined in this order: `-l`, `--map-syntax` (and `map-syntax` in the configuration file), extension, file name, then the first line (shebang, etc.). If none match, the file is printed as plain text.
- If the first 8 KB of a file contain a NUL byte, the file is treated as binary and printed as-is without highlighting.
- Input that is not valid UTF-8, such as Shift_JIS or EUC-JP, is printed as-is without highlighting, from the first invalid line on (or entirely, if the first 8 KB are invalid), so no bytes are replaced. hl does not guess other encodings.
- UTF-16 files with a BOM are converted to UTF-8 and highlighted when output is colored. Without color they are printed as-is. A UTF-8 BOM is kept in the output and ignored for language detection.
- Highlighting only manages a few hundred KB per second, so hl limits it to stay fast on huge files and minified JS/CSS. A file larger than `--max-size` is not highlighted at all; for input of unknown size such as stdin, highlighting stops once the limit is reached. When a line longer than `--max-line-length` appears, highlighting stops from that line on. In both cases hl prints the same bytes as `cat`, without a warning.
- With `-v`, `-E`, `-T` (or `-A`, `-e`, `-t`), output is not highlighted, so it is identical to `cat`'s. `-n`, `-b` and `-s` work together with highlighting.
- With `--header`, each input, including a single file, starts with a `==> FILE <==` line (`==> standard input <==` for stdin), with a blank line between files. The header is bold when output is colored. Header lines are not numbered by `-n` or `-b`, and numbering continues across files as usual. A file that can't be opened gets no header. Without `--header`, the output is the same as `cat`'s.
- With `-r`, the lines before the range are still read and highlighted without being printed, so the printed lines get the same colors as when the whole file is shown, and reading stops after the range. The range applies to each input on its own: `-n` and `-b` show the line numbers of that input, counting the lines outside the range (and those squeezed by `-s`), and a file that ends without a newline inside the range gets one before the next input. Without `-r`, numbering continues across files as with `cat`.
- If a file can't be opened, `hl` prints `hl: <path>: <reason>` to stderr, processes the remaining files, and exits with status 1.
- If the output is closed early, as in `hl file | head`, `hl` exits successfully without an error.

## Using it as `cat`

Add an alias to your shell configuration file (`~/.bashrc`, `~/.zshrc`, etc.):

```sh
alias cat='hl'
```

To fix the theme, also set `HL_THEME` (or `theme` in the [configuration file](#configuration-file)):

```sh
export HL_THEME='Monokai Extended'
```

To run the real `cat` without going through the alias, use `\cat` or `command cat`.

## Development

```sh
cargo build
cargo test          # unit tests in src/ and integration tests in tests/cli.rs
cargo clippy
cargo fmt --check   # formatting (rustfmt.toml)
```

GitHub Actions runs `cargo fmt --check`, `cargo clippy` (warnings are errors) and `cargo test` on every push and pull request, on Linux and macOS with stable Rust and the minimum supported version (1.88). Formatting is checked with nightly rustfmt, because some options in `rustfmt.toml` are nightly-only; run `cargo +nightly fmt` to match it.

| File | Role |
| --- | --- |
| `src/main.rs` | Entry point: builds the printer from the arguments and config, picks the pager or stdout, handles errors and the exit code |
| `src/assets.rs` | Custom syntaxes and themes from the config directory, merged with the bundled ones, and the syntax cache (`--build-cache`/`--clear-cache`) |
| `src/cli.rs` | Command-line argument definitions (clap) and defaults from the config file |
| `src/color.rs` | Terminal color depth detection and the 256-color fallback |
| `src/config.rs` | Config file loading and the config/cache directory paths |
| `src/diag.rs` | Warning and error messages on stderr |
| `src/encoding.rs` | UTF-8/UTF-16 BOM detection and UTF-16 decoding |
| `src/highlight.rs` | Language detection, `--map-syntax` mappings, size limits, and highlighted output (syntect) |
| `src/lines.rs` | Line numbering, blank-line squeezing, and `cat -v`/`-E`/`-T` notation |
| `src/pager.rs` | Starting the pager and waiting for it to exit |
| `src/print.rs` | Printing each input in turn, highlighted or plain, with headers and encoding handling |

### Test coverage

Coverage is measured with [cargo-llvm-cov](https://github.com/taiki-e/cargo-llvm-cov). It also covers the `hl` binary that the integration tests start as a child process. Install it once:

```sh
rustup component add llvm-tools-preview
cargo install cargo-llvm-cov --locked
```

```sh
cargo llvm-cov          # run the tests and print a per-file summary
cargo llvm-cov --html   # write an HTML report to target/llvm-cov/html/index.html
                        # (add --open to open it in a browser)
```
