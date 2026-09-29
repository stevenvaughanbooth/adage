# Changelog

All notable changes to Adage are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/). The release workflow uses the section for a tag's
version as the GitHub Release notes.

Nothing has been released yet, so this file describes the first version as it stands; the path
there is in the git history and in `docs/` (the Implementation Plan, and the track documents
under `docs/tracks/`).

## 0.1.0 - Unreleased

**Agent traces become reflexes.** Adage reads the traces coding agents leave behind, finds the
paths that get work done, and compiles them into reflexes: small, typed, deterministic responses
with their evidence attached. At each decision point the matching reflex tells the agent what to
do next. Adage never runs the commands it directs; execution is a separate, opt-in binary.

### Zero configuration: install the plugin, start a session

- **The marketplace plugin carries its binaries.** The release workflow stages both Claude Code
  plugins with verified binaries for all five targets (`scripts/stage-plugin.mjs`, archives
  checked against dist's `.sha256`, binaries against `dist/SHA256SUMS`) and publishes them to the
  `marketplace` branch (`.github/workflows/publish-marketplace.yml`); `marketplace.json` installs
  from it (`git-subdir`, `ref: marketplace`). No `adage` on `PATH` is needed and nothing is
  downloaded. When no binary can serve, the session-start shim says so in one line the person sees
  (a JSON `systemMessage`, verified against Claude Code 2.1.283: shown in the UI, never sent to
  the model), records it in `$DATA/status`, and points the other hooks at `scripts/quiet`.
- **Bootstrap at the first session start.** In a repository with no reflexes (or a damaged index)
  the SessionStart hook starts the learner at once (`condition --auto --bootstrap`): instincts and
  the index first, then this repository's earlier traces, so the first prompt is already directed.
  The hook says `adage: learning from N past sessions in this repo…` once.
- **Every session start is counted.** In a repository with reflexes, SessionStart assigns the arm
  and spools one `session_start` record, so the savings estimate's denominator includes sessions
  where nothing matched.
- **Notices.** One line at session start (Claude Code), at most once a day per repository and only
  when something is new: a health problem, an install step waiting for review, a savings verdict,
  new reflexes. `ui.notices = off` silences them.
- **Repository-scoped reading.** Automatic learning picks a repository's traces by Claude Code
  project directory and recorded working directory before parsing; other repositories'
  transcripts are never opened. Worktrees count as the repository (including repositories
  without `origin`: a linked worktree now shares its main checkout's key).
- **Sources are additive.** A learner started from a sandboxed session that cannot see some
  transcripts keeps them (and their cached parses); only files verifiably gone are forgotten.
- **`learn.codex_sessions = auto`** (the new default) reads `$CODEX_HOME/sessions`, this
  repository's rollouts only, once Codex hooks are registered.
- **Bare `adage` is a status card** everywhere (terminal, pipe, `!adage` in Claude Code):
  reflexes, learning, counts, savings, health and what waits for you. `adage ui` opens the app.
- **Savings, honestly.** A ledger per repository (`ledger/<slug>.jsonl`: turns, tool calls,
  tokens, cost, whether the agent ended asking for help; no prompt, command or path) and an
  estimate against the control arm (`adage savings [--json]`): Hajek IPW, winsorised at the 95th
  percentile, a stratified bootstrap with an expanded percentile interval, seeded from the session
  ids. Dollars appear only past 10 control and 30 treated sessions, always with an interval,
  labelled API-list equivalent from a dated price table (`crates/adage-core/src/pricing.toml`);
  unknown models are tokens and turns only. An interval below zero says "Adage appears to add
  ~$x per session".
- **Self-heal.** A damaged index is rebuilt at the next session start, silently. Three failed
  learns in a row back off and say so. With `delegate on` but no adage-exec plugin, the Adage
  plugin keeps serving and says why. A blocked model download is classified (`offline`,
  `blocked`, `tls`, `timeout`, `http`), recorded in `models/.fetch-failed.json`, never retried by
  itself, and explained by `adage doctor`; `ADAGE_MODEL_BASE_URL` downloads from a mirror.
- **`adage config set`** needs a person only for keys that change what Adage learns from or how
  much evidence it needs; other keys can be set from scripts and sandboxes.

### Conditioning (learning from traces)

- `adage condition` imports Claude Code transcripts (sub-agents' traces included), Codex rollouts
  (`--codex-sessions`) and OpenCode exports (`--transcripts`), normalises every tool call into
  canonical actions (tree-sitter-bash plus a command table kept as data, with golden fixtures),
  and hands the traces to **the graph engine**, the one learner (docs/tracks/track-g-graph-engine.md).
- The engine has no list of kinds: it mines what reliably precedes a step and compiles it to a
  delivery shape: a lookup of the prompt's item (its value type learned from the values), a read
  of the files a result lists, a dead end to avoid (announced at session start), a finish line
  after edits (an obligation, with one Stop request if unmet), steps in order, a check scoped to
  the part edited, an environment step after a dependency file changes, and context triggers
  (a short run of events). Housekeeping commands, one-off inline code and edits outside the
  repository never become reflexes.
- Every reflex carries its evidence (distinct traces, Wilson lower bound, why it formed), an
  effect class that caps how strongly it may be directed, and typed slot contracts. A step few
  agents took but the successful ones did forms at prime through outcome lift. Scripts the command
  table does not know are classified observe from what the traces show them doing, with the
  evidence recorded.
- Real traces form real reflexes. Agents' command variants (`cargo test -p <crate>`,
  `cargo test --workspace --locked -j4`) direct the family's covering form, a command agents ran
  that covers the narrowings (`cargo test --workspace --locked`); after-edit reflexes split across
  sibling directories generalise to one precise glob (`crates/**/*.rs`); a check after an edit
  forms as a prime run when agents neither run it anyway nor already run it after every such edit; compound (`a && b`) and redirected
  commands keep each step's own effect; a scoped test run binds the edited part
  (`npm test -- packages/{part}`); and a directing reflex needs support from at least two
  distinct tasks. On Adage's own traces: `cargo fmt --all` owed after Rust edits, then
  `cargo clippy … -D warnings` and `cargo test --workspace --locked` suggested.
- Steering is by influence: what a delivered directive named is no evidence from the point it
  arrived. Only hook output counts as a directive.
- Strengths: **fire** (do this), **prime** (consider this), **inhibit** (a known dead end).
  Lifecycle: **nascent** (waits for review), **forming**, **firm**, **dormant**, **extinguished**.
- **Live conditioning:** the hook starts a debounced, single-flight background learner per
  repository when a session stops or ends; `learn.log` records every change; a parse cache makes
  re-conditioning read only what changed (docs/live-learning.md).
- `adage sense`: a read-only report of what agents do and which reflexes their traces would form.
- **Intent reflexes** (optional): `adage models fetch` adds the pinned potion-base-8M static
  embedding model; prompts that mean the same thing and lead to the same first step form one.
- **Instincts:** reflexes that ship with Adage, bound on day 0 from a repository's own Makefile,
  `package.json`, CI, lockfiles and generators (not its test fixtures), prime at most, and giving
  way to any conditioned reflex (docs/instincts.md). `adage condition --instincts-only` binds them
  before any trace exists.
- Instincts after a failure fire only on a **failure signature** that says the command itself was
  the wrong one (`pytest: command not found`, `No module named '<the project's package>'`,
  `Unsupported URL Type`, an env guard's variable, the repository's message naming the declared
  command), never on a bare non-zero exit: on RepoBench-Local, vuejs/core's `npm test` → `pnpm test`
  fired on genuine vitest failures. A failure route with an empty signature fails validation.
- **U11, use the project's environment**, is enabled: after pytest, tox or nox fails outside the
  project's environment, `.venv/bin/python -m pytest` (a `.venv` made but not activated, as
  RepoBench's Python sandboxes had), else `uv run pytest` (`--group tests` when only that group has
  pytest). P1 directs `tox -e py` when tox's base test environment runs plain commands; nox is
  reported. click, attrs and marshmallow, which bound nothing but a lockfile advisory, now bind
  their `.venv` convention.
- N1 binds `npm install`/`npm ci` → `pnpm install` on npm's own refusals (`Unsupported URL Type`,
  an `only-allow` guard, `EBADDEVENGINES`, no `package-lock.json`), the other of yarn/pnpm on
  `command not found` or corepack's refusal, and `npm test` only on `EBADDEVENGINES`.
- The command table knows Python project managers and task runners: `uv run X`, `uvx`,
  `poetry run`, `pdm run`, `hatch run`, `pipenv run`, `npm exec`, `pnpm exec`, `bunx` classify as
  the program they run; `uv sync`/`uv pip install`/`poetry install` are environment; `tox` and
  `nox` verify; `just` and `task` by recipe name; `pnpm check` and `pnpm --filter x test` as the
  script they run; `cargo nextest` verifies.
- Declared `.go` files over 64 KiB (urfave/cli's `command_test.go`) are scanned for `go:generate`
  up to their first 64 KiB instead of skipped; whole-file parses keep the cap.
- **Compile your skill:** `adage train <SKILL.md>` (the separate `adage-train` binary) parses a
  skill's procedure graph deterministically, with no model and no session, and compiles each step
  into pack routes: the lookup a prompt's work item id triggers, finish lines after an edit, a
  "before you finish" check after *any* edit, a sequence that carries both, forbidden runners, the
  tests of the edited file (`edit_tests`, from the repository's test names and history), and the
  fixed advice to leave changes uncommitted. Every compiled command is replayed on a clean
  checkout of `HEAD` in adage-exec's sandbox (`adage_exec::replay`: network off, the working tree
  untouched, `.venv` lent read-only): a command that fails there is dropped and reported as stale,
  a forbidden runner that fails differently gets a route with the signature observed, and only
  verified reflexes may fire. The review lists every step, compiled (to what, verified how) or
  left as text (needs judgment, no stimulus, …); the pack (origin `trained`) is staged, and
  `adage train accept <name>` installs exactly what was reviewed, signed when a key is at hand, at
  the reviewed strengths. On the skill lab's acme comparison (Haiku, 36 sessions, $2.25) the
  compiled pack matched the skill in context on pass rate (12/12) and captured 97 to 99% of its
  savings in cost, turns and tokens, with no session committing (docs/training.md, "Validation").
- **Procedure cards for recurring tasks** (`crates/adage-learn/src/cards.rs`, to the
  recurring-workflows research's card compiler spec, `confirm/README.md`): with the embedding
  model installed, `adage condition` groups a repository's traces into **families** of similar
  tasks (first prompts at cosine ≥ 0.6 *and* edited file shapes overlapping, Jaccard ≥ 0.25). A
  family with a passing trace becomes a reflex whose response is a **card**: the files such a
  change touched in order (names that vary per task as patterns, `flag_<x>.go`, test siblings
  apart; files written by shell redirection included), what the latest passing trace added to
  each (clipped to fit 800 tokens), the commands that worked and those that failed with their
  error, and the check the traces finished with. One trace suffices only when it created no file.
  It is delivered once per session when a prompt's best cosine to the family's prompts reaches
  `learn.card_threshold` (default 0.811, the research's pre-registered strict threshold; 0.753 for
  one prompt), or, within 0.10 below it, at the session's first edit of a card file; prime and
  observe only, and `adage review` refuses to strengthen it. The family's prompt library grows
  with every trace that joins it. Detection is precise but literal: through the hook, the Go
  family's card fired on 6/6 held-out tasks, 0/6 paraphrases and 0/53 same-repository negatives.
  `adage train cards` replays a card's commands on a clean checkout in adage-exec's sandbox
  (installs are not replayed: the sandbox has no network) and the next learn marks the ones that
  worked and stops listing the ones that did not. `adage sense` counts the families and cards,
  the status card counts cards among learned reflexes, and `learn.cards = false` turns them off.
- Skill-steered sessions stay optional evidence: `adage train plan` estimates their cost and runs
  nothing, `adage train --plan` runs them, and `report` says, claim by claim, what they showed.
- **Packs** carry `procedure.json` (the procedure a trained pack was compiled from: every step and
  what became of it) and `[provenance] skill`; a `sequence` of two or three fixed runs binds live
  (it bound inert); an input may be the edited file's tests; a step may run a program of the
  project's environment (`.venv/bin/python`), never pinned, bound where the checkout has it.
- Directives from a compiled skill end with *From this repository's written instructions*, and
  their finish lines say the instructions require the step, never that agents were seen doing it.
- A sequence after an edit carries every finish line whose command is one of its steps (it
  carried only the one that began it), so a compiled skill's `make test`, `make ci` sequence is one
  directive for a code edit.

### Runtime

- Hook clients for Claude Code (plugin with exec-form hooks), OpenCode (bridge plugin) and Codex
  (`hooks.json`); `adage install <harness>` prints, or with `--write` merges, the registration.
  Hooks always fail open, and a no-match fast path skips events that cannot change a decision.
- Directives are rendered from fixed templates inside `<adage-reflex … strength="…">`; slot values
  reach them only through validated contracts and safe, existing repository-relative paths.
- Interlocking: inhibit beats fire, at most one fire per decision point, instincts last, and one
  edit yields one directive (a sequence that begins with a finish line's command carries its
  obligation).
- Daemonless per-session storage: two-slot state files and CRC-framed spools, rebuilt from the
  spool if a state file is damaged.
- A control arm (`runtime.control`, `ADAGE_CONTROL`, default 10%) computes what Adage would have
  said and withholds it, so every effect is measured against a control. Kill switches:
  `ADAGE_DISABLE=1` and `.adage/disabled`.

### Review, packs and sharing

- `adage review` lists reflexes that need a person's decision with their evidence and the exact
  directive text; `approve`, `strengthen`, `weaken`, `extinguish`, `pin` and `dismiss` are
  recorded in `review.yaml` and survive re-conditioning. A pinned reflex is never overwritten:
  changes wait as proposals.
- **Packs** (format `adage-pack/1`) are the only format that travels: closed over everything they
  reference, content-addressed and signed (sshsig, namespace `adage-pack`). `adage share` converts
  a track, redaction-checks every byte and signs; `adage add` verifies against trusted signers
  (trust on first use, pinned by hash), validates in full, binds the pack to the repository and
  installs its reflexes at forming (docs/packs.md).

### Execution (opt-in)

- `adage-exec`, a separate binary, runs only reflexes a person approved on that machine
  (`adage review delegate --level e1|e2`), inside an OS sandbox (Landlock and seccomp on Linux,
  bubblewrap, the macOS sandbox): **E1** prefetches an observe step's typed output beside the
  directive; **E2** (beta, behind policy) carries out a finish line or check in a staged overlay
  and applies only the declared outputs. `adage delegate on|off|status` swaps the hook
  registrations. Policy files, approvals keyed to what runs, automatic revocation; nothing is
  approved automatically unless policy allows the inert-observe class (docs/execution.md).

### Seeing it

- `adage watch` (a live pane), `adage ui` (the full-screen app: live feed, reflexes and review,
  health fixes, settings), the interactive `adage init` wizard, `adage map` (the map in a
  browser, read-only, loopback only), `adage doctor` (every check, with fixes), `adage status`,
  `adage report` and `adage graph`. `adage --help` groups every command; `adage <command> --help`
  shows its options and never runs it.

### Evals

- `adage eval`: offline replay of recorded traces through the real runtime, scored against what
  agents did; `adage eval routes` scores learned reflexes against a sandbox's conventions
  (too-narrow reflexes are reported apart from false ones); `adage eval fidelity`; the L0 bench
  with a committed baseline, sweeps and a scorecard; property tests and a safety oracle; the live
  A/B runner and generated sandboxes (docs/evals.md).

### Platforms and distribution

- macOS (arm64, x64), Linux (x64, arm64; static musl) and Windows (x64; execution is unavailable
  there). CI runs rustfmt, clippy, the whole test suite, cargo-deny, a NOTICE check and plugin
  validation on every platform, and the slow eval gates weekly.
- Releases built by dist for every target: `adage`, `adage-exec` and `adage-train`, each with
  shell, PowerShell and Homebrew installers, Sigstore build-provenance attestations and a
  CycloneDX SBOM. The npm package's launcher runs the binaries from a per-platform package chosen
  by `os`/`cpu` (all three binaries in it); nothing is downloaded at install time.
- A Claude Code plugin marketplace with two plugins (`adage`, and `adage-exec` for execution);
  each uses a bundled, checksum-verified binary, else the one on `PATH`, and never downloads.
- Apache-2.0, with a generated NOTICE of third-party licenses; `SECURITY.md`, `CONTRIBUTING.md`.
