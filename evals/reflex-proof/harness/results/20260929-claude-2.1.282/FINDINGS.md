# Real Claude hook-boundary result

2026-09-29, Claude Code 2.1.282, `claude-haiku-4-5-20251001`, macOS.
One successful paid run: **$0.0152216**, bounded by `--max-budget-usd 1`.
A prior sandboxed attempt returned `Not logged in · Please run /login`, zero usage and
reported cost $0; retry used normal approved sandbox escalation, not altered credentials
or removed protection markers. Total reported spend across these two attempts: $0.0152216.

## What this establishes

The real CLI invoked both `UserPromptSubmit` and `PostToolBatch`. Each synchronously ran
the fixed fixture child script, received its successful receipt, and supplied that receipt
to the model. Claude's final answer identified both receipts and correctly read both files.
The stream contains two distinct real `request_id` values, separately from message IDs.
The version-specific debug log records two `/v1/messages` dispatches:

| Diagnostic event | UTC timestamp |
| --- | --- |
| Prompt receipt accepted as additional context | 22:13:51.126 |
| First model request dispatched | 22:13:51.147 |
| Batch receipt accepted as additional context | 22:13:54.369 |
| Second model request dispatched | 22:13:54.381 |

The batch context was accepted 12 ms before the next dispatch. The script start/end and
receipt precede hook completion on the shared OS monotonic clock. This demonstrates the
substrate needed for CPU work to run at a real hook boundary before the next model request.
It does not require an extra model request to ask for or carry out that fixture action.

`debug-milestones.json` contains a narrow extraction of these diagnostic markers, excluding
the broad debug log. `stdout.jsonl` preserves the stream, `events.jsonl` preserves local
process evidence, and `summary.json` records real request IDs, usage, result status and the
fixture script's SHA-256 **at export** (not a claim that this run enforced a hash grant).

## What this does not establish

This fixture is not production Adage execution. It proves no production grant enforcement,
route accuracy, sandbox confinement, signed-pack safety, developer usefulness, or net savings.
It is one headless run, not an interactive/long-running reliability study. Both reads belonged
to one model tool-call batch; their execution overlap is not needed for, or proven by, this result.
Deduplication and SIGTERM behavior were tested offline with real subprocesses, not induced
in this paid run. Crash, forced-kill and harness-timeout recovery remain unproven here.

`num_turns` reported by Claude was 3; there were 2 observed request IDs and 2 diagnostic
dispatches. These are different quantities. No energy claim follows from this observation.

After the run, main was merged to `acdb0a4`, which already supplies `adage-exec` and a
Claude execution plugin. Subsequent proof should exercise that existing implementation.
No production executor should be built from this deliberately minimal fixture.
