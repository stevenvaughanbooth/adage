# Releasing Adage

How a version of Adage is cut, what the release workflow builds and publishes, the checks that
guard it, and the checklist for the first release, v0.1.0, which no one has run yet. Maintainers
only; [CONTRIBUTING.md](../CONTRIBUTING.md#releasing-maintainers) has the three-line version.

## What one tag produces

A tag `v<version>` on a commit whose versions agree runs `.github/workflows/release.yml`, which
[dist](https://github.com/axodotdev/cargo-dist) generates from `dist-workspace.toml`:

| Job | What it does |
| --- | --- |
| `plan` | `dist host --steps=create` on a tag; on a pull request only `dist plan` (`pr-run-mode = "plan"`), so a pull request builds and publishes nothing |
| `build-local-artifacts` | For each of the five targets (macOS arm64 and x64, Linux arm64 and x64 as static musl, Windows x64): `adage-cli-<target>`, `adage-exec-<target>` and `adage-train-<target>` archives (`.tar.xz`, `.zip` on Windows), each with a `.sha256` beside it and a Sigstore build-provenance attestation |
| `build-global-artifacts` | The shell and PowerShell installers of each app, `sha256.sum`, `source.tar.gz`, and a CycloneDX SBOM (`.cdx.xml`) per app |
| `host` | Creates the GitHub Release with every file above; its notes are CHANGELOG.md's section for the version |
| `custom-publish-marketplace` | `.github/workflows/publish-marketplace.yml`: stages the Claude Code plugin with all three binaries for every target (`scripts/stage-plugin.mjs`, each archive checked against its `.sha256` before it is unpacked, every binary against `dist/SHA256SUMS` after), checks the tree (`node scripts/check-plugin.mjs --staged`), and commits it to the `marketplace` branch, which `/plugin install adage@adage` installs from |
| `custom-publish-npm` | `.github/workflows/publish-npm.yml`: `@adage-sh/cli` and one package per platform, all three binaries in it. Skipped unless the repository variable `ADAGE_NPM_PUBLISH` is `true` |
| `announce` | Waits for the publish jobs; a skipped job does not fail it |

There is no Homebrew job: Homebrew is off the release until macOS signing exists (below).

## The checks that guard a release

| Check | Fails when | Where it runs |
| --- | --- | --- |
| `cargo test -p xtask` (`xtask/src/release.rs`) | `Cargo.toml`, the lockfile's members, `plugin/claude-code/.claude-plugin/plugin.json`, `.claude-plugin/marketplace.json` (metadata and every plugin) or the npm templates (`npm/cli/package.json`, `npm/platform/package.json`) disagree; `release.yml` names Homebrew, lacks a custom publish job, does not plan on a pull request, or lacks the SBOM patch | every CI run |
| `cargo xtask release-check --require-dist` | the above, and `release.yml` differs from what the pinned dist generates from `dist-workspace.toml` by more than the documented SBOM patch, or the release plan would build a Homebrew formula | CI's `plugin` job, with the dist version `dist-workspace.toml` pins |
| `node scripts/check-plugin.mjs` | the marketplace lists anything but the one `adage` plugin, a description says the plugin needs a binary on `PATH` or names Homebrew, the shim stops placing a binary, versions disagree, a hook points nowhere | CI's `plugin` job |
| `node scripts/test-stage-plugin.mjs` | staging accepts a target without `adage-train`, or an archive without (or with a wrong) `.sha256` | CI's `plugin` job |
| `plugin/test/session-start.test.sh` | the shim mislinks any of its three binaries, or a missing or tampered `adage-train` stops `adage` serving | CI's `test` job |

dist reads the CHANGELOG section headed `## <version> - <date>`: with the heading still
`## <version> - Unreleased` the GitHub Release gets install instructions and no notes.
`cargo xtask bump` renames it.

## Every release

```bash
cargo xtask bump 0.1.1            # writes every version file, dates CHANGELOG's Unreleased heading, runs the checks
git commit -s -am "Release 0.1.1"
git tag v0.1.1
git push origin HEAD v0.1.1
```

`bump` changes exactly `Cargo.toml`, `Cargo.lock`, the two plugin manifests, the two npm templates
and CHANGELOG.md, and writes nothing if it refuses (a malformed version, no Unreleased section, a
version already released). `--date YYYY-MM-DD` sets the date; `--no-changelog` leaves CHANGELOG.md
alone (to undo a bump). A pushed tag is never moved or deleted: if a release goes wrong, fix it
and cut the next patch version.

After `dist-workspace.toml` changes, regenerate the workflow: dist refuses while `allow-dirty`
names `ci`, so remove that line, run `dist generate --mode ci` with the pinned dist, put the line
back, and re-apply the one patch (`steps.cargo-cyclonedx.output.paths` becomes `outputs.paths`).
`cargo xtask release-check` then passes.

## Dry runs that need no tag

```bash
cargo build --release --locked -p adage-cli -p adage-exec -p adage-train
plugin/build.sh --no-build --dist /tmp/adage-dist                    # this machine's three, with SHA256SUMS, into a scratch dist/
mkdir -p /tmp/adage-bins/<target> && cp target/release/adage target/release/adage-exec target/release/adage-train /tmp/adage-bins/<target>/
node scripts/stage-plugin.mjs --bins /tmp/adage-bins --out /tmp/adage-staged --allow-missing   # the marketplace tree for this target; runs `adage version`
echo '{"session_id":"dry","hook_event_name":"SessionStart","source":"startup"}' \
  | /tmp/adage-staged/plugin/claude-code/scripts/session-start /tmp/adage-staged/plugin/claude-code /tmp/adage-data
ls -l /tmp/adage-data/bin                                           # adage, adage-exec, adage-train -> <name>-<sha256>; hook -> adage
cargo xtask release-check --dist "$(command -v dist)"               # with the pinned dist installed
```

`<target>` is the release triple (`x86_64-unknown-linux-musl`, `aarch64-apple-darwin`, …, as in
`npm/config.mjs`). `check-plugin.mjs --staged` needs all five targets, so it fails on a one-target
dry run, naming the four missing ones; the release workflow stages all five.

## Homebrew, later

Homebrew bottles of unsigned macOS binaries are quarantined, and dist's Homebrew publish job would
fail every tag that has no `HOMEBREW_TAP_TOKEN`, so the release has no Homebrew (onboarding plan O2
and §5). To bring it back: macOS signing first (`macos-sign = true` and its three secrets, see
`dist-workspace.toml`); then the `stevenvaughanbooth/homebrew-tap` repository and the
`HOMEBREW_TAP_TOKEN` secret; then in `dist-workspace.toml` add `homebrew` to `installers` and to
the front of `publish-jobs` and set `tap` and `formula = "adage"`, and `formula = "adage-exec"` /
`"adage-train"` under `[package.metadata.dist]` in those crates; regenerate; drop the Homebrew
rules from `xtask/src/release.rs` and `scripts/check-plugin.mjs`; and give `adage update` its brew
arm.

## First release: v0.1.0 (onboarding plan O3)

The owner runs this once. It waits for O0 (the live check, `docs/claude-mod/impl/O0-live-notes.md`),
O1 and O2; nothing else. Mark each line done with the date, the machine and what was seen.

**Before the tag (settings, outside the repository)**

- [ ] The repository is readable by the people installing: public at the tag. `/plugin marketplace
      add stevenvaughanbooth/adage` clones it, and `releases/latest/download/…` and the update
      check read it. (The reserve, from `docs/zero-config.md`: push the `marketplace` branch to a
      public `stevenvaughanbooth/adage-marketplace` and point `marketplace.json`'s source at it;
      the release assets then need a public mirror for the curl line.)
- [ ] Optional for this tag: npm. Add the `NPM_TOKEN` secret (an automation token that can publish
      to the `adage-sh` org) and the repository variable `ADAGE_NPM_PUBLISH=true`. Without them
      `custom-publish-npm` is skipped and nothing fails.
- [ ] No `HOMEBREW_TAP_TOKEN` is needed.

**The release commit (one pull request into `main`)**

- [ ] On an up-to-date `main`: `cargo xtask bump 0.1.0`. Expected: `wrote CHANGELOG.md` only (every
      version file already says 0.1.0); the heading `## 0.1.0 - Unreleased` becomes
      `## 0.1.0 - <today>`; `plugin adage 0.1.0: ok`.
- [ ] The words that become true with the tag, in the same commit (O3's files): `README.md`
      Install says the two routes (in Claude Code, `/plugin marketplace add stevenvaughanbooth/adage`
      then `/plugin install adage@adage`; in a terminal, the curl installer line, then `adage init`)
      with no "not published yet"; `docs/usage.md`'s install section likewise (the checkout route
      moves under Contributing); `npm/cli/README.md` (it is packed into the npm package from the
      tagged commit); CHANGELOG.md's "Platforms and distribution" bullets rewritten as released.
      npm is named only if it is switched on; Homebrew is not named.
- [ ] `git commit -s`, open the pull request, and wait for CI: the `plugin` job (check-plugin,
      both staging tests, `cargo xtask release-check --require-dist`, `claude plugin validate`,
      `VALIDATE.txt`, `claude plugin test`) and the Release workflow's `plan` job on the pull
      request (it plans; no build job runs). Merge.

**The tag**

- [ ] `git fetch origin && git tag v0.1.0 origin/main && git push origin v0.1.0`.
- [ ] `gh run watch $(gh run list --workflow Release --limit 1 --json databaseId -q '.[0].databaseId')`.
      Expected: `plan`, five `build-local-artifacts`, `build-global-artifacts`, `host`,
      `custom-publish-marketplace` succeed; `custom-publish-npm` succeeds or is skipped; `announce`
      succeeds.

**Check what was published**

- [ ] `gh release view v0.1.0 --json assets -q '.assets[].name'` lists, for each of `adage-cli`,
      `adage-exec` and `adage-train`: five archives and five `.sha256`, the `-installer.sh` and
      `-installer.ps1`, and the `.cdx.xml`; plus `sha256.sum` and `source.tar.gz`. No `.rb`.
- [ ] `gh release download v0.1.0 -p 'adage-cli-x86_64-unknown-linux-musl.tar.xz*'`,
      `sha256sum -c adage-cli-x86_64-unknown-linux-musl.tar.xz.sha256`, and
      `gh attestation verify adage-cli-x86_64-unknown-linux-musl.tar.xz --repo stevenvaughanbooth/adage`.
- [ ] The release notes are CHANGELOG's 0.1.0 section (not only install instructions).
- [ ] `git ls-remote origin marketplace` resolves. `git fetch origin marketplace`, then
      `git worktree add /tmp/mkt origin/marketplace && node scripts/check-plugin.mjs --staged /tmp/mkt`
      passes (every target, three binaries, every sum), then `git worktree remove /tmp/mkt`.
- [ ] If npm is on: `npm view @adage-sh/cli version` prints `0.1.0`.

**Install it for real (T-O3-01 to T-O3-03, on macOS arm64 and Linux x86_64)**

- [ ] In Claude Code with no `adage` on `PATH`: `/plugin marketplace add stevenvaughanbooth/adage`,
      `/plugin install adage@adage` (the spelling O0 recorded). The marketplace lists one plugin.
      In a repository with history, a new session shows `adage: learning from N past sessions in
      this repo; …`; `ls -l <plugin data>/bin` shows `adage`, `adage-exec`, `adage-train` linked to
      `<name>-<sha256>` and `hook -> adage`; `<plugin data>/bin/adage train --help` prints
      adage-train's usage. Record it in `evals/launch/fresh-install.md` (B1 closed).
- [ ] In a terminal: `curl --proto '=https' --tlsv1.2 -LsSf https://github.com/stevenvaughanbooth/adage/releases/latest/download/adage-cli-installer.sh | sh`,
      then `adage version` prints `adage 0.1.0` from `~/.cargo/bin`. On macOS record what
      Gatekeeper does with the unsigned binary.
- [ ] On the tagged commit: `cargo test --release --locked -p adage-cli --test zero_config_plugin
      release_plugin_startup -- --nocapture` (three binaries staged; first start under 1500 ms,
      repeated p95 under 200 ms).

**After**

- [ ] Add `## 0.1.1 - Unreleased` above `## 0.1.0 - <date>` in CHANGELOG.md for what comes next.
- [ ] If anything above failed: fix forward. `cargo xtask bump 0.1.1`, tag `v0.1.1`. A failed publish
      job alone can be re-run with `gh run rerun <run id> --failed`.
