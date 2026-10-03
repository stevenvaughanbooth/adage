# adage-exec's hook: structure

Item 3 of the Rust maintainability review (S3 in its refactor plan): `crates/adage-exec/src/hook.rs`
(3,164 lines, six jobs and 29 tests in one file) split into one module per job. Behaviour is
unchanged: the same functions with the same bodies, moved; repeated blocks folded into helpers
that do exactly what each copy did. Line references below are at the working tree on top of
`83b36bc`.

## Module map

`src/hook.rs` stays the module root: the `Extension` impl and the dispatch between the jobs.

| Module | What it holds |
| --- | --- |
| `hook.rs` (224 lines) | `Prefetch`, `PREFETCH`, `main`, `adage_beside`; `impl Extension for Prefetch` (every method one call into a job, `deliver` composing E1 and E2); three shared helpers: `label` (`track/route`), `executes_at` (an execute grant for this repository at a level), `run_context` (the executor's `run::Context` for a run the hook starts). Re-exports `predict_grants`, `predict_off`, `data_type` at their old paths. |
| `hook/grants.rs` | Which grants hold here: `session_texts` (the session contract and the lapse notice), `execution_contract` (the contract's reflex block), `exec_only`, `predict_off`, `predict_grants`, `verifications` / `current_verifications` (learned context checks under a current E2 grant), `edit_pattern`, and the person's lapse lines (`key_lapse_notice`, `unplanned_notice`). |
| `hook/prefetch.rs` | E1: `prefetch`, `data_type`, `withheld` (the E1 spool record), `payload`. |
| `hook/act.rs` | E2: `Acted` (with `place`: a text in its directive's slot or after the directives), `Candidate`, `candidates`, `directive_for`, `act`. |
| `hook/stop_gate.rs` | The finish-line gate: `stop_gate`, `gate_answer`, `MAX_STOP_GATES`, `GATE_OVERHEAD_MS`. |
| `hook/receipt.rs` | The `PreToolUse` receipt: `covering_receipt`, `receipt_result`, `RECEIPT_CHECK_MS`, `MAX_RESULT_BYTES`. |
| `hook/pytest.rs` | pytest, read from command lines and cache records only: `pytest_calls`, `pytest_command`, `pytest_call`, `covers`, `PytestCall`, `pytest_nodeids`, `test_evidence`, `failure_record`, `failing_cases`, `run_source`, the id checks (`valid_nodeid`, `valid_file_id`, `in_targets`, `test_function`), `read_record`, the cache paths and bounds. |
| `hook/testkit.rs` (`#[cfg(test)]`) | The tests' world: `Fixture` (a repository with the learned `fixture/context/predict-check` check and a home beside it), `pytest_fixture`, journal rows (`completed`, `applied`, `journal`, `record_receipt`), the Stop call (`stop`, `stop_by`), the pipeline (`hook`, `edit`), grants (`grant_predict`, `grant_finish_line`, `predict_key_by_hand`, `seed`), `failed_run`, `check_directive`. |

Each job's tests sit beside it in a `#[cfg(test)] mod tests`: `act` 6, `grants` 7, `pytest` 4,
`receipt` 6, `stop_gate` 6 (29, as before). `time.rs` gains `pub(crate) fn now_secs()`, which
`time::now` and the receipt card's `ago` share.

What was folded:

- `run::Context { … }` built three times (stop gate, act, prefetch) → `run_context`.
- `format!("{}/{}", track, route)` → `label`; the "execute grant for this repository at E1/E2"
  predicate (prefetch, act, receipt) → `executes_at`.
- `pytest_call(&command.split_whitespace().collect())` four times → `pytest_command`.
- The bounded read of a pytest cache record (`symlink_metadata`, not-found, regular file within
  4 MiB) in `failure_record` and `pytest_nodeids` → `read_record`; `pytest_nodeids`'s literal
  `4 << 20` is now `MAX_RECORD_BYTES` (the same value).
- The path rules shared by `valid_nodeid` and `valid_file_id` → `valid_py_file`.
- "Replace the directive if it is delivered now, else append" three times in `act` → `Acted::place`;
  "not run, keep the run's own record" twice → a `not_applied` closure.
- The lapse line built twice each in `act` and `prefetch` → `key_lapse_notice`, `unplanned_notice`.
- Tests: the journal `Receipt` literal (six copies), the `PreCall` literal (five), the planned
  request for the fixture route (five), the `Ctx` literal (two), the one-call batch (two), the
  edit payload (three), the finish-line execute grant (two) and the check's `Directive` (two)
  → `testkit`.

## How to add a grant mode, as it stands

A grant mode is `request::GrantMode` (`Execute`, `Predict`), part of `ApprovalKey`. A new mode
touches:

1. `request.rs`: the variant, its `as_str`, and `is_execute` if it may run anything.
2. `route.rs`: a planner for it (as `plan_predict` is for `Predict`), its arm in `plan_grants`
   (the planned key a grant is compared with), and its words in `Lapse::grant`, `Lapse::notice`
   and `reapprove_command`.
3. `policy.rs`: `Policy::permits` (as `permits_prediction` for `Predict`); `run::check` refuses
   anything that is not an execute mode before it spawns.
4. `main.rs`: `approve --mode`, `revoke` and `status` parse and print it; outside this crate,
   `adage review delegate` (`adage-app` review, `adage-cli` exec) writes it.
5. The hook: a query in `hook/grants.rs` beside `predict_grants` (filter by `a.key.mode`, plan the
   key again, check policy, honour any machine switch), exposed through the `Extension` method
   adage-hook calls for it. Every execution site already filters on `is_execute` (`executes_at`,
   `exec_only`, `current_verifications`), so a mode that never runs needs nothing in `prefetch`,
   `act`, `stop_gate` or `receipt`.

## How to add an execution path, as it stands

An execution path is a moment at which the hook itself runs an approved route. Today there are
four: E1 `prefetch` (from `deliver`), E2 `act` (from `deliver`), `stop_gate` (from
`Extension::stop_gate`) and, reading only, `covering_receipt` (from `Extension::pretool_context`).
A new one:

1. Gets its own module under `hook/` and one line in the `Extension` impl in `hook.rs` (or a new
   `Extension` method in adage-hook, if no event reaches it yet).
2. Finds its grant with `executes_at(a, repo_key, level)` and `label`, plans the request with
   `route::plan` under the grant's terms, and refuses when `ApprovalKey::for_request` no longer
   finds the grant (say so with `key_lapse_notice`).
3. Builds its context with `run_context`, then goes through `run::execute` (E1) or `stage::stage`
   and `journal::finish` (E2, with a `RunOrigin` of its own), never around them.
4. Checks `journal::pending` first and runs nothing while a worktree needs recovery; fits its run
   inside the hook's deadline as `stop_gate` does; records what it did, or why it withheld, in the
   event's `exec` spool records; and fails open (returns nothing) on any error.
5. Is tested with `testkit::Fixture` in its module, and through the whole pipeline with
   `testkit::hook` when it changes what the hook prints. With no grants the output must stay
   `adage hook`'s byte for byte (`tests/parity.rs`).

## Test ids

The tests that map to a test-plan case now carry its id in their names (6 of 29; the other 23
predate `docs/claude-mod/test-plan.md` and cite the review findings F1–F8 and dev runs in their
doc comments instead). Renames (old path `hook::prediction_tests::<old>`):

| Old name | New path |
| --- | --- |
| `pretool_context_needs_a_fresh_successful_receipt_in_this_session_covering_the_call` | `hook::receipt::tests::t_c3_01_pretool_context_needs_a_fresh_successful_receipt_in_this_session_covering_the_call` |
| `a_receipt_never_covers_a_call_from_elsewhere` | `hook::receipt::tests::t_c3_01_a_receipt_never_covers_a_call_from_elsewhere` (new id: T-C3-01's "a call outside the root, `fresh: false`") |
| `a_fresh_receipt_answers_the_repeated_test_run_and_meets_the_finish_line` | `hook::receipt::tests::t_c3_02_03_a_fresh_receipt_answers_the_repeated_test_run_and_meets_the_finish_line` |
| `predict_grants_name_only_current_permitted_switched_on_grants` | `hook::grants::tests::t_c2_03_predict_grants_name_only_current_permitted_switched_on_grants` |
| `turn_step_under_adage_exec_predicts_the_granted_finish_line` | `hook::grants::tests::t_c1_10_t_c2_08_turn_step_under_adage_exec_predicts_the_granted_finish_line` (new id: T-C2-08's hook half, `predict-off` stops `adage-exec hook claude TurnStep`) |
| `both_grants_on_one_route_keep_the_execute_paths` | `hook::grants::tests::t_c2_07_both_grants_on_one_route_keep_the_execute_paths` |

The other 23 keep their names under their new module paths. `docs/claude-mod/poc/P2-notes.md` and
`impl/C3-C7-C8-D2-C9-E-notes.md` name the old paths; they record their own commits and are left
as they are.

## Departures from the review's plan

- The plan has `hook/mod.rs`; the root stays `src/hook.rs` with its modules in `src/hook/`, so
  the history of `hook.rs` follows its remaining code.
- `verifications`, `current_verifications` and `edit_pattern` are in `grants.rs`, not
  `stop_gate.rs`: the session contract uses them too.
- The plan moves the contract's text to `adage_runtime::render::execution_contract`. adage-runtime
  belonged to another agent in this pass, so it is a private `grants::execution_contract` with the
  byte-identical format string; moving it to `render` is one function and one call.
- The review's other adage-exec items fall outside this crate (`predict_off` and the switch paths
  onto `adage_store::Paths`, S8; one `ago` in `adage_core::time`, S9) and are not done here. The
  one in it, the doc pointers, is: `cargo doc -p adage-exec --no-deps` had six warnings (a
  link to the private `act`, private links in `project_python.rs`, `route.rs`, `run.rs`, a stale
  `changes` link in `stage.rs`, and the ambiguous `env` in `lib.rs`) and now has none; only doc
  comments changed in those files.
- `#[allow(clippy::type_complexity)]` on `prefetch` is replaced by a `Prefetched` type alias.

## Merging main (origin/main at `1ea8377`)

Main changed `crates/adage-exec/src/hook.rs` after `fff4fa7`: `adage_core::CommandTable::load()`
now returns `&'static CommandTable`, so every `&CommandTable::load()` became `CommandTable::load()`
and every `&table` became `table` (hunks at lines of `fff4fa7`'s `hook.rs`: 91, 386, 456, 481, 775, 973, 1005, 1247,
1268, and the tests' 1418, 1462, 1541, 1626, 1876, 2011, 2240, 2584). That cannot compile on this
branch, whose adage-core still returns an owned table, so it is not applied in the tree. For the
merge: take this branch's `hook.rs` (main's hunks there all fall in code that moved out), then
apply the same change to the modules. The result is mechanical:

```bash
sed -i -E 's/&CommandTable::load\(\)/CommandTable::load()/g; s/&table\b/table/g' crates/adage-exec/src/hook/*.rs
cargo fmt --all
```

which edits exactly these sites (16 hunks; `hook.rs` itself needs none, since `run_context` already
takes `&CommandTable`):

| Main's hunk (line in `fff4fa7`'s `hook.rs`) | Now in |
| --- | --- |
| `session_texts` `plan_grants` (91) | `grants.rs` `session_texts` |
| `verifications` `plan_grants` (386) | `grants.rs` `verifications` |
| `stop_gate` `route::plan` and `Context.table` (456, 481) | `stop_gate.rs` `stop_gate` (`route::plan`, `run_context`) |
| `covering_receipt` `route::plan` (775) | `receipt.rs` `covering_receipt` (`unchanged`) |
| `act` `route::plan` and `Context.table` (973, 1005) | `act.rs` `act` (`route::plan`, `run_context`) |
| `prefetch` `Context.table` and `route::plan` (1247, 1268) | `prefetch.rs` `prefetch` (`run_context`, `route::plan`) |
| tests' `Ctx.table` (1418, 2011) | `testkit.rs` `Fixture::selected`, `act.rs` `prior_sessions_…` (`f.ctx(table, …)`) |
| tests' `route::plan` (1462, 1541, 1626, 1876, 2240, 2584) | `testkit.rs` `approve_with`, `request` (which `current_digest`, `failed_run` and the receipt tests now share) |
| (branch-only code, same change) | `grants.rs` `predict_grants` (`predict_classified`), `testkit.rs` `grant_finish_line`, `predict_key_by_hand` |

The same diff, ready to apply, was produced from these files with that sed and `rustfmt` under
the repository's `rustfmt.toml`. Main's changes to `approvals.rs`, `route.rs`, `main.rs`,
`classify.rs`, `project_python.rs`, `tests/*` and `examples/exec_latency.rs` do not touch lines
this change edits (the doc fixes in `route.rs` and `project_python.rs` are outside main's hunks).
Main's new `crates/adage-exec/AGENT.md` says "`hook.rs`'s unit tests learn tracks into a fixture
home" and lists `hook` as `Prefetch`; after the merge the first should read "`hook::act`'s unit
tests", and the `hook` line can name the modules above.

## Commands and results

All in `/home/user/adage-poc`, after the gate file showed `Running`.

- `cargo test -p adage-exec -- --list`, before (the original `hook.rs` swapped back in) and after:
  167 tests both times. The 158 lines outside `hook::` are identical; the 29 `hook::` tests differ
  only by module path (`prediction_tests` → `act`, `grants`, `pytest`, `receipt`, `stop_gate`)
  and the six id prefixes above (checked by normalising both and diffing: identical).
- `cargo test -p adage-exec`: lib 111, cli 8, e1 2, e1_private 3, escape 13, no_spawn 2, parity 23,
  replay 2, stage 3, all passed. No golden touched, `ADAGE_BLESS` not used.
- `cargo clippy -p adage-exec --all-targets -- -D warnings`: clean.
- `cargo doc -p adage-exec --no-deps`: no warnings (six before).
- `cargo test -p xtask`: 18 passed.
- `cargo fmt --all --check` on the crate: clean.
- Public API: `grep -rn "^pub " src/` before and after. Every `pub` signature is the same; the
  three that moved (`predict_off`, `predict_grants` to `hook/grants.rs`, `data_type` to
  `hook/prefetch.rs`) are re-exported from `hook.rs`, so `adage_exec::hook::{predict_off,
  predict_grants, data_type}` resolve as before (and rustdoc lists the same seven items under
  `hook`). The only added lines are the two `pub use`.
