# The two phases of a step: a step held back is recorded as held back

The request in [live-fixes-2-notes.md §7](live-fixes-2-notes.md) ("the request to the runtime
owner"), built. Since `78d0934` the mod yields a predicted step only when Claude Code's permission
check (`$.tool.check`) allows every call unasked. The binary, though, committed the prediction the
moment it answered `TurnStep`: the `turn_step` record said `predicted`, the step counted against
`runtime.predict_cap` and the in-a-row rule, and the route was never offered again that session.
So a step the mod held back was recorded as made. Now the mod asks in two phases, as it does for
a Bash call (`crates/adage-hook/src/pretool/phases.rs`, [A1-A2-C5-notes.md](A1-A2-C5-notes.md)).
Built on `claude/mods-poc-m98u3o` at `78d0934`. Nothing is committed.

## The contract

`<bin> hook claude TurnStep`, the payload on stdin, exit 0 always. Beyond the step's fields
(`session_id`, `cwd`, `turn_id`, `index`, `model`, `ending`, `agent_id?`, `via: "mod"`,
`routine_model?`, `observe?`):

| Field | Meaning |
| --- | --- |
| `dry_run: true` | Answer the prediction and commit nothing. Wins over `verdict` in the same payload. |
| `verdict: { permission, route?, tool?, reason? }` | The person's permissions on the dry run's calls. `permission` is `"allow"`, `"ask"`, `"deny"` or `"none"`; any other value, a missing one, or a `verdict` that is not an object is `none`. `route` is the route the dry run answered. `tool` is the first call not allowed unasked, and `reason` the engine's sentence on it, kept when non-empty (cut to 64 and 512 bytes). |
| neither | The one-phase step: a command hook's, or a mod without the phases. Decided and committed at once, exactly as before. |

**Dry run.**

- *A step that would be yielded* (a prediction under the cap, treated arm, not observe mode) is
  answered `{"predict": {kind, route, track, calls}}`. Nothing is committed: no spool record, and
  the session state is written back as it was found, the arm's assignment included. So no bound
  counts the step, the route stays offered, and nothing is routed while the step waits. Asked
  again, the dry run gives the same answer.
- *A step that would yield nothing* needs no verdict: nothing to predict, the cap reached, the
  control arm, or observe mode. It is decided and recorded at once, exactly as the one-phase step
  is (`would_predict` and `predicted_at` in the control arm and observe mode, `would_route` /
  `routine`). The answer is `{}`, or `{"routine": true}`.

**Verdict.** The step is decided again under the state lock, with the verdict.

| Verdict | stdout | `turn_step` record | State |
| --- | --- | --- | --- |
| `allow`, the route found is `route` (or `route` absent) | `{"predict": …}` | as the one-phase step: `predicted`, `would_predict`, `predicted_at` | `predictions += 1`, `last_step_predicted`, `chained`, `predicted_routes` |
| `ask` / `deny` / `none`, the route found is `route` | nothing, or `{"routine": true}` when the step is routed | `would_predict`, `withheld: "permission"`, `permission: {decision, tool?, reason?}`; no `predicted`, no `predicted_at`; `would_route` / `routine` as when nothing is predicted | the route is appended to `SessionState::withheld_routes`; nothing else changes |
| any verdict, a different route found (the plan moved since the dry run) | nothing, or `{"routine": true}` | none, unless the step is routed (`would_route`) | unchanged |

A held step counts against neither the cap nor the in-a-row rule: the very next step may predict
another route. A held route is not offered again that session, but a different route can be. It
is not offered again even if a later edit re-arms the finish line: the session's permissions are
not expected to change mid-session. `withheld_routes` is rebuilt from the spool: each `turn_step`
record whose decision has `withheld: "permission"` adds its `would_predict.route`.

The mod yields only after an `allow` verdict, and only when the verdict's answer names the same
route and calls as the dry run's (`samePrediction`). Any other answer (nothing, a failed ask, a
different step) forwards the step as if nothing were predicted.

The record (spool snapshot, `turn_step: withheld`):

```json
{"arm":"treated","decision":{…,"permission":{"decision":"ask","reason":"This command requires approval","tool":"Bash"},"withheld":"permission",…,"would_predict":{"calls":[{"input":{"command":"make gen"},"tool":"Bash"}],"kind":"run","route":"regenerate-registry","track":"acme/finish-lines"}},"event":"TurnStep","kind":"turn_step",…}
```

## What was built

- **adage-runtime.**
  - `StepPhase { Commit (default), DryRun, Verdict { route, permission } }` on `Policy::step`.
  - `Permission { Allow, Ask, Deny, None }` and `PermissionVerdict { decision, tool, reason }`.
  - `Withheld::Permission`.
  - `Decision::withheld` and `Decision::permission`, each skipped when empty.
  - `SessionState::withheld_routes`, skipped when empty.
  - `prediction::predict_step` reads the phase after the cap check. A dry run whose step would be
    yielded sets `predicted` and returns before any state change. A verdict other than `allow`
    sets `withheld` and `permission` and holds the route. A verdict naming another route returns
    before `would_predict`. `eligible` skips `withheld_routes`. `on_turn_step` routes only when
    neither `predicted_at` nor `predicted` is set.
  - `rebuild` reads `withheld`.
- **adage-hook.**
  - `phase::step_phase(p)` reads the payload.
  - `respond` puts the phase on the policy (only for a `TurnStep`). For a dry run it takes a copy
    of the state before `arms.ensure`; when the decision has `predicted`, it writes the copy back
    and answers the prediction before the extension, the record and rendering.
  - The command hook's path is untouched: no `dry_run`, no `verdict`, `StepPhase::Commit`.
- **adage-exec.** No change: it reaches the step through adage-hook's `respond` with its grants.
- **The mod** (`hooks/step.ts`, `hooks/predict.ts`).
  - `predicted(ending)` asks the dry run (`dry_run: true`). After a prediction it checks the calls
    (`heldBy`), then sends `verdict: verdictOf(p, held)`. The verdict's answer decides routing.
  - On `allow` it yields only the answer that `samePrediction` matches.
  - A skill the session does not list is sent as `{permission: "none", tool: "Skill", reason:
    "This session does not list the skill"}`. No band line is raised for it, since it is not the
    person's permissions.
  - Observe mode sends only the dry run: the binary records it at once.
  - `TurnStepPayload` gains `dry_run?` and `verdict?`.
  - `StepVerdict`, `verdictOf` and `samePrediction` are in `predict.ts`.
  - The kit has `phasedStep(prediction)`: a world `answer` that answers each phase as the binary
    does.
- **Fast path.** Nothing new runs before the state lock. `step_phase` reads two keys only for a
  `TurnStep` past the grant check. The state copy is taken only on a dry run. A held route is one
  `Vec` scan in `eligible`. The mod's second ask happens only after a prediction (rare): a step
  with nothing predicted asks once, as before.

## Departures

- **The dry run that would yield nothing records at once.** §7 item 3 had the control arm and
  observe mode answer the dry run `{}`, with nothing recorded and no verdict. They yield nothing,
  so no permission decides whether a call is made. If nothing were recorded, the control arm's
  `would_predict` records, and its bounds, would disappear under the mod (the bounds count in both
  arms, T-C1-04). The same holds for a step that is only routed, or only past the cap. So a dry run
  commits when it would yield nothing, exactly as the one-phase step does. One consequence is an
  asymmetry: in a mode that holds a Bash step (`auto`), the treated arm holds it uncounted, while
  the control arm counts it as `would_predict`. Closing that needs the control arm's dry run to
  answer something the mod checks and sends a verdict for (a new answer shape, `would_predict`);
  that was left out.
- **The verdict's fields.** §7 named `tool` and `reason` beside `permission`, and the brief named
  `route`. All four are read, and only `permission` is required.
- **A moved plan** answers nothing and records nothing, like `PreToolUse`'s substitute verdict for
  a moved plan.

## Open

- `adage watch` (`adage-cli` `ui/feed.rs`) shows a record with `would_predict` as "would choose";
  a withheld step shows that way too, since the feed does not read `withheld` yet. That file is
  another owner's.
- A mod with the phases sent to a binary without them would commit both asks; they ship together
  in one plugin `dist/`, so this does not arise.
- `TURN_STEP_TIMEOUT_MS` (250 ms) applies to each of the two asks. A verdict that times out after
  the binary committed would leave a `predicted` record for a step not yielded; that is no worse
  than before, and rare.

## Tests

| Case | Test |
| --- | --- |
| T-C4-08, the runtime | `tests::turn_step::a_dry_run_answers_the_prediction_and_commits_nothing`, `a_dry_run_that_would_yield_nothing_commits_as_before`, `an_allow_verdict_commits_as_before`, `a_verdict_other_than_allow_withholds_the_step_and_its_route`, `a_withheld_step_is_routed_as_when_nothing_is_predicted`, `a_verdict_for_a_moved_plan_decides_no_prediction`, `held_routes_survive_a_rebuild` |
| T-C4-08, the hook | `phase::tests::a_steps_phase_is_read_and_only_allow_allows`; `respond::step_phase_tests::{the_dry_run_answers_and_the_allow_verdict_commits_as_before, a_verdict_other_than_allow_records_the_step_withheld, a_held_route_is_rebuilt_from_the_spool, a_dry_run_that_yields_nothing_records_as_before, a_verdict_for_a_moved_plan_records_nothing}` |
| T-C4-08, the spool | `record::snapshot_tests::every_record_kind_as_the_spool_holds_it`: a `turn_step: withheld` section, three added lines and nothing else changed |
| T-C4-08, the mod | `step.test.ts` "T-C4-08 · the two phases…" (no verdict without a prediction; the held verdict's fields; a held step routed; allow with an empty, failed, throwing or different answer not yielded; an unlisted skill sent as `none` with no band line; observe sends only the dry run; `verdictOf`, `samePrediction`), and "T-C4-08 · onStep through the engine…" (default mode: the dry run then `allow`, yielded; auto mode: `ask` with the engine's sentence, nothing yielded, the request made) |
| Existing, updated | T-C4-01 (the two payloads), T-C8-02 and T-C4-07's finish line (three asks: the dry run before the request, the ending dry run, its verdict), the engine's T-C4-01 (both asks' argv, env, timeout, cwd, stdin), `session.test.ts`'s activation (four hook runs: the probe, Standing, the two phases) |

The goldens (`crates/adage-cli/tests/hooks.rs`) and `crates/adage-exec/tests/parity.rs` are
unchanged and green: neither sends `dry_run` or `verdict` with a `TurnStep`.

## Docs

`docs/spool.md` (the decision's `withheld` and `permission`, the two-phase record rule, rebuild's
read); `docs/how-it-works.md` (the predicted-call paragraph); `docs/claude-mod/test-plan.md`
(T-C4-08, the C4 row); `docs/claude-mod/implementation-plan.md` C4 (the files' contract, tests);
`live-fixes-2-notes.md` §7 (a pointer here); `CHANGELOG.md` (The Claude Code mod).

## Files touched

- `crates/adage-runtime/src/{decision.rs, lib.rs, policy.rs, prediction.rs, rebuild.rs, state.rs, tests/turn_step.rs}`
- `crates/adage-hook/src/{phase.rs, respond.rs, respond/step_phase_tests.rs (new), record/snapshot_tests.rs, record/snapshots/adage_hook__record__snapshot_tests__every_record_kind.snap}`
- `plugin/claude-code/hooks/{step.ts, predict.ts, step.test.ts, session.test.ts, fixtures/kit.ts}`
- `docs/spool.md`, `docs/how-it-works.md`, `docs/claude-mod/test-plan.md`,
  `docs/claude-mod/implementation-plan.md`, `docs/claude-mod/impl/live-fixes-2-notes.md`, this
  file, `CHANGELOG.md`

## Commands and results

Offline, every one green:

- `cargo fmt --all -- --check`: clean.
- `cargo clippy -p adage-runtime -p adage-hook -p adage-exec --all-targets -- -D warnings`: clean.
- `cargo test -p adage-runtime -p adage-hook -p adage-exec -p adage-cli --test hooks
  --no-fail-fast`: 25 passed, goldens unchanged. `--test hooks` selects only adage-cli's target, so
  the three crates were also run whole with `cargo test -p adage-runtime -p adage-hook -p
  adage-exec --no-fail-fast`. Results: runtime 133 + 1; hook 70 + 2 (wire); exec 142 lib, and its
  integration tests (parity 25, cli 9, completion_cli 4, e1 2, e1_private 3, escape 13, no_spawn 2,
  replay 2, stage 3).
- `cargo test -p adage-cli --test harness_sim --test mod_surfaces`: 10, and 11 (1 ignored).
- `cargo test -p xtask`: 24.
- The snapshot was re-accepted with `INSTA_UPDATE=always`. Its `git diff` is the three added lines
  of the `turn_step: withheld` section, and nothing else.
- With the pinned CLI (`$S/cli288/node_modules/.bin/claude`, 2.1.288):
  - `plugin validate --strict plugin/claude-code` and `plugin validate --strict .` pass.
  - The validate output, made the way CI makes it (`sed "s#$PWD/##g"`), is identical to
    `VALIDATE.txt`: unchanged, since `step.ts` calls no new `$` API.
  - `plugin test plugin/claude-code`: 418 pass, 0 fail.
- `node scripts/check-plugin.mjs`: `plugin adage 0.1.0: ok`.

Live (Claude Code 2.1.292 on PATH). The runs were driven by `$S/tsp-live/one.py`, which is
`$S/wd-live/one.py` with these differences:

- The binaries are the debug build of this change (`cargo build -p adage-exec -p adage-cli`;
  `target/debug/adage` and `adage-exec` replace `dist/`'s, `adage-train` is kept, and `SHA256SUMS`
  is rewritten for all three).
- The plugin source is a copy of `plugin/claude-code` taken after this change (`$S/tsp-live/src`,
  the same tree as the repository's minus `dist/`).
- The home is a COPY of `$S/o0n/home-FL`, the finish-line home.
- `CLAUDE_CONFIG_DIR` is fresh.
- The sandbox is generated.
- The item is the finish line ("add `po_number` … do not run anything").

Nothing under `/tmp/adage-live*` was read or written.

| run | mode | requests | Adage's calls | denials / `is_error` / classifier lines | Stop blocks | result | `turn_step` record |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `runs/fl-auto` | auto | 4 | none | 0 / 0 / 0 | 1 | success | `would_predict: regen-after-schema`, `withheld: "permission"`, `permission: {decision: "ask", reason: "This command requires approval", tool: "Bash"}`, no `predicted`, no `predicted_at` |
| `runs/fl-default` | default | 4 | Bash `make gen` (`adage_regen-after-schema_…`), ok | 0 / 0 / 0 | 0 | success | `predicted: regen-after-schema`, `predicted_at: [turn, 2]`, as before |

- **Auto mode.** The session state after the run holds `withheld_routes: ["regen-after-schema"]`,
  with `predictions` 0 and `predicted_routes` empty. After the Stop gate's request, the model's
  next response ended the turn again with the finish line still unmet. The mod asked again, and
  the route was not offered: there was no second `turn_step` record and no call of Adage's.
- **Auto mode, replayed.** On COPIES of the run's home, with a corrupted state file to force a
  rebuild from the spool, an ending dry run for that session (`index` 5) was run with the debug
  `adage-exec hook claude TurnStep`. With the spool whole it answers nothing: the held route is
  rebuilt from the `withheld` record. With the `turn_step` frame cut out it answers
  `{"predict": {… "route": "regen-after-schema" …}}`. So the held route, and nothing else, keeps
  the step from being offered again.
- **Default mode.** The finish line was predicted and made exactly as before the change.
