# hl

[![CI](https://github.com/codemajin/hl/actions/workflows/ci.yml/badge.svg)](https://github.com/codemajin/hl/actions/workflows/ci.yml)

English | [日本語](README_ja.md)

`hl` is a `cat` replacement that detects the file type and prints the file with syntax highlighting. It is written in Rust.

- Detects the language from the file extension, the file name (`Makefile`, `Cargo.lock`, etc.), and the shebang.
- Uses the same syntax definitions and themes as [bat](https://github.com/sharkdp/bat) (via [two-face](https://crates.io/crates/two-face)), so TOML, TypeScript, Dockerfile and many more are supported.
- Writes the input unmodified when stdout is not a terminal, so aliasing `cat` to `hl` doesn't change what pipes and redirects produce.

## Requirements

- To build from source: a Rust toolchain (Rust 1.88 or later, for edition 2024)
- `less` for the pager options (`--paging`, `-p`). If `$PAGER` is set, that pager is used instead.
- A terminal with 24-bit color (Windows Terminal, iTerm2, GNOME Terminal, WezTerm, etc.) or 256 color support

## Installation

### Prebuilt binaries

Each [release](https://github.com/codemajin/hl/releases) has binaries for these platforms:

| Platform | Target |
| --- | --- |
| Linux x86_64 | `x86_64-unknown-linux-musl` |
| Linux aarch64 | `aarch64-unknown-linux-musl` |
| macOS Intel | `x86_64-apple-darwin` |
| macOS Apple silicon | `aarch64-apple-darwin` |

The Linux binaries are statically linked, so they run on any distribution. To download, verify and install one to `~/.local/bin`:

```sh
VERSION=v0.1.0                      # the release to install
TARGET=x86_64-unknown-linux-musl    # your platform, from the table above
NAME=hl-$VERSION-$TARGET
curl -fLO "https://github.com/codemajin/hl/releases/download/$VERSION/$NAME.tar.gz"
curl -fLO "https://github.com/codemajin/hl/releases/download/$VERSION/$NAME.tar.gz.sha256"
shasum -a 256 -c "$NAME.tar.gz.sha256"
tar xzf "$NAME.tar.gz"
install -d ~/.local/bin
install "$NAME/hl" ~/.local/bin/hl
```

Make sure `~/.local/bin` is on your `PATH`. To update, run the same commands with the new version.

On macOS, a file downloaded with a web browser is quarantined and blocked by Gatekeeper. If that happens, remove the attribute with `xattr -d com.apple.quarantine ~/.local/bin/hl`. Files downloaded with `curl` are not affected.

To uninstall, remove the binary:

```sh
rm ~/.local/bin/hl
```

### From source

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

To uninstall:

```sh
cargo uninstall hl                  # add the same --root if you used one
```

### Shell completions and man page

The release archives include completion scripts for bash, zsh and fish in `completions/`, and the man page in `man/hl.1`. `hl` can also print them itself with `hl --completions <SHELL>` and `hl --man`, which is the way to get them when installing from source. To install them for your user:

```sh
# bash (needs the bash-completion package)
mkdir -p ~/.local/share/bash-completion/completions
hl --completions bash > ~/.local/share/bash-completion/completions/hl

# zsh: also add `fpath=(~/.zfunc $fpath)` to ~/.zshrc, before `compinit`
mkdir -p ~/.zfunc
hl --completions zsh > ~/.zfunc/_hl

# fish
mkdir -p ~/.config/fish/completions
hl --completions fish > ~/.config/fish/completions/hl.fish

# man page
mkdir -p ~/.local/share/man/man1
hl --man > ~/.local/share/man/man1/hl.1
```

With a release archive, you can instead copy `$NAME/completions/hl.bash`, `$NAME/completions/_hl`, `$NAME/completions/hl.fish` and `$NAME/man/hl.1` to the same places. Start a new shell to load the completions. If `man hl` doesn't find the page, add `~/.local/share/man` to `MANPATH`. Regenerate the files after updating `hl` so that they cover new options, and remove them when uninstalling.

## Usage

```sh
hl src/main.rs Cargo.toml        # highlight with automatic language detection
hl -n file.py                    # number lines (continuous across files)
hl --header src/*.rs             # print a header with the file name before each file
hl -n -r 120:160 src/main.rs     # print only lines 120 to 160, with their line numbers
hl -r 110:130 -H 120 src/main.rs # print lines 110 to 130 and highlight line 120
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
| `-H`, `--highlight-line <N:M>` | Show lines N to M of each input with the background color of the theme's line highlight. Takes the same forms as `-r` and can be given more than once. The line number from `-n` or `-b` is highlighted too, but not the rest of the line beyond its text. Only applies when the output is colored, so it does not change plain output |
| `--header` | Print a `==> FILE <==` header before each input |
| `--no-number`, `--no-number-nonblank`, `--no-squeeze-blank`, `--no-header` | Turn off `-n`, `-b`, `-s` or `--header` set in the [configuration file](#configuration-file). Of a flag and its `--no-*` option, the one given last wins |
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
| `--max-size <SIZE>` | Print inputs larger than this without highlighting. Default `512K`. A whole number of bytes, optionally followed by `K`, `M` or `G` (powers of 1024, either case), such as `1048576` or `1M`; negative numbers and fractions such as `1.5K` are rejected. `0` removes the limit |
| `--max-line-length <LEN>` | Stop highlighting at the first line longer than this many bytes (excluding the newline). Default `16K`. Written like `--max-size`; `0` removes the limit |
| `--paging <WHEN>` | When to show output in a pager: `auto`, `always`, or `never` (default). `auto` pages only when the output does not fit on one screen. The pager is `$PAGER`, or `less -RFX` (`auto`) / `less -RX` (`always`) if unset. `$PAGER` is split into words like the shell does, so quotes work (`PAGER="less -R -S"`, `PAGER="'/opt/my pager/pg'"`). A bare `less` in `$PAGER` (no arguments) gets the same `-RFX` / `-RX` unless `LESS` is set; otherwise the options of `$PAGER` and `LESS` are used as they are, so include `-R` to see the colors. No pager is started when stdout is not a terminal |
| `-p` | Same as `--paging=always` |
| `--color <WHEN>` | When to use colors: `auto`, `always`, or `never`. The default `auto` colors output only when stdout is a terminal |
| `--color-depth <DEPTH>` | Color depth: `auto`, `truecolor` (alias `24bit`), or `256`. The default `auto` uses 24-bit color when `COLORTERM` is `truecolor` or `24bit`, `TERM` ends in `-direct`, or `WT_SESSION` is set (Windows Terminal), and approximates colors to the 256 color palette otherwise |
| `--strip-ansi <WHEN>` | When to remove escape sequences (colors, window titles, hyperlinks, ...) from the input: `auto`, `always`, or `never`. The default `auto` removes them only when output is colored; `always` also removes them from uncolored output; `never` prints them as they are (see [Behavior details](#behavior-details)) |
| `--list-themes` | List available themes |
| `--list-languages` | List supported languages |
| `--build-cache` | Cache the custom syntaxes (see below) so that they load fast |
| `--clear-cache` | Remove the cache made by `--build-cache` |
| `--completions <SHELL>` | Print the completion script for `bash`, `zsh`, `fish`, `elvish` or `powershell` (see [Shell completions and man page](#shell-completions-and-man-page)) |
| `--man` | Print the man page in roff format |

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
| `LESS` | Options for `less`. When set, hl adds no flags to a bare `less` in `$PAGER` |

An invalid value in `HL_MAX_SIZE`, `HL_MAX_LINE_LENGTH` or `HL_COLOR_DEPTH` is an error, as on the command line: `hl` prints a message naming the variable, such as `error: invalid value 'abc' for HL_MAX_SIZE: ...`, and exits with status 2 without printing anything. This differs from the configuration file, whose invalid values get a warning and are skipped. An option given on the command line replaces the variable, so its value is then not checked. An unknown theme in `HL_THEME` only gets a warning, and the default theme is used, as with `--theme`.

### Configuration file

Defaults for some options can be set in `$XDG_CONFIG_HOME/hl/config.toml` (`~/.config/hl/config.toml` when `XDG_CONFIG_HOME` is not set). Set `HL_CONFIG` to use another file. Keys are the long option names:

```toml
theme = "Monokai Extended"
color = "auto"            # auto, always, never
color-depth = "truecolor" # auto, truecolor, 256
strip-ansi = "auto"       # auto, always, never
paging = "auto"           # auto, always, never
max-size = "1M"           # integers such as 0 work too
max-line-length = "16K"
number = true             # also number-nonblank, squeeze-blank, header
map-syntax = ["*.conf:INI", "Jenkinsfile:Groovy"]
```

Settings apply in this order, highest first: command-line options, environment variables, the configuration file, then the built-in defaults. `hl --help` shows the defaults with the configuration file applied. Without a configuration file, `hl` behaves as before. If the file can't be read or isn't valid TOML, `hl` prints a warning and ignores the whole file. Unknown keys and invalid values get a warning and are skipped one by one. The flags `number`, `number-nonblank`, `squeeze-blank` and `header` take `true` or `false`; a flag turned on here can be turned off for one run with `--no-number`, `--no-number-nonblank`, `--no-squeeze-blank` or `--no-header`. `-b` still takes precedence over `-n` wherever each comes from.

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
- If the first 8 KB of a file contain a NUL byte, the file is treated as binary and printed without highlighting (as-is, except for the escape sequences removed as described below).
- Input that is not valid UTF-8, such as Shift_JIS or EUC-JP, is printed as-is without highlighting, from the first invalid line on (or entirely, if the first 8 KB are invalid), so no bytes are replaced. hl does not guess other encodings.
- UTF-16 files with a BOM are converted to UTF-8 when output is colored or escape sequences are removed, and highlighted when output is colored and the file is within `--max-size`. Without color and with escape sequences kept, they are printed as-is. A UTF-8 BOM is kept in the output and ignored for language detection.
- Highlighting only manages a few hundred KB per second, so hl limits it to stay fast on huge files and minified JS/CSS. A file larger than `--max-size` is not highlighted at all; for input of unknown size such as stdin, highlighting stops once the limit is reached. When a line longer than `--max-line-length` appears, highlighting stops from that line on. In both cases hl prints the same bytes as `cat`, without a warning.
- If highlighting a line takes longer than 3 seconds, hl warns (`cannot highlight: a line took longer than 3s, printing the rest as plain text`) and prints that line and the rest of the input without highlighting, as it does after an error in a syntax. This happens when a syntax definition makes the highlighter loop forever on some input, as the Scala syntax does on `:a@<`. The stuck highlighting runs on a thread of its own that cannot be stopped, so it keeps a CPU core busy until hl exits, also while a pager shows the output.
- The bundled JavaScript syntax, also used for JavaScript inside HTML, Markdown, Vue, Svelte and QML, has a fix on top of bat's: a comment after an expression, as in `x /*`, made it loop forever.
- When output is colored, escape sequences in the input are removed, so that they neither mix with the highlight colors nor control the terminal (window title, hyperlinks, clipboard, ...). This covers CSI (`ESC [ ... m`, etc.), OSC, DCS and the other strings ended by BEL or ST, and the other sequences starting with ESC, as well as the C1 control characters encoded in UTF-8 (U+0080–U+009F; the 8-bit CSI U+009B and OSC U+009D start a sequence like `ESC [` and `ESC ]`), also in UTF-16 input after conversion and in the parts printed without highlighting (binary files, input past `--max-size` or `--max-line-length`, lines after one that is not valid UTF-8). A string without its terminator ends at the end of the line, and a sequence cut off by the end of the input is dropped. Without color, the output is the same as `cat`'s. `--strip-ansi=always` removes them from uncolored output too, and `--strip-ansi=never` keeps them. Carriage returns (`\r`) are kept as with `cat`, so a line can still be overwritten on the terminal.
- Control characters in file names and theme names are shown as `\x1b` and the like in headers and messages, so a crafted name cannot control the terminal.
- With `-v`, `-E`, `-T` (or `-A`, `-e`, `-t`), output is not highlighted, so it is identical to `cat`'s. `-n`, `-b` and `-s` work together with highlighting.
- With `--header`, each input, including a single file, starts with a `==> FILE <==` line (`==> standard input <==` for stdin), with a blank line between files. The header is bold when output is colored. Header lines are not numbered by `-n` or `-b`, and numbering continues across files as usual. A file that can't be opened gets no header. Without `--header`, the output is the same as `cat`'s.
- With `-r`, the lines before the range are still read and highlighted without being printed, so the printed lines get the same colors as when the whole file is shown, and reading stops after the range. The range applies to each input on its own: `-n` and `-b` show the line numbers of that input, counting the lines outside the range (and those squeezed by `-s`), and a file that ends without a newline inside the range gets one before the next input. Without `-r`, numbering continues across files as with `cat`.
- If a file can't be opened, `hl` prints `hl: <path>: <reason>` to stderr, processes the remaining files, and exits with status 1.
- If the output is closed early, as in `hl file | head`, `hl` exits successfully without an error.
- If the output can't be written, as when stdout is closed (`hl file >&-`) or the disk is full, `hl` prints `hl: write error: <reason>` to stderr and exits with status 1, as `cat` does. This also applies to `--list-themes`, `--man` and the other commands that print something and exit.
- If stderr can't be written, as when it is a pipe closed by the reader, the messages are dropped; the output and the exit status stay the same.
- The exit status is 0 on success, also when `hl` warns and goes on with a fallback (an unknown theme, an invalid entry in the configuration file, a line that takes too long to highlight); 1 when something fails while running (a file that can't be opened, an unknown language given to `-l` or `--map-syntax`, a write error, a pager that fails to start); and 2 when the command line, or an environment variable standing in for an option, is rejected before anything runs (an unknown option, an invalid value such as `--max-size=1.5K` or `--color=sometimes`).

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
cargo test          # unit tests in src/ and integration tests in tests/cli/ (one file per feature)
cargo clippy
cargo fmt --check   # formatting (rustfmt.toml)
```

GitHub Actions runs `cargo fmt --check`, `cargo clippy` (warnings are errors) and `cargo test` on every push and pull request, on Linux and macOS with stable Rust and the minimum supported version (1.88). The tests spell out the output they expect from the `cat` options. One more test compares `hl` with GNU coreutils `cat` 9.0 or later, found as `gcat` or `cat`, and is skipped with a message when there is none; on macOS, `brew install coreutils` provides `gcat`. The tests ignore the `HL_*` variables, `HL_CONFIG` and the XDG directories, so your own settings, config file and custom syntaxes don't affect them. Formatting is checked with nightly rustfmt, because some options in `rustfmt.toml` are nightly-only; run `cargo +nightly fmt` to match it. Clippy runs on a fixed Rust version (the `clippy` jobs in `.github/workflows/ci.yml` and `release.yml`), so that lints added in a new Rust release don't fail CI by themselves; raise it by hand in both files, and fix the new warnings in the same change.

| File | Role |
| --- | --- |
| `src/main.rs` | Entry point: runs the commands that print no inputs (`--list-themes`, ...), builds the printer and the line state from the arguments and config, picks the pager or stdout, handles errors and the exit code. The only place that reads the environment (`PAGER`, `LESS`, the color depth) |
| `src/assets.rs` | The bundled syntaxes, custom syntaxes and themes from the config directory, merged with the bundled ones, and the syntax cache (`--build-cache`/`--clear-cache`) |
| `src/cli.rs` | Command-line argument definitions (clap) and defaults from the config file |
| `src/color.rs` | Terminal color depth detection and the 256-color fallback |
| `src/config.rs` | Config file loading and the config/cache directory paths |
| `src/diag.rs` | Warning and error messages on stderr |
| `src/encoding.rs` | UTF-8/UTF-16 BOM detection and UTF-16 decoding |
| `build.rs` | Fixes the JavaScript syntax of two-face and bundles the syntaxes |
| `src/highlight.rs` | Language detection, `--map-syntax` mappings, size limits, and highlighted output (syntect, on a thread of its own with a time limit) |
| `src/lines.rs` | Line numbering, blank-line squeezing, and `cat -v`/`-E`/`-T` notation |
| `src/pager.rs` | Building the pager command from `$PAGER`, starting the pager and waiting for it to exit |
| `src/print.rs` | Printing each input in turn, highlighted or plain, with headers and encoding handling |
| `src/stdout.rs` | Telling whether stdout was closed at startup, before std reopens it on `/dev/null` |

### Dependencies

Dependabot (`.github/dependabot.yml`) opens update PRs every week, for the crates and for the GitHub Actions. Minor and patch updates of the crates come in one PR, a major update in a PR of its own, and the Actions in one PR.

The workflows pin each action to a commit SHA, with the version in a comment (`uses: actions/checkout@<sha> # v7.0.1`), since a tag can be moved to other code; Dependabot updates the SHA and the comment together. When adding an action, pin it the same way. The workflows get a read-only `GITHUB_TOKEN` (only the job that publishes a release can write), and `actions/checkout` runs with `persist-credentials: false`, so the token isn't left in `.git/config` for the build to read.

[cargo-deny](https://github.com/EmbarkStudios/cargo-deny) (`.github/workflows/deny.yml`) checks the dependencies on every push and pull request: known vulnerabilities and yanked versions (advisories), licenses, bans and sources (crates.io only). The advisories are also checked every day, since new ones appear while the dependencies stay the same. The allowed licenses, and the advisories ignored for now with the reason, are listed in `deny.toml`. To run it locally:

```sh
cargo install cargo-deny --locked
cargo deny check
```

The syntaxes come from two-face, but `build.rs` fixes its JavaScript syntax at build time (see Behavior details). If an update of two-face changes that syntax so that the fix no longer applies, the build fails. Then check whether `printf 'x /*' | hl --color=always -l js` still hangs with two-face's own syntaxes (for example by having `assets::bundled_syntaxes` return `two_face::syntax::extra_newlines()`), and update `build.rs`, or remove it once the fix is upstream.

The minimum supported Rust version (MSRV, `rust-version` in `Cargo.toml`) may be raised when an update needs it, to a version at least six releases behind the latest stable; to get a fix for a vulnerability, it is always raised. Such an update fails the CI job with the MSRV. When raising it, update `rust-version` in `Cargo.toml`, the matrix in `.github/workflows/ci.yml`, the version in this README (Requirements and Development) and `CHANGELOG.md` together.

### Releasing

Versions follow [Semantic Versioning](https://semver.org/), and a release is tagged `vX.Y.Z` with the version in `Cargo.toml`. Changes are recorded in [CHANGELOG.md](CHANGELOG.md).

1. Set `version` in `Cargo.toml` and run `cargo build` to update `Cargo.lock`.
2. In `CHANGELOG.md`, move the entries under `Unreleased` to a new `## [X.Y.Z] - YYYY-MM-DD` section, and update the links at the end.
3. Commit, then push the tag:

   ```sh
   git tag vX.Y.Z
   git push origin vX.Y.Z
   ```

`.github/workflows/release.yml` then builds the binaries for the four platforms, packs each with the completion scripts and the man page generated by `hl` itself, and publishes them, with SHA-256 checksums, as a GitHub release whose notes are the changelog section. It fails without publishing anything if the tag doesn't match the version in `Cargo.toml` or the changelog has no section for it. Before the builds it runs the tests (on Linux and macOS) and clippy, since it doesn't wait for the CI workflow, and publishes nothing if they fail. Nothing in it uses a build cache, so the release is built from the tagged source only.

Anyone who can push a `v*` tag can publish a release. To limit that, add a tag ruleset in the repository settings (Settings → Rules → Rulesets) that targets `v*` and restricts creating, updating and deleting the tags to the maintainers.

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
