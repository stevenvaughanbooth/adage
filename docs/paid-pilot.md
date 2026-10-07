# Run a learned-reflex pilot locally

This pilot connects actual Claude Code training sessions to production corpus learning,
operator review, a pinned compiled index, and held-out paired evaluation. It uses the bundled
Python maintenance tasks and independent external grader. It is an exploratory **synthetic
paid pilot**, not evidence of customer token savings. No paid calls run until `--execute`.

The default treatment is learned **E0 advice** through the normal Adage hooks. It does not enable
automatic command execution, completion enforcement, or a different model in the treatment arm.
Both arms use the exact model revision you name.

Use a Linux operator terminal outside an agent session. Install Python 3, Git, Make,
Bubblewrap, Rust, and Claude Code; configure Claude Code authentication locally. Bubblewrap
must be able to create its namespaces. The dry plan checks configured authentication locally;
it cannot establish whether a credential is valid or expired. An authenticated paid session
can still fail, and its original evidence and reservation remain retained.

On macOS, use [the Docker Desktop setup](paid-pilot-docker.md) to run this same Linux
collector locally. It retains Bubblewrap confinement and operator-only paid dispatch.

From the Adage checkout:

```bash
cargo build --release --locked -p adage-cli
python3 evals/live/sandboxes/pilot/selfcheck.py
claude --version

export PILOT_ROOT=/tmp/adage-paid-pilot
python3 evals/live/pilot.py init --root "$PILOT_ROOT" \
  --adage "$PWD/target/release/adage" \
  --model claude-haiku-4-5-20251001 \
  --budget-usd 30 --session-budget-usd 1 \
  --max-turns 30 --timeout-seconds 600
python3 evals/live/pilot.py plan --root "$PILOT_ROOT"
```

To test the native Claude mod, initialize a **new** root with the verified Linux standalone
Claude Code 2.1.288 harness and a staged, checksummed plugin containing both binaries. Add
`--runtime-profile native-mod-bundled-defaults-e0/v1 --plugin-dir /absolute/staged/plugin`
to `init`, and set `--adage` to that plugin's `dist/<linux-target>/adage`. Keep the same
reviewed allocation; a new root is not authorization for another paid study.

The native profile pins the complete plugin, paired binaries, harness, managed settings and
supported environment. Training remains untreated: no Adage module or reflex is loaded.
Its receipts reconcile native history request IDs, debug request counts and stream usage;
missing observations or later drift stop further dispatch while preserving the attempt.
After operator review, both frozen cohorts use the native profile with `off` and `mod` arms.
Custom forwarded environment and execution grants are unsupported. An observed module load
or spool directive alone does not establish useful delivery or adequate task verification.

`init` creates the complete registry and all three configurations; no unpublished input files
are needed. It pins the binary, collector, task/grader sources, model, Claude Code version,
and settings. Keep that checkout and binary unchanged while collecting. `plan` is read-only
and reports missing prerequisites. Resolve its blockers before execution. An absent Claude
version at initialization requires a fresh pilot after installing the harness.

A private repository workload can use `init --pack /absolute/private/pack`. The pack is
trusted local code: `generate.py --list` returns the split/group registry, `generate.py
--out DIR --task ID` creates a fresh checkout, and `grade.py --workdir DIR --task ID` returns
the independent grade. Keep the pack under a hidden root outside the agent's tool paths.
Its complete files are pinned with the collector; remote graders and tool inputs also need
an independently pinned closure and confinement checks before admission.

Custom packs must include `analysis.json` with exactly `evidence_kind`, `classify`,
`obligation`, and `labels`. Use `exploratory_repository_pilot` (or `synthetic_paid_pilot` for
synthetic tasks). `classify` declares string arrays for `lookup`, `regen`, `verify`,
`bash_edit`, and `check_flags`; `obligation.paths` is a path glob or explicit null; `labels`
contains nonempty `lookup` and `regen` names. These fields describe observed commands and
edits. They do not establish adequate verification, override the independent grader, or
change confinement, approval, model, budget, or learning thresholds. The bundled Python
pack retains its existing defaults. A custom schedule needs its own reviewed allocation;
choosing a pack never authorizes paid dispatch or a customer savings claim.

The fixed schedule reserves 20 training sessions, four validation sessions, and six test
sessions. At $1 per session the absolute admission budget is $30, allocated $20/$4/$6 across
the stages. Training uses ten semantic issue groups with two variants each. Validation uses
two new groups; test uses three further groups, including an unrelated documentation control.
Each held-out group gets a fresh baseline and treatment checkout. Related variants never
cross splits or count as independent held-out groups.

The runner passes the per-session dollar cap to Claude Code, limits turns and wall time, and
reserves the full cap before dispatch. Native CLI cost estimates govern admission; they are
not provider invoices. Unknown costs or observed overruns stop subsequent dispatch across
the pilot. A provider can settle an in-flight request above a CLI cap, so this is not a
guarantee about the final invoice. Budget accordingly.

```bash
# Paid, untreated sessions; rerunning resumes only untouched scheduled jobs.
python3 evals/live/pilot.py train --root "$PILOT_ROOT" --execute

# Offline: native histories → canonical corpus → real production learner.
python3 evals/live/pilot.py learn --root "$PILOT_ROOT"
ADAGE_HOME="$PILOT_ROOT/learner-home" target/release/adage review
```

Inspect the actual candidates and their evidence in that review output. From the same
operator terminal, approve a chosen candidate with
`ADAGE_HOME="$PILOT_ROOT/learner-home" target/release/adage review approve REFLEX_REFERENCE ROUTE_ID`,
using the reference and route ID printed by the queue. Review is an explicit human decision;
the coordinator does not approve candidates or remove agent-session protections.

**No useful candidate is a valid result.** Inspect the recorded learn summary, support,
canonical traces and review evidence. Do not insert an authored reflex, substitute the diagnostic fixture
index, lower the production learning thresholds, or relabel task families to force progress.
Start a separately registered pilot if new training evidence or a different design is needed.

```bash
# Offline: attest review and freeze both cohorts before any held-out execution.
python3 evals/live/pilot.py freeze --root "$PILOT_ROOT" --reviewed-by "Your name"

# Without --execute, run prints the readiness plan and makes no model calls.
python3 evals/live/pilot.py run --root "$PILOT_ROOT" --split validation
python3 evals/live/pilot.py run --root "$PILOT_ROOT" --split validation --execute
python3 evals/live/pilot.py report --root "$PILOT_ROOT"

# After inspecting validation, explicitly release the untouched test cohort.
python3 evals/live/pilot.py run --root "$PILOT_ROOT" --split test --execute
python3 evals/live/pilot.py report --root "$PILOT_ROOT"
```

Freeze verifies that the live compiled E0 routes came from this pilot's recorded production
learner output. It checks full canonical route payloads, the reviewed compiler output, the
unchanged corpus generation, and actual checkout routing. Changing state or strength through
review is allowed; rewriting the learned procedure or adding execution bindings is not.

Before releasing test, inspect validation for baseline-pass/treatment-fail pairs and whether
the learned advice actually applied. Diagnose routing or task regressions before buying more
sessions. Test requires complete validation usage and independent grades; it does not require
a positive token-savings result. Do not discard, replace, or rerun unfavorable pairs. Any
change to the frozen treatment requires a new pilot.

Each root retains these reviewable artifacts:

| Artifact | What it records |
| --- | --- |
| `pilot.json`, `*-config.json`, `registry.json` | Pinned inputs, bounds, stage allocations and honest split/group identities |
| `train/state.json`, `train/started/`, `train/receipts/` | Original reservations, attempts and immutable receipt references |
| `train/private/transcripts/`, `training-bundle/` | Native histories, canonical sessions, task/outcome mapping and source hashes |
| `learning.json`, `objects/` | Production import/learn/audit results, original canonical routes and review queue |
| `review.json`, `reviewed-index.json`, `*-frozen.json` | Operator attestation, exact learned artifact and frozen evaluation protocols |
| `{validation,test}/runs/`, `receipts/`, `report.json` | Native request streams, independent grades and paired results |

`report` includes raw request input/output/cached-input tokens, model-call counts, known native
CLI cost estimates and unknown estimate counts for each stage. Cached input is a subset of
reported input, not an additional token total. Provider billed cost remains unknown. Compare
the frozen report's `evidence_complete` separately from `paired.token_target_accepted`: a fully
collected negative or statistically underpowered result is a completed pilot.

An interrupted started job without a receipt, deleted accounting, changed source, unknown
usage or grader error blocks further paid work. Training receipts also pin the original native
history and request stream; missing, malformed or later-altered histories stop admission.
Held-out collection stops immediately after retaining a receipt with incomplete evidence;
even a cost overrun in the final cohort job blocks later paid stages. Restore/reconcile the original evidence;
never delete the ledger or retry the same job to manufacture a clean outcome. Completed
reports can be regenerated offline. This small cohort checks collection, learned applicability
and failure modes; a customer savings claim needs substantially more independent tasks from
the customer's distribution and separate provider billing evidence.
