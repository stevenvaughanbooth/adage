# Adage

**Agent traces become reflexes.**

Adage reads the traces your coding agents leave behind (Claude Code, OpenCode and Codex session
transcripts), finds recurring procedures, and compiles them into **reflexes**:
small, typed, deterministic responses with their evidence attached. At each decision point, the
matching reflex can offer learned guidance or correct a supported command to the repository's
declared environment. With separately approved execution, it can perform a routine lookup or
staged action on the CPU. The aim is to reduce repeated work while leaving new reasoning to the
agent; savings depend on the repository and the matched procedure.

[How Adage works](docs/how-it-works.md) is the current behavior reference: what runs automatically,
what requires approval, and what the evidence does and does not establish.

Learning accumulates across sessions and is intended to generalize to later tasks. The graph
engine already models related event sequences; connecting those predictions broadly to automatic
actions remains an active launch gate. See [current capabilities and limits](docs/how-it-works.md#accumulated-prediction-and-current-execution-limits).

Reflexes come from three places:

- **Traces.** `adage condition` mines real traces in a repository.
- **Instincts.** Reflexes that ship with Adage and work on day 0, read from the repository's own
  Makefile, `package.json`, CI and lockfiles, with no history needed. A learned reflex always
  overrides the matching instinct.
- **Skills.** `adage train <SKILL.md>` compiles a skill's procedure (which command, after which
  edit, in which order, never which runner) into one reviewed pack of reflexes, deterministically
  and for free, and replays every compiled command on a clean checkout in a sandbox: a stale
  instruction is dropped and reported, and the review says which steps stay text and why. `adage
  train accept` installs it; the agent then follows the procedure without the skill in context.

Reflexes live in **tracks** on disk, and travel as signed, closed **packs** you can review, commit
and share. By default, reflexes advise the agent and supported pre-call instincts can rewrite its
tool arguments; the harness executes those calls under its own permissions. The `adage` binary starts no process on the
agent's behalf (it starts only itself, to condition in the background, and `adage-train` when you
ask it to train; a test pins this). Execution is a separate, opt-in binary (`adage-exec`, turned
on with `adage delegate on`) that runs only reflexes you approved on that machine (`adage review
delegate`), inside an OS sandbox: E1 prefetches read-only results, while E2 stages actions and
applies only permitted outputs. Selecting and running such an invocation needs no model request;
the next model request may still consume its result.

The learner also retains **predictive execution candidates**: commands supported by recurring
edit or multi-event contexts across sessions, even when reminding the agent would add no value. They appear as needing execution approval
in review and stay silent without a valid local grant. Accepting their evidence with ordinary
`adage review approve` does not authorize execution. Their evidence travels in packs; your local
execution permission does not.

For a learned lookup to replace an agent action, it must deliver the information the agent
would have fetched. Approved lookups can declare a bounded **complete stdout** contract: Adage
preserves the full text or withholds it on overflow, instead of silently returning a partial
task. Automatic argument binding also abstains when a prompt contains multiple distinct matching
values. See [complete lookup results](docs/execution.md#e1-prefetch-adage-exec-hook)
and the [action-elimination gate](evals/reflex-proof/harness/ACTION-GATE.md).

That [gate now passes](evals/reflex-proof/harness/results/20260929-learned-action-gate/FINDINGS.md):
on three held-out work-item triage tasks, automatic execution removed all three agent lookup
actions and reduced observed model requests from six to three, with all answers correct.
This is a small generated-repository mechanism proof, not a general coding speedup claim.
The [launch evidence index](evals/reflex-proof/LAUNCH-STATUS.md) tracks real-repository validation,
release gates, failed attempts and evaluation spending.
Across [two runs of four cold-start tasks](evals/reflex-proof/harness/results/20260930-cold-start-replication/FINDINGS.md)
on Marshmallow, shipped instincts passed **8/8 outputs versus 6/8** for the agent alone, with **8.8%
lower reported API cost** and **10.5% fewer observed model requests**. Three of eight pairs cost more
with instincts. The repeat alone saved only 0.6% and had equal request counts. This small,
single-repository result measures advisory value, not automatic CPU execution.
The latest [15-session real-repository learning pilot](evals/reflex-proof/harness/results/20260930-learning-curve-native/report.json)
passed independent task grading on 12/15 outputs but learned zero automatically executable actions.
Real-coding CPU savings remain unproven; the full result is retained alongside the mechanism proof.
A separate [five-session guided routine check](evals/reflex-proof/harness/results/20260930-guided-demonstrations/FINDINGS.md)
also produced zero executable candidates despite five eventual test-command successes. Its audit
identified a mining/admission gap and insufficient trigger-specific evidence; the launch gate remains open.
The [mining integration fix](evals/reflex-proof/harness/results/20260930-execution-prefix-integration/FINDINGS.md)
now preserves executable prefixes: replaying those five histories compiles nine candidates instead
of zero. Those five histories alone do not pass the unchanged execution evidence gate; compilation is not a savings result.
A [separately registered five-session follow-up](evals/reflex-proof/harness/results/20260930-cumulative-guided/FINDINGS.md)
then brings the retained history to ten sessions and certifies four triggers for one test routine,
with **9/10 qualifying continuations**. This connects real traces to an executable learned artifact.
The [completed eight-session CPU transfer study](evals/reflex-proof/harness/results/20260930-live-cpu-transfer/FINDINGS.md)
then executed that routine successfully in all four automatic sessions, whose outputs all passed
independent grading (baseline: 3/4). The agent nevertheless repeated the command in every automatic
session: **0/4 confirmed action eliminations**. Execution is proven in this pilot; replacement of
agent work remains the launch gate.

The current integration gives approved verification sessions a model-facing execution contract
and delivers completion receipts with journal identity, input digest and exit status. A small
authored diagnostic motivates this change; it does **not** establish real-repository savings
for the updated implementation. The [handoff](docs/launch-handoff.md) records the remaining tests.

```text
  agent traces ──► adage condition ──► reflexes ──► index ──► hooks
  + instincts                        (tracks,               ├─ guidance / supported rewrites
  + adage train                       packs)                └─ approved adage-exec actions
```

## Install

For a first session, expected results, and the separate approval path for CPU actions, follow
[Your first useful reflex](docs/first-reflex.md). The current release is a developer preview;
real-coding automatic action savings remain an open [launch gate](evals/reflex-proof/LAUNCH-STATUS.md).

In Claude Code:

```text
/plugin marketplace add stevenvaughanbooth/adage
/plugin install adage@adage
```

Then start a session. The release plugin carries checksummed binaries for macOS (arm64, x64),
Linux (x64, arm64) and Windows (x64), so nothing else is installed and nothing is downloaded.
Native Windows plugin hook invocation remains unverified; automatic execution requires macOS
or Linux (including WSL). At
the first session in a repository you see one line, which the agent never sees:

```text
adage: learning from 23 past sessions in this repo; reflexes arrive within a minute. Nothing runs without you.
```

In the background, Adage binds its instincts from the repository's own files (Makefile,
`package.json`, CI) and learns from this repository's earlier Claude Code traces, and only this
repository's: other repositories' transcripts are never opened. From the next prompt on, the agent
gets its reflexes. Adage keeps learning after every session, and at most once a day, only when
something is new, tells you in one line (`adage: 3 new reflexes active.`, or `1 install step waits
for you: run adage review in a terminal.`; with no `adage` on your `PATH`, as with the plugin alone,
the line names the plugin's own binary instead, so the command works as written).

From a terminal, `brew install stevenvaughanbooth/tap/adage` or `npm install -g @adage-sh/cli`
(**not published yet:** until the first release is out, the tap and the npm package do not exist;
build from source with `cargo install --locked --git https://github.com/stevenvaughanbooth/adage
adage-cli`), then run `adage init` inside your repository to connect a detected agent and learn
from its previous sessions. For scripted setup, use `adage install claude-code --write --scope user`
(or `codex`, `opencode`) and `adage condition --repo-root .`. Plain `adage`, anywhere (a terminal, a pipe, or
`!adage` inside Claude Code), prints where things stand. An illustrative card (the numbers are made
up to show every line; measured numbers are in [Benchmarks](#benchmarks)):

```text
adage 0.1.0 · github.com/acme/billing                          ● healthy
  14 reflexes active: 4 instincts, 9 learned, 1 from your team
  learning after each session · last: 2 min ago, 1 change
  last 30 days: 61 sessions · 188 directives · 152 followed · 7 dead ends headed off
  savings: ~$0.034 and 3.1 turns per session, API-list equivalent (95% CI $0.011–$0.058)
           ≈ $1.87 over 55 sessions, vs 6 where Adage stayed quiet (priced as of 2026-06-24)
  ended asking you for help: 3% with Adage, 5% without
  1 install step waits for you → adage review
  adage ui · adage map · adage doctor
```

**What is measured, honestly.** Adage stays quiet in 1 session in 10 (the control), so its effect
can be measured. Until a repository has 10 quiet sessions and 30 with Adage, the card shows counts
of what happened, never dollars (`measuring savings: 4 of 10 quiet sessions so far`). After that
it shows the saving per session with a 95% interval, or "no measurable difference yet", or, if
that is what the sessions say, "Adage appears to add ~$x per session". Dollars are API-list
equivalents from a dated price table (subscription users do not pay per token); a model not in
the table is shown as tokens and turns only. `adage savings --json` has the whole estimate.

**What never happens without you:** nothing is ever run on the agent's behalf unless you turn on
delegation at a terminal; install steps (environment changes) wait for `adage review`; no hook or
learner touches the network; settings files change only when you install the plugin or run
`adage install … --write` or `--remove` (which show the diff, ask before writing, keep the previous
file as `<file>.adage-bak`, and leave the rest of your settings as you wrote them).

**The one change Adage makes by default** (Claude Code): when a Python repository's own `.venv` was
verified to have pytest and the project, a test run or scratch repro the agent starts under the
system Python runs with `.venv/bin/python` instead. Only the interpreter word of a top-level
command changes, never text in a heredoc, a quoted string or a `-c` payload; a command that
activates an environment, `cd`s elsewhere, or pins another Python version is left alone. If your
permission rules allow the command as written but not rewritten, Adage advises instead of
rewriting. You see each rewrite, and the card counts them. A `pip install` of the project or its
test dependencies into the system Python gets advice naming the command that reaches the project;
nothing is refused. `adage delegate rewrites off` (or `adage config set runtime.rewrite false`)
turns this off, from a script too, and `adage delegate rewrites on` back on
([docs/execution.md](docs/execution.md#rewrites-to-the-projects-environment-pretooluse-u11)).

## What it does, concretely

From real traces in a repository, one learner, a graph engine
([Track G](docs/tracks/track-g-graph-engine.md)), finds the steps agents take together and
compiles them into reflexes. It has no list of kinds to look for: a reflex is whatever stimulus
reliably precedes a step, and how it is delivered follows from its shape. Reflexes it discovers
on real and generated repositories include:

| Reflex | Learned from | At runtime |
| --- | --- | --- |
| **Look the item up** | Agents passing a value from the prompt to the same command; the value's type (`W-2042`, `#482`, a UUID) is learned from the values themselves | fire/prime: "Work item details are looked up with: `python3 scripts/wi-lookup W-2042`" |
| **Read what it lists** | Agents reading, one turn at a time, the files a lookup listed | prime: "That result lists these files … read them together in one turn" |
| **Avoid a dead end** | A command that fails with the same signature, followed by one that works | inhibit: "Running `python3 -m unittest` fails (…). Use this instead: `make test`" |
| **Regenerate after an edit** | A regenerate step that follows edits to a path pattern | fire: after the edit, and once at Stop if still unmet: "changes to `schema/*.json` must be followed by `make gen`" |
| **Steps in order** | Commands run one after another after edits to a path pattern | fire/prime: "changes to `registry/*.csv` are followed by these steps, in order: `make gen`, `make check`" |
| **Check the part you edited** | A command naming the directory an edit touched | fire/prime: after an edit under `packages/ledger/`: `make -C packages/ledger check` |
| **Run the repository's checks** | The checks agents run after editing a kind of file, in the repo-wide form some of them ran (their `cargo test -p <crate>` and `cargo test --workspace --locked -j4` runs are one `cargo test --workspace --locked`) | prime: after an edit to `crates/**/*.rs`: `cargo clippy --workspace --all-targets --locked -- -D warnings`, and `cargo fmt --all` owed as a finish line |
| **Sync the environment** | A dependency step after a dependency file changes | prime: after review, once `requirements.txt` changes: `make deps then make check` |
| **Start like others did** | Prompts that mean the same thing whose sessions ran the same command first | fire/prime: "Requests like this one usually start by running: `make test`" (the embedding model is bundled) |

Every reflex carries its evidence (distinct traces, Wilson lower bound), an effect class that caps
how strongly it may be directed, and typed slot contracts: a value from the prompt reaches a directive
only if it matches the contract learned from real values (`^W-\d{4}$`), and file paths only if they
are safe, repository-relative, and exist.

## Benchmarks

Real Claude Code sessions (Claude Haiku 4.5 unless noted), headless, on real open-source
repositories, scored by hidden tests. Each Adage session is paired with a control session on the
same task: same repository, same model, hooks installed, directives withheld. Intervals are 90%
bootstrap intervals clustered by task. With 6–12 tasks per comparison, read them as closer to
80%. Every number links to its data and code. Where a result comes from a prototype rather than
the shipped binary, the text says so.

### Recurring work: the procedure a repository repeats

Most work in a repository is a variation of something done before: add a CLI option, add a
validator, add a flag. Adage groups those sessions into a **family** and compiles what worked into
a **procedure card**: a 350–700-token reflex listing the files edited, in order, with the lines
added last time, the test command that works, and the commands that kept failing. When a new prompt
matches the family (by local embedding similarity), the card is delivered once, at the prompt.

**The shipped product on five repositories (pre-registered showcase).** The product ran as a user
installs it. Its background learner formed each family's card by itself from 4 ordinary Haiku
sessions. Each Adage session was then paired with a control session on 8 held-out variants per
family, 2 repeats each.

- **Cost per task:** Adage cut it by **28.2%**, 90% CI [−31.2%, −25.0%], with Claude Haiku 4.5
  (80 pairs, 40 variants, 5 families; intention to treat). Tool calls were cut by 25.1%
  [−28.4, −21.6] and wall time by 16.8% [−19.7, −13.7].
- **Hidden tests passed: lower with Adage, 51/80 vs 59/80** (−10.0 points [−15.7, −4.3]). Almost
  all of it is one family, http-header (4/16 vs 14/16). Every failure there is the same check: the
  task asks for a test, and the Adage session did not write one. None of the 4 training sessions
  wrote a test either, so the card the product learned from them lists only `name.rs` and `mod.rs`.
  The card faithfully repeats an incomplete procedure. The other four families pass 47/64 vs
  45/64.
- **Delivery:** the card reached 80/80 Adage sessions.
- **False fires:** 3 of 30 same-repository negative prompts (upper bound 22.6%). None fired at the
  prompt; all 3 fired at the first edit of a card file.

| Family (repository) | Cost per task [90% CI] | Tool calls | Passed (control / Adage) |
| --- | ---: | ---: | --- |
| New parameter type (pallets/click, Python) | −19.4% [−24.1, −13.4] | −4.9% (no detectable change) | 13/16 / 14/16 |
| New validator (marshmallow, Python) | −30.5% [−35.5, −25.4] | −29.1% | 16/16 / 16/16 |
| New flag type (urfave/cli, Go) | −47.4% [−53.4, −40.6] | −39.8% | 1/16 / 1/16 |
| New standard header (hyperium/http, Rust) | −16.2% [−25.1, −5.8] | −29.3% | 14/16 / **4/16** |
| New middleware (go-chi/chi, Go) | −21.7% [−29.7, −13.4] | −20.9% | 15/16 / 16/16 |

- **Per-family intervals:** each rests on 8 variants (about ±10 points). The go-flag saving is
  partly control sessions hitting the 60-turn cap (5 of 16); without the 7 capped sessions, the
  pooled cut is 25.1% [−28.5, −21.6]. Both go-flag arms fail CI's approval step, which no training
  session performed.
- **Claude Sonnet 5 (directional, 12 pairs, same Haiku-learned cards):** cost −17.0% [−28.7, −2.9],
  12/12 vs 12/12 passed. Within that, mm-validator is −22.2% [−38.2, −0.7], and chi-middleware
  shows no detectable change (−6.8% [−19.9, +7.4]).
- **Spend:** $47.64 for 204 sessions, against a $60 hard cap.
- **Details:** see [evals/showcase/RESULTS.md](evals/showcase/RESULTS.md); machine-readable data is
  in [results.json](evals/showcase/results.json).

The research that led to cards (prototype, before the shipped product):

| Family (repository) | Card learned from | Cost per task | Tool calls | Hidden tests passed |
| --- | --- | ---: | ---: | --- |
| New parameter type (click) + new validator (marshmallow) | 4 earlier sessions | **−18.8%** [−25.8, −11.6] | −16% | 20/24 vs 19/24 |
| New flag type (urfave/cli, Go); card detected from the prompt, delivered by the hook | 4 earlier sessions | **−33.8%** [−44.3, −23.6] | −31.5% | 2/12 vs 2/12 |
| The same click and marshmallow pairs, Adage without cards (its instinct and a learned test reflex) | – | −8.8% [−14.6, −1.9] | −10.7% | 21/24 vs 19/24 |

- **Read the Go row with care:** half the control sessions hit the 60-turn cap, so part of the
  saving is Adage sessions not running out of turns. Both arms passed only 2 of 12. The failures
  were a docs-snapshot step no training session had done, so no card could contain it.
- **Detection:** 0 wrong deliveries on 53 unrelated prompts in the same repository (upper bound
  4.9%). The strict threshold misses rephrasings: 0 of 6 informally reworded tasks were recognised.
  A family's prompt examples grow from your own matched sessions.
- **How much history:** the shipped learner forms a card after one passing session when that
  session created no new files. Otherwise it needs two, which is the usual case: a new test file,
  or file names that vary per task (`flag_url.go`, `flag_port.go`).
- **What it is:** the card was a Python prototype in the rows above. It is now built into
  `adage condition` and the hooks, and on by default (`learn.cards`).
- **The shipped product, pre-registered:** a new family (add a URL utility to unjs/ufo,
  TypeScript) that no card research had used. The product ran as a user installs it; its
  background learner formed the card by itself from 4 sessions.
  - **Cost per task:** **−10.3%**, 90% CI [−20.1%, +2.0%], over 8 held-out variants × 2 repeats
    (16 pairs, 8 clusters).
  - **Tool calls:** −8.3% [−19.2%, +5.3%].
  - **Passes:** 16/16 vs 15/16.
  - **Delivery:** the card reached 16/16 Adage sessions and 0 of 8 unrelated prompts.
  - **One-off work:** on 6 click bug fixes, no false fires and no harm ($ +5.2%, passes 4/6 vs
    2/6).
  - **Verdict:** GO-WITH-CAVEATS under the pre-registered rules. The point estimate only just
    clears the −10% threshold, and the interval includes zero, so the saving is plausible but not
    demonstrated. The design had about 50% power at a true −20% effect. See
    [RESULTS.md](evals/launch/go-validation/RESULTS.md).
- **Data:** [evals/lab/research/recurring-workflows](evals/lab/research/recurring-workflows/README.md).

### Compiled skills: a skill's effect without its text in context

`adage train <SKILL.md>` compiles a skill's procedural steps into verified reflexes. It checks every
command by replaying it in a sandbox on a clean checkout, drops stale ones and keeps judgment as
text. The skill tested here is a repository workflow skill (look up the work item, regenerate after
schema edits, `make test`, `make ci` before finishing), run with the shipped `adage train`, 12
sessions per arm:

| | Neither | Skill in context | **Compiled by `adage train`** |
| --- | ---: | ---: | ---: |
| Hidden tests passed | 9/12 | 12/12 | **12/12** |
| Cost per session | $0.122 | $0.032 | **$0.034** |
| Turns | 27.3 | 7.3 | **7.9** |
| Input tokens | 564k | 103k | **109k** |

- **What it captures in this sample:** the compiled reflexes matched the skill's 12/12 pass count
  and captured about 98% of its observed cost saving. Twelve sessions per arm do not establish
  equivalence across tasks or repositories.
- **Why compile at all:** the compiled steps are verified, delivered only when they apply, and
  work across Claude Code, Codex and OpenCode.
- **The compiler catches stale instructions:** on a second skill, replay found that one of its
  instructions fails on a clean checkout and dropped it. With the skill in context, agents
  followed that instruction 8 times out of 8.
- **Limits:** the workflow repository here is a sandbox built for the lab, and a skill that is
  mostly judgment stays text.
- **Data:** [evals/lab/skills](evals/lab/skills/README.md).

### One-off bug fixes on real repositories

On novel bug fixes (RepoBench: click, attrs, marshmallow and vuejs/core, plus 9 more repositories
in Go, Rust, TypeScript and C), each task is new, so there is less to learn from past traces.

- **Instincts: the dead end.** Agents run tests under the system Python, where the project isn't
  installed. Adage rewrites the interpreter to the project's `.venv` before the call runs.
  - **Live micro slice** (6 click tasks): 0 of 6 Adage sessions hit that dead end, against 5 of 6
    without Adage.
  - **Held-out replay:** about 3.9% of cost per task.
  - **Learned, not written:** the same reflex is rediscovered from other repositories' traces with
    no hand-written rule.
- **Ceiling:** we measured every lever a hook can pull on these tasks: advice, memory of similar
  situations, file hints, output trimming and hook-run steps. Each is worth 0–4% of cost. The
  expensive part of a novel fix is reading and reasoning about new code, which past traces cannot
  shortcut ([research sprint](evals/lab/research/BRIEF.md), 19 studies, negative results included).
- **Pass rate:** Adage does not change it on these tasks; failed sessions are wrong fixes, not
  process. No harm was measured in 196 real-repository sessions. Those sessions predate the
  pre-call rewrite and default-on cards, which the live validation re-checks.
- **Stronger models gain less:** Sonnet-class agents already avoid most procedural waste.

**What this means for you:** these experiments identify recurring procedures as a promising target;
they do not establish a savings percentage for a new repository. The shipped-product recurring
task interval above includes zero. Direct CPU action elimination has its own
[launch gate](evals/reflex-proof/LAUNCH-STATUS.md), and real-repository coding results for that gate
remain pending. The status card measures your own sessions, with a control, before showing a
dollar figure.

The next [cross-session evaluation](evals/reflex-proof/harness/LEARNING-CURVE.md) measures learning
after 0, 3, 5, 10 and 15 related tasks, then compares four new tasks with no Adage, shipped instincts,
learned guidance, and approved CPU execution. This is a prospective protocol, not a measured savings
claim; correctness and duplicate agent work are part of its acceptance criteria.

## Results on generated sandboxes

Real **Claude Haiku 4.5** agents, headless, each in a fresh copy of a sandbox repository whose
conventions they cannot guess. Adage learned from 18 training sessions, then two learning-loop rounds
of held-out work items ran 18 sessions with Adage and 18 without (hooks active, directives withheld).
Hidden tests scored every run. Round 2 learned from every earlier session.

> **Caveat (2026-09-27 audit).** These runs predate stricter protocol: round 2 re-used the same six
> work items as round 1, arm order was fixed, both arms shared the operator's Claude Code
> auto-memory, and p-values treat 18 runs (6 tasks × 3 repeats) as independent. The effects are
> large enough to survive most of this, but treat the numbers as indicative until the runs are
> repeated under the corrected runner. See [docs/evals.md](docs/evals.md#known-limitations-of-the-published-numbers).

| Round 2, held-out | With Adage | Without | p (permutation) |
| --- | ---: | ---: | ---: |
| Turns before first edit (median) | **2** | 20 | 0.0002 |
| Seconds to first edit (median) | **12.0** | 42.5 | 0.0001 |
| Total turns (median) | **9** | 27.5 | < 0.001 |
| Cost per run (mean, USD) | **0.053** | 0.135 | 0.0001 |
| Hidden checks passed | **100%** | 78% | 0.11 (n.s. at n = 18) |

Round 1 showed the loop at work: removing exploration exposed a dead end (agents that no longer read
the Makefile tried `pytest` first: 1.44 failed runs per session). Round 2 conditioned it as an inhibit reflex
and it disappeared (0.00). Without Adage, 4 of 18 round-1 agents gave up asking what the ticket was.
Full data: [demo/results](demo/results/); interactive results page published as an artifact.

### A second demo: a stronger model, a different stack

Next, **Claude Sonnet 5** on **harbor-dispatch**, a TypeScript service on Node 26 whose conventions
differ from the first sandbox:

- incidents like `INC-94` resolve only through `bin/fetch-incident`;
- `npm test` fails and `npm run check` works;
- `registry/*.csv` edits need `npm run codegen`.

There were 84 sessions: 12 for training, then 36 with Adage and 36 without over two rounds. The run
cost $6.55 in total.

| Round 2, held-out | With Adage | Without | p (permutation) |
| --- | ---: | ---: | ---: |
| Turns before first edit (median) | **3** | 6 | 0.02 |
| Seconds to first edit (median) | **6.7** | 11.0 | < 0.001 |
| Exploration calls before first edit (median) | **2** | 4 | < 0.001 |
| Total turns (median) | **7** | 10 | 0.002 |
| Cost per run (mean, USD) | **0.065** | 0.086 | < 0.001 |
| Hidden checks passed | 100% | 100% | — |

Sonnet found every convention on its own:

- it ran the lookup unaided in every training session;
- it met the finish-line obligation 100% of the time in both arms;
- it never tried `npm test`, so no dead end was learned.

Adage's effect therefore moved from avoiding failures (Haiku: 78% → 100% hidden checks) to
**skipping exploration**: agents went to the lookup and the linked files directly. That halved the
turns before the first edit and cut cost by 24%, with quality unchanged. The weaker the agent is at
finding conventions, the more Adage adds.

Results are in [demo/results/20260923-1249-harbor-dispatch-claude-sonnet-5](demo/results/20260923-1249-harbor-dispatch-claude-sonnet-5/).


## Going further

### Other ways to install

From a tagged release, for macOS (arm64, x64), Linux (x64, arm64; static) and Windows (x64):

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/stevenvaughanbooth/adage/releases/latest/download/adage-cli-installer.sh | sh
powershell -ExecutionPolicy Bypass -c "irm https://github.com/stevenvaughanbooth/adage/releases/latest/download/adage-cli-installer.ps1 | iex"
brew install stevenvaughanbooth/tap/adage   # not published yet: once the first release is out
npm install -g @adage-sh/cli               # not published yet: once the first release is out
cargo install --locked --git https://github.com/stevenvaughanbooth/adage adage-cli
```

Three binaries ship: `adage`; `adage-exec`, which runs the reflexes you let act (`adage delegate`;
Linux and macOS); and `adage-train`, which compiles a skill into reflexes (`adage train`). The npm
package carries all three. The other channels install each on its own: `adage-exec-installer.sh`
and `adage-train-installer.sh` beside `adage-cli-installer.sh`, `brew install
stevenvaughanbooth/tap/adage-exec` (and `adage-train`), or `cargo install --locked --git … adage-exec`
(and `adage-train`). Put them beside `adage` or on `PATH`.

Every release archive has a SHA-256 checksum, a Sigstore build-provenance attestation
(`gh attestation verify <file> --repo stevenvaughanbooth/adage`) and a CycloneDX SBOM. The release
workflow publishes the Claude Code plugins, with those binaries bundled and checksummed, to the
`marketplace` branch that `/plugin install` reads; the plugin verifies its binary against
`dist/SHA256SUMS` at every session start, falls back to `adage` on `PATH`, and otherwise says in one
line what to install.

### Commands

`adage --help` lists every command; `adage <command> --help` its options.

```bash
adage init             # guided setup at a terminal: hooks, model, first conditioning, live conditioning
adage sense            # read-only: which reflexes your traces would form, per repository
adage condition        # form them: read your agents' traces, bind instincts, compile the index
adage watch            # in a split pane: Adage's health, hook activity and conditioning, live
adage map              # the map of your reflexes in the browser (read-only, loopback only)
adage ui               # the full terminal app: reflexes, map, health, settings
adage doctor           # check binary, data dir, index, tracks, model, hooks, kill switches
adage review           # evidence, advice previews, and routine candidates awaiting execution approval
```

Then, when you want to:

```bash
adage share <track> --out <dir> --key ~/.ssh/id_ed25519   # a signed pack for your team
adage add <dir> --trust                                    # a teammate's pack, bound here at forming
adage delegate on                                          # let reflexes you approve act (adage-exec)
adage train .claude/skills/x/SKILL.md                      # compile a skill into reflexes, replayed and reviewed; installs nothing
adage train accept x                                       # install the reviewed pack
```

At a terminal, `init` is a wizard: it detects your harnesses and, one step at a time, asks before
it registers hooks (showing the exact file and diff), fetches the embedding model, conditions
the repository for the first time, and turns automatic conditioning on if it is off; it ends with the doctor's
summary. Piped, inside an agent session, or with `--print`, it prints the same report and writes
nothing; `--yes` downloads the embedding model and binds this repository's instincts, but registers
no hooks (it ends by saying whether they are registered; `adage install <harness> --write` registers
them). `sense` reads
the same traces `condition` does and lists the reflexes `condition` would form as candidates, with
prompts shown only as short redacted snippets. With no history yet, plain `adage condition`
inside a repository binds available instincts from its files; `--instincts-only` requests that
explicitly. `doctor` exits
non-zero only when a check fails. From a source checkout, `cargo build --release --locked -p
adage-cli -p adage-exec -p adage-train` builds all three into `target/release/`. Every command
ends with the next one to run; `--plain` drops colour and glyphs, and `NO_COLOR` is honoured.

Manual `sense` and `condition` discover existing Claude Code history (`CLAUDE_CONFIG_DIR/projects`,
else `~/.claude/projects`), Codex history (`CODEX_HOME/sessions`, else `~/.codex/sessions`), and
configured OpenCode exports. The `learn.*` source settings apply to both; `learn.codex_sessions = ""`
disables Codex discovery. Explicit `--transcripts`, `--claude-projects`, or `--codex-sessions`
replace defaults. The wizard conditions only the selected repository; plain `condition` processes
all discovered repositories. Automatic Codex discovery requires registered Codex hooks or a Codex
hook request. History is read locally; conditioning never executes its commands.

See [How Adage works](docs/how-it-works.md) for the current behavior and limits, and
[live learning](docs/live-learning.md) for discovery and background learning. Conditioning and
advisory hooks are automatic after setup. CPU execution of learned actions requires the separate
execution policy and local approval described in [execution](docs/execution.md).

### Watch it live

`adage watch` (or `adage doctor --watch`) is a compact live pane for a split terminal. It works in
60×20 and grows with the pane:

```text
 adage watch  harbor-dispatch · demo           LIVE  FIRE 4  PRIME 1  INHIBIT 1
─ now ──────────────────────────────────────────────────────────────────────────
 04:22:53 bc0c7d cc  tools    ✗ ignored lookup-incident
 04:22:53 19b2cf cc  stop     ○ would ask the agent to continue (withheld)
 04:22:53 d76f0f cc  tools    → ◠ codegen-after-regist… FIRE       npm run code…
 04:22:53 bc0c7d cc  failure  → ◠ dead-end-npm-test     INHIBIT    npm run check
 04:22:53 d76f0f cc  tools    ✓ finish line met codegen-after-registry
 04:22:53 bc0c7d cc  stop     ■ finish line: asked the agent to continue
─ conditioning ─────────────────────────────────────────────────────────────────
 ◆ 04:22 auto-condition · 27/28 traces used · 0.73 s · 2 changes
   ◠ read-linked-files   prime → fire       13/17 · lb 0.53
   ◠ lookup-incident     evidence           24/25 → 25/26
   ○ 4 reflexes wait for review · adage review
─ health ───────────────────────────────────────────────────────────────────────
 + hooks claude    + 4 reflexes    + auto-condition
 + embedding model bundled    + no fail-open errors
────────────────────────────────────────────────────────────────────────────────
 p pause · f scope · / filter · enter inspect · m open map · ? keys      q quit
```

(`NO_COLOR` output, as above; in colour the strengths are lamps.)

- **Now:** every hook call as it happens, from the trace spools: time, session, harness, event,
  and what Adage did: a directive (→, with its strength: fire, prime or inhibit), withheld in a
  control session (○), followed ✓ or ignored ✗, a finish line met, or a stop request. `Enter`
  inspects the selected row: the exact directive text and its evidence.
- **Conditioning:** what conditioning changed, from `learn.log`, manual or automatic, and how many
  reflexes wait for your review.
- **Health:** lamps re-checked every few seconds: hooks that run for this repository, its reflexes
  in the index, live conditioning (`auto-condition`), the embedding model, recent fail-open
  errors; each problem names the command that fixes it.

Keys: `q` quits, `p` pauses, `f` switches between this repository and all of them, `/` filters by
trace, reflex or command, `Enter` inspects, `m` shows how to open the map, `v` hides events where
Adage said nothing, `↑`/`↓` scroll (`G` back to the newest), `?` lists every key. `NO_COLOR` is
honoured: strengths are then spelled out (`FIRE`, `PRIME`, `INHIBIT`), and `--plain` drops glyphs
too.

`adage ui` is the full-screen app (plain `adage` prints the status card above). `:` or `ctrl-k`
opens a command palette over every action and reflex; `?` lists the keys. Its tabs:

| Tab | What it does |
| --- | --- |
| **Live** | the watch view, larger |
| **Reflexes** | every instinct and reflex by track, with a fuzzy `/` filter: strength, stage, trigger and step, evidence (traces, Wilson lower bound), live counts, and the exact directive text. Review decisions (`a`pprove, `e`xtinguish, `w`eaken, `s`trengthen, pi`n`, dismiss `x`) ask first, then run exactly what `adage review` runs, with the same ceilings, and show the equivalent command |
| **Map** | stimulus → reflex → response as a layered graph in the terminal; `adage map` opens the same in the browser |
| **Health** | every doctor check, live, including live conditioning's. `Enter` fixes the selected one where a key can: register hooks (confirming the exact file and diff), fetch the model (with progress), condition this repository now (what a background learner runs: `adage condition --auto --repo-root`), turn live conditioning on, open the settings, or open the fail-open log |
| **Settings** | every `adage config` key with its value and where it came from (default, file, or environment); `Enter` edits, `Space` toggles, and `adage config set`'s validation and agent-session refusal apply |

`adage watch --demo` and `adage ui --demo` replay a small scripted scenario (three sessions of the
harbor-dispatch demo, through real hook calls) into a temporary data directory, so you can see it
all without an agent running. `adage watch --once [--size 80x24]` prints a single frame as text.
Decisions, installs and settings refuse inside an agent session, as their commands do.

### Claude Code

Installed from the marketplace (above), the plugin needs nothing else. To try the plugin from a
source checkout instead (it then uses the binary `plugin/build.sh` stages, or `adage` on `PATH`):

```bash
# 1. Build the binaries and stage them into the plugins
plugin/build.sh

# 2. Condition on your existing Claude Code traces (reads ~/.claude/projects, sub-agents'
#    traces included). After this, Adage keeps conditioning by itself as sessions end
#    (see "Live conditioning" below).
target/release/adage condition
target/release/adage status

# 3. Run Claude Code with the Adage plugin
claude --plugin-dir plugin/claude-code
```

### OpenCode and Codex

```bash
adage install opencode            # print the plugin; add --write to install it
adage install codex               # print a hooks.json; add --write to merge it
opencode export <session-id> > s.json && adage condition --transcripts s.json
adage condition --codex-sessions ~/.codex/sessions   # Codex rollouts
```

`--write` installs into the repository (`.opencode/plugins/adage.js`, `.codex/hooks.json`) or, with
`--scope user`, the harness's documented user location; it never runs inside an agent session.
`adage install claude-code` does the same for Claude Code settings. A harness already registered for
the repository (in the other scope, or by the Claude Code plugin) is not registered again, since a
second registration runs every hook twice; `adage doctor` flags one made by hand. To remove Adage's
hooks, `adage install <harness> --remove [--scope user]` shows the diff, keeps `<file>.adage-bak`, and
leaves your own hooks and settings as they were. What each harness supports, and
what it cannot do yet (Codex reports no tool failures to hooks, so only reflexes that react to a
failed command stay silent there; learned dead ends are announced at the first prompt), is in
[docs/harnesses.md](docs/harnesses.md).

`adage status` shows every track and reflex with its strength and evidence; `adage report` summarises
directives emitted, followed, and ignored. Data lives in `~/Library/Application Support/Adage`
(macOS), `~/.local/share/adage` (Linux) or `%APPDATA%\Adage` (Windows); `ADAGE_HOME` overrides it.

**Kill switches:** `ADAGE_DISABLE=1`, or a `.adage/disabled` file in a repository.

**Live conditioning:** you do not have to run `adage condition` again. When an agent's session stops or
ends, the hook starts a background learner for that repository (detached; the hook never waits,
and never says anything different). It conditions at most once per `learn.min_interval_secs` per
repository, one learner at a time, and appends each reflex that changed (new, extinguished, prime →
fire, evidence, awaiting review) to `$ADAGE_HOME/learn.log`. `adage doctor` shows the last run
per repository. The file contract for tools that follow it live is
[docs/live-learning.md](docs/live-learning.md).

**Settings:** `$ADAGE_HOME/config.toml`, shown and changed with `adage config`:

```bash
adage config                                  # every key, its value, and where it came from
adage config set learn.auto false             # stop live conditioning (ADAGE_AUTO_LEARN=0 also works)
adage config set learn.thresholds demo        # lower evidence bars, for small histories
adage config set learn.min_interval_secs 300  # condition each repository at most every 5 minutes
```

| Key | Default | |
| --- | --- | --- |
| `learn.auto` | `true` | Learn automatically when sessions stop or end (`ADAGE_AUTO_LEARN` wins) |
| `learn.min_interval_secs` | `120` | At most one automatic learn per repository per interval |
| `learn.thresholds` | `default` | `default` or `demo`, for `adage condition` too (`--demo` wins) |
| `learn.claude_projects` | `""` | Claude Code history (`""` = `~/.claude/projects`) |
| `learn.codex_sessions` | `auto` | Codex rollouts: `auto` reads `$CODEX_HOME/sessions` (this repository's only) once Codex hooks are registered; a directory; or `""` for none |
| `learn.opencode_exports` | `""` | A directory of `opencode export` files automatic learning reads |
| `learn.parse_cache` | `true` | Keep parsed transcripts in `cache/sessions` (owner-only), so re-learning parses only what changed |
| `learn.cards` | `true` | Procedure cards for recurring tasks (embedded with the installed or bundled model) ([docs/training.md](docs/training.md#recurring-tasks-procedure-cards)) |
| `learn.card_threshold` | `0.811` | A prompt's best similarity to a family's prompts that delivers its card (strict: it recognises phrasing close to earlier prompts, not paraphrases) |
| `instincts.enabled` | `true` | Bind Adage's instincts from each repository's files |
| `ui.notices` | `daily` | One-line notices at session start (Claude Code), at most daily and only when new; `off` silences them |
| `runtime.control` | `0.1` | Control share (`ADAGE_CONTROL` wins) |

Keys that change what Adage learns from or how much evidence it needs (`learn.thresholds`, the
three source directories, `instincts.packs`) are a person's decision: `config set` asks for a
terminal and refuses inside an agent session. Every other key only tunes Adage or turns something
down, and can be set from a script, CI or a sandboxed session.

**Embeddings and intent families (nothing to do):** the `adage` binary bundles the static
embedding model potion-base-8M, quantised to int8 (7.9 MB, SHA-256 pinned), so `adage condition`
clusters each repository's prompts into intent families (exact kNN + deterministic Louvain, split
and merged by what the sessions did next — spike S7) and forms an intent reflex per usable family,
with no download. `adage models fetch [potion-base-8m | potion-code-16m-v2]` installs a model's
pinned upstream files instead (from Hugging Face, else the byte-identical npm package; every file is
checked against its SHA-256), `adage models use <key|auto|hashed>` chooses, and `adage models
status`, `adage doctor` and the status card (`embeddings: …`) say what is in use. With no model at
all the explicit hashed fallback is shown, intent reflexes cannot fire, and everything else works
unchanged. What embeddings are and are not worth on replay: [evals/launch/embeddings.md](evals/launch/embeddings.md).

**Sharing:** `adage share <track> --out <dir> [--key <path>]` converts a track to a **pack** (the
only format that travels; [docs/packs.md](docs/packs.md)), checks every byte for absolute paths,
home-directory paths and secret-looking strings, re-validates it as a recipient would, and signs its
digest (`sshsig`, namespace `adage-pack`; ssh-agent first, else the key file, whose passphrase is asked
for at the terminal). `adage add <dir>` verifies the
signature against `$ADAGE_HOME/trusted-signers` or the repository's `.adage/trusted-signers` (approved
once with `--trust` and pinned by hash in `$ADAGE_HOME/trust.json`) and installs the reflexes
forming, never fire, under `tracks/@imported/` where `adage condition` never writes; if you later condition
the same reflex yourself, your first-hand reflex wins. Adding a newer version keeps your own review
decisions for every reflex that survives. An unknown signer needs `--trust`. Both commands refuse to run inside an agent
session (`CLAUDECODE`) or without a terminal; `ADAGE_ALLOW_NONINTERACTIVE=1` lifts only the terminal check.

**Review:** `adage review [--json] [--verbose]` lists the reflexes that need a person's decision: new reflexes from
`adage condition`, reflexes whose strength or evidence changed materially since the last decision (shape,
strength, a Wilson lower bound moving by 0.10, or support doubling or halving), imports forming,
extinguished reflexes that came back stronger, and reflexes agents keep ignoring in treated traces. Each
entry shows its trigger, evidence (in plain counts; `--verbose` adds the bounds, uplift and the graph
engine's reasons), effect and ceiling, and the exact directive text with example slot values.
Instincts in effect are listed once per instinct and step, with what triggers each, and wait for no
decision; an execution-only routine waits once, for its evidence, and is then listed as ready to
delegate.
A pinned reflex is never overwritten: when learning (or a re-import) finds a materially different
version, it waits as *proposed*, with a diff; `approve` applies it (still pinned) and `dismiss` keeps
yours. `adage review <approve|extinguish|weaken|strengthen|pin|dismiss> <track> <reflex> [--by <name>]` records the decision
in the track's `review.yaml`, re-validates the track, and recompiles the index. Strengthening never exceeds
the effect ceiling, and an import cannot be strengthened to fire without first-hand evidence learned
locally. `adage condition` keeps extinguished reflexes out of service until they strengthen materially, and holds
weakenings and strengthenings. An extinguished reflex also keeps a teammate's import of the same
reflex out of the index. Decisions refuse to run inside an agent session. Listing is allowed there.

**Experiment:** `runtime.control = 0.1` (default; `ADAGE_CONTROL` overrides) makes 10% of sessions the control — Adage computes what it would
have said and logs it, but says nothing — so every claimed effect is measured against a control.

## The demo

`demo/` runs real headless Claude Code agents on a sandbox repository whose conventions an agent
cannot guess (work items only via a tracker CLI; tests only via `make test`; `make gen` required
after schema edits), with twelve work items and hidden checks.

```bash
python3 demo/run.py all                  # train (no Adage) → condition → held-out eval, two arms
python3 demo/dashboard/serve.py          # live dashboard at http://127.0.0.1:8765
```

For your own repositories, `adage map` opens the live map of instincts and reflexes in the
browser; it is served by the binary, no Python needed.

See [demo/SANDBOX.md](demo/SANDBOX.md). `python3 demo/sandbox/selfcheck.py` proves every work item
fails untouched and passes with its reference solution.

## Repository

| Path | Contents |
| --- | --- |
| `crates/adage-core` | Canonical actions, shell normaliser (tree-sitter-bash + command table as data), effect classes, spans, repo identity |
| `crates/adage-store` | Per-session two-slot state files and CRC-framed spools — no daemon |
| `crates/adage-tracks` | Track format, validation, compiled index, interlocking, review decisions |
| `crates/adage-runtime` | Hook-time decisions and directive rendering from fixed templates |
| `crates/adage-hook` | The hook pipeline `adage` and `adage-exec` share: harness adapters, the no-match fast path, fail-open |
| `crates/adage-graph` | The graph engine: the only learner |
| `crates/adage-learn` | Transcript import (Claude Code and its sub-agents, Codex, OpenCode), steering by influence, and the learner's driver |
| `crates/adage-instincts` | Instincts: reflexes that ship with Adage, bound from a repository's own files |
| `crates/adage-intent` | Prompt normalisation, the static embedding model in plain Rust (WordPiece + safetensors rows read on demand), deterministic kNN + Louvain clustering, behavioural split/merge |
| `crates/adage-pack` | Packs (format `adage-pack/1`): validation, closure, digest, binding; the `adage-pack` inspect/verify/digest tool |
| `crates/adage-sign` | Native `sshsig` signing (ssh-agent or key file) and `allowed_signers` verification |
| `crates/adage-exec` | The `adage-exec` binary: approvals, policy, the OS sandbox, E1 prefetch and E2 actions |
| `crates/adage-train` | The `adage-train` binary: a skill → procedure graph → replayed, reviewed pack of reflexes (and optional skill-steered sessions) |
| `crates/adage-eval` | `adage eval`: offline replay of recorded sessions through the runtime, scored against what agents did; the L0 bench; safety oracle; property tests |
| `crates/adage-cli` | The `adage` binary |
| `crates/adage-sim` | Harness simulator: seeded mock histories (`sim/fixtures/`) and native hook payloads for Claude Code, OpenCode, and Codex |
| `crates/adage-term` | The terminal design system: tokens, capability detection, theme, glyphs and the output grammar every command prints through |
| `xtask` | `cargo xtask tokens [--check]`: generates the terminal palette and the web map's CSS variables from the design tokens |
| `design/` | The design tokens (`design/tokens/adage.tokens.json`, W3C DTCG), the single source for every surface ([style guide](docs/design/style.md)) |
| `web/map` | The map web app `adage map` serves, embedded in the binary |
| `web/site` | The public website |
| `plugin/claude-code` | The Claude Code plugin |
| `plugin/claude-code-exec` | The `adage-exec` plugin, which serves hooks once `adage delegate on` |
| `plugin/opencode` | The OpenCode plugin (installed by `adage install opencode`) |
| `npm/` | The npm launcher and per-platform packages |
| `demo/` | Sandbox, runner, live dashboard |
| `evals/` | Eval datasets (90 real demo sessions, synthetic, adversarial) and the live eval runner — see [docs/evals.md](docs/evals.md) |
| `docs/` | [Design spec](docs/Adage%20—%20Design%20Spec.md) · [Architecture](docs/Adage%20—%20Architecture.md) · [Implementation plan](docs/Adage%20—%20Implementation%20Plan.md) |
| `spikes/` | The experiments that settled the architecture — see [the board](spikes/README.md) |

`cargo test --workspace --locked` runs the suite, including the normaliser's golden fixtures, the
eval gates and the L0 bench against its baseline.

## Status

v0.1: Claude Code, plus OpenCode and Codex adapters and importers, built against their
documentation and source and verified end to end by the harness simulator (condition on each
harness's mock history, replay a held-out session through its native hooks in both arms), but not
yet run against a live installation — see [docs/harnesses.md](docs/harnesses.md#verification). Reflexes travel
as signed packs (`adage share` / `adage add`) and are reviewed with `adage review`. Intent families use static
embeddings (a model bundled in the binary; everything else works without one). Hardened against an
adversarial review (tracks are untrusted input; effects re-derived; bounded hot path; symlink-safe
paths). Team depots and lockfiles are designed but not yet built. See the implementation plan.

## License

Adage is licensed under the [Apache License 2.0](LICENSE). Third-party licenses for everything
compiled into the binary are in [NOTICE](NOTICE). Contributions are welcome under the same
license with a DCO sign-off (`git commit -s`); see [CONTRIBUTING.md](CONTRIBUTING.md).
