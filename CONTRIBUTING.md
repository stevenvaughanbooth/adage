# Contributing to Adage

Read the [README](README.md) first, then the [design spec](docs/Adage%20—%20Design%20Spec.md),
[architecture](docs/Adage%20—%20Architecture.md) and
[implementation plan](docs/Adage%20—%20Implementation%20Plan.md). [Components and boundaries](docs/architecture.md) is the current map of
what each crate owns and may depend on; `cargo test -p xtask` enforces it. Security reports go through
[SECURITY.md](SECURITY.md), never a public issue.

## Build

You need stable Rust (`rustup`) and, for the plugin and npm tests, Node.js 18 or later.

```bash
cargo build --workspace --locked
cargo build --release -p adage-cli     # target/release/adage
plugin/build.sh                        # release build of adage, adage-exec, adage-train staged into the Claude Code plugin, with SHA256SUMS
```

`Cargo.lock` is committed; build and test with `--locked`.

## Test

CI (`.github/workflows/ci.yml`) runs all of this on macOS arm64 and x64, Linux x64 and arm64, and
Windows x64. Run it locally before sending a change:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked        # includes the normaliser's table-driven cases and, with node, the OpenCode plugin test
node npm/test/launcher.test.mjs        # the npm launcher, against target/debug/adage
plugin/test/session-start.test.sh      # how the Claude Code plugin finds its three binaries (build adage-exec and adage-train too)
node scripts/check-plugin.mjs          # plugin and marketplace versions and hook paths
node scripts/test-check-plugin.mjs     # check-plugin refuses a second plugin or an on-PATH description
node scripts/test-stage-plugin.mjs     # release staging: three binaries per target; bad archive checksums refused (Unix)
cargo xtask release-check              # every version file agrees; release.yml is what dist generates (needs dist for that half)
claude plugin validate --strict plugin/claude-code && claude plugin validate --strict .
cargo deny check                       # licenses, advisories, bans, sources (deny.toml)
scripts/cross-check.sh                 # clippy for every release target, including Windows, from any host
```

On a pull request, CI tests only the packages a change can affect: the changed ones, the ones whose
tests read a changed fixture, and everything that depends on them. To see that set locally, run
`cargo xtask affected` (against `origin/main`; `--files <paths>` to ask about specific paths). A
test that reads a file outside its crate must be declared in `READS` in `xtask/src/affected.rs`.

`scripts/cross-check.sh` proves that code compiles for other targets (it stubs out the C compiler
tree-sitter's build script needs, so it never links). Only CI proves that it links and passes tests
there. Code that differs by platform goes behind `cfg(unix)` / `cfg(windows)`, and tests that need
Unix-only facilities (symlinks, `#!/bin/sh` scripts) are `#[cfg(unix)]`.

Before a Unix plugin release, also run the optimized artifact gate:

```bash
cargo test --release --locked -p adage-cli --test zero_config_plugin release_plugin_startup -- --nocapture
cargo build --release --locked -p adage-cli -p adage-exec
plugin/test/session-start.test.sh target/release/adage
```

The startup gate stages a checksummed release binary and dispatches the checked-in plugin hook
registration. It requires a real background bootstrap from 12 historical sessions, a useful
post-failure response, first startup under 1,500 ms and all ten repeated starts under 200 ms.
Those latency limits apply to optimized release artifacts; the debug integration suite tests
semantics without applying release budgets to a larger instrumented binary. Tests that need
`ADAGE_TEST_NO_SPAWN` are explicitly ignored in release builds because the recorder and fault
injection exist only with debug assertions. The release gate uses neither, and cannot silently
skip an unsupported Unix target. The shell suite additionally checks that bundled delegation
installs its verified learner companion and actually bootstraps without an `adage` on PATH.

Local success on one machine does not establish the full release matrix. CI must link and test
each promised target, and release staging must have checksummed archives for every required
binary. Windows automatic execution remains unavailable (E0 only); the Unix startup gate does
not validate Windows Claude Code plugin invocation. Validate that harness integration separately
before describing Windows plugin support as verified.

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

Releases are built by [dist](https://github.com/axodotdev/cargo-dist) from `dist-workspace.toml`;
[docs/release.md](docs/release.md) is the whole procedure and the first release's checklist.

1. `cargo xtask bump <version>`: writes the version into `Cargo.toml`, `Cargo.lock`, the plugin and
   marketplace manifests and the npm templates, renames CHANGELOG.md's Unreleased heading to
   `## <version> - <date>` (its text becomes the GitHub Release notes), and runs the checks. Commit
   it with `git commit -s`.
2. `git tag v<version>`.
3. `git push origin HEAD v<version>`. The release workflow builds every target, attests each
   archive (Sigstore), attaches checksums and a CycloneDX SBOM, creates the GitHub Release, writes
   the `marketplace` branch, and publishes npm when it is switched on.

`cargo xtask release-check` (CI runs it with dist installed) fails while the version files
disagree or `.github/workflows/release.yml` is not what dist generates from `dist-workspace.toml`
plus the one documented SBOM patch; after editing `dist-workspace.toml`, regenerate as its header
says. npm publishing needs the `NPM_TOKEN` secret plus the variable `ADAGE_NPM_PUBLISH=true`. The
npm package is `@adage-sh/cli` (decision D3), in the `adage-sh` npm org; `npm/config.mjs` is the
one place the name lives. The `NPM_TOKEN` must be an automation token of an account that can
publish to that org. Homebrew is off the release until macOS signing (decision D4) exists; see the
notes in `dist-workspace.toml`.
