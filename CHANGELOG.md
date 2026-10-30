# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
