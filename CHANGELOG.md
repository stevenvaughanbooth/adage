# Changelog

All notable changes to Adage are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/). The release workflow uses the section for a tag's
version as the GitHub Release notes.

## Unreleased

### Added

- **Palette-led terminal and web UX.** The terminal app opens on Home with a searchable command
  palette for repository setup, history sources, learning, review, reports, and signed packs.
  The launch-themed web map includes an isolated demo, setup guidance, and trace inspection with
  links from version-specific outcomes to their contributing sessions and recorded responses.
- **Inspectable learning and approvals.** Learn previews its history sources and scoped session
  counts; OpenCode history can be imported from the app. Proposed reflex changes show before/after
  fields and directives, with confirmation bound to the reviewed revision. Health distinguishes
  registered hooks from received events and keeps first-receipt setup instructions visible.
- **Live learning.** Adage now learns by itself while agents work: when a session stops or ends
  (Claude Code and Codex `Stop`/`SessionEnd`, OpenCode `session.idle`/`session.deleted`), the hook
  starts a detached background learner for that repository, at most once per
  `learn.min_interval_secs` (a request inside the interval is learned when it ends, never
  dropped), one learner per repository at a time. The hook never waits for it and its output never
  changes (Stop p95 +0.3 ms, +0.7 ms when it starts a learner). `adage learn` still works, and
  both write the same tracks. Turn it off with `adage config set learn.auto false` or
  `ADAGE_AUTO_LEARN=0`. See docs/live-learning.md.
- **`$ADAGE_HOME/config.toml` and `adage config`** (`get`, `set`, `--json`): `learn.auto`,
  `learn.min_interval_secs`, `learn.thresholds` (`default` or `demo`; also used by `adage learn`),
  history sources for automatic learning, `learn.parse_cache`, and `runtime.holdout`.
  Environment variables still win. `config set` validates, writes atomically, and refuses inside an
  agent session.
- **`learn.log` records what changed.** Every learn, manual or automatic, appends one line per
  repository with `kind`, `trigger` (`auto`/`manual`), `repo`, `duration_ms` and `changes`: each
  route that is new, drafted, retired, changed aspect (yellow → green), changed evidence, or got a
  proposal, in fixed-template summaries with no prompt, command, output or path. Failures are
  `learn_error` lines. Existing fields are unchanged.
- **Parse cache.** Parsed transcripts are kept in `$ADAGE_HOME/cache/sessions`, keyed by path,
  size and modification time, so re-learning parses only what changed: 60 ms instead of about
  300–670 ms for 207 real transcripts (130 MB). Results are byte-identical with and without it
  (`--no-cache`, `learn.parse_cache = false`).
- `adage_store::spool::read_from(path, offset)` for following spools live, safe against records
  still being written; `adage_store::lock` file locks.
- `adage doctor` checks the configuration and live learning: on or off, each repository's last
  automatic learn and its outcome, a stuck learner, and requests left waiting.
- **A terminal UI.** `adage watch` (also `adage doctor --watch`) is a live pane for a split
  terminal: health lamps re-checked every few seconds, every hook call as it happens (directives
  by aspect, holdout sessions withheld, followed and derailed judgements, finish lines met, stop
  requests), and what learning changed, from `learn.log`. `adage ui`, or plain `adage` at a
  terminal, is a full-screen app with Live, Routes (evidence, live counts, exact directive text,
  and review decisions through `adage review`'s own code), Health (every doctor check with one-key
  fixes: install hooks after showing the file and diff, fetch the model with progress, learn this
  repository now as a background learner would, turn auto-learn on, open the fail-open log) and
  Settings (every `adage config` key with its source, edited through `adage config set`'s code).
  The learning ticker says when a background learner is running. `--demo`
  replays a scripted scenario through real hook calls into a temporary data directory; `adage
  watch --once` prints one frame as text. Built on ratatui and crossterm; the terminal is restored
  on every exit, including a panic, and `NO_COLOR` is honoured. Hooks never load any of it.
- **`adage init` is a guided wizard at a terminal.** It asks before each step (register hooks per
  harness, fetch the model, run a first learn, turn auto-learning on) and ends with the doctor's
  summary. Piped, inside an agent session, or with the new `--print` (or `--yes`), it prints the
  report it always has.
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

- Reflex measurements and review warnings no longer inherit outcomes from ambiguous installations
  or older versions. Complete selected traces remain available outside the recent-event window,
  and missing observations are described as a bounded follow window rather than deliberate refusal.
- Demo setup actions require opening a real repository and switching to persistent workspace data,
  protecting real hook configuration from changes made while exploring the scripted demo.
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
- **A corrupt session state started the session over**, so a dead end could be announced twice
  and a finish-line obligation forgotten. When neither state slot can be read, the hook now
  rebuilds the state from the session's spool (up to 4 MiB): emitted routes, obligations, stop
  requests, arm, repository and counters. Spool records gain `kind` and `repo` fields.
- **Oversized payloads were cut off, not drained** (Design §13.3). A payload over 8 MiB is now
  read to the end and discarded, so the harness writing it never sees a broken pipe; the same
  with a kill switch on. The refusal is recorded in `fail-open.log`.
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

### Changed

- `learn.log` has one line per repository per run instead of one per run (a run that found no
  usable session writes one line with `repo` null).
- `adage learn`, automatic learners, review decisions and `adage add` take `$ADAGE_HOME/tracks.lock`
  while they write tracks and the index, so they never interleave.
- The demo and live eval runners set `ADAGE_AUTO_LEARN=0`, so the index stays fixed within a round.

- **`adage learn` writes `learn.log`**: one JSON line per run with the sessions read and used and,
  per repository, the tracks written (version, change, route count). No prompt, command or path.
  Rotated at 256 KiB.
- **CI builds the aarch64 musl release target**, natively on the arm64 runner, as it did x86_64.
- **Hooks return early on events that cannot match** (Design §13.2). The index now stores, per
  repository, what can make its routes act, and a hook skips an event that cannot change any
  decision before loading the command table, locking the state or writing the spool: a no-match
  tool batch now costs 1.9 ms p50 instead of 2.8 ms. The eval replay checks on every event of
  every dataset that the fast path changes no decision. Skipped events are not written to the
  spool, so `adage graph` shows fewer off-route commands.

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
