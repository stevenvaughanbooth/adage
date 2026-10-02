# Adage in Claude Code: the test plan

> Status: **proposed 2026-10-02**. How the work in [implementation-plan.md](implementation-plan.md)
> is proven, chunk by chunk and as a whole: the levels, the environments, the fixtures, every
> test case with its pass criterion, the evaluation protocol that decides the "fewer model
> requests" claim, and the gates a release must clear. Written against the test infrastructure
> as it stands (`crates/adage-cli/tests/`, `crates/adage-exec/tests/`, `crates/adage-sim`,
> `crates/adage-eval`, `demo/`, `evals/reflex-proof/`, the `plugin` CI job). Case ids are stable
> and every chunk in the plan cites its ids; a pull request names the cases it adds or changes.
> Revised once after review: the first draft named case families it never defined, asserted
> receipts and candidates the fixtures could not produce, and quoted a development rule the named
> harness no longer uses; this version does not.

## 1. Objectives and quality bars

1. **The floor never regresses.** `adage hook claude <event>` prints the same bytes for every
   event and payload it printed before; `adage-exec hook` matches it with no grants. Any
   difference is a reviewed golden change. A field the mod needs is emitted only for the mod's
   payloads.
2. **Fail open is total.** No path in the binary or the mod can block, delay past budget, or
   alter a tool call, a prompt or a model request on an error. Fault injection proves it, per
   event.
3. **The control arm is silent and never named**, in the binary (no output), on the screen, in
   the pane and in the feed for the current session.
4. **Authority is never widened.** No test may pass in which the mod approves a call, denies a
   call, strips an environment variable, runs a person-only command, answers a receipt for a call
   the person's rules would have asked about, or predicts without a grant.
5. **Budgets hold, measured in pairs.** Every latency bound is a paired `hook_latency
   <before>,<after>` run (the tool's interleaved two-binary mode), 500 runs after its 20 warm-ups,
   repeated five times, with the machine named; the bound is on the p95 difference.
6. **The claim is measured, not assumed.** Fewer model requests at preserved hidden-test quality,
   against a no-hooks baseline, first under the registered development rule (v9) and then under
   the registered protocol on new groups.
7. **A person can read it and use it.** The surfaces pass the usability check in ux-design §14.

## 2. Test levels and environments

| Level | Tooling | Isolation | Where it runs |
| --- | --- | --- | --- |
| L1 Unit, Rust | `cargo test -p <crate>` | in-process, `tempfile` | every PR |
| L2 Integration, binary | `crates/adage-cli/tests/*` and `crates/adage-exec/tests/*`; each file defines its own `World` with a fresh `ADAGE_HOME`, `ADAGE_AUTO_LEARN=0`, `ADAGE_ARM=treated` (`hooks.rs:158`, `harness_sim.rs:63`, `zero_config_plugin.rs:89`) and `CLAUDECODE`, `OPENCODE`, `CODEX_SANDBOX*`, `ADAGE_DISABLE` removed; control cases set `ADAGE_ARM=control`; person-only cases remove `ADAGE_ARM` as well (it is in adage-exec's `AGENT_ENV`, `person.rs:11-12`) and set `ADAGE_ALLOW_NONINTERACTIVE=1`, as `adage-exec/tests/cli.rs:106-113` does; `NO_COLOR=1` only where a case reads a frame | per test tempdir | every PR |
| L3 Golden and parity | `ADAGE_BLESS=1` goldens in `tests/golden/`, `adage-exec/tests/parity.rs` (the same scenario against both binaries, `hooks.rs:7-32`) | as L2 | every PR |
| L4 Simulator | `crates/adage-sim` fixtures, `harness_sim.rs` (`replay_claude`, `check_graph`), `committed_fixtures_are_current` | as L2 | every PR; `cargo run -p adage-sim` on fixture change |
| L5 Latency | `crates/adage-cli/examples/hook_latency.rs` in its paired mode (`<bin a>,<bin b>`, `:12, 124-160`), release builds of the base and the branch, 500 runs after the 20 warm-ups (the default is 300, `:181`), five repetitions; `learn_bench` (300 × 2, `learn_bench.rs:7`) | a quiet developer machine, named; numbers and the command in the PR | before merging A1, A2, A6, C1, D1, D2 (no workflow runs these today) |
| L6 Replay gates | `cargo test -p adage-eval --test gates` | recorded datasets | weekly `evals.yml`; before merging C1, D1, D2, D3 |
| L7 Mod unit | `claude plugin test plugin/claude-code` (`*.test.ts(x)`; `ui.mount` over `terminal` and `desktop` with a stubbed `$`; handlers called directly from the mounted tree) | the engine's test kit, no session | the `plugin` CI job, CLI pinned at 2.1.287 |
| L8 Mod static | `claude plugin validate --strict` diffed against the committed `VALIDATE.txt`; `scripts/check-plugin.mjs` | | the `plugin` CI job |
| L9 Live probes | S9's probe mod under `--plugin-dir`, an isolated `CLAUDE_CONFIG_DIR`, `claude --debug` | a developer's machine; `spikes/S9-claude-mods/FINDINGS.md`, `rerun.sh`, `results/` | once per question |
| L10 Headless end to end | `demo/run.py` arms `a0`, `control`, `adage`, `mod-observe`, `mod` on the acme-billing sandbox (`DEMO_MODEL` default `claude-sonnet-5`, `run.py:36`); `demo/replay_hooks.py` for the recorded-payload replay | fresh sandbox per run (`generate.py`), `ADAGE_HOME` under `DEMO_ROOT` | before merging B2, C3, C4; model spend |
| L11 Development gate | `evals/reflex-proof/harness/dev_v9.py` with a `MOD` arm under the registered `dev_thresholds_v9` rule | per the harness (16 blocks, `SESSION_CAP_USD` 0.50, `STUDY_CEILING_USD` 8.50) | before E3, twice |
| L12 Registered | `docs/launch-gate-protocol.md` on new groups | per the protocol | once |
| L13 Usability | ux-design §14: eight readers, the recordings, twenty matched pairs, five live tasks | | once per surface change |

## 3. Fixtures and data

| Fixture | Holds | Used by |
| --- | --- | --- |
| `sim/fixtures/` (12 Claude, 12 OpenCode, 12 Codex seeded sessions; held-out W-2045) | The acme-billing scenario: lookup, dead ends (`DEAD_ENDS`, `scenario.rs:252`), the finish line (`make gen` after every schema edit, `:320-327`); after D1, the 2-of-12 `make test → make lint` sequence | L4 |
| New sim payloads: `PreToolUse` per Bash call with `tool_use_id`; `TurnStep` per turn after the batch; `Standing`; `SkillPrompt` | `crates/adage-sim/src/claude.rs` | L4 |
| `demo/sandbox/` (`generate.py`, `score.py`, `hidden/`; the checkout is generated per run, `run.py:130`) | L10's repository; `score.py` keys `passed, ci_passed, hidden_passed, hidden_tests_run, stale_generated, ci_tail, hidden_tail`; no pytest, `make test` runs unittest | L10 |
| `evals/sandboxes/acme-billing` (copied by `zero_config_plugin.rs::World::repo`, `:68`) plus `.venv/pyvenv.cfg` and a real `SessionStart` to bind U11 (`:66-71, 515-523`, debug builds, `ADAGE_TEST_NO_SPAWN`) | The Python fixture where a rewrite and a receipt can happen | L2 (A1, C3) |
| `evals/datasets/*/recorded/index-*.json` (26) | Indexes from before this work; must keep parsing | L6 |
| `evals/lab/skills/skills/acme-work-item/SKILL.md`, `click-bugfix/SKILL.md` | Skills for `adage train` and `SkillPrompt` | L1 (train), L2, L9 |
| Settings fixtures for rewrites (new, `crates/adage-cli/tests/fixtures/settings/`): `allow-pytest.json` (`Bash(pytest:*)`), `allow-venv.json`, `dont-ask.json`, `none.json`, and a `--allowedTools` case | S9 Q4 and C3 | L2, L7, L9 |
| Synthetic spools and ledgers (`zero_config_plugin.rs::population()`, `:699-701`), extended with a `source: harness` line whose steps include `predicted: true` | Savings gate inputs | L2 |
| Synthetic transcripts with Skill tool uses (`adage-learn/tests/golden.rs` `Tx::call("Skill", …)`) | D3 learner tests | L1 |
| Mod fixtures (`plugin/claude-code/hooks/fixtures/`): classic hook payloads and results, feed lines (with and without arm-naming records), `graph --json`, `doctor --json`, `adage --json`, `verified.json`, `$.state` snapshots per scenario (§6) | L7 | L7 |
| Grant fixtures (new, for `hook_latency` and `adage-exec/tests`): `exec/approvals.json` with a `mode: Predict` key, `policy.toml` at `max_level e2`, a state file with one pending directive | C1, C2 | L2, L5 |
| Fault injection: `ADAGE_TEST_FAULT=panic` (debug builds, fires once in `run()` before dispatch, `lib.rs:321-327`) and the new event-scoped faults `ADAGE_TEST_FAULT=panic:turn_step`, `panic:skill_prompt`, `panic:standing`, `panic:predict_grants`, `panic:receipt`; a `quiet` binary; a binary that sleeps 12 s; a binary that prints `{`; an unreadable `verified.json`; an unparseable `approvals.json` | Fail-open cases | L2, L7 |

## 4. Traceability

Every chunk's "Tests" line in the plan cites these ids; every id here belongs to one chunk or to
the cross-cutting set.

| Feature (architecture §) | Plan chunk | Cases |
| --- | --- | --- |
| Dedup per `tool_use_id`; `dry_run` / `verdict` (§2 rule 2, §3.3) | A1 | T-A1-01 … 07 |
| Latency on record (§3.9) | A2 | T-A2-01 … 04 |
| The feed (§2, §4.1) | A3 | T-A3-01 … 06 |
| Harness-priced ledger (§3.9) | A4 | T-A4-01 … 05 |
| Tokens and card JSON (§4 rules) | A5 | T-A5-01, 02 |
| `adage off`/`on`, `adage-exec version` (§5, §2) | A6 | T-A6-01 … 03 |
| Module, bridge, activation, CI (§2) | B1 | T-B1-01 … 06 |
| Observe and draw (§3.1, §4) | B2 | T-B2-01 … 11 |
| Pane and commands (§4.1, §5) | B3 | T-B3-01 … 10 |
| `TurnStep` (§3.2) | C1 | T-C1-01 … 13 |
| `predict` grant (§3.2) | C2 | T-C2-01 … 09 |
| Receipt as result, `$.tool.check` (§3.3, §3.4) | C3 | T-C3-01 … 07 |
| The mod's `turn.step`, sim (§3.2) | C4 | T-C4-01 … 06 |
| Standing, compaction, routing (§3.5–3.7) | C5 | T-C5-01 … 05 |
| Candidates (§3.10) | D1 | T-D1-01 … 10 |
| `noticed` (§3.10) | D2 | T-D2-01 … 07 |
| `skill` kind (§3.11) | D3 | T-D3-01 … 08 |
| `verified.json`, `SkillPrompt` (§3.11) | D4 | T-D4-01 … 05 |
| Drawing noticed, learning, skills (§4) | D5 | T-D5-01 … 03 |
| Arms and the requests metric (§9) | E1 | T-E1-01 … 04 |
| Development gate (§9) | E2 | T-E2-01 … 03 |
| Registered run (§9) | E3 | T-E3-01, 02 |
| Cross-cutting: fail open, control, authority, hosts, drift, both plugins | all | T-X-01 … 12 |
| Usability (ux-design §14) | B2, B3, D5 | T-U-01 |

## 5. Test cases: the binary

Format: id · level · title. **Given / When / Then**, then the pass criterion where it is not
the Then itself.

### A1 · `PreToolUse` identity and the two-phase payload

- **T-A1-01 · L1 · identity for PreToolUse.** In `harness/mod.rs::identity_tests` (`:175`):
  `Claude::event_identity("PreToolUse", {tool_use_id: "t1"})` is `Some`; without `tool_use_id`,
  `None`; `tool_event_identity` still refuses empty, duplicate and oversized ids.
- **T-A1-02 · L2 · one decision per id.** In `zero_config_plugin.rs`'s Python fixture with U11
  bound: the same `PreToolUse` payload (`python -m pytest tests -q`, `tool_use_id: t1`) piped
  twice prints the rewrite then nothing, and the session spool holds exactly one `pretool`
  record.
- **T-A1-03 · L2 · the ring is bounded and exact.** 64 distinct 256-byte ids (the maximum,
  `harness/mod.rs:93`) each decide; id 1 re-sent is deduplicated; a 65th distinct id decides and
  then id 1 decides again (the oldest was dropped); the state payload stays under
  `SLOT − HEADER` (`adage-store/src/state.rs:10-11`).
- **T-A1-04 · L2 · no id, no dedup.** Two payloads without `tool_use_id` both decide, as today.
- **T-A1-05 · L2 · dry run records nothing.** A `dry_run: true` payload prints the plan
  (`rewrite`, `advice`, `receipt` as applicable) and leaves the spool and the ring untouched; the
  same payload with `verdict: { engine: "same" }` then decides and records once with
  `checked_by: "engine"`; with `verdict: { engine: "differs" }` the record shows advice and no
  `updatedInput`.
- **T-A1-06 · L3 · golden unchanged.** `claude_scenario` gains a `PreToolUse` step that prints
  `(nothing)` (its `World` binds no environment, `hooks.rs:34-141`); the golden diff shows only
  that step; parity passes.
- **T-A1-07 · L5 · cost.** Paired `hook_latency <base>,<branch> - 500`, five repetitions: the
  `pretool_no_match` p95 difference is within 0.3 ms every time, and the no-match path makes no
  state write (`strace -c` or the state file's mtime unchanged in L2).

### A2 · `duration_us` and the doctor check

- **T-A2-01 · L1 · every record carries it.** The decision record, both `pretool` records
  (`lib.rs:591`, `:712`), `stop_gate` (`:403`) and `record_start` (`:936`) carry `duration_us`
  as an integer; `rebuild` ignores it.
- **T-A2-02 · L1 · the percentile and the floor.** Nearest-rank p95 over 500 synthetic `batch`
  records with empty `would_have` matches a hand-computed value; records with directives are
  excluded; fewer than 20 qualifying records → `pass` with detail `too few hooks to measure`.
- **T-A2-03 · L2 · doctor reports it.** `adage doctor --json` on a seeded home lists `hook
  latency` with `status`, `detail` and the new `action` field; seeded p95 12 ms → `warn`, 30 ms →
  `fail`; a second call within 5 s does not re-read the spools (the cache; checked by mtime).
- **T-A2-04 · L5 · cost.** Paired run: the no-match p95 difference within 0.3 ms.

### A3 · `adage watch --follow --json`

- **T-A3-01 · L2 · streams.** Start `--follow --json --session S`; append one spool record and
  one `learn.log` line; two JSON lines arrive within 2 s with `kind` `decision` and `learn`, each
  with a non-empty `cursor`; EOF on stdin exits 0.
- **T-A3-02 · L2 · resumes across processes.** After five rows, kill the process, start a new
  one with `--since <cursor of row 3>`: rows 4 and 5 arrive first, then it follows.
- **T-A3-03 · L2 · health and learner records.** Touching `<slug>.spawn.json` with a future
  `after` emits `{"kind":"learner","state":"waiting","next_at":…}`; taking the lock emits
  `running`; a health change emits `{"kind":"health","lights":[{label,lamp,hint}]}` as
  `health::gather` builds them.
- **T-A3-04 · L2 · the current session never names the arm.** With `ADAGE_ARM=control`, a
  session S whose spool holds withheld directives, a `would_predict` and a `StopWithheld`: the
  stream for `--session S` carries no `control` field, no `would_*`, no `Withheld`, `Unaided`,
  `NotReached` or `StopWithheld` act; the same records for another session T are streamed as
  `adage watch` shows them.
- **T-A3-05 · L2 · never a terminal, exits with the parent.** No escape sequence in the output
  (regex `\x1b\[` absent); the `--once` frame and the UI snapshots are unchanged; killing the
  parent ends the child within 2 s (`#[cfg(unix)]`, `prctl`/poll on the parent pid).
- **T-A3-06 · L5 · steady.** 200 rows at 2 Hz for 60 s with one forced restart: no gap across
  the cursor; RSS sampled from `/proc/<pid>/status` flat within 10 MiB (`#[cfg(target_os =
  "linux")]`).

### A4 · `adage ledger note`

- **T-A4-01 · L1 · the merge.** A `harness.jsonl` line for session S is merged under the ledger
  lock; a later transcript-priced summary for S does not replace it, and a second harness line
  for S replaces the first; `cost.source == "harness"`.
- **T-A4-02 · L1 · steps and predictions.** Six `StepUsage` entries, two `predicted: true`:
  `model_turns` is four, `predicted_steps` two, tokens sum the four counts of the six.
- **T-A4-03 · L2 · the command.** `echo '<valid line>' | adage ledger note` exits 0 with
  `CLAUDECODE` set and the line appears in `harness.jsonl`; a malformed line exits 2 and writes
  nothing; a line for a session id with no `session_start` in the spool exits 2 and writes
  nothing; after the learner's merge `adage savings --json` shows S with `source: harness`.
- **T-A4-04 · L2 · the gates.** `savings_gate` and `pricing_honesty` (`zero_config_plugin.rs:717,
  760`; the latter debug-only) pass with `population()` extended by a harness line with predicted
  steps, and the estimate counts a predicted step as a turn not spent.
- **T-A4-05 · L2 · the ledger race.** Two concurrent merges (the learner and a second `adage
  savings` refresh) leave a well-formed ledger with one line per session.

### A5 · Tokens and card JSON

- **T-A5-01 · L1 · generated and current.** `the_committed_tokens_are_current` (`xtask/src/
  main.rs:414`) covers `tokens.ts`; it exports every glyph in the tokens' `glyph` group with its
  ASCII twin and every role with a theme key; editing `tokens.ts` by hand fails the test.
- **T-A5-02 · L2 · `adage --json`.** Parses; its rows equal the card's text rows one to one (an
  insta snapshot of both); with `--for-session` the `next` row names `/adage watch` and not
  `adage ui`.

### A6 · `adage off`, `adage on`, `adage-exec version`

- **T-A6-01 · L2 · off from anywhere.** `adage off` with `CLAUDECODE=1` exits 0, creates
  `<root>/.adage/disabled`, is idempotent, and every hook then prints nothing (the kill switch,
  `lib.rs:355`); `adage doctor` shows the off state.
- **T-A6-02 · L2 · on is a person's.** `adage on` with any `AGENT_ENV` variable set (all four of
  adage-cli's, `install.rs:47`) exits non-zero with the person-only message and leaves the file;
  at a terminal (`ADAGE_ALLOW_NONINTERACTIVE=1`, `ADAGE_ARM` removed) it removes the file; both
  exit 0 with one line outside a repository.
- **T-A6-03 · L2 + L5 · version.** `adage-exec version` prints one line and exits 0; `adage-exec
  hook` paired latency is unchanged (the `hook` dispatch stays first, `main.rs:428-431`).

### C1 · `TurnStep`

- **T-C1-01 · L1 · predicts a delivered directive.** Given a state with a `pending` for route R
  (`expect_key: exec make gen`, `command: "make gen"`) and `with_predict_grants({R})`. When
  `TurnStep { turn_id: t, index: 1 }` is decided. Then `Decision.predicted` is `{ route: R, tool:
  Bash, input.command: "make gen", kind: run }` and `last_step_predicted == (t, 1)`,
  `predictions == 1`. An armed obligation predicts with `kind: obligation`.
- **T-C1-02 · L1 · never two in a row, never at index 0.** `TurnStep { t, 0 }` predicts nothing
  with a pending present; the step after a prediction with no batch between predicts nothing;
  after a `Batch` that observes `exec make gen`, the pending is cleared and nothing is pending.
- **T-C1-03 · L1 · the cap.** With `runtime.predict_cap = 2`, the third eligible step predicts
  nothing and `would_predict` is set.
- **T-C1-04 · L1 · control.** In the control arm `predicted` is `None` and `would_predict` is
  set.
- **T-C1-05 · L1 · no grant, no command, inhibited: nothing.** With an empty grant set; with a
  pending whose `command` is empty (an old state); with an `avoid` interlock active for the
  route's command (`approach_lock`): nothing is predicted.
- **T-C1-06 · L1 · a skill step.** A pending `Step::Skill { skill: "release-checklist" }` on a
  granted route predicts `{ tool: Skill, input: { skill: "release-checklist" }, kind: skill }`.
- **T-C1-07 · L1 · bounds survive a rebuild.** `rebuild` over a spool with one `turn_step` record
  restores `last_step_predicted` and `predictions`; `rebuild::kind_of` knows `turn_step`.
- **T-C1-08 · L1 · fast path.** `fast::skip` returns true for `TurnStep` with no pending and no
  armed obligation, false otherwise.
- **T-C1-09 · L1 · `by: Adage`.** A `PostToolBatch` call with `tool_use_id` `adage_R_t_1` sets
  `Call.by = Adage`, and `decide` judges `Predicted { route }` not `Followed`; the judgement is
  not counted as a follow in `evidence.rs`, not as extinction evidence in `instincts.rs`, and is
  counted by `savings.rs` as a turn not spent.
- **T-C1-10 · L2 · the event end to end, both binaries.** `adage hook claude TurnStep` after a
  seeded pending prints nothing (no extension, so no grants); `adage-exec hook claude TurnStep`
  with the grant fixture prints `{"predict":…}` (in `crates/adage-exec/tests/`); on Codex and
  OpenCode adapters the same event prints nothing.
- **T-C1-11 · L3 · goldens.** `claude_scenario` gains one silent `TurnStep` step; parity passes
  (neither binary predicts without a grant).
- **T-C1-12 · L5 · latency.** Paired run: `turn_step_nothing_pending` p95 within 0.3 ms of
  `batch_no_match`; `turn_step_predicts` against `adage-exec` with the grant fixture (which
  `hook_latency` learns to seed) p95 ≤ 10 ms.
- **T-C1-13 · L6 · replay gate.** `fast_path_changes_no_decision` passes with a `TurnStep`
  injected after each `Batch` on every dataset it covers (demo-haiku45's three splits, synthetic,
  adversarial, `gates.rs:332-338`).

### C2 · The `predict` grant

- **T-C2-01 · L1 · distinct keys, both kept.** `ApprovalKey` for the same request with `mode:
  Predict` and `mode: Execute` differ; `diff` names `mode`; `Store::approve` of one does not
  delete the other; `revoke_automatically` and `lapsed` act on their own mode only.
- **T-C2-02 · L1 · old stores load.** An `approvals.json` written without `mode` loads with
  `Execute`; a round trip preserves it.
- **T-C2-03 · L1 · policy and effect.** `permits` refuses a predict request at `max_level e1`,
  for an effect above Regenerate, for an effect in `effects_deny`, for an environment action when
  the policy does not allow them, and for an `exec_only` route; allows it at `e2` otherwise;
  `exec/predict-off` present → `predict_grants` is empty.
- **T-C2-04 · L2 · person only.** `adage review delegate T R --predict` with each of adage-exec's
  six `AGENT_ENV` variables set in turn (`person.rs:11-12`) exits non-zero naming the variable and
  writes nothing; at a terminal (all six removed, `ADAGE_ALLOW_NONINTERACTIVE=1`) it writes the
  approval.
- **T-C2-05 · L2 · round trip.** `delegate status --json` lists the grant with `mode: predict`;
  `revoke` removes it; changing the route's command template makes `lapsed` list it and the
  notice's reapprove command ends in `--predict`.
- **T-C2-06 · L2 · exclusive flags and refusals.** `--predict --effect verify` and `--predict
  --level e2` are usage errors; `--predict` on a route that already holds an `Execute` grant for
  the step is refused with a message naming the double run.
- **T-C2-07 · L2 · both grants on one route, E2 still runs.** Seed an `Execute` grant and a
  `Predict` grant for R; the E2 action at `PostToolBatch` still runs (`hook.rs:931` finds the
  execute key), `covering_receipt` still finds its receipt (`:774`), and the journal shows no
  lapse notice.
- **T-C2-08 · L2 · the machine switch.** `adage delegate predict off` (any env) stops
  `adage-exec hook claude TurnStep` from predicting; `predict on` requires a person.
- **T-C2-09 · L3 · parity.** `adage-exec hook` with no grants matches `adage hook` byte for byte.

### C3 · Receipt as result, rewrites checked

- **T-C3-01 · L1 · fresh.** `covering_receipt` returns `fresh: true`, `result` and `covering`
  for an applied run on identical inputs covering the call; after one byte changes in a
  snapshotted file, `fresh: false` and no `result`; a call outside the root, `fresh: false`; a
  non-pytest call, `None`.
- **T-C3-02 · L2 · the floor is unchanged.** In the Python fixture with a journaled passing run,
  `adage hook claude PreToolUse` without `dry_run` prints exactly today's context (golden
  unchanged); with `dry_run: true` the plan carries `receipt: { fresh: true, result, covering }`.
- **T-C3-03 · L2 · answered as receipt, one record.** `verdict: { answered: "receipt" }` spools
  one `pretool` record with `answered: receipt`, marks the test obligation met (the Stop gate
  does not ask for the run), and the command hook's repeat of the id prints nothing.
- **T-C3-04 · L7 · the mod answers only on allow.** Stubbed `ask` dry run returning
  `receipt.fresh`: with `$.tool.check` → `allow`, the hook returns `{ result, context }` and never
  calls `next`; with `ask` or `deny`, it falls through to the rewrite path and `next` is called;
  the recorded argv shows two asks on the first path and one on the second.
- **T-C3-05 · L7 · rewrite with the engine's verdict.** Stubbed dry run returns a rewrite;
  stubbed `$.tool.check` answers the same decision for both commands → the final ask carries
  `engine: "same"` and the hook calls `next` with the new command and `$.ui.notice` once;
  different decisions → `engine: "differs"`, `next(e)` unchanged, no notice.
- **T-C3-06 · L9 · agreement.** Over the five settings fixtures, `$.tool.check`'s verdict on the
  original and rewritten commands agrees with `pretool::keeps_permissions` (`runtime/pretool.rs:587`)
  on every settings-file case, and answers the `--allowedTools` case the planner cannot see.
- **T-C3-07 · L7 + L2 · the beside line.** A receipt that is not fresh draws the dim
  `result.receipt.beside` line and `next(e)`, never the card.

### C5 · Standing, compaction, routing

- **T-C5-01 · L1 + L2 · `standing` is stable, bounded, read-only.** Two `Standing` events in one
  session answer identical text, ≤ 2,000 characters; a control session answers empty; the event
  does not write `record_start`, spawn the learner or consume a notice (the spool and
  `notice-shown.json` are unchanged); after it, the per-event standing text is suppressed.
- **T-C5-02 · L7 · the section.** `prompt.compose` appends `adage:procedures` with `scope:
  session` once; with empty `standing` it returns `next(e)`'s sections unchanged.
- **T-C5-03 · L7 · compaction.** Without a structural handle (`isAddressable` false),
  `session.compact` passes every message through and appends the instruction; with the handle,
  rows whose attachment origin is the plugin are removed and no row is removed by its text (a row
  quoting `<adage-reflex` survives).
- **T-C5-04 · L1 + L7 · routing is consented and never in control.** The binary answers `routine`
  only when the payload says `routine_model` is set and the arm is treated; in control it spools
  `would_route`; the mod calls `next({ ...e, model, effort })` only on `routine` with a non-empty
  `routine_model`.
- **T-C5-05 · L9 · byte-identical across requests.** `$.prompt.compose()` read at three points of
  a session returns the same `adage:procedures` text (Q2).

### D1 · Candidates

- **T-D1-01 · L1 · the routes' pass is untouched.** `mine()` over the sim's history and over
  every `adage-learn/tests/golden.rs` input produces the same patterns, routes and `truncated ==
  false` before and after D1 (the goldens pin it).
- **T-D1-02 · L1 · the candidate pass.** With `min_support 5, border 2`: a two-event `Context`
  pattern with support 3 and 4 yields a `Candidate` (support 3 is the lowest border value); with
  support 2 nothing; with support 5 a route and no candidate; a one-event after-edit pattern with
  support 4 yields nothing; a prompt-family pattern yields nothing.
- **T-D1-03 · L1 · stable id, never a route.** The candidate's id equals the route id the same
  pattern gets at support 5; no candidate id ever appears in `Index.routes`; one more supporting
  session makes it a route with the same id and `learn.log` says `change: new` with the tipping
  session ids.
- **T-D1-04 · L1 · nascent routes are candidates.** A nascent route with a `Context` trigger
  compiles into `candidates` with `review: true` and not into `routes`; a nascent route with an
  after-edit trigger is not a candidate.
- **T-D1-05 · L1 · index.** `Triggers::of` includes candidate words and globs; `candidate_contexts`
  is separate from `contexts` and has its own digest; `compile_root` reads `candidates.json` and
  `compile(tracks)` is unchanged; `index_is_deterministic` holds; an index JSON without the new
  fields loads with empty maps; `format` round-trips.
- **T-D1-06 · L1 · incremental equals full.** `cargo test -p adage-graph --test border` passes.
- **T-D1-07 · L2 · the sim's candidate.** `adage condition --demo` over `sim/fixtures` writes
  `candidates.json` for acme-billing with the `make test → make lint` candidate (support 2,
  `s_min` 3 under `Params::demo()`), and `adage status --json` lists it under `candidates`.
- **T-D1-08 · L5 · learn time and the fast path.** `learn_bench` (300 × 2) ≤ 1.3× the base;
  `condition --auto` with one changed transcript ≤ 150 ms; the fast-path skip rate over the sim
  and the dataset replays is reported before and after (a number, with the bound set by the
  reviewer of D1's PR).
- **T-D1-09 · L6 · recorded indexes and gates.** A new test loads all 26
  `evals/datasets/*/recorded/index-*.json`; the route-fidelity gates pass unchanged.
- **T-D1-10 · L5 · hook cost.** Paired `batch_no_match` p95 within +0.3 ms with candidates
  compiled.

### D2 · `noticed`

- **T-D2-01 · L1 · once, never a directive.** A candidate's context fires `noticed` once; the
  same events again in the session fire nothing; `directives` and `would_have` are empty for it;
  a live route with the same events fires its directive and no `noticed`.
- **T-D2-02 · L1 · both arms.** Control and treated states produce the same `noticed`.
- **T-D2-03 · L1 · separate rings, bounded.** A relearn that changes only candidates changes
  `candidate_contexts.digest`, resets `cand_ctx`, and leaves `ctx` intact (a route context still
  fires on the next event); `noticed` holds at most 64 ids.
- **T-D2-04 · L1 · rebuild and the early exit.** `cand_ctx` and `noticed` are restored from the
  spool; a repository with candidates and no routes enters the match path.
- **T-D2-05 · L4 · the sim.** `replay_claude` asserts `noticed` for the `make lint` candidate at
  the right turn in both arms; `check_graph` counts it; `adage watch --once` shows the row.
- **T-D2-06 · L5 · cost.** Paired `batch_no_match` p95 within +0.3 ms.
- **T-D2-07 · L6 · replay gate.** A skipped event's `noticed` is empty on every dataset.

### D3 · The `skill` kind

- **T-D3-01 · L1 · model and validation.** `Step::Skill` round-trips through YAML; `shape()` is
  `skill`; `SHAPES.len() == 8`; `ceiling` is prime; validation accepts `release-checklist`,
  refuses `../x`, a 65-character name, slots, and `Inhibit`.
- **T-D3-02 · L1 · rendering and reading back.** `route_text` renders the fixed template;
  `render::named` on that body returns `Named.skills == ["release-checklist"]`, `commands` empty
  and `all: false`.
- **T-D3-03 · L1 · follow.** A `Batch` with a `Skill { skill: "release-checklist" }` call within
  the window judges `Followed`; a different skill does not.
- **T-D3-04 · L1 · the bridge.** A `Control { tool: "Skill release-checklist" }` step becomes
  `input::Step::Skill`; `Control { tool: "TodoWrite" }` is dropped as before.
- **T-D3-05 · L1 · learned.** Six synthetic sessions (prompt family, version bump, then the Skill
  call) compile to a `skill` route at prime with the family's intent trigger; four sessions do
  not; a session steered by the skill directive contributes no support and is steered by that
  skill alone.
- **T-D3-06 · L1 · every match site.** `cargo clippy --workspace --all-targets -- -D warnings`
  is clean, and the wildcard sites the plan lists have a reviewed comment naming the `Skill`
  case.
- **T-D3-07 · L3 · golden.** A prompt step in `claude_scenario` delivers `kind="skill"`.
- **T-D3-08 · L6 · gates and old binaries.** The whitelist in
  `replay_with_the_learned_index_gives_every_recorded_directive` names the new id; every other
  gate unchanged; an index with `format` bumped and a `Skill` step is reported by `doctor` as
  "newer than this binary" by a build without D3 (a unit test over the parse error path).

### D4 · `verified.json` and `SkillPrompt`

- **T-D4-01 · L1 · written, not digested.** `adage train` on the acme skill writes `verified.json`
  with one entry per candidate (`works`, `stale`, `not-replayed`), a date and `inert`; the pack's
  digest and `procedure.json` are unchanged by its presence; a SKILL.md with a dynamic-context
  block or `context: fork` gets `inert: false`.
- **T-D4-02 · L2 · accepted only, sanitised.** Before `train accept`, `adage hook claude
  SkillPrompt` prints nothing; after it, with `skill: acme-work-item` in that repository it prints
  `{"text": …}` holding the original text, the fixed heading and one line per step, ≤ 2,400 bytes
  added, with a stale signature containing an escape sequence rendered through `render::sanitise`;
  another skill name prints nothing.
- **T-D4-03 · L2 · the arm.** With no arm assigned yet, `SkillPrompt` assigns one as `pretool`
  does; the control arm prints nothing and spools `would_annotate`.
- **T-D4-04 · L7 · the mod.** `skill.prompt` with a stubbed answer returns the annotated text;
  with none, `next(e)`.
- **T-D4-05 · L9 · three entry points.** `/acme-work-item`, the Skill tool and a subagent
  preload each read the annotated text (S9 Q13).

### Cross-cutting

- **T-X-01 · L2 · fail open, binary, per event.** For each new event (`TurnStep`, `SkillPrompt`,
  `Standing`) and each changed path: `ADAGE_TEST_FAULT=panic` and the event-scoped faults
  (`panic:turn_step`, `panic:skill_prompt`, `panic:standing`, `panic:predict_grants`,
  `panic:receipt`) → exit 0, empty stdout, one `fail-open.log` line; a 9 MiB payload → exit 0,
  empty stdout; a payload missing `session_id` → exit 0, empty stdout; an unparseable
  `approvals.json` or `verified.json` → exit 0, the hook's normal output without the grant or the
  annotation.
- **T-X-02 · L7 · fail open, mod.** For every hook: stubbed `$.process.run` rejects, times out,
  returns exit 1, returns `{`, returns `{}` → the hook returns `next(e)`'s result unchanged;
  `turn.step` forwards every chunk; `ui.render` returns `next(e)`; the bridge's answer is `{ ok:
  false }` for the first four and `{ ok: true, answer: {} }` for the last.
- **T-X-03 · L7 · nothing drawn or started headless.** `session.start` with `isInteractive:
  false` starts no feed and every `ui.render` hook returns `next(e)`; the classic observers and
  `tool.call` still run.
- **T-X-04 · L7 + L8 · authority.** An argv recorder around the stubbed `$.process.run` runs every
  hook and command over every fixture; the set of argv heads is exactly `{hook, version, watch,
  graph, doctor, status, ledger note, off}` and never contains `review`, `delegate`, `install`,
  `config` or `on`; `check-plugin.mjs` fails if the allow-list literal in `bridge.ts` changes
  without the test; the mod returns no `decision` from `tool.check`, no `deny` from `tool.call`,
  never calls `$.prompt.submit` or `env.set`; CI's fresh `validate --strict` equals
  `VALIDATE.txt`.
- **T-X-05 · L2 · control arm, binary.** For every new output (`predict`, `routine`,
  `SkillPrompt` text, `Standing` text): `ADAGE_ARM=control` prints nothing or empty text and
  spools the `would_*` field; the `INJECTION` string from the existing goldens (`hooks.rs:214`)
  placed in a skill text, a standing card title and a pending command never reaches an output
  unsanitised.
- **T-X-06 · L7 · control arm, screen and pane.** With a control fixture whose records hold a
  non-empty `would_have` and a `would_predict`, and a treated fixture with no match: every
  site's tree, the band, the pane's Live tab and `/adage why`'s output are byte-identical.
- **T-X-07 · L7 · both surfaces.** Every `ui.render` hook mounts on `terminal` and `desktop`
  from the same fixture state without a refused tree (the kit reports validation).
- **T-X-08 · L9 · hosts.** The host table (architecture §7) is filled by observation: hooks run
  and nothing draws in `-p`, the VS Code panel and a cloud session; `$.process.run` on the
  Desktop app; the module on Windows; the pinned 2.1.221 loads a `hooks.json` carrying `modules`
  and still runs all seven command hooks.
- **T-X-09 · L8 · drift.** CI installs `@anthropic-ai/claude-code@2.1.287` for `validate` and
  `test`; `check-plugin.mjs` fails on a different pin; a bump is a reviewed change that re-runs
  L7, L8 and the `claude-code.d.ts` diff for each name `VALIDATE.txt` lists (S9 Q8's method).
- **T-X-10 · L8 · two plugins, one module.** `check-plugin.mjs` fails when `plugin/claude-code/
  hooks/*.ts` and `plugin/claude-code-exec/hooks/*.ts` differ other than the binary names.
- **T-X-11 · L9 + L7 · both installed, one active.** With both plugins installed and delegation
  on, the adage copy's probe runs `quiet` and stays inert: it registers no `/adage` command, draws
  nothing, starts no feed; the exec copy is active; `/plugin` shows one `adage` mod active per
  plugin but one set of `/adage` commands. L7: a stubbed probe returning empty stdout leaves every
  hook at `next(e)`.
- **T-X-12 · L3 · parity.** After every chunk, `adage-exec/tests/parity.rs` passes.

## 6. Test cases: the mod's surfaces (L7 unless marked)

Fixture state is a `$.state` snapshot per scenario: `empty`, `fired-1`, `fired-3`, `rewrite`,
`predicted`, `receipt`, `receipt-beside`, `noticed-forming`, `noticed-review`, `skill-invoked`,
`skill-verified`, `lapsed`, `control` (with `would_*` in its feed rows), `learning-waiting`,
`learning-running`, `learn-failed`, `off`, `paused`, `binary-missing`, `no-repo`. The kit drives
no keyboard: keyboard behaviour is asserted by calling the mounted tree's `onPress`/`onInput`
handlers directly and checking `$.state` and the recorded `$.ui.*` calls.

### B1 · Module, bridge, activation

- **T-B1-01 · bridge outcomes.** Stubbed `$.process.run`: exit 0 with JSON → `{ ok: true, answer
  }`; exit 0 with empty stdout → `{ ok: true, answer: undefined }`; exit 1, a rejection, a
  timeout, non-JSON → `{ ok: false }`; every call carries `env.CLAUDECODE === '1'` and an argv in
  the allow-list.
- **T-B1-02 · activation.** A probe returning `adage 0.1.0` sets `active`; empty stdout (the
  `quiet` script) leaves it unset and every registered hook returns `next(e)`; a probe failure
  leaves it unset.
- **T-B1-03 · where the binaries are.** With `CLAUDE_PLUGIN_DATA` set, `hookBinary` is
  `$DATA/bin/<name>` and `cliBinary` is `$DATA/bin/adage`; with a link to `scripts/quiet`, no
  `PATH` fallback is attempted; with no link at all, `PATH` is tried once.
- **T-B1-04 · observe mode.** With `ADAGE_MOD_OBSERVE=1`, only the `classic.*` observers and
  `ui.render` hooks are registered (the kit lists registrations); `tool.call`, `turn.step`,
  `skill.prompt`, `prompt.compose`, `session.compact` are absent.
- **T-B1-05 · L8 · static.** `validate --strict` output equals `VALIDATE.txt`; `check-plugin.mjs`
  passes on the committed tree and fails on a missing module file, a missing `types`, a changed
  allow-list, a changed CLI pin.
- **T-B1-06 · L9 · installed.** `/plugin` shows `1 mod active · adage`; with the binary removed,
  the hook outputs (`--include-hook-events`) and spool records of a `-p` run equal a run without
  the mod.

### B2 · Observe and draw

- **T-B2-01 · opening line.** `InfoNotice` with the engine's notice props for the hook's
  `systemMessage` draws `open.notice` with the glyph and the prefix stripped; with no notice
  props, `next(e)`; on `desktop` the same text as a toast.
- **T-B2-02 · prompt line.** `UserMessage` for the nth prompt with `fired-3` draws three lines in
  delivery order under the engine's row, none containing a strength word; `fired-1` one; `empty`
  returns `next(e)`; `isExpanded: true` returns `next(e)`.
- **T-B2-03 · truncation order.** At `bodyColumns: 40` the prompt line keeps glyph and route id
  and drops the command slot first; the `Text` carries `wrap: "truncate-end"` (the width itself
  is the engine's).
- **T-B2-04 · tool markers.** `ToolUse` matching a `rewrite` row draws `tool.rewrite`;
  `predicted` draws `tool.predicted`; a feed judgement row for an avoided dead end draws
  `tool.avoided`; an unmatched `requestId` returns `next(e)`.
- **T-B2-05 · receipt card and beside line.** `ToolResult` with `receipt` draws the two-line card
  with `✓` and no pytest output, names cut at 2,400 bytes; `receipt-beside` draws
  `result.receipt.beside` and `next(e)`.
- **T-B2-06 · spinner and turn line.** `Spinner` suffix present only with `fired-*`;
  `TurnDuration` summary only on `terminal` with counts > 0, worded `step(s) without a model
  request` and `test run(s) reused`, never `saved`; zero counts return `next(e)`.
- **T-B2-07 · band cells and priority.** `band.main` cells appear per ux-design §5; at
  `bodyColumns: 60` cells drop from the right in priority order and the lamp's word is never
  dropped; with `empty` and green health the band returns `next(e)`; `hasSurvey: true` returns
  `next(e)`; `off` draws `band.off`; `paused` draws `band.paused`; `binary-missing` draws
  `band.nobinary`; `no-repo` returns `next(e)`.
- **T-B2-08 · band second line.** `lapsed` adds `band.lapsed`; two attention items show the
  higher priority only (lapsed > problem > review > version).
- **T-B2-09 · lamp roles and words.** The health cell is `● healthy`, `● check: <check>` or
  `● problem: <check>` in the fire, prime and inhibit roles; `○` appears only in `band.off` and
  `band.nobinary`.
- **T-B2-10 · theme keys only, no banned words.** Every `Text` colour in every tree is a key from
  `tokens.ts`'s roles (a literal hex fails); no tree outside the pane contains `fire`, `prime`,
  `inhibit`, `forming`, `firm`, `dormant`, `approval` or `saved`; no `$.ui.status` text starts
  with `adage:`.
- **T-B2-11 · ASCII twins and motion.** With `ADAGE_ASCII=1` stubbed in `$.env`, every glyph is
  its twin and key names are words; with `ADAGE_REDUCED_MOTION=1`, no `invalidate` pulse is
  scheduled.

### B3 · Pane and commands

- **T-B3-01 · tabs and hotkeys.** The tab Buttons carry hotkeys `l`, `r`, `t` (lowercase, no
  `h`); pressing each tab's `onPress` switches `$.state.tab`; `/adage` opens the pane with
  `focus`, `closeOnEscape`, `holdToasts`, `rows: 20` (the recorded `$.ui.open` call).
- **T-B3-02 · Live rows and inspect.** Rows from fixture feed lines; a row's `onPress` opens
  `adage-inspect` with the directive text (the recorded `$.ui.open`); the row stays selected in
  state.
- **T-B3-03 · filter.** `onInput("lookup")` leaves rows containing it; submit keeps the filter; an
  empty submit clears it; the inspect handler refuses a filtered-out row.
- **T-B3-04 · footer actions.** The `p` button toggles `paused` in state; `v` hides quiet rows;
  `n` records a `$.ui.scroll` to the end.
- **T-B3-05 · Reflexes read-only, one copy per verb.** No Button in the tree has an `onPress`
  that calls `$.process.run`; for a row whose `review.verbs` is `["strengthen", "pin"]` there are
  two copy buttons whose commands are `adage review strengthen <track> <id>` and `adage review
  pin <track> <id>`; a press calls `$.ui.copy` and toasts `toast.copied`; a result saying the
  copy did not happen toasts `toast.copy.unavailable`; the strength and lifecycle filters are
  `Select`s.
- **T-B3-06 · Health actions.** A check whose `action` is `condition` shows `pane.health.action`
  and its press runs the allowed argv; any other shows `Copy command`; a lapsed grant lists its
  reapprove command behind `Copy command`.
- **T-B3-07 · empty states.** Each tab's empty text per ux-design §6.4; `pane.live.desktop` when
  the fixture says the bridge is unavailable.
- **T-B3-08 · the feed logic.** `nextArgs(lastCursor)` returns the `--since` form only when a
  cursor exists; a chunk ending mid-line is buffered and parsed on the next chunk; an unparseable
  line is skipped; the failure counter reaches 3 and sets `feed: 'stopped'`; rows cap at 500.
- **T-B3-09 · commands.** Each `/adage` command's `command.run` returns the text in ux-design §7
  and the right `$.ui.open` call; `/adage off` runs exactly `['<cli>', 'off']`; `/adage on` runs
  nothing and copies `adage on`; `/adage why` in the `control` fixture returns `cmd.why.none`;
  every command is `immediate`.
- **T-B3-10 · settings reload.** `register(on, { band: false })` registers no `AbovePrompt` hook;
  `noticed: false` draws no noticed line; `pane: 'auto'` opens at `viewport.columns ≥ 144` only;
  `skills_invoke` defaults to false.

### C4 · The mod's `turn.step`

- **T-C4-01 · the generator.** With a stubbed `ask` answering a prediction, the hook yields
  exactly three chunks (`tool` with an `adage_<route>_<turn>_<index>` id, `input` with the
  prediction's JSON, `stop` with `stopReason: 'tool_use'`) and returns `{ stopReason: 'tool_use',
  usage: null, toolUses: [...] }` without calling `next`.
- **T-C4-02 · forwarded.** With `ask` answering nothing or `{ ok: false }`, with `e.agentId` set,
  or with `e.index === 0`, the hook is `yield* next(e)` and asks nothing in the last two cases;
  the ask's timeout is 250 ms.
- **T-C4-03 · skills and routing.** A `kind: skill` prediction yields the `Skill` tool with `{
  skill, args? }` only when `skills_invoke` is on and `$.command.list()` lists the skill; a
  `routine` answer with `routine_model` set calls `next({ ...e, model, effort: 'low' })`.
- **T-C4-04 · L4 · the simulator, both arms.** `replay_claude` under `adage-exec` with the grant
  fixture predicts at the schema-edit turn; in the control arm nothing is predicted and
  `would_predict` is spooled; without the grant nothing; under `adage` nothing; `check_graph`
  counts `predicted`.
- **T-C4-05 · L9 · the dialog and the id.** Q9: ten of ten live runs skip the request, the
  predicted call meets the permission dialog where the model's own call would, `$.ui.notice`
  shows under it, and the transcript on disk keeps the `adage_` id.
- **T-C4-06 · L10 · end to end.** The demo's `mod` arm runs an item with one predicted step; the
  spool's `turn_step` record and the transcript agree on the id.

### D5 · Drawing noticed, learning and skills

- **T-D5-01 · noticed line.** `ToolGroup` with `noticed-forming` draws `noticed.forming` with the
  `○` glyph once; the same candidate in a later batch draws nothing; `noticed-review` draws
  `noticed.review`; a one-event candidate record (should not exist) draws nothing.
- **T-D5-02 · learner countdown and toasts.** `learning-waiting` shows `learning in 38 s`;
  `learning-running` shows `learning…`; a `learn` feed line with a strength change toasts
  `toast.learned.strength` with `from this session` only when this session's id is in its
  `sessions`, else `from recent sessions`; a line with no change toasts nothing; a line turning a
  candidate into a route toasts `toast.learned.formed`; `learn-failed` toasts
  `toast.learn.failed` once.
- **T-D5-03 · skill markers.** `skill-invoked` and `skill-verified` draw their `◈` markers;
  `/adage why` lists the verified steps in `cmd.why.step` form.

### Usability

- **T-U-01 · L13.** Per ux-design §14: the reading part (8 readers, 7 of 8), the arm part (20
  matched pairs, 160 answers, pooled accuracy's 95% interval includes 0.5), the doing part (five
  tasks, 7 of 8 under 60 s each). Verbatim confusions recorded in the findings file.

## 7. Evaluation cases and protocol (L10–L12)

### 7.1 Metrics

| Metric | Definition | Source |
| --- | --- | --- |
| `requests` | The debug log's `[API REQUEST]` dispatch count, cross-checked against the stream's distinct explicit request ids (`x-client-request-id=… source=…`, never message ids), which must be unique; `requests_verified` is true when they agree | `heldout_analysis.requests()`, `heldout_analysis.py:67, 233-249`, `probe.py:155-156`; mirrored in `demo/run.py` |
| `predicted_steps` | `turn_step` spool records with `predicted` set | the session spool |
| `receipts_as_results` | `pretool` records with `answered: receipt` | the session spool |
| `cost_usd`, tokens | `result.total_cost_usd` and the four counts per step | `stream-json` result; `ledger note` |
| `passed`, `ci_passed`, `hidden_passed`, `hidden_tests_run`, `stale_generated` | `score.py` | the sandbox |
| `turns`, `tool_calls`, `turns_to_first_edit`, `dead_end_hits` | as today | `demo/run.py` |
| Hook p95 | `duration_us` | the spool |

### 7.2 Arms

| Arm | Hooks | Mod | Grants | Purpose |
| --- | --- | --- | --- | --- |
| `a0` | none | off | none | the no-hooks baseline the registered protocol uses |
| `control` | on, `ADAGE_ARM=control` | off | none | the in-product savings baseline; not A0 |
| `adage` | on, treated | off | as the study | today's product |
| `mod-observe` | on, treated | on, `ADAGE_MOD_OBSERVE=1` | as the study | proves observation changes nothing |
| `mod` | on, treated | `--plugin-dir plugin/claude-code-exec` (plugin only, never `--settings` as well) | the study's plus `--predict` on the finish-line route | the design |

### 7.3 Cases

- **T-E1-01 · L10 · the metric.** `python3 demo/run.py one W-2041 adage --debug` records
  `requests` and `requests_verified`; a synthetic debug log with a retry shows `requests` equal
  to the distinct id count and `requests_verified` true; duplicate ids make it false.
- **T-E1-02 · L10 · the arms wire once.** The `mod` and `mod-observe` arms produce exactly one
  hook invocation per event (the plugin's), checked by counting `session_start` records; the `a0`
  arm produces no spool.
- **T-E1-03 · L10 · observation changes nothing.** `demo/replay_hooks.py` feeds a recorded stream
  of hook payloads to `adage hook` with and without the mod's observe mode; the outputs and the
  spool records are byte-identical; the set of `<adage-reflex>` blocks and `systemMessage`s per
  event class in six live `mod-observe` runs equals six `adage` runs' (live transcripts are not
  compared byte for byte: the model is not deterministic).
- **T-E1-04 · L10 · the report.** `compare(runs, a="mod", b="a0")` reports the task-stratified
  permutation p and the bootstrap interval for `requests`, `cost_usd`, `passed`, `hidden_passed`
  and `stale_generated`.
- **T-E2-01 · L11 · the arm.** `dev_v9.py` with `MOD` in `ARMS` and `CONTROL_ARMS` dispatches 16
  blocks with MOD's five realized controls; a dry run shows the `predict` grant issued as the
  human step before any positive block; the revision is registered in
  `docs/launch-gate-protocol.md` before dispatch.
- **T-E2-02 · L11 · the rule.** The verdict loop reads `D1_requests_max_ratio_to_A0`,
  `D1_blocks_fewer_min` and `D4_sessions_with_repeat_attempts_max` from the frozen manifest's
  `dev_thresholds_v9` (`dev_v9.py:522-527`) and the study's validity rules
  (`launch-gate-protocol.md:346-352`); the findings file states each as pass or fail.
- **T-E2-03 · L11 · twice.** Two runs on fresh seeds both pass before E3 is dispatched.
- **T-E3-01 · L12 · registered.** New groups, arms A0 / NOEXEC / BEST = MOD, 20 blocks, safety
  controls first, every applicable G criterion (G1, G2, G5–G8), the censoring rule, the
  sensitivity analyses, `QUALITY_MARGIN = 1`, the one-sided 95% bootstrap upper bound of
  `sum(MOD)/sum(A0)` ≤ 0.90, at most 2 excluded blocks (`launch-gate-protocol.md:561-611`;
  `heldout_analysis.py:57-61`); `heldout_cohort.py` and `heldout_analysis.py` accept the arm name.
- **T-E3-02 · L12 · recorded.** The outcome, pass or not, is in
  `evals/reflex-proof/LAUNCH-STATUS.md` with the scripts that produced every number. Model
  requests are never presented as GPU savings.

### 7.4 Savings in the product

The in-product estimate (`savings.rs`: Hajek IPW, winsorised, stratified bootstrap, the 10/30
gate, `savings.rs:19, 33-34`) is unchanged in method; harness-priced lines enter it through A4's
merge. T-A4-04 guards the method with a population that includes predicted steps.

## 8. Live probes (L9, from S9)

Each probe has a script under `spikes/S9-claude-mods/` (`rerun.sh` runs them all), a `results/`
file and a pass criterion in the spike: Q1 coexistence and `CLAUDE_PLUGIN_DATA`, Q2 latency and
whether time inside `next(e)` counts toward the budget, Q3 surfaces and silence (and the props
ux-design §11 leans on), Q3b the feed, keys and panes across a reload, Q4 `$.tool.check`
agreement and where the permission mode is read, Q5 provenance and cost on disk, Q6 fail-open,
reload and the pre-2.1.287 `modules` key, Q7 hosts and `$.process` on the Desktop, Q8 drift, Q9
the skipped request, the dialog and the id on disk (10 of 10), Q10 the gate, Q11 routing, Q12
noticed on the fast path and in a person's eyes, Q13 skills, inertness and the three entry
points. A chunk that depends on a question (plan §3) does not merge before the question's result
is committed.

## 9. CI and release gates

| Gate | Runs | Blocks |
| --- | --- | --- |
| `cargo fmt`, `clippy -D warnings`, `cargo test --workspace --exclude adage-eval --locked` | every PR | merge |
| `node scripts/check-plugin.mjs` (modules, types, allow-list, pin), `test-stage-plugin.mjs`, `claude plugin validate --strict` ×2 diffed against `VALIDATE.txt`, `claude plugin test` ×2 (CLI pinned at 2.1.287) | every PR | merge |
| `plugin/test/session-start.test.sh`, `npm/test/launcher.test.mjs` | every PR | merge |
| `cargo test -p adage-eval --test gates` | weekly; required before merging C1, D1, D2, D3 | merge of those chunks |
| Paired `hook_latency` and `learn_bench` on release builds, numbers and the machine in the PR | before merging A1, A2, A6, C1, D1, D2 | merge of those chunks |
| `release_plugin_startup` (first start < 1,500 ms, ten warm starts < 200 ms; `zero_config_plugin.rs:246, 275`) | scheduled; before a release | release |
| `cargo run -p adage-sim` fixtures current (`committed_fixtures_are_current`) | every PR | merge |
| S9 question results committed | before the chunk that depends on them | merge |
| T-U-01 | before merging B3 and D5 | merge of those chunks |
| The v9 development rule, twice | before E3 | the registered run |
| `cargo deny`, DCO | every PR | merge |

Flakes are not re-run to green: a failing test is a defect or a test to make robust (this plan's
rule; `CONTRIBUTING.md` states only that hook paths fail open, and its local command list gains
`claude plugin test`). Evals are not part of PR CI (paid model calls); their numbers go in the
PR. No workflow runs the latency examples today; a scheduled job that builds `--release
--examples` and uploads the JSON is a follow-up, and until it exists L5 is a developer's
responsibility stated in each PR.

## 10. Entry and exit criteria per milestone

| Milestone | Entry | Exit |
| --- | --- | --- |
| M0 Spike | — | Q1–Q13 findings in `FINDINGS.md` with `rerun.sh`; Q1, Q2 (Layer 1), Q3, Q6 pass |
| M1 Foundation | — | T-A1 … T-A6 all green; the goldens unchanged except the silent `PreToolUse` step; paired latency numbers in the PRs |
| M2 See it | M0's four questions, A3, A5, A6 | T-B1 … T-B3, T-X-02, T-X-03, T-X-04, T-X-06, T-X-07, T-X-10, T-X-11 green; T-E1-03's replay diff empty; T-U-01 reading and doing parts |
| M3 Trust it | M1, Q2 (Layer 2), Q4 | T-C1 … T-C3, T-X-01, T-X-05, T-X-12 green |
| M4 Skip it | M3, Q9 | T-C4, T-C5 green; Q9 10 of 10; the `mod` arm runs end to end (T-C4-06) |
| M5 Notice | D1's L5 numbers | T-D1, T-D2, T-D5-01, T-D5-02 green; Q12; T-U-01 noticed questions |
| M6 Skills | — | T-D3, T-D4, T-D5-03 green; Q13 |
| M7 Evidence | M4 | T-E1 green; T-E2 twice; T-E3 recorded |

## 11. Defects and reporting

- Severity: **S1** blocks or alters the agent's work (a hook that does not fail open, a deny, a
  changed model request outside a grant or the person's own routing setting, a receipt answered
  for a call the rules would have asked about); **S2** a wrong decision or wrong count (a
  prediction without a grant, a dedup miss, a candidate from a single call, an arm named in the
  pane); **S3** a drawing defect; **S4** copy and polish. S1 and S2 block merge; S3 blocks the
  milestone; S4 is logged.
- Every eval and probe result is a `FINDINGS.md` in its folder with the script that produced
  every number (the spikes' rule); the PR links it.
- A golden change is called out in the PR description with the diff.

## 12. Risks to the testing itself

- **The engine's test kit stubs `$` and drives no keyboard, no child process and no real
  latency**: L7 asserts handlers and trees; focus, scrolling, the feed child and latency belong
  to L9 and L5.
- **`claude plugin test` depends on a pinned CLI**; a CLI update is a PR of its own that re-runs
  L7, L8 and the drift check.
- **Headless runs load the mod** (`-p` runs hooks): every L10 run states its arm, `mod-observe`
  exists to prove observation changes nothing, and `a0` exists because `control` is not a
  no-hooks baseline.
- **The four held-out tasks are spent** (`LAUNCH-STATUS.md`): E3 needs new groups; E1 uses the
  acme-billing sandbox and E2 the development repositories only.
- **Two protocols exist in the repository** (dev-arms-1's v8 rule at
  `launch-gate-protocol.md:249-253`, dev-arms-2's v9 at `:340-352`): this plan uses v9, as the
  named harness does.
- **A person's reading is one person's**: T-U-01 uses eight readers and twenty matched pairs and
  reports verbatim.
