> Exploration, 2026-09-29: research plus an offline prototype (Python, this directory). No Rust crate is changed. The data is the RepoBench v1 and v2 raw traces: 520 Claude Code sessions, 15,829 tool calls and 9,519 Bash calls on attrs, click, marshmallow, urfave/cli and vuejs/core.

# Action abstraction: matching "roughly the same" behaviour in agent traces

## 0. Answer in brief

**Recommendation: (d), a combination led by the typed semantic grammar (a).**

- **The label.** Every command gets a deterministic **action label**: role × environment × scope. It is computed from the normalised argv, the output signature and static repo facts.
- **Relation to recent edits.** This is *not* part of the node label. It is a **pattern constraint** `tests_of(edit → act)`, computed from a file→test map, beside the existing `flow` / `same_art` constraints.
- **Environment class.** `project` versus `system` is an item attribute, like ok/fail polarity. It is not part of what the agent "meant to do".
- **Mining level.** The miner gates on the env-free **intent** (`test-run | narrow | edited-area`).
- **Compilation.** The compiler turns the intent back into one concrete command using repo facts, not traces:
  - **Environment:** the verified project environment.
  - **Targets:** the file→test map of the triggering edit.
  - **Verification:** the command is checked by executing it.
- **Anti-unification (b)** stays one level below, as the typed template of a class. It tells the compiler which argument positions are holes and what the agents' flags were.
- **Embeddings (c)** are off the path. They lose to the grammar on every measure here. The one job left for them is offline clustering of the unclassified residual (0.3% of Bash calls), to propose new grammar table rows.

Key numbers:

| | literal commands | LGG (loose) | potion-8M clusters (best τ = 0.80) | grammar L3 (role·env·scope·relation) | grammar **intent** |
|---|---:|---:|---:|---:|---:|
| Labels covering 90% of Bash calls (sum over 5 repos) | 6,195 | 1,057 | 622 | 87 | **35** |
| Same, pooled across repos | 6,054 | – | – | 25 | **8** |
| Labels seen in ≥ 2 distinct tasks | 5% | 32% | 33% | 83% | **90%** |
| Calls whose label recurs in ≥ 2 tasks | 18% | 82% | 66% | 99% | **100%** |
| Labels seen in ≥ 2 repositories | 1% | 18% | 0% (per repo) | 82% | **100%** |
| Held-out (train → eval): share of calls whose label was seen in training | 8% | 52% | 53% | 87% | **96%** |
| Held-out self-prediction skill κ (train → eval) | 0.00 | 0.09 | 0.06 | 0.20 | **0.31** |
| As a trigger: bits saved predicting the next command's HeadSub (train → eval) | 0.27 | 0.35 | 0.31 | 0.46 | **0.47** |
| Supports "after a source edit → the next test run" in training traces (repos passing `min_support` 5, rate 0.5, ≥ 2 tasks) | 0 / 5 | 2 / 5 | – | 1 / 5 | **5 / 5** |
| Compiles back to a concrete, safe command at directive time | only the literal itself (another task's paths) | 0% right env in Python repos; 76% right target with a flow-bound hole | 62% right target, 41% right env | **98% compiled, 100% safe, 75 / 75 distinct directives executed** | (through L3 / facts) |

Two results decide it:

1. **Mining.** On the 7–10 training traces per repository a learner actually gets, the intent "after a source edit, run the tests of the edited area" passes Adage's support gates in **all five repositories** (6–7 sessions, 6–7 distinct tasks). No literal command passes in any repository.
2. **Compilation and the follow judge.**
   - At every one of 2,461 post-edit test runs in held-out sessions, the compiled directive (for example `.venv/bin/python -m pytest tests/test_options.py tests/test_commands.py`) selects what the agent itself ran in **87%** of narrowed runs. It includes the task's hidden test file in **90%**.
   - It would have pre-empted the environment dead end in **13%** of those runs (21–28% on marshmallow and attrs).
   - Judged as an abstract action, **42 of 42** delivered U11 directives in RepoBench v2 were followed. `pattern::meets` says 0 of 42 (launch bug 1).

The honest caveat, repeated from the architecture review: this makes the learner *able* to form the one reflex real traces contain ("the project's tests for what you edited, in the project's environment"). It does not create value that is not there. The value is still about two calls per session that needs it, plus the dead ends pre-empted.

## 1. The problem, measured again

Over all 2,904 test runs (v1 and v2, Haiku and Sonnet):
- 74% of test-run strings are distinct, and 61% occur once;
- 90% are narrowed by a path, `::`, `-k`, `-t` or `-run`;
- 56% are piped.

Of 9,519 Bash calls, 7,144 distinct strings are needed to cover them, and 6,195 to cover 90%. Only 5% of distinct command strings recur in a second task. Frequency over literals, or over prototype literals that must have an 80%-dominant variant (Track G §2), therefore has nothing to count.

## 2. Methods compared

Every method sees the same shell normalisation (`shell.py`):
- heredoc bodies, `cd`, `export`, wrappers (`timeout`) and redirections are removed;
- `source …/activate` is kept as an environment marker;
- output filters (`| head`) are kept as a modifier.

Every non-Bash tool call gets the same shared label in every method (`edit:src`, `read:test:edited`, …), so only the command abstraction differs.

| Method | What a label is | Module |
|---|---|---|
| literal / argv | the command string / normalised argv | `evaluate.py` |
| headsub | program + subcommand (≈ `pattern::Level::HeadSub`) | `evaluate.py` |
| **(a) grammar** | `role · env · scope · relation`. Levels: L0 role, L1 + env class, L2 + scope, L3 + relation, L3e with the concrete env, and **intent** (role · scope class · relation class, env-free) | `grammar.py`, `facts.py` |
| **(b) LGG** | typed-hole skeleton: tokens typed like G1 value types (`<P:test>`, `<T:test>` test id, `<S>` selector value, `<CODE>`, `<N>`). Strict keeps flags in order. Loose star-folds repeated holes, treats flags as a set and drops output filters. The template of a group is Plotkin's LGG of its members | `lgg.py` |
| **(c) embeddings** | potion-base-8M (the model Adage ships, loaded locally) or 512-d hashed features over the command (and optionally its first 300 output characters). Average-linkage clusters at τ ∈ {0.80, 0.85, 0.90, 0.95} per repository. Held-out calls go to the nearest centroid if ≥ τ, else they become a new label (as `proto.rs` assigns to a frozen epoch) | `embed.py` |
| **(d) combo** | grammar L2 × LGG hole signature | `evaluate.py` |

**The grammar**, in short (`grammar.py` has the full rules; the rules are tables like `MULTIPLEXERS`):

- **Role:** test-run, repro, lookup, build, lint, codegen, env-setup, vcs-read, vcs-write, write-file, fs-edit, read, search, trivial, other.
- **Env:** venv (`.venv/bin/…`, an activated venv), uv, tox, poetry, npx, pnpm, npm-script, local-bin, go, or system.
  - The env class is `project` for all of these except system, which is the dead-end variant.
- **Scope:**
  - test runs: repo, package, file or test-id;
  - repros: inline (`-c`, heredoc), scratch-file or repo-file;
  - reads and searches: src, test, doc, config, env or outside.
- **Relation**, from the session prefix and the repo's file→test map (naming rule plus git co-change before the base commit):
  - test runs: edited-test, src-test (a test of an edited source file), unrelated, by-name (`-k` only), pre-edit, or all;
  - repros: project (imports the project package, including a scratch file's content written earlier) or stdlib.

A compound command takes the role of its most significant part (`pip install pytest && python -m pytest …` is a test run). Only 26 of 9,519 Bash calls (0.3%) end up `other` or `trivial`.

## 3. Results

Every script writes to `results/`, and `results/tables.md` holds every table in full, including every τ.

### 3.1 Compression and cross-task recurrence

These figures are per repository, summed over the five repositories.

| Method | Distinct labels | Covering 90% | Labels in ≥ 2 tasks | Calls in ≥ 2 tasks | Labels in ≥ 2 repos |
|---|---:|---:|---:|---:|---:|
| literal | 7,144 | 6,195 | 5% | 18% | 1% |
| argv | 5,275 | 4,326 | 7% | 34% | 2% |
| headsub | 217 | 66 | 65% | 98% | 34% |
| lgg-strict | 2,686 | 1,737 | 26% | 75% | 15% |
| lgg-loose | 1,995 | 1,057 | 32% | 82% | 18% |
| emb-potion@0.80 | 1,271 | 622 | 33% | 66% | – |
| emb-potion+out@0.80 | 1,419 | 653 | 25% | 66% | – |
| emb-hash@0.80 | 3,568 | 2,619 | 14% | 39% | – |
| grammar L1 (role · env class) | 70 | 31 | 89% | 100% | 100% |
| grammar L3 | 207 | 87 | 83% | 99% | 82% |
| grammar L3e (concrete env) | 226 | 94 | 80% | 99% | 44% |
| **intent** | 84 | 35 | 90% | 100% | 100% |
| combo (L2 × LGG holes) | 562 | 181 | 57% | 96% | 35% |

**What this shows:**
- **LGG** compresses literals about 6×, but most of its templates still differ by flag spelling and by which options the agent added. Only a third of its templates recur across tasks.
- **Embeddings** at usable thresholds merge too little (τ ≥ 0.85) or merge unlike things (at 0.80 a cluster holds `python -m pytest …` next to `python -c "import click…"`). They never cross repositories, because paths and package names dominate the vector.
- **The grammar** reaches 17 labels covering 90% of a repository's Bash calls at L3, and 6–8 at intent level. Its labels are the same across repositories (82% of L3 labels occur in two or more), so cross-repo transfer becomes possible in principle.

### 3.2 Predictive value on held-out traces

Setup:
- **Model:** a back-off model predicts the next Bash call's label from (previous call's label, its outcome).
- **Splits:** (i) task-grouped 5-fold CV within each repo; (ii) fit on the 7–10 `train` sessions per repo and test on all eval sessions.
- **Two views:**
  - **self**: each method predicts its own label. κ = (acc − majority) / (1 − majority).
  - **trigger**: each method's labels are the context, and every method predicts one fixed, method-neutral target, the next call's HeadSub identity. Coarse labels cannot win here by being easier to predict.

| Method | Train→eval: labels seen | Train→eval: self acc / κ | Train→eval: trigger IG (bits) | CV: self κ | CV: trigger IG |
|---|---:|---:|---:|---:|---:|
| literal | 0.08 | 0.02 / 0.00 | 0.27 | 0.01 | 0.53 |
| lgg-loose | 0.52 | 0.18 / 0.09 | 0.35 | 0.19 | 0.94 |
| emb-potion@0.80 | 0.53 | 0.15 / 0.06 | 0.31 | 0.08 | 0.73 |
| emb-potion+out@0.80 | 0.56 | 0.23 / 0.06 | 0.32 | 0.10 | 0.74 |
| headsub | 0.87 | 0.41 / 0.27 | **0.57** | 0.39 | **1.15** |
| grammar L1 | 0.94 | 0.52 / **0.34** | 0.50 | **0.46** | 0.92 |
| grammar L3 | 0.87 | 0.30 / 0.20 | 0.46 | 0.30 | 0.93 |
| intent | 0.96 | 0.51 / 0.31 | 0.47 | 0.36 | 0.89 |

**Reading the table:**
- **Self-prediction.** The grammar levels predict their own next label best at equal or finer granularity than the other methods. L1 and intent beat headsub (κ 0.34 / 0.31 vs 0.27) with far more meaning per label.
- **Trigger.** Grammar contexts carry 1.7–1.9× the information of literal contexts. HeadSub is slightly better here only because the target is in its own label space.
- **Absolute level.** Next-action prediction is weak on data this small (top-1 of 0.3–0.5 at useful granularity). That is the process-mining result the review cited. Prediction is not the lever. **Support for the one reflex that matters** is the lever, measured next.

### 3.3 What the miner would see: "after a source edit → the next test run"

Per repository, over the training traces only:
- the sessions that edited a source file;
- how many of them ran a test within 8 events whose label is the best continuation (support / sessions, distinct tasks);
- ✓ = passes `min_support` 5, `prime_rate` 0.5 and ≥ 2 tasks (`miner_support.py`).

| Method | attrs | click | marshmallow | urfave-cli | vue-core |
|---|---|---|---|---|---|
| literal | 1/10 | 1/9 | 2/6 | 1/9 | 1/8 |
| lgg-loose | 3/10 | 2/9 | 2/6 | 6/9 ✓ | 5/8 ✓ |
| headsub | 6/10 ✓ | 7/9 ✓ | 4/6 | 9/9 ✓ | 7/8 ✓ |
| grammar L3 | 6/10 ✓ | 3/9 | 3/6 | 3/9 | 4/8 |
| **intent** `test-run\|narrow\|edited-area` | **7/10 ✓** | **7/9 ✓** | **6/6 ✓** | **6/9 ✓** | **6/8 ✓** |

Three lessons for the design:
- **Scope and relation fragment support when they are node labels.** file vs test-id and src-test vs edited-test are the same intent, and L3 fails in four repositories. Hence intent-level items plus a `tests_of` constraint.
- **Env must not be part of the mined continuation.** The best L3 label is `test-run|system|…` in all three Python repositories. The majority ran the dead-end form, as the review found for recoveries. In these traces a system-env test run hit the dead end 34% of the time (373 / 1,112), a project-env run 0% (1 / 625). System repros that import the project hit it 16% of the time, project-env repros 0%. So env is an attribute the scorer contrasts (an `avoid` pattern), and the directed env comes from facts.
- **HeadSub passes too, but it cannot compile to anything useful.** `python -m pytest` is the dead end, and `npm test` without targets is the whole suite in watch mode.

### 3.4 Compiling back to a concrete, safe command

This is `compile_back.py`, at every test run an agent made after at least one edit in held-out sessions: 2,461 points.

The grammar's compiler uses repo facts and the session so far:
- the Python interpreter from a verified `.venv`, then `-m pytest`;
- `node_modules/.bin/vitest run` for the node `test` script's runner (never watch mode);
- `go test ./<package of the edit>`.

Targets are the tests edited in the session, then the file→test map of the edited sources, at most 3. `safe()` requires:
- an allow-listed prefix;
- operands that are tracked test modules;
- no shell metacharacters and nothing installed.

`verify.py` rebuilt each task's base checkout offline, using the repository's own setup from the local uv, pnpm and Go caches, and executed every distinct compiled directive:
- `--collect-only` for pytest;
- a real vitest run;
- `go test -run '^$'`.

| Measure | attrs | click | marshmallow | urfave-cli | vue-core | all |
|---|---:|---:|---:|---:|---:|---:|
| Points | 457 | 717 | 425 | 173 | 689 | 2,461 |
| Compiled (file-level; Go: package) | 1.00 | 0.99 | 1.00 | 1.00 (pkg) | 0.92 | 0.98 |
| Safe | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 |
| Distinct directives executed OK | 12/12 | 17/17 | 9/9 | 8/8 | 29/29 | **75/75** |
| Selects what the agent's own (narrowed) run selected | 0.79 | 0.92 | 0.84 | 1.00 | 0.88 | **0.87** |
| …from the static map alone (no edited tests) | 0.77 | 0.83 | 0.76 | 1.00 | 0.47 | 0.72 |
| Includes a hidden (gold) test file | 0.79 | 0.92 | 0.89 | 1.00 | 0.94 | **0.90** |
| …from the static map alone | 0.73 | 0.84 | 0.80 | 1.00 | 0.45 | 0.72 |
| The agent ran outside the project env | 0.71 | 0.52 | 0.70 | 0 | 0 | 0.40 |
| The agent's run hit the dead end, and the directive would have pre-empted it | 0.28 | 0.15 | 0.21 | 0 | 0 | 0.13 |
| **LGG baseline:** the dominant training template has the project env | 0.00 | 0.00 | 0.00 | 1.00 | 1.00 | 0.35 |
| **LGG:** a flow-bound hole (last test file touched) selects the agent's target | 0.86 | 0.85 | 0.78 | 0.23 | 0.72 | 0.76 |
| **Embedding baseline:** assigned to a training cluster | 0.56 | 0.73 | 0.83 | 0.27 | 0.46 | 0.61 |
| **Embedding:** the cluster's commonest literal selects the agent's target | 0.56 | 0.49 | 0.92 | 1.00 | 0.24 | 0.62 |
| **Embedding:** that literal uses the project env | 0.09 | 0.44 | 0.00 | 1.00 | 1.00 | 0.41 |

**What this shows:**
- **LGG** can bind its holes (91% of the agents' own target values appeared earlier in the session: prompt, output or an edited path). But the template it would direct is the training majority's: `python -m pytest <T:test> -xvs` in click and marshmallow, `python -m pytest <P:test> --tb -x` in attrs. In all three Python repositories that is the dead end.
- **Embeddings** direct another task's literal command.
- **The grammar's compiler** takes the environment from facts and the targets from the edit. It is the only one that is right on both.
- **Where the static map is weak.** vue-core co-change hit is 0.45. Here the session's own edited tests carry the directive. An import graph (Track G's Art edges) would help.

### 3.5 The follow judge as an abstract match

This is `follow_judge.py`, over RepoBench v2 `adage` sessions with a delivered U11 route. At the next test run after the dead end:
- `pattern::meets` (the directed argv plus options only) credits **0 / 42**;
- judged as an abstract action (role test-run, env class project, any targets) it credits **42 / 42**.

This reproduces launch bug 1 (41 / 41 there) and shows the fix.

### 3.6 The question's examples

These come from `demo.py` and `results/demo.txt`: click with `src/click/core.py` edited, and vue-core with `renderer.ts` edited.

| Command | L3 | Intent |
|---|---|---|
| `.venv/bin/python -m pytest tests/test_commands.py::test_x -xvs \| head -50` | `test-run\|project\|test-id\|src-test` | `test-run\|narrow\|edited-area` |
| `source .venv/bin/activate && python -m pytest tests/test_options.py -q 2>&1 \| tail -20` | `test-run\|project\|file\|src-test` | `test-run\|narrow\|edited-area` |
| `python3 -m pytest tests/test_basic.py::test_group -xvs` | `test-run\|system\|test-id\|src-test` | `test-run\|narrow\|edited-area` (env class: system, the dead end) |
| `uv run pytest -k parser` | `test-run\|project\|test-id\|by-name` | `test-run\|narrow\|by-name` |
| `python -m pytest tests/` | `test-run\|system\|repo\|all` | `test-run\|all\|all` (satisfies the directive by covering it) |
| `npx vitest run packages/runtime-core` | `test-run\|project\|package\|src-test` | `test-run\|narrow\|edited-area` |
| `npm test -- packages/runtime-core/__tests__/rendererChildren.spec.ts --run` | `test-run\|project\|file\|src-test` | `test-run\|narrow\|edited-area` |
| `cat > /tmp/repro.py << EOF … import click …` then `python /tmp/repro.py` | `repro\|system\|scratch-file\|project` | `repro\|scratch\|project` |
| `.venv/bin/python << EOF … from click.testing import CliRunner` | `repro\|project\|inline\|project` | `repro\|scratch\|project` |

- **Compiled test directive (click):** `test-run|project|file|tests_of(src/click/core.py)` → `.venv/bin/python -m pytest tests/test_options.py tests/test_commands.py` (safe, executes).
- **Compiled test directive (vue-core):** → `node_modules/.bin/vitest run packages/runtime-core/__tests__/components/Suspense.spec.ts packages/runtime-core/__tests__/hydration.spec.ts`.
- **Repro:** the grammar's "scratch repro in the system env importing the project" (1,347 calls on the Python repositories, 16% dead ends) compiles to a prefix rewrite, `python3` → `.venv/bin/python`, with the rest of argv unchanged. This is the launch report's bug 5.

## 4. Recommended design

### 4.1 Data model (`adage-core/src/action.rs`, shared by learner and hook, as `pattern.rs` is)

```rust
pub enum Role { TestRun, Repro, Lookup, Build, Lint, Codegen, EnvSetup, VcsRead, VcsWrite,
                WriteFile, FsEdit, Read, Search, Trivial, Other }
pub enum Env { Venv, Uv, Tox, Poetry, Npx, Pnpm, NpmScript, LocalBin, Go, System, Shell }
pub enum EnvClass { Project, System, NotApplicable }         // Env → class is a table
pub enum Scope { Repo, Package, File, TestId, Inline, ScratchFile, RepoFile, Area(Area) }
pub enum Target { File(RepoPath), Dir(RepoPath), TestId(RepoPath, String), Filter(String) }

pub struct Action {                                  // a pure function of (argv segments, RepoFacts)
    pub role: Role, pub env: Env, pub scope: Scope,
    pub runner: Runner,                              // pytest | unittest | vitest | go-test | python | node …
    pub targets: Vec<Target>, pub selectors: Vec<String>,
    pub piped: bool,                                 // output filtered (a diet signal, not identity)
    pub imports_project: bool,                       // repros: the code (or the scratch file it runs) imports the project
}
pub fn classify(segments: &[Segment], facts: &RepoFacts) -> Action   // table-driven; no regex compiled at hook time
pub fn intent(a: &Action) -> Intent                  // role · scope class · (relation class supplied by a constraint)
```

`RepoFacts` is the affordance map's static half:
- environments;
- runners;
- project packages;
- tracked files at HEAD;
- `tests_of: BTreeMap<RepoPath, Vec<RepoPath>>` (naming rule and co-change today, the import graph later).

It is versioned by HEAD and stored per repo in the index, so the hook can read it.

**The relation to recent edits is deliberately *not* in `Action`.** It depends on the session prefix, not on the node. Put into the node label, it breaks the property that a node's label is a function of its content (proto.rs assigns per distinct node) and fragments support (§3.3). It becomes a constraint instead (§4.3).

### 4.2 Labels: the taxonomy levels in `events.rs` / `proto.rs`

`Event` gains `action: Option<Action>`, filled in `events::from_input` from the argv the importer already has. `Epoch::names` (proto.rs:583) maps the five existing levels as follows for an Act the grammar classifies:

| `Level` | Today | Proposed |
|---|---|---|
| `Exact` | template key | unchanged |
| `Proto` | potion/hash kNN prototype | **LGG typed template** (`lgg.py`: `<venv>/python -m pytest <T:test> {-x,-v,-s}`), deterministic, no epoch needed for Acts |
| `Coarse` | second Louvain at τ − 0.1 | **role · env · scope** (L2 with the concrete env) |
| `Head` | HeadSub identity | **intent**: role · scope class (`test-run\|narrow`) |
| `Kind` | failure family for failed acts, else none | **role**; failed acts keep the failure family |

**Env class is a pattern-item attribute, like ok/fail polarity** (`mine/seq.rs` `Ext { pol }`). An item may require `project` or `system`, or leave it free.
- `avoid` patterns use it: `(test-run, fail, system)` → `recovers_with (test-run, ok, project)`. Across roles it generalises the dead end from pytest to repros (bug 5).
- Continuation items for `run` leave it free, so the dead-end majority pools with the correct minority.
- The compiler fixes the env from facts.

Acts the grammar leaves `Other` (0.3%) keep today's path: HeadSub at Head, and embedding prototypes if wanted. Embeddings stay useful offline only, to cluster that residual and propose grammar table rows for review.

### 4.3 Mining: one new constraint

`tests_of(i → j)` joins `flow`, `same_art` and `last` in constraint extension (`mine/seq.rs` Ext, `select.rs` constraints, `EventSession` gains `tests_of: Vec<(u32, u32)>`). It holds when:
- event *i* is an edit of a source file and act *j*'s targets intersect `tests_of[i.path]`, or
- event *i* is an edit of a test file that act *j* selects, or
- act *j* is repo-wide (it covers everything).

It is computed in `from_input` from `RepoFacts`. Like `same_art`, it is what makes a continuation *bound*: a test run with no `tests_of` from its context is not directable as "tests of the edit". Support is counted at the intent level. With it, the pattern `edit:src → test-run|narrow [tests_of]` passes the gates in 5/5 repositories on training traces alone (§3.3).

### 4.4 Compiler: a facts-bound `run`

A new row in the shape table (`compile/shapes.rs`, `compile/mod.rs`):

| Continuation | Context | Shape | Route |
|---|---|---|---|
| one intent-level act with `tests_of` from the edit | an edit | `run` (prime) | `after_edit` → `run`, slot `EditTests { k: 3 }`, command from the affordance map |

- **The command is built, not copied.**
  - The prefix is the affordance map's **verified** invocation for that role and runner: `.venv/bin/python -m pytest`, `node_modules/.bin/vitest run`, `go test`. It is verified by executing it (collect-only), as `verify.py` does.
  - The operands are bound at runtime from the triggering edit through `tests_of` (a new `SlotSource::EditTests`, a sibling of `EditSegment`), always repo-relative tracked test modules.
- **What this replaces.** It replaces Track G §2's rule that "a continuation compiles to its ≥ 80% dominant variant" and the covering-variant rule, *for this shape only*: the directed command is verified by execution instead of by having been run by the agents. Everything else keeps the dominance rule: "embeddings widen triggers, never actions" still holds, since nothing here comes from embeddings.
- **LGG's role.** It proposes optional flags when the class's template is unanimous across the traces (for example `-x`), and it reports hole types for review.
- **Safety** is `safe()` in `compile_back.py`:
  - an allow-listed prefix;
  - tracked test operands only;
  - no metacharacters, installs or network;
  - the effect ceiling is unchanged (Verify).

### 4.5 Runtime

- **Follow judge (bug 1).** `Sat(d)` for a directed test run is an abstract match, not `pattern::meets`:
  - same role;
  - env class `project`;
  - targets overlapping or covering d's.
- **Signatures on exit-0 output (bug 2).** The grammar's `outcome` reads output signatures whatever the exit code (`env-fail` / `ran` / `fail`).
- **`PreToolUse`.** Classify the pending call. For `role ∈ {test-run, repro}` with env class `system` where the affordance map has a verified project env, rewrite only the interpreter prefix (`python3 -m pytest …` → `.venv/bin/python -m pytest …`), or deny with a reason.
- **Cost.** Classification is a few table lookups per segment on argv the hook already parses; `tests_of` is one map lookup.

### 4.6 Order of work

1. `adage-core::action` (classify, env class, intent) and the follow judge on it. This fixes bug 1 and is replay-measurable (§3.5).
2. `RepoFacts.tests_of` and the `tests_of` constraint in `events.rs` / `mine`.
3. The Proto, Coarse, Head and Kind mapping in `Epoch::names`.
4. The facts-bound `run` row with `EditTests` and execution verification.
5. The `PreToolUse` rewrite for test-run and repro in the system env.

## 5. Limits

- **Every number is offline**, on 5 small repositories with generated prompts. Next-action accuracy is low in absolute terms. §3.2 compares methods; it does not promise directive precision.
- **The grammar is hand-written tables** (runners, package managers, test layouts), with the same maintenance model as `MULTIPLEXERS`. The residual is 0.3% here; other ecosystems (cargo, maven, bazel) need rows. The dogfood traces' `cargo test -p …` would classify as `test-run|project|package`.
- **The relation depends on the file→test map.**
  - Co-change is weak on vue-core (0.45 gold hit from the map alone).
  - `-k`-only runs (`by-name`) are related to edits only when the selector appears in an edit's text.
- **"Agreement with the agent" is partly circular** when the agent edited the test it then ran (1,322 of 2,404 compiled points have an edited test as a target). The map-only rows separate the static contribution.
- **The LGG baseline's flow binding** (the most recent test file touched) is a heuristic stand-in for G1 flows.
- **The embedding baselines use one clustering family** (average linkage). `proto.rs` uses kNN + Louvain with must-link and cannot-link; its blocking by program would not merge `python` with `.venv/bin/python` either.
- **Relative to the ceiling.** None of this changes the architecture review's estimate of the ceiling: roughly 5–10% of cost on repositories like these.

## 6. Files

| File | Purpose |
|---|---|
| `traces.py` | loads the raw transcripts (`/tmp/adage-rb2`, `/tmp/adage-repobench`; layout in `evals/live/runner.py`) |
| `shell.py` | shell normalisation into segments |
| `facts.py` | repo facts from the base commit (local git mirrors): environments, packages, `tests_of` (naming + co-change) |
| `grammar.py` | method (a): `Act`, `classify_seg`, `relation_of`, `intent`, `label_session` |
| `lgg.py` | method (b): typed skeletons and LGG templates |
| `embed.py` | method (c): potion-8M (local copy via `POTION_DIR`) and hashed embeddings, clustering, assignment |
| `corpus.py` | labels every session once (cache: `$ABSTRACTION_CACHE`, default `/tmp/adage-abstraction-cache.pkl`) |
| `evaluate.py` | compression, recurrence, held-out prediction → `results/metrics.json` |
| `miner_support.py` | support for "after a source edit → test run" per method → `results/miner_support.{json,md}` |
| `compile_back.py` | compiler, `safe()`, scoring against agents, gold tests and baselines → `results/compile_back.json` (and `compile_points.jsonl`, per point, not committed: regenerated) |
| `verify.py` | executes compiled directives in offline-rebuilt sandboxes → `results/verify.json` |
| `follow_judge.py` | abstract vs literal follow judge on v2 → `results/follow_judge.json` |
| `demo.py` | the examples above → `results/demo.txt` |
| `report.py` | `results/tables.md` |

To reproduce (offline, about 5 minutes; `verify.py` about 15 more), run from this directory:

```
python3 corpus.py && python3 evaluate.py && python3 compile_back.py && python3 miner_support.py \
  && python3 follow_judge.py && python3 demo.py > results/demo.txt && python3 verify.py && python3 report.py
```
