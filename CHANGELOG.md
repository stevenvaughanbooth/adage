# Changelog

All notable changes to Adage are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/). The release workflow uses the section for a tag's
version as the GitHub Release notes.

## Unreleased

### Added

- `adage init`, `adage survey`, `adage doctor` and `adage graph --serve` (the signal box served
  from the binary).
- `adage eval`, which replays held-out sessions offline and scores what Adage would have said. It
  ships with committed datasets (real demo, synthetic, adversarial) and CI gates.
- A Codex rollout importer: `adage learn --codex-sessions`.
- A harness simulator (`crates/adage-sim`), which replays mock sessions end to end through each
  harness's native hooks.
- Windows support. CI covers macOS arm64 and x64, Linux x64 and arm64, and Windows x64.
- **Hooks record their fail-open errors.** A caught panic or internal error (unreadable stdin, a
  malformed payload, a state file that cannot be updated, a spool that cannot be appended) still
  makes the hook say nothing and exit 0, and now also appends one line to
  `$ADAGE_HOME/fail-open.log`, which `adage doctor` reports. Entries carry the time, harness,
  event, an error class and a fixed message or panic location; never a prompt, tool output,
  command or path. The log rotates at 256 KiB, keeping one previous file. "Nothing to do" (no
  tracks for the repository, a kill switch, an ignored event) is not recorded.

### Fixed

- **Over-broad dead-end warnings.** A dead end named every runner that failed before the working
  command, including `python3 -m unittest`, which passes once `ACME_ENV` is set. Dead ends are now
  keyed by failure signature: a runner is named only if its failures share the route's signature
  and it failed in at least 9 in 10 of the sessions that ran it. Route ids are unchanged.
- **A second way to fail went unnamed.** There was one dead end per working command, so a second
  failure signature before it (the demo's `python -m pytest`, "command not found", 4 sessions)
  was not warned about. There is now one dead end per (working command, signature), at most three
  per working command, each runner named under one signature only. The most common signature's
  route keeps its id; each further one gets an id derived from the command and signature. Offline
  recall rose (demo `hash:0.3` 0.81 → 0.84, synthetic 0.97 → 1.00) with no precision lost.
- **Unrelated repositories without `origin` shared routes** when their directories had the same
  name. Such a repository is now keyed `<directory name>@<root commit>`; the root commit is read
  straight from `.git` (loose objects, packs, packed refs, linked worktrees) without running git,
  and remembered in `$ADAGE_HOME/repo-keys`. A repository whose root cannot be read (no commits
  yet, SHA-256, shallow, alternates) is keyed by a hash of its path. Routes learned under the bare
  directory name keep working until `adage learn` re-keys a repository of that name. New
  dependency: `miniz_oxide` (MIT OR Zlib OR Apache-2.0), with `adler2`.
- **Environment steps reached agents without a person's approval** (Design O11). Unknown commands
  are classified as environment, and a learned route running one was delivered at yellow at once.
  Learned and imported environment routes now start as drafts, which are never delivered, and
  wait in `adage review` (reason "approval") until `approve` puts them into service. Re-learning
  keeps the approval while the step is unchanged; a different command waits again.
- **Dead-end and linked-file directives stated no exit condition** (Design W2). Both are now
  learned with one ("Ignore this if the user is asking for something else."), and the dead-end
  template renders it on its own line after the command. Routes stored without one render as
  before.
- **Normalisation depended on machine speed.** Pathological commands were cut off by a 50 ms
  parse timeout, so a slow machine could record a command as unknown where a fast one parsed it.
  The cut-off is now a budget of bytes the lexer may read (2^20, re-reads included), the same on
  every machine; a 1 s wall-clock backstop remains for input shapes not yet known.
- **Linux segfault in hooks.** A segfault in tree-sitter-bash's scanner (it calls `isdigit()` on
  full code points) could crash a hook on Linux, given a `{` followed by a large code point. The
  parser now only sees an ASCII shadow of each command, with the same byte offsets.
- **Windows finish-line routes never fired.** Paths under `\\?\` roots, and `C:\…` paths, did
  not resolve or relativize.
- **Eight bugs found by property tests**, including a stack overflow on long `&&` chains, slow
  parses of malformed input (now cut off by a fixed lexing budget, about 7 ms), invisible
  characters passing sanitisation, and a track able to direct a command naming a path outside the
  repository.
- **`adage learn --codex-sessions <dir>` fell back to all of `~/.claude/projects`** when the
  directory held nothing. An explicit source that finds nothing is now an error.

## 0.1.0 - Unreleased

The first version: Adage learns the routes coding agents take and, at each decision point, tells
the agent deterministically what to do next. It never executes anything itself.

### Learning

- `adage learn` imports Claude Code transcripts (and OpenCode exports), normalises every tool call
  into canonical actions (tree-sitter-bash plus a command table kept as data; 315 golden fixtures),
  and mines five route kinds: prompt → lookup, linked files, dead ends, finish lines, and (optional)
  intent families.
- Every route carries its evidence (distinct sessions, Wilson lower bound), an effect class that caps
  how strongly it may be directed, and typed slot contracts learned from real values.
- Intent families (optional): `adage models fetch` / `adage models install <dir>` add the pinned
  potion-base-8M static embedding model (SHA-256 verified); prompts are clustered per repository
  with exact kNN and deterministic Louvain, then split and merged by what the sessions did next.

### Runtime

- Hook client for Claude Code (plugin with exec-form hooks), OpenCode (bridge plugin) and Codex
  (`hooks.json`); `adage install <harness>` prints, or with `--write` merges, the registration.
- Directives rendered from fixed templates; slot values only through validated contracts and safe,
  existing repository-relative paths. Hooks always fail open.
- Daemonless per-session storage: two-slot state files and CRC-framed spools.
- A holdout arm (`ADAGE_HOLDOUT`, default 10%) measures every effect against a control; kill
  switches `ADAGE_DISABLE=1` and `.adage/disabled`.

### Tracks, review and sharing

- The track format with validation (tracks are untrusted input), a canonical digest, and a compiled
  index.
- `adage review` lists routes that need a person's decision; `approve`, `reject`, `demote`,
  `promote` and `pin` are recorded in `review.yaml`.
- `adage share` redaction-checks a track and signs it (native `sshsig`, ssh-agent or key file);
  `adage add` verifies it against trusted signers and installs it at probation.
- `adage status`, `adage report` and `adage graph` (situations → routes → steps → agent responses,
  with live counts).

### Platforms and distribution

- Windows support in the store (positional I/O and `LockFileEx` locking through `std`), the data
  directory (`%APPDATA%\Adage`), home-directory lookup and path handling. Compiles for every release
  target; Windows tests run in CI.
- Release engineering: CI on macOS (arm64, x64), Linux (x64, arm64) and Windows (x64) with rustfmt,
  clippy, tests, cargo-deny, and plugin validation; releases built by dist for macOS, static Linux
  (musl) and Windows, with shell, PowerShell and Homebrew installers, Sigstore build-provenance
  attestations and a CycloneDX SBOM.
- npm packaging: a main package whose launcher runs the binary from a per-platform package chosen
  by `os`/`cpu`; nothing is downloaded at install time. Publishing is off until the package name is
  chosen.
- A Claude Code plugin marketplace (`.claude-plugin/marketplace.json`). The plugin uses its bundled,
  checksum-verified binary, else `adage` on `PATH`, and never downloads at hook time.
- `SECURITY.md`, `CONTRIBUTING.md`, and a cargo-about NOTICE generator.
