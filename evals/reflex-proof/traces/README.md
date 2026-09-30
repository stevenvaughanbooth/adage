# Trace sufficiency audit

## Default onboarding proof

`onboarding.py` separately exercises the production first-use path against nine archived
training sessions from the generated clinic repository. It uses `init --print`, a registration
fixture from `install --print`, the wizard's exact learner arguments, and the registered hook.
No `--outcomes`, demo thresholds, authored routes or paid model calls are used. Another
repository's history is present and must remain excluded. Agent-session guards stay enabled.

```sh
cargo build --offline -p adage-cli -p adage-exec
python3 -B evals/reflex-proof/traces/onboarding.py --output /tmp/adage-onboarding.json
```

The [saved report](results/onboarding.json) records the binary, proof script, Rust sources and
input checksums, including the dirty-worktree status. Nine sessions were read and used, producing
three learned routes (two cards and a lookup) and three repository instincts. The held-out
CARE-279 prompt received a learned card and lookup guidance through the registered hook.
This proves discovery, compilation and delivery on that fixture. It does not measure interactive
setup usability, automatic execution, avoided model requests or net task savings.

## Source audit

This is a standard-library, CPU-only **data audit**, not a second learner or an executor.
It tests whether the available time series carry the identities, original arguments,
results and context needed to study executable reflexes. It never executes a command
found in a trace, calls a model, downloads data or exports raw prompt/command text.

The current production path already has generic graph learning. Its
`adage-learn::bridge` retains literal exec argv/output/failure and prompt/turn metadata
in `adage_graph::input`. This audit deliberately examines the **source** rather than
replacing that projection: source IDs, JSON locations and checksums let future
production importer/graph changes be evaluated against the same material.

## Reproduce

From the repository root, with Python 3.9 or later:

```sh
python3 -B -m unittest discover -s evals/reflex-proof/traces -p 'test_*.py' -v
python3 -B evals/reflex-proof/traces/audit.py \
  --revision acdb0a4acd875be56821469aaeadeb29b8d6d493 \
  --output /tmp/adage-trace-audit.json
```

That audits the committed three-harness simulator and all committed live sandbox
datasets. The report additionally checked locally available S11 public history files
and the already-installed embedding model:

```sh
python3 -B evals/reflex-proof/traces/audit.py \
  --revision acdb0a4acd875be56821469aaeadeb29b8d6d493 \
  --public-root /path/to/S11-families-variance/data/swebench \
  --public-tasks 12 \
  --model-dir /path/to/Adage/models/potion-base-8m \
  --output evals/reflex-proof/traces/results/source-audit.json
```

Paths supplied for public data and the model are not written into the report. Public
source IDs are submission-relative filenames. Model weights/vocabulary are hash
checked against their manifest; no vectors are computed. Semantic similarity and CPU
embedding timing belong to the separate exact-production-model probe, not to this
audit. There is **no lexical similarity advertised as embeddings**.

## Default onboarding proof

```sh
cargo build --offline -p adage-cli
python3 -B evals/reflex-proof/traces/onboarding.py --output /tmp/adage-onboarding-proof.json
```

This runs the production binary on nine committed live Claude training sessions from
`live-20260928-py-clinic-s12`, with default thresholds and **no outcome labels**. It generates
the matching sandbox, rebases only recorded checkout paths into an isolated temporary history
directory, previews `init`, and calls the exact repository learner used by the wizard. A foreign
history fixture must remain excluded. It then invokes the registered production hook on a held-out
task prompt and asserts delivery of a learned route ID. No model is called and no trace command
is executed. Registration is a fixture from `install --print`; installer approval/UI behavior is
covered by CLI tests, not claimed by this proof. Agent-session guards remain intact.

The aggregate report records input hashes, default evidence bars, counts and delivered route IDs.
It establishes discover → learn → hook delivery for live sessions on a generated repository.
It establishes neither enterprise generalization nor execution savings or outcome improvement.

## What the audit preserves

The in-memory `Trace`, `Prompt` and `Call` objects retain every source user prompt,
original argument payload (including JSON strings), parsed arguments, original tool
output, original call/event/parent IDs, record order, assistant batch, cwd and its
basis, explicit environment field, model/request IDs, timing and result metadata.
Missing values stay missing. Unknown fields are preserved through a content-addressed
source reference: `trace.original(pointer)` reads the exact source JSON location.
No raw trace copy is saved.

These are **as-recorded** values. The committed live corpus is curated; fields removed
or output trimmed by `build_demo.py` cannot be reconstructed. A source timestamp may
be when the tool call/result was recorded, rather than the actual process start/end.
A numeric exit is read only from explicit metadata or a recognized Codex exit header;
`is_error: false` is not promoted to an invented exit code zero.

`precedes(a, b)` means a's tool response was available before b's invocation.
It never adds an edge within one assistant batch. A returned asynchronous tool result
does **not** prove its process terminated, nor does observed order prove dependency.
The reports count available-history windows of 1, 2 and 4 prior responses and preserve
the missing timing information explicitly.

## Sampling, splits and descriptive opportunities

The source manifest contains schema, dataset kind, source ID/SHA-256, opaque session
and task-group hashes, and split. A fixed hash of `(seed, task group)` assigns
60/20/20 train/calibration/test. All repeats and model/harness copies of an identified
task remain together. Unknown or multi-task simulator sessions are quarantined from
the recurrence screen. This is reproducible task holdout, **not chronological
validation**, and related task families may still share repository conventions.

Public sampling selects 12 task IDs by fixed hash, independent of outcomes or trace
length, then seeks those same tasks in each submission. It does not cherry-pick
successful or easily parsed runs. `_results.json` supplies task outcomes where
available. Submission labels do not become invented per-call model IDs. Unknown
intervention origin remains unknown.

The opportunity screen is intentionally narrow and descriptive:

1. Preserve original argv arrays, or use `shlex` only for literal single shell commands.
   Reject dynamic/compound syntax, inline environment assignment and shell expansion.
   Native structured tools remain in the sufficiency audit but outside this argv screen.
2. Count repeated shapes observed in at least two distinct training task groups.
   Interpreter script identity remains literal. Only path/ID/number operands become
   shape markers. Known steered training sessions are excluded. Unknown-origin public
   data is counted descriptively and flagged, never treated as certified training.
3. Count matching shapes on calibration and test separately. Their combined
   `heldout_*` counters mean non-training, not a tuned final test result.
4. For variable operands of those observed held-out calls, count exact literal matches
   in strictly earlier prompts and available tool outputs: unique source, multiple
   sources or unavailable. Current/future values and same-batch results cannot bind.

There is no trigger learned, no next-step prediction, no synthetic execution and no
accuracy claim. Shape repetition is not a fully specified action. Exact-copy presence
is not a causal binding; transformations could recover additional values and unrelated
text can create false candidates. These counts identify where to inspect real
value-flow induction, not the percentage of developer work that can be automated.

## Measured report on `acdb0a4`

`results/source-audit.json` includes 540 traces and 10,846 structured tool calls:

| Source stratum | Traces | Calls | Meaning |
| --- | ---: | ---: | --- |
| Simulator, three harnesses | 36 | 244 | Generated fixtures, not agent outcomes |
| Live sandbox corpus, 13 datasets | 444 | 8,036 | Real agent sessions in synthetic repositories; includes the older 90 plus newer 354 |
| Public SWE-bench sample, five source folders | 60 | 2,566 | Real repositories, 12 shared tasks; not enterprise longitudinal sessions |

The public sample has important format gaps: 12 OpenHands Devstral histories contain no
structured `tool_calls` recognized by this adapter and are reported as such, not as
zero-action agents. Another submission contains 150 `.info.log` files rather than the
supported JSON histories; it is listed in the unsupported source inventory and is
not silently parsed from free-form text. No extra data was fetched to fill these gaps.

Source availability in all 8,036 live curated calls:

- Original argument payload, result link/output, cwd, model ID and call/result record
  timestamps: 8,036 present each.
- Original source record ID, model request ID, numeric exit code and explicit environment
  field: 0 present each. Complete environment is not recoverable from cwd.
- 218 outputs explicitly carry the curator's trim marker.

For the 2,566 public structured calls, call IDs/arguments/linked outputs are present.
Per-call cwd/timing/exit/environment and model request IDs are absent in these history
views; 1,133 calls have a model ID from SWE-agent's replay configuration. Extra
trajectory state may exist elsewhere in a file, but post-action snapshots are not
silently borrowed as pre-action state.

The literal argv screen accepts 2,163 of 4,882 shell calls in the live sandbox corpus;
2,719 compound/dynamic calls are outside its scope, alongside 3,154 native tools.
Of 683 non-training literal argv calls, 474 match a shape seen in two training tasks
(test alone 181/258). Among 289 variable operand occurrences in the combined matched
calibration/test calls, 75 have exactly one prior literal source, 10 have multiple
sources and 204 have no exact prior source. This does not establish that any of the 75
can safely execute without reasoning.

In the much smaller public sample only 203 of 1,331 shell calls pass the literal-command
screen. Three of 70 non-training calls match a repeated training shape (test 1/9).
This is a limitation of this deliberately strict screen and tiny task sample, **not**
evidence that the production graph learner cannot learn public-repository workflows.

The installed `potion-base-8m@f65d0f325faa` manifest reports 256 dimensions and 29,528
vocabulary entries; both `model.safetensors` and `vocab.txt` match their declared SHA-256.

## Gates and next use

- **PASS:** 10 selftests covering original payloads/unknown fields, all prompts,
  ambiguous result IDs, same-batch partial order, no hindsight, literal shell scope,
  task grouping, steered-support exclusion and quarantine.
- **PASS:** each audited source has a checksum and resolvable JSON references;
  identified task groups do not cross split assignments.
- **QUARANTINED:** six simulator traces have unresolved/multiple task IDs and are not
  training or held-out recurrence evidence.
- **NOT ESTABLISHED:**execution readiness, executable binding precision, useful
  coverage, zero model-turn execution, task quality or cost benefit. These need
  production-learner replay and actual execution/quality experiments.

The immediate reusable value is the source manifest and conservative availability
contract. Feed these same sources through the production graph bridge and compare
which context survives; add explicit numeric termination, model-request and
environment snapshots to future instrumentation. Keep richer structured tool
arguments and temporal provenance available when adding production binders. There is
no reason to build an alternate learning engine inside this audit.
