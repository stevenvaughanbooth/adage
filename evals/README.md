# Evals

Everything here measures Adage. Code, fixtures and curated evidence are tracked. Raw run output
is not: it stays on the machine that ran it, or in an archive outside git.

## What lives where

| Directory | What it is | Kind |
| --- | --- | --- |
| `datasets/` | Recorded and generated sessions the Rust test gates replay (`adage-eval`, `adage-graph`) | fixture |
| `baselines/` | The accepted benchmark baseline (`adage-eval/tests/bench.rs`) | fixture |
| `sandboxes/`, `live/sandboxes/` | Sandbox repositories agents work in | fixture |
| `gen/` | The procedural sandbox generator | code |
| `replay/` | The offline replay-savings metric | code |
| `live/` | The live-agent runner (paid) | code |
| `repobench/` | RepoBench-Local: Adage on real repositories | code |
| `external/` | Public benchmarks through Harbor | code |
| `bench/` | Benchmark suites and their result schema | code + evidence |
| `lab/` | Offline research studies ($0), one directory per study | code + evidence |
| `showcase/` | The showcase on five repositories | code + evidence |
| `launch/` | Launch readiness evaluations | code + evidence |
| `reflex-proof/` | The reflex proof: preregistrations, harness, evidence | code + evidence |
| `corpus/` | The first live corpus (a write-up) | evidence |

## Data policy

Three kinds of file, and a home for each:

1. **Fixtures**: inputs a test or tool reads. They are tracked, live under the test's own
   directory or `datasets/`, and every read from outside a crate is declared
   (`xtask/src/affected.rs`, `READS`), so CI knows which tests a change affects.
2. **Evidence**: the curated result of a study, cited by the docs or the site: a `README.md`,
   `FINDINGS.md` or `RESULTS.md`, a `RESULT.json`, small summary tables. It is tracked beside the
   study's code.
3. **Raw run output**: per-session logs, transcripts, state dumps, archives of `ADAGE_HOME`,
   frozen copies of source, rendered HTML. It is **not tracked**. Runners write to `/tmp` (or a
   path you give) and you copy the evidence out. `.gitignore` ignores the output directories, so
   committing raw output takes a deliberate `git add -f`, and the repository hygiene test
   (`xtask/src/hygiene.rs`) refuses large files and logs except the few listed there.

Some archived runs are themselves test fixtures: the reflex-proof harness tests replay
`reflex-proof/harness/results/<run>/` for the runs they name, and `showcase/live/<family>/`
archives seed the real-repository gate. Those stay in place, whole.

### Where the removed raw output went

Raw output committed before this policy (about 38 MB: run logs, transcripts, per-attempt JSON,
frozen source copies, spike transcripts) was removed from the tree in the commit that added this
file. Nothing that a test, a tool, the docs or the site reads was removed. It is all still in the
history. To get a file back:

```bash
git log --diff-filter=D --format=%h -1 -- <path>     # the commit that removed it
git show <that commit>^:<path> > <path>
```

## Running the evals' own tests

Each Python package here keeps its tests beside its modules (`test_*.py`) and runs them with the
standard library's `unittest` from its own directory:

```bash
cd evals/<package> && python3 -m unittest discover -p 'test_*.py'
```

The Rust gates that read `datasets/` run in `.github/workflows/evals.yml`. See
[docs/testing.md](../docs/testing.md) for how every test suite in the repository is laid out and run.
