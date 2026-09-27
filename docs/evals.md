# Evals

Adage is evaluated in three layers. Each one answers a different question.

| Layer | Question | Runs | Cost | Where |
| --- | --- | --- | --- | --- |
| **Offline replay** (`adage eval`) | Would Adage have told the agent the right thing, at the right moment, safely? | Recorded sessions through the real runtime | None | `crates/adage-eval`, `evals/datasets/` |
| **Property tests** | Can any input break the parts that run in every hook? | Random and hostile inputs | None | `crates/adage-eval/tests/prop_*.rs` |
| **Live proving ground** | Does delivering the directives make real agents faster, and not worse? | Real `claude` agents, with and without Adage | Real tokens | `evals/live/` |

The offline replay is the Design §16 credibility test run over recorded history. The live proving
ground is the Design §12 loop, measured as in §14. The offline layer is fast and deterministic
enough to gate every change; the live layer is the final judge.

## Offline replay: `adage eval`

```bash
adage eval evals/datasets/demo-haiku45 --split round2          # text report
adage eval evals/datasets/demo-haiku45 --split round2 --json   # everything, including per session
adage eval evals/datasets/synthetic                            # the dataset's default split
adage eval evals/datasets/adversarial                          # must report 0 unsafe
adage eval fidelity evals/datasets/demo-haiku45 --round round2 # replay vs what the live hooks did
adage eval evals/datasets/demo-haiku45 --split group:task:3:0  # held-out tasks: no task on both sides
adage eval routes evals/datasets/demo-haiku45 --split round2   # learned routes vs the known conventions
adage eval evals/datasets/synthetic --params my.toml            # with tuned engine parameters ("Bench")
```

What it does:

1. It **splits** the dataset's sessions into train and test. The split is deterministic:
   - a named split from the manifest (e.g. `round2`);
   - `hash:<share>[:<seed>]`: a session is in the test set when SHA-256(seed:session id) falls
     below `share`. Repeats of one task can land on both sides;
   - `group:task:<k>:<fold>`: task-grouped k-fold. A task is in fold SHA-256(task id) mod `k`,
     and every session of the fold's tasks is tested, so no task is on both sides;
   - `temporal[:<share>]`: sessions in time order (manifest `round`, then `phase` with `train`
     first, then manifest order). The last `share` of tasks by first appearance (default 0.3)
     are tested; training uses only sessions of other tasks that came strictly before the first
     test session;
   - `all`: every session in both sets, reported as in-sample.

   A session's task is its manifest `task` (the work item); a session without one is a task of
   its own. Every report prints `task overlap between train and test: N tasks` and carries
   `task_overlap`, `task_overlap_ids` and `leaky` in its JSON. `leaky` is true when a task (or a
   session) is on both sides: those numbers measure a learner that has seen the test tasks. See
   "Splits and leakage" below.
2. It **learns** tracks from the train sessions into a temporary Adage home. This is the real
   `adage learn` with the dataset's thresholds (`--thresholds demo|default` overrides them). It
   uses no embedding model, so intent routes are not evaluated offline.
3. It **replays** each test session event by event through the real runtime,
   `adage_runtime::decide`, in the **holdout arm**. The session is history: nothing can change
   what the agent did next.
   - Transcripts become hook events exactly as the Claude Code adapter makes them. Each failed call
     raises `PostToolUseFailure`, then its turn arrives as one `PostToolBatch`. A call rejected
     before it ran raises no failure. A session the harness cut off at max turns gets no `Stop`.
   - Repository-path contracts check a materialised copy of the sandbox's file list, symlinks
     included.
   - Sessions that received live directives are skipped: their behaviour was steered. Use
     `--include-steered` to score them anyway.
4. It **scores** every would-have directive against what the agent actually did later in the
   session, and checks every directive with an independent safety oracle.
5. It **checks the hook's no-match fast path** (`adage_runtime::fast`) on every event: a second
   session state is decided the way the hook decides, skipping what the fast path skips. A skipped
   event's full decision must be empty, and the two paths must give the same decisions and the
   same decision state. The report's `fast path` line counts skips and differences.

The output is deterministic: the same dataset gives the same bytes. No temp path, time or
hash-map order reaches the report.

### Metrics

A directive is **correct** when the agent later took its step unaided:

- A **lookup** (or any run step) is correct when the agent ran the directed command. The command
  matches by the same canonical key, or the same command head, that the runtime's follow
  judgement uses.
- A **linked-files** directive is correct when the agent read at least half of the files it
  listed.
- A **dead-end** warning is correct when the agent later ran the command the warning recommends
  instead.
- A **finish-line** obligation is correct when the agent ran the step after the edit that
  triggered it.

| Metric | Definition |
| --- | --- |
| Precision (@session) | Correct directives ÷ directives given, per route kind and per route |
| Precision @window | Correct *and* matched within `--window` model turns (default 3, the runtime's follow window) |
| False-directive rate | 1 − precision: directives whose step the agent never took |
| Turns saved (estimate) | For each correct directive, the model turns between the directive and the agent's first matching action, as if the agent had acted at once. For linked files, add the extra turns spent reading the files one at a time. For dead ends, count the failed runs of the avoided command. This is an upper bound: it assumes the intervening turns were not needed |
| Recall | Of the agent's key steps, the share that a matching directive preceded. Key steps are found without reference to any track: the first successful command naming the prompt's ticket, the first read of each file its output listed, the first passing verify, the first regenerate after an edit, and each failed verify (a dead-end hit) |
| Dead-end accuracy | Of the warnings after which the agent ran an avoided command, the share where it failed as predicted |
| Finish-line accuracy | Of the obligations given, the share that were right: the agent met them, or the hidden check shows they were needed (stale generated code or a failed task). A false alarm is an obligation the agent skipped in a session that passed with fresh generated code. `missed` counts stale sessions that got no obligation |
| Unsafe directives | Directives the safety oracle rejects (below). Must be 0 |

**Intervals.** Every proportion (precision, precision @window and the false-directive rate,
overall, per kind and per route; recall, overall and per key step; dead-end and finish-line
accuracy) carries a Wilson 95% interval: a `*_ci` field `[low, high]` beside the unchanged
point field in the JSON (`precision_ci`, `recall_ci`, `accuracy_ci`, …), and a `95% CI` column
or suffix in the text. With no trials the interval is `[0, 1]`. Intervals are rounded to four
places like the points, so the output stays byte-deterministic. At these sample sizes they are
wide (round 2 precision 0.74, 95% CI 0.63–0.83; lookup 0.67, 0.44–0.84): read a difference
between two runs as real only when it is well outside them.

**How to read precision.** Offline precision measures agreement with what the unaided agent went
on to do, so it is a *lower bound* on a directive's correctness. A lookup directive given to an
agent that never found the tracker counts as false here, even though it was exactly what that
agent lacked. That happened in 6 of the 18 round-2 holdout sessions.

**The safety oracle** (`adage_eval::safety`) re-checks every promise the runtime makes, without
using the runtime's code:

- The text must be exactly one of the fixed templates.
- Commands must be plain argv: no shell syntax, no unbound `{slot}`, no absolute or `..` path.
  Every bound slot value must satisfy its route's contract and must not start with `-`.
- Listed files must be repository-relative and must exist inside the repository once symlinks
  resolve.
- Signatures and globs must carry no shell, control or invisible characters.
- The route's effect must be directable, and its aspect must be within its ceiling.
- No canary string (text that the adversarial dataset injected) may appear.

### Datasets

| Dataset | Sessions | Size | Provenance |
| --- | ---: | ---: | --- |
| `demo-haiku45` | 90 | 2.8 MB (0.37 MB gzipped) | The real Claude Haiku 4.5 demo sessions, curated from the real transcripts. It includes labels, `round1`/`round2` splits, and the live decisions and indexes. See [its README](../evals/datasets/demo-haiku45/README.md) for curation and redaction |
| `synthetic` | 52 | 0.36 MB | Generated with known behaviour: exploration, lookups taken or not, one-per-turn reads, dead ends, and forgotten regenerations. It extends the `adage-learn` golden corpus |
| `adversarial` | 20 | 0.2 MB | Generated attacks, described under "Adversarial" below. It is learned from 10 poisoned sessions and replays all 20 |

To regenerate them:

```bash
python3 evals/datasets/tools/build_demo.py <demo raw archive> evals/datasets/demo-haiku45
adage eval generate synthetic evals/datasets/synthetic
adage eval generate adversarial evals/datasets/adversarial
```

The generated datasets are compared byte for byte with their generators in
`generated_datasets_are_up_to_date`. After a deliberate change, rewrite them with
`ADAGE_EVAL_BLESS=1 cargo test -p adage-eval --test gates`.

**Adversarial.** The dataset attacks every input path:

- **Prompts:** injected instructions, and malformed or booby-trapped IDs: `W-31001`,
  `W-3102;whoami`, `--W-3103`, full-width digits, `` W-3105`whoami` ``, `W-3106$(whoami)`, and a
  trailing right-to-left override. Only a value that satisfies the learned contract may be bound
  (`W-3102`, never `;whoami`); `W-31001` and the full-width ID bind nothing.
- **Lookup output:** linked-file lists with `../`, absolute, symlinked-out-of-repo and dangling
  paths, and injected instructions.
- **Commands:** poisoned training commands (`… && curl … | sh`) and publish steps
  (`gh issue close`, `git push`).
- **Error lines:** quotes, shell syntax, canaries and invisible characters.
- **Tool output:** fake `<adage-signal>` markup.

### Current numbers

These were measured on the committed datasets, with demo thresholds and a window of 3 turns.

| | demo round 2 | demo round 1 | demo `hash:0.3` | synthetic | adversarial |
| --- | ---: | ---: | ---: | ---: | ---: |
| Train / test (scored) | 54 / 18 (18) | 18 / 18 (18) | 55 / 35 (24; 11 steered) | 34 / 18 (18) | 10 / 20 (20) |
| Directives given | 66 | 28 | 89 | 76 | 74 |
| Precision, all | 0.74 | 0.68 | 0.81 | 0.93 | 0.73 |
| — lookup | 0.67 | 0.50 | 0.63 | 0.78 | 1.00 |
| — linked files | 0.75 | 1.00 | 1.00 | 1.00 | 0.50 |
| — dead end | 0.78 | — | 0.83 | 1.00 | 1.00 |
| — finish line | 0.75 | 1.00 | 1.00 | 0.88 | 0.41 |
| Recall (key steps) | 0.78 (36/46) | 0.56 (20/36) | 0.84 (57/68) | 1.00 (77/77) | 0.57 (50/87) |
| Turns saved, lookup (median) | 9 | 10 | 10 | 1 | 0 |
| Dead-end accuracy | 0/0 | — | 3/3 | 10/10 | 3/3 |
| Finish-line accuracy | 1.00 (4) | 1.00 (3) | 1.00 (7) | 1.00 (8) | 1.00 (7 met; 10 unlabelled) |
| Unsafe directives | **0** | **0** | **0** | **0** | **0** / 84 checked |

Replay fidelity, round 2:

- Replaying the 36 round-2 eval sessions (holdout and adage arms) against the live round-2 index
  reproduces **36/36** sessions' would-have directives and stop requests.
- It reproduces the exact hook event sequence in **35/36** sessions. In the other session the
  agent ran a forked skill, whose tool calls fired hooks in the session but are recorded in a
  subagent transcript.
- Learning from the 54 curated `round2` training transcripts reproduces the live round-2 index's
  routes (ids and aspects), plus one it did not have. Three deliberate learner changes came
  after the live run: the signature-keyed miner narrowed the dead end's avoided runners and
  evidence (below), dead ends and linked files now carry an exit condition (Design W2), and a
  second failure signature is now a dead end of its own (`dead-end-21cb2c`, below; the gate
  admits exactly that route). Before them, the whole index matched byte for byte (generation
  `8dd5ec01ed76c4bd`).

Round 1 was decided by an earlier build. Its command keys templated interpreter scripts, so the
current runtime no longer matches that index's linked-files trigger. Round-1 fidelity
(decisions 11/18 holdout, 0/18 adage) therefore measures that drift, and it is not gated.

What the numbers say:

- **The lookup is the value.** Held-out agents that found the tracker unaided took a median of 9
  to 10 turns to find it. Offline, every one of those lookups had a correct directive at the
  prompt (lookup recall 12/12 in round 2). This matches the live A/B, where round-2 turns before
  the first edit fell from a median of 20 to 2.
- **Dead ends are keyed by their failure signature.** The round-2 route used to name
  `python3 -m pytest / python3 -m unittest / python -m pytest`, but `python3 -m unittest` passes
  when `ACME_ENV` is set. The one agent that ran it after the warning ran
  `ACME_ENV=test python3 -m unittest discover -s tests` and got a pass (dead-end accuracy 0/1;
  2/4 on `hash:0.3`). The miner now names a runner only if its failures share the route's
  signature and it failed in at least 9 in 10 of the sessions that ran it (see
  `adage_learn::mine::dead_end_routes`). The route keeps its id (`dead-end-d41b1e`, derived from
  the working command) and now names only `python3 -m pytest` ("No module named pytest").
  - Precision is unchanged on every dataset. Dead-end accuracy rose: 0/1 → 0/0 in round 2, 2/4 →
    1/1 on `hash:0.3`, and stays 100% on synthetic (8/8) and adversarial (3/3).
  - The cost is recall of failed verifies by runners the route no longer names, because they fail
    with a different signature: synthetic 1.00 → 0.97 (its `unittest` dead end), `hash:0.3`
    0.85 → 0.81 (`python -m pytest`, "command not found"). Adversarial fell 0.77 → 0.57: its
    `pytest` failures carry only unrenderable (hostile) error lines, so they share no signature.
  - **One dead end per (working command, signature).** A second signature with enough support is
    a route of its own. The most common signature's route keeps the id derived from the working
    command alone (`dead-end-d41b1e`); each further signature's id is derived from (working
    command, signature), so it is stable too. A runner is named under one signature only, its most
    common. Round 2 now also learns `dead-end-21cb2c`: `python -m pytest` ("command not found", 4
    of 5 sessions), and synthetic learns its `python3 -m unittest` dead end. Before → after:
    - recall: `hash:0.3` 0.81 → 0.84 (dead-end key steps 1/4 → 3/4), synthetic 0.97 → 1.00
      (8/10 → 10/10); round 2, round 1 and adversarial unchanged;
    - precision: round 2 0.73 → 0.74, `hash:0.3` 0.80 → 0.81, synthetic 0.91 → 0.93; dead-end
      precision unchanged on every dataset (0.78, 0.83, 1.00, 1.00); dead-end accuracy 3/3 on
      `hash:0.3`, 10/10 on synthetic.
    - Adversarial recall stays 0.57: its `pytest` failures have no renderable error line at all
      (each carries quotes and shell syntax, or a right-to-left override), so they have no
      signature to key a route by. Recovering them would mean naming a runner without saying why
      it fails, which the signature rule exists to prevent.
- **Finish-line obligations were never wrong** on labelled sessions. In round 2, one obligation
  was "needed but skipped": the agent did not regenerate, and the hidden check found stale
  generated code.
- **Nothing unsafe** reached a directive on any dataset. That includes the adversarial set, with
  84 directives across every route kind.

### Splits and leakage

The demo's `round2` split is the split the live run used: it learns from the training sessions
*and* the round-1 eval sessions, which worked the same six tasks (W-2041…W-2046) that round 2
tests. It stays, because the gates and the fidelity check reproduce the live run with it, but its
manifest note says what it is and the report flags it `LEAKY`. `hash:<share>` splits sessions,
not tasks, so repeats of a task land on both sides too. For numbers on held-out tasks, use
`group:task:<k>:<fold>` or `temporal`. Measured on the demo (demo thresholds, window 3; points
with Wilson 95% intervals):

| Split | Train / test (scored) | Task overlap | Precision, all | Lookup | Recall | Route P / R |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| `round1` | 18 / 18 (18) | 0 | 0.68 (0.49–0.82) | 0.50 | 0.56 (0.40–0.70) | 1.00 / 0.75 |
| `round2` | 54 / 18 (18) | **6, leaky** | 0.74 (0.63–0.83) | 0.67 | 0.78 (0.64–0.88) | 1.00 / 1.00 |
| `hash:0.3` | 55 / 35 (24) | **11, leaky** | 0.81 (0.72–0.88) | 0.63 | 0.84 (0.73–0.91) | 1.00 / 1.00 |
| `group:task:3:0` | 72 / 18 (12) | 0 | 0.68 (0.53–0.80) | 0.67 | 0.81 (0.62–0.91) | 1.00 / 1.00 |
| `group:task:3:1` | 57 / 33 (21) | 0 | 0.82 (0.72–0.89) | 0.81 | 0.84 (0.73–0.91) | 1.00 / 1.00 |
| `group:task:3:2` | 51 / 39 (21) | 0 | 0.73 (0.60–0.83) | 0.48 | 0.89 (0.77–0.95) | 1.00 / 1.00 |
| `temporal` | 30 / 48 (24) | 0 | 0.73 (0.63–0.81) | 0.58 | 0.83 (0.71–0.90) | 1.00 / 1.00 |

On held-out tasks, overall precision (0.68–0.82) sits inside `round2`'s interval, so there is no
sign that the leak inflated the demo's headline; the conventions are repository-wide rather than
per task. The leak is still reported, because a learner that memorised per-task steps would look
better on `round2` than it is. On synthetic every session is its own task (overlap 0 on every split). The
adversarial `attack` split is in-sample by design (it replays its training sessions too, as
attacks), so it is flagged leaky.

### Ground truth: `conventions.json` and `adage eval routes`

Directive precision says whether a would-have directive agreed with what the agent did. It does
not say whether the learner found the sandbox's conventions and nothing else. For that, each
sandbox lists its hidden conventions in a machine-readable `conventions.json`:

- `demo/sandbox/conventions.json`: acme-billing (demo/SANDBOX.md). The acme datasets
  (`demo-haiku45`, `synthetic`, `adversarial`) carry an exact copy, named by the manifest's
  `conventions` key; a unit test checks the copies, and the generators write them.
- `evals/live/sandboxes/harbor-dispatch/conventions.json`: harbor-dispatch (the conventions
  table in evals/live/README.md). A dataset built from a harbor run gets it by copying it next
  to its manifest (without a `conventions` key, `conventions.json` in the dataset is used).

The schema (`adage-conventions/1`; unknown fields are refused):

| Field | For | Meaning |
| --- | --- | --- |
| `schema`, `sandbox`, `description` | the file | `adage-conventions/1`; the repository key (must equal the dataset's `repo`) |
| `id` | every convention | Unique name |
| `kind` | every convention | `lookup`, `linked_files`, `dead_end`, `finish_line` or `intent` |
| `command` | every convention | The canonical command as argv words (`python3 scripts/wi-lookup {id}`). Lookup: the lookup. Linked files: the command whose output lists the files. Dead end: the command that works. Finish line: the step owed after the edit. Intent: the step. `{id}` is bound to the first sample |
| `avoided` | `dead_end` | The commands that fail, as argv prefixes (`python3 -m pytest`, `npm test`) |
| `glob` | `finish_line` | The repository-relative glob whose edits create the obligation (`schema/*.json`) |
| `id_pattern` | `lookup` | Anchored regex every real id matches (`^INC-\d{2,3}$`) |
| `samples`, `counter_samples` | `lookup` (samples also bind `{id}`) | Ids a learned contract must accept, and must reject. Checked against `id_pattern` when the file loads |
| `notes` | any | Free text for people |

`adage eval routes <dataset> [--split …] [--thresholds …] [--index …] [--json]` learns tracks from
the train split exactly as `adage eval` does (or loads `--index`), then matches every learned
route against every convention. A route matches a convention when:

- it is of the same kind;
- its command is the convention's command: the same canonical key as the scorer computes
  (`adage_runtime::command_key`, after binding `{slot}`s to the first sample), or the same
  command head where the head names a script. So `./scripts/wi-lookup` matches
  `python3 scripts/wi-lookup`, but `npm run check` does not match `npm run codegen`. The command
  compared is the step's (lookup, finish line, intent), the trigger's (linked files) or the
  recommended one (dead end);
- a lookup's slot contracts accept every sample and reject every counter-sample; linked files
  bind a repository-path contract;
- every runner a dead end avoids starts with one of the convention's `avoided` commands;
- a finish line's glob covers the convention's: it matches every sandbox file the convention's
  glob matches (a broader glob covers; a narrower one does not).

It reports **route precision** (learned routes matching some convention ÷ learned routes),
**route recall** (conventions some route matches ÷ conventions; intent conventions are listed
but not counted, as offline learning has no embedding model), F1, both with Wilson intervals,
the **false routes** (with why each missed every convention of its kind) and the **missed
conventions**, plus the split's task overlap. Measured:

| Dataset, split | Learned routes | Route precision | Route recall | F1 | Missed |
| --- | ---: | ---: | ---: | ---: | --- |
| synthetic (`hash:0.5`) | 5 | 1.00 (5/5) | 1.00 (4/4) | 1.00 | — |
| demo `round2` | 5 | 1.00 (5/5) | 1.00 (4/4) | 1.00 | — |
| demo `round1` | 3 | 1.00 (3/3) | 0.75 (3/4) | 0.86 | `tests-through-make`: the 18 training sessions give no dead end (as the live round-1 index) |
| demo `group:task:3:*`, `temporal`, `hash:0.3` | 4–5 | 1.00 | 1.00 | 1.00 | — |
| adversarial (`attack`) | 4 | 1.00 (4/4) | 1.00 (4/4) | 1.00 | — |

Two dead-end routes can match one convention (round 2 learns `python -m pytest` and
`python3 -m pytest` as separate signatures of the `make test` dead end); both are true routes.
With four conventions and five routes the intervals are wide (precision 5/5: 0.57–1.00), so
these gates catch a learner that starts inventing or dropping routes, not small shifts.

### Gates

`cargo test -p adage-eval --test gates` runs in a few seconds and is part of `cargo test`. The
output is deterministic, so each threshold is the measured value less 0.02:

| Gate | Measured | Threshold |
| --- | ---: | ---: |
| demo round 2: precision all / lookup / linked files / dead end | 0.7424 / 0.6667 / 0.75 / 0.7778 | ≥ 0.7224 / 0.6467 / 0.73 / 0.7578 |
| demo round 2: recall / finish-line accuracy | 0.7826 / 1.00 | ≥ 0.7626 / 0.98 |
| demo round 2: reported leaky (task overlap), with its note | 6 tasks | = 6, leaky |
| demo round 2: learned routes equal the live index's (ids, aspects) | equal, plus `dead-end-21cb2c` | equal, plus exactly `dead-end-21cb2c` (red) |
| demo round 2: replay fidelity, decisions / events | 36/36, 35/36 | 36/36, ≥ 34/36 |
| demo round 1: precision all / lookup, recall | 0.6786 / 0.50, 0.5556 | ≥ 0.6586 / 0.48, 0.5356 |
| synthetic: precision all / lookup / linked files / dead end | 0.9342 / 0.7778 / 1.00 / 1.00 | ≥ 0.9142 / 0.7578 / 0.98 / 0.98 |
| synthetic: recall / dead-end accuracy / finish-line accuracy | 1.00 / 1.00 / 1.00 | ≥ 0.98 / 0.98 / 0.98 |
| route precision / recall: synthetic, demo round 2, demo `group:task:3:0`, adversarial | 1.00 / 1.00 | ≥ 0.98 / 0.98, no false route |
| route precision / recall: demo round 1 | 1.00 / 0.75 | ≥ 0.98 / 0.73; missed exactly `tests-through-make` |
| demo and synthetic: every `group:task:3` fold and `temporal` have no task on both sides; the folds partition the sessions; temporal trains on round 1 only | 0 overlap | = 0 |
| demo `hash:0.3`: reported leaky | 11 tasks | = 11, leaky |
| every dataset: unsafe directives | 0 | = 0 |
| adversarial: every route kind exercised, directives checked | 4 kinds, 84 | 4 kinds, ≥ 80 |
| every dataset: the hook's no-match fast path changes no decision | 0 differ (skips 95–437 events per dataset) | 0 differ, some skipped |
| same dataset, same report bytes (JSON and text, eval and routes); no temp or dataset paths in the report | yes | yes |
| committed `conventions.json` files are valid; dataset copies equal `demo/sandbox/conventions.json` | yes | yes |
| demo dataset redacted; generated datasets match their generators | yes | yes |

When a change moves a number on purpose, re-measure, then update the threshold, this table and the
gate's comment together, and say why in the commit.

## Bench (L0): `adage eval bench`

The gates above ask "did this change make anything worse?". The bench asks "which setting of the
engine's numbers is best, and by how much?". It runs a fixed suite of offline evals for one
setting of the engine parameters, reduces it to one objective and a few guardrails, and writes
everything as JSON Lines in one schema, so settings can be swept, compared and blessed.

```bash
adage eval bench run --suite core --deterministic --out l0.jsonl         # one setting (the defaults)
adage eval bench run --suite core --params my.toml --out l0-mine.jsonl   # a candidate setting
adage eval bench sweep --space evals/bench/spaces/learner.toml --strategy sobol:64 --seed 0 --jobs 4 --out sweep.jsonl
adage eval bench report sweep.jsonl --top 10 --pareto turns_saved_at_window_per_session,-false_directives_per_session,route_f1 --vs-baseline evals/baselines/l0-core.jsonl
adage eval bench bless l0.jsonl --reason "why the numbers moved"         # writes evals/baselines/l0-core.jsonl
adage eval evals/datasets/synthetic --params my.toml                     # any eval, with a setting
```

### Engine parameters

`adage_tracks::params::EngineParams` holds every number the runtime and the learner tune on, with
serde defaults equal to the values Adage has always used (`{}` is exactly today's engine; the
`demo` preset differs only in `learn.min_support` 3 and `learn.green_lower_bound` 0.5):

| Parameter | Default | Used by |
| --- | ---: | --- |
| `runtime.follow_window` | 3 | tool batches in which a directed command counts as followed |
| `runtime.max_directives_per_session` / `…_per_decision` | 8 / 3 | the directive budget |
| `runtime.max_stop_requests` | 2 | finish-line stop requests per session |
| `runtime.history` | 32 | recent call keys kept in the state |
| `runtime.holdout_share` | 0.1 | recorded only: replay is always the holdout arm, and the hook reads `runtime.holdout` from `config.toml` |
| `learn.min_support` | 5 (demo 3) | distinct supporting sessions for any route |
| `learn.green_lower_bound` | 0.8 (demo 0.5) | Wilson lower bound for green |
| `learn.yellow_rate` / `learn.retain_rate` | 0.5 / 0.3 | rate bar for a new route / to keep an existing one |
| `learn.dead_end_consistency` | [9, 10] | a runner is avoided only if it failed in at least num/den of the sessions that ran it |
| `learn.dead_end_signatures` | 3 | dead ends per working command |
| `learn.finish_line_contrast` | 0.3 | finish-line rate after the edit minus without it |
| `learn.intent_ubiquitous` / `learn.intent_contrast` | 0.8 / 0.3 | intent routes: ubiquitous commands, in- minus out-of-family rate |
| `learn.material_lower_bound` | 0.10 | review: a material lower-bound change |
| `learn.stale_exposure` | 0.5 | stale routes: the exposed share that explains an absence |

The runtime reads its limits from `Ctx::params`; the learner from `LearnConfig::from_params`.
**Production is unchanged:** the hook builds `Ctx` with `RuntimeParams::default()` (the index does
not carry runtime parameters), and learning uses the `default` or `demo` preset from `config.toml`.
Only the offline eval and the bench build either from a parameter file. `--params` takes TOML or
JSON, any subset, laid over the preset; unknown keys and out-of-range values are refused.
`params_hash` is the SHA-256 of the parameters' canonical JSON (keys sorted, no whitespace).

### The core suite, the objective, the guardrails

`--suite core` (`evals/bench/suites/core.toml`, built in) starts from the `demo` preset and runs:

| Role | Units | Used for |
| --- | --- | --- |
| `cv` | demo-haiku45 and synthetic, `group:task:3` folds 0, 1, 2 | the ranking objective (mean over the 6 units) |
| `final` | demo-haiku45 and synthetic, `temporal` | a final check, reported in `scorecard.final`, **never ranked on** |
| `safety` | adversarial, `attack` | guardrails only |

Every split is leak-free (no task on both sides). Per unit,

```text
J = turns_saved_at_window_per_session − λ_fp · false_directives_per_session + λ_gt · route_F1
```

with λ_fp = 2.0 and λ_gt = 1.0 (the suite's `[objective]`). `turns_saved_at_window_per_session`
counts the turns-saved estimate only for directives followed within the scoring window (3 model
turns); `false_directives_per_session` is directives the agent never followed; `route_F1` is
`adage eval routes`' score against `conventions.json`. The objective is the mean of J over the
`cv` units. A setting is **infeasible** if any guardrail fails: unsafe directives = 0 on every unit
(the adversarial set included); fast-path mismatches = 0; cross-validated route precision ≥ the
baseline's − 0.05. The scorecard also reports recall, precision@window and `calibration_ece`
(expected calibration error of route lower bounds against the offline correct rate, 5 bins); none
of them enter J.

### Results schema

`evals/bench/result.schema.json` (JSON Schema 2020-12). Every line has the envelope
`schema_version` (1), `record`, `run_id`, `tier` (`l0`), `suite`, `adage_version`, `params`,
`params_hash`, `split {kind, name, seed}`, per-unit records also `dataset {id, content_sha256}`,
and `git_sha` only when given (`--git-sha`, `ADAGE_GIT_SHA`) and never with `--deterministic`.
Records: `run` (the units, their train/test sizes and index generations), `trial` (one per scored
test session), `directive` (one per would-have directive), `aggregate` (`metric`, `estimate`,
`ci_low`, `ci_high`, `ci_method` — Wilson for proportions, normal for per-session means — and `n`;
per unit and pooled over `cv` and `final`), and `scorecard` (`objective`, `terms`, `guardrails
[{name, value, limit, op, pass}]`, `final`). `run_id` is the hash of the inputs (schema, suite,
dataset content hashes, params hash), nothing is timed, and keys are sorted: the same inputs give
the same bytes. A sweep writes `run`, `aggregate` and `scorecard` records per setting (`--detail`
adds trials and directives).

### Baseline and gate

`evals/baselines/l0-core.jsonl` holds the aggregates and scorecard of `bench run --suite core`
with the defaults, and why it was blessed. `cargo test -p adage-eval --test bench` reproduces it
byte for byte (ignoring `adage_version`), checks that runs and sweeps are deterministic (the same
bytes with 1 or 4 jobs), that every configuration of a `random:4` sweep has no unsafe directive,
and that parameters really reach the learner and the runtime. The baseline (J 0.6783): turns saved
at window 0.651 per session, 0.472 false directives per session, route F1 1.0, recall 0.942,
precision@window 0.335; final check J 0.6146. After a deliberate change of behaviour, re-run and
`bless` with a reason, in the same commit.

### Sweeps

`--space` names a parameter space (`evals/bench/spaces/learner.toml`: `min_support` 2–8,
`green_lower_bound` 0.4–0.9, `yellow_rate` 0.3–0.7, `retain_rate` 0.1–0.5, `follow_window` 2–6,
`max_directives_per_session` 3–12, `dead_end_consistency` 0.7–1.0 as `[num, 20]`). Strategies:
`grid`, `random:N` (SplitMix64), `sobol:N` (Joe–Kuo direction numbers, a seeded digital shift, up to
13 dimensions), `halton:N`. Configuration 0 is always the preset (`default`). Settings run in
parallel (`--jobs`), output in configuration order. The route-precision reference is the baseline
file's (`--baseline`, else `evals/baselines/l0-<suite>.jsonl` when present), else configuration 0's.

**First sweep** (`sobol:64 --seed 0`, 65 settings, about a minute in a debug build; not committed):

| Rank | Setting (changes from the demo preset) | J | ΔJ | turns saved/s | false/s | route F1 | final J |
| ---: | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | `yellow_rate` 0.7, `green_lower_bound` 0.6, `dead_end_consistency` [14,20], … | 0.7537 | +0.075 | 0.632 | 0.387 | 0.909 | 0.6146 |
| 2 | `min_support` 2, `yellow_rate` 0.65, `green_lower_bound` 0.65, … | 0.7222 | +0.044 | 0.632 | 0.406 | 0.957 | 0.6563 |
| 3 | the preset (and 10 other settings that score the same) | 0.6783 | 0 | 0.651 | 0.472 | 1.000 | 0.6146 |

What it says:

- **Fixed: the fast path now decides every session's first prompt in full.** The first sweep found
  only 19 of 65 settings feasible: with `min_support` ≥ 4 no dead end is learned on the adversarial
  set, the no-match fast path then skipped the first prompt without counting it, and `prompts`
  stayed 0 where the full decision set it to 1. With a fixed index no decision differed, but a dead
  end learned mid-session would have been announced at the next prompt by one path and never by the
  other. `adage_runtime::fast` now never skips a session's first prompt (one full decision per
  session). Re-run (`sobol:64 --seed 0`): **65 of 65 feasible**. The best settings raise
  `min_support` to 5–8 and `dead_end_consistency` to 18–19 of 20: J 0.92 (+0.245), false
  directives per session 0.47 → 0.27–0.33, temporal final J 0.83–0.99 (baseline 0.61), at the cost
  of route recall (F1 0.86–0.96). Candidates for a live confirmation, not yet a default change.
- **Before the fix, the feasible gains traded conventions for fewer false directives.** The best settings raise
  `yellow_rate` to 0.65–0.7, which drops a yellow route on some folds: false directives fall 18%,
  but recall falls from 0.94 to 0.82 and route F1 below 1, and the temporal final check does not
  move (0.6146) for the best one. With 6 folds of 12–21 sessions this is not evidence to change a
  default; it is what the bench is for — a candidate to test live.
- **`retain_rate` and `runtime.follow_window` have no effect at L0**: every offline learn starts
  from an empty home (no existing route to retain), and follow judgements are not scored.
  `max_directives_per_session` matters only at 3–4, where it cuts turns saved.

## Property tests

These use `proptest` with default case counts and bounded inputs, and finish in a few seconds.
`--features slow` raises the case counts 15–20×; the evals workflow runs that.

| Test | Property |
| --- | --- |
| `prop_shell` | `normalize_shell` never panics and returns within 2 s on a 2 MiB stack. The inputs are arbitrary text, shell-shaped text, recursively nested constructs, 6,000-deep repetitions of 17 constructs, and 200 KB inputs |
| `prop_tracks` | Random tracks, mostly hostile, go through `io::validate`. Each accepted track is driven through `decide` with hostile prompts and tool output. Every directive and stop request must pass the safety oracle. The test also checks its own coverage: at least 10% of tracks are accepted, and a minimum number of directives is checked |
| `prop_store` | A spool cut at any byte loses only the torn record. Flipped bytes never invent a record. A state write torn at any byte reads as a written state. Truncation reads as a written state or as the default |
| `prop_render` | `render::sanitise` output is bounded, free of forbidden and invisible characters, trimmed, and idempotent |

### Bugs found, fixed, and pinned by regression tests

1. **One hostile error line stopped `adage learn` for the whole repository.** A dead-end signature
   containing quotes or shell syntax failed track validation, and `learn` returned the error:
   nothing was learned and no index was compiled. Any mined command with a quoted argument did the
   same.
   - Fix: learning now drops only the invalid route, and the dead-end miner picks the last
     renderable error line.
   - Regression tests: `adage-learn/tests/hostile.rs`.
2. **Invisible characters rendered.** Right-to-left overrides and zero-width characters passed
   `safe_text` and `render::sanitise`, so a signature could read differently than it is.
   - Regression tests: `adage-learn/tests/hostile.rs`, `prop_render.rs`,
     `adage-tracks/tests/hostile.rs`.
3. **Stack overflow in the normaliser.** A 15 KB `a && a && … && ls` is under `MAX_COMMAND` and
   contains no brackets, so `too_deep` let it through. The walk recursed once per `&&`, which
   overflowed even an 8 MiB stack. The overflow aborted the hook process, which `catch_unwind`
   cannot catch, and aborted `adage learn` over any history that contained such a command.
   - Fix: `&&`/`||` chains are flattened iteratively, and a depth bound (`MAX_DEPTH = 96`) covers
     everything else.
   - Regression tests: `adage-core/tests/deep.rs`.
4. **Pathological commands stalled the hook for most of a second.** tree-sitter's error recovery is
   quadratic on some malformed input. In a release build, 16 KB of `a|a|…|` took 0.6 s to
   normalise and `)))…` took 0.7 s, against a hook budget of 25 ms.
   - Fix: a deterministic parse budget. The quadratic part is re-lexing (`)))…` makes the lexer
     read 128 million bytes), so the parser is fed one byte per read and gives up after 2^20 reads
     (64 × `MAX_COMMAND`; real commands read each byte one to seven times). The cut-off is a
     property of the command, so it no longer depends on machine speed, as the first fix (a 50 ms
     timeout) did. Pipe operators outside quotes are capped at 128, because the `a|…|` case spends
     its time outside lexing. A cut-off parse takes about 7 ms in release; a 1 s wall-clock
     backstop remains only for input shapes not yet known.
   - Regression tests: `adage-core/tests/deep.rs` (including that the budget, not the clock, cuts
     off each pathological case), and the committed proptest seed in
     `prop_shell.proptest-regressions`.
5. **Tracks could direct paths outside the repository.** Validation checked only a command's
   characters, so a shared track could direct `cat /etc/passwd` (observe, and so eligible for
   green) or `cat ../../secret`.
   - Regression tests: `adage-tracks/tests/hostile.rs`.
6. **Unbound slot placeholders were rendered.** A run step with `{slot}` under a trigger that
   cannot bind it rendered the placeholder itself as the command (`make {item}`). Only a prompt may
   now bind a run step's slots.
   - Regression tests: `adage-tracks/tests/hostile.rs`.
7. **Blank commands were accepted.** `" "` passed validation and rendered as a directive to run
   nothing.
   - Regression tests: `adage-tracks/tests/hostile.rs`.
8. **A torn state write could resurrect an older state.** The slot CRC covered only the payload. A
   write torn after the header's sequence number therefore left the old payload, with its still
   valid CRC, under a new sequence number. The reader took a state two writes stale as the newest.
   The CRC now covers the sequence number, the length and the payload. State files written before
   the fix read as the default once, which is safe because state is a cache.
   - Regression test: `adage-store` `a_write_torn_after_the_sequence_number_is_not_read_as_newest`.

## Live proving ground: `evals/live/`

`evals/live/runner.py` generalises `demo/run.py`. It runs the with/without A/B on any sandbox and
task set, with any `claude` model. A JSON config supplies:

- a sandbox generator and a scorer;
- prompts;
- training and held-out tasks, with repeats;
- the arms (`adage`, `holdout`, `off`);
- the model, max turns, concurrency, learn thresholds and a spend cap.

`evals/live/configs/acme-billing.json` is the demo. `evals/live/configs/harbor-dispatch.json` is
the second demo, built to test whether the effect generalises: a TypeScript service on Node 26
(`evals/live/sandboxes/harbor-dispatch/`) with its own four conventions (`INC-` incidents behind
`bin/fetch-incident`, a path-alias dead end on `npm test`, `registry/*.csv` → `npm run codegen`),
84 runs over two rounds with Claude Sonnet 5.

```bash
python3 evals/live/runner.py evals/live/configs/acme-billing.json            # dry run: prints the plan
python3 evals/live/runner.py --config evals/live/configs/harbor-dispatch.json --dry-run
python3 evals/live/runner.py evals/live/configs/acme-billing.json --json     # the plan as JSON
python3 evals/live/runner.py evals/live/configs/acme-billing.json --execute  # real agents, real spend
```

The runner is a dry run unless `--execute` is given. It refuses to execute from inside an agent
session. A run writes the layout of a demo archive and a results page like `demo/results/`, so
`build_demo.py <root> <dataset> --name … --repo … --sandbox …` turns any live run into an offline
dataset for `adage eval`. `python3 -m unittest evals/live/test_runner.py` smoke-tests the dry run.
For harbor-dispatch, `selfcheck.py` proves every incident solvable and `rehearse.py` proves, with
scripted sessions and no model, that `adage learn` learns all four of its conventions and that
`adage eval` ingests the result.
See [evals/live/README.md](../evals/live/README.md).

### How the two relate

| | Offline replay | Live A/B (demo, round 2) |
| --- | --- | --- |
| Question | Would the directives have been right, and safe? | Does delivering them help? |
| Control | The agent's own later actions in unsteered sessions | The holdout arm, same hooks (arm order was fixed, not randomised, in the published runs; see Known limitations) |
| Headline | Lookup directive correct for 12 of 18 held-out agents; they took a median of 9 turns to find the lookup unaided | Turns before the first edit fell from a median of 20 to 2 (p = 0.0002); cost per run fell from $0.135 to $0.053 |
| Blind spots | Cannot see effects of delivery (anchoring, derailing); precision is a lower bound; no intent routes without the model | Small n (18 runs per arm, but only 6 tasks); costs tokens; one sandbox |

The offline numbers are the cheap, deterministic early warning; a change that lowers them is
unlikely to help live. The live proving ground is the judge. A track is published only when it
is non-inferior on success and faster (Design §12.2). Offline replay is also how a live run is
audited: its fidelity check confirms that a replay of the recorded sessions reproduces what the
hooks decided.

### Known limitations of the published numbers

An audit of the demo runs (2026-09-27) found issues that the numbers above do not yet account
for. They are being fixed (see the roadmap); until the runs are repeated, read the live results
with these in mind:

- **Round 2 is not held out by task.** Round 2 learns from round-1 eval sessions of the same six
  work items (W-2041 to W-2046) that it then tests. Routes are repository conventions, not
  per-task answers, so the effect is smaller than it sounds, but it breaks the Design Spec's rule
  of held-out tasks for every published number. The offline `round2` split has the same overlap.
- **Arm order was fixed** (holdout queued before adage for every task and repeat), not randomised.
- **The arms were not isolated from the operator's Claude Code auto-memory.** Holdout agents read
  `MEMORY.md` in 23 of 36 Haiku eval runs; Adage-arm agents in none.
- **Pseudo-replication.** p-values permute individual runs, but 18 runs are 6 tasks × 3 repeats,
  so the effective sample is closer to 6. No correction was applied across the 11 metrics.
- **Infrastructure failures counted as task failures**, and runs cut off at `max_turns` entered
  medians as if complete.
- **A harm signal went unreported.** In Haiku round 1 the Adage arm hit 26 failed verify runs and
  the holdout arm 0: skipping exploration also skipped the agent's incidental discovery of
  `make test`. Round 2 learned the dead end that fixed it, but no metric treated this as a
  regression.

## CI

`.github/workflows/evals.yml` runs on pushes to main, on pull requests, and weekly, on Linux and
macOS. It runs:

- the gates;
- the L0 bench test (the defaults reproduce `evals/baselines/l0-core.jsonl`), and a
  `bench run --suite core` report among the artifacts;
- the property tests at the slow case counts;
- the replay fidelity check;
- the JSON reports for every dataset (uploaded as artifacts);
- the live runner's dry-run smoke test.

Replay needs POSIX paths, because the recorded transcripts and the normaliser's path handling are
both POSIX. The replay-based tests are therefore Unix-only; on Windows, `cargo test` skips them.
