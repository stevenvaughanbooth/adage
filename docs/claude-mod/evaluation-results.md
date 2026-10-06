# Claude mod verification, 2026-10-06

The commercial-evidence branch was rebased onto merged main `7aaeedd2` (PR #27).
These cover local checks of the real merged plugin and binaries, followed by one operator-run
cold-mod Coder cost scout. No learned Coder treatment, held-out comparison, request saving or
billed-cost result is claimed.

Machines: macOS arm64 and Docker Linux aarch64 on the same Mac; Rust 1.98.1, Claude Code
2.1.288 on both platforms, Node 22.15.0 in the Coder container. Hosted CI was unavailable.

## Offline results

- `claude plugin test plugin/claude-code`: 351 passed on macOS and 351 passed inside the
  isolated Linux Coder runtime. These use the actual plugin test engine; they do not replace
  live activation, permission, prediction, receipt or UI tests.
- Strict validation passes for the plugin, retired alias and marketplace. The committed
  validator snapshot's two decorative glyphs differed from 2.1.288 on both platforms; only
  those glyphs were refreshed. Exact comparison after removing the checkout prefix passes.
- Both binaries build on macOS and in Linux; the scout stages checksummed Linux release
  binaries with the plugin. Plugin staging/integrity checks and the npm launcher check pass.
- The original plugin startup shell test initially failed on macOS: `current_exe()` returned
  the `hook` symlink. Resolving it now records the actual verified binary. The original cold
  fixture passes; cold starts remain `session_start` with `unrouted: true`, not exposure to a
  learned reflex. A separate confined cold-start probe produced exactly one such record.
- The first workspace run (`--exclude adage-eval --no-fail-fast`) recorded 1,394 passes,
  eight failures and five existing ignored tests. Six failures were terminal-dependent text
  assertions in four CLI suites; two used a noncanonical temporary path for the completion
  fixture's repository key. The fixes preserve their semantic assertions. The four CLI suites
  pass all 45 tests; completion passes all four. The final CLI/hook/app run passes 438 tests with no failures.
- The complete Linux live-collector fixture suite passes all 179 tests. An earlier attempt
  lacked staged binaries and failed 52 setup assertions; that log is retained, not discarded.
- The demo replay/accounting suite passes all 36 tests with the built binaries supplied.
  Architecture checks pass 18 tests, the spawn guard passes two, and the experiment/source/
  prediction integration suites pass 22. Formatting and workspace/all-target Clippy pass.
- Bubblewrap hides the source archive and collector while exposing the staged plugin. The
  CLI reports an authenticated first-party login without exposing credentials. No model
  request was used to check availability; live availability remains unverified.

The weekly `adage-eval` replay/latency gates, paired release latency measurements and the
interactive/live sections of the evaluation plan have not run. This is not release-matrix or
launch-claim certification. Authentication status is not proof of a successful paid dispatch.

## Reproduction and retained evidence

From this branch, build both binaries before testing. The plugin CLI must be 2.1.288.

```sh
cargo build --locked -p adage-cli -p adage-exec
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --exclude adage-eval --locked --no-fail-fast
cargo test --locked -p adage-eval --test experiment --test experiment_sources --test predict_gate
claude plugin validate --strict plugin/claude-code
claude plugin test plugin/claude-code
bash plugin/test/session-start.test.sh
ADAGE_BIN="$PWD/target/debug/adage" ADAGE_EXEC_BIN="$PWD/target/debug/adage-exec" \
  python3 -m unittest discover -s demo -p 'test_*.py'
python3 -m unittest discover -s evals/live -p 'test_*.py'  # Linux with working Bubblewrap
```

Private logs, including unsuccessful checks and diagnoses:
`/Users/stevenbooth/Documents/adage-studies/2026-10-06-claude-mod-verification/`.
The new cost scout is under
`/Users/stevenbooth/Documents/adage-studies/2026-10-06-coder-mod-cost-01/`.
Its plan and input manifests pin the code, plugin, binaries, runtime, task and grader.
The earlier baseline-only preparation is retained at
`/Users/stevenbooth/Documents/adage-studies/2026-10-06-coder-cost-calibration-01/`.

## First Coder cost scout: completed, task failed

The operator ran the sealed scout at commit `bc9428bf16952e870d5659beeb7c3b4f1b2ef7f5`.
One exposed database-development task used Claude Code 2.1.288 and
`claude-haiku-4-5-20251001`, with the actual mod, no learned reflexes and no execution grants.
It stopped at the 60-turn limit (`error_max_turns`) after 169,770 ms. The terminal's turn
counter is 61; this is not the number of model requests.

| Observation | Result |
| --- | --- |
| CLI cost estimate | $0.3157685, against the $2 admission cap |
| Billed cost | Unknown: no billing evidence |
| Model requests | 60; debug dispatches, stream IDs and native history reconcile |
| Input tokens, including both cache categories | 1,839,936 |
| Uncached / cache creation / cache read input tokens | 482 / 39,019 / 1,800,435 |
| Output tokens | 11,441 |
| Independent candidate grade | Task failed; 6 of 9 scoped database checks passed |
| Mod activation | One module load, 60 observed turn.step callbacks, one startup record |
| Learned directives / predictions / receipt substitutions | 0 / 0 / 0 |

The migration, uniqueness constraint, downgrade and selected SQLC parity checks passed.
First-organization insertion failed through the source query, generated query and compiled Go
Store. The parent grader observed both inserted organizations with `is_default=false`. The
candidate put default selection in an application caller while adding a required boolean to
the database query; the task required the existing query itself to make the first organization
default. Generation output included a passing gentest; that does not establish feature correctness.
The nine checks are not nine independent task samples, and no overall correctness rate is inferred.

The initial private summary incorrectly fed `{t,line}` envelopes to the debug-request checker,
reporting zero stream request IDs. The derived audit instead uses the canonical native parser's
request identities and checks the native history too: all three counts are 60, with reconciled
terminal usage. Original summaries, streams, histories, failed attempts and the frozen preparation
are unchanged. The collector's placeholder `scorer_error` also remains in the original state;
external grading is the quality authority. Its `reference` key names the submitted candidate
when `--candidate` is used, not the earlier positive control.

Reproduce the free audit (no dispatch):

```sh
python3 /Users/stevenbooth/Documents/adage-studies/2026-10-06-coder-mod-cost-01/analysis-01/audit.py
```

Retained artifacts in that scout directory: `analysis-01/audit.json` (source hashes and corrected
accounting), `grade-01/result.json` (independent checks), `REPORT.json` and
`cost-observation.json` (original summaries), and `attempt-01/` (native streams/debug/history,
plugin spools and final checkout). No new training or held-out evaluation occurred: this cost
belongs entirely to development. Prior synthetic training remains a separate study.

No further paid run is admitted. Before expansion, future collectors must use decoded/canonical
request identities and report native termination separately from an external-grade placeholder.
The next learning experiment needs untreated independent training groups, production learning,
operator review and an explicitly frozen mod/plugin/settings/grants treatment before held-out
validation and untouched confirmation. Do not replace this unfavorable run or extrapolate its
cost into a cost per successful task. There is no savings estimate to publish.
