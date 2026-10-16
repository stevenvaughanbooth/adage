# Adversarial review: launch candidate `claude/repo-maturity-roadmap-l95ohs` @ f0698ff

2026-09-29. Read-only review of the last 36 hours of changes. Nothing in `crates/` was edited, and
nothing was committed.

**How each finding was checked:**

- **Code reading.** Every finding cites file:line.
- **The PreToolUse planner, compiled on its own.** `crates/adage-runtime/src/pretool.rs` has no
  dependencies, so it was compiled with `rustc` in a scratch directory and fed edge cases.
- **The existing binary, end to end.** `target/debug/adage`, built 14:52 today, ran a real
  `hook claude PreToolUse` against a throwaway `ADAGE_HOME` and a fake uv-style `.venv`. The setup
  copies `pretool_rewrites_to_the_verified_environment` in `crates/adage-cli/tests/zero_config_plugin.rs`.

No cargo build was needed, so `/tmp/adage-review-target` was never created.

Severity: **blocker** (fix before launch), **should-fix** (fix or disclose at launch), **nit**.

---

## Blockers

### B1. The PreToolUse rewrite edits text inside heredocs: files and commit messages get the machine's absolute venv path

- **Where:** `crates/adage-runtime/src/pretool.rs:68` (a `\n` resets `at_start`, with no heredoc
  awareness), `:166-221`, and the hook at `crates/adage-hook/src/lib.rs:310-337`.
- **Verified end to end with the binary.** The agent runs:
  ```
  cat > ci.sh <<'EOF'
  #!/bin/sh
  pytest -q
  EOF
  ```
  Claude Code receives this `updatedInput`:
  `cat > ci.sh <<'EOF'\n#!/bin/sh\n/tmp/.../work/click/.venv/bin/python -m pytest -q\nEOF`.
- **Other cases:**
  - `git commit -F - <<'EOF' … pytest tests passes now … EOF` rewrites the commit message.
  - A heredoc with a line starting `pip install` (a `setup.sh` or a Dockerfile helper) is
    **denied** outright.
- **What goes wrong:** the agent writes a CI script or README snippet containing
  `/home/alice/src/proj/.venv/bin/python`, and it is committed and pushed. This is a machine path
  and user name leaking into a repository. It also contradicts the claim, printed on every
  rewrite, that "Nothing else in the command changed" (README:96, docs/execution.md:136-140).
- **Fix:**
  - Treat a heredoc body as data. After `<<[-]['"]?WORD`, skip to the line equal to `WORD`
    (`adage_core::judge::tokens` already treats the heredoc body as data).
  - Refuse to plan any command with an unterminated heredoc.
  - Add the cases above as tests.

### B2. The `pip install` refusal is too broad, and it overrides the user

- **Where:** `pretool.rs:171-184`; `lib.rs:339-341`; `claude.rs:93-95` (`permissionDecision: deny`).
- **Refused (verified):**
  - `pip install --user httpie`, `pip install pre-commit && pre-commit install`: a CLI tool
    unrelated to the project.
  - `conda activate py311 && pip install numpy`.
  - `python -m pip install --target /tmp/x foo`: this never touches the system Python.
  - `if …; then pip install x; fi`.
  - Heredoc bodies (B1).
- **Ignores the shell's environment.** Nothing reads `VIRTUAL_ENV` or `CONDA_PREFIX` (grep finds
  none in `crates/`). A user who launched `claude` from an activated venv or conda env has `pip`
  pointing at that environment. Adage still refuses, with the false reason "installing into the
  system Python does not reach it".
- **Beats the user's own rules.** A hook `deny` wins over the user's own
  `permissions.allow: ["Bash(pip install:*)"]`, so the user cannot opt in per command.
- **On from day one.** It is on as soon as the instinct binds, with zero sessions of evidence.
  Reproduced: the bootstrap learn alone enabled it.
- **The person is not told.** `Deny` carries no `systemMessage` (lib.rs:339-341, compare
  :329-333), so they may not know why their agent keeps failing.
- **Fix:**
  - Refuse only when all of these hold:
    - neither `VIRTUAL_ENV` nor `CONDA_PREFIX` is set in the hook's environment;
    - there is no `--user`, `--target`, `--prefix` or `--root`;
    - there is no `sudo`, `conda`, `activate` or `venv` in the command;
    - the command is not inside a heredoc.
  - Or better, **only advise** (`additionalContext`) instead of denying unless the requested
    package is one of the project's own or declared test dependencies.
  - Add a `systemMessage` on deny.
  - Add a dedicated `instincts.pretool = off` config key (see S6).

### B3. Procedure cards copy file contents, commands and error lines into agent context with no secret scan

- **Where:** `crates/adage-learn/src/cards.rs`:
  - `:112-124` (`added`): all added lines;
  - `:302`: a `Write` puts the whole file into an exemplar;
  - `:316-320`: `echo "KEY=…" > .env` makes the whole command line an exemplar;
  - `:352-362` (`error_line`);
  - `:483` (`plain` commands).
- **The only filter is `line_ok`** (`crates/adage-tracks/src/card.rs:160-167`). It checks
  printable characters, length and markup, not secrets. `safe_command` (`io.rs:105-112`) allows
  `=`, `:`, `@` and `/`, so `GITHUB_TOKEN=ghp_… gh pr create` passes. `path_ok` accepts `.env`.
- **What goes wrong:**
  - In session A the user pastes an API key, and the agent writes it into `config/settings.py` or
    `.env`. The file is edited in half of the family's sessions, so it becomes a card file.
  - Every later session of the family gets those lines "as an exemplar": re-sent to the model
    provider in every matching session.
  - The index is harness-agnostic, so a card learned from Claude Code traces is also delivered to
    Codex or OpenCode sessions, which means a different provider.
  - Cards live in `index.json` and the tracks' YAML, written with the default umask (usually
    world-readable, 0644). Only the parse cache is 0600 (`cache.rs:95`).
- **This is default-on.** `learn.cards` defaults to true (`adage-store/src/config.rs:197-203`).
- **Fix:**
  - Move `adage-cli/src/redact.rs`'s token rules into `adage-tracks` or `adage-core`, and apply
    them in `card::check`, dropping the line or the command.
  - Never take exemplars from dotfiles, `*.env`, `*secret*`, `*credential*` or `*.pem`.
  - Write `index.json` and the tracks with mode 0600.
  - Add a test with a `sk-…` or `ghp_…` line.

### B4. The README says the shipped cards are "validated separately below": no such validation exists, and none has run

- **What the README says** (README:152-154): cards are "now built into `adage condition` and the
  hooks, and on by default … The shipped version is validated separately below." Nothing below
  it does that.
- **What the data says:**
  - `evals/launch/go-validation/README.md:3` states: "**No paid session has run.**"
  - The only product-binary evidence is a $0 dry run on four **synthetic** transcripts built from
    the gold patches (`dryrun_gate.json`), with 8 negatives (Wilson upper bound 25%).
  - The click regression slice has not run. It is the one designed to see whether cards learned
    from ordinary bug-fix history hurt one-off fixes (S8).
- **Every live card number** in the README comes from the Python prototype (a `CLAUDE.md` file or
  `card_hook.py`), not from `adage_learn::cards` plus the Rust hook.
- **Fix:** do one of:
  - run the pre-registered GO validation, both the family and the regression slice, before
    launch;
  - ship `learn.cards = false` by default, labelled experimental;
  - at minimum, delete "validated separately below" and say plainly that the Rust port has not
    been measured live.

---

## Should-fix

### S1. PreToolUse fires on commands aimed at another directory

- **Where:** `pretool.rs:166-213`; `lib.rs:204` (the root comes from the payload's `cwd` only).
- **Verified end to end:** `cd /home/me/otherproj && pytest` becomes
  `cd /home/me/otherproj && /…/click/.venv/bin/python -m pytest`. The other project's tests run
  with this repository's interpreter and packages.
- **Fix:** if any `cd`, `pushd` or `-C` target in the command resolves outside `root`, plan
  nothing.

### S2. Rewriting explicit interpreter versions is not "strictly equivalent"

- **Where:** `pretool.rs:134-136`, where `is_system_python` matches `python2`, `python3.9` and
  so on.
- **Verified:** `python3.9 -m pytest tests` and `python2 -m pytest tests` are both rewritten to
  the venv's Python.
- **What goes wrong:** an agent deliberately checking 3.9 compatibility gets a pass from 3.12, and
  is told nothing else changed. This breaks the "strictly equivalent" claim in
  docs/execution.md:159.
- **Fix:** only rewrite bare `python`/`python3`, or a version equal to the venv's `pyvenv.cfg`
  `version`.

### S3. A second checkout's tests are rewritten without checking that the project imports from that checkout

- **Where:** `lib.rs:273-289`. In another checkout it sets `env.pytest = true`, clears `modules`,
  and checks only that the interpreter, `pyvenv.cfg` and pytest exist.
- **What goes wrong:**
  - A git worktree whose `.venv` is a symlink to the main checkout's venv, or a copy of it, has an
    editable `.pth` that points at the **main** checkout's `src/`.
  - The rewritten `python -m pytest` then tests the main checkout's code, not the worktree's.
  - A loud "No module named pytest" turns into a silent pass or fail against the wrong code.
- **The code already knows this:** its comment says a repro "needs the bind-time check that the
  project imports from this checkout". Test runs need it just as much.
- **Fix:** read the `.pth` files, as `verify_env` does (`adage-instincts/src/lib.rs:136-152`), and
  require them to point inside `root`. Or skip rewriting in other checkouts until the rebind.
- **Related (nit):** `verify_env` accepts an environment with pytest but no project import
  (`pytest || !modules.is_empty()`, lib.rs:165), while docs/execution.md:144-148 says "and the
  project imported from this checkout".

### S4. "Claude Code's own permission checks apply … exactly as to the original" is not true for rule matching

- **Where:** docs/execution.md:151-152; README:96-97; `claude.rs:86-91`.
- **How permissions match:** Claude Code matches `Bash(...)` rules against the command string. The
  rewritten string starts with `/abs/path/.venv/bin/python -m pytest`, so
  `Bash(pytest:*)` and `Bash(python -m pytest:*)` no longer match it.
- **What goes wrong:**
  - Interactive users get a new permission prompt for commands they had allowlisted.
  - Headless `claude -p --allowedTools "Bash(pytest:*)"` (CI, scripts) gets the call **denied**.
- **Why the evidence missed it:** the micro slice ran with every Bash call allowed.
- **Fix:**
  - State this in the docs.
  - Consider emitting the relative `.venv/bin/python`, which matches `Bash(.venv/bin/python:*)`.
  - Or skip the rewrite when the session's permission mode is not interactive. The payload
    carries `permission_mode`.

### S5. The documented off switch is coupled to execution and cannot be turned back on without `adage-exec`

- **Where:** `crates/adage-cli/src/exec.rs:125-141` and `:172-180`.
- **What goes wrong:**
  - `adage delegate off` also turns execution off and rewrites hook registrations.
  - `adage delegate on` **bails** unless an `adage-exec` binary sits beside `adage`, before it
    removes `exec/rewrite-off`.
  - Plugin-only users have no `adage-exec` (launch README limitation 6). Once they turn rewrites
    off, they cannot turn them back on except by deleting `$ADAGE_HOME/exec/rewrite-off` by hand.
- **Fix:** add its own switch, for example `adage config set instincts.pretool off` or
  `adage instincts pretool on|off`, independent of E1 and E2.

### S6. A stale design doc still promises the opposite

- **Where:** `docs/Adage — Design Spec.md:1030`, which says: "Adage never blocks a tool call,
  never rewrites a tool's input, and never denies one." The PreToolUse row above it says "Observe
  only".
- **Why it matters:** readers will check.
- **Fix:** update the spec, or mark it as superseded by docs/execution.md.

### S7. A grey-zone card waits for the rest of the session and fires on a later, unrelated task

- **Where:** `crates/adage-runtime/src/lib.rs`:
  - `:518` sets `st.card_waiting`;
  - `:611-622` fires it at the first edit of any card file;
  - nothing clears it when a new prompt arrives (grep: set at :518, cleared only at :616).
- **What goes wrong:**
  - Prompt 1 scores in the grey zone and the agent edits nothing of the card.
  - Prompt 5, an unrelated bug fix, edits `src/utils.ts`.
  - The card is delivered then, with "How changes of this kind were done here before".
- **Side effect:** `fast.rs:64` sends every batch down the full path while a card waits, so the
  latency cost lasts the whole session.
- **Fix:** clear `card_waiting` on every `Prompt` event, and on `IntentChanges`.

### S8. Cards are unbounded, and single-session cards form from ordinary bug fixes

- **Where:** `cards.rs:380-383`, `cards.rs:579-699`, `engine.rs:1045-1051`.
- **A card per bug fix:** any passing session that created no new file (most bug fixes) becomes a
  one-trace card:
  - all its edited files (up to 8) are card files;
  - its threshold is 0.753, with a grey zone down to 0.653.
- **The research warns about exactly this** (`recurring-workflows/confirm/README.md:103-107`):
  - Whole-prompt cosine "mostly measures style".
  - On the tuning families, bug-fix negatives reached **0.80**, which is inside the grey zone.
  - The 0/53 false-fire result was measured against a 4-trace family card, not against cards made
    from bug fixes.
- **No cap on count or size:**
  - The graph compiler caps routes at 256 (`adage-graph/src/compile/mod.rs:322`), but cards are
    appended after it with no cap.
  - One real card is **9,001 bytes** in `index.json` (`dryrun_gate.json`), mostly 16×256 `i16`
    prompt vectors as JSON.
  - A repository with a few hundred sessions could reach megabytes. The whole index is parsed on
    every hook, including PreToolUse. The budget is p95 ≤ 10 ms with no match.
- **Families may merge:** families are single-link components at cosine ≥ 0.6. In a real history
  they can chain into one mega-family with no file edited in half its sessions, which silently
  yields **no** card for the real recurring family. Nobody has tested this with realistic history.
  The GO dry run had exactly 4 family sessions and nothing else.
- **Fix:**
  - Require at least 2 traces for delivery, or at least keep single-trace cards out of the grey
    zone.
  - Cap cards per repository, for example the top 16 by support.
  - Store prompts compactly: base64 `i8`, or outside the hot index.
  - Run the regression slice.

### S9. `adage train cards` marks good commands as failing and deletes them from cards

- **Where:** `crates/adage-train/src/cards.rs:36-41` and `:66-70`, `adage-exec/src/env.rs:28`
  and `:137`.
- **Placeholders replayed literally:** an anti-unified finishing check such as
  `go test ./<…>` is pushed into `cmds` and replayed with the literal `<…>` argument. It always
  fails, the verdict becomes `false`, and the next learn drops the finishing check
  (`cards.rs:537`).
- **No network, no home:** the replay sandbox has no network, and `HOME`/`XDG_CACHE_HOME` point
  at scratch. `go test ./...` (module cache in `~/go/pkg/mod`), `cargo test`
  (`~/.cargo/registry`) and similar commands fail for reasons that have nothing to do with the
  command. They are recorded as "does not work on a clean checkout" and removed from "Commands"
  (card.rs:254).
- **The irony:** the docs' own example card lists `go test ./...` (docs/training.md:364).
- **Fix:**
  - Never replay commands containing `<…>`.
  - Record a verdict only when the failure is not an environment or network failure. Reuse
    `outcome::environment_failure` and add patterns such as `dial tcp`, `could not resolve`,
    `failed to download`.
  - Or lend read-only module caches.

### S10. The follow judge credits a follow that did not happen, and that can extinguish U11

- **Where:** `crates/adage-core/src/judge.rs:65-79`: token containment over the whole first line,
  across `&&`/`;` segments.
- **What goes wrong:**
  - Directed: `.venv/bin/python -m pytest tests`.
  - The agent runs `ls .venv/bin && python -m pytest tests`, which ignores the directive and uses
    the system Python.
  - Every directed token is present, so the score is 1.0 and it counts as "followed".
  - The output has "No module named pytest", so it is judged **misfired**.
  - `instincts.rs:223` extinguishes an instinct once it has misfired
    `EXTINGUISH_MISFIRED` times and at least as often as followed. A correct reflex is turned off
    by agents that ignored it.
- **Fix:** score each simple-command segment separately and take the maximum. Require the
  directed program token to be in command position.

### S11. Card ids are keyed to the earliest session, so rejections do not stick

- **Where:** `cards.rs:571-575`, `:628`; review decisions are keyed by route id
  (`adage-tracks/src/review.rs:89`).
- **What goes wrong:**
  - Claude Code deletes transcripts after 30 days by default
    (`docs/Adage — Architecture.md:1228`).
  - When the earliest session of a family ages out, or an older transcript is imported, the id
    changes.
  - A card the person rejected in `adage review` comes back under a new id, and its replay
    verdicts are lost.
- **Not fully verified:** I did not check whether the parse cache outlives transcript deletion.
- **Fix:** give the id a stable basis (the family's file-shape set plus the first prompt's
  quantised embedding), or carry decisions over by overlap.

### S12. README Benchmarks: numbers match their sources, but caveats are missing and some claims go beyond them

Every number was checked against `evals/lab/research/recurring-workflows/{README,confirm/README}.md`,
`evals/lab/skills/README.md` plus `results/acme-product-arms.json`, `evals/launch/README.md` and
`evals/lab/research/harness/README.md`. **The figures themselves match.** The problems are claims
and caveats:

| README | Problem | Source |
| --- | --- | --- |
| :130 "Intervals are 90%" | With 6–12 clusters these under-cover. The source says "read them as roughly 80%". The Go CI is a bootstrap over **6** variants. | recurring-workflows/README.md "Results"; confirm/README.md:69 |
| :145 Go row, −33.8% | **6 of 12 control sessions hit the 60-turn cap**, against 0 of 12 with the card. Both arms passed 2/12: 10 of 12 failed on a step no training session did. It is a cost cut on mostly failing work, and the row says neither. | confirm/README.md:71-92 |
| :146 "Adage's instincts only" | That arm also delivered a **learned** click route: 12 of its 34 directives. It is "Adage as of then", not instincts only. | recurring-workflows/README.md "Arms" |
| :148 "0 wrong deliveries on 53 …" | Missing: paraphrase recall **0/6**; on the tuning families a negative scored 0.80 (inside the product's grey zone); the prototype hook, not the Rust port. | confirm/README.md:32, :103-108 |
| :149 "one earlier session is enough … (click, marshmallow)" | n=1 was run live only on marshmallow: −15.3% [−30.9, **+0.4**], n=6, with the CI crossing zero. Click's family **creates** a new test file each time, and the shipped learner refuses a one-trace card when a file was created (cards.rs:380). The click example is wrong for the product. | recurring-workflows/README.md "Learning curve" |
| :152-154 "validated separately below" | See B4. | go-validation/README.md:3 |
| :175 "work across Claude Code, Codex and OpenCode" | Compiled skills were measured only in Claude Code. | skills/README.md §6 |
| :192 "Held-out replay: about 3.9%" | This is an offline counterfactual (hindsight replay), not a measurement. Say "offline". | harness/README.md:242-243 |
| :200 "No harm was measured in 196 real-repository sessions" | Written at 04:35 (e92321d), **before** PreToolUse rewrite and deny (05:14) and default-on cards (10:37). The same source lists resolve rate 0.62 vs 0.65, and "Measurement power" as a limitation. Say "no significant harm, underpowered, measured before the current defaults". | evals/launch/README.md:315-322 |
| docs/execution.md:172 "63 of 66 … 130 system pip installs" | No linked data or script. The planner test replays one trace. | – |

### S13. Third-party licence notices are missing from the npm and plugin distributions

- **Where:**
  - `NOTICE` covers the bundled potion-base-8M data (MIT) and the crates, but only `cargo-dist`
    archives include it (`dist-workspace.toml:43`).
  - The npm packages ship `files: ["bin/"]` and `["bin/adage.js"]` (`npm/platform/package.json:10`,
    `npm/cli/package.json:9`). npm adds LICENSE automatically, never NOTICE.
  - `plugin/build.sh:12` stages only the binary.
- **Why it matters:** MIT requires the notice "in all copies or substantial portions". The binary
  embeds the model table, 7.7 MB.
- **Fix:** copy `NOTICE` into the npm platform package and `plugin/claude-code/`, or add
  `adage licenses`, which prints an embedded NOTICE.
- **Checked and fine:**
  - The model's MIT licence and attribution text are correct.
  - The bundled hashes match `BUNDLED_*_SHA256` (sha256sum run).
  - Fetch verifies the npm tarball's SHA-256 before extracting (`adage-cli/src/models.rs:283-292`),
    and every file's SHA-256 after.
  - Extraction (`adage-intent/src/npm.rs:88-110`) writes only spec-named files under fixed names.
    A tar path is compared, never joined, so path traversal is **not possible**.
  - Size limits bound decompression.

### S14. `adage train` agent sessions: the sandbox is unverified and the repository's settings are loaded

- **Where:** `crates/adage-train/src/harness.rs:280-316`.
- **What the code says:** it relies on Claude Code's own sandbox, and its comment admits
  "Unverified here: no session was run".
- **The fallback risk:** where bubblewrap or Seatbelt is unavailable, whether Claude Code refuses
  or silently runs Bash unsandboxed with `dontAsk` is untested. This is where a malicious SKILL.md
  in the system prompt would act.
- **Settings loaded from the repository:** `--setting-sources project,local` loads the cloned
  repository's `.claude/settings*.json`. Hooks defined there run outside any sandbox.
- **Fix:**
  - Pre-flight: refuse to `--execute` unless `bwrap` or `sandbox-exec` is present.
  - Pass `--setting-sources` without `project`.
  - Run one canary session (the agent tries to write outside the clone) before claiming it.
- **Checked and fine:** the replay path itself (`adage-exec/src/replay.rs`) looks sound:
  - a shallow `--no-local` clone with global and system git config disabled;
  - Landlock plus seccomp that denies all `socket` calls, plus a network namespace, or bwrap
    `--unshare-all`;
  - HOME excluded from reads;
  - lent directories read-only;
  - no sandbox means nothing runs.

### S15. "`adage` spawns nothing" is enforced by a grep that misses library calls

- **Where:** `crates/adage-exec/tests/no_spawn.rs:12-31` scans only the `adage-*` crates' own
  `src/`.
- **The gap:** `adage-cli` links `adage-exec`, and `adage review delegate`'s preview calls
  `adage_exec::stage::stage` (`adage-cli/src/exec.rs:340`). That stage runs the action, sandboxed,
  from the `adage` process.
- **The claim this contradicts:** the Architecture doc (Q4a) says the `adage` binary runs "no
  command", as do `cards.rs:30` and `adage-train/src/cards.rs:4`.
- **Severity:** this code is older than the 36-hour window, but the new text restates the claim.
- **Fix:** reword the claim ("the hooks spawn nothing; a person's `review delegate` preview stages
  once in the sandbox"), or extend the test to calls into `adage_exec::{stage,run,replay}`.

---

## Nits

- **N1. `adage share` dumps whole cards into its terminal output.**
  - `adage-pack/src/convert.rs:302` formats a skipped card as `format!("step {other:?} …")`.
  - `share_pack.rs:266-270` prints it under "Not carried": multi-KB `Debug` text with the code
    exemplars, which could hold secrets (B3), and 16×256 vectors.
  - The pack itself correctly excludes cards (verified: `Step::Card` has no v2 form, and the
    redaction check scans every pack byte).
  - Fix: print "procedure card (first-hand only)".
- **N2. The card overstates what "passed" means.**
  - The text says "(k passed the project's checks)" (`card.rs:236`), but "passed" is
    `proxy_outcome`: the last verify-class command the agent chose did not fail
    (`import.rs:364-373`).
  - Say "ended with a passing check".
- **N3. Card commands include noise.** `norm_cmd` keeps `cat`, `ls` and so on, and the docs'
  example card lists `cat (ran in 4/4 sessions, 33/34 succeeded)` (docs/training.md:363). Filter
  read-only look-around programs; `c629db3` already has that list for directives.
- **N4. The refusal message's advice can be wrong.**
  - It uses the relative `.venv/bin/python`, which is wrong when the cwd is a subdirectory.
  - uv-made venvs have no `pip`, so the first suggestion fails.
  - Lead with `uv pip install --python <abs>` when `uv.lock` exists.
- **N5. `pytest` → `python -m pytest` is not a pure interpreter swap.** `-m` puts the cwd on
  `sys.path`, which can change import results. "Nothing else in the command changed" is not
  strictly true.
- **N6. Prompt embeddings are not "no text".**
  - Card `prompts` are per-prompt quantised embeddings from a static (bag-of-words mean) model,
    roughly invertible to the prompt's words.
  - Local only, but "no text" (`card.rs:11`) overstates it.
  - Intent-route centroids **do** travel in packs (`convert.rs:192`), so they leak a family's
    prompt vocabulary when shared. Mention it in `adage share`'s summary.
- **N7. `PATH` can widen what the replay sandbox reads.** A `PATH` entry of `.` or empty
  canonicalises to the cwd, which is granted read in replay (`sandbox/mod.rs:370-376`). Drop
  relative `PATH` entries.
- **N8. The PreToolUse cheap path matches too much.** `command.contains("pip")` also matches
  `pipeline` and `pipe` (`lib.rs:264`), and each match loads the full index (S8). Match words, not
  substrings.

## What held up

- **PreToolUse never fires in the control arm.** The spool records the kind of rewrite and the
  interpreter, never the command (verified in the integration test).
- **PreToolUse is off in each of these cases:** `.adage/disabled`, `ADAGE_DISABLE=1`,
  `exec/rewrite-off`, non-Claude harnesses, and non-Bash tools.
- **Commands the planner leaves alone:** activated venvs, `uv run`, `poetry run`, `tox`, `$PY`,
  `ssh` and `docker`.
- **Cards never travel in packs.** Their exemplars are quoted (`    | `), and markup and
  control characters are rejected.
- **Card compilation is deterministic,** given the same sessions and model: ordering is by
  (day, id), everything is BTree-based, and ids are hashes.
- **Model fetch and extraction are sound** (S13).
- **Replay sandboxing is sound** (S14).
- **Every benchmark figure matches its source file.** The problems are framing (S12).
