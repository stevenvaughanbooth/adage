# Adage showcase: the shipped product on recurring work, five repositories

Prepared 2026-09-29. **No paid session has run.** This is the benchmark behind the website's numbers:
what the shipped product does on recurring developer tasks when it learns each one from a user's own
sessions. The pre-registration is [`prereg.json`](prereg.json); `analyze.py` and `render.py` read every
parameter from it. The protocol is the GO validation's ([`evals/launch/go-validation`](../launch/go-validation/README.md)),
and its driver is reused unchanged.

## Families (`families.py`, 12 variants each, validated 60/60)

| family | repository @ base | ecosystem | the task that repeats | source |
| --- | --- | --- | --- | --- |
| `click-type` | pallets/click @ 06b2a67 | Python (uv, pytest) | a built-in parameter type: `types.py`, the export, API reference, parameter-types guide, changelog, test; Click's light-imports test stays green | ported + 2 new variants |
| `mm-validator` | marshmallow @ 7f0792bd | Python (uv, pytest) | a validator with the house error/repr conventions, docstring, changelog, tests | ported + 2 new |
| `go-flag` | urfave/cli v3 @ 58ec406e | Go | a flag type + accessor: `flag_<x>.go`, tests, the flags doc, generated API docs and their approved copy | ported + 2 new |
| `http-header` | hyperium/http @ fdc6ab8 | Rust (cargo) | a standard header constant: a documented `standard_headers!` entry, the re-export, tests (as upstream #583) | **new** |
| `chi-middleware` | go-chi/chi v5 @ 3d1777a | Go | a middleware: `middleware/<name>.go` with a doc comment, its test, the README's middleware table and link | **new** |

- **Variant order** was fixed before any session. The first 4 variants are the training sessions (the
  user's history), and the other 8 are held out. The ported families keep the research's own training
  variants.
- **Hidden checks** run the project's CI (the whole suite) with a hidden test covering every example
  in the prompt. They also check the deliverables the prompt names: the docs, the changelog where the
  project keeps one, and a test written by the agent.
- **Validation** (`validate.py` -> `validation.json`): all 60 variants fail on the base and pass with
  the gold patch. The slowest check takes 28 s.
- **Negatives** (6 per family) are used by the false-fire probe:
  - 3 RepoBench one-off fixes in the same repository whose real fix edits the family's files;
  - 3 hard negatives in the family's vocabulary. The research's own were reused where they existed.

## Protocol (identical to the GO validation)

| | sessions | how |
| --- | ---: | --- |
| training (per family) | 4, one at a time | Haiku 4.5, the product installed, learning by itself (`adage condition --auto` after each session); the driver waits for the learner |
| held out (per family) | 8 variants x 2 repeats x {control, adage} = 32 | paired by (variant, repeat), arm order seeded; learning frozen; `control` = hooks on, directives withheld |
| Sonnet 5 arm | 2 families x 6 variants x 1 repeat x 2 = 24 | `mm-validator`, `chi-middleware`, on a copy of the Haiku-learned index (no Sonnet training) |

- **Runner flags:** `--host-auth`, `confine: true`, isolated HOME, 60-turn cap, concurrency 2.
  `sc_run.py` refuses to execute while `CLAUDECODE` or `CLAUDE_CODE_SESSION_ID` is set.
- **Primary:** $/task for Haiku 4.5, pooled over every held-out pair of the 5 families, intention to
  treat (prompts the detector missed count too). The estimate is the ratio of the pooled paired sums,
  with a 90% CI from a Rao-Wu bootstrap stratified by family and clustered by variant.
- **Secondary:** tool calls, wall time, pass rate (per arm and paired), card delivery rate, and false
  fires (the $0 probe's 30 negatives, at the prompt or at the first edit).
- **Per family and Sonnet:** the same metrics within each family and for the Sonnet arm.

## Power and budget

The power analysis (`analyze.py --power` -> `power.json`) is a plasmode simulation on the 36 real
card-vs-control pairs of the recurring-workflows research (`calibration.json`).

| design | held-out sessions | pooled 90% half-width (median) | P(half-width <= 6) | P(CI excludes 0), card effect -20% / -10% | per-family half-width |
| --- | ---: | ---: | ---: | --- | ---: |
| **5 x 8 x 2 (chosen)** | 160 | **4.9 points** | **0.94** | 0.995 / 0.78 | ~10 points |
| 5 x 8 x 1 | 80 | 6.7 | 0.24 | 0.97 / 0.56 | ~13 |
| 5 x 10 x 2 | 200 | 4.5 | 0.99 | 0.995 / 0.83 | ~9 |
| Sonnet arm, 2 x 6 x 1 | 24 | ~12 | | | |

The budget comes from `make_configs.py` -> `budget.json`. Per-session estimates are based on the
research's live sessions and on the data-expansion corpus.

| stage | sessions | central |
| --- | ---: | ---: |
| A. training, 5 families | 20 | $6.30 |
| B. held out, repeat 1 | 80 | $22.40 |
| C. held out, repeat 2 | 80 | $22.40 |
| D. Sonnet arm | 24 | $7.39 |
| **total** | **204** | **$58.49** |

- **Hard cap:** $60 across all 7 roots, enforced by the driver before every session. The pessimistic
  total is $76.
- **Order:** every family's repeat 1 runs before any repeat 2, and the Sonnet arm runs last. A cap
  that binds therefore costs repeats or the Sonnet arm, never a whole family.

**What the money buys.**

- **About $51 of Haiku sessions** gives a pooled interval of about ±5 points. A -15% ITT saving
  almost always excludes 0, and a -7% saving does so about 3 times in 4.
- **Per-family numbers are descriptive only** (about ±10 points each).
- **The Sonnet arm's $7 is directional** (about ±12 points).

## Stopping rules (details in `prereg.json`)

- **R1:** global cap of $60.
- **R2:** if a family's learner did not run, that is infrastructure: re-run its training. If the
  learner ran and formed no card, that is the product's behaviour and the family continues.
- **R3:** the $0 probe is reported, never a stop.
- **R4:** interim harm check after stage B. It stops if Adage has at least 5 fewer passes than
  control, or if $/task is above +15% with the CI lower bound above 0. On synthetic data it
  false-stops 1% of the time under the null and 5% at a true -20%.
- **R5:** infrastructure failures are resumed.
- **R6:** the Sonnet arm runs only if at least half its estimate is left.
- **R7:** nothing is dropped or re-run because of its outcome.

## Commands (from a terminal, not inside an agent session)

```bash
cd evals/showcase
BIN=/abs/path/to/release/adage   # a build of 7db8b22+ (with adage-exec beside it) whose dryrun.py result is recorded in prereg.json
R=/tmp/adage-showcase/runs
RUN="env -u CLAUDECODE -u CLAUDE_CODE_SESSION_ID python3 sc_run.py"
FAMS="click-type mm-validator go-flag http-header chi-middleware"
export RUSTUP_HOME=${RUSTUP_HOME:-/root/.rustup} GOFLAGS=-mod=mod

# 0. No spend: validation, configs, the product learning each family (synthetic history) + probe, analysis
python3 dryrun.py --adage $BIN                          # exit 0 = every check passed; findings in dryrun.json

# A. Training: the user's history (20 sessions, ~$6.3), then the $0 probe per family (card? delivery? false fires?)
for f in $FAMS; do
  $RUN configs/$f.json train --execute --host-auth --root $R/$f --adage $BIN
  python3 sc_detect.py --family $f --home $R/$f/private/adage-home --adage $BIN --out $R/$f/gate.json
done
#    R2: check $R/<f>/private/learner-waits.jsonl (no "timeout") before going on

# B. Held out, repeat 1, every family (80 sessions, ~$22), then the interim harm check R4
for f in $FAMS; do $RUN configs/$f.json eval --eval-repeats 1 --execute --host-auth --root $R/$f --adage $BIN; done
python3 analyze.py --interim                            # exit 4 = stop for harm: skip C and D, report repeat 1

# C. Held out, repeat 2 (80 sessions, ~$22): the same command without --eval-repeats resumes repeat 2 only
for f in $FAMS; do $RUN configs/$f.json eval report --execute --host-auth --root $R/$f --adage $BIN; done

# D. The Sonnet 5 arm, last (24 sessions, ~$7.4; refuses if less than half its estimate remains)
for f in mm-validator chi-middleware; do
  $RUN configs/$f-sonnet.json eval report --execute --host-auth --root $R/$f-sonnet --adage $BIN
done

# E. Pre-registered analysis and the publishable outputs ($0)
python3 analyze.py --out analysis.json
python3 render.py                                       # RESULTS.md + results.json (status final / incomplete)
```

- **Resume:** a run that is interrupted resumes when you re-run the same command.
- **Disk:** keep this directory's footprint under 1 GB.
  - Mirrors take 41 MB and the uv cache 20 MB.
  - The scorer trims its Go build cache above 250 MB and prunes each sandbox's `.venv` and `target/`.
  - Transcripts add about 1-2 MB per session.

## Dry run ($0)

It ran twice, both times before any session.

1. **`dryrun.json`:** the GO validation's release binary (`/tmp/adage-go-target/release/adage`,
   sha256 `1918e9b4…195e`).
2. **`dryrun-7db8b22.json`:** a release build of 7db8b22, which groups the files a task names after
   itself by role (sha256 `009a4ce7…3da11`). `dryrun/gate.<family>.json` holds this run.

**What passed both times:**

- **Validation:** 60/60 variants.
- **Configs:** all 7 plan through `sc_run.py` -> `gv_run.py` -> the runner. That gives 36 sessions
  per Haiku family and 12 per Sonnet config.
- **Isolation:** training sessions get only `ADAGE_HOME`. Held-out sessions get `ADAGE_ARM` set to
  control or treated, with learning frozen, and nothing forbidden is forwarded.
- **Sonnet configs:** they refuse training and refuse to start before a learned home exists. Home
  seeding works and runs only once.
- **Analysis:** every synthetic check passes.
  - Two-sided error is 0.075 under a true null, against 0.10 nominal.
  - Known-truth coverage of the 90% CI is 0.89 pooled and 0.875 per family.
  - `dryrun/RESULTS.synthetic.md` and `dryrun/results.synthetic.json` show the published format on
    synthetic numbers.

**What the product did with synthetic training history** (built from the gold patches):

| family | card formed (first binary / 7db8b22) | held out delivered (7db8b22) | negatives with a card (both) |
| --- | --- | --- | --- |
| click-type | yes / yes | 8/8 at the prompt | 0/6 |
| mm-validator | yes / yes | 8/8 at the prompt | **2/6**, both at the first edit of `validate.py` |
| go-flag | yes / yes | 8/8 at the prompt | 0/6 |
| http-header | yes / yes | 8/8 at the prompt | **1/6**, at the first edit of `name.rs` |
| chi-middleware | **no** / **yes** | 8/8 (7 at the prompt, 1 at the first edit) | 0/6 |

**Findings:**

1. **chi-middleware, first binary:** no card.
   - The learner joined traces only if the shapes of their edited files overlapped: directory,
     name-token count, first token and extension.
   - chi names each middleware's files after it (`no_sniff.go`, `hsts.go`), so the shapes did not
     overlap.
2. **chi-middleware, 7db8b22:** the fix groups the files a task created by role, and the card is
   `README.md`, `middleware/<x>.go`, `middleware/<x>_test.go` plus `go test -count=1 ./middleware/`.
   The family was not changed between the two runs.
3. **False fires, both binaries:** 3 of 30 negatives got the card, all at the first edit of a card
   file on same-file fixes or a doc edit, and none at the prompt. They count, and the split is
   reported.
4. **Binary for the live run:** a build of 7db8b22 or later whose dry run is recorded in `prereg.json`
   (`product.binary_rule`). The live run also needs `adage-exec`: build
   `-p adage-cli -p adage-exec -p adage-train`, re-run `dryrun.py`, and record the sha256 first. The
   7db8b22 target was deleted after the dry run.
5. **Prediction:** pooled ITT between -10% and -20%.

## Files

| file | what |
| --- | --- |
| `families.py`, `fam_python.py`, `fam_urfave.py`, `fam_http.py`, `fam_chi.py` | the families: variants, prompts, hidden checks, gold patches, negatives |
| `sc_sandbox.py`, `sc_score.py`, `validate.py`, `validation.json` | sandbox, scorer, validation (60/60) |
| `make_configs.py`, `configs/`, `budget.json` | the 7 runner configs and the budget |
| `sc_run.py` | the GO validation's `gv_run.py` (imported unchanged), plus seeding the Sonnet arm's home |
| `sc_detect.py` | the $0 probe through the installed hook (the GO validation's `Probe`): card, delivery, false fires |
| `analyze.py`, `calibration.json`, `power.json`, `synthetic.json` | the pre-registered analysis; `--interim` (R4), `--power`, `--synthetic` |
| `render.py` | `RESULTS.md` and `results.json` (schema `adage-showcase-results/1`) from `analysis.json` |
| `prereg.json` | question, families, design, metrics, analysis, power, budget, stopping rules, reporting rules, predictions, dry-run findings |
| `dryrun.py`, `dryrun.json`, `dryrun-7db8b22.json`, `dryrun/` | the no-spend dry runs (first binary, 7db8b22 build), the latest probe results per family and the synthetic rendering |
