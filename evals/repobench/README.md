# RepoBench-Local: Adage on real repositories (tier L3)

The first live corpus (evals/corpus) measured Adage on generated sandboxes whose conventions were
designed to be undiscoverable. That flatters a learner. RepoBench-Local asks the ordinary user's
question instead: on a real open-source repository, working real bugs, does Adage make a coding
agent cheaper or better, compared with no Adage and with a plain context file?

Results: [evals/bench/results/2026-09-28-repobench](../bench/results/2026-09-28-repobench/README.md).
Suite descriptor: [evals/bench/suites/repobench-local.toml](../bench/suites/repobench-local.toml).

## Tasks from real history

| Step | Script | What it does |
| --- | --- | --- |
| Mine | `mine.py <repo>` | First-parent commits in the repository's window (`repos.json`) whose subject reads as a fix, changing 1–3 source files by a few dozen lines and at least one test file |
| Validate | `validate.py <repo> <candidates>` | Builds the sandbox at the fix's parent, writes the fix's test files in, runs the hidden command: it must **fail** there, **pass** with the real fix (every non-test file the commit changed), in **< 60 s**. Fixes that change a manifest or lockfile are refused |
| Prompt | `prompts.py <repo>` | One tool-less `claude -p` call per batch writes an issue-style prompt from the commit message and the **test** diff (never the source diff); `leaks()` refuses prompts naming tests, issue/PR numbers, commits or GitHub |
| Split | `build.py <repo>` | The latest 20 validated tasks, split chronologically with the external protocol's code (`evals/external/protocol.py`): oldest 50% train, 2 buffer, rest test; the SHA-256 digest is the pre-registration. Writes `splits/` and the runner config `configs/` |

`tasks/<repo>.json` holds every validated task (repo URL, fix SHA, base SHA, source and test files,
the hidden command, how long it ran, the base failure's tail, and the prompt). The repositories
themselves are not committed: `sandbox.py` extracts the base tree from a mirror in
`$REPOBENCH_CACHE` (default `/tmp/adage-repobench`, which `confine` hides from every agent).

## Sandbox and scorer

- **Sandbox** (`sandbox.py`): a one-commit repository with the base tree, `origin` set to the
  upstream URL (so the Adage repository key is the normalised URL, as in a real clone), and the
  environment a contributor who followed CONTRIBUTING would have: a project `.venv` (uv; not
  activated), `pnpm install`, `go mod download`. No history: nothing names the fix.
- **Scorer** (`score.py`), outside the agent's view: writes the fix commit's test files over
  whatever the agent left, reruns `rescue` steps, runs the hidden command, and reports `passed`
  (exit 0 and at least one test ran), the files the agent changed (committed or not) and whether
  they include the real fix's files. It then prunes the sandbox's environment (disk).

`conventions/<repo>.json` (`adage-conventions/1`) records each repository's real conventions from
its CONTRIBUTING, CI and Makefile, as ground truth for what a learner should find.

## Arms (evals/live/runner.py)

| Arm | Hooks | Gets |
| --- | --- | --- |
| `control` | on, `ADAGE_ARM=control` | nothing (directives computed and logged) |
| `adage` | on, delivered | reflexes learned from the training traces, plus instincts |
| `agents-md` | as control | a `CLAUDE.md` committed in the sandbox, written by one `claude -p` call from the training sessions' command lists (`agents_md.py`, after Gloaguen et al. 2026) |
| `instincts` | on, delivered | the day-0 index (`adage condition --instincts-only`); run only where learning added reflexes, since elsewhere `adage` *is* day 0 |
| `e1` | adage-exec | E1 prefetch of approved lookups; run only when a lookup reflex qualifies |

Sessions are isolated (fresh HOME and CLAUDE_CONFIG_DIR), `--host-auth`, confined under bubblewrap
and pinned to the runner's tool set. Arm order is seeded per (task, repeat).

## Other suites

- **Discoverable-conventions control** (`gen_doc.py`): two evals/gen sandboxes rerun with every
  convention stated in a committed CLAUDE.md, in every phase and arm.
- **Learning curve** (`curve.py`): a repository's tasks in fix-date order, an `adage` sequence that
  conditions after every session and a `control` sequence.
- **Model slice** (`variant.py`): the held-out tasks with Claude Sonnet 5, reusing the index and
  AGENTS.md learned from the Haiku training traces.
- **More history** (`offline_learn.py`): conditions each repository on every trace of the run.

## Reproducing

```bash
cargo build --release -p adage-cli -p adage-exec -p adage-train
for r in click attrs marshmallow urfave-cli vue-core; do
  python3 evals/repobench/mine.py $r --out /tmp/c-$r.json && python3 evals/repobench/validate.py $r /tmp/c-$r.json --max 60
  python3 evals/repobench/prompts.py $r
done
python3 evals/repobench/build.py
python3 evals/live/runner.py evals/repobench/configs/<repo>.json train learn --execute --root R
python3 evals/repobench/agents_md.py <repo> R
python3 evals/live/runner.py evals/repobench/configs/<repo>.json eval report --execute --root R
python3 evals/repobench/curve.py <repo> --root C --execute
python3 evals/repobench/make_report.py --out evals/bench/results/<date>-repobench
python3 -m unittest discover evals/repobench     # no tokens, no network
```

The runner refuses `--execute` inside an agent session (`CLAUDECODE`); run it from a terminal.
