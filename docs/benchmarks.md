# Benchmarks and evidence

Every number Adage publishes, with its caveats and links to the data and code. The [README](../README.md#results) carries the short version.

## Launch evidence: CPU execution and cross-session learning

For a learned lookup to replace an agent action, it must deliver the information the agent
would have fetched. Approved lookups can declare a bounded **complete stdout** contract: Adage
preserves the full text or withholds it on overflow, instead of silently returning a partial
task. Automatic argument binding also abstains when a prompt contains multiple distinct matching
values. See [complete lookup results](execution.md#e1-prefetch-adage-exec-hook)
and the [action-elimination gate](../evals/reflex-proof/harness/ACTION-GATE.md).

That [gate now passes](../evals/reflex-proof/harness/results/20260929-learned-action-gate/FINDINGS.md):
on three held-out work-item triage tasks, automatic execution removed all three agent lookup
actions and reduced observed model requests from six to three, with all answers correct.
This is a small generated-repository mechanism proof, not a general coding speedup claim.
The [launch evidence index](../evals/reflex-proof/LAUNCH-STATUS.md) tracks real-repository validation,
release gates, failed attempts and evaluation spending.
Across [two runs of four cold-start tasks](../evals/reflex-proof/harness/results/20260930-cold-start-replication/FINDINGS.md)
on Marshmallow, shipped instincts passed **8/8 outputs versus 6/8** for the agent alone, with **8.8%
lower reported API cost** and **10.5% fewer observed model requests**. Three of eight pairs cost more
with instincts. The repeat alone saved only 0.6% and had equal request counts. This small,
single-repository result measures advisory value, not automatic CPU execution.
The latest [15-session real-repository learning pilot](../evals/reflex-proof/harness/results/20260930-learning-curve-native/report.json)
passed independent task grading on 12/15 outputs but learned zero automatically executable actions.
Real-coding CPU savings remain unproven; the full result is retained alongside the mechanism proof.
A separate [five-session guided routine check](../evals/reflex-proof/harness/results/20260930-guided-demonstrations/FINDINGS.md)
also produced zero executable candidates despite five eventual test-command successes. Its audit
identified a mining/admission gap and insufficient trigger-specific evidence; the launch gate remains open.
The [mining integration fix](../evals/reflex-proof/harness/results/20260930-execution-prefix-integration/FINDINGS.md)
now preserves executable prefixes: replaying those five histories compiles nine candidates instead
of zero. Those five histories alone do not pass the unchanged execution evidence gate; compilation is not a savings result.
A [separately registered five-session follow-up](../evals/reflex-proof/harness/results/20260930-cumulative-guided/FINDINGS.md)
then brings the retained history to ten sessions and certifies four triggers for one test routine,
with **9/10 qualifying continuations**. This connects real traces to an executable learned artifact;
live automatic action elimination remains a separate, human-approved evaluation.

## Real-repository benchmarks

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
- **Details:** see [evals/showcase/RESULTS.md](../evals/showcase/RESULTS.md); machine-readable data is
  in [results.json](../evals/showcase/results.json).

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
    [RESULTS.md](../evals/launch/go-validation/RESULTS.md).
- **Data:** [evals/lab/research/recurring-workflows](../evals/lab/research/recurring-workflows/README.md).

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
- **Data:** [evals/lab/skills](../evals/lab/skills/README.md).

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
  shortcut ([research sprint](../evals/lab/research/BRIEF.md), 19 studies, negative results included).
- **Pass rate:** Adage does not change it on these tasks; failed sessions are wrong fixes, not
  process. No harm was measured in 196 real-repository sessions. Those sessions predate the
  pre-call rewrite and default-on cards, which the live validation re-checks.
- **Stronger models gain less:** Sonnet-class agents already avoid most procedural waste.

**What this means for you:** these experiments identify recurring procedures as a promising target;
they do not establish a savings percentage for a new repository. The shipped-product recurring
task interval above includes zero. Direct CPU action elimination has its own
[launch gate](../evals/reflex-proof/LAUNCH-STATUS.md), and real-repository coding results for that gate
remain pending. The status card measures your own sessions, with a control, before showing a
dollar figure.

The next [cross-session evaluation](../evals/reflex-proof/harness/LEARNING-CURVE.md) measures learning
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
> repeated under the corrected runner. See [docs/evals.md](evals.md#known-limitations-of-the-published-numbers).

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
Full data: [demo/results](../demo/results/); interactive results page published as an artifact.

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

Results are in [demo/results/20260923-1249-harbor-dispatch-claude-sonnet-5](../demo/results/20260923-1249-harbor-dispatch-claude-sonnet-5/).
