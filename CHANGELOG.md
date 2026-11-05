# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Windows (x86_64) support: CI runs the tests on Windows, and each release has a zip for `x86_64-pc-windows-msvc`. The README describes installing it with PowerShell.
- The PowerShell completion script (`completions/_hl.ps1`) in every release archive.
- On Windows, the config directory is `%APPDATA%\hl` and the cache directory `%LOCALAPPDATA%\hl` when `XDG_CONFIG_HOME` / `XDG_CACHE_HOME` is not set; `~/.config` and `~/.cache` are used only without them.
- On Windows, colored output turns on escape sequence processing in the console, so that the colors show in cmd.exe and PowerShell outside Windows Terminal too.
- On Windows, starting without a stdout handle is a write error (exit status 1), as a closed stdout is on Unix, instead of discarding the output.

### Changed

- When `PAGER` is unset or blank and `less` is not installed, `--paging` and `-p` print the output without a pager and warn (`hl: less not found, printing without a pager (set PAGER to choose one)`) instead of failing. A pager set in `PAGER` that fails to start is still an error.
- The README covers installation and a quick start only. The specification, a new design document and the development guide are in `docs/` (`SPEC.md`, `DESIGN.md`, `DEV.md`, each also in Japanese), which every release archive includes.

### Fixed

- Input that arrives a little at a time, such as from a pipe, is no longer judged by the first read alone. hl reads the first 8 KB (or up to the end, if shorter) before telling binary and invalid UTF-8, a UTF-16 BOM and the shebang, so a NUL on the second line, a BOM split across reads, or a shebang cut off midway gives the same output as from a file.
- A line is no longer lost when highlighting is given up (after an error in the syntax or a line that takes longer than 3 seconds) just before a line that goes over `--max-size` or `--max-line-length` or is not UTF-8. That line, or the part of it already read, is printed with the rest.
- Scala no longer hangs on an operator right after `@` in a type, such as `type L = A @> B` or `x: A @< B`, which printed the rest of the input without highlighting after 3 seconds and kept a CPU core busy. The bundled Scala syntax carries a fix for it until it is fixed upstream.
- On Windows, `PAGER` is split by Windows rules: backslashes are kept, and double quotes group words (`PAGER='"C:\Program Files\Git\usr\bin\less.exe" -R'`). A bare `less.exe` gets `-RX` / `-RFX` like `less`.
- On Windows, a `--map-syntax` pattern with `\` (such as `**\conf\*.cfg:INI`) matches the path, like one with `/`.
- On Windows, a directory given as an input is reported as `Is a directory`, as on Unix, instead of `Access is denied.`.

## [0.2.0] - 2026-11-03

### Added

- `--completions <SHELL>` to print a completion script (bash, zsh, fish, elvish, PowerShell) and `--man` to print the man page.
- Completion scripts for bash, zsh and fish and the man page in the release archives.
- `-H`/`--highlight-line` to show some lines with a background color.
- `--strip-ansi` (`auto`, `always`, `never`) to choose when escape sequences are removed from the input.
- `number`, `number-nonblank`, `squeeze-blank` and `header` in the config file, and `--no-number`, `--no-number-nonblank`, `--no-squeeze-blank` and `--no-header` to turn them off for one run.

### Changed

- When output is colored, escape sequences in the input (colors, OSC, ...) are removed by default, so that they no longer break the highlighting or control the terminal. Uncolored output is unchanged.
- The removal also covers the C1 control characters encoded in UTF-8 (U+0080–U+009F, such as the 8-bit CSI and OSC), including one formed when removing a sequence leaves a stray `0xC2` byte next to the byte after it, and UTF-16 input, which is converted to UTF-8 for it even when it is not highlighted (over `--max-size`, or with `--strip-ansi=always` and no color).
- An invalid value in `HL_MAX_SIZE`, `HL_MAX_LINE_LENGTH` or `HL_COLOR_DEPTH` is reported by the name of the variable, such as `error: invalid value 'abc' for HL_MAX_SIZE: ...`, instead of the option the user did not write. It is still an error with exit status 2. The README now describes this and what exit statuses 0, 1 and 2 mean.

### Fixed

- With `--header`, an input that opens but can't be read, such as a directory, no longer gets a header after its error, like a file that can't be opened.
- `PAGER=less` (without arguments, and with `LESS` unset) now shows the colors instead of raw escape sequences, and `--paging=auto` no longer pages output that fits on one screen: `less` gets the same `-RX` / `-RFX` as when `PAGER` is unset. `less` with arguments of its own, or with `LESS` set, is run as given.
- `$PAGER` is split into words like the shell does, so quotes work (`PAGER="sh -c 'less -R'"`). An unclosed quote is reported as `hl: invalid PAGER: missing closing quote`.
- A pager that fails to start is reported by name and without the OS error code, such as `hl: failed to start pager 'less': No such file or directory`.
- JavaScript no longer hangs on a comment after an expression, such as `x /*`, also inside HTML, Markdown, Vue, Svelte and QML. The bundled JavaScript syntax carries a fix for it until it is fixed upstream.
- A line that takes longer than 3 seconds to highlight no longer freezes `hl`: it warns and prints the rest of the input without highlighting.
- `hl` no longer panics (exit status 101) when stderr can't be written, such as a pipe closed by the reader. The message is dropped, and the output and the exit status stay the same; before, a warning about the config file stopped `hl` before it printed anything.
- With `--header`, a file that does not end with a newline is now followed by the blank line before the next header in every mode. Before, the blank line was missing without options and with `--strip-ansi=always`.
- An invalid size for `--max-size` or `--max-line-length`, such as `-1`, `1.5K` or `1KB`, is reported as `not a size: use a whole number with an optional K, M or G suffix (e.g. 512K)` instead of a misleading `invalid unit '.5K'`, and a number too large for 64 bits as `too large`.
- A closed stdout (`hl file >&-`) is reported as `hl: write error: Bad file descriptor` with exit status 1, like `cat`, instead of throwing the output away and exiting with 0.
- With color, `-r` stops reading an input after the last line of the range, as it already did without color. Before, the lines after it were still read (and highlighted) up to the end of the input when the range ended within the first few hundred lines.
- A file starting with the UTF-32LE BOM (`FF FE 00 00`) is no longer decoded as UTF-16, which printed NUL characters. Like other UTF-32, it is printed as-is as binary.
- `-r` and `-H` reject an empty value instead of taking it for the whole input, and a line number with a sign or spaces, such as `+3` or `+2:+3`, which was taken as `3` and `2:3`.
- A broken syntax cache (`syntaxes.bin`) no longer crashes `hl` (exit status 134 after a huge memory allocation, or 101). It is ignored like an out-of-date cache: the custom syntaxes are merged on the spot, with the reminder to run `hl --build-cache`.

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

[Unreleased]: https://github.com/codemajin/hl/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/codemajin/hl/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/codemajin/hl/releases/tag/v0.1.0
