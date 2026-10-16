# data-expansion: a broader live trace corpus

**Question.** The research agents had 5 repositories (3 of them Python) and about 10 training
sessions each. Can we add repositories from other ecosystems — with validated hidden-test tasks and
unsteered Haiku sessions that record per-call tokens, dollars and pass/fail — cheaply enough
(hard cap $45) for other agents to test "any repository" and leave-one-repo-out claims?

**Answer.** Yes. The corpus adds 9 repositories in 5 toolchains: Go ×2 (go-toml, chi), cargo ×3
(bytes, http, serde-json), pnpm ×1 (ufo), Python/nox ×1 (packaging), and make-driven C ×2
(janet, jq). It has 140 validated hidden-test tasks and **115 finished Claude Haiku 4.5
sessions**: 107 on the new repositories and 8 extra click training sessions. The sessions contain
4,073 model calls, each with per-call token usage and dollars, and every session has its
hidden-test outcome.

The pass rate is 64% [57%, 71%] overall. By repository it ranges from 38% (serde-json) to 89%
(bytes), so the data has the realistic pass/fail spread that $/task and pass-rate work needs.
Total spend is **$36.09 of the $45 cap**.

This is a data deliverable, not a lever. It has no held-out effect of its own, so `result.json`
reports the corpus, its validity checks and the spend.

## What is where

| Thing | Path |
|---|---|
| Manifest: every session, with its paths, split, outcome, tokens and cost | `evals/lab/research/data-expansion/corpus.json` |
| Tasks (hidden-test, validated) | `evals/repobench/tasks/<repo>.json` |
| Splits (chronological; SHA-256 digest = pre-registration) | `evals/repobench/splits/<repo>.json` |
| Repository definitions (setup, hidden command, globs) | `evals/repobench/repos.json` (new ids: `go-toml`, `chi`, `bytes`, `http`, `serde-json`, `ufo`, `packaging`, `janet`, `jq`; `commander-dx` and `itertools` are defined but yielded too few tasks) |
| Runner configs | `evals/lab/research/data-expansion/configs/<repo>.json` |
| Runner roots (transcripts, streams, state.json) | `/tmp/adage-research/data-expansion/runs/<root>/` |
| Per session | `runs/<run-id>/calls.jsonl` (one line per model call: input/output/thinking/cache-read/cache-write tokens, list-price $, running $, tool uses), `agent.diff` (what the agent changed), `result.json` (final stream event), `stream.jsonl`; transcript under `private/transcripts/` |

Every session in `corpus.json` has these fields: `repo`, `task`, `split` (`train` / `test`, from the
published split; `train-extra` for the older click tasks), `arm` (`train`: no hooks;
`control`: Adage hooks on with directives withheld and logged in state.json), `passed` (hidden
tests), `n_tests`, `touched_gold`, `cost_usd`, `tool_calls`, `turns`, `run_status` (`ok` /
`censored` = hit 60 turns), token totals, and the paths above. The per-call `cum_usd` matches the
session's reported cost to the cent (checked on all click sessions). It uses the transcript's final
per-message usage, because the stream's copy is taken at message start and undercounts output, and
it prices 1-hour cache writes at 2× input.

## Method

1. **Repositories** (small to medium, permissive licences, working test suites):
   pelletier/go-toml (MIT, Go), go-chi/chi (MIT, Go), tokio-rs/bytes, hyperium/http and
   serde-rs/json (MIT/Apache, cargo), unjs/ufo (MIT, pnpm + vitest), pypa/packaging (BSD/Apache,
   nox + uv), janet-lang/janet (MIT, Makefile C: `make` builds `build/janet`, which runs
   `test/suite-*.janet`), and jqlang/jq (MIT, autotools + make: `./tests/jqtest`; built without
   oniguruma, which is a git submodule that is absent from the sandbox).
2. **Tasks.** The RepoBench tooling runs unchanged: `mine.py`, then `validate.py` (the hidden test
   fails at the base, passes with the real fix, and runs in under 60–240 s), then `prompts.py`
   (Sonnet writes an issue-style prompt from the commit message and the test diff only; `leaks()`
   refuses test names, PR numbers and similar). Additive changes to `score.py`: `rust` (cargo
   `--test <target>`, `test result:` counting), `c` (Janet suites) and `make` (a fixed
   make-driven suite; `N of M tests passed` counting), plus a per-repository `prune` list
   (`target/`, `build/`) and Cargo registry pruning of session homes. The existing unit tests
   still pass.
3. **Harder tasks.** Following the sprint guidance to favour a realistic pass/fail spread,
   `make_configs.py --by-size` takes the 10 tasks whose real fix changes the most non-test lines
   (used for go-toml, ufo, janet and chi), then splits them chronologically: oldest 5 train,
   newest 5 test, no buffer. Repositories with fewer than 10 valid tasks (`http`, `serde-json`,
   `jq`) are all held out, and are worked in the control arm only.
4. **Sessions.** `evals/live/runner.py` with `--host-auth`, `confine: true` (bubblewrap), an
   isolated HOME, Claude Haiku 4.5, 60 max turns (the v2 protocol), and concurrency 2 under
   `nice`. CLAUDECODE and session ids are unset for the runner and never forwarded. Rust sessions
   get `RUSTUP_HOME` and a read-only `/root/.rustup` mount, and nothing else. `queue.sh` runs one
   root at a time. Before each root it checks the spend cap and free disk (≥ 1.5 GB), and every
   3 minutes `postprocess.py` saves each finished session's diff and per-call usage and deletes
   its tree.
5. **Infrastructure incidents, all recovered by resuming.** There were three:
   - The shared disk filled up twice (06:18 and 10:17 UTC).
   - The account session limit was hit (09:30–09:50 UTC).
   - The container restarted.

   The sessions affected were marked `infra_error` or `running` and re-run from the same root
   (the runner resumes only unfinished sessions). The cost of the lost partial sessions is kept in
   the spend ledger (`cache/spend.jsonl`, kinds `killed-sessions` and `lost-sessions`), so no
   spend is hidden.

   The coordinator deleted the shared pnpm store to free disk. My cache had linked to it, so the
   ufo repeat-2 sandboxes failed once, and passed after a private store was recreated (then
   deleted). The per-session homes were pruned to the transcript, and tool-result dumps over 1 MB
   were truncated: one jq session had printed 50–67 MB of JSON. `filter_diffs.py` removes jq's
   autotools and libtool output from `agent.diff`. The scorer's `changed_files` for jq still lists
   those generated files, so use `agent.diff` or `touched_gold` for jq.
6. **Coordinator steer, mid-run.** The steer was breadth of ecosystems at 2–3 repositories each,
   with about 10 unsteered sessions per repository, over repeats and over extra click/vue-core
   sessions. From go-toml on, repeats went from 2 to 1. The vue-core extra training (0 sessions
   completed; they failed when the shared disk filled up) was dropped. Click got 8 extra training
   sessions on tasks older than its published split.

## Corpus

| repo | lang / tooling | validated tasks | sessions (train / control) | pass rate [90% Wilson] | $ / session | calls / session | censored at 60 turns |
|---|---|---|---|---|---|---|---|
| bytes | rust | 9 | 18 (8 / 10) | 16/18 = 89% [71%, 96%] | $0.329 | 34.2 | 2 |
| chi | go | 20 | 10 (5 / 5) | 7/10 = 70% [44%, 87%] | $0.214 | 25.2 | 0 |
| click | python (existing) | (existing) | 8 (8 / 0) | 8/8 = 100% [75%, 100%] | $0.343 | 44.0 | 1 |
| go-toml | go | 30 | 10 (5 / 5) | 4/10 = 40% [19%, 65%] | $0.428 | 48.7 | 3 |
| http | rust | 3 | 6 (0 / 6) | 5/6 = 83% [50%, 96%] | $0.481 | 32.5 | 0 |
| janet | c | 23 | 10 (5 / 5) | 4/10 = 40% [19%, 65%] | $0.507 | 53.0 | 5 |
| jq | make | 8 | 15 (0 / 15) | 9/15 = 60% [39%, 78%] | $0.241 | 38.9 | 6 |
| packaging | python | 27 | 10 (5 / 5) | 8/10 = 80% [54%, 93%] | $0.244 | 29.2 | 0 |
| serde-json | rust | 4 | 8 (0 / 8) | 3/8 = 38% [16%, 65%] | $0.320 | 45.1 | 3 |
| ufo | typescript | 16 | 20 (10 / 10) | 10/20 = 50% [33%, 67%] | $0.115 | 16.9 | 0 |
| **all** | | | 115 | 74/115 = 64% [57%, 71%] | $0.295 | 34.9 | |

Spend: sessions $33.95 + tool-less `claude -p` (prompt writing, killed-session estimates) $1.90 = **$36.09** of the $45 cap.

The table is regenerated by `python3 manifest.py && python3 report.py`. Pass rates are over all
the sessions of a repository, train and control together.

By split and arm:

| Split / arm | Passed |
|---|---|
| held-out `test` / `control` | 46/69 |
| `train` / `train` | 20/38 |
| click `train-extra` | 8/8 |

Repeats per task: 2 for bytes, http, serde-json, ufo and jq (the jq task 532ccea6 r2 was skipped
when its per-root budget ran out), and 1 for go-toml, chi, packaging and janet.

**Suggested uses:**

- **Leave-one-repo-out, ecosystem transfer.** Every new repository has only unsteered sessions.
  Go now has 3 repositories (urfave-cli from v1 plus go-toml and chi), cargo has 3, make-driven C
  has 2, and pnpm has 2 (vue-core from v1 plus ufo).
- **$/task and calls/task.** Read `calls.jsonl`. Its `cum_usd` is the cumulative-context cost
  curve per call.
- **Pass rate.** Use `passed` (the hidden tests). `touched_gold` gives a partial-credit signal.

## Task validity

The validator checks that the hidden test fails at the base and passes with the real fix. The
results per repository:

| repo | candidates tried | valid | main rejection reasons |
|---|---|---|---|
| go-toml | 34 | 30 | real fix fails the hidden test (2), passes at base (1) |
| chi | 25 | 20 | passes at base (docs, lint and alignment commits) |
| bytes | 14 | 9 | pre-2021 bases: the real fix fails with today's toolchain (no Cargo.lock) |
| http | 9 | 3 | same (no Cargo.lock, so old bases resolve new dependencies) |
| serde-json | 10 | 4 | passes at base (clippy or refactor commits), old bases break |
| ufo | 37 | 16 | pnpm-8 lockfiles need the project's pinned pnpm (setup now lets pnpm switch versions); remaining old bases time out installing |
| packaging | 30 | 27 | — |
| janet | 30 | 23 | passes at base (doc and test-only commits) |
| jq | 20 | 8 | passes at base: the fix's test is in a suite that is not run (shtest, man.test, onig) |
| itertools | 4 of 14 tried | 0 | all passed at base (clippy commits); stopped |
| commander (jest era) | — | — | 8 candidates in 2019–2026, too few; not validated |

Every prompt passed `leaks()`. Prompts come from the commit message and the test diff only.

## Caveats (read before using the data)

- **The hard-coded parts are measurement instruments only.** These are the repository
  definitions (setup and hidden commands) and the runner's `classify` substrings (the dashboard's
  tool categories). Nothing here is a learner.
- **Train and control arms differ in hooks.** `train` has no hooks and `control` has hooks on
  with directives withheld. Both are unsteered.
- **ufo's hidden tests pin exact edge-case outputs.** Examples are trailing slashes and fragments.
  The issue-style prompt does not always state these outputs, so some ufo failures come partly
  from an under-specified prompt, not from agent error. That is realistic, but keep it in mind
  before reading ufo failures as capability.
- **The prompt writer is Sonnet.** Task prompts are paraphrases of real fixes, so they can be
  easier than a real bug report.
- **Missing ecosystems.** There is no npm/yarn repository: every candidate lacked a lockfile or
  had too few fix commits. pnpm has ufo here plus vue-core in RepoBench v1.
- **Rust is limited to recent history.** Without a lockfile, Rust validity is limited to recent
  bases.
- **jq runs without regex support.** Its regex (oniguruma) suites are not run.
- **Load affects timings.** The shared machine was heavily loaded (load average 12–28 on 4
  cores), so wall-clock times are inflated. Token counts and calls are unaffected.
- **Some session slots failed and were excluded.** A shared disk-full event (06:18 UTC) broke 3
  session slots (1 click, 2 vue-core): they are marked `abandoned` or were removed, and are not
  in the corpus. Two packaging sessions were killed seconds after starting, when the plan switched
  to 1 repeat. Their estimated cost is in the spend ledger.
