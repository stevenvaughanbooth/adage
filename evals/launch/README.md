# Adage launch readiness

2026-09-29. The final evaluation of the finished build (branch `claude/repo-maturity-roadmap-l95ohs`
at `cae21c9`, release binaries built once), before a 0.1.0 launch. Real headless Claude Code
sessions (2.1.284), isolated per session (fresh `HOME` and `CLAUDE_CONFIG_DIR`, the host-auth
allowlist, `--tools` pinned) and confined under bubblewrap unless stated. Everything here was spent
from one $120 cap; this run spent **about $77.83** (table at the end).

## Verdict: NO-GO

**Do not launch Adage with its pitch ("save money, make models more capable") as built.** On real
repositories, the setting a user is in, it changes nothing a user can see: on four real open-source
repositories and 30 real bug-fix tasks, with the turn cap no longer censoring the sessions, the
resolve rate was 0.62 with Adage against 0.65 without and the cost per resolved task $0.47 against
$0.47. It learned no reflex from real traces: from the benchmark's training traces, and in the
zero-config run from 58 real past sessions, the learner compiled nothing a user would get. The one
thing it now delivers on real repositories is an instinct: "tests run in the project's `.venv`".
It fired in 41 of 66 Python sessions and the agents followed it every time. It works: about two
tool calls saved per session that needs it (CI [−2.56, −1.17]), which is about $0.01. But a bug
makes Adage judge every one of those follows as "ignored", so in real use it would switch that
instinct off by itself after about five deliveries (reproduced). E1 prefetch cannot be approved for
any lookup the learner produces (a second bug). The machinery around all this is sound and ready:
the plugin bundles its binary, the zero-config bootstrap and notices work, E2 actions apply, `adage
train` gives real verdicts, the offline gates pass, and nowhere did Adage measurably harm a session.
So the product is safe and well made, but nothing yet shows it is worth installing on a typical
repository. **What would change the verdict:** fix bugs 1–3, and rerun RepoBench with the U11 fix.
With that, a **GO-with-caveats** as an explicitly experimental preview is defensible, with only the
claims listed under "What a user should expect". A launch that claims savings needs a real-repository
result that does not exist yet.

## Evidence

### 1. RepoBench v2: real repositories, real bugs (the headline)

The same task definitions, chronological splits and archived Haiku training traces as the first
RepoBench ([v1](../bench/results/2026-09-28-repobench/README.md)), with `max_turns` 60 (v1: 30),
the thresholds a user's automatic learner uses (`default`; v1 used `--demo`), and instincts bound
against a fresh checkout with its `.venv`/`node_modules`, as the zero-config bootstrap binds them.
Arms: `control` (hooks on, directives withheld) vs `adage`. Claude Haiku 4.5, 3 repeats on the
Python repositories and 2 on vuejs/core. Full tables, charts and task-clustered statistics:
[evals/bench/results/2026-09-29-repobench-v2](../bench/results/2026-09-29-repobench-v2/README.md).

**Pooled, Haiku 4.5** (30 tasks; 82 runs per arm; the task is the unit; CIs resample tasks; Holm
across the metrics):

| Metric | adage | control | diff | 95% CI | p (Holm) | v1 (30 turns): adage vs control |
| --- | ---: | ---: | ---: | --- | ---: | --- |
| Resolve rate | 0.62 | 0.65 | −0.02 | [−0.10, 0.04] | 1.00 | 0.51 vs 0.51 |
| Mean cost per session | $0.29 | $0.30 | −$0.01 | [−0.03, 0.01] | 1.00 | $0.22 vs $0.22 |
| **$ per resolved task** | **$0.47** | **$0.47** | +$0.01 | [−0.06, 0.09] | – | $0.43 vs $0.42 |
| Median turns | 35 | 33.5 | +1.5 | [−3, 5] | 1.00 | 30 vs 30 (censored) |
| Median turns to first edit | 8 | 8 | 0 | [−1, 1.5] | 1.00 | 10 vs 9.5 |
| Dead-end hits per session | 1.32 | 1.06 | +0.26 | [−0.11, 0.65] | 1.00 | 1.01 vs 0.89 |
| Sessions that hit the turn cap | 8 of 82 | 10 of 82 | | | | 174 of 228 (all arms) |
| Harm: tasks resolved less often with Adage | 3 of 30 (2 more often) | | | | | 3 of 38 (4 more often) |

**Per repository, Haiku 4.5** (v1 → v2, adage vs control):

| Repository | Runs per arm (v2) | Resolve rate v2 | $ per resolved v2 | Median turns v2 | Sessions with a directive (adage) v2 | v1 resolve rate | v1 sessions with a directive |
| --- | ---: | --- | --- | --- | --- | --- | --- |
| attrs | 24 | 0.79 vs 0.79 | $0.26 vs $0.25 | 29 vs 30 | 16 of 24 | 0.75 vs 0.81 | 0 of 16 |
| click | 24 | 0.38 vs 0.42 | $0.88 vs $0.78 | 39 vs 37.5 | 14 of 24 | 0.50 vs 0.50 | 0 of 16 |
| marshmallow | 18 | 1.00 vs 1.00 | $0.23 vs $0.23 | 27 vs 26 | 11 of 18 | 1.00 vs 1.00 | 0 of 12 |
| vuejs/core | 16 | 0.31 vs 0.38 | $1.38 vs $1.32 | 44.5 vs 56 | 0 of 16 | 0.19 vs 0.19 | 2 of 16 |

**What was delivered, and what it did** (the first run's failure was that nothing was delivered):

| | Haiku 4.5 (82 adage sessions) | Sonnet 5 (16 adage sessions) |
| --- | --- | --- |
| Reflexes learned from the training traces | 0 in 4 of 4 repositories | (the Haiku index) |
| Sessions that received a directive | 41 (all on click, attrs, marshmallow; 0 of 16 on vuejs/core) | 1 |
| Directives per session | 0.50 (one each) | 0.06 |
| Which | Instinct U11: `dead-end-0cdcfb` (after `python -m pytest` fails with `No module named pytest`, run `.venv/bin/python -m pytest`) 33×; `dead-end-855e84` (the `python3 -m pytest` spelling) 8× | U11, 1× |
| Followed (the agent's next test command ran in the project's `.venv`, with its own arguments) | **41 of 41** | 0 of 1 |
| Followed, as Adage judged it | **0 of 41** (all "ignored": bug 1) | 1 of 1 |
| Sessions that hit U11's dead end | 63 of 82 (control 58 of 82) | 6 of 16 |
| ...only through a pipe (`… 2>&1 \| head`, exit 0), invisible to Adage | 22 of 63 | 5 of 6 |

**The mechanism, task-clustered** (Haiku, the three Python repositories, sessions that hit the dead
end; `evals/launch/mechanism.py`):

| Measure | adage | control | diff | 95% CI | sign-flip p (21 tasks) |
| --- | ---: | ---: | ---: | --- | ---: |
| Tool calls from the dead end to a test run that ran tests | 1.44 | 3.29 | −1.85 | [−2.56, −1.17] | 0.0001 |
| Next test command used the project's environment | 71% | 37% | +35 pts | [+17, +52] | 0.004 |

So U11 does what it says, and agents take it. It is worth about two tool calls (roughly one turn,
about $0.01 at Haiku prices) in sessions of 30 to 40 turns: far below what the session-level metrics
can resolve, and consistent with them showing nothing.

**Sonnet 5 slice** (click and vuejs/core, 8 tasks each, 1 repeat, $18.34): resolve rate 0.44 vs 0.44;
mean cost $0.51 vs $0.64 (CI [−0.25, −0.01], Holm p = 0.50); $ per resolved $1.17 vs $1.45. With one
directive in 16 adage sessions, that cost difference is not Adage's doing: it is noise between tasks.

### 2. The corpus (generated sandboxes, for contrast)

| Setting | Result | Source |
| --- | --- | --- |
| First corpus, 4 Haiku sandboxes where a reflex was learned | median turns 12.5 vs 28 (−15.5, CI [−18.5, −9], Holm p = 0.001); cost $0.063 vs $0.120 (−48%) | [evals/corpus](../corpus/README.md) |
| First corpus, all 10 Haiku sandboxes | turns 18 vs 26 (Holm p = 0.03); cost −20% (Holm p = 0.055); pass +11 pts (Holm p = 0.76) | same |
| First corpus, sandboxes where only instincts applied | no measurable effect (turns −1, CI [−4, 3]) | same |
| Sonnet 5, two sandboxes | −3 turns, −24% cost (Holm p < 0.05) | same |
| Same kind of sandboxes with every convention in a committed CLAUDE.md | ~1 turn (median 7 vs 8, Holm p = 0.26), no cost difference | [RepoBench v1](../bench/results/2026-09-28-repobench/README.md) |

The large effects exist only where a convention is both necessary and undiscoverable (a private
work-item lookup, a regenerate step no file mentions). When the convention is written down they
mostly disappear, and on real repositories we have not found one.

### 3. Zero-config, end to end with a real agent

The staged marketplace plugin (`scripts/stage-plugin.mjs` with this build as the Linux binary, loaded
by `claude --plugin-dir`), a scratch clone of click (at a buffer task's base commit, with its `.venv`),
and a fake `HOME` whose `~/.claude/projects` held 58 real earlier click sessions (RepoBench v1's,
rewritten to the clone's path). No `ADAGE_*` variable and no Adage command beforehand. Three Haiku
sessions on three click bugs, run twice: unconfined, and again confined
([`zero_config_e2e.py`](zero_config_e2e.py); records in [data/](data/)).

| Check | Result |
| --- | --- |
| SessionStart notice in the transcript as a hook system message | **Pass.** A `hook_system_message` attachment (`SessionStart:startup`) with the text `adage: learning from 58 past sessions in this repo; reflexes arrive within a minute. Nothing runs without you.`, in session 1 only (sessions 2–3: nothing, as designed) |
| The bootstrap learns | **Pass, and learns nothing.** `learn.log` shows a learn over 58 sessions during session 1, then one after each session (59, 60, 61). The result was 15 instinct routes and 0 learned reflexes. In the unconfined run, after session 2 it formed one learned reflex: a context trigger that directs `git log --oneline -5` after `source {path}` and `cat` (noise; bug 6) |
| Directives delivered from session 1 | **Fail: 0 directives in 6 of 6 sessions.** The one opportunity (confined session 2: `python -m pytest … 2>&1 \| head -100` printed `No module named pytest`) went through a pipe, so no failure event fired (bug 2); the agent then `pip install`ed pytest into its user site |
| Bare `adage` shows the status card | **Pass.** Before: `Not connected to any agent yet. Claude Code found → … /plugin install adage@adage`. After (verbatim below). A plugin-only user has no `adage` on `PATH`, though: this was run with the plugin's own copy (limitation 6) |
| The ledger records cost | **Pass, about 10% low.** `ledger/github-com-pallets-click.jsonl` records turns, tool calls, tokens and cost for all 61 sessions (including the 58 of history), priced from the table: session 1 $0.2367 against the harness's $0.2678 (−12%), session 3 $0.2612 against $0.2908 (−10%) |
| Auto-learn at Stop | **Pass.** A learn after every session (`trigger: auto`), in the detached learner, which survived the session's exit |

What a user sees at the end, verbatim (`adage` in the clone, unconfined run; the confined run's card
is the same with 15 instincts, no learned reflex, and "2 sessions"):

```text
adage 0.1.0 · github.com/pallets/click                         ● check: hooks: codex
  16 reflexes active: 15 instincts, 1 learned
  learning after each session · last: just now, 0 changes
  last 30 days: 3 sessions · 0 directives · 0 followed · 0 dead ends headed off
  measuring savings: 0 of 10 quiet sessions so far (Adage stays quiet in 1 of 10 sessions so its effect can be measured)
  hooks: codex: codex is installed, but Adage's hooks are not registered with it → `adage install codex --write` (this repository) or add `--scope user`
  adage ui · adage map · adage doctor
```

In the session itself the user sees the one notice line and nothing else.

### 4. Delegation (E1 prefetch, E2 actions)

| Check | Result |
| --- | --- |
| Approve the learned `wi-lookup` at E1 (acme sandbox, 12 training sessions, $1.18) | **Fails as shipped.** `adage review delegate acme-billing/learned lookup-work-item --level e1` → `slot {item} has no E1 form (a prompt value with a pattern contract)` (bug 3). With the slot rewritten by hand to the older prompt-span form, approval, `delegate on` and the sandboxed run all work |
| Data block delivered (with that workaround) | **3 of 3 e1 sessions** got `<adage-data … mode="typed">{"files":["acme/customers.py"]}</adage-data>` beside the directive |
| Did turns drop? | **No evidence.** Median turns 11 (e1) vs 12 (adage), 3 tasks each; all 6 passed; cost $0.124 vs $0.142. The agent still ran `wi-lookup` itself in 3 of 3 e1 sessions: the typed default carries only the linked files, not the work item's text (limitation 4) |
| E2: approve `make gen` after `registry/*.csv` edits (gen bundle py-clinic-s12, policy `max_level = "E2"`) | **Pass.** `delegate` staged the action once, showed no diff, recorded the approval |
| E2 applies and the agent is told (2 real sessions) | **2 of 2.** `Adage ran \`make gen\` after your edit to registry/wards.csv: updated 1 file (wardline/generated/registry.py). Nothing else changed.` Generated code fresh, CI and hidden tests passed; `actions.spool` records `applied`. The runner's `obligation_met` metric reports False here because it credits only a regeneration the agent ran itself (harness bug 8) |

### 5. `adage train`, real run

The committed fixture (`crates/adage-train/tests/fixtures`: the acme sandbox and a SKILL.md with a
stale `python3 -m unittest` and a correct `make gen` rule), 24 exploration and 2×3 validation
sessions, default thresholds, **$0.84** ([data/train-report.json](data/train-report.json)):

| Claim | Verdict (real sessions) |
| --- | --- |
| c1 verify `python3 -m unittest` | **contradicted**: failed in 5/5 sessions; learned dead end `dead-end-d41b1e` → `make test` ("your skill is stale") |
| c2 after `schema/*.json`, `make gen` | **compiled** → `finish-line-e7e809` (followed in 6/6) |
| c3 lookup `python3 scripts/wi-lookup` | not compiled: followed in only 1 of 6 exploration sessions |
| c4 `proto/*.proto` | never exercised (no matching file) |
| c5 `pip install -r requirements.txt` | unsafe, never exercised |
| Validation (c1, 1 task per arm) | accepted: skill arm followed 0%, reflex arm 100%, 2 vs 3 turns |

### 6. Offline gates

| Gate | Result |
| --- | --- |
| `cargo test --workspace --locked` | **721 passed, 0 failed, 3 ignored** (101 test binaries and doc-test runs); the eval crate's slow gates (bench baseline reproduction, generated-convention learning, the escape suite) included |
| Python suites (`evals/live`, `evals/gen`, `evals/external`, `evals/repobench`, dataset tools) | 132 tests, all pass |
| `adage eval bench run --suite core` | **J = 0.6307** (as expected); guardrails pass (0 unsafe directives, 0 fast-path mismatches, route precision 1.0); temporal final check 0.5937. J is carried by the synthetic units (1.56–2.30); the three demo-haiku45 units are negative (−1.5, −0.29, −0.29) |
| `adage eval routes`, 15 datasets × {default, demo} | 0 false routes at default thresholds in all 15; 1 false route at demo (py-clinic-s12: a dead end that avoids `python -m pytest`); 0 decoy hits. Recall at default: 1.0 on synthetic, 0.75 on adversarial and demo-haiku45, 0–0.75 on the live corpus datasets (4 of 12 learn nothing). Every live dataset's default split is flagged leaky. [data/routes-table.md](data/routes-table.md) |

## Bugs found

1. **The follow judge scores a followed directive as ignored, and that auto-extinguishes the
   instinct.** `adage_core::pattern::meets` (crates/adage-core/src/pattern.rs) accepts an observed
   command only if it "extends the directed one with options only". U11 directs
   `.venv/bin/python -m pytest`, and agents run `.venv/bin/python -m pytest tests/test_x.py::t -xvs`:
   the positional test path fails the match. In RepoBench v2 all 41 delivered U11 directives were
   followed by the next test command and all 41 were judged `ignored` (0 followed). Consequences:
   (a) `crates/adage-learn/src/instincts.rs::should_extinguish` (at least 5 judgements, Wilson lower
   bound of the ignore rate at least 0.5) extinguishes the route. Reproduced: `adage condition --instincts-only`
   over a copy of the click v2 home prints `dead-end-0cdcfb: extinguished automatically (ignored 10 of
   10 times)`, and the learn.log summary reads "agents followed it and it failed 0 times, or ignored".
   (b) The status card and ledger say "0 followed". (c) The savings estimate and learning treat it as
   useless. Any directed runner that agents narrow with a path (pytest, `go test`, `cargo test -p`, vitest)
   is affected.
2. **Failure-triggered routes never see a failure that went through a pipe.** Only `Event::Failure`
   (Claude Code `PostToolUseFailure`, non-zero exit) checks `after_failure` triggers
   (crates/adage-runtime/src/lib.rs). `python -m pytest … 2>&1 | head -100` exits 0 even when it prints
   `No module named pytest`. That was the only form of the dead end in 22 of 63 Haiku sessions that hit
   it, in 5 of 6 Sonnet 5 sessions, and in the zero-config E2E's one opportunity. Signatures in a
   successful call's output are never checked.
3. **E1 cannot be approved for any lookup the learner produces.** `crates/adage-exec/src/route.rs`
   accepts only `SlotSource::Prompt { span }` at E1; the graph engine (the one learner) emits
   `SlotSource::PromptValue { vtype }` with the same anchored pattern contract. `adage review delegate
   … --level e1` refuses with `slot {item} has no E1 form (a prompt value with a pattern contract)`,
   which describes exactly the slot it refuses. Rewriting the slot to `from: prompt, span: ticket`
   makes approval and delivery work.
4. **E1's default typed output drops what the agent needs.** The projected fields are the linked
   files only (`{"files":["acme/customers.py"]}`); the work item's title, description and acceptance
   text are not delivered, so agents still ran the lookup in 3 of 3 sessions and no turn is saved.
   Free text needs `--raw`, which policy disallows by default.
5. **U11 covers only the test runner.** A `python -c "import click"` or `python repro.py` that fails
   with `No module named 'click'` (4 of 6 zero-config sessions started this way) gets nothing; the
   agent `pip install -e .`s into the system or user site instead.
6. **A housekeeping command became a learned reflex.** In the unconfined zero-config run, after session 2:
   `run-3961d0`, firm, prime, a context trigger ("after `source {path}` and `cat`") directing `git log
   --oneline -5`, from 8 of 9 traces over one day. The changelog says housekeeping commands never
   become reflexes.
7. **Instincts bind against whatever checkout the traces name.** Learning over the RepoBench training
   transcripts, whose checkouts had since lost their `.venv`, bound U11 to `uv run pytest` in click
   and nothing at all in attrs (not even U6). Bound against a checkout with its `.venv`, the same
   repositories bind 14–15 routes. A user whose sessions run in per-task worktrees or containers gets
   the stale checkout's instincts.
8. **(harness) `evals/live/runner.py` `obligation_met`** credits only a regeneration the agent ran,
   so an applied E2 action counts as "not met" even though the generated code is fresh.
9. **Status card noise.** On a machine with Codex installed, a Claude-plugin-only user sees a
   persistent `● check: hooks: codex … not registered` warning.
10. **The ledger's cost is about 10% below the harness's** `total_cost_usd` (−12% and −10% on the two
    sessions compared). It is labelled an API-list estimate, but the gap is not explained anywhere.
11. **The bootstrap session never counts toward savings**: it has no `session_start` record, because
    the repository had no index when it started. In the confined E2E the card's session count lagged
    one session behind.

(Also found, in this evaluation's own harness: an unconfined E2E agent `pip install`ed pytest and an
editable click into the container's system Python mid-run. The affected RepoBench runs were discarded
and rerun, and the E2E driver now has `--confine`. `evals/repobench/rerun.py` is new.)

## Known limitations

1. **Learning from real traces yields nothing on ordinary repositories.** 0 reflexes in 4 of 4
   RepoBench repositories (default and `--demo` thresholds), 0 from 58 real click sessions in the E2E,
   4 of 12 corpus datasets learn nothing. Agents vary their test commands (paths, flags) and the
   compiler needs one dominant literal. Until that changes, Adage on a real repository is its
   instincts.
2. **Instincts help only where agents hit a known dead end.** On the three Python repositories that
   is the `.venv` dead end (hit in 121 of 132 sessions, both arms). On vuejs/core nothing fired, and
   on urfave/cli (v1) nothing but a `go.sum` advisory was bound. Its value is about two tool calls.
3. **Effects on generated sandboxes do not transfer.** The −15.5 turns / −48% cost of the first
   corpus came from conventions designed to be undiscoverable; with a CLAUDE.md they shrink to ~1
   turn.
4. **E1 is blocked for learned lookups (bug 3), and with its default typed output it saved nothing (bug 4).**
   E2 works but is a beta behind policy and needs a person at a terminal for every approval.
5. **Measurement power.** 30 tasks × 2–3 repeats detects only effects of several points of resolve rate
   or several cents per task; a 1-turn effect is invisible at session level.
6. **Plugin-only users have no `adage` command.** The notices point to `adage review`, and the status
   card, delegation and `adage train` need a separate install (brew, npm, or the shell installer). The base
   plugin bundles only `adage`, not `adage-train`.
7. **Savings need time.** A dollar figure appears only after 10 control and 30 treated sessions. In
   headless sessions a session counts only after its idle timeout, because no stop record is spooled.
8. **Sonnet-class models pipe their test output** (5 of 6 dead ends), so failure-triggered instincts
   rarely reach them until bug 2 is fixed.

## What a user should expect

Claims we can make today:

- "Install the plugin and start a session: Adage stages its own verified binary, learns from your
  past sessions in that repository in the background, and says so in one line. Nothing is downloaded
  and nothing runs without you."
- "On Python repositories with a project `.venv`, when your agent runs the tests outside it, Adage
  tells it to use `.venv/bin/python -m pytest`; agents followed that every time in our real-repository
  benchmark and reached a working test run about two tool calls sooner." (True once bug 1 is fixed;
  today the instinct switches itself off after about five uses.)
- "On repositories where agents repeatedly need something they cannot discover (a private lookup, a
  regenerate step no file mentions), Adage learned it from about 9–12 sessions and cut turns roughly in
  half on our generated benchmark." (Generated sandboxes only; say so.)
- "No measurable harm": no resolve-rate or cost penalty in 196 real-repository sessions, and 0 false
  learned routes on 15 datasets at default thresholds.
- "`adage train` checks a skill or runbook against real agent runs and tells you which instructions
  are stale" (contradicted and compiled verdicts reproduced with real sessions for $0.84).
- "Approved regenerate steps (E2) run staged and verified from the hook, and the agent is told what
  changed."

Claims we cannot make:

- Any saving in dollars, turns or time on real repositories. None was measured: $ per resolved task
  $0.47 vs $0.47.
- "Makes models more capable." No resolve-rate improvement anywhere on real code (0.62 vs 0.65).
- "Learns your repository's conventions from your traces." Not on any real repository tested.
- "Reflexes arrive within a minute" as a promise of directives. The notice says it, and in 6 of 6
  E2E sessions no directive arrived.
- "E1 prefetch saves a turn." It cannot be enabled for learned lookups, and where forced on it did not.
- Anything about Sonnet-class models beyond "no harm measured".

## Pre-launch checklist

| Item | State | Decision needed |
| --- | --- | --- |
| **Bugs 1–3** | open | Fix before any launch: 1 (follow judge / auto-extinguish) destroys the only delivered value; 2 (piped failures) hides a third of it; 3 makes `delegate --level e1` unusable on learned reflexes |
| **Re-measure** | – | RepoBench v2 on the same roots after the fixes (`evals/repobench/rerun.py`; about $50 Haiku) |
| **DCO** | CONTRIBUTING requires a `Signed-off-by` on every PR commit and CI checks PR commits; **no commit in the history is signed off** | Decide: (a) declare the pre-launch history the owner's own work (single author) and require sign-off only from the first external PR, stating it in CONTRIBUTING; or (b) rewrite history with `git rebase --signoff` before making the repository public. (a) is simpler and standard |
| **Distribution plan** | dist 0.32 (`dist-workspace.toml`): five targets, shell/PowerShell/Homebrew installers, Sigstore attestations, CycloneDX SBOMs; npm through `publish-npm.yml`; plugins through `publish-marketplace.yml` | Create the `HOMEBREW_TAP_TOKEN` secret and `stevenvaughanbooth/homebrew-tap`; decide on macOS signing (D4 is off: unsigned binaries, fine for brew/npm/plugin copies, not for downloaded archives); do one dry release tag on a fork |
| **npm name** | `@adage/cli` (`npm/config.mjs`, decision D3), `adage` is taken | Claim the `@adage` scope (`npm org create adage`) now; if unavailable set `PACKAGE_NAME = "adage-cli"` (the one place). Set `NPM_TOKEN` and `ADAGE_NPM_PUBLISH=true` |
| **Marketplace branch** | `.claude-plugin/marketplace.json` installs both plugins from `ref: marketplace` (`git-subdir`); the branch is written only by the release workflow, so **it does not exist until the first release** and `/plugin install adage@adage` fails before then | Tag the release before announcing; verify `/plugin marketplace add stevenvaughanbooth/adage` then `/plugin install adage@adage` on macOS and Linux from the published branch (this evaluation verified the staged layout with a Linux gnu build standing in for musl) |
| **Plugin users and the CLI** | The plugin puts no `adage` on `PATH`; notices send users to `adage review` | Either bundle a `bin/` the user can add to `PATH`, or make the notice name the install command |
| **Claims** | The README's results are the demo sandboxes' (acme-billing, harbor-dispatch: conventions designed to be undiscoverable, pre-audit protocol, with a caveat), its example status card shows a savings line (`~$0.034 and 3.1 turns per session`) that no real repository has produced, and it does not mention the RepoBench results; the notice says "reflexes arrive within a minute" | Lead with the real-repository result and the claims under "What a user should expect"; label the example card as illustrative |

## Spend

| Item | Sessions | USD (`total_cost_usd`) |
| --- | ---: | ---: |
| RepoBench v2, Haiku 4.5 (click, attrs, marshmallow, vuejs/core; held-out) | 164 | 48.69 |
| RepoBench v2, Sonnet 5 slice (click, vuejs/core) | 32 | 18.34 |
| RepoBench v2 runs discarded after the system-Python contamination (rerun) | 37 | 6.48 |
| Sessions killed mid-run for that cleanup (no result record; estimated) | ~6 | ~0.60 |
| Zero-config E2E (2 × 3 sessions) | 6 | 1.35 |
| Delegation: acme training (12), E1 check (6), E2 check (2) | 20 | 1.53 |
| `adage train` real run | 30 | 0.84 |
| **Total** | **~295** | **~77.83** |

The Sonnet slice stayed under its $30 cap. RepoBench reused v1's training sessions ($7.50 of v1
spend, not repeated).

## Files

- [evals/bench/results/2026-09-29-repobench-v2](../bench/results/2026-09-29-repobench-v2/README.md):
  RepoBench v2 tables, charts, `results.jsonl` (schema v2, L3).
- `data/rb-compare-*.jsonl`: v1 and v2 per repository and arm (`rb_compare.py`).
- `data/deadends-v{1,2}.jsonl`: per session, U11 dead ends, visibility, recovery and delivered routes
  (`deadends.py`); `mechanism.py` computes the task-clustered mechanism table from them.
- `data/zero-config-*.jsonl`, `data/zero-config-adage-*.txt`: the E2E sessions and status cards.
- `data/delegation-sessions.jsonl`, `data/train-report.json`, `data/routes-table.md`.
- Configs: `evals/repobench/configs/v2/`, `configs/acme-e1.json`, `configs/gen-e2.json`. Raw run roots
  (transcripts, sandboxes) are not committed.
