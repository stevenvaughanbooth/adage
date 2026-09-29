# Training: compile your skill

`adage train <SKILL.md>` compiles a skill (or a runbook, `CONTRIBUTING.md`) into **one reviewed
pack of reflexes**: the parts of the skill that are procedure (which command, after which edit, in
which order, never which runner) become reflexes Adage delivers just in time, so the agent follows
the procedure without the whole skill in its context. The compile is deterministic and free: no
model and no agent session. Every compiled command is **replayed** on a clean checkout in
adage-exec's sandbox, so a stale instruction is caught before any agent sees it. Nothing is
installed until you have read the review and accepted it.

```text
adage train .claude/skills/work-item/SKILL.md     # compile, replay, review; installs nothing
adage train accept acme-work-item                 # install the pack, exactly as reviewed
adage share acme/skill-acme-work-item --out pack/ # optional: sign it and hand it to your team
```

Terminology:

| | Where it comes from | Evidence |
| --- | --- | --- |
| **Reflex** | Conditioned on traces (`adage condition`), or compiled from a skill (`adage train`) | Distinct sessions, Wilson lower bound; a compiled reflex: its replay |
| **Instinct** | Prebuilt route bound from the repository's declared files ([instincts.md](instincts.md)) | None: a capped default |
| **Skill** | Text a person wrote | None until `adage train` compiles and replays it |

**Principle.** The `adage` binary, which hooks run, starts no process on the agent's behalf (Design
principle 1; `adage-exec/tests/no_spawn.rs` pins it). `adage train` hands over to `adage-train`, a
separate binary; the replays run through `adage_exec::replay`, in adage-exec's sandbox, and only
when a person asks at a terminal. `adage train accept` is `adage`'s own: it installs a pack and runs
nothing.

## The review

On the lab's acme skill (`evals/lab/skills/skills/acme-work-item/SKILL.md`: look up the work item,
regenerate after schema edits, test with `make test`, finish with `make ci`), in the acme sandbox:

```text
adage train: acme-work-item (SKILL.md, 1479 bytes, about 370 tokens in context on every call)
repository  acme-billing@d4a946033f6a  (/work/acme-billing)
replay      Linux, Landlock ABI 7 + seccomp + user/net namespaces: E1-E2 available

s1  lookup     Read the work item
    compiled  fire     on a work item id in the prompt: run `scripts/wi-lookup {id}`
              verified  [acme-work-item-s1-lookup]
              replay: `scripts/wi-lookup W-2042`: exit 0
    compiled  prime    after `scripts/wi-lookup {id}`: read the files it lists, together
              not replayed: nothing to run: it reads what the lookup lists  [acme-work-item-s1-read-linked]
s2  regenerate If the item adds or changes a record field
    compiled  fire     after editing schema/*.json: run `make gen`
              verified  [acme-work-item-s2-regenerate]
              replay: `make gen`: exit 0, leaves a clean checkout unchanged
s3  edit       Make the code change
    text      needs judgment: the step is an edit the agent makes
s4  verify     Run the tests with `make test`
    compiled  inhibit  from the session's start: not `python3 -m unittest` (fails with `RuntimeError: ACME_ENV is not set`): run `make test`
              verified  [acme-work-item-s4-avoid]
              replay: `python3 -m unittest`: exit 1, with `RuntimeError: ACME_ENV is not set` as the skill says
              replay: instead, `make test`: exit 0
    compiled  inhibit  from the session's start: not `pytest` (fails with `pytest: command not found`): run `make test`
              verified  [acme-work-item-s4-avoid-2]
              replay: `pytest`: pytest is not on the replay's PATH, so it fails with `pytest: command not found`
              replay: instead, `make test`: exit 0
    compiled  fire     after editing acme/**: run `make test`
              verified  [acme-work-item-s4-verify]
              replay: `make test`: exit 0, shows OK
    compiled  fire     after editing acme/**: run `make test`, then `make ci`
              verified  [acme-work-item-s4-s5-sequence]
              replay: `make test`: exit 0, shows OK
              replay: `make ci`: exit 0
s5  verify     Before you finish, run `make ci`
    compiled  fire     after editing acme/**: run `make test`, then `make ci`
              verified  [acme-work-item-s4-s5-sequence]
              ...
    compiled  fire     after any edit, before finishing: run `make ci`
              verified  [acme-work-item-s5-before-finish]
              replay: `make ci`: exit 0
advice
    text      Work items live only in the internal tracker; a prompt usually names just the id, such as W-2042.
    text      Keep changes minimal and inside the linked files.
    compiled  prime    after any edit: advise: leave changes uncommitted  [acme-work-item-uncommitted]
              from: Do not commit; leave the working tree for review.

4 of 5 steps compiled into 9 reflexes (7 verified by replay); 1 left as text; 0 stale.
pack        acme-billing/skill-acme-work-item 0.1.0 (sha256 …)
            ~/.adage/train/skills/acme-work-item/pack

Nothing is installed yet. Read the review above; to install the pack as reviewed:
  adage train accept acme-work-item
```

Every step is listed: what it compiled to (the trigger, what the agent is told, the strength) and
how the replay verified it, or why it stays text. A step dropped by its replay reads `STALE … your
skill is stale here`. `--json` prints the same as data; `review.json` beside the staged pack keeps
it, with the pack's digest.

## 1. Compile (deterministic)

The skill is parsed into a **procedure graph** (`adage_train::procedure`, the lab's `procedure.py`
ported): steps in order, each with a role (lookup, read, search, edit, regenerate, verify, run,
install, vcs, remove, note), the commands it directs as templates with typed slots, the commands it
forbids and what to run instead (with the failure signature the skill gives), a precondition
(always, after an edit to a glob, before finishing, or a condition that needs judgment) and what its
output shows. Blocks that direct no command are advice. The parsing rules:

- A command counts only in an imperative sentence (run, use, never, after, before, …) or in a shell
  code block; a mention is not an instruction ("fails if you skipped `make gen`").
- In a sentence that forbids (never, do not, instead of), the commands before *instead of* are
  forbidden and the one after it is what to run instead.
- A concrete command that instantiates a template is its example (`scripts/wi-lookup W-2042` gives
  the slot `{id}` its example and the pattern `^W-\d{4}$`); a quoted literal in an example command
  (`grep -rn "prompt_suffix" src/click`) is the agent's choice.

Each step then compiles (`adage_train::compile`) to pack routes ([packs.md](packs.md)):

| Step | Reflex | Shape |
| --- | --- | --- |
| a lookup whose slot the prompt carries (`scripts/wi-lookup {id}`) | run it on a work item id in the prompt, the slot typed by its example | `run` |
| "read the linked files" after it | read the files its output lists, together | `read` |
| a command after an edit to a glob (`make gen` after `schema/*.json`) | a finish line after that edit, checked at Stop | `obligation` |
| a verify step (`make test`) | a finish line after an edit to the code the skill's edit step names (`acme/**`), else after any edit | `obligation` |
| a verify step, then a "before you finish" verify step | both, in order, after a code edit: one directive that carries both finish lines | `sequence` |
| "before you finish, run `make ci`" | a finish line after **any** edit (`*`), so a schema-only change still gets it | `obligation` |
| a forbidden command | not this (it fails with the signature): run that instead, from the session's start | `avoid` |
| "run the tests of the module you changed" (`tests/test_{module}.py`) | the edited file's tests, from the repository's test map (names and history, `adage_learn::testmap`) | `obligation` with an `edit_tests` input |
| "do not commit; leave the working tree for review" | the fixed advice `uncommitted` after any edit | `advise` |

What stays text, with the reason the review gives:

| Reason | Example |
| --- | --- |
| needs judgment: an edit the agent makes, or a step with no command | "Make the code change in the linked files" |
| a slot the agent chooses: nothing Adage sees binds it | `grep -rn "<symbol>" src/click` |
| no stimulus Adage can see | a fixed command with no edit, prompt or command before it |
| a condition that needs judgment | "if the report mentions a crash, run …" |
| never directed (effect ceiling) | `pip install`, `git commit`, `rm` |
| not one plain command | pipes, redirects, quoting |
| a script that is not a committed file of the repository | `.venv/bin/python repro.py` (the agent writes `repro.py`) |
| the project's environment is missing here | `.venv/bin/python -m pytest` in a checkout without `.venv` |
| no file under the code glob has tests the repository's names or history point to | `tests/test_{module}.py` in a repository without such files |

A repository script a step runs (`scripts/wi-lookup`) is pinned by its SHA-256 (L-ref); a program
of the project's own environment (`.venv/bin/python`) is not content of the repository, so it is
never pinned: the pack binds it where the checkout has it.

## 2. Verify by replay, not by counting

Every compiled command is replayed (`adage_train::verify`, through `adage_exec::replay`):

- **A clean checkout.** Each route gets a fresh shallow clone of `HEAD`, git hardened (no hooks),
  in a scratch directory. Your working tree, index and uncommitted changes are never touched.
- **The sandbox.** The one adage-exec runs E1 and E2 in: the scratch directory (the checkout in it)
  is the only writable place, the toolchain is read-only, the network is off, the environment is
  built from nothing, and the run is bounded (5 minutes by default, `--timeout-secs`). Root runs as
  nobody. Environment directories a clean checkout lacks (`.venv`, `venv`, `node_modules`) are lent
  read-only from your repository, and then the repository is readable too (an editable install
  imports from it); the review says what was lent. Without a sandbox nothing runs.
- **What must hold.** A directed command (a lookup with the skill's example value, a finish line,
  a regenerate step, each step of a sequence, the first mapped test file of an edited-file route)
  must exit 0 and show what the step says it shows (`Expect OK`). One that fails on a clean
  checkout is **stale**: its route is dropped and the review says so. A regenerate step also says
  whether it changed a clean checkout (the committed output is out of date).
- **Forbidden commands** must fail. With the skill's signature, the route stands; with another,
  that runner gets a route of its own with the signature observed (`pytest`: `command not found`,
  not the skill's `ACME_ENV is not set`); one that works on a clean checkout makes the prohibition
  stale, and it is dropped.
- **Not replayed** (no sandbox here, `--no-replay`, a tool missing, no example value): the route
  is kept and says so, at prime at most.

Session counts are not the check. Skill-steered sessions stay optional evidence (below).

## 3. Review, then accept

`adage train <skill>` stages everything under `$ADAGE_HOME/train/skills/<name>/`: `pack/` (the pack,
unsigned), `review.json` (the review, the pack's digest, the checkout it was compiled against) and
`procedure.graph.json` (the parsed graph). Running it again replaces them.

`adage train accept <name>` (a person, at a terminal: it refuses inside an agent session):

1. reads the staged pack strictly and refuses it if its digest is not the one the review recorded
   (what installs is exactly what was reviewed);
2. signs it when a key is at hand (ssh-agent, or `--key <file>`), namespace `adage-pack`; otherwise
   it installs unsigned and says so (`adage share` signs a pack when it travels);
3. installs it under `$ADAGE_HOME/tracks/@imported/<owner>/<name>/`, records the acceptance
   (`$ADAGE_HOME/train/accepted.json`: name → digest), binds it to the checkout it was compiled
   against, and recompiles the index.

Accepted reflexes are ordinary reflexes from then on: `adage review` shows them, `weaken`,
`extinguish` and `pin` apply, and the runtime judges them followed, ignored or misfired.

## Strength: the rule for compiled skills

A compiled reflex **may fire only if three things hold**: a person wrote it (the skill), its
command worked on a clean checkout (the replay), and a person accepted the pack after reading the
review (`adage train accept`). Anything short of that is prime at most:

| Compiled reflex | Strength |
| --- | --- |
| verified by replay, pack accepted here | its shape's, within the effect ceiling: fire for a lookup, a finish line, a regenerate step, a sequence; inhibit for an avoid step |
| not replayed (no sandbox, `--no-replay`, a missing tool, no example) | prime at most |
| read and advice routes | prime (as every read and advice route) |
| the edited file's tests | prime (a check after an edit is suggested, as the learner's is) |
| the same pack received from someone else (`adage add`) | an import: forming, prime at most (O2) |
| the pack changed after its review | an import (the acceptance names the digest) |
| a lookup through an unlisted script whose name says nothing of what it does (`scripts/ticket`) | prime: its effect is environment |

The index's interlock still applies: two fire reflexes that direct different commands at one
decision point cannot both fire. A "before you finish" check after any edit and a regenerate step
after a schema edit meet at an edit of a schema, so the index keeps one at fire and weakens the
other to prime (`adage_tracks::interlock`). `adage train accept` prints the pack's strengths; the
index's are in `adage review`.

This does not relax the evidence-origin policy below: *synthetic session evidence* (the optional
skill-steered sessions) still never lifts a reflex above prime. A compiled skill's strength comes
from authorship, replay and review, not from sessions, and a replay that could not run gives it
nothing. Directives from a compiled skill say so: their last line is *From this repository's
written instructions*, and a finish line's basis is *This repository's written instructions
require it*, never that agents were seen doing it (the eval's safety oracle checks both).

## The pack

One pack per skill, `<repository name>/skill-<skill name>`, origin `trained`, applicability `repo`
(the checkout's keys), E0 ([packs.md](packs.md)):

- `routes/`: the compiled reflexes, each with evidence `origin: authored`, `provenance:
  skill:<12 hex>`;
- `procedure.json`: every step of the skill and what became of it (compiled to which routes, left
  as text and why, or stale), with the replay lines, and every advice block; for people only;
- `[provenance] skill = "sha256:<hex>"`: the skill file's hash, equal to `procedure.json`'s.

## What does not compile yet

- **Judgment**: edits, decisions, search terms, "reproduce the bug first". They stay text; keep
  the skill for them (the review lists them), or drop it when nothing is left.
- **Scripts the agent writes** (`repro.py`): nothing can pin them.
- **A command with no stimulus** (a fixed command the agent should just run): an `intent` trigger
  could deliver it, with the embedding model at install; the compiler does not make one yet.
- **A slot from the edited file's path** other than its tests (`{path}` itself): no pack input
  carries it yet.
- **Prohibitions other than committing**: only `uncommitted` has a fixed advice template.
- **The compile-time LLM** (the lab's `--llm`): not in the product; its proposals were unreliable
  (it invented commands and dropped globs).
- **Aligning skill-steered sessions to the procedure** (the lab's `align.py`: per-step evidence,
  deviations, slot bindings): not in the product yet; the sessions below learn with the claim
  pipeline.

## Validation

The skill lab's acme comparison (`evals/lab/skills`, README "Productised"), re-run with the pack
`adage train` makes: 6 held-out work items × 2 repeats × 3 arms, Claude Haiku 4.5, sessions
isolated and confined, $2.25 in all. *Neither* has no skill and no reflexes; *skill* reads the
SKILL.md as `CLAUDE.md` on every call; *compiled* has no skill text, only the accepted pack's
reflexes (nothing learned, no instincts). `run_compiled.py` compiles and accepts the pack, then runs
the runner; the review it produced is `evals/lab/skills/results/acme-product-review.txt`.

| | neither | skill in context | compiled | compiled captures |
| --- | ---: | ---: | ---: | ---: |
| Pass rate (hidden test and `make ci`) | 0.75 (9/12) | 1.00 (12/12) | **1.00** (12/12) | 100% |
| Mean cost per session | $0.122 | $0.032 | $0.034 | 98% |
| Mean turns | 27.3 | 7.3 | 7.9 | 97% |
| Mean input tokens read (incl. cache) | 564k | 103k | 109k | 99% |
| Skill text read per session | – | 2.6k tokens | – | |
| Directive text delivered per session | – | – | 0.6k tokens | |
| Git commands per session (sessions that committed) | 3.1 (9) | 0 (0) | 0 (0) | |
| Sessions cut off at 30 turns | 4 | 0 | 0 | |

Adherence per step (sessions that did it / sessions it applied to):

| Step | neither | skill | compiled |
| --- | ---: | ---: | ---: |
| s1 look up the work item | 7/12 | 12/12 | 12/12 |
| s2 `make gen` after a schema edit, on time | 0/2 | 4/4 | 4/4 |
| s4 `make test` (a forbidden runner used) | 12/12 (5) | 12/12 (0) | 8/12 (0) |
| s5 `make ci` before finishing | 9/12 | 12/12 | **11/12** |

Against the lab's hand-built track (0.046 $, 10.6 turns, 161k tokens, s5 8/12, 2.5 git commands a
session), the two fixes it asked for show: the before-finish check after *any* edit (s5: 11/12, the
four schema-only sessions included) and the fixed advice to leave changes uncommitted (no session
ran git). Compiled closes 97 to 99% of the gap to the skill in context on cost, turns and tokens,
with 0.6k tokens of directives where the skill costs 2.6k a session; its total input is still 6%
above the skill arm's (0.7 turns more), so on this skill it matches the skill in context rather
than beating it. s4's 8/12 are the eight code-edit sessions: a schema-only change gets `make ci`
(which runs the suite), not `make test`. Small n, one model, one purpose-built sandbox: read it as
"no loss against the skill in context", not as a general rate.

## Recurring tasks: procedure cards

A skill is written down. Much recurring work is not: "add a validator", "add a flag type", done a
few times a month, the same files and checks each time, the specifics different. For that, Adage
learns the procedure from the traces themselves (`crates/adage-learn/src/cards.rs`), to the
research's card compiler spec
([confirm/README.md](../evals/lab/research/recurring-workflows/confirm/README.md), "Card compiler
spec"; the reference is `procedure.py`). It embeds prompts with the model adage-intent resolves,
as the hook does: an installed potion-base-8M, else the int8 copy bundled in the binary, so it
works with no user action.

1. **Families.** `adage condition` (and the automatic learn) embeds each trace's first prompt with
   the pinned model. Two traces are of one family when their prompts are similar (cosine ≥ 0.6)
   **and** the files they edited overlap by shape (Jaccard ≥ 0.25; a shape is directory, name
   token count, first token and extension, so `flag_url.go` and `flag_port.go` agree). Families
   are the connected components. Every later trace that matched joins its family, and its prompt
   joins the family's prompt library, so the library grows in the user's own phrasing.
2. **The card.** The files edited in at least half of the traces, in the order first edited;
   files edited in one trace each are grouped by shape and anti-unified (`flag_<x>.go`,
   `flag_<x>_test.go`, a test sibling apart from its file); a file a command wrote by redirection
   (`go doc -all . > godoc-current.txt`) counts as edited. For each, the longest hunk the latest
   passing trace wrote, whole, as the agent wrote it (to a file of the pattern's shape, for a
   pattern). The productive commands run in at least half of the traces (never `git status`,
   `ls`, `cat`, `grep` or a commit; a family's test runs are one command, by runner), kept when
   they mostly worked and listed to avoid when they mostly failed, with the first line of the
   error; a test run to avoid comes with the test run that worked; inline scripts
   (`python -c "…"`) are the task's own code and never listed. The check the passing traces
   finished with, anti-unified. Exemplars are clipped to 14, 10, 6, 3 lines, then dropped, until
   the card fits 800 tokens. **Never quoted:** a file that holds secrets by its kind (`.env*`,
   keys, credential stores) or that the checkout's `.gitignore` ignores; a command that holds a
   secret or names such a file; a line, command or error that looks like a secret (a secret-looking
   exemplar line becomes `…`). The index and the tracks are written owner-only (0600).
3. **How many traces.** No passing trace, no card, and no card from one trace: one trace is any
   one-off bug fix, and cannot show which part of a file name varies (the research's Go family:
   a card from 1 trace saved 9%, CI including 0; from 4, 34%). A repository keeps at most 16
   cards, the best supported.
4. **Delivery.** A family's card is a **reflex**: stimulus, a prompt whose best cosine to the
   family's prompts reaches `learn.card_threshold` (default **0.811**, the research's
   pre-registered threshold); response, the card, said once per session at UserPromptSubmit. A
   prompt within 0.10 below the threshold gets the card at the first edit of one of its files
   (PostToolUse) before the next prompt: a new prompt is a new task, and ends the wait. It is
   prime and observe: it directs no command, and `adage review strengthen` refuses it.
5. **Verification.** `adage train cards` replays each card's commands on a clean checkout in
   adage-exec's sandbox (as `adage train` does for a skill; `adage` itself runs nothing). Installs
   are not replayed, since the sandbox has no network. The next learn marks the commands that
   worked ("and on a clean checkout") and stops listing the ones that did not.
6. **Seeing it.** `adage sense` reports `recurring tasks: N families, M with a procedure card` and
   lists each card among the candidate reflexes; `adage review` shows the card's text; the status
   card counts procedure cards among the learned reflexes. `learn.cards = false` turns them off.

**Detection is strict, and literal.** The threshold is kept strict for launch: it is precise, but
it recognises only phrasing close to the family's earlier prompts. In the research's held-out Go
family it fired on 6 of 6 templated tasks, on **0 of 6 paraphrases** (AUC 0.075), and on none of
53 same-repository negatives; the file signal fired on none of 58 real sessions. Whole-prompt
cosine mostly measures style, so a terse paraphrase of a family task scores below some unrelated
bug-fix prompts. The library growing with matched sessions is what lets it follow a user's own
phrasing; a learned contrastive weighting lifted paraphrase AUC to 0.92 post hoc, but no threshold
had both recall and no false fires from 4 examples, so it is not built.

Reproduced through the hook (`adage hook claude UserPromptSubmit`, then `PostToolUse` edits, the
control share set to 0), from the research's training traces, with the bundled model
(`potion-base-8m-i8`, nothing installed) and again with an installed potion-base-8M, the same
counts both times:

| family | held-out tasks | paraphrases | same-repository negatives | edits of non-card files |
| --- | --- | --- | --- | --- |
| go-flag (urfave/cli, 4 traces) | 6/6 at the prompt | 0/6 | 0/48 RepoBench prompts, 0/5 hard negatives (including at a card file's edit) | 0 |
| mm-validator (marshmallow, 4 traces) | 6/6 at the prompt | – | 0/8 hand-written tasks | – |

With one Go trace no card is made (its files were created); with one marshmallow trace a card was
made then, at 0.753, and since the launch review none is (at least two traces). The Go card, as
the hook delivered it then (its commands and exemplars now follow step 2's rules):

```text
How changes of this kind were done here before: learned from 4 earlier sessions of the same kind of task in this repository (3 passed the project's checks). Use it as a checklist; the specifics of your task differ.
Files such a change touched (in order):
  - flag_<x>.go (its name differs each time) (4/4 sessions)
  - flag_<x>_test.go (its name differs each time) (4/4 sessions)
  - docs/v3/examples/flags/basics.md (4/4 sessions)
What the edit looked like last time (another variant; copy the format, not the content):
  flag_<x>.go:
    | package cli
    | …
    | type RegexpFlag = FlagBase[*regexp.Regexp, NoConfig, regexpValue]
    …
Commands:
  - cat (ran in 4/4 sessions, 33/34 succeeded)
  - go test ./... (ran in 3/4 sessions, 6/6 succeeded)
  …
Ignore this if the user is asking for something else.
```

"Passed" is the session's outcome when one is recorded (`--outcomes`), else Adage's proxy for it;
the research's hidden check failed all four Go training sessions (none updated the approved docs
copy), and the card, learned from traces, cannot contain a step no trace took.

The research measured the card's value with this detection and delivery: −33.8% of $ per task
(90% CI −44.3 to −23.6) and −31.5% of calls on the Go family, with pass rate unchanged; −16% and
−23% on click and marshmallow with the card in the prompt.

## Skill-steered sessions (optional evidence)

The compile needs no sessions. The claim pipeline below drives **your own agent harness**
headlessly on generated tasks in disposable clones of your repository, learns from what the agents
did with `adage condition`'s graph engine, and reports, claim by claim, whether the skill's claims
were followed, stale, never exercised, or unsafe. Its staged reflexes are synthetic evidence:
prime at most, never installed by it.

### How the modes relate

- **Skill → reflex.** A skill is a hand-written hypothesis about good routes (Design §10.10,
  bridge 3). `adage train <skill>` compiles its procedure and replays it; the sessions below
  exercise its claims and keep what the agents' behaviour supports.
- **Declared facts → instincts** ([instincts.md](instincts.md)). The same facts (`Makefile` targets
  and prerequisites, `package.json` scripts, CI `run:` steps, generator links) bind instincts on
  day 0 with no sessions at all. `adage train plan --from Makefile` reads them through the same
  extractor (`adage_instincts::extract`), so a declared step can be *tested* rather than assumed.
- **Ids line up.** A learned route takes the id the learner derives (`finish-line-<glob, command>`,
  `dead-end-<working command>`), which is also the id an instinct binds for the same behaviour.
  On the acme fixture training stages `finish-line-e7e809` and `dead-end-d41b1e`, the ids the demo
  learned and instinct U5/U1 bind. Once installed, a trained reflex therefore overrides the
  instinct with its id, as a learned route does.

### The pipeline

#### 1. `plan` (free)

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

#### 2. `run` (dry run by default; `--execute` spends money)

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

#### 3. `report`

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

### Safety model

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

### Evidence-origin policy

Where a route's evidence comes from caps how strongly Adage may direct it. The cap for *synthetic*
is enforced in staging today; the others describe the policy the model changes below make
enforceable in the index.

| Origin | Source | Max strength | Counts toward organic support | Activation |
| --- | --- | --- | --- | --- |
| organic | your agents' own sessions (`adage condition`, live learning) | fire (within the effect ceiling) | yes | automatic, reviewable |
| compiled | `adage train <skill>`: authored, replayed, accepted (`skill:<sha>`) | fire only when verified by replay and accepted here; else prime | never | `adage train accept` |
| synthetic | skill-steered `adage train` sessions (`synthetic:skill:<sha>@<run>`) | prime (a dead end stays inhibit: the model requires it of an avoid step) | never | after review, and preferably an accepted validation |
| declared | a repository's declared facts, as `adage train plan --from Makefile` exercises them | prime | never | after review |
| instinct | prebuilt, bound from declared facts (`instinct:core@<v>`) | prime, never a Stop block | never | automatic forming (environment steps: nascent) |
| imported | a teammate's signed track (`adage add`) | prime until first-hand evidence | no | forming; strengthening needs local evidence |
| demonstrated | a person doing the task themselves in a recorded session | fire after review | yes, once reviewed | review |
| corrected | a person's decision in `adage review` (authored, pinned, strengthened) | as decided, within the effect ceiling | n/a | immediate |

### CLI reference

```text
adage train <SKILL.md|runbook.md> [--repo <path>] [--no-replay] [--timeout-secs N (300)] [--json]
adage train accept <name> [--key <private key file>]

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

- `--repo` names any path inside the repository (its git root is used); by default, for a compile,
  the current directory's repository, else the skill's; for `plan`, the source file's repository.
- `--no-replay` compiles without replaying (every reflex is then prime); `--timeout-secs` bounds
  each replayed command (a compile) or each session (a run).
- `--adage` is the `adage` binary whose hooks the reflex arm runs; required with `--validate` and a
  real harness.
- `--thresholds demo` uses `adage condition --demo`'s lower evidence bars (min support 3), for small runs.
- `--harness fake --fake-world world.json` runs the whole pipeline with the scripted agent the
  tests use (below), spending nothing; it needs no terminal.

Run layout, under `$ADAGE_HOME/train/<run>/`: `plan.json`, `state.json` (every session: arm, task,
status, session id, turns, cost; spend; staging), `settings/`, `sessions/<job>/stream.jsonl`,
`transcripts/<phase>/<job>.jsonl`, `staged/{tracks/, index.json, provenance.json}`, `reflex-home/`,
`report.json`.

### The fake harness

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

### What is verified, and what is not

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

### Follow-up model changes

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
