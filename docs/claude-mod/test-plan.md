# Adage in Claude Code: the test plan

> Status: **proposed 2026-10-02**. How the work in [implementation-plan.md](implementation-plan.md)
> is proven, chunk by chunk and as a whole: the levels, the environments, the fixtures, every
> test case with its pass criterion, the evaluation protocol that decides the "fewer model
> requests" claim, and the gates a release must clear. Written against the test infrastructure
> as it stands (`crates/adage-cli/tests/`, `crates/adage-sim`, `crates/adage-eval`, `demo/`,
> `evals/reflex-proof/`, the `plugin` CI job). Case ids are stable: a pull request names the
> cases it adds or changes.

## 1. Objectives and quality bars

1. **The floor never regresses.** `adage hook claude <event>` prints the same bytes for every
   event it printed before; `adage-exec hook` matches it with no grants. Any difference is a
   reviewed golden change.
2. **Fail open is total.** No path in the binary or the mod can block, delay past budget, or
   alter a tool call, a prompt or a model request on an error. Fault injection proves it.
3. **The control arm is silent and indistinguishable**, in the binary (no output) and on the
   screen (the neutral band and notice only).
4. **Authority is never widened.** No test may pass in which the mod approves a call, denies a
   call, strips an environment variable, or runs a person-only command.
5. **Budgets hold.** Hook p95s within Architecture §9's budget plus the margins each chunk
   states; learn time within 1.3×; the mod adds ≤ 1 ms p95 when observing.
6. **The claim is measured, not assumed.** Fewer model requests at preserved hidden-test quality,
   first against the development thresholds (D1–D4), then under the registered protocol.
7. **A person can read it.** The surfaces pass a usability check with readers who have not seen
   Adage.

## 2. Test levels and environments

| Level | Tooling | Isolation | Where it runs |
| --- | --- | --- | --- |
| L1 Unit, Rust | `cargo test -p <crate>` | in-process, `tempfile` | every PR |
| L2 Integration, binary | `crates/adage-cli/tests/*` with a `World`/`Sandbox` (fresh `ADAGE_HOME`, `HOME`, `XDG_CONFIG_HOME`, `CODEX_HOME`; `CLAUDECODE`, `OPENCODE`, `CODEX_SANDBOX*`, `ADAGE_DISABLE`, `ADAGE_ARM` removed; `ADAGE_AUTO_LEARN=0`; `NO_COLOR=1`) | per test tempdir | every PR |
| L3 Golden and parity | `ADAGE_BLESS=1` goldens in `tests/golden/`, `adage-exec/tests/parity.rs` | as L2 | every PR |
| L4 Simulator | `crates/adage-sim` fixtures, `harness_sim.rs` | as L2 | every PR; `cargo run -p adage-sim` on fixture change |
| L5 Latency | `crates/adage-cli/examples/hook_latency.rs`, release build, 500 runs after 20 warm-ups; `learn_bench` | a quiet machine, numbers in the PR | scheduled CI; before merging A, C, D chunks |
| L6 Replay gates | `cargo test -p adage-eval --test gates` | recorded datasets | weekly `evals.yml`; before merging D1, D2, C1 |
| L7 Mod unit | `claude plugin test plugin/claude-code` (`*.test.ts`, `ui.mount` over `terminal` and `desktop`, stubbed `$`) | the engine's test kit, no session | the `plugin` CI job, pinned CLI version |
| L8 Mod static | `claude plugin validate --strict`, `scripts/check-plugin.mjs`, committed `VALIDATE.txt` | | the `plugin` CI job |
| L9 Live probes | S9's probe mod under `--plugin-dir`, an isolated `CLAUDE_CONFIG_DIR`, `claude --debug` | a developer's machine; results in `spikes/S9-claude-mods/results/` | once per question |
| L10 Headless end to end | `demo/run.py` arms `control`, `adage`, `mod` on the acme-billing sandbox, Haiku | fresh sandbox per run, `ADAGE_HOME` under `DEMO_ROOT` | before merging B2, C3, C4; model spend |
| L11 Development gate | `evals/reflex-proof/harness/dev_v9.py` with a `MOD` arm | per the harness | before E3 |
| L12 Registered | `docs/launch-gate-protocol.md` on new groups | per the protocol | once |
| L13 Usability | three readers, the recording | | once per surface change |

## 3. Fixtures and data

| Fixture | Holds | Used by |
| --- | --- | --- |
| `sim/fixtures/` (12 Claude, 12 OpenCode, 12 Codex seeded sessions; held-out W-2045) | The acme-billing scenario: lookup, dead ends, finish line, sequence | L4 |
| New sim payloads: `PreToolUse` per Bash call with `tool_use_id`; `TurnStep` per turn; `SkillPrompt` | `crates/adage-sim/src/claude.rs` | L4 |
| `demo/sandbox/` (acme-billing with hidden tests) | L10's repository; `score.py` keys `passed, ci_passed, hidden_passed, hidden_tests_run, stale_generated` | L10 |
| `evals/datasets/*/recorded/index-*.json` (about twenty) | Indexes from before this work; must keep parsing | L6 |
| `evals/lab/skills/skills/acme-work-item/SKILL.md`, `click-bugfix/SKILL.md` | Skills for `adage train` and `SkillPrompt` | L1 (train), L2, L9 |
| Settings fixtures for rewrites (new, `crates/adage-cli/tests/fixtures/settings/`): `allow-pytest.json` (`Bash(pytest:*)`), `allow-venv.json`, `dont-ask.json`, `none.json` | S9 Q4 and C3 | L2, L7, L9 |
| Synthetic spools and ledgers (`zero_config_plugin.rs::population()`) | Savings gate inputs | L2 |
| Synthetic transcripts with Skill tool uses (`adage-learn/tests/golden.rs` `Tx::call("Skill", …)`) | D3 learner tests | L1 |
| Mod fixtures (`plugin/claude-code/hooks/fixtures/`): classic hook payloads and results, spool rows, `graph --json`, `doctor --json`, `verified.json`, `watch --follow --json` lines | L7 | L7 |
| Fault injection: `ADAGE_TEST_FAULT=panic`, a `quiet` binary, a binary that sleeps 12 s, a binary that prints `{` | Fail-open cases | L2, L7 |

## 4. Traceability

| Feature (architecture §) | Plan chunk | Cases |
| --- | --- | --- |
| Dedup per `tool_use_id` (§2 rule 2) | A1 | T-A1-* |
| Latency on record (§3.10) | A2 | T-A2-* |
| The feed (§2, §4.1) | A3 | T-A3-* |
| Harness-priced ledger (§3.10) | A4 | T-A4-* |
| Tokens and card JSON (§4 rules) | A5 | T-A5-* |
| Module, bridge, CI (§2) | B1 | T-B1-* |
| Observe and draw (§3.1, §4) | B2 | T-B2-* |
| Pane and commands (§4.1, §5) | B3 | T-B3-* |
| `TurnStep` (§3.2) | C1 | T-C1-* |
| `predict` grant (§3.2) | C2 | T-C2-* |
| Receipt as result, `$.tool.check` (§3.3, §3.4) | C3 | T-C3-* |
| The mod's `turn.step`, sim (§3.2) | C4 | T-C4-* |
| Cached delivery, compaction, routing (§3.6–3.8) | C5 | T-C5-* |
| Candidates (§3.11) | D1 | T-D1-* |
| `noticed` (§3.11) | D2 | T-D2-* |
| `skill` kind (§3.12) | D3 | T-D3-* |
| `verified.json`, `SkillPrompt` (§3.12) | D4 | T-D4-* |
| Drawing noticed, learning, skills (§4) | D5 | T-D5-* |
| Requests metric, arms (§9) | E1 | T-E1-* |
| Development gate, registered run (§9) | E2, E3 | T-E2-*, T-E3-* |
| Cross-cutting: fail open, control, authority, hosts, drift | all | T-X-* |

## 5. Test cases: the binary

Format: id · level · title. **Given / When / Then**, then the pass criterion where it is not
the Then itself.

### A1 · `PreToolUse` identity

- **T-A1-01 · L1 · identity for PreToolUse.** `Claude::event_identity("PreToolUse", {tool_use_id:
  "t1"})` is `Some`; without `tool_use_id`, `None`; `tool_event_identity` still refuses empty,
  duplicate and oversized ids.
- **T-A1-02 · L2 · one decision per id.** Given a repository with the U11 environment bound
  (the `zero_config_plugin.rs` Python fixture). When the same `PreToolUse` payload (`python -m
  pytest tests -q`, `tool_use_id: t1`) is piped twice. Then the first prints the rewrite, the
  second prints nothing, and the session spool holds exactly one `pretool` record.
- **T-A1-03 · L2 · the ring is bounded.** 65 distinct ids each decide; the 66th with the first
  id decides again (the oldest was dropped); the state file stays under one slot (64 KiB).
- **T-A1-04 · L2 · no id, no dedup.** Two payloads without `tool_use_id` both decide, as today.
- **T-A1-05 · L3 · golden.** `claude_scenario` gains the PreToolUse step; the golden diff shows
  only the added section; parity passes.
- **T-A1-06 · L5 · cost.** `pretool_no_match` p95 within 0.1 ms of the pre-change value.

### A2 · `duration_us` and the doctor check

- **T-A2-01 · L1 · every record carries it.** Decision, `pretool` and `session_start` records
  have `duration_us` as an integer; rebuild ignores it.
- **T-A2-02 · L1 · the percentile.** Nearest-rank p95 over 500 synthetic durations matches a
  hand-computed value; fewer than 20 records → the check is `pass` with detail `too few hooks to
  measure`.
- **T-A2-03 · L2 · doctor reports it.** `adage doctor --json` on a seeded home lists `hook
  latency` with `status` and `detail` naming the p95 and the record count; a seeded spool with
  p95 12 ms gives `warn`, 30 ms gives `fail`.

### A3 · `adage watch --follow --json`

- **T-A3-01 · L2 · streams.** Start `--follow --json`; append one spool record and one
  `learn.log` line; two JSON lines arrive within 2 s with `kind` `decision` and `learn` and
  increasing `seq`; EOF on stdin exits 0.
- **T-A3-02 · L2 · resumes.** `--since 3` after five rows prints rows 4 and 5 then follows.
- **T-A3-03 · L2 · health and learner records.** Touching `<slug>.spawn.json` with a future
  `after` emits `{"kind":"learner","state":"waiting","next_at":…}`; taking the lock emits
  `running`.
- **T-A3-04 · L2 · never a terminal.** With stdout a pipe, no escape sequence appears in the
  output (regex `\x1b\[` absent); the `--once` frame and the UI snapshots are unchanged.
- **T-A3-05 · L5 · steady.** 200 rows at 2 Hz for 60 s: no gap in `seq`; RSS flat within 10 MiB.

### A4 · `adage ledger note`

- **T-A4-01 · L1 · harness wins.** A `SessionSummary` with `cost.source: harness` for session S
  replaces a transcript-priced line for S; `.seen.json` marks S harness-priced so a later
  transcript pass does not overwrite it.
- **T-A4-02 · L1 · steps and predictions.** Six `StepUsage` entries, two `predicted: true`:
  `model_turns` counts six, `predicted_steps` two, tokens sum the four counts.
- **T-A4-03 · L2 · the command.** `echo '<valid line>' | adage ledger note` exits 0 and `adage
  savings --json` shows S with `source: harness`; a malformed line exits 2 and writes nothing;
  the command runs with `CLAUDECODE` set.
- **T-A4-04 · L2 · the gate unchanged.** `savings_gate` and `pricing_honesty` pass unchanged.

### A5 · Tokens and card JSON

- **T-A5-01 · L1 · generated and current.** `cargo xtask tokens --check` passes; `tokens.ts`
  exports every glyph in `adage.tokens.json`'s `glyph` group with its ASCII twin and every role
  with a theme key; the staleness test fails when `tokens.ts` is edited by hand.
- **T-A5-02 · L2 · `adage --json`.** Parses; its rows equal the card's text rows one to one
  (an insta snapshot of both).

### C1 · `TurnStep`

- **T-C1-01 · L1 · predicts a delivered directive.** Given a state with a `pending` for route R
  (`expect_key: exec make gen`) and a stub extension returning a `predict` grant for R. When
  `TurnStep { turn_id: t, index: 1 }` is decided. Then `Decision.predicted` is `{ route: R, tool:
  Bash, input.command: "make gen", kind: obligation }` and `last_step_predicted == (t, 1)`,
  `predictions == 1`.
- **T-C1-02 · L1 · never two in a row.** The next `TurnStep { t, 2 }` with no batch between
  predicts nothing; after a `Batch` that observes `exec make gen`, the pending is cleared and
  the next step predicts nothing because nothing is pending.
- **T-C1-03 · L1 · the cap.** With `runtime.predict_cap = 2`, the third eligible step predicts
  nothing and `would_predict` is set.
- **T-C1-04 · L1 · control.** In the control arm `predicted` is `None` and `would_predict` is
  set.
- **T-C1-05 · L1 · no grant, nothing.** Without the extension or with the stub answering `None`,
  nothing is predicted.
- **T-C1-06 · L1 · a skill step.** A pending `Step::Skill { skill: "release-checklist" }`
  predicts `{ tool: Skill, input: { skill: "release-checklist" } }`.
- **T-C1-07 · L1 · bounds survive a rebuild.** `rebuild` over a spool with one `turn_step`
  record restores `last_step_predicted` and `predictions`.
- **T-C1-08 · L1 · fast path.** `fast::skip` returns true for `TurnStep` with no pending and no
  armed obligation, false otherwise.
- **T-C1-09 · L1 · `by: Adage`.** A `PostToolBatch` call with `tool_use_id` `adage_r_t_1` sets
  `Call.by = Adage`, and `decide` judges `Predicted { route }` not `Followed`.
- **T-C1-10 · L2 · the event end to end.** `adage hook claude TurnStep` with a payload after a
  seeded pending prints `{"predict":…}`; on Codex and OpenCode adapters the same event prints
  nothing.
- **T-C1-11 · L3 · goldens.** Two `TurnStep` steps in `claude_scenario` (one predicting, one
  silent); parity passes with no grants (the parity scenario predicts nothing).
- **T-C1-12 · L5 · latency.** `turn_step_nothing_pending` p95 ≤ `batch_no_match` p95 + 0.3 ms;
  `turn_step_predicts` p95 ≤ 10 ms.
- **T-C1-13 · L6 · replay gate.** `fast_path_changes_no_decision` passes with `TurnStep` events
  injected into every dataset's replay.

### C2 · The `predict` grant

- **T-C2-01 · L1 · distinct keys.** `ApprovalKey` for the same request with `mode: Predict` and
  `mode: Execute` differ; `diff` names `mode`.
- **T-C2-02 · L1 · old stores load.** An `approvals.json` written without `mode` loads with
  `Execute`; a round trip preserves it.
- **T-C2-03 · L1 · policy.** `permits` refuses a predict request at `max_level e1` and allows it
  at `e2`; `exec/predict-off` present → `predict_grant` answers `None`.
- **T-C2-04 · L2 · person only.** `adage review delegate T R --predict` with `CLAUDECODE=1` exits
  non-zero naming the variable and writes nothing; at a terminal (`ADAGE_ALLOW_NONINTERACTIVE=1`
  for the test) it writes the approval.
- **T-C2-05 · L2 · round trip.** `delegate status --json` lists the grant with `mode: predict`;
  `revoke` removes it; changing the route's command template makes `lapsed` list it and the
  notice's reapprove command ends in `--predict`.
- **T-C2-06 · L2 · exclusive flags.** `--predict --effect verify` is a usage error.
- **T-C2-07 · L3 · parity.** `adage-exec hook` with no grants matches `adage hook` byte for byte.

### C3 · Receipt as result, rewrites checked

- **T-C3-01 · L1 · fresh.** `covering_receipt` returns `fresh: true` and `result` for an applied
  run on identical inputs covering the call; after one byte changes in a snapshotted file,
  `fresh: false` and no `result`; a call outside the root, `fresh: false`.
- **T-C3-02 · L2 · the floor still gets the note.** `adage hook claude PreToolUse` prints the
  receipt as context (unchanged golden apart from the new `receipt` field).
- **T-C3-03 · L7 · the mod answers.** `tool.call { Bash }` with a stubbed `ask` returning
  `receipt.fresh` returns `{ result, context }` and never calls `next`; the stubbed `$.process.run`
  count is one (the ask) and the test's tool hook is never reached.
- **T-C3-04 · L7 · rewrite with the engine's verdict.** Stubbed `ask` returns a rewrite; stubbed
  `$.tool.check` answers `allow` → `next` called with the new command and `$.ui.notice` once;
  answers `deny` → `next(e)` unchanged and no notice; answers `ask` → `next` with the new
  command (the dialog decides).
- **T-C3-05 · L9 · agreement.** Over the four settings fixtures, `$.tool.check`'s verdict on the
  rewritten command equals `pretool::keeps_permissions`' on every settings-file case, and
  answers the `--allowedTools` case the planner cannot see.
- **T-C3-06 · L10 · no duplicate run.** In the demo with E2 on and a passing Adage run, the
  agent's repeated `make test` returns the receipt and the sandbox's command log shows no second
  `make test` process.

### C5 · Cached delivery, compaction

- **T-C5-01 · L1 · `standing` is stable and bounded.** Two `SessionStart`s in one session answer
  identical `standing` text, ≤ 2,000 characters; a control session answers none.
- **T-C5-02 · L7 · the section.** `prompt.compose` appends `adage:procedures` with `scope:
  session` once; with empty `standing` it returns `next(e)`'s sections unchanged.
- **T-C5-03 · L7 · compaction.** `session.compact` removes rows whose text holds `<adage-reflex`,
  `<adage-receipt` or the finish-line sentence and appends the instruction; other rows are
  passed with their handles.
- **T-C5-04 · L9 · byte-identical across requests.** `$.prompt.compose()` read at three points of
  a session returns the same `adage:procedures` text.

### D1 · Candidates

- **T-D1-01 · L1 · the floor.** With `min_support 5, border 2`, a label seen in 3 sessions stays
  in the alphabet; seen in 2, it is dropped.
- **T-D1-02 · L1 · border to candidate.** A context pattern with support 4 (below `s_min` 5,
  inside the border) over qualifying items yields a `Candidate` with `support 4, s_min 5`; with
  support 2 nothing; a one-event after-edit pattern with support 4 yields nothing.
- **T-D1-03 · L1 · stable id.** The candidate's id equals the route id the same pattern gets at
  support 5 (one more supporting session makes it a route with the same id; `learn.log` says
  `change: new` with the tipping session ids).
- **T-D1-04 · L1 · nascent routes are candidates.** A nascent environment route compiles into
  `candidates` with `review: true` and not into `routes`.
- **T-D1-05 · L1 · index.** `Triggers::of` includes candidate words and globs; `candidate_contexts`
  is separate from `contexts` and has its own digest; `index_is_deterministic` holds; an index
  JSON without the new fields loads with empty maps.
- **T-D1-06 · L1 · incremental equals full.** `cargo test -p adage-graph --test border` passes
  with the new floor.
- **T-D1-07 · L2 · the sim's candidate.** `adage condition --demo` over `sim/fixtures` writes
  `candidates.json` for acme-billing with the registry sequence (the three-of-twelve pattern) and
  `adage status --json` lists it under a new `candidates` key.
- **T-D1-08 · L5 · learn time.** Full learn over the sim's 36 sessions ≤ 1.3× the pre-change
  time; `condition --auto` with one changed transcript ≤ 150 ms.
- **T-D1-09 · L6 · recorded indexes.** Every `evals/datasets/*/recorded/index-*.json` loads; the
  gates pass.

### D2 · `noticed`

- **T-D2-01 · L1 · once, never a directive.** A candidate's context fires `noticed` once; the
  same events again in the session fire nothing; `directives` and `would_have` are empty for it.
- **T-D2-02 · L1 · both arms.** Control and treated states produce the same `noticed`.
- **T-D2-03 · L1 · separate rings.** A relearn that changes only candidates changes
  `candidate_contexts.digest`, resets `cand_ctx`, and leaves `ctx` intact (a route context still
  fires on the next event).
- **T-D2-04 · L1 · rebuild.** `cand_ctx` and `noticed` are restored from the spool.
- **T-D2-05 · L4 · the sim.** `replay_claude` asserts `noticed` for the registry candidate at
  the `make gen` turn in both arms; `check_graph` counts it.
- **T-D2-06 · L5 · cost.** `batch_no_match` p95 with candidates compiled within +0.3 ms.
- **T-D2-07 · L6 · replay gate.** A skipped event's `noticed` is empty on every dataset.

### D3 · The `skill` kind

- **T-D3-01 · L1 · model and validation.** `Step::Skill` round-trips through YAML; `shape()` is
  `skill`; `SHAPES.len() == 8`; validation accepts `release-checklist`, refuses `../x`, a
  65-character name, slots, and `Inhibit`.
- **T-D3-02 · L1 · rendering and reading back.** `route_text` renders the fixed template;
  `render::named` on that body returns `skills: ["release-checklist"]` and `all: false`.
- **T-D3-03 · L1 · follow.** A `Batch` with a `Skill { skill: "release-checklist" }` call within
  the window judges `Followed`; a different skill does not.
- **T-D3-04 · L1 · the bridge.** A `Control { tool: "Skill release-checklist" }` step becomes
  `input::Step::Skill`; `Control { tool: "TodoWrite" }` is dropped as before.
- **T-D3-05 · L1 · learned.** Six synthetic sessions (prompt family, version bump, then the Skill
  call) compile to a `skill` route at prime with the family's intent trigger; four sessions do
  not; a session steered by the skill directive contributes no support.
- **T-D3-06 · L1 · every match site.** `cargo clippy --workspace --all-targets -- -D warnings`
  is clean (an exhaustive match left unmatched fails to compile; the list in the plan is the
  checklist).
- **T-D3-07 · L3 · golden.** A prompt step in `claude_scenario` delivers `kind="skill"`.
- **T-D3-08 · L6 · gates.** The whitelist in `replay_with_the_learned_index_gives_every_recorded_directive`
  names the new id; every other gate unchanged.

### D4 · `verified.json` and `SkillPrompt`

- **T-D4-01 · L1 · written, not digested.** `adage train` on the acme skill writes `verified.json`
  with one entry per candidate (`works`, `stale`, `not-replayed`) and a date; the pack's digest
  and `procedure.json` are unchanged by its presence.
- **T-D4-02 · L2 · annotated.** `adage hook claude SkillPrompt` with `skill: acme-work-item` in
  that repository prints `{"text": …}` holding the original text, the fixed heading and one line
  per step, ≤ 2,400 bytes added; another skill name prints nothing; the control arm prints
  nothing and spools `would_annotate`.
- **T-D4-03 · L7 · the mod.** `skill.prompt` with a stubbed answer returns the annotated text;
  with none, `next(e)`.
- **T-D4-04 · L9 · three entry points.** `/acme-work-item`, the Skill tool and a subagent
  preload each read the annotated text (S9 Q13).

### Cross-cutting

- **T-X-01 · L2 · fail open, binary.** For each new event (`TurnStep`, `SkillPrompt`) and each
  changed one: `ADAGE_TEST_FAULT=panic` → exit 0, empty stdout, one `fail-open.log` line; a
  9 MiB payload → exit 0, empty stdout; a payload missing `session_id` → exit 0, empty stdout.
- **T-X-02 · L7 · fail open, mod.** For every hook: stubbed `$.process.run` rejects, times out,
  returns exit 1, returns `{`, returns `{}` → the hook returns `next(e)`'s result unchanged;
  `turn.step` forwards every chunk; `ui.render` returns `next(e)`.
- **T-X-03 · L7 · no registration headless.** `session.start` with `isInteractive: false`
  registers no `ui.render` hook and starts no feed; the command hooks' classic observers still
  run.
- **T-X-04 · L8 · authority.** `VALIDATE.txt` lists no `tool.check` answer other than
  observation (the mod never returns a `decision`), no `deny` in `tool.call`, no `env.set`, no
  `prompt.submit` call, no `process.run` argv containing `review`, `delegate`, `install` or
  `config`; `check-plugin.mjs` fails if any appears.
- **T-X-05 · L2 · control arm, binary.** For every new output (`predict`, `SkillPrompt` text):
  `ADAGE_ARM=control` prints nothing and spools the `would_*` field.
- **T-X-06 · L7 · control arm, screen.** With fixture state of a control session and of a
  treated session with no match, every site's tree is byte-identical.
- **T-X-07 · L7 · both surfaces.** Every `ui.render` hook mounts on `terminal` and `desktop`
  from the same fixture state without a refused tree (the kit reports validation).
- **T-X-08 · L9 · hosts.** The host table (architecture §7) is filled by observation: hooks run
  and nothing draws in `-p`, the VS Code panel and a cloud session; `$.process.run` on the
  Desktop app; the module on Windows.
- **T-X-09 · L8 · drift.** CI pins the Claude Code version for `validate` and `test`; a bump is a
  reviewed change; `git log -p` of the public `claude-code.d.ts` is checked for each name
  `VALIDATE.txt` lists (S9 Q8's method).
- **T-X-10 · L2 · two plugins, one module.** `check-plugin.mjs` fails when `plugin/claude-code/
  hooks/*.ts` and `plugin/claude-code-exec/hooks/*.ts` differ other than the binary name.
- **T-X-11 · L3 · parity.** After every chunk, `adage-exec/tests/parity.rs` passes.

## 6. Test cases: the mod's surfaces (L7 unless marked)

Fixture state is a `$.state` snapshot per scenario: `empty`, `fired-1`, `fired-3`, `rewrite`,
`predicted`, `receipt`, `noticed-forming`, `noticed-review`, `skill-invoked`, `skill-verified`,
`lapsed`, `control`, `learning-waiting`, `learning-running`, `binary-missing`.

- **T-B2-01 · opening line.** `InfoNotice` with `empty` draws `open.ready`; with the bootstrap
  flag `open.bootstrap`; on `desktop` the same text as a toast (the kit records `$.ui.toast`).
- **T-B2-02 · prompt line.** `UserMessage` with `fired-3` draws three lines in delivery order
  under the engine's row; with `fired-1` one; with `empty` returns `next(e)`; `isExpanded: true`
  returns `next(e)`.
- **T-B2-03 · truncation.** At `bodyColumns: 40` the prompt line is cut at 39 columns plus `…`
  and keeps the glyph and route id.
- **T-B2-04 · tool markers.** `ToolUse` matching a `rewrite` row draws `tool.rewrite`;
  `predicted` draws `tool.predicted`; an unmatched `requestId` returns `next(e)`.
- **T-B2-05 · receipt card.** `ToolResult` with `receipt` draws two lines and no pytest output;
  the test names are listed and cut at 2,400 bytes.
- **T-B2-06 · spinner and turn line.** `Spinner` suffix present only when `fired-*`;
  `TurnDuration` summary only on `terminal` with counts > 0; zero counts return `next(e)`.
- **T-B2-07 · band cells and priority.** `band.main` cells appear per §5's table; at
  `bodyColumns: 60` cells drop from the right in priority order; with `empty` and green health
  the band returns `next(e)`; `hasSurvey: true` returns `next(e)`.
- **T-B2-08 · band second line.** `lapsed` adds `band.lapsed`; two attention items show the
  higher priority only (lapsed > review > version).
- **T-B2-09 · control is neutral (T-X-06).**
- **T-B2-10 · theme keys only.** Every `Text` colour in every tree is a key from `tokens.ts`'s
  roles; a literal hex fails the test.
- **T-B2-11 · ASCII twins.** With `ADAGE_ASCII=1` stubbed in `$.env`, every glyph is its twin.
- **T-B3-01 · pane tabs.** `/adage` opens the pane focused; `r` and `h` switch tabs; `l` back;
  Esc closes.
- **T-B3-02 · Live rows and inspect.** Rows from fixture feed lines; Enter on a row opens
  `adage-inspect` with the directive text; Esc returns with the row selected.
- **T-B3-03 · filter.** Typing `lookup` in the `Input` leaves rows containing it; Enter keeps
  the filter; an empty submit clears it.
- **T-B3-04 · footer hotkeys.** `p` toggles the paused label; `v` hides quiet rows; `G` scrolls to
  the end (`$.ui.scroll` recorded).
- **T-B3-05 · Reflexes read-only.** No Button in the tree has an `onPress` that calls
  `$.process.run`; `Copy command` calls `$.ui.copy` with the `review.verbs` command and toasts
  `toast.copied`; a remote surface's `isCopied: false` toasts the fallback.
- **T-B3-06 · Health fixes.** A check whose `fix` is `condition now` shows the Enter action; any
  other shows `Copy command`.
- **T-B3-07 · empty states.** Each tab's empty text per §6.4.
- **T-B3-08 · the feed child.** A stubbed `$.process.spawn` stream feeds rows; ending the stream
  restarts it with `--since <last seq>`; three failures show `pane.feed.stopped`.
- **T-B3-09 · commands.** Each `/adage` command's `command.run` returns the text in §7 and the
  right `$.ui.open` call; `off` and `on` run the binary with `off`/`on`; no other argv.
- **T-B3-10 · settings reload.** `register(on, options)` with `band: false` registers no
  `AbovePrompt` hook; `noticed: false` draws no noticed line; `pane: auto` opens at
  `viewport.columns ≥ 144` only.
- **T-D5-01 · noticed line.** `ToolGroup` with `noticed-forming` draws `noticed.forming` once;
  the same candidate in a later batch draws nothing; `noticed-review` draws `noticed.review`.
- **T-D5-02 · learner countdown and toasts.** `learning-waiting` shows `learning in 38 s`;
  `learning-running` shows `learning…`; a `learn` feed line with a strength change toasts
  `toast.learned.strength`; one with no change toasts nothing; one naming this session among the
  tipping ids toasts `toast.learned.formed`.
- **T-D5-03 · skill markers.** `skill-invoked` and `skill-verified` draw their markers; `/adage
  why` lists the verified steps.
- **T-U-01 · L13 · usability.** Per ux-design §14: three readers, four questions; pass when 3 of 3
  answer the first three and 0 of 3 identify the control session better than chance.

## 7. Live probes (L9, from S9)

Each probe has a script under `spikes/S9-claude-mods/`, a `results/` file and a pass criterion
in the spike: Q1 coexistence and dedup, Q2 latency, Q3 surfaces and silence, Q3b the feed and
keys, Q4 `$.tool.check` agreement, Q5 provenance and cost on disk, Q6 fail-open and reload, Q7
hosts, Q8 drift, Q9 the skipped request (10 of 10), Q10 the gate, Q11 routing, Q12 noticed on the
fast path and in a person's eyes, Q13 skills. A chunk that depends on a question (plan §3) does
not merge before the question's result is committed.

## 8. Evaluation protocol (L10–L12)

### 8.1 Metrics

| Metric | Definition | Source |
| --- | --- | --- |
| `requests` | `[DEBUG] [API REQUEST] /v1/messages` lines in the session's debug log, cross-checked against distinct assistant `message.id`s; a surplus is verified only when `0 ≤ dispatches − ids ≤ retries` | `heldout_analysis.py:233-249`, to be mirrored in `demo/run.py` |
| `predicted_steps` | `turn_step` spool records with `predicted` set | the session spool |
| `receipts_as_results` | `pretool` records with `receipt.fresh` answered by the mod | the session spool |
| `cost_usd`, tokens | `result.total_cost_usd` and the four counts per step | `stream-json` result; `ledger note` |
| `passed`, `hidden_passed`, `stale_generated` | `score.py` | the sandbox |
| `turns`, `tool_calls`, `turns_to_first_edit`, `dead_end_hits` | as today | `demo/run.py` |
| Hook p95 | `duration_us` | the spool |

### 8.2 Arms

| Arm | Hooks | Mod | Grants | Purpose |
| --- | --- | --- | --- | --- |
| `control` | on, `ADAGE_ARM=control` | off | none | the baseline with identical hook overhead |
| `adage` | on, treated | off | as the study (none, or E2 verify) | today's product |
| `mod` | on, treated | `--plugin-dir plugin/claude-code` | the study's plus `--predict` on the registry and test routes | the design |
| `mod-observe` | on, treated | on, `skills_invoke: false`, no predict grant | none | B2's identity check: must equal `adage` in model-facing output |

### 8.3 Development loop (E1, E2)

On the acme-billing sandbox and then the reflex-proof repositories: six held-out items × three
prompts × repeats, task-stratified permutation and bootstrap as `demo/report.py` computes them.
Thresholds (`docs/launch-gate-protocol.md:247-252`): D1 `requests(mod) ≤ 0.85 × requests(A0)`
and fewer in at least 4 of 6 blocks; D2 no more hidden-test failures than A0; D3 zero necessary
verifications skipped; D4 covered repeats in at most 2 of 6 positives. Report-only: tokens,
cost, wall time, `predicted_steps`, `receipts_as_results`, the NOEXEC ratio. Pass twice on
fresh seeds before E3.

### 8.4 Registered confirmation (E3)

Per `docs/launch-gate-protocol.md`: new evaluation groups registered before dispatch, three arms
in paired blocks, 20 blocks, safety controls first, the one-sided 95% bootstrap upper bound of
`sum(MOD)/sum(A0)` ≤ 0.90, quality `QUALITY_MARGIN = 1` and G5 governing, at most 2 excluded
blocks. The outcome, whatever it is, goes in `evals/reflex-proof/LAUNCH-STATUS.md`. Model
requests are never presented as GPU savings.

### 8.5 Savings in the product

The in-product estimate (`savings.rs`: Hajek IPW, winsorised, stratified bootstrap, the 10/30
gate) is unchanged in method; harness-priced lines enter it through A4. `savings_gate` guards
the method; a new synthetic population with `predicted` steps checks that a predicted step counts
as a model turn not spent.

## 9. CI and release gates

| Gate | Runs | Blocks |
| --- | --- | --- |
| `cargo fmt`, `clippy -D warnings`, `cargo test --workspace --exclude adage-eval --locked` | every PR | merge |
| `node scripts/check-plugin.mjs` (with the module rules), `test-stage-plugin.mjs`, `claude plugin validate --strict` ×3, **`claude plugin test` ×2** (pinned CLI) | every PR | merge |
| `plugin/test/session-start.test.sh`, `npm/test/launcher.test.mjs` | every PR | merge |
| `cargo test -p adage-eval --test gates` | weekly; required before merging D1, D2, C1, D3 | merge of those chunks |
| `hook_latency` and `learn_bench` on a release build, numbers pasted into the PR | before merging A1, C1, D1, D2 | merge of those chunks |
| `release_plugin_startup` (first start < 1,500 ms, ten warm starts < 200 ms) | scheduled; before a release | release |
| `cargo run -p adage-sim` fixtures current | every PR (`committed_fixtures_are_current`) | merge |
| S9 question results committed | before the chunk that depends on them | merge |
| Development thresholds D1–D4, twice | before E3 | the registered run |
| `cargo deny`, DCO | every PR | merge |

Flakes are not re-run to green: a failing test is a defect or a test to make robust
(`CONTRIBUTING.md`'s spirit, and the project's rule that a failing test is never an infra
flake). Evals are not part of PR CI (paid model calls); their numbers go in the PR.

## 10. Entry and exit criteria per milestone

| Milestone | Entry | Exit |
| --- | --- | --- |
| M0 Spike | — | Q1–Q13 findings committed; Q1, Q2, Q3, Q6 pass |
| M1 Foundation | — | T-A1 … T-A5 all green; goldens re-blessed with only the added step; latency numbers in the PRs |
| M2 See it | M0's four questions | T-B1 … T-B3, T-X-02 … T-X-07, T-X-10 green; `mod-observe` arm identical to `adage` in model-facing output on six demo items; usability T-U-01 |
| M3 Trust it | M1 | T-C1 … T-C3, T-X-01, T-X-05, T-X-11 green; T-C3-06 in the demo |
| M4 Skip it | M3, Q9 | T-C4 (sim both arms), T-C5; Q9 10 of 10; the `mod` arm runs end to end |
| M5 Notice | — | T-D1, T-D2, T-D5-01/02; Q12 |
| M6 Skills | — | T-D3, T-D4, T-D5-03; Q13 |
| M7 Evidence | M4 | T-E1 (metric present), D1–D4 twice, E3 recorded |

## 11. Defects and reporting

- Severity: **S1** blocks or alters the agent's work (a hook that does not fail open, a deny, a
  changed model request outside a grant); **S2** a wrong decision or wrong count (a prediction
  without a grant, a dedup miss, a candidate from a single call); **S3** a drawing defect; **S4**
  copy and polish. S1 and S2 block merge; S3 blocks the milestone; S4 is logged.
- Every eval and probe result is a `FINDINGS.md` in its folder with the script that produced
  every number (the spikes' rule); the PR links it.
- A golden change is called out in the PR description with the diff.

## 12. Risks to the testing itself

- **The engine's test kit stubs `$` and models no real hook latency**: L5 and L9 carry the
  latency and coexistence claims, not L7.
- **`claude plugin test` depends on a pinned CLI**; a CLI update is a PR of its own that
  re-runs L7, L8 and the drift check.
- **Headless runs load the mod** (`-p` runs hooks): every L10 run states which arm had the mod,
  and `mod-observe` exists to prove the mod's observation changes nothing.
- **The four held-out tasks are spent** (`LAUNCH-STATUS.md`): E3 needs new groups; E1 and E2
  use the acme-billing sandbox and development repositories only.
- **A person's reading is one person's**: T-U-01 uses three readers and reports verbatim.
