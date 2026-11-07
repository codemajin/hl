# hl development guide

[English](DEV.md) | [日本語](DEV_ja.md) · [README](../README.md)

This document is for working on `hl` itself: building and testing, dependencies, releasing and test coverage. How the code is organized is described in [DESIGN.md](DESIGN.md).

## Building and testing

```sh
cargo build
cargo test          # unit tests in src/ and integration tests in tests/cli/ (one file per feature)
cargo clippy
cargo fmt --check   # formatting (rustfmt.toml)
```

GitHub Actions runs `cargo fmt --check`, `cargo clippy`, `cargo doc` and `cargo test` on every push and pull request, the tests on Linux, macOS and Windows with stable Rust and the minimum supported version (1.88). Warnings fail only clippy and the doc build (`RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --document-private-items`), which run on a fixed Rust version (see below); the tests don't fail on a warning, so that one added in a new stable release doesn't fail CI by itself. The line coverage is checked too (see [Test coverage](#test-coverage)). The tests spell out the output they expect from the `cat` options, including the `-v` notation of all 256 bytes. The unit tests also make reads fail (with `Chunked` in `src/test_support.rs`) to check that an interrupted read is tried again and that any other error is reported. One more test compares `hl` with GNU coreutils `cat` 9.0 or later, found as `gcat` or `cat`, and is skipped with a message when there is none (as on Windows); on macOS, `brew install coreutils` provides `gcat`. On Linux, two pager tests run `hl --paging=always` on a pseudo-terminal made by `script` (util-linux), with a `PAGER` that writes what it reads to a file: one checks that the output reaches the pager, the other that a pager quitting early, as `less` does on `q`, is not an error. They are skipped with a message when there is no `script`. The tests ignore the `HL_*` variables, `HL_CONFIG`, the XDG directories, `APPDATA`, `LOCALAPPDATA` and `HOME`, so your own settings, config file and custom syntaxes don't affect them. The escape removal, the UTF-16 decoding and the parsing of `--max-size`/`--line-range` also have property tests ([proptest](https://github.com/proptest-rs/proptest)) that try random inputs, split at random places between reads, 256 cases each. Set `PROPTEST_CASES` (such as `PROPTEST_CASES=20000 cargo test`) to try more locally. A failing case is saved in `proptest-regressions/`; commit it, so that it is tried first from then on. Once a week, `.github/workflows/proptest.yml` runs the tests with 4096 cases each (it can also be started by hand from the Actions tab); a case found there is left on the runner, so copy the minimal input from the log into `proptest-regressions/` or a test. Formatting is checked with nightly rustfmt, because some options in `rustfmt.toml` are nightly-only; run `cargo +nightly fmt` to match it. `.gitattributes` checks the files out with LF line endings, also on Windows with `core.autocrlf`, so that the tests see the same files everywhere. To type-check the Windows code from Linux or macOS, run `rustup target add x86_64-pc-windows-msvc` once and then `cargo clippy --all-targets --target x86_64-pc-windows-msvc`; it doesn't link, so it needs no Windows toolchain, but only Windows runs the tests. Clippy and the doc build run on a fixed Rust version (the `clippy` jobs in `.github/workflows/ci.yml` and `release.yml`, and the `coverage` job in `ci.yml`), so that lints added in a new Rust release don't fail CI by themselves; raise it by hand in all three jobs, and fix the new warnings in the same change. The `[lints]` table in `Cargo.toml` turns on the `clippy::pedantic` lints as well; where one of them is wrong for a piece of code, silence it there with `#[expect(clippy::..., reason = "...")]`, saying why. It also denies `unsafe_code`: `unsafe` is allowed only in `src/stdout.rs` (the check of fd 1 before `main` and the Windows console calls) and in `with_closed_stdout` of `tests/cli/streams.rs` (`pre_exec`), each with `#[expect(unsafe_code, reason = "...")]`. Keep new `unsafe` code in `src/stdout.rs` or behind a safe function like those, with a `// SAFETY:` comment on each block saying why it holds.

Some tests check how `hl` gives up on a line that never finishes highlighting, with `LOOPING_SYNTAX`, a syntax that makes syntect loop forever. Each input highlighted with it leaves a thread spinning until the test process exits (see [the highlighting pipeline](DESIGN.md#highlighting-pipeline)), which slows down every other test, so that the ones waiting for a deadline can fail on a CI runner with few cores. Use it only to test the stall itself, and test what happens after giving up with `FAILING_SYNTAX`, which fails at once and takes the same path from there. Integration tests (`tests/cli/`) can use it more freely, as each `hl` they start exits on its own.

The integration tests read only the fixtures in `tests/cli/fixtures/` (`sample.toml` and `sample.rs`, through `fixture()` in `tests/cli/common.rs`) or files they write into a temporary directory, never the project's own files such as `Cargo.toml`, so that what they check does not change when those files do. The unit tests share their helpers through `src/test_support.rs`, including the bundled syntaxes that `hl` uses (`bundled()`), so that they highlight with the same syntaxes as `hl` itself.

The role of each source file is listed in [DESIGN.md](DESIGN.md#modules).

## Dependencies

Dependabot (`.github/dependabot.yml`) opens update PRs every week, for the crates and for the GitHub Actions. Minor and patch updates of the crates come in one PR, a major update in a PR of its own, and the Actions in one PR. A version is taken in only after it has been out for 7 days (`cooldown`), so that one published from a hijacked account is likely to be found and yanked first; security updates don't wait.

The workflows pin each action to a commit SHA, with the version in a comment (`uses: actions/checkout@<sha> # v7.0.1`), since a tag can be moved to other code; Dependabot updates the SHA and the comment together. When adding an action, pin it the same way. The workflows get a read-only `GITHUB_TOKEN` (only the job that publishes a release can write to the repository, and only the build job of the release can sign provenance attestations), and `actions/checkout` runs with `persist-credentials: false`, so the token isn't left in `.git/config` for the build to read.

[cargo-deny](https://github.com/EmbarkStudios/cargo-deny) (`.github/workflows/deny.yml`) checks the dependencies on every push and pull request: known vulnerabilities and yanked versions (advisories), licenses, bans and sources (crates.io only). The advisories are also checked every day, since new ones appear while the dependencies stay the same. The allowed licenses, and the advisories ignored for now with the reason, are listed in `deny.toml`. To run it locally:

```sh
cargo install cargo-deny --locked
cargo deny check
```

The syntaxes come from two-face, but `build.rs` fixes its JavaScript and Scala syntaxes at build time (see [Behavior details](SPEC.md#behavior-details)). If an update of two-face changes one of them so that its fix no longer applies, the build fails, naming the syntax. Then check whether `printf 'x /*' | hl --color=always -l js` (JavaScript) or `printf ':a@<' | hl --color=always -l scala` (Scala) still hangs with two-face's own syntaxes (for example by having `assets::bundled_syntaxes` return `two_face::syntax::extra_newlines()`), and update the fix in `build.rs`, or remove it once it is fixed upstream.

The minimum supported Rust version (MSRV, `rust-version` in `Cargo.toml`) may be raised when an update needs it, to a version at least six releases behind the latest stable; to get a fix for a vulnerability, it is always raised. Such an update fails the CI job with the MSRV. When raising it, update `rust-version` in `Cargo.toml`, the matrix in `.github/workflows/ci.yml`, the version in the README (Requirements) and in this document (Building and testing) and `CHANGELOG.md` together.

## Releasing

Versions follow [Semantic Versioning](https://semver.org/), and a release is tagged `vX.Y.Z` with the version in `Cargo.toml`. Changes are recorded in [CHANGELOG.md](../CHANGELOG.md).

1. Set `version` in `Cargo.toml` and run `cargo build` to update `Cargo.lock`.
2. In `CHANGELOG.md`, move the entries under `Unreleased` to a new `## [X.Y.Z] - YYYY-MM-DD` section, and update the links at the end.
3. Commit, then push the tag:

   ```sh
   git tag vX.Y.Z
   git push origin vX.Y.Z
   ```

`.github/workflows/release.yml` then builds the binaries for the five platforms, packs each (in a zip for Windows, a tar.gz otherwise) with the documentation (the READMEs, `docs/` and the changelog) and the completion scripts and the man page generated by `hl` itself, and publishes them, with SHA-256 checksums and a build provenance attestation for each archive, as a GitHub release whose notes are the changelog section. It fails without publishing anything if the tag doesn't match the version in `Cargo.toml` or the changelog has no section for it. Before the builds it runs the tests (on Linux, macOS and Windows), clippy and the doc build, since it doesn't wait for the CI workflow, and publishes nothing if they fail. Nothing in it uses a build cache, so the release is built from the tagged source only.

The checksums are made in the same job and published in the same release as the archives, so they find a broken download but not a replaced archive. The attestation, made by `actions/attest` in the build job, is signed with a short-lived Sigstore certificate for the workflow run (through the job's OIDC token, `id-token: write`) and stored in the repository's attestations (`attestations: write`), apart from the release; only the build job has these permissions. Users check it with `gh attestation verify <archive> --repo codemajin/hl --signer-workflow codemajin/hl/.github/workflows/release.yml`, as the README describes. Provenance was left out at first (issue 030) because artifact attestations need GitHub Enterprise Cloud for a private repository; it was added after the repository became public on 2026-11-03.

Anyone who can push a `v*` tag can publish a release, so the repository settings limit who can, and what can change afterwards. They are set by hand in the repository settings, and should be:

- A tag ruleset (Settings → Rules → Rulesets) that targets `v*` and restricts creating, updating and deleting the tags, with only the maintainers in the bypass list.
- A branch ruleset on `master` that blocks deleting it and force-pushing to it.
- Immutable releases (Settings → General → Releases), so that the assets and the tag of a published release can't be changed. `gh release create` in the workflow uploads the assets before it publishes the release, so it works with them.

To check them, run `gh api repos/codemajin/hl/rulesets` and `gh api repos/codemajin/hl/immutable-releases`.

## Test coverage

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

The `coverage` job in `.github/workflows/ci.yml` runs on Linux, with the Rust version of the clippy job, since the figure changes with the compiler (nightly gives a different one), and fails when the line coverage is below 95%, a little under the current figure (96.7%), so that a change that leaves much of its code untested is noticed. When it fails, add tests rather than lower the threshold; lower it only for a reason, such as code that can't run on Linux, and say why in the same change. To check it locally as CI does:

```sh
rustup toolchain install 1.99 --component llvm-tools-preview
cargo +1.99 llvm-cov --fail-under-lines 95
```
