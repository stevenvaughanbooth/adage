# Fifteen-session learning pilot

**Result: no executable reflex emerged.** This is a readiness finding, not a CPU savings claim.

| Measurement | Result |
| --- | --- |
| Repository | Marshmallow, pinned commit `7f0792bd7a06f72e393ec866ac1e89e1502ccfe1` |
| Teacher | `claude-haiku-4-5-20251001` |
| Independent native training sessions | 15, chronological, no Adage hooks |
| Hidden checks, requested artifacts and upstream-suite grading | 12/15 pass |
| Sessions with a recognized successful pytest invocation | 7/15; descriptive, not equivalence proof |
| Executable candidates after 0/3/5/10/15 sessions | 0/0/0/0/0 |
| Checkpoint-15 routes | 17 shipped-instinct routes, 4 learned guidance routes |
| Paid training | $3.7549221 |
| Two retained native probes | $0.0259818 |
| Paid held-out automatic sessions | 0, stopped by the registered eligibility rule |

The three independently failing tasks were multiple-of, alphanumeric and printable. All their
traces stayed in every applicable chronological checkpoint. CLI completion was never substituted
for task correctness. The first native probe passed but its cohort encountered an environment
fingerprint mismatch before training; the corrected fresh cohort and probe are both retained.

The learner did discover environment-recovery guidance. Successful verification evidence, however,
was split between project Python, activation chains, task-specific selectors and different command
flags. Only three sessions had successful direct project-Python pytest commands. More correct
feature implementations alone did not yield a supported, certifiable automatic action.

## Development replay

After observing training, a narrow production fix combined distinct successful session identities
for direct project-Python pytest commands that differ only in verbosity. It preserves interpreter,
scope, selectors and other flags. Native cwd provenance is required; unknown cwd, previous/same-turn
unknown Bash, activation and legacy cached evidence fail closed for this new pooling.

The same frozen 15 histories, production defaults and a fresh knowledge home still produced
**zero execution candidates**. The [replay record](development-replay/attempt.json) includes the
new binary/source hashes; the exact [production patch](development-replay/production.patch) is
retained. The original checkpoints were not overwritten. No new model calls or grants were used.
Synthetic integration demonstrates five qualifying sessions can certify this equivalence; it is
not evidence that natural traces in this pilot did so.

## Evidence and interpretation

[report.json](report.json) contains every assignment, independent quality result, invocation audit,
observed request diagnostics and all 16 unattempted held-out assignments. Checkpoint directories
retain native input identities/hashes, condition output, source/binary hashes and artifact hashes.
`frozen-source/` preserves the executed protocol and harness. Native transcripts remain in the
local immutable study archive; broad debug logs and execution grants are not published.

The next engineering task is trustworthy execution-context capture and extraction of recurring
actions from naturally varied shell usage. A later stronger-teacher study must be prospectively
registered and retain this negative result. Cold-start instincts can be measured separately; any
improvement from their guidance must not be called automatic CPU execution or measured GPU savings.
