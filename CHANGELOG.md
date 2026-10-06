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

### Fixes from the architecture work

- **Keep paid dispatch behind current operator and evidence checks.** The pilot, frozen-study
  and live-runner entrypoints consistently refuse Claude Code, Codex and OpenCode agent sessions.
  Collectors recheck evidence under their dispatch lock, including cross-stage validation gates,
  so a concurrent incomplete result cannot admit another paid job from a stale readiness plan.

- **Stop paid collection when evidence is incomplete.** Held-out runs now retain the first
  bad receipt and stop on unknown grades, incomplete usage, unplanned models or model-call
  overruns. A final-session cost overrun also blocks later stages. Training pins original
  native histories and request streams, stopping on missing or altered evidence before
  another session can spend. A documented Docker Desktop setup supports the Linux pilot on Macs.
  Free executor checks also retain their inherited-lock test on macOS through portable pipes.

- **A bounded paid-pilot launch path.** A local coordinator connects real model training traces,
  production learning, explicit review and frozen held-out evaluation. The included programming
  task pack has honest semantic groups, unrelated controls and an independent grader. No model
  calls are made by its free checks; synthetic pilot results do not establish customer savings.

- **Explicit, durable completion readiness.** Optional required verification reflexes run even
  when the agent never started them, under existing E2 grants and sandbox gates. Current-input
  execution evidence distinguishes passed, missing, stale, failed and unknown checks; durable
  attempt limits survive restart. Ordinary advisory obligations also preserve later-edit
  invalidation during recovery. Command completion remains distinct from coverage or task quality.

- **A reproducible handoff for local model evaluations.** Train-only live archives can be
  exported into canonical learning with explicit task/group identities, independent outcomes
  and source provenance. Diagnostic workflow fixtures and a conservative native-action audit
  separate correct final state from evidence that tests were actually run. The local-agent
  runbook provides staged budgets, capability gaps, controls and frozen claim criteria.

- **A complete learning-to-delivery proof.** A deterministic CLI fixture turns native traces
  into reviewed reflexes, delivers the learned continuation through the real hook, and verifies
  a fresh task's corrected output. It checks review, disable, unrelated-task and repeated-delivery
  controls without calling a model. The README links the one-command reproduction.

- **Studies fail before paying for an inert intervention.** Frozen experiments require an
  actual compiled index with enabled reflexes for the selected repository. `adage eval experiment
  artifact` checks its generation, directive safety, derived caches and checkout routing.
  Live plans report readiness blockers, progress, budget reservations and output paths; execution
  checks prerequisites and interrupted accounting before creating a new attempt.

- **Frozen live collection.** `evals/live/frozen.py` prepares, plans, executes and reports a fixed
  Claude Code study on Linux. It pins inputs and the compiled reflex set, enforces workflow
  deadlines, preserves terminal results on resume and refuses to rerun interrupted attempts.
  Immutable receipts retain independent grades and reconciled native usage with checksummed
  source evidence. Simulated-agent contracts run against the real offline validator in CI.

- **Native request accounting for learning studies.** The live-evaluation adapter reconciles
  completed Claude usage events against terminal and per-model totals, deduplicates assistant
  chunks by provider request, includes cache creation/read tokens once, and rejects incomplete
  logs. Independent graders must exit successfully with a boolean result. CLI list-price
  estimates remain separate from billed costs.

- **Frozen learning experiments.** `adage eval experiment object|freeze|validate|report` binds a declared
  test cohort to its training audit, model settings, compiled reflex set, budgets and grader.
  Every scheduled task remains in the report, including missing results and timeouts. Whole
  workflow accounting includes retries; changed artifacts, reused billing requests and incomplete
  evidence cannot qualify a savings claim. The commands are offline; live harness adapters supply
  independent grades and provider usage. New fixtures run in the fast PR checks.

- **Auditable outcomes and paired savings evidence.** A passing check before later edits or a
  new task no longer labels the final trace successful. Canonical exports accept independent
  outcome labels; corpus audits distinguish recorded, proxy and unknown outcomes and expose
  training groups and the exact learned generation. `adage eval paired` checks held-out task
  pairs, quality noninferiority, token use and billed cost separately, rejecting repeated tasks,
  train/test overlap, missing evidence and mismatched experimental conditions. Deterministic
  fixtures cover stale checks, contamination, quality regressions and misleading token savings.

- **Reusable mining checkpoints.** Repeated learning reuses unchanged repository mining while
  rerunning qualification, current review decisions and compilation. Content, parameter and build
  fingerprints invalidate stale calculations; corrupt caches trigger recomputation. `--no-cache`
  forces a full run, and reports identify mining cache hits.

- **Canonical trace corpora.** `adage corpus export`, `import`, `inspect` and `learn` normalize
  portable traces, publish repository-scoped evidence snapshots and feed them into the existing
  learner. Imports bound record size, report malformed evidence, deduplicate delivery and reject
  conflicts atomically. Explicit snapshot replacement supports corrections and removals; stored
  objects are content-verified and private on Unix.

- **Task-aware learning evidence.** Canonical sessions preserve declared task, group and attempt
  ids; `condition --identities <file>` imports them and evaluation manifests pass their task ids
  through the production learner. Retries stay together, unknown identities cannot establish task
  diversity, and prompt fallback uses the complete recorded prompt. Calibration now refits
  prototypes and candidates inside held-out folds on at most 400 independent task representatives,
  including for large corpora. Learning reports declared, inferred and unknown identity counts.
  Identical session copies count once; conflicting records with the same repository/session id
  reject learning before tracks are written.
- **A learn run reads earlier results through its store.** It reads earlier tracks, a person's
  review decisions, proposals, route explanations, instinct bindings and installed packs through
  the same `Store` it writes through, so a store other than `$ADAGE_HOME` (a team service) can
  hold all of a repository's learned state. `adage_learn::plan` takes the store in place of the
  tracks directory.
- **Learning is about 4× faster.** The command table built into the binary was parsed again
  (TOML, then every pattern's regex) on each lookup, many times per learn run. It is now parsed
  once per process. A command's effect is remembered once classified, and reading whether a
  command's output reports a test run no longer drops to the regex engine's slowest path on
  non-ASCII output. Over 497 sessions, `adage condition` drops from 8.5 s to 1.9 s, with
  byte-identical tracks.

- **Breaking: one JSON shape for a reflex.** `adage status --json`, `adage review --json` and
  `adage map` (`/api/v1/graph`) render each reflex with the same fields and types, and add only
  their own. The fields:
  - **Identity:** `id`, `track` (the track's name), `reference`, `version`, `repo`.
  - **What it is:** `state`, `strength`, `instinct`.
  - **What it does:** `trigger` and `step` as `{kind, label}` objects, and `directive`, which is
    `null` for an execution-only reflex.
  - **Where it came from:** `origin`, an object with `kind`, a `label`, and for an instinct its
    pack, instinct id and declared file.
  - **Evidence:** `evidence`, the full object, including the `describe` sentence.

  The changes from before:
  - **`review --json`:** `route` is now `id`. `track` now holds the track's name (the directory
    is `reference`), and `name` is gone. `decisions` is now `review.verbs`, `reasons[].detail` is
    now `review.reasons[].why`, and `next` is now `review.next`. `runtime` is now `live` (the
    map's counts), and `why` is now `evidence.why`.
  - **`status --json`:** `lifecycle` is now `state`, and `kind` is now `instinct`. The flat
    evidence counts and the `evidence` sentence now sit under `evidence` (the sentence as
    `evidence.describe`), and `uplift` is now `evidence.uplift.line`. A track's `origin` is now
    `kind`.
  - **The map:** `origin` is now the object above, `review.execution_command` is now
    `review.next`, and an execution-only reflex's `directive` is now `null`.

- **`adage install claude-code --scope user` honours `CLAUDE_CONFIG_DIR`.** It wrote
  `~/.claude/settings.json` regardless, so with the variable set the hooks were never loaded and
  `adage doctor` could not see them. Install and detection now share one table of where each
  harness keeps Adage's registration.
- **Review decisions in `adage ui` take the tracks lock**, as `adage review <verb>` always did, so
  a decision can no longer race an automatic learner writing the same track. While a learn run
  holds the lock, `adage ui` waits at most two seconds and then says to try again, with nothing
  recorded, instead of freezing.

### Launch clarity and local experience

- **Adage owns an approved verification.** With E2 allowed and one learned verification approved,
  a treated Claude Code or Codex session starts with a short, factual contract naming the command
  Adage runs (at most two), when, and that it runs again at finish; only grants that still match
  are named, and control sessions get none. The receipt after a run is short: result, exit, time,
  a receipt id, the number of tests pytest collected and the tests this session's edits added, by
  name, from pytest's declared cache record (never from output). A test call that a fresh passing
  receipt covers gets a note, never a refusal. Guided sessions stay excluded from organic learning.
- **The finish-line check (Stop gate).** When the agent finishes, a verification that ran this
  session and has not passed on the current inputs runs again through the same gates. Passed: the
  turn ends with one line for the person. Failed: the agent continues once, with the failing tests,
  in a continuation marked as Adage's; a second failure ends the turn with a line for the person.
  At most two gate runs per session; never on OpenCode. An unconfirmed run pauses actions until a
  person acknowledges recovery.
- **The orientation card.** A delivered procedure card no longer quotes the exemplar lines; it says
  where its files stand in the checkout instead (line counts, a Python source's last definitions,
  a test file's last lines, a changelog's first headings), read locally with no command run and
  bounded at about 1,200 tokens. `ADAGE_CARD_ORIENTATION=0` delivers the earlier form.
- **Lapsed approvals are told, not silent.** When a grant stops matching, the person sees one line
  at most once per session naming the grant, what changed and the `adage review delegate …`
  command that approves it again; `adage delegate status` and `adage review` list lapsed grants.
  Bytecode in `__pycache__` no longer lapses a project-Python grant (adapter version 2).
- **`adage delegate policy [--max-level e1|e2]`** shows this machine's execution policy and its
  file; `--max-level e2` (a person, never in an agent session) writes the opt-in with automatic
  approval off and the sandbox required, keeping other settings. Refusal hints name it instead of
  an internal gate. `review delegate --timeout-ms` above the level's limit says it was clamped.
- **`adage install <harness> --remove`** takes Adage's hooks out of the scope's file with a diff, a
  confirmation and a `.adage-bak`, leaving the person's own hooks and settings as they were
  (install then remove restores the file byte for byte).
  `install --write` and the init wizard no longer add a second registration where one exists.
- **`adage condition --repo-root <dir>`** conditions that repository alone; it was a hidden flag
  that plain `condition` ignored.
- Evidence for this work is development-stage only: one valid development study on one task with
  development scaffolding (dev-v9b), not a launch statistic. The held-out comparison has not run.
  Live real-repository savings remain unproven.
- Live evaluation subprocesses no longer inherit launcher stdin as unintended model input.
  The regression reproduces the prior leak; contaminated diagnostic cohorts remain archived.

- Keep the unsuccessful learned-CPU pilot in the repository evaluation record, outside
  launch-site assets. Document hook-result reuse research and the next diagnostic protocol.

- First-run completion now points to learned-candidate review and execution status, distinguishing
  evidence approval from permission to run scripts. The first-reflex guide explains empty-history
  states, execution policy, scoped grants and how to verify an action actually replaced agent work.
- Pytest's `.pytest_cache/**` is supported as an explicit cache declaration, while hidden output
  paths and root wildcards remain restricted. Invalid action-glob errors distinguish outputs from caches.

- The teaser opens with sound by default, with an explicit play button when the browser blocks
  audible autoplay. Reduced-motion and data-saving preferences still wait for a click.

- Early-access signup checks availability before asking for an email. Closed registration and
  failed checks have clear states, retry controls, and a public walkthrough; requests time out
  without discarding the entered address.

- Plugin bootstrap prefers a native SHA-256 provider, reducing first-start checksum cost while
  retaining the same digest verification and portable fallback.

- On macOS, a direct system-Python action resolves Apple's launcher to the interpreter in the
  already selected developer toolchain, preventing sandbox startup timeouts without expanding
  permissions. Unix-only staging tests are guarded so Windows compile checks remain valid;
  the ARM64 musl sandbox retains its kexec-file denial when libc omits the syscall constant.

- The launch site now walks one concrete trace through conditioning and hook delivery, introduces
  the watch/TUI/map demos, and leads with completed shipped-product validation and its uncertainty.
  The approved 45-second dark/day teaser replaces the older duration and posters. The interactive
  walkthrough gives visitors a way to explore while early-access signup is unavailable; source
  installation clearly requires repository access before the public release.
- The local map retries failed snapshots with bounded backoff, times out stalled requests, and
  labels stale data with a manual retry. Continuous events no longer starve snapshot refreshes.
  First-run hook checks can recover and be rechecked; demo data is explicitly labeled.
- In the TUI's Reflexes tab, Escape resets both the search and filter chips, including an empty
  result caused solely by strength/lifecycle filters. Reverse map navigation starts at the last item.

### Zero configuration: install the plugin, start a session

- The graph learner can retain routine after-edit verification/generation as execution-only
  candidates when reminders have little value. Independent evidence requires normal session
  support and at least 90% immediate successful next-turn actions. Candidates remain silent
  without an exact local execution grant; their evidence is not a savings claim.

- E2 verification can use an explicitly approved project Python environment. The interpreter,
  dependency environment, and runtime are hash-bound and privately staged; pytest reads edited
  staged tests without turning those test inputs into fixed scripts. Unsupported layouts refuse
  execution. Review and both maps distinguish execution candidates from advisory previews.

- The execution plugin installs its checksummed `adage` learner companion beside `adage-exec`,
  preserving background learning when the plugin is the only installation. Release staging
  rejects archives with missing checksums. Plugin startup latency is verified on the release
  binary with real background learning; debug-only spawn-recorder tests remain separate.

- Approved learned lookups can deliver complete bounded multiline stdout. Unsafe, empty or
  oversized results are withheld; selected partial fields no longer claim the agent need not
  repeat the lookup. Complete stdout retains the explicit unstructured-output policy and human
  grant boundary. Automatic E1 execution abstains on ambiguous prompt arguments.
- The macOS executor resolves Apple's system Python launcher to its selected interpreter,
  avoiding Xcode discovery timeouts in fresh sandbox homes. Complete/raw lookup output from a
  nonzero exit is withheld instead of presented as a completed lookup.
- First-run integration checks now exercise six shipped advisory instincts and the Python
  environment CPU rewrite through real hooks, including opt-out and execution permissions.
- History discovery now agrees across learning, preview and setup, including configured Claude
  and Codex locations. Empty repositories can bind applicable instincts; mixed-case and
  dot-prefixed repository names publish valid learned tracks.
- Approved staged actions record durable preparation and completion in the existing journal.
  Uncertain completion pauses application for inspection and explicit recovery. Native tool
  identities prevent duplicate application; recovery guidance never requests an automatic retry.
- The Reflexes TUI separates evidence from execution authority and shows current grant state,
  declared effects and the latest staged-action outcome. Execution approval remains person-gated.
- macOS execution supports current loader and selected Xcode toolchain requirements while
  preserving confined reads, writes and network denial. Updated sandbox grants require review.
- Paid evaluation dispatch reserves per-session budgets durably and stops on unknown accounting.
  The current behavior reference and reproducible proof ledger distinguish execution from savings.

- **The marketplace plugin carries its binaries** (not published yet: the workflow has not run on
  a tag, and the `marketplace` branch does not exist). The release workflow stages both Claude Code
  plugins with checksummed binaries for all five targets (`scripts/stage-plugin.mjs`, archives
  checked against dist's `.sha256`, binaries against `dist/SHA256SUMS`) and publishes them to the
  `marketplace` branch (`.github/workflows/publish-marketplace.yml`); `marketplace.json` installs
  from it (`git-subdir`, `ref: marketplace`). No `adage` on `PATH` is needed and nothing is
  downloaded. When no binary can serve, the session-start shim says so in one line the person sees
  (a JSON `systemMessage`, verified against Claude Code 2.1.283: shown in the UI, never sent to
  the model), records it in `$DATA/status`, and points the other hooks at `scripts/quiet`.
- **Bootstrap at the first session start.** In a repository with no reflexes (or a damaged index)
  the SessionStart hook starts the learner at once (`condition --auto --bootstrap`): instincts and
  the index first, then this repository's earlier traces, so the first prompt is already directed.
  The hook says `adage: learning from N past sessions in this repo…` once. Only in a repository:
  a session opened in `~`, `/` or a deleted directory says nothing and starts no learner. A data
  directory that cannot be written is said once, with the fix (`ADAGE_HOME`), instead.
- **A fresh install is healthy.** Before the first session, doctor passes (no tracks or index yet
  is not a problem), the card counts the repository's past traces, and `adage status` and
  `adage watch` show the bundled embedding model. Switching models (`models fetch|install|use`)
  says which prompt-matching reflexes stop matching and conditions their repository again.
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

- **Card families whose files are named per task.** When each task creates files named whole
  after itself (chi's `middleware/no_sniff.go`, `middleware/hsts.go`), the files share no name
  shape, and no card formed. Files a task created now also count by their role: directory, test
  convention and extension. They group into `middleware/<x>.go` and `middleware/<x>_test.go`,
  both for family overlap and on the card. A test sibling stays its own role, in the learner and
  in the runtime's file match.
- **The follow judge reads wrappers again.** A run behind a wrapper
  (`xvfb-run -a .venv/bin/python -m pytest`) is a follow. `echo`/`cat`/`ls` naming the program is
  not, as before.
- **Linked worktrees share their repository's key.** A worktree made with `git worktree add`
  (one per task in tools that run agents in parallel, such as Conductor) has a `.git` file, and
  its `origin` lives in the common git directory; the key now reads it there (submodules read
  their own). `adage condition`, and the hook at the end of each turn, remember each checkout's
  key in `$ADAGE_HOME/repo-keys/<hash>.key`, so the traces of a removed worktree keep their
  repository even when it was removed before its learner ran. Key lookups write nothing.
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
  model (installed, or the one bundled in the binary), `adage condition` groups a repository's traces into **families** of similar
  tasks (first prompts at cosine ≥ 0.6 *and* edited file shapes overlapping, Jaccard ≥ 0.25). A
  family with a passing trace becomes a reflex whose response is a **card**: the files such a
  change touched in order (names that vary per task as patterns, `flag_<x>.go`, test siblings
  apart; files written by shell redirection included), what the latest passing trace added to
  each (whole hunks as the agent wrote them, clipped to fit 800 tokens), the productive commands
  that worked (no `git status`, `ls`, `cat`, `grep` or commits; a family's test runs as one
  command, and the test run that worked beside one to avoid) and those that failed with their
  error, and the check the traces finished with. A family needs at least two traces (one is any
  one-off bug fix), and a repository keeps at most 16 cards, the best supported. A card never
  quotes a secret-holding file (`.env*`, keys, credential stores), a file the checkout's
  `.gitignore` ignores, or a line, command or error that looks like a secret (the rules
  `adage share` checks packs with); `index.json` and the tracks are written owner-only (0600).
  It is delivered once per session when a prompt's best cosine to the family's prompts reaches
  `learn.card_threshold` (default 0.811, the research's pre-registered strict threshold), or,
  within 0.10 below it, at the first edit of a card file before the next prompt (a new prompt
  ends the wait); prime and observe only, and `adage review` refuses to strengthen it. The family's prompt library grows
  with every trace that joins it. Detection is precise but literal: through the hook, the Go
  family's card fired on 6/6 held-out tasks, 0/6 paraphrases and 0/53 same-repository negatives.
  `adage train cards` replays a card's commands on a clean checkout in adage-exec's sandbox
  (installs and commands with a placeholder are not replayed, and a failure that needed the
  network or a package cache records no verdict) and the next learn marks the ones that worked
  and stops listing the ones that did not. `adage sense` counts the families and cards,
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
- **Uplift gating.** A reflex is worth how much more often agents take its step when told than
  unaided (`p_follow − p_self`), not how often they follow it (evals/lab/research/uptake-delivery:
  96% followed, 21% changed behaviour). Conditioning estimates both as Beta posteriors per route,
  from the kind's prior, the unexposed training traces in the matching situation (the learned
  follow judge) and the control arm's `would_have` judgements; the value model uses the uplift in
  `A(s, d)`. A reflex or instinct below +0.1 is kept quiet (nascent, never delivered) until a
  person approves it; `adage review` and `adage sense` show each reflex's uplift and why a quiet
  one is quiet. The bar is `learn.uplift_min` in the engine parameters (0.1; the `demo` preset and
  `adage train`'s skill-directed sessions estimate uplift but do not gate).

### Runtime

- Hook clients for Claude Code (plugin with exec-form hooks), OpenCode (bridge plugin) and Codex
  (`hooks.json`); `adage install <harness>` prints, or with `--write` merges, the registration
  (showing the diff, asking at a terminal unless `--yes`, and keeping `<file>.adage-bak`).
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
- **Rewrites to the project's environment** (Claude Code `PreToolUse`, instinct U11): a Python
  test run or repro under the system Python is pointed at the project's verified `.venv`. The
  command is read as the shell reads it: only the command word of a top-level simple command is
  rewritten, never a heredoc body, a quoted string or a `-c` payload, and a command whose structure
  is not followed, that activates an environment (or runs in a session with `VIRTUAL_ENV` or
  `CONDA_PREFIX`), `cd`s outside the checkout, or pins another Python version is left alone. A
  rewrite the person's `Bash(…)` allow rules would no longer match is advice instead. A
  `pip install` of the project or its test dependencies into the system Python gets advice, never
  a refusal. In another checkout before its rebind, only an environment of that checkout's own is
  used. `adage delegate rewrites on|off` (`runtime.rewrite`) switches it, with no terminal and no
  adage-exec needed; `adage delegate off` needs no terminal either.
- The follow judge scores the simple command that runs the directed program, with its setup
  (`cd`, `source …/activate`): `ls .venv/bin && python -m pytest` no longer counts as following
  `.venv/bin/python -m pytest`, so it cannot misfire a correct instinct into extinction.

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

- `adage watch` (a live pane), `adage ui` (the full-screen app), the interactive `adage init`
  wizard, `adage map`, `adage doctor` (every check, with fixes), `adage status`, `adage report`
  and `adage graph`. Plain `adage` prints the status card; `adage ui` opens the app.
- **The terminal UI.** `adage watch` shows what happens now, what conditioning changed and the
  health lamps, each problem naming the command that fixes it; `Enter` inspects a row's exact
  directive text and evidence. `adage ui` has Live, Reflexes (with a fuzzy filter and review
  decisions), Map (stimulus → reflex → response in the terminal), Health and Settings tabs, and a
  command palette (`:` or `ctrl-k`) over every action and reflex. Under `NO_COLOR` strengths are
  spelled out (`FIRE`, `PRIME`, `INHIBIT`); `--plain` also drops glyphs, and `ADAGE_ASCII=1`
  keeps colour with ASCII glyphs.
- **The map.** `adage map` serves a web app from the binary (`web/map`, embedded): the reflex
  graph, live evidence as hooks fire (server-sent events), and a "How it works" walkthrough. It
  listens on 127.0.0.1 only and is read-only.
- **The command line is parsed by clap.** `adage --help` groups every command; `adage <command>
  --help` shows its options and never runs it. A mistyped command gets a suggestion,
  `adage completions <shell>` prints shell completions, `--plain` works everywhere, and usage
  errors exit with status 2. `adage status` uses the shared output grammar and has `--json`.

### Design system

- **One design system for every surface** (docs/design/style.md). Colours, type (Bricolage
  Grotesque, Hanken Grotesk, Red Hat Mono), spacing, motion and terminal glyphs are defined once
  in `design/tokens/adage.tokens.json` (W3C DTCG); `cargo xtask tokens` generates the terminal
  palette (`adage-term`) and the web map's CSS variables, and `cargo xtask tokens --check` and
  `cargo test` fail if either is stale or any text colour is below 4.5:1 contrast. The
  `adage-term` crate detects what the terminal can do (colour depth, UTF-8, light or dark ground;
  `ADAGE_COLOR`, `ADAGE_THEME` and `ADAGE_ASCII` override it) and provides the output grammar
  every command prints through. Terminal frames and printed output are pinned by insta snapshots.

### Evals

- `adage eval`: offline replay of recorded traces through the real runtime, scored against what
  agents did; `adage eval routes` scores learned reflexes against a sandbox's conventions
  (too-narrow reflexes are reported apart from false ones); `adage eval fidelity`; the L0 bench
  with a committed baseline, sweeps and a scorecard; property tests and a safety oracle; the live
  A/B runner and generated sandboxes (docs/evals.md).

### Platforms and distribution

- Build targets: macOS (arm64, x64), Linux (x64, arm64; static musl) and Windows (x64; execution
  is unavailable there). Pushes and pull requests run rustfmt, clippy, the Linux x64 tests and the
  DCO, cargo-deny and plugin checks; the full matrix (macOS, Windows, Linux arm64, the musl builds,
  NOTICE, the design and site suites) and the slow eval gates run weekly and on dispatch. Native
  Linux and Windows runs remain unverified for this preview.
- A release workflow (dist) set up for every target: `adage`, `adage-exec` and `adage-train`, each
  with shell, PowerShell and Homebrew installers, Sigstore build-provenance attestations and a
  CycloneDX SBOM. The npm package's launcher runs the binaries from a per-platform package chosen
  by `os`/`cpu` (all three binaries in it); nothing is downloaded at install time. None of this
  is published yet: the workflow has not run on a tag, and the tap and npm package do not exist.
- A Claude Code plugin marketplace with two plugins (`adage`, and `adage-exec` for execution);
  each uses a bundled, checksummed binary, else the one on `PATH`, and never downloads. Not
  published yet.
- The website (`web/site`).
- Apache-2.0, with a generated NOTICE of third-party licenses; `SECURITY.md`, `CONTRIBUTING.md`.
