# Recurring workflows: does learning compound when the procedure repeats?

2026-09-29, research sprint wave 2, slug `recurring-workflows`. Live Claude Haiku 4.5 sessions
(Claude Code 2.1.284) run through the RepoBench runner with `--host-auth --confine`. Spend was
**$24.22 of $25** over 86 sessions (`spend.json`). The design was pre-registered in `prereg.json`
before any session ran.

## Question

Wave 1 found only small levers (under 5%) on RepoBench. RepoBench tasks are one-off bug fixes, so
there is little to carry from one session to the next. Real use is dominated by **recurring** work
in the same repository, where the procedure repeats and the specifics change. On that kind of work,
does learning from earlier sessions of the same family produce large savings? And how do the
savings grow with the number of earlier sessions?

## Answer

**Partly yes.** A procedure card learned from 4 earlier sessions of the family saves about 19% of $
per task (CI 12–26%) with no pass-rate loss. That is about twice what today's Adage delivers on the
same tasks. **But the learning does not compound.** The curve is a step: almost all of the
learnable structure is present after the **first** session of the family, and the curve is flat
after that.

## Benchmark (`families.py`, `validate_families.py`)

There are two task families on real repositories at their current upstream heads, with 10
variants each. The variant order was fixed before running. The first 4 variants are the training
sessions, and variants 5–10 are held out.

| family | repository @ base | procedure that repeats | variants (training first) |
| --- | --- | --- | --- |
| `click-type` | pallets/click `06b2a67` (8.6.0 unreleased) | add `click.<NAME>`: class and instance in `types.py`, export in `__init__.py`, `docs/api.md` autodata, a `docs/parameter-types.md` bullet, a `CHANGES.md` entry, a test; Click's light-imports CI test must stay green | HEX_COLOR, PORT, DURATION, IP_ADDRESS \| SEMVER, PERCENTAGE, BYTE_SIZE, SLUG, DATE, HOST_PORT |
| `mm-validator` | marshmallow `7f0792bd` (4.4.0 unreleased) | add `marshmallow.validate.<Name>` following the validators' `error`/`_repr_args`/`_format_error` conventions, with a docstring, a `CHANGELOG.rst` entry and tests | StartsWith, MultipleOf, Unique, Hexadecimal \| EndsWith, IPAddress, NotBlank, Ascii, Contains, Lowercase |

- **Prompt.** Each prompt is issue-style. It gives the behaviour with examples and names the
  deliverables ("tests, API reference and guide, changelog"), but not where they go or how they
  are done.
- **Hidden check (`rw_score.py`).** The scorer writes the variant's hidden test, restores Click's
  pristine `tests/test_imports.py`, and runs the **whole** test suite, which is the project's CI.
  The hidden test checks four things:
  - the behaviour: valid inputs, invalid inputs failing with a usage error, the default and custom
    messages, and the repr;
  - that the new item is documented (docs files, or a docstring for marshmallow);
  - the changelog entry in the unreleased section;
  - that the agent wrote a test referencing the new item.
- **Validation.** All 20 hidden checks fail on the base and pass with a gold patch
  (`validation.json`). The sandbox and scorer reuse RepoBench's `sandbox.create`, scorer helpers,
  `.venv` setup and confinement.

## Arms

On the 6 held-out variants per family, each arm ran 2 repeats. Arm order was seeded per (variant,
repeat), and runs are paired by (variant, repeat): 24 pairs over 12 variants per contrast.

- **control:** Adage hooks on, directives withheld (the RepoBench control).
- **adage:** the current `target/debug/adage`, with `adage condition --demo` over the 4 training
  transcripts and their outcomes. It learned:
  - one click route: after editing `src/click/*.py`, `uv run pytest {tests}` (prime);
  - the marshmallow route, which stayed nascent and was never delivered;
  - the U11/U3 instincts.

  34 directives were delivered in 24 sessions: 22 were U11 "No module named pytest" and 12 were
  the click route.
- **card:** the control's hooks plus a **procedure card** learned by `procedure.py` from the same 4
  training transcripts and committed as `CLAUDE.md` (the runner's agents-md slot). The card
  contains:
  - the files edited in at least half of the sessions, in median order of first edit, with each
    session's differently named new test file anti-unified to `tests/test_types/<new>.py`;
  - for each file, the **added lines** of the edit from the latest passing session, as an
    exemplar;
  - the commands run in at least half of the sessions, with their success rates, plus an avoid
    list for commands that mostly failed (with the error's first line).

  The cards are in `cards/` and are about 500–700 tokens.
- **card n=1 (optional pre-registered stage):** a card from the family's first session only, run
  on the marshmallow held-out variants with 1 repeat.

## Results (held out, live)

$ is billed `total_cost_usd`. Each contrast is arm/control − 1 over the paired sessions. CIs are
90% percentile bootstraps clustered by variant (12 clusters pooled, 6 per family). With this few
clusters they under-cover, as the harness showed for RepoBench slices, so read them as roughly 80%.

| arm (24 sessions each) | $/session | tool calls | pass rate [Wilson 90%] | Δ$ vs control [90% CI] | Δcalls vs control [90% CI] |
| --- | ---: | ---: | --- | --- | --- |
| control | 0.317 | 41.5 | 19/24 = 0.79 [0.63, 0.90] | – | – |
| adage (today) | 0.289 | 37.0 | 21/24 = 0.88 [0.73, 0.95] | **−8.8%** [−14.6, −1.9] | −10.7% [−17.9, −2.7] |
| **card (n=4)** | **0.258** | **34.8** | 20/24 = 0.83 [0.68, 0.92] | **−18.8%** [−25.8, −11.6] | **−16.0%** [−23.5, −7.8] |
| card vs adage (paired) | | | | −10.9% [−17.4, −3.9] | −6.0% [−10.9, −0.2] |

Per family:

| | click-type Δ$ | mm-validator Δ$ | click pass (ctl / adage / card) | mm pass |
| --- | --- | --- | --- | --- |
| adage | −7.1% [−14.1, +2.4] | −11.7% [−19.7, −2.6] | 7 / 9 / 9 of 12 | 12 / 12 / 11 of 12 |
| card | −16.3% [−26.5, −5.1] | −22.9% [−29.6, −15.0] | | |

**Pass rate** is non-inferior, as pre-registered. On pairs, card vs control is 3 wins against 2
losses. Click controls mostly failed on procedure: 4 missed documentation checks and 2 missed
changelog checks. Card sessions missed 1 of each.

**Where the card's savings came from** (calls per session, card vs control):

| | card | control |
| --- | ---: | ---: |
| searches and listings | 4.5 | 7.8 |
| test and other runs | 7.0 | 10.1 |
| failed calls | 1.5 | 2.3 |
| reads | 10.7 | 10.3 |
| calls before the first edit | 12.8 | 13.0 |

So the card removed search and the verify/fix loop. **It did not shorten orientation**: agents
still read the same files before editing.

**Today's Adage is U11 here.** The harness U11 replay on these control sessions predicts +7.6% of $
[5.2, 10.6] and +3.6 calls (`u11_replay.json`); live gave 8.8% and 4.4 calls, which is consistent.
That is 4× U11's RepoBench value (1.9%), because in a recurring family every session hits the same
system-Python pytest dead end. The same effect makes environment friction a compounding cost of
recurring work.

## Offline decomposition (`analyze.py`, harness cost model, 32 control and training sessions)

$ figures are for the removal of a set of calls, computed as a direct counterfactual: removed
responses are dropped, and later responses are charged for the context left once the removed
calls' share of it is gone. This replaces summing `costmodel.removal()`, which double-counts large
removals. The role taxonomy is an instrument only.

| | pooled | click-type | mm-validator |
| --- | --- | --- | --- |
| repeated-procedure share of calls¹ | 50% [48, 53] | 46% | 55% |
| novel share of calls | 50% | 54% | 45% |
| oracle "perfect procedure"²: $ removable | 73% [70, 76] | 66% | 79% |
| oracle "hook-executed" (also template edits³) | 77% [74, 79] | 71% | 82% |
| realistic checklist⁴ (predicted) | 22% [21, 24] | 19% | 25% |
| **card, live** | **18.8%** [11.6, 25.8] | 16.3% | 22.9% |

1. An action (tool plus path, or a normalised command) counts as repeated procedure if it occurs in
   at least half of the family's *other* sessions.
2. The perfect-procedure oracle keeps only one read of each file finally edited, the successful
   edits, and the final successful command.
3. Template edits are edits to files changed in every session whose added text, with variant names
   masked, is at least 0.6 similar across sessions. These were click `__init__.py` and
   `docs/api.md`, and marshmallow `CHANGELOG.rst`.
4. The realistic checklist removes searches before the last card file's first touch, and failures
   of commands on the card's avoid list.

Call shares by role (pooled) are: reads 25%, runs 24%, searches 18%, edits 16%, git 10%, failed
6%.

- **The ceiling is large.** Roughly three-quarters of a recurring task's $ is outside the minimal
  path.
- **Hook-executed steps add little.** Executing the templatable boilerplate edits (export line,
  autodata block, changelog bullet) adds only about 4 points over the perfect-procedure oracle.
  Those edits are cheap; the cost is in the exploration and verification loops around them.
- **The card captures about a quarter of the ceiling.** The live 18.8% is about 26% of the
  perfect-procedure oracle, and it is close to the realistic-checklist prediction (22%). The rest
  sits in reads before the first edit (unchanged by the card) and in repeated test runs.

## Learning curve: savings vs number of prior sessions in the family

| prior sessions n | offline realistic $ share (90% CI, 32 sessions) | card-file recall / precision | live Δ$ vs control, marshmallow (6 variants) |
| ---: | --- | --- | --- |
| 0 | 0 | – | 0 (control, $0.235) |
| 1 | 21.8% [20.3, 23.4] | 0.91 / 0.91 | card n=1: **−15.3%** [−30.9, +0.4], 6/6 passed |
| 2 | 22.2% | 0.91 / 0.84 | |
| 3–9 | 22.2% [20.6, 23.7] (flat) | 0.91 / 1.00 | card n=4: **−22.9%** [−29.6, −15.0], 11/12 passed; adage n=4: −11.7% |

The offline curve uses random prior sets of other variants, 8 draws per held-out session and n.

**Learning does not compound; it steps.**

- **The first session gives almost all of it.** One earlier session already gives the card its
  file list (recall 0.91), its exemplars and its test command.
- **More sessions only prune.** From 3 sessions on, precision is 1.00 and the realistic value is
  flat.
- **Live agrees within noise.** The n=1 card got about two-thirds of the n=4 card's saving
  (−15% vs −23%, overlapping CIs, 6 sessions).
- **What more sessions could add is outcome signal, and the card doesn't use it yet.** The one
  pattern that appears only across sessions is a repeated failure mode: the IP_ADDRESS training
  session failed Click's light-imports CI (it added `import ipaddress` at module top). To learn a
  warning about that, the card needs outcomes it doesn't read today.

## Verdict

1. **On recurring work the lever is large, and it is a different lever from reflexes.** A learned
   procedure card, meaning "which files, in what order, what the edit looked like, which test
   command works", saves about 19% of $ and 16% of calls with no pass-rate loss. It beats today's
   Adage by 11% of $ on the same pairs. It passes the pre-registered threshold (≥ 15%, CI lower
   bound > 0), with the caveat that the CIs are about 80% in practice.
2. **Today's Adage is worth 2 to 4 times more on recurring work than on one-off fixes** (8.8% vs
   1.9–3.9%). The same environment dead end repeats in every session of a family, so U11 fires
   every time.
3. **"Traces become reflexes" is not a compounding curve here.** The value arrives after the first
   session of a family and then plateaus. The headroom above it (a 73% oracle against the card's
   19%) is not reached by more sessions. It is reached by making the agent skip orientation and
   test loops. The card doesn't do that: calls before the first edit were unchanged.

## What to build

1. **`adage` workflow families plus a compiled procedure card** (Rust, `adage-train`/`adage-core`).
   - **Clustering.** Cluster a repository's sessions into families by prompt similarity
     (potion-base-8M, the existing `intent` trigger) together with a shared edited-file set.
   - **Compiling.** After the **first passing session** of a family, compile a card (this
     `procedure.py`: frequent files in order, added-line exemplars, working command, avoid list),
     and verify its commands in a sandbox, as the skill compiler does.
   - **Delivery.** Deliver it at `UserPromptSubmit` when a new prompt matches the family. It costs
     about 600 tokens, which carried over about 35 responses is roughly $0.003 per session.
   - **Expected value.** About −15% to −20% of $ on family-matching tasks. The overall effect is
     that times the share of a user's work that is recurring, which is unknown and plausibly 30–60%.
2. **Keep U11 predictive** (the harness's recommendation). On recurring work it is worth about 8%,
   not 2%.
3. **Before building, measure in the next micro-slice:**
   - the family trigger's precision on real prompt streams (a wrong card is 600 tokens of
     misleading context);
   - a card delivered by hook instead of as `CLAUDE.md`;
   - an outcome-aware card, which warns about failures seen in failed sessions.

## So what

Shipping a learned procedure card for recurring families is expected to save **18.8% of $ per
task (90% CI 11.6–25.8%) and 16% of calls** on tasks that match a family, with pass rate unchanged
(20/24 vs 19/24). That is **+10.9% of $ beyond today's Adage** on the same pairs. The value is
available after **one** earlier session of the family and does not grow after that.

**Confidence that it survives a live A/B: moderate.** Against it:

- 12 variants, 2 families, 1 model;
- families and prompts designed by the researcher, with the deliverables named;
- membership in the family given, not detected.

For it:

- two families agree in sign and size;
- the offline realistic prediction (22%) matched live (19%);
- an n=1 replication points the same way.

**Confirming micro-slice:** a third family on another repository (for example urfave-cli "add a
flag type") with a card delivered at `UserPromptSubmit` by hook, 6 variants × 2 repeats × {control,
card}, about $6.

## Hard-coded parts (honest list)

- **Benchmark (not the learner).** The families, prompts, hidden checks and gold patches were
  written by hand.
- **`procedure.py` (the learner).**
  - which tools edit files (Edit, Write, MultiEdit, NotebookEdit);
  - the shell normalisation: a leading `cd …&&` dropped, pipes, redirections and heredoc bodies
    dropped, 6 tokens kept;
  - the "at least half of the sessions" threshold, the clip length, and the card's fixed headings.

  It names no file, command or error of these repositories.
- **`analyze.py` (instrument only).** The role taxonomy (find/grep/rg/ls/tree = search;
  cat/head/tail/sed/wc = read; git = vcs), the 0.6 template-similarity threshold, and the
  definitions of the oracle and realistic sets.
- **Family membership was given.** All prompts of a held-out family got that family's card. A real
  system must detect the family, and that trigger was not measured here.

## Reproduce

```bash
cd evals/lab/research/recurring-workflows
python3 validate_families.py                       # 20/20 valid, no tokens
python3 make_configs.py                            # configs/ (+ budget caps edited: click 14.7, mm 11.6)
R=/tmp/adage-research/recurring-workflows/runs
env -u CLAUDECODE -u CLAUDE_CODE_SESSION_ID python3 ../../../live/runner.py configs/<family>.json train --execute --host-auth --root $R/<family> --adage ../../../../target/debug/adage
python3 procedure.py --out /tmp/adage-research/recurring-workflows/cards/<family>.md --outcomes <sid->passed.json> <4 training transcripts>
env -u CLAUDECODE -u CLAUDE_CODE_SESSION_ID python3 ../../../live/runner.py configs/<family>.json learn eval --execute --host-auth --root $R/<family> --adage ../../../../target/debug/adage
# optional n=1 stage: configs/mm-validator-card1.json with cards/mm-validator.n1.md
OMP_NUM_THREADS=1 nice -n 10 python3 analyze.py     # analysis.json; $0
```

Raw runs (transcripts and state, 101 MB, sandboxes pruned) are in
`/tmp/adage-research/recurring-workflows/runs`. They are not committed.
