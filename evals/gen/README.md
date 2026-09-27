# Procedural sandbox generator

`evals/gen/` generates sandbox repositories for Adage's evals from a typed **convention grammar**.
The two hand-built sandboxes, [acme-billing](../../demo/SANDBOX.md) and
[harbor-dispatch](../live/README.md#demo-2-harbor-dispatch), each test one fixed set of conventions
in one stack and one domain. This generator produces as many as you want, varying the stack, the
domain vocabulary (the "skin"), the conventions, the tasks and every concrete detail (IDs, which
functions carry bugs, the order of the tracker) by seed. Each sandbox comes with ground truth
(`conventions.json`), tasks with hidden tests, a scorer, and a config the
[live runner](../live/runner.py) can run as it is.

Python 3 standard library only. Generation needs `git`; the python-make stack needs `make`; the
node-npm stack needs Node >= 20.6 and npm (only to run or check a sandbox, not to generate one).

```bash
python3 evals/gen/gen.py --spec evals/gen/specs/example.json --seed 7 --out /tmp/gen/one   # one bundle
python3 evals/gen/gen.py --suite smoke --out /tmp/gen/smoke                                # 6 bundles
python3 evals/gen/selfcheck.py /tmp/gen/smoke            # ~1 min; prints ALL OK
python3 evals/gen/rehearse.py /tmp/gen/smoke             # ~1.5 min; needs a built adage (cargo build -p adage-cli)
python3 -m unittest discover evals/gen                   # the tests (~2.5 min)
```

## The grammar

A sandbox is a set of conventions, each of one **kind**. `conventions.json` lists them, one entry
each, as ground truth:

| Kind | Entry fields | What the sandbox contains |
| --- | --- | --- |
| `lookup` | `command` (with `{item}`), `id_pattern` | Prompts name only an item ID (`BIL-1041`, `OPS-86`, `CARE-203`). The item lives in an opaque store (`.tracker/cache.bin`: magic bytes + zlib, no readable IDs) that only a repository tool reads (`./scripts/wi-lookup`, `./bin/fetch-ticket`, `./tools/case`). The README says only "the internal tracker" |
| `linked_files` | `command` (the lookup) | The lookup output lists the files to change under `Files:` (`files` with `--json`); only the lookup knows them. Its plain-text output contains no other path-like text (Adage reads paths from it) |
| `dead_end` | `command` (works), `avoided` (fails), `signature`, `mode` | The obvious runner fails at once with a distinctive message; the working command is declared only in `.ci/pipeline.yml`. Modes below |
| `instinct_violation` | `command`, `avoided`, `signature` | The universal default is wrong here: `make test` / `npm test` is retired and prints a failure, while `make check` / `npm run check` works |
| `finish_line` | `command` (regenerate), `glob`, `signature` | Lookups read a generated module built from `registry/*.csv`. Editing a CSV requires `make gen` / `npm run codegen`; CI (`make check` / `npm run check`) and the hidden checks fail on stale output |
| `decoy` | `command`, `mode` | A plausible command that is **not** a convention, to measure false routes: `unused_target` (`make bench`), `lookalike_regen` (`make proto`, `npm run gen:docs`: looks like a regenerate step, generates nothing), `lookalike_verify` (a lint that always passes), `legacy_script` (a leftover no-op script). Decoys exit 0 and change nothing |
| `scope`, `env_sync` | | Reserved in the schema; Adage does not mine them yet, and the generator does not produce them |

Every entry also has `id` and `notes`. Dead-end modes, per stack:

| Mode | python-make | node-npm |
| --- | --- | --- |
| `env_guard` | `tests/__init__.py` aborts unless `<SKIN>_ENV=test` (set only by `make check`). `python3 -m unittest` fails; `<SKIN>_ENV=test python3 -m unittest` would work | Test files import `tools/testing/env.mjs`, which throws unless `<SKIN>_ENV=test`. `node --test` fails |
| `wrong_runner` | `tests/__init__.py` aborts unless `tools/testkit.py` loaded the fixtures. Every generic runner fails | Test files import `tools/testing/fixtures.mjs`, which throws unless `npm run check` preloaded the fixtures (`--import ./tools/testing/setup.mjs`) |
| `path_alias` | (not applicable) | Sources and tests import `@<scope>/<pkg>/*`, an alias in `jsconfig.json` that Node resolves only through the hook `npm run check` preloads. `node --test` fails with `ERR_MODULE_NOT_FOUND` |

### Tasks

`tasks.json` lists every task: `id`, `kind`, `split` (`train` / `eval`, alternating within each
kind), `prompts` (variants), `exercises` (the conventions the task needs), `files`, `hidden`,
`hidden_tests` and a `summary`. `session_conventions` lists the conventions any session that runs
the tests meets (the dead end and the instinct violation).

| Kind | What it is | Exercises |
| --- | --- | --- |
| `bugfix` | A tracker item; its bug is a mutated function (bugs.py) in the file the lookup lists | `lookup`, `linked-files` |
| `registry` | A tracker item: add or change a row of `registry/*.csv`. The lookup lists the CSV and the module that reads it, never the generated file | `lookup`, `linked-files`, `finish-line` |
| `control` | A self-contained prompt that names the file, the function and the expected behaviour. No ID, no lookup, no generated code | nothing |

At least 25% of the tasks must be control tasks. Every task has a hidden test outside the
repository (`hidden/`), fails untouched and passes with its reference solution (`solutions.json`:
exact edits, then commands); `selfcheck.py` proves it.

The bug catalog (`bugs.py`) has twelve templates: rounding that truncates, an inclusive boundary,
a case-sensitive code lookup, a missing upper clamp, a sum that skips the last entry, a mean of an
empty list, a blank name accepted, an ascending sort that should descend, a double unit conversion,
a dedupe that reorders, a range that drops its end, and a negative percentage accepted. Every
sandbox contains all twelve functions (four per module); the seed picks which carry bugs, where
they live, and which become tracker tasks or control tasks.

## The spec

```json
{
  "name": "py-billing",
  "stack": "python-make",
  "skin": "billing",
  "conventions": {
    "lookup": true,
    "linked_files": true,
    "dead_end": "env_guard",
    "finish_line": true,
    "instinct_violation": true,
    "decoys": ["unused_target", "lookalike_regen"]
  },
  "tasks": {"bugfix": 4, "registry": 2, "control": 2},
  "id_digits": [3, 4]
}
```

| Key | Default | Meaning |
| --- | --- | --- |
| `stack` | (required) | `python-make` or `node-npm` |
| `skin` | (required) | `billing` (tallyhouse-billing, `BIL-`), `logistics` (portside-dispatch, `OPS-`), `clinic` (wardline-clinic, `CARE-`) |
| `conventions.lookup`, `.linked_files`, `.finish_line`, `.instinct_violation` | `true` | On or off. `linked_files` needs `lookup` |
| `conventions.dead_end` | `"auto"` | A mode, `"auto"` (the seed picks one the stack supports), or `null` for none |
| `conventions.decoys` | `["unused_target", "lookalike_regen"]` | A list of modes, or a count |
| `tasks.bugfix`, `.registry`, `.control` | 4, 2, 2 | `bugfix + control` <= 12; `registry` <= 6 and needs `finish_line`; tracker tasks need `lookup`; `control` >= 25% of all |
| `id_digits` | the skin's | `[min, max]` digits of item numbers |
| `id_shape` | `prefix_dash` | Tracker ids: `prefix_dash` (`BIL-1041`), `hash_num` (`#482` in prompts, `482` to the tool), `uuid`, `path_num` (`bil/412`), `branch` (`fix/<word>-<n>`), `mixed` (`BIL-1041` and `#482`, one tool). Left out of the normalised spec when default, so existing bundles keep their bytes. `specs/idshapes.json` covers every shape; `rehearse.py` also writes `holdout.json` (200 fresh ids, 200 near misses) for `adage eval types` |
| `name` | `<stack>-<skin>` | The bundle's name |

A spec is validated before anything is written; a bad one exits 2 with the reason. A suite
(`specs/<name>.json`) is a list of `{"spec": ..., "seed": N}`; `specs/smoke.json` covers both stacks,
all three skins, every dead-end mode and every decoy mode in six sandboxes.

## What a bundle contains

```text
<out>/
  repo/                 the sandbox: a git repository with one commit (fixed author and date)
  hidden/               one hidden test per task
  pristine/             the scorer's own generator and test setup (the agent cannot edit them)
  conventions.json      ground truth
  tasks.json            tasks
  solutions.json        reference solutions
  scoring.json          how score.py runs CI, the hidden test and the staleness check
  spec.json             the normalised spec, the seed and the commit
  config.json           live-runner config: tracker tasks (train/eval splits)
  config-control.json   live-runner config: control tasks
  score.py              score.py <sandbox> <task>
```

Generation is a pure function of (spec, seed): the same inputs give the same bytes and the same
commit, wherever the output goes (tested). A different seed changes the IDs, which functions carry
bugs and where, the registry tasks, and the tracker's decoy items.

**The scorer** copies the task's hidden test into the sandbox, runs the repository's CI command,
runs the hidden test again explicitly with the scorer's own setup (which satisfies every guard, so
the result depends on no file the agent could change), removes the test, and checks generated code
with the pristine generator. `passed` needs CI green, the hidden test seen in the CI output, the
explicit run passing at least as many tests as the file declares, and fresh generated code. Output
matches the existing scorers' JSON (`passed`, `ci_passed`, `hidden_passed`, `stale_generated`, ...).

## With the live runner

`config.json` and `config-control.json` are ordinary [runner](../live/README.md) configs. Their
`sandbox` command regenerates the repository from the spec embedded in the config
(`gen.py --spec-json ... --seed N --repo-only {dir} --task {task}`), and `score` regenerates the
bundle in a temporary directory and scores (`evals/gen/score.py --spec-json ... --seed N {dir}
{task}`), so a config works from any checkout and does not depend on where the bundle was written.
The inline spec's braces are doubled because the runner `str.format`s every argument.

The runner fills one set of prompt templates with the task ID, so control tasks (whose prompts
describe the change) get their own config: the sandbox command writes the task's brief to an
untracked `BRIEF.md`, and the prompts say to read it. `tasks.json` has the direct prompt variants
for harnesses that take a per-task prompt.

```bash
python3 evals/live/runner.py /tmp/gen/smoke/py-billing-s11/config.json            # the plan (dry run)
python3 evals/live/runner.py /tmp/gen/smoke/py-billing-s11/config.json --execute --root /tmp/adage-live/gen-py
```

## Self-check and rehearsal

`selfcheck.py DIR` (a bundle or a suite) generalises the hand-built self-checks: determinism; a clean
one-commit repository whose CI passes; the lookup for every tracker task (any case, text and
`--json`, exactly its files, no other path-like text, an opaque store, unknown IDs rejected); prompts
that name only the ID (tracker tasks) or nothing ID-like (control tasks); every task fails untouched
and passes with its reference solution; every dead end and instinct violation fails with its
declared signature while the working command passes; every finish line's edit without the
regenerate step fails CI with its signature, is scored stale and fails the hidden check; every decoy
exits 0, changes nothing, and does not stand in for the regenerate step; and all solutions together
pass every task. Node bundles are skipped (not failed) without Node.

`rehearse.py DIR` runs scripted, model-free sessions with the real commands in real copies of each
sandbox (every task twice, 16 sessions per smoke bundle), writes them as Claude Code transcripts,
runs `adage learn --demo` on them with an isolated `ADAGE_HOME`, and compares the learned routes
(read from the track files, so drafts count) with `conventions.json`. The scripted agent explores,
runs decoys (while exploring, and after some code edits), looks items up and reads the listed files,
applies the reference fix, regenerates after registry edits (sometimes only after CI reports stale
output), tries the obvious runner and the universal default before the working command in two
sessions of three, and always finishes with the working command.

On the smoke suite it recovers 28 of 28 conventions and learns nothing from the 13 decoys:

| Bundle | Recovered | Notes |
| --- | --- | --- |
| py-billing-s11 (env_guard) | 5/5 | |
| py-clinic-s12 (wrong_runner) | 5/5 | lookup learned as a **draft** (below) |
| py-logistics-s13 (env_guard, no instinct violation) | 4/4 | |
| node-logistics-s21 (path_alias) | 5/5 | dead end learned as `avoid node` (below) |
| node-billing-s22 (env_guard) | 5/5 | same |
| node-clinic-s23 (wrong_runner, no instinct violation) | 4/4 | lookup a draft; `avoid node` |

What the rehearsal shows about Adage's learner, not about the generator:

- **A lookup tool without a read verb in its name is learned as a draft.** `./tools/case CARE-203`
  is classified as an environment-effect command (only names such as `wi-lookup` or `fetch-ticket`
  are classified observe by name), so `lookup-work-item` waits in `adage review` and is not in the
  index until approved. The clinic skin keeps this name on purpose, as a harder variant.
- **`node --test` is generalised to `node`.** The dead-end miner names a runner without its flags,
  so the directive says running `node` fails, which is too broad (`node tools/codegen.mjs` works).
- **Signatures are the last error-like line, which may not be the distinctive one.** For
  `node --test` on Node 20/22 (TAP output) it is `error: 'test failed'`; for a failing Makefile
  target it is make's own `make: *** [Makefile:11: test] Error 1`. The declared signature (the
  guard's own message) always appears in the output; `rehearse.py` reports both.

## Extending

- **A stack** (rust-cargo, go): a module in `stacks/` with a `Stack` subclass (the contract is at
  the top of `stacks/base.py`: naming, commands, signatures, the files, hidden tests, pristine
  files, scoring), `<lang>_good` / `<lang>_bad` / `<lang>_test` variants for each template in
  `bugs.py`, and an entry in `stacks/__init__.py`. The tracker, tasks, ground truth, solutions and
  configs come from the base class.
- **A skin**: an entry in `skins.py` (vocabulary for all twelve templates, two registry tables and
  six registry tasks). Keep its texts free of path-like tokens (no decimals, no "e.g.").
- **A convention kind or mode**: the kind in `grammar.py`, its files in each stack, its entry in
  `Stack.conventions()`, its proof in `selfcheck.py`, its scripted behaviour in `rehearse.py`, and
  its match in `rehearse.compare()`.

## Files

| File | Role |
| --- | --- |
| `gen.py` | CLI: bundles, suites, `--repo-only` for the runner |
| `grammar.py` | Kinds, spec validation and defaults, resolution of (spec, seed) into a plan |
| `skins.py` | Domain vocabularies |
| `bugs.py` | The bug templates, per language |
| `stacks/` | `base.py` (shared), `python_make.py`, `node_npm.py` |
| `score.py` | The scorer (copied into each bundle) |
| `selfcheck.py` | Proves every bundle is sound |
| `rehearse.py` | Scripted transcripts, `adage learn`, recovered vs expected |
| `specs/` | `smoke.json` (the suite), `example.json` |
| `test_gen.py` | The tests (`python3 -m unittest discover evals/gen`) |
