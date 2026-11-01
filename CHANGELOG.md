# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `--completions <SHELL>` to print a completion script (bash, zsh, fish, elvish, PowerShell) and `--man` to print the man page.
- Completion scripts for bash, zsh and fish and the man page in the release archives.
- `-H`/`--highlight-line` to show some lines with a background color.
- `--strip-ansi` (`auto`, `always`, `never`) to choose when escape sequences are removed from the input.
- `number`, `number-nonblank`, `squeeze-blank` and `header` in the config file, and `--no-number`, `--no-number-nonblank`, `--no-squeeze-blank` and `--no-header` to turn them off for one run.

### Changed

- When output is colored, escape sequences in the input (colors, OSC, ...) are removed by default, so that they no longer break the highlighting or control the terminal. Uncolored output is unchanged.
- The removal also covers the C1 control characters encoded in UTF-8 (U+0080–U+009F, such as the 8-bit CSI and OSC), and UTF-16 input, which is converted to UTF-8 for it even when it is not highlighted (over `--max-size`, or with `--strip-ansi=always` and no color).
- An invalid value in `HL_MAX_SIZE`, `HL_MAX_LINE_LENGTH` or `HL_COLOR_DEPTH` is reported by the name of the variable, such as `error: invalid value 'abc' for HL_MAX_SIZE: ...`, instead of the option the user did not write. It is still an error with exit status 2. The README now describes this and what exit statuses 0, 1 and 2 mean.

### Fixed

- `PAGER=less` (without arguments, and with `LESS` unset) now shows the colors instead of raw escape sequences, and `--paging=auto` no longer pages output that fits on one screen: `less` gets the same `-RX` / `-RFX` as when `PAGER` is unset. `less` with arguments of its own, or with `LESS` set, is run as given.
- `$PAGER` is split into words like the shell does, so quotes work (`PAGER="sh -c 'less -R'"`). An unclosed quote is reported as `hl: invalid PAGER: missing closing quote`.
- A pager that fails to start is reported by name and without the OS error code, such as `hl: failed to start pager 'less': No such file or directory`.
- JavaScript no longer hangs on a comment after an expression, such as `x /*`, also inside HTML, Markdown, Vue, Svelte and QML. The bundled JavaScript syntax carries a fix for it until it is fixed upstream.
- A line that takes longer than 3 seconds to highlight no longer freezes `hl`: it warns and prints the rest of the input without highlighting.
- `hl` no longer panics (exit status 101) when stderr can't be written, such as a pipe closed by the reader. The message is dropped, and the output and the exit status stay the same; before, a warning about the config file stopped `hl` before it printed anything.
- With `--header`, a file that does not end with a newline is now followed by the blank line before the next header in every mode. Before, the blank line was missing without options and with `--strip-ansi=always`.
- An invalid size for `--max-size` or `--max-line-length`, such as `-1`, `1.5K` or `1KB`, is reported as `not a size: use a whole number with an optional K, M or G suffix (e.g. 512K)` instead of a misleading `invalid unit '.5K'`, and a number too large for 64 bits as `too large`.
- A closed stdout (`hl file >&-`) is reported as `hl: write error: Bad file descriptor` with exit status 1, like `cat`, instead of throwing the output away and exiting with 0.

### Security

- Control characters in file names, theme names and error messages are shown as `\x1b` and the like in headers and on stderr, so that a crafted file name cannot control the terminal.

## [0.1.0] - 2026-10-30

First release.

### Added

- Syntax highlighting with the language detected from the file extension, the file name and the first line (shebang, etc.), using the syntaxes and themes of bat (via two-face).
- Unmodified output when stdout is not a terminal, so `hl` can stand in for `cat` in pipes and redirects.
- `cat` options `-n`, `-b`, `-s`, `-v`, `-E`, `-T`, `-A`, `-e`, `-t` and `-u`.
- `-l`/`--language` to choose the language, and `--map-syntax` to map glob patterns to languages.
- `--theme`, with a fallback to `base16-ocean.dark` and a warning when the theme is not found.
- `--header` to print the file name before each input.
- `-r`/`--line-range` to print only some lines.
- `--paging` (`auto`, `always`, `never`) and `-p`.
- `--color` and `--color-depth`, with 24-bit color detection and a fallback to 256 colors.
- `--max-size` and `--max-line-length` to stop highlighting huge files and very long lines.
- Output of non-UTF-8 input without changing any bytes, and highlighting of UTF-16 files with a BOM.
- Configuration file (`~/.config/hl/config.toml`) and the `HL_*` environment variables.
- Custom syntaxes and themes from `~/.config/hl`, with `--build-cache` and `--clear-cache`.
- Prebuilt binaries for Linux and macOS (x86_64 and aarch64) on GitHub Releases.

[Unreleased]: https://github.com/codemajin/hl/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/codemajin/hl/releases/tag/v0.1.0
