# Skill lab: compiling a SKILL.md into a collection of reflexes

2026-09-29. An exploration, not a product: Python prototypes over the existing runtime, one live
budget of $12 (spent **$9.89**), Claude Haiku 4.5 on Claude Code 2.1.284. No Rust was changed.

**Question.** A skill steers an agent, so it is already close to deterministic. Can we compile it
once into typed, verified reflexes that Adage's hooks deliver just in time, and keep the skill's
value without paying for its text in every model call?

**Answer, in one line per skill.**

- **acme-work-item** (a repository workflow skill: look up the ticket, regenerate after schema
  edits, test with `make test`, finish with `make ci`): compiled reflexes alone captured **all of
  the skill's effect on success** (pass rate: 0.58 with neither, **1.00** with the skill in
  context, **1.00** with the compiled reflexes) and **79% of its savings** in cost and turns ($0.093
  → $0.033 with the skill, $0.046 compiled; 24.3 → 7.0 turns with the skill, 10.6 compiled). The
  remaining 21% is text the compiler could not or did not turn into reflexes (below).
- **click-bugfix** (a "reproduce, test and fix a bug here" skill): the skill in context **did not
  help** (pass 0.50 vs 0.58 with neither, n = 8 vs 12), so there was nothing to capture; compiled
  reflexes covered 2 of its 6 steps (pass 0.75, n = 8, within noise; more turns and cost). The
  compiler's sandbox replay found one of the skill's own instructions **stale** (the whole-suite
  run fails on a clean checkout) and left it out; the skill arm followed it in 8 of 8 sessions and
  had twice the dead ends.
- **webapp-testing** (Anthropic's public skill, parse only): 3 of 17 blocks direct a command; one
  compiles (as an intent-triggered `--help` lookup). The rest is code to imitate and judgment. A
  skill like this stays text.

So: compile the *procedural* part of a skill (commands, their order, their preconditions, what not
to run); keep the judgment part as text; and verify every compiled step in a sandbox before it is
ever delivered. Recommendation at the end.

## 1. Design

### Pipeline

```text
SKILL.md ──procedure.py──▶ procedure graph ──align.py (sessions)──▶ per-step evidence, deviations, slot bindings
                (deterministic;               ▲                                   │
                 optional 1 LLM call)         │ abstract.py: transcripts ─▶ (role, env, scope) actions
                                              │                                   ▼
                                   skill-steered sessions             compile.py ──▶ compiled skill
                                   (runner train phase or             (reflexes + gaps + replay
                                    adage train)                        verification) ──▶ Adage track
                                                                                      (validated by adage)
```

| File | What it does |
| --- | --- |
| `procedure.py` | SKILL.md → procedure graph, deterministically (no model). Optional `--llm`: one tool-less `claude -p` call at compile time returning the same schema, validated, never trusted |
| `abstract.py` | Claude Code transcripts → abstract actions `(role, environment, scope)` plus the concrete command, path, failure and output head |
| `align.py` | Profile alignment (left-to-right profile HMM, Viterbi) of each trajectory to the procedure; per-step coverage, exact follows, deviations with their failure rate, forbidden-command hits, slot bindings by anti-unification, common insertions |
| `compile.py` | Graph + alignment → compiled skill (`adage-compiled-skill/0`): reflexes in Adage's route model, gaps with reasons, sandbox replay (`--verify`) and refinement, and `--home`: one track in an `ADAGE_HOME`, index compiled and validated by the `adage` binary |
| `trigger.py` | The skill-level trigger, measured offline on real prompts (span, lexical centroid) |
| `report.py` | Per arm: outcome, cost, turns, tokens, directives, and per-step procedure adherence (the alignment again) |
| `sandbox.py`, `make_configs.py`, `run_heldout.py` | The live runner's sandbox command (the skill as `CLAUDE.md` for skill arms), configs, and the held-out phase with the compiled home |
| `test_skills.py` | Offline tests (`python3 evals/lab/skills/test_skills.py`) |

### Data model: the procedure graph (`adage-procedure/0`)

A **step** has an order, a title, a **role** (`lookup read search edit regenerate verify run install
vcs remove note`), the **commands** it directs as templates with typed slots (each command also
abstracted to `role / environment / scope`: `verify / make / suite`), the commands it **forbids**
(`avoid: {command, instead, signature}`), a **precondition** (`always`, `after_edit(glob)`,
`before_finish`, `judgment`), an **expected observation** (`exit`, `contains`, free text), and
whether it is **compilable**, and why not. **Slots** are typed (`work-item-id`, `repo-path`,
`module`, `symbol`) with an anchored pattern, an example, and a **binding source**: `prompt`,
`edit` (from an edited path), `judgment` (the agent's choice: never compilable). **Edges** are
`next` plus `skip-if-not` around conditional steps. Non-procedural blocks are kept as **advice**
(counted, not compiled). Real outputs: `results/*.procedure.{json,txt}`.

Deterministic parsing rules that mattered (each found by a wrong first parse): a command counts
only in an imperative sentence (`run`, `use`, `never`, `after`, `before`, …); a mention is not an
instruction (`fails … if you skipped `make gen``); in an avoid sentence the commands before
*instead of* are forbidden and the one after is the replacement; `**bold**` is stripped before
sentence splitting; a concrete command that instantiates a template is its example
(`scripts/wi-lookup W-2042` → slot `id`, pattern `^W-\d{4}$`); a quoted literal in an example
(`grep -rn "prompt_suffix" src/click`) is a judgment slot.

### Abstract actions and alignment

An action is `(role, environment, scope)`: `.venv/bin/python -m pytest tests/test_termui.py -q` is
`verify / project-venv / file`; `python3 -m unittest` is `verify / system / repo`; a compound Bash
line is reduced to its most significant segment (`cd x && make test 2>&1 | tail` → `make test`).
Alignment is monotone: each action matches the current or a later step (a step absorbs repeats) or
is an insertion (reads, searches and git are free insertions). Scores: same role +3, same
environment +1 (else −1: a **deviation**), same scope +0.5, template match +1.5; a forbidden
command aligns to its step and is counted. Skipping a required step costs 2; a conditional step
costs nothing, and applies only if the trajectory edited a matching file. The alignment is
roughly-the-same, not exact, by construction: `python3 scripts/wi-lookup W-1046` aligns to
`scripts/wi-lookup {id}` as a deviation in the interpreter, which never failed; the skill's
`.venv/bin/python -m pytest tests -q -x` and the agents' `… tests/test_options.py::test_x -xvs`
align as a scope deviation.

### The compiled skill: a collection of reflexes (`adage-compiled-skill/0`)

Per step, the reflexes it becomes, in Adage's route model (local tracks today, route v2 in packs):

| Step pattern | Reflex | Adage shape (docs/packs.md) |
| --- | --- | --- |
| lookup with a prompt-bound slot | `prompt{span: ticket}` → `run scripts/wi-lookup {id}`, slot typed by the learned pattern | `run` |
| "read the linked files" after it | `after_command(exec scripts/wi-lookup {id})` → read the listed paths together | `read` |
| regenerate `after_edit(glob)` | `after_edit(schema/*.json)` → `make gen`, checked at Stop | `obligation` |
| forbidden runners, with signature | `session_start` → avoid `python3 -m unittest`, instead `make test` | `avoid` |
| verify, then a finishing verify | `after_edit(acme/*.py)` → 1. `make test` 2. `make ci` | `sequence` |
| a fixed observe command with no stimulus | intent-triggered run (local embedding centroid) — proposed, needs the model at install | `run` (intent) |
| edits, judgment slots, `rm`, commits | **gap**, with the reason | — |

Every reflex carries its **evidence** from the alignment (applicable sessions, sessions that did it
on time) and a **verification record**: its command replayed in a fresh sandbox against the step's
expected observation (exit code, `contains`); an avoided command must fail *with the skill's
signature*. The replay then **refines** the collection: a runner that fails differently gets its
own avoid route with the observed signature (acme: `pytest` fails with `pytest: not found`, not
with `ACME_ENV is not set`); a directed command that fails on a clean checkout is dropped and
reported as stale (click: `.venv/bin/python -m pytest tests -q -x` exits 1: `tests/test_basic.py`
errors at collection under the project's pytest).

`--home` writes the reflexes as one track, `<repo>/skill-<name>`, and compiles the index with the
`adage` binary, so every route passes `adage_tracks::io::validate` exactly as a learned one: effect
classification against the command table, strength ceilings, strict slot contracts, trigger/slot
binding. The installed tracks used in the live runs are in `results/tracks/`.

### How this maps onto packs (`adage-pack/1`, docs/packs.md)

A compiled skill is one pack: `origin = "trained"`, one track namespace
(`<owner>/skill-<name>`), applicability `repo` (acme, click) or `fingerprint` (a public skill:
`path_exists = "scripts/with_server.py"`), `requires.templates` = the shapes above,
`requires.repo_files` pins `scripts/wi-lookup` (L-ref, by SHA-256), a user type `work-item-id`
(`^W-\d{4}$`) for the slot, `[provenance] origins = { synthetic = <exploration sessions> }`, and
the three-arm result as an `attestations/<id>.json` `arms` record. Three things do not fit yet:

1. **Sequences in packs bind inert.** Local tracks deliver `sequence` (Track G G3), but pack binding
   still says "shape not yet supported at runtime" for sequences and multi-step routes. A compiled
   skill is mostly sequences and obligations; sharing one needs that binding.
2. **The procedure itself has no home.** The pack holds the reflexes but not the step graph, the
   gaps or the skill's hash. Proposal: `[provenance] skill = "sha256:…"`, and the compiled-skill
   JSON (graph, gaps, replay records) as an attestation or a review-only file, so a reviewer sees
   what stayed text and why.
3. **Strength policy.** The live runs used the strengths a reviewed skill would get (fire for
   observe/verify/regenerate, prime for environment, inhibit for avoid; `authored: true`). By the
   evidence-origin policy in docs/training.md, synthetic evidence caps at prime until review. A
   compiled skill is *authored* (a person wrote it) *and* verified (replay), so the review step that
   installs it should be what lifts it to fire.

## 2. Live results

Held-out tasks, three arms, the task as the unit; `neither` = no skill, no directives; `skill` =
the SKILL.md committed as `CLAUDE.md` (read in every model call: the upper bound of adherence and
of its token cost); `compiled` = no skill text, Adage's hooks deliver the compiled track (and
nothing else: no learned reflexes, no instincts). Sessions isolated (fresh HOME and
CLAUDE_CONFIG_DIR, host-auth allowlist, `--tools` pinned), confined under bubblewrap for acme,
arm order seeded per task. Full numbers: `results/acme-arms.json`, `results/click-arms.json`.

### acme-work-item: 6 held-out work items × 2 repeats × 3 arms (36 sessions, $2.06)

| | neither | skill in context | compiled reflexes | value captured |
| --- | ---: | ---: | ---: | ---: |
| Pass rate (hidden test + `make ci`) | 0.58 (7/12) | **1.00** (12/12) | **1.00** (12/12) | **100%** |
| Mean cost per session | $0.093 | $0.033 | $0.046 | 79% |
| Mean turns (tool turns) | 24.3 | 7.0 | 10.6 | 79% |
| Mean input tokens read (incl. cache) | 420k | 102k | 161k | 82% |
| Skill text read per session | – | 2.5k (370 tokens × 6.8 calls) | – | |
| Directive text delivered per session | – | – | 563 tokens | |
| Sessions that were censored (30 turns) | 3 | 0 | 0 | |

Per task, compiled had a lower mean cost than neither on 6 of 6 tasks, a higher one than skill on
6 of 6, and the same success as skill on all 6. No session of any arm was an infrastructure error. Adherence per step (sessions that did the step / sessions it applied to, from the alignment):

| Step | neither | skill | compiled |
| --- | ---: | ---: | ---: |
| s1 look up the work item (`scripts/wi-lookup {id}`) | 6/12 | 12/12 | 12/12 |
| s2 `make gen` after a schema edit (on time) | 2/2 | 4/4 | 4/4 |
| s4 `make test` (forbidden runner used) | 10/12 (2) | 12/12 (0) | 12/12 (0) |
| s5 `make ci` before finishing | 6/12 | 12/12 | **8/12** |

Where the remaining 21% went, measured on the transcripts:

- **Uncompiled advice.** The skill says "Do not commit; leave the working tree for review". That is
  advice about a `vcs`/author action, which Adage never directs. Compiled sessions ran 2.5 git
  commands per session (2 of 12 committed), skill sessions 0.17. That is most of the 3.6 extra
  actions per session.
- **A compile bug the alignment exposed.** The finishing `make ci` was compiled as a sequence after
  edits to the code glob `acme/*.py`; sessions that only edited `schema/*.json` never triggered it
  (s5: 8/12, exactly the 4 schema-task sessions). `before_finish` should mean "after any edit"
  (or one sequence per edit glob). Found by the held-out alignment, not fixed after the fact.

### click-bugfix: 4 held-out bugs × 2 repeats × {skill, compiled} (16 sessions, $6.36) vs the RepoBench v2 control (12 sessions)

| | neither (v2 control) | skill in context | compiled reflexes |
| --- | ---: | ---: | ---: |
| Pass rate | 0.58 (7/12) | 0.50 (4/8) | 0.75 (6/8) |
| Mean cost per session | $0.315 | $0.357 | $0.438 |
| Mean turns | 37.8 | 41.9 | 48.1 |
| Dead-end hits per session | 1.08 | **2.25** | 1.63 |
| Skill text read per session | – | 15.2k | – |
| Directive text per session | – | – | 310 tokens |

With n = 2 per task these differences are noise (one task, `c3535905`, carries the pass-rate gap:
compiled 2/2, skill 0/2, neither 1/3). The finding is structural:

- **Only 2 of 6 steps compiled**: the avoid (`python -m pytest`, `python3 -m pytest` → `.venv/bin/python -m
  pytest`, "No module named pytest"; `pytest` → own route, "pytest: not found") and, at prime, "after
  editing `src/click/*.py`, run `.venv/bin/python repro.py`". Gaps: *reproduce first* (the agent
  writes `repro.py`: no stimulus Adage sees before it), *find the code* (the search term is
  judgment), *run the module's tests* (`tests/test_{module}.py` is bound from the edited file's
  **stem**; no Adage slot source binds a stem, and the alignment shows the stem rule held in only 4
  of 9 aligned runs: `core.py` is tested in `test_basic.py`, `test_options.py`), *fix the code*.
- **The replay caught a stale instruction.** "After editing `src/click/*.py`, run
  `.venv/bin/python -m pytest tests -q -x` … expect passed" fails on a clean checkout (a collection
  error in `tests/test_basic.py` under the project's pytest). The compiler dropped it; the skill arm
  followed it in 8 of 8 sessions and hit twice the dead ends of neither. Text cannot be tested; a
  compiled step is.
- **Where agents improve on the skill.** The alignment of the skill-steered exploration sessions
  shows agents narrowing the test scope (`::test_name`, a file) rather than running the suite (every
  deviation aligned to s3 and s5 is a narrower scope than the skill's; the skill's own whole-suite
  command is the one that fails on every checkout), and deleting `repro.py` (`rm` in 2 of 3). Both are candidate edits to the skill; the first is what the
  RepoBench `.venv` instinct already directs.

### adage train on the same acme skill (8 sessions, $0.16)

`adage train` (today's claim-by-claim path) on the same SKILL.md: 5 claims, **0 routes staged**
(every claim "followed, below the learning thresholds" or "not after the last edit in enough"
sessions); it also read `make test` → `make test` as an avoid claim and invented
`regenerate_after(?) make gen` from a mention (`results/adage-train-acme-report.json`). The
procedure compiler, from 6 exploration sessions on real work items, produced 8 verified routes.
The difference is not the budget: claims are extracted and exercised one at a time, on synthetic
single-claim prompts, and must be re-discovered by the learner's thresholds; the procedure graph
compiles what the skill *says*, uses sessions only as evidence and for deviations, and uses the
sandbox, not session counts, as the check.

### The compile-time LLM (one call per skill, $0.18 for three)

`results/llm-structure.json`: Haiku structured all three skills into the same schema. It found
something the parser missed (webapp-testing: "DO NOT read the source" → avoid `cat
scripts/with_server.py`), and it also invented commands (`cat {html_path}`, a `cat > script.py <<EOF`
heredoc), dropped the acme `schema/*.json` glob (the one precondition that matters), and every graph
failed validation (undeclared slots, non-commands, an unknown role). Use: a proposal generator whose
additions are shown to the reviewer, validated, and replayed; never the source of truth.

### Trigger (offline, `results/trigger.json`)

The skill-level trigger decides whether any of the collection applies. Acme: a prompt span
(`W-2041`) is exact (recall 1.0, false positives 0 of 166 prompts from five other repositories).
Click: a lexical centroid of the description plus the exploration prompts separates click bug
reports from other repositories' (AUC 0.92, recall 0.60 at a threshold with 0 false positives of
157); the description alone ranks well (AUC 0.96) but its threshold is uncalibrated (recall 0.13).
In practice most compiled reflexes are reactive (after an edit, a command, a failure) and scoped to
the repository, so the skill-level trigger matters mainly for prompt-time steps; Adage's `intent`
trigger (potion-base-8M centroid) is the productised form.

## 3. What fraction of a skill's value is deterministic?

| Skill | Steps | Compiled (verified) | Stale (dropped by replay) | Gaps | Effect captured |
| --- | ---: | ---: | ---: | ---: | --- |
| acme-work-item (procedural) | 5 | 4 steps, 8 routes | 0 | 1 (the edit) + 2 advice blocks | 100% of success, ~80% of cost/turns |
| click-bugfix (investigative) | 6 | 2 steps, 3 routes | 1 | 4 | skill had no measurable effect |
| webapp-testing (code craft) | 3 of 17 blocks | 1 (proposed, intent) | – | 2 + 14 advice blocks | not run |

The deterministic share is the share of the skill that is *procedure*: commands, their order,
their preconditions and their forbidden alternatives. Workflow skills are mostly that and compile
well; debugging and craft skills are mostly judgment and do not.

## 4. Recommendation: evolve `adage train` into "compile your skill"

Productise it, but as a different pipeline from today's claim trainer:

1. **Compile the procedure, don't re-learn it.** `adage train compile SKILL.md` parses the skill into
   the typed procedure graph (deterministic; an optional one-call LLM proposal, validated and shown
   as a diff), prints it, and says per step what compiles, to which shape, and why not. Free, no
   sessions. This is the reviewable artifact.
2. **Verify by replay, not by thresholds.** Replay every compiled command in a disposable clone
   (`adage-exec`'s sandbox) against the step's expected observation; split avoid routes by observed
   signature; refuse stale steps and report them as "your skill is stale here" (the click case,
   found for free). This replaces most of what exploration sessions were for.
3. **Sessions become evidence, not the source.** A few skill-steered sessions (6 sufficed on acme),
   aligned over abstract actions, give each reflex its evidence, find deviations the skill should
   adopt (narrower test scope), find insertions every agent makes that the skill lacks, and learn
   slot bindings (`{id}` ← prompt 6/6). Optional; the compile works without them.
4. **Install as one pack** (`origin = "trained"`, provenance = the skill's hash), reviewed as a unit;
   the review lifts authored-and-verified reflexes to their ceiling. Then the skill text can be
   dropped from context, or kept for its judgment part only (the gaps list says which part).
5. **Measure it with the three-arm eval** (`--validate`: neither / skill / compiled, task-clustered),
   reporting value captured, as here.

Runtime and format work this needs, in priority order: (a) `before_finish` as a Stop obligation after
*any* edit (the acme 21% bug); (b) pack binding for `sequence` and multi-step routes; (c) an
`EditStem`-style slot source (or a learned path map: `src/click/core.py` → `tests/test_basic.py`)
for "run the tests of what you changed"; (d) a fixed-template **advise** for negative advice on
author actions ("leave changes uncommitted"), since that single uncompiled line cost the compiled
arm most of its extra turns; (e) the failure-trigger-through-a-pipe fix from the launch report,
since compiled avoid routes depend on it.

## 5. Caveats

- Small n: acme 12 sessions per arm (6 tasks), click 8 (4 tasks), one model (Haiku 4.5). The click
  `neither` arm is the RepoBench v2 control (same config, harness and model, directives withheld),
  not re-run.
- The skill arm reads the skill as `CLAUDE.md` every call. A real skill is loaded on demand by the
  Skill tool, which costs less when unused and may be loaded late or not at all; this arm is the
  upper bound of adherence.
- The acme sandbox was built to have conventions an agent cannot guess, which flatters any
  procedure. The acme skill and the click skill were written for this lab (the click one from the
  repository's real conventions, `evals/repobench/conventions/click.json`); only webapp-testing is
  third-party, and it was parsed, not run.
- The compiled arm used reviewed strengths (fire), not the synthetic cap (prime).

## Reproduce

```bash
cargo build -p adage-cli -p adage-train                     # CARGO_PROFILE_DEV_DEBUG=0
python3 evals/lab/skills/procedure.py evals/lab/skills/skills/acme-work-item/SKILL.md
python3 evals/lab/skills/make_configs.py
R=/tmp/skill-lab                                            # outside anything confine hides from itself
python3 evals/live/runner.py evals/lab/skills/configs/acme.json train --root $R/acme --adage target/debug/adage --execute
python3 evals/lab/skills/compile.py evals/lab/skills/skills/acme-work-item/SKILL.md --repo-key acme-billing \
  --sessions "$R/acme/private/transcripts/*train*/*.jsonl" --verify acme --home $R/acme/private/adage-home
python3 evals/lab/skills/run_heldout.py evals/lab/skills/configs/acme.json $R/acme --execute
python3 evals/lab/skills/report.py evals/lab/skills/skills/acme-work-item/SKILL.md $R/acme
# click: the same with configs/click.json, --verify click --task click-0cd28d65, --repo-key github.com/pallets/click,
# and report.py ... --neither <RepoBench v2 click root>
```

`--execute` spends money and refuses inside an agent session; the sessions get only the runner's
allowlisted environment (never `CLAUDECODE` or a session id). Raw runs are not committed.

## Spend

| Item | Sessions | USD |
| --- | ---: | ---: |
| acme exploration (skill-steered) | 6 | 0.20 |
| acme held-out, 3 arms | 36 | 2.06 |
| click exploration (skill-steered) | 3 | 0.93 |
| click held-out, 2 arms | 16 | 6.36 |
| `adage train` on the acme skill | 8 | 0.16 |
| compile-time LLM structuring | 3 calls | 0.18 |
| **Total** | | **9.89** |
