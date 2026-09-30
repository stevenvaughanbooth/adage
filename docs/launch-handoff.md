# Launch handoff — 2026-09-30

## Objective and current decision

Ship a useful open-source developer preview: learn recurring behavior across sessions, predict
when a bounded action helps, run approved actions on the CPU, and demonstrate that the agent
actually avoids work without losing correctness. Do not declare broad launch readiness from
compilation, a successful hook, or advisory benchmark gains alone.

PR #14 merged into main at `e5f5e33651df86a3b1dc89bafb6ba10e9193c2e1`.
This follow-up uses branch `codex/launch-first-reflex` in
`/Users/stevenbooth/.codex/worktrees/adage-reflex-proof/skill-compile`.
Follow-up PR: https://github.com/stevenvaughanbooth/adage/pull/16 (draft, mergeable when checked).
The primary checkout is `/Users/stevenbooth/Documents/skill-compile`; leave its local work alone.

The user lifted the Codex 5% reserve and authorized using the remaining allowance. The independent
paid evaluation ledger still has a $60 cap. At the last settled snapshot: $13.2920411 measured
provider cost plus a separately human-approved $0.50 conservative unknown charge, totaling
$13.7920411 across 62 attempts before transfer. After the completed eight-session transfer:
**$15.1229241 measured + $0.50 conservative charge = $15.6229241**, 70 attempts, **$44.3770759
remaining**. All settled or cap-charged; no active paid transfer process. Read the live ledger before scheduling anything; never reset it,
double-dispatch, or treat an unknown bill as zero.

## What is established

- Sixteen cold-start assignments on four Marshmallow tasks, two repetitions per arm: instincts
  8/8 correct versus 6/8 alone; $1.7720458 versus $1.9429724; 272 versus 304 model requests.
  Combined reductions: 8.8% cost, 10.5% requests. Three cost regressions remain. Replication alone
  saved only 0.6% with equal requests. These are advisory instincts, not automatic CPU savings.
- Fifteen organic training sessions produced no executable candidates. Preserve that negative.
- Ten explicitly guided sessions across five repeated tasks produced four certified triggers for
  **one** pytest routine, with 9/10 qualifying continuations. Task quality was 9/10. Three triggers
  are sequence contexts. This is real cross-session learning, not autonomous routine discovery.
- Production execution-prefix admission now retains routine actions without relaxing the
  execution evidence bar. Execution-only candidates cannot fall back into ordinary advice.
- The earlier generated lookup benchmark proved action elimination; real coding transfer remains
  a separate open gate. The completed real-coding study ran the learned CPU reflex successfully
  in 4/4 automatic sessions, with 4/4 correct outputs versus 3/4 baseline. **0/4 confirmed action
  eliminations:** every automatic session requested the same command again after its receipt.
  GPU utilization and energy were never measured.

Website evidence is in `web/site/index.html` and `web/site/public/evidence/cold-start.json`.
All study records are indexed by `evals/reflex-proof/LAUNCH-STATUS.md`. Do not cherry-pick or delete
earlier failures. GitHub deployment `6768511910` reports successful Production deployment of the
PR #14 merge commit. Its reported URL is `https://adage-site-r5gqonovm-adage3.vercel.app`;
public-domain accessibility was not independently verified. Recheck future deployments after merge.

## CPU transfer study and approval boundary

Local archive: `/private/tmp/adage-real-repo-gate-v1`.
Shared accounting: `paid-state.json` and `evaluation-allocation.json` there.
Cohort: `cohorts/curve-guided-v2`.
Original learned artifacts: `cumulative-10`; first cache-fix replay: `cumulative-cache-10`.
Current replay: `cumulative-launch-10`; current prepared study: `transfer-v3`.
Current human script: `/private/tmp/adage-real-repo-cpu-approve-v4.sh` (earlier scripts are superseded).

**Completed:** all eight `transfer-v3` assignments are collected and settled. Executor session
58750 has exited; `/private/tmp/adage-transfer-v3-run.json` contains the run output. **Do not
dispatch this study again.** The result and diagnosis are in
`evals/reflex-proof/harness/results/20260930-live-cpu-transfer/FINDINGS.md`.

To inspect the existing completed study, from the worktree run:

```sh
python3 evals/reflex-proof/harness/curve_transfer.py report \
  /private/tmp/adage-real-repo-gate-v1/cohorts/curve-guided-v2/transfer-v3
```

The archived report and export script preserve the original decision even after future harness
changes. Do not overwrite frozen roots, their task material, histories or binaries. Any new paid
study needs its own prospective protocol and assignments; do not reuse these outcomes as unseen.

The frozen transfer study compares `learned` (same artifact, no execution permission) against
`automatic` (one human-approved context). Eight sessions cover four tasks excluded from training,
but previously observed in cold pilots. Prompts contain no workflow guidance. Select the first
certified context deterministically: `run-175d85`, edits to `tests/*.py` then `CHANGELOG.rst` within
eight events; command `.venv/bin/python -m pytest tests/test_validate.py -q`.

Gate: at least three confirmed action eliminations, all eight complete, all quality checks passing,
known costs, exact source/receipt/approval checks and no unknown shell behavior. Alternate pytest
scopes do not qualify as the same avoided action. Preserve a negative result; do not loosen scoring
or rerun outcomes. Runtime bin hashes and source hashes are frozen; after a code change, preserve
the old root and prepare a new one. Never overwrite the training or replay history.

`transfer-v1` stopped before dispatch because `.pytest_cache/**` was rejected. `transfer-v2`
also stopped before dispatch: missing E2 policy, then the human's sandboxed approval run passed
pytest but refused `.pytest_cache/.gitignore`. No grants were saved. The new fix supports exactly
that additional cache file; it does not authorize repository `.gitignore` changes or hidden outputs.

**Agents must never issue grants.** `require_person` rejects agent sessions. Do not strip environment
markers, write `approvals.json`, or execute the human approval script yourself. User budget permission
does not grant product execution permission. A complete human script must explicitly opt only its
four temporary homes into E2, keep automatic approval off, require sandboxing, and declare both
`.pytest_cache/**` and `.pytest_cache/.gitignore` plus scoped bytecode caches. Check the resulting
four grant files before dispatch. Earlier scripts are historical; do not reuse their paths blindly.

## Launch experience and next priorities

### Latest checkpoint: receipt integration and evaluator isolation

PR #16 remains draft. There are no paid processes running. Shared ledger now has **100
attempts**, $15.6474504 measured plus the previous $0.50 conservative charge =
**$16.1474504 admission spend**, **$43.8525496 remaining**. Accounting is unblocked.
The latest diagnostic work cost $0.5245263 including failed/excluded attempts.
Codex allowance last checked at 98% used (2% remaining); the user authorized the remainder.

Research: [hook action reuse](hook-action-reuse-research.md). Native Read delivery canaries
all passed ($0.0288169), archived in `results/20260930-delivery-canary` under the harness.
Both context and valid result replacement delivered random markers; malformed replacement
and no-result controls did not. This verifies native delivery, not production action avoidance.

A **real evaluator bug** was found during the edit diagnostic: Claude inherited launcher
stdin and one agent reproduced the launcher Python program as an attempted command.
`probe.execute` now uses `stdin=DEVNULL`; the intended prompt is already in argv. A real
child-process regression fails on the old implementation and passes after the fix. All 85
harness tests pass. Preserve `receipt-edit-v1` as contaminated; the earlier heredoc-launched
read comparison `receipt-v3` is also excluded from behavioral conclusions. Do not promote
its apparently positive rates. `receipt-v2` previously stopped on a navigation/permission
confound and remains excluded. All outcomes and frozen code are archived.

Clean fresh `receipt-edit-v2` ran eight assignments after stdin isolation, $0.1513985 total:

| Arm | Agent requested verifier | Independent quality | Successful CPU runs |
| --- | --- | --- | --- |
| Baseline | 2/2 | 2/2 | 0 |
| Current-style receipt | 2/2 | 2/2 | 2 |
| Structured receipt | 1/2 | 2/2 | 2 |
| Structured receipt + startup responsibility | 0/2 | 2/2 | 2 |

Archive: `evals/reflex-proof/harness/results/20260930-edit-receipt-ablation`. This is an
authored function-fix/changelog fixture, not learned real-repository evidence. Two repetitions
per arm are a diagnostic signal, not a reliable effect estimate. Current-style text was labeled
simulated; acknowledge that difference. No new launch-site statistics were added.

The candidate production integration is now implemented in the existing pipeline:

- `Extension::session_context` supplies a bounded model-facing responsibility contract only
  in treated supported sessions with a scoped E2 verification approval and E2 policy enabled.
  A stored grant is not a promise of execution; execution still rechecks the actual request.
- SessionStart keeps model context separate from the person's notice; muting notices does not
  mute the contract. Control, damaged-index, no-extension and no-grant paths remain silent.
- Successful learned CPU actions deliver journal operation ID, declared-input digest, exit
  status and duration, with explicit scope and later-change caveats. Cache-only writes are
  no longer described as no repository changes. No raw stdout is newly exposed.
- The contract uses existing provenance tags. Learning conservatively treats the continuation
  as guided from session start, rather than learning its own instructions as organic evidence.
  This reduces eligible organic training while approved verification is enabled; control
  sessions remain important. Do not silently relax this contamination boundary.

Local validation: execution/runtime/hook library suites pass; startup treatment/control,
notice separation, no-grant/E1 policy, real journal receipt identity and learning provenance
regressions pass. These tests do not establish a live effect from this exact production text.
The diagnostic supplied fixture-specific test counts; production receipts intentionally do not
expose undeclared stdout or invent test counts. That distinction needs prospective validation.

The next agent should:

1. Run a production-binary SessionStart/receipt canary, then stale-input, failed verification,
   changed environment, broader scope and explicit-rerun controls. Do not implement command
   denial to disguise repeats; no permission or execution grant may be bypassed.
2. Prepare a NEW real-repository cohort with fresh frozen binaries, replayed original training,
   updated native-confinement verification where source identity requires it, and corrected
   prospective cache-only scoring. The old binary hash allowlist must remain a check: replace
   it only with measured new hashes in the new reviewed protocol. Never overwrite old studies.
3. Obtain new person-issued grants for new evaluation homes after preparation. Do not copy old
   approvals. Freeze the whole matrix before the first paid agent. Keep the shared ledger.
4. Measure repeated tool requests, model requests/tokens/cost, quality and necessary reruns.
   Include a no-execution control. A small authored diagnostic is not a replacement for this gate.

The four old real-repository repeats occur in new assistant messages after the hook attachment,
not in the same message as the triggering edit. Message IDs are not API request IDs. A scan of
all eight old native transcripts found no known launcher-code markers; that negative scan does
not prove full historical prompt isolation. Preserve the frozen failure and use the fixed runner
for all new evidence. Research and diagnostics identify a promising integration direction;
open-source launch readiness is still not proven.

The follow-up adds `docs/first-reflex.md`, first-run links to review and execution status, clearer
cache errors, and regression coverage for pytest's hidden metadata. It preserves the current TUI.
Do not create another learner or parallel onboarding implementation.

1. Address the observed duplicate-work failure. Native transcripts confirm that the CPU completion
   reached the model as `hook_additional_context`, yet every automatic arm requested the exact test
   again (2/1/1/1 times). Build an offline regression from these events, investigate how to present
   completed verification with explicit scope and freshness, and measure action avoidance separately
   from merely suppressing a subprocess after the model has already requested it.
2. Before a new paid study, fix receipt scoring prospectively: approved cache-only changes currently
   fail the no-changes receipt filter despite unchanged final source digests. Predeclare appropriate
   pytest flag equivalences. Preserve this failed result; a scoring correction alone cannot erase
   the actual duplicate agent requests. See the post-hoc delivery and runtime diagnostics.
3. Walk the first-reflex guide on a fresh supported installation. Verify hook registration, history
   discovery, empty-history behavior, candidate review, policy/grant setup, result delivery and revoke.
4. Improve naturally occurring history coverage; current organic-versus-guided gap is material.
   Measure unnecessary executions, duplicates, abstention and drift along with saved actions.
5. Run release platform CI before claiming all advertised binaries and harnesses work. Local macOS
   success is not Linux or Windows evidence; Windows automatic execution is unsupported.
6. Replicate on another repository/model before treating the pilot as a general savings estimate.
   Frontier-to-cheaper-model transfer and a marketplace remain future hypotheses.

Do not promise a universal session count for learning or guaranteed savings from `init`. The launch
pitch should show a concrete demonstrated workflow, publish its limits, and invite developers to
contribute histories and reproducible reflex cases.
