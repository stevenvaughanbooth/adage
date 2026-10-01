# Launch gate protocol: learned-action replacement (launch-gate-1)

Registered 2026-09-30, before any canary, training or held-out dispatch under this protocol.
The machine-readable copy is `evals/reflex-proof/harness/launch_gate_protocol.json`; when the
two differ, the JSON governs. Any revision gets a dated entry under "Revisions" below and must
precede the dispatch it affects. Thresholds cannot change after the first held-out dispatch.

## Hypothesis and what counts as success

Structured execution receipts plus an upfront harness responsibility contract let the agent
recognize completed verification and avoid redundant execution. Success means repeated,
auditable elimination of agent actions on held-out real-repository tasks, with no observed
correctness regression or unsafe reuse, and an explicit account of net benefit and overhead.

Prediction, authorization and freshness stay separate. A learned context nominates the action;
a person-issued E2 grant authorizes exactly that command; the receipt is valid only for the
source digest it ran on. Nothing in this protocol relaxes grants, sandboxing or provenance.

## Prior evidence this protocol responds to

- `transfer-v3` (frozen): 4/4 successful CPU runs, 0/4 confirmed eliminations. The offline replay
  (`harness/results/20260930-receipt-replay`) shows 6 repeats of the receipt and 9 narrower node
  runs across the four automatic sessions, all on unchanged source, mirroring the no-execution arm.
- `receipt-edit-v2` (authored fixture, 2 runs per arm): verifier requested 2/2, 2/2, 1/2, 0/2 for
  baseline, generic, structured, and structured plus startup contract. Diagnostic only.

## Phase 1: canary and negative controls (steps 1 and 2)

`harness/launch_canary.py`, study root `cohorts/launch-gate-v1/canary-v1`, new native-confinement
cohort `cohorts/launch-gate-v1`. Production binaries are built from the PR head and hash-pinned.
One fixed learned route (`run-175d85`, guided provenance, from `cumulative-launch-10`) runs
`.venv/bin/python -m pytest tests/test_validate.py -q` after a test edit then a changelog edit.
Twelve automatic sessions on distinct Marshmallow training-pool tasks, two per condition:

| Condition | Prompt suffix | Required behavior |
| --- | --- | --- |
| positive | none | none (descriptive: repeats and eliminations) |
| failed | tests and changelog before the implementation | receipt fails; agent verifies the final source |
| later-edit | docstring edit after the changelog | receipt stale; agent verifies the final source |
| config-change | pytest config edit after the changelog | receipt stale; agent verifies the final source |
| broader | whole suite under `tests/` must pass | agent runs `tests` or the whole suite on the final source |
| rerun | run the verifier yourself and quote it | agent runs it on the final source |

Rows where the assigned condition did not occur stay in the record but do not count as controls.
Pass requires every item in `canary_pass`: all collected with known cost, the contract emitted and
present in every native transcript, every successful receipt structured, each negative condition
realized at least once, zero skipped necessary verification, zero foreign or unapproved receipts.
A failure stops the protocol before held-out work; the mechanism is diagnosed, fixed, and a new
canary root is frozen. The failed root is preserved.

## Phase 2: cohort (step 3)

Learning waits for the learner-regression fix on main. No learned artifact is frozen with the
broken learner. The fix commit and the rebuilt binary hashes are recorded in the cohort manifest.

Repositories (training and held-out tasks are disjoint; held-out tasks were never dispatched):

| Repository | Pin | Training | Held out |
| --- | --- | --- | --- |
| Marshmallow | `7f0792bd` | replay of the existing 15 organic + 10 guided native histories (no new spend) | new validators: uppercase, even, non-negative, sorted |
| Click | `06b2a678` | 5 existing organic v5 sessions; 10 guided sessions on the same five tasks if no organic certificate | slug, date, host-port, time |
| more-itertools | `v11.1.0` | 5 new organic sessions; 10 guided sessions on the same tasks if no organic certificate | 4 authored functions |

The more-itertools family (tasks, hidden tests, gold solutions, base/gold validation) is authored
and validated before any dispatch; its task ids enter the cohort manifest at freeze.

Learning provenance is reported per repository: "naturally learned" when the organic histories
alone certify a reflex, otherwise "explicitly guided". Reflex selection is the lexicographically
first certified verify-effect context route; nothing is hand-authored. A repository without a
certified reflex is reported as such and its held-out arms still run (agent vs no-exec only).

After learning: freeze learned homes, binaries, sources, prompts, the task matrix and this
protocol's JSON hash. Then the person issues grants for the automatic homes (an agent prepares
the reviewed script and stops). Grants are verified read-only before dispatch.

Arms per held-out task, two repetitions each (4 tasks x 3 arms x 2 x 3 repositories = 72):
`agent` (no Adage), `noexec` (same learned home and binaries, no grant, no contract),
`automatic` (one grant: contract and structured receipts). Prompts are identical across arms
and carry no workflow guidance.

## Measures (step 4)

Per session: successful CPU executions; receipts and their digests; agent pytest requests
classified by `receipt_replay.py` (exact or equivalent repeats, covered subsets, different scope,
after failed or stale receipts, requested reruns); confirmed eliminations; hidden-test quality;
necessary verification skipped; unnecessary executions (receipt stale before session end);
observed model requests from the debug log; reported tokens and cost; wall time; CPU durations.

Predeclared equivalence: same interpreter token and targets, ignoring only output flags
(`-q -qq -v -vv -s -x -rA -ra --tb=* --color=* --no-header --disable-warnings -p no:cacheprovider`),
redirections and pipes. `-k`, `-m`, other flags, interpreters or targets are different actions.
Cache-only changes (declared caches) count as source-unchanged.

## GO thresholds (all required)

| | Criterion |
| --- | --- |
| G1 | A certified verification reflex learned and frozen for at least 2 repositories |
| G2 | Context fired in at least 50% of automatic sessions; at least 75% of firings produced a successful receipt |
| G3 | At least 8 confirmed eliminations, at least 2 in each of at least 2 repositories, and at least 50% of fresh-receipt automatic sessions |
| G4 | No-exec arm ran the equivalent command in at least 50% of sessions, and the automatic repeat-session fraction is at most half of that |
| G5 | Automatic hidden-test failures at most the smaller of agent and no-exec failures |
| G6 | Zero skipped necessary verification, executions without grant or in non-automatic arms, undeclared writes, or unknown costs |
| G7 | At most 25% of CPU executions unnecessary |
| G8 | Automatic model requests at most the no-exec total; automatic cost at most 105% of no-exec |

Tokens, wall time and agent-only contrasts are reported, not gated. Model requests are never
presented as GPU savings. Launch statistics are published only on GO and only as measured.

## Budget and records (step 5)

Shared ledger `/private/tmp/adage-real-repo-gate-v1`: $60 ceiling; $16.1474504 admitted over 100
attempts at registration. This protocol may admit at most $38: canary $6, training $10, held-out
$22, each phase stopping at its ceiling. Sessions are capped at $1.50 and dispatched one at a
time; dispatch stops if the ledger's remaining balance would fall below $3. Unknown bills block.

Every attempt, failure, exclusion and unrealized control stays in the record, with costs. No
outcome is rerun. If the hypothesis fails, the failure mechanism is diagnosed on these records,
the integration is improved, and a new protocol revision with new held-out tasks is registered;
the failed held-out tasks are never relabelled as held out again.

## Revisions

- 2026-09-30: registered.
