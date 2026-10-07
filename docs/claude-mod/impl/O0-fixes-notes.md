# O0 fixes: the binary side of the live per-event verification

What [O0-live-notes.md](O0-live-notes.md) found, fixed in `crates/adage-runtime`, `crates/adage-hook`
(but `autolearn.rs` and `session_start.rs`), `crates/adage-exec`, `crates/adage-trace`,
`crates/adage-app/src/harness_ledger.rs` and `crates/adage-cli/src/ledger.rs`. The live notes
number the defects 1–12; the brief numbered them differently. Both are given below.

| brief | live notes | what | status |
| --- | --- | --- | --- |
| 1 | defect 1 | a subagent's lookup armed a prediction in the main thread | fixed, tested |
| 2 | defect 2 | a refused predicted call was recorded as made | fixed, tested |
| 3 | defect 3 | a verification's result read as a regeneration | fixed, tested |
| 4 | defect 4 | the model takes a yielded call for its own | the other agent's (mod) |
| 5 | defect 5 | spool records carry no `agent` | fixed, tested, snapshot re-accepted (added lines only) |
| 6 (gap) | defect 8 | the lookup convention never reaches the Bash description | binary half built, behind an ask the mod must add |
| 7 (gap) | defect 9 | one 5.75 s SessionStart | cause confirmed (hashing), cached, measured |
| 8 | defect 10 | the ledger's `tool_calls` is 0 | binary half built; the mod half written below for its owner |
| 9 (gap) | defect 11 | a resumed session adds `session_start` records | reported only (below) |
| 10 | defect 12 | `.pytest_cache/**` refused `.pytest_cache/.gitignore` | fixed, tested |

## 1. A subagent's calls bind nothing the main loop is predicted from

**Cause.** One `SessionState` per session; the batch and failure arms bound `st.reads`,
`st.pending` and `st.obligations` from every call whatever its `agent`, and only a `TurnStep`
with an agent was skipped.

**Fix** (`crates/adage-runtime/src/decide/{mod,batch,failure}.rs`). `MainThread` is what the main
loop's steps are predicted from: `pending`, `reads`, `obligations`, `predicted_routes`. Before a
call with `Call::agent` set it is kept, and after the call it is put back; the call's own follow
judgements, read retirement and obligation meeting are skipped (they would judge the main loop's
directives by the subagent's work). A batch made only of subagent calls does not count the main
loop's follow windows down. What a subagent's call still does, as architecture §3.8 says it may:
it is observed (`observed`, `history`, the context alphabets), recorded (with `agent`, item 5), a
directive it triggers is said to the subagent (`emitted` grows as before, so the main loop is not
told the same route again this session), new tests it adds are kept, and the `PreToolUse` paths
(receipt, rewrite, substitution) are untouched.

**Departure.** A directive a subagent's call triggers carries no follow judgement (its pending entry
is what is put back). Keying pending state by agent would keep it; the brief asked that nothing the
main loop predicts from be bound, and a per-agent pending set is a wider state change than this
defect needs.

**Tests.** `crates/adage-runtime/src/tests/subagent.rs`:
`a_subagents_lookup_binds_no_prediction_in_the_main_thread` (the SUB run: the read directive is said,
nothing is predicted at the next step or the turn end; the main loop's own lookup still binds and
predicts), `a_subagents_calls_leave_the_main_loops_directives_and_obligations_alone` (no judgement,
windows not counted down, an edit arms nothing, a failure judges nothing).

## 2. A refused predicted call

**Cause.** `harness/claude.rs` built every `PostToolBatch` call with `failed: false`; a permission
refusal reaches no `PostToolUseFailure`, only the batch, as `<tool_use_error>…</tool_use_error>`.

**Fix.**
- `adage_trace::TOOL_USE_ERROR`, `is_tool_use_error(output)` and `Call::refused()` (failed, and the
  result leads with the marker).
- `call_from` marks a call whose result is a `<tool_use_error>` failed (batch entries included).
- The batch arm (`on_call`): a refused call is observed as `<key> !refused` (also in `history`),
  adds no tests, triggers nothing (no edit, command, context or failure trigger: nothing ran). Made
  by Adage (`By::Adage`) in the main loop, the pending directive or armed obligation it stood for is
  judged the new `Judgement::Refused { route }` (`"kind": "refused"`): a pending directive is
  retired, an obligation stays unmet (so the Stop request asks, as for a failed predicted call), a
  read batch is retired (`retire_reads`). The route stays in `predicted_routes`, so it is not
  predicted again this session. Never `Predicted`, `Followed` or `ObligationMet`.
- The agent's own refused call judges nothing and leaves its directive pending (it ran nothing;
  a later call may still follow it).

**Contract.** `decision.judgements[].kind` gains `refused`. Readers that match kinds by string
(`adage-learn` instincts, `adage-app` savings and graph) ignore an unknown kind, so nothing else
changes; `docs/spool.md` lists it.

**Not mine, now out of step.** `crates/adage-eval/src/transcript.rs` `events()` says it "mirrors
the Claude Code adapter: … a PostToolBatch whose calls are all marked not failed". The adapter now
marks `<tool_use_error>` calls failed. The eval's replay of a transcript with a refused call
therefore differs from what the live hook decides (the replay still observes it as made). The
one-line change for that file's owner: in the `PostToolBatch` calls, `failed: c.failed &&
c.output.trim_start().starts_with("<tool_use_error>")` (or `adage_trace::is_tool_use_error`).

**Tests.** `crates/adage-trace/src/lib.rs` `a_refused_call_is_a_failure_whose_result_is_the_refusal`;
`crates/adage-runtime/src/tests/refused.rs` (the DENY run's read; a refused finish line meets no
obligation and the Stop gate asks; a refused pending command; the agent's own refused call);
`crates/adage-hook/src/respond.rs` `a_finish_line_the_engine_refused_is_recorded_as_refused_and_never_met`
(through the adapter, with the DENY run's payload shape, and the spool record).

## 3. A verification's result

**Fix.** `render::action_verified(d, edited, command, Verified::{Passed { tests, duration_ms } |
Failed { exit, record }}, operation)`, one fixed template:

```text
Adage ran `.venv/bin/python -m pytest -q tests` after your edit to tests/test_calc.py: it passed (exit 0, 4.0 s; receipt 392a79fe8781). Nothing changed.
Tests: 3 collected; exit 0; the tests added in this session: test_add.
```

```text
Adage ran `…` after your edit to tests/test_calc.py: it failed (exit 1; receipt 392a79fe8781). Nothing was changed.
Failing tests: tests/test_calc.py::test_add.
```

In the directive's wrapper with `exec="e2"`, as `action_applied`. `adage-exec`'s `act` uses it for an
obligation's (not a context's) E2 action whose route's effect is `Verify`: `Applied` → passed, with
`test_evidence` (the counts the receipt has); `Failed` outside a Stop → failed, with
`failure_record`, in place of the directive, the records as before (`withheld: failed` and the
run's own). At a Stop the continuation is unchanged. `action_applied` (the regeneration) is
unchanged, pinned by the test.

**Tests.** `render::result_text_tests::a_verification_after_an_edit_says_it_passed_or_failed`;
`hook::act::tests::a_finish_line_verification_says_it_passed_or_failed` (staged end to end, pass and
fail, the obligation met only on a pass, the `withheld` record only on a fail).

## 5. `agent` on spool records

`Record::tool_use_id(p)` (the `pretool` records) and `Record::calls(ev)` (decided `batch` and
`failure`) also set `agent`: the payload's `agent_id`, or the first call's `Call::agent`. Absent for
the main loop; an id over 128 bytes is left out. The snapshot gains one section, "failure, batch,
pretool: a subagent's calls" (5 added lines, nothing else changed: `git diff --stat` on the `.snap`
is `5 insertions(+)`). The unrouted records (`session_start.rs`, not mine) carry no ids and no
`agent`.

## 6 (gap). The lookup convention in the Bash tool's description

**Decision.** Architecture §1b.3 puts "how things are done here" on the tool's description ("tests
here run with `make test`"); §3.6 has the description carry the environment note. The convention a
lookup route encodes belongs there too: one sentence per learned prompt route in play at full
strength whose step is a command with the item's slot, from a fixed template
(`render::lookup_convention`), slot left as written:

```text
In this repository, work item details are looked up with ./scripts/wi-lookup {item} --json, where {item} is the work item the request names.
```

Not an instinct's, not a route only primed, not one a session-start dead end avoids; at most three,
within `MAX_STANDING`. The prompt that names an item is **still told** the bound command: its
directive carries the item, the E1 prefetch and the chain's prediction, so the route is not added to
`Standing::routes`. T-C9-05 ("directives per session fall") therefore does not follow from this for
the lookup; it would only if the prompt's lookup directive were withheld, which would also withhold
its prefetch and prediction. That is a design call for the plan, not made here.

**Contract.** `Standing` (runtime, kept on the session state) gains `conventions: String`
(`#[serde(default, skip_serializing_if = "String::is_empty")]`), placed by `standing_for` only with
`tool_description`. The binary answers it only when the ask carries `conventions: true`:

```json
{ "session_id": "…", "cwd": "…", "hook_event_name": "Standing", "conventions": true }
→ { "standing": "…", "environment": "…", "compact": "…", "conventions": "In this repository, …" }
```

Without `conventions: true` the answer is byte-identical to before (the golden `### Standing` line
is unchanged). In control and observe mode it is `""`.

**For the mod's owner (`attach.ts`, `fixtures/wire.ts`, a test):** add `conventions: true` to
`standingPayload`, read `conventions` in `standingOf` (`str(answer.conventions)`), and append it in
the Bash `tool.describe` after the environment note
(`[standing.environment, standing.conventions].filter(s => s !== '').join('\n\n')`). Then add a wire
case to `crates/adage-hook/tests/wire.rs`:

```rust
("standing-conventions", ModAnswer::Standing { standing: String::new(), environment: String::new(),
  compact: adage_runtime::render::COMPACT_INSTRUCTION.into(), subagent: None,
  conventions: Some("In this repository, work item details are looked up with python3 scripts/wi-lookup {item}, where {item} is the work item the request names.".into()) }),
```

bless it (`ADAGE_BLESS=1 cargo test -p adage-hook --test wire`), run `node scripts/check-plugin.mjs
--wire`, and read `wire('standing-conventions')` in a mod test. I did not add the wire file: with no
mod test reading it, `check-plugin.mjs` would fail.

**Tests.** `crates/adage-runtime/src/tests/attachment.rs`
`the_lookup_conventions_are_the_learned_prompt_routes_at_full_strength` (and the capability split);
`crates/adage-hook/src/standing.rs`
`the_shell_tools_description_carries_the_lookup_convention_when_asked` (absent unless asked, fixed
for the session, the prompt still told, empty in control and observe).

## 7 (gap). The slow SessionStart: hashing, now cached

**Cause, confirmed.** With RV1's home (one E2 grant on `.venv/bin/python -m pytest -q tests`) and
repository, `target/release/adage-exec hook claude SessionStart` (the binary O0 ran) took 164–179 ms
warm and 317–327 ms after evicting the runtime's and the venv's pages (`posix_fadvise DONTNEED`);
the same home without `approvals.json` took 6 ms. So all of it is planning the grant, which
`project_python::identify` does by hashing the venv, the interpreter (28 MB) and the runtime's whole
`lib` tree (66 MB, 1,602 files outside `__pycache__`). 5.75 s once is that read on a busy disk.

**Fix** (`crates/adage-exec/src/project_python.rs`, `hook.rs`). A per-file digest cache,
`$ADAGE_HOME/cache/python-digests.json` (`{"files": {"<abs path>": {dev, ino, size, mtime, ctime,
sha256}}}`): a regular file whose `(dev, inode, size, mtime, ctime)` is unchanged is not read again
(the change time cannot be set back by the file's owner; a file that changed while it was read is
not kept). It serves `identify` only (the session contract, planning, `run::check`'s validation);
`prepare` hashes the live environment and the private copies the staged run executes without it, so
a forged entry can never get an unapproved environment run. Unreadable cache: empty; unwritable:
lost. Past 160,000 entries only the latest identification's files are kept. Written by a rename. The
hook enables it (`keep_digests_in`, once per process); nothing else does.

**Proof.** Debug build (release builds are not allowed here), the same home and payload, output
identical every run (md5 `a06da53d`):

| run | ms |
| --- | --- |
| no grant (baseline) | 20, 21 |
| uncached (cache file removed each time) | 2,287, 1,892 |
| first with the cache (fills it) | 2,021 |
| second, third | 69, 60 |
| after evicting the runtime's pages | 65 |

Unit test: `project_python::tests::a_second_identification_reads_no_unchanged_file` (second
identification reads 0 files and equals the uncached identity; a changed file is the only one read
and changes the identity; a corrupt cache is an empty one).

## 8. `tool_calls: 0` in the ledger

**Cause.** Both sides: the mod's note (`ledger.ts` `noteOf`) carries no count, and the binary's
merge (`Note::over`) took the count from a transcript summary only, so with `learn.auto` off (no
transcript pass) every line was 0.

**Binary fix** (`crates/adage-app/src/harness_ledger.rs`, not in either other agent's list).
`Note.tool_calls: Option<u32>`; `parse` reads an optional `tool_calls` (a non-negative integer that
fits u32, else the note is refused); `over` takes the note's count over a transcript's, the
transcript's without one, else 0. `crates/adage-cli/src/ledger.rs`'s contract comment lists it.
Test: `harness_ledger::tests::a_notes_tool_calls_are_kept`.

**For the mod's owner (`ledger.ts`, and wherever main-loop calls are seen).** Count the main loop's
tool calls and send them:

```ts
// mod.ts rt: turnCalls: number (0)
// call.ts, in the tool.call hook, before deciding: if (e.agentId === undefined) m.rt.turnCalls += 1
//   (every main-loop call: the model's, the predicted ones, and those answered by a receipt, as a
//   transcript lists them)
export type Noted = { session: string; steps: StepUsage[]; tool_calls: number }
export const NOTED0: Noted = { session: '', steps: [], tool_calls: 0 }
// accrue(noted, session, turn, calls): tool_calls: (noted.session === session ? noted.tool_calls : 0) + calls
// noteLedger: m.rt.noted = accrue(m.rt.noted, session, m.rt.turnSteps.splice(0), take(m.rt, 'turnCalls'))
// Note gains tool_calls: number; noteOf: ..., tool_calls: n.noted.tool_calls
```

The binary accepts the field today; a mod without it is recorded as before.

## 9 (gap). Resumed sessions add `session_start` records (report only)

`crates/adage-hook/src/session_start.rs` `record_start` (the other agent's file) writes one
`session_start` per `SessionStart`, and Claude Code sends `SessionStart` again on `--resume` and
after compaction (`source: resume`, `source: compact`), into the same session id and so the same
spool. The record carries no `source`, so a reader cannot tell a start from a resume:
`demo/adage_metrics.py` (`session_starts`) and any "exactly one `session_start`" check (evaluation
plan §2.1) count them all; `harness_ledger::check_session` and `adage savings` are unaffected (they
need one, or count sessions by id). The fix for that file's owner: `.with("source",
p["source"].as_str())` on the record (a field added, so the record snapshot gains it on the
Claude Code lines and `docs/spool.md` lists it), and count only `source` absent or `startup`.

## Departures

- **Item 10 reverses a documented decision.** `docs/execution.md` said "wildcards do not implicitly
  cover hidden files" and `stage.rs`'s `pytest_hidden_cache_metadata_needs_its_explicit_contract`
  pinned that `.pytest_cache/**` did not cover `.pytest_cache/.gitignore`. The brief asked for the
  opposite, and the producers already wrote the narrower form (`route::propose_outputs` proposes
  only `.pytest_cache/**`; the lapse notice suggests only `--cache '.pytest_cache/**'`), which is
  why the live grant was refused. The rule now: a leading dot is still matched only literally,
  except below the first dotted segment of the path when that segment is a directory and the glob
  names it as written. `**` matches no `.env`, `.git/config` or `.gitignore`; `gen/**` no
  `gen/.env`; `.pytest_cache/**` no `.pytest_cache2/…`. The explicit `.pytest_cache/.gitignore`
  declaration is still accepted (grants made before). The pinned test is renamed
  `pytest_hidden_cache_metadata_is_covered_by_its_directory_or_its_explicit_contract` and still
  asserts the repository's own `.gitignore` is refused; `docs/execution.md` says the new rule.
  `docs/launch-handoff.md` (history) and `docs/first-reflex.md` (its example still valid) are left.
- **Item 6** is built behind an opt-in field so the golden stays byte-identical; nothing changes live
  until the mod asks.
- **Item 8** is in `adage-app` (the binary side of the ledger lives there, not in
  `adage-cli/src/ledger.rs`).

## Live-only, not verified here

The subagent and DENY runs were not repeated in a real `claude`; the fixes are verified against the
recorded payload shapes (`agent_id` on the batch and its calls, the DENY batch's
`<tool_use_error>` response). The 5.75 s start was not reproduced (164–327 ms release, warm and
cold); the cache's effect was measured in a debug build.

## Commands and results

- `cargo fmt --all -- --check`: clean (my files formatted with `rustfmt` one by one).
- `cargo clippy -p adage-runtime -p adage-hook -p adage-exec -p adage-trace --all-targets -- -D warnings`:
  clean; `cargo clippy -p adage-app --lib --tests -- -D warnings`: clean.
- `cargo test -p adage-runtime -p adage-hook -p adage-exec -p adage-trace --no-fail-fast`: all pass
  (runtime 126 + 1, hook 64 + wire 2, exec 142 lib + cli 9, completion_cli 4, e1 2, e1_private 3,
  escape 13, no_spawn 2, **parity 25**, replay 2, stage 3, trace 7 + 3 + 1).
- `cargo test -p adage-cli --test hooks`: 25 passed, goldens unchanged (`git diff --stat
  crates/adage-cli/tests/golden` empty).
- `cargo test -p adage-exec --test parity`: 25 passed.
- `cargo test -p xtask`: 24 passed.
- Also run, as readers of what changed: `cargo test -p adage-app --lib` 50 passed;
  `cargo test -p adage-cli --test harness_sim --test state_rebuild --test mod_surfaces --test g4_hook
  --test hook_intent` all pass; `cargo test -p adage-sim` pass; `cargo test -p adage-eval --test
  predict_gate --test parity --test prop_render` pass.
- `ADAGE_BLESS=1 cargo test -p adage-hook --test wire`: no wire file changed.
