# Protocol 5: fresh training with general sandboxed Bash

The first three Marshmallow training assignments exposed a preparation problem:
the command permission policy denied bare `python -m pytest`. The whole earlier
policy cohort is excluded from learning, including successful results. Its
attempts, transcripts and costs remain recorded. This is an infrastructure cohort
restart, not an outcome retry or an efficacy result. No held-out results informed
this revision. Task IDs, prompts, pinned upstream commits, dependency versions,
training/held-out split and held-out arm order are unchanged.

`cohort_v5.py` prepares a new data root under the original budget authority:
`/private/tmp/adage-real-repo-gate-v1/cohorts/v5`. The shared dispatcher continues
to charge `/private/tmp/adage-real-repo-gate-v1/paid-state.json`, with its existing
$27.50 admission ceiling. All earlier charges remain included. The same policy
is used for new training and both held-out arms: general Bash inside the native
sandbox, restricted file tools, no network, protected dependencies/configuration,
no unsandboxed fallback, and no Adage hooks during training. The original parent
experiment directory is denied to tool reads; only the current checkout and the
canonical managed Python runtime are reopened. This does not activate the venv,
change PATH, add a model command hint, or promise that bare `python` resolves to
the prepared interpreter. Interpreter choices and failures remain observed data.

The new root links trusted runtime/cache/base/validation inputs for preparation
and scoring only. Their canonical locations are recorded in its manifest. Each
agent receives a freshly extracted checkout with pinned dependencies installed
offline. No previous edited checkout or training transcript is reused. Hidden
scoring inputs stay outside the permitted agent read root.

The new native probe must pass before any training assignment is prepared. Its
review record must contain `passed: true`, the exact canonical `cohort_root`, and
the SHA256 `manifest_sha256` of the new native assignment. Each training and
held-out recipe freezes that review file's hash. The shared dispatcher rejects
old-cohort reviews, changed reviews, wrong recipe identities and noncanonical or
unrelated budget roots. The preparation-only loopback bind failure was retained
under `sessions/native-confinement-preparation-bind-denied`; no provider was
called in that failed preparation.

The shared runner and paired helper now support separate data and budget roots.
Their hashes changed explicitly for this protocol; no old recipe was rewritten.
The new recipe freezes the shared runner, paired helper, cohort helper, budget
code, original/cohort manifests and validation controls. An offline admission
check reaches the original shared budget admission with dispatch mocked out and
confirms unchanged ledger bytes. Actual native confinement still requires the
new paid probe; offline preparation does not establish that result.

Preparation commands (no model calls):

```sh
python3 -B evals/reflex-proof/harness/cohort_v5.py prepare-cohort ORIGINAL_ROOT ORIGINAL_ROOT/cohorts/v5
python3 -B evals/reflex-proof/harness/cohort_v5.py prepare-session ORIGINAL_ROOT/cohorts/v5
# After the exact native probe is reviewed:
python3 -B evals/reflex-proof/harness/cohort_v5.py prepare-session ORIGINAL_ROOT/cohorts/v5 --task TASK_ID
```

The existing `real_repo_gate.py run-session` entry point remains the only
training/native dispatcher. Preparation creates no execution grants. General
Bash increases the possible verification command forms, so the previous direct
pytest counter alone cannot establish eliminated verification on held-out coding
tasks: shell wrappers and scripts must be classified conservatively before a
launch claim. This does not affect collecting fresh unsteered training traces.

Before any protocol-5 dispatch, the unused native proposal `a507f5b1…` was
preserved under the cohort's `private/unused-proposals` directory and corrected.
`autoAllowBashIfSandboxed` remains false; general Bash permission is expressed
only in `permissions.allow`. The corrected review additionally binds the current
runner and policy-helper source hashes and checks the native and assignment
sandbox policies against that implementation. Both paired arms get the same
fresh private messaging socket and startup profiler setup as training. A policy
source change requires a new native review; changes confined to evidence parsing
in the paired helper do not invalidate an already observed confinement result.
