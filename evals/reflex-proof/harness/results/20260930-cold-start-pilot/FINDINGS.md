# Cold-start instincts: four-task pilot

**Shipped instincts passed 4/4 tasks versus 2/4 for the agent alone, with 16.3% lower reported
API cost and 20.5% fewer observed model requests in this pilot.** This is a descriptive result
from four authored tasks on one real repository, with one run per arm. It is not a population
estimate or evidence of learned automatic CPU execution.

| Task | Agent quality | Instincts quality | Agent cost | Instincts cost | Agent requests | Instincts requests |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| Odd | Pass | Pass | $0.2500275 | $0.1822488 | 40 | 22 |
| Finite | Fail | Pass | $0.3325417 | $0.1771915 | 51 | 26 |
| No adjacent duplicates | Pass | Pass | $0.2165387 | $0.2473956 | 33 | 38 |
| Maximum UTF-8 bytes | Fail | Pass | $0.2112938 | $0.2384389 | 32 | 38 |
| **Total** | **2/4** | **4/4** | **$1.0104017** | **$0.8452748** | **156** | **124** |

Instincts cost more on two tasks. Both baseline failures and both cost regressions remain in the
totals. Observed agent tool calls were 152 versus 120; this is a descriptive count, not 32 confirmed
CPU action replacements. All eight collections completed without collection errors or unknown bills.

## What was tested

- Marshmallow at `7f0792bd7a06f72e393ec866ac1e89e1502ccfe1`.
- Fixed model `claude-haiku-4-5-20251001`, fresh checkouts and isolated knowledge homes.
- Original checkpoint-zero shipped instincts; no learned routes, new grants or executing hook.
- Agent/instinct order alternated by task, prospectively frozen before dispatch.
- Identical prompts, dependencies, native sandbox and independent quality rubric in each pair.
- Quality checks hidden behavior, requested artifacts and the complete upstream suite, with exact
  expected hidden-test identities checked to prevent accidentally missing tests.

Each instincts run delivered one project-Python hint before a system-Python test attempt. The
hook explicitly declined to rewrite the command under these permission rules and supplied advisory
context instead. This is evidence of out-of-the-box advisory value. It does **not** show that Adage
executed the test automatically or eliminated that test action through the CPU.

Measured hook-wrapper wall time, including evaluation evidence hashing, totaled 18.82 seconds
across the four instincts sessions. Per-assignment timing and delivered text are in the report.
Neither these timings nor API counts measure GPU utilization, CPU utilization or energy savings.

## Reproducibility and limits

[report.json](report.json) includes all eight outcomes, action IDs and commands, observed request
IDs, token accounting, hook deliveries, timings and source hashes. Frozen assignment manifests,
settings, task definitions and checkpoint-zero artifacts are included. All frozen harness sources
were checked against commit `1963477`; the original production artifact/source record identifies
commit `86355ec`. Broad debug logs and local execution grants are not published. Native transcripts
and full collections remain in the local study archive, identified by hashes in this export.

This cold-only study was separately frozen after the [15-session learning study](../20260930-learning-curve-native/FINDINGS.md)
stopped with zero executable candidates. It does not replace that negative result. The tasks had
not been dispatched before this study. No prompts, scoring rules or product binaries were adjusted
between these eight runs. Ordinary model variation remains a possible contributor to differences.

These numbers can be quoted with the four-task/one-repository qualifier. A general launch claim
needs a separately frozen, replicated evaluation across more tasks and repositories. The learned
automatic execution launch gate remains unpassed.
