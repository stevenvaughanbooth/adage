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
python3 evals/live/runner.py evals/live/configs/acme-billing.json --json       # the plan, as JSON
python3 evals/live/runner.py evals/live/configs/acme-billing.json eval --round 2 --model claude-sonnet-5

# For real. Needs `claude` on PATH and a release build of adage.
cargo build --release -p adage-cli
python3 evals/live/runner.py evals/live/configs/acme-billing.json --execute --root /tmp/adage-live/acme
```

## Phases

| Phase | What happens |
| --- | --- |
| `train` | Round 1 only. Baseline agents work the training tasks with no Adage hooks installed |
| `learn` | `adage learn` reads the training transcripts. In later rounds it reads every earlier session, with each route kind excluded from the sessions it steered |
| `eval` | Agents work the held-out tasks, one session per arm, with the arms interleaved: `adage` (hooks installed, directives delivered), `holdout` (hooks installed, directives logged but not delivered: the control) and optionally `off` (no hooks: measures hook overhead) |
| `report` | `summary.json`: per-arm medians and means (turns and seconds to first edit, total turns, cost, pass rate, stale generated code), with permutation p-values between `adage` and `holdout` |

The phases default to all four. Name some to run only those: for example `eval report --round 2`.

## Config

The config is JSON. `configs/acme-billing.json` is the demo.

| Key | Meaning |
| --- | --- |
| `name`, `repo_key` | The run's name, and the repository key tracks are learned under (`adage learn --repo`) |
| `workdir` | The sandbox directory name inside each run directory |
| `sandbox` | argv that creates a fresh sandbox at `{dir}` for task `{task}` (`{repo}` = this repository) |
| `score` | argv that scores a finished sandbox and prints JSON with `passed` (and optionally `stale_generated`) |
| `prompts` | Prompt templates, with `{id}` replaced by the task. Tasks rotate through them |
| `train`, `eval` | `{"tasks": [...], "repeats": n}`. `eval.arms` defaults to `["holdout", "adage"]` |
| `model`, `max_turns`, `concurrency` | Passed to `claude -p`, and the number of sessions run in parallel |
| `learn_args` | Extra `adage learn` arguments (`["--demo"]` for small runs) |
| `allowed_tools`, `disallowed_tools` | `claude --allowedTools/--disallowedTools` |
| `budget_usd` | Spend cap. No new session starts once the reported cost reaches it |

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
```

So any live run becomes a replayable offline dataset, and `adage eval` can then measure it
without spending anything more:

```bash
python3 evals/datasets/tools/build_demo.py /tmp/adage-live/acme evals/datasets/my-run
adage eval evals/datasets/my-run --split round1
```

## Tests

```bash
python3 -m unittest evals/live/test_runner.py
```

This smoke-tests the dry run. It runs no agent and spends nothing. It checks:

- the demo plan: 54 sessions, the right arms and environment, and no hooks in training;
- that a dry run writes nothing;
- the overrides and later rounds;
- that `--execute` is refused inside an agent session;
- that a bad config is refused.
