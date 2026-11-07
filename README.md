# hl

[![CI](https://github.com/codemajin/hl/actions/workflows/ci.yml/badge.svg)](https://github.com/codemajin/hl/actions/workflows/ci.yml)

English | [日本語](README_ja.md)

`hl` is a `cat` replacement that detects the file type and prints the file with syntax highlighting. It is written in Rust.

- Detects the language from the file extension, the file name (`Makefile`, `Cargo.lock`, etc.), and the shebang.
- Uses the same syntax definitions and themes as [bat](https://github.com/sharkdp/bat) (via [two-face](https://crates.io/crates/two-face)), so TOML, TypeScript, Dockerfile and many more are supported.
- Writes the input unmodified when stdout is not a terminal, so aliasing `cat` to `hl` doesn't change what pipes and redirects produce.

## Requirements

- To verify a prebuilt binary's build provenance: the [GitHub CLI](https://cli.github.com/) 2.49 or later (see [Prebuilt binaries](#prebuilt-binaries))
- To build from source: a Rust toolchain (Rust 1.88 or later, for edition 2024)
- `less` for the pager options (`--paging`, `-p`). If `$PAGER` is set, that pager is used instead. Without either, `hl` prints the output without a pager and warns. Windows usually has no `less` on the `PATH`; Git for Windows has one in `C:\Program Files\Git\usr\bin\less.exe`, which `PAGER` can point to (see `--paging` in [docs/SPEC.md](docs/SPEC.md#options)).
- A terminal with 24-bit color (Windows Terminal, iTerm2, GNOME Terminal, WezTerm, etc.) or 256 color support. On Windows, the console of Windows 10 or later (cmd.exe, PowerShell) works too.

## Installation

### Prebuilt binaries

Each [release](https://github.com/codemajin/hl/releases) has binaries for these platforms:

| Platform | Target |
| --- | --- |
| Linux x86_64 | `x86_64-unknown-linux-musl` |
| Linux aarch64 | `aarch64-unknown-linux-musl` |
| macOS Intel | `x86_64-apple-darwin` |
| macOS Apple silicon | `aarch64-apple-darwin` |
| Windows x86_64 | `x86_64-pc-windows-msvc` |

The Linux binaries are statically linked, so they run on any distribution. To download, verify and install one to `~/.local/bin`:

```sh
VERSION=v0.2.0                      # the release to install
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

The `.sha256` check finds a broken download, but not a tampered one: the checksum is published next to the archive, so whoever could replace one could replace both. To check that the archive was built from this repository by its release workflow, verify its build provenance attestation with the [GitHub CLI](https://cli.github.com/) (2.49 or later), before extracting it:

```sh
gh attestation verify "$NAME.tar.gz" --repo codemajin/hl --signer-workflow codemajin/hl/.github/workflows/release.yml
```

It fails if the archive was not signed by that workflow. Releases have attestations starting with the one after v0.2.0; v0.2.0 and earlier have only the checksums.

On macOS, a file downloaded with a web browser is quarantined and blocked by Gatekeeper. If that happens, remove the attribute with `xattr -d com.apple.quarantine ~/.local/bin/hl`. Files downloaded with `curl` are not affected.

To uninstall, remove the binary:

```sh
rm ~/.local/bin/hl
```

On Windows, the binary comes as a zip (with `hl.exe`) instead of a tar.gz, starting with the release after v0.2.0. To download, verify (the checksum, and the attestation with the GitHub CLI as above) and install it to `%LOCALAPPDATA%\Programs\hl` in PowerShell:

```powershell
$Version = "vX.Y.Z"                 # the release to install
$Name = "hl-$Version-x86_64-pc-windows-msvc"
$Base = "https://github.com/codemajin/hl/releases/download/$Version"
Invoke-WebRequest "$Base/$Name.zip" -OutFile "$Name.zip"
Invoke-WebRequest "$Base/$Name.zip.sha256" -OutFile "$Name.zip.sha256"
$Expected = (Get-Content "$Name.zip.sha256").Split(" ")[0]
if ((Get-FileHash "$Name.zip" -Algorithm SHA256).Hash -ne $Expected) { throw "checksum mismatch" }
gh attestation verify "$Name.zip" --repo codemajin/hl --signer-workflow codemajin/hl/.github/workflows/release.yml
if ($LASTEXITCODE -ne 0) { throw "attestation verification failed" }
Expand-Archive "$Name.zip" -DestinationPath .
$Dir = "$env:LOCALAPPDATA\Programs\hl"
New-Item -ItemType Directory -Force $Dir | Out-Null
Copy-Item "$Name\hl.exe" $Dir
```

Then add the directory to your `PATH` once, and open a new terminal:

```powershell
$Path = [Environment]::GetEnvironmentVariable("Path", "User")
if (($Path -split ";") -notcontains $Dir) {
    [Environment]::SetEnvironmentVariable("Path", "$Path;$Dir", "User")
}
```

To update, run the first block with the new version. To uninstall, remove the directory with `Remove-Item -Recurse "$env:LOCALAPPDATA\Programs\hl"`, and remove it from `Path` in the user environment variables.

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

The release archives include completion scripts for bash, zsh, fish and PowerShell in `completions/`, and the man page in `man/hl.1`. `hl` can also print them itself with `hl --completions <SHELL>` and `hl --man`, which is the way to get them when installing from source. To install them for your user:

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

For PowerShell, save the script next to your profile and load it from there:

```powershell
$Completion = Join-Path (Split-Path $PROFILE) "hl.ps1"
New-Item -ItemType Directory -Force (Split-Path $PROFILE) | Out-Null
hl --completions powershell > $Completion
Add-Content $PROFILE ". `"$Completion`""
```

With a release archive, you can instead copy `$NAME/completions/hl.bash`, `$NAME/completions/_hl`, `$NAME/completions/hl.fish`, `$NAME/completions/_hl.ps1` and `$NAME/man/hl.1` to the same places. Start a new shell to load the completions. If `man hl` doesn't find the page, add `~/.local/share/man` to `MANPATH`. Regenerate the files after updating `hl` so that they cover new options, and remove them when uninstalling.

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

With no file, or when the file is `-`, `hl` reads from standard input. All the options, the environment variables, the configuration file and the details of how the input is printed are described in [docs/SPEC.md](docs/SPEC.md).

## Using it as `cat`

Add an alias to your shell configuration file (`~/.bashrc`, `~/.zshrc`, etc.):

```sh
alias cat='hl'
```

To fix the theme, also set `HL_THEME` (or `theme` in the [configuration file](docs/SPEC.md#configuration-file)):

```sh
export HL_THEME='Monokai Extended'
```

To run the real `cat` without going through the alias, use `\cat` or `command cat`.

## Documentation

| Document | Contents |
| --- | --- |
| [docs/SPEC.md](docs/SPEC.md) | Specification: options, environment variables, configuration file, custom syntaxes and themes, behavior details, exit status |
| [docs/DESIGN.md](docs/DESIGN.md) | Design: the flow of the program, the modules and how they depend on each other, the highlighting pipeline, design rules |
| [docs/DEV.md](docs/DEV.md) | Development: building and testing, dependencies, releasing, test coverage |
| [CHANGELOG.md](CHANGELOG.md) | Changes in each release |
