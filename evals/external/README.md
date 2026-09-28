# External benchmarks: Adage inside Harbor

This directory runs Adage on public agent benchmarks through
[Harbor](https://github.com/harbor-framework/harbor), the harness behind Terminal-Bench 2. The target
suites are SWE-bench Pro, SWE-Gym, SWE-smith, swe-gen-js and Terminal-Bench. The offline replay eval
(`adage eval`) and the live proving ground (`evals/live/`) measure Adage on repositories we built.
This tier (L4 in the trial records) asks the question those can't: does a frozen Adage index,
learned only from earlier agent sessions in a real open-source repository, change how often later
sessions in that repository resolve their task, and what does that cost?

Everything here is stdlib Python. Harbor is only needed to run jobs; the planner, the checkers, the
statistics and the tests work without it.

| File | What it is |
| --- | --- |
| `adage_harbor.py` | `AdageClaudeCode`: Harbor's `ClaudeCode` agent with Adage's hooks, per arm. Codex and OpenCode are TODO stubs, with notes on how they differ. |
| `protocol.py` | The pre-registered protocol as code: task metadata, the chronological split with a buffer, the split digest, the leakage checker and the patch-cleanliness checker. |
| `plan.py` | CLI with these subcommands: `split`, `phases` (every command, plus a cost estimate), `harvest`, `emit` (JSONL trial records), `check-leakage`, `check-patches`, `analyse` and `power`. |
| `stats.py` | Paired analysis: per-task means, a sign-flip permutation test, a repo-then-task cluster bootstrap, Holm correction and power. |
| `test_*.py`, `fixtures/` | Unit tests, including a fake Harbor job directory. Run them with `python3 -m unittest discover evals/external`. |

## The protocol

**Arms.** Every arm runs the same Claude Code agent, model, turn limit and container. They differ only
in what Adage contributes.

| Arm | Hooks | What the agent gets |
| --- | --- | --- |
| `off` | none | Plain Claude Code. This is the baseline, and it also measures hook overhead. |
| `holdout` | installed, `ADAGE_ARM=holdout` | Nothing. Adage decides and spools every directive but never delivers one. **This is the primary control.** |
| `adage` | installed, `ADAGE_ARM=treated` | Directives through `additionalContext`, as in production. |
| `agents-md` | none | The learned routes' summaries, appended to the system prompt. This is the "just write it in AGENTS.md" baseline, and it writes nothing into the repository. |
| `adage-lro` | as `adage` | Another repository's index. This is the leave-repo-out control: it separates repository-specific knowledge from generic "be careful" nudging. |

The primary contrast is `adage` against `holdout`. Both run identical hook processes, so the only
difference is whether the text is delivered. `adage` against `off` gives the net effect, hook
overhead included. `adage` against `adage-lro` and `adage` against `agents-md` are secondary. For
the secondary arms, pass `--arms off,holdout,adage,agents-md` and `--leave-repo-out` to `plan.py phases`.

**Chronological split.** The split is made per repository, by base-commit date, with a buffer.
- Each task's base commit is dated with `git show -s --format=%cI` in a cached partial clone of the
  upstream repository, and the dates are cached in `dates.json`.
- The oldest 50% of a repository's tasks are for training. The next 3 are discarded as a buffer
  (near-simultaneous PRs often touch the same code). The rest are the test set.
- A repository with fewer than 5 test tasks is excluded and listed as such, never merged into another.
- Ties are broken by base commit, then by task id. The split is deterministic whatever order the
  input comes in.
- `split_digest`, a SHA-256 of the task lists, is the pre-registration. Publish it before the
  first eval job.

**What training means.** Training is `off`-arm agents solving the training tasks. Their transcripts
are harvested per repository, and `adage condition --repo adage-eval:<suite>/<owner>__<name>` is run
once per repository with the default thresholds and its own `ADAGE_HOME`. The result is frozen as a
tarball, recorded in `MANIFEST.sha256`, and nothing is learned after that point. Every eval trial sets
`ADAGE_AUTO_LEARN=0`. No gold patches, hints, test names or upstream history are given to Adage.
Adage sees only what the training agents did.

**Isolation in every arm:**
- Web tools are disallowed (`disable_web_search=True` → `--disallowedTools WebSearch,WebFetch`).
- Each trial gets a fresh `CLAUDE_CONFIG_DIR`: Harbor's per-trial `/logs/agent/sessions`.
- `memory_dir` is refused.
- Nothing is written into the repository worktree. The hooks come in through `--settings`, the
  binary goes to `/installed-agent`, and `ADAGE_HOME` goes to `/logs/agent/adage-home`.

**Leakage check (blocks the eval).** After learning, `check-leakage` scans `index.json` and every
track file for test-set gold-patch paths, test-patch paths and FAIL_TO_PASS/PASS_TO_PASS ids that no
training transcript mentions. Any finding stops the run. An item the training transcripts did touch
is legitimately learned repository knowledge and is allowed.

**Patch cleanliness.** After the agent finishes, each trial records `git status --porcelain` and
`git diff HEAD` into `<agent logs>/adage/`. It uses `--no-optional-locks`, so it does not even
refresh the git index. `check-patches` fails any trial with `.adage/`, `.claude/`, `.codex/` or
`.opencode/` paths in the diff, status or untracked files.

**Repository keys.** The index is learned under a canonical key. Inside the container, a hook computes
its own key: the normalised `origin` URL, else `<dir>@<root commit>`, else `<dir>@path-<hash>`
(adage-core `repo.rs`). At setup, the agent probes the checkout, then re-keys that repository's routes
under every key the hook could compute there. The re-keyed routes are unchanged, as is the
`generation`. `emit` flags any hook-arm trial that left no spool record as `hooks_inert`. Such a
trial means the lookup failed, and it must be investigated, not analysed as a null.

**Pairing, repeats and scheduling.**
- There is one Harbor job per (repository, repeat), and each job holds every arm as a separate agent
  config. Harbor expands trials as attempt × task × agent (`JobPlan.build_trial_configs`), so the arms
  run back to back on the same task.
- Arm order within a job, task order and job order are all shuffled from `--seed`.
- The default is 3 repeats per (task, arm).

**Outcomes and statuses (`emit`).** Each trial record gets one of these statuses:

| Status | When | Counted in the analysis? |
| --- | --- | --- |
| `ok` | The verifier gave a reward | Yes |
| `censored` | The agent timed out, or hit the turn or budget limit, and was still verified | Yes, with its verifier outcome. A sensitivity analysis excludes these. |
| `infra_error` | Environment, setup or API failure, or a timeout that was never verified | No. Reported per arm, never imputed. |
| `scorer_error` | The verifier failed or no reward was written | No. Reported per arm, never imputed. |

A trial passes when `reward >= 1`. Cost, input tokens, cache tokens and output tokens come from
Harbor's `AgentContext`. Turns come from the stream-json `result` event's `num_turns`, falling back
to the number of distinct assistant messages in the transcript.

**Statistics (`stats.py`).**
- The unit of analysis is the task, with each arm summarised by its mean over repeats.
- Differences are paired within task.
- Effects:
  - `delta_resolve`: the mean per-task difference in resolve rate.
  - `cost_ratio` and `turns_ratio`: the geometric mean of the per-task ratios, that is `exp(mean log ratio)`.
- p-values come from a sign-flip permutation test: exact up to 16 non-zero pairs, Monte Carlo with
  10,000 draws beyond that.
- 95% CIs come from a two-stage cluster bootstrap: repositories with replacement, then tasks within
  each drawn repository.
- Holm correction is applied across all (comparison × metric) tests.
- The primary endpoint is `delta_resolve` for `adage` against `holdout`. Cost and turns are secondary.

## Power

Pairs (test tasks) needed for 80% power at a two-sided alpha of 0.05. These use the paired binary
normal approximation (McNemar; Connor 1987) and come from `python3 evals/external/plan.py power`.
*Discordance* is the share of tasks where the two arms disagree. With per-task means over three
repeats, discordance behaves like a continuous spread, so treat these figures as conservative.

| Discordance | Δ = 3 pts | Δ = 5 pts | Δ = 8 pts | Δ = 10 pts |
| --- | --- | --- | --- | --- |
| 0.10 | 870 | 312 | 121 | – |
| 0.15 | 1306 | 469 | 182 | 116 |
| 0.20 | 1742 | 626 | 243 | 155 |
| 0.30 | 2614 | 940 | 366 | 234 |

Cost (log ratio, `pairs_for_continuous`):
- With a per-task SD of 0.5, a 10% change needs 177 pairs and a 20% change needs 40.
- With an SD of 0.8, the same changes need 453 and 101.

Clustering by repository inflates variance by `1 + (m - 1) · ICC` (see `stats.design_effect`).

What SWE-bench Pro can support:
- It has 731 tasks in 11 repositories, with 20 to 96 per repository. After a 50% split and a
  3-task buffer, that leaves roughly 330 test tasks.
- That is enough to detect about 8 points at a discordance of 0.2, or 5 points only if discordance
  is near 0.1.
- The pilot measures discordance and cost SD. Those two figures set the final repeat count.

## Running it

### Build

```sh
cargo build --release --target x86_64-unknown-linux-musl -p adage-cli   # the static binary for the containers
cargo build --release -p adage-cli                                     # the host binary for `adage condition`
uv tool install harbor   # or: pip install -e <harbor checkout>; Harbor needs Python >= 3.12
export PYTHONPATH=$PWD/evals/external   # so Harbor can import adage_harbor:AdageClaudeCode
```

### Pilot (20 test tasks)

```sh
R=<harbor>/registry.json
python3 evals/external/plan.py phases --registry $R --dataset swebenchpro@1.0     # prints step 0: metadata + split
# ... run step 0 (sparse checkout of tests/config.json, then `plan.py split`) ...
python3 evals/external/plan.py phases --registry $R --dataset swebenchpro@1.0 --split split.json \
    --pilot 20 --repeats 1 --write-configs configs/pilot
```

`phases` prints every command in order, with its cost estimate:

1. train job
2. harvest
3. one `adage condition` per repository
4. freeze and leakage check
5. seeded eval jobs
6. `check-patches`, `emit`, `analyse`

`--pilot N` takes the two largest repositories, the latest N training tasks of each (the closest to
the cutoff), and N test tasks drawn round-robin between the two. With 3 arms and 1 repeat, that is
2N + 3N trials, or about $150 at the Sonnet estimate for N = 20. The pilot must show three things:
- zero `hooks_inert`
- zero patch violations
- zero leakage findings

It then yields the measured cost per trial and the measured discordance for the full run's plan.

### Full run

Repeat the same steps without `--pilot`, with `--repeats 3`. For SWE-bench Pro with Sonnet, the
estimate is about 365 training trials plus about 3,000 eval trials, around $5,000.

## Suites

Counts were checked in Harbor's `registry.json` at commit 3c82380.

| Suite | Registry | Tasks / repos | Split | Notes |
| --- | --- | --- | --- | --- |
| SWE-bench Pro | `swebenchpro@1.0` | 731 / 11 (20–96 each) | chronological | Primary suite. `tests/config.json` holds repo, base_commit, patch, test_patch, fail_to_pass. |
| SWE-bench Verified | `swebench-verified@1.0` | 500 / 12 | chronological | Only django, sympy and a few others have enough tasks per repository. |
| swe-gen-js | `swe-gen-js@1.0` | 1000 / 30 | chronological | From `abundant-ai/SWE-gen-JS`. Its task layout was not inspected: if tasks lack `tests/config.json`, metadata needs a loader. |
| SWE-smith | `swesmith@1.0` | 100 / 1 (oauthlib) | by synthetic id | Synthetic bugs on a single commit, so base-commit dates are all equal. The split falls back to task id. Use it as a within-repo learning check, not for chronology. |
| SWE-Gym | not in the registry | 2,438 (lite 230) / 11 | chronological | Generate with `adapters/swegym`, then pass `-p <dir>` tasks. The adapter writes `tests/config.json`. |
| Terminal-Bench 2 | `terminal-bench@2.0` | 89 / none | n/a | No repositories to learn from. Use it only as a no-harm check (`off` against `adage` with an index from another suite): its resolve rate must not drop. |

## Verified against source, and assumed

**Verified in Harbor source (commit 3c82380):**
- `ClaudeCode` in `installed/claude_code.py`:
  - It uploads a `config` dict or file to `/tmp/claude-code-settings/settings.json`, runs
    `claude --settings <that>`, and deletes it afterwards.
  - `CLAUDE_CONFIG_DIR` is `<environment_logs_dir>/sessions` (`/logs/agent/sessions`), so transcripts
    come back under `agent/sessions/projects/<slug>/*.jsonl`, and subagent transcripts are skipped.
  - `disable_web_search` appends `WebSearch,WebFetch` to `--disallowedTools`.
  - `env.update(self._resolved_env_vars)` is how the arm's environment reaches `claude`.
  - Stream-json output is teed to `/logs/agent/claude-code.txt`.
  - `populate_context_post_run` fills `cost_usd` and `n_input_tokens`, `n_cache_tokens` and
    `n_output_tokens`.
- Custom agents load via `-a module:Class`, or `import_path` in a job config (`AgentFactory`).
  Kwargs are validated against `options_model`, with `extra="forbid"`.
- `JobConfig` takes `agents: [...]`, explicit `tasks: [{git_url, git_commit_id, path}]` and
  `n_attempts`. Trials are expanded as attempt × task × agent.
- `TrialResult` in `result.json` carries `task_name`, `config.agent.kwargs`, `agent_info`,
  `agent_result`, `verifier_result.rewards`, and `exception_info.exception_type`
  (`AgentTimeoutError`, the verifier's `RewardFileNotFoundError`, and so on).
- The registry entries and counts above, including SWE-bench Pro's 11 repositories with 20 to 96
  tasks each. SWE-Gym is an adapter, not a registry dataset.
- SWE-bench Pro's `tests/config.json`: its fields, and its Python-literal `fail_to_pass` lists.
  One task was fetched from harbor-datasets at commit c8e8f3f.
- `AdageClaudeCode` was constructed under Harbor, installed in a Python 3.12 scratch venv, for every
  arm, directly and through `AgentFactory.create_agent_from_config` with an import path. Its
  `setup()` was driven against a fake environment and uploaded the re-keyed home and the binary.

**Verified in Adage source:**
- The hook block matches `harness/claude.rs` `settings` and `adage hooks-config`.
- The merge semantics match `install.rs` `merge_hooks`.
- `ADAGE_ARM` forces the arm (`hook.rs`), and `ADAGE_AUTO_LEARN=0` stops live learning.
- The spool frame format.
- The index format, `{generation, routes[{repo, track, version, route}], triggers{repo: …}}`
  (`adage-tracks` `index.rs` and `model.rs`).
- The repository-key rules in `repo.rs`, checked end to end with a release build of `adage`:
  - Its `hooks-config` output equals `hooks_settings`.
  - For two scratch repositories, one without `origin` and one with it, `PROBE_SCRIPT` was run, the
    index was re-keyed with `candidate_keys`, and the real `adage hook claude UserPromptSubmit`
    (`ADAGE_ARM=holdout`) found the routes and spooled under the expected key.
  - With a wrong key it spooled nothing.

**Assumed, not verified by a run:**
- No Harbor job has been run, so there is no Docker here and no API spend.
- That Claude Code honours `hooks` from a `--settings` file in headless `-p` mode. `evals/live`
  relies on the same thing and works.
- `CLAUDE_CODE_DISABLE_AUTO_MEMORY=1` as the switch for auto-memory. A fresh config dir already
  gives no memory, so this is belt and braces.
- That the static musl binary runs in every task image (x86_64 only; arm64 images need the aarch64
  build).
- That `base64` and `git` exist in the image for the probe. Without `git`, only the path-hash and
  origin keys are offered, and `hooks_inert` catches a miss.
- The per-trial cost table (`plan.py` `PER_TRIAL_USD`). **Every dollar figure is an estimate** until
  the pilot measures it.
- That the task name in `result.json` matches the registry name, possibly with an org prefix;
  `match_task` handles both.

## Open issues

- Codex and OpenCode arms are stubs (see the notes in `adage_harbor.py`). Codex needs its hooks
  pre-approved inside Harbor's `CODEX_HOME`. OpenCode needs the plugin in `$XDG_CONFIG_HOME` and a
  transcript importer.
- swe-gen-js task metadata is unverified.
- Terminal-Bench has no per-repository structure, so it cannot test learning.
- SWE-smith's single commit has no chronology.
- Harbor's `with_prompt_template` and its `load`/`resume` paths are untouched and untested with
  this agent.
