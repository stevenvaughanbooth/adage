# adage-train

The `adage-train` binary: compiles a skill (or runbook, `CONTRIBUTING.md`, Makefile, `package.json`,
CI workflow) into staged reflexes, replays every compiled command in `adage-exec`'s sandbox, and,
when a person asks, drives their own agent harness headlessly to gather evidence. People reach it
as `adage train …`, which execs this binary. Guide: `../../docs/training.md`.

## Where it sits

- Layer 9 in `xtask/src/layers.rs` (binaries), beside `adage-cli`.
- Links: `adage-core`, `adage-tracks`, `adage-learn`, `adage-instincts`, `adage-store`,
  `adage-pack`, `adage-exec` (the only crate that runs anything), `adage-sign`.
- Depended on by `adage-cli` as a dev-dependency only (`tests/train_accept.rs`); at run time
  `adage train` looks for the `adage-train` binary beside `adage` or on `PATH`.
- No `FORBIDDEN` edge names it. It is not scanned by `adage-exec`'s `no_spawn` test, which is why
  agent sessions and git clones may be started here and nowhere behind `adage`.

## What's inside

- `main.rs`: `adage train <SKILL.md>` (`cmd_compile`), `plan --from <file>`, `--plan plan.json`
  (dry run; `--execute` spends money and needs a terminal), `report <run>`, `cards`; `accept` is
  refused here because `adage` installs packs.
- `skill`: `Options`, `run`, `render`, `Review` (schema `adage-train-review/1`, staged under
  `$ADAGE_HOME/train/skills/<name>/`), `pack`.
- `procedure` (skill to `Graph` of `Step`s), `compile` (`compile`, `Compiled`, `Candidate`), `claims`
  (`Claim`, `from_markdown`, `from_declared`), `tasks` (`generate`, seeded trigger and control tasks).
- `verify`: replays compiled commands through `adage_exec::replay` (`verify`, `settle`, `signature_of`).
- `cards`: `verify` a repository's procedure cards, recording verdicts for the next learn.
- `plan` (`PlanOptions`, `build`, `load`, `Plan`), `run` (`RunOptions`, `execute`, `describe`,
  `RunState`, `Job`, `run_dir`), `report` (`build`, `render`, `Verdict`).
- `harness`: the `Harness` trait, `ClaudeCode`, `Arm`, `SessionSpec`, `Outcome`; `fake`: `Fake`,
  `FakeWorld`, a scripted agent for tests.
- `isolate`: `require_person`, `process_env`, `isolation_env`, `prepare_home`, `clone_worktree`.
- `oracle`: what a session did about a claim, read through `adage-learn`'s parser.

## How it connects

- In: a source file and a repository; `adage_core::CommandTable` for classification; an installed
  Adage home (`adage_store::Paths`).
- Out: a staged pack and `review.json` that `adage train accept` (in `adage-cli/src/train.rs`)
  installs exactly as reviewed; run directories under `$ADAGE_HOME/train/<run>/` with `plan.json`,
  `state.json`, `report.json`; card verdicts for `adage_learn::cards`.
- Mining goes through `adage_learn`; sandboxed replay through `adage_exec::replay::Replay`
  (nothing runs without a sandbox); the `Harness` port lets the pipeline run against `Fake`.

## Changing it

- Invariants: `plan` and compile execute nothing; `--plan` without `--execute` runs nothing;
  `--execute` refuses inside an agent session (`isolate::require_person`) and without `--budget`;
  staged tracks are never activated by this crate.
- Tests: `cargo test -p adage-train`. `tests/pipeline.rs` runs plan, run, mining and report against
  `Fake` on `evals/sandboxes/acme-billing`; `tests/skill.rs` compiles a fixture skill (replay only
  where this machine has a sandbox, `cfg(unix)`). Unit tests in `compile.rs` and `procedure.rs`
  include skills from `evals/lab/skills/`.
- Fixtures read (`READS`): `evals/sandboxes/`, `evals/lab/`; local `tests/fixtures/SKILL.md`, `world.json`.
- Dependents to run: `cargo test -p adage-cli --test train_accept`, and `adage-exec` if `replay` changed.
- Pitfall: `main.rs` treats any first argument that is an existing file as a skill to compile;
  a new subcommand name must not collide with that.
