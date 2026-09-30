# Hook-boundary substrate probe

For the next real-repository experiment, see the [cross-session learning-curve protocol](LEARNING-CURVE.md).
`learning_curve.py` freezes task/source hashes and validates the 19 base/gold controls without paid
calls. It reuses the scoring/confinement substrate below; paid execution is not enabled by preparation.

This is permanent evaluation scaffolding, not a second Adage runtime. The tiny fixture
executor can only append `fixture-effect` inside a marked temporary directory. It does
not implement grants, route mining, general scripts, production scheduling, or sandboxing.

The recipe was checked against installed Claude Code **2.1.282** (`claude --help`,
`claude --version`). No hook ordering is claimed from CLI help alone. Managed settings
may still apply even when user/project/local sources are excluded.

## Offline verification

Run from this directory:

```sh
python3 -B -m unittest -v test_probe.py
```

Tests use real local subprocesses to establish fixture start/end ordering, concurrent
deduplication, cancellation and refusal to retry an uncertain result. They do **not**
prove Claude's hook barrier. There are no network/model calls in these tests.

## Prepare without spending

```sh
python3 -B probe.py prepare --model claude-haiku-4-5-20251001 --budget-usd 1
```

This prints a manifest path under a new private temporary Git repository. Inspect it
before running. The manifest contains exact argv, model, proposed spending cap, hook
settings, and limitations. It makes no Claude call, changes no global settings, and
does not activate anything in another repository. Use `--delay 0.25` to make the fixture
boundary easier to see. The scripts support POSIX/macOS only (`flock`, process groups).

## Explicit live run (requires an approved budget)

```sh
python3 -B probe.py live /absolute/temp/path/manifest.json --budget-usd 1 --wall-seconds 90
```

The runner refuses a nested `CLAUDECODE` environment; do not remove protection markers.
It pins the locally checked version and uses session-only settings, no MCP servers,
Read-only model tools, normal permission checks, stream JSON with hook events and
partial messages, a debug log, and no session persistence. It intentionally does not
use `--bare` or `--safe-mode`, which skip the hooks under test. A fresh fixture is
required for every run. The cap is per run; the operator must account for the sum of
actual costs across runs from Claude's final result. No automatic retry is performed.

The model is asked to read two files in one batch; whether it actually emits parallel
calls is an observation, not a guarantee. Prompt and batch hooks execute the fixed
fixture action; remaining hooks only log. Hook output explicitly attributes execution
to the fixture rather than to the user or model.

## Evidence contract

- `events.jsonl`: monotonic timestamps, PIDs, native event/session/turn/tool IDs when
  supplied, a clearly labeled *derived* batch key, child start/end, receipts and duplicates.
- `stdout.jsonl`: unmodified Claude stream. `stream-observations.jsonl`: each line's
  local arrival timestamp plus parsed event, including hook lifecycle data.
- `claude-debug.log`: version-specific diagnostic evidence; not a stable parser API.
- `run.json`: exact invocation, version, start/end, exit/cancellation status.
- `summary.json`: observed events/receipts and literal stream request-id fields.

Assistant message IDs are **not** API request IDs. Stream arrival times are **not** API
dispatch times. `api_request_count` deliberately remains null until actual requests
can be independently identified. Preserve raw logs; do not claim zero intervening
inference solely because `script_end` precedes the next streamed assistant message.
Hook receipts can establish result visibility; a verified request boundary requires
correlation with diagnostics or another real transport observation.

Cancellation selftests exercise SIGTERM handled by the fixture. Hard process kill,
host crash, and a harness timeout discarding output remain separate live fault cases;
this runner is not a guarantee of exactly-once execution. A persisted `running` record
suppresses reruns conservatively. Only effects inside the disposable fixture are allowed.

Current hook contract: [Claude Code hooks](https://code.claude.com/docs/en/hooks).

Completed real-run evidence: [2026-09-29 findings](results/20260929-claude-2.1.282/FINDINGS.md).
Use `probe.py export ROOT DESTINATION` to preserve fixture logs and a narrow diagnostic
milestone extraction. It excludes the broad debug log; the source remains in the private
temporary run directory. Capture copies before temporary-directory cleanup.
