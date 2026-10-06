# Adage in Claude Code: implementation plan

> Status: **proposed 2026-10-02**. The plan for [architecture.md](architecture.md) (the
> design), [ux-design.md](ux-design.md) (what the person sees) and
> [spikes/S9](../../spikes/S9-claude-mods/SPIKE.md) (what must be measured first); the tests it
> names are specified in [test-plan.md](test-plan.md) by id (`T-<chunk>-<nn>`).
> Written against commit `0eb40ae`, then re-verified reference by reference against `main` at
> `fff4fa7` (the architecture-boundaries merge: `adage-trace`, `adage-ingest`, `adage-app`,
> `adage-safety`, `Policy` on `Ctx`, the `Store` port, one `Reflex` JSON shape); every path and
> line below was read in that tree, and §1's last rows say what the merge changed. It is meant to be worked chunk by chunk by an agent: each chunk names its files, its
> contract, its tests and the criteria that close it. Nothing here is implemented. Revised once
> after review: the first draft's grant mode would have broken existing E2 grants, its candidate
> mining could not produce a candidate, and several acceptance criteria could not be met with the
> fixtures named; this version fixes each and says where it departs from the first design draft.

## 0. How to work this plan

Every chunk of this plan is built on the branch `claude/mods-poc-m98u3o` (the name is
historical: it began as the proof of concept). [impl/README.md](impl/README.md) maps each chunk
to its commit and notes; [evaluation-plan.md](evaluation-plan.md) is the brief for the live
evaluation, which is the only part not done: nothing has run in a live Claude Code session.
Each chunk below keeps its **Built** line (the proof of concept's state where that is where it
landed, then the hardening commit) so a reader knows where the code is and how it departs.

- **One chunk, one branch, one pull request.** A chunk is done when its acceptance criteria all
  pass in CI and the docs it names are updated in the same change (`docs/how-it-works.md` rule:
  "Implementation changes must update this page and the relevant operational guide in the same
  change"). Record user-visible changes under `## 0.1.0 - Unreleased` in `CHANGELOG.md`, in the
  matching thematic subsection (`### Runtime`, `### Execution (opt-in)`, `### Seeing it`, …), as a
  bullet opening with a bold lead sentence. Commit with `git commit -s` (DCO).
- **The floor never moves.** Every chunk leaves `adage hook claude <event>` byte-identical for
  the events and payloads it does not add (the goldens in `crates/adage-cli/tests/golden/` and
  the exec parity test `crates/adage-exec/tests/parity.rs` pin this). A new event is a new golden
  step, re-blessed with `ADAGE_BLESS=1 cargo test -p adage-cli --test hooks`, and the diff is
  reviewed. A field the mod needs is emitted only when the mod's payload asks for it, so a command
  hook's output never changes.
- **Fail open, always.** Anything on the hook path exits 0 and prints nothing on any error
  (`CONTRIBUTING.md`). The mod does the same in TypeScript: a missing binary, a timeout, a
  non-zero exit or bad JSON is a tagged failure and `next(e)`.
- **The binary decides; the mod bridges and draws.** No reflex, grant, arm, bound or threshold is
  evaluated in TypeScript. Where this plan says "the mod asks", it means one `$.process.run` of
  `<bin> hook claude <Event>` with the payload on stdin and JSON on stdout, exactly as a command
  hook would (mod architecture §2). The argv the mod may run is an allow-list (B1).
- **Words.** Person-facing text follows `docs/lexicon.md` and `docs/design/style.md`; the model's
  text stays plain and factual (`<adage-reflex>` blocks, fixed templates in
  `crates/adage-runtime/src/render.rs`, `render::sanitise` on anything that came from a sandbox).
- **Tests.** Integration tests build their own `World` per file (`crates/adage-cli/tests/hooks.rs`,
  `harness_sim.rs`, `zero_config_plugin.rs` each define one); every `World` sets a fresh
  `ADAGE_HOME`, `ADAGE_AUTO_LEARN=0` and `ADAGE_ARM=treated`, and removes `CLAUDECODE`,
  `OPENCODE`, `CODEX_SANDBOX*` and `ADAGE_DISABLE`. A test that exercises the control arm sets
  `ADAGE_ARM=control`; a test that runs a person-only verb removes `ADAGE_ARM` too (it is in
  adage-exec's `AGENT_ENV`) and sets `ADAGE_ALLOW_NONINTERACTIVE=1` (the list is `adage_app::person::AGENT_ENV`, `crates/adage-app/src/person.rs:11`, with copies in adage-exec and adage-train). CI tests the affected packages
  (`cargo xtask affected`, `docs/testing.md`) and always runs the architecture guards
  (`cargo test -p xtask`: layers, affected, hygiene, suites); a test that reads outside its crate
  is declared in `READS` (`xtask/src/affected.rs`); the eval gates run weekly and before the
  chunks §5 names. The layering rules (`xtask/src/layers.rs`) bind every chunk: the runtime never
  links adage-exec or the store, the hook never links the learner, the hook never links adage-train, ingestion never links
  tracks; this plan's `By` and `Event::TurnStep` live in `adage-trace`, `Capabilities` in
  `adage-trace`, the grant lookup reaches `decide()` as a `Policy` field, the `verified.json`
  type lives in `adage-pack` (layer 4) so the hook (layer 6) can read it, the press check lives in
  `adage-app` because it must mint a `Person`, and nothing here adds an exception.
- **Order.** Workstream A (foundation in the binary) has no Claude Code dependency and starts
  now. Workstream B (the mod) needs S9's Q1, Q2 (Layer 1), Q3 and Q6 to pass first. Workstream C
  (acting in the model's place) needs Q2 (Layer 2), Q4 and Q9. Workstream D (noticed, skills) is
  independent of the mod on the binary side and needs the mod only to draw. Workstream E
  (evaluation) grows with each.

## 1. Where things stand (the touch points)

What the design needs, against what the code has. Lines are from the tree named above.

| Need | Today | Where |
| --- | --- | --- |
| A per-`tool_use_id` identity for `PreToolUse`, so the mod and the command hook never both decide | `Claude::event_identity` answers only `PostToolBatch`, `PostToolUse`, `PostToolUseFailure`, and only to hand adage-exec a journal occurrence id; `pretool()` never calls it and exits on a word scan before any I/O | `crates/adage-hook/src/harness/claude.rs:60-72`, `crates/adage-hook/src/lib.rs:559-569`, `crates/adage-exec/src/hook.rs:1036-1039` |
| An event between "tool results in" and "next request out" | None. `Event` is `SessionStart, Prompt, Batch, Failure, Stop, SessionEnd` in the canonical model; `fast::skip` and `decide` match on it exhaustively; `decide()` is pure over `Ctx`, whose `Policy` carries the exec-only routes (`exec_only`), filled by the hook | `crates/adage-trace/src/lib.rs:36-56`; `crates/adage-runtime/src/lib.rs:496-514`, `:302-335` (`Policy`, `exec_only` at `:317`); `crates/adage-hook/src/lib.rs:1041-1051`; `crates/adage-runtime/src/fast.rs:40` |
| The command a pending directive named | `Pending { route, expect_key, expect_head, batches_left }`: the canonical key, not the bound command text | `crates/adage-runtime/src/lib.rs:75-84`, `pending_for :1174-1182`, `bind_prompt_route :1237-1264` |
| An authority to predict a call | Grants are `ApprovalKey { repo, route_exec_digest, argv_templates, scripts, effect, level: E1\|E2, output_mode, profile_id, signer }`; every lookup selects by `(repo, route, level)` and only then checks the full key; `Store::approve` deletes the older approval for the same route and level; `--effect` on `review delegate` accepts `verify`/`regenerate` | `crates/adage-exec/src/approvals.rs:22, 200-236`, `crates/adage-exec/src/hook.rs:195, 774, 931-948, 992, 1230`, `crates/adage-exec/src/route.rs:570-589, 681-705`, `crates/adage-cli/src/cli.rs:251-267` |
| Sub-support patterns at hook time | The alphabet floor is `c >= s_min` in `Db::build`; mining targets and expansions need support `>= s_min`; border children are dropped unless `record_all`; `mine()` runs with `record_all = false` and keeps frequent patterns only; `border::Incremental` is used only in `tests/border.rs`; `judge`'s `Trigger::Context` arm returns `Ok` unconditionally (only after-edit and after-command routes hit the support error); nascent routes are filtered at compile | `crates/adage-graph/src/mine/mod.rs:46-47, 82-84, 257-273, 505-512`, `mine/seq.rs:3, 95-114, 367, 380-381`, `mine/border.rs`; `crates/adage-learn/src/engine.rs:132, 746-768`; `crates/adage-tracks/src/index.rs:19-33, 157-164, 199-223` |
| A `skill` response | `Step` is `Run, ReadBatch, Avoid, Advise, Sequence, Card`; `shape()` and `SHAPES` list seven; Skill tool uses are imported as `CanonicalAction::Control { tool: "Skill <name>" }`, effect Observe, and the canonical bridge drops every `Control` action; `wire::named` records every backquoted span as a command | `crates/adage-tracks/src/model.rs:188-209, 558-596`; `crates/adage-core/src/tools.rs:72-86`; `crates/adage-graph/src/canonical.rs:40-72` (`_ => {}` at `:70`; `adage-learn/src/bridge.rs` is a re-export shim); `crates/adage-graph/src/input.rs:26-33`, `events.rs:27-34`, `mine/mod.rs:467-475`; `crates/adage-trace/src/wire.rs:49-104` (re-exported by `render.rs:1025`); `Directed` at `crates/adage-trace/src/session.rs:95-113` |
| Verified skill steps a hook can read | `adage train` writes `procedure.json` (deterministic, digested) and `review.json` to a staged pack under `train/skills/<name>/`; `Verified { NotReplayed, Works, Stale }` per candidate; `train accept` is the person's step | `crates/adage-pack/src/procedure.rs:32-79`, `crates/adage-train/src/compile.rs:38-62`, `crates/adage-train/src/skill.rs:297`, `crates/adage-cli/src/train.rs:85` |
| A machine-readable live feed | `adage watch` tails spools and `learn.log` into `FeedItem`s whose `seq` is "assigned by the live view" and draws with ratatui; `--once` prints a frame; the feed's acts include `Withheld`, `Unaided`, `NotReached`, `StopWithheld` and records carry the arm | `crates/adage-cli/src/ui/tail.rs:31-33, 105-107`, `ui/feed.rs:16-70, 232-275`, `ui/watch.rs:16, 37-44, 178` |
| Harness-priced cost | `Cost { usd, source }`; Claude sessions priced from a pinned table; `.seen.json` keyed by a hash of the transcript path; `Ledger::update` rewrites the file by session id without a lock | `crates/adage-learn/src/ledger.rs:7, 30-62, 381, 435-455` |
| Evidence that the mod saves requests | `demo/run.py` records `num_turns` and `total_cost_usd`, not requests, and wires hooks through `--settings` built by `hooks-config`; the registered metric is the debug log's dispatch count cross-checked against the stream's explicit request ids; the development harness `dev_v9.py` has arms `A0, A2f, A4`, 16 blocks and the registered `dev_thresholds_v9` rule | `demo/run.py:36, 105-113, 140-149, 198-219`; `evals/reflex-proof/harness/heldout_analysis.py:67, 233-249`, `probe.py:155-156`, `dev_v9.py:57-74, 515-527`; `docs/launch-gate-protocol.md:340-352, 561-611` |
| JSON surfaces for a pane | `graph --json`, `doctor --json`, `status --json`, `delegate status --json`, `savings --json` exist and every reflex is one JSON shape, `adage_app::reflex::Reflex` (test `every_surface_renders_a_reflex_the_same_way`); the status card has no `--json`; no `adage off`/`on`; `adage-exec` has `hook, run, approve, revoke, status, doctor, journal, recover` only | `crates/adage-cli/src/graph.rs:186-203` over `crates/adage-app/src/graph.rs:83`, `doctor.rs:1026-1036`, `main.rs:211-268, 624-660`, `exec.rs:327`, `crates/adage-app/src/savings.rs`, `crates/adage-app/src/reflex.rs:15, 153-154`; `crates/adage-exec/src/main.rs:428-446` |
| The plugin's CI | The `plugin` job installs `@anthropic-ai/claude-code` unpinned and runs `validate --strict` only; PR CI tests the affected packages and always runs `cargo test -p xtask` and `adage-exec --test no_spawn` | `.github/workflows/ci.yml:228-247`, `:104-134` |
| Today's CPU prediction | adage-exec's `candidates()` picks a learned `Context` route with execution evidence, no slots and a Verify or Regenerate effect, blanks its directive, runs it under the E2 grant or abstains, and reports the route ids in `Delivery.cpu_predictions`; the hook retires those routes' `Pending` entries | `crates/adage-exec/src/hook.rs:137-150, 863-903`; `crates/adage-hook/src/lib.rs:111-113, 1118-1122` |
| The no-routes path | `respond` writes one `seen` record (`unrouted: true`) for a `.git` repository with no routes before returning | `crates/adage-hook/src/lib.rs:961-981, 997-1007` |
| The person check | `require_person` is a wrapper over `adage_app::person::require`, which returns a `Person` token that `adage_app::review::decide` requires and that takes the tracks lock itself | `crates/adage-cli/src/share.rs:28-30`; `crates/adage-app/src/person.rs:15, 29-50`; `crates/adage-app/src/review.rs:596-609` |
| The learner's writes | Every write goes through `adage_learn::store::Store`; the index is compiled by `Files::compile_index`; an in-memory store learns with nothing on disk | `crates/adage-learn/src/store.rs:24, 83-88`; `tests/remote_sessions.rs` |

## 2. What changes where

### 2.1 The learner (`adage-learn`, `adage-graph`)

1. **Candidates are mined in a pass of their own.** The routes' mining is untouched: lowering
   its alphabet floor would insert events between the items of today's patterns (`gap` and
   `within` count alphabet events, `mine/mod.rs:46-47`) and change reflexes in service. Instead
   `adage-graph` gains `mine_candidates(db_input, params)`: a second `Db` built with the floor at
   `s_min − border` (`Db::build`, `:257-267`), `seq::mine` run with the border recorded
   (`record_all`, `seq.rs:380-381`), and only patterns with support in `[s_min − border, s_min)`
   and a `Context` trigger of two or more events kept (one-event after-edit or after-command
   patterns and prompt-family patterns do not qualify: the design's rule is a sequence, never a
   single call, and the runtime notices by walking a context net). They are scored and compiled
   as reflexes are (`select.rs:533 select`, `compile/shapes.rs:1094`), flagged
   `Compiled.candidate: true`; the engine branches on the flag **before** `judge`
   (`engine.rs:746`), so a candidate is never judged into `routes`, and writes, through a new
   `Store::write_candidates` method (`store.rs:24`; the in-memory store of `tests/remote_sessions.rs`
   implements it too),
   `Candidate { id, trigger, step, support, s_min, days, last_seen, review: false }` to
   `<tracks>/<repo>/learned/candidates.json`. A candidate's id is the stable id the same pattern
   gets as a route (`compile/ids.rs`), so the day it crosses `s_min` the person sees one name.
   A **nascent** route with a `Context` trigger (`RouteState::Nascent`, `model.rs:550`) is also
   written as a candidate with `review: true`. `learn.log`'s `changes[]` gains
   `change: "candidate"` (`diff.rs:27-35 ChangeKind`) with `support` and `s_min`, and the line
   that turns a candidate into a route names the session ids that tipped it (the border entry's
   `sessions`). Acceptance pins that the learned routes, the goldens and the route-fidelity gates
   are unchanged and that the miner's `truncated` flag stays false on every dataset.
2. **Skill selection is learned.** Today the canonical bridge drops every `Control` action
   (`adage-graph/src/canonical.rs:70` `_ => {}`) and the graph's `input::Step` is `Exec | Read |
   Edit` (`input.rs:26-33`). Add `input::Step::Skill { name }` and `NodeType::Skill`
   (`events.rs:27-34`), map `CanonicalAction::Control { tool }` whose `tool` starts with `Skill `
   to it in `canonical::push`,
   admit it in `target_label` (`mine/mod.rs:467-475`) as a continuation target only (never a
   context item on its own), and give the compiler a `skill` shape (`compile/shapes.rs:1094
   compile`, beside `fixed_command` at `:224`): continuation one Skill node → `Step::Skill {
   skill }`, the same support, rate and uplift gates as `run`, strength ceiling **prime**. Every
   other `Control` tool stays out. The delegation stimulus is already a key: `control Task
   <subagent_type>` (`tools.rs:77-81`) becomes a `CtxItem::Act`. `StepObs` is unchanged by this
   (the key already carries the name), so no cache format bump here.
3. **Adage's predicted calls are not organic evidence.** `By { Agent, Adage }` is defined once in
   `adage-trace` and carried on the canonical `Call` (`adage-trace/src/lib.rs:27-32`; `Call` has no
   `Default`, so every literal site gains `by: By::Agent`) and, through `StepObs.call`
   (`session.rs:18`), into the learner; no second `by` field. The Claude decoder
   (`adage-ingest/src/claude.rs`) sets it from the `tool_use_id` prefix `adage_` in the transcript
   (the mod mints `adage_<route>_<turn>_<index>`, one format in the spike, the design and the
   tests), bumps `adage-ingest/src/cache.rs:73 SESSION_FORMAT` to 5 so cached parses are rebuilt,
   and the engine excludes such steps from `support` the way exposed sessions are (`exposed()`,
   defined at `engine.rs:132`, called at `:665`). `wire::named` (`adage-trace/src/wire.rs:69-104`;
   the writer is `render::route_text` in adage-runtime, the reader in adage-trace, kept in step by
   the runtime's round-trip tests) matches the skill template **before** its backquote scan, so
   the skill name lands in a new `Named.skills` and `Directed.skills` (`session.rs:101-113`), not
   in `commands`; exposure (`engine.rs:119-136`) treats a named skill as it treats a named command. When S9 Q5 finds the
   row's `origin` on disk, read that too.
4. **The ledger takes a harness-priced line without changing its schema.** `adage ledger note`
   appends to `ledger/harness.jsonl`, not to the ledger; the learner, when it writes the ledger
   under its lock, merges one harness line per session id (`SessionSummary` gains optional
   `steps: Vec<StepUsage { index, model, input, output, cache_read, cache_write, predicted }>`,
   `hook_ms: Vec<u32>`, `harness_version`), prefers the harness line for `cost` and never lets
   a later transcript pass replace it (`ledger.rs:381`). `model_turns` counts steps the model
   answered; `predicted_steps` counts the rest, so the savings estimate sees a predicted step as a
   model turn not spent (`adage-app/src/savings.rs:417`). Integrity: a note is accepted only for a session id
   whose spool holds a `session_start` in this repository and whose `harness_version` is a
   version string; one line per session; the agent cannot mint a session it did not have.

### 2.2 The harness (`adage-hook`, `adage-runtime`, `adage-exec`)

1. **`PreToolUse` identity, two phases, one record.** The payload the mod sends gains two optional
   fields a command hook never sends: `dry_run: true` (return the plan, spool nothing) and
   `verdict: { engine?: "same" | "differs", answered?: "receipt" }` (decide finally, spool once).
   `pretool()` records the `tool_use_id` (as a 64-bit hash) in a bounded ring on `SessionState`
   (`seen_pretool`, 64 entries, `#[serde(default)]`) **after** the word scan at `lib.rs:565-570`
   (which now gates on `capabilities().pre_tool_rewrite`, `:559`) and before any receipt or
   planner work, on every path that prints (the rewrite, the receipt-only record at `:586-604`,
   the control arm's `Ok(None)` at `:720`), and returns
   `Ok(None)` with no record on a repeat. A `dry_run` call records nothing. The spooled `pretool`
   record gains `checked_by: "engine" | "planner"`.
2. **`TurnStep`.** A new `Event::TurnStep { turn_id, index, model, agent: Option<String> }`,
   translated from a Claude payload the mod builds (§2.5), decided by a new arm of `decide()`
   that **predicts only what Adage has already directed**: a `pending` directive whose
   `expect_key` has not been observed, or an armed, unmet obligation, in delivery order, whose
   route is in the predictable set (below), whose step is a `Run` with every slot bound, a
   `Sequence`'s first command, or a `Skill`, and whose bound command is non-empty. `Pending` gains
   `#[serde(default)] command: String`, filled where the directive is bound (`bind_prompt_route
   :1237-1264`, `pending_for :1174-1182`). The arm answers nothing at `index == 0`, re-checks
   `interlock::avoids` against the session's inhibits (`approach_lock`, `:1009-1030`), never
   predicts a route adage-exec's own CPU prediction handled at this batch (any id in
   `Delivery.cpu_predictions`, `adage-hook/src/lib.rs:111-113`: that path ran or abstained in the
   sandbox under an Execute grant, and the §2.2.3 refusal keeps the two grants apart), and holds
   two bounds in the state: `last_step_predicted: Option<(String, u32)>` (never two predicted
   steps in a row) and `predictions: u32` (default cap 6, config `runtime.predict_cap`).
   `Decision` gains `predicted: Option<Prediction { route, track, tool, input, kind }>` with `kind
   ∈ { obligation, run, sequence, skill }`, `would_predict`, `routine`, `would_route`.
   `fast::skip` answers `false` for `TurnStep` when the state has a pending directive or an armed
   obligation, `true` otherwise. The grant is read in `respond()` before the state lock, like
   exec-only routes (`adage-hook/src/lib.rs:1032-1038`), as `Extension::predict_grants(repo) ->
   Vec<(String, String)>` (empty without adage-exec), and handed to `decide()` as a new `Policy`
   field, `predict_grants`, beside `exec_only` (`adage-runtime/src/lib.rs:302-335`; there are no
   thread-locals any more, and the runtime, at layer 3, cannot link adage-exec at layer 7, so the
   field carries `(track, route)` strings). A new `Output::Predict`
   renders on Claude as `{"predict": {…}}` and as `None` on the other adapters
   (`codex.rs:79`, `opencode.rs:77`). The spool record kind `turn_step` is added to every
   consumer: `rebuild::kind_of` (`rebuild.rs:45-58`), `ui/feed.rs::from_record` (`:232`),
   `adage-app/src/savings.rs:345-349`, `adage-app/src/graph.rs:387`,
   `adage-eval/src/transcript.rs:320-328 hook_name`, `crates/adage-cli/tests/g4_hook.rs:129-137`;
   `Event::TurnStep` itself is an `adage-trace` change (`adage-trace/src/lib.rs:36-56`) and the
   exhaustive `Event` matches at `runtime/src/lib.rs:514-515` and `fast.rs:41-42` gain the arm.
2b. **Prediction kinds: the contract between the binary and the mod.** Every way Adage acts in
   the model's place is one `kind` the binary names and the mod dispatches on; adding a kind is a
   row here, a variant in `PredictionKind`, and one branch in the mod. The binary decides which
   kind applies; the mod never chooses.

   | Kind | Event | The binary needs | What the mod does | Bound | Judgement it leaves | POC |
   | --- | --- | --- | --- | --- | --- | --- |
   | `run` | `TurnStep` | a `pending` `Run` directive with its command bound, a `predict` grant | yields one `Bash` call | never at index 0, never twice in a row, the session cap, not an inhibited route | `Predicted` on the batch that observes it | P1, P3 |
   | `obligation` | `TurnStep` with `ending: true` | an armed, unmet obligation, a `predict` grant | yields the obligation's call in place of the model's stop (Q15) | as `run`; the Stop gate stays as the fallback | `Predicted`; the obligation is met by its call | P1 (binary); mod pending Q15 |
   | `sequence` | `TurnStep` | a `pending` `Sequence` directive | yields its first command; later steps are new `TurnStep`s with a model step between | as `run` | `Predicted` per step | P1, P3 |
   | `chain` | `TurnStep` | a `pending` `ReadBatch` whose files are bound (from the directive, or from the preceding call's result) | yields several `Read` calls in one response | as `run`, counted as one yielded response | `Predicted` | P1, P3 (binding from a result: C7) |
   | `skill` | `TurnStep` | a `pending` `Skill` step, a `predict` grant, the skill recorded inert and accepted | yields `Skill { skill }` | as `run`, plus `skills.invoke` | `Predicted` | P1, P3 (D3/D4 supply the step) |
   | `substitute` | `PreToolUse` (`tool.call`, `dry_run`) | an `avoid` reflex at `fire` with a fixed `instead`; the engine's verdict equal for both commands | `next({ ...e, command })` with one context line | same permission verdict only | `Substituted`, never a follow | P5 |
   | `receipt` | `PreToolUse` (`tool.call`, `dry_run`) | a fresh covering receipt (adage-exec); the engine would have allowed the call unasked | answers `{ result }` without `next` | `allow` verdict only | the obligation it covers is met | C3 |
   | `routine` | `TurnStep` | a `fire` reflex pending without a grant; `runtime.routine_model` set | `next({ ...e, model, effort })` | never in control | none (a routing, not a call) | C5 |

   The JSON on the wire for `TurnStep`: stdin `{ "session_id", "cwd", "hook_event_name":
   "TurnStep", "turn_id", "index", "model"?, "agent_id"?, "ending"? }`; stdout nothing, or
   `{ "predict": { "kind", "route", "track", "calls": [{ "tool", "input" }, …] } }` (one call for
   `run`, `obligation`, `sequence`; `{ "tool": "Skill", "input": { "skill" } }` for `skill`;
   several for `chain`), optionally `"routine": true`. A non-zero exit, a timeout or non-JSON is
   "nothing". The control arm answers nothing and spools `would_predict` with the same shape.

3. **The `predict` grant.** Not a new `Effect` (its order is security-relevant in `run.rs:453,
   461`, `route.rs:404`, `policy.rs:163`). A new field on `ApprovalKey`, `mode: GrantMode {
   Execute, Predict }` with `#[serde(default)] = Execute`, level `E2`, `profile_id` the literal
   `harness`. Because every lookup selects by `(repo, route, level)` first, **`mode` joins every
   selector**: `Store::approve` (`approvals.rs:232-236`), `revoke_automatically` (`:210-212`),
   `lapsed` (`:200-206`), `Revocation`, `plan_grants` and `lapses` (`route.rs:570-589,
   681-705`, which plan a predict key on its own path: no action spec, no sandbox profile), and
   the four hook sites (`hook.rs:195 exec_only`, `:774 covering_receipt`, `:931 E2 actions`,
   `:1230 E1 prefetch`), each filtering `mode == Execute`. A route with an `Execute` grant and a
   `Predict` grant keeps both through approve, revoke and lapse. `adage review delegate <track>
   <route> --predict` writes it (person-only, `exec.rs:623 require_person`; exclusive with
   `--effect` and `--level`), and refuses when the route's effect is above Regenerate, when
   `policy.effects_deny` names it, when it is an environment action and the policy does not allow
   them, when the route is `exec_only`, or when the route already holds an `Execute` grant for
   the same step (two paths would run it). `reapprove_command` (`route.rs:650`) appends
   `--predict`. A machine switch `adage delegate predict on|off` (beside `rewrites` in
   `exec.rs:134-188`) writes `exec/predict-off` like `rewrite-off`; `on` requires a person.
   `adage-exec` gains a `version` verb (the mod's activation probe, §2.6).
4. **Provenance of a predicted call.** The Claude adapter reads `tool_use_id` on `PostToolBatch`
   and `PostToolUseFailure` (`claude.rs:42-53 call_from` drops it today) and sets `Call.by =
   By::Adage` for the `adage_` prefix; `PreToolUse` reads it directly in `pretool()`. `decide()`
   judges such a call `Judgement::Predicted { route }` instead of `Followed`; downstream it is
   excluded from follow rates and strength evidence (`evidence.rs`, `review.rs`, `ui/routes.rs`),
   is not extinction evidence (`instincts.rs`), counts in `savings.rs` as a model turn not spent,
   and draws in the feed as a "chose" row (`ui/feed.rs`, `ui/view.rs`, `graph.rs`); the receipt
   path in adage-exec treats it as Adage's own run for the duplicate-run rule.
5. **Receipt as the result.** `Extension::pretool_context` returns a string today
   (`lib.rs:190-194`, `adage-exec/src/hook.rs:177, 751`). It returns `PreNote { text, fresh:
   bool, result: Option<String>, covering: String }`: `fresh` when `covering_receipt` found a
   completed, applied run on byte-identical inputs covering the call, `result` the receipt as the
   tool's stdout would read, `covering` the command that ran. The `receipt` object is rendered
   **only** when the payload carried `dry_run` or `verdict` (a command hook's output is
   unchanged, so the golden does not move). The receipt check stays pytest-and-Verify only
   (`hook.rs:757-769`); widening it is not in this plan.
6. **`noticed`.** After `fire_contexts` (`runtime/src/lib.rs:1116-1145`), a second walk over the candidate net
   with `st.cand_ctx.recent()` (§2.3.2); every id it returns goes to `Decision.noticed:
   Vec<Noticed { candidate, support, s_min, review }>` and never to `out`; `SessionState.noticed`
   keeps each candidate to once per session, bounded at 64 like `MAX_NEW_TESTS`
   (`runtime/src/lib.rs:150`). Both arms record it (it is not a directive). The spool record
   carries it inside `decision`, so `rebuild` needs no new kind, and `rebuild` replays `cand_ctx`
   as it replays `ctx` (`rebuild.rs:88-89, 163-165`). The no-routes path
   (`adage-hook/src/lib.rs:997-1007`, which now writes a `seen` record first, `:961-981`) becomes
   "no routes and no candidates".
7. **`SkillPrompt`.** A new Claude-only event, `<bin> hook claude SkillPrompt`, payload
   `{ session_id, cwd, skill, text }`, answered `{ "text": <annotated> }` or `{}`. It reads
   `$ADAGE_HOME/train/skills/<name>/verified.json` (§2.4) **only for a pack the person has
   accepted** (`train.rs:85`), whose `skill.name` equals `skill` and whose repository matches,
   and appends one plain line per verified or stale step under a fixed heading, every line
   through `render::sanitise` (`render.rs:12`). The file's type and reader live in `adage-pack`
   beside `procedure.rs` (layer 4), because the hook (layer 6) may not link `adage-train`
   (layer 9; `xtask/src/layers.rs`). It assigns the arm if none is assigned yet, as `pretool`
   does (`lib.rs:673-684`); in the control arm the text is unchanged and `would_annotate` is
   spooled (`kind: "skill_prompt"`).
8. **`Standing`.** A read-only Claude-only event, `<bin> hook claude Standing`, answered
   `{ standing, environment }` (mod architecture §3.6): the repository's applicable procedure cards'
   titles and the instincts' one-liners in `render.rs`'s fixed templates, ≤ 2,000 characters,
   deterministic for the session; the U11 environment sentence when bound. It sets
   `SessionState.has_standing` so the per-event delivery of the same text is suppressed. Empty in
   the control arm. It does not touch `record_start`, the learner spawn or `take_notice`
   (`session_start`, `lib.rs:751-872`), which stay the command hooks' `SessionStart`.
9. **Latency on record.** Every spool record gains `duration_us` (the hook's own wall time from
   `started`, a lower bound that excludes the spawn): the decision record (`lib.rs:1144`), the
   `pretool` records (`:592`, `:713`), `stop_gate` (`:403`), `record_start` (`:950`) and the
   `seen` record of the no-routes path (`:961-981`); `started` already reaches `handle()`
   (`:335-342`) and is passed on into `respond` and `pretool`. `doctor`'s new check reads it (§2.5)
   and caches its result for a few seconds, because `watch` and `ui` re-run `checks()`
   (`doctor.rs:948-950`).

### 2.3 Tracks and the index (`adage-tracks`)

1. **`Step::Skill { skill: String, args: Option<String> }`**, shape `"skill"`, `SHAPES` grows to
   eight; `io::validate` requires a safe id (`[A-Za-z0-9_-]`, 1–64), no slots, and refuses
   `Inhibit` (a skill is never a dead end); `ceiling` is **prime** (`model.rs:588-596`).
   `render::route_text` gets a fixed template: "In this repository the `{skill}` skill is used at
   this point. Ignore this if the user is asking for something else." Follow is judged on the key
   `control Skill {skill}` with `pattern::meets`. Exhaustive `Step` matches that gain an arm:
   `adage-tracks/src/io.rs:344`, `interlock.rs:203`, `adage-runtime/src/lib.rs:358-365`
   (`card_text`), `render.rs:901-1023` and `:889-890`, `adage-pack/src/convert.rs:303`
   (`Skill => Err(..)`: packs are deferred), `adage-safety/src/lib.rs:243`, `adage-eval/src/score.rs:379,
   575`, `savings_replay.rs:456`, `conventions.rs:423`, `lib.rs:225-226`,
   `adage-app/src/reflex.rs:293-294`, `adage-app/src/graph.rs:74-76`, `adage-graph/src/compile/ids.rs:50`,
   `adage-cli/src/share.rs:139`, `ui/routes.rs:288-291`; wildcard arms to review by hand (they
   compile silently): `adage-learn/src/value.rs:731-734`, `dormant.rs:219`,
   `adage-tracks/src/interlock.rs:91-99`, `engine.rs:119`. `cargo clippy --workspace --all-targets -- -D warnings` is the checklist for
   the exhaustive ones.
2. **`Index.candidates: BTreeMap<repo, Vec<Candidate>>`** and **`Index.candidate_contexts:
   BTreeMap<repo, CtxNet>`**, both `#[serde(default)]`. `Index::compile(tracks)` has 60 callers
   and derives `triggers` and `contexts` from `routes` (`index.rs:199-210`); candidates are not
   derivable from tracks, so `compile_root` (`:216-223`, where `exec_bindings` is read) reads
   `candidates.json` beside the `learned` track, and `compile` itself is unchanged, as are its
   callers and `index_is_deterministic`. The candidates get a net of their own: the routes'
   `CtxNet.digest` is the alphabet's hash, a changed digest resets every session's context state
   (`CtxState::align`, `adage-tracks/src/matching.rs:72-77`; the matching primitives moved there,
   and `adage-runtime/src/context.rs` is a re-export), and candidates change at nearly every learn.
   `Triggers::of` (`triggers.rs:102`) folds every candidate item's words and globs in. The runtime
   keeps a second ring, `SessionState.cand_ctx: CtxState`, symbolised from the same event by a
   second `Symbols` over the candidate net (`matching.rs:135`, built as at
   `runtime/src/lib.rs:504` with `.exact_keys(..)`; only when that net is non-empty). The 26 recorded
   indexes under `evals/datasets/*/recorded/` keep parsing.
3. **`Candidate`** lives in `adage-tracks/src/model.rs` beside `Route`; `io` reads and writes
   `candidates.json` with the same owner-only mode as the index (`io::write_private`).
4. **Older binaries.** An index holding a `Step::Skill` fails to parse in a binary from before
   D3 (`index.rs:287` → `Damaged` → relearn), and two binaries on one machine (an npm `adage`
   beside the plugin's) would flip the index back and forth. The index gains a `format: u32`
   field that older binaries ignore and newer ones read; D3 bumps it, `doctor` warns when the
   binaries on `PATH` and in the plugin differ in major version, and `docs/live-learning.md`
   states the minimum-version rule.

### 2.4 Training (`adage-train`)

1. **`verified.json`** beside the staged pack: `$ADAGE_HOME/train/skills/<name>/verified.json`,
   `{ schema: "adage-train-verified/1", skill: { name, sha256, inert: bool }, repo, steps: [{
   id, command, result: "works" | "stale" | "not-replayed", signature?, date, sandbox }] }`,
   written in `skill.rs::stage` (`:297`) from `Candidate.verified` and `Report.sandbox`.
   `inert` is true when the SKILL.md holds no dynamic-context block (`` !`…` ``) and no
   `context: fork`; a skill that is not inert is never predicted (§2.2.2 checks the flag for
   `Skill` steps). It is outside the pack on purpose: a date would break the pack's deterministic
   digest (the `cards.json` precedent).
2. `adage train accept` marks it accepted (`accepted_at`); `SkillPrompt` and `TurnStep` read only
   accepted files; `adage train` on a changed skill (new `sha256`) replaces it unaccepted.

### 2.5 The CLI and surfaces (`adage-cli`)

1. **`adage watch --follow --json --session <id> [--since <cursor>]`**: `ui/watch.rs` gains a
   mode that needs no terminal and bypasses the ratatui `Live` view (`watch.rs:37-44`), emitting
   one JSON line per `FeedItem` (`ui/feed.rs:56`), per `learn.log` line (`kind: "learn"`), per
   health change (`kind: "health"`, the lights as `health::gather` builds them) and per learner
   state change (`kind: "learner"`, `waiting | running | idle`, `next_at`), from the same
   `Sources` (`watch.rs:16`; argument parsing at `ui/mod.rs:97-120`). Every line carries an opaque `cursor` (the spool paths and offsets
   from `SpoolTail.cursors`, `tail.rs:33`, plus the `LineTail.offset`, `:107`, encoded as one
   string); `--since <cursor>` resumes a new process from it. For the `--session` id, the stream
   **omits** the `control` flag, `would_have`, `would_predict`, `would_continue`, withheld
   pre-tool records and the `Withheld`, `Unaided`, `NotReached` and `StopWithheld` acts. It exits
   0 on stdin EOF, SIGTERM or the parent's death. The record shape is documented in
   `docs/live-learning.md` under "Streams to follow".
2. **`adage ledger note`** (`cli.rs` and the `main.rs` dispatch): reads one JSON object from
   stdin (§2.1.4's shape), validates it against the spool, appends to `ledger/harness.jsonl`,
   and refreshes the savings summary (`adage-app/src/savings.rs:427 refresh`). Allowed from inside a session
   (it tunes nothing; integrity per §2.1.4).
3. **`adage review delegate … --predict`**, **`adage delegate predict on|off`**, and
   `delegate status --json` showing `mode` (`exec.rs:327`).
4. **`adage doctor`** gains the hook p95 check from spool `duration_us` over `batch` records
   with an empty `would_have` (warn above 10,000 µs, fail above 25,000, `pass` with the detail
   "too few hooks to measure" under 20 records), cached, and a line for the harness version last
   seen; `doctor --json` gains a structured `action` beside the prose `fix` (`Check` at
   `doctor.rs:42-51`; the status card's `single_command` already extracts one backticked command
   from `fix`, the precedent) so a pane can tell "run `adage condition`" from a terminal-only fix.
5. **`cargo xtask tokens`** gains a third output, `plugin/claude-code/hooks/tokens.ts`
   (`xtask/src/main.rs:90-96` `outputs`), with the glyph table (UTF-8 and ASCII) and the roles the
   mod maps to theme keys; `the_committed_tokens_are_current` (`xtask/src/main.rs:431`) asserts
   it like the other two.
6. **The status card** gains `--json` (`status_card.rs:461`; a top-level flag at `main.rs:267`),
   the same rows the card prints, and swaps `adage ui …` for `/adage watch` when the caller says
   it draws in a session (`--for-session`).
7. **`adage off` and `adage on`** (new, A6): `off` writes `<root>/.adage/disabled` and is
   allowed anywhere (it turns something down, `docs/live-learning.md`); `on` removes it and
   requires a person (`adage_app::person::require`, `person.rs:29-50`, or a press); both exit 0
   outside a repository with one line.
8. **The press** (A7, design §3.12): a `Press` event issues a single-use, chained token in the
   window before the agent's first action, before the kill switches; a new
   `adage_app::person::require_or_press` at specific call sites reads `{ press, session }` on
   stdin, mints the `Person` token the application layer requires, and prints the next token; `review delegate` gains `--stage --json` and `--confirm`; every
   decision records `via: press | terminal`; `graph --json` gains `offers`. **One plugin** (A8):
   both binaries in `plugin/claude-code`, `$DATA/bin/hook` linked by the marker, 30 s timeouts.

### 2.6 The mod (`plugin/claude-code`, `plugin/claude-code-exec`)

Files as in the design §2: `hooks/hooks.json` gains `"modules": ["./register.ts"]` beside the
command hooks; `hooks/register.ts` (the activation probe, `active`), `bridge.ts` (`ask()` with
tagged results, `env: { CLAUDECODE: '1' }`, the argv allow-list `hook | version | watch | graph |
doctor | status | ledger note | off`), `decide.ts`, `draw/*.tsx`, `feed.ts`, `commands.ts`,
`tokens.ts` (generated), `*.test.ts`; `types/index.d.ts`; `plugin.json` gains `"types"` and
`userConfig` (`band` and `pane` only; `ui.noticed`, `ui.spinner`, `ui.turn_line`, `skills.invoke`,
`runtime.routine_model` and `runtime.predict_cap` are `config.toml` keys; no switch for asking). One command registration, `/adage`, with an argument hint. The person-only
verbs run only through `press()` (B4), with the token on stdin. Until A8 the same
module ships in `plugin/claude-code-exec`, where hook events go to `adage-exec` and CLI verbs to
`adage`, which `plugin/build.sh` stages beside it (its `SHA256SUMS` step already lists both
names) and the exec shim links into `$DATA/bin`. The mod never falls back to `PATH` when its
linked binary is `scripts/quiet`. `ADAGE_MOD_OBSERVE=1` (read through `$.env.get` at `register`)
puts the mod in observe mode: only the `classic.*` observers and `ui.render` register. `scripts/
check-plugin.mjs` learns the module files, `types`, the allow-list and the pinned CLI version.
CI's `plugin` job pins `@anthropic-ai/claude-code@2.1.287`, runs `claude plugin test` after
`validate --strict`, and diffs a fresh `validate --strict` output against the committed
`VALIDATE.txt`. `turn.complete` calls `adage ledger note` (§2.1.4).

### 2.7 Evaluation (`adage-sim`, `demo/`, `evals/`, `adage-eval`)

1. **The simulator** emits `PreToolUse` (per Bash call, with `tool_use_id`), `TurnStep` (per
   turn, after the batch), `Standing` and `SkillPrompt` payloads for Claude
   (`crates/adage-sim/src/claude.rs` beside `turn_hooks` at `:158`), and `scenario::Replay` gains
   per-turn expectations (`predict`, `noticed`). `scenario::history` gains a deliberately
   sub-support sequence: in 2 of the 12 history sessions the agent runs `make lint` right after
   `make test` (a two-event context with support 2, below `--demo`'s `min_support = 3`,
   `select.rs:72-75`, and inside the border); `cargo run -p adage-sim` regenerates the fixtures
   and `committed_fixtures_are_current` enforces it. `harness_sim.rs::replay_claude` asserts the
   expectations in both arms and `check_graph` counts predictions.
2. **The demo runner** gains arms `a0` (no hooks), `mod-observe` and `mod` beside `control` and
   `adage` (`run.py:105-113`); the mod arms load the plugin only (`--plugin-dir`, with the binary
   `plugin/build.sh` staged and the same `ADAGE_HOME`), never `--settings` from `hooks-config`
   as well, so no hook runs twice; `DEMO_MODEL` stays `claude-sonnet-5` unless set. A `requests`
   metric counted as `heldout_analysis.requests()` counts it: `[DEBUG] [API REQUEST]` dispatches
   under `--debug`, cross-checked against the stream's distinct explicit request ids, which must
   be unique (`heldout_analysis.py:67, 233-249`, `probe.py:155-156`); `report.py::compare`
   already takes arm names. `score.py`'s real keys (`passed, ci_passed, hidden_passed,
   hidden_tests_run, stale_generated, ci_tail, hidden_tail`) are all recorded.
3. **The offline replay gate** `fast_path_changes_no_decision` (`adage_eval::replay`) covers
   `TurnStep` (one injected after each `Batch`) and candidates: a skipped event's full decision
   must be empty, `noticed` included. A new test loads every recorded index (26).
4. **The reflex-proof harness** gets a `MOD` arm (hooks as `A4`, `--plugin-dir`, the `predict`
   grant issued as the human step) judged by the registered `dev_thresholds_v9` rule with its own
   five realized controls, under a new protocol revision registered before any positive block
   runs; a registered held-out run on new groups only after the development loop passes.

## 3. The chunks

Each chunk: what it delivers, the files, the contract, the tests (by test-plan id), the
acceptance criteria, an estimate in engineer-days, and what it depends on. Chunks in one
workstream are ordered.

### Workstream A · Foundation in the binary (no Claude Code dependency)

#### A1 · `PreToolUse` identity dedup and the two-phase payload

- **Delivers.** One decision per `tool_use_id` however many times the event is delivered, and
  the `dry_run` / `verdict` fields the mod will use.
- **Files.** `crates/adage-hook/src/lib.rs:549-723` (after the word scan at `:565-570`: read
  `dry_run` and `verdict`; `state::update` records the hashed identity on every printing path,
  `:586-604`, `:713`, `:720`; a repeat returns `Ok(None)`; `checked_by` on the record);
  `crates/adage-runtime/src/lib.rs:108-147` (`SessionState.seen_pretool: Vec<u64>`,
  `#[serde(default, skip_serializing_if = "Vec::is_empty")]`, bounded at 64, oldest dropped);
  `harness/claude.rs:60-72` (`event_identity` gains `"PreToolUse"`); `rebuild.rs` (the ring is
  not rebuilt: a rebuilt state starts empty, which is fail-open).
- **Contract.** A `PreToolUse` payload with a `tool_use_id` seen in this session produces no
  output and no spool record. A payload without `tool_use_id` is decided as today. A `dry_run`
  payload prints the plan and records nothing. A payload with no Python word costs what it costs
  today (no I/O).
- **Tests.** T-A1-01 … T-A1-07.
- **Acceptance.** `cargo test -p adage-hook -p adage-cli --test hooks` green; one spool record
  per id in the integration test; the Claude golden unchanged (the U11 rewrite is pinned in
  `zero_config_plugin.rs`, where the environment fixture lives; `claude_scenario`'s new
  `PreToolUse` step prints `(nothing)` because its `World` binds no environment); paired
  `hook_latency <before>,<after> - 500` shows the `pretool_no_match` p95 difference within 0.3 ms.
- **Estimate.** 1.5 days. **Depends on.** Nothing.
- **Built.** `09f2d1d` ([impl/A1-A2-C5-notes.md](impl/A1-A2-C5-notes.md)): the identity ring, the two-phase payload with `receipt`, `rewrite`, `advice`, `substitute` plans and the `verdict` phase.

#### A2 · Spool `duration_us` and the doctor p95 check

- **Files.** `crates/adage-hook/src/lib.rs:1144, 592, 713, 403, 950, 961-981` (every record site,
  the no-routes `seen` record included; `started` from `handle()` `:335-342` into `respond` and
  `pretool`); `crates/adage-cli/src/doctor.rs:950` (`checks()` gains `hook latency`, cached for
  5 s; §2.5.4's thresholds and the under-20 rule), `doctor.rs:42-51` (the structured `action` on
  `Check`); `ui/feed.rs` ignores the field.
- **Tests.** T-A2-01 … T-A2-04.
- **Acceptance.** `adage doctor --json | jq '.checks[] | select(.check=="hook latency")'` present
  on a seeded home; paired `hook_latency` shows the no-match cost within 0.3 ms.
- **Estimate.** 1 day. **Depends on.** Nothing.
- **Built.** `09f2d1d`: `duration_us` on every spool record, `adage doctor`'s hook p95 line and threshold.

#### A3 · `adage watch --follow --json`

- **Delivers.** The pane's feed and the band's countdown without a terminal, resumable, and
  silent about the current session's arm.
- **Files.** `crates/adage-cli/src/ui/watch.rs` (`follow_json(ctx, session, since)` over
  `Sources::poll` `:37-44`, bypassing `Live`); `ui/feed.rs` (the cursor; the per-session filter;
  `FeedItem :56-70`, `from_record :232`); `ui/mod.rs:97-120` (accept `--follow`, `--json`,
  `--session`, `--since` beside `--repo --demo --once --size`); `docs/live-learning.md`
  "Streams to follow".
- **Contract.** NDJSON on stdout, one object per line, each with `kind`, `cursor` and the
  record; `--since <cursor>` resumes exactly after that line in a new process; for `--session
  <id>` no line names the arm or a withheld act; exits 0 on stdin EOF, SIGTERM or parent death;
  never writes an escape sequence.
- **Tests.** T-A3-01 … T-A3-06.
- **Acceptance.** 200 rows at 2 Hz for 60 s with no gap across one forced restart (`--since`);
  RSS, sampled from `/proc/self/status` on Linux, flat within 10 MiB; the terminal pane's
  snapshots unchanged.
- **Estimate.** 2.5 days. **Depends on.** Nothing.
- **Built.** `97405bc` ([impl/A3-A6-notes.md](impl/A3-A6-notes.md)), `89d57ff` (`--stdin ignore`, tool-use ids, the `answered` and `recovery` acts).

#### A4 · `adage ledger note` and the harness ledger file

- **Files.** `crates/adage-learn/src/ledger.rs:29-62` (`StepUsage`, `hook_ms` on
  `SessionSummary`, which already has `harness_version`; `Ledger::merge_harness(path)` under the
  ledger lock), `:381` (never replace a harness line); `crates/adage-cli/src/ledger.rs` (new),
  `cli.rs:38-121`, `main.rs:211-268` (dispatch); `crates/adage-app/src/savings.rs:379 gather`
  prefers `source: harness`, `:417` counts `predicted_steps` as turns not spent, `:427 refresh`
  (the estimate lives in adage-app, layer 8, which links adage-learn); `docs/live-learning.md`
  "Files".
- **Contract.** stdin: `{ "session_id", "repo", "harness": "claude-code", "harness_version",
  "started_at", "ended_at", "steps": [{ "index", "model", "input", "output", "cache_read",
  "cache_write", "predicted" }], "cost_usd"?, "hook_ms": [..] }`. Accepted only when the spool
  holds `session_start` for the session in this repository; a second note for the same session
  replaces the first in `harness.jsonl`. Exit 2 on a malformed or unmatched line, nothing
  written.
- **Tests.** T-A4-01 … T-A4-05.
- **Acceptance.** `echo '<line>' | adage ledger note && adage savings --json` shows the session
  with `source: harness` after the learner's next merge; `savings_gate` and `pricing_honesty`
  (`zero_config_plugin.rs:717, 760`) pass with a `population()` that includes a harness line with
  predicted steps.
- **Estimate.** 2 days. **Depends on.** Nothing.
- **Built.** `97405bc`: the verb, the ledger file, its merge into the savings estimate with predicted steps in the population.

#### A5 · JSON for the pane: status card `--json`, `tokens.ts`

- **Files.** `crates/adage-cli/src/status_card.rs:461`, `main.rs:267` (`--json`,
  `--for-session`); `xtask/src/main.rs:90-96, 431` (third output `plugin/claude-code/hooks/
  tokens.ts`: `GLYPHS` with UTF-8 and ASCII twins from the tokens' `glyph` group, `ROLES` mapping
  Adage roles to Claude Code theme keys); `docs/design/style.md` notes the third output.
- **Tests.** T-A5-01, T-A5-02.
- **Acceptance.** `the_committed_tokens_are_current` passes with the committed `tokens.ts`;
  `adage --json` parses and matches the card's rows one to one in a snapshot test.
- **Estimate.** 1 day. **Depends on.** Nothing.
- **Built.** `97405bc`: the card's JSON, `tokens.ts`; `89d57ff` adds `ui` and `recovery`; `af02e95` adds `runtime.routine_model`.

#### A6 · `adage off`, `adage on`, `adage-exec version`

- **Files.** `crates/adage-cli/src/cli.rs:28-121` (`Off`, `On`), `main.rs:211-268` (dispatch),
  a new `switch.rs` (`off` writes `<root>/.adage/disabled`; `on` removes it behind
  `adage_app::person::require`); `crates/adage-exec/src/main.rs:428-446` (`version` prints the crate version;
  `hook` stays first so the no-match path is untouched); `docs/live-learning.md` (the kill
  switches); the status card's first line when off.
- **Contract.** `adage off` exits 0 and creates the file, idempotent, allowed under `AGENT_ENV`;
  `adage on` refuses under `AGENT_ENV` with the person-only message and otherwise removes the
  file; both exit 0 with one line outside a repository.
- **Tests.** T-A6-01 … T-A6-03.
- **Acceptance.** The hooks print nothing after `adage off` in the integration `World`; `adage
  on` with `CLAUDECODE=1` and no press exits non-zero and leaves the file; `adage-exec version`
  prints one line and `adage-exec hook` latency is unchanged (paired run).
- **Estimate.** 0.5 day. **Depends on.** Nothing.
- **Built.** `97405bc`: `adage off`, `adage on`; `adage-exec version` from `d41d655`.

#### A7 · The press token, and `require_person_or_press` at specific sites

- **Delivers.** A person's decision made from the mod, recorded as a person's (`via: press`),
  with a token the model cannot obtain by being steered (mod architecture §3.12). Not a boundary
  against a same-user agent with a shell, and not claimed as one.
- **Files.** `crates/adage-hook/src/lib.rs` (a `Press` event, Claude-only, dispatched **before**
  the kill switches at `:318` and `:355-357`; it issues under the window rule and records
  `press_issued`; `handle()` closes the window on the first `UserPromptSubmit`, `PreToolUse` or
  `PostToolBatch` invocation before `respond()`/`fast::skip`; `SessionEnd` clears the hash),
  `crates/adage-hook/src/harness/claude.rs:74-76` (`translate` reads `source` on `SessionStart`
  and `Press`), `crates/adage-runtime/src/lib.rs:108-147` (`SessionState.press: { hash, chain,
  issued, closed, since }`, `#[serde(default)]`), `rebuild.rs` (`press` is not rebuilt: a rebuilt
  state has no token, which is fail-closed for presses and fail-open for everything else);
  `crates/adage-app/src/person.rs:29-50` (new `require_or_press(cmd, press: Option<Press>) ->
  Result<Person>` beside `require`, which is untouched; it lives in adage-app because it must
  mint the `Person` token that `adage_app::review::decide` requires, `review.rs:596-609`; the
  press is read from stdin JSON `{ press, session }` by `press::read_stdin()`; it lifts the
  terminal check, refuses when `ADAGE_ALLOW_NONINTERACTIVE` is set alongside, checks the
  repository, and on success prints `{ "next": <token> }` and advances the chain), called from
  `adage-cli/src/review.rs:537` (`decide`) and `:581` (`decide_quietly`), `exec.rs:623` (`review
  delegate`, `review revoke`), `exec.rs:194` (`delegate on`: under a press, marker only plus a
  message naming registrations that need a terminal), `exec.rs:286` (`policy --max-level e2`), the
  new `delegate predict on`, the new `on` (A6), `add.rs:217` (only with `--trust` on a pack
  directory committed in the repository); `review.rs:538-550` and `exec.rs:627-637` (no new
  flags: the press travels on stdin); the review judgement and
  `crates/adage-exec/src/approvals.rs:100` (`via: "press" | "terminal"`, set by the binary;
  `--by` unchanged); `exec.rs:521-619` (`preview()` gains a JSON form: `--stage --json` answers
  proposed terms, facts and `digest`; `--confirm` re-plans, re-stages and compares), `route.rs`
  (proposed outputs and caches from the route's observed writes; E1 fields from prompt slots);
  `crates/adage-hook/src/lib.rs:549-723` (the tripwire: a Bash command naming `hook claude Press`
  or carrying `"press"` on a heredoc records `tripwire` and warns once, advice only);
  `crates/adage-app/src/reflex.rs:153-154` and `graph.rs:216-218` (`offers` beside
  `Decisions.verbs`, so every surface renders it), `crates/adage-cli/src/main.rs:624` (`status
  --json`: committed packs awaiting trust, through `adage_app`);
  `docs/execution.md` ("Turning it on", "Approving an action": the press as the second path),
  `docs/live-learning.md` (guarded keys: unchanged; the press paragraph), `docs/zero-config.md`
  §1(a) (persona (a) can now approve) and "Must never happen silently" (a press is a person).
- **Contract.** `adage hook claude Press` with `{ session_id, cwd, source }`: `{ press }` when the
  window is open and nothing was issued for this start (`startup`, `clear`), or once more for
  `resume`; nothing for `compact`, nothing after the window closed, nothing twice. A verb reading
  a valid `{ press, session }` on stdin behaves exactly as at a terminal, records `via: press`,
  and prints the next token; a wrong, spent, stale (> 24 h) or absent press behaves exactly as
  today. State and spool hold the hash only.
- **Tests.** T-A7-01 … T-A7-09; T-X-13.
- **Acceptance.** In the integration `World`: `Press` after `SessionStart` returns a token, a
  second `Press` does not, a `Press` after any `UserPromptSubmit` (matched or not, routes or
  not) does not, a `Press` after a `SessionStart` with `source: compact` does not, after
  `source: resume` does once; a token spent once is refused the second time and the printed next
  token works; `adage review approve T R` with the press on stdin, `CLAUDECODE=1`, stdin not a
  TTY and `ADAGE_ALLOW_NONINTERACTIVE` unset records `via: press`; the same with the variable set
  refuses; `install`, guarded `config set`, `share`, `train accept`, `packs on`, `init`,
  `adage-exec recover` all refuse a press; `--stage --json` then `--confirm` records a grant,
  `--confirm` after a terms change refuses and after an output change answers the fresh diff.
- **Estimate.** 4 days. **Depends on.** A6.
- **Built.** `efcb365` ([impl/A7-A8-notes.md](impl/A7-A8-notes.md)): the `Press` hook event, the token's window and the tripwire, `require_or_press` at the named sites and nowhere else.

#### A8 · One plugin, both binaries (reverses Track C D2)

- **Delivers.** `/plugin install adage@adage` is the only install step; execution needs no second
  plugin.
- **Files.** `plugin/build.sh` (stage `adage-exec` beside `adage` in `plugin/claude-code/dist/
  <target>/`; `SHA256SUMS` lists both), `plugin/claude-code/scripts/session-start` (link or, on
  Windows, copy `$DATA/bin/adage`, `$DATA/bin/adage-exec` and `$DATA/bin/hook` → `adage-exec`
  when `$ADAGE_HOME/exec/enabled` exists, else `adage`, re-made at every start; exec
  `$DATA/bin/hook hook claude SessionStart` at `:121`; carry the exec shim's `PATH` and npm
  fallbacks), `plugin/claude-code/hooks/hooks.json:4-9` (the seven command hooks call
  `${CLAUDE_PLUGIN_DATA}/bin/hook`; `PostToolBatch` and `Stop` at 30 s, as the exec plugin has
  them and the Stop deadline assumes, `lib.rs:440-441`; `hooks-config` output is
  `harness::claude::settings`, `claude.rs:132-146`), `crates/adage-hook/src/lib.rs:936-950`
  (`record_start` records `bin`), `crates/adage-cli/src/exec.rs:245-252` (the plugin message:
  no second install; the settings-file, Codex and OpenCode rewrites at `:216-225` are unchanged),
  `plugin/claude-code-exec/` (one release as an alias: `hooks.json` registers `SessionStart`
  alone, its shim relinks `bin/adage-exec` → `scripts/quiet` and prints `the adage plugin now
  includes execution; uninstall this one`), `.claude-plugin/marketplace.json`,
  `scripts/check-plugin.mjs`, `plugin/test/session-start.test.sh`, `docs/execution.md` ("Two
  plugins" becomes "One plugin"; "takes effect in new sessions" kept),
  `docs/tracks/track-c-compiled-skills.md` (D2 revisited at `:230` and `:448`, with the reason:
  consent is a press).
- **Contract.** With the marker, every hook event reaches `adage-exec hook`; without it, `adage
  hook`; never both; `adage` is always linked for CLI verbs; a stale `bin/adage-exec` link in the
  alias's data directory is overwritten at its first start.
- **Tests.** T-A8-01 … T-A8-05.
- **Acceptance.** `session-start.test.sh` covers both marker states, Windows copies, the
  fallbacks and the alias with a seeded stale link; parity holds; the mod's activation probe (§2
  of the design) becomes a presence probe and T-X-10/T-X-11 are retired; `release_plugin_startup`
  thresholds hold with two binaries staged.
- **Estimate.** 2.5 days. **Depends on.** A7 (the press makes the marker the consent).
- **Built.** `efcb365`: one plugin with both binaries, the shim, `claude-code-exec` kept as an alias that bundles nothing, CI on one plugin.

### Workstream B · The mod: see it (Layer 1 of the design)

Needs S9 Q1, Q2 (Layer 1), Q3 and Q6 to have passed. Every chunk here ships behind
`claude plugin validate --strict` and `claude plugin test` in CI on the pinned CLI.

#### B1 · The module skeleton, the bridge, activation, and CI

- **Files.** `plugin/claude-code/hooks/hooks.json` (`"modules": ["./register.ts"]`),
  `hooks/register.ts` (the probe: `[hookBin, 'version']`; `active`; every registration wrapped;
  `ADAGE_MOD_OBSERVE`), `hooks/bridge.ts` (`ask()` per mod architecture §2 rule 1; `hookBinary($)` =
  `$DATA/bin/<adage|adage-exec>`, `cliBinary($)` = `$DATA/bin/adage`; the allow-list; `env: {
  CLAUDECODE: '1' }`), `types/index.d.ts` (`PluginState`: `session`, `fired`, `rewrites`,
  `rows`, `noticed`, `standing`, `environment`, `hookMs`, `feed`), `.claude-plugin/plugin.json`
  (`types`, `userConfig`), `hooks/bridge.test.ts`, `hooks/register.test.ts`;
  `scripts/check-plugin.mjs` (allow `modules`, require each module file and `types`, check the
  allow-list literal and the CLI pin, keep every other rule); `.github/workflows/ci.yml:195-214`
  (`npm install -g @anthropic-ai/claude-code@2.1.287`, `claude plugin test` for both plugins,
  `claude plugin validate --strict > /tmp/v && diff /tmp/v plugin/claude-code/VALIDATE.txt`);
  the same files mirrored into `plugin/claude-code-exec` until A8 retires it; `plugin/build.sh`
  stages `adage` into the exec plugin's `dist/<target>/` and the exec shim links it (A8 then
  moves both binaries into the one plugin).
- **Where the binaries are.** The shim links `${CLAUDE_PLUGIN_DATA}/bin/<name>`; the mod reads
  `$.env.get('CLAUDE_PLUGIN_DATA')` (Q1 verifies the engine's process carries it; else the mod
  derives it from `$.plugin.root` as the shim does). No `PATH` fallback when the link is
  `scripts/quiet`; a `PATH` fallback only when no link exists at all.
- **Tests.** T-B1-01 … T-B1-06.
- **Acceptance.** `claude plugin validate --strict` output committed as `VALIDATE.txt` and
  diffed in CI; `claude plugin test` passes on the pinned CLI; a session with the mod loaded shows
  `1 mod active · adage` under `/plugin`; with both plugins installed exactly one copy is active
  (the other's probe prints nothing); with the binary removed every hook returns `next(e)` and
  the hook outputs and spool records are identical to a run without the mod.
- **Estimate.** 2.5 days. **Depends on.** A6, S9 Q1, Q6.
- **Built.** Built (`4086596`, `9f6eceb`): one module per plugin, the probe and activation, the bridge's allow-list and `CLAUDECODE=1`, the classic observers, `scripts/check-plugin.mjs`, `VALIDATE.txt` diffed in CI, the CLI pinned at 2.1.288, `claude plugin test` (53 per plugin). Not built: drawing of any kind. The validator's rules shaped the layout (every `$` call in `register.ts`; see [poc/P3-notes.md](poc/P3-notes.md)).

#### B2 · Observe the command hooks; the rows and the band

- **Files.** `hooks/decide.ts` (`classic.UserPromptSubmit`, `classic.PostToolBatch`,
  `classic.PostToolUseFailure`, `classic.Stop`: `await next(e)`, parse `<adage-reflex …>`
  attributes, update `$.state`; the avoid marker and `noticed` come from the feed, not from
  stdout); `hooks/draw/rows.tsx` (`ui.render` on `UserMessage` keyed by prompt order, `ToolUse`,
  `ToolGroup`, `Spinner`, `TurnDuration`, `InfoNotice` as a redraw of the engine's notice: the
  lines of ux-design §10, each a `Box` with the engine's reference then one dim `Text`);
  `hooks/draw/band.tsx` (`AbovePrompt` per ux-design §5, `next(e)` when nothing to say);
  `hooks/draw/*.test.tsx` (`ui.mount` per site over `['terminal','desktop']`, fixture state per
  test-plan §6).
- **Rules enforced in code.** `ui.render` hooks return `next(e)` while `active` is false or the
  session is not interactive; nothing drawn per event when the record is empty; theme keys from
  `tokens.ts`, never a literal; no strength or lifecycle word outside the pane; no string starts
  with `adage:` on `$.ui.status`.
- **Tests.** T-B2-01 … T-B2-11; T-X-06, T-X-07.
- **Acceptance.** `claude plugin test` green on both surfaces for every site; the `mod-observe`
  arm's hook outputs and spool records for a recorded payload stream equal the `adage` arm's
  byte for byte (E1's recorded replay, run early with the stream alone); an interactive session
  on the acme-billing sandbox shows the prompt line, the tool-row marker and the band as in
  design §1 (recording in `spikes/S9-claude-mods/results/`).
- **Estimate.** 4 days. **Depends on.** B1, S9 Q3.
- **Built.** `fe70973` ([impl/B2-B3-notes.md](impl/B2-B3-notes.md)): the rows, the markers, the band and its narrow-width priority, the toasts, from `adage watch --follow --json`.

#### B3 · The pane and `/adage …`

- **Files.** `hooks/feed.ts` (`$.process.spawn([cliBin, 'watch', '--follow', '--json', '--repo',
  cwd, '--session', id, '--since', cursor])` from `session.start`, line buffering, rows into
  `$.state`, restart from the last cursor, three failures → `feed: 'stopped'`; the reconnect
  logic as pure functions), `hooks/draw/pane.tsx` (tabs Live, Reflexes, Health per ux-design
  §6; lowercase hotkeys `l`, `r`, `t`; rows as `plain` Buttons; the inspect pane; a `Copy` per
  `review.verbs` entry via `$.ui.copy`; `Select`s for strength and lifecycle; a `Link` to the
  map), `hooks/commands.ts` (one registration, `/adage`, the verb parsed from the arguments:
  `watch|reflexes|health|why|off|on|pause` per ux-design §7; `off` runs `adage off`; `on` prints
  the terminal command until B4 turns it into an ask),
  `hooks/draw/card.tsx` (`CommandOutput` for `/adage` from `adage --json --for-session` and
  `/adage why` from the feed's last decision for this session).
- **Tests.** T-B3-01 … T-B3-10; T-X-04.
- **Acceptance.** Q3b's criteria: no gap at 2 Hz across a restart; the child is gone after
  `/clear` and exit (`ps` in the live probe); every row of the design's `adage watch` mapping
  table measured Direct, Close or Out and recorded; the recorded argv across every L7 fixture is
  within the allow-list; the pane in a control-arm fixture is byte-identical to a treated
  no-match fixture.
- **Estimate.** 5 days. **Depends on.** A3, A5, A6, B2.
- **Built.** `fe70973`: the pane with its tabs, `/adage …`, `Explore in browser`, the settings rows.

#### B4 · Asking in the engine's dialog, presses in the pane, and `Explore in browser`

- **Files.** `hooks/decide.ts` (`classic.SessionStart`: after `await next(e)`, ask `Press` with
  the payload's `session_id` and `source`, keep the token in `$.state`, and count `press_issued`
  records off the feed against the mod's own count (a mismatch switches presses off for the
  session and sets a status warning); `turn.complete`: when `graph --json` shows a reflex waiting
  for review, an `offers` entry, or `status --json` a pack awaiting trust, and the reflex has not
  been asked today, call `$.ui.ask` per ux-design §4.17, at most once per turn end, and run the
  verb for the answer with `{ press, session }` on stdin, keeping the returned next token),
  `hooks/draw/pane.tsx` (the **waiting for you** group; the verb `Button`s in an expanded row with
  lowercase hotkeys; the grant pane: terms, facts, scripts' text, the staged diff as `Code`,
  `a allow` / `n not now` / `c copy`; `Explore in browser` in every tab's footer), `hooks/bridge.ts`
  (`press(argv, { press, session })`: the only function that runs a person-only verb, callable
  only from a `$.ui.ask` continuation or a `Button`'s `onPress`; the allow-list gains `review`,
  `delegate on`, `delegate policy`, `delegate predict on`, `on`, `add … --trust`, `map`; `install`
  and `config` never), `hooks/feed.ts` (`press_issued` and `tripwire` records), `hooks/commands.ts`
  (one registration, `/adage` with `argumentHint '[watch|reflexes|health|review|why|off|on|map|
  pause]'`, the verb parsed from `e.args`; `/adage on` opens the ask; `/adage review` opens the
  pane on **waiting for you**; `/adage map` starts `adage map --no-open` on a free port once and
  opens the link), `types/index.d.ts` (`press`, `pressChain`, `asked`), `plugin.json` (no `asks`
  row: decisions are never behind a setting).
- **Rules enforced in code.** A person-only verb runs only from `press()`, which is reachable only
  from a dialog answer or a focused `Button`; never from a hook, a timer, a feed line or
  `command.run`; the token travels on stdin; no ask inside a hook with a budget or during a turn;
  one ask per reflex per day; a `dismiss` is recorded, not hidden; no strength or lifecycle word
  in a question, an answer or the pane.
- **Tests.** T-B4-01 … T-B4-09; T-X-13.
- **Acceptance.** `claude plugin test` green for the dialog continuations and the pane's states
  on both surfaces; in a live session (Q14) a nascent reflex is enabled with one answer and an E2
  grant with two acts, the facts shown between, and `adage review` history shows `via: press`;
  the model, told in the same session to enable a reflex, cannot press and its shell command
  with `CLAUDECODE` set refuses; `Explore in browser` opens the map once; T-U-01's doing part
  passes without a terminal.
- **Estimate.** 5 days. **Depends on.** A7, B3, S9 Q14.
- **Built.** `4fd8e78` ([impl/B4-D5-notes.md](impl/B4-D5-notes.md)): the ask in the engine's dialog at `turn.complete`, the press minted at session start and spent on the verb, the pane's press buttons, the grant pane with the diff.

### Workstream C · Act in the model's place (Layers 2 and 4)

#### C1 · `TurnStep` in the binary

- **Delivers.** The decision "predict this call now, or nothing", on the CPU, for a directive
  Adage already delivered.
- **Files.** `crates/adage-trace/src/lib.rs:36-56` (`Event::TurnStep`, `name() = "turn_step"`;
  `Call.by: By`, `By { Agent, Adage }` defined here), `crates/adage-runtime/src/lib.rs:75-84`
  (`Pending.command`), `:108-147` (`last_step_predicted`, `predictions`), `:221-276`
  (`Prediction`, `Decision.predicted`, `would_predict`, `routine`, `would_route`,
  `Judgement::Predicted` beside `Followed, Ignored, ObligationMet, Misfired`), `:496-514` (the
  arm per §2.2.2, in `decide`'s `match ev`), `:302-335` (`Policy.predict_grants` beside
  `exec_only`), `:1174-1182` and `:1237-1264` (fill `command`), `fast.rs:41-42`,
  `rebuild.rs:45-58`; `crates/adage-hook/src/harness/mod.rs:36-53` (`Output::Predict`),
  `harness/claude.rs:42-53` (`call_from` keeps `tool_use_id`; sets `by`), `:74-123` (`translate`,
  `render`), `codex.rs:79`, `opencode.rs:77`; `crates/adage-hook/src/lib.rs:985-1051`
  (`Extension::predict_grants(repo)` on the trait at `:173-200`, default empty; collected before
  the lock beside `exec_only` at `:1032-1038` and placed on `Policy` at `:1041-1051`; routes in
  `delivery.cpu_predictions` excluded, `:1118-1122`); the `turn_step` consumers listed in §2.2.2;
  `docs/how-it-works.md` (the prediction behaviour row beside "Accumulated prediction").
- **Contract.** stdin: `{ "session_id", "cwd", "hook_event_name": "TurnStep", "turn_id",
  "index", "model"?, "agent_id"?, "ending"?, "observe"? }`. stdout: nothing, or `{ "predict": {
  "kind": "obligation" | "run" | "sequence" | "skill" | "chain", "route", "track", "calls": [ {
  "tool": "Bash", "input": { "command": "make gen" } } ] } }` (a pending directive's kind is
  `run` or `sequence`; an armed obligation's is `obligation`, predicted only with `ending`; a
  read batch bound from a lookup's result is a `chain` of several `Read` calls; the table in
  §2.2 item 2b). Never at `index 0`; never two predictions in a row for one `turn_id`; at most
  `runtime.predict_cap` per session; never a route the interlock inhibits; nothing in the control
  arm (`would_predict` spooled); nothing without a grant (so nothing from `adage hook`).
- **Tests.** T-C1-01 … T-C1-13; T-X-01, T-X-05.
- **Acceptance.** Paired `hook_latency`: `turn_step_nothing_pending` p95 within 0.3 ms of
  `batch_no_match`; `turn_step_predicts` (run against `adage-exec` with a seeded
  `exec/approvals.json`, `policy.toml` and a pending directive, which `hook_latency` learns to
  write) p95 ≤ 10 ms; the Claude golden gains one silent `TurnStep` step and parity holds; the
  control arm emits nothing in the sim (C4).
- **Estimate.** 4.5 days. **Depends on.** A1 (shared payload handling).
- **Built.** Built (`45bc903`, `9a01b14`): the event, `By`, `Pending.command`, every kind of the §2.2 2b table but `routine` and `substitute`, every bound, `Judgement::Predicted`, `would_predict`, the `turn_step` record, the adapters and the spool consumers; T-C1-01 … T-C1-08 and the adapter half of T-C1-09 as unit tests; the golden's one silent step; `turn_step_nothing_pending` in `hook_latency` (one unpaired run). Not built: `Capabilities`, `duration_us`, `by` from ingest, the predicted step in `savings.rs`, the docs rows ([poc/P1-notes.md](poc/P1-notes.md)).

#### C2 · The `predict` grant in adage-exec and the CLI

- **Files.** `crates/adage-exec/src/approvals.rs:22, 200-236` (`mode: GrantMode`, default
  `Execute`; `approve`, `revoke_automatically`, `lapsed`, `Revocation`, `diff` all mode-aware),
  `request.rs` (`ExecRequest.mode`), `route.rs:264 plan` (a `Predict` request plans no action
  spec and no sandbox profile: `profile_id = "harness"`), `:570-589, 681-705` (`plan_grants`,
  `lapses` on the predict path; `Lapse::what` wording), `:650` (`reapprove_command` appends
  `--predict`), `hook.rs:195, 774, 931, 1230` (`mode == Execute` filters), `hook.rs` (`impl
  Extension::predict_grants`: current approvals with `mode == Predict`, current key, policy
  permits, `exec/predict-off` absent), `policy.rs:250` (`permits` for predict: `max_level >= E2`,
  effect ≤ Regenerate, `effects_deny`, `allow_environment_actions`); `crates/adage-cli/src/
  cli.rs:251-267` (`--predict`, exclusive with `--effect` and `--level`), `exec.rs:622-873` (the
  branch: no preview, `basis` from evidence, the refusals of §2.2.3), `exec.rs:134-188` (`delegate
  predict on|off`), `exec.rs:327` (`status --json` shows `mode`); `docs/execution.md` ("Predicted
  calls" beside "Approvals" and "Policy"); `docs/claude-mod/architecture.md` §3.2 if any wording
  moves.
- **Tests.** T-C2-01 … T-C2-09; T-X-11.
- **Acceptance.** `adage review delegate <track> <route> --predict` round-trips through
  `delegate status`, `revoke`, and a lapse notice; a route holding both grants keeps both through
  approve, revoke and lapse, and its E2 action and receipts still run; `adage-exec hook` parity
  with `adage hook` holds with no grants.
- **Estimate.** 3.5 days. **Depends on.** C1.
- **Built.** Built (`d41d655`): `GrantMode` on the key and every selector, the predict planning path, the policy terms, the execute-only filters in the hook, `predict_grants`, `--predict`, `delegate predict on|off`, `status --json`, `revoke --predict`, `adage-exec version`, the "Predicted calls" section of `docs/execution.md`; T-C2-01 … T-C2-09. Departures: the checkout is taken from the working directory (the trait does not pass the root); a predict grant is withheld while the route holds any execute grant; the TUI routes view does not list predict grants ([poc/P2-notes.md](poc/P2-notes.md)).

#### C3 · Receipt as the result, rewrites checked by the engine

- **Files.** `crates/adage-hook/src/lib.rs:190-194` (`pretool_context` returns `PreNote`),
  `:549-723` (the `dry_run` plan and the `verdict` decision; `receipt` rendered only for those
  payloads; `keeps_permissions` call at `:668`), `adage-exec/src/hook.rs:751-830`
  (`covering_receipt` fills `fresh`, `result`,
  `covering`; `answered: 'receipt'` marks the obligation met and the receipt used); the mod's
  `hooks/decide.ts` (`tool.call { tool: 'Bash' }` per mod architecture §3.3 and §3.4: dry run →
  `$.tool.check` on the original → receipt only on `allow` → final ask → result; or two checks →
  `verdict.engine` → final ask → `next({ ...e, command })` with `$.ui.notice`), `hooks/draw/
  rows.tsx` (`ToolResult` receipt card only when answered; the dim line beside a real run).
- **Tests.** T-C3-01 … T-C3-07.
- **Acceptance.** In the `zero_config_plugin.rs` Python fixture (the U11 environment, an E2
  `verify` grant seeded, a passing run journaled), the mod's dry run reports `fresh` and the
  final call spools one `pretool` record with `answered: receipt`; the command hook's output for
  the same payload without `dry_run` is unchanged (golden); `$.tool.check` agreement with
  `pretool::keeps_permissions` recorded on every settings fixture (Q4).
- **Estimate.** 3 days. **Depends on.** A1, B1, C1 (for `by: Adage`), S9 Q2 (Layer 2), Q4.
- **Built.** `89d57ff` (binary, [impl/C3-C7-C8-D2-C9-E-notes.md](impl/C3-C7-C8-D2-C9-E-notes.md)) and `af02e95` (the mod, [impl/C3-C9-mod-notes.md](impl/C3-C9-mod-notes.md)): the receipt as the result with its card, the beside line, rewrites checked by the engine, the advice on `differs`.

#### C4 · The mod's `turn.step` and the simulator

- **Files.** `hooks/decide.ts` (`turn.step` generator per mod architecture §3.2: main loop only,
  `index > 0`, ask `TurnStep` with a 250 ms timeout, yield `tool`/`input`/`stop`, return the
  result; a `Skill` prediction yields the `Skill` tool with `{ skill, args? }` and checks
  `$.command.list()` lists the skill first; `routine` → `next({ ...e, model, effort })` only when
  `routine_model` is set); `hooks/draw/rows.tsx` (the "chose this step" marker keyed by the
  `adage_` id); `crates/adage-sim/src/claude.rs` (`turn_step_hook`, `pretool_hook`,
  `standing_hook`), `scenario.rs:417 Replay` (`predict` and `pretool` expectations per turn),
  `crates/adage-cli/tests/harness_sim.rs:193 replay_claude` (a prediction at the schema-edit turn
  under `adage-exec` with the grant, nothing in control, nothing without the grant, nothing from
  `adage`; `check_graph` counts `predicted`); `cargo run -p adage-sim` regenerates fixtures.
- **Tests.** T-C4-01 … T-C4-06.
- **Acceptance.** S9 Q9's criteria in a live session (10 of 10 runs, the permission dialog met
  for a predicted call, the `adage_` id preserved in the transcript on disk); the sim passes in
  both arms; `claude plugin test` covers the generator with a stubbed `ask`; with the binary
  missing the step is forwarded unchanged.
- **Estimate.** 4 days. **Depends on.** C1, C2, B1, S9 Q2 (Layer 2), Q9.
- **Built.** Built (`4086596`, `f7b399e`): the `turn.step` generator (main loop only, `index > 0`, every kind, the `adage_` ids), the finish line taken after an `end_turn` stop, the sim's `TurnStep` and two-phase `PreToolUse` payloads, `Replay`'s `predict` and `pretool` expectations, `replay_claude` in both arms with the grant issued through `--predict`, `check_graph` counting predicted steps. Not run: a live session (no key), so S9 Q9's criteria and T-C4-06 stand open ([poc/P4b-notes.md](poc/P4b-notes.md)).

#### C5 · Standing text, compaction, routing

- **Files.** `crates/adage-hook/src/lib.rs` (the `Standing` event per §2.2.8;
  `SessionState.has_standing`), `harness/claude.rs` (`translate`, `render` for `{ standing,
  environment }`); the mod's `hooks/decide.ts` (`prompt.compose` appends `adage:procedures` from
  `$.state.standing`; `tool.describe { tool: 'Bash' }` appends `environment`; `session.compact`
  adds the instruction and filters rows only when Q5 found a structural handle);
  `crates/adage-runtime/src/lib.rs` (`routine` only when the payload says `routine_model` is set;
  `would_route` in control).
- **Tests.** T-C5-01 … T-C5-05.
- **Acceptance.** The `adage:procedures` section is byte-identical across every request of a
  session (`$.prompt.compose()` read in a Q2 probe); the per-event delivery of standing text is
  not emitted once `has_standing` is set; a control session answers empty `standing` and never
  `routine`; Q11 reported.
- **Estimate.** 3 days. **Depends on.** B2, C1.
- **Built.** `09f2d1d` (binary: the `Standing` event, `routine`) and `af02e95` (the mod: the cached section, compaction, routing to `runtime.routine_model`).

#### C6 · The dead end that never runs

- **Delivers.** A learned `avoid` pair substituted before the dead end runs (mod architecture §3.4).
- **Files.** `crates/adage-hook/src/lib.rs` (`pretool()`'s `dry_run` plan gains `substitute` when
  the call's canonical key meets an `avoid` route at `fire` whose `instead` is a fixed command;
  the `verdict.engine` phase records `Judgement::Substituted` or `would_substitute`), the runtime's
  judgement enum and its consumers (feed, graph, savings, evidence: a substitution is not a
  follow), the mod's `hooks/decide.ts` (the `substitute` branch beside the rewrite branch: two
  `$.tool.check`s, same verdict, `next({ ...e, command })` with the context line), `docs/execution.md`
  ("Rewrites" gains the learned pair).
- **Contract.** Only a `fire` `avoid` route with a slot-free `instead`; only on the same permission
  verdict; one line of context; never in control; counted in `dead_end_hits` as avoided.
- **Tests.** T-C6-01 … T-C6-05.
- **Acceptance.** In the sim, `python3 -m pytest tests` at the dead-end turn becomes `make test`
  with the context line, the failure never occurs, and `check_graph` counts a substitution and
  no follow; the control arm runs the dead end as today and spools `would_substitute`; the demo's
  `dead_end_hits` falls in the `mod` arm.
- **Estimate.** 2 days. **Depends on.** C3.
- **Built.** Built (`9f6eceb`): the `dry_run` plan with `substitute`, the `verdict` phase with the route named, `Judgement::Substituted` and `would_substitute`, the mod's branch with two `$.tool.check`s and the context line, `adage delegate rewrites off` as the switch, the sim's dead-end turn becoming `make test`; T-C6-01 … T-C6-05 offline. Open: whether the engine shows the model a `context` added to a kept result; the mod's order against the `PreToolUse` command hook ([poc/P5-notes.md](poc/P5-notes.md)).

#### C7 · Chains and multi-call steps

- **Delivers.** A lookup-then-read chain and a `ReadBatch` predicted as one response of several
  calls (mod architecture §3.2).
- **Files.** `crates/adage-runtime/src/lib.rs` (`Pending` binds a `ReadBatch`'s files from the
  preceding call's result when the directive names them; `Prediction.calls: Vec<{tool, input}>`;
  the "never two in a row per route" bound counts yielded responses), `harness/claude.rs`
  (`render` emits `calls`), the mod's `turn.step` (one `tool`/`input` pair per call, indexes
  0..n), `crates/adage-sim` (the held-out replay expects the lookup prediction and the reads).
- **Tests.** T-C7-01 … T-C7-04.
- **Acceptance.** The sim's held-out session predicts the lookup, then the reads of its
  `linked_files`, with a model step between; `requests` on lookup items falls by two in the `mod`
  arm of the demo.
- **Estimate.** 3 days. **Depends on.** C4.
- **Built.** The chain from a lookup's bound read batch is built with C1 (several `Read` calls in one response); a `ReadBatch` as one response of several calls beyond that binding is not.

#### C8 · The finish line at the last step

- **Delivers.** An armed, unmet, granted obligation run as the agent's own call at the turn's
  end, instead of the Stop gate's block and extra request (mod architecture §3.2, Q15).
- **Files.** the mod's `turn.step` (on an `end_turn` stop with the binary answering
  `obligation`, keep the text chunks, yield the obligation's `tool`/`input` and a `tool_use`
  stop), `crates/adage-runtime/src/lib.rs` (`TurnStep` with `ending: true` answers an obligation
  under the grant; the obligation is marked met when its call is observed; the Stop gate stays as
  the fallback), `crates/adage-sim`.
- **Tests.** T-C8-01 … T-C8-04.
- **Acceptance.** Q15 passes (10 of 10: the text is kept, the call runs, the loop continues, the
  obligation is met, no `block`); the sim's finish-line turn shows no Stop request in the `mod`
  arm; `requests` per finish line falls by one.
- **Estimate.** 3 days. **Depends on.** C4, S9 Q15.
- **Built.** `ending` is built with C1 and the mod: the finish line is taken after the model's `end_turn` stop (the mod holds the stop chunk, adds the calls and turns it into `tool_use`), so it saves the Stop gate's request, not the request that ended the turn; whether the engine records the rewritten stop is unverified.

#### C9 · Attachment: the tool description, the subagent prompt, and the mod capabilities

- **Delivers.** Conventions attached to the Bash tool's description and to a subagent's prompt
  (mod architecture §1b.3, §3.6, §3.8); the mod's capabilities as fields on the canonical harness.
- **Files.** `crates/adage-trace/src/harness.rs` (`Capabilities` gains `predict_step`,
  `answer_call`, `substitute_call`, `tool_description`, `cached_section`, `subagent_prompt`,
  `compaction`; `Harness::ClaudeCode` keeps today's four; the live adapter reports the mod's set
  when the payload carries `via: mod`), `crates/adage-hook/src/harness/claude.rs` (`translate`
  reads `via`), the runtime (behaviour by capability: a `Standing` answer for a delegation's
  description; the environment note for `tool.describe`), the mod's `tool.describe { tool: 'Bash' }`
  and `agent.spawn` hooks, `crates/adage-trace/src/lib.rs` (`Call.agent: Option<String>`,
  `#[serde(default)]`), `docs/harnesses.md` (the capability table gains the mod's row).
- **Tests.** T-C9-01 … T-C9-05.
- **Acceptance.** `cargo test -p adage-trace` green with the new fields; a subagent spawned in the
  sim receives the lookup convention in its prompt and makes no lookup of its own; the Bash
  description in a Q2 probe carries the test-runner line once per session; directives per session
  fall in the `mod` arm with hidden tests unchanged.
- **Estimate.** 3 days. **Depends on.** C5.
- **Built.** `89d57ff` (binary: the `Capabilities` fields and the attachment payloads) and `af02e95` (the mod: the Bash tool's description, the subagent prompt, the flags).

### Workstream D · Noticed and skills (binary first, mod to draw)

#### D1 · Candidates in the learner and the index

- **Files.** `crates/adage-graph/src/mine/mod.rs` (`mine_candidates`: the second `Db` at floor
  `s_min − border` from `:257-267`, `seq::mine` with the border recorded, the `[s_min − border,
  s_min)` and `Context ≥ 2` filters), `select.rs:535` (score border patterns, carry as
  candidates, never select; `learn_split` at `:603`), `compile/shapes.rs:1094`
  (`Compiled.candidate`); `crates/adage-tracks/src/model.rs` (`Candidate`; `RouteState::Nascent`
  in `initial_state` at `:601-603`), `io.rs` (`candidates.json`), `index.rs:19-33, 216-223`
  (`candidates`, `candidate_contexts`, `format`; `compile_root` reads the file), `triggers.rs:102`,
  `context.rs:51` (`CtxNet::of_candidates`); `crates/adage-learn/src/engine.rs:746` (branch on
  `Compiled.candidate` before `judge`), `store.rs:24` (`Store::write_candidates`, implemented by
  `Files` and by the in-memory store of `tests/remote_sessions.rs`; nothing writes the file
  directly), `lib.rs:200 learn_sessions_into` (calls it; nascent `Context` routes → `review:
  true` candidates), `diff.rs:27-50` (`ChangeKind::Candidate`, the tipping session ids on `New`);
  `crates/adage-app/src/reflex.rs` and `crates/adage-cli/src/main.rs:624` (`status --json` lists
  `candidates` through the one `Reflex` shape); `crates/adage-sim/src/scenario.rs` (the 2-of-12 `make test` →
  `make lint` sequence; fixtures regenerated); `docs/live-learning.md`, `docs/how-it-works.md`
  ("From history to a reflex": a candidate is counted, never delivered),
  `docs/tracks/track-g-graph-engine.md` §9 (the candidate pass).
- **Contract.** `candidates.json`: `{ "schema": "adage-candidates/1", "repo", "s_min",
  "candidates": [{ "id", "trigger", "step", "support", "days", "last_seen", "review" }] }`, ids
  the stable route ids (`compile/ids.rs`). The routes' mining input, result and `truncated` flag
  are unchanged by the pass.
- **Tests.** T-D1-01 … T-D1-10.
- **Acceptance.** `adage condition --demo` on the sim's twelve sessions writes the `make test →
  make lint` candidate (support 2, `s_min` 3) and `status --json` lists it; the routes compiled
  from the sim and from every dataset are unchanged (`adage-learn/tests/golden.rs` and the
  route-fidelity gates); `truncated` is false on every dataset; the fast path's skip rate over the
  sim and the dataset replays is reported before and after; paired `hook_latency`
  `batch_no_match` p95 within +0.3 ms with candidates compiled; `learn_bench` (300 × 2) within
  1.3× of before, measured and pasted; `condition --auto` with one changed transcript under
  150 ms; `cargo test -p adage-graph --test border` still passes; every recorded index loads.
- **Estimate.** 6 days. **Depends on.** Nothing.
- **Built.** `ecd4e03` ([impl/D1-notes.md](impl/D1-notes.md)): candidates through the learner's `Store` port, the index shape, the CLI surface, `candidates_bench`.

#### D2 · `noticed` at hook time

- **Files.** `crates/adage-runtime/src/lib.rs:108-147` (`SessionState.cand_ctx`, `noticed`
  bounded at 64), `:496-505` (a second `Symbols` over `ctx.index.candidate_contexts(&repo)`),
  `crates/adage-hook/src/lib.rs:997-1007` (the no-routes path considers candidates),
  `runtime/src/lib.rs:893-925, 960-970, 609-615` (push the same symbols into `cand_ctx`), after
  `:1116-1145 fire_contexts` (the candidate walk into `d.noticed`, once per session),
  `:258-276 Decision.noticed`, `Decision.cand_ctx`; `rebuild.rs:88-89, 163-165` (replays both);
  `crates/adage-cli/src/ui/feed.rs:16 Act::Noticed { candidate, support, s_min, review }`,
  `crates/adage-app/src/graph.rs:387 event()` includes it; `adage-app/src/savings.rs:345-349`
  ignores it; the `fast_path_changes_no_decision` gate (`adage-eval/tests/gates.rs:321`) covers
  it.
- **Contract.** In the spool record's `decision`: `"noticed": [{ "candidate", "support", "s_min",
  "review" }]`; nothing in `additionalContext`; both arms; once per candidate per session.
- **Tests.** T-D2-01 … T-D2-07.
- **Acceptance.** Replay gate green over every dataset with candidates compiled; the sim's
  held-out session notices the `make lint` candidate in both arms and `adage watch --once` shows
  the row; a live route fires its directive and no `noticed`; p95 as D1.
- **Estimate.** 2 days. **Depends on.** D1.
- **Built.** `89d57ff`: `noticed` at hook time within its bounds, silent in control, the fast path untouched without candidates.

#### D3 · The `skill` response kind

- **Files.** `crates/adage-tracks/src/model.rs:188-209, 558-596` (`Step::Skill`, shape, `SHAPES`
  to eight, ceiling prime), `io.rs:242-371, 406-414` (validation; `Skill` allowed under `Prompt`,
  `Intent`, `AfterEdit`, `Context`; `Inhibit` refused), `index.rs:287` (`format`);
  `crates/adage-runtime/src/render.rs:901-1023` (`route_text`: the template),
  `crates/adage-trace/src/wire.rs:49-104` (`named` matches the template first; `Named.skills`;
  the writer and the reader are in different crates, kept in step by the runtime's round-trip
  tests), `runtime/src/lib.rs:1133-1140` (`fire_contexts` admits a `Skill` step), and the
  follow judge on `control Skill {skill}`; `crates/adage-graph/src/canonical.rs:70`,
  `input.rs:26-33`, `events.rs:27-34`, `mine/mod.rs:467-475`, `compile/shapes.rs:1094, 224`,
  `compile/ids.rs:50`; `crates/adage-learn/src/engine.rs` (judge `skill` like `run`),
  `crates/adage-trace/src/session.rs:101-113` (`Directed.skills`); the match sites of §2.3.1;
  `adage-eval` fixtures with a Skill tool use; `docs/training.md`, `docs/how-it-works.md` (a
  fifth behaviour row), `docs/live-learning.md` (the minimum-version rule).
- **Packs, deferred.** `adage-pack`'s `Shape`, `TriggerV2` and `StepV2` have no skill (and no
  context trigger), every struct is `deny_unknown_fields`, and `convert.rs:198, 255-306` refuses
  what it does not know; `convert.rs:303` gains `Skill => Err(..)`. A `skill` route is learned
  and delivered locally in this chunk; sharing it in a pack is a format revision (`adage-pack/2`)
  scheduled after D4 and not needed for the mod.
- **Tests.** T-D3-01 … T-D3-08.
- **Acceptance.** `cargo test -p adage-tracks -p adage-graph -p adage-learn -p adage-runtime
  -p adage-eval -p adage-cli` green; `cargo clippy --workspace --all-targets -- -D warnings`
  clean; the `claude-code` golden gains a prompt step delivering a `skill` directive
  (`kind="skill"`) and is re-blessed; the eval gates unchanged (the
  `replay_with_the_learned_index_gives_every_recorded_directive` whitelist lists the new id).
- **Estimate.** 5 days. **Depends on.** Nothing.
- **Built.** `ecd4e03` ([impl/D3-D4-notes.md](impl/D3-D4-notes.md)): `do: skill` on a route, the directive, the judgement, the learned skill route, the `skill` prediction from a real route under `skills.invoke`.

#### D4 · `verified.json` and `SkillPrompt`

- **Files.** `crates/adage-train/src/skill.rs:297 stage` (write `verified.json` with `inert`),
  `crates/adage-train/src/compile.rs:38-62` (expose `Verified` per step with the step id),
  `crates/adage-pack/src/verified.rs` (new: the file's type and reader, beside `procedure.rs`, at
  layer 4 so the hook can read it), `crates/adage-cli/src/train.rs:85` (`accept` marks
  `accepted_at`); `crates/adage-hook/src/lib.rs:358` (a `SkillPrompt` branch like `PreToolUse`:
  gated by a capability, not a name; accepted packs only; arm assignment; `render::sanitise`;
  spool `kind: "skill_prompt"`),
  `harness/claude.rs` (`translate` and `render` for `{ "text": … }`); `docs/training.md` (a
  "Verified in place" subsection). The mod's `skill.prompt` hook (`hooks/decide.ts`: ask
  `SkillPrompt` → `{ text }` or `next(e)`) is a one-day addendum that lands with D5.
- **Contract.** stdin `{ "session_id", "cwd", "hook_event_name": "SkillPrompt", "skill", "text"
  }`; stdout `{ "text": "<original>\n\n<fixed heading>\n<one line per step>" }` or nothing. The
  heading is a fixed template: "Adage verified these steps in this repository on <date>:".
  Lines name the step's command and `works`/`stale (<signature>)`, sanitised. Plain and factual;
  at most 2,400 bytes added. Nothing for an unaccepted pack.
- **Tests.** T-D4-01 … T-D4-05.
- **Acceptance.** `adage train evals/lab/skills/skills/acme-work-item/SKILL.md --repo <sandbox>`
  writes `verified.json` with `inert`; before `train accept`, `SkillPrompt` prints nothing; after
  it, `echo '{…"skill":"acme-work-item"…}' | adage hook claude SkillPrompt` prints the annotated
  text; S9 Q13's three entry points verified in a live session once D5 lands.
- **Estimate.** 3 days. **Depends on.** D3 (for `Step::Skill`, optional).
- **Built.** `ecd4e03`: `verified.json` in adage-pack written by `adage train`, `adage hook claude SkillPrompt`.

#### D5 · Drawing noticed, learning and skills

- **Files.** `hooks/draw/rows.tsx` (the `ToolGroup`/last-`ToolUse` noticed line; the `Skill`
  tool-row markers), `hooks/draw/band.tsx` (the learner countdown from the feed's `learner`
  records; `learning…` while running), `hooks/decide.ts` (toasts from `learn.log` lines per
  ux-design §4.6, the header rule on `sessions`; the `skill.prompt` hook), `feed.ts` (passes
  `learn` and `learner` records).
- **Tests.** T-D5-01 … T-D5-03; T-U-01 (the noticed questions).
- **Acceptance.** S9 Q12's read of the recording; `claude plugin test` for each row; no line for
  a single call (unit over fixture records); Q13's three entry points.
- **Estimate.** 2.5 days. **Depends on.** D2, D4, B3.
- **Built.** `4fd8e78`: the noticed line under the tool group with its ask, the learning toast's detail, the skill rows and `skill.prompt`.

### Workstream E · Evaluation

#### E1 · Arms and the requests metric in the demo runner

- **Files.** `demo/run.py:36, 105-113, 140-149` (arms `a0`, `mod-observe`, `mod` per §2.7.2;
  `--debug` and the debug log path; `metrics.requests`, `metrics.requests_verified` per
  `heldout_analysis.requests()`; the `hidden_passed` key recorded), a recorded-payload replay
  (`demo/replay_hooks.py`: feed a recorded stream of hook payloads to `adage hook` with and
  without the mod's observe mode and diff the outputs and spool), `demo/report.py:128-142
  METRICS` (`requests`), `demo/SANDBOX.md` (the arms and keys).
- **Tests.** T-E1-01 … T-E1-04.
- **Acceptance.** `python3 demo/run.py one W-2041 mod` records `requests` equal to the distinct
  explicit request-id count; the `mod-observe` replay diff is empty; `compare(runs, a="mod",
  b="a0")` reports the task-stratified permutation p and the bootstrap interval for `requests`.
- **Estimate.** 2.5 days. **Depends on.** B1.
- **Built.** Built (`13b2495`): the five arms, `requests` and `requests_verified`, `replay_hooks.py --identity`, the report's new metrics, the `demo` Python suite (35 tests); T-E1-01 … T-E1-04 offline. The arms were dry-run against fixtures, never against a model ([poc/P4a-notes.md](poc/P4a-notes.md)).

#### E2 · Development gate for Layer 4

- **Files.** `evals/reflex-proof/harness/dev_arms.py:43-50`, `dev_v9.py:57-74, 515-527` (a `MOD`
  arm: hooks as `A4`, `--plugin-dir`, the `predict` grant issued as the human step; its own five
  realized controls; the verdict loop reads the frozen manifest's `dev_thresholds_v9`), a new
  protocol revision in `docs/launch-gate-protocol.md` registered before dispatch, the findings
  file under `harness/results/<date>-mod-dev/`.
- **Acceptance.** The registered v9 rule for `MOD` against `A0`: the study valid; D1 mean requests
  at most 0.80 × A0 and fewer than A0 in at least 10 of 16 blocks; D2 no more hidden-test
  failures than A0; D3 all five controls realized with no skipped necessary verification; D4
  covered repeat attempts in at most 5 of 16 positives. Reported as development, "not a launch
  statistic". Pass twice on fresh seeds before E3.
- **Tests.** T-E2-01 … T-E2-03.
- **Estimate.** 3 days plus model spend (`SESSION_CAP_USD` 0.50, `STUDY_CEILING_USD` 8.50 per
  run). **Depends on.** C4, E1.

- **Built.** An offline form only (`f7b399e`): `crates/adage-eval/tests/predict_gate.rs` replays the recorded datasets with a `TurnStep` at every boundary in both arms and pins requests saved, zero in control, and the measured next-action accuracy as floors (the numbers and their caveat: [poc/README.md](poc/README.md)). The development gate itself needs model runs.
- **Built.** `89d57ff`: `dev_v9.py`'s `MOD` arm with its controls and the human step in a dry run; the runs themselves need a key (evaluation-plan.md §6).

#### E3 · Registered confirmation

- **Scope.** Only after E2 passes twice: a pre-registered held-out run per
  `docs/launch-gate-protocol.md` on new evaluation groups (the current four are spent,
  `LAUNCH-STATUS.md`), arms A0, NOEXEC (hooks, no `predict` grant) and BEST = MOD in paired
  blocks, every applicable G criterion (G1, G2, G5–G8), the censoring rule, the sensitivity
  analyses, `QUALITY_MARGIN = 1` and the one-sided 95% bootstrap upper bound of
  `sum(MOD)/sum(A0)` ≤ 0.90, at most 2 excluded blocks; `heldout_cohort.py` and
  `heldout_analysis.py` learn the `MOD` arm name. Outcome recorded in
  `evals/reflex-proof/LAUNCH-STATUS.md`.
- **Tests.** T-E3-01, T-E3-02.
- **Acceptance.** The protocol's `pass`; otherwise the finding is recorded and Layer 4 stays
  behind its grant as experimental.
- **Depends on.** E2.
- **Built.** `89d57ff`: the held-out scripts accept the arm name; `LAUNCH-STATUS.md` says no run has happened.

## 4. Sequencing

```text
A1 ──▶ C1 ──▶ C2 ──▶ C4 ──▶ E2 ──▶ E3
 │      └───▶ C3          ▲
 └───────────▶ C3         │
A6 ──▶ B1 ──▶ B2 ──▶ B3 ──▶ D5
 │      ├────▶ C3, C4, E1 ─┘
 │      └────▶ C5 (with C1, B2)
A3, A5 ──────▶ B3
S9 (Q1, Q2, Q3, Q6) ──▶ B1;  Q2 (Layer 2), Q4 ──▶ C3;  Q9 ──▶ C4;  Q12 ──▶ D5;  Q13 ──▶ D4/D5
D1 ──▶ D2 ──────────────▶ D5
D3 ──▶ D4 ──────────────▶ D5
A2, A4 (independent)
A7 ──▶ A8;  A7, B3 ──▶ B4;  Q14 ──▶ B4
C3 ──▶ C6;  C4 ──▶ C7, C8 (Q15);  C5 ──▶ C9
```

Estimates: A 15 days, B 16.5, C 29, D 18.5, E 5.5 plus spend: about 85 engineer-days, seventeen
engineer-weeks of code before the registered run. The critical path is A1 → C1 → C2 → C4 → E2
(16 days of work, gated by Q9); B and D run beside it, and A7 → B4 (9 days, gated by Q14) is the
path to the no-terminal product.

## 5. Testing strategy

The levels, environments and every case are in [test-plan.md](test-plan.md); this is the map.

| Level (test plan §2) | What | Where | Runs |
| --- | --- | --- | --- |
| L1 Unit, Rust | Every new type, bound and template: `Candidate`, `Step::Skill`, `Event::TurnStep`, `GrantMode`, the dedup ring, `PreNote.fresh`, `verified.json`, `By`, `Pending.command`, `Capabilities` | each crate's `#[cfg(test)]`, `adage-runtime/src/tests.rs` | `cargo xtask affected`, PR CI |
| L2 Integration | The binary end to end in a `World` | `crates/adage-cli/tests/*`, `crates/adage-exec/tests/*` | PR CI |
| L3 Golden and parity | The exact bytes `adage hook claude <event>` prints for the scenario, now with a silent `PreToolUse`, a silent `TurnStep` and a `skill` directive; `adage-exec hook` identical with no grants | `crates/adage-cli/tests/golden/claude-code.txt`, `hooks.rs::claude_scenario`, `adage-exec/tests/parity.rs` | PR CI; re-blessed with `ADAGE_BLESS=1`, diff reviewed |
| L4 Simulator | Twelve seeded Claude sessions, held-out replay in both arms with `PreToolUse`, `TurnStep`, `Standing`, `SkillPrompt`; the sub-support sequence; control silent | `crates/adage-sim`, `crates/adage-cli/tests/harness_sim.rs` | PR CI; `cargo run -p adage-sim` when fixtures change |
| L5 Latency | Paired `hook_latency <before>,<after> - 500` scenarios: `pretool_no_match`, `turn_step_nothing_pending`, `turn_step_predicts` (adage-exec, seeded grant), `batch_no_match` with candidates; `learn_bench` 300 × 2 | `crates/adage-cli/examples/hook_latency.rs`, `learn_bench.rs` | A developer's release build before merging A1, A2, A6, C1, D1, D2, numbers in the PR (no workflow runs them today; a scheduled job is a follow-up) |
| L6 Replay gates | `fast_path_changes_no_decision` with `TurnStep` injected and `noticed`; every recorded index loads; the route-fidelity gates unchanged | `crates/adage-eval/tests/gates.rs` | `evals.yml` weekly, and `cargo test -p adage-eval --test gates` before merging C1, D1, D2, D3 |
| L7 Mod, unit | Every hook with stubbed `ask`, `$.tool.check`, `$.process`; every render site mounted on `terminal` and `desktop`; `onPress` handlers called directly; the feed's reconnect as pure functions; the argv recorder | `plugin/claude-code/hooks/*.test.ts(x)` | `claude plugin test` in the `plugin` CI job, pinned CLI |
| L8 Mod, static | `validate --strict` diffed against `VALIDATE.txt`; `check-plugin.mjs` rules for modules, types, the allow-list, the pin | `scripts/check-plugin.mjs`, CI `plugin` job | PR CI |
| L9 Mod, live | S9's probes | `spikes/S9-claude-mods/` (`FINDINGS.md`, `rerun.sh`, `results/`) | Once per question, results committed |
| L10 End to end, headless | The demo runner's arms on the six held-out items: `requests`, `cost_usd`, `passed`, `hidden_passed`, `stale_generated`; the recorded-payload replay for `mod-observe` | `demo/run.py`, `demo/replay_hooks.py`, `demo/report.py` | Before merging B2, C3, C4; numbers in the PR |
| L11 Development gate | The v9 rule for `MOD` against `A0` | `evals/reflex-proof/harness/dev_v9.py` | Before E3, twice |
| L12 Registered | The launch gate protocol on new groups | `evals/reflex-proof/` | Once |
| L13 Usability | ux-design §14 | the recordings and a live sandbox session | Once per surface change (B2, B3, D5) |

Three conventions hold throughout: every number in a findings file comes from a script in the
same folder (the spikes' rule); a changed golden is a behaviour change and is called out in the
PR; and a failing test is a defect or a test to make robust, never re-run to green (this plan's
own rule; `CONTRIBUTING.md` says only that hook paths fail open).

## 6. Acceptance criteria, rolled up

| Milestone | Chunks | Done when |
| --- | --- | --- |
| **M0 · Spike** | S9 | Findings for Q1–Q15 in `FINDINGS.md` with `rerun.sh`; Q1, Q2 (Layer 1), Q3, Q6 pass before B starts; Q2 (Layer 2), Q4 before C3; Q9 before C4; Q14 before B4 |
| **M1 · Foundation** | A1–A6 | One decision per `tool_use_id`; `duration_us` on every record and the doctor check; `watch --follow --json --session` streams and resumes from a cursor, silent about the arm; `ledger note` into `harness.jsonl` with integrity; `tokens.ts` generated and checked; `adage off`/`on` and `adage-exec version`; all goldens unchanged except the added silent `PreToolUse` step |
| **M2 · See it** | B1–B3 | `1 mod active · adage`; exactly one copy active with both plugins; the `mod-observe` replay diff empty; every site drawn on both surfaces in `plugin test`; the pane fed by the binary with no gap across a restart; the recorded argv within the allow-list; the control-arm pane identical to a treated no-match pane; T-U-01 reading and doing parts |
| **M3 · Trust it** | C1–C3 | `TurnStep` predicts only a delivered directive with a bound command and a `predict` grant, never at index 0, never twice in a row, never an inhibited route, never a route adage-exec's own CPU prediction handled, never in control, nothing from `adage`; a route with both grant modes keeps both; a fresh receipt answers the repeated pytest run in the Python fixture with one record |
| **M4 · Skip it** | C4–C9 | S9 Q9 10 of 10 with the dialog met and the id on disk; the sim predicts at the schema-edit turn under adage-exec and nothing in control; the dead end is substituted and the chain predicted in the sim; Q15 10 of 10 and no Stop request on the finish-line turn; a subagent told at spawn makes no lookup; `adage:procedures` byte-identical across a session; the demo's `mod` arm runs end to end with `requests`, `dead_end_hits` and directives per session all lower than the `adage` arm |
| **M5 · Notice** | D1, D2, D5 (noticed) | The `make test → make lint` candidate from the sim's history in the index; the routes unchanged; `noticed` once per session in both arms; replay gate green; the line drawn under the batch; p95 within +0.3 ms |
| **M6 · Skills** | D3, D4, D5 (skills) | A `skill` route learned from six synthetic sessions and delivered at prime; the Skill tool predicted at `turn.step` under a `predict` grant for an accepted, inert skill; `verified.json` written by `adage train` and read back by `SkillPrompt` only after `accept`; Q13's three entry points |
| **M7 · Evidence** | E1–E3 | `requests` in the demo report with an `a0` arm; the v9 rule passes twice for `MOD`; the registered run's outcome recorded, whatever it is |
| **M8 · Ask it** | A7, A8, B4 | An answer in the engine's dialog enables a nascent reflex, and two acts grant E2 with the terms, scripts and diff shown between, recorded `via: press`; with `AGENT_ENV` set and no CI override no verb passes without a valid token and no token is issued after the agent's first action; one plugin installs everything and execution needs no second install; `Explore in browser` opens the map; the usability tasks pass without a terminal |

## 7. Risks, and what the plan does about them

- **The API moves.** Mods are early access; the public declaration file is in the Claude Code
  repository. CI pins the CLI version for `validate` and `test`; S9 Q8 measures churn; the
  command hooks remain the floor so a broken mod degrades to today.
- **A fabricated assistant turn hurts quality.** Layer 4 ships only behind a `predict` grant,
  with the three bounds, and only after E2's v9 rule and E3's quality rule; the control arm
  records `would_predict` so the saving is measured against a baseline from day one.
- **A predicted call runs unsandboxed.** The grant is narrower than delegation (effect, policy,
  exec-only and double-grant refusals), the call meets Claude Code's permissions (Q9 proves the
  dialog), and the notice and marker name it.
- **The learner relearns Adage's own calls.** The `adage_` id prefix and `Call.by` mark them
  (C1), the importer excludes them (§2.1.3), and S9 Q5 may add the transcript's `origin`.
- **Candidates cost the fast path, and the learner.** Their words join `Triggers`; D1 reports
  the skip rate before and after and bounds p95 at +0.3 ms. The candidate pass is separate, so
  the routes' mining is unchanged by construction; D1 measures learn time with `learn_bench`
  before committing to the 1.3× bound, since `condition --auto` already runs at 44–138 ms against
  a 150 ms target.
- **Old indexes and recorded datasets.** Every new `Index` field is `serde(default)`; a new
  `format` field and a minimum-version rule cover binaries older than D3; the 26 recorded indexes
  must keep parsing (a test loads them all).
- **Noise.** `noticed` is sequences only, once per candidate per session, off with one
  `userConfig` row; Q12 and T-U-01 read the recording with people.
- **Person-only commands from a pane.** They run only through `press()`, reached only from a
  dialog answer or a focused `Button`, with the session's single-use token on stdin (A7, B4);
  the allow-list, its unit test and `check-plugin.mjs` are the structural guard; `CLAUDECODE=1`
  on every child makes `AGENT_ENV` refuse any call without a valid press; `VALIDATE.txt` is review
  evidence, not proof. The token is issued only in the window before the agent's first action,
  which the binary closes on the first invocation rather than the first record, and lives only
  in engine memory; a `press_issued` record the mod did not cause is a tamper signal. What the
  press is: a provenance record (`via: press`) and a guard against steering, of F11's class. What
  it is not: a boundary against an agent that runs as the same user, which can unset
  `CLAUDECODE`, write the state file or forge a hook payload today, as Track C D8 already says of
  the approval store. The docs never claim more, and Q14 measures the two facts the design rests
  on (the window and the mod channel's privacy), not a security property.
- **Reversing D2.** One plugin with both binaries removes the second install as consent; the
  press is the consent instead. The `Command::new` promise stays checkable because the binaries
  stay separate; hooks cannot double because there is one plugin.
- **Two plugins, two mods.** The activation probe leaves exactly one copy active; `check-plugin.mjs`
  diffs the two module trees and fails on drift; T-X-11 installs both.
- **The evidence can be forged from a session.** `ledger note` accepts only a session the spool
  knows and keeps harness lines apart from transcript-priced ones.
