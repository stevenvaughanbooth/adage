# A declared instinct's dead end replaced before it runs (C6, T-C6-07)

The index-0 notes (`index0-notes.md`, "Live") closed on this: in every live W-2041 session, the two
responses the index-0 lookup saved went to the sandbox's dead end, `python3 -m unittest discover
-s tests -t . 2>&1 | tail -5`, and a retry. Nothing replaced that call. In the run-3 home it is a
declared instinct (U1/U3, bound from the Makefile), and `substitution` replaced only learned,
firm routes (P5 read the plan's "at fire" as learned and firm).

The coordinator decided to replace a declared instinct's `avoid` with a fixed, slot-free
`instead` exactly as a learned firm one is replaced. The same engine double verdict, the same
pipeline rule and the same context line apply. The line names the instinct's declared reason.
Forming, dormant and nascent learned routes stay excluded. Instincts still never block Stop and
never become obligations.

## What distinguishes an instinct's `avoid` in the runtime

| property | learned `avoid` | instinct `avoid` |
| --- | --- | --- |
| shape | `Step::Avoid`, or `Step::Run` after `AfterFailure` (`adage_tracks::shape` = `avoid`) | in practice only `Step::Run` after `AfterFailure` (`adage_instincts::bind::check_route` refuses any other step). An `avoid` step in a hand-written instinct track is treated the same |
| strength | inhibit / fire / prime, by evidence | always prime (`adage_tracks::io::validate`: "an instinct route is prime") |
| `delivered_strength` | its strength, prime while forming or dormant | always `Prime` (`model.rs`) |
| state | nascent → forming → firm, dormant, extinguished | bound `Forming` (environment effect: `Nascent` until approved). `adage review approve` keeps it `Forming` (`adage-app/src/review.rs`). It never firms |
| failure match | `Policy::meets`, or the bare-runner match for a key without positionals | the runner match always (`policy::failure_matches`: `r.is_instinct() \|\| bare()`) |
| signature | the failure text learning found | a declared signature (`ACME_ENV`, the wrapper's own command `make test`, `No module named pytest`), never empty |

So "firm" and "at fire" can never hold for an instinct. The rule says what stands in for them.

## The rule (`adage_runtime::substitution`, `crates/adage-runtime/src/pretool_decide.rs`)

A route replaces the call (or its first stage, under T-C6-06's pipeline rule, which is unchanged)
when it meets all of these:

- it is in play;
- it has no slots;
- it is an `avoid` step whose runners the stage runs, or a run step after a failure of the
  stage's key (the trigger's own match);
- its `instead` is fixed (no `{`), is not avoided, is not the call (`same_command`), and is not
  the call's own tool in another environment (`same_tool`, below);
- one of these holds:
  - **learned** (or imported, or authored): `state == Firm`, and for a run after a failure,
    `delivered_strength == Fire`. This is exactly as before;
  - **an instinct** (`Route::is_instinct`): `state` is `Forming` or `Firm` (in service). Nascent,
    dormant and extinguished are excluded. Strength is not checked: it is always prime, and the
    standing of an instinct is the declared fact it was bound on. The engine's verdict on both
    commands guards the run in either case.

The first such route in index order wins. In the run-3 home that is `dead-end-b8ac50` (U3,
`exec python3 -m unittest`, signature `make test`); U1's `dead-end-d41b1e` follows it.

**The context line.** The learned line is unchanged. For an instinct, the "because" clause gives
the declared reason instead of a failure, since none was seen here:

```text
adage ran make test for you, through the rest of your pipeline as written; python3 -m unittest discover -s tests -t . was replaced because this repository's Makefile declares make test in its place (this repository declares one test command). The output above is from make test.
```

The declaring file comes from `Origin::Instinct.declared` and is checked by `render::declared_ok`,
which is now `pub(crate)`. The same check guards the finish line's "`Makefile` declares this."
When no file is named, the line says "this repository's build". The reason is the route's summary
without its `Instinct U3: ` label. Everything shown passes `render::sanitise`, and the line is a
single line.

**Contract shape: unchanged.** The plan is `{"plan":{"substitute":{command, route, track,
context}}}` and the verdict answer is `{"substitute":{…}}`. The `pretool` spool record is
`substituted: true` with judgement `substituted`, or `would_substitute: true, withheld: true`.
`demo/adage_metrics.py` counts it with no change. The command-hook path never substitutes, and no
golden changed (`git status` shows none).

**Untouched.** Instincts still never block Stop and are never obligations. Neither
`decide/batch.rs` nor `rebuild.rs` nor `standing.rs` is edited, and the finish-line rule from the
live-fixes notes is as it was.

## Departure: `same_tool` (for learned routes too)

Admitting instincts brought in U11 (`adage/python-env`): after `python -m pytest` fails outside
the project's environment, run `.venv/bin/python -m pytest` (or `uv run pytest`). The hook's own
`u11_world` tests caught it. The dry run for `python -m pytest tests -q` started to plan
`substitute: .venv/bin/python -m pytest`. That replacement drops the call's arguments (which
tests), and it duplicates the interpreter rewrite (`crates/adage-runtime/src/pretool.rs`, U11),
which points the same call at the environment and keeps them (`.venv/bin/python -m pytest tests
-q`).

So `substitution` now refuses an `instead` whose action runner (`adage_core::action::classify_argv`)
is the call's own: `pytest` and `pytest`, as opposed to `make test` for `unittest`. That is the
rewrite's job. The check applies to learned routes as well as instincts, because the argument
loss is the same for a learned `python -m pytest` → `.venv/bin/python -m pytest` pair. No existing
test or replay had such a learned pair, and every suite listed below passes. It is stated in
architecture §3.4, in `execution.md` and in T-C6-07.

Instincts that can now replace a call, besides U1 and U3:

- P1 (pytest → the declared wrapper, or `tox -e py`);
- N1's `npm test` → `pnpm test`;
- N1's install pair (`npm install` → `pnpm install`), but only once a person approves it in
  `adage review`, because it is nascent until then. This mirrors the learned case: an approved
  learned environment route becomes firm and was already replaceable.

## Tests

- **Runtime, `crates/adage-runtime/src/tests/instinct_substitution.rs`** (new; one `mod` line in
  `tests/mod.rs`). It covers T-C6-07 with the U1 route as `--instincts-only` binds it:
  - plain → `make test`, with the exact line;
  - run 3's `-t . 2>&1 | tail -5` → `make test 2>&1 | tail -5`, with the exact line;
  - a list, a file redirection, a non-filter stage, `make test` and pytest are refused;
  - nascent, dormant and extinguished are refused, and firm is accepted;
  - a slotted `instead`, the call itself, another runner's failure, and U11's
    `.venv/bin/python -m pytest` and `uv run pytest` are refused;
  - no declaring file gives "this repository's build";
  - an instinct `avoid` step is accepted;
  - the decision is substituted and judged in the treated arm, and `would_substitute` in control
    and in observe mode.
- **Runtime, `crates/adage-runtime/src/tests/dead_end.rs`.** The old assertion ("an instinct is
  advice only") now reads: a forming instinct substitutes, a nascent one does not.
- **Hook, `crates/adage-hook/src/pretool/phases.rs`.**
  `an_instinct_dead_end_is_planned_then_substituted_on_the_same_verdict` covers plain and piped.
  The dry run gives the exact plan with the instinct line, the command hook prints nothing, and
  the treated verdict substitutes once with a `substituted` record. Control gets the plan and
  spools `would_substitute, withheld`. The world for it is `testkit::instinct_dead_end_world`.
- **Mod, `plugin/claude-code/hooks/call.ts`.** Comments only: the branch is route-agnostic and is
  unchanged. `claude plugin test`: 430 pass.
- **Sim, `crates/adage-sim/src/scenario.rs`.**
  - `instinct_substitutions(&replay())` lists the replay's dead-end turn and run 3's piped form of
    it, with the declared-reason context prefix. Its unit test is
    `the_instinct_dead_end_is_substituted_plain_and_piped`.
  - In the sim's learned replay, the dead end is the learned `dead-end-…` route. It was already
    substituted, so the shared `replay` and `substitutions` are unchanged, as are Codex and
    OpenCode.
  - The flip is in a home where only the instincts are bound. That is what the live sandbox is.
- **`crates/adage-cli/tests/harness_sim.rs`.** `claude_code_substitutes_the_instinct_dead_end`
  runs `adage condition --instincts-only --repo acme-billing --repo-path <sandbox>` over the
  sandbox's Makefile, with nothing learned. It asserts that every `dead-end-` route in `graph
  --json` is an instinct. Then, through the binary in both arms, each instinct substitution goes
  through the existing `check_substitute`: dry-run plan, treated substitution with its judgement,
  control `would_substitute, withheld`.

## Live proof (Claude Code 2.1.292 on PATH)

Setup, under `$S/is-live` (`$S` is this session's scratchpad):

- `cargo build -p adage-cli -p adage-exec` (debug).
- A plugin from `git archive HEAD plugin/claude-code`, with this change's `call.ts` laid over it
  (the diff is comments only).
- `dist/` is copied from the tree, with `adage` and `adage-exec` replaced by `target/debug`'s and
  `SHA256SUMS` regenerated. The driver is `$S/i0-live/one.py`, copied to `$S/is-live/one.py`.
- A fresh `CLAUDE_CONFIG_DIR` per run.
- A per-run COPY of `/tmp/adage-live3/adage-home`. The original was only read, and nothing was
  added to the copy: its dead ends are the bound U1/U3 instincts.
- `CLAUDE_PLUGIN_DATA` from `demo/run.py`'s `arm_invocation("mod")`, `--permission-mode default`,
  and a generated sandbox.
- W-2041 with run 3's prompt. For control, the same wiring with `ADAGE_ARM=control` set after
  `arm_invocation`.
- Metrics: `demo/adage_metrics.py`'s `dead_end_hits` and `spool_metrics`, over the stream and over
  this session's own spool file.

| run | arm | `[API REQUEST]` | the dead end the model wrote | what ran | spool `pretool` | `dead_end_hits` / `dead_end_substituted` | passed |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `treated-a` | treated | **3** | `python3 -m unittest discover -s tests -t . 2>&1 \| tail -5` | `make test 2>&1 \| tail -5` ("Ran 4 tests … OK") | `substituted: true`, `checked_by: engine`, judgement `substituted dead-end-b8ac50` | **0** / 1 | yes |
| `treated-b` | treated | **3** | the same | the same | the same | **0** / 1 | yes |
| `control-cmd` | control | 2 | the same (the prompt asks for that one command) | the dead end (`ACME_ENV` failure) | `would_substitute: true`, `withheld: true`, judgements `[]`, `would_substitute.command` `make test 2>&1 \| tail -5` | 1 / 0 | n/a |
| `control-a`/`b`/`c` | control | 7 / 8 / 6 | none: the agent read the Makefile first and never ran the dead end | n/a | no `pretool` substitution record | 0 / 0 | yes |

- Both treated runs follow the same sequence:
  1. the predicted lookup at index 0 (`msg_hook_…`);
  2. the chain read at index 1;
  3. the model's first response: two edits (in `treated-a`, an edit and a read);
  4. the dead end, replaced (request 2);
  5. the answer (request 3).

  Index-0 runs `a`–`d` used 5 requests. The two dead-end responses are gone.
- One `hook_additional_context` attachment in each transcript carries the context line (the
  `PostToolUse` of the replaced call). It reads exactly as in the example in "The rule" above. The model took it at its word in both runs: "A
  hook swapped my test command for `make test` …" and "The test run was swapped for `make test`
  …". No apology appears, and the debug log has no `API error`, `thread_not_found` or tether line.
- Control runs `a`–`c` (W-2041) never wrote the dead end: without the predicted lookup, the agent
  explores and reads the Makefile. So the control half was proved with a prompt that names the
  command, as the pipeline-substitution proof did. That run recorded `would_substitute` and ran
  the dead end.
- Sessions:
  - `47f3d063…` (treated-a)
  - `827fbfd7…` (treated-b)
  - `0efd4cbe…` (control-cmd)
  - `f739f4d9…`, `e49e1528…`, `d56311af…` (control W-2041)
- Spend: about $0.33.

## Commands and results

| command | result |
| --- | --- |
| `cargo fmt --all`, then `cargo fmt --all -- --check` | clean |
| `cargo clippy -p adage-runtime -p adage-instincts -p adage-hook -p adage-sim --all-targets -- -D warnings` | clean |
| `cargo clippy -p adage-cli --tests -- -D warnings` | clean |
| `cargo test -p adage-runtime -p adage-instincts -p adage-hook -p adage-sim` | runtime 141 + 1, instincts 11 + 24 + 1, hook 72 + wire 2, sim 6 + 2 + 1: all pass |
| `cargo test -p adage-cli --test hooks --test harness_sim` | 25 and 12 pass; no golden changed |
| `cargo test -p adage-cli --test mod_surfaces` | 11 pass, 1 ignored |
| `cargo test -p adage-exec` (whole) | all pass (142 lib; parity 25) |
| `cargo test -p xtask` | 24 pass |
| pinned CLI 2.1.288: `claude plugin validate --strict plugin/claude-code` and `.` | pass; the output made as CI makes it is identical to `VALIDATE.txt` |
| pinned CLI 2.1.288: `claude plugin test plugin/claude-code` | 430 pass, 0 fail |
| `node scripts/check-plugin.mjs` | `plugin adage 0.1.0: ok` |

## Files touched

- `crates/adage-runtime/src/pretool_decide.rs`: the rule, `instinct_reason`, `same_tool`, and
  the docs
- `crates/adage-runtime/src/render.rs`: `declared_ok` is now `pub(crate)`
- `crates/adage-runtime/src/tests/instinct_substitution.rs` (new), `tests/mod.rs` (one line),
  `tests/dead_end.rs` (one assertion and a doc)
- `crates/adage-hook/src/pretool/phases.rs`: a doc line and one test
- `crates/adage-hook/src/testkit.rs`: `instinct_dead_end_world`
- `crates/adage-sim/src/scenario.rs`: `instinct_substitutions`, its test, and the `Substitute`
  docs
- `crates/adage-cli/tests/harness_sim.rs`: `claude_code_substitutes_the_instinct_dead_end`
- `plugin/claude-code/hooks/call.ts`: comments only
- `docs/claude-mod/architecture.md` §3.4, `docs/execution.md` ("The learned pair"),
  `docs/claude-mod/test-plan.md` (T-C6-07, T-C6-01's wording, the index row "01 … 07"),
  `docs/instincts.md` (the shipped-behaviour table and the CPU-matching paragraph)
- `CHANGELOG.md` (The Claude Code mod)
- these notes

`crates/adage-instincts` needed no change: the binding already produces what the rule admits.
