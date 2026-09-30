# Contributing to Adage

Read the [README](README.md) first, then the [design spec](docs/Adage%20—%20Design%20Spec.md),
[architecture](docs/Adage%20—%20Architecture.md) and
[implementation plan](docs/Adage%20—%20Implementation%20Plan.md). Security reports go through
[SECURITY.md](SECURITY.md), never a public issue.

## Build

You need stable Rust (`rustup`) and, for the plugin and npm tests, Node.js 18 or later.

```bash
cargo build --workspace --locked
cargo build --release -p adage-cli     # target/release/adage
plugin/build.sh                        # release build staged into the Claude Code plugin, with SHA256SUMS
```

`Cargo.lock` is committed; build and test with `--locked`.

## Test

CI (`.github/workflows/ci.yml`) runs all of this on macOS arm64 and x64, Linux x64 and arm64, and
Windows x64. Run it locally before sending a change:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked        # includes the normaliser's golden fixtures and, with node, the OpenCode plugin test
node npm/test/launcher.test.mjs        # the npm launcher, against target/debug/adage
plugin/test/session-start.test.sh      # how the Claude Code plugin finds its binary
node scripts/check-plugin.mjs          # plugin and marketplace versions and hook paths
claude plugin validate --strict plugin/claude-code && claude plugin validate --strict .
cargo deny check                       # licenses, advisories, bans, sources (deny.toml)
scripts/cross-check.sh                 # clippy for every release target, including Windows, from any host
```

`scripts/cross-check.sh` proves that code compiles for other targets (it stubs out the C compiler
tree-sitter's build script needs, so it never links). Only CI proves that it links and passes tests
there. Code that differs by platform goes behind `cfg(unix)` / `cfg(windows)`, and tests that need
Unix-only facilities (symlinks, `#!/bin/sh` scripts) are `#[cfg(unix)]`.

## Evals

`demo/` runs real headless agents on a sandbox repository and scores them with hidden checks (see
[demo/SANDBOX.md](demo/SANDBOX.md)). Evals call a paid model API and take a while, so they are not
part of CI: run them yourself when a change affects what directives say or when they are given, and
include the numbers in the pull request.

```bash
python3 demo/sandbox/selfcheck.py      # every work item fails untouched and passes with its reference solution
python3 demo/run.py all                # train, learn, held-out eval in two arms
```

## Changes

- Keep changes focused, with tests. Anything on the hook path must fail open: exit 0 and print
  nothing on any error.
- Record user-visible changes in [CHANGELOG.md](CHANGELOG.md) under "Unreleased".
- Third-party dependencies must use a license on the allow list in `deny.toml` (permissive only).
  `scripts/notice.sh` regenerates the third-party NOTICE with cargo-about.

## Sign-off

Adage is licensed under the [Apache License 2.0](LICENSE). Contributions are accepted under the
same license, with a [Developer Certificate of Origin](https://developercertificate.org/) sign-off
on every commit (decision D2): commit with `git commit -s`, which adds

    Signed-off-by: Your Name <you@example.com>

and certifies that you wrote the change or otherwise have the right to submit it under the
project's license. There is no contributor license agreement. CI checks pull-request commits for
the sign-off.

## Releasing (maintainers)

Releases are built by [dist](https://github.com/axodotdev/cargo-dist) from `dist-workspace.toml`.

1. Bump `version` in `[workspace.package]` of `Cargo.toml`, and the versions in
   `plugin/claude-code/.claude-plugin/plugin.json` and `.claude-plugin/marketplace.json`
   (`scripts/check-plugin.mjs` fails CI until they agree). In CHANGELOG.md, move "Unreleased" under
   a `## <version> - <date>` heading: its text becomes the GitHub Release notes.
2. After editing `dist-workspace.toml`, run `dist generate` and commit `.github/workflows/release.yml`.
3. Tag and push: `git tag v0.1.0 && git push origin v0.1.0`. The release workflow builds every
   target, attests each archive (Sigstore), attaches checksums and a CycloneDX SBOM, creates the
   GitHub Release, and runs the publish jobs.

Publishing needs, in the repository settings: the `HOMEBREW_TAP_TOKEN` secret (push access to
`stevenvaughanbooth/homebrew-tap`), and for npm the `NPM_TOKEN` secret plus the variable
`ADAGE_NPM_PUBLISH=true`. The npm package is `@adage-sh/cli` (decision D3), in the `adage-sh` npm org;
`npm/config.mjs` is the one place the name lives. The `NPM_TOKEN` must be an automation token of
an account that can publish to that org. macOS signing (decision D4) is off until the Apple
Developer ID secrets exist; see the note in `dist-workspace.toml`.
