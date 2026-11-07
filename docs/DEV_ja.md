# hl の開発ガイド

[English](DEV.md) | [日本語](DEV_ja.md) · [README](../README_ja.md)

このドキュメントは、`hl` 自体の開発のためのものです。ビルドとテスト、依存関係、リリース、テストカバレッジを説明します。コードの構成は [DESIGN_ja.md](DESIGN_ja.md) で説明しています。

## ビルドとテスト

```sh
cargo build
cargo test          # src/ のユニットテストと tests/cli/ の結合テスト（機能ごとに 1 ファイル）
cargo clippy
cargo fmt --check   # フォーマットの確認（rustfmt.toml）
```

push と pull request のたびに、GitHub Actions が `cargo fmt --check`、`cargo clippy`（警告はエラー扱い）、`cargo test` を実行します。Linux、macOS、Windows のそれぞれで、stable と最小対応バージョン（1.88）の Rust を使います。`cat` 互換のオプションについては、期待する出力をテストに直接書いています。それとは別に、GNU coreutils 9.0 以降の `cat`（`gcat` または `cat`）が見つかれば `hl` の出力と比べるテストも動きます。見つからないとき（Windows など）は、メッセージを出してスキップします。macOS では `brew install coreutils` で `gcat` が入ります。テストは `HL_*` 環境変数、`HL_CONFIG`、XDG ディレクトリ、`APPDATA`、`LOCALAPPDATA`、`HOME` を読まないので、手元の設定や設定ファイル、カスタムシンタックスの影響を受けません。エスケープシーケンスの除去、UTF-16 のデコード、`--max-size` と `--line-range` の値の解析には、プロパティテスト（[proptest](https://github.com/proptest-rs/proptest)）もあります。ランダムな入力を、読み込みの区切りもランダムにして、テストごとに 256 件試します。手元でもっと多く試すときは、`PROPTEST_CASES=20000 cargo test` のように `PROPTEST_CASES` を指定してください。失敗したケースは `proptest-regressions/` に保存されます。コミットしておくと、以降は最初にそのケースを試します。`rustfmt.toml` には nightly でしか効かないオプションがあるため、フォーマットの確認には nightly の rustfmt を使います。手元で揃えるときは `cargo +nightly fmt` を実行してください。`.gitattributes` で、Windows で `core.autocrlf` を使っていても改行を LF のままチェックアウトするので、テストはどの OS でも同じファイルを見ます。Linux や macOS から Windows 向けのコードを型検査するときは、一度 `rustup target add x86_64-pc-windows-msvc` を実行してから `cargo clippy --all-targets --target x86_64-pc-windows-msvc` を実行します。リンクはしないので Windows のツールチェーンは要りませんが、テストを実行できるのは Windows だけです。clippy は固定した Rust のバージョンで実行します（`.github/workflows/ci.yml` と `release.yml` の `clippy` ジョブ）。新しい Rust のリリースで lint が増えても、それだけで CI が失敗しないようにするためです。バージョンは両方のファイルで手動で上げ、新しく出た警告も同じ変更で直してください。`Cargo.toml` の `[lints]` で `clippy::pedantic` の lint も有効にしています。その箇所には合わない lint は、そこで `#[expect(clippy::..., reason = "...")]` を付けて止め、理由を書いてください。`unsafe_code` も deny にしています。`unsafe` を許可しているのは、`src/stdout.rs`（`main` より前の fd 1 の確認と Windows のコンソールの呼び出し）と、`tests/cli/streams.rs` の `with_closed_stdout`（`pre_exec`）だけです。どちらも `#[expect(unsafe_code, reason = "...")]` を付けています。新しく `unsafe` を書くときは `src/stdout.rs` に置くか、これらと同じように安全な関数で包んでください。ブロックごとに、なぜ安全なのかを `// SAFETY:` コメントで書いてください。

一部のテストは、ハイライトがいつまでも終わらない行で `hl` があきらめる動作を、syntect を無限ループさせる構文 `LOOPING_SYNTAX` で確かめています。この構文でハイライトした入力は、テストプロセスが終わるまで回り続けるスレッドを 1 本ずつ残します（[ハイライトのパイプライン](DESIGN_ja.md#ハイライトのパイプライン)を参照）。そのぶん他のテストがすべて遅くなり、コアの少ない CI のランナーでは、時間を区切って待つテストが失敗することがあります。この構文は止まる動作そのもののテストだけに使い、あきらめたあとの動作は、すぐに失敗して同じ経路を通る `FAILING_SYNTAX` で確かめてください。結合テスト（`tests/cli/`）は、起動した `hl` がそれぞれ終了するので、気にせず使えます。

各ソースファイルの役割は [DESIGN_ja.md](DESIGN_ja.md#モジュール) にまとめています。

## 依存関係

Dependabot（`.github/dependabot.yml`）が、依存クレートと GitHub Actions の更新 PR を毎週作ります。クレートのマイナー・パッチ更新は 1 本の PR にまとめ、メジャー更新は個別の PR にします。Actions の更新も 1 本にまとめます。

タグは付け替えられるので、ワークフローでは各 Action をコミットの SHA で固定し、バージョンをコメントに書いています（`uses: actions/checkout@<sha> # v7.0.1`）。Dependabot は SHA とコメントを一緒に更新します。Action を追加するときも同じ形で固定してください。ワークフローの `GITHUB_TOKEN` は読み取り専用です（書き込めるのはリリースを公開するジョブだけ）。`actions/checkout` には `persist-credentials: false` を付けているので、トークンが `.git/config` に残ってビルドから読まれることはありません。

[cargo-deny](https://github.com/EmbarkStudios/cargo-deny)（`.github/workflows/deny.yml`）は push と pull request のたびに依存を確認します。確認するのは、既知の脆弱性と yank されたバージョン（advisories）、ライセンス、bans、取得元（crates.io 以外は不可）です。依存が変わらなくても新しい脆弱性は公開されるので、advisories は毎日も確認します。許可するライセンスと、理由を付けて当面無視する advisory は `deny.toml` に書いてあります。手元で実行するときは次のようにします。

```sh
cargo install cargo-deny --locked
cargo deny check
```

構文は two-face のものを使いますが、JavaScript 構文と Scala 構文だけは `build.rs` がビルド時に修正します（[動作の詳細](SPEC_ja.md#動作の詳細)を参照）。two-face の更新でどちらかの構文が変わり、修正を当てられなくなったときは、その構文の名前を出してビルドが失敗します。その場合は、two-face そのままの構文（たとえば `assets::bundled_syntaxes` が `two_face::syntax::extra_newlines()` を返すように変えたもの）で、JavaScript なら `printf 'x /*' | hl --color=always -l js`、Scala なら `printf ':a@<' | hl --color=always -l scala` がまだ止まるかを確かめてください。止まるなら `build.rs` の修正を直し、上流で直っていればその修正を削除します。

最小対応バージョン（MSRV、`Cargo.toml` の `rust-version`）は、依存の更新に必要なら上げてかまいません。ただし、最新の stable より 6 リリース以上古いバージョンまでにとどめます。脆弱性の修正を取り込むためなら、必ず上げます。そうした更新では、MSRV でテストする CI のジョブが失敗します。MSRV を上げるときは、`Cargo.toml` の `rust-version`、`.github/workflows/ci.yml` のマトリクス、README（「必要なもの」）とこのドキュメント（「ビルドとテスト」）のバージョン、`CHANGELOG.md` を一緒に更新してください。

## リリース

バージョン番号は[セマンティック バージョニング](https://semver.org/lang/ja/)に従い、リリースには `Cargo.toml` のバージョンに合わせて `vX.Y.Z` のタグを付けます。変更内容は [CHANGELOG.md](../CHANGELOG.md) に記録します。

1. `Cargo.toml` の `version` を変更し、`cargo build` を実行して `Cargo.lock` を更新する
2. `CHANGELOG.md` の `Unreleased` の項目を新しい `## [X.Y.Z] - YYYY-MM-DD` の節に移し、末尾のリンクを更新する
3. コミットしてから、タグを push する

   ```sh
   git tag vX.Y.Z
   git push origin vX.Y.Z
   ```

すると `.github/workflows/release.yml` が 5 つのプラットフォーム向けにバイナリをビルドし、ドキュメント（README、`docs/`、変更履歴）や、`hl` 自身で生成した補完スクリプトと man ページとまとめてアーカイブし（Windows は zip、それ以外は tar.gz）、SHA-256 のチェックサムとともに GitHub のリリースとして公開します。リリースノートは CHANGELOG の該当の節です。タグが `Cargo.toml` のバージョンと一致しない場合や、CHANGELOG にそのバージョンの節がない場合は、何も公開せずに失敗します。CI のワークフローの結果は待たないので、ビルドの前にテスト（Linux、macOS、Windows）と clippy を自分で実行し、失敗したら何も公開しません。ビルドキャッシュは使わず、タグのソースだけからビルドします。

`v*` のタグを push できる人は誰でもリリースを公開できます。これを制限するには、リポジトリの設定（Settings → Rules → Rulesets）で `v*` を対象にしたタグのルールセットを作り、タグの作成・更新・削除をメンテナーに限ってください。

## テストカバレッジ

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
