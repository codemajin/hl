# hl

[English](README.md) | 日本語

`hl` は、ファイルの種類を自動で判定してシンタックスハイライトを付ける `cat` 代替コマンドです。Rust で書かれています。

- 拡張子、ファイル名（`Makefile`、`Cargo.lock` など）、shebang から言語を判定します。
- 構文定義とテーマは [bat](https://github.com/sharkdp/bat) と同じセットを使います（[two-face](https://crates.io/crates/two-face) 経由）。TOML、TypeScript、Dockerfile などにも対応しています。
- 出力先が端末でなければ何も加工せずに出力します。そのため、`cat` のエイリアスにしてもパイプやリダイレクトの結果は変わりません。

## 必要なもの

- Rust ツールチェーン（edition 2024 に対応した Rust 1.85 以降）
- ページャ機能（`-p`）を使う場合は `less`。環境変数 `$PAGER` を設定していればそちらを使います。
- 24bit カラーに対応した端末（Windows Terminal、iTerm2、GNOME Terminal、WezTerm など）

## インストール

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

### アンインストール

```sh
cargo uninstall hl                  # --root を指定した場合は同じ --root を付ける
```

## 使い方

```sh
hl src/main.rs Cargo.toml        # 言語を自動判定してハイライトする
hl -n file.py                    # 行番号付き（複数ファイルでも通し番号）
cat script | hl -l bash          # stdin から読み、言語を明示する
hl -t Nord file.rs               # テーマを指定する
hl -p big.rs                     # ページャで表示する
hl --list-themes                 # テーマの一覧
hl --list-languages              # 対応言語と拡張子の一覧
```

### オプション

| オプション | 説明 |
| --- | --- |
| `-n`, `--number` | 行番号を表示します。`cat -n` と同じく、複数のファイルにまたがって通し番号を振ります |
| `-l`, `--language <LANG>` | 自動判定を使わずに言語を指定します。言語名か拡張子で指定できます（例: `rust`、`py`） |
| `-t`, `--theme <THEME>` | テーマを指定します。既定は `base16-ocean.dark` です |
| `-p`, `--paging` | ページャで表示します（`$PAGER`、未設定なら `less -RFX`）。1 画面に収まる場合はそのまま終了します |
| `--color <WHEN>` | 色を付ける条件を `auto`、`always`、`never` から選びます。既定の `auto` では、出力先が端末のときだけ色を付けます |
| `--list-themes` | 使えるテーマを一覧表示します |
| `--list-languages` | 対応している言語を一覧表示します |

ファイルを指定しないか `-` を指定すると、標準入力から読み込みます。

### 環境変数

| 変数 | 説明 |
| --- | --- |
| `HL_THEME` | 既定のテーマ。`-t` を指定した場合はそちらが優先されます |
| `PAGER` | `-p` で使うページャ |

### 動作の詳細

- 言語は `-l` の指定、拡張子、ファイル名、1 行目（shebang など）の順に判定します。どれでも判定できなければプレーンテキストとして出力します。
- ファイルの先頭 8KB に NUL バイトが含まれていればバイナリとみなし、ハイライトせずにそのまま出力します。
- 開けないファイルがあると `hl: <path>: <理由>` を標準エラー出力に表示し、残りのファイルを処理したあと終了コード 1 で終了します。
- `hl file | head` のように出力先が途中で閉じられても、エラーにはせず正常終了します。

## `cat` の代わりに使う

シェルの設定ファイル（`~/.bashrc`、`~/.zshrc` など）にエイリアスを追加します。

```sh
alias cat='hl'
```

テーマを固定したい場合は、あわせて `HL_THEME` を設定してください。

```sh
export HL_THEME='Monokai Extended'
```

エイリアスを経由せずに本来の `cat` を実行したいときは、`\cat` または `command cat` を使います。

## 開発

```sh
cargo build
cargo test          # tests/cli.rs の結合テスト
cargo clippy
```

| ファイル | 役割 |
| --- | --- |
| `src/main.rs` | エントリポイント。入力の振り分け、出力先の選択、エラー処理 |
| `src/cli.rs` | コマンドライン引数の定義（clap） |
| `src/highlight.rs` | 言語判定とハイライト出力（syntect） |
| `src/pager.rs` | ページャの起動と終了待ち |
