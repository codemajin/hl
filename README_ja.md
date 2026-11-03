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

### オプション

| オプション | 説明 |
| --- | --- |
| `-n`, `--number` | 行番号を表示します。`cat -n` と同じく、複数のファイルにまたがって通し番号を振ります |
| `-b`, `--number-nonblank` | 空行以外に行番号を振ります。`-n` より優先されます |
| `-s`, `--squeeze-blank` | 連続する空行を 1 行にまとめます |
| `-r`, `--line-range <N:M>` | 各入力の N 行目から M 行目だけを表示します。`N:` は末尾まで、`:M` は 1 行目から、`N` はその 1 行だけです。行番号は 1 から始まり、数字だけで書きます（`+3` や ` 3` はエラーです） |
| `-H`, `--highlight-line <N:M>` | 各入力の N 行目から M 行目を、テーマの行強調色を背景にして表示します。書式は `-r` と同じで、複数回指定できます。`-n` や `-b` の行番号も一緒に色が付きますが、テキストより右側は塗りません。色を付けて出力するときだけ効くので、色なしの出力は変わりません |
| `--header` | 各入力の前に `==> FILE <==` というヘッダを表示します |
| `--no-number`、`--no-number-nonblank`、`--no-squeeze-blank`、`--no-header` | [設定ファイル](#設定ファイル)でオンにした `-n`、`-b`、`-s`、`--header` をオフにします。フラグと対応する `--no-*` を両方指定したときは、後に書いたほうが効きます |
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
| `--max-size <SIZE>` | これより大きい入力はハイライトせずに出力します。既定は `512K` です。0 以上の整数のバイト数で、後ろに `K`、`M`、`G`（1024 倍。大文字でも小文字でも可）の単位を付けられます（`1048576`、`1M` など）。負の数や `1.5K` のような小数は使えません。`0` で上限をなくします |
| `--max-line-length <LEN>` | これより長い行（改行を除いたバイト数）が出てきたら、その行から先をハイライトせずに出力します。既定は `16K` です。書き方は `--max-size` と同じです。`0` で上限をなくします |
| `--paging <WHEN>` | ページャを使うかどうか。`auto`、`always`、`never`（既定）から選びます。`auto` では、出力が 1 画面に収まらないときだけページャで表示します。ページャは `$PAGER`、未設定なら `less -RFX`（`auto`）か `less -RX`（`always`）です。`$PAGER` はシェルと同じ規則で単語に分けるので、クォートが使えます（`PAGER="less -R -S"`、`PAGER="'/opt/my pager/pg'"`）。`$PAGER` が引数のない `less` なら、`LESS` が未設定のときに限り同じ `-RFX` か `-RX` を補います。それ以外は `$PAGER` と `LESS` のオプションをそのまま使うので、色を表示するには `-R` を含めてください。標準出力が端末でなければページャは起動しません |
| `-p` | `--paging=always` と同じ |
| `--color <WHEN>` | 色を付ける条件を `auto`、`always`、`never` から選びます。既定の `auto` では、出力先が端末のときだけ色を付けます |
| `--color-depth <DEPTH>` | 色深度を `auto`、`truecolor`（別名 `24bit`）、`256` から選びます。既定の `auto` では、`COLORTERM` が `truecolor` か `24bit` のとき、`TERM` が `-direct` で終わるとき、`WT_SESSION` が設定されているとき（Windows Terminal）に 24bit カラーを使い、それ以外では 256 色に近い色に置き換えて表示します。`COLORTERM` は大文字小文字を区別せずに比べますが、空白は取り除きません（`truecolor ` は該当しません）。`-direct` は terminfo の名前と同じく大文字小文字を区別します（`xterm-DIRECT` は該当しません） |
| `--strip-ansi <WHEN>` | 入力に含まれるエスケープシーケンス（色、ウィンドウタイトル、ハイパーリンクなど）を取り除く条件を `auto`、`always`、`never` から選びます。既定の `auto` では、色を付けて出力するときだけ取り除きます。`always` は色を付けないときも取り除き、`never` はそのまま出力します（[動作の詳細](#動作の詳細)を参照） |
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
| `LESS` | `less` のオプション。設定されていれば、`$PAGER` が引数のない `less` でもオプションを補いません |

`HL_MAX_SIZE`、`HL_MAX_LINE_LENGTH`、`HL_COLOR_DEPTH` の値が不正なときは、コマンドラインと同じくエラーになります。`error: invalid value 'abc' for HL_MAX_SIZE: ...` のように変数名を含むメッセージを表示し、何も出力せずに終了コード 2 で終了します。不正な値を警告して読み飛ばす設定ファイルとは扱いが違います。コマンドラインでオプションを指定すれば環境変数の値は使わないので、その値は検査しません。`HL_THEME` のテーマが見つからないときは、`--theme` と同じく警告を出して既定のテーマを使います。

### 設定ファイル

一部のオプションの既定値は `$XDG_CONFIG_HOME/hl/config.toml`（`XDG_CONFIG_HOME` が未設定か絶対パスでなければ `~/.config/hl/config.toml`）で設定できます。別のファイルを使うときは `HL_CONFIG` で場所を指定します。キーにはオプションの長い名前を使います。

```toml
theme = "Monokai Extended"
color = "auto"            # auto, always, never
color-depth = "truecolor" # auto, truecolor, 256
strip-ansi = "auto"       # auto, always, never
paging = "auto"           # auto, always, never
max-size = "1M"           # 0 のような整数も書けます
max-line-length = "16K"
number = true             # number-nonblank、squeeze-blank、header も同様
map-syntax = ["*.conf:INI", "Jenkinsfile:Groovy"]
```

優先順位は高い順に、コマンドライン引数、環境変数、設定ファイル、組み込みの既定値です。`hl --help` には設定ファイルを反映した既定値が出ます。設定ファイルがなければ今までどおり動きます。ファイルが読めないときや TOML として正しくないときは、警告を出してファイル全体を無視します。知らないキーや不正な値は、警告を出してその項目だけ読み飛ばします。フラグの `number`、`number-nonblank`、`squeeze-blank`、`header` には `true` か `false` を書きます。ここでオンにしたフラグは、`--no-number`、`--no-number-nonblank`、`--no-squeeze-blank`、`--no-header` でその場だけオフにできます。`-b` が `-n` より優先されるのは、どちらをどこで指定しても同じです。

`map-syntax` には `--map-syntax` に渡す値を配列で書きます。ほかのキーと違い、コマンドラインで指定すると置き換わる既定値ではありません。`--map-syntax` で指定した対応付けはここに追加され、両方が合うときはコマンドラインのほうを使います。形式が正しくない項目や知らない言語を指定した項目は、警告を出して読み飛ばします。

### ユーザー定義の構文とテーマ

組み込みのものに加えて、設定ディレクトリ（`$XDG_CONFIG_HOME/hl`、`XDG_CONFIG_HOME` が未設定か絶対パスでなければ `~/.config/hl`。`HL_CONFIG` の影響は受けません）に置いた Sublime Text の構文と TextMate のテーマも使えます。

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
hl --build-cache   # $XDG_CACHE_HOME/hl/syntaxes.bin（未設定か絶対パスでなければ ~/.cache/hl/syntaxes.bin）を作る
hl --clear-cache   # キャッシュを消す
```

構文を追加・変更・削除したときや `hl` を更新したときは、もう一度 `hl --build-cache` を実行してください。それまでの間はキャッシュが古いことを `hl` が検出し、毎回その場で構文をまとめたうえで、キャッシュを作り直すよう stderr に表示します。キャッシュのファイルが壊れているときも同じです。テーマは使うときにだけ読むので、キャッシュは要りません。ユーザー定義の構文がなければ、キャッシュは読みませんし、作る必要もありません。

### 動作の詳細

- 言語は `-l` の指定、`--map-syntax`（と設定ファイルの `map-syntax`）、拡張子、ファイル名、1 行目（shebang など）の順に判定します。どれでも判定できなければプレーンテキストとして出力します。
- ファイルの先頭 8KB に NUL バイトが含まれていればバイナリとみなし、ハイライトせずに出力します（後述のエスケープシーケンスを取り除くほかはそのままです）。
- Shift_JIS や EUC-JP など UTF-8 として正しくない入力は、正しくない行から先をハイライトせずにそのまま出力します（先頭 8KB に含まれていればファイル全体。8KB の境目で途中まで入った多バイト文字は正しくないとはみなしません）。バイト列が置き換えられることはありません。ほかの文字コードの自動判定はしません。
- BOM（`FF FE` か `FE FF`）付きの UTF-16 は、色を付けるときやエスケープシーケンスを取り除くときに UTF-8 に変換します。ハイライトするのは、色を付けるときで、かつファイルが `--max-size` 以下のときです。色を付けず、エスケープシーケンスも残すときはそのまま出力します。UTF-8 の BOM は出力に残し、言語の判定では無視します。`-r` では BOM は 1 行目の一部として扱うので、1 行目を表示するときだけ出力します。UTF-32 には対応していません。その BOM（`FF FE 00 00` か `00 00 FE FF`）は UTF-16 とはみなさず、NUL バイトを含むのでバイナリとしてそのまま出力します。
- ハイライトは 1 秒あたり数百 KB しか処理できないため、巨大なファイルや minify された JS / CSS で待たされないように上限を設けています。`--max-size` を超えるファイルは最初からハイライトしません。標準入力のようにサイズがわからない入力は、上限を超えたところから先をハイライトしません。`--max-line-length` を超える行が出てきた場合も、その行から先をハイライトしません。どちらの場合も警告は出さず、`cat` と同じ内容を出力します。
- 1 行のハイライトに 3 秒より長くかかった場合は、警告（`cannot highlight: a line took longer than 3s, printing the rest as plain text`）を出し、その行から先をハイライトせずに出力します。構文定義のエラーで止まったときと同じ扱いです。構文定義によっては、特定の入力でハイライトが終わらなくなることがあるためです（Scala 構文の `:a@<` など）。止まったハイライトは専用のスレッドで動いていて途中で止められないので、`hl` が終了するまで CPU を 1 コア使い続けます。ページャで表示している間も同じです。
- 同梱の JavaScript 構文には、bat のものに修正を加えています（HTML、Markdown、Vue、Svelte、QML の中の JavaScript にも使われます）。`x /*` のように式の後にコメントがあると、ハイライトが終わらなくなっていたためです。
- 色を付けて出力するときは、入力に含まれるエスケープシーケンスを取り除きます。ハイライトの色が崩れたり、端末を操作されたり（ウィンドウタイトル、ハイパーリンク、クリップボードなど）しないようにするためです。対象は CSI（`ESC [ ... m` など）、BEL か ST で終わる OSC や DCS などの文字列、そのほか ESC で始まるシーケンス、それに UTF-8 で表した C1 制御文字（U+0080–U+009F）です。8 ビットの CSI（U+009B）や OSC（U+009D）は、`ESC [` や `ESC ]` と同じくシーケンスの始まりとして扱います。UTF-8 に変換した後の UTF-16 や、ハイライトせずに出力する部分（バイナリ、`--max-size` や `--max-line-length` を超えた部分、UTF-8 として正しくない行から先）も対象です。終端のない文字列はその行の終わりで終わったものとみなし、入力の末尾で途切れたシーケンスは捨てます。色を付けないときは `cat` と同じ出力になります。`--strip-ansi=always` を指定すると色を付けないときも取り除き、`--strip-ansi=never` を指定するとそのまま出力します。復帰（`\r`）は `cat` と同じく残すので、端末上で行を上書きする表示はそのまま起こります。
- ヘッダやメッセージに出すファイル名やテーマ名に制御文字が含まれていれば、`\x1b` のような形で表示します。細工した名前で端末を操作されないようにするためです。
- `-v`、`-E`、`-T`（と `-A`、`-e`、`-t`）を指定するとハイライトせずに出力するので、`cat` と同じ結果になります。ただし `--strip-ansi=always` を一緒に指定すると、エスケープシーケンスを取り除いてから残りを表示します。たとえば `a\x1b[31m\tb` は、`cat -A` では `a^[[31m^Ib$` ですが、`a^Ib$` になります。`-n`、`-b`、`-s` はハイライトと組み合わせて使えます。
- `-n`、`-b`、`-s` は、エスケープシーケンスを取り除いた後の行を見ます。そのため、`\x1b[0m` のようにエスケープシーケンスしかない行は、`-b` と `-s` では空行として扱います。
- `--header` を指定すると、ファイルが 1 つだけのときも含めて、各入力の先頭に `==> FILE <==` の行を出力します（標準入力は `==> standard input <==`）。ファイルとファイルの間には空行を 1 行入れます。色を付けるときはヘッダを太字にします。ヘッダの行には `-n` や `-b` の行番号を振らず、行番号はこれまでどおりファイルをまたいで通しで数えます。開けなかったファイルや、ディレクトリのように読めなかったファイルにはヘッダを出しません。`--header` を指定しなければ、`cat` と同じ出力になります。
- `-r` を指定しても、範囲より前の行は読み込んでハイライトの解析だけ行います。そのため、ファイル全体を表示したときと同じ色で表示されます。範囲の終わりまで来たら、それより先は読みません。範囲は入力ごとに適用します。`-n` や `-b` はその入力での行番号を表示し、範囲外の行（と `-s` でまとめた空行）も数えます。範囲内で改行なしに終わったファイルの後には、次の入力の前に改行を補います。`-r` を指定しなければ、行番号は `cat` と同じくファイルをまたいで通しで数えます。
- 開けないファイルや読めないファイルがあると `hl: <path>: <理由>`（`hl: src: Is a directory` など）を標準エラー出力に表示し、残りのファイルを処理したあと終了コード 1 で終了します。
- `hl file | head` のように出力先が途中で閉じられても、エラーにはせず正常終了します。
- 標準出力が閉じられている（`hl file >&-`）、ディスクがいっぱいなどで出力を書き込めないときは、`hl: write error: <理由>` を標準エラー出力に表示し、`cat` と同じく終了コード 1 で終了します。`--list-themes` や `--man` など、何かを表示して終わるコマンドでも同じです。
- 読み手が閉じたパイプなどで標準エラー出力に書き込めないときは、メッセージを捨てます。出力と終了コードは変わりません。
- 終了コードは次のとおりです。0 は成功で、警告を出して代わりの方法で続けたとき（見つからないテーマ、設定ファイルの不正な項目、ハイライトに時間がかかりすぎた行）も含みます。1 は実行中の失敗です（開けないファイルや読めないファイル、`-l` や `--map-syntax` に指定した未知の言語、書き込みエラー、ページャの起動失敗）。2 は、実行を始める前にコマンドラインや、オプションの代わりになる環境変数を受け付けなかったときです（未知のオプション、`--max-size=1.5K` や `--color=sometimes` のような不正な値）。

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
cargo test          # src/ のユニットテストと tests/cli/ の結合テスト（機能ごとに 1 ファイル）
cargo clippy
cargo fmt --check   # フォーマットの確認（rustfmt.toml）
```

push と pull request のたびに、GitHub Actions が `cargo fmt --check`、`cargo clippy`（警告はエラー扱い）、`cargo test` を実行します。Linux と macOS の両方で、stable と最小対応バージョン（1.88）の Rust を使います。`cat` 互換のオプションについては、期待する出力をテストに直接書いています。それとは別に、GNU coreutils 9.0 以降の `cat`（`gcat` または `cat`）が見つかれば `hl` の出力と比べるテストも動きます。見つからないときは、メッセージを出してスキップします。macOS では `brew install coreutils` で `gcat` が入ります。テストは `HL_*` 環境変数、`HL_CONFIG`、XDG ディレクトリ、`HOME` を読まないので、手元の設定や設定ファイル、カスタムシンタックスの影響を受けません。`rustfmt.toml` には nightly でしか効かないオプションがあるため、フォーマットの確認には nightly の rustfmt を使います。手元で揃えるときは `cargo +nightly fmt` を実行してください。clippy は固定した Rust のバージョンで実行します（`.github/workflows/ci.yml` と `release.yml` の `clippy` ジョブ）。新しい Rust のリリースで lint が増えても、それだけで CI が失敗しないようにするためです。バージョンは両方のファイルで手動で上げ、新しく出た警告も同じ変更で直してください。`Cargo.toml` の `[lints]` で `clippy::pedantic` の lint も有効にしています。その箇所には合わない lint は、そこで `#[expect(clippy::..., reason = "...")]` を付けて止め、理由を書いてください。

| ファイル | 役割 |
| --- | --- |
| `src/main.rs` | エントリポイント。入力を表示しないコマンド（`--list-themes` など）の実行、引数と設定からの Printer と行の状態の組み立て、ページャか標準出力かの選択、エラー処理と終了コード。途中で見つかった問題の扱いもここで決める（`--map-syntax` の未知の言語はエラー、設定ファイルの未知の言語は警告、未知のテーマや壊れたテーマは警告してデフォルトに戻す、`src/print.rs` から届いた入力の問題は警告）。環境変数（`PAGER`、`LESS`、色深度）を読むのはここだけ。設定・キャッシュディレクトリもここで 1 回だけ求め（`config::Dirs::from_env`）、設定ファイルと `src/assets.rs` に渡す |
| `src/assets.rs` | デフォルトのテーマ、同梱の構文、設定ディレクトリのカスタム構文・テーマの読み込みと同梱分とのマージ、構文キャッシュ（`--build-cache`/`--clear-cache`）。ディレクトリは引数で受け取り、環境変数は読まない |
| `src/cli.rs` | コマンドライン引数の定義（clap）と、設定ファイルの適用（オプションのデフォルト値、フラグ、`map-syntax`）。ほかのモジュールは生のフィールドではなく、関連するオプションをまとめるメソッド（`numbering()`、`visibility()`、`limits()` など）で引数を読む |
| `src/color.rs` | 端末の色深度の判定、256 色へのフォールバック、色・太字・リセットのエスケープシーケンス |
| `src/config.rs` | 設定ファイルの読み込み（`HL_CONFIG` か設定ディレクトリ）と、設定・キャッシュディレクトリ（`Dirs`。`XDG_CONFIG_HOME`/`XDG_CACHE_HOME` と `HOME` から求める） |
| `src/diag.rs` | 標準エラー出力への警告・エラーメッセージ。標準エラー出力にはすべてここを通して書くので、`hl: ` の書式は 1 か所で決まる。エラーは原因まで含めて表示する（`パス: 理由`） |
| `src/encoding.rs` | UTF-8/UTF-16 の BOM 判定と UTF-16 のデコード |
| `build.rs` | two-face の JavaScript 構文の修正と、構文の同梱 |
| `src/highlight.rs` | 言語判定、`--map-syntax` の対応付け、サイズの上限、ハイライト出力（syntect。専用のスレッドで、時間の上限付き）。警告は出さず、ハイライトをあきらめた理由は呼び出し側に渡す |
| `src/lines.rs` | 行番号、空行の圧縮、`cat -v`/`-E`/`-T` の表記 |
| `src/pager.rs` | `$PAGER` からのページャのコマンドの組み立て、起動、終了待ち |
| `src/print.rs` | 各入力の順次出力（ハイライトありかなしか）、ヘッダ、文字コードの扱い。警告は出さず、入力の問題（読めない、ハイライトをあきらめた）はコールバックに渡す。`print_all` は `PrintOutcome`（入力の失敗があったか、出力を中断させた書き込みエラー）を返す |
| `src/stdout.rs` | 起動時に標準出力が閉じられていたかの判定（std が `/dev/null` で開き直す前に確かめる） |

### 依存関係

Dependabot（`.github/dependabot.yml`）が、依存クレートと GitHub Actions の更新 PR を毎週作ります。クレートのマイナー・パッチ更新は 1 本の PR にまとめ、メジャー更新は個別の PR にします。Actions の更新も 1 本にまとめます。

タグは付け替えられるので、ワークフローでは各 Action をコミットの SHA で固定し、バージョンをコメントに書いています（`uses: actions/checkout@<sha> # v7.0.1`）。Dependabot は SHA とコメントを一緒に更新します。Action を追加するときも同じ形で固定してください。ワークフローの `GITHUB_TOKEN` は読み取り専用です（書き込めるのはリリースを公開するジョブだけ）。`actions/checkout` には `persist-credentials: false` を付けているので、トークンが `.git/config` に残ってビルドから読まれることはありません。

[cargo-deny](https://github.com/EmbarkStudios/cargo-deny)（`.github/workflows/deny.yml`）は push と pull request のたびに依存を確認します。確認するのは、既知の脆弱性と yank されたバージョン（advisories）、ライセンス、bans、取得元（crates.io 以外は不可）です。依存が変わらなくても新しい脆弱性は公開されるので、advisories は毎日も確認します。許可するライセンスと、理由を付けて当面無視する advisory は `deny.toml` に書いてあります。手元で実行するときは次のようにします。

```sh
cargo install cargo-deny --locked
cargo deny check
```

構文は two-face のものを使いますが、JavaScript 構文だけは `build.rs` がビルド時に修正します（「動作の詳細」を参照）。two-face の更新でこの構文が変わり、修正を当てられなくなったときはビルドが失敗します。その場合は、two-face そのままの構文（たとえば `assets::bundled_syntaxes` が `two_face::syntax::extra_newlines()` を返すように変えたもの）で `printf 'x /*' | hl --color=always -l js` がまだ止まるかを確かめてください。止まるなら `build.rs` を直し、上流で直っていれば `build.rs` を削除します。

最小対応バージョン（MSRV、`Cargo.toml` の `rust-version`）は、依存の更新に必要なら上げてかまいません。ただし、最新の stable より 6 リリース以上古いバージョンまでにとどめます。脆弱性の修正を取り込むためなら、必ず上げます。そうした更新では、MSRV でテストする CI のジョブが失敗します。MSRV を上げるときは、`Cargo.toml` の `rust-version`、`.github/workflows/ci.yml` のマトリクス、この README のバージョン（「必要なもの」と「開発」）、`CHANGELOG.md` を一緒に更新してください。

### リリース

バージョン番号は[セマンティック バージョニング](https://semver.org/lang/ja/)に従い、リリースには `Cargo.toml` のバージョンに合わせて `vX.Y.Z` のタグを付けます。変更内容は [CHANGELOG.md](CHANGELOG.md) に記録します。

1. `Cargo.toml` の `version` を変更し、`cargo build` を実行して `Cargo.lock` を更新する
2. `CHANGELOG.md` の `Unreleased` の項目を新しい `## [X.Y.Z] - YYYY-MM-DD` の節に移し、末尾のリンクを更新する
3. コミットしてから、タグを push する

   ```sh
   git tag vX.Y.Z
   git push origin vX.Y.Z
   ```

すると `.github/workflows/release.yml` が 4 つのプラットフォーム向けにバイナリをビルドし、`hl` 自身で生成した補完スクリプトと man ページとまとめてアーカイブし、SHA-256 のチェックサムとともに GitHub のリリースとして公開します。リリースノートは CHANGELOG の該当の節です。タグが `Cargo.toml` のバージョンと一致しない場合や、CHANGELOG にそのバージョンの節がない場合は、何も公開せずに失敗します。CI のワークフローの結果は待たないので、ビルドの前にテスト（Linux と macOS）と clippy を自分で実行し、失敗したら何も公開しません。ビルドキャッシュは使わず、タグのソースだけからビルドします。

`v*` のタグを push できる人は誰でもリリースを公開できます。これを制限するには、リポジトリの設定（Settings → Rules → Rulesets）で `v*` を対象にしたタグのルールセットを作り、タグの作成・更新・削除をメンテナーに限ってください。

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
