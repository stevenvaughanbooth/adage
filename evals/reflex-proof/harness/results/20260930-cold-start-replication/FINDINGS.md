# Cold-start instincts: two runs of four tasks

Across **16 sessions on four Marshmallow tasks**, shipped instincts passed **8/8 outputs versus
6/8** for the agent alone, with **8.8% lower reported API cost** and **10.5% fewer observed model
requests**. Each task ran twice per arm. These are descriptive pilot results, not a general savings
estimate or evidence of automatic CPU execution.

| Run | Agent quality | Instincts quality | Agent cost | Instincts cost | Agent requests | Instincts requests |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| Original | 2/4 | 4/4 | $1.0104017 | $0.8452748 | 156 | 124 |
| Reversed-order replication | 4/4 | 4/4 | $0.9325707 | $0.9267710 | 148 | 148 |
| **Combined** | **6/8** | **8/8** | **$1.9429724** | **$1.7720458** | **304** | **272** |

The replication's cost reduction was only **0.6%**, versus 16.3% in the original run. Request
counts were equal in the replication, and both arms passed all four tasks. This variation limits
the strength of the savings claim. **Three of eight paired comparisons cost more with instincts**;
all sixteen outcomes and both baseline failures remain in the combined totals. Total reported
cost for the new replication was $1.8593417; both runs together cost $3.7150182.

## Fixed comparison

The second run was registered after observing the first pilot, with each task's arm order reversed.
It reused the same frozen model, task prompts, pinned upstream code, production binaries,
checkpoint-zero instincts, dependency runtime and independent hidden/full-suite rubric. All eight
assignments were prepared before dispatch. No task or outcome was retried, and no successful-only
subset was selected. The native review used the current harness source and the identical sandbox
policy/runtime; the prior review's source hash correctly refused the newer harness. An earlier
offline directory-nesting mismatch was also rejected. Both are recorded in validation, with no paid
calls made by those failed preparations.

The intervention supplies shipped advisory context about the repository's Python environment.
There are no learned routes, new grants or automatic-execution treatment. Tool-call totals were
296 versus 264 across both runs; these do not prove 32 CPU action replacements. API cost and request
counts do not measure GPU utilization, power or energy.

## Reproduction and reporting

[report.json](report.json) contains every assignment from both runs, costs, independent quality,
observed request/action metadata and limitations. [replication-report.json](replication-report.json)
retains the new run separately. Frozen source, manifests, settings, process records and native
transcript hashes are included. The original [pilot report](../20260930-cold-start-pilot/FINDINGS.md)
is unchanged. Broad debug logs, grants and raw transcripts remain in the local study archive.

This is **four distinct tasks on one repository, two runs per arm**. The replication was not an
independently powered confirmatory trial. Use the combined numbers with those qualifiers, disclose
the weaker repeat, and do not describe this as learned reflex efficacy. A broader launch claim
still needs more repositories and independent task families.
