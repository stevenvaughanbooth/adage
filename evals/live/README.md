# Live proving ground

`runner.py` runs real headless Claude Code agents on a sandbox repository, with and without
Adage. It generalises `demo/run.py`: the sandbox, tasks, prompts, arms, model and budget all come
from a config file. It covers the with/without comparison of Design §12 (the proving ground) and
§14 (measurement). For what the numbers mean next to the offline replay eval, see
[docs/evals.md](../../docs/evals.md).

**The runner never runs anything unless you pass `--execute`.** Without it, you get a dry run:
the full plan of sessions, arms, prompts and command lines, with nothing executed and nothing
written. Executing spends real tokens: one `claude -p` session per planned run.

```bash
python3 evals/live/runner.py evals/live/configs/acme-billing.json              # the plan
python3 evals/live/runner.py --config evals/live/configs/harbor-dispatch.json --dry-run
python3 evals/live/runner.py evals/live/configs/acme-billing.json --json       # the plan, as JSON
python3 evals/live/runner.py evals/live/configs/acme-billing.json eval --round 2 --model claude-sonnet-5

# For real. Needs `claude` on PATH and a release build of adage.
cargo build --release -p adage-cli
python3 evals/live/runner.py evals/live/configs/acme-billing.json --execute --root /tmp/adage-live/acme
```

The dry run prints the number of agent runs as the product that makes it, e.g. `train 6 tasks x
2 repeats = 12; eval 6 tasks x 3 repeats x 2 arms (holdout, adage) x 2 rounds = 72; total 84
agent runs`.

## Phases and rounds

| Phase | What happens |
| --- | --- |
| `train` | Round 1 only. Baseline agents work the training tasks with no Adage hooks installed |
| `learn` | `adage learn` reads the training transcripts. In later rounds it reads every earlier session, with each route kind excluded from the sessions it steered |
| `eval` | Agents work the held-out tasks, one session per arm, with the arms interleaved: `adage` (hooks installed, directives delivered), `holdout` (hooks installed, directives logged but not delivered: the control) and optionally `off` (no hooks: measures hook overhead) |
| `report` | `summary.json` and a results page (below): per-arm medians and means, with permutation p-values between `adage` and `holdout` |

The phases default to all four. Name some to run only those: for example `eval report --round 2`.
A config's `rounds` (default 1) run in order in one invocation: round 1 is train, learn, eval,
report; each later round learns from every earlier session and runs the held-out tasks again
(`eval2-…`). `--round N` runs only round N. The spend cap counts every run already in the root's
`state.json`, so it holds across invocations.

## Metrics and the results page

Every run records a timeline of its tool calls (each classified as explore, lookup, edit, verify,
dead end, regenerate), the directives it received, what Adage would have said and judged (from the
session's spool), and the scorer's verdict. `report` summarises the held-out runs per arm and round:

| Metric | Per run | Aggregate |
| --- | --- | --- |
| `turns_to_first_edit`, `time_to_first_edit_s` | Model turns (seconds) before the first edit | median |
| `total_turns`, `explore_calls_before_edit` | | median |
| `cost_usd` | The session's reported cost | mean |
| `pass_rate`, `hidden_pass_rate` | The scorer's `passed`, and its hidden test alone | mean |
| `dead_end_hits` | Failed verify commands | mean |
| `obligation_met_rate` | Of runs that edited the obligation's paths: regenerated after the last such edit, and generated code is fresh | mean |
| `stale_generated_rate` | The scorer found stale generated code | mean |
| `follow_rate` | Routes the session followed ÷ routes it was exposed to (would-have directives; both arms) | mean |

It then writes `<root>/results/`: `index.html` (the demo dashboard with the final state embedded,
as in `demo/results/`), `artifact.html`, `summary.json` and `state.json`. While a run is going,
`DEMO_ROOT=<root> python3 demo/dashboard/serve.py` shows it live.

## Config

The config is JSON. `configs/acme-billing.json` is the demo.

| Key | Meaning |
| --- | --- |
| `name`, `repo_key` | The run's name, and the repository key tracks are learned under (`adage learn --repo`) |
| `workdir` | The sandbox directory name inside each run directory |
| `sandbox` | argv that creates a fresh sandbox at `{dir}` for task `{task}` (`{repo}` = this repository) |
| `score` | argv that scores a finished sandbox and prints JSON with `passed` (and optionally `stale_generated`, `ci_passed`, `hidden_passed`) |
| `prompts` | Prompt templates, with `{id}` replaced by the task. Tasks rotate through them |
| `train`, `eval` | `{"tasks": [...], "repeats": n}`. `eval.arms` defaults to `["holdout", "adage"]` |
| `rounds` | Learning rounds run by one invocation (default 1) |
| `model`, `max_turns`, `concurrency` | Passed to `claude -p`, and the number of sessions run in parallel |
| `learn_args` | Extra `adage learn` arguments (`["--demo"]` for small runs) |
| `allowed_tools`, `disallowed_tools` | `claude --allowedTools/--disallowedTools` |
| `budget_usd` | Spend cap. No new session starts once the reported cost reaches it |
| `classify` | Substrings of a Bash command that make it a `lookup`, `regen`, `verify` or `bash_edit` call; `check_flags` turn a regen command into a verify (`--verify`). Defaults describe acme-billing |
| `obligation.paths` | The glob whose edits the finish-line obligation covers (acme: `schema/*.json`) |
| `labels` | Legend text for the results page (`lookup`, `regen`) |

## Demo 2: harbor-dispatch

The first demo's caveat was one synthetic repository and one model. `configs/harbor-dispatch.json`
tests whether the effect generalises: a different stack, domain, ID shape, and four conventions of
the same kinds with different mechanics, run with Claude Sonnet 5.

**The sandbox.** `sandboxes/harbor-dispatch/generate.py <dir>` writes a TypeScript service for
container-terminal operations (berth planning, cargo manifests, dangerous-goods stowage, arrival
estimates, a port and vessel registry). It runs directly on Node 26 with native type stripping: no
build step, no dependencies, nothing fetched. The output is a one-commit git repository; the same
seed (`--seed`, default 20260923) gives the same bytes and commit, and the seed only varies the
decoy incidents in the queue. The README says only that "incidents are tracked in the ops queue".

| Convention | In harbor-dispatch | Why an agent cannot guess it |
| --- | --- | --- |
| Prompt → lookup | Prompts name only an incident (`Could you fix INC-94?`). `./bin/fetch-incident INC-94` prints it; `--json` too | The queue is `.ops/incidents.pack`, a deflated binary with no readable IDs, so grepping finds nothing. Nothing points to `bin/`. IDs are 2–3 digits (`INC-86`, `INC-203`), so the learned contract must generalise across lengths (`^INC-\d{2,3}$`) |
| Linked files | The incident's `Files:` lists the one or two files to change | Only the lookup knows them; registry incidents list the CSV and the module that reads it, never the generated file |
| Dead end | `npm test` (plain `node --test`) fails every time with `Error [ERR_MODULE_NOT_FOUND]: Cannot find package '@harbor/dispatch'`. `npm run check` works | Sources import `@harbor/dispatch/*`, a tsconfig path alias that Node resolves only through the resolve hook `npm run check` preloads (`--import ./tools/aliases.mjs`). `npm test` is the reflex for a Node repository, and its error names no fix |
| Finish line | Editing `registry/*.csv` requires `npm run codegen` (regenerates `src/generated/registry.ts`). `npm run check` fails with `… is out of date with registry/*.csv` otherwise | Registry lookups read the generated module, not the CSV, and the generated file carries no marker. The hidden test also reads through the generated module, so a stale registry fails it |

**The incidents.** Twelve, each with a hidden test (`sandboxes/harbor-dispatch/hidden/`) that fails
untouched and passes with a reference fix (`solutions.py`). Bugs sit where a real one would, and no
comment names a fix.

| Split | ID | Kind | Problem (file) |
| --- | --- | --- | --- |
| train | INC-58 | code | 45-foot boxes counted as 2 TEU, not 2.25 (`src/cargo/manifest.ts`) |
| train | INC-63 | code | Port codes with a space or hyphen are not recognised (`src/ports/locode.ts`) |
| train | INC-107 | code | Berth fit ignores the 0.5 m under-keel clearance (`src/berths/fit.ts`) |
| train | INC-112 | code | Transit time converts nautical miles to km before dividing by knots (`src/eta/transit.ts`) |
| train | INC-164 | registry | Add the port PLGDN, Gdansk (`registry/ports.csv`) |
| train | INC-171 | registry | Add the Feedermax vessel class (`registry/vessels.csv`) |
| held-out | INC-86 | code | Segregation misses hazard divisions such as 5.1 (`src/cargo/hazmat.ts`) |
| held-out | INC-94 | code | Back-to-back berth bookings are treated as overlapping (`src/berths/schedule.ts`) |
| held-out | INC-203 | code | Verified gross mass leaves out the tare (`src/cargo/manifest.ts`) |
| held-out | INC-219 | code | Arrival stamps in UTC, not the port's time zone (`src/eta/local.ts`) |
| held-out | INC-245 | registry | Rotterdam's maximum draft is now 24 m (`registry/ports.csv`) |
| held-out | INC-268 | registry | Add the Neopanamax vessel class (`registry/vessels.csv`) |

**The scorer.** `score.py <dir> <INC-id>` copies the hidden test into `test/`, runs the repository's
CI (`npm run check`), then runs the hidden test again explicitly with its own resolver hook and the
TAP reporter, and removes it. `passed` needs all of: CI green, the hidden suite visible in the CI
output (`ci_ran_hidden`: a CI that skipped it does not count), the explicit run executing at least
as many tests as the file declares with none failing (`hidden_passed`), and generated code fresh
by the template's own generator, not the sandbox's (`stale_generated`).

**Checks that spend nothing.** Run both before a live run:

```bash
python3 evals/live/sandboxes/harbor-dispatch/selfcheck.py        # ~20 s; prints ALL OK
python3 evals/live/sandboxes/harbor-dispatch/rehearse.py /tmp/harbor-rehearsal   # ~40 s; needs target/release/adage
```

`selfcheck.py` proves determinism, the dead end, the lookup for every ID (and that its plain-text
output contains no path-like text besides the listed files, which `adage learn` would take for
linked files), that every incident fails untouched and passes solved, the obligation, the scorer's
skip guard, and that all fixes together pass. `rehearse.py` runs scripted, model-free sessions with
the real commands in real sandboxes, writes them as Claude Code transcripts in the runner's layout,
and checks that `adage learn` learns all four conventions from them, that the runner's metrics and
results page work, and that `build_demo.py` plus `adage eval` ingest the root.

**The run.** 84 agent runs: 6 training incidents x 2 repeats (no hooks), then in each of 2 rounds 6
held-out incidents x 3 repeats x 2 arms (holdout, adage). Round 2 learns from all 48 earlier
sessions. The spend cap is $45.

```bash
cargo build --release -p adage-cli
python3 evals/live/runner.py --config evals/live/configs/harbor-dispatch.json --dry-run
python3 evals/live/runner.py --config evals/live/configs/harbor-dispatch.json --execute --root /tmp/adage-live/harbor
# afterwards: results page and an offline dataset
open /tmp/adage-live/harbor/results/index.html
python3 evals/live/sandboxes/harbor-dispatch/generate.py /tmp/harbor-files     # the file list for sandbox.txt
python3 evals/datasets/tools/build_demo.py /tmp/adage-live/harbor evals/datasets/harbor-sonnet5 \
    --name harbor-sonnet5 --repo harbor-dispatch --sandbox /tmp/harbor-files \
    --description "Real Claude Code sessions (Claude Sonnet 5) on the harbor-dispatch sandbox"
target/release/adage eval evals/datasets/harbor-sonnet5 --split round2
```

What could blunt the effect, so it is visible in the numbers rather than a surprise: agents that
read `package.json` first may run `npm run check` without ever trying `npm test`, which leaves the
dead end with little training evidence (round 2 also learns from round 1's eval sessions); and an
agent may try `npx` for a TypeScript tool, which the sandbox cannot stop from using the network.

## Safety

- The runner refuses `--execute` inside an agent session (when `CLAUDECODE` is set). An agent
  cannot start a proving run by accident.
- Each session runs in a fresh sandbox under the output root, with the harness's own
  permissions and `--setting-sources project,local`. Your user settings and MCP servers are not
  loaded.
- The spend cap is checked before every session.

## Output, and turning a run into an offline dataset

The output root mirrors a demo archive:

```text
<root>/runs/<run>/<workdir>/              the sandbox after the agent finished
<root>/runs/<run>/stream.jsonl            the `claude -p` stream, time-stamped
<root>/transcripts/<project>/<session>.jsonl   the Claude Code transcript (what `adage learn` reads)
<root>/adage-home/                        tracks, index, and per-session spools and state
<root>/state.json, outcomes.json, summary.json
<root>/results/                           the results page (index.html, artifact.html, summary.json, state.json)
```

So any live run becomes a replayable offline dataset, and `adage eval` can then measure it
without spending anything more. Name the dataset, and give a fresh sandbox for `sandbox.txt`:

```bash
python3 evals/datasets/tools/build_demo.py /tmp/adage-live/acme evals/datasets/my-run \
    --name my-run --repo acme-billing --sandbox <a fresh sandbox dir>
adage eval evals/datasets/my-run --split round1
```

The dataset gets one split per round (`round1`, `round2`, …): train on the training sessions and
every earlier round's eval sessions, test on that round's holdout arm.

## Tests

```bash
python3 -m unittest evals/live/test_runner.py
```

This smoke-tests the dry run. It runs no agent and spends nothing. It checks:

- the demo plan: 54 sessions, the right arms and environment, and no hooks in training;
- the second demo's plan: 84 runs over two rounds, Sonnet 5, and its estimate line;
- `--config` and `--dry-run`, and that `--dry-run --execute` is refused;
- that a dry run writes nothing;
- the overrides and later rounds;
- the second sandbox's command classification and the per-run metrics (dead ends, obligation, follow rate);
- that `--execute` is refused inside an agent session;
- that a bad config is refused.
