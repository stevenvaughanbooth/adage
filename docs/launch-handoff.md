# Historical launch study: what we learned

This page summarizes the September–October 2026 launch experiments. It replaces the old
working handoff; use the [release guide](release.md) for current publishing instructions and
[the evidence index](../evals/reflex-proof/LAUNCH-STATUS.md) for the individual studies.

## The result we must keep visible

The held-out Marshmallow study reduced model requests but **failed its quality gate**.
The full protocol, decision and limitations are in the
[registered launch study](launch-gate-protocol.md).

| Measure | Result |
| --- | --- |
| Model requests, execution arm / baseline | 0.6741; one-sided 95% upper bound 0.7265 |
| Hidden-test passes | Execution arm 17/20; baseline 19/20 |
| Registered quality gate | Failed |
| Advice-only arm / baseline model requests | 0.7845; report-only comparison |
| Advice-only hidden-test passes | 19/20; no registered pass for this arm |

All 65 assigned sessions were included. The execution arm combined learned execution,
verification ownership and an orientation card, so the result does not isolate the value of
any one feature. Model requests are not a measurement of GPU use. Earlier pilots and failures
remain part of the evidence; see the [study harness](../evals/reflex-proof/harness/README.md).

## What this means for the preview

Adage is an experiment in learning reusable responses from agent history. Advice is the
starting point. Command execution requires separate approval and remains experimental.
We can demonstrate particular learned patterns and avoided work, but cannot promise that
Adage makes every task faster or preserves quality in every setting.

A stronger execution claim needs a fresh registered comparison that meets both the work
reduction and quality criteria. The tasks used in this study are now development material;
testing them again would not produce a new held-out confirmation.

For current behavior and setup, start with the [README](../README.md),
[first reflex guide](first-reflex.md) and [execution guide](execution.md).
