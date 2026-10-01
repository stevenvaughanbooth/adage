# Instincts

A **reflex** is learned from a repository's own agent sessions. An **instinct** ships with Adage
and binds from declared repository files, so a new repository can benefit before it has history.

An **instinct** is a prebuilt route that ships with Adage. It is a template (data, in
[`crates/adage-instincts/data/catalogue.toml`](../crates/adage-instincts/data/catalogue.toml)) plus a
detector (Rust, in [`src/detect.rs`](../crates/adage-instincts/src/detect.rs)). At learn time a
**binder** reads the repository's *declared facts* from its files and binds each enabled instinct
into concrete routes for that repository. Nothing comes from session history, so instincts work the
first time Adage sees a repository. They are weaker than reflexes by design: prime at most, never a
Stop block, and a reflex that says otherwise always wins.

**Status: integrated.** `adage condition` binds instincts per repository after mining (and
`adage condition --instincts-only` binds them with no sessions at all); the index, the runtime, the CLI
and the offline eval know them by their provenance. This document is the catalogue, the binding
and override model, the safety rules, and how the crate is wired in ([Integration](#integration)).

## What ships at launch

The core pack contains **eight enabled instinct detectors**: U1, U3, U4, U5, U6, N1, P1 and U11.
They may bind multiple routes, one route, or none in a repository. A route is installed only when
its declared facts support it; competing test entry points, unsafe command bodies, or an absent
environment can correctly leave an instinct unbound. The remaining catalogue entries are disabled.

Run `adage init --yes --repo /path/to/repo` to bind these defaults without transcripts; it
registers no hooks (it says whether they are registered, and `adage install <harness> --write`
registers them). Noninteractive `adage init` previews setup without writing.
In a terminal, the interactive wizard's Condition step binds defaults alongside learned reflexes. With automatic
learning enabled, SessionStart requests a background bootstrap for a new or changed checkout;
the hook itself does not wait for binding, so a first event can arrive before that work finishes.

| Shipped behavior | Matching and delivery | What happens automatically |
| --- | --- | --- |
| U1 / P1: use the declared test wrapper | A known runner failure names the wrapper's required environment or loader | CPU match; the agent receives the replacement command as a suggestion |
| U3: use the repository's test command | An undeclared runner fails with a specific signature and one unambiguous entry point exists | CPU match; suggested command |
| U4 / U6: generated files and lockfiles | An edit touches a declared generated file or root lockfile | CPU match; advice naming the owner or generator |
| U5: regenerate after input changes | An edit matches declared generator prerequisites | CPU match; suggested regeneration command |
| N1: use the declared package manager | A known package-manager mismatch fails with its specific signature | CPU match; suggestions; environment-changing suggestions await pack review |
| U11: use the project's Python environment | A supported Python command targets the project and its local environment was verified at bind time | On Claude Code, CPU rewrites the interpreter in `PreToolUse`; the harness executes the corrected pending tool call under its permissions |

**Matching on the CPU is distinct from eliminating an agent action.** Most shipped instincts
deliver suggestions and still require agent inference. U11 can prevent an environment-recovery
turn by rewriting a tool call the agent already proposed. It does not itself launch a new script,
and its additional context is an explanation, not a request for the agent to perform the rewrite.
Codex and OpenCode retain post-failure advice for this instinct. Automatic execution of learned
actions is a separate mode with explicit human grants; `init` never grants arbitrary scripts.
See [execution](execution.md) for that permission and runtime contract.

### Reproducible launch checks

`cargo test --locked -p adage-cli --test instincts` includes two checks of the public setup path:

- `init_without_history_delivers_six_shipped_instincts` starts with isolated homes and no history,
  runs `init --yes`, and delivers U1, U3, U4, U5, U6 and N1 through real hook processes. It verifies
  that an actual assertion-failure signature stays silent, no instinct blocks Stop, and no
  execution approval is created. These are suggestion checks, not six saved-action claims.
- `init_environment_instinct_rewrites_and_runs_without_inference` (Unix, requires Python 3 with
  `venv`) creates a real local environment without downloading packages, runs `init --yes`,
  invokes the Claude PreToolUse hook, and executes the exact returned command. The command
  imports the project and asserts that it runs inside the virtualenv. The test also verifies that
  `delegate rewrites off` takes effect at the next hook. No model API is called.

The second check proves the rewritten command works, with **zero model calls during matching and
rewrite**. It does not measure live-agent cost savings. Existing catalogue, detector, hostile-file,
real-failure, and cross-harness tests cover binding precision and delivery separately. The learned
action launch gate must additionally demonstrate one fewer agent tool action for the same verified
task outcome; neither catalogue size nor an emitted suggestion meets that gate.

## Catalogue

`P0` instincts bind now. `P1` instincts are catalogued with their detection and cap, and ship
disabled with stub detectors, until the model can express them (see [Not done yet](#not-done-yet)).
U2 is P0 on the roadmap but is disabled here: it needs a trigger the model does not have.

Every `after_failure` instinct fires only on its **failure signature**: text the failure prints
when the command itself was the wrong one (see [Failure signatures](#failure-signatures)). Never
on a bare non-zero exit, which includes every test that fails on its assertions.

| Id | Priority | Instinct | Trigger | Directive (fixed template, sketched) | Cap | Detection (no history) | FP risk | Enabled |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| U1 | P0 | Declared wrapper explains a failure | `after_failure` of runner R, one route per signature: each env var the wrapper sets; else `ERR_MODULE_NOT_FOUND` (`--import` loader) or `Cannot find module` (`--require`), and the wrapper's own command (`make check`) as the repository's message names it | "A known next step here is: `S`" | prime | A Makefile target or package.json script with a canonical check name (`test`, `check`, `ci`, …) whose plain body runs R plus env vars, setup flags or `--import`, or runs a repository script that drives R (`unittest.TextTestRunner`, `pytest.main`) | low | yes |
| U2 | P0 | No blind retry | `repeat_failure` (same key, same signature, twice, no edit between) | "That failed the same way twice; read the error before retrying" | prime | Runtime history only; timeouts and connection errors exempt | medium | no |
| U3 | P0 | The repository's declared test command | `after_failure` of an undeclared runner (`pytest`, `python -m pytest`, `npm test` in an npm repository, …) on: the wrapper's signatures when `S` is a wrapper, a missing environment's when `S` runs in one (tox, `uv run`, a venv), and `S` itself when the output names it | "A known next step here is: `S`" | prime | Exactly one test entry point (Makefile `test`/`tests`, package.json `test`, `tox -e py`, nox, just, Taskfile), body plain and at most verify; CI `run:` steps break ties | low-med | yes |
| U4 | P0 | Don't hand-edit generated files | `after_edit` of the generated file (advisory) | "`path` is generated: change its inputs and run `command` rather than editing it by hand" | prime | `Code generated … DO NOT EDIT`, `@generated` in a comment in the first 40 lines; files a declared generator writes (Makefile file rules, paths named in the generator script) | very-low | yes |
| U5 | P0 | Declared regeneration | `after_edit` of the generator's inputs | the finish-line template: "changes to files matching `glob` must be followed by running: `command`" | prime | Makefile rule prerequisites (`models.py: schema/*.json`, `$(wildcard …)`), else input paths named in the generator script (`Path("schema")` + `"*.json"`) | low | yes |
| U6 | P0 | Don't hand-edit lockfiles | `after_edit` of the lockfile (advisory) | "`path` is maintained by `manager`: change the manifest …" | prime | A lockfile at the repository root (npm, pnpm, yarn, bun, cargo, go, poetry, uv, pipenv, pdm, bundler, composer) | very-low | yes |
| N1 | P0 | Package manager discipline (node) | `after_failure` of `npm install` / `npm ci` (`Unsupported URL Type` for `workspace:`/`catalog:` dependencies, an `only-allow` guard's `for installation in this project`, `EBADDEVENGINES`, `an existing package-lock.json`), of `npm test` (`EBADDEVENGINES` only), of the other of `yarn`/`pnpm` (`command not found`, corepack's `configured to use pnpm`); advisory after `npm install` succeeds | "A known next step here is: `pnpm install`" / "`pnpm test`" | prime | `packageManager` field or a pnpm/yarn/bun lockfile, and no conflicting npm lockfile | very-low | yes |
| P1 | P0 | Python runner wrapper / `uv run` | `after_failure` of `pytest`, `python -m pytest`, `python3 -m pytest`, on the wrapper's signatures (U1's), or for tox a missing environment's | "A known next step here is: `make test`" / "`tox -e py`" | prime | pytest configured (pyproject, pytest.ini, setup.cfg, tox.ini, conftest.py) and a Makefile wrapper that sets env or flags; else `tox -e py` when tox's base test environment runs plain commands (not `tests: pytest` factors); nox is reported, never bound (its sessions are code); `uv run` is U11's | low | yes |
| U7 | P1 | Manifest changed, sync environment | `after_edit` of a dependency section | "Run `pnpm install`" | prime | Package manager from the lockfile; pack-level approval (environment effect) | medium | no |
| U8 | P1 | CI runs checks the agent skipped | `after_edit` of files CI lints | "CI runs `…`; run it before finishing" | prime | CI `run:` steps that classify as verify | low | no |
| U9 | P1 | Watch mode never exits | `after_failure` (timeout) of a watch command | "Run the one-shot form: `vitest run`" | prime | Table: `jest --watch`, `vitest`, `tsc -w` … | low | no |
| U10 | P1 | Missing dependency is declared | `after_failure` with `ModuleNotFoundError` / `Cannot find module` | "It is declared; install the project's dependencies" | prime | Only if the module is in the manifest (rules out typosquatting); pack-level approval | low | no |
| U11 | P0 | Use the project's environment | `after_failure` of `pytest`, `python -m pytest`, `python3 -m pytest`, `tox`, `nox` with `pytest: command not found` (bash and zsh), `No module named pytest`, `python: command not found`, or `No module named '<the project's own package>'`; and `after_failure` of any other `python`/`python3` run (`python -c "import click"`, `python repro.py`) with `No module named '<the project's own package or a declared test dependency>'` | "A known next step here is: `.venv/bin/python -m pytest`" (else `uv run pytest`); for another Python run, advice: "That Python is not this repository's environment, which has the project and its test dependencies installed: run Python here as `.venv/bin/python`." (said only when no run step answers the failure) | prime | A `.venv` or `venv` with a `pyvenv.cfg` (made, not activated); else a uv project whose dependency groups have pytest (`uv run --group tests pytest` when only a non-default group has it); Python only, `node_modules/.bin` not yet | low | yes |
| N2 | P1 | Path-alias loader | `after_failure` with `ERR_MODULE_NOT_FOUND` on an alias | "Run through the declared loader" | prime | tsconfig `paths` plus an `--import` loader | low | no |
| N3 | P1 | Monorepo scope | `after_failure` with "Missing script" at the root | "Run it in the package that declares it" | prime | package.json `workspaces` | medium | no |
| R1 | P1 | Crate scope | `after_edit` in a workspace member | "Test that crate: `cargo test -p x`" | prime | Cargo workspace members | low | no |
| R2 | P1 | clippy/fmt gate | `after_edit` of Rust files | "CI gates on clippy and fmt" | prime | CI steps running `cargo clippy -D warnings`, `cargo fmt --check` | low | no |
| G1 | P1 | go generate | `after_edit` of a file with a `//go:generate` line | "Run `go generate ./...`" | prime | `//go:generate` lines | low | no |
| X1 | P1 | protobuf / sqlc / OpenAPI codegen | `after_edit` of a generator input | "Run the declared generator" | prime | Generator configs (`buf.gen.yaml`, `sqlc.yaml`, OpenAPI generator configs) | low | no |

Deliberately not shipped, ever: "update snapshots on failure", `--no-verify`, `npm audit fix`,
`rm -rf node_modules`, installing undeclared packages, and any Stop block.

### What binds on the reference repositories

The crate's tests bind every fixture under `crates/adage-instincts/tests/fixtures/`:

Advisories are routes too (an advisory step, `Step::Advise`, with a fixed template); they are listed
apart here because they direct no command.

| Fixture | Routes | Advisories |
| --- | --- | --- |
| `acme` (demo/sandbox) | U1 `python3 -m unittest` fails naming `ACME_ENV` → `make test` (`dead-end-d41b1e`, the id the demo learned); U3 `pytest`, `python -m pytest`, `python3 -m pytest`, `python -m unittest` fail naming `ACME_ENV` or `make test` → `make test`; U5 `schema/*.json` → `make gen` (`finish-line-e7e809`, the id the demo learned) | U4 `acme/generated/models.py` (`make gen`) |
| `harbor` (harbor-dispatch) | U1 `npm test` and `node --test` fail with `ERR_MODULE_NOT_FOUND` → `npm run check`, and `node --test` fails naming `npm run check` → the same | U4 `src/generated/registry.ts` (`npm run codegen`) |
| `pnpm` | N1 `npm install`, `npm ci` fail with `Unsupported URL Type` (a `workspace:*` dependency), `npm ci` with `an existing package-lock.json`, `yarn install` with `yarn: command not found` / `configured to use pnpm` → `pnpm install` (environment: **nascent**, waits for review); `yarn test` likewise → `pnpm test`. `npm test` binds nothing: it runs the same `vitest run` | N1 "uses pnpm" after `npm install`; U6 `pnpm-lock.yaml` |
| `rust` (workspace) | none (R1, R2 disabled) | U6 `Cargo.lock` |
| `go` (`//go:generate stringer`) | none (G1 disabled) | U4 `strength/strength_string.go` (`go generate ./...`); U6 `go.sum` |
| `pytool` (pytest + Makefile wrapper, uv, prerequisite rule) | U1/P1 every spelling of pytest fails naming `DJANGO_SETTINGS_MODULE` → `make test`; U3 the same, naming `make test`; U11 pytest missing (`pytest: command not found`, `No module named pytest`, …) → `uv run pytest`; U5 `schema/*.json` and `tools/gen.py` → `make gen` | U4 `quayside/models_gen.py` (`make gen`); U6 `uv.lock` |
| `decoy` (bench, test-legacy, docs, release targets) | none | none |
| `hostile-npm`, `hostile-hooks`, `hostile-make`, `hostile-unicode` | none | none |
| `hostile-ci` (injection text in a workflow) | only the plain `make test` target, after unittest fails naming it; no workflow text reaches anything bound | none |

Symlinks out of the repository, files over 64 KiB, invisible characters in target names and
pathological manifests are covered by tests that build their repositories at test time; so are tox
(plain and factor-conditional), nox, a project `.venv`, uv dependency groups, and a large `_test.go`.
`no_instinct_fires_on_a_genuine_test_failure` checks every failure route of every fixture against
real assertion-failure output of pytest, unittest, vitest, `go test` and npm: none fires.

On the generated `smoke` suite (`evals/gen`, six sandboxes over python-make and node-npm, bound
from their files alone by `tests/generated.rs`): **6 of 6 dead ends and 6 of 6 finish lines
recovered on day 0; 0 of 13 decoys and 0 of 4 instinct violations bound.** A dead end counts as
recovered only if the test *runs* the dead-end command in the sandbox and one of the bound routes'
signatures is in what it prints. The violations (`make test` / `npm test` retired and printing a
failure) are refused because their bodies redirect or run an unknown script; the python dead ends
are recovered through a runner script (`tools/testkit.py` driving `unittest`), on its env guard or
on the repository's message naming `make check`.

### What binds on RepoBench-Local's repositories

The five real repositories of [RepoBench-Local](../evals/bench/results/2026-09-28-repobench/README.md)
at the base commit of their latest task (attrs `f616f73`, click `150d107`, marshmallow `3207be3`,
urfave/cli `d1d8108`, vuejs/core `5dda192`), bound with `cargo run -p adage-instincts --example
dry_run -- <dir>`; the Python ones with a `.venv` made and not activated, as the sandboxes had
(without it, U11 directs `uv run pytest` instead). Checked against
`evals/repobench/conventions/*.json`:

| Repository | Before (RepoBench run) | After | Convention |
| --- | --- | --- | --- |
| attrs | U6 `uv.lock` only; P1, U3 skipped ("tox is not in adage-core's command table") | U11 `pytest`, `python -m pytest`, `python3 -m pytest`, `tox` failing outside the environment (`command not found`, `No module named pytest`, `No module named 'attr'`/`'attrs'`) → `.venv/bin/python -m pytest` (14 routes); U6. P1/U3 skipped: tox's commands depend on factors (`tests: pytest`) | `tests-in-venv` (`.venv/bin/python -m pytest` after pytest, python -m pytest, python3 -m pytest, tox, nox): bound for all but nox, which attrs does not declare |
| click | the same as attrs | U11 as for attrs (11 routes, `No module named 'click'`); U3 the three pytest spellings failing naming `tox -e py` → `tox -e py` (pyproject `[tool.tox.env_run_base]`); U6 | `tests-in-venv`: bound as for attrs |
| marshmallow | the same as attrs | as click (`No module named 'marshmallow'`; tox.ini `commands = pytest {posargs}`) | `tests-in-venv`: bound as for attrs |
| urfave/cli | U6 `go.sum`; `command_test.go`, `flag_test.go` skipped as larger than 64 KiB | U6 `go.sum`; nothing skipped (the `go:generate` scan reads their first 64 KiB) | `godoc-snapshot` (a sequence after `*.go` edits behind a `%:` pattern rule): not expressible as an instinct; not bound |
| vuejs/core | N1 `npm test` fails (empty signature) → `pnpm test`, delivered twice on genuine vitest failures, ignored both times; N1 `npm install`/`npm ci` (empty signatures) → `pnpm install` | N1 `npm install`/`npm ci` fail with `Unsupported URL Type` (`catalog:` dependencies) or only-allow's `for installation in this project`, `npm ci` with `an existing package-lock.json` → `pnpm install` (nascent); `yarn test`/`yarn install` missing or refused by corepack → `pnpm test`/`pnpm install`; `npm test` **not bound** (it runs the same `vitest`) | `no-nr` (`nr test` → `pnpm test`): not bound, `nr` is named only in the contributing guide; `vitest-by-package` (a scope): not an instinct |

The signatures were checked against the tools' real output here: `python3 -m pytest` prints
`/usr/local/bin/python3: No module named pytest`, a system `pytest` prints `ModuleNotFoundError:
No module named 'click'` at collection, and vuejs/core's `npm ci` / `npm install` print ``The `npm
ci` command can only install with an existing package-lock.json`` and `Unsupported URL Type
"catalog:": catalog:` (npm 10.9).

## Binding

### Declared facts

[`declared::extract`](../crates/adage-instincts/src/declared.rs) reads, from the repository root:

- **Makefile** (`GNUmakefile`, `makefile`, `Makefile`): targets, prerequisites and recipes, with
  variables (`=`, `:=`, `?=`, `+=`), `$(wildcard …)`, `$(MAKE)` and `$@ $< $^` expanded. A target's
  body is its prerequisite targets' recipes, then its own. `!=` and `$(shell …)` are never
  expanded; a body that still contains `$(` is refused.
- **package.json**: scripts, with `pre<name>`/`post<name>` hooks and nested `npm run x` / `yarn x`
  references expanded into the body; `packageManager`; the lockfile type. A script is directed with
  the repository's package manager (`npm test`, `pnpm run check`).
- **pyproject.toml** (`tool.pytest.ini_options`, `tool.tox`, `tool.uv`, `tool.poetry`,
  `dependency-groups`), `tox.ini`, `setup.cfg`, `pytest.ini`, `conftest.py`, `noxfile.py`. tox
  becomes the entry `tox -e py` whose body is the base test environment's commands (`[testenv]
  commands`, or `[tool.tox.env_run_base] commands`) without `{posargs}`; factor-conditional commands
  (`tests: pytest`) refuse it. nox becomes `nox -s tests` (a `@nox.session` `def tests(`), always
  refused: a session is Python code. The uv dependency group that has pytest (`uv run` syncs the
  default groups, `dev` unless `tool.uv.default-groups` says otherwise; `include-group` followed).
- **The project's environment and packages**: a `.venv` or `venv` holding a `pyvenv.cfg` (only its
  existence; nothing inside a virtual environment is read), and the project's import packages
  (`src/<name>/__init__.py`, else `<name>/__init__.py` at the root; at most three).
- **Node guards**: a `preinstall` `only-allow <pm>`, `devEngines.packageManager`, and root
  dependencies through `workspace:`, `catalog:`, `link:`, `patch:` or `portal:`.
- **Cargo.toml** workspace members; **go.mod** module and `//go:generate` lines from up to 200
  `.go` files (a file over 64 KiB is scanned as far as its first 64 KiB, and listed in
  `DeclaredFacts::partial_files`, not skipped).
- **justfile** and **Taskfile.yml** recipe names (counted as test entry points; their bodies are
  not directed yet, though `just` and `task` are now in adage-core's command table).
- **`.github/workflows/*.yml`** `run:` steps, by line (no YAML parser; a workflow is untrusted).
- **Generated-file markers** in the first 4 KiB of up to 400 code files, and **generator links**:
  for each entry that regenerates, its inputs (Makefile prerequisites, else the data paths and
  `dir` + `"*.ext"` pairs its generator script names) and outputs.
- **Lockfiles** at the root.

Every declared command is classified twice with adage-core's normaliser: the command Adage would
direct (`adage_tracks::io::classify`, exactly as track validation does), and the body it expands to
([`safety::check_body`](../crates/adage-instincts/src/safety.rs)). Each entry carries its verdict:
`plain` with the body's strongest effect, or `refused` with the reason.

### From facts to routes

The binder ([`bind.rs`](../crates/adage-instincts/src/bind.rs)) runs the detectors in a fixed order
(U1, U11, P1, N1, U3, U5, U4, U6, then the disabled ones) and, for every candidate:

1. **One directive per trigger.** A failure (a failing command *and* its signature) or an edited
   glob already claimed by an earlier instinct is not bound again (specific before generic: U1's
   wrapper, then the project environment, before U3's test command). The same command failing for
   a different reason is a different trigger: `pytest` failing on `DJANGO_SETTINGS_MODULE` is
   answered by `make test`, `pytest` not found by `uv run pytest`.
2. **Ids the learner would derive.** A dead-end-shaped route directing `S` takes
   `dead-end-<stable(key(S))>`, the id the learner gives the dead end whose working command is `S`
   (`adage_graph::compile::stable`); further triggers for the same `S` take
   `dead-end-<stable(key(S), key(R))>`, and further signatures of the same runner
   `dead-end-<stable(key(S), key(R), signature)>`. A finish line takes `finish-line-<stable(glob, command)>`,
   as the learner's obligation does. Shapes the learner does not produce take `instinct-<id>-<stable(…)>`. `BindResult::origins` maps every id to the
   instinct that bound it.
3. **Override rules** (learned wins). A candidate is skipped, with the reason, if a route from any
   other source (learned, imported, authored; routes with an `instinct:` provenance are ignored):
   - has its id;
   - avoids its command (a dead end whose `avoid` names `S`);
   - already directs something for the same trigger (a dead end avoiding R, an after-failure route
     on R, a finish line on the same glob), whether the same command or a different one;
   - or a person rejected its id in `adage review`.
   A skipped candidate still claims its trigger, so a lower-priority instinct cannot slip in behind
   the learned route.
4. **Capped.** Strength prime; state `forming`, except an environment step (`pnpm install`),
   which is a `nascent` until a person approves it (Design O11); no slots; exit "the user is asking
   for something else" (dead-end shapes) or "this is an exploratory or draft change" (finish lines);
   evidence empty with provenance `instinct:core@0.1.0`.
5. **Validated.** [`adage_tracks::io::validate`](../crates/adage-tracks/src/io.rs) on each route,
   then the instinct rules it does not know: prime only, a run step only, plain argv, the directed
   command's own class no stronger than the route's effect, effect at most regenerate (or
   environment in nascent), a reactive trigger (after a failure or an edit — never session start,
   never a prompt), a repository-relative glob, a renderable and **non-empty** signature (a failure
   route without one would fire on any failure), the instinct provenance, and no non-ASCII, control
   or invisible character anywhere in the serialised route.

### Failure signatures

The runtime fires an `after_failure` route when the failing call's output contains its signature.
An instinct's signature must say the command itself was the wrong way in, never merely that it
failed; a test that fails on its assertions fails the same way under any runner, and directing
another command then only costs the agent a turn (RepoBench: vuejs/core's `npm test` → `pnpm test`
fired on genuine vitest failures). The signatures each instinct uses:

| Why the command was wrong | Signature | Instincts |
| --- | --- | --- |
| A guard for the setup the wrapper supplies | the env variable's name (`ACME_ENV`, `DJANGO_SETTINGS_MODULE`) | U1, P1, U3 |
| A missing `--import` / `--require` loader | `ERR_MODULE_NOT_FOUND`, `Cannot find module` | U1, P1, U3 |
| The repository's own message names the command | the directed command (`make check`); never for a package-manager script, which echoes its body | U1, U3 |
| The runner is not installed | `pytest: command not found`, `command not found: pytest` (zsh), `No module named pytest`, `python: command not found`, `tox: command not found` | U11, P1 (tox), U3 (managed environment) |
| The project's own package is not importable | `No module named '<package>'` for the project's own packages, never a submodule or another name | U11, P1 (tox), U3 |
| A declared test dependency is not importable | `No module named '<dependency>'` for pyproject's `test`, `tests`, `testing` and `dev` dependency groups and optional dependencies (at most 12), after a bare `python`/`python3` run | U11 (advice) |
| npm cannot install this manifest | `Unsupported URL Type` (`workspace:`, `catalog:`, … dependencies) | N1 |
| The repository refuses npm | only-allow's `for installation in this project`; `EBADDEVENGINES` (`devEngines.packageManager`) | N1 |
| `npm ci` without an npm lockfile | `an existing package-lock.json` | N1 |
| The other of yarn/pnpm is missing or refused | `yarn: command not found`, `command not found: yarn`, corepack's `configured to use pnpm` | N1 |

`npm test` in a pnpm, yarn or bun repository binds only on `EBADDEVENGINES`: otherwise it runs the
very script `pnpm test` runs.

Advisories (U4, U6, N1's "uses pnpm") are validated the same way against their template's slot
contracts: `path` a repository-relative glob, `command` plain argv classifying at regenerate or
below, `manager` one of the catalogue's lockfile managers, `lockfile` a known lockfile name.

Advisories (U4, U6, N1's "uses pnpm") bind as routes with `Step::Advise { template, slots }`:
prime, observe, forming, ids `instinct-<tag>-<stable>`, triggered after an edit (the generated
file, the lockfile) or after a command (`npm install`).

The API:

```rust
pub fn bind(repo_root: &Path, existing: &[Route], rejections: &BTreeSet<String>) -> BindResult;
pub fn bind_with(repo_root: &Path, existing: &[Route], rejections: &BTreeSet<String>, opts: &BindOptions) -> BindResult;
pub fn bind_facts(facts: &DeclaredFacts, existing: &[Route], rejections: &BTreeSet<String>, opts: &BindOptions) -> BindResult;

pub struct BindResult {
    pub routes: Vec<Route>,               // sorted by id; run steps and advisory steps
    pub skipped: Vec<(String, String)>,   // (instinct id or "declared", reason), sorted
    pub origins: BTreeMap<String, String>,// route id → instinct id
}
impl BindResult {
    pub fn advisories(&self) -> Vec<Advice<'_>>;  // the advisory routes, through their templates
    pub fn runs(&self) -> Vec<&Route>;             // the routes that direct a command
}
pub struct BindOptions { pub disabled: BTreeSet<String> }
```

Every bound route carries `evidence.provenance = "instinct:<pack>@<version>"`, `evidence.origin =
Origin::Instinct { pack, version, instinct, ecosystem, declared }` (the catalogue id, the ecosystem
the wrapper names, and the file that declares what it directs) and `evidence.by_origin =
{declared: 1}`; `support` and `opportunities` stay 0.

**Determinism.** The same repository files produce byte-identical `BindResult`s, wherever the
repository lives: every collection is a sorted `Vec` or a `BTreeMap`, walks are sorted, and no
absolute path appears in the output (tested by binding each fixture twice and from a copy).

## Safety rules

A repository is untrusted input, exactly like a track.

- **Read only, bounded.** The binder never executes anything. Whole-file reads (Makefile,
  package.json, TOML, workflows, generator scripts) skip files over 64 KiB, since a parse of part
  of one could bind a body that is not the real one; line scans (`//go:generate`) read the first
  64 KiB up to the last whole line; marker scans read at most the first 4 KiB. At most 512 reads and 5,000 directory entries
  per bind, depth 8; `node_modules`, `target`, `vendor`, virtual environments and hidden
  directories (except `.github`) are never entered. Every skipped file is reported.
- **No symlink out of the repository.** Every path is canonicalised and must stay under the
  canonical root; walks never follow a symlink at all.
- **A body binds only if it is plain.** Refused outright: pipes, redirections, `$` expansions and
  command substitutions, backticks, backslashes, background jobs, any non-ASCII or non-printable
  character (so no invisible or bidirectional text), paths outside the repository in an argument, a
  flag value or an env value (absolute, `~`, `..`), network and privilege tools (`curl`, `wget`,
  `ssh`, `sudo`, …), `eval`/`source`/`exec`, `rm`/`chmod`/`kill`, env variables that load code or
  change lookup (`PATH`, `NODE_OPTIONS`, `LD_*`, `PYTHONSTARTUP`, …), repository scripts whose name
  says they deploy, publish, install or delete, and every action the normaliser classifies above
  regenerate or as author/publish, or that writes or pipes (`mutating`).
- **Two narrow exceptions**, still subject to every rule above: a script inside the repository run
  by an interpreter (`node tools/codegen.mjs`) is the repository's own code, which the declared
  command runs anyway, and is neutral; a known code generator (`stringer`, `protoc`, `sqlc`, … from
  the catalogue) counts as regenerate.
- **The directed command is classified by adage-core alone**, and must be at most verify for a
  failure route (U1, U3, P1, N1's test route) and at most regenerate for a finish line.
- **Decoys.** Only canonical check names (`test`, `tests`, `check`, `ci`, `verify`, `unit`, …) can
  explain a failure (U1); U3 needs exactly one test entry point; a generator with more than three
  input patterns binds nothing.
- **No free text reaches the agent.** Routes carry only commands, keys, globs and signatures that
  pass track validation; advisories carry only contract-checked slot values for fixed templates.

## Limits of this version

- **Failure triggers match by runner, for instincts only.** An instinct's `after_failure` trigger
  fires on any run of the same runner (`adage_core::pattern::runs_avoided`: `python3 -m unittest
  tests.test_billing -v` for `python3 -m unittest`); learned and imported routes still need the
  directed key or it with options added.
- **No repeat-failure trigger.** U2 stays disabled: `Trigger::RepeatFailure` does not exist.
- **Runners and wrappers in the command table.** adage-core classifies `uv run X`, `uvx X`,
  `poetry run X`, `pdm run X`, `hatch run X`, `pipenv run X`, `pnpm exec X`, `npm exec X`, `bunx X`
  as `X` (`wraps` in `data/commands.toml`); `uv sync`, `uv pip install`, `uv lock`, `poetry install`
  are environment; `tox` and `nox` verify; `just` and `task` by recipe name as for make;
  `pnpm check`/`yarn vitest` as `run check`/`vitest` (`run_shorthand`); `pnpm --filter x test` skips
  the leading flags (`leading_value_flags`); `cargo nextest` verifies. A signature is still needed
  before any of them is directed after a failure.
- **Signatures are fixed strings.** A failure message in another language or a different shell's
  wording (fish, PowerShell) is not recognised, and the instinct stays silent.
- **Generator-script inference** (U5, U4) reads string literals; a generator that builds its paths
  dynamically is not linked. Declared Makefile prerequisites always take precedence.
- **Root only.** Workspace packages' own manifests and lockfiles are not read yet (N3).

## Integration

Instincts are wired into every crate that learns, compiles, decides, shows or measures routes. What
each part does, and where:

### adage-tracks (the model and the index)

- **Provenance.** `Evidence.origin: Option<Origin>` (`learned`, `instinct {pack, version,
  instinct, ecosystem, declared}`, `imported`, `authored`, `synthetic`; serde default, so every
  older track reads), `Route::origin()` (the recorded origin, else what `provenance` says) and
  `Route::is_instinct()`. `Evidence.by_origin` (serde default) keeps counts by origin apart from
  the organic `support`/`opportunities` (roadmap E1): `declared: 1` for an instinct, then its
  runtime judgements (`followed`, `misfired`, `ignored`). Validation checks the origin's
  fields (a closed ecosystem list, a repository-relative declared file) and refuses an instinct
  that is not prime.
- **`Step::Advise { template, slots }`** with the closed template set in `adage_tracks::advice`
  (`generated-file`, `lockfile`, `package-manager`) and each slot's contract (`path` a
  repository-relative glob, `command` plain argv at most regenerate, `manager` and `lockfile` from
  the lockfile table). Validation refuses an unknown template, an unknown or missing slot, a value
  that fails its contract, and anything but prime + observe after an edit, a command or a failure.
- **Delivery shape** (`adage_tracks::shape`): `after_failure` + `run` is `avoid` (the shape of the
  learned route sharing the `dead-end-*` id), and an advisory step is `advise`.
- **Index precedence by provenance** (`Index::compile`): on an id collision, a first-hand route
  (learned or authored) beats an import, which beats an instinct, whatever the track names sort to;
  an extinguished first-hand route shadows an instinct with its id, as it shadows imports.
- **Interlocking** (`interlock::resolve`): instincts have the lowest precedence. An instinct that a
  route from any other source contradicts at the same decision point (a different command) is left
  out of the index (`ConflictKind::Instinct`, reported by `adage condition` and `adage doctor`).
- **No repeat-failure trigger**: U2 stays disabled, so `Trigger::RepeatFailure` was not added.

### adage-learn

- **Bind in the plan, per repository** (`adage_learn::instincts`). After mining and review
  reconciliation, `plan()` calls `adage_instincts::bind_with(root, others, rejections, opts)` when
  `LearnConfig::instincts` is set: `others` are the repository's routes just planned plus its other
  installed tracks and imports; `rejections` are the ids rejected in any of the repository's
  `review.yaml` files. The routes go to the track `<repo>/instincts`, provenance
  `instinct:<pack>@<version>`, versioned by the usual semver rules, and appear in the report (`bound`,
  `advice`, `nascent reflexes`, `skipped`, `extinguished`, `confirmed`) and, as counts, in `learn.log`.
- **The repository's root** is the live checkout: the root the caller names (`Config::roots`:
  automatic learning names the hook's root, the session's own checkout; `adage condition` names the
  checkout it runs in), else the working directory most of its sessions recorded that still exists
  (`instincts::root_of`). Old traces may name checkouts that have since lost their `.venv` or
  `node_modules` (per-task worktrees, containers), which bound the wrong instincts (launch report,
  bug 7). Unknown: nothing is bound, with the reason in the report.
- **Rebinding.** Each binding is recorded beside the repository's tracks
  (`.instincts-binding.json`: the checkout and a fingerprint of its declared files,
  `adage_instincts::fingerprint`, a few dozen `stat`s). A session start whose checkout differs, or
  whose declared files changed since (a `.venv` made, a Makefile edited), asks the learner to bind
  again, by the bootstrap's fast first phase.
- **Day 0.** `adage condition --instincts-only [--repo-path <dir>]` binds a repository with no sessions
  (`instincts::learn_only`), and an automatic learn for a repository with no usable session yet
  binds its instincts once from the root the hook named.
- **Never learn from instincts.** A session that received an instinct's directive is steered by
  influence, exactly as for a reflex: what the directive names is exposed from then on and is no
  evidence for a reflex directing it.
- **Hand-over.** When the learner learns a route with an instinct's id, the instinct leaves its track;
  `learn.log` says "instinct route confirmed by sessions: the learned route takes over".
- **Review.** Extinguished ids are never bound again, and an extinguished instinct stays in the
  track. Environment steps stay nascent reflexes until approved; approving one approves every environment
  step of its pack for the repository (`review.yaml`, `packs:`). Decisions never raise an instinct
  above prime or past forming.
- **Auto-extinguish, conservatively.** The spools' judgements are read at learn, by arm. An instinct
  route with at least 3 `misfired` (followed, and the directed command failed for want of its
  environment or target: `adage_core::outcome::misfired`, never a test run that reported failing
  tests), and no fewer misfires than working follows, is extinguished. An ignore rate alone never
  extinguishes: an "ignored" may be a follow the matcher missed (the launch report's 41 of 41). It
  extinguishes (Wilson lower bound at least 0.5 over at least 5 judgements) only with a control-arm
  comparison of at least 5 judgements that shows follows of the route are seen at all and that
  delivering it made agents take the step no more often than withholding it. An extinction is
  recorded as a person's `extinguish` would be in its `review.yaml` (`by: "adage: misfired 3
  times"`), so a re-bind does not bring it back; `learn.log` records it.

### adage-runtime

- **Wrapper per provenance.** An instinct's wrapper carries `origin="instinct"` after `kind`, and
  its last line is `Adage default for <ecosystem> repositories (from this repository's <file>); not
  requested by the user.` (`repositories like this one` without an ecosystem), never "Learned by
  Adage from this repository's earlier agent sessions".
- **Templates.** `after_failure` + `run`: "That failed. In this repository, run this instead:" with
  the command and the exit on their own lines. An instinct's finish line ends with `<file> declares
  this.` (or "This repository's build declares this step."), never "CI fails if this is skipped."
  or "Agents in this repository run it after such changes.". Advice renders its fixed sentence.
- **Never a Stop request.** An instinct finish line makes no obligation (a follow judgement
  instead), so no Stop hook ever asks the agent to continue for it; a rebuilt state makes none
  either. Instincts are delivered prime whatever a track says (`delivered_strength`).
- **`Judgement::FollowedFailed`**: the directed command ran within the follow window and failed
  (a failure event, or a batch call marked failed). Directives after a failure are judged too. The
  fast path decides a failure in full whenever a judgement is pending.

### adage-cli

- **Config**: `instincts.enabled` (default `true`), `instincts.packs` (default `core`),
  `instincts.disabled` (ids, comma-separated); in `adage config` and the settings tab of `adage ui`.
  `adage condition --no-instincts` skips them for one run.
- **`adage init`** shows what would bind for the current repository (a dry bind), explains pack
  approval, and prints `adage condition --instincts-only`; `adage init --yes` runs it.
- **`adage doctor`**: an `instincts` check with, per repository, the routes bound and delivered,
  advice, nascent reflexes awaiting approval, and those overridden.
- **`adage review`** lists instinct routes with their origin (`instinct U1 (from Makefile)`) until a
  person decides; `strengthen` is never offered or accepted for them. `adage status` labels the
  instincts track.

### adage-eval

- **`adage eval <dataset> --instincts-only`**: nothing is learned from the train split; the index
  holds only what binds from the repository's real files (`--sandbox-dir`, default
  `evals/sandboxes/<repo>`; `evals/sandboxes/acme-billing` is the demo sandbox), and the test
  sessions are replayed. The report's `instincts` block gives precision of instinct directives
  (dead ends and finish lines separately), advice given, and the share of failed verify runs an
  instinct answered with a command the agent then ran (day-0 recall). `--instincts` binds them
  after learning, as `adage condition` does. Instincts are off by default, so the committed numbers and
  the L0 baseline are learned routes alone; `adage eval bench run --instincts` measures with them.
- **The oracle** (`safety::check_wrapper`) accepts the instinct wrapper, the declared-source
  finish-line sentence and the advisory sentences exactly, with each advisory value equal to its
  route's slot, and rejects anything else; the property tests generate instinct-provenance routes
  and advisory steps.
- **Gates** (`tests/gates.rs`): instincts-only on the demo gives at least one correct dead end and
  one correct finish line, no unsafe directive, and learns nothing; with learning, every directive
  stays safe on the demo, synthetic and adversarial sets.

Measured on the demo (`demo-haiku45`, instincts only, nothing learned; docs/evals.md), before
failure signatures; with them the gates (`tests/gates.rs`: precision ≥ 0.89, dead-end recall
≥ 0.73) still hold, because the demo's failures print `ACME_ENV is not set — run tests with 'make
test'`, `No module named pytest` and `command not found: python`:

| Split | Directives | Correct | Dead ends | Finish lines | Failed verify runs answered |
| --- | --- | --- | --- | --- | --- |
| hash:0.3 (default) | 11 | 10 (90.9%) | 3/4 | 7/7 | 3/4 |
| all (steered included) | 55 | 48 (87.3%) | 26/32 | 22/23 | 26/32 |

Advice was never triggered (no agent edited the generated file). The L0 bench (`core` suite): J
0.6307 without instincts (the baseline, unchanged), 0.6466 with them (`--instincts`); unsafe
directives and fast-path mismatches 0 either way.

### Not done yet

- **U2 (no blind retry)** needs `Trigger::RepeatFailure`; it stays disabled.
- **Synchronous first-event availability**: SessionStart asks the background learner to bootstrap
  instincts in an unseen or changed checkout; an event that arrives before it finishes may have
  no defaults yet. `adage init --yes` binds before returning.
- **U11 for node** (`node_modules/.bin`) and for poetry/pdm projects (their `run` does not sync the
  environment, so it would not answer a missing one).
- **Workspace packages** (N3) and the other P1 instincts, each with its missing trigger or slot.
- **Documented-only tools** (vuejs/core's contributing guide says `nr test`): instincts read
  declared files, not prose.
