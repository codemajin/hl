# hl

[![CI](https://github.com/codemajin/hl/actions/workflows/ci.yml/badge.svg)](https://github.com/codemajin/hl/actions/workflows/ci.yml)

[English](README.md) | 日本語

`hl` は、ファイルの種類を自動で判定してシンタックスハイライトを付ける `cat` 代替コマンドです。Rust で書かれています。

- 拡張子、ファイル名（`Makefile`、`Cargo.lock` など）、shebang から言語を判定します。
- 構文定義とテーマは [bat](https://github.com/sharkdp/bat) と同じセットを使います（[two-face](https://crates.io/crates/two-face) 経由）。TOML、TypeScript、Dockerfile などにも対応しています。
- 出力先が端末でなければ何も加工せずに出力します。そのため、`cat` のエイリアスにしてもパイプやリダイレクトの結果は変わりません。

## 必要なもの

- ソースからビルドする場合は Rust ツールチェーン（edition 2024 に対応した Rust 1.88 以降）
- ページャ機能（`--paging`、`-p`）を使う場合は `less`。環境変数 `$PAGER` を設定していればそちらを使います。
- 24bit カラー（Windows Terminal、iTerm2、GNOME Terminal、WezTerm など）か 256 色に対応した端末

## インストール

### ビルド済みバイナリ

各[リリース](https://github.com/codemajin/hl/releases)に、次のプラットフォーム向けのバイナリがあります。

| プラットフォーム | ターゲット |
| --- | --- |
| Linux x86_64 | `x86_64-unknown-linux-musl` |
| Linux aarch64 | `aarch64-unknown-linux-musl` |
| macOS Intel | `x86_64-apple-darwin` |
| macOS Apple シリコン | `aarch64-apple-darwin` |

Linux 版は静的リンクなので、ディストリビューションを問わず動きます。ダウンロードして検証し、`~/.local/bin` にインストールする例です。

```sh
VERSION=v0.1.0                      # インストールするリリース
TARGET=x86_64-unknown-linux-musl    # 上の表から自分のプラットフォームを選ぶ
NAME=hl-$VERSION-$TARGET
curl -fLO "https://github.com/codemajin/hl/releases/download/$VERSION/$NAME.tar.gz"
curl -fLO "https://github.com/codemajin/hl/releases/download/$VERSION/$NAME.tar.gz.sha256"
shasum -a 256 -c "$NAME.tar.gz.sha256"
tar xzf "$NAME.tar.gz"
install -d ~/.local/bin
install "$NAME/hl" ~/.local/bin/hl
```

`~/.local/bin` に PATH が通っていることを確認してください。更新するときは、新しいバージョンで同じコマンドを実行します。

macOS では、Web ブラウザでダウンロードしたファイルに隔離属性が付き、Gatekeeper に実行を止められます。そのときは `xattr -d com.apple.quarantine ~/.local/bin/hl` で属性を外してください。`curl` でダウンロードした場合は付きません。

アンインストールするときは、ファイルを削除します。

```sh
rm ~/.local/bin/hl
```

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

リリースのアーカイブには、bash、zsh、fish の補完スクリプトが `completions/` に、man ページが `man/hl.1` に入っています。`hl --completions <SHELL>` と `hl --man` で `hl` 自身に出力させることもできるので、ソースからインストールした場合はこちらを使います。自分のユーザーにインストールする例です。

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

リリースのアーカイブを使う場合は、代わりに `$NAME/completions/hl.bash`、`$NAME/completions/_hl`、`$NAME/completions/hl.fish`、`$NAME/man/hl.1` を同じ場所にコピーしてもかまいません。補完は新しく起動したシェルから有効になります。`man hl` でページが見つからないときは、`MANPATH` に `~/.local/share/man` を追加してください。`hl` を更新したら、新しいオプションも補完されるようにファイルを作り直してください。アンインストールするときはこれらのファイルも削除します。

## 使い方

```sh
hl src/main.rs Cargo.toml        # 言語を自動判定してハイライトする
hl -n file.py                    # 行番号付き（複数ファイルでも通し番号）
hl --header src/*.rs             # 各ファイルの前にファイル名のヘッダを表示する
hl -n -r 120:160 src/main.rs     # 120〜160 行目だけを行番号付きで表示する
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

### オプション

| オプション | 説明 |
| --- | --- |
| `-n`, `--number` | 行番号を表示します。`cat -n` と同じく、複数のファイルにまたがって通し番号を振ります |
| `-b`, `--number-nonblank` | 空行以外に行番号を振ります。`-n` より優先されます |
| `-s`, `--squeeze-blank` | 連続する空行を 1 行にまとめます |
| `-r`, `--line-range <N:M>` | 各入力の N 行目から M 行目だけを表示します。`N:` は末尾まで、`:M` は 1 行目から、`N` はその 1 行だけです |
| `--header` | 各入力の前に `==> FILE <==` というヘッダを表示します |
| `-v`, `--show-nonprinting` | 制御文字と 128 以上のバイトを `^` と `M-` の表記で表示します（改行とタブは除く） |
| `-E`, `--show-ends` | 各行の末尾に `$` を表示します |
| `-T`, `--show-tabs` | タブを `^I` と表示します |
| `-A`, `--show-all` | `-vET` と同じです |
| `-e` | `-vE` と同じです |
| `-t` | `-vT` と同じです |
| `-u` | `cat` との互換のために受け付けますが、何もしません |
| `-l`, `--language <LANG>` | 自動判定を使わずに言語を指定します。言語名か拡張子で指定できます（例: `rust`、`py`） |
| `--map-syntax <PATTERN:LANG>` | グロブパターン PATTERN に合うファイルを言語 LANG で表示します（例: `'*.conf:INI'`）。LANG は `-l` と同じく言語名か拡張子で指定します。`/` を含まないパターンはファイル名と、`/` を含むパターンは絶対パスと照合します（`*` は 1 つのディレクトリの中だけ、`**` は任意の深さのディレクトリに合います）。何度でも指定でき、複数の対応付けが合うときは最後に指定したものを使います。標準入力には適用しません |
| `--theme <THEME>` | テーマを指定します（大文字・小文字は区別しません）。組み込みのテーマとユーザー定義のテーマが使えます。既定は `base16-ocean.dark` です。見つからないテーマ名を指定した場合は警告を出し、既定のテーマで表示します |
| `--max-size <SIZE>` | これより大きい入力はハイライトせずに出力します。既定は `512K` です。`K`、`M`、`G`（1024 倍）の単位が使えます。`0` で上限をなくします |
| `--max-line-length <LEN>` | これより長い行（改行を除いたバイト数）が出てきたら、その行から先をハイライトせずに出力します。既定は `16K` です。`0` で上限をなくします |
| `--paging <WHEN>` | ページャを使うかどうか。`auto`、`always`、`never`（既定）から選びます。`auto` では、出力が 1 画面に収まらないときだけページャで表示します。ページャは `$PAGER`、未設定なら `less -RFX`（`auto`）か `less -RX`（`always`）です。標準出力が端末でなければページャは起動しません |
| `-p` | `--paging=always` と同じ |
| `--color <WHEN>` | 色を付ける条件を `auto`、`always`、`never` から選びます。既定の `auto` では、出力先が端末のときだけ色を付けます |
| `--color-depth <DEPTH>` | 色深度を `auto`、`truecolor`（別名 `24bit`）、`256` から選びます。既定の `auto` では、`COLORTERM` が `truecolor` か `24bit` のとき、`TERM` が `-direct` で終わるとき、`WT_SESSION` が設定されているとき（Windows Terminal）に 24bit カラーを使い、それ以外では 256 色に近い色に置き換えて表示します |
| `--list-themes` | 使えるテーマを一覧表示します |
| `--list-languages` | 対応している言語を一覧表示します |
| `--build-cache` | ユーザー定義の構文（後述）をキャッシュして、速く読めるようにします |
| `--clear-cache` | `--build-cache` で作ったキャッシュを消します |
| `--completions <SHELL>` | `bash`、`zsh`、`fish`、`elvish`、`powershell` の補完スクリプトを出力します（[シェル補完と man ページ](#シェル補完と-man-ページ)を参照） |
| `--man` | man ページを roff 形式で出力します |

ファイルを指定しないか `-` を指定すると、標準入力から読み込みます。

### 環境変数

| 変数 | 説明 |
| --- | --- |
| `HL_THEME` | 既定のテーマ。`--theme` を指定した場合はそちらが優先されます |
| `HL_MAX_SIZE` | `--max-size` の既定値 |
| `HL_MAX_LINE_LENGTH` | `--max-line-length` の既定値 |
| `HL_COLOR_DEPTH` | `--color-depth` の既定値 |
| `HL_CONFIG` | 設定ファイルの場所 |
| `PAGER` | `--paging` と `-p` で使うページャ |

### 設定ファイル

一部のオプションの既定値は `$XDG_CONFIG_HOME/hl/config.toml`（`XDG_CONFIG_HOME` が未設定なら `~/.config/hl/config.toml`）で設定できます。別のファイルを使うときは `HL_CONFIG` で場所を指定します。キーにはオプションの長い名前を使います。

```toml
theme = "Monokai Extended"
color = "auto"            # auto, always, never
color-depth = "truecolor" # auto, truecolor, 256
paging = "auto"           # auto, always, never
max-size = "1M"           # 0 のような整数も書けます
max-line-length = "16K"
map-syntax = ["*.conf:INI", "Jenkinsfile:Groovy"]
```

優先順位は高い順に、コマンドライン引数、環境変数、設定ファイル、組み込みの既定値です。`hl --help` には設定ファイルを反映した既定値が出ます。設定ファイルがなければ今までどおり動きます。ファイルが読めないときや TOML として正しくないときは、警告を出してファイル全体を無視します。知らないキーや不正な値は、警告を出してその項目だけ読み飛ばします。`-n` や `--header` のようなフラグは、コマンドラインで打ち消せなくなるため設定できません。

`map-syntax` には `--map-syntax` に渡す値を配列で書きます。ほかのキーと違い、コマンドラインで指定すると置き換わる既定値ではありません。`--map-syntax` で指定した対応付けはここに追加され、両方が合うときはコマンドラインのほうを使います。形式が正しくない項目や知らない言語を指定した項目は、警告を出して読み飛ばします。

### ユーザー定義の構文とテーマ

組み込みのものに加えて、設定ディレクトリ（`$XDG_CONFIG_HOME/hl`、`XDG_CONFIG_HOME` が未設定なら `~/.config/hl`。`HL_CONFIG` の影響は受けません）に置いた Sublime Text の構文と TextMate のテーマも使えます。

```text
~/.config/hl/
├── syntaxes/   # *.sublime-syntax
└── themes/     # *.tmTheme
```

- ユーザー定義の構文は組み込みのものと同じように使えます。`file_extensions` や `first_line_match` による判定、`-l`、`--map-syntax` のどれでも選べます。組み込みの構文と拡張子が重なったときは、ユーザー定義のほうを使います。
- テーマの名前は拡張子を除いたファイル名です。`themes/My Theme.tmTheme` なら `--theme 'My Theme'` で選びます。組み込みと同じ名前なら、ユーザー定義のほうで置き換わります。
- `--list-languages` と `--list-themes` にはユーザー定義のものも出ます。
- 読み込めないファイルは、警告を出して読み飛ばします。使おうとしたテーマが読み込めないときは `base16-ocean.dark` を使います。

ユーザー定義の構文を組み込みのものとまとめるには数百ミリ秒かかるので、結果をキャッシュしておきます。

```sh
hl --build-cache   # $XDG_CACHE_HOME/hl/syntaxes.bin（~/.cache/hl/syntaxes.bin）を作る
hl --clear-cache   # キャッシュを消す
```

構文を追加・変更・削除したときや `hl` を更新したときは、もう一度 `hl --build-cache` を実行してください。それまでの間はキャッシュが古いことを `hl` が検出し、毎回その場で構文をまとめたうえで、キャッシュを作り直すよう stderr に表示します。テーマは使うときにだけ読むので、キャッシュは要りません。ユーザー定義の構文がなければ、キャッシュは読みませんし、作る必要もありません。

### 動作の詳細

- 言語は `-l` の指定、`--map-syntax`（と設定ファイルの `map-syntax`）、拡張子、ファイル名、1 行目（shebang など）の順に判定します。どれでも判定できなければプレーンテキストとして出力します。
- ファイルの先頭 8KB に NUL バイトが含まれていればバイナリとみなし、ハイライトせずにそのまま出力します。
- Shift_JIS や EUC-JP など UTF-8 として正しくない入力は、正しくない行から先をハイライトせずにそのまま出力します（先頭 8KB に含まれていればファイル全体）。バイト列が置き換えられることはありません。ほかの文字コードの自動判定はしません。
- BOM 付きの UTF-16 は、色を付けるときだけ UTF-8 に変換してハイライトします。色を付けないときはそのまま出力します。UTF-8 の BOM は出力に残し、言語の判定では無視します。
- ハイライトは 1 秒あたり数百 KB しか処理できないため、巨大なファイルや minify された JS / CSS で待たされないように上限を設けています。`--max-size` を超えるファイルは最初からハイライトしません。標準入力のようにサイズがわからない入力は、上限を超えたところから先をハイライトしません。`--max-line-length` を超える行が出てきた場合も、その行から先をハイライトしません。どちらの場合も警告は出さず、`cat` と同じ内容を出力します。
- `-v`、`-E`、`-T`（と `-A`、`-e`、`-t`）を指定するとハイライトせずに出力するので、`cat` と同じ結果になります。`-n`、`-b`、`-s` はハイライトと組み合わせて使えます。
- `--header` を指定すると、ファイルが 1 つだけのときも含めて、各入力の先頭に `==> FILE <==` の行を出力します（標準入力は `==> standard input <==`）。ファイルとファイルの間には空行を 1 行入れます。色を付けるときはヘッダを太字にします。ヘッダの行には `-n` や `-b` の行番号を振らず、行番号はこれまでどおりファイルをまたいで通しで数えます。開けなかったファイルにはヘッダを出しません。`--header` を指定しなければ、`cat` と同じ出力になります。
- `-r` を指定しても、範囲より前の行は読み込んでハイライトの解析だけ行います。そのため、ファイル全体を表示したときと同じ色で表示されます。範囲の終わりまで来たら、それより先は読みません。範囲は入力ごとに適用します。`-n` や `-b` はその入力での行番号を表示し、範囲外の行（と `-s` でまとめた空行）も数えます。範囲内で改行なしに終わったファイルの後には、次の入力の前に改行を補います。`-r` を指定しなければ、行番号は `cat` と同じくファイルをまたいで通しで数えます。
- 開けないファイルがあると `hl: <path>: <理由>` を標準エラー出力に表示し、残りのファイルを処理したあと終了コード 1 で終了します。
- `hl file | head` のように出力先が途中で閉じられても、エラーにはせず正常終了します。

## `cat` の代わりに使う

シェルの設定ファイル（`~/.bashrc`、`~/.zshrc` など）にエイリアスを追加します。

```sh
alias cat='hl'
```

テーマを固定したい場合は、あわせて `HL_THEME` を設定してください（[設定ファイル](#設定ファイル)の `theme` でも構いません）。

```sh
export HL_THEME='Monokai Extended'
```

エイリアスを経由せずに本来の `cat` を実行したいときは、`\cat` または `command cat` を使います。

## 開発

```sh
cargo build
cargo test          # src/ のユニットテストと tests/cli.rs の結合テスト
cargo clippy
cargo fmt --check   # フォーマットの確認（rustfmt.toml）
```

push と pull request のたびに、GitHub Actions が `cargo fmt --check`、`cargo clippy`（警告はエラー扱い）、`cargo test` を実行します。Linux と macOS の両方で、stable と最小対応バージョン（1.88）の Rust を使います。テストは `hl` の出力を GNU coreutils の `cat` と比べるので、macOS では先に `brew install coreutils` で `gcat` を入れておいてください。`rustfmt.toml` には nightly でしか効かないオプションがあるため、フォーマットの確認には nightly の rustfmt を使います。手元で揃えるときは `cargo +nightly fmt` を実行してください。

| ファイル | 役割 |
| --- | --- |
| `src/main.rs` | エントリポイント。引数と設定からの組み立て、ページャか標準出力かの選択、エラー処理と終了コード |
| `src/assets.rs` | 設定ディレクトリのカスタム構文・テーマの読み込みと同梱分とのマージ、構文キャッシュ（`--build-cache`/`--clear-cache`） |
| `src/cli.rs` | コマンドライン引数の定義（clap）と設定ファイルからのデフォルト値 |
| `src/color.rs` | 端末の色深度の判定と 256 色へのフォールバック |
| `src/config.rs` | 設定ファイルの読み込みと、設定・キャッシュディレクトリのパス |
| `src/diag.rs` | 標準エラー出力への警告・エラーメッセージ |
| `src/encoding.rs` | UTF-8/UTF-16 の BOM 判定と UTF-16 のデコード |
| `src/highlight.rs` | 言語判定、`--map-syntax` の対応付け、サイズの上限、ハイライト出力（syntect） |
| `src/lines.rs` | 行番号、空行の圧縮、`cat -v`/`-E`/`-T` の表記 |
| `src/pager.rs` | ページャの起動と終了待ち |
| `src/print.rs` | 各入力の順次出力（ハイライトありかなしか）、ヘッダ、文字コードの扱い |

### リリース

バージョン番号は[セマンティック バージョニング](https://semver.org/lang/ja/)に従い、リリースには `Cargo.toml` のバージョンに合わせて `vX.Y.Z` のタグを付けます。変更内容は [CHANGELOG.md](CHANGELOG.md) に記録します。

1. `Cargo.toml` の `version` を変更し、`cargo build` を実行して `Cargo.lock` を更新する
2. `CHANGELOG.md` の `Unreleased` の項目を新しい `## [X.Y.Z] - YYYY-MM-DD` の節に移し、末尾のリンクを更新する
3. コミットしてから、タグを push する

   ```sh
   git tag vX.Y.Z
   git push origin vX.Y.Z
   ```

すると `.github/workflows/release.yml` が 4 つのプラットフォーム向けにバイナリをビルドし、`hl` 自身で生成した補完スクリプトと man ページとまとめてアーカイブし、SHA-256 のチェックサムとともに GitHub のリリースとして公開します。リリースノートは CHANGELOG の該当の節です。タグが `Cargo.toml` のバージョンと一致しない場合や、CHANGELOG にそのバージョンの節がない場合は、何も公開せずに失敗します。

### テストカバレッジ

カバレッジの計測には [cargo-llvm-cov](https://github.com/taiki-e/cargo-llvm-cov) を使います。結合テストが子プロセスとして起動する `hl` バイナリも計測の対象になります。最初に一度だけインストールしてください。

```sh
rustup component add llvm-tools-preview
cargo install cargo-llvm-cov --locked
```

```sh
cargo llvm-cov          # テストを実行し、ファイルごとのサマリを表示する
cargo llvm-cov --html   # HTML レポートを target/llvm-cov/html/index.html に出力する
                        # （--open を付けるとブラウザで開く）
```
