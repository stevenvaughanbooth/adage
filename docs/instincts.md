# Instincts

Every route Adage had until now is a **reflex**: a route learned from a repository's own agent
sessions. A reflex needs history, so a repository Adage has never seen gets nothing on day 0.

An **instinct** is a prebuilt route that ships with Adage. It is a template (data, in
[`crates/adage-instincts/data/catalogue.toml`](../crates/adage-instincts/data/catalogue.toml)) plus a
detector (Rust, in [`src/detect.rs`](../crates/adage-instincts/src/detect.rs)). At learn time a
**binder** reads the repository's *declared facts* from its files and binds each enabled instinct
into concrete routes for that repository. Nothing comes from session history, so instincts work the
first time Adage sees a repository. They are weaker than reflexes by design: yellow at most, never a
Stop block, and a reflex that says otherwise always wins.

This document is the catalogue, the binding and override model, the safety rules, and the plan for
wiring the crate into learning, the runtime and the CLI (the crate is self-contained today; nothing
calls it yet).

## Catalogue

`P0` instincts bind now. `P1` instincts are catalogued with their detection and cap, and ship
disabled with stub detectors, until the model can express them (see [Integration plan](#integration-plan)).
U2 is P0 on the roadmap but is disabled here: it needs a trigger the model does not have.

| Id | Priority | Instinct | Trigger | Directive (fixed template, sketched) | Cap | Detection (no history) | FP risk | Enabled |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| U1 | P0 | Declared wrapper explains a failure | `after_failure` of runner R (signature: the env var the wrapper sets, or `ERR_MODULE_NOT_FOUND` for an `--import` loader) | "A known next step here is: `S`" | yellow | A Makefile target or package.json script with a canonical check name (`test`, `check`, `ci`, …) whose plain body runs R plus env vars, setup flags or `--import`, or runs a repository script that drives R (`unittest.TextTestRunner`, `pytest.main`) | low | yes |
| U2 | P0 | No blind retry | `repeat_failure` (same key, same signature, twice, no edit between) | "That failed the same way twice; read the error before retrying" | yellow | Runtime history only; timeouts and connection errors exempt | medium | no |
| U3 | P0 | The repository's declared test command | `after_failure` of an undeclared runner (`pytest`, `python -m pytest`, `npm test`, …) | "A known next step here is: `S`" | yellow | Exactly one test entry point (Makefile `test`/`tests`, package.json `test`, tox, nox, just, Taskfile), body plain and at most verify; CI `run:` steps break ties | low-med | yes |
| U4 | P0 | Don't hand-edit generated files | `after_edit` of the generated file (advisory) | "`path` is generated: change its inputs and run `command` rather than editing it by hand" | yellow | `Code generated … DO NOT EDIT`, `@generated` in a comment in the first 40 lines; files a declared generator writes (Makefile file rules, paths named in the generator script) | very-low | yes |
| U5 | P0 | Declared regeneration | `after_edit` of the generator's inputs | the finish-line template: "changes to files matching `glob` must be followed by running: `command`" | yellow | Makefile rule prerequisites (`models.py: schema/*.json`, `$(wildcard …)`), else input paths named in the generator script (`Path("schema")` + `"*.json"`) | low | yes |
| U6 | P0 | Don't hand-edit lockfiles | `after_edit` of the lockfile (advisory) | "`path` is maintained by `manager`: change the manifest …" | yellow | A lockfile at the repository root (npm, pnpm, yarn, bun, cargo, go, poetry, uv, pipenv, pdm, bundler, composer) | very-low | yes |
| N1 | P0 | Package manager discipline (node) | `after_failure` of `npm install` / `npm ci` / `npm test`; advisory after `npm install` succeeds | "A known next step here is: `pnpm install`" / "`pnpm test`" | yellow | `packageManager` field or a pnpm/yarn/bun lockfile, and no conflicting npm lockfile | very-low | yes |
| P1 | P0 | Python runner wrapper / `uv run` | `after_failure` of `pytest`, `python -m pytest`, `python3 -m pytest` | "A known next step here is: `make test`" | yellow | pytest configured (pyproject, pytest.ini, setup.cfg, tox.ini, conftest.py) and a Makefile wrapper that sets env or flags; tox, nox and `uv run` wrappers are detected and reported, not bound | low | yes |
| U7 | P1 | Manifest changed, sync environment | `after_edit` of a dependency section | "Run `pnpm install`" | yellow | Package manager from the lockfile; pack-level approval (environment effect) | medium | no |
| U8 | P1 | CI runs checks the agent skipped | `after_edit` of files CI lints | "CI runs `…`; run it before finishing" | yellow | CI `run:` steps that classify as verify | low | no |
| U9 | P1 | Watch mode never exits | `after_failure` (timeout) of a watch command | "Run the one-shot form: `vitest run`" | yellow | Table: `jest --watch`, `vitest`, `tsc -w` … | low | no |
| U10 | P1 | Missing dependency is declared | `after_failure` with `ModuleNotFoundError` / `Cannot find module` | "It is declared; install the project's dependencies" | yellow | Only if the module is in the manifest (rules out typosquatting); pack-level approval | low | no |
| U11 | P1 | Use the project's environment | `after_failure` with `command not found` | "Run it through the project's environment" | yellow | `.venv`, `uv.lock`, `node_modules/.bin` | low | no |
| N2 | P1 | Path-alias loader | `after_failure` with `ERR_MODULE_NOT_FOUND` on an alias | "Run through the declared loader" | yellow | tsconfig `paths` plus an `--import` loader | low | no |
| N3 | P1 | Monorepo scope | `after_failure` with "Missing script" at the root | "Run it in the package that declares it" | yellow | package.json `workspaces` | medium | no |
| R1 | P1 | Crate scope | `after_edit` in a workspace member | "Test that crate: `cargo test -p x`" | yellow | Cargo workspace members | low | no |
| R2 | P1 | clippy/fmt gate | `after_edit` of Rust files | "CI gates on clippy and fmt" | yellow | CI steps running `cargo clippy -D warnings`, `cargo fmt --check` | low | no |
| G1 | P1 | go generate | `after_edit` of a file with a `//go:generate` line | "Run `go generate ./...`" | yellow | `//go:generate` lines | low | no |
| X1 | P1 | protobuf / sqlc / OpenAPI codegen | `after_edit` of a generator input | "Run the declared generator" | yellow | Generator configs (`buf.gen.yaml`, `sqlc.yaml`, OpenAPI generator configs) | low | no |

Deliberately not shipped, ever: "update snapshots on failure", `--no-verify`, `npm audit fix`,
`rm -rf node_modules`, installing undeclared packages, and any Stop block.

### What binds on the reference repositories

The crate's tests bind every fixture under `crates/adage-instincts/tests/fixtures/`:

| Fixture | Routes | Advisories |
| --- | --- | --- |
| `acme` (demo/sandbox) | U1 `python3 -m unittest` fails naming `ACME_ENV` → `make test` (`dead-end-d41b1e`, the id the demo learned); U3 `pytest`, `python -m pytest`, `python3 -m pytest`, `python -m unittest` fail → `make test`; U5 `schema/*.json` → `make gen` (`finish-line-e7e809`, the id the demo learned) | U4 `acme/generated/models.py` (`make gen`) |
| `harbor` (harbor-dispatch) | U1 `npm test` and `node --test` fail with `ERR_MODULE_NOT_FOUND` → `npm run check`; U5 `registry/*.csv` → `npm run codegen` (`finish-line-04e5d4`, the id learned in the live runs) | U4 `src/generated/registry.ts` (`npm run codegen`) |
| `pnpm` | N1 `npm install`, `npm ci` fail → `pnpm install` (environment: **draft**, waits for review); `npm test` fails → `pnpm test` | N1 "uses pnpm" after `npm install`; U6 `pnpm-lock.yaml` |
| `rust` (workspace) | none (R1, R2 disabled) | U6 `Cargo.lock` |
| `go` (`//go:generate stringer`) | none (G1 disabled) | U4 `aspect/aspect_string.go` (`go generate ./...`); U6 `go.sum` |
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
2. **Ids the miner would derive.** A dead-end-shaped route directing `S` takes
   `dead-end-<stable(key(S))>`, the id `mine::dead_end_routes` gives the dead end whose working
   command is `S`; further triggers for the same `S` take `dead-end-<stable(key(S), key(R))>`. A
   finish line takes `finish-line-<stable(glob, command)>`, as `mine::obligation_routes` does. Shapes
   no miner learns take `instinct-<id>-<stable(…)>`. `BindResult::origins` maps every id to the
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
4. **Capped.** Aspect yellow; state `probation`, except an environment step (`pnpm install`),
   which is a `draft` until a person approves it (Design O11); no slots; exit "the user is asking
   for something else" (dead-end shapes) or "this is an exploratory or draft change" (finish lines);
   evidence empty with provenance `instinct:core@0.1.0`.
5. **Validated.** [`adage_tracks::io::validate`](../crates/adage-tracks/src/io.rs) on each route,
   then the instinct rules it does not know: yellow only, a run step only, plain argv, the directed
   command's own class no stronger than the route's effect, effect at most regenerate (or
   environment in draft), a reactive trigger (after a failure or an edit — never session start,
   never a prompt), a repository-relative glob, a renderable signature, the instinct provenance, and
   no non-ASCII, control or invisible character anywhere in the serialised route.

Advisories (U4, U6, N1's "uses pnpm") are validated the same way against their template's slot
contracts: `path` a repository-relative glob, `command` plain argv classifying at regenerate or
below, `manager` one of the catalogue's lockfile managers, `lockfile` a known lockfile name.

The API:

```rust
pub fn bind(repo_root: &Path, existing: &[Route], rejections: &BTreeSet<String>) -> BindResult;
pub fn bind_with(repo_root: &Path, existing: &[Route], rejections: &BTreeSet<String>, opts: &BindOptions) -> BindResult;
pub fn bind_facts(facts: &DeclaredFacts, existing: &[Route], rejections: &BTreeSet<String>, opts: &BindOptions) -> BindResult;

pub struct BindResult {
    pub routes: Vec<Route>,               // sorted by id
    pub advisories: Vec<Advisory>,        // sorted by id
    pub skipped: Vec<(String, String)>,   // (instinct id or "declared", reason), sorted
    pub origins: BTreeMap<String, String>,// route/advisory id → instinct id
}
pub struct Advisory { id, instinct, template_id, trigger, slots: BTreeMap<String, String>, provenance }
pub struct BindOptions { pub disabled: BTreeSet<String> }
```

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

- **Exact trigger keys.** The runtime matches `after_failure` on the exact canonical key, so
  `python3 -m unittest discover -s tests` is not `python3 -m unittest`. Instincts bind the bare
  spellings agents use; see `Trigger::AfterFailure` prefix matching below.
- **Unclassified runners.** `uv`, `tox`, `nox`, `poetry`, `just` and `task` are not in adage-core's
  command table, so they classify as environment and are never directed; they are detected and
  reported in `skipped`.
- **Generator-script inference** (U5, U4) reads string literals; a generator that builds its paths
  dynamically is not linked. Declared Makefile prerequisites always take precedence.
- **Root only.** Workspace packages' own manifests and lockfiles are not read yet (N3).

## Integration plan

The crate is self-contained. Wiring it in touches four crates; each item below is required before
instincts reach an agent as designed.

### adage-tracks

1. **Provenance on routes and tracks.** Instinct routes carry `evidence.provenance =
   "instinct:<pack>@<version>"` today. Add `pub fn is_instinct(r: &Route) -> bool` (the crate has
   one) and treat a track whose `manifest.track.provenance` starts with `instinct:` like an import
   in [`Index::compile`](../crates/adage-tracks/src/index.rs): the sort key at `index.rs` ~70–78
   must rank first-hand routes before instinct routes explicitly (today the learned route wins only
   because `<repo>/dead-ends` and `<repo>/finish-line` sort before `<repo>/instincts`; the crate's
   test `the_index_keeps_the_learned_route_over_an_instinct_with_the_same_id` pins that). A retired
   first-hand route must shadow an instinct with its id, as it shadows imports.
2. **`Step::Advise { template_id: String, slots: BTreeMap<String, String> }`.** Validation: the
   template id is one of a fixed set owned by the runtime (`generated-file`, `lockfile`,
   `package-manager`), each slot is known to the template and passes its contract (the crate's
   `bind::check_advisory` is the reference), the aspect is yellow, the effect is observe. Then U4,
   U6 and N1's advisory become routes and `BindResult::advisories` goes away.
3. **`Trigger::RepeatFailure { command, signature }`** for U2: the same key failing with the same
   signature twice with no edit in between. Timeouts and connection errors exempt.
4. **Failure-trigger matching by runner.** Either a `prefix: bool` on `Trigger::AfterFailure` or a
   separate trigger that matches the runner (`mine::runner`) rather than the exact key, so
   `python3 -m unittest discover -s tests` and `pytest tests/x.py -q` also fire.
5. **`Evidence.by_origin`** (roadmap E1): keep `support`/`opportunities` organic only; record
   instinct, declared and synthetic counts separately. An instinct never earns green on its own.
6. **`route_kind`** ([`model.rs`](../crates/adage-tracks/src/model.rs)): `after_edit` + `run` is
   `finish-line`, matching the `finish-line-*` ids instincts bind, but `after_failure` + `run` is
   `other`, which excludes a session an instinct steered from *every* miner and renders
   `kind="other"`. Map `after_failure` + `run` to `dead-ends` (the kind of the learned route sharing
   the `dead-end-*` id; `adage_instincts::intended_kind` gives it, and the test
   `route_ids_line_up_with_route_kinds` pins today's gap), and advisories to a new `advice` kind.
7. **A `Stale`-like state for instincts is not needed**: an instinct is re-bound from files on
   every learn, so it never goes stale; it is retired (below) or overridden.

### adage-runtime

1. **Wrapper text per provenance.** [`render::wrap_kind`](../crates/adage-runtime/src/render.rs)
   always says "Learned by Adage from this repository's earlier agent sessions", which is false for
   an instinct. Pass the provenance and render "Adage's built-in default for repositories like this
   one; not requested by the user." for `instinct:` routes. Put `origin="instinct"` in the
   `<adage-signal>` attributes, beside `kind`, so transcripts, steering exclusion and evals can
   tell them apart.
2. **Render `after_failure` + `run`.** Today it falls through to "A known next step here is:";
   give it its own template ("That failed. In this repository, run instead: …"), since instincts
   are the main source of this shape.
3. **Render `after_command` + `run`/`advise`.** `bind_result_route` renders only `read_batch` after
   a command; N1's advisory after `npm install` needs the advise step.
4. **No Stop blocks for instincts.** `decide` turns every `after_edit` + `run` route into an
   obligation, and `Event::Stop` asks the agent to continue for an unmet one. Skip obligations
   whose route is an instinct (deliver the after-edit directive only). The finish-line text needs a
   third sentence for instincts: both `FINISH_CI_FAILS` ("CI fails if this is skipped", said only
   when `deviations_failed > 0`, which is 0 for an instinct) and `FINISH_OBSERVED` ("Agents in this
   repository run it after such changes") claim session evidence an instinct does not have. Use
   "This repository's build declares this step." for `instinct:` routes.
5. **`FollowedFailed` judgement and auto-retire.** A new judgement when the agent followed an
   instinct's directive and the directed command then failed. Three `FollowedFailed` judgements
   retire the instinct route for that repository (recorded like a review rejection, so a re-bind
   does not bring it back); losing authority is always automatic.

### adage-learn

1. **Bind in the plan, per repository.** After mining (so learned routes exist), call
   `adage_instincts::bind(root, &learned_routes_for_repo, &rejected_ids)` where `rejected_ids` are
   the ids with `Decision::Rejected` in the repository's `review.yaml` files plus instinct ids
   auto-retired by `FollowedFailed`. Write the routes as one track `<repo>/instincts`, provenance
   `instinct:core@<version>`, version bumped by the usual semver rules. Needs the repository's root
   on disk; skip repositories whose root is gone.
2. **Never learn from instincts.** Sessions steered by an instinct are steered sessions (Design
   principle 6): exclude them per `route_kind` as for reflexes, and never count an instinct's
   directive as support for the learned route with its id.
3. **Hand-over.** When the miner learns a route with an instinct's id, the instinct route simply
   loses in the index; nothing else is needed. Log it in `learn.log` as "instinct confirmed by
   sessions".
4. **Review.** `adage review` lists instinct routes separately, with `origins` (which instinct) and
   the declared fact that bound them; rejecting one stores the usual decision.

### adage-cli

1. **Config.** `instincts.enabled` (default `true`, per repository overridable) and
   `instincts.packs` (default `["core"]`); `instincts.disabled = ["U3", …]` maps to
   `BindOptions::disabled`.
2. **`adage init`.** Show what would bind for the current repository (a dry `bind`) and ask once
   for pack-level approval of environment-effect instincts (N1's install route; U7 and U10 later),
   which otherwise stay drafts.
3. **`adage doctor`.** Report per repository: instincts bound, skipped with reasons (including
   files skipped for size or symlinks), and any instinct overridden by a learned route.
4. **SessionStart bind for unseen repositories** (`hook.rs`). When a session starts in a repository
   with no index entry, bind instincts in the background learner (never in the hook's hot path:
   the hook's budget is milliseconds and binding reads files), compile the index, and let the next
   event use it. The hook itself stays a pure function of the index.

### adage-core

Add command-table entries so wrappers can be directed: `uv` (`run` → classify the wrapped
command), `tox`/`nox` (verify), `poetry run`, `just`/`task` (classify by recipe body is not
possible in the table; keep them environment unless the binder vouches for the body, which needs a
track-level "declared body" field). Until then P1's `uv run`, tox and nox wrappers are reported, not
bound.

### adage-eval

An `instincts-only` arm (instincts on, no reflexes) against `off` on the generated sandboxes for
day-0 lift; decoy and violation sandboxes for the false-route rate (roadmap exit: ≤ 5%) and
retire latency (≤ 3 sessions); and the adversarial binder set in this crate's fixtures run as a gate.
