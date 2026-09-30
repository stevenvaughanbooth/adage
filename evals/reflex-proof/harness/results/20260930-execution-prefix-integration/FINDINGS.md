# Preserve executable prefixes through mining

The production miner now admits independently executable single-action prefixes even when an
equally supported longer continuation absorbs them during closure. It reuses the existing mined
patterns, score cache, compiler, trigger recount and runtime; no second prediction engine was added.

Execution admission can bypass advisory contrast and value, but not support, days, consistency,
90% anchor confidence or the final executable-trigger recount. Explicit `execution_only` provenance
prevents a candidate from becoming advice when execution judgment rejects it. Advisory calibration
still uses closed patterns. The existing 20,000 scoring cap and closed-pattern priority remain;
large corpora can still crowd execution prefixes out of that bounded pool.

## Frozen real-history replay

| Cohort | Histories | Compiled before | Compiled after | Certified execution contexts after |
| --- | ---: | ---: | ---: | ---: |
| Organic Marshmallow | 15 | 1 | 1 | 0 |
| Explicitly guided Marshmallow | 5 | 0 | 9 | 0 |

The guided replay clears the diagnosed mining/admission gap. Six candidates now reach the existing
execution evidence gate with **four supporting sessions and 4/5 qualifying continuations**; three
have zero qualifying continuations. None meets the unchanged minimum of five supporting sessions
and 90% supported continuations. These are development replays of previously observed histories,
not fresh held-out efficacy tests. All original checkpoints remain unchanged. No paid calls,
execution grants, automatic actions or savings claims result from these replays.

## Validation

The final implementation passes **120 focused Rust tests**: graph 45, compiler 11, learner 64.
The new native-shaped regression certifies five edit → pytest → unrelated-action histories;
four-history support and a failed continuation remain rejected. A compiler regression proves
execution-only provenance cannot fall back to advice even when advisory value is positive.
The release builds for `adage` and `adage-exec`, formatting and whitespace checks passed.

[validation.json](validation.json), both complete replay outputs, final test logs and
[production.patch](production.patch) retain the implementation and evidence. The separate cumulative
guided follow-up was prospectively frozen before another five demonstrations: preserve all ten
histories, keep thresholds unchanged, condition once at ten, and stop if no candidate qualifies.
Any automatic action comparison still requires a separately frozen evaluation and a human grant.
