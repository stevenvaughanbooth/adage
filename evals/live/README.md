# Live evals

`runner.py` runs real headless Claude Code agents on a sandbox repository, with and without
Adage. It generalises `demo/run.py`: the sandbox, tasks, prompts, arms, model and budget all come
from a config file. It covers the with/without comparison of Design §12 (the eval loop) and
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
2 repeats = 12; eval 6 tasks x 3 repeats x 2 arms (control, adage) x 2 rounds = 72; total 84
agent runs`.

## Frozen studies

Use `frozen.py` for a prospective savings study after training, candidate review and compilation
have finished. It reuses the execution and budget machinery below, with a fixed paired cohort
and the stricter [experiment contract](../../docs/learning-experiments.md). The exploratory
rounds and summaries in `runner.py` remain useful for development; they are not this contract.

This collector requires Linux, Bubblewrap, a pinned Claude Code version and a current Adage
build. Supply an existing live config with `confine: true`, the standard hidden directories
(no `confine_hide`/`confine_expose` overrides), a resolved model revision, held-out evaluation task
ids and enough total budget to reserve both arms for every task. The collector fixes one round,
one attempt per arm/task, sequential execution and no learning. Each job gets a fresh checkout
and private home; the baseline disables reflexes and the intervention uses the exact frozen
index. Project/local settings are disabled in favor of the explicit runner settings.

```sh
# Build this revision before preparing: the executable digest is part of the protocol.
cargo build --release --locked -p adage-cli
target/release/adage corpus inspect --repo acme/api --evidence > training-evidence.json

# Preparation runs sandbox generators and version probes, but calls no model.
python3 evals/live/frozen.py prepare config.json \
  --training-audit training-evidence.json --reflex-set frozen-index.json \
  --learner-revision 'actual-training-build-and-parameters' \
  --input /absolute/path/to/grade.py --input /absolute/path/to/task-data.json \
  --objects study/objects --out study/frozen.json

# Read-only plan: no run directory is created.
python3 evals/live/frozen.py run study/frozen.json \
  --objects study/objects --root study/run

# Paid execution requires configured authentication and an explicit --execute.
python3 evals/live/frozen.py run study/frozen.json \
  --objects study/objects --root study/run --execute

# Recompute qualification offline from all immutable receipts.
python3 evals/live/frozen.py report study/frozen.json \
  --objects study/objects --root study/run --require-token-target
```

Use the same `--adage /absolute/path/to/adage` on every command when overriding the default
`target/release/adage`. `--learner-revision` must describe the build and parameters that actually
produced the reflex set; the collector cannot infer that provenance from its own build. The audit
must describe that training snapshot. `--identities identities.json` supplies canonical identities
keyed by execution task id; each `task_id` must equal that key. Related tasks belong in the same
group across training and evaluation. A frozen cohort has one representative per independent
group. `--policy policy.json` and `--timeout-seconds 600` set predeclared thresholds and budgets;
`--intervention-model` prepares a fixed model-transfer comparison.

Declare the complete grader/data dependency closure with repeated `--input` arguments, including
local imports, test data and environment lockfiles. Keep these private files under the standard
hidden roots (`/workspace`, `/tmp`, `/var/tmp`, `/home`, `/root` or `/mnt`) and outside mounted tool
or authentication directories. The collector pins their contents, its own source, executable
file arguments, initial checkout files and executable bits, effective prompts, working directory,
harness version and Adage binary. It rejects checkout symlinks. Unlisted external dependencies
are not made hermetic by these hashes; pin the installed environment separately. Inputs are
checked before dispatch and after collection. Do not change them while the study is running.
The run root and object directory must also stay under these hidden roots, outside mounted
tool/authentication directories, so agents cannot read the other jobs' evidence.

The workflow deadline covers preparation, agent execution and grading. The harness enforces
`--max-turns` and `--max-budget-usd`; receipt validation also checks actual request counts and
known billed costs against the frozen budget. A turn cap is not an independent provider request
interceptor. The shared ledger reserves each session cap and blocks further dispatch when spend
is unknown. Its admission accounting uses the CLI's cost estimate; actual billed cost remains
unknown until an invoice/provider integration supplies it. A token claim can qualify separately
from a cost claim. Unsupported helper/subagent or fallback-model accounting cannot silently
qualify as complete usage.

Completed, failed and timed-out receipts are immutable. Running the same command again skips
terminal jobs, including bad or unknown grades. A started job without a terminal receipt stops
resume: reconcile that original attempt without running a replacement. Never delete journal
entries to improve results. Every scheduled job remains in the denominator, including jobs not
run because the budget blocked. Incomplete collection exits unsuccessfully; a complete study
with a negative quality/savings result still completes normally. Use `--require-token-target`
on `report` when acceptance itself must control the exit status.

Private objects retain native accounting, independent grader output and execution observations;
raw streams remain under `study/run/runs`. Keep the entire study outside git. SHA-256 objects
establish integrity, not provider authenticity or an independent registration timestamp. Publish
the frozen digest before running a claim study. These contracts and fixtures do not establish
20–30% customer savings; that needs representative live evidence.

Free contract checks, including the real Rust validator and simulated agents:

```sh
cargo build --locked -p adage-cli
python3 -m unittest evals/live/test_evidence.py evals/live/test_frozen.py
```

## Phases and rounds

| Phase | What happens |
| --- | --- |
| `train` | Round 1 only. Baseline agents work the training tasks with no Adage hooks installed |
| `learn` | Outcome-assisted `adage condition` reads training transcripts **and hidden scorer pass/fail labels via `--outcomes`**. This is not the shipped default bootstrap protocol. In later rounds it also reads earlier rounds' eval sessions (see below), with each route kind excluded from the sessions it steered |
| `eval` | Agents work the held-out tasks: every (task, repeat) runs one session per arm, in a seeded random order: `adage` (hooks installed, directives delivered), `control` (hooks installed, directives logged but not delivered: the control) and optionally `off` (no hooks: measures hook overhead), `instincts` (hooks installed, directives delivered, from a day-0 index of instincts only: `adage condition --instincts-only` over a fresh sandbox, in `private/instincts-home`) and `e1` (the learned index plus E1 prefetch through `adage-exec`, from `private/e1-home`, which a person prepares: a copy of `adage-home` with `adage review delegate ... --level e1` approvals; the runner refuses the arm without it) and `agents-md` (the control's hooks, directives withheld, plus a context file the sandbox command commits as `CLAUDE.md` when it gets `{arm}` = `agents-md`: an up-front context file against just-in-time reflexes; see evals/repobench) |
| `report` | `summary.json` and a results page (below): per-arm aggregates, task-level statistics between `adage` and `control` (see [Statistics](#statistics)) |

The phases default to all four. Name some to run only those: for example `eval report --round 2`.
A config's `rounds` (default 1) run in order in one invocation: round 1 is train, learn, eval,
report; each later round learns again and runs its held-out tasks (`eval2-…`). `--round N` runs
only round N. The spend cap counts every run already in the root's `state.json`, so it holds
across invocations.

**Fresh held-out tasks per round.** A round must not learn from sessions of the tasks it is then
scored on: that measures recall of the answer, not transfer. Give the config
`eval.tasks_by_round`, one disjoint pool per round (a list of lists, or a dict keyed `"1"`,
`"2"`, …). Round N then evaluates only its pool, and learns from the training sessions plus
earlier rounds' eval sessions of tasks *not* in its pool. The config is refused if two pools
overlap, a training task is held out, or a round has no pool. Without `tasks_by_round` every round
reuses `eval.tasks` and learns from every earlier session, as before; the dry run and the run print
a loud warning, and `state.json` and the round's summary say `leaky: true`. Both shipped configs
are still in this mode, so their round-2 numbers are leaky.

## Metrics and the results page

Every run records a timeline of its tool calls (each classified as explore, lookup, edit, verify,
dead end, regenerate), the directives it received, what Adage would have said and judged (from the
session's spool), and the scorer's verdict. `report` summarises the held-out runs per arm and round:

| Metric | Per run | Aggregate |
| --- | --- | --- |
| `turns_to_first_edit`, `time_to_first_edit_s` | Model turns (seconds) before the first edit | median |
| `total_turns`, `elapsed_s`, `explore_calls_before_edit` | | median |
| `cost_usd` | The session's reported cost | mean |
| `pass_rate`, `hidden_pass_rate` | The scorer's `passed`, and its hidden test alone | mean |
| `dead_end_hits` | Failed verify commands | mean |
| `obligation_met_rate` | Of runs that edited the obligation's paths: regenerated after the last such edit, and generated code is fresh | mean |
| `stale_generated_rate` | The scorer found stale generated code | mean |
| `follow_rate` | Routes the session followed ÷ routes it was exposed to (would-have directives; both arms) | mean |

A session that never edited has no first edit. It is not dropped (that would flatter whichever
arm gives up more often): its value is restricted at what was observed, `max_turns` if it was
censored and otherwise the turns (seconds) it ran, and counted in `n_imputed`. A censored run's
`total_turns` counts as `max_turns`.

It then writes `<root>/results/`: `index.html` (the demo dashboard with the final state embedded,
as in `demo/results/`), `artifact.html`, `summary.json` and `state.json`, with the home directory
written as `~`. While a run is going,
`DEMO_ROOT=<root> ADAGE_HOME=<root>/private/adage-home python3 demo/dashboard/serve.py` shows it
live.

## Run statuses

Every finished run gets a status in `metrics.status`, with a `status_reason` (and a copy in the
run's `run_status`; the run's top-level `status` stays `passed`/`failed`/`running` for the
dashboard):

| Status | When | In the comparison |
| --- | --- | --- |
| `ok` | Anything else | Kept |
| `censored` | The session hit `max_turns` (`error_max_turns`) | Kept and counted (`n_censored`); turns capped at `max_turns` |
| `infra_error` | The harness failed: no result message, an error result other than the turn limit (API errors, `error_during_execution`), no tool calls in at most one turn, or the runner itself failed on the session | Left out of both arms and counted |
| `scorer_error` | The scorer crashed, printed no JSON, or printed no `passed` | Left out of both arms and counted |

An infrastructure failure says nothing about the agent, so it is not a failed task; but it is
never dropped silently: every summary has `status_counts` and `excluded` per arm, sessions that
never ran (spend cap) are in `unfinished`, and every metric has `n`, `n_missing` (runs with no
value, such as `obligation_met_rate` where the obligation did not apply) and `n_imputed` per arm.
If one arm has many more errors than the other, that is a finding in itself. States written before
statuses existed are classified from their metrics (`demo/report.py` `status_of`).

## Statistics

Repeats of one task are not independent, so the unit of analysis is the task, not the run.
Treating 36 runs as 36 observations (as the first reports did) overstates the evidence. For each
metric, `report` gives (implemented in `demo/report.py`, so `python3 demo/report.py <root>` computes
the same):

| Field | Meaning |
| --- | --- |
| `adage`, `control` | The arm's aggregate over its runs (median or mean, as in the table above) |
| `diff`, `ci95` | `adage - control`, with a 95% percentile bootstrap CI that resamples whole tasks (2000 resamples) |
| `task_mean_diff` | The test statistic: the mean over tasks of (task mean of adage - task mean of control) |
| `p_value` | Two-sided permutation test that shuffles arm labels **within each task** (the randomisation unit) and recomputes `task_mean_diff` (10000 permutations) |
| `p_holm` | `p_value` after Holm's step-down adjustment across the round's metrics (the family) |
| `p_tasks` | Two-sided sign-flip test on the per-task differences alone: whether the effect would hold for *new tasks*. Exact; with k tasks its smallest possible value is 2/2^k (0.031 for 6) |
| `n_tasks` | Tasks with values in both arms |

Read `p_holm` for "did Adage change this metric on these tasks", and `p_tasks` and `ci95` for how
far that generalises. The permutation statistic is a mean, so one extreme run (a 150 s session
among 20 s ones) can dominate it even for a metric whose headline is a median. All randomness is
seeded, so a report is reproducible.

## Isolation

Each session gets an empty home of its own, `<root>/runs/<run>/home`: `HOME`, `CLAUDE_CONFIG_DIR`
(`home/.claude`: Claude Code's transcripts, auto-memory and settings), `XDG_CONFIG_HOME`,
`XDG_DATA_HOME`, `XDG_STATE_HOME`, `XDG_CACHE_HOME`, `CODEX_HOME` and `TMPDIR` all point into it,
and its environment is built from scratch: only `PATH`, locale, terminal, time zone, proxy and CA
variables, the auth variables below, and the config's `forward_env` pass through. So no session,
of either arm, can read your Claude Code auto-memory (`MEMORY.md`), your earlier transcripts
(`~/.claude/projects`), your settings or MCP servers, or another session's transcript. (Before
this, control agents read the operator's auto-memory in 23 of 36 runs.) Transcripts are archived
from the session's own `CLAUDE_CONFIG_DIR/projects`. The learned index, the transcripts and
`outcomes.json` live in `<root>/private/`, outside every session's run directory; the hooks reach
the index through `ADAGE_HOME`. The home also gets a `.gitconfig` with a neutral identity, and
`DISABLE_AUTOUPDATER=1` keeps the harness version fixed. `state.json` records `harness_version`
(`claude --version`) and each run records the `claude_code_version` its session reported.

**Auth.** An isolated session cannot see your login, so give it one explicitly, in this order:

1. `ANTHROPIC_API_KEY`, `CLAUDE_CODE_OAUTH_TOKEN` (from `claude setup-token`) or
   `ANTHROPIC_AUTH_TOKEN` in the environment: forwarded as is (with `ANTHROPIC_BASE_URL`).
2. Otherwise `~/.claude/.credentials.json` (or `$CLAUDE_CONFIG_DIR/.credentials.json`) is copied
   into the session's config directory for the session only, and deleted when it ends. macOS keeps
   the login in the Keychain instead, so there use option 1.
3. Bedrock, Vertex or anything else: name the variables in the config's `forward_env`
   (e.g. `["CLAUDE_CODE_USE_BEDROCK", "AWS_REGION", "AWS_BEARER_TOKEN_BEDROCK"]`).
4. A host that manages the login itself (Claude Code on the web and similar containers): `--host-auth`
   (or `"host_auth": true`) forwards exactly `HOST_AUTH_ENV`, when set: `ANTHROPIC_BASE_URL`,
   `CLAUDE_CODE_PROVIDER_MANAGED_BY_HOST`, `CLAUDE_SESSION_INGRESS_TOKEN_FILE`,
   `CLAUDE_CODE_OAUTH_TOKEN`, `CLAUDE_CODE_ORGANIZATION_UUID`, `CLAUDE_CODE_ACCOUNT_UUID`, the
   proxy variables (both cases) and `SSL_CERT_FILE`, `NODE_EXTRA_CA_CERTS`, `REQUESTS_CA_BUNDLE`.
   Auth is then the host's token (`auth: host`); no credentials file is copied.

`forward_env` never passes an agent session's own markers: `CLAUDECODE`, any `*SESSION_ID*`,
`CLAUDE_CODE_REMOTE*`, `CLAUDE_CODE_MESSAGING_*`, `CLAUDE_CODE_ENTRYPOINT`, `ADAGE_*` are refused
when the config loads. A child session that carried the host's session id would attach to, and
report into, the operator's own session.

`--execute` refuses to start when it finds none of these. `state.json` records which was used
(`auth`), never the secret.

`--no-isolate` runs sessions with your own `HOME`, `~/.claude` and full environment, and reads
transcripts from your `~/.claude/projects`, as the runner used to. It prints a warning and
`state.json` says `isolated: false`. Use it only to debug the harness.

This is isolation from accidents, not a security boundary: a session runs as you, can walk the
filesystem, and sees `ADAGE_HOME` in its environment. Real agents do walk it: in the first corpus run
(evals/corpus), Haiku sessions ran `find /tmp -name "*hidden*"`, read the hidden test of their own task,
and read the runner's `state.json` (every other session's commands and prompts). So for anything you
mean to measure, confine the sessions:

**Confinement** (`"confine": true`, needs bubblewrap). Each `claude -p` runs under `bwrap` in its own
mount and PID namespace: the whole filesystem read-only, `/tmp`, `/home`, `/root`, `/var/tmp`, `/mnt`
and `/workspace` (`confine_hide`) replaced by empty directories, then mounted back: the run directory
(read-write: the sandbox and the session's home), the arm's `ADAGE_HOME` (read-write, hooks write
spools there), the settings file and the hook binaries (read-only), the directories of the files named
by `SSL_CERT_FILE`, `NODE_EXTRA_CA_CERTS`, `REQUESTS_CA_BUNDLE`, `CLAUDE_SESSION_INGRESS_TOKEN_FILE`
(directories, because a bind mount pins the inode and the host replaces its token file by rename),
the `PATH` entries under a hidden directory, and `confine_expose` (e.g. a newer Node). A fresh `/proc`
hides other processes' environments. The network is shared. The sandbox command and the scorer run
outside, so the hidden tests, the solutions, other runs and `private/` are out of the agent's reach.

**The tool set.** `claude -p` gets `--tools <allowed_tools>` (or the config's `tools`), not only
`--allowedTools`: newer Claude Code versions offer more built-in tools, and a session asked to "pick
up W-1042" called `TaskGet W-1042` (the harness's own task list), found nothing and asked the user
what to do. Pinning the tool set keeps sessions of different harness versions comparable.

## Arm order

Within each (task, repeat) the arms run in an order shuffled with a seed, so neither arm always
starts first (and, with `concurrency` > 1, always gets the less loaded machine). The seed is
`--seed N`, else the config's `seed`, else the one already in the root's `state.json` (so later
rounds continue a run), else a fresh random one. It is printed by the dry run, recorded in
`state.json` (and on every run), and the order is a pure function of (seed, round, task, repeat):
`--seed N` in the dry run shows exactly the order `--execute --seed N` runs.

## Config

The config is JSON. `configs/acme-billing.json` is the demo.

| Key | Meaning |
| --- | --- |
| `name`, `repo_key` | The run's name, and the repository key tracks are learned under (`adage condition --repo`) |
| `workdir` | The sandbox directory name inside each run directory |
| `sandbox` | argv that creates a fresh sandbox at `{dir}` for task `{task}` (`{repo}` = this repository, `{arm}` = the session's arm) |
| `score` | argv that scores a finished sandbox and prints JSON with `passed` (and optionally `stale_generated`, `ci_passed`, `hidden_passed`) |
| `prompts` | Prompt templates, with `{id}` replaced by the task. Tasks rotate through them |
| `task_prompts` | `{task: prompt}`: a task's own prompt, used as written instead of the templates (RepoBench's issue paraphrases) |
| `train`, `eval` | `{"tasks": [...], "repeats": n}`. `eval.arms` defaults to `["control", "adage"]` |
| `eval.tasks_by_round` | One disjoint held-out pool per round (list of lists, or `{"1": [...], "2": [...]}`); replaces `eval.tasks`. Without it later rounds are `leaky` |
| `rounds` | Learning rounds run by one invocation (default 1) |
| `seed` | Arm-order seed (`--seed` overrides) |
| `forward_env` | Extra environment variables passed to isolated sessions (default none; session markers are refused) |
| `host_auth` | Forward `HOST_AUTH_ENV`, a host-managed login (`--host-auth`; see [Isolation](#isolation)) |
| `confine`, `confine_hide`, `confine_expose` | Run sessions under bubblewrap, seeing only their run directory and what they need (see [Isolation](#isolation)) |
| `tools` | The built-in tools a session has (`claude --tools`); default `allowed_tools` |
| `adage_exec` | The `adage-exec` binary the `e1` arm's hooks run (default: next to `--adage`) |
| `model`, `max_turns`, `concurrency` | Passed to `claude -p`, and the number of sessions run in parallel |
| `learn_args` | Extra `adage condition` arguments (`["--demo"]` for small runs) |
| `allowed_tools`, `disallowed_tools` | `claude --allowedTools/--disallowedTools` |
| `budget_usd` | Total admission budget: known spend plus every outstanding full-session reservation must fit before another session is admitted |
| `session_budget_usd` | Positive per-session reservation and Claude `--max-budget-usd` value (default `$1`); cannot exceed `budget_usd` |
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
TAP reporter, and removes it. `passed` needs all of: CI fire, the hidden suite visible in the CI
output (`ci_ran_hidden`: a CI that skipped it does not count), the explicit run executing at least
as many tests as the file declares with none failing (`hidden_passed`), and generated code fresh
by the template's own generator, not the sandbox's (`stale_generated`).

**Checks that spend nothing.** Run both before a live run:

```bash
python3 evals/live/sandboxes/harbor-dispatch/selfcheck.py        # ~20 s; prints ALL OK
python3 evals/live/sandboxes/harbor-dispatch/rehearse.py /tmp/harbor-rehearsal   # ~40 s; needs target/release/adage
```

`selfcheck.py` proves determinism, the dead end, the lookup for every ID (and that its plain-text
output contains no path-like text besides the listed files, which `adage condition` would take for
linked files), that every incident fails untouched and passes solved, the obligation, the scorer's
skip guard, and that all fixes together pass. `rehearse.py` runs scripted, model-free sessions with
the real commands in real sandboxes, writes them as Claude Code transcripts in the runner's layout,
and checks that `adage condition` learns all four conventions from them, that the runner's metrics and
results page work, and that `build_demo.py` plus `adage eval` ingest the root.

**The run.** 84 agent runs: 6 training incidents x 2 repeats (no hooks), then in each of 2 rounds 6
held-out incidents x 3 repeats x 2 arms (control, adage). Round 2 learns from all 48 earlier
sessions, including round 1's sessions of the same six held-out incidents, so it is `leaky` (see
[Phases and rounds](#phases-and-rounds)). The spend cap is $45.

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
- Each session runs in a fresh sandbox and an empty home of its own (see [Isolation](#isolation)),
  with the harness's own permissions and `--setting-sources project,local`. Your user settings,
  memory and MCP servers are not loaded.
- Before each session, the runner atomically reserves its full `session_budget_usd`. Concurrent
  workers wait for outstanding reservations to settle. Each Claude invocation receives
  `--max-budget-usd`; leave headroom below the real authorized ceiling because the harness may
  report a cost above its requested cap.
- `state.json.accounting` records every attempt, including reruns with the same ID. Missing,
  invalid, or interrupted terminal cost is **unknown**, never zero. Unknown accounting or a cap
  overrun stops further dispatch, persists across restarts, and returns exit code 2. Already
  dispatched workers can finish within their existing reservations; no new worker may start.
- A failed accounting write blocks further dispatch and wakes waiting workers. A process lock
  refuses two simultaneous runner invocations against the same experiment root. Separate roots
  have separate ledgers; the operator must allocate their budgets from the shared authorization.
- Review actual terminal result records before reconciling an unknown attempt. Do not delete the
  ledger, clear the stop reason, or treat a missing cost as free to resume. Legacy zero-cost records
  need reconciliation because the old runner also wrote zero when the cost was missing.

## Output, and turning a run into an offline dataset

```text
<root>/runs/<run>/<workdir>/              the sandbox after the agent finished
<root>/runs/<run>/home/                   the session's own HOME (and CLAUDE_CONFIG_DIR, XDG dirs, TMPDIR)
<root>/runs/<run>/stream.jsonl            the `claude -p` stream, time-stamped
<root>/private/transcripts/<project>/<session>.jsonl   the Claude Code transcript (what `adage condition` reads)
<root>/private/adage-home/                tracks, index, and per-session spools and state
<root>/private/outcomes.json, settings-*.json
<root>/state.json, summary.json
<root>/results/                           the results page (index.html, artifact.html, summary.json, state.json)
```

Once the last session of an invocation has finished, the root also gets `adage-home`,
`transcripts` and `outcomes.json` links into `private/` (removed again before any new session
starts), so it has the layout of a demo archive. A root written before `private/` existed is moved
into this layout on its next `--execute`.

So any live run becomes a replayable offline dataset, and `adage eval` can then measure it
without spending anything more. Name the dataset, and give a fresh sandbox for `sandbox.txt`:

```bash
python3 evals/datasets/tools/build_demo.py /tmp/adage-live/acme evals/datasets/my-run \
    --name my-run --repo acme-billing --sandbox <a fresh sandbox dir>
adage eval evals/datasets/my-run --split round1
```

The dataset gets one split per round (`round1`, `round2`, …): train on the training sessions and
every earlier round's eval sessions, test on that round's control arm.

## Tests

```bash
python3 -m unittest evals/live/test_runner.py evals/live/test_budget.py
```

This smoke-tests the dry run and unit-tests the analysis. It runs no agent and spends nothing. It
checks:

- the demo plan: 54 sessions, the right arms and environment, and no hooks in training;
- the second demo's plan: 84 runs over two rounds, Sonnet 5, and its estimate line;
- `--config` and `--dry-run`, and that `--dry-run --execute` is refused;
- that a dry run writes nothing;
- the overrides and later rounds;
- the second sandbox's command classification and the per-run metrics (dead ends, obligation, follow rate);
- that `--execute` is refused inside an agent session;
- that a bad config is refused;
- the arm order: one of each arm per (task, repeat), random across pairs, fixed by the seed;
- run statuses (and the statuses of older states), and that summaries leave errors out of both
  arms, count them, and keep censored runs;
- the task-stratified permutation test, the task bootstrap, the task-level sign-flip test and Holm
  on toy data;
- the isolated environment (nothing of the operator's but the allowlist and auth), auth discovery,
  and that session homes lie outside `private/`;
- `tasks_by_round`: per-round pools, refused overlaps, the leaky warning, and that a round never
  learns from its own tasks;
- that nothing under `demo/results/` names a home directory.
