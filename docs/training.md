# Training: skill → reflex

`adage train` turns what a skill *says* into reflexes Adage can *prove*. It reads a `SKILL.md` (or a
runbook, `CONTRIBUTING.md`, a Makefile, `package.json` or a GitHub Actions workflow), extracts its
claims, drives **your own agent harness** headlessly on generated tasks in disposable clones of
your repository, learns from what the agents did with `adage condition`'s graph engine, and reports, claim by
claim, whether the skill compiled into a reflex, is stale, was never exercised, or is unsafe.

Terminology:

| | Where it comes from | Evidence |
| --- | --- | --- |
| **Reflex** | Conditioned on traces: your agents' (organic), or `adage train`'s (synthetic) | Distinct sessions, Wilson lower bound |
| **Instinct** | Prebuilt route bound from the repository's declared files ([instincts.md](instincts.md)) | None: a capped default |
| **Skill** | Text a person wrote | None until `adage train` tests it |

**Principle.** The `adage` binary, which hooks run, never executes anything (Design principle 1).
`adage train` runs `adage-train`, a separate, optional binary (the `adage` binary never drives an
agent session itself). It is the offline eval loop of Design §12 for one skill:
it runs *agents*, sandboxed, only when a person at a terminal explicitly asks (`--execute`), with a
spending cap. Nothing it produces reaches a hook until a person reviews it.

## Modes and how they relate

- **Skill → reflex** (this document). A skill is a hand-written hypothesis about good routes
  (Design §10.10, bridge 3). Training exercises it and keeps what the agents' behaviour supports.
- **Declared facts → instincts** ([instincts.md](instincts.md)). The same facts (`Makefile` targets
  and prerequisites, `package.json` scripts, CI `run:` steps, generator links) bind instincts on
  day 0 with no sessions at all. `adage train plan --from Makefile` reads them through the same
  extractor (`adage_instincts::extract`), so a declared step can be *tested* rather than assumed.
- **Ids line up.** A learned route takes the id the learner derives (`finish-line-<glob, command>`,
  `dead-end-<working command>`), which is also the id an instinct binds for the same behaviour.
  On the acme fixture training stages `finish-line-e7e809` and `dead-end-d41b1e`, the ids the demo
  learned and instinct U5/U1 bind. Once installed, a trained reflex therefore overrides the
  instinct with its id, as a learned route does.

## The pipeline

### 1. `plan` (free)

**Claims** are extracted deterministically (no LLM):

- Markdown: commands, paths and globs from fenced shell blocks and inline code. An inline command
  counts only in an imperative sentence (run, use, always, never, after, before, instead of,
  install, …); a shell block counts as the instruction of the sentence before it. Front matter is
  skipped (its `name:` names the skill).
- Makefile / package.json: `adage_instincts::extract` (bodies expanded, verdicts, generator inputs).
- Workflows: `run:` steps (`adage_instincts::declared::run_steps`); a regenerate step takes its
  inputs from the declared generator links.

Each claim is typed:

| Type | Read from | Exercised by |
| --- | --- | --- |
| `verify` | a verify-class command | "check / fix the tests" prompts |
| `regenerate_after(glob)` | a regenerate-class command in a sentence with *after / whenever / when you* and a path or glob | a change to a real file matching the glob |
| `lookup(tool, id example)` | an observe-class command carrying an id or placeholder, or in a look-up sentence | prompts carrying the example id |
| `avoid(x → y)` | two commands with *instead of*, *never*, *don't*, *fails* | "check the tests" prompts |
| `environment` | anything classifying above regenerate | never |

Every command is normalised by adage-core's shell normaliser and effect-classified. A claim whose
directed command classifies above *regenerate* (environment, author, publish), that is not one
plain command, or that writes, redirects or pipes (other than a regenerate step's own output) is
**unsafe**: it is reported and never exercised.

**Tasks** are generated from a seed: trigger tasks per exercisable claim (three templates × two
verbosities per type, plus neutral lead-ins), about 70% *implicit* (never naming the skill or the
command) and 30% *explicit*, and at least 25% *controls* outside every claim (files no claim's glob
matches; questions that need no build). A claim with nothing to exercise (no matching file, no
example id) gets no tasks and is reported as never exercised, with the reason.

**Budget estimate**, printed as its factors: sessions × an estimated cost per session for the model
family (Haiku $0.15, Sonnet $0.25, Opus $0.75, other $0.50: rounded up from the demo runs), plus
three sessions per held-out validation task. It is marked as an estimate.

### 2. `run` (dry run by default; `--execute` spends money)

Without `--execute`, `run` prints every session (arm, claim, prompt), the exact command line of one
session per arm, the environment that passes through, the settings files, the layout, the budget
cap and the estimate, and exits. Nothing is written.

With `--execute`:

1. **Exploration.** One session per planned task, with the skill text appended to the system
   prompt (`--append-system-prompt`) and no hooks (`ADAGE_DISABLE=1`).
2. **Mining.** The exploration transcripts go through `adage_learn::learn` with the repository key
   forced to yours (`repo_override`, what `adage condition --repo` does) into a *separate* staging
   directory, `staged/tracks`, with provenance `synthetic:skill:<sha256 of the source>@<run>` on
   every track and route, and a sidecar `staged/provenance.json`. Then the synthetic cap is applied:
   no route is fire and none is beyond forming. **Staged reflexes are not activated**: nothing
   is written to `$ADAGE_HOME/tracks` or `$ADAGE_HOME/index.json`.
3. **Validation** (`--validate N`). N held-out tasks from another seed (no prompt of the
   exploration set), each in three arms, in a seeded order:
   - `baseline`: no skill, no hooks;
   - `skill`: the skill appended, no hooks;
   - `reflex`: no skill; Adage's hooks on (`adage hooks-config`), `ADAGE_HOME` pointing at
     `reflex-home/` which holds only the staged reflexes, `ADAGE_CONTROL=0`, `ADAGE_AUTO_LEARN=0`.
     A clone has no `origin`, so hooks inside it compute the key `<name>@<root commit>`; the
     reflex home's copy of the staged tracks is re-keyed to it (from a probe clone).

### 3. `report`

Per claim, one of:

| Verdict | Meaning |
| --- | --- |
| `compiled → route <id>` | a staged reflex does what the claim says (same glob and command; same working command; same lookup tool) |
| `contradicted` | the skill says X; X failed in at least 9 in 10 of the exploration sessions that ran it, and a dead end X → Y was learned: **your skill is stale** |
| `never exercised` | nothing could trigger it (no matching file, no example id, no session gave it a chance) |
| `not compiled` | exercised, but no reflex met the learning thresholds (e.g. a `verify` claim that sessions ran, but not after their last edit, or as often without editing anything). A plain verify claim ("run `make check` before finishing") compiles to a check owed after any edit when the exploration sessions ran it after their last edit (`adage_learn::mine::claimed_verify_route`) |
| `unsafe` | above regenerate, or not one plain command; never exercised |

Plus sessions by arm, skipped (budget) and failed sessions, reported cost against the cap, the
staged routes, and with validation the **acceptance table**, per claim and arm: sessions,
applicable sessions, followed share, mean turns, mean cost. A reflex is **accepted** when, on the
trajectory oracle, the reflex arm follows it in at least 70% of applicable held-out sessions, is
within 10 points of the skill arm, and its mean turns and cost are no more than 10% worse than the
skill arm's. For a contradicted claim, "followed" means the session's first verification was the
working command and it passed.

The trajectory oracle reads each transcript exactly as `adage condition` does (`adage_learn::read_session`):
`regenerate_after` is followed when a successful run of the command comes after the last edit to a
matching file; `verify` when the command ran and passed; `avoid` and contradicted claims when the
first verification is the working command and passed; `lookup` when the tool ran with the prompt's id.

## Safety model

| Control | How |
| --- | --- |
| Only a person starts agents | `--execute` with a harness that spawns agents refuses when `CLAUDECODE`, `OPENCODE`, `CODEX_SANDBOX`, `CODEX_SANDBOX_NETWORK_DISABLED`, `ADAGE_ARM` or `ADAGE_TRAIN_SESSION` is set, or stdin is not a terminal (`ADAGE_ALLOW_NONINTERACTIVE=1` lifts only the terminal check, as for `adage share`). It also needs an explicit `--budget`. Training sessions carry `ADAGE_TRAIN_SESSION`, so an agent inside one cannot start another run. |
| Disposable clones | `git clone --local --no-hardlinks` of HEAD into `$ADAGE_HOME/train/<run>/wt/<job>/<repo name>`, then `git remote remove origin`: nothing can be pushed from it, and your working tree (and uncommitted changes) is never touched. Removed after the session (an empty `<name>/.git` is left so learning resolves repository-relative paths at the clone's root); `--keep` keeps it. |
| Isolated harness state | Each session has its own `HOME`, `CLAUDE_CONFIG_DIR`, `XDG_*`, `CODEX_HOME` and `TMPDIR` under `homes/<job>`: no auto-memory, settings or earlier transcripts of yours, and none of theirs left behind. |
| Environment allowlist | Only `PATH`, locale, `TERM`, `SHELL`, `USER`, `LOGNAME`, `TZ`, proxies and CA bundles, plus the harness's own auth variables (`ANTHROPIC_API_KEY`, `CLAUDE_CODE_OAUTH_TOKEN`, `ANTHROPIC_AUTH_TOKEN`, `ANTHROPIC_BASE_URL`). Without one of those, a copy of `~/.claude/.credentials.json` is put in the session's config directory and removed right after it. `--forward-env A,B` passes further named variables (Bedrock, Vertex); `--host-auth` passes a host-managed login's (the session-ingress token file, organisation and account ids, `CLAUDE_CODE_PROVIDER_MANAGED_BY_HOST`, base URL, proxies, CA bundles), and then no credentials file is copied. An agent session's own markers (`CLAUDECODE`, any `*SESSION_ID*`, `ADAGE_*`) are refused. |
| Tools | `--allowedTools Bash Read Edit Write Glob Grep`, `--disallowedTools WebFetch WebSearch Task Agent`, `--permission-mode dontAsk` (anything not pre-allowed is denied, never prompted), `--strict-mcp-config` with no MCP config (no MCP servers). |
| Sandbox | The settings file each session loads sets `sandbox.enabled: true`, `autoAllowBashIfSandboxed: true`, `allowUnsandboxedCommands: false` and denies `WebFetch`/`WebSearch`. `--container none` is the only container mode. |
| Budget | Sessions stop *starting* once the harness-reported cost reaches `--budget`; each session also gets `--max-budget-usd` equal to what remains when it starts. With `--concurrency N`, the overshoot is at most the N sessions already running, each capped as above. Never more than N sessions at once. A per-session wall-clock `--timeout-secs` (default 1800) kills a hung session. |
| Transcripts | Copied out of each session's own config directory into `$ADAGE_HOME/train/<run>/transcripts/`, never into `~/.claude/projects` or any directory `adage condition` or live learning reads. `run` refuses an `ADAGE_HOME` inside the Claude Code projects directory. |
| No influence on live learning | Exploration, baseline and skill sessions run with `ADAGE_DISABLE=1`; the reflex arm with `ADAGE_AUTO_LEARN=0` and its own `ADAGE_HOME`. |
| No activation | Staged reflexes live under `staged/`; the reflex arm reads a copy in `reflex-home/`. Nothing is installed. |
| Untrusted input | The source file is read-only input (at most 1 MiB); repository reads go through `adage_instincts::files::RepoFiles` (bounded, no symlinks out). Mined routes pass `adage_tracks::io::validate` like any learned route. |

The repository's own committed project settings (`.claude/settings.json`, including any hooks it
declares) still load (`--setting-sources project,local`), as they would for your own agent.

## Evidence-origin policy

Where a route's evidence comes from caps how strongly Adage may direct it. The cap for *synthetic*
is enforced in staging today; the others describe the policy the model changes below make
enforceable in the index.

| Origin | Source | Max strength | Counts toward organic support | Activation |
| --- | --- | --- | --- | --- |
| organic | your agents' own sessions (`adage condition`, live learning) | fire (within the effect ceiling) | yes | automatic, reviewable |
| synthetic | `adage train` sessions (`synthetic:skill:<sha>@<run>`) | prime (a dead end stays inhibit: the model requires it of an avoid step) | never | after review, and preferably an accepted validation |
| declared | a repository's declared facts, as `adage train plan --from Makefile` exercises them | prime | never | after review |
| instinct | prebuilt, bound from declared facts (`instinct:core@<v>`) | prime, never a Stop block | never | automatic forming (environment steps: nascent) |
| imported | a teammate's signed track (`adage add`) | prime until first-hand evidence | no | forming; strengthening needs local evidence |
| demonstrated | a person doing the task themselves in a recorded session | fire after review | yes, once reviewed | review |
| corrected | a person's decision in `adage review` (authored, pinned, strengthened) | as decided, within the effect ceiling | n/a | immediate |

## CLI reference

```text
adage train plan --from <SKILL.md|CONTRIBUTING.md|Makefile|package.json|.github/workflows/x.yml|runbook.md>
                 [--repo <path>] [--seed N (1)] [--sessions N (24)] [--model ID (claude-haiku-4-5)]
                 [--json] [--out plan.json]

adage train --plan plan.json [--harness claude-code|fake] [--fake-world world.json]
                [--model ID (the plan's)] [--budget USD] [--max-turns N (30)] [--concurrency N (2)]
                [--validate N (0)] [--oracle trajectory] [--container none] [--run-id ID]
                [--adage PATH] [--thresholds default|demo] [--timeout-secs N (1800)] [--keep]
                [--json] [--execute]

adage train report <run id | run directory> [--json]
```

- `--repo` names any path inside the repository (its git root is used); by default, the source
  file's repository.
- `--adage` is the `adage` binary whose hooks the reflex arm runs; required with `--validate` and a
  real harness.
- `--thresholds demo` uses `adage condition --demo`'s lower evidence bars (min support 3), for small runs.
- `--harness fake --fake-world world.json` runs the whole pipeline with the scripted agent the
  tests use (below), spending nothing; it needs no terminal.

Run layout, under `$ADAGE_HOME/train/<run>/`: `plan.json`, `state.json` (every session: arm, task,
status, session id, turns, cost; spend; staging), `settings/`, `sessions/<job>/stream.jsonl`,
`transcripts/<phase>/<job>.jsonl`, `staged/{tracks/, index.json, provenance.json}`, `reflex-home/`,
`report.json`.

## The fake harness

`Harness` is a trait (`crates/adage-train/src/harness.rs`): `ClaudeCode` drives `claude -p`; `Fake`
(`fake.rs`) writes scripted Claude Code JSONL transcripts into the session's
`CLAUDE_CONFIG_DIR/projects/<project>/<session>.jsonl`, exactly where Claude Code would, so
collection, mining, the oracle and the report run unchanged. Its agent follows a small policy over
a *world* file (what each command prints and whether it fails): it knows the skill's claims (skill
arms), or the reflexes in the index its hooks would read (reflex arm), or nothing (baseline: it
tries `naive_verify` first and runs a needed regenerate step `baseline_regen_percent` of the time).
See `crates/adage-train/tests/fixtures/world.json`.

On the committed fixture (the acme sandbox, a `SKILL.md` that says "run the tests with
`python3 -m unittest`" and "after editing `schema/*.json`, run `make gen`"), 24 exploration and
9 × 3 validation sessions give:

```text
c1  verify: `python3 -m unittest`                contradicted: failed in 12/12 sessions; learned dead end
                                                 dead-end-d41b1e → `make test` (your skill is stale)
c2  regenerate_after(schema/*.json): `make gen`  compiled → route finish-line-e7e809
c3  lookup: `python3 scripts/wi-lookup`          compiled → route lookup-work-item
c4  regenerate_after(proto/*.proto): `make gen`  never exercised (no file matches `proto/*.proto`)
c5  environment: `pip install -r requirements.txt`  unsafe
acceptance: c1, c2, c3 accepted (c1: skill arm 0%, reflex arm 100%, 2.0 vs 3.0 turns)
```

## What is verified, and what is not

Verified here (`cargo test -p adage-train`): claim extraction and typing; task generation (seeded,
≥ 25% controls, ~30% explicit, implicit prompts never naming the skill or command, held-out prompts
never repeating); the dry run writes nothing and shows the exact command lines; the refusal inside
an agent session and without a terminal (library and CLI); the environment allowlist; clone,
isolation, budget stop, transcript collection from the session's config directory; mining into
staging under the forced repository key with the synthetic provenance and cap; the reflex home's
re-keying (the fixture repository has an origin, clones do not); the report verdicts and the
acceptance table; the same plan staging the same routes on a second run.

Not verified (no real agent was run; no money was spent):

- **Claude Code's flags** were checked against `claude --help` of 2.1.283 (`-p`, `--max-turns`,
  `--max-budget-usd`, `--output-format stream-json`, `--verbose`, `--setting-sources`,
  `--strict-mcp-config`, `--settings`, `--permission-mode dontAsk`, `--append-system-prompt`,
  `--allowedTools`, `--disallowedTools`) but not run.
- **The sandbox settings** (`sandbox.enabled`, `autoAllowBashIfSandboxed`,
  `allowUnsandboxedCommands`) follow Claude Code's documented settings; whether the sandbox engages
  headlessly on a given machine (bubblewrap on Linux, Seatbelt on macOS) was not tested. A sandbox
  that blocks the repository's own build tools will produce failures that mine as dead ends: read
  `contradicted` verdicts with that in mind, and check them with `--validate`.
- **Cost accounting** trusts `total_cost_usd` from the stream's `result` record, as
  `evals/live/runner.py` does.
- **The reflex arm with real hooks**: `adage hooks-config` output and `ADAGE_HOME` re-keying are the
  runner's proven path, but not exercised end to end here.
- Outcomes are not scored (no hidden checks): `adage condition` receives no outcomes, so finish lines
  have no `deviations_failed`.

## Follow-up model changes

1. **`Evidence.by_origin`** (roadmap E1; also [instincts.md](instincts.md), adage-tracks item 5):
   counts per origin (`organic`, `synthetic`, `declared`, `instinct`, `imported`, `demonstrated`),
   with `support`/`opportunities` organic only. Until then synthetic evidence is marked only by the
   `provenance` string (`synthetic:skill:<sha>@<run>`, on the track and on every route) and the
   sidecar `staged/provenance.json`.
2. **`TrackInfo.origin`** (or treating a `synthetic:` provenance like `imported` in
   `Index::compile`): first-hand organic routes must rank before synthetic ones with the same id,
   and the strength cap above must be enforced at compile time, not only in staging.
3. **Strengthening from staging**: an `adage review`-style step that installs accepted staged reflexes
   into `$ADAGE_HOME/tracks/<repo>/…` forming, keeping the provenance, and records the
   decision. Not built: today staged reflexes are only inspected (`staged/tracks`, `report`).
4. **A verify step in general**: outside a claim (`adage_learn::claims`), a correct `verify` step
   followed without failures compiles to nothing, because the learner learns "the tests here run with `make test`" except through a dead
   end or an intent family.
5. **Sandbox-aware dead ends**: record whether a failing session ran sandboxed, so a dead end that
   exists only inside the sandbox can be told apart.
6. **Other harnesses**: a `Harness` for Codex (`codex exec`) and OpenCode (`opencode run`); their
   transcripts are already read by `adage_learn::read_session`.
