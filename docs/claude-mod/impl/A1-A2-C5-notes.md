# A1 · A2 · C5 (binary side) · C1/C2 hardening: implementation notes

What was built for chunks A1, A2 and the binary half of C5 of
[the implementation plan](../implementation-plan.md), and the open issues of the proof of concept's
C1 and C2 that fall in the binary ([poc/P1-notes.md](../poc/P1-notes.md),
[poc/P2-notes.md](../poc/P2-notes.md)). The mod's side of C5 (`prompt.compose`, `tool.describe`,
`session.compact`, routing a step) is a later chunk's; the payloads and answers it consumes are
specified exactly below.

## A1 · One decision per Bash call, and the mod's two phases

### What was built

- **The identity.** `Claude::event_identity("PreToolUse", p)` is the call's `tool_use_id`, through
  the same `tool_event_identity` as the batch events (empty, missing or over 256 bytes: no
  identity). `adage_hook::phase::identity_hash` keeps it as the first eight bytes of its SHA-256.
- **The ring.** `SessionState.seen_pretool: Vec<u64>` (`#[serde(default, skip_serializing_if =
  "Vec::is_empty")]`), at most `SEEN_PRETOOL` = 64, oldest dropped. `SessionState::see_pretool(id)`
  marks a call decided and returns `false` when it already was; `has_seen_pretool(id)` reads it.
  The ring is not rebuilt from the spool (a rebuilt state starts empty: it can only decide again).
- **The command hook's path** (`pretool()`): the capability gate (`pre_tool_rewrite`) and the
  no-Python word scan come first and read nothing; then one state read: a call decided already is
  answered with nothing and no record. Every path that decides marks the call decided in the same
  state update that assigns the arm (`claim`): the rewrite, the advice, the control arm's withheld
  record, the receipt-only record. A payload with no usable id is decided as before.
- **The mod's path** (`pretool_mod`, for a payload with `dry_run: true` or `verdict`): a call decided
  already gets nothing, whatever the phase. Payload parsing is `adage_hook::phase::Phase::of`
  (public, for C3): `Phase::DryRun` or `Phase::Verdict(Verdict { engine, answered, substitute })`.
- **`checked_by`** on every decided `pretool` record: `"planner"` (the planner's own reading of the
  settings files, `pretool::keeps_permissions`) or `"engine"` (the mod's `verdict.engine`).

### The contract (the mod builds on this)

`<bin> hook claude PreToolUse`, the payload on stdin, exit 0 always.

stdin, beyond the command hook's fields (`session_id`, `cwd`, `tool_name: "Bash"`,
`tool_input.command`, `tool_use_id`, `permission_mode`):

| Field | Meaning |
| --- | --- |
| `dry_run: true` | Answer the plan; record nothing; change no state. Wins over `verdict` in the same payload. |
| `verdict: { engine?: "same" \| "differs", answered?: "receipt", substitute?: "<route>" }` | Decide once, record once. Unknown values are left unset. A `verdict` that is not an object is a verdict naming nothing. |
| `observe: true` | Observe mode: a substitution is recorded as the control arm records it. |

**Dry run** answers nothing, or one line:

```json
{"plan":{"substitute":{"command":"make test","context":"…","route":"dead-end-d41b1e","track":"acme/learned"},
         "rewrite":{"command":".venv/bin/python -m pytest tests -q","kind":"test"},
         "advice":{"kind":"pip-install","text":"…"},
         "receipt":{"note":"…"}}}
```

Each key only when it applies: `substitute` for any command (a learned dead end at full strength
with a fixed `instead`, rewrites on); `rewrite` or `advice` (never both) only for a command naming
a Python program with U11 in service and an environment bound for this checkout (the rewrite is
**not** checked against the person's permission rules: the engine checks it); `receipt` only in a
treated session with a covering receipt (adage-exec). The dry run answers in either arm.

**Verdict** answers:

| Verdict | stdout | Spool record (`kind: "pretool"`, `duration_us`) |
| --- | --- | --- |
| `{ engine: "same", substitute: R }`, R still the plan's | treated: `{"substitute":{…}}`; control or observe: nothing | `checked_by: "engine"`, `decision` judged `substituted`, or `would_substitute` + `withheld` |
| `substitute` with any other engine answer, or a moved plan | nothing | none |
| `{ engine: "same" }` (no `substitute`), a rewrite planned | the command hook's own rewrite: `{"systemMessage":"adage: pointed this test run at …","hookSpecificOutput":{"hookEventName":"PreToolUse","updatedInput":{…,"command":"…"},"additionalContext":"…"}}` | `pretool: {rewrite, interpreter}`, `checked_by: "engine"` |
| `{ engine: "differs" }`, a rewrite planned | advice: `{"hookSpecificOutput":{"hookEventName":"PreToolUse","additionalContext":"Adage: this test run uses the system Python … (Adage did not rewrite the command: …)"}}` | `pretool: {advise: "permissions", …}`, `checked_by: "engine"` |
| no `engine` | as the command hook decides (the planner's reading) | `checked_by: "planner"` |
| control arm, any of the three | nothing | `withheld: true` |
| no plan but a receipt | `{"hookSpecificOutput":{…,"additionalContext":"<receipt>"}}` | `receipt: true` |

After a verdict decided a call, the command hook that follows for the same `tool_use_id` prints
nothing and records nothing. **So on `differs` the mod must carry the advice's
`additionalContext` itself** (on the result's `context`): the command hook will not repeat it.
When the dry run plans nothing, the mod calls `next(e)` and the command hook decides as it always
has. `answered: "receipt"` is parsed (`Verdict.answered`) and reserved for C3: until C3 adds
`fresh` and `result` to the receipt (`PreNote`), no plan reports a fresh receipt, and a verdict
carrying `answered` decides as one without it.

### Tests

| Case | Test |
| --- | --- |
| T-A1-01 | `harness::identity_tests::a_pretool_call_is_identified_by_its_tool_use_id` (adage-hook) |
| T-A1-02 | `a_bash_call_is_decided_once_per_tool_use_id` (`crates/adage-cli/tests/hooks.rs`, through both binaries: the U11 environment bound in the hooks `World`, the rewrite then nothing, one record) |
| T-A1-03 | `the_pretool_ring_is_bounded_and_exact` (adage-runtime), `the_identity_ring_is_bounded_and_exact` (adage-hook: 64 ids of 256 bytes, the 65th, the state size under `SLOT − 64`) |
| T-A1-04 | the no-id halves of the two tests above |
| T-A1-05 | `the_dry_run_plans_and_the_verdict_decides_once`, `the_verdict_in_the_control_arm_is_withheld_and_decided_once`, `a_receipt_alone_is_decided_once_and_planned_for_the_mod`, `a_substitution_is_decided_once_per_call` (adage-hook); `phase::tests::*` |
| T-A1-06 | `claude_code_golden`: the new `PreToolUse` step prints `(nothing)`; parity holds |
| T-A1-07 | L2 half: `a_call_naming_no_python_program_touches_nothing` (no state file, no spool). L5 (the paired release run): **not run**, see below. `hook_latency` gained `pretool_no_match` |

The golden diff, re-blessed with `ADAGE_BLESS=1 cargo test -p adage-cli --test hooks
claude_code_golden`, is only added lines:

```diff
@@ -7,6 +7,9 @@
 ### PostToolBatch
 {"hookSpecificOutput":{…"route=\"read-linked\"…}}
 
+### PreToolUse
+(nothing)
+
 ### PostToolUseFailure
```

## A2 · `duration_us` and the doctor's p95

- **Every spool record a hook writes carries `duration_us`**, the hook's own wall time from
  `started` (`Instant` at `run()`'s start, a lower bound: the spawn is not in it), as an integer:
  the decision record (every event, `turn_step` included), both `pretool` records and the mod's,
  `stop_gate`, `session_start`, and the no-routes `seen` record. `started` reaches `respond` (as
  `Here`), `pretool`, `session_start` (`Start.started`) and `record_seen`. stdout is unchanged, so
  no golden moved. `rebuild` and `ui/feed.rs` read records as `Value` and ignore the field.
- **`adage doctor`'s `hook latency` check** (`crates/adage-cli/src/doctor_latency.rs`, a submodule
  of `doctor.rs`): nearest-rank p95 of `duration_us` over `batch` records (`rebuild::kind_of`)
  whose `decision.would_have` is empty, from the 200 newest spools, newest records first, at most
  2,000. Pass at or under 10,000 µs; warn above; fail above 25,000 µs; pass with the detail
  `too few hooks to measure` under 20 records. Detail: `p95 12.0 ms over the last 25 tool batches
  with nothing to say` (plus `(above 10.0 ms)` when it warns or fails). Cached for 5 s in
  `$ADAGE_HOME/cache/hook-latency.json` (`{at, hooks, p95_us}`), written only once
  `$ADAGE_HOME/sessions` exists, so a fresh install's doctor still creates nothing.
- **`doctor --json`'s structured `action`** beside `fix`, on every check that has a fix:
  `{"kind": "run", "command": "adage condition"}` when the fix is one `adage` command (the whole
  fix, or a fix that begins with one in backticks, after an optional `run `, that does not send
  the person to a terminal), else `{"kind": "manual"}`. `Check` keeps its fields (no literal
  changes for other crates); its `Serialize` is written out to add the derived `action`.
- **The kill-switch check's fix names `adage on`** (A6's verb) for a repository turned off with
  `.adage/disabled`: exactly `adage on` (so `action: {kind: run, command: "adage on"}`) when that
  is the only switch on, "unset the variables, and run `adage on` here, when you want Adage on"
  beside `ADAGE_*` variables, and "unset them when you want Adage on" without the marker. Test:
  `doctor::tests::a_repository_turned_off_is_fixed_with_adage_on`.

Tests: T-A2-01 `every_record_carries_the_hooks_own_time` (adage-hook, plus the `duration_us`
assertions in the A1 and C5 tests); T-A2-02 `doctor::latency::tests::*` (p95 over 500 records =
47.5 ms by hand: rank ⌈0.95 × 500⌉ = 475; exclusions; the under-20 rule; thresholds; the cache);
T-A2-03 `doctor_reports_the_hook_latency_p95` (hooks.rs, adage only: seeded spools, `warn` at
12 ms, `action: manual`, the cache file's mtime unchanged on a second call within 5 s while more
records are seeded, `fail` at 30 ms once the cache is gone) and `doctor::tests::a_fix_that_is_one_adage_command_is_an_action`.
T-A2-04 (L5): not run.

Not built: §2.5.4's "a line for the harness version last seen". The version arrives with A4's
harness ledger line (`ledger/.harness.jsonl`, another chunk's), and nothing in the binary records
it yet; the check reads nothing it could report.

## C5 · Standing text, compaction, routing: the binary's side

### `Standing`

`<bin> hook claude Standing`, stdin `{ "session_id", "cwd", "hook_event_name": "Standing",
"observe"? }`, exit 0 always. stdout: nothing outside a repository with reflexes (and on any
other harness), else one line:

```json
{"standing":"<adage-reflex …>…</adage-reflex>\n\n<adage-reflex …>…</adage-reflex>","environment":"In this repository, Python runs in the project's environment, .venv/bin/python: the system Python lacks the project and its test dependencies. Tests run with .venv/bin/python -m pytest.","compact":"Keep the repository procedures this session followed: how tests run here, and what was run after which edits."}
```

- `standing` (`adage_runtime::standing`): the repository's session-start reflexes (the routes a
  first prompt announces: learned dead ends, instincts' session-start advice) in index order, each
  in the directive wrapper and template it is delivered with, then one line per procedure card
  (`render::card_line`: "In this repository, changes that touch src/a.py, tests/test_*.py follow a
  known procedure, learned from 4 earlier sessions. It is given when such a task starts."), whole
  items only, at most `render::MAX_STANDING` = 2,000 bytes, joined by a blank line. Empty when
  there is none.
- `environment` (`render::environment_note`): when U11 is in service here and its binding
  verified an environment for this checkout (its interpreter present); else `""`.
- `compact` (`render::COMPACT_INSTRUCTION`): what `session.compact` appends to the instructions,
  said in both arms (it names no reflex). The mod reads it from this answer, so the words stay in
  the binary.
- **Once per session.** The first answer in a treated session is kept on
  `SessionState.standing: Option<Standing { standing, environment, routes }>` (this is the
  plan's `has_standing`); every later `Standing` answers it verbatim, whatever the index does
  meanwhile. `routes` are the reflexes the text carries: the first prompt does not announce them
  again (`decide`'s prompt arm), and they count as told for the interlock (a dead end in the
  standing text still holds back what it avoids, `SessionState::told`). Cards are not suppressed:
  the card itself is still given at the prompt of such a task.
- **Read-only for the spool, the learner and the notices**: no spool record, no `record_start`,
  no learner request, no notice taken. It assigns the session's arm if it has none (as
  `PreToolUse` does), which is a state write.
- **Control arm and observe mode**: `standing` and `environment` are `""`, `compact` is said;
  nothing is kept (observe mode writes no state at all).

The mod: `prompt.compose` appends `{ id: "adage:procedures", text: standing, scope: "session" }`
when `standing` is non-empty; `tool.describe { tool: 'Bash' }` appends `environment` when
non-empty; `session.compact` appends `compact` to `instructions`. Ask once after
`session.start`, keep the answer in `$.state`; a failure is an empty answer.

### Routing a step

The mod adds `"routine_model": "<model>"` (a non-empty string, or `true`) to its `TurnStep`
payload when the person set `runtime.routine_model`; absent, empty or `false`, nothing is routed.
The binary never sees which model.

- `Policy.routine` carries it to `decide()`. `route_step` runs at a main-loop step with
  `index > 0` whose prediction did not count (no `predicted_at`): the first pending directive in
  delivery order whose route is in play, delivered at `fire`, not exec-only, holding no `predict`
  grant, and not avoided by a dead end the agent was told. `Decision.would_route: Option<Routing {
  route, track }>` is recorded in both arms; `Decision.routine` only in the treated arm outside
  observe mode.
- stdout: `{"routine":true}` (a step is predicted or routed, never both). Codex and OpenCode
  render `Output::Routine` as nothing.
- A step with a routine model set is decided even without any `predict` grant (the early return
  before the state lock now needs both "no grant" and "no routine model"); a step that would
  neither predict nor route still leaves no record. A routed step is spooled as `turn_step` with
  `decision.routine` / `decision.would_route`. `rebuild` ignores routing.

The mod: on `routine: true` and a non-empty `routine_model`, `next({ ...e, model:
routine_model, effort: 'low' })`.

### Tests

T-C5-01: `standing_text_carries_the_session_start_reflexes_and_the_cards_within_its_bound`,
`a_reflex_in_the_standing_text_is_not_announced_again` (adage-runtime);
`the_standing_text_is_stable_bounded_and_read_only`, `the_standing_answer_names_the_bound_environment`
(adage-hook); `standing_is_answered_once_per_session_and_spools_nothing` (hooks.rs, both binaries).
T-C5-04 (binary): `routing_is_consented_and_never_in_control` (adage-runtime),
`a_routine_step_is_routed_only_when_the_person_set_a_model` (adage-hook). T-C5-02, 03, 05 are the
mod's (L7, L9).

## C1 / C2 hardening

- **`Extension::predict_grants(&self, repo_key, root, index, paths)`** takes the checkout the
  pipeline resolved the step's repository from. **adage-exec (`crates/adage-exec/src/hook.rs`,
  another agent's file): the impl now calls `predict_grants(repo_key, root, index, paths)`
  directly; `predict_root`, the `#[cfg(test)]` `TEST_CWD` thread-local and `asked_from` are
  removed; the two tests that used them pass `&f.root` (the "asked from elsewhere" assertion went
  with the working-directory rule).** `docs/execution.md` "When it predicts" says so. The
  in-process test's `Predicts` extension asserts the root it is given.
- **Ingest sets `Call.by`** from the transcript's `tool_use_id` prefix `adage_`
  (`By::of_tool_use_id`); the id itself is not kept, so an agent's step serialises as before.
  `SESSION_FORMAT` is 5. Test: `claude::tests::calls_with_adages_ids_are_adages`. (Excluding such
  steps from support is the learner's, plan §2.1 item 3.)
- **`Capabilities`** (adage-trace) gains `predict_step`, `answer_call`, `substitute_call`,
  `tool_description`, `cached_section`, `subagent_prompt`, `compaction` (`#[serde(default)]`), and
  derives `Serialize`, `Deserialize`, `Default`; `Capabilities::NONE`; every harness keeps today's
  set and reports the new ones unset. C9 makes the live adapter report them for `via: mod`. Test:
  `the_mods_capabilities_are_unset_for_every_harness_and_round_trip` (T-C9-01's type half).
- **`hook_latency`** gained `pretool_no_match` and `turn_step_predicts`: when every binary given
  is `adage-exec`, it writes a `predict` grant on `make gen` (`plan_predict` → `ApprovalKey`, the
  approval built through serde so new `Approval` fields do not break it), `policy.toml` at E2, and
  before each spawn (untimed) a session state with the pending `make gen` directive; it asserts the
  answer predicts `make gen`.
- **Docs.** `docs/how-it-works.md`: the predicted call as a fifth behavior row ("Five distinct
  behaviors"), a paragraph on what the mod adds, two source-map rows; `CHANGELOG.md`: two Runtime
  bullets (predicted steps and routing; one decision per Bash call, the two phases and
  `Standing`) and one Seeing it bullet (hook latency, `action`).

## Departures from the plan

1. `has_standing` is `SessionState.standing: Option<Standing>`: the text itself is kept, so the
   answer is identical for the session even after a learn changes the index.
2. "The applicable procedure cards' titles and the instincts' one-liners": cards have no title,
   so each card is one fixed-template line naming its files; the "one-liners" are the
   session-start reflexes (instincts' and learned dead ends'), the exact text the first prompt
   carries, which is also what the suppression must match.
3. The compaction instruction is a field of the `Standing` answer, not an event of its own: it is
   said in both arms and costs no spawn at compaction.
4. The mod's dry run and verdict are taken before the no-Python word scan (as P5 built), because a
   learned substitution applies to any command; a command hook's payload keeps the scan first and
   costs no I/O when it names no Python program.
5. A dry run for a call already decided answers nothing (the decision was made).
6. Routing considers pending directives only (a run, a sequence's first step, a skill), not
   bound read batches or armed obligations.
7. `answered: "receipt"` is parsed but not acted on (C3).

## Not verified here

- **L5 latency (T-A1-07, T-A2-04, T-C1-12's pairs)**: the paired release runs were not made (this
  brief allows no `--release` builds and no second worktree for a base build). One unpaired debug
  run of `target/debug/examples/hook_latency target/debug/adage-exec - 30` (shared machine, other
  builds running): `batch_no_match` p95 7.96 ms, `turn_step_nothing_pending` p95 11.9 ms,
  `pretool_no_match` p95 8.25 ms, `turn_step_predicts` p50 40.1 ms / p95 55.1 ms (debug build:
  not comparable with the 10 ms release bound).
- Nothing here ran in a live Claude Code session (T-C5-05, the mod's halves).

## Files touched

```
crates/adage-trace/src/harness.rs
crates/adage-ingest/src/claude.rs, cache.rs
crates/adage-runtime/src/lib.rs, render.rs, tests.rs
crates/adage-hook/Cargo.toml (sha2), src/lib.rs, src/phase.rs (new), src/harness/{mod,claude,codex,opencode}.rs
crates/adage-exec/src/hook.rs          (predict_grants impl and its two tests; see above)
crates/adage-cli/src/doctor.rs, src/doctor_latency.rs (new)
crates/adage-cli/tests/hooks.rs, tests/golden/claude-code.txt
crates/adage-cli/examples/hook_latency.rs
docs/how-it-works.md, docs/execution.md (one sentence), CHANGELOG.md
docs/claude-mod/impl/A1-A2-C5-notes.md
```

## Commands run (in /home/user/adage-poc)

| Command | Result |
| --- | --- |
| `rustfmt` on every file this work touched, then `cargo fmt --all -- --check` | only `crates/adage-learn/src/lib.rs` differs (another agent's file, not touched) |
| `cargo clippy -p adage-trace -p adage-ingest -p adage-runtime -p adage-hook --all-targets -- -D warnings` | clean |
| `cargo clippy -p adage-cli -p adage-exec --all-targets -- -D warnings` | clean (after waiting out other agents' in-progress compile errors in adage-app) |
| `cargo test -p adage-trace -p adage-ingest -p adage-runtime -p adage-hook` | all pass: hook 43, ingest 24, runtime 103 + 1, trace 3 |
| `cargo test -p adage-exec` | all pass: lib 108, parity 22 (the new hooks tests run through `adage-exec hook` too), cli 7, the rest unchanged |
| `ADAGE_BLESS=1 cargo test -p adage-cli --test hooks claude_code_golden` | the golden diff above (added lines only) |
| `cargo test -p adage-cli --test hooks` | 22 passed |
| `cargo test -p adage-cli --no-fail-fast` | everything this work touches passes. Failures seen, none in these files: `ui::tests::settings_show_rewrites_off_by_the_older_marker_and_notices` (an insta snapshot gaining `skills.invoke`, the skills agent's key), `live_learn::learn_auto_logs_what_changed` (a `change: "candidate"` line, the candidates agent's), `press::a_press_is_refused_everywhere_else` (the press agent's); four hooks.rs failures in that run were the disk filling mid-run (ENOSPC) and pass on rerun |
| `cargo test -p xtask` | 18 passed |
| after the last two fixes (the `pretool` record's `interpreter` as the person reads it, which the refactor had made absolute and `zero_config_plugin::pretool_rewrites_to_the_verified_environment` caught; the `adage on` fix): `cargo test -p adage-hook`, `cargo test -p adage-cli --test hooks --test zero_config_plugin`, `cargo test -p adage-cli --bin adage -- doctor`, `cargo clippy -p adage-hook -p adage-cli --all-targets -- -D warnings` | all pass, clippy clean; `ui::tests::settings_show_rewrites_off_by_the_older_marker_and_notices` still fails on this tree: its snapshot lacks the skills agent's new `skills.invoke  false  (default)` row (needs a re-blessed snapshot by that agent; not this work) |
| `cargo build -p adage-cli --example hook_latency`; `target/debug/examples/hook_latency target/debug/adage-exec - 30` | runs every scenario; `turn_step_predicts` predicts `make gen` (numbers under "Not verified here") |

The disk filled several times during the work (other agents' builds share `target/`); I removed
`target/debug/incremental`, `target/debug/examples` and test executables in `target/debug/deps`
older than 20 minutes that had a newer build of the same name, nothing else.
