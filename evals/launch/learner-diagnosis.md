> Learner diagnosis, 2026-09-29. Why the learner formed zero reflexes on real repositories, what it
> should have learned, which gates were wrong, and what replaced them. Measured on the RepoBench
> traces (v1 training, v1+v2 held-out control sessions), leave-one-repository-out, with a $4 live
> slice on click. Tooling: [learner/](learner/README.md).

# Why the learner learned nothing on real repositories, and what it learns now

## 0. Summary

- **The diagnosis.** On 7–10 training traces per repository, the frequent-pattern learner scored
  65–227 candidates per repository and delivered none (drop ledger, §2). The miner and scorer were
  not the main problem. The gates were: they ask how *often* agents did something, and on real
  repositories the frequent thing is either what agents already do unaided, or wrong.
  - Agents almost never repeat a test command: `python -m pytest tests/test_options.py::test_x
    -xvs` differs every time. The compiler's dominant-variant rule ("ambiguous", "variable
    argument") dropped every test reflex.
  - The majority recovery from the environment dead end is `pip install pytest` into the system
    Python (16 of 23 recoveries in training). The scorer's dead-end consistency gate (the runner
    must fail in 90% of its runs) then says `python -m pytest` "passes elsewhere", because it
    passes *after* the harm-class install.
  - The judge's habit gates (contrast against traces without the edit, "agents already run it")
    are correct for habits, and cannot be measured when every trace edits the same area.
- **The fix, in line with the architecture review and the outcome-optimisation design.** Traces
  verify and value; they do not generate:
  - a typed **action classifier** (`adage-core::action`): role, environment and its class,
    runner, targets; "roughly the same" is an equivalence class, not a literal;
  - **local outcomes** (`adage-learn::outcome`): calls from a trigger to the first test run that
    actually ran tests, read from output whatever the exit code;
  - **directive value** (`adage-learn::value`, `adage-graph::stats`):
    `A = p_follow · (1 − p_sat) · (Q(other) − Q(Sat d)) − C − p_misfire · m`, with hierarchical
    empirical-Bayes priors pooled across repositories; admission is *verified or A's 90% lower
    bound > 0*; strength comes from A under the ceiling;
  - candidates **built from the repository**, not copied from traces: the project's runner
    (`.venv/bin/python -m pytest` from the declared environment) and the tests of the edited file
    (`edit_tests` slot: a map from the naming rule, git co-change and the traces).
- **Result.** Every Python repository now forms, from its training traces alone:
  - the dead-end reflex (after `python -m pytest` fails with `No module named …`, run
    `.venv/bin/python -m pytest`), at **fire**;
  - the test-target reflex (after editing `src/<pkg>/*.py`, run `.venv/bin/python -m pytest
    <that file's tests>`), at **prime**.
  - vue-core keeps its vitest reflex (`npm test -- packages/{part}`, prime), from the habit path;
    the value model says it is worth nothing there (agents already do it), and so does replay.
  - urfave-cli, the untouched generality check, learns nothing, which is right by the value
    model's lights but misses the small value the embedding lab's usefulness baseline found (§7).
- **Replay (held-out).** On the Python repositories, 186 directives in 103 of 110 held-out
  sessions: useful 186/186 (Wilson 90% lower bound 0.99), harmful 0, **2.37 calls saved per
  affected session** (task-clustered 90% lower bound 1.87). The pre-registered criterion of the
  outcome design §5.3 (dead-end situations, instincts off, leave-one-repository-out) passes:
  Ŝ_O − Ŝ_F = 1.50 calls per affected session (lower bound 1.12), harmful 0 = F's 0.
- **Live slice** (click, 6 held-out tasks, control vs adage, Haiku 4.5, $4.04): the learned
  test-target reflex was delivered in 5 of 6 adage sessions and run by the agent in 5 of 5; in
  the control arm agents ran it unprompted in 2 of 6. Session outcomes do not move at n = 6
  (as expected: §6).
- **Gates:** parity, gates (fidelity, false routes 0, gen datasets, adversarial), g4, bench (J),
  the dogfood fixture, fmt, clippy, the workspace tests and `evals/gen` all pass (§9).

## 1. Data and method

- **Traces.** The RepoBench run roots were still present (`/tmp/adage-repobench`,
  `/tmp/adage-rb2`). Per repository, a dataset in `adage eval` form:
  - **train:** the v1 training sessions (10 per repository; 7 for marshmallow), the only traces
    any tuning here saw;
  - **held out:** the unsteered arms of the eval phase, v1 control (16) and v2 control (24; 16 for
    vue-core; urfave-cli has v1 only). Adage-arm sessions are steered and left out.
- **Generality check.** urfave-cli (Go) was never used for any choice: not for tuning, not as a
  source of priors.
- **Static facts.** Each repository's history checked out from its mirror at the base commit of its
  *last training task* (so no held-out fix is in the history), plus the `.venv` marker the sandbox
  created. This is what `adage condition` reads at a user's repository root.
- **Priors.** Leave-one-repository-out: for each target, the value priors are fitted on the
  training traces of the other repositories (`examples/outcome_priors.rs`, κ by the method of
  moments), urfave-cli excluded as a source.
- **Replay.** `adage eval` (instincts off, `--params` with the fold's priors, `--sandbox-dir` with
  the root), then an interim implementation of the review's Ŝ (§5.2) over its directives
  ([learner/shat.py](learner/shat.py)), until the replay-metric work (L8) lands in `adage-eval`:
  - **Sat(d):** a later test run with d's environment class and runner (arguments free) that ran
    tests;
  - **useful:** Sat(d) happened, or a run of d's class ran tests somewhere in the held-out traces;
  - **harmful:** d's own class failed for the environment and never ran tests, or not useful;
  - **saved:** environment-friction calls (failed test runs, installs, environment probes; plus
    test discovery for a directive that names test files) between the directive and the first test
    run that ran tests, each call credited once per session, times p_follow 0.8, minus 0.1 per
    directive;
  - **calibration:** replaying U11 alone (instincts only, v2 held-out) predicts 2.09 calls saved
    per directive (click 0.73, attrs 2.81, marshmallow 2.50), inside the live measurement's
    interval [1.17, 2.56] (live: 1.85).

## 2. The drop ledger (default thresholds, training traces, before)

Every candidate the miner scored, and the gate that stopped it
([learner/ledger](../../crates/adage-learn/examples/ledger.rs), `cargo run --release -p
adage-learn --example ledger`). Patterns in fewer than 5 traces are never scored at all (the
miner's own support floor); the rows below are what got past it.

| Gate that dropped the candidate | click | attrs | marshmallow | vue-core | urfave-cli |
|---|---:|---:|---:|---:|---:|
| (traces, scored candidates) | 10, 65 | 10, 174 | 7, 189 | 10, 173 | 10, 227 |
| scorer: contrast < 0.3 | 24 | 98 | 40 | 146 | 165 |
| scorer: dead-end consistency (the runner passes elsewhere) | 2 | 53 | 73 | 0 | 4 |
| no delivery shape (a read or act context; context routes need MDL selection) | 19 | 6 | 28 | 15 | 40 |
| scorer: value V <= 0 | 6 | 3 | 13 | 5 | 9 |
| compiler: no dominant or covering variant (ambiguous) | 12 | 7 | 2 | 4 | 1 |
| compiler: not a fixed, plain command (inline code, quotes) | 0 | 4 | 22 | 0 | 0 |
| compiler: not an obligation, sequence or environment step | 0 | 3 | 11 | 0 | 0 |
| compiler: housekeeping | 0 | 0 | 0 | 0 | 6 |
| compiler: variable argument (one test file per run) | 2 | 0 | 0 | 3 | 0 |
| judge: support < min_support | 0 | 0 | 0 | 0 | 2 |
| **kept** | **0** | **0** | **0** | **0** | **0** |

What each gate did to the behaviour that mattered:

- **Scorer contrast.** The check after the last edit (`last[edit src/click/core.py] → python -m
  pytest`) has contrast 0.17 in click and −0.16 in vue-core: agents run tests after *every* edit
  and before editing, so the last edit is not special. The contrast gate is built for obligations
  (a regeneration owed by one file kind), not for checks.
- **Dead-end consistency.** `python -m pytest` fails in 24% of its runs in click, 21–27% in attrs
  and marshmallow: it fails first, then the agent installs pytest (and simplejson, hypothesis) into
  the system Python, and it passes. The gate reads the harm-class install as "the environment was
  fixed".
- **Ambiguous / variable argument.** Every candidate that reached the compiler with a test
  continuation died here: `exec python -m pytest {path} -xvs` has no dominant literal (74% of test
  strings are distinct) and no repo-wide form anyone ran successfully (click's full suite has a
  failing test at the base commit).
- **Not a fixed, plain command** (marshmallow's 22): `python3 -c '<stdin>'` repro scripts, rightly
  refused.
- **No delivery shape:** candidates whose context is a read or a successful act (`[read
  tests/**/*.py] → python -m pytest`) compile only as context routes, which need MDL selection;
  correct to leave out.

## 3. What should Adage have learned?

Read from the traces, per repository, as a user would:

| Repository | What agents repeatedly paid for | What the training traces show |
|---|---|---|
| click | `python -m pytest` → `No module named pytest` (8 of 10 training sessions), then `pip install pytest` into the system Python, sometimes `No module named 'click'`; finding the test file (`find -name "*test*"`) | 22 of 30 successful test runs are `python -m pytest` *after* a system install; `.venv` in 4 sessions (`.venv/bin/python`, `source .venv/bin/activate`); src → tests pairs: shell_completion.py → test_shell_completion.py 9×, core.py → test_options.py 5× |
| attrs | the same dead end, plus `No module named 'hypothesis'` after installing pytest | `.venv/bin/pytest` in 1 session; `_make.py` → `test_make.py` 13× |
| marshmallow | the same, plus `simplejson`, `pytz`, `dateutil` one install at a time (3–9 calls to a working test run) | **no** session used the `.venv`; schema.py → test_schema.py 8×, validate.py → test_validate.py 3× |
| vue-core | writing node scripts against unbuilt `dist/` (they fail), then `npm test -- <spec>` | `npm test -- packages/<pkg>/__tests__/<x>.spec.ts` in 48 of 48 test runs: agents already know how to test |
| urfave-cli | little: `go test -run X -v` works first time | nothing to fix at the test command |

So the value on these repositories is one reflex family: *run the tests of what you edited, in the
project's environment*, and its after-failure form. Frequency cannot find the environment half:
the right answer (the `.venv`) is a minority in click and attrs and absent in marshmallow. It has
to come from the repository (the declared environment) and be valued by outcome (the minority that
used it reached a working test run in 2 calls; the majority took 3–9, through a harm-class
install).

## 4. Which gates were wrong, and what replaced them

| Gate | Verdict | Replacement |
|---|---|---|
| `min_support` 5 with ≥ 2 tasks, for every reflex | Right for habits, wrong as the only admission: with 7–10 traces only what every session does passes, which is what the model already knows | Admission = **verified** (declared by the repository and seen running tests in ≥ 1 trace) **or** the one-sided 90% lower bound of the directive's value A > 0 with ≥ 3 effective samples (prior + data). Frequency gates stay for habit reflexes (obligations, checks) |
| Dead-end consistency (fails in ≥ 90% of the runner's runs) | Wrong where the "fix" is an install into the system | The value model's failure situations: a test run whose output says `No module named …` / `command not found` (exit code ignored), valued by calls to the first test run that ran tests |
| Compiler: dominant or covering variant | Wrong for narrowed test runs | The command is **built**: the project's runner from the repository's declared environment (else the project-environment spelling that ran tests), targets bound at hook time from the edited file (`edit_tests` slot) |
| Compiler: segment templates bind only whole path segments at ≥ 80% coverage | Too narrow | `tests/test_{part}` for `src/pkg/{part}` (a test file named for the edited file), a pytest node id or trailing options dropped, the prime share of runs (the judge counts per trace) |
| Judge: contrast unmeasurable / "agents already run it" | Right for a plain check | Unchanged for habits. The value model's mixture `Q(∅) = p_sat · Q(Sat) + (1 − p_sat) · Q(other)` derives the same thing ("a directive for what agents do anyway is worth nothing") from outcomes |
| Majority recovery as the dead end's `instead` | Harmful (pip into the system) | Candidates are safe by construction (project environments only; an install is never a candidate); ranked by value, not count |
| The runtime's learned after-failure trigger matched only the exact key | A learned `exec python -m pytest` never fired on `python -m pytest tests/x.py -xvs` | A trigger naming a bare runner matches by runner, as an instinct's does |
| Instinct extinguish on the ignore rate's Wilson bound | Extinguishes a working instinct when the follow judge is wrong (bug 1) | P(A > 0) < 0.1 over ≥ 5 judgements, or ≥ 3 misfires: an ignore only lowers p_follow |
| Demo vs default thresholds | Not the problem: `--demo` also compiled 0 routes on all five repositories | Unchanged |

## 5. Before and after, per repository (training traces, default thresholds)

| Repository | Before | After |
|---|---|---|
| click | none | `dead-end-586db0` **fire**: after `python -m pytest` fails with `No module named pytest`, `.venv/bin/python -m pytest` (A 2.49 calls, P(A>0) 0.98, verified); `dead-end-c44607` **fire**: the same after `No module named 'click'`; `run-3e72af` **prime**: after editing `src/click/*.py`, `.venv/bin/python -m pytest {tests}` (16 files mapped; A 4.41, verified) |
| attrs | none | four dead ends at **fire** (`python` and `python3 -m pytest`; `No module named pytest` and `'hypothesis'`), all to `.venv/bin/python -m pytest`; `run-7f0aa3` **prime**: after editing `src/attr/*.py`, `.venv/bin/python -m pytest {tests}` (11 files; `_make.py` → `tests/test_make.py`) |
| marshmallow | none | five dead ends at **fire** (`pytest`, `simplejson`, `pytz`; `python` and `python3`), to `.venv/bin/python -m pytest`, **from the priors alone**: no training session used the `.venv`; `run-d0b5a9` **prime**: after editing `src/marshmallow/*.py`, `.venv/bin/python -m pytest {tests}` (10 files; A 4.82, valued, not verified) |
| vue-core | none | `run-b62702` **prime**: after editing `packages/**/*.ts`, `npm test -- packages/{part}` (habit path, 6 of 9 traces). The tests-of-the-edit route (`npm test -- {tests}`, 170 files mapped) is weighed and not admitted: A 0.31, P(A>0) 0.75 (agents already run it) |
| urfave-cli (never tuned on) | none | none. No test-environment failures in its traces; the session-start test line (§7) was useful in 1 of 10 training sessions |

The ledger after, by gate, is the same table as §2 except for the rows the value path owns
(`kept` 1 in vue-core; `judge: explained by another route` 1 in click, where the tests-of-the-edit
route replaces the respelled `tests/test_{part}` template); the value path's own decisions are in
`EngineReport::valued`, `testmap` and `card` (`adage condition --json`).

## 6. Iteration log (change → replay → live)

Ŝ is calls saved per affected held-out session (task-clustered 90% lower bound), pooled over the
three Python repositories (110 held-out sessions), instincts off, leave-one-repository-out priors.

| # | Change | Directives | Ŝ / affected | Harmful | Notes |
|---|---|---:|---:|---:|---|
| 0 | Before | 0 | 0 | 0 | 0 routes in 5/5 repositories, at default and at demo |
| 1 | Segment template binds test files (`tests/test_{part}`) | 38 (click) | 0.92 (click) | 12 of 38 | Directs bare `python -m pytest`: a dead end in the held-out environment |
| 2 | Value model: failure situations, LOO priors | 1 | – | 0 | Formed everywhere, fired almost never: learned after-failure triggers matched only exact keys |
| 3 | Runtime: a bare-runner trigger matches by runner | 122 | 1.40 (1.08) | 12 | Dead ends fire (84, harmful 0); the bare-python template still harms |
| 4 | Respell steps that run a failing runner; `edit_tests` map | 186 | 2.37 (1.87) | 0 | Pre-emptive test reflex in all three Python repositories |
| 5 | Priors: a role without data borrows the null chain at every level | 152 | 2.00 (1.57) | 0 | Regression: the test reflex lost its prior in click and attrs |
| 6 | Pass over role-less levels; value as the `p_sat` mixture | 186 | **2.37 (1.87)** | **0** | Final. vue-core's test map no longer admitted on a cross-ecosystem prior |
| 7 | Co-change read from the object store (no `git log` process); dead-end ids include command and signature | 186 | 2.37 (1.87) | 0 | Same routes and replay as 6. A first-parent-only walk mapped fewer click files (11 vs 16) and dropped click to 1.14 (0.85); the date-ordered walk over all parents restores `git log`'s commit set. History roots are now plain local clones (`--shared` clones use alternates, which the reader does not follow) |
| live | click slice, 6 tasks × {control, adage}, $4.04 | 6 of 6 sessions got a learned directive | – | – | see below |

**Live slice** (evals/repobench/configs/v2/click-slice.json: 6 held-out tasks, 1 repeat, Haiku
4.5, `max_turns` 40, confined, host auth; learned by `adage condition` from the 10 training
transcripts, instincts bound as usual):

| | adage | control |
|---|---|---|
| Learned test reflex (`run-3e72af`) | delivered in 5 of 6; the agent ran it in **5 of 5** (`obligation_met`) | would have fired in 6 of 6; agents ran it unprompted in 2 of 6 |
| Learned dead end (`dead-end-95d94b` in the slice build; now `dead-end-586db0`, as dead-end ids include the failing command and signature) | delivered in 2; judged "ignored" 2 of 2 (the literal follow judge, bug 1) | would have fired in 4 |
| Runtime follow rate | 0.75 | 0.17 (coincidence) |
| Environment dead-end hits per session | 1.33 | 1.33 |
| Turns to first edit (median) | 8 | 10.5 |
| Pass rate | 0.33 | 0.50 (6 tasks: noise) |
| Cost | $2.03 | $2.01 |

The slice confirms what replay predicts about *delivery* (the reflex fires where replay says, and
agents run it); it cannot resolve 2 calls at n = 6. In this run the control agents already used
the `.venv` in 19 of 31 test runs (v2 control: 37% of next test runs), so there was less friction to
remove.

## 7. Against the baselines

- **Frequency (F: the current engine at `--demo`, plus the most frequent safe recovery):** no
  route in any Python repository (the most frequent recovery is a system install, excluded; the
  `.venv` recovery has 2 traces in click, below demo's 3). Pre-registered criterion (outcome design
  §5.3, dead-end situations): Ŝ_O = 1.50 per affected session (90% lower bound 1.12), 84 directives,
  useful 84/84, harmful 0; Ŝ_F = 0, harmful 0. **Pass.**
- **The embedding lab's usefulness baseline** ("the command most often a useful directive in
  training, once per session, at usefulness ≥ 0.7 over ≥ 2 sessions"): precision 0.73 on the
  tuning repositories and 1.00 / 0.91 on urfave-cli / vue-core, 0.4–1.6 calls per session by its
  own metric. The learner now includes that rule (`testmap::card`, the advice template
  `test-command`, delivered once at session start), with a stricter usefulness label: *useful*
  only if the session later ran tests the directed way (project environment) and first spent a
  failed command or ≥ 2 steps finding out how. By that label it is useful in 1 of 10 training
  sessions in click, attrs and urfave-cli, 0 in marshmallow and vue-core, so it forms nowhere. The
  lab's label counts any two exploratory steps (reading code included) before the command, and
  counts bare `python -m pytest` as the command in the Python repositories, which is the dead end
  there. So on the three Python repositories the learner's reflexes save more (1.14–2.95 calls per
  held-out session vs 0.3–1.9), and on urfave-cli and vue-core the lab baseline's ~0.4–0.6 calls
  per session are not captured. Metrics differ (Ŝ counts friction in each directive's window; the
  lab counts discovery), so compare orders of magnitude, not decimals.
- **Cross-repository transfer** goes through abstract cells only (ecosystem, environment kind,
  trigger family, role): no command, path or prompt crosses repositories.

## 8. The generality check (urfave-cli)

Never tuned on, never a prior source. The learner forms nothing there: its traces contain no
environment failure of a test runner (Go brings its own), the test-target map does not apply to
Go's package-and-name test selection (`go test -run X ./pkg`), and the session-start line was
useful in 1 of 10 sessions. The value model's answer "there is nothing to say" agrees with the
architecture review's waste table (0 of urfave-cli's runs hit an environment dead end); the lab's
~0.56 calls per session of discovery is the value left on the table.

## 9. Gates

| Check | Result |
|---|---|
| `cargo test -p adage-eval --test gates` (fidelity, false routes 0 on every committed dataset and generated bundle, adversarial unsafe 0, determinism) | pass (15/15) |
| `--test parity` | pass |
| `--test g4` | pass |
| `--test bench` (J ≥ 0.6307, committed baseline reproduced) | pass (4/4) |
| `crates/adage-learn/tests/rust_workspace.rs` (dogfood fixture) | pass |
| `cargo fmt --all --check`; `cargo clippy --workspace --all-targets --locked -- -D warnings` | pass |
| `cargo test --workspace --locked` (in parts) | pass, every crate. `adage-cli --test live_learn` fails 4 tests when run in the release profile only, because its spawn recorder is `#[cfg(debug_assertions)]`; in the debug profile 7/7 pass. `adage-exec --test no_spawn` passes: the co-change map reads the git object store with `adage_core::gitobj` and spawns nothing |
| `python3 -m unittest discover evals/gen` | pass (22 tests) |

## 10. What changed, where

| Crate / file | Change |
|---|---|
| `adage-core/src/action.rs` (new) | Typed action classifier: role, environment and class, runner, targets, selectors, piped; `satisfies()` as the follow equivalence (for the runtime's follow judge, bug 1) |
| `adage-learn/src/outcome.rs` (new) | Goal detectors (G_test from output, exit code ignored), environment signatures, harm, local reward, situation keys |
| `adage-learn/src/value.rs` (new) | Directive value, priors, failure routes; `judged()` for instincts |
| `adage-learn/src/testmap.rs` (new) | The file → tests map (naming; co-change over the last 2,000 non-merge commits reachable from HEAD, read from the object store without spawning, pair counts identical to `git log --no-merges` on 4 of 5 repositories and within one commit on 2 marshmallow pairs; traces), the tests-of-the-edit route, the session-start line |
| `adage-learn/src/engine.rs` | Value routes replace mined routes on the same trigger; respelling of steps that run a failing runner; the test route explains mined checks of the same tool |
| `adage-learn/src/instincts.rs` | Extinguish by value (L6) |
| `adage-graph/src/stats.rs` | Gamma–Poisson posteriors with a supplied prior, κ by moments, `advantage`, `advantage_mix` |
| `adage-graph/src/compile/shapes.rs` | Segment templates bind test files named for the edited file; stable prefix; prime share |
| `adage-graph/src/compile/ids.rs`, `select.rs` | `edit_tests` ids by glob and bound; `candidates_shaped` for the ledger |
| `adage-tracks` | `SlotSource::EditTests` (validated, bounded); advice template `test-command`; `learn.outcome_priors` |
| `adage-runtime/src/lib.rs` | Binds `edit_tests` (each file must exist); a learned bare-runner after-failure trigger matches by runner |
| `adage-eval/src/lib.rs` | `--sandbox-dir` feeds the static scan with instincts off |
| `adage-learn/examples/{ledger,outcome_priors}.rs` | The drop ledger; cross-repository prior fitting |

## 11. Limits and follow-ups

- **The runtime's follow judge is still literal** (bug 1): in the slice the learned dead end was
  judged "ignored" 2 of 2. `adage_core::action::satisfies` is the equivalence the runtime-fix work
  can call; with it, the extinguish rule above reads correct follows.
- **Directive text** for an after-edit run says "must be followed by running … Agents in this
  repository run it after such changes" (the runtime's wording for every learned after-edit run),
  and the runtime makes it an obligation with a Stop request. For a valued test reflex the plain,
  factual wording would be "after changing these files, the tests for them run with: …".
- **Piped failures** (exit 0) never raise the after-failure event (bug 2): the dead-end routes fire
  only on non-piped failures; the after-edit test reflex pre-empts both.
- **The miner is unchanged.** The abstraction lab's proposal (intent-level items, env class as an
  attribute, a `tests_of` constraint) would let the habit path see what the value path now builds
  directly; not done here.
- **The interim Ŝ** is a scratch implementation of review §5.2; numbers should be recomputed with
  the L8 implementation when it lands. Replay assumes p_follow 0.8 and cannot see compensating
  behaviour.
- **Model distribution** (from the embedding lab): Hugging Face, hf-mirror, ModelScope, jsDelivr
  and GitHub releases are blocked here, but npm's `@yarflam/potion-base-8m` is byte-identical to the
  pinned potion-base-8M SHA-256s (MIT) and the qdrant-fastembed GCS bucket is reachable: candidates
  for the `adage models fetch` fallback (not changed here).
