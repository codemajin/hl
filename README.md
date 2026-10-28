# hl

English | [日本語](README_ja.md)

`hl` is a `cat` replacement that detects the file type and prints the file with syntax highlighting. It is written in Rust.

- Detects the language from the file extension, the file name (`Makefile`, `Cargo.lock`, etc.), and the shebang.
- Uses the same syntax definitions and themes as [bat](https://github.com/sharkdp/bat) (via [two-face](https://crates.io/crates/two-face)), so TOML, TypeScript, Dockerfile and many more are supported.
- Writes the input unmodified when stdout is not a terminal, so aliasing `cat` to `hl` doesn't change what pipes and redirects produce.

## Requirements

- A Rust toolchain (Rust 1.85 or later, for edition 2024)
- `less` for the pager option (`-p`). If `$PAGER` is set, that pager is used instead.
- A terminal with 24-bit color support (Windows Terminal, iTerm2, GNOME Terminal, WezTerm, etc.)

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
cat script | hl -l bash          # read stdin with an explicit language
hl --theme Nord file.rs          # choose a theme
hl -p big.rs                     # view through a pager
hl -A file.txt                   # show tabs, line ends and control characters, like cat -A
hl --list-themes                 # list themes
hl --list-languages              # list languages and their extensions
```

### Options

| Option | Description |
| --- | --- |
| `-n`, `--number` | Number output lines. Like `cat -n`, numbering continues across files |
| `-b`, `--number-nonblank` | Number non-empty output lines. Overrides `-n` |
| `-s`, `--squeeze-blank` | Squeeze repeated empty lines into one |
| `-v`, `--show-nonprinting` | Show control characters and bytes 128 and above in `^` and `M-` notation (except newline and tab) |
| `-E`, `--show-ends` | Print `$` at the end of each line |
| `-T`, `--show-tabs` | Print tabs as `^I` |
| `-A`, `--show-all` | Same as `-vET` |
| `-e` | Same as `-vE` |
| `-t` | Same as `-vT` |
| `-u` | Accepted for `cat` compatibility and ignored |
| `-l`, `--language <LANG>` | Use this language instead of auto-detection, given as a name or an extension (e.g. `rust`, `py`) |
| `--theme <THEME>` | Color theme (case-insensitive). Defaults to `base16-ocean.dark`, which is also used with a warning if the theme is not found |
| `--max-size <SIZE>` | Print inputs larger than this without highlighting. Default `512K`. Accepts `K`, `M` and `G` (powers of 1024); `0` removes the limit |
| `--max-line-length <LEN>` | Stop highlighting at the first line longer than this many bytes (excluding the newline). Default `16K`; `0` removes the limit |
| `-p`, `--paging` | Show output in a pager (`$PAGER`, or `less -RFX` if unset). Exits right away if the output fits on one screen |
| `--color <WHEN>` | When to use colors: `auto`, `always`, or `never`. The default `auto` colors output only when stdout is a terminal |
| `--list-themes` | List available themes |
| `--list-languages` | List supported languages |

With no file, or when the file is `-`, `hl` reads from standard input.

### Environment variables

| Variable | Description |
| --- | --- |
| `HL_THEME` | Default theme. `--theme` takes precedence |
| `HL_MAX_SIZE` | Default for `--max-size` |
| `HL_MAX_LINE_LENGTH` | Default for `--max-line-length` |
| `PAGER` | Pager used by `-p` |

### Behavior details

- The language is determined in this order: `-l`, extension, file name, then the first line (shebang, etc.). If none match, the file is printed as plain text.
- If the first 8 KB of a file contain a NUL byte, the file is treated as binary and printed as-is without highlighting.
- Input that is not valid UTF-8, such as Shift_JIS or EUC-JP, is printed as-is without highlighting, from the first invalid line on (or entirely, if the first 8 KB are invalid), so no bytes are replaced. hl does not guess other encodings.
- UTF-16 files with a BOM are converted to UTF-8 and highlighted when output is colored. Without color they are printed as-is. A UTF-8 BOM is kept in the output and ignored for language detection.
- Highlighting only manages a few hundred KB per second, so hl limits it to stay fast on huge files and minified JS/CSS. A file larger than `--max-size` is not highlighted at all; for input of unknown size such as stdin, highlighting stops once the limit is reached. When a line longer than `--max-line-length` appears, highlighting stops from that line on. In both cases hl prints the same bytes as `cat`, without a warning.
- With `-v`, `-E`, `-T` (or `-A`, `-e`, `-t`), output is not highlighted, so it is identical to `cat`'s. `-n`, `-b` and `-s` work together with highlighting.
- If a file can't be opened, `hl` prints `hl: <path>: <reason>` to stderr, processes the remaining files, and exits with status 1.
- If the output is closed early, as in `hl file | head`, `hl` exits successfully without an error.

## Using it as `cat`

Add an alias to your shell configuration file (`~/.bashrc`, `~/.zshrc`, etc.):

```sh
alias cat='hl'
```

To fix the theme, also set `HL_THEME`:

```sh
export HL_THEME='Monokai Extended'
```

To run the real `cat` without going through the alias, use `\cat` or `command cat`.

## Development

```sh
cargo build
cargo test          # integration tests in tests/cli.rs
cargo clippy
cargo fmt --check   # formatting (rustfmt.toml)
```

| File | Role |
| --- | --- |
| `src/main.rs` | Entry point: input dispatch, output selection, error handling |
| `src/cli.rs` | Command-line argument definitions (clap) |
| `src/highlight.rs` | Language detection and highlighted output (syntect) |
| `src/lines.rs` | Line numbering, blank-line squeezing, and `cat -v`/`-E`/`-T` notation |
| `src/pager.rs` | Starting the pager and waiting for it to exit |
