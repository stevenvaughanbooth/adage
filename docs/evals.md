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
```

What it does:

1. It **splits** the dataset's sessions into train and test. The split is deterministic:
   - a named split from the manifest (e.g. `round2`);
   - `hash:<share>[:<seed>]`: a session is in the test set when SHA-256(seed:session id) falls
     below `share`;
   - `all`: every session in both sets, reported as in-sample.
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
| Directives given | 48 | 28 | 65 | 58 | 74 |
| Precision, all | 0.73 | 0.68 | 0.80 | 0.91 | 0.73 |
| — lookup | 0.67 | 0.50 | 0.63 | 0.78 | 1.00 |
| — linked files | 0.75 | 1.00 | 1.00 | 1.00 | 0.50 |
| — dead end | 0.78 | — | 0.83 | 1.00 | 1.00 |
| — finish line | 0.75 | 1.00 | 1.00 | 0.88 | 0.41 |
| Recall (key steps) | 0.78 (36/46) | 0.56 (20/36) | 0.85 (58/68) | 1.00 (77/77) | 0.77 (67/87) |
| Turns saved, lookup (median) | 9 | 10 | 10 | 1 | 0 |
| Dead-end accuracy | 0/1 | — | 2/4 | 10/10 | 20/20 |
| Finish-line accuracy | 1.00 (4) | 1.00 (3) | 1.00 (7) | 1.00 (8) | 1.00 (7 met; 10 unlabelled) |
| Unsafe directives | **0** | **0** | **0** | **0** | **0** / 84 checked |

Replay fidelity, round 2:

- Replaying the 36 round-2 eval sessions (holdout and adage arms) against the live round-2 index
  reproduces **36/36** sessions' would-have directives and stop requests.
- It reproduces the exact hook event sequence in **35/36** sessions. In the other session the
  agent ran a forked skill, whose tool calls fired hooks in the session but are recorded in a
  subagent transcript.
- Learning from the 54 curated `round2` training transcripts reproduces the live round-2 index
  **byte for byte** (generation `8dd5ec01ed76c4bd`).

Round 1 was decided by an earlier build. Its command keys templated interpreter scripts, so the
current runtime no longer matches that index's linked-files trigger. Round-1 fidelity
(decisions 11/18 holdout, 0/18 adage) therefore measures that drift, and it is not gated.

What the numbers say:

- **The lookup is the value.** Held-out agents that found the tracker unaided took a median of 9
  to 10 turns to find it. Offline, every one of those lookups had a correct directive at the
  prompt (lookup recall 12/12 in round 2). This matches the live A/B, where round-2 turns before
  the first edit fell from a median of 20 to 2.
- **The dead-end route is too broad.** In round 2 it names `python3 -m unittest` as failing, but
  that command passes when `ACME_ENV` is set. The one agent that ran it after the warning ran
  `ACME_ENV=test python3 -m unittest discover -s tests` and got a pass (dead-end accuracy 0/1; 2/4 on `hash:0.3`). The miner records the failing runner without
  the condition under which it fails.
- **Finish-line obligations were never wrong** on labelled sessions. In round 2, one obligation
  was "needed but skipped": the agent did not regenerate, and the hidden check found stale
  generated code.
- **Nothing unsafe** reached a directive on any dataset. That includes the adversarial set, with
  84 directives across every route kind.

### Gates

`cargo test -p adage-eval --test gates` runs in under a second and is part of `cargo test`. The
thresholds are the measured values less a margin:

| Gate | Measured | Threshold |
| --- | ---: | ---: |
| demo round 2: precision all / lookup / linked files / dead end | 0.73 / 0.67 / 0.75 / 0.78 | ≥ 0.60 / 0.55 / 0.60 / 0.65 |
| demo round 2: recall / finish-line accuracy | 0.78 / 1.00 | ≥ 0.65 / 0.75 |
| demo round 2: learned routes equal the live index's (ids, aspects) | equal | equal |
| demo round 2: replay fidelity, decisions / events | 36/36, 35/36 | 36/36, ≥ 34/36 |
| demo round 1: precision all / lookup, recall | 0.68 / 0.50, 0.56 | ≥ 0.55 / 0.40, 0.45 |
| synthetic: precision all / lookup / linked files / dead end | 0.91 / 0.78 / 1.00 / 1.00 | ≥ 0.85 / 0.70 / 0.90 / 0.90 |
| synthetic: recall / dead-end accuracy / finish-line accuracy | 1.00 / 1.00 / 1.00 | ≥ 0.90 / 0.90 / 0.90 |
| every dataset: unsafe directives | 0 | = 0 |
| adversarial: every route kind exercised, directives checked | 4 kinds, 84 | 4 kinds, ≥ 80 |
| same dataset, same report bytes; no temp paths in the report | yes | yes |
| demo dataset redacted; generated datasets match their generators | yes | yes |

When a change moves a number on purpose, re-measure, then update the threshold, this table and the
gate's comment together, and say why in the commit.

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
4. **Tracks could direct paths outside the repository.** Validation checked only a command's
   characters, so a shared track could direct `cat /etc/passwd` (observe, and so eligible for
   green) or `cat ../../secret`.
   - Regression tests: `adage-tracks/tests/hostile.rs`.
5. **Unbound slot placeholders were rendered.** A run step with `{slot}` under a trigger that
   cannot bind it rendered the placeholder itself as the command (`make {item}`). Only a prompt may
   now bind a run step's slots.
   - Regression tests: `adage-tracks/tests/hostile.rs`.
6. **Blank commands were accepted.** `" "` passed validation and rendered as a directive to run
   nothing.
   - Regression tests: `adage-tracks/tests/hostile.rs`.
7. **A torn state write could resurrect an older state.** The slot CRC covered only the payload. A
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

`evals/live/configs/acme-billing.json` is the demo.

```bash
python3 evals/live/runner.py evals/live/configs/acme-billing.json            # dry run: prints the plan
python3 evals/live/runner.py evals/live/configs/acme-billing.json --json     # the plan as JSON
python3 evals/live/runner.py evals/live/configs/acme-billing.json --execute  # real agents, real spend
```

The runner is a dry run unless `--execute` is given. It refuses to execute from inside an agent
session. A run writes the layout of a demo archive, so
`build_demo.py <root> <dataset>` turns any live run into an offline dataset for `adage eval`.
`python3 -m unittest evals/live/test_runner.py` smoke-tests the dry run.
See [evals/live/README.md](../evals/live/README.md).

### How the two relate

| | Offline replay | Live A/B (demo, round 2) |
| --- | --- | --- |
| Question | Would the directives have been right, and safe? | Does delivering them help? |
| Control | The agent's own later actions in unsteered sessions | The holdout arm, randomised, same hooks |
| Headline | Lookup directive correct for 12 of 18 held-out agents; they took a median of 9 turns to find the lookup unaided | Turns before the first edit fell from a median of 20 to 2 (p = 0.0002); cost per run fell from $0.135 to $0.053 |
| Blind spots | Cannot see effects of delivery (anchoring, derailing); precision is a lower bound; no intent routes without the model | Small n (18 per arm); costs tokens; one sandbox |

The offline numbers are the cheap, deterministic early warning; a change that lowers them is
unlikely to help live. The live proving ground is the judge. A track is published only when it
is non-inferior on success and faster (Design §12.2). Offline replay is also how a live run is
audited: its fidelity check confirms that a replay of the recorded sessions reproduces what the
hooks decided.

## CI

`.github/workflows/evals.yml` runs on pushes to main, on pull requests, and weekly, on Linux and
macOS. It runs:

- the gates;
- the property tests at the slow case counts;
- the replay fidelity check;
- the JSON reports for every dataset (uploaded as artifacts);
- the live runner's dry-run smoke test.

Replay needs POSIX paths, because the recorded transcripts and the normaliser's path handling are
both POSIX. The replay-based tests are therefore Unix-only; on Windows, `cargo test` skips them.
