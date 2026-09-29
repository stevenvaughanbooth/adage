# GO validation: the shipped product on a fresh recurring family

Prepared 2026-09-29. **No paid session has run.** This is the end-to-end validation to run once the
product binary (procedure cards and uplift gating, merged) is ready. The pre-registration is
`prereg.json`; `analyze.py` reads its thresholds from there.

## What it tests

A user works in a repository with Adage installed. After each session the product's own background
learner (`adage condition --auto`, started by the Stop/SessionEnd hooks) forms families and compiles
a procedure card, and the UserPromptSubmit hook delivers the card when a new prompt matches. The
harness only installs the hooks (`adage hooks-config`, as `adage init` does) and gives the sessions
one persistent Adage data directory. It never runs `adage condition` for the family.

## Family: `ufo-util` (unjs/ufo, TypeScript, pnpm, vitest)

This repository and ecosystem were never used for card research. The task is "add a URL utility":

- the function in `src/utils.ts`, with its JSDoc;
- tests;
- the README reference, which `automd` generates from the JSDoc;
- CI, which runs `eslint .`, `prettier -c src test` and `vitest run --typecheck`.

The variant order was fixed before any session (`family_ufo.py`):

- **Training (the user's history):** `withoutQuery`, `withPort`, `getOrigin`, `getPathSegments`.
- **Held out:** `withoutAuth`, `isSameOrigin`, `sortQuery`, `withLowercaseHost`, `getExtension`,
  `withoutDotSegments`, `getQueryParam`, `isHttpURL`.

**Hidden check** (`gv_score.py`). All five checks must pass:

- lint;
- the README heading as the agent left it;
- the README heading after `automd` regenerates the README;
- the agent's own test naming the utility;
- the whole suite with typecheck, plus a hidden behaviour test covering the export and every
  example in the prompt.

**Validation** (`validation.json`): **12 of 12 variants** fail on the base and pass with the gold
patch. The check takes 12–16 s.

**Negatives** for the false-fire check:

- 5 RepoBench ufo bug fixes whose real fix edits `src/utils.ts`;
- 3 hand-written hard negatives: a docs-only change, a bug fix, and an option added to an
  existing utility.

## Arms and slices

| | sessions | how conditioned |
| --- | ---: | --- |
| training | 4, one at a time | product installed, learning by itself; the driver waits for each learner to finish |
| held out: `control` / `adage` | 8 variants × 2 repeats × 2 arms = 32, paired | the index the product learned; learning frozen (`ADAGE_AUTO_LEARN=0`); `control` withholds directives |
| regression (click) | 6 × 1 × 2 = 12 | click-a3slice's held-out one-off fixes; `adage condition` over the same 10 archived training transcripts, **without outcomes** |

- **Runner flags:** `--host-auth` and `confine: true` (bubblewrap). `CLAUDECODE` and
  `CLAUDE_CODE_SESSION_ID` are never forwarded, and `gv_run.py` refuses to start while either is
  set.
- **Training sessions** keep the runner's mounts but drop `--unshare-pid` and `--die-with-parent`,
  so the detached learner survives the session's exit.
- **The learner** reads earlier sessions' transcripts from the product's parse cache.

## Budget (target ≤ $15)

| | sessions | central | pessimistic | hard cap |
| --- | ---: | ---: | ---: | ---: |
| family ($0.20 / $0.30 a session) | 36 | $7.20 | $10.80 | $9.50 |
| regression ($0.30 / $0.40 a session) | 12 | $3.60 | $4.80 | $4.00 |
| **total** | 48 | **$10.80** | | **$13.50** (+ ≤ 2 in-flight sessions) |

**Where the per-session estimates come from:**

- ufo bug fixes cost $0.115 a session (p90 $0.19) in the data-expansion corpus.
- The earlier recurring families cost 1.0–1.5× their repository's bug fixes.
- For click, click-a3slice measured $0.294 and $0.306 at 40 turns.

**How the caps are enforced.** `gv_run.py` sums the spend already recorded in both roots and clamps
each runner cap to what remains of $15. The gate, probes and analysis cost $0.

## Commands (from a terminal, not inside an agent session)

```bash
cd evals/launch/go-validation
BIN=/abs/path/to/the/merged/release/adage          # e.g. target/release/adage after the merge
R=/tmp/adage-launch/go-validation/runs
RUN="env -u CLAUDECODE -u CLAUDE_CODE_SESSION_ID python3 gv_run.py"

# 0. No spend: the dry run against the product binary (validation, configs, conditioning + probe, analysis)
python3 dryrun.py --skip-validate --adage $BIN          # exit 0 = every check passed

# 1. Training: the user's history (4 sessions, sequential, ~$0.8)
$RUN configs/ufo-util.json train --execute --host-auth --root $R/ufo-util --adage $BIN

# 2. Gate R2/R3 ($0): the product formed a card, and the hook delivers it for >= 4/8 held-out prompts
python3 detect_check.py --home $R/ufo-util/private/adage-home --adage $BIN --out $R/ufo-util/gate.json
#    exit 3 = stop (report NO-GO / detection failure); gate.json also fixes G2b's delivered subset

# 3. Held out, repeat 1 (16 sessions), then the harm check R4
$RUN configs/ufo-util.json eval --eval-repeats 1 --execute --host-auth --root $R/ufo-util --adage $BIN
python3 analyze.py --interim                            # exit 4 = stop for harm
$RUN configs/ufo-util.json eval report --execute --host-auth --root $R/ufo-util --adage $BIN   # resumes: repeat 2 only

# 4. Regression slice (refuses if < $3 of the global cap remains)
$RUN configs/click-regression.json learn eval report --execute --host-auth --root $R/click-regression --adage $BIN

# 5. Pre-registered analysis and verdict ($0)
python3 analyze.py --out analysis.json
```

A run that is interrupted resumes when you re-run the same command, because the runner skips
finished sessions. Keep disk use under 500 MB:

- the ufo pnpm store is about 195 MB (`/tmp/adage-launch/go-validation/cache`);
- the scorers prune `node_modules` and `.venv` after each session.

## Dry run (`dryrun.json`, $0)

The dry run used the card-branch build `.claude/worktrees/agent-ae7d626216059442c/target/debug/adage`
(branch `worktree-agent-ae7d626216059442c` at 4882a34), the only card-capable binary so far. Its
conditioning ran on four synthetic training transcripts built from the gold patches.

**What passed:**

- **Validation:** 12/12 variants valid.
- **Configs:** 36 sessions (4 training, 16 control, 16 adage) for the family and 12 for the
  regression.
- **Isolation:** training sessions get only `ADAGE_HOME` and the hooks. Held-out sessions get
  `ADAGE_ARM` set to control or treated, with `ADAGE_AUTO_LEARN=0`.
- **Guards:** no forbidden variable is forwarded, and the confinement and learner-wait logic behave
  as intended.
- **Card:** the product forms a card for the family (`src/utils.ts`, `test/utilities.test.ts`,
  `pnpm vitest run`).
- **Analysis on synthetic runs:** coverage of the 90% interval is 0.85–0.86 with 8 clusters. At a
  true −20% effect, P(GO) is 0.50, or 0.65 counting GO-WITH-CAVEATS. Under a null effect, P(GO) is
  0.06.

**Findings that shaped the pre-registration (all before any live session):**

1. **Detection recall 5/8, negatives 0/8.** At the shipped threshold (0.811) the installed hook
   delivers the card at the prompt for 5 of 8 held-out prompts. It misses `isSameOrigin`,
   `getQueryParam` and `isHttpURL`. It fires on none of the 8 negatives. The primary metric is
   therefore stated as intention-to-treat. G2b, restricted to the probe-delivered variants, is a
   secondary. The gate R3 is ≥ 4/8.
2. **Product bug: the grey-zone file signal never fires through the hook.** The no-match fast path
   drops edit batches without checking for a waiting card:
   - `adage_runtime::fast::skip`, Batch arm, never checks `SessionState.card_waiting`;
   - `Triggers::of` does not add a card's files to the edit triggers.

   A prompt in the grey zone therefore never gets its card at the first edit, unless some other
   route happens to match that edit. Reproduced: `card_waiting` is set at the prompt, and the edit
   is skipped. A fix is proposed as a separate task. With the bug, the ITT effect is diluted by the
   misses.
3. **The click "one-off" slice contains a real family.** Conditioning on the 10 archived click fixes
   forms a card from 4 shell-completion fixes. It reaches the 2 held-out shell-completion fixes
   (`click-19fd4d6e`, `click-c3535905`) at their first edit. There the batch is not skipped, because
   the testmap's `src/click/*.py` route matches. G4c therefore counts only **false** fires: a
   delivery to a task whose real fix edits none of the card's files.
4. **A synthetic session must end with an assistant reply.** Otherwise it counts as cut off, its
   proxy outcome is a fail, and no card forms. Real sessions always end with one.
5. **Learner warning.** Every learn logs `not learning a route: route advise-… an advisory step
   follows an edit, a command or a failure`. It comes from testmap's test-command advice route and
   is reported with the bug.

## Files

| file | what |
| --- | --- |
| `family_ufo.py` | variants, prompts, hidden test, gold patches, negatives |
| `gv_sandbox.py`, `gv_score.py`, `validate.py`, `validation.json` | sandbox, scorer, validation (12/12) |
| `make_configs.py`, `configs/` | `ufo-util.json`, `click-regression.json` |
| `gv_run.py` | the runner, unchanged, plus: training as a user, learner wait, no-outcome learn, seeding, global cap, `--eval-repeats`, guards |
| `detect_check.py` | gate R2/R3 and the false-fire probe through the installed hook ($0) |
| `analyze.py` | pre-registered analysis; `--interim` (R4); `--synthetic` (operating characteristics) |
| `prereg.json` | question, design, metrics, criteria, stopping rules, predictions, budget, dry-run findings and revisions |
| `dryrun.py`, `dryrun.json`, `dryrun_gate.json` | the no-spend dry run and its results |
