# P2 · The `predict` grant in adage-exec and the CLI (POC of chunk C2, with A6's `version`)

The grant that lets `adage-exec hook claude TurnStep` predict (plan §2.2 item 3, chunk C2). P1
built the runtime half (`Policy.predict_grants`, `Extension::predict_grants`, asked only at a
`TurnStep`); this POC fills the extension in adage-exec, the approval store, policy, the CLI, and
the `adage-exec version` verb the mod probes (P3 open issue 1).

## What was built

- **`GrantMode { Execute, Predict }`** (`adage-exec/src/request.rs`; re-exported as
  `adage_exec::approvals::GrantMode`). Serde lowercase, default `Execute`, and omitted when
  `Execute` (`skip_serializing_if = "GrantMode::is_execute"`) on `ApprovalKey.mode`,
  `Revocation.mode`, `ExecRequest.mode` and `route::Lapse.mode`, so every file and JSON written
  before reads and writes the same bytes. `HARNESS_PROFILE = "harness"`.
- **Approvals** (`approvals.rs`). `ApprovalKey::for_request` takes `mode` from the request and, for
  `Predict`, writes `profile_id: "harness"` instead of a profile hash. `diff` names `mode`. `mode`
  joins every selector: `approve` (replaces only the same repo/route/level/mode, clears only that
  mode's revocation), `revoke_automatically(repo, route, level, mode, cause)`, `lapsed`, `find`
  (key equality), and `revoke(repo, route, mode: Option<GrantMode>)` (`None` = every mode, what
  `adage-exec revoke` keeps doing). `auto_eligible` refuses a predict request.
- **Planning** (`route.rs`). `plan_predict(ir, root, tracks)`: the route's command template, its
  slot contracts (a pattern, or `REPO_PATH_CONTRACT` for a repository path), the scripts it names
  (hashed now, via the same `sources` helper `plan` now uses; pytest selection paths under the
  project's interpreter are skipped, as for E2), `effect` = the route's, `level` E2, typed output
  with no fields, no action, no project-Python identity, `mode: Predict`. Refused for a non-`Run`
  step and for `ir.exec_only()` (a vendored script or a learned action with execution evidence).
  `plan_grants` plans each grant on its mode's path; `lapses` groups by route, level **and mode**;
  `Lapse::grant()` is `E1`/`E2`/`predict`; `Lapse::notice` says "the predict approval for … so
  Adage does not make its call in the agent's place"; `Lapse::what` words `mode` as "what it
  grants"; `reapprove_command` for a predict grant is `adage review delegate <track> <route>
  --predict` (no `--level`: the flags are exclusive).
- **Policy** (`policy.rs`). `permits` on a predict request also runs `permits_prediction`:
  `max_level < E2` refused ("policy caps execution at E1; a call made in the agent's place needs
  E2"); the request's effect above Regenerate refused; for an unscripted command, a classified
  effect above Environment refused and a classified Environment refused without the managed
  `allow_environment_actions`. `effects_deny`, vendored-script and publisher checks are the shared
  ones (they apply to `[req.effect, classified]`).
- **Never run** (`run.rs`, `hook.rs`). `run::check` refuses a predict request outright. In the
  hook every execution site reads execute grants only: `exec_only` (`hook.rs` ~195),
  `current_verifications` (session contract and Stop gate), `covering_receipt` (~774), the E2
  action selection in `act` (~931), the E1 prefetch grant (~1230). `stage.rs`'s automatic
  revocation revokes the execute mode only.
- **`Extension::predict_grants`** (`hook.rs`). Returns nothing while `$ADAGE_HOME/exec/
  predict-off` exists (`hook::predict_off(paths)`, checked first). Otherwise resolves the checkout
  (`predict_root`, below) and calls `hook::predict_grants(repo_key, root, index, paths)`: each
  approval with `mode == Predict` for the repository whose route is in the index, whose route holds
  **no execute grant**, whose key equals `ApprovalKey::for_request(plan_predict(..))` now, and that
  `policy.permits` (with `classify::effect` on the template argv). Returns `(track, route id)`
  pairs, deduplicated. Store and policy are read only when a predict grant exists, and the
  command table only then.
- **`adage-exec version`** (`main.rs`): `adage-exec 0.1.0\n` on stdout, exit 0, handled before
  argument parsing and before anything is read.
- **CLI** (`adage-cli`). `cli.rs`: `review delegate … --predict` (`--level` now
  `required_unless_present = "predict"`; `--predict` conflicts with `--level`, `--effect`,
  `--outputs`, `--cache`, `--raw`, `--fields`, `--exit-when`, `--timeout-ms`: usage errors, exit
  2); `review revoke … [--predict]`; `delegate predict <on|off>`. `exec.rs`:
  `delegate_predict` (person-only through `review`'s existing `require_person`; refuses when the
  route already holds an E1 or E2 grant, "… predicting the same call too would run it twice";
  `plan_predict`; `permits` with the E2 opt-in hint; shows argv, inputs, effect vs classified,
  scripts, evidence and attestations; no preview; records `basis` from the route's evidence, no
  `terms`); `predict on|off` (off from anywhere, on via `require_person`); `status --json` gains
  `mode` per grant and a top-level `predict: "on"|"off"`; human status prints a predict grant as
  `predict · in the agent's place`; `print_lapsed` uses `Lapse::grant()`; `review revoke` without
  `--predict` withdraws the execute grants only, with `--predict` the predict grant only.
- **Other consumers** read execute grants only: `adage-app/src/review.rs` (E2 "granted" in the
  review queue) and `adage-cli/src/ui/routes.rs` (the routes view's execution column, `has_e2`).
- **Docs.** `docs/execution.md`: `adage-exec version` under "Commands", and a "Predicted calls"
  section between "Approvals" and "Policy".

## The approval-file shape (for fixtures)

`$ADAGE_HOME/approvals.json`, version 1. A predict grant is an ordinary approval whose key has
`"mode": "predict"`, `"level": "e2"`, `"profile_id": "harness"`, `"output_mode": "typed"`, and no
`terms`. An execute grant and a predict grant for the same route sit side by side:

```json
{
  "version": 1,
  "approvals": [
    {
      "key": {
        "repo": "acme-billing",
        "route_exec_digest": "<64 hex: see below>",
        "argv_templates": [["python3", "scripts/wi-lookup", "{id}", "--json"]],
        "scripts": [["scripts/wi-lookup", "<sha256 hex of the script's bytes>"]],
        "effect": "observe",
        "level": "e2",
        "output_mode": "typed",
        "profile_id": "harness",
        "signer": null,
        "mode": "predict"
      },
      "route": "acme-billing/work-items/lookup-work-item",
      "approved_on": "2026-10-03T00:00:00Z",
      "basis": { "support": 9, "opportunities": 10, "days": 3, "by_origin": {}, "effect_evidence": null },
      "auto": false
    }
  ]
}
```

`route` is `<track name without @version>/<route id>`; `repo` is the repository key the index
uses. `route_exec_digest` for a local route is SHA-256 of the canonical JSON
`{"argv": <template>, "contracts": {<slot>: <regex>}, "effect": <effect>, "output": {"mode":
"typed", "fields": {}}, "scripts": [{"path", "sha256"}…]}` (`ExecRequest::exec_digest`); for a pack
route, the pack's digest. Do not hand-compute it: in Rust, seed with

```rust
let req = adage_exec::route::plan_predict(ir, &root, &paths.tracks()).unwrap().request;
let key = adage_exec::approvals::ApprovalKey::for_request(&req, &ir.repo);
store.approve(Approval { key, route: format!("{}/{}", ir.track, ir.route.id), approved_on: "…".into(),
    by: None, basis: serde_json::Value::Null, auto: false, terms: None });
```

and set `[exec]\nmax_level = "E2"` in `$ADAGE_HOME/policy.toml`. From a shell:
`adage review delegate <track> <route> --predict` (needs `ADAGE_ALLOW_NONINTERACTIVE=1` and no
agent marker in the environment). A fixture must not also hold an E1/E2 grant for that route, or
`predict_grants` returns nothing for it. The step must be asked with the working directory inside
the checkout (see open issue 1).

## How this differs from the plan

1. **The root comes from the working directory.** `Extension::predict_grants(repo_key, &Index,
   &Paths)` (P1's signature, in adage-hook, which another agent is editing) gets no root, and the
   key check needs one (scripts are hashed in the checkout). `predict_root` takes
   `std::env::current_dir()`, its `find_root`, and accepts it only if `repo::lookup_key` gives the
   same `repo_key`; otherwise nothing is predicted (fail closed). P3's bridge runs the binary with
   `cwd = $.session.cwd()`, so this holds for the mod. Unit tests set a `#[cfg(test)]`
   thread-local (`TEST_CWD`) instead, because the process's directory is shared by test threads.
   The cleaner fix is passing `root` through the trait (one-line change in adage-hook's
   `respond`, which already has it).
2. **A predict grant is withheld while the route holds any execute grant**, at hook time as well
   as refused at `--predict` time. The plan names only the CLI refusal; without the hook-time rule
   a predict grant written first and an E1/E2 grant approved later would both act. Approving E1/E2
   on a route that has a predict grant is not refused (the plan does not ask for it); the predict
   grant simply stops predicting, and `status` still lists it.
3. **`review revoke` is per mode.** Without `--predict` it withdraws the E1 and E2 grants (all
   grants of the route before this change, which were all execute); with it, the predict grant.
   `adage-exec revoke` still withdraws every mode.
4. **The environment clause of `permits`.** Read as: the route's effect at most Regenerate; an
   unscripted command the table classifies as Environment needs `allow_environment_actions`;
   above Environment never. A scripted route's classification (always Environment, any path as the
   program) is not held against it, as for E2 actions; `effects_deny` still applies to both the
   declared and the classified effect (so `deny = ["environment"]` refuses every scripted predict,
   as it refuses every scripted E1 lookup today).
5. **The predict key carries no project-Python identity** (the agent's environment runs the call)
   and no output fields; a changed interpreter environment does not lapse it.
6. **`exec_only` routes are refused in `plan_predict`**, not in `permits`: `permits` sees a request,
   not the route.
7. **Added beyond the plan:** `Revocation.mode`; `Lapse.mode` and `Lapse::grant()`; top-level
   `predict` in `delegate status --json`; `run::check` refusing predict requests.
8. **T-C2-07** cannot use the CLI to write both grants (it refuses); the test seeds them directly,
   with the predict grant first in the store so a mode-blind lookup would pick it.
9. **`docs/claude-mod/architecture.md` §3.2** was not edited.

## Tests

| Test | Where | Covers |
| --- | --- | --- |
| `execute_and_predict_grants_are_kept_apart` | `adage-exec/src/approvals.rs` | T-C2-01: distinct keys, `diff` names `mode`, approve/re-approve, `lapsed`, `revoke_automatically`, revocation clearing, `revoke` per mode, never automatic |
| `stores_written_before_modes_load_as_execute` | `approvals.rs` | T-C2-02: an old file (approval and revocation) loads as execute and writes back without `mode`; a predict grant round-trips with `"mode": "predict"` |
| `a_prediction_needs_e2_and_an_allowed_effect` | `adage-exec/src/policy.rs` | T-C2-03 (policy): E1 refuses, E2 permits, effects above regenerate, `effects_deny`, environment with/without the managed opt-in, a scripted lookup |
| `a_predict_grant_plans_on_its_own_path_and_lapses_on_a_script_change` | `adage-exec/src/route.rs` | the predict key (harness profile, no action/fields), `plan_grants`/`lapses` with a script change, reapprove `--predict`, notice wording, exec-only refused |
| `predict_grants_name_only_current_permitted_switched_on_grants` | `adage-exec/src/hook.rs` | T-C2-03 (hook): granted pair; via the trait from inside/outside the checkout and for another key; kill switch; E1 policy; denied effect; lapsed command (and its notice); an execute grant on the route |
| `turn_step_under_adage_exec_predicts_the_granted_finish_line` | `hook.rs` | end to end through `adage_hook::handle` with `PREFETCH`: nothing before the edit, the edit's directive, nothing while `predict-off`, then exactly `{"predict":{"kind":"obligation","route":"regenerate-registry","track":"fixture/finish","calls":[{"tool":"Bash","input":{"command":"make gen"}}]}}`; no journal entry and no `exec` spool record |
| `a_predict_grant_alone_runs_nothing` | `hook.rs` | a predict grant (seeded by hand) on the learned context check: no action, no prefetch, no exec-only reflex, no session contract, no Stop gate run |
| `both_grants_on_one_route_keep_the_execute_paths` | `hook.rs` | T-C2-07: both grants (predict first) on `make gen`: no lapse notice; the E2 action at `PostToolBatch` runs (`Adage ran \`make gen\``, `gen/currencies.txt` written, one receipt; staging available here); on the pytest check with both grants, the session contract and `covering_receipt` still find the execute grant |
| `version_is_one_line_and_reads_nothing` | `adage-exec/tests/cli.rs` | `adage-exec version`: one line, exit 0, nothing on stderr, with an unparseable policy file present |
| `a_predict_grant_is_a_persons_decision` | `adage-cli/tests/exec.rs` | T-C2-04: each `adage_app::person::AGENT_ENV` variable refuses (exit 1, names it, writes nothing); at a terminal the key is `mode: predict`, `profile_id: harness`, level e2, script pinned, `basis` from evidence, no `terms`, no preview |
| `a_predict_grant_round_trips_through_status_lapse_and_revoke` | `adage-cli/tests/exec.rs` | T-C2-05: status `mode: predict`, `predict: on`, human line; script change → `lapsed[0].mode == predict`, reapprove ends `--predict`; `revoke` withdraws 0, `revoke --predict` withdraws 1 |
| `predict_is_exclusive_and_refused_where_it_would_not_hold` | `adage-cli/tests/exec.rs` | T-C2-06: `--predict --effect verify` and `--predict --level e2` exit 2; E1 policy refused with the opt-in hint; an existing E1 grant refused naming the double run; exec-only and environment-effect routes refused |
| `the_predict_switch_is_off_from_anywhere_and_on_by_a_person` | `adage-cli/tests/exec.rs` | T-C2-08 (CLI): `predict off` under `CLAUDECODE=1` writes the marker, status says `off`; `on` refused in a session, works at a terminal. The hook side of T-C2-08 is in the two `hook.rs` tests above |
| `crates/adage-exec/tests/parity.rs` (unchanged) | | T-C2-09: `adage-exec hook` with no grants equals `adage hook` on every golden scenario (19 pass) |
| `delegate_takes_subcommands` (extended) | `adage-cli/src/cli.rs` | `delegate predict off` parses; `delegate predict maybe` does not |

A mutation check: dropping the `is_execute()` filter from the E2 action selection makes
`turn_step_under_adage_exec_predicts_the_granted_finish_line` and
`both_grants_on_one_route_keep_the_execute_paths` fail (restored).

T-X-11 is retired by A8 and lives in `plugin/` (another agent's area); its binary half, a `version`
that prints one line and exits 0, is the test above. No golden changed.

## Commands run (in /home/user/adage-poc)

| Command | Result |
| --- | --- |
| `cargo fmt --all` | done; `cargo fmt --all -- --check` exit 0 |
| `cargo clippy -p adage-exec -p adage-cli --all-targets -- -D warnings` | first two runs failed on `crates/adage-cli/tests/harness_sim.rs:203` (`variant Exec is never constructed`, another agent's file in progress); a run over every other target was clean; the final full run: exit 0, no warnings |
| `cargo test -p adage-exec` | all pass: lib 106, cli 7, e1 2, e1_private 3, escape 13, no_spawn 2, parity 19, replay 2, stage 3 |
| `cargo test -p adage-cli --no-fail-fast` | all pass: unit 108, exec 12, actions 3, first_run 11, fresh_install 12, g4_hook 1, gaps 2, graph 5, harness_sim 9, hook_intent 4, hooks 19, instincts 10, live_learn 9, packs 9, repo_keys 2, review 7, sense 3, serve 2, share 6, sources 8, state_rebuild 3, train_accept 1, watch 4, zero_config_plugin 11. No golden changed (no `ADAGE_BLESS`) |
| `cargo test -p xtask` | 18 passed (layering holds) |

An earlier `cargo test -p adage-cli` run failed in `fresh_install::l5_doctor_flags_plugin_and_settings_registered_together`
with the disk full (100%, 144 MB free); after freeing space it passed. The disk filled twice during
this work. To keep building I deleted, in the shared `target/debug`, older duplicate test
executables (cargo relinks a missing one) and incremental-cache directories not modified for 20
minutes; nothing else. One build with `CARGO_INCREMENTAL=0` made a second set of artifacts and is
part of why the disk filled; not repeated.

## Open issues

1. **Root resolution by working directory** (difference 1). Not exercised against the real binary
   in a subprocess (only in process, through the test seam). If Claude Code or the mod ever runs
   the binary outside the session's directory, nothing is predicted. Passing `root` through
   `Extension::predict_grants` would remove the seam and the dependency on cwd.
2. **Cost on every step.** With a predict grant present, each `TurnStep` reads the store and the
   policy, loads the command table, plans each predict grant (hashing its scripts) and classifies
   it. Not measured against the 250 ms budget; `hook_latency`'s `turn_step_predicts` scenario
   (P1 open issue 1) can now be written against `adage-exec`.
3. **The TUI routes view and the review queue do not show predict grants** (they read execute
   grants only); `delegate status` does.
4. **Two grants, one route:** approving E1/E2 after `--predict` silently stops the predict grant
   (difference 2). `delegate status` could say so.
5. **`effects_deny = ["environment"]` refuses every scripted predict grant**, as it refuses
   scripted E1 lookups. If predicting `./scripts/wi-lookup` under such a policy matters, the
   classified effect of a script should not be checked against the deny list for predictions.
6. **Pack routes:** `plan_predict` refuses a pack route whose pack cannot be read or that the pack
   binds at E0 only (the shared `sources` rule); no pack fixture tests a predict grant.

## Files touched

```
crates/adage-exec/src/request.rs
crates/adage-exec/src/approvals.rs
crates/adage-exec/src/route.rs
crates/adage-exec/src/policy.rs
crates/adage-exec/src/run.rs
crates/adage-exec/src/stage.rs        (revoke_automatically call)
crates/adage-exec/src/journal.rs      (ExecRequest literal in a test)
crates/adage-exec/src/hook.rs
crates/adage-exec/src/main.rs
crates/adage-exec/tests/cli.rs
crates/adage-exec/tests/stage.rs      (ExecRequest literal)
crates/adage-cli/src/cli.rs
crates/adage-cli/src/exec.rs
crates/adage-cli/src/ui/routes.rs
crates/adage-cli/tests/exec.rs
crates/adage-app/src/review.rs
docs/execution.md
docs/claude-mod/poc/P2-notes.md
```

`cargo fmt --all` formats the whole workspace. A `git status --short` taken before it and one
after list the same files, so it modified no file that was clean; a file another agent had already
modified could have been reformatted without showing there. An earlier
`cargo fmt -p adage-exec -p adage-cli -p adage-app` formatted those three crates whole.
