2026-09-28. 501 real headless Claude Code sessions (`claude -p`, Claude Code 2.1.283) on five real
open-source repositories at pinned commits, working 100 real bug fixes mined from their history, plus
two documented evals/gen sandboxes. Code and task definitions: [evals/repobench](../../../repobench/README.md).

**The short version: on real repositories Adage learned nothing it could deliver, so it changed
nothing.** In an ordinary user's setting it was neither an upgrade nor a harm.

- **Nothing learned.** From 10 training traces per repository (and, offline, from all 43–102 traces
  each repository produced), `adage condition --demo` compiled **zero active reflexes** in all five
  repositories. The one delivered route in the whole headline was an *instinct* (vue-core, N1: "after
  `npm test` fails, run `pnpm test`"), which fired in 2 of 16 adage sessions and was ignored both times.
  In the other 62 Haiku adage sessions nothing was delivered at all. So the `adage` arm *was* the day-0
  (instincts-only) arm in every repository, and "day-0 value" is the adage row below.
- **Headline, Claude Haiku 4.5, pooled (38 test tasks x 2 repeats per arm):** resolve rate 0.51 with
  Adage vs 0.51 without (diff 0.00, 95% CI [-0.08, 0.08]); mean cost $0.22 vs $0.22; **$ per resolved
  task $0.43 vs $0.42** (diff +$0.01, CI [-0.07, 0.09]). The AGENTS.md baseline: 0.51 resolved,
  **$0.42 per resolved task**. No metric differs after Holm in any comparison.
- **Harm.** Adage resolved 3 of 38 tasks less often than the control and 4 more often: noise of the
  same size both ways. AGENTS.md: 6 worse, 7 better.
- **Learning curve: flat, because nothing was learned.** On click and vue-core, 20 sessions each in
  fix-date order, the adage sequence ended with 0 reflexes live. Cost per session rose in both arms
  over the sequence (later tasks are harder), and the adage line tracks the control line.
- **Discoverable conventions.** In two gen sandboxes with every convention written in a committed
  CLAUDE.md, Adage still learned the lookup, linked-files and finish-line reflexes, but they were worth
  ~1 turn (median 7 vs 8, Holm p = 0.26) and no cost: the −15.5 turns of the first corpus came from
  conventions a context file would have stated.
- **Sonnet 5 slice** (click and vue-core, 16 tasks x 1): adage 0.44 vs control 0.50 resolved, $1.04 vs
  $1.00 per resolved task; AGENTS.md 0.38 and $1.24. Nothing was delivered to Sonnet either.

**Why nothing was learned on real repositories, from `adage condition`'s own drop reasons.** Real
agents vary their commands. The test command, the one convention every repository has, appeared as
`python -m pytest tests/test_x.py -xvs`, `python -m pytest tests/ -q`, `source .venv/bin/activate &&
pytest …`, `go test -run TestX -v`, `npm test -- packages/…`: "exec python -m pytest {path} -xvs has a
variable argument" and "… is ambiguous" (no variant held the 80% dominance the compiler needs) account
for most drops. The repositories' environments also offered no dead end to learn: the prepared
`.venv`, `node_modules` and module cache made the obvious commands work (click, where pytest is only in the `.venv`: the first test run failed in 5 of 9
training sessions, then agents recovered in different ways). The only things that recurred exactly were per-task scratch
scripts (`python test_converter_tuple.py` after editing it), which the learner did compile, as nascent
reflexes (see the Adage bugs). A learner that needs one literal command repeated in most sessions
finds little in real traces; the conventions that did exist (`.venv`, `pnpm test <package>`) were
documented or discoverable, and Haiku found them within a turn or two.

**Caveats.** 174 of 228 headline sessions hit the 30-turn limit (Haiku on real bugs works long), so
median turns is censored at 30 in almost every cell and cost is effectively capped at ~$0.20–0.30;
resolve rate and $ per resolved task are the informative metrics. 38 tasks x 2 repeats detects only
large effects (see evals/external/README.md, Power). Prompts were written by Claude Sonnet 5 from the
commit message and the *test* diff, and filtered for leaks. The E1 arm was not run: E1 prefetches
approved lookups, and no repository produced a lookup (or any active) reflex to approve.

## Problems found in Adage during real use

1. **`adage condition --help` runs a real learn.** It prints no help: it conditioned the operator's
   own `~/.claude/projects` (2 traces) and wrote tracks and an index to `~/.local/share/adage`. Any
   unknown flag is ignored the same way (`condition` parses only the flags it knows).
2. **Test commands never compile.** On real traces the test step carries a path or test id
   (`python -m pytest tests/test_x.py::t -xvs`, `go test -run TestX -v`), so the graph compiler drops
   it as "has a variable argument" or "is ambiguous" (no variant ≥ 80%), or a step after an edit is "not an obligation, a sequence or an environment
   step" (19–233 candidate patterns dropped per repository), and 0 compiled into active reflexes in 5 of 5 repositories. The one convention
   every repository has (how to run its tests, and on which part) is the one Adage cannot express
   from real traces unless the agents type the same literal.
3. **Nascent reflexes from one task's scratch files.** Conditioning attrs on all 58 traces compiled 4
   nascent routes, all per-task scratch scripts: after editing `test_converter_tuple.py` run
   `python test_converter_tuple.py`, and after editing `src/attrs/*.pyi` run `python
   test_converter_tuple.py`. The support came from repeated sessions of the same task (arms x
   repeats); nothing requires distinct tasks or days. From the 10 training traces alone it compiled
   `run-801299`: "an environment step this repository needs after editing `src/attr/*.py`: `python3`"
   (a bare interpreter, from agents' `python3 -c …` checks). All were left nascent, so none was
   delivered, but a person reviewing them would see noise.
4. **Instinct N1 fires on a genuine test failure.** vue-core's bound dead end ("after `npm test`
   fails, run `pnpm test`", empty signature) fired whenever `npm test -- <spec>` exited non-zero,
   including when the spec failed because the bug was not fixed yet: 2 delivered in adage sessions,
   3 would-have in control and agents-md, all judged `ignored`. `pnpm test` runs the same failing
   spec; `npm test` works in vue-core (only `npm install` is refused). A dead end should need the
   runner's own failure signature, not any non-zero exit.
5. **Instincts skip Python repositories that use tox or uv.** In click, attrs and marshmallow every
   Python instinct was skipped: "P1: tox wraps pytest here, but tox is not in adage-core's command
   table", "P1: uv run would wrap pytest, but uv is not …", "U3: body refused: tox is not …". Modern
   Python projects (all three here) are exactly tox/uv projects.
6. **Declared files over 64 KiB are not read for instincts** (urfave-cli: `command_test.go`,
   `flag_test.go`: "larger than 64 KiB"), so a Go repository with large test files binds less.
7. **Automatic learning forgets history when sessions are sandboxed.** In the first click curve
   (hook learner on, `ADAGE_AUTO_LEARN=1`, sessions confined to their own run directory), the learn
   log's `sessions_read` climbed 1→7 and then fell back to 1, and `learn-queue/<repo>.sources.json`
   kept only the last 8 transcripts: the learner run from inside a confined session appears to drop
   the sources it cannot see. Anyone running agents in per-task containers or worktrees would hit
   the same. (The curves' conclusions do not depend on it: conditioning on the full history offline
   also found nothing.)
8. **`adage config set` needs a terminal even for a scratch `ADAGE_HOME`** in automation (there is
   `ADAGE_ALLOW_NONINTERACTIVE` for `install --write`, not for `config`); the curve driver runs it
   under `script`.
