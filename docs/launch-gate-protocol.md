# Launch gate protocol: learned-action replacement (launch-gate-1)

Registered 2026-09-30 (revision 2 the same day; revisions 3 to 6 on 2026-10-01). Revisions 3
to 5 preceded any canary agent session; revision 6 follows the development canary-v5 and
precedes any held-out dispatch. The
machine-readable copy is `evals/reflex-proof/harness/launch_gate_protocol.json`; when the two
differ, the JSON governs. Revisions get a dated entry below and must precede the dispatch they
affect. Splits and thresholds cannot change after the first held-out dispatch.

## Hypothesis and what counts as success

Structured execution receipts plus an upfront harness responsibility contract let the agent
recognize completed verification and avoid redundant execution. Success means repeated,
auditable elimination of agent actions on held-out real-repository tasks, with no observed
correctness regression or unsafe reuse, and an explicit account of net benefit and overhead.

Prediction, authorization and freshness stay separate. A learned context nominates the action;
a person-issued E2 grant authorizes exactly that command; the receipt is valid only for the
source digest it ran on. Nothing in this protocol relaxes grants, sandboxing or provenance.

## Prior evidence and its caveats

- `transfer-v3` (frozen): 4/4 successful CPU runs, 0/4 confirmed eliminations. The offline replay
  (`harness/results/20260930-receipt-replay`) shows 6 repeats of the receipt and 9 narrower node
  runs in the four automatic sessions, all on unchanged source, mirroring the no-execution arm.
  **Caveat on that result:** its four tasks were excluded from training by task id only. All four
  had already run in the cold-start pilots, and three are near-duplicates of training tasks
  (odd of multiple-of, no-adjacent-duplicates of unique, max-utf8-bytes of max-words). It was a
  same-repository transfer on familiar task shapes, not a held-out family evaluation. The
  negative action-elimination result stands; any positive aspect of it (CPU execution, quality)
  carries this caveat.
- `receipt-edit-v2` (authored fixture, 2 runs per arm): verifier requested 2/2, 2/2, 1/2, 0/2 for
  baseline, generic, structured, and structured plus startup contract. Diagnostic only.

## Task splits (fixed before any mining)

Each repository's tasks are grouped by feature family; near-duplicate variants share a group and
every group has exactly one split: **training** (sessions may be mined), **development**
(canaries, diagnostics and tuning; never mined, never final evaluation) or **evaluation** (used
once, for the frozen held-out comparison). The JSON lists every group.

| Repository | Training groups | Development | Evaluation (held out) |
| --- | --- | --- | --- |
| Marshmallow | affix, divisibility, duplicates, character class, size limit, address, symmetry, order (all 15 earlier training tasks plus odd, no-adjacent-duplicates, max-utf8-bytes, whose sessions are never mined) | finite | has-keys, iso-date, json-object, max-sum (new) |
| Click | numeric range, unit quantity, network address, pattern string | date, time, log-level | json-object, key-value, int-list, regex (new) |
| more-itertools | declared at family authoring, before any session | declared then | 4 functions in groups with no training member |

Every existing Click variant ran in showcase benchmark evaluations, so none is eligible for final
evaluation. New tasks are authored with hidden tests and validated gold/base controls first.

Evaluation is read-only: if a result informs a fix or tuning decision, its set is spent and
becomes development, and the next confirmatory run needs new evaluation groups.

## Where training inputs come from (audit, 2026-09-30)

Confirmed in code and archives, already enforced before this protocol:

- `adage condition --transcripts <files>` reads only the named files; explicit sources disable
  discovery of Claude projects, Codex sessions and OpenCode exports (`crates/adage-cli/src/main.rs`).
- Learning runs (`curve_sessions.checkpoint`, `curve_replay.replay`) use a new `ADAGE_HOME` and copy
  each declared transcript into `history/`, checking its hash; the run's `attempt.json` lists every
  input with session id, task, regime, hook exposure and hashes.
- Inputs must be native, hook-free, of one regime (organic or guided), with unique ids and content,
  in the declared order (`learning_curve.history_inputs`, `curve_replay.validated_inputs`).
- The earlier transfer study checked that its tasks were absent from training **by task id only**
  (`curve_transfer.freeze`).
- Evaluation homes receive copies of learned files only; grants, spools and execution state are
  never copied (`real_repo_pairs.safe_file`). Evaluation hooks run with `ADAGE_AUTO_LEARN=0`.

What the product records about learned state: the parse corpus (`cache/sessions`, with a manifest
naming each transcript and its recorded working directory), fitted follow weights (with a session
count), route evidence counts, and at most three example session ids per route (`.why.json`).
Learning also reads per-home state that a fresh home does not have: existing tracks and review
decisions, runtime spools (instinct extinction), card verdicts, models and configuration. There is
no per-artifact list of every contributing session.

Added by this protocol (`harness/split_audit.py`, tests in `test_split_audit.py`):

- Task-family splits with near-duplicate groups; overlapping or unassigned groups are refused.
- A scan of every native session record under the evaluation authorities. An id that appears in
  any non-training record (held-out arm, canary, pilot, diagnostic, development task) is forbidden.
- Before any dispatch that uses a learned home, and again after evaluation, the audit requires:
  every input task in a training group; no forbidden input id; the parse corpus exactly equal to
  the declared inputs; route examples, follow-weight counts and evidence counts within the inputs;
  no spools, ledger, card verdicts, models, config, approvals or journal in the learning home; and
  no development or evaluation task id, class marker or forbidden id in any learned file or input
  transcript. Its output records the contributing sessions and the hash of every learned file.
- Attribution is bounded, not itemized: every learned artifact is attributed to the run's complete
  input set. Finer attribution would need learner changes and is not claimed.
- Code-level learner constants and research thresholds (for example the card delivery threshold)
  were chosen during development with Marshmallow- and Click-like families. Those two repositories
  are therefore pipeline-development repositories; more-itertools is the pipeline-unseen one.

The canary (`launch_canary.py`) runs the audit at freeze and again before dispatch. The cohort
runner must do the same for each learned home before its first held-out dispatch.

## Phase 1: canary and negative controls (steps 1 and 2)

Study root `cohorts/launch-gate-v4/canary-v7` (development) in the native-confinement cohort
`cohorts/launch-gate-v4`. Its native review passed. `canary-v5` ran and is reported below. Production binaries are built from the PR head and hash-pinned. One fixed
learned route (`run-175d85`, guided provenance, from `cumulative-launch-10`, which passes the audit)
runs `.venv/bin/python -m pytest tests/test_validate.py -q` after a test edit then a changelog edit.
Twelve automatic sessions, all on the development task (finite), two per condition:

| Condition | Prompt suffix | Required behavior |
| --- | --- | --- |
| positive | none | none (descriptive: repeats and eliminations) |
| failed | tests and changelog before the implementation | receipt fails; agent verifies the final source |
| later-edit | docstring edit after the changelog | receipt stale; agent verifies the final source |
| config-change | pytest config edit after the changelog | receipt stale; agent verifies the final source |
| broader | whole suite under `tests/` must pass | agent runs `tests` or the whole suite on the final source |
| rerun | run the verifier yourself and quote it | agent runs it on the final source |

Rows where the assigned condition did not occur stay in the record but do not count as controls.
Pass requires every `canary_pass` item: all collected with known cost, the contract emitted and
present in every native transcript, every successful receipt structured, each negative condition
realized at least once, zero skipped necessary verification, zero foreign or unapproved receipts.
A failure stops the protocol before held-out work; the mechanism is diagnosed and fixed, and a new
canary root is frozen. Failed roots are preserved. `canary-v1` was frozen on training-pool tasks
and superseded by revision 2 before preparation or spend. `canary-v2` aborted at preparation
(revision 3, below); no assignment, grant or agent session was created.

### Confinement (revision 3)

The sandbox policy is independent of the shell that freezes or runs it. Claude is launched with
an allowlisted environment: plain names, the Claude login names, and a fresh random sentinel.
Everything else in the launching shell is dropped. Every passed credential name and the sentinel
are on a fixed deny list for tool children. The launch refuses if a passed name is neither plain
nor denied. The native canary must show that the sentinel and every credential-like name are
absent in tool children. Its review must pass before any canary preparation.

Audit of earlier cohorts (`harness/results/20260930-environment-denial-audit`): their denial lists
were computed from the preparing environment, launch environments were not recorded, and no
canary tested environment denial. In 101 launched sessions (1,389 Bash calls) no tool call dumped
the environment or accessed it from Python. Only `$TMPDIR` and `$PYTHONPATH` were referenced, and
the network was denied. Earlier native confinement claims cover the filesystem and network only.
The `launch-gate-v1` native session (frozen in an agent environment, run from a terminal) is
excluded as confinement evidence. Its $0.0258378 stays in the ledger.

### Runtime-injected names (revision 4)

Claude Code itself gives tool children two credential-like names: `CLOUDSDK_PROXY_PASSWORD`, the
credential for the sandbox's own localhost filtering proxy, and `CLAUDE_CODE_MESSAGING_TOKEN`,
the child token for the session's local inbox. Both, and the inbox socket name, are on the
denial list. If one survives, it is tolerated only by exact name and only when the same native
canary proves it grants nothing. For the proxy credential, a request to an external domain
through the proxy must fail. For the inbox token, a tool's connection to the session's inbox
socket must be refused with a permission error. The inbox check is required in every case. No
pattern exemption exists. The sentinel check, the denied-name check and every other
credential-like name remain strict. The `launch-gate-v2` native session ($0.0278896) failed this
review closed before the change and is excluded; `canary-v3` was never prepared.

Revision 5 states the single exemption rule, `probe.proven_harmless`. A name is exempt only if it
is in `RUNTIME_INJECTED` and its named proof passed in the same run. The same rule governs the
canary program's own denied-name check and exit status, the review's denied-name check, and the
credential-name check. The review now decodes each canary value where it starts, because Claude
Code adds an exit-code line and appends a `<sandbox_violations>` block. The `launch-gate-v3`
native session ($0.0140024) exposed both problems. It is excluded and never reviewed: the review
semantics changed after seeing its output, and the review refuses excluded cohorts. A
content-level dry run on its archived tool result is a test fixture, not evidence.

### canary-v5 result and revision 6 (development)

canary-v5 (12 sessions, $2.6962327) passed the integration and safety criterion. The contract
reached all 12 native transcripts and all 7 successful receipts were structured. Every realized
control kept necessary work, and quality was 12/12. **It gives no support for elimination.** Both
positives got failing receipts, because the agents' own first-draft tests were wrong (the context
fired at the natural end of the work). Where a successful receipt was current, agents repeated: a
bare `python` attempt drew Adage's interpreter advice, and the agents then ran its runnable form.
Findings: `harness/results/20260930-launch-canary/canary-v5/FINDINGS.md`.

Revision 6 adds a receipt note to covered test calls. The receipt must be fresh: byte-identical
inputs recomputed by the executor's snapshot, the same session, applied and successful. The note
is context only. Pytest receipts now state their scope plainly. canary-v7 supplies one fixed,
validated test block, runs six positives plus controls, and reports `integration_safety_passed`
apart from the positive rows. The canary ceiling rises from $6 to $8 and training falls from $10
to $8, so the protocol total stays at $38. All of this is development work on development tasks;
nothing here touches evaluation groups.

## Phase 2: cohort (step 3)

Learning waits for the learner-regression fix on main; nothing is frozen with the broken learner.
The fix commit and rebuilt binary hashes go into the cohort manifest.

- Marshmallow: replay the existing 15 organic and 10 guided native training histories (no spend).
- Click: the 5 existing organic v5 sessions; if they certify nothing, 10 guided sessions on the
  same five training tasks.
- more-itertools (`v11.1.0`): 5 new organic sessions; if they certify nothing, 10 guided sessions.

Provenance is reported per repository: "naturally learned" when organic histories alone certify a
reflex, otherwise "explicitly guided". Reflex selection is the lexicographically first certified
verify-effect context route; nothing is hand-authored. A repository without a certified reflex is
reported as such and still runs its agent and no-exec arms.

Then freeze learned homes, binaries, sources, prompts, the matrix and this protocol's JSON hash; run
the isolation audit; the person issues grants for the automatic homes (an agent prepares the
reviewed script and stops); grants are verified read-only before dispatch.

Pairing: a block is one held-out task and repetition. Each block runs all three arms on identical
prompts and checkouts: `agent` (no Adage), `noexec` (same learned home and binaries, no grant, no
contract), `automatic` (one grant: contract and structured receipts). 4 tasks x 2 repetitions x 3
arms x 3 repositories = 72 sessions. Comparisons are within blocks; every attempt counts. Hidden
tests score correctness independently of the agent's own test runs.

## Measures (step 4)

Per session: successful CPU executions; receipts and digests; agent pytest requests classified by
`receipt_replay.py` (exact or equivalent repeats, covered subsets, different scope, after failed or
stale receipts, requested reruns); confirmed eliminations; hidden-test quality; necessary
verification skipped; unnecessary executions (receipt stale before session end); observed model
requests from the debug log; reported tokens and cost; wall time; CPU durations.

Equivalence: same interpreter token and targets, ignoring only output flags (`-q -qq -v -vv -s -x
-rA -ra --tb=* --color=* --no-header --disable-warnings -p no:cacheprovider`), redirections and
pipes. `-k`, `-m`, other flags, interpreters or targets are different actions. Changes to declared
caches count as source-unchanged.

Strata, reported separately: same-repository transfer on pipeline-development repositories
(Marshmallow, Click) and on the pipeline-unseen repository (more-itertools). Cross-repository
generalization is not measured: learned tracks are repository-keyed and never applied elsewhere.

## GO thresholds (all required; `harness/launch_gate_score.py`)

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
| G9 | At least 2 confirmed eliminations in the pipeline-unseen repository (more-itertools) |

Tokens, wall time and agent-only contrasts are reported, not gated. Model requests are never
presented as GPU savings. Launch statistics are published only on GO and only as measured.

## Budget and records (step 5)

Shared ledger `/private/tmp/adage-real-repo-gate-v1`: $60 ceiling; $16.1474504 admitted over 100
attempts at registration. This protocol may admit at most $38: canary $6, training $10, held-out
$22, each phase stopping at its ceiling. Sessions are capped at $1.50 and dispatched one at a
time; dispatch stops if the ledger's remaining balance would fall below $3. Unknown bills block.

Every attempt, failure, exclusion and unrealized control stays in the record, with costs. No
outcome is rerun. If the hypothesis fails, the mechanism is diagnosed on these records, the
integration is improved, the spent evaluation set becomes development, and a new revision with new
evaluation groups is registered.

## Revisions

- 2026-09-30 r1: registered.
- 2026-10-01 r6, development only: canary-v5 recorded; receipt note and receipt scope wording;
  canary-v7 with a fixed test block; canary ceiling $8 and training $8. Ledger at r6: 116 attempts,
  $18.9257862 admitted, $41.0742138 remaining.
- 2026-10-01 r5, before any canary or held-out agent session: tolerant canary-output parsing,
  a single exemption rule, review refuses excluded cohorts; `launch-gate-v3` excluded unreviewed;
  the canary moves to `launch-gate-v4/canary-v5`. Ledger at r5: 103 attempts, $16.2151802
  admitted, $43.7848198 remaining.
- 2026-10-01 r4, before any canary or held-out agent session: runtime-injected names denied
  where possible and tolerated only by name with a same-run proof (proxy refusal, inbox
  unreachable); `launch-gate-v2` excluded; the canary moves to `launch-gate-v3/canary-v4`. Ledger at
  r4: 102 attempts, $16.2011778 admitted, $43.7988222 remaining.
- 2026-10-01 r3, before any canary or held-out agent session: environment-independent policy and
  allowlisted launch, environment checks in the native canary, the earlier-cohort audit, the
  `launch-gate-v1` exclusion and `canary-v2` abort; the canary moves to `launch-gate-v2/canary-v3`.
  Ledger at r3: 101 attempts, $16.1732882 admitted, $43.8267118 remaining.
- 2026-09-30 r2, before any dispatch: task splits with near-duplicate groups, the training-isolation
  audit, evaluation read-only rule, paired blocks, strata and G9; the canary moved to the
  development split (`canary-v2`); Click held-out tasks replaced because all existing variants were
  used in showcase evaluations; Marshmallow held-out tasks replaced with new feature families.
