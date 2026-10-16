# O1 · O2 notes: one plugin, three binaries; one bump, one check, no Homebrew

Onboarding plan (`../onboarding-plan.md`) chunks O1 and O2, built on `claude/mods-poc-m98u3o` from
`b6548a2`, alongside O4/O10 and O5/O7/O8 in the same tree. Nothing here is committed, tagged or
pushed; O3 (shipping v0.1.0) is the owner's, and its checklist is in [`docs/release.md`](../../release.md).

## For the agents working beside this one (read first)

Edits O1 needed in files another agent owns.

1. **O4, `crates/adage-cli/src/detect.rs` (done by O4).** The shim places a third link,
   `${CLAUDE_PLUGIN_DATA}/bin/adage-train` (to `adage-train-<sha256>` in the same directory, or to
   an `adage-train` on `PATH` / beside the serving `adage`; absent when there is none). O4 added
   `detect::plugin_companions(data)` (`detect.rs:90`), which reports it.
2. **O8, the shim's half (open; the file is this chunk's).** Removing superseded
   `bin/<name>-<sha>` copies is O8's (safe only once O4's registrations name the stable link) and
   is not built here. Its place is the end of `bundled()`, after `ln -sfn "$NAME-$GOT"
   "$DATA/bin/$NAME"`: remove every `"$DATA/bin/$NAME"-*` other than `"$NAME-$GOT"` whose name is
   exactly `<NAME>-` plus 64 hex characters (match the full length, so `NAME=adage` never removes
   `adage-exec-*` or `adage-train-*`), with T-O8-04 in `plugin/test/session-start.test.sh`.
3. **O5, `crates/adage-cli/tests/zero_config_plugin.rs`.** This chunk edited only
   `World::staged_plugin_with` (it stages `adage-train` when it is built beside `adage`) and added
   `train_runs_from_the_plugin` (T-O1-03) after `exec_marker_picks_the_hook_binary`.

## What was built

### O1 · One plugin, three binaries, no retired alias

- **The alias is gone.** `plugin/claude-code-exec/` deleted; its marketplace entry, its
  `ci.yml` validate line, its `check-plugin.mjs` branch, its shim test (T-A8-04, marked retired in
  `test-plan.md`), its `build.sh` line and its `.gitignore` line with it. `check-plugin.mjs` now
  fails if the directory comes back or the marketplace lists anything but `adage`.
- **The shim places `adage-train`** (`plugin/claude-code/scripts/session-start`, block 3): bundled
  and verified against `dist/SHA256SUMS`, else `adage-train` on `PATH`, else beside the `adage`
  that serves (npm's platform package); a mismatched bundled one is not replaced from `PATH` (as
  with `adage-exec`). Missing or mismatched only removes `bin/adage-train`: `adage` keeps serving
  and nothing is printed, since no hook runs it. `quiet()` (nothing can serve) removes
  `bin/adage-train` too. Every path still exits 0.
- **The fallback names channels that exist:** `adage: no binary for <os-arch>; reinstall with
  /plugin install adage@adage, or npm i -g @adage-sh/cli` and `adage: the bundled binary for
  <os-arch> does not match its checksum; reinstall with /plugin install adage@adage, or npm i -g
  @adage-sh/cli`. Homebrew is named nowhere in the shim's code (check-plugin enforces it).
- **Staging needs all three.** `scripts/stage-plugin.mjs` takes the plugin's binaries as `adage`
  plus `npm/config.mjs` `COMPANIONS` (one list for npm and the plugin), so without
  `--allow-missing` it fails with `plugin/claude-code: no adage-train for <target>` unless every
  target has `adage`, `adage-train`, and `adage-exec` except on Windows; one `SHA256SUMS` covers
  them. `check-plugin.mjs --staged` checks the same three per target.
- **`plugin/build.sh`** builds `-p adage-cli -p adage-exec -p adage-train`, stages the host's three
  and writes `SHA256SUMS`, and is now testable without a build: `--no-build`, `--from <dir>`,
  `--dist <dir>`, `--target <triple>`. It refuses, writing nothing, unless every binary is there;
  it writes `SHA256SUMS` aside, checks it with `sha256sum -c`/`shasum -c`, then moves it in place.
  The shim test stages its bundled fixture through it, so the writer is tested against the reader.
- **The marketplace description tells the truth** (`.claude-plugin/marketplace.json`): "… Three
  binaries (adage, adage-exec, adage-train) are bundled for macOS and Linux and checked against
  their checksums before they run; nothing is downloaded. Windows hooks are untested and execute
  nothing." (It said Windows binaries were bundled and worked, execution included.)
- **`adage train` from the plugin** (T-O1-03): `train.rs::locate` is unchanged; the running
  `adage` is `<data>/bin/adage-<sha>`, so `<data>/bin/adage-train` is beside it.

### O2 · Release tooling: one bump, one check, Homebrew off the first release

- **`cargo xtask bump <version> [--date YYYY-MM-DD] [--no-changelog]`** (`xtask/src/release.rs`):
  writes the version into `Cargo.toml` `[workspace.package]`, every lockfile entry of a member
  with `version.workspace = true`, `plugin/claude-code/.claude-plugin/plugin.json`,
  `.claude-plugin/marketplace.json` (`/metadata/version` and every `/plugins/*/version`), and the
  npm templates `npm/cli/package.json` and `npm/platform/package.json`; renames CHANGELOG.md's first
  `## Unreleased`, `## [Unreleased]` or `## <x> - Unreleased` heading to `## <version> - <date>`
  (UTC today by default); then runs the version check and `node scripts/check-plugin.mjs` and
  prints the tag and push lines. Only version strings change (text edits; JSON key order and
  formatting kept). It computes every file before writing any, and refuses a malformed version,
  a bad date, a CHANGELOG with no Unreleased heading, or a version already released.
- **`cargo xtask release-check [--require-dist] [--dist <path>]`**: (1) the version check: every
  place above equals `Cargo.toml`, and each JSON file has exactly the `"version"` keys `VERSIONED`
  names (a new one is a decision in `release.rs`); (2) the workflow's static facts: no Homebrew in
  `release.yml`, `installers`, `tap`/`formula`, or any member's `[package.metadata.dist]`;
  `pr-run-mode = "plan"` and the plan job's `|| 'plan'`; each `./<job>` in `publish-jobs` is a
  `custom-<job>` job using `./.github/workflows/<job>.yml` and the file exists; the jobs are
  exactly plan, build-local-artifacts, build-global-artifacts, host, custom-publish-npm,
  custom-publish-marketplace, announce; the SBOM patch is applied; the workflow installs the dist
  version `dist-workspace.toml` pins; (3) with dist (`--dist`, else `dist` on `PATH`;
  `--require-dist` makes it mandatory): a scratch directory of symbolic links to the repository's
  top-level entries (not `.git`, `.github`, `target`), with `dist-workspace.toml` copied minus
  `allow-dirty` and the publish workflows copied, `dist generate --mode ci` there, the one SBOM
  patch applied (it must find the unpatched string exactly once), compared byte for byte with the
  committed `release.yml` (the first differing line is named); then `dist plan
  --output-format=json` there must list no `.rb` artifact. Unix only for (3).
- **`cargo test -p xtask` gains** `the_release_versions_agree` (the repository itself),
  `the_release_workflow_plans_on_a_pull_request_with_no_homebrew` (T-O2-03, static),
  `bump_round_trips_touching_the_named_files_only` (T-O2-01), `bump_refuses_without_writing`,
  `release_check_fails_on_a_drifted_version` (T-O2-02), `versions_dates_and_spans`.
  `check-plugin.mjs` keeps its own three equalities (the plan's contract), so a plugin-only change
  still fails without Rust.
- **Homebrew off**: `dist-workspace.toml` `installers = ["shell", "powershell"]`, `publish-jobs =
  ["./publish-npm", "./publish-marketplace"]`, `tap` and `formula` removed, and the per-package
  `formula = "adage-exec"` / `"adage-train"` removed from `crates/adage-exec/Cargo.toml` and
  `crates/adage-train/Cargo.toml` (they alone made dist build `adage-exec.rb` and `adage-train.rb`
  with the installer off; found with `dist plan`). `release.yml` regenerated with dist 0.32.0 and
  the SBOM patch re-applied: the `publish-homebrew-formula` job and its `announce` dependency are
  gone, nothing else changed. `grep -ci homebrew .github/workflows/release.yml` is 0. The way
  back is in `dist-workspace.toml`'s comment and `docs/release.md`.
- **CI** (`ci.yml`): the `plugin` job runs `test-check-plugin.mjs` and `test-stage-plugin.mjs`,
  installs Rust and the dist version `dist-workspace.toml` pins (dist's own installer URL, the line
  `release.yml` uses), and runs `cargo xtask release-check --require-dist`; the alias validate
  line is gone; the `test` job builds `adage-train` before the shim test.
- **Docs**: `docs/release.md` (new: what a tag produces, the guards, every release, dry runs,
  Homebrew later, the v0.1.0 checklist); `CONTRIBUTING.md` (the three steps are `cargo xtask bump`,
  `git tag`, `git push`; the test list); `CHANGELOG.md` (`### Installing and updating`, two
  bullets; three stale bullets in "Platforms and distribution" corrected: both plugins, Homebrew
  installers, the exec plugin's companion).

## The plugin's final layout

As committed (`plugin/claude-code/`), and as the release stages it on the `marketplace` branch:

```text
plugin/claude-code/
  .claude-plugin/plugin.json      name adage, version (bump writes it), types, userConfig band|pane
  hooks/hooks.json                SessionStart -> scripts/session-start; six hooks -> ${CLAUDE_PLUGIN_DATA}/bin/hook
  hooks/**/*.ts(x)                the mod: register.ts, its modules and their tests
  scripts/session-start           the shim: bin/adage, bin/adage-exec, bin/adage-train, bin/hook
  scripts/quiet                   what bin/hook and bin/adage run when nothing can serve
  types/index.d.ts                the $.state contract
  README.md, VALIDATE.txt
  dist/                           not committed (gitignored); plugin/build.sh or the release writes it:
    <target>/adage                aarch64-apple-darwin, x86_64-apple-darwin,
    <target>/adage-exec             aarch64-unknown-linux-musl, x86_64-unknown-linux-musl,
    <target>/adage-train            x86_64-pc-windows-msvc (.exe; no adage-exec there)
    SHA256SUMS                    "<sha256>  <target>/<name>" per binary
    LICENSE, NOTICE
```

In the plugin's data directory after a start: `bin/adage -> adage-<sha>`, `bin/adage-exec ->
adage-exec-<sha>`, `bin/adage-train -> adage-train-<sha>` (each a verified copy beside it),
`bin/hook -> adage` (or `adage-exec` with `$ADAGE_HOME/exec/enabled`); copies on Windows.

The marketplace (`.claude-plugin/marketplace.json`) lists `adage` alone, from `git-subdir`
`stevenvaughanbooth/adage` `plugin/claude-code` at `ref: marketplace`.

## The release checklist

`docs/release.md` "First release: v0.1.0" is O3's checklist: repository visibility, npm optional
(`NPM_TOKEN`, `ADAGE_NPM_PUBLISH=true`), no Homebrew token; `cargo xtask bump 0.1.0` (expected to
write CHANGELOG.md only) plus O3's words in one pull request; `git tag v0.1.0 origin/main && git
push origin v0.1.0`; watch the run; check the assets (no `.rb`), a checksum and an attestation, the
notes, the `marketplace` branch with `check-plugin.mjs --staged`, npm if on; install it for real in
Claude Code and with the curl line on macOS arm64 and Linux x86_64; `release_plugin_startup` on the
tag; then `## 0.1.1 - Unreleased`. Fix forward with a patch tag; never move a pushed tag.

## Departures from the plan, and why

- **Homebrew is removed, not gated on its token.** The brief's "conditional on the token" and the
  plan's acceptance (`grep -c homebrew release.yml` is 0, no hand patch to `release.yml`) are met
  together only by taking it out: dist's generated Homebrew job cannot be made conditional without
  hand-editing `release.yml`, and a custom token-gated job would publish formulas for unsigned
  macOS binaries, which §5 defers. With no Homebrew job, a missing token can never fail a tag. If
  the owner wants a token-gated job anyway, it is a custom `./publish-homebrew` job plus the
  `homebrew` installer back, and the Homebrew rules in `release.rs` and `check-plugin.mjs` go.
- **Two crate manifests outside the plan's file list** (`crates/adage-exec/Cargo.toml`,
  `crates/adage-train/Cargo.toml`): their `formula` keys alone kept dist building `.rb` files.
  `release-check` now refuses a per-package formula.
- **`release-check`'s regeneration runs in a scratch tree of links.** dist refuses `generate` while
  `allow-dirty` names `ci`, and editing the real `dist-workspace.toml` to run it would race other
  work; the links give dist the same workspace with nothing written to the tree. Without dist the
  check prints that it skipped the regeneration (CI passes `--require-dist`).
- **The lockfile is edited as text**, the member entries only (`[[package]]` blocks without a
  `source` whose name is a member inheriting the version), not with `cargo update -w`: offline,
  deterministic, and testable on a scratch copy without the crates' sources.
- **The npm templates now carry the real version** (`0.0.0-dev` became `0.1.0`), so `bump` syncs
  "the npm config" as the brief asks and the check covers it. `npm/scripts/stage.mjs` still stamps
  `--version` (the release passes dist's), so nothing about staging changed.
- **`bump` can go back** with `--no-changelog` (the round trip restores every version file byte
  for byte; the CHANGELOG keeps its dated heading by design).
- **New test file `scripts/test-check-plugin.mjs`** (T-O1-04), run in CI's `plugin` job. To test
  `check-plugin.mjs` on altered copies it gained `--root <dir>`.
- **`check-plugin.mjs` also refuses Homebrew** in the marketplace description and in the shim's
  code (the O12 rule "no channel that is not live"), and checks the shim places each companion.
- **T-O1-03 uses a stub `adage-train`** (checksummed into the staged plugin), so it proves the exec
  whether or not the real one is built; `PATH` holds a different `adage-train` that must not run.
- **Docs outside the brief's list** were corrected where they described the alias or two
  binaries: `AGENT.md`, `docs/how-it-works.md`, `docs/execution.md`, `docs/repository.md`,
  `docs/claude-mod/local-setup.md` (its `session-start:`/`build.sh:` line references moved with the
  shim), `docs/claude-mod/implementation-plan.md` (A8's Built line), `docs/claude-mod/test-plan.md`
  (O1/O2 cases, T-A8-04 retired, the CI gate row), the root `Cargo.toml` comment, `.gitignore`,
  `xtask/AGENT.md`, `xtask/Cargo.toml` (description), `xtask/src/affected.rs` (`xtask` added to the
  `plugin/` and `.claude-plugin/` reads). Left for O3/O12, because they name channels or say
  "not published yet" and are O12's sweep (and O5 is editing `docs/zero-config.md`): `README.md`
  (a `brew install … # not published yet` line), `docs/usage.md` (`brew install` lines),
  `docs/zero-config.md` ("ships both binaries", brew), `evals/launch/README.md`,
  `npm/cli/README.md`, the plugin README's person-facing "Installing and updating" section.
- **Not changed**: `npm/config.mjs` (already listed `adage-train` as a required companion);
  `train.rs` (its logic already works under the plugin).

## Live-only, not verified here

- The release workflow on a tag (it has never run): the five-target build, attestations, the
  GitHub Release, `publish-marketplace.yml` writing the branch, `publish-npm.yml`. Locally
  verified instead: `dist generate` reproduces the committed `release.yml` exactly (plus the
  patch), `dist plan` builds no `.rb`, and the staging scripts run on fake and real binaries.
- CI's new `plugin` job steps (installing dist from GitHub inside the job; `cargo xtask
  release-check --require-dist` there). Run locally with the same dist 0.32.0 binary, checksum
  verified against its release's `.sha256`.
- `release_plugin_startup` with three binaries staged (a release build of `adage-train` is
  needed, and this brief allows no release build). The local proxy: the staged plugin's first
  start with release `adage`/`adage-exec` and a stripped debug `adage-train` (95 MB hashed and
  copied) took 153 ms, a warm start 20 ms.
- Claude Code installing the plugin from the marketplace branch; macOS; Windows (untested and
  said so).

## Commands run and results

| Command | Result |
| --- | --- |
| `claude plugin validate --strict plugin/claude-code` (pinned 2.1.288) | pass |
| `claude plugin validate --strict .` (pinned) | `√ Validation passed` |
| `claude plugin validate --strict plugin/claude-code 2>&1 \| sed "s#$PWD/##g"` diffed with `VALIDATE.txt` | identical (no hook moved; file unchanged) |
| `claude plugin test plugin/claude-code` (pinned) | 351 pass, 0 fail |
| `node scripts/check-plugin.mjs` | `plugin adage 0.1.0: ok` |
| `node scripts/test-check-plugin.mjs` | pass (T-O1-04) |
| `node scripts/test-stage-plugin.mjs` | pass (T-O1-01, T-O2-04) |
| `bash plugin/test/session-start.test.sh` | `session-start ok`, 67 checks (T-O1-02) |
| `node npm/test/launcher.test.mjs` | `npm launcher ok (companions staged: adage-exec, adage-train)` |
| `cargo test --locked -p xtask` | 24 passed (6 new: T-O2-01..03 and three more) |
| `cargo clippy --locked -p xtask --all-targets -- -D warnings` | clean |
| `cargo test --locked -p adage-cli --test zero_config_plugin` | 12 passed, including `train_runs_from_the_plugin` (T-O1-03) |
| `cargo clippy --locked -p adage-cli --test zero_config_plugin -- -D warnings` | clean |
| `cargo fmt -p xtask -- --check`; `rustfmt --check crates/adage-cli/tests/zero_config_plugin.rs` | clean |
| `cargo fmt --all -- --check` | fails only in O4/O10 and O5/O7/O8's in-progress files (`claude_plugin.rs`, `detect.rs`, `doctor/mod.rs`, `init.rs`, `install.rs`, `status_card.rs`, `ui/wizard.rs`, `update*.rs`, `tests/onboarding.rs`, `tests/update.rs`); none of this chunk's |
| `cargo xtask release-check --require-dist --dist <dist 0.32.0>` | pass; with one `runs-on` changed in `release.yml` it fails naming line 50 (restored after); without dist and `--require-dist` it refuses; without both it passes and says it skipped |
| `dist plan` in the scratch tree, before and after | before: `adage-exec.rb`, `adage-train.rb`; after: no `.rb`; with `## 0.1.0 - Unreleased` the release body is install instructions only, with a dated heading it is the CHANGELOG section |
| Dry run: `plugin/build.sh --no-build --from <release adage, adage-exec + stripped debug adage-train> --dist <scratch>` | three lines in `SHA256SUMS` (adage and adage-exec digests equal the coordinator's `plugin/build.sh` stage) |
| Dry run: `stage-plugin.mjs --bins <host as x86_64-unknown-linux-musl> --allow-missing`, then the staged shim | 3 binaries staged, smoke `adage 0.1.0`; first start 153 ms, warm 20 ms, exit 0, nothing printed; links `adage`, `adage-exec`, `adage-train` to `<name>-<sha>`, `hook -> adage`; `bin/adage train --help` prints adage-train's usage. `check-plugin --staged` on that one-target tree names the four absent targets, as it must |
