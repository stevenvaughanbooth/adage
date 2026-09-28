# Instincts

Every route Adage had until now is a **reflex**: a route learned from a repository's own agent
sessions. A reflex needs history, so a repository Adage has never seen gets nothing on day 0.

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

## Catalogue

`P0` instincts bind now. `P1` instincts are catalogued with their detection and cap, and ship
disabled with stub detectors, until the model can express them (see [Not done yet](#not-done-yet)).
U2 is P0 on the roadmap but is disabled here: it needs a trigger the model does not have.

| Id | Priority | Instinct | Trigger | Directive (fixed template, sketched) | Cap | Detection (no history) | FP risk | Enabled |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| U1 | P0 | Declared wrapper explains a failure | `after_failure` of runner R (signature: the env var the wrapper sets, or `ERR_MODULE_NOT_FOUND` for an `--import` loader) | "A known next step here is: `S`" | prime | A Makefile target or package.json script with a canonical check name (`test`, `check`, `ci`, …) whose plain body runs R plus env vars, setup flags or `--import`, or runs a repository script that drives R (`unittest.TextTestRunner`, `pytest.main`) | low | yes |
| U2 | P0 | No blind retry | `repeat_failure` (same key, same signature, twice, no edit between) | "That failed the same way twice; read the error before retrying" | prime | Runtime history only; timeouts and connection errors exempt | medium | no |
| U3 | P0 | The repository's declared test command | `after_failure` of an undeclared runner (`pytest`, `python -m pytest`, `npm test`, …) | "A known next step here is: `S`" | prime | Exactly one test entry point (Makefile `test`/`tests`, package.json `test`, tox, nox, just, Taskfile), body plain and at most verify; CI `run:` steps break ties | low-med | yes |
| U4 | P0 | Don't hand-edit generated files | `after_edit` of the generated file (advisory) | "`path` is generated: change its inputs and run `command` rather than editing it by hand" | prime | `Code generated … DO NOT EDIT`, `@generated` in a comment in the first 40 lines; files a declared generator writes (Makefile file rules, paths named in the generator script) | very-low | yes |
| U5 | P0 | Declared regeneration | `after_edit` of the generator's inputs | the finish-line template: "changes to files matching `glob` must be followed by running: `command`" | prime | Makefile rule prerequisites (`models.py: schema/*.json`, `$(wildcard …)`), else input paths named in the generator script (`Path("schema")` + `"*.json"`) | low | yes |
| U6 | P0 | Don't hand-edit lockfiles | `after_edit` of the lockfile (advisory) | "`path` is maintained by `manager`: change the manifest …" | prime | A lockfile at the repository root (npm, pnpm, yarn, bun, cargo, go, poetry, uv, pipenv, pdm, bundler, composer) | very-low | yes |
| N1 | P0 | Package manager discipline (node) | `after_failure` of `npm install` / `npm ci` / `npm test`; advisory after `npm install` succeeds | "A known next step here is: `pnpm install`" / "`pnpm test`" | prime | `packageManager` field or a pnpm/yarn/bun lockfile, and no conflicting npm lockfile | very-low | yes |
| P1 | P0 | Python runner wrapper / `uv run` | `after_failure` of `pytest`, `python -m pytest`, `python3 -m pytest` | "A known next step here is: `make test`" | prime | pytest configured (pyproject, pytest.ini, setup.cfg, tox.ini, conftest.py) and a Makefile wrapper that sets env or flags; tox, nox and `uv run` wrappers are detected and reported, not bound | low | yes |
| U7 | P1 | Manifest changed, sync environment | `after_edit` of a dependency section | "Run `pnpm install`" | prime | Package manager from the lockfile; pack-level approval (environment effect) | medium | no |
| U8 | P1 | CI runs checks the agent skipped | `after_edit` of files CI lints | "CI runs `…`; run it before finishing" | prime | CI `run:` steps that classify as verify | low | no |
| U9 | P1 | Watch mode never exits | `after_failure` (timeout) of a watch command | "Run the one-shot form: `vitest run`" | prime | Table: `jest --watch`, `vitest`, `tsc -w` … | low | no |
| U10 | P1 | Missing dependency is declared | `after_failure` with `ModuleNotFoundError` / `Cannot find module` | "It is declared; install the project's dependencies" | prime | Only if the module is in the manifest (rules out typosquatting); pack-level approval | low | no |
| U11 | P1 | Use the project's environment | `after_failure` with `command not found` | "Run it through the project's environment" | prime | `.venv`, `uv.lock`, `node_modules/.bin` | low | no |
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
| `acme` (demo/sandbox) | U1 `python3 -m unittest` fails naming `ACME_ENV` → `make test` (`dead-end-d41b1e`, the id the demo learned); U3 `pytest`, `python -m pytest`, `python3 -m pytest`, `python -m unittest` fail → `make test`; U5 `schema/*.json` → `make gen` (`finish-line-e7e809`, the id the demo learned) | U4 `acme/generated/models.py` (`make gen`) |
| `harbor` (harbor-dispatch) | U1 `npm test` and `node --test` fail with `ERR_MODULE_NOT_FOUND` → `npm run check`; U5 `registry/*.csv` → `npm run codegen` (`finish-line-04e5d4`, the id learned in the live runs) | U4 `src/generated/registry.ts` (`npm run codegen`) |
| `pnpm` | N1 `npm install`, `npm ci` fail → `pnpm install` (environment: **nascent**, waits for review); `npm test` fails → `pnpm test` | N1 "uses pnpm" after `npm install`; U6 `pnpm-lock.yaml` |
| `rust` (workspace) | none (R1, R2 disabled) | U6 `Cargo.lock` |
| `go` (`//go:generate stringer`) | none (G1 disabled) | U4 `strength/strength_string.go` (`go generate ./...`); U6 `go.sum` |
| `pytool` (pytest + Makefile wrapper, uv, prerequisite rule) | U1/P1 every spelling of pytest fails naming `DJANGO_SETTINGS_MODULE` → `make test`; U3 unittest → `make test`; U5 `schema/*.json` and `tools/gen.py` → `make gen` | U4 `quayside/models_gen.py` (`make gen`); U6 `uv.lock` |
| `decoy` (bench, test-legacy, docs, release targets) | none | none |
| `hostile-npm`, `hostile-hooks`, `hostile-make`, `hostile-unicode` | none | none |
| `hostile-ci` (injection text in a workflow) | only the plain `make test` target; no workflow text reaches anything bound | none |

Symlinks out of the repository, files over 64 KiB, invisible characters in target names and
pathological manifests are covered by tests that build their repositories at test time.

On the generated `smoke` suite (`evals/gen`, six sandboxes over python-make and node-npm, bound
from their files alone by `tests/generated.rs`): **6 of 6 dead ends and 6 of 6 finish lines
recovered on day 0; 0 of 13 decoys and 0 of 4 instinct violations bound.** The violations
(`make test` / `npm test` retired and printing a failure) are refused because their bodies redirect
or run an unknown script; the python dead ends are recovered through a runner script
(`tools/testkit.py` driving `unittest`).

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
- **pyproject.toml** (`tool.pytest.ini_options`, `tool.tox`, `tool.uv`, `tool.poetry`), `tox.ini`,
  `setup.cfg`, `pytest.ini`, `conftest.py`, `noxfile.py`.
- **Cargo.toml** workspace members; **go.mod** module and `//go:generate` lines from up to 200
  `.go` files.
- **justfile** and **Taskfile.yml** recipe names (counted as test entry points; `just` and `task`
  are not in adage-core's command table, so they are never directed).
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
(U1, P1, N1, U3, U5, U4, U6, then the disabled ones) and, for every candidate:

1. **One directive per trigger.** A failing command or an edited glob already claimed by an earlier
   instinct is not bound again (specific before generic: U1's wrapper before U3's test command).
2. **Ids the learner would derive.** A dead-end-shaped route directing `S` takes
   `dead-end-<stable(key(S))>`, the id the learner gives the dead end whose working command is `S`
   (`adage_graph::compile::stable`); further triggers for the same `S` take
   `dead-end-<stable(key(S), key(R))>`. A finish line takes `finish-line-<stable(glob, command)>`,
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
   never a prompt), a repository-relative glob, a renderable signature, the instinct provenance, and
   no non-ASCII, control or invisible character anywhere in the serialised route.

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

- **Read only, bounded.** The binder never executes anything. Whole-file reads skip files over
  64 KiB; marker scans read at most the first 4 KiB. At most 512 reads and 5,000 directory entries
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
- **Unclassified runners.** `uv`, `tox`, `nox`, `poetry`, `just` and `task` are not in adage-core's
  command table, so they classify as environment and are never directed; they are detected and
  reported in `skipped`.
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
- **The repository's root** is the working directory most of its sessions recorded that still
  exists (`instincts::root_of`), or the root the caller names (`Config::roots`: automatic learning
  names the hook's root). Unknown: nothing is bound, with the reason in the report.
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
- **Auto-extinguish.** The spools' treated judgements are read at learn (as `adage review` reads them).
  An instinct route with at least 3 `misfired`, or whose ignore rate has a Wilson lower bound
  of at least 0.5 over at least 5 judgements, is extinguished and recorded as a person's `extinguish` would be in its
  `review.yaml` (`by: "adage: misfired 3 times"`), so a re-bind does not bring it back;
  `learn.log` records the extinction.

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

Measured on the demo (`demo-haiku45`, instincts only, nothing learned; docs/evals.md):

| Split | Directives | Correct | Dead ends | Finish lines | Failed verify runs answered |
| --- | --- | --- | --- | --- | --- |
| hash:0.3 (default) | 11 | 10 (90.9%) | 3/4 | 7/7 | 3/4 |
| all (steered included) | 55 | 48 (87.3%) | 26/32 | 22/23 | 26/32 |

Advice was never triggered (no agent edited the generated file). The L0 bench (`core` suite): J
0.6307 without instincts (the baseline, unchanged), 0.6466 with them (`--instincts`); unsafe
directives and fast-path mismatches 0 either way.

### Not done yet

- **U2 (no blind retry)** needs `Trigger::RepeatFailure`; it stays disabled.
- **SessionStart bind in the hook**: binding happens in the background learner at the end of a
  repository's first session, or through `adage condition --instincts-only` / `adage init`; not yet
  when a session *starts* in an unseen repository.
- **The interactive `adage init` wizard** does not have an instincts step (the plain `adage init`
  does).
- **adage-core command-table entries** for `uv`, `tox`, `nox`, `poetry run`, `just` and `task`, so
  P1's wrappers can be directed; until then they are reported, not bound.
- **Workspace packages** (N3) and the other P1 instincts, each with its missing trigger or slot.
