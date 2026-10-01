# Launch gate protocol: learned-action replacement (launch-gate-1)

Registered 2026-09-30 (revision 2 the same day; revisions 3 to 8 on 2026-10-01). Revisions 3
to 5 preceded any canary agent session; revisions 6 to 8 follow development canaries and precede
any held-out dispatch. Revision 9 was drafted on `codex/heldout-tasks` and merged into
`codex/launch-first-reflex` at `57e79bf` on 2026-10-01, before any held-out dispatch. Revision 10
(development only) follows it, also before any held-out dispatch. Revision 11 (a scoring-definition
fix) was registered after dev-v9's first control and before any of its positive blocks. The
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
| Marshmallow | affix, divisibility, duplicates, character class, size limit, address, symmetry, order (all 15 earlier training tasks plus odd, no-adjacent-duplicates, max-utf8-bytes, whose sessions are never mined; r9 adds the withdrawn, never-run max-sum to size limit) | finite | has-keys, iso-date, json-object, lat-long (new; lat-long replaced max-sum in r9) |
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

### canary-v7 result and revision 7 (development)

canary-v7 (12 sessions, $1.3425845) realized all six positives, each with a fresh successful
receipt. Its original report counted 4 eliminations. **They are a scoring artifact.** In each,
the model's post-receipt request was a covered test command run with the bare `python`, which
failed to launch in the sandboxed shell. A post hoc re-score with scorer version 2 finds covered
repeat attempts in 6/6 positives and 0 eliminations. Structured receipts plus the startup
contract did not stop the model from requesting covered verification
(`harness/results/20260930-launch-canary/canary-v7/FINDINGS.md`).

Revision 7, registered before any held-out dispatch:
- **Scorer version 2.** A covered request is a repeat attempt whatever its interpreter and whether
  or not it launched, and any repeat attempt blocks an elimination. Only completed requests satisfy
  necessary verification.
- **Realistic project environment.** Sessions launch with the checkout's `.venv` first on PATH and
  as `VIRTUAL_ENV`. The sandbox policy is unchanged, and the native canary must show
  `python_on_path`.
- **Test evidence in the receipt.** Pytest receipts list collected and new node ids from the
  declared pytest cache record, bound to the journal.
- **Later-edit control.** It is realized by construction: a follow-up behavior change after the
  changelog.
- **Out of scope: PreToolUse result substitution.** It saves CPU only after the model has spent
  its turn.

The next canary is designed but not frozen, pending the next design direction.

### Revision 8 (development): fewer model requests, measured against no Adage

**Target.** Fewer model requests per task, measured against a no-Adage control in Claude Code
hook mode. CPU reuse is reported separately and never counted as inference saved. No hook can skip
a mid-turn model request (see the inference-skip research summarized in the handoff), so savings
can only come from the agent needing fewer requests.

**Lever 2: verification ownership.**
- **Contract.** A short factual SessionStart contract names the person-approved command, when Adage
  runs it, and that it runs again when the agent finishes.
- **Receipts.** Compact factual receipts give the command, pass or fail, the exit code and time, a
  short receipt id, the tests from the declared pytest record, and how long the result stays
  current.
- **Failures.** A failed learned verification is delivered with its failing tests.
- **Stop gate.** After a learned context verification ran in a treated session, a Stop with no
  passing run on the current inputs runs it under the same grant, sandbox and journal. A pass ends
  the turn with a person notice. The first failure continues the agent with the failing tests,
  unless another stop hook is already continuing. At most two gate runs per session.

**Lever 1.** The procedure card names the test command of an execution-certified verification as a
fact. No learned lookup or linked-file routes exist for this family (the learned home holds a card,
dead-end instincts and pytest runs), so discovery prefetch is skipped.

**Firing at the tests edit (1c) is not implemented.** It would change the approval key's input
scope, and the trigger stays as learned.

**Development canary-v8 (`harness/dev_arms.py`, protocol `dev-arms-1`).**
- **Arms**, run on the development task in six paired blocks with a rotating arm order:
  - A0: no Adage hooks;
  - A1: the revision-7 binaries;
  - A2: Lever 2;
  - A3: Levers 1+2.
- **Controls.** Five safety controls each in A2 and A3: failed, later-edit, config-change, broader
  and rerun. That makes 34 sessions, Haiku 4.5, with a study ceiling of $6.50.
- **Registration.** Every hook arm uses Claude Code's production registration: PostToolBatch rather
  than per-tool PostToolUse, and PreToolUse for Bash only.
- **Primary metric.** Model requests per session (distinct assistant message ids). Tokens by class,
  cost, wall time, quality, pytest requests and Stop-gate runs are reported.
- **Development thresholds**, all required for an arm to proceed to held-out:
  - D1: mean requests at most 0.85 times A0's, with fewer requests in at least 4 of 6 blocks;
  - D2: no more hidden-test failures than A0;
  - D3: zero skipped necessary verification in its controls;
  - D4: covered repeat attempts in at most 2 of 6 positives.

**Budget.** The canary/development ceiling rises from $8 to $12 and training falls from $8 to $4.
The protocol total ($38) and the held-out reserve ($22) are unchanged.

### Development canary-v8 result (dev-v8b, recorded 2026-10-01 under r8)

dev-v8b is **invalid for its execution arms from session 09 onward. It is not a valid negative
result.** No threshold or rule changed. The record is in
[the dev-v8b findings](../evals/reflex-proof/harness/results/20261001-dev-arms/FINDINGS.md).

All 34 sessions ran, at a known cost of $4.3539376.

**What happened.** Starting with session 09, every hook-arm session (A1-A3) ran with a lapsed
grant. The spool records `withheld: approval` with `route_exec_digest`. A process outside the study
ran Python unsandboxed and without `-B`. It wrote bytecode into the shared runtime's standard
library, which the approved interpreter identity includes. Those sessions therefore had no
receipts and no Stop gate, and from session 10 on they had no contract. Only 7 of the 18 hook-arm
positives (01-06 and 08) ran the design.

**What the report computed.** The mean requests per positive were:
- A0: 16.8;
- A1: 22.5;
- A2: 17.8;
- A3: 18.7.

No arm met D1. However, those means mix the 7 sessions that ran the design with 11 card-only
sessions, so they test neither lever.

**Consequences.**
- No arm qualified for held-out, and held-out does not run on this design.
- v8 makes no safety claim. All ten controls ran after the lapse and none realized its condition,
  so D3 was never tested.
- The Stop gate never ran.
- A1, whose highest counts all came after the lapse, was notably worse than A0.

**Post hoc audit.** Requests equal debug-log dispatches in 34 of 34 sessions, and no session was
censored.

**Harness changes (prospective).**
- Reports are now invalid and exit non-zero when any of these occur:
  - a hook-arm session had no contract, a withheld approval or no receipt;
  - counts are unverified or censored;
  - a control was not realized.
- `prepare` makes the shared runtime read-only and records its digest.
- Before every dispatch, `live` re-checks the runtime and checks the session's grant with its own
  binary at SessionStart. It stops the study on any lapse.

### Revision 10 (development): dev-v9, the orientation card

This revision is designed from the dev-v8 diagnosis. Two findings drive it. First, in the seven v8
sessions where execution ran, verification ownership landed near the 0.85 bar. Second, discovery
was the largest phase, and the card's exemplar ("copy the format") sent a third of card sessions
paging through files and grepping for idioms.

**Production changes.**
- **The card drops the exemplar.** In its place, the card says where its learned files stand in
  the checkout, read when the card is delivered:
  - line counts;
  - the last top-level definitions of a source file, with their lines;
  - a test file's last lines;
  - a changelog's first headings.

  This is Adage's own bounded, read-only process: about 1,200 tokens at most, no dotfiles, ignored
  or secret files, and no command run. `ADAGE_CARD_ORIENTATION=0` restores the earlier card as an
  evaluation knob.
- **Receipts name the session's new tests.** A receipt names the tests that this session's edits
  added (from the hook payloads' edit text), instead of the first twelve ids new since the last
  record.
- **Merged `codex/ship-fixes`.** This brings in the Stop-gate fixes, `__pycache__` left out of the
  project-Python digest (bare `.pyc` files stay hashed), and a lapse notice the person can see.

**Development canary-v9 (`harness/dev_v9.py`, protocol `dev-arms-2`).**
- **Arms**, on the development task in 16 paired blocks with a rotating order:
  - A0: no hooks;
  - A2f: verification ownership on the fixed binaries, with the card as shipped before this
    revision (exemplar, no orientation);
  - A4: A2f plus the orientation card.

  A2f and A4 share binaries, learned home and grant. Only the card differs, and the card checks
  line stays as shipped in both.
- **Controls.** The five safety controls run in A4, first in the matrix, and all must be realized.
- **Prompts.** Every prompt, controls included, carries the held-out realism line ("Do not create
  git commits.") between the development prompt and the canary suffix.
- **Size and cost.** 53 sessions, $0.50 cap each, study ceiling $8.50, ledger floor $3. Every
  input file must be local: freeze and dispatch refuse dataless (cloud-evicted) files.

**Registered rule** (`dev_thresholds_v9`). A4 passes development only if all of the following hold:
- the study is valid;
- D1: mean requests at most 0.80 times A0's, and fewer than A0 in at least 10 of 16 blocks;
- D2: no more hidden-test failures than A0;
- D3: all five controls realized, with no skipped necessary verification;
- D4: covered repeat attempts in at most 5 of 16 positives (v8's 2 of 6, scaled down).

The study is valid only if all of these hold:
- every hook-arm session has its contract, a receipt and no withheld approval;
- requests equal the debug log's dispatches;
- no session is censored;
- every control is realized;
- each hook arm's card arrived in its form.

A2f vs A0 is reported with the same D1, D2 and D4. A2f has no controls, so it is never BEST. BEST is
chosen by the r9 rule, and only A4 can be BEST.

**Budget.** The canary/development ceiling rises from $12 to $18, and the protocol total from $38
to $44. Held-out keeps $26 and training $0.

At r10 the ledger has $24.6504369 admitted across 163 attempts, leaving $35.3495631 of $60. Suppose
both the new native session and dev-v9 use their whole caps ($0.50 and $8.50). Then $26.3495631
remains. Held-out's $3 ledger floor then leaves $23.3495631 for held-out, so held-out would stop at
the floor before its own ceiling if every study hit its worst case. Expected dev-v9 spend is about
$6.6, at v8's $0.125 per session, and a native session has cost about $0.03.

**Run location and native confinement.** dev-v9 is frozen and run from a fresh clone outside iCloud
(`/private/tmp/adage-launch-src`). Its manifests bind that clone's paths. The runner sources are
byte-identical to those that `launch-gate-v5` reviewed, but a native review binds them by path. A new
cohort, `launch-gate-v6`, is therefore prepared from the clone. Its native-confinement session (paid,
$0.50 cap) must run and pass review before dev-v9's `prepare`.

### Revision 11 (development): scoring that tells the context's runs from the gate's

dev-v9 stopped by its own rule after order 0, the failed control, which it reported as "not
realized". The session did exactly what the design intends:
- the agent edited the tests and CHANGELOG first;
- the learned context ran pytest, and the run failed;
- the failure was delivered with its failing tests;
- the agent fixed `validate.py` with two edits and stopped without running any tests;
- the finish-line gate ran the approved command at Stop, and it passed on the final sources.

Version 2 of the scoring read the gate's passing run as a successful context run, so the control
could not be realized. That is a scoring-definition error, not a product failure. This fix was
registered before any positive block ran, and `dev_thresholds_v9` is unchanged.

**Production.** Every journal receipt now records its origin: `context`, `obligation`,
`stop-gate` or `manual`. The origin never enters the receipt's identity. A journal written before
this revision has no origin, so a run journaled at Stop is read as the gate's.

**Scoring** (`receipt_replay` version 3, `launch_canary.realized`).
- **Necessity** is decided by the learned context's runs alone. A failed or stale context run makes
  verification necessary. So do a requested rerun and a requested wider scope.
- **What meets it.** Any passing run on the final sources meets the need: the agent's, the
  context's, or the gate's. There are two exceptions. A requested rerun is met only by the agent's
  own run, because the user asked the agent. A wider scope is met only by a run of that scope.
- **Realization** is judged on context runs:
  - failed: the first context run failed;
  - later-edit and config-change: a successful context run on sources that are not the final ones;
  - positive: a successful context run on the final sources;
  - broader and rerun: a successful context run.
- **Gate runs** are reported apart: their count, outcome, and whether they covered the final
  sources.
- **Held-out G2 and G7** count context runs only.

**dev-v9b.** dev-v9 is archived as aborted (`results/20261001-dev-v9`). It cost $0.0675211 for
one session, plus $0.0164897 for the `launch-gate-v6` native session. dev-v9b refreezes the same
design with these scoring sources, from a fresh clone at the same path outside iCloud. The
`launch-gate-v6` native review still applies: the runner sources it binds (`probe`,
`real_repo_gate`, `real_repo_pairs`, `cohort_v5`, `budget`) are unchanged, byte for byte, at the
same paths.

### Development canary-v9 result (dev-v9b, recorded 2026-10-01 under r11)

**What this is.** dev-v9b is a development result on one task (`mm-curve--finite`), run with
development scaffolding: every prompt carries a supplied test block and a work order. The design
was iterated on this same task (canary-v5, canary-v7, dev-v8b, dev-v9). It qualifies A4 as BEST for
the held-out comparison under r9. **It is not a launch statistic, not held-out evidence and not a
savings estimate.** No threshold or rule changed. The record is in
`evals/reflex-proof/harness/results/20261001-dev-v9b` (`RESULT.json`, `report.json` and the 53
collected rows).

**Validity.** All 53 sessions were collected, at a known cost of $5.245021 with no unknown cost.
The report is valid with no problems. Its 26 frozen sources match commit `91a9714` byte for byte,
and so does its copy of this protocol's JSON.

**Mean model requests per positive** (16 paired blocks):

| Arm | Requests | Ratio to A0 | Blocks fewer than A0 | Status |
| --- | --- | --- | --- | --- |
| A0 | 18.0 | | | |
| A2f | 15.06 | 0.837 | 13/16 | not eligible (no controls) |
| A4 | 11.19 | 0.6215 | 15/16 | passed D1-D4 |

- D2: there were no hidden-test failures in any arm (16/16 in each).
- D3 was tested: all five A4 controls were realized, and none skipped necessary verification.
  - failed, later-edit and config-change were each met by a passing Stop-gate run on the final
    sources (`receipt_replay` version 3);
  - broader and rerun were met by the agent's own runs.
- D4: sessions with repeat attempts were A0 0, A2f 2 and A4 1 (the maximum allowed is 5).
- **BEST = A4**, with the orientation card on.

**Reported, not gated** (means per positive, A0 against A4):
- cost: $0.117 against $0.0825;
- pytest requests: 2.81 against 0.19;
- cache-read tokens: 445k against 245k;
- wall time: 57.7 s against 50.5 s.

**What it does not establish:**
- benefit on held-out tasks;
- benefit on other repositories;
- benefit on prompts without the test block and work order;
- any GPU effect.

Model requests are counted against the debug log's dispatches. CPU runs (receipts and the Stop
gate) are reported apart and are never counted as inference saved.

**Held-out run location.** The held-out phase runs from a separate fresh clone,
`/private/tmp/adage-heldout-src`, which contains the main merge (`32e1cfa`). The confinement review
binds runner sources by path, so this clone gets its own native-confinement cohort. Its native
session ($0.50 cap) must pass review before held-out `prepare`. This follows the existing
confinement rule and changes no rule.

`launch-gate-v7` was prepared offline, but its native recipe cannot be admitted: cohort native
recipes carry no ledger allocation, so they fall back to the legacy $27.50 admission ceiling, which
the ledger has passed. The remedy needs the user's decision and is recorded in the handoff. Nothing
was dispatched.

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

### Held-out Marshmallow tasks (revision 9 draft)

The four evaluation tasks were authored, validated and frozen before any development canary-v8
result was read. They were frozen at commit `260729ae0723531fd2c40308a98cdad71257f9ae`
(2026-10-01T06:09:53Z). The manifest is `harness/results/20261001-heldout-tasks/manifest.json`, sha256
`02341cc07e54627f60767c39ef22747250e52d388873c715d6242b156646f556`. Definitions are in
`harness/heldout_tasks.py`, and the archived command outputs are in that results directory's
`README.md`.

| Task | Group | Validator | Prompt sha256 | Hidden test sha256 | Gold patch sha256 |
| --- | --- | --- | --- | --- | --- |
| `mm-curve--has-keys` | mapping-keys | `HasKeys(keys)`: a mapping containing every key | `5daff605…` | `4d11270b…` | `0499b1e9…` |
| `mm-curve--iso-date` | temporal-format | `IsoDate()`: exactly `YYYY-MM-DD`, a real date | `5f29842f…` | `13ab2ae5…` | `4e2b1de6…` |
| `mm-curve--json-object` | structured-text | `JsonObject()`: `json.loads` gives an object | `423da26b…` | `1f812a5c…` | `9a19e415…` |
| `mm-curve--lat-long` | geo-coordinate | `LatLong()`: a `(latitude, longitude)` pair in range | `346a2951…` | `d28a3dbc…` | `3a046e61…` |

- **Prompts.** The earlier neutral mm-curve template asks for the validator, tests, a docstring and
  a changelog entry, and shows only the first valid and first invalid example. Every held-out prompt
  adds one realism line, identical in every arm: `Do not create git commits.`. There is no work
  order, no test block and no verification instruction; agents write their own tests.
- **Validation** (offline, a copy of the pinned substrate, Python 3.12.14, pytest 9.1.1, base
  `7f0792bd`):
  - each base fails only the hidden tests;
  - each gold passes the hidden tests and the full suite;
  - each task's three broken implementations fail hidden behaviour tests only;
  - the gold hidden-test identities are unique.
- **max-sum withdrawn.** Its `(max, *, error=None)` interface and measure-at-most-`max` predicate
  match the size-limit group. Its check similarity to max-words is 0.73; the registered pair
  max-utf8-bytes/max-words scores 0.74. It joins size-limit, is never run, and stays recorded with
  its validation. lat-long (geo-coordinate) replaces it.
- **Isolation.** `split_audit.py` on `cumulative-launch-10` passes with the new markers. A scan of
  671 earlier session files finds no authored family name. Unlike the transfer-v3 tasks, none of
  these families ran in any cold pilot or earlier session.
- **Natural workflow, recorded before any run.** Of 31 earlier unguided sessions on this template,
  31 edited `validate.py`, then tests, then `CHANGELOG.rst`. 27 produced run-175d85's context
  (tests then changelog within eight events). The context is expected to fire in most held-out
  sessions; the tasks were not tuned for it.
- **Disclosure.** Before the coordinator's lock (2026-10-01), the task author's read-only scan for
  the natural tests-then-CHANGELOG base rate read the edit order (only) in 3 dev-v8b A0 transcripts
  (`00-A0-positive-b1`, `07-A0-positive-b2`, `10-A0-positive-b3`): the file paths of their
  Edit/Write calls. For `07-A0-positive-b2` it also confirmed that one Edit to a nonexistent path had
  failed with a tool error. It computed no outcome metrics (no request counts, costs, quality or arm
  comparisons), and those sessions are excluded from the base-rate scan.

### Revision 9 (draft): pre-registered Marshmallow held-out analysis

The analysis (`harness/heldout_analysis.py`) and the cohort runner (`harness/heldout_cohort.py`) are
committed with tests before any dispatch. The governing values are in the JSON's
`heldout_analysis`. The stratum is labelled **same repository, same test file family,
guided-learned route (run-175d85)**.

**Arms.**
- A0: no Adage hooks.
- NOEXEC: Adage hooks with BEST's binaries, learned home and card setting, but no execution grant and
  no E2 policy, so no contract and no execution.
- BEST: the dev-selected design built from the final pre-freeze commit. That commit includes generic
  correctness fixes made before any held-out run (for example `codex/ship-fixes` and
  `codex/first-run-fixes`). BEST gets one person-issued E2 grant.

The selection rule picks the development canary-v8 arm (A2 or A3) that passes D1-D4 with the lowest
mean requests. Ties go to the lower mean cost, then the name. If no arm passes, the held-out
comparison is not run.

The build commit and the `adage`/`adage-exec` sha256 values are inputs to `heldout_cohort.py freeze`,
never constants. Freeze requires the runner checkout to contain the build commit, with identical,
clean production sources.

**Design.**
- 4 tasks x 5 repetitions give 20 paired blocks.
- Each block runs all three arms on the same task, repetition, frozen prompt and fresh checkout.
- The arm order within each block is drawn from `random.Random(20261001)` and recorded in the frozen
  matrix.
- Dispatch is repetition-major, one session at a time.
- The approval script grants only BEST's homes (its 5 controls and 20 block sessions). NOEXEC and A0
  hold no grant and no policy, and `verify-grants` checks both.

**Primary metric.** Model requests per session: the count of `[API REQUEST] /v1/messages` lines in the
debug log. Retries are real, billed requests and count. Every session is cross-checked against the
stream's distinct request ids, and the result is reported.
- A mismatch is verified only when it is fully explained by logged `[ERROR] API error (attempt N/M)`
  entries, that is 0 <= dispatches - ids <= entries. The archived v5 `click-type--hex-color` session
  has 62 dispatches, 60 ids and 2 entries.
- An unexplained mismatch makes the session unverified. Its whole paired block leaves the ratio,
  symmetrically for all arms.
- If more than 2 of the 20 blocks are excluded, the outcome is `unverified`, which is not a pass.
- Quality is scored independently over all blocks.

**Headline test (the GO criterion for this stratum).**
- Statistic: sum(BEST)/sum(A0) requests over the included blocks.
- Its one-sided 95% upper bound (a percentile bootstrap over blocks, stratified by task, 20,000 draws,
  seed 20261001) must be at most 0.90.
- Quality must be non-inferior: BEST's hidden-test passes are at least A0's minus one.

**Censoring (conservative against Adage).** A session without a successful result is a quality failure
in every arm. This covers max turns, the budget cap, the wall clock or any other stop. Such sessions
are never dropped or rerun.
- For requests, a censored BEST session counts as max(observed, 60).
- A censored A0 or NOEXEC session counts its observed requests.

Censoring can therefore only hurt BEST.

**Sensitivity analyses** are reported alongside; GO stays on the primary.
1. The primary test without every block that contains a censored session.
2. The primary test without the has-keys blocks (its nearest training neighbour is `contains`). If the
   primary passes and this fails, the report says so prominently, and so must any write-up.

**G1-G9 in this stratum.** GO requires the headline test plus every criterion marked as applying:

| | Applies | Form or reason |
| --- | --- | --- |
| G1 | yes, single repository | the frozen home holds the certified route run-175d85 (checked at freeze); the two-repository clause belongs to the cross-repository decision |
| G2 | yes | in BEST positive sessions, a learned verification ran in at least 50%, and at least 75% of those had a successful receipt |
| G3 | no | eliminations were the r1-r7 primary; r8/r9 gate model requests, and the two-repository clauses cannot apply to one repository; eliminations are reported |
| G4 | no | a repeat-elimination mechanism criterion superseded by the r9 primary; repeats were gated in development (D4); the fractions are reported |
| G5 | yes | BEST failures at most min(A0, NOEXEC failures). This is stricter than the margin of one, so it governs quality |
| G6 | yes | zero skipped necessary verification, executions without a grant or in NOEXEC/A0, foreign or unapproved receipts, undeclared writes, or unknown costs |
| G7 | yes | at most 25% of BEST CPU executions in positive sessions unnecessary; controls are stale by construction and excluded |
| G8 | yes | BEST requests at most NOEXEC's, and BEST cost at most 105% of NOEXEC's (included blocks, censoring rule) |
| G9 | no | the pipeline-unseen repository (more-itertools) is not run here; G9 stays required for any claim that covers it |

**Safety controls.** The five controls run first, in BEST, on the development task with the validated
canary suffixes. Each control prompt is the development prompt, then the same realism line
(`Do not create git commits.`), then the suffix. Every session in the study therefore carries the
line. Development showed git-commit fights with the sandbox costing up to 9 requests, which could
also trigger budget censoring. Realization comes from the suffix alone.
`results/20261001-heldout-tasks/control-prompts-check.*` re-validates the composed prompts offline:
- each keeps the development prompt, one realism line and the intact suffix and test block;
- their test blocks pass a gold `Finite` and fail the broken ones on the pinned Python 3.12.
- Each control must be realized (`launch_canary.realized`) and must skip no necessary verification.
- An unrealized control is a protocol failure, not a pass. The runner checks all five before
  dispatching the first block, and stops on any failure.

**Verification classifier (reported, not gated).** It counts Bash requests running any of: pytest in
any form (`pytest`, `py.test`, `python -m pytest`, `uv run pytest`), `tox`/`nox`,
`python -m unittest`, `python -c`, Python reading stdin or a heredoc, or a test script.

**Budget.** 65 sessions (60 held-out plus 5 controls), each capped at $0.40 and reserved at the cap.
- The worst case is $26.00. The held-out ceiling becomes $26: training's $4 moves to held-out, and
  Click and more-itertools training gets a new allocation later. The protocol total stays $38.
- The ledger had $39.7316293 remaining before development canary-v8.
- None of the 78 earlier Haiku Marshmallow sessions exceeded $0.40 (maximum $0.3365, mean $0.1905).
- Expected spend is $7.15-$9.75 at $0.11-$0.15 per session, or $12.38 at the historical mean.
- Dispatch stops if spend plus one cap would exceed $26, or if the ledger's remaining balance would
  fall below $3. An incomplete matrix is not a pass.

**Outcomes.**
- `pass`: a complete matrix, realized and safe controls, at most 2 excluded blocks, the headline
  bound, quality, and every applicable G criterion.
- Otherwise the outcome is `fail`, `unverified`, `protocol-failure` or `incomplete`.

Report only: NOEXEC/A0 and BEST/NOEXEC ratios, a sign-flip permutation p-value, tokens, cost, wall
time, verification requests and CPU work. Model requests are never presented as GPU savings.

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
- 2026-10-01 r12, before any held-out dispatch: cohort native recipes bind the person's
  evaluation allocation (`evaluation-allocation.json`, listed in `shared_sources`) instead of the
  legacy $27.50 admission ceiling. The user approved it in chat and applied the `cohort_v5.py`
  edit themselves. The $0.50 native cap, the $3 floor, the $60 ledger ceiling (not raised) and old
  cohorts' $27.50 fallback are unchanged. `launch-gate-v7` is superseded before any dispatch;
  `launch-gate-v8` replaces it.
- 2026-10-01 r11, development only, a scoring-definition fix registered after dev-v9's first control
  (order 0) and before any of its positive blocks, and before any held-out dispatch:
  - journal receipts record their origin;
  - `receipt_replay` version 3 takes necessity from context runs; any passing run on the final
    sources meets it, the gate's included (a rerun only by the agent's own run, a wider scope only
    by a run of that scope); gate runs are reported apart;
  - realization is judged on context runs;
  - held-out G2 and G7 count context runs only;
  - `dev_thresholds_v9` is unchanged; dev-v9 is aborted and archived, and dev-v9b is frozen;
  - ledger at r11: 165 attempts, $24.7344477 admitted, $35.2655523 remaining.
- 2026-10-01 r10, development only, before any held-out dispatch:
  - dev-v8b archived as invalid for its execution arms from session 09;
  - fail-loud reports, a protected runtime, and a grant check before every dispatch;
  - production: the orientation card replaces the exemplar, and receipts name the session's new
    tests;
  - `codex/ship-fixes`, `codex/heldout-tasks` (r9) and `codex/first-run-fixes` merged;
  - development canary-v9 with A0, A2f and A4 in 16 blocks plus five A4 controls, under rule
    `dev_thresholds_v9`;
  - canary/development ceiling $12 to $18, protocol total $38 to $44 (held-out keeps $26);
  - ledger at r10: 163 attempts, $24.6504369 admitted, $35.3495631 remaining.
- 2026-10-01 r9 (draft on `codex/heldout-tasks`; registered only when merged, before any held-out dispatch):
  - the held-out Marshmallow tasks are authored and frozen (commit `260729a`);
  - max-sum is withdrawn as a near-duplicate of size-limit and replaced by lat-long;
  - every held-out prompt carries the realism line, with no work order or test block;
  - the pre-registered analysis and runner cover:
    - arms A0/NOEXEC/BEST, with BEST built from the final pre-freeze commit;
    - 4 x 5 paired blocks;
    - debug-log requests with a per-session cross-check, where unexplained mismatches exclude their
      block and more than 2 exclusions make the result unverified;
    - a headline 95% bootstrap bound of at most 0.90 plus quality within one;
    - censoring conservative against Adage, with censoring and has-keys sensitivity analyses;
    - G1, G2, G5-G8 applying and G3, G4 and G9 not applicable with reasons;
    - realized controls first, whose prompts also carry the realism line (re-validated offline);
    - a $0.40 cap with a $26 worst case, and $26 held-out / $0 training (total $38);
  - it records the task author's disclosure.
- 2026-10-01 r8, development only: Lever 2 and Lever 1 in production code; development canary-v8
  with arms A0-A3, the production hook registration, and thresholds D1-D4; ceilings $12
  canary/development, $4 training and $22 held-out. Ledger at r8: 128 attempts, $20.2683707
  admitted, $39.7316293 remaining.
- 2026-10-01 r7, before any held-out dispatch: scorer version 2, realistic project environment,
  test evidence in pytest receipts, later-edit by construction; result substitution out of scope.
  Ledger at r7: 128 attempts, $20.2683707 admitted, $39.7316293 remaining.
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
