# Claude mod verification, 2026-10-06

The commercial-evidence branch was rebased onto merged main `7aaeedd2` (PR #27).
These are free local checks of the real merged plugin and binaries. No live model session,
learned Coder treatment, held-out comparison, request saving or billed-cost result is claimed.

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

## Paid sections: not run

Proposed next dispatch: one exposed Coder database-development task, cold `mod` arm, the same
pinned Haiku model, $2 CLI cap, 60 turns, 1,800 seconds, no retry or expansion. No reflexes or
execution grants are preinstalled. The operator reviews and invokes the sealed command outside
an agent session; refusing inside an agent is intentional. The independent parent grader runs
after the model container stops. CLI cost remains distinct from billed cost.

The mod's plugin, settings, stores and grants must be frozen explicitly before a learned
comparison. The existing E0-only frozen coordinator cannot certify that treatment. Training,
operator review, frozen validation and untouched confirmation remain required before any
website or documentation savings claim.
