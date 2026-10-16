# A learned CPU reflex eliminated the agent's lookup

**Mechanism gate: passed.** Adage learned `python3 tools/case {item}` from nine archived real
agent sessions, using default thresholds and no supplied outcome labels. A human approved its
read-only execution once. On three held-out work items, the production hook then supplied the
complete result before the first observed model request. The agent performed no lookup calls.

| Across three held-out tasks | Same learned reflex: suggest | Same learned reflex: automatic |
| --- | ---: | ---: |
| Correct complete task answers | 3/3 | 3/3 |
| Agent lookup tool calls | 3 | 0 |
| Observed model API dispatches | 6 | 3 |
| Successful hook executions | 0 | 3 |
| Reported model cost | $0.0231545 | $0.0181981 |

That is one agent action and one observed model request eliminated per task, with **21.4% lower
reported cost in this pilot**. This is three task instances from one generated repository, using
Claude Code 2.1.282 and Haiku 4.5, on macOS. It is a narrow work-item retrieval/triage experiment,
not a coding benchmark, independent multi-repository sample, quality noninferiority study, or
GPU-energy measurement. Costs include actual cache behavior and are descriptive, not a promised
percentage reduction. No claim of broad launch readiness follows from this result alone.

The task asked for exact title, full description, full acceptance criteria and file paths in
JSON. The command was not supplied in the task prompt. Both arms received the same learned
knowledge and the same tools; only production hook execution differed. Exact content checks,
successful tool-result matching, execution receipts, process completion, diagnostic request
boundaries and durable accounting all had to pass. No sessions were excluded or retried.
The full frozen [protocol](../../ACTION-GATE.md) explains the deliberately restricted tool surface.

## Reliability and defects found

After the live pilot, **100/100 repeated local production-hook invocations** returned complete
correct results with successful execution receipts. End-to-end hook wall latency was p50
244.3 ms, p95 274.5 ms, maximum 306.2 ms. These reuse the same three fixture lookups and warm host;
they are not 100 independent developer tasks or a cross-platform reliability estimate.

The first two approved preflight calls timed out before Python started. Apple's `/usr/bin/python3`
shim repeatedly invoked Xcode discovery under a fresh sandbox home. The executor now resolves
that exact shim to the selected developer interpreter under already allowed filesystem paths.
The 1500 ms timeout, human grant, script bytes, fields and sandbox permissions stayed unchanged.
Original binary hashes and failed receipts are retained in [preflight-failures.json](preflight-failures.json).
No paid run occurred before this fix; the prepared binary/source hashes were revised explicitly.

Complete stdout is an explicit, bounded, scanned output contract. It requires the existing
unstructured-output policy and a human grant. Empty, unsafe, oversized or nonzero-exit results
withhold the completion claim and retain advisory fallback. Automatic binding refuses multiple
distinct matching IDs. The actual learned route abstained on ambiguous IDs, malformed IDs,
unrelated prompts and missing grants. Integration tests also exercise changed scripts, output
overflow, policy denial, failed commands and same-ID repetition.

The receipt's `auto: false` means the grant came from the human rather than automatic approval;
the hook execution itself was automatic and made no model request to select or run the command.

## Evidence and reproduction

- [Aggregate](aggregate.json), [six scored sessions](report.json), [frozen preparation](preparation.json).
- [100 local hook runs](reliability.json), [negative preflight](negative-preflight.json),
  [complete offline delivery](offline.json), [export provenance](export.json).
- `runs/<task>-<arm>/`: filtered real assistant/tool/result stream, production hook output and
  process invocation. Diagnostic dispatch/context milestones are in the scored report.
- Preparation records training checksums, source files, binaries, exact learned route, approved
  output contract and frozen matrix. The baseline Git revision alone does not identify the
  measured implementation: the working-tree source hashes do. Local source/fixture prefixes are
  replaced with placeholders in the export; original and exported stream hashes are separate.

Run `action_gate.py prepare`, the human approval command, `check`, then `live` as described in
the protocol. This re-learns from the committed training traces; it does not install a fabricated
route or reuse an exported grant. Execution grants and broad host diagnostics are not exported.

Paid pilot spend was **$0.0413526**. Including the earlier substrate probe, this task's known paid
spend is **$0.0565742** against its $60 authorization. No other experiments' spending is implied.

## Launch decision

**Go for the learned lookup primitive:** automatic CPU execution now demonstrably replaces an
agent action, rather than merely sending another directive. Keep this result's scope explicit.

Before a broad efficiency claim or general launch, run complete coding tasks across more real
repositories, validate native release builds on supported platforms, and test the first approval
experience with developers. The next learning lever is equally concrete: current verify-route
scoring discounts checks agents already run reliably. Those repeated checks are promising CPU
actions, so they need an execution-value evaluation distinct from suggestion usefulness, while
retaining the same learner, grants and executor.
