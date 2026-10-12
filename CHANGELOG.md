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

### The runtime and hooks

- **An edit made through Bash is an edit.** A Bash call in a tool batch is read with the files it
  writes: a redirection's target (`cat > f <<'EOF'`, `printf … >> f`), `tee`'s operands, and the
  files an inline Python program opens for writing (`python3 - <<'E'`, `python3 -c`, a path held in
  a name), each right after the action that wrote it, and the commands written after a heredoc's
  start. So a schema rewritten by a heredoc arms the finish line, contexts and triggers as an Edit
  does, and `make gen` in the same command meets it. Learning, the graph and the fast path read
  commands as before.
- **The orientation card.** A delivered procedure card no longer quotes the exemplar lines; it says
  where its files stand in the checkout instead (line counts, a Python source's last definitions,
  a test file's last lines, a changelog's first headings), read locally with no command run and
  bounded at about 1,200 tokens. `ADAGE_CARD_ORIENTATION=0` delivers the earlier form.
- **Bootstrap at the first session start.** In a repository with no reflexes (or a damaged index)
  the SessionStart hook starts the learner at once (`condition --auto --bootstrap`): instincts and
  the index first, then this repository's earlier traces, so the first prompt is already directed.
  The hook says `adage: learning from N past sessions in this repo…` once. Only in a repository:
  a session opened in `~`, `/` or a deleted directory says nothing and starts no learner. A data
  directory that cannot be written is said once, with the fix (`ADAGE_HOME`), instead.
- **Every session start is counted.** In a repository with reflexes, SessionStart assigns the arm
  and spools one `session_start` record, so the savings estimate's denominator includes sessions
  where nothing matched.
- **Notices.** One line at session start (Claude Code), at most once a day per repository and only
  when something is new: a health problem, an install step waiting for review, a savings verdict,
  new reflexes. `ui.notices = off` silences them.
- **Self-heal.** A damaged index is rebuilt at the next session start, silently. Three failed
  learns in a row back off and say so. With `delegate on` but no adage-exec plugin, the Adage
  plugin keeps serving and says why. A blocked model download is classified (`offline`,
  `blocked`, `tls`, `timeout`, `http`), recorded in `models/.fetch-failed.json`, never retried by
  itself, and explained by `adage doctor`; `ADAGE_MODEL_BASE_URL` downloads from a mirror.
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
- **Measuring is opt-in.** A control arm (`runtime.control`, `ADAGE_CONTROL`) computes what Adage
  would have said and withholds it, so an effect can be measured against a control. It is off by
  default (0): a session where Adage stays silent reads as a broken install to the person in it,
  so every session gets Adage until someone runs `adage config set runtime.control 0.1`.
  `adage savings` says `not measured (measuring is off)` and how to turn it on; the learner's quiet
  test reads the training traces' unaided rate alone until control sessions exist. Evaluations
  force their arm (`ADAGE_ARM`) and are unaffected. Kill switches:
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
- **Steps Adage takes in the agent's place, for the Claude Code mod.** `adage hook claude
  TurnStep` answers, before a model request, the call a directive Adage already delivered names
  (`{"predict": {kind, route, track, calls}}`: a command, a sequence's first step, the reads a
  lookup listed, an armed finish line at the turn's end), only under a person's `predict` grant
  in `adage-exec`, never in the control arm, never at a turn's first request or twice in a row, at
  most six per session; `adage hook` alone never predicts. With the person's `routine_model` set,
  a step the model takes only to do what a delivered `fire` directive says answers
  `{"routine": true}` (`would_route` in the control arm). A call made with an `adage_` tool-use id
  is Adage's, live and in the trace (`by: adage`; cached parses are rebuilt), and is judged
  `predicted`, never `followed`. The command hooks' output is unchanged.
- **One decision per Bash call, and the mod's two phases.** A `PreToolUse` whose `tool_use_id`
  was decided this session is answered with nothing and no record (a ring of 64 per session,
  consulted only after the no-Python word scan, so the no-match path still reads nothing). The
  mod's `dry_run` answers the plan (a learned substitution, the interpreter rewrite, advice, a
  receipt's note) and records nothing; its `verdict` decides once, with Claude Code's own
  permission check replacing the planner's reading of the settings files (`checked_by: engine`).
  `adage hook claude Standing` answers the repository's standing reflexes once per session for the
  mod's cached system-prompt section, with the project's environment and what compaction should
  keep; the first prompt then leaves them out; a control session's text is empty.
- **Chains, the finish line, receipts as results, and attachment, for the Claude Code mod.** A
  lookup Adage makes is followed at the very next step by the reads its result names, as one
  response of several Read calls (`kind: "chain"`): the only step that may follow a prediction at
  once, never a third; a read batch takes a `predict` grant of its own (`Read {files}`). An armed
  finish line is predicted once per arming at the turn's end, and if Adage's call fails the Stop
  request asks as before. The mod's dry run of a Bash call answers a covering receipt as `fresh`
  (with its note and the result the call would be answered with) or not; its `verdict: {answered:
  "receipt"}` records the call once as made and passed and meets the finish line it covers. Every
  payload the mod sends reports its capabilities (`via: "mod"`, or an event only it sends), and
  `Standing` with `agent` answers what a spawned subagent is told: the lookup its own prompt binds
  and the standing reflexes. A subagent's tool calls carry its `agent`. Command hooks' output is
  unchanged.
- **Noticed: learning the person can watch.** The hook walks the candidates' own context net
  beside the reflexes' and records `noticed` (the candidate, its support, the support a reflex
  needs, whether it waits for review) once per candidate per session, at most 64, in both arms,
  never to the model; `adage watch` and `watch --follow --json` show it, `adage graph --json`
  lists it, and a repository with candidates and no reflexes takes the match path.
- **A `skill` reflex: the skill this repository uses at this point.** A route's step may be
  `do: skill` with a skill name (and optional arguments), after a prompt, an intent family, an
  edit or a learned context. It is delivered as one fixed sentence (`kind="skill"`, never above
  prime, never a dead end) and judged followed when the agent uses that skill. With the Claude
  Code mod, `TurnStep` may invoke it as the agent's own Skill call (`kind: "skill"`), only under a
  `predict` grant, with `skills.invoke` on, and for a skill `adage train` recorded as inert in a
  pack the person accepted. The index carries a `format`; a binary that cannot read an index a
  newer one wrote serves nothing and leaves it alone, and `adage doctor` says so.
- **Verified in place.** `adage train` writes `verified.json` beside the staged pack: each
  compiled command, what its replay found (`works`, `stale` with what failed, `not-replayed`), the
  date, and whether the skill is inert. Once the pack is accepted, `adage hook claude SkillPrompt`
  (the mod's `skill.prompt`) answers the skill's text as the model reads it with those lines added
  under a fixed, dated heading (sanitised, at most 2,400 bytes); the control arm changes nothing
  and records `would_annotate`.
- **A skill edited after it was accepted is neither invoked nor annotated.** `verified.json` names
  the skill file it was trained from (`skill.path`), and both `SkillPrompt` and a `skill`
  prediction read that file again first: unless it still hashes to the record, nothing is added and
  nothing is invoked, and whether it is inert is read from the file, not the record. A record from
  an earlier build has no path and is not used until the skill is trained again.

### Conditioning (learning from traces)

- **A repository script is classified from its command and its text, not from what it printed.**
  A lookup prints a work item, and a work item says things like "generated models include it":
  live run 2 kept `scripts/wi-lookup` an environment step on that line and so could not grant it
  predict. Once a trace shows the script's text and the text writes nothing, its output is not
  read for change words; without the text only a line that opens with one counts. A change a
  `git status` lists after the agent's own command that writes (a heredoc, `make gen`) is not
  laid on the script, and `python3 -I scripts/x` counts as a run of `scripts/x`.

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
- The graph learner can retain routine after-edit verification/generation as execution-only
  candidates when reminders have little value. Independent evidence requires normal session
  support and at least 90% immediate successful next-turn actions. Candidates remain silent
  without an exact local execution grant; their evidence is not a savings claim.
- History discovery now agrees across learning, preview and setup, including configured Claude
  and Codex locations. Empty repositories can bind applicable instincts; mixed-case and
  dot-prefixed repository names publish valid learned tracks.
- **Repository-scoped reading.** Automatic learning picks a repository's traces by Claude Code
  project directory and recorded working directory before parsing; other repositories'
  transcripts are never opened. Worktrees count as the repository (including repositories
  without `origin`: a linked worktree now shares its main checkout's key).
- **Sources are additive.** A learner started from a sandboxed session that cannot see some
  transcripts keeps them (and their cached parses); only files verifiably gone are forgotten.
- **Skill reflexes are learned from traces.** A Claude Code Skill tool use is now a step the
  learner reads (every other control tool stays out): a skill used after a family of similar
  requests, after an edit, or after a learned sequence becomes a `skill` reflex with the same
  support, rate and uplift bars as a command, observe and prime at most. A skill is never the
  context of another reflex, and a trace that a skill directive steered is no evidence for that
  skill alone. Skill uses are alphabet events now, so the miner sees them between other events.
- **Candidates: sequences counted before they are reflexes.** A sequence (a context of two or three
  events before a command) seen in fewer traces than a reflex needs, but in at least
  `min_support − 2` and never fewer than two, is now kept as a candidate, mined in a pass of its
  own that never changes the reflexes. Of the variants of one habit (the same traces, the same
  step) one is kept: the context nearest the step. A nascent reflex with a context is a candidate
  that waits for review. Candidates carry the id the same pattern gets as a reflex, are written
  through the learner's store to `<tracks>/<repo>/learned/candidates.json` (owner-only), compiled
  into the index apart from the routes (`candidates`, `candidate_contexts`, their words in the
  fast path's triggers) and listed under `candidates` in `adage status --json`. They are counted,
  never delivered. `learn.log` gains `change: "candidate"` with `support` and `s_min`, and a
  `new` reflex that was a candidate names the traces that tipped it (`sessions`).
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
- **Reusable mining checkpoints.** Repeated learning reuses unchanged repository mining while
  rerunning qualification, current review decisions and compilation. Content, parameter and build
  fingerprints invalidate stale calculations; corrupt caches trigger recomputation. `--no-cache`
  forces a full run, and reports identify mining cache hits. A warm run keeps the candidates and
  every reflex's supporting sessions, so its index is a full run's.
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

### Execution (opt-in)

- **A finish line's verification says whether the tests passed.** The result of a learned
  verification adage-exec ran after an edit used the regeneration's template ("what it generates
  was already up to date. Nothing changed."); it now says "it passed (exit 0, 4.0 s; receipt …)"
  with the tests the run's pytest record shows, or "it failed (exit 1; …)" with the tests that
  failed, in place of the directive. A regeneration's result is unchanged.
- **`.pytest_cache/**` covers the `.gitignore` pytest writes in its cache.** A wildcard still never
  matches a leading dot, except inside a directory the glob names with its dot, so a verify grant
  declaring the cache directory is no longer refused for the cache's own `.gitignore`; `**` still
  matches no `.env`, `.git/…` or repository `.gitignore`.
- **A session start no longer hashes a project Python's runtime every time.** Identifying a
  project Python read and hashed its interpreter (28 MB) and its runtime's `lib` tree (66 MB, 1,600
  files) at every session start, action and step that planned such a grant; one live start took
  5.75 s of its 10 s budget. The hook keeps each file's digest in
  `$ADAGE_HOME/cache/python-digests.json` by path, inode, size, modification and change time, and
  reads only what changed (a debug build: 2.0 s, then 65 ms with a cold page cache). The staged run
  still hashes the environment and its private copies in full.

- **`adage-exec` reads grants under the key they were written under.** `status`, `revoke`,
  `approve`, `run`, `completion`, `readiness` and `journal` key a checkout as the index holds its
  reflexes (the key `adage review delegate` writes a grant under and the hooks read it by), so
  `adage-exec status --repo .` lists a grant made for reflexes learned under a name such as
  `acme-billing`, where it showed none.
- **Delegating a nascent reflex says it is nascent.** `adage review delegate … --predict` on a
  reflex that waits for review said "no reflex …"; it now says the reflex is nascent and that
  `adage review approve <track> <reflex>` makes it eligible. Other refusals are unchanged.

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
- **Lapsed approvals are told, not silent.** When a grant stops matching, the person sees one line
  at most once per session naming the grant, what changed and the `adage review delegate …`
  command that approves it again; `adage delegate status` and `adage review` list lapsed grants.
  Bytecode in `__pycache__` no longer lapses a project-Python grant (adapter version 2).
- **`adage delegate policy [--max-level e1|e2]`** shows this machine's execution policy and its
  file; `--max-level e2` (a person, never in an agent session) writes the opt-in with automatic
  approval off and the sandbox required, keeping other settings. Refusal hints name it instead of
  an internal gate. `review delegate --timeout-ms` above the level's limit says it was clamped.
- Pytest's `.pytest_cache/**` is supported as an explicit cache declaration, while hidden output
  paths and root wildcards remain restricted. Invalid action-glob errors distinguish outputs from caches.
- On macOS, a direct system-Python action resolves Apple's launcher to the interpreter in the
  already selected developer toolchain, preventing sandbox startup timeouts without expanding
  permissions. Unix-only staging tests are guarded so Windows compile checks remain valid;
  the ARM64 musl sandbox retains its kexec-file denial when libc omits the syscall constant.
- E2 verification can use an explicitly approved project Python environment. The interpreter,
  dependency environment, and runtime are hash-bound and privately staged; pytest reads edited
  staged tests without turning those test inputs into fixed scripts. Unsupported layouts refuse
  execution. Review and both maps distinguish execution candidates from advisory previews.
- Approved learned lookups can deliver complete bounded multiline stdout. Unsafe, empty or
  oversized results are withheld; selected partial fields no longer claim the agent need not
  repeat the lookup. Complete stdout retains the explicit unstructured-output policy and human
  grant boundary. Automatic E1 execution abstains on ambiguous prompt arguments.
- The macOS executor resolves Apple's system Python launcher to its selected interpreter,
  avoiding Xcode discovery timeouts in fresh sandbox homes. Complete/raw lookup output from a
  nonzero exit is withheld instead of presented as a completed lookup.
- Approved staged actions record durable preparation and completion in the existing journal.
  Uncertain completion pauses application for inspection and explicit recovery. Native tool
  identities prevent duplicate application; recovery guidance never requests an automatic retry.
- macOS execution supports current loader and selected Xcode toolchain requirements while
  preserving confined reads, writes and network denial. Updated sandbox grants require review.
- `adage-exec`, a separate binary, runs only reflexes a person approved on that machine
  (`adage review delegate --level e1|e2`), inside an OS sandbox (Landlock and seccomp on Linux,
  bubblewrap, the macOS sandbox): **E1** prefetches an observe step's typed output beside the
  directive; **E2** (beta, behind policy) carries out a finish line or check in a staged overlay
  and applies only the declared outputs. `adage delegate on|off|status` swaps the hook
  registrations. Policy files, approvals keyed to what runs, automatic revocation; nothing is
  approved automatically unless policy allows the inert-observe class (docs/execution.md).
- **A person can decide from the Claude Code session, with a press.** `adage hook claude Press`
  issues the mod a single-use token in the window before the agent's first act (once per start;
  once more on `resume`; never on `compact`), and the person-only verbs the design lists (the
  review verbs, `review delegate` and `revoke`, `delegate on`, `delegate policy --max-level e2`,
  `delegate predict on`, `adage on`, and `adage add --trust` for a pack committed in the
  repository) accept it on stdin in place of a terminal, print the next token, and record `via:
  press` on the decision or approval (`via: terminal` otherwise). A wrong, spent, stale or foreign
  token behaves exactly as none; `install`, guarded settings, `share`, `train accept`, `packs on`,
  `init` and every `adage-exec` verb never take one. `review delegate … --stage --json` answers what
  a grant would be and do, recording nothing, and `--confirm` records it only if nothing but the
  repository moved; the first confirmed action on a machine at E1 writes the policy and the marker
  with it. `graph --json` gives each reflex's `offers`, `status --json` the packs awaiting trust,
  and the terminal UI's routes view shows predict grants (docs/execution.md, "Deciding from the
  session").
- **A press is harder to forge, and what it trusts must predate its session.** A window opens only
  for a start that names the session's own transcript, and a session whose window opened while
  another session on the machine was mid-turn (as a start written from a Bash call always is) has
  its press refused for good, in any repository, whatever acts in it later; before, acting in it
  once made it live. The tripwire reads a command with the shell's quotes taken out (`"Pr"ess`)
  and warns from the command hook, which the mod's own asks no longer pre-empt. `adage add --trust`
  takes a press only for a pack committed before the session opened and unchanged on disk since.
  A verb reads stdin for a press only inside an agent session, so CI with an open pipe waits for
  nothing. `adage-exec status` lists prediction grants apart, and `adage-exec revoke` revokes
  execution grants unless `--mode predict` is given (before, both). A session resumed a second
  time no longer warns that something else claimed its press.
- **An apply is no longer refused as busy because another process was starting.** A lock (the
  execution journal's, a worktree's, the learner's) is released explicitly when its holder lets
  go. Before, closing the file alone left it held while any process the same program was starting
  still had a copy of the descriptor, so the next apply in that worktree could be refused with
  "another action is applying in this worktree" although nothing was.
- **Press records carry `duration_us`, as every other spool record does.** `press_issued` and
  `tripwire` record the hook's own time and `press_spent` the spend's; before, they were the only
  spool records without it.
- **Explicit, durable completion readiness.** Optional required verification reflexes run even
  when the agent never started them, under existing E2 grants and sandbox gates. Current-input
  execution evidence distinguishes passed, missing, stale, failed and unknown checks; durable
  attempt limits survive restart. Ordinary advisory obligations also preserve later-edit
  invalidation during recovery. Command completion remains distinct from coverage or task quality.

### The CLI verbs

- **A reflex is named by what it does, everywhere it is listed.** A reflex that fires after a
  learned sequence says the sequence (`after editing crates/**/*.rs, then cargo build -p
  adage-cli, run cargo build -p adage-cli`), not "after a sequence of steps seen before"; the
  noticed patterns read the same way. `adage ui`'s Reflexes tree leads each row with the step and
  then when, the id heading the detail pane; and `adage watch` and `adage ui` say why a directive
  was not given in a control session (`would fire cargo fmt --all (withheld · control session)`).
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
- **`adage install <harness> --remove`** takes Adage's hooks out of the scope's file with a diff, a
  confirmation and a `.adage-bak`, leaving the person's own hooks and settings as they were
  (install then remove restores the file byte for byte).
  `install --write` and the init wizard no longer add a second registration where one exists.
- **`adage condition --repo-root <dir>`** conditions that repository alone; it was a hidden flag
  that plain `condition` ignored.
- First-run completion now points to learned-candidate review and execution status, distinguishing
  evidence approval from permission to run scripts. The first-reflex guide explains empty-history
  states, execution policy, scoped grants and how to verify an action actually replaced agent work.
- The local map retries failed snapshots with bounded backoff, times out stalled requests, and
  labels stale data with a manual retry. Continuous events no longer starve snapshot refreshes.
  First-run hook checks can recover and be rechecked; demo data is explicitly labeled.
- **Follow the evidence:** complete trace inventories and individual session views extend beyond
  the recent activity window, retain URL selections and browser navigation, and distinguish
  current-version outcomes from historical or unattributed events.
- In the TUI's Reflexes tab, Escape resets both the search and filter chips, including an empty
  result caused solely by strength/lifecycle filters. Reverse map navigation starts at the last item.
- The Reflexes TUI separates evidence from execution authority and shows current grant state,
  declared effects and the latest staged-action outcome. Execution approval remains person-gated.
- **Guided terminal Home and one palette:** choose a repository, connect a harness, configure or
  import local history, preview conditioning, run reports, manage models and signed packs, and open
  the browser map through forms. Demo data and real repository operations stay separate.
- **A fresh install is healthy.** Once its first hook reaches Adage, before anything is learned,
  doctor passes (no tracks or index yet is not a problem; until that first delivery it says the
  hooks are registered and waits), the card counts the repository's past traces, and `adage
  status` and `adage watch` show the bundled embedding model. Switching models (`models
  fetch|install|use`) says which prompt-matching reflexes stop matching and conditions their
  repository again.
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
- **The status card and `adage doctor` name the same fix.** Where a check's fix is one `adage`
  command, the card's `next` row now names the command `adage doctor --json` offers as the
  check's `action` (`adage condition` for a stale or damaged index, `adage models fetch` for the
  embedding model), where it used to say `adage doctor`; a fix that sends you to a terminal still
  points at `adage doctor`.
- **A check's fix is one command or yours to make, decided where the check is.** `adage doctor`'s
  fix line, its `next`, `--json`'s `action`, the status card's command and `adage ui`'s one-key
  fixes all read that one choice, and none reads a command back out of a sentence. Two outputs
  change. A hook registration that runs a binary which is gone now offers `adage install <harness>
  --write` as its action and on the card, where they said `manual` and `adage doctor`: run from
  this binary, that command is the fix. And `adage doctor`'s `next` gives a fix you make yourself
  as written, where it used to lift the first command out of it (`adage config set <key> <value>`
  for a bad setting, `adage review` for inert pack reflexes, `adage doctor --json` for slow hooks,
  `adage condition` for invalid tracks or a failed automatic run, `adage on` while a variable also
  keeps Adage off, one copy's `--remove` for hooks registered twice, `adage models fetch` for a
  damaged binary), none of which is the whole fix.
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
- **`adage ui` opens on Adage's brain.** The site hero's brain (its lobes, fissures, cerebellum
  and stem, folded by the same seeded noise), lit as a solid in shaded text and violet, turns
  slowly for about two seconds, seen from above and behind so both hemispheres, the fissure and
  the cerebellum show, with fire, prime and inhibit pulses running up the cord; then it dissolves
  into the app. Any key skips it, and every key but `Enter`, `Space` and `Esc` goes on to the
  app. A frame takes well under a millisecond, and is the same on every platform.
  `ui.start_screen` (on by default; `ADAGE_START_SCREEN=0`) turns it off; it never shows away
  from a terminal, in an agent's session, below 60×20, on a dumb terminal, with
  `ADAGE_REDUCED_MOTION=1`, or when `adage watch` hands over to the app.
- **Live is a cockpit: the session drawn as a graph while it runs.** `adage watch` and the Live
  tab of `adage ui` open on a pulse (a heartbeat lamp, reflexes fired by strength, followed as a
  meter, finish lines met, tool calls, and activity per minute with the minutes a reflex fired
  marked), then the session as it grows: each turn a box, its tool steps on a trunk in plain
  words, and each delivered reflex a `◆ adage` box branching off the step it came after and
  pointing to the response it asked for, green once the agent follows it and red when it does
  not. Quiet steps and quiet turns fold to one line; no session id shows until `Enter`. From 120
  columns a rail beside it has the reflexes in play (fired and followed, as meters), the last
  conditioning as a card with a run timeline and how close the nearest forming pattern is, and
  the health lamps; `l` lists every conditioning run. The heartbeat, a new step sliding in and
  a newly fired reflex flashing are off under `ADAGE_REDUCED_MOTION`, and the frame rate rises
  only while something moves. Narrow split panes keep one line per step. A harness set up but
  not used in a repository now shows as dimmed "not used here", not as "hook received".
- **Conditioning in `adage ui` is three cards, not a wall of text.** Before it runs, the history
  sources (one row per harness) and what they hold, as a bar: to learn from, already steered,
  empty. While it runs, its stages tick as a checklist (reading traces, n of N; finding patterns;
  compiling reflexes; settling overlaps; writing the index), followed through
  `adage condition --progress`; `esc` hides the card and the run goes on. After, one headline
  (`Read 122 traces · kept 7 reflexes · nothing changed`), a funnel from traces to reflexes, what
  changed by what each reflex tells the agent, the reflex sets, overlaps settled and notes, each
  in plain words and laid out to the width; thresholds, ids and the notes as written are under
  `v`, and `enter` takes the next step. `adage condition --json` adds `elapsed_ms`, `changes`
  (with each reflex's gist) and `waiting`, and each track its `reflexes`; its fields are otherwise
  unchanged.
- **A reflex that cannot be learned says why in one line.** `1 reflex not learned: its trigger
  matches too many different commands (20; the limit is 16): a, b, c and 17 more (run-ce9cdb)`,
  instead of every command it held; reflexes left out for the same reason are counted together.
- **The map.** `adage map` serves a web app from the binary (`web/map`, embedded): the reflex
  graph, live evidence as hooks fire (server-sent events), and a "How it works" walkthrough. It
  listens on 127.0.0.1 only and is read-only.
- **The command line is parsed by clap.** `adage --help` groups every command; `adage <command>
  --help` shows its options and never runs it. A mistyped command gets a suggestion,
  `adage completions <shell>` prints shell completions, `--plain` works everywhere, and usage
  errors exit with status 2. `adage status` uses the shared output grammar and has `--json`.
- **The live feed and the card as JSON, for a pane beside the session.** `adage watch --follow
  --json [--session <id>] [--since <cursor>]` sends the live pane's rows, `learn.log` lines,
  health changes and the learner's state as JSON lines with no terminal; every line carries a
  cursor a restarted follower resumes from with nothing lost or repeated, the reader's own session
  never names its arm, and it exits with its parent, its stdin or SIGTERM. Bare `adage --json` is
  the status card's rows one to one (`--for-session` names `/adage watch` in place of `adage ui`)
  (docs/live-learning.md).
- **`adage off` and `adage on`.** `off` writes the repository's `.adage/disabled` from anywhere,
  an agent session included; `on` removes it and is a person's decision. The status card's first
  line says when Adage is off here.
- **What a session cost, as its harness reported it.** `adage ledger note` takes one session's
  per-step usage (and the engine's own cost) on stdin, accepted only for a session the spool shows
  started in that repository, into `ledger/.harness.jsonl`; the savings estimate merges it over the
  transcript's figures under the ledger's lock, and a step Adage took in the model's place counts
  as a model turn not spent (`counts.predicted_steps`, `harness` in `adage savings --json`). The
  Claude Code mod sends the note at every turn's end, with each main-loop step's usage as the
  engine counted it.
- **How long the hooks take, on this machine.** Every spool record carries `duration_us`, the
  hook's own time, and `adage doctor` reports the p95 over tool batches that had nothing to say
  (a warning above 10 ms, a failure above 25 ms, "too few hooks to measure" under 20), measured at
  most every five seconds. `adage doctor --json` gives each fix a structured `action` beside the
  prose: one `adage` command a pane may offer to run, or `manual`.
- **The live feed carries what the mod's cards need.** `adage watch --follow --json --stdin
  ignore` keeps following when its parent closes stdin at spawn (it still ends with its parent or
  on SIGTERM); rows name their calls (`tool_use_ids`), a call answered by a receipt is an
  `answered` act with the run, its summary, the tests it ran and when, and an interrupted action is
  a `recovery` act naming what to run once the worktree is inspected. `adage --json` adds `ui`
  (the mod's `ui.spinner`, `ui.turn_line` and `ui.noticed` settings, off by default) and
  `recovery`.

### Review, packs and sharing

- **Review decisions in `adage ui` take the tracks lock**, as `adage review <verb>` always did, so
  a decision can no longer race an automatic learner writing the same track. While a learn run
  holds the lock, `adage ui` waits at most two seconds and then says to try again, with nothing
  recorded, instead of freezing.
- **Review the exact change:** proposal and pack confirmations bind to the previewed content;
  stale previews fail before trust or installation changes. Hook registration is shown separately
  from confirmed delivery, including Codex's manual acceptance step.
- `adage review` lists reflexes that need a person's decision with their evidence and the exact
  directive text; `approve`, `strengthen`, `weaken`, `extinguish`, `pin` and `dismiss` are
  recorded in `review.yaml` and survive re-conditioning. A pinned reflex is never overwritten:
  changes wait as proposals.
- **Packs** (format `adage-pack/1`) are the only format that travels: closed over everything they
  reference, content-addressed and signed (sshsig, namespace `adage-pack`). `adage share` converts
  a track, redaction-checks every byte and signs; `adage add` verifies against trusted signers
  (trust on first use, pinned by hash), validates in full, binds the pack to the repository and
  installs its reflexes at forming (docs/packs.md).

### The Claude Code mod

- **The prompt's lookup is made before the model's first response.** In run 3 the lookup reflex
  held its predict grant in every treated session and was never predicted: the model made the call
  in its very first response, and no step at index 0 was ever predicted. At a turn's first step
  Adage may now make one thing, a run reflex this prompt directed whose slots the prompt's own
  words bound (`scripts/wi-lookup W-2041`), under the same grant, permission verdict, session cap,
  once-per-route rule and dead-end interlock as any step, never for a subagent; a sequence, a
  skill, a chain, a finish line or an earlier prompt's directive still waits for a model step. The
  reads its result names follow at the next step, so the model's first request already holds the
  item and its files; the directive goes with the prompt as before, and the call's result says
  "Adage made this call in your place before your first response". The substitution's context
  line now reads the same whatever output a filter keeps: "adage ran make test for you;
  python3 -m pytest tests was replaced because that command fails in this repository (…). The
  output above is from make test."
- **A dead end piped into output filters is replaced too.** In run 3 every dead end hit was
  `python3 -m unittest … 2>&1 | tail -5` (or `| head`), and nothing was replaced because only a
  whole plain command was. When the dead end is a pipeline's first stage and every later stage only
  filters its output (`tail`, `head`, `grep`, `wc`, `cat`, `tee`, `sort`, `uniq`), the first stage
  is replaced and its own `2>&1` and the filters are kept byte for byte (`make test 2>&1 | tail
  -5`). The mod asks the permission check about both whole pipelines, the context line names the
  stage replaced, and the control arm and observe mode record `would_substitute` and change
  nothing. A list, any other redirection, a subshell or an expansion is still left as written.
- **A dead end a repository's files declare is replaced before it runs, as a learned one is.**
  Live, the two responses the index-0 lookup saved went to `python3 -m unittest … 2>&1 | tail -5`
  and a retry, because that dead end was a declared instinct (U1/U3: the Makefile's `test` target
  sets what the bare runner lacks) and only learned, firm routes were replaced. An instinct's
  `avoid` in service (an instinct is always prime and stays forming; never nascent, dormant or
  extinguished) with a fixed, slot-free `instead` is now replaced under the same permission
  double-check and pipeline rule, and the line gives its declared reason ("this repository's
  Makefile declares make test in its place (…)"). An `instead` that only runs the call's own tool
  in another environment (U11's `.venv/bin/python -m pytest`) is left to the interpreter rewrite,
  which keeps the call's arguments. Instincts still never block Stop and never become obligations.
  Live on W-2041: 3 requests where it was 5, no dead-end hit; the control arm records
  `would_substitute` and runs the dead end.
- **A predicted call is made only where the person's permissions allow it unasked.** Before it
  yields a predicted step, the mod asks Claude Code's own permission check about each of its calls,
  as it already did before a substitution or a rewrite, and yields only when every verdict is
  `allow`. On `ask`, `deny` or no verdict the request goes out as if nothing were predicted, and the
  band says once per session `» adage left a step to the agent: your permissions do not allow it
  unasked`. In `auto` mode every Bash call is the classifier's to decide, and it has no verdict for a
  call no model request produced: live, a predicted `make gen` was denied there; it is now left to
  the model, with no denial in the transcript, while a read an allow rule names is still taken.
- **A step held back is recorded as held back, not as made.** The mod now asks the binary about a
  step in two phases, as it does for a Bash call: a dry run (`TurnStep` with `dry_run: true`) that
  answers the prediction and commits nothing, then a verdict carrying the permission check's answer.
  On `allow` the step is committed as before; otherwise its `turn_step` record says
  `withheld: "permission"` with the engine's decision and sentence, it counts against neither
  `runtime.predict_cap` nor the rule against two predicted steps in a row, and its route is not
  offered again that session (another route still may be). A step from a command hook carries
  neither field and is decided exactly as before.
- **The harness ledger counts the session's tool calls.** The mod's note to `adage ledger note`
  carries `tool_calls`, every main-loop call the engine ran (the model's, the predicted ones, those a
  receipt answered; not a subagent's, not the mod's own), so a session's line no longer reads 0
  calls when no transcript pass ran.
- **The Bash tool's description carries the learned lookup conventions.** The mod asks `Standing`
  with `conventions: true` and appends each sentence the binary answers ("In this repository, work
  item details are looked up with …") after the environment note, once per session.
- **A subagent's calls no longer arm a prediction in the main loop.** An Explore subagent's lookup
  bound the read batch, and the main loop's turn end then read the files in the main thread (live
  O0, SUB). A subagent's calls are observed and recorded, and a directive they trigger is said to
  the subagent, but they bind no read batch, pending command or obligation, judge none of the main
  loop's directives, meet none of its obligations and count none of its follow windows down.
- **A predicted call the person's permission rules refuse is recorded as refused.** Claude Code
  reports such a call only in the batch, as `<tool_use_error>`; the batch took it as made, so a
  denied `Read` was judged `predicted` and a denied `make gen` would have met its finish line. It
  is now a failed call, judged `refused` (never `predicted`, never `obligation_met`), observed as
  `… !refused`, and the Stop request asks for the finish line as without the mod.
- **Spool records name the subagent that made the calls.** Decided `batch` and `failure` records
  and `pretool` records carry `agent` (Claude Code's `agent_id`) when a subagent made the call,
  through the record builder, so a delegation's calls can be counted on their own
  (`docs/spool.md`).
- **The Bash tool's description can carry the repository's lookup conventions.** `Standing`
  answers `conventions` (one sentence per learned prompt lookup at full strength, with its slot as
  written: "work item details are looked up with ./scripts/wi-lookup {item} --json, …") when the
  mod's ask carries `conventions: true`, fixed for the session and empty in control and observe
  mode; an ask without it is answered as before. The prompt that names an item is still told the
  bound command.
- **A session's ledger line keeps the tool calls its note counts.** `adage ledger note` reads an
  optional `tool_calls` and records it over a transcript's count; without it (and with no
  transcript pass, `learn.auto` off) the line read `tool_calls: 0` for every session.

- **The mod finds its binary without `CLAUDE_PLUGIN_DATA`.** Claude Code gives that variable to
  command hooks only, so under `--plugin-dir` and after `adage init`'s local-marketplace install the
  mod never ran its binary and stayed inert. It now derives the data directory itself: from an
  installed plugin's cache root, from the key Claude Code registered the plugin's folder under
  (`adage@adage-local` → `plugins/data/adage-adage-local`), or `<name>-inline` for `--plugin-dir`,
  under `CLAUDE_CONFIG_DIR` or `~/.claude` (`USERPROFILE` on Windows, either separator), and takes
  one only once the session-start shim has linked `bin/hook` there.
- **A step the mod makes in the model's place no longer costs a failed request.** Its stop states
  no reason (no request was made), so Claude Code sends the next request whole instead of naming
  a response the server never issued; before, every predicted step was followed by a 404
  `thread_not_found` and a replayed dispatch (live run 2: 12 of 12), which spent the request the
  prediction saved.
- **A call the mod made in the model's place says so.** Its result carries one plain line: Adage
  made the call in the model's place, from which learned reflex, and it need not be repeated.
  Before, a model that read a predicted `make gen` in its own turn apologised for having run it.

- **The Claude Code mod acts on what the binary answers.** Before a Bash call runs, the mod asks
  the binary's plan and Claude Code's own permission check: a fresh receipt answers a test run
  the rules would have let run unasked (the model reads the receipt as the result, the row draws
  the receipt card, and a covering receipt that could not answer draws one dim line beside the
  run), a learned replacement or the interpreter rewrite runs only when the rules treat both
  commands alike (the engine's verdict, with the binary's advice carried to the model when they
  do not), and anything the binary cannot plan runs as the model wrote it. Once per session the
  repository's standing reflexes become a `session` section of the cached system prompt, the
  project's environment note joins the Bash tool's description, compaction is told to keep the
  procedures followed, and a subagent is told the conventions its own task binds before its first
  request. A step the binary marks routine goes to the model named by the new
  `runtime.routine_model` setting at low effort (`adage --json` reports it as `runtime`; the hook
  is told only that one is set); a chain of reads is yielded as one response of several calls,
  and an armed finish line as the agent's own call at the turn's end. Every path fails open to
  the call, the step and the prompt as they were.
- **Adage draws in the Claude Code session.** The plugin's module observes the command hooks
  (their answers unchanged) and follows `adage watch --follow --json` for the session, resuming
  from its cursor when the reader restarts: the opening line with Adage's glyph, a dim line under
  a prompt a reflex fired at (what it tells the agent, never its id or a strength word), a marker
  under a tool row Adage rewrote, predicted, replaced or headed off, the band above the prompt
  (counts, the health lamp with its word, learning, one attention line), a toast when learning
  changed a reflex, and `/adage` (the card, then a pane with Live, Reflexes and Health, `Explore in
  browser`, `/adage why|off|pause|watch|reflexes|review|health|map`). Two `/config` rows: the band
  and an unasked pane. Nothing is drawn headless; a control session draws what a treated one with
  nothing fired draws; every child gets `CLAUDECODE=1` from a fixed argv allow-list, and nothing in
  the pane runs a person-only verb (docs/usage.md, "What the plugin shows in the session").
- **The pane reads in plain words, about this repository.** The Reflexes tab lists only the
  session's repository's reflexes (the graph holds every repository's; the status card names this
  one). What waits for a decision is **suggestions · optional; nothing waits on them**, one heading
  per kind (`Adage can run these itself, instead of asking the agent to`, `Adage can take these
  steps without a model request`, `Learned here, not told to the agent until you enable them`,
  `Agents here already do these unaided, so Adage stays quiet about them`), each sentence whole and
  wrapped with its answers beneath and `command` to show the terminal command. A reflex row leads
  with its step and then when (`cargo fmt --all · after editing crates/**/*.rs`), never its id,
  which moves to the expanded row with the trigger and the bound; tracks are headed `learned in
  this repository` and `instincts: Adage's built-in defaults…`, learned first. The band says `»
  Adage has 4 suggestions · /adage review`. Live hides rows where nothing was decided until `v`,
  saying how many it hides; the conditioning ticker shows its last five lines, short, a reflex by
  its sentence and without the index hash.
- **Adage asks in the session, and a press is a person's.** At a turn's end the mod asks, in the
  dialog Claude Code uses to ask a question, about one suggestion (`Adage learned: …. Tell the
  agent?` with `enable` / `not this one` / `why`; `Adage can do this itself, instead of asking the
  agent: …. See what it would do?` with `see what it would do` / `not now`, which opens a pane with
  three plain lines and the staged diff, then `allow` confirms against the stage's digest; `Adage
  can take this step without a model request: …. Allow it?`; a team pack with `use them`), the
  reflex's sentence quoted whole after a colon so it reads right in any form. At most one ask a
  session and one per item per day; a reflex kept quiet because agents already do it is never
  asked about or counted in the band. `/adage on` opens its own ask. Every answer, and every
  decision pressed in the pane's **suggestions** group or an expanded reflex row (`e enable`,
  `n not this one`, the four more verbs behind `d details` with the command to copy), runs the verb
  through the press: the single-use token `adage hook claude Press` issued at the session's start,
  held in engine memory, sent on stdin with the session id, its successor kept from the verb's
  first line; a `press_issued` record the mod did not cause switches presses off for the session
  with a warning; a verb that refuses the press before spending it leaves the token held; an
  answer given with no token held is kept for the next start in the same repository
  (`» kept for your next session`), where it is asked again (`Kept from your last session: … Run
  it now?`) and runs only on that answer, never from the store. The model cannot press. Learning shows as it happens: `○ adage
  noticed` under the tool batch that completed a counted sequence (`ui.noticed`), the band's `1
  pattern noticed`, and under a Skill tool row what Adage did with the skill (`◈ adage loaded …`,
  `◈ adage verified 3 of 5 steps here`), with the skill's text reaching the model annotated by
  `SkillPrompt` and `/adage why` listing the steps.
- **The tokens for the Claude Code mod.** `cargo xtask tokens` also writes
  `plugin/claude-code/hooks/tokens.ts`: every glyph with its ASCII twin, and Adage's roles as the
  engine's theme keys, so the mod draws with the person's Claude Code theme and no colour literal.
- **One Claude Code plugin ships both binaries.** `/plugin install adage@adage` is the only
  install step: the plugin bundles `adage` and `adage-exec` (each checksummed, else the one on
  `PATH`; it never downloads), and its hooks run `adage-exec` while `adage delegate on`'s marker
  exists, else `adage`, so they are registered once and never fire twice; the edit and Stop hooks
  have 30 s. This reverses Track C's second plugin (its D2): the retired `adage-exec` plugin stays
  one release as an alias that quiets what it left behind and asks to be uninstalled. Not
  published yet.

### Settings

- **`learn.codex_sessions = auto`** (the new default) reads `$CODEX_HOME/sessions`, this
  repository's rollouts only, once Codex hooks are registered.
- **`adage config set`** needs a person only for keys that change what Adage learns from or how
  much evidence it needs; other keys can be set from scripts and sandboxes.
- **Predictions per session are a setting, and a withheld decision leaves the call as it was.**
  `runtime.predict_cap` (default 6, 0 to 50, guarded) bounds the steps a session's model does not
  take; it was fixed at 6. In the control arm and in the mod's observe mode a learned substitution
  is recorded `would_substitute` without deciding the call, so the interpreter rewrite still
  decides it exactly as without the mod. A call the mod answered with a receipt is observed once,
  not again when the batch lists it.

### Evaluation

- **A dead end piped through `| tail` is counted.** The demo's `dead_end_hits` read a failed
  verification from its exit status, which a pipeline takes from `tail` (0); it now also reads it
  from the command and what it printed (`No module named pytest`, `FAILED`, `make: ***`).

- Evidence for this work is development-stage only: one valid development study on one task with
  development scaffolding (dev-v9b), not a launch statistic. The held-out comparison has not run.
  Live real-repository savings remain unproven.
- Live evaluation subprocesses no longer inherit launcher stdin as unintended model input.
  The regression reproduces the prior leak; contaminated diagnostic cohorts remain archived.
- Keep the unsuccessful learned-CPU pilot in the repository evaluation record, outside
  launch-site assets. Document hook-result reuse research and the next diagnostic protocol.
- First-run integration checks now exercise six shipped advisory instincts and the Python
  environment CPU rewrite through real hooks, including opt-out and execution permissions.
- Paid evaluation dispatch reserves per-session budgets durably and stops on unknown accounting.
  The current behavior reference and reproducible proof ledger distinguish execution from savings.
- `adage eval`: offline replay of recorded traces through the real runtime, scored against what
  agents did; `adage eval routes` scores learned reflexes against a sandbox's conventions
  (too-narrow reflexes are reported apart from false ones); `adage eval fidelity`; the L0 bench
  with a committed baseline, sweeps and a scorecard; property tests and a safety oracle; the live
  A/B runner and generated sandboxes (docs/evals.md).
- **The development gate for the Claude Code mod, scaffolded and not run.** `dev_v9.py --design
  dev-v9-mod` adds a MOD arm (A4's hooks plus the mod, a person's `predict` grant) with its own
  five controls, a `dry-run` that shows the person's grants before any positive block, and the
  registered `dev_thresholds_v9` rule read from the frozen manifest; the held-out runner takes MOD
  as BEST. The simulator's history gains a sub-support `make lint` habit (2 of 12 sessions), and
  its held-out replay a look around, so the lookup, the reads and the finish line can each be
  predicted. No model ran.
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

### Design system

- **One design system for every surface** (docs/design/style.md). Colours, type (Bricolage
  Grotesque, Hanken Grotesk, Red Hat Mono), spacing, motion and terminal glyphs are defined once
  in `design/tokens/adage.tokens.json` (W3C DTCG); `cargo xtask tokens` generates the terminal
  palette (`adage-term`) and the web map's CSS variables, and `cargo xtask tokens --check` and
  `cargo test` fail if either is stale or any text colour is below 4.5:1 contrast. The
  `adage-term` crate detects what the terminal can do (colour depth, UTF-8, light or dark ground;
  `ADAGE_COLOR`, `ADAGE_THEME` and `ADAGE_ASCII` override it) and provides the output grammar
  every command prints through. Terminal frames and printed output are pinned by insta snapshots.

### Installing and updating

- **One plugin carries all three binaries.** The Claude Code plugin bundles `adage`, `adage-exec`
  and `adage-train` for every release target, each verified against `dist/SHA256SUMS`, and its
  session-start shim links all three beside `bin/hook`, so `adage train` works for a person who
  installed nothing but the plugin; a missing or mismatched `adage-train` only removes its own
  link, and `adage` keeps serving. The retired `adage-exec` alias plugin is gone (nothing had been
  published, so there was nothing to quiet): the marketplace lists `adage` alone, and its
  description says what is bundled and that Windows is untested. When no binary can serve, the
  shim names channels that exist: `/plugin install adage@adage`, or `npm i -g @adage-sh/cli`.
- **One command sets a release's version.** `cargo xtask bump <version>` writes it into
  `Cargo.toml`, the lockfile, the plugin and marketplace manifests and the npm templates, and dates
  CHANGELOG.md's Unreleased section (the GitHub Release's notes); `cargo xtask release-check`, run
  in CI with the pinned dist, fails while those disagree or `release.yml` is not what dist
  generates. Homebrew is off the release until macOS signing exists, so a missing tap token can
  never fail a tag. `docs/release.md` is the procedure and the first release's checklist.
- **Adage knows when a newer version is out, and no hook asks.** `adage update --check` (and the
  background learner, at most once a day after it conditions a repository) asks the npm registry,
  then GitHub's latest release when the package is not published, with one 3-second request that
  sends nothing but itself, and caches the answer or the failure (`offline`, `blocked`, `tls`,
  `timeout`, `unavailable`) in `$ADAGE_HOME/cache/latest.json` for a day; `update.check = off`,
  `ADAGE_UPDATE_CHECK=0` or `ADAGE_OFFLINE=1` stop it. A test runs every hook event against a
  server that counts requests: zero. `adage --json` carries an `update` object (`current`,
  `latest`, `channel`, `next`, `text`) when a newer release is known.
- **`adage update` is a transaction with a way back.** It checks first (`… is the latest ·
  nothing to do` runs nothing), keeps the running binaries in `$ADAGE_HOME/previous/<version>/`,
  runs the channel's command with the version pinned (`npm install --global
  @adage-sh/cli@0.2.0`, `cargo install … --tag v0.2.0`, the release installer and its two
  companions; never `@latest`), verifies that the new binary answers that version (and its
  SHA-256 against the release's `SHA256SUMS` when one is published), then lets the new binary
  re-point the hook registrations that moved (each diff shown, the previous file kept as
  `.adage-bak`, only at a terminal), stage the local Claude Code plugin again and print doctor's
  summary. A failed step stops and names `adage update --rollback`, which puts the kept binaries
  back byte for byte. From the plugin's own binary it runs `claude plugin update`, or prints the
  `/plugin` steps.
- **Two installed versions never diverge silently.** Doctor's new `versions` check warns when
  `PATH`'s adage, the plugin's and the binaries hook registrations run answer different versions,
  with `adage update` as the fix; the plugin check compares versions before digests, so two builds
  of one release are fine and only a `--plugin-dir` checkout is sent to `plugin/build.sh`. An
  older adage that finds the index a newer one wrote says so once a day at a session start, stays
  quiet otherwise, and no longer conditions over it; doctor's `index` failure for it names `adage
  update`. The plugin's session-start shim removes the `bin/<name>-<sha>` copies an earlier plugin
  version installed once no link names them.
- **Every surface says when a newer Adage is out, and none calls it a problem.** The card's
  `update` row (`0.2.0 is out (you have 0.1.0) → adage update`), `adage version` (`adage 0.1.0 ·
  0.2.0 is out (adage update)`), doctor's `binary` check (still a pass, with `update adage update`
  and the lamp unchanged), `adage ui` Home's first rows with `u` and the palette's `Update Adage`
  (`adage update` as a job, confirmed first), `adage watch`'s header, and the session-start notice:
  its fifth and lowest part, `adage: 0.2.0 is out (you have 0.1.0); run adage update.` (`/adage
  update brings it` from the plugin's binary), said once per machine and dropped first when two
  higher parts fill the line. Notices from the plugin's binary name `/adage review` and `/adage
  health`, never "in a terminal".
- **Update from inside Claude Code with one answer.** `/adage update` asks `Adage 0.2.0 is out (you
  have 0.1.0). Update now? It takes effect in your next session.`; `update` is a press (the
  fourteenth site, `PersonVerb::Update`) that runs exactly `adage update`, which under a press
  updates only a channel that fetches (the plugin through `claude plugin update`, npm, the release
  installer), writes no settings file, registration or config, answers `manual` for a source
  build, a checkout or Homebrew, and ends on one JSON line the toast reads (`adage updated 0.1.0 →
  0.2.0 · takes effect in your next session`). `what changes` opens Health on a `version` row with
  the release's first lines, `u: update` and `Copy: adage update`. The band's second line says `»
  adage 0.2.0 is out · /adage update` below every other item, until the first prompt, once per
  release per machine. With no press held the answer is kept and asked again at the next start;
  headless, `/adage update` names the terminal. Every flag of `adage update` keeps the terminal's
  check, and `--rollback` is never a press.
- **`adage init` installs the Claude Code plugin itself, and connects every other harness.** With
  `claude` on `PATH` and the plugin not installed, `adage init` stages the plugin from the binary
  you ran (its hooks and modules travel inside `adage`; `adage`, `adage-exec` and `adage-train` are
  copied beside them with their `SHA256SUMS`) as a one-plugin marketplace under
  `$ADAGE_HOME/plugin/`, runs `claude plugin marketplace add` and `claude plugin install
  adage@adage-local`, and checks `claude plugin list`: no release and no `/plugin` typing needed.
  Claude Code reads it in place, so running `adage init` (or `adage update`) again stages the new
  binaries for the next session. A settings-file registration beside the plugin is taken out (the
  diff shown, the previous file kept as `.adage-bak`) so every hook runs once. Without `claude` on
  `PATH`, or when it refuses, the settings file is the route and the two `/plugin` commands are
  named. Codex and OpenCode get their hook files, each diffed and backed up. `adage init --yes
  --hooks user` (or `project`) does it all without a question; `--yes` alone still registers
  nothing and says so; `--print` runs and writes nothing; a second run writes nothing that is in
  place.
- **`adage init` fetches the binaries it needs, verified.** When `adage-exec` or `adage-train` is
  not beside `adage` (the shell installer and `cargo install adage-cli` install `adage` alone), it
  fetches them from the release of the same version, checks each archive against the `.sha256` the
  release publishes, and refuses one that differs by a byte; a checkout build is pointed at the
  `cargo build` that makes them. It ends on one screen: what is connected, where the binaries are,
  what Adage learns from, the first line your next Claude Code session opens with, and at most two
  next steps. A registration written by the plugin's own binary names the plugin's stable link, so
  a plugin update carries it.
- **One front door.** Bare `adage` ends with at most two next steps (`adage update` first when a
  newer Adage is out, then what waits for review, else `adage ui`); before any agent is connected it
  names the plugin's two slash commands and `adage init`, and each other harness found says `adage
  init connects it`. The map words every next step for where you work: `/adage review` and `/adage
  health` for a Claude Code plugin user, never "start `adage ui` in your terminal"; the `adage`
  commands otherwise; its top bar shows the version. `m` in `adage ui`'s Live tab opens the map in
  the browser, as in `adage watch`; the demo dashboard moved to port 8766 so it runs beside `adage
  map`.

### Platforms and distribution

- The teaser opens with sound by default, with an explicit play button when the browser blocks
  audible autoplay. Reduced-motion and data-saving preferences still wait for a click.
- Early-access signup checks availability before asking for an email. Closed registration and
  failed checks have clear states, retry controls, and a public walkthrough; requests time out
  without discarding the entered address.
- Plugin bootstrap prefers a native SHA-256 provider, reducing first-start checksum cost while
  retaining the same digest verification and portable fallback.
- The launch site now walks one concrete trace through conditioning and hook delivery, introduces
  the watch/TUI/map demos, and leads with completed shipped-product validation and its uncertainty.
  The approved 45-second dark/day teaser replaces the older duration and posters. The interactive
  walkthrough gives visitors a way to explore while early-access signup is unavailable; source
  installation clearly requires repository access before the public release.
- The plugin installs its checksummed binaries side by side, preserving background learning when
  the plugin is the only installation. Release staging rejects archives with missing checksums.
  Plugin startup latency is verified on the release binary with real background learning;
  debug-only spawn-recorder tests remain separate.
- **The marketplace plugin carries its binaries** (not published yet: the workflow has not run on a
  tag, and the `marketplace` branch does not exist). The release workflow stages the Claude Code
  plugin with its three checksummed binaries for all five targets (`scripts/stage-plugin.mjs`,
  archives checked against dist's `.sha256`, binaries against `dist/SHA256SUMS`) and publishes it to
  the `marketplace` branch (`.github/workflows/publish-marketplace.yml`); `marketplace.json`
  installs from it (`git-subdir`, `ref: marketplace`). No `adage` on `PATH` is needed and nothing is
  downloaded. When no binary can serve, the session-start shim says so in one line the person sees
  (a JSON `systemMessage`, verified against Claude Code 2.1.283: shown in the UI, never sent to the
  model), records it in `$DATA/status`, and points the other hooks at `scripts/quiet`.
- Build targets: macOS (arm64, x64), Linux (x64, arm64; static musl) and Windows (x64; execution
  is unavailable there). Pushes and pull requests run rustfmt, clippy, the Linux x64 tests and the
  DCO, cargo-deny and plugin checks; the full matrix (macOS, Windows, Linux arm64, the musl builds,
  NOTICE, the design and site suites) and the slow eval gates run weekly and on dispatch. Native
  Linux and Windows runs remain unverified for this preview.
- A release workflow (dist) set up for every target: `adage`, `adage-exec` and `adage-train`, each
  with shell and PowerShell installers, Sigstore build-provenance attestations and a CycloneDX
  SBOM. The npm package's launcher runs the binaries from a per-platform package chosen by
  `os`/`cpu` (all three binaries in it); nothing is downloaded at install time. None of this is
  published yet: the workflow has not run on a tag, and the npm package does not exist.
- The website (`web/site`).
- Apache-2.0, with a generated NOTICE of third-party licenses; `SECURITY.md`, `CONTRIBUTING.md`.
