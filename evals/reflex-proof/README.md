# Reflex proof: current evidence

For the current open-source release decision, outstanding gates and cumulative paid spend, see
the [launch evidence index](LAUNCH-STATUS.md). Earlier reports below retain their original scope.

**New launch mechanism gate passed:** a genuinely learned, human-approved lookup eliminated
one agent action and one observed model request on each of three held-out tasks. All six
paired sessions returned complete correct answers; 100 subsequent local hook runs passed.
[Results, costs, reproduction and limits](harness/results/20260929-learned-action-gate/FINDINGS.md).

Isolated branch `codex/reflex-proof`, original base main `acdb0a4acd875be56821469aaeadeb29b8d6d493`;
main `d4f7e60` integrated before opening the PR, then UX main `52c256b` merged into that PR.
Historical measurements retain their original
source checksums. The [launch sequence](../../docs/reflex-proof-plan.md#launch-sequence-after-this-pr)
incorporates the advisory results and showcase work subsequently landed on main.
Protocol and six decision gates: [design plan](../../docs/reflex-proof-plan.md).

This work evaluates and strengthens the existing Adage components: history discovery,
macOS execution, typed results, durable E2 recovery, execution status in the TUI, and
paid-run accounting. It introduces no parallel learner, executor, approval store or package format.
The current product reference is [How Adage works](../../docs/how-it-works.md).

| Evidence | Result | Limit |
| --- | --- | --- |
| [Learned action elimination](harness/results/20260929-learned-action-gate/FINDINGS.md) | 3/3 automatic sessions made zero agent calls versus one in each suggest session; requests 3 versus 6; all 6 answers correct; reported cost 21.4% lower | Three triage tasks in one generated repository, not general coding savings; 100 local repetitions are a separate reliability check |
| [Live hook boundary](harness/results/20260929-claude-2.1.282/FINDINGS.md) | Two fixture actions completed and receipts reached context before the next observed API dispatch | One real Claude run; not production Adage execution or task savings |
| [Trace sufficiency](traces/README.md) | 540 traces / 10,846 calls audited, source checksums and grouped splits | Source availability, not executable coverage or prediction accuracy |
| [Default onboarding](traces/results/onboarding.json) | Nine archived training sessions, default thresholds and no supplied outcomes produced three learned routes plus three instincts; a held-out prompt received a learned card and lookup guidance | Generated repository; real CLI learner and registered hook, but fixture registration rather than an interactive installer test; no savings claim |
| [CPU embedder](embedding-result.json) | Verified installed Potion model; loaded p95 0.0187 ms, lazy open+embed p95 2.506 ms; zero external requests | Warm filesystem cache; authored contrasts; not whole-hook latency |
| [Execution journal](../../crates/adage-exec/src/journal.rs) | Durable preparation before apply; lost completion becomes Unknown and blocks further application until inspection | Fault injection and real apply APIs; not power-loss testing or universal exactly-once execution |
| [Production learner preparation](harness/PRODUCTION-PILOT.md) | Nine training sessions plus supplied success labels produced two cards and one lookup route; frozen identical advice/automatic homes | Outcome-assisted learning, not default onboarding; no regeneration route; lookup result remains incomplete; no paid pair dispatched |
| [macOS replay and containment](../../crates/adage-exec/tests/replay.rs) | Loader defect fixed; successful replay and blocked external file/network operations | Focused macOS tests, not the Linux-only hostile escape suite |

## Validation

UX integration preserves main's clap CLI, terminal design system, redesigned tabs, palette,
card/quiet views and embedded web map. Execution authority and receipt details use the new
Reflexes tab; obsolete text snapshots are removed in favor of reviewed insta snapshots.
The web sources and embedded assets are unchanged from UX main. The onboarding proof now
expects the new "traces" terminology, with the same nine training sessions and held-out check.
On the resolved UX merge, 99 CLI unit tests (including 41 UI tests), 3 real-hook action/pack
tests, 8 first-run tests, 7 background-learning tests, 7 history-source tests, accepted-pack
integration, 58 executor unit tests and 8 shared-hook tests passed. Formatting, all-target
warnings-denied lint and the generated-token/contrast check passed. The full native-platform
matrix and the previously documented debug startup timing limitation are not cleared by these
targeted checks.

Earlier, after integrating `d4f7e60`: workspace formatting and warnings-denied all-target lint passed;
50 CLI unit tests, 3 action/pack tests, 6 source tests, accepted-pack integration, 58 executor
unit tests and 8 shared-hook tests passed. The paid-runner/accounting suite passed all 50 tests;
npm launcher, session-start shell tests and plugin metadata checks passed. This does not replace
the full native-platform CI matrix. A [separate integrated onboarding report](traces/results/onboarding-integrated.json)
binds the rerun to its actual sources and binary; the earlier report is preserved.

The [validation ledger](validation.json) preserves completed broad-run targets and final
focused checks. The broad debug workspace run was stopped during the unmodified graph
property tests after benchmark, gate, parity and execution suites completed. It is **incomplete**,
not a green full-workspace run. Its initial action/pack failures were repaired and the affected
targets rerun successfully; the debug startup timing failure remains as described below.

- Harness fixture: 6 tests passed, including real subprocess ordering, concurrent
  duplicate delivery, SIGTERM and missing-accounting behavior.
- Trace audit: 10 tests passed; complete report reproduced byte-for-byte.
- The original receipt-loss characterization reproduced a real defect. It has been
  replaced by regression tests of the canonical journal's recovery contract; there
  is no alternate best-effort execution writer.
- CPU diagnostic: actual release run on the current base; formatting check passed.
- Existing executor suite initially failed at sandboxed replay on this Mac (`/bin/sh`,
  signal 6, empty output). The root-directory-node permission fix now passes all
  three replay tests, including real external file and loopback containment checks.
  The escape helper only enumerates Linux backends, so its passing tests on this
  host exercise fail-closed behavior, not the macOS backend.
- The initial full executor rerun exposed `fork: Resource temporarily unavailable`
  during legitimate staged regeneration. macOS now counts existing same-user processes
  when setting its bounded process allowance, and resource-limit setup failures are
  checked. The focused legitimate-generation control passed after this fix; the
  integrated regression run is recorded below when complete.
- TUI: 19 focused tests passed, including actual grant invalidation, quiet-control
  mode, interrupted-outcome labels, and wide/narrow snapshots. This is layout and
  behavior verification, not a measured first-user usability study.
- Updated CLI unit suite: 50 passed. Integration checks: 3 action/pack tests, 6
  history-source tests and the accepted-skill-pack test passed. The real-hook action
  test checks duplicate native IDs, a fresh identical event, interrupted recovery
  after grant removal, and a corrupt journal without retry instructions.
- Executor: 58 unit tests and all applicable integration targets passed, including
  the 500-run staged-change safety gate. Store: 20 tests passed. Shared hook: 8 tests
  passed after adding native-ID validation; the final journal correction passed all
  10 receipt tests. Linux-only escape tests remain platform-limited as noted above.
- Paid runner: 50 accounting/runner tests passed. No new paid dispatch occurred.
- The debug plugin's isolated first-start check still exceeded its unchanged 1.5 s
  limit (2.407 s). Profiling a no-history start attributed 1.316 s to first execution
  of the copied 95 MiB debug binary. Release-mode validation is tracked separately;
  a debug failure is not hidden by relaxing the threshold.
- The optimized release test cleared both existing startup timing assertions
  (<1.5 s first start, <200 ms subsequent start), then failed at its next assertion:
  the spawn recorder is compiled only in debug builds. This is timing evidence,
  not a passing release end-to-end test or a latency distribution.

Do not count a production-platform failure as an agent task failure or run expensive
comparisons until the platform can execute the intended workload. The loader repair grants
the literal root directory node, not its subtree. The broader CLI tests also exposed Apple's
launcher dependencies on the selected Xcode toolchain and its license-status file. Those
narrow read grants now pass action and pack integration tests; they never open all of
`/Applications` or `/Library/Preferences`. No unsuccessful
experimental production patch is retained. The paid runner now reserves per-session
caps durably and blocks on unknown accounting. It still needs supported confinement
and authentication before any new paid dispatch.

## Budget

User-approved ceiling for this task: **$60**. Known spend is **$0.0565742**:
$0.0152216 for the earlier substrate probe and $0.0413526 for six completed production
lookup sessions. All paid attempts have settled accounting; none is outstanding. The separate
historical $15 pilot allocation was unused and released. These figures cover this task, not the
provider account or separate experiments. The product's human execution grant was obtained
separately and never bypassed.

## How the evidence changes the next step

Current main already includes generic graph learning, E1/E2 execution, exact local
grants and signed packages. Preserve them. Complete integration checks and establish
the supported evaluation platform,
then extend the now-passed **complete result delivery and avoided duplicate agent work** gate
from triage to full coding tasks, with the same learned candidate in suggest and automatic modes.
No additional workflow family
or TUI redesign can compensate for a result the agent must immediately fetch again.

The Reflexes read model now joins effective grant/lapse state with the canonical E2
journal. Execution approval and recovery still use person-gated CLI controls. E1
prefetch activity remains hook telemetry; the E2 journal is not a complete history
of every intervention. Default onboarding and net live value remain distinct gates.
