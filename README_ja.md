# hl

[![CI](https://github.com/codemajin/hl/actions/workflows/ci.yml/badge.svg)](https://github.com/codemajin/hl/actions/workflows/ci.yml)

[English](README.md) | 日本語

`hl` は、ファイルの種類を自動で判定してシンタックスハイライトを付ける `cat` 代替コマンドです。Rust で書かれています。

- 拡張子、ファイル名（`Makefile`、`Cargo.lock` など）、shebang から言語を判定します。
- 構文定義とテーマは [bat](https://github.com/sharkdp/bat) と同じセットを使います（[two-face](https://crates.io/crates/two-face) 経由）。TOML、TypeScript、Dockerfile などにも対応しています。
- 出力先が端末でなければ何も加工せずに出力します。そのため、`cat` のエイリアスにしてもパイプやリダイレクトの結果は変わりません。

## 必要なもの

- ビルド済みバイナリの来歴を確かめる場合は [GitHub CLI](https://cli.github.com/) 2.49 以降（[ビルド済みバイナリ](#ビルド済みバイナリ)を参照）
- ソースからビルドする場合は Rust ツールチェーン（edition 2024 に対応した Rust 1.88 以降）
- ページャ機能（`--paging`、`-p`）を使う場合は `less`。環境変数 `$PAGER` を設定していればそちらを使います。どちらもなければ、警告を出してページャなしで表示します。Windows では普通 `less` に PATH が通っていません。Git for Windows には `C:\Program Files\Git\usr\bin\less.exe` があるので、`PAGER` でこれを指定できます（[docs/SPEC_ja.md](docs/SPEC_ja.md#オプション) の `--paging` を参照）。
- 24bit カラー（Windows Terminal、iTerm2、GNOME Terminal、WezTerm など）か 256 色に対応した端末。Windows では、Windows 10 以降のコンソール（cmd.exe、PowerShell）でも表示できます。

## インストール

### ビルド済みバイナリ

各[リリース](https://github.com/codemajin/hl/releases)に、次のプラットフォーム向けのバイナリがあります。

| プラットフォーム | ターゲット |
| --- | --- |
| Linux x86_64 | `x86_64-unknown-linux-musl` |
| Linux aarch64 | `aarch64-unknown-linux-musl` |
| macOS Intel | `x86_64-apple-darwin` |
| macOS Apple シリコン | `aarch64-apple-darwin` |
| Windows x86_64 | `x86_64-pc-windows-msvc` |

Linux 版は静的リンクなので、ディストリビューションを問わず動きます。ダウンロードして検証し、`~/.local/bin` にインストールする例です。

```sh
VERSION=v0.3.0                      # インストールするリリース
TARGET=x86_64-unknown-linux-musl    # 上の表から自分のプラットフォームを選ぶ
NAME=hl-$VERSION-$TARGET
curl -fLO "https://github.com/codemajin/hl/releases/download/$VERSION/$NAME.tar.gz"
curl -fLO "https://github.com/codemajin/hl/releases/download/$VERSION/$NAME.tar.gz.sha256"
shasum -a 256 -c "$NAME.tar.gz.sha256"
gh attestation verify "$NAME.tar.gz" --repo codemajin/hl --signer-workflow codemajin/hl/.github/workflows/release.yml
tar xzf "$NAME.tar.gz"
install -d ~/.local/bin
install "$NAME/hl" ~/.local/bin/hl
```

`~/.local/bin` に PATH が通っていることを確認してください。更新するときは、新しいバージョンで同じコマンドを実行します。

`.sha256` で分かるのは、ダウンロードが壊れていないかどうかまでです。改ざんは見つけられません。チェックサムはアーカイブと同じ場所に置いてあるので、片方を差し替えられる人はもう片方も差し替えられるからです。`gh attestation verify`（[GitHub CLI](https://cli.github.com/) 2.49 以降）は、アーカイブに付いているビルドの来歴の構成証明（attestation）を検証します。このリポジトリのリリース用ワークフローでビルドされたものでなければ失敗します。構成証明は v0.3.0 から付いています。v0.2.0 以前にはチェックサムしかないので、その行は飛ばしてください。

macOS では、Web ブラウザでダウンロードしたファイルに隔離属性が付き、Gatekeeper に実行を止められます。そのときは `xattr -d com.apple.quarantine ~/.local/bin/hl` で属性を外してください。`curl` でダウンロードした場合は付きません。

アンインストールするときは、ファイルを削除します。

```sh
rm ~/.local/bin/hl
```

Windows 版は tar.gz ではなく zip（`hl.exe` 入り）で、v0.3.0 から用意しています。PowerShell でダウンロードして検証し（チェックサムと、上と同じく GitHub CLI で構成証明を確かめます）、`%LOCALAPPDATA%\Programs\hl` にインストールする例です。

```powershell
$Version = "v0.3.0"                 # インストールするリリース
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

続けて、このディレクトリを一度だけ `PATH` に追加し、新しい端末を開きます。

```powershell
$Path = [Environment]::GetEnvironmentVariable("Path", "User")
if (($Path -split ";") -notcontains $Dir) {
    [Environment]::SetEnvironmentVariable("Path", "$Path;$Dir", "User")
}
```

更新するときは、新しいバージョンで最初のブロックを実行します。アンインストールするときは、`Remove-Item -Recurse "$env:LOCALAPPDATA\Programs\hl"` でディレクトリを削除し、ユーザー環境変数の `Path` からも外してください。

### ソースから

```sh
git clone https://github.com/codemajin/hl.git
cd hl
cargo install --path .
```

標準では `~/.cargo/bin/hl` にインストールされます。`~/.cargo/bin` に PATH が通っていることを確認してください。

別の場所に入れたい場合は `--root` を指定します。実行ファイルは `<root>/bin/` に置かれます。

```sh
cargo install --path . --root ~/.local      # → ~/.local/bin/hl
```

更新するときは、リポジトリを最新にしてから同じコマンドを実行してください。既存のファイルは上書きされます。

アンインストールするには次のようにします。

```sh
cargo uninstall hl                  # --root を指定した場合は同じ --root を付ける
```

### シェル補完と man ページ

リリースのアーカイブには、bash、zsh、fish、PowerShell の補完スクリプトが `completions/` に、man ページが `man/hl.1` に入っています。`hl --completions <SHELL>` と `hl --man` で `hl` 自身に出力させることもできるので、ソースからインストールした場合はこちらを使います。自分のユーザーにインストールする例です。

```sh
# bash（bash-completion パッケージが必要）
mkdir -p ~/.local/share/bash-completion/completions
hl --completions bash > ~/.local/share/bash-completion/completions/hl

# zsh: ~/.zshrc の compinit より前に `fpath=(~/.zfunc $fpath)` も追加する
mkdir -p ~/.zfunc
hl --completions zsh > ~/.zfunc/_hl

# fish
mkdir -p ~/.config/fish/completions
hl --completions fish > ~/.config/fish/completions/hl.fish

# man ページ
mkdir -p ~/.local/share/man/man1
hl --man > ~/.local/share/man/man1/hl.1
```

PowerShell では、スクリプトをプロファイルと同じディレクトリに保存し、プロファイルから読み込みます。

```powershell
$Completion = Join-Path (Split-Path $PROFILE) "hl.ps1"
New-Item -ItemType Directory -Force (Split-Path $PROFILE) | Out-Null
hl --completions powershell > $Completion
Add-Content $PROFILE ". `"$Completion`""
```

リリースのアーカイブを使う場合は、代わりに `$NAME/completions/hl.bash`、`$NAME/completions/_hl`、`$NAME/completions/hl.fish`、`$NAME/completions/_hl.ps1`、`$NAME/man/hl.1` を同じ場所にコピーしてもかまいません。補完は新しく起動したシェルから有効になります。`man hl` でページが見つからないときは、`MANPATH` に `~/.local/share/man` を追加してください。`hl` を更新したら、新しいオプションも補完されるようにファイルを作り直してください。アンインストールするときはこれらのファイルも削除します。

## 使い方

```sh
hl src/main.rs Cargo.toml        # 言語を自動判定してハイライトする
hl -n file.py                    # 行番号付き（複数ファイルでも通し番号）
hl --header src/*.rs             # 各ファイルの前にファイル名のヘッダを表示する
hl -n -r 120:160 src/main.rs     # 120〜160 行目だけを行番号付きで表示する
hl -r 110:130 -H 120 src/main.rs # 110〜130 行目を表示し、120 行目を強調する
cat script | hl -l bash          # stdin から読み、言語を明示する
hl --map-syntax '*.conf:INI' app.conf  # *.conf を INI としてハイライトする
hl --theme Nord file.rs          # テーマを指定する
hl --paging=auto big.rs          # 1 画面に収まらないときだけページャで表示する
hl -p big.rs                     # 常にページャで表示する
hl -A file.txt                   # cat -A と同じく、タブや行末、制御文字を見えるようにする
hl --list-themes                 # テーマの一覧
hl --list-languages              # 対応言語と拡張子の一覧
hl --build-cache                 # ユーザー定義の構文をキャッシュして速く読めるようにする
```

ファイルを指定しないか `-` を指定すると、標準入力から読み込みます。すべてのオプション、環境変数、設定ファイル、入力の出力のしかたの詳細は [docs/SPEC_ja.md](docs/SPEC_ja.md) で説明しています。

## `cat` の代わりに使う

シェルの設定ファイル（`~/.bashrc`、`~/.zshrc` など）にエイリアスを追加します。

```sh
alias cat='hl'
```

テーマを固定したい場合は、あわせて `HL_THEME` を設定してください（[設定ファイル](docs/SPEC_ja.md#設定ファイル)の `theme` でも構いません）。

```sh
export HL_THEME='Monokai Extended'
```

エイリアスを経由せずに本来の `cat` を実行したいときは、`\cat` または `command cat` を使います。

## ドキュメント

| ドキュメント | 内容 |
| --- | --- |
| [docs/SPEC_ja.md](docs/SPEC_ja.md) | 仕様：オプション、環境変数、設定ファイル、ユーザー定義の構文とテーマ、動作の詳細、終了ステータス |
| [docs/DESIGN_ja.md](docs/DESIGN_ja.md) | 設計：処理の流れ、モジュールとその依存関係、ハイライトのパイプライン、設計上の決まり |
| [docs/DEV_ja.md](docs/DEV_ja.md) | 開発：ビルドとテスト、依存関係、リリース、テストカバレッジ |
| [CHANGELOG.md](CHANGELOG.md) | リリースごとの変更内容（英語） |
