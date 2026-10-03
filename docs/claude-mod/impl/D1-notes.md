# D1 · Candidates in the learner and the index (and D3's learning half)

What was built for chunk D1 of [the implementation plan](../implementation-plan.md) (§2.1.1,
§2.3.2–3, §3 D1; architecture §3.10 "Noticed"), the exact index shape and read API D2 builds on,
departures, and the commands run. The last section is D3's learning half (§2.1.2, T-D3-04/05/08),
added to this chunk by the coordinator.

## What was built

1. **The candidate pass** (`adage-graph`). `mine::mine_candidates(labels, sessions, params) ->
   (Db, Mined)`: a second `Db` over the same sessions with its floor at
   `mine::candidate_floor(s_min, border) = max(s_min − border, 2)`, `seq::mine` on it with the
   same parameters otherwise, and only the patterns with support in `[floor, s_min)` and a context
   of two or more items kept, closed among themselves. `Mined::s_min` is the reflexes' own.
   `select::candidate_reflexes` turns each closed border pattern into a `select::Reflex` (support,
   days, opportunities, the posterior as counted, `sessions`, `lag` = median turns from the end
   of the context to the step); never gated, never selected. It runs inside `select::learn_split`
   after everything else, only with `select::Params::candidates` (default `false`; the engine sets
   it from `LearnConfig::candidates`, default `true` in both presets), into
   `select::Learned::candidates` (serde-skipped, so `Learned`'s JSON is unchanged). Timed as
   `Timings::candidates_ms`.
2. **Compiled as the reflex would be** (`compile::compile_candidates(&Input) -> Output`): each
   candidate goes through the same `Builder::context` as a selected reflex, so its id is
   `ids::route_id(shape, trigger, step, [])` exactly as the route of the same pattern; no strength
   bar (provisional at the ceiling); only `Trigger::Context` of 2–3 items comes out (a context
   ending in a failure or an edit would be another shape as a reflex and is dropped); every
   output has `Compiled::candidate = true` and `Compiled::sessions`. **One per habit**: of the
   compiled candidates with the same supporting sessions and the same step, the one nearest the
   step (`lag`), then the fewest items, the highest rate, the id. Then one per id.
3. **The engine** (`adage-learn/src/engine.rs::run`) compiles the candidates apart from the routes
   (so the routes' compile is untouched) and branches on `Compiled::candidate` before `judge`:
   `EngineRun::candidates` (never judged) and `EngineRun::sessions` (route id → supporting session
   ids, for the tip). `EngineReport::candidates` counts them (`skip_serializing_if` zero).
4. **What is recorded** (`adage-learn/src/lib.rs::plan` → `candidates_of`): per repository, the
   engine's candidates whose id is not a route of the learned track (any state), plus the track's
   nascent reflexes with a context trigger as `review: true`; `formed` = last learn's candidates
   (read with `adage_tracks::candidates::load`) that are reflexes in service now and were not,
   with the session ids that tipped each. Carried as `RepoReport::candidates` (serde-skipped) and
   written by `learn_sessions_into` through **`Store::write_candidates(&CandidateFile)`**, after
   the tracks and before `compile_index`. `Files` writes it with
   `adage_tracks::candidates::write`; the in-memory store of `tests/remote_sessions.rs` records a
   row. Nothing else writes the file. With `candidates` off no file is written.
5. **The index** (`adage-tracks`). `Candidate`, the file format and its reader/validator live in a
   new `src/candidates.rs` (not `model.rs`: the skills agent was editing it). `Index` gains
   `candidates` and `candidate_contexts`; `Index::compile_root` reads every repository's file
   (`attach_candidates`), `Index::compile(tracks)` is unchanged, the generation is unchanged.
   `Triggers::with_candidates` folds every candidate item's words and globs into the repository's
   triggers. `CtxNet::of_candidates` builds the separate net, its digest salted. `format` is D3's
   (`INDEX_FORMAT = 1`); D1 needs no bump (older binaries ignore unknown fields).
6. **`learn.log`** (`adage-learn/src/diff.rs`): `ChangeKind::Candidate`; `Change` gains
   `support`, `s_min` (on a candidate) and `sessions` (on a `new` reflex that was a candidate),
   all `skip_serializing_if` none, so existing lines are byte-identical. `Snapshot` is now a struct
   (`tracks`, `candidates`) that derefs to the old map, so every CLI caller compiles unchanged;
   `snapshot`/`snapshot_all` read the candidates file.
7. **The CLI** (`adage-cli/src/candidates.rs`, new; registration in `main.rs`): `adage status
   --json` gains `candidates` (always present, possibly empty).

## Contracts

### `candidates.json` (`adage_tracks::candidates`)

`<tracks>/<slug>/learned/candidates.json` (`candidates::path(tracks_root, repo)`), owner-only
(0600), written atomically (temp + rename), `normalize`d (sorted by id, one per id, at most
`MAX_CANDIDATES = 128` keeping the most support, session lists sorted, ≤ 256):

```json
{ "schema": "adage-candidates/1", "repo": "acme-billing", "s_min": 3,
  "candidates": [
    { "id": "run-69ef44",
      "trigger": { "on": "context", "items": [ {"item":"act","keys":["exec make test"]}, … ], "within": 8 },
      "step": { "do": "run", "command": "make lint" },
      "support": 2, "days": 2, "last_seen": "2026-09-09", "review": false,
      "sessions": ["<session id>", "<session id>"] } ],
  "formed": [ { "id": "run-1a5b55", "sessions": ["<session id>"] } ] }
```

The plan's contract plus `sessions` (per candidate) and `formed` (top level), both needed for
"the line that turns a candidate into a route names the session ids that tipped it"; `last_seen`
is `null` when the traces carry no date. A file is read only when `schema` matches and its `repo`
slugs to the directory it is in; an entry is in the index only if `validate_entry` passes (safe
id; `Trigger::Context` of 2–3 `act`/`fail`/`edit` items, `within` ≤ 8; `run` or `sequence` with
no slot; support in `1..s_min` unless `review`; and `io::validate` of the probe route, effect
classified from the commands).

### The index shape (what D2 reads)

```json
{ "format": 1, "generation": "…", "routes": [ … ], "triggers": { "<repo>": { … candidate words merged … } },
  "contexts": { "<repo>": <routes' CtxNet> },
  "candidates": { "<repo>": [ { "id", "trigger", "step", "support", "s_min", "days", "last_seen", "review" } ] },
  "candidate_contexts": { "<repo>": { "digest": <u32>, "alphabet": [CtxItem…], "nodes": [ { "next": [[sym, node]…], "routes": [["<candidate id>", within]…] } ] } } }
```

- `candidates[repo]`: sorted by id, never the id of one of the repository's routes in the index.
  Both new maps are `#[serde(default, skip_serializing_if = "BTreeMap::is_empty")]`: an index
  without them loads with empty maps; the 26 recorded indexes load.
- `candidate_contexts[repo]` is a `CtxNet` like `contexts`, built only from candidates, whose
  `DNode::routes` hold **candidate ids**; its `digest` is salted (`b"candidates\0"`), so it never
  equals a routes' net digest. Not part of `generation`.
- `triggers[repo]` includes every candidate item's words/globs, so `fast::skip` never skips an
  event that could move the candidate ring. A repository with candidates and no routes has a
  `triggers` entry (and `Index::contexts` answers an empty net for it).

### The read API (`adage_tracks::Index`, for D2)

```rust
pub fn candidates(&self, repo: &str) -> &[Candidate];                 // sorted by id, empty if none
pub fn candidate(&self, repo: &str, id: &str) -> Option<&Candidate>;  // binary search: ids from the walk
pub fn candidate_contexts(&self, repo: &str) -> Cow<'_, CtxNet>;      // empty net if none
pub fn triggers(&self, repo: &str) -> Cow<'_, Triggers>;              // already includes candidates
pub fn attach_candidates(&mut self, by_repo: BTreeMap<String, Vec<Candidate>>); // what compile_root calls
// adage_tracks::candidates::Candidate { id, trigger, step, support, s_min, days, last_seen: Option<String>, review }
//   .gist() -> String   ("after …, run make lint": for a noticed line)
// adage_tracks::context::CtxNet::of_candidates(&[Candidate]) -> CtxNet
```

D2 per the plan: a second `Symbols` over `index.candidate_contexts(&repo)` (`.exact_keys(..)` as
at `runtime/src/lib.rs:504`) when it is non-empty; `CtxNet::walk(&st.cand_ctx.recent())` returns
candidate ids; `index.candidate(&repo, id)` gives `support`, `s_min`, `review` for
`Noticed { candidate, support, s_min, review }`. The runtime never needs the file.

### `adage status --json`

`"candidates": [ <adage_app::reflex::Reflex fields> + "candidate": {support, s_min, days,
last_seen, review}, "next": "adage review approve <track> <id>" | null ]`, from the compiled
index. A border candidate is shown in the learned track it would join with `state: "candidate"`
and `directive: null`; a `review: true` one is its track's own nascent route (state `nascent`,
evidence, why), with `next` the review command that puts it into service. Acting on a candidate
is that existing `adage review approve`; no new verb.

### `learn.log`

`changes[]`: `{"route","track":"<slug>/learned","change":"candidate","from":null,"to":null,
"summary":"candidate seen in 3 traces; a reflex needs at least 5","support":3,"s_min":5}` for a
candidate that is new or whose support moved (`review` ones are reported as `nascent`, as
before); a `new` change gains `"sessions":["…"]` when the route was a candidate.

## Departures from the plan, and why

1. **Floor never below 2.** `s_min − border` is 1 under `--demo` (3 − 2): every event of every
   trace would be alphabet, and a pattern seen once is no pattern seen before. T-D1-02 and T-D1-07
   are consistent with it.
2. **No `record_all`.** The second `Db`'s own `s_min` *is* the floor, so its frequent set is the
   whole border; recording the climb's losers adds nothing. Its result never touches the routes'
   (T-D1-01 checks byte equality with the pass on and off).
3. **Candidates are counted, not scored** (`candidate_reflex`): support, days, opportunities and
   the plain rate, no next-action model, calibration, value or MDL: a candidate is never gated or
   selected, and `score_all` is the costly half of a learn.
4. **One candidate per habit.** A miner returns every subsequence of a habit's context (19 for
   one habit in the test corpus); the person should notice it once. Grouped by (supporting
   sessions, step); the representative is the context nearest the step, which in the tests is also
   among the routes that form at `s_min`.
5. **`Candidate` lives in `adage-tracks/src/candidates.rs`**, with its file I/O, not in
   `model.rs`/`io.rs` (another agent's files this wave); `index.rs` gained only the two fields,
   `attach_candidates`, the accessors, the `compile_root` call, and `..Default::default()` in
   D3's `Index` literal. `triggers.rs` and `context.rs` gained functions; the existing ones were
   refactored to share them, with unchanged results.
6. **`Snapshot` became a struct** (deref to the old map) so the CLI's diff callers needed no edit.
7. **The sim's 2-of-12 sequence is added in the test, not in `scenario::history`.** adage-sim is
   not this chunk's; changing the history regenerates every fixture under `sim/fixtures` and the
   replays other agents assert on. `crates/adage-cli/tests/candidates.rs` takes
   `scenario::history(CLAUDE_SEED)`, inserts `make lint` right after the last `make test` of the
   first two sessions, and runs the binary on it. Moving that into `scenario.rs` (and
   regenerating) is the sim owner's step; nothing in D1 depends on it.
8. **`condition` prints candidate changes** through `condition_out`'s generic row (its `_` arm:
   "changed · <gist> · candidate seen in 2 traces; …"); `condition_out.rs` was not touched.

## Tests (by test-plan id)

| Id | Where | Status |
| --- | --- | --- |
| T-D1-01 | `adage-graph/tests/candidates.rs::the_reflexes_pass_is_untouched` (synthetic sessions in both presets, every dataset with a manifest in the default: `Learned` JSON byte-equal with the pass on/off, `truncated` false); `adage-learn/tests/candidates.rs::the_sims_routes_are_the_same_with_candidates_on_or_off` (the sim's three histories) and `the_routes_are_the_same_with_candidates_on_or_off`; the codex/opencode goldens unchanged | green |
| T-D1-02 | `adage-learn/tests/candidates.rs::the_candidate_pass_keeps_the_border_only` (2 → none, 3, 4 → one), `a_single_call_is_never_a_candidate` (after-edit and prompt family, and 5 → a route, no candidate); `adage-graph/tests/candidates.rs::the_pass_keeps_the_border_sequences_only` | green |
| T-D1-03 | `adage-learn/tests/candidates.rs::a_candidate_keeps_its_id_when_it_forms` (same id at 5, never in `routes`, `change: new` with `sessions: ["s04"]`); `adage-graph/tests/candidates.rs::a_candidate_compiles_to_the_id_of_its_reflex` | green |
| T-D1-04 | `adage-learn/src/lib.rs::nascent_context_reflexes_are_candidates_for_review` (context → `review: true`, after-edit → none, index: candidate, not route) | green |
| T-D1-05 | `adage-tracks/tests/candidates.rs` (3 tests); `adage-tracks/src/candidates.rs` unit tests (3) | green |
| T-D1-06 | `cargo test -p adage-graph --test border` | green |
| T-D1-07 | `adage-cli/tests/candidates.rs::the_sims_sub_support_sequence_is_a_candidate` | green |
| T-D1-08 | `adage-learn/examples/candidates_bench.rs` (numbers below) | measured |
| T-D1-09 | `adage-graph/tests/candidates.rs::every_recorded_index_loads` (26); `cargo test -p adage-eval --test gates` 15/15 | green |
| T-D1-10 | not measured: needs a release build of `adage` (`hook_latency`), outside this brief's build allowance. The fast path's skip rate is unchanged on the sim history (below). | open |

## L5: `candidates_bench` (T-D1-08)

`cargo build --release -p adage-learn --examples`, then `target/release/examples/candidates_bench 300 2 3`:
in process (`adage_learn::learn` into the files store, the demo preset), the simulator's
Claude Code history, 300 sessions in each of 2 repositories, median of 3 runs each on a fresh
home, candidates off (the base: the reflexes' pass alone) and on. Machine: 4 vCPU Intel Xeon @
2.10 GHz, 15 GiB, Linux 6.18 (a shared build container: other agents' builds ran concurrently).

| Run (ms, median of 3) | candidates off | candidates on | on / off |
| --- | --- | --- | --- |
| learn, cold parse cache (600 traces) | 13,853 | 13,243 | 0.96 |
| learn, warm parse cache | 13,407 | 13,776 | 1.03 |
| one repository, one transcript grown | 6,855 | 6,721 | 0.98 |

Routes identical (72), candidates 0: in 300 sessions built from the scenario's 12 the floor is 2
and every habit repeats ~25 times, so nothing sits in `[2, 3)`; the pass still mines its second
`Db` (that is its cost). Hence a second run on a history with candidates,
`candidates_bench 24 2 5` (24 sessions per repository, median of 5): cold 2,458 → 2,386 ms (0.97),
warm 2,359 → 2,465 ms (1.04), one repository 1,115 → 1,113 ms (1.00); routes identical (43);
1 candidate compiled. **Within 1.3× in every row** (the differences are within the noise of a
shared machine). The fast path's skip rate over every tool call of the history (3,951 calls in
the 300 × 2 run, 324 in the 24 × 2 run) is the same with and without candidates compiled: 0.412
and 0.336.

Not measured, and why: `condition --auto` with one changed transcript ≤ 150 ms and the paired
`hook_latency batch_no_match` p95 (T-D1-10) are timings of the release `adage` binary, which this
brief did not allow building. The in-process "one repository" row above is a full relearn of that
repository (300 traces), not the binary's `--auto` path, so its absolute value is not the 150 ms
criterion; its ratio is.

## D3's learning half (§2.1.2; T-D3-04, T-D3-05, T-D3-08)

Built:

- `input::Step::Skill { name }`; `canonical::push` maps `CanonicalAction::Control { tool }` whose
  tool is `Skill <name>` with a `safe_skill_name` to it (meta key `control Skill <name>`); every
  other control tool stays out.
- `NodeType::Skill` (`"skill"`): `events::from_input` emits one event (key and head the name,
  block `skill`, effect observe, ok), labelled at the exact level only (`proto::names`; skills are
  left out of prototype families), never a flow source. `Influence::skills` (from
  `Directed::skills`) and `names_skill`: a skill directive exposes that skill's events only.
- `mine::target_label` admits `Skill`; `seq::keys_at` admits it as a continuation item and never
  as a context item. `select::shape` gives a skill continuation its own MDL table (`"skill"`).
- The compiler's `skill` shape (`Builder::skill`, checked first in `compile`): a continuation of
  exactly one skill → `Step::Skill { skill, args: None }`, observe, after an edit (`after_edit`,
  provisional prime, recounted by the engine) or after a selected context of 2–3 events
  (`context`, its own posterior), capped at `SKILL_CEILING`; id `skill-<hash>` (`ids.rs`'s arm,
  D3's). `Builder::context_items` was factored out of `context` for it.
- Intent families: a skill used before the first edit is a continuation candidate
  (`intent::candidates`, group `control Skill <name>|observe`); a family whose continuation is a
  skill compiles to `intent → Step::Skill`, prime at most.
- The engine: `Want::skill`/`Want::of` so `ran_after` and `count_after_edit` judge a skill step
  as a run (a Skill tool use of that name, not failed); `exposed` adds "a directive named this
  skill"; strength capped at prime. The reviewed comments D3 left at `named_commands`,
  `dormant.rs` and `value.rs` are kept.
- Exhaustive matches on `input::Step`: `flow.rs`, `facts.rs`, `compile/mod.rs` (`Corpus::of`) skip
  a skill (no command, file or value).

Tests: T-D3-04 `adage-graph/src/canonical.rs::a_skill_used_is_a_skill_step_and_no_other_control_tool_is`;
T-D3-05 `adage-learn/tests/skill.rs` (six traces: `intent → skill release-checklist`, prime,
observe, support 6, shape `skill`; four: none; two of six steered: none, and the steered trace's
influence names the skill only) and `adage-graph/tests/compile.rs::a_skill_used_after_its_context_compiles_to_the_skill_shape`.
The six-trace test turns uplift gating off (`uplift_min: 0`): every trace of the family took the
skill unaided, so with the default bar the reflex is learned quiet (nascent), as any reflex agents
already follow. **T-D3-08's whitelist**: the gates were run with skill events in the alphabet
(`cargo test -p adage-eval --test gates`: 15 passed, including
`replay_with_the_learned_index_gives_every_recorded_directive` and the route-fidelity gates); no
dataset learns a skill route, so no new id joined the whitelist and none was added.

## Files

Owned: `crates/adage-tracks/src/{candidates.rs (new), lib.rs (one line), index.rs, triggers.rs,
context.rs}`, `crates/adage-tracks/tests/candidates.rs` (new);
`crates/adage-graph/src/{mine/mod.rs, mine/seq.rs, select.rs, compile/mod.rs, compile/shapes.rs,
compile/intent.rs, input.rs, canonical.rs, events.rs, proto.rs, flow.rs, facts.rs}`,
`crates/adage-graph/tests/{candidates.rs (new), compile.rs}`;
`crates/adage-learn/src/{engine.rs, lib.rs, store.rs, diff.rs, evidence.rs}`,
`crates/adage-learn/tests/{candidates.rs (new), skill.rs (new), remote_sessions.rs}`,
`crates/adage-learn/examples/candidates_bench.rs` (new); `crates/adage-cli/src/candidates.rs` (new),
`crates/adage-cli/tests/candidates.rs` (new). In `crates/adage-tracks` the skills agent owns
`model.rs`, `io.rs`, `interlock.rs`, `tests/skill.rs` and `index.rs`'s `format`/`Unloaded`; this
chunk edited none of those but the `Index` literal in `compile_with_conflicts` (added
`..Default::default()`).

Small edit in another owner's file: `crates/adage-cli/src/main.rs` (`mod candidates;` and one
line in `status`'s JSON branch: `v["candidates"] = candidates::status_json(paths, &installed);`). `crates/adage-cli/tests/live_learn.rs` (`learn_auto_logs_what_changed` accepts a `candidate` change on a first learn).

Docs: `docs/live-learning.md` (Files: "Candidates"; the `learn.log` change table),
`docs/how-it-works.md` ("From history to a reflex": candidates; the skill paragraph: how a skill
reflex is learned), `docs/tracks/track-g-graph-engine.md` §9 ("Skills", "Candidates"),
`CHANGELOG.md` (two bullets under Conditioning).

## Commands run

All with `CARGO_INCREMENTAL=0` (other agents were deleting `target/debug/incremental` while
building; no other flag, the shared `target/`).

- `cargo fmt --all --check`: clean.
- `cargo clippy -p adage-tracks -p adage-graph -p adage-learn --all-targets -- -D warnings`: clean.
- `cargo clippy -p adage-cli --all-targets -- -D warnings`: one error, not in this change
  (`crates/adage-cli/tests/zero_config_plugin.rs:922`, `doc_lazy_continuation`, another chunk's
  test). `cargo clippy -p adage-cli --bins --examples --test candidates -- -D warnings`: clean.
- `cargo check --workspace --all-targets` (after D3's learning half): clean.
- `cargo test -p adage-tracks -p adage-graph --no-fail-fast`: all green (adage-graph 49 lib and
  every test binary including `border` and `candidates`; adage-tracks 48 + 3 + 4 + 1 + 4). In
  that run `adage-tracks --test candidates` and the two crates' doctests failed once while
  another agent's build replaced artifacts underneath ("can't find crate for adage_core"); rerun
  alone six times and `--doc`: green.
- `cargo test -p adage-learn --no-fail-fast`: all green (58 lib + 15 test binaries, including
  `candidates` 6 and `skill` 2).
- `cargo test -p adage-cli --no-fail-fast`: green except `ui::tests::settings_show_rewrites_off_by_the_older_marker_and_notices`
  (an insta snapshot of the settings screen, not this change) and `live_learn::learn_auto_logs_what_changed`,
  which asserted every first-learn change is `new`/`nascent`; it now accepts a `candidate` change
  with `support < s_min` (smallest edit in that test file) and passes (9/9). `candidates` 1/1,
  `hooks` 22/22 (goldens unchanged), `harness_sim` 9/9.
- `cargo test -p adage-eval --test gates`: 15 passed (819 s), including
  `fast_path_changes_no_decision`, `replay_with_the_learned_index_gives_every_recorded_directive`,
  `replay_reproduces_the_live_round2_decisions`, `learned_routes_match_the_sandbox_conventions`.
  `--test predict_gate --test parity --test prop_tracks`: 3 + 1 + 2 passed.
- `cargo test -p adage-exec --test parity`: 22 passed.
- `cargo test -p xtask`: 18 passed.
- `cargo build --release -p adage-learn --examples`, then `target/release/examples/candidates_bench 300 2 3`
  and `candidates_bench 24 2 5`: above.

## Review fixes

| Finding | What changed | Test |
| --- | --- | --- |
| **L5** `candidates::read_all` sorted by id and cut at 128, keeping alphabetical order where `normalize` promises the most support; `read` had no size bound | `read_all` keeps one entry per id (the strongest: `(review, support)`), and past `MAX_CANDIDATES` keeps the strongest, ties by id, then sorts by id: the rule `normalize` applies when the learner writes, now also for a file it did not write. `read` reads at most `MAX_FILE_BYTES` (8 MiB, far above 128 entries of 256 session ids) and refuses a larger file | `candidates::tests::read_all_keeps_the_strongest_past_the_bound` (138 entries, the 10 weakest with the lowest ids: all cut; a file grown past the bound is refused) |
