2026-09-29. **RepoBench v2**, the rerun for the launch evaluation ([evals/launch](../../../launch/README.md)).
The same task definitions, chronological splits and archived training traces as
[v1](../2026-09-28-repobench/README.md), on click, attrs, marshmallow and vuejs/core, with three changes:
`max_turns` 60 (v1: 30, which censored 174 of 228 headline sessions), learning at the thresholds a
user's automatic learner uses (`learn.thresholds = default`; v1 used `--demo`), and instincts bound
against a fresh sandbox of the repository (its `.venv`, `node_modules`), as the zero-config bootstrap
binds them against a real checkout. Arms: `control` (hooks on, directives withheld) and `adage`.
Claude Haiku 4.5 at 3 repeats on the Python repositories and 2 on vuejs/core (164 sessions); a Claude
Sonnet 5 slice on click and vuejs/core (32 sessions). Claude Code 2.1.284. `evals/repobench/rerun.py`
seeds the roots; `configs/v2/` holds the configs.

**The short version: Adage now delivers something on real Python repositories, the agents follow it,
and it saves about two tool calls in the sessions that need it. That is too small to move any
session-level outcome, and nothing was learned from the traces.**

- **Learned: still nothing.** From the same 7–10 training traces per repository, `adage condition` at
  default thresholds compiled 0 reflexes in 4 of 4 repositories (the drop reasons are v1's: `python -m
  pytest {path} -xvs` "is ambiguous" or "has a variable argument"). Every directive below is an
  instinct.
- **Delivered: yes, on the Python repositories.** U11 ("after `python -m pytest` fails with `No module
  named pytest`, run `.venv/bin/python -m pytest`"), bound from each repository's own `.venv`, was
  delivered in 41 of 66 Haiku adage sessions on click, attrs and marshmallow (one directive each), and
  in 0 of 16 on vuejs/core (its N1 routes are now bound only to npm's own refusals, which never
  happened). Sonnet 5: 1 of 16.
- **Followed: every time, but Adage scored it as ignored.** In 41 of 41 sessions the agent's next test
  command ran `.venv/bin/python -m pytest …` with its own arguments; Adage's follow judge requires the
  directed command with options only, so it judged all 41 `ignored` (0 followed), and the instinct
  auto-extinguishes after 5 such judgements (see the launch report, bug 1).
- **The mechanism works.** In sessions that hit the dead end (63 of 82 adage, 58 of 82 control), the
  agent reached a test run that actually ran tests in **1.44 tool calls with Adage vs 3.29 without**
  (task-clustered diff −1.85, 95% CI [−2.56, −1.17], sign-flip p = 0.0001, 21 tasks), and its next
  test command used the project's environment in 71% vs 37% of them.
- **The outcome does not move.** Pooled Haiku: resolve rate 0.62 with Adage vs 0.65 without (diff
  −0.02, CI [−0.10, 0.04]); mean cost $0.29 vs $0.30; **$ per resolved task $0.47 vs $0.47** (CI
  [−0.06, 0.09]); median turns 35 vs 33.5; turns to first edit 8 vs 8. No metric differs after Holm.
  Harm: 3 of 30 tasks resolved less often with Adage, 2 more often (noise of the same size both ways).
  Two tool calls in sessions of 30–40 turns is about 1 turn and ~$0.01 at Haiku prices; the design can
  detect nothing near that.
- **A third of the dead ends are invisible to Adage.** 22 of the 63 Haiku adage sessions that hit U11's
  dead end, and 5 of 6 Sonnet ones, hit it only as `python -m pytest … 2>&1 | head`: the pipeline exits
  0, so `PostToolUseFailure` never fires and no failure-triggered instinct can match (bug 2).
- **Sonnet 5 slice.** Resolve rate 0.44 vs 0.44; mean cost $0.51 vs $0.64 (CI [−0.25, −0.01], Holm p =
  0.50), $ per resolved $1.17 vs $1.45. With one directive delivered in 16 adage sessions, that cost
  difference cannot be Adage's doing: it is task-level noise.
- **Censoring is fixed.** 18 of 164 Haiku sessions hit 60 turns (v1: 174 of 228 at 30); median turns and
  cost are now informative. Costs are higher than v1's (click $0.33 vs $0.22 a session) because the
  sessions are no longer cut off.

**Rerun details.** While the click and attrs held-out phases were running, an unconfined zero-config
E2E session (evals/launch) `pip install`ed pytest and an editable click into the container's system
Python; every click and attrs held-out run that ended after that moment (37 runs, $6.48) was
discarded and rerun once the packages were removed. The spend table below includes the reused v1
training sessions copied into each root ($11.70 over the six roots, not spent again); this run's own
spend is in the launch report.
