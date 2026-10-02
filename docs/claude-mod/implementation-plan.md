# Adage in Claude Code: implementation plan

> Status: **proposed 2026-10-02**. The plan for [architecture.md](architecture.md) (the
> design), [ux-design.md](ux-design.md) (what the person sees) and
> [spikes/S9](../../spikes/S9-claude-mods/SPIKE.md) (what must be measured first); the tests it
> names are specified in [test-plan.md](test-plan.md).
> Written against the code at commit `0eb40ae` on `main`; every path and line below was read in
> that tree. It is meant to be worked chunk by chunk by an agent: each chunk names its files, its
> contract, its tests and the criteria that close it. Nothing here is implemented.

## 0. How to work this plan

- **One chunk, one branch, one pull request.** A chunk is done when its acceptance criteria all
  pass in CI and the docs it names are updated in the same change (`docs/how-it-works.md` rule:
  "Implementation changes must update this page and the relevant operational guide in the same
  change"). Record user-visible changes under `## 0.1.0 - Unreleased` in `CHANGELOG.md`, in the
  matching thematic subsection (`### Runtime`, `### Execution (opt-in)`, `### Seeing it`, …), as a
  bullet opening with a bold lead sentence. Commit with `git commit -s` (DCO).
- **The floor never moves.** Every chunk leaves `adage hook claude <event>` byte-identical for
  the events it does not add (the goldens in `crates/adage-cli/tests/golden/` and the exec parity
  test `crates/adage-exec/tests/parity.rs` pin this). A new event is a new golden step, re-blessed
  with `ADAGE_BLESS=1 cargo test -p adage-cli --test hooks`, and the diff is reviewed.
- **Fail open, always.** Anything on the hook path exits 0 and prints nothing on any error
  (`CONTRIBUTING.md`). The mod does the same in TypeScript: a missing binary, a timeout or bad
  JSON is `undefined` and `next(e)`.
- **The binary decides; the mod bridges and draws.** No reflex, grant, arm, bound or threshold is
  evaluated in TypeScript. Where this plan says "the mod asks", it means one `$.process.run` of
  `adage hook claude <Event>` with the payload on stdin and JSON on stdout, exactly as a command
  hook would (`docs/claude-code-mod.md` §2).
- **Words.** Person-facing text follows `docs/lexicon.md` and `docs/design/style.md`; the model's
  text stays plain and factual (`<adage-reflex>` blocks, fixed templates in
  `crates/adage-runtime/src/render.rs`).
- **Order.** Workstream A (foundation in the binary) has no Claude Code dependency and starts
  now. Workstream B (the mod) needs S9's Q1, Q2, Q3 and Q6 to pass first. Workstream C (acting
  in the model's place) needs S9's Q9. Workstream D (noticed, skills) is independent of the mod
  on the binary side and needs the mod only to draw. Workstream E (evaluation) grows with each.

## 1. Where things stand (the touch points)

What the design needs, against what the code has. Lines are from the tree named above.

| Need | Today | Where |
| --- | --- | --- |
| A per-`tool_use_id` identity for `PreToolUse`, so the mod and the command hook never both decide | `Claude::event_identity` answers only `PostToolBatch`, `PostToolUse`, `PostToolUseFailure`; `pretool()` never calls it; the only dedup anywhere is adage-exec's journal occurrence id | `crates/adage-hook/src/harness/claude.rs:60-72`, `crates/adage-hook/src/lib.rs:549-722`, `crates/adage-exec/src/hook.rs:1036-1039` |
| An event between "tool results in" and "next request out" | None. `Event` is `SessionStart, Prompt, Batch, Failure, Stop, SessionEnd`; `fast::skip` and `decide` match on it exhaustively | `crates/adage-runtime/src/lib.rs:81-101`, `:559-577`; `crates/adage-runtime/src/fast.rs:40` |
| An authority to predict a call | Grants are `ApprovalKey { repo, route_exec_digest, argv_templates, scripts, effect, level: E1\|E2, output_mode, profile_id, signer }`; `Effect` is a six-variant ordered enum whose order is security-relevant in four places; `--effect` on `review delegate` accepts `verify`/`regenerate` | `crates/adage-exec/src/approvals.rs:22`, `crates/adage-core/src/types.rs:11`, `crates/adage-cli/src/cli.rs:266`, `crates/adage-cli/src/exec.rs:620-871` |
| Sub-support patterns at hook time | The miner keeps a negative border, support in `[s_min − border, s_min)`, `border: 2`, for incremental mining only (`border::Incremental` stores border patterns with their session ids); but labels with session support below `s_min` are removed from the alphabet before mining, so a command seen in three sessions can be in no pattern at all; `judge` returns `Err("… fewer than min_support")` and the compiled `Route` is thrown away into `EngineReport.dropped`; `Index` holds `routes`, `triggers`, `contexts`; nascent routes are filtered out at compile (`RouteState::is_live`) | `crates/adage-graph/src/mine/mod.rs:41-79, 262-273`, `mine/seq.rs:341-382`, `mine/border.rs:55-151`; `crates/adage-learn/src/engine.rs:68, 645-653, 766-767, 972-974`; `crates/adage-tracks/src/index.rs:19-33, 157-164` |
| A `skill` response | `Step` is `Run, ReadBatch, Avoid, Advise, Sequence, Card`; `shape()` and `SHAPES` list seven; Skill tool uses are imported as `CanonicalAction::Control { tool: "Skill <name>" }` with key `control Skill <name>`, effect Observe, "never directed", and the learner's bridge drops every `Control` action before the graph sees it (`input::Step` is `Exec \| Read \| Edit`, `NodeType` has no skill); the steering detector `render::named` cannot read a body it does not know and then marks the session steered by everything after it (`Directed.all`) | `crates/adage-tracks/src/model.rs:186-209, 507-522`; `crates/adage-core/src/tools.rs:72-86`; `crates/adage-learn/src/bridge.rs:69`; `crates/adage-graph/src/input.rs:26-33`, `events.rs:27-34`, `mine/mod.rs:467-475`; `crates/adage-runtime/src/render.rs:1058-1107`; `crates/adage-learn/src/import.rs:187-199` |
| Verified skill steps a hook can read | `adage train` writes `procedure.json` (deterministic, digested, "for people only") and `review.json`; `Verified { NotReplayed, Works, Stale }` per candidate; no dated record outside the pack; precedent for one is `train/cards.json` | `crates/adage-pack/src/procedure.rs:32-79`, `crates/adage-train/src/compile.rs:38-62`, `crates/adage-train/src/skill.rs:297`, `crates/adage-learn/src/cards.rs:64-69` |
| A machine-readable live feed | `adage watch` tails spools and `learn.log` through `SpoolTail`/`LineTail` into `FeedItem`s and draws with ratatui; `--once` prints a frame; no `--follow --json`; the map has an SSE stream | `crates/adage-cli/src/ui/tail.rs:31,105`, `ui/feed.rs:16-56, 238`, `ui/watch.rs:176`, `map/events.rs`, `map/http.rs:191` |
| Harness-priced cost | `Cost { usd, source }`; Claude sessions are priced from a pinned table (`priced@<date>`), Codex `none`, OpenCode `harness`; summaries come from transcripts | `crates/adage-learn/src/ledger.rs:30-41, 96-115` |
| Evidence that the mod saves requests | `demo/run.py` records `num_turns` and `total_cost_usd`, not requests; the registered metric is API request lines in the debug log with a one-sided bootstrap bound | `demo/run.py:198-219`; `evals/reflex-proof/harness/heldout_analysis.py:233-249, 287, 315`; `docs/launch-gate-protocol.md` |
| JSON surfaces for a pane | `graph --json`, `doctor --json`, `status --json`, `delegate status --json`, `savings --json` exist; the status card has no `--json` | `crates/adage-cli/src/graph.rs:342,711`, `doctor.rs:887`, `main.rs:590`, `exec.rs:325`, `savings.rs:678` |

## 2. What changes where

### 2.1 The learner (`adage-learn`, `adage-graph`)

1. **Candidates become an output, not a `Dropped` reason.** Two changes in the miner and one in
   the judge. (a) The alphabet floor: `prepare` (`mine/mod.rs:262-273`) drops labels with
   session support below `s_min`; it drops below `s_min − border` instead, so a command seen in
   three of five sessions can take part in a border pattern. Patterns still need `s_min` to be
   frequent; the border is what `border::Incremental` already stores (`mine/border.rs:55-151`:
   pattern, sessions, parent, extension). The learn-time cost is measured (D1's acceptance:
   `condition --auto` with one transcript changed stays within Track G §9's 150 ms target on the
   sim's history; a full learn within 1.3× of before). (b) The border patterns are passed to
   `select` and `compile` as today's reflexes are, flagged `candidate`; the compiler produces the
   `Route` it would have produced. (c) Where `judge` refuses that route for support alone
   (`engine.rs:766`, and the `Trigger::Context` arm at `:745`), the route is kept as a
   `Candidate { id, trigger, step, support, s_min, days, last_seen, review: false }` instead of
   going to `EngineReport.dropped`. Only triggers of two or more events (`Trigger::Context`) and
   the prompt-family → command shape qualify; one-event after-command or after-edit patterns do
   not (the design's rule: a sequence, never a single call). Candidates are written beside the
   tracks as `<tracks>/<repo>/learned/candidates.json` (not a track: no review, no strength, no
   delivery) and compiled into the index (§2.3). A **nascent** route (`RouteState::Nascent`,
   environment steps waiting for review, `model.rs:550`) is compiled as a candidate with
   `review: true` so the person is told it waits. `learn.log`'s `changes[]` gains
   `change: "candidate"` (`diff.rs:27-35 ChangeKind`) with `support` and `s_min`, and the line
   that turns a candidate into a route names the session ids that tipped it (the `sessions` of
   the border entry), which is what the "this session was its 5th" toast reads.
2. **Skill selection is learned.** Today the bridge drops every `Control` action (`bridge.rs:69`
   `_ => {}`) and the graph's `input::Step` is `Exec | Read | Edit` (`input.rs:26-33`). Add
   `input::Step::Skill { name }` and `NodeType::Skill` (`events.rs:27-34`), map
   `CanonicalAction::Control { tool }` whose `tool` starts with `Skill ` to it in the bridge,
   admit it in `target_label` (`mine/mod.rs:467-475`) as a continuation target only (never a
   context item on its own), and give the compiler a `skill` shape (`compile/shapes.rs:1094
   compile`, beside `fixed_command` at `:224`): continuation one Skill node → `Step::Skill {
   skill }`, effect `Observe`, the same support, rate and uplift gates as `run`. Every other
   `Control` tool stays out. The delegation stimulus is already a key: `control Task
   <subagent_type>` (`tools.rs:77-81`) becomes a `CtxItem::Act`. `StepObs` is unchanged by this
   (the key already carries the name), so no cache format bump here.
3. **Adage's predicted calls are not organic evidence.** The importer marks steering from
   `<adage-reflex>` markers (`import.rs:173-199`, `Directed`). A predicted call is recognised by
   its `tool_use_id` prefix `adage_` in the transcript (the mod mints it, §2.5) and recorded on
   `StepObs` as `by: By::Adage` (new field, `#[serde(default)]`, and `cache.rs:72 SESSION_FORMAT`
   bumps to 5 so cached parses are rebuilt), excluded from `support` the way exposed sessions are
   (`exposed()` in `engine.rs:664`). When S9 Q5 finds the row's `origin` on disk, read that too.
   The skill directive's body must be readable by `render::named` (`render.rs:1058-1107`), else
   `Directed.all` marks the whole rest of the session steered: `named` gains the skill template
   and `Directed` gains `skills: Vec<String>`, and exposure (`engine.rs:118-135`) treats a named
   skill as it treats a named command.
4. **The ledger takes a harness-priced line.** `SessionSummary` gains optional `steps:
   Vec<StepUsage { index, model, input, output, cache_read, cache_write, predicted: bool }>`,
   `hook_ms: Vec<u32>` and `harness_version`; a line with `cost.source = "harness"` for a session
   id replaces the transcript-priced one for that session (`ledger.rs:381`, the `.seen.json`
   dedupe).

### 2.2 The harness (`adage-hook`, `adage-runtime`, `adage-exec`)

1. **`PreToolUse` identity.** `Claude::event_identity` gains a `"PreToolUse"` arm on
   `tool_use_id`; `pretool()` records the identity in a bounded ring on `SessionState`
   (`seen_pretool: Vec<String>`, 64 entries, `#[serde(default)]`) under the state lock before any
   receipt or planner work, and returns `Ok(None)` with no spool record on a repeat.
2. **`TurnStep`.** A new `Event::TurnStep { turn_id, index, model, agent: Option<String> }`,
   translated from a Claude payload the mod builds (§2.5), decided by a new arm of `decide()`
   that **predicts only what Adage has already directed**: a `pending` directive whose
   `expect_key` has not been observed, or an armed, unmet obligation, whose route holds a current
   `predict` grant (§2.2.3) and whose step is a `Run` with every slot bound, a `Sequence`'s first
   command, or a `Skill`. Two bounds live in the state: `last_step_predicted: Option<(String,
   u32)>` (never two predicted steps in a row) and `predictions: u32` (default cap 6, config
   `runtime.predict_cap`). `Decision` gains `predicted: Option<Prediction { route, track, tool,
   input, kind }>` and `would_predict` for the control arm. `fast::skip` answers `false` for
   `TurnStep` when the state has a pending directive or an armed obligation, `true` otherwise, so
   the common case costs the state read alone. A new `Output::Predict { .. }` renders on Claude as
   `{"predict": {"tool": …, "input": {…}, "route": …, "track": …, "kind": …}}` and as `None` on
   the other adapters.
3. **The `predict` grant.** Not a new `Effect` (its order is security-relevant in `run.rs:453,
   461`, `route.rs:404`, `policy.rs:163`, and thirteen match sites would change). A new field on
   `ApprovalKey`, `mode: GrantMode { Execute, Predict }` with `#[serde(default)] = Execute`, so
   every stored approval still parses and a predict key never equals an execute key; `profile_id`
   is the literal `harness` for predict (no sandbox is involved: the harness runs the agent's own
   tool under the person's permission mode). `adage review delegate <track> <route> --predict`
   writes it (person-only, as every delegate verb: `exec.rs:621 require_person`), requires policy
   `max_level >= e2` (`policy.rs:250 permits`), skips the E2 staged preview (nothing is staged),
   and records `basis` as the route's evidence. Lapse, revoke and `delegate status` work unchanged
   through `Store::lapsed`, `route::lapses` and `reapprove_command` (`route.rs:594-681`), which
   gains ` --predict`. A machine switch `adage delegate predict on|off` writes `exec/predict-off`
   like `rewrite-off` (`lib.rs:469`); on requires a terminal.
4. **Provenance of a predicted call.** The Claude adapter reads `tool_use_id` on `PreToolUse`,
   `PostToolBatch` and `PostToolUseFailure`; an id with the `adage_` prefix sets `Call.by =
   By::Adage` (new field, default `Agent`). `decide()` judges such a call `Judgement::Predicted {
   route }` instead of `Followed`, and the receipt path in adage-exec treats it as Adage's own
   run for the purpose of the duplicate-run rule.
5. **Receipt as the result.** `Extension::pretool_context` returns a string today
   (`lib.rs:190-194`, `adage-exec/src/hook.rs:177, 751`). It returns `PreNote { text, fresh:
   bool, result: Option<String> }`: `fresh` when `covering_receipt` found a completed, applied
   run on byte-identical inputs covering the call, and `result` the receipt as the tool's stdout
   would read. `Output::Rewrite` and `Output::Context` carry the note unchanged; the mod (not the
   command hook) answers the tool call with `result` when `fresh` (design §3.3). The command-hook
   path is unchanged, so the floor still delivers the note as context.
6. **`noticed`.** After `fire_contexts` (`lib.rs:1179`), a second walk over the candidate net
   with `st.cand_ctx.recent()` (§2.3.2); every id it returns goes to `Decision.noticed:
   Vec<Noticed { candidate, support, s_min, review }>` and never to `out`; `SessionState.noticed:
   Vec<String>` keeps each candidate to once per session. Both arms record it (it is not a
   directive). The spool record carries it inside `decision`, so `rebuild` needs no new kind,
   and `rebuild` replays `cand_ctx` from `decision.cand_ctx` as it replays `ctx`.
7. **`SkillPrompt`.** A new Claude-only event, `adage hook claude SkillPrompt`, payload
   `{ session_id, cwd, skill, text }`, answered `{ "text": <annotated> }` or `{}`. It reads
   `$ADAGE_HOME/train/skills/<prefix>/verified.json` (§2.4) for a pack whose `procedure.skill.name`
   equals `skill` and whose repository matches, and appends one plain line per verified or stale
   step under a fixed heading. It is spooled as `kind: "skill_prompt"` with the skill name and the
   number of lines added; nothing in the control arm (the text is unchanged there, and
   `would_annotate` is recorded).
8. **Latency on record.** The spool record gains `duration_us` (the hook's own wall time from
   `started`), as zero-config P1 asks, so `doctor` can report a p95 per repository without the
   mod; the mod's measurement (the engine's dispatch time) goes to the ledger line (§2.1.4).

### 2.3 Tracks and the index (`adage-tracks`)

1. **`Step::Skill { skill: String, args: Option<String> }`**, shape `"skill"`, `SHAPES` grows to
   eight; `io::validate` requires a safe id (`[A-Za-z0-9_-]`, 1–64) and no slots; `ceiling` is
   `Observe`'s. `render::route_text` gets a fixed template: "In this repository the `{skill}`
   skill is used at this point. Ignore this if the user is asking for something else." Follow is
   judged on the key `control Skill {skill}` with `pattern::meets`.
2. **`Index.candidates: BTreeMap<repo, Vec<Candidate>>`** and **`Index.candidate_contexts:
   BTreeMap<repo, CtxNet>`**, both `#[serde(default)]` and derived like `triggers` and
   `contexts` (not part of `generation`; the twenty recorded indexes under `evals/datasets/*/
   recorded/` and the one `fast_path_changes_no_decision` loads must keep parsing). The
   candidates get a net of their own, not nodes in the routes' net: the routes' `CtxNet.digest`
   is the alphabet's hash, and a changed digest resets every session's context state
   (`adage-runtime/src/context.rs:72-76`); candidates change at nearly every learn, and a
   relearn must not reset route matching mid-session. `Triggers::of` folds every candidate
   item's words and globs in, so the fast path never skips an event that could notice. The
   runtime keeps a second ring, `SessionState.cand_ctx: CtxState`, symbolised from the same
   event by a second `Symbols` over the candidate net (only when that net is non-empty).
3. **`Candidate`** lives in `adage-tracks/src/model.rs` beside `Route`; `io` reads and writes
   `candidates.json` next to a repository's `learned` track with the same owner-only mode as the
   index (`io::write_private`).

### 2.4 Training (`adage-train`)

1. **`verified.json`** beside the staged pack: `$ADAGE_HOME/train/skills/<prefix>/verified.json`,
   `{ schema: "adage-train-verified/1", skill: { name, sha256 }, repo, steps: [{ id, command,
   result: "works" | "stale" | "not-replayed", signature?, date, sandbox }] }`, written in
   `skill.rs::stage` (`:297`) from `Candidate.verified` and `Report.sandbox`. It is outside the pack
   on purpose: a date would break the pack's deterministic digest (the `cards.json` precedent).
2. `adage train accept` leaves it in place; `adage train` on a changed skill (new `sha256`)
   replaces it.

### 2.5 The CLI and surfaces (`adage-cli`)

1. **`adage watch --follow --json`**: `ui/watch.rs` gains a mode that needs no terminal, emits
   one JSON line per `FeedItem` (`ui/feed.rs:56`), per `learn.log` line and per health change,
   from the same `Sources` (`watch.rs:16`), with `--since <offset>` for a reconnect; the
   terminal frame is untouched. Health lamps every few seconds as today (`LOCK_EVERY`).
2. **`adage ledger note`**: reads one JSON object from stdin (§2.1.4's shape), validates it,
   writes the ledger line and refreshes the savings summary (`savings.rs:415 refresh`). Allowed
   from inside a session (it tunes nothing).
3. **`adage review delegate … --predict`**, **`adage delegate predict on|off`**, and
   `delegate status --json` showing `mode`.
4. **`adage doctor`** gains the hook p95 check from spool `duration_us` (a warning above 10 ms
   no-match, per Architecture §9's budget) and a line for the harness version last seen.
5. **`cargo xtask tokens`** gains a third output, `plugin/claude-code/hooks/tokens.ts`
   (`xtask/src/main.rs:74-80` `outputs`), with the glyph table (UTF-8 and ASCII) and the roles the
   mod maps to theme keys; the staleness test asserts it like the other two.
6. **The status card** gains `--json` (`status_card.rs:367`), the same rows the card prints.

### 2.6 The mod (`plugin/claude-code`)

Files as in the design §2: `hooks/hooks.json` gains `"modules": ["./register.ts"]` beside the
seven command hooks; `hooks/register.ts`, `bridge.ts`, `decide.ts`, `draw/*.tsx`, `feed.ts`,
`commands.ts`, `tokens.ts` (generated), `*.test.ts`; `types/index.d.ts`; `plugin.json` gains
`"types"` and `userConfig`. The same module ships in `plugin/claude-code-exec`. `scripts/
check-plugin.mjs` learns the module files and `types`. CI's `plugin` job runs `claude plugin test`
after `validate --strict`.

### 2.7 Evaluation (`adage-sim`, `demo/`, `evals/`, `adage-eval`)

1. **The simulator** emits `PreToolUse` (per Bash call, with `tool_use_id`), `TurnStep` (per
   turn, after the batch) and `SkillPrompt` payloads for Claude (`crates/adage-sim/src/claude.rs`
   beside `turn_hooks` at `:158`), and `scenario::Replay` gains per-turn expectations
   (`predict`, `noticed`); `harness_sim.rs::replay_claude` asserts them in both arms and
   `check_graph` counts predictions.
2. **The demo runner** gains a `mod` arm (`claude -p … --plugin-dir plugin/claude-code`; hooks
   run headless, drawing does not) and a `requests` metric counted as the registered protocol
   counts it: `[DEBUG] [API REQUEST]` lines under `--debug`, cross-checked against distinct
   assistant message ids (`heldout_analysis.py:233-249`). `report.py::compare` already takes arm
   names.
3. **The offline replay gate** `fast_path_changes_no_decision` (`adage_eval::replay`) covers
   `TurnStep` and candidates: a skipped event's full decision must be empty, `noticed` included.
4. **The reflex-proof harness** gets a `MOD` arm beside `BEST` with the D1–D4 development
   thresholds; a registered held-out run only after the development loop passes.

## 3. The chunks

Each chunk: what it delivers, the files, the contract, the tests, the acceptance criteria, an
estimate in engineer-days, and what it depends on. Chunks in one workstream are ordered.

### Workstream A · Foundation in the binary (no Claude Code dependency)

#### A1 · `PreToolUse` identity dedup

- **Delivers.** One decision per `tool_use_id` however many times the event is delivered.
- **Files.** `crates/adage-hook/src/harness/claude.rs:60-72` (`event_identity` gains
  `"PreToolUse" => vec![p.get("tool_use_id")?.as_str()?]`); `crates/adage-runtime/src/lib.rs:137`
  (`SessionState.seen_pretool: Vec<String>`, `#[serde(default, skip_serializing_if =
  "Vec::is_empty")]`, bounded at 64, oldest dropped); `crates/adage-hook/src/lib.rs:549-570`
  (before the receipt and planner: `state::update` records the identity; a repeat returns
  `Ok(None)`); `crates/adage-runtime/src/rebuild.rs` (the ring is not rebuilt: a rebuilt state
  starts empty, which is fail-open).
- **Contract.** A `PreToolUse` payload with a `tool_use_id` seen in this session produces no
  output and no spool record. A payload without `tool_use_id` is decided as today.
- **Tests.** Unit: `harness/mod.rs identity_tests` gains the PreToolUse case. Integration
  (`crates/adage-cli/tests/hooks.rs`): the same PreToolUse payload twice gives one rewrite then
  nothing, and the spool has one `pretool` record; 65 distinct ids keep deciding (the ring
  bound). Golden: `claude_scenario` gains a `PreToolUse` step with a rewrite (the U11 fixture
  from `zero_config_plugin.rs`'s environment) so the output is pinned; parity re-blessed.
- **Acceptance.** `cargo test -p adage-hook -p adage-cli --test hooks` green; one spool record
  per id in the integration test; the Claude golden diff shows only the added step; p95 of the
  PreToolUse no-match path unchanged within 0.1 ms in `hook_latency` (add a `pretool_no_match`
  scenario).
- **Estimate.** 1 day. **Depends on.** Nothing.

#### A2 · Spool `duration_us` and the doctor p95 check

- **Files.** `crates/adage-hook/src/lib.rs:1091` (the decision record), `:712` (`pretool`),
  `:936` (`session_start`): add `"duration_us": started.elapsed().as_micros()` (pass `started`
  into `respond` and `pretool`); `crates/adage-cli/src/doctor.rs:812` (`checks()` gains `hook
  latency`: p95 of `duration_us` over the last 500 records of this repository's sessions; warn
  above 10,000 µs for `batch` records with empty `would_have`, fail above 25,000); `ui/feed.rs`
  ignores the field.
- **Tests.** Unit for the percentile over synthetic spools (nearest rank, as
  `release_plugin_startup` computes it); `doctor --json` shows the new check with `status:
  pass|warn|fail`; the `fresh_install.rs` snapshots re-blessed if the card changed.
- **Acceptance.** `adage doctor --json | jq '.checks[] | select(.check=="hook latency")'` present
  on a seeded home; the no-match hook cost unchanged within 0.1 ms p95.
- **Estimate.** 1 day. **Depends on.** Nothing.

#### A3 · `adage watch --follow --json`

- **Delivers.** The pane's feed and the band's countdown without a terminal.
- **Files.** `crates/adage-cli/src/ui/watch.rs` (a `follow_json(ctx, since)` loop over
  `Sources::poll`, writing one line per `FeedItem`, per `learn.log` line
  `{"kind":"learn", …}` and per health change `{"kind":"health", lamps:[…]}`; also
  `{"kind":"learner", state: "waiting"|"running"|"idle", next_at}` from
  `<slug>.spawn.json`/`.lock`); `ui/mod.rs:100` (accept `--follow`, `--json`, `--since`); the
  record shape documented in `docs/live-learning.md` under "Streams to follow".
- **Contract.** NDJSON on stdout, one object per line, `seq` monotonic per run, `--since <seq>`
  resumes; exits 0 on stdin EOF or SIGTERM; never writes to the terminal.
- **Tests.** `crates/adage-cli/tests/watch.rs` gains `follow_json_streams_spool_and_learn_log`:
  start the command, append a spool record and a `learn.log` line, read two JSON lines within
  2 s, send EOF, exit 0. A `--since` test.
- **Acceptance.** 200 rows at 2 Hz for 60 s with no dropped `seq`; memory flat (the ring is
  bounded at 500 rows); the terminal pane's snapshots unchanged.
- **Estimate.** 2 days. **Depends on.** Nothing.

#### A4 · `adage ledger note` and harness-priced cost

- **Files.** `crates/adage-learn/src/ledger.rs:30-41` (`StepUsage`, `hook_ms`,
  `harness_version` on `SessionSummary`; `Ledger::note(summary)` writing the line and marking
  the session in `.seen.json` as harness-priced so a later transcript summary does not replace
  it); `crates/adage-cli/src/ledger.rs` (new: `adage ledger note` reads stdin, validates, calls
  `Ledger::note`, then `savings::refresh`); `savings.rs:370 gather` prefers `source: harness`;
  `docs/live-learning.md` "Files".
- **Contract.** stdin: `{ "session_id", "repo", "harness": "claude-code", "harness_version",
  "started_at", "ended_at", "steps": [{ "index", "model", "input", "output", "cache_read",
  "cache_write", "predicted" }], "cost_usd"?, "hook_ms": [..] }`. A second line for the same
  session replaces the first. Exit 2 on a malformed line, nothing written.
- **Tests.** Unit: a harness line wins over a priced one; `cost.source == "harness"`;
  `predicted` steps are counted as zero-cost steps and listed. Integration: `savings --json`
  reflects the line; `pricing_honesty` still passes for transcripts with unknown models.
- **Acceptance.** `echo '<line>' | adage ledger note && adage savings --json` shows the session
  with `source: harness`; the Hajek estimator's inputs are unchanged for sessions without a
  note (the `savings_gate` test passes).
- **Estimate.** 2 days. **Depends on.** Nothing.

#### A5 · JSON for the pane: status card `--json`, `tokens.ts`

- **Files.** `crates/adage-cli/src/status_card.rs:367` (`--json` emits the rows); `xtask/src/
  main.rs:74-80` (third output `TS_OUT = plugin/claude-code/hooks/tokens.ts`: `export const
  GLYPHS = { reflex: ['◠','~'], instinct: ['◉','(o)'], rewrite: ['↪','=>'], … }` and `export const
  ROLES = { accent: 'accent', fire: 'success', prime: 'warning', inhibit: 'error', … }` mapping
  Adage roles to Claude Code theme keys, generated from the tokens' `glyph` and role groups);
  the staleness test asserts three outputs; `docs/design/style.md` notes the third output.
- **Acceptance.** `cargo xtask tokens --check` passes with the committed `tokens.ts`;
  `adage --json` parses and matches the card's rows one to one in a snapshot test.
- **Estimate.** 1 day. **Depends on.** Nothing.

### Workstream B · The mod: see it (Layer 1 of the design)

Needs S9 Q1, Q2 (Layer 1), Q3 and Q6 to have passed. Every chunk here ships behind
`claude plugin validate --strict` and `claude plugin test` in CI.

#### B1 · The module skeleton, the bridge, and CI

- **Files.** `plugin/claude-code/hooks/hooks.json` (add `"modules": ["./register.ts"]`),
  `hooks/register.ts`, `hooks/bridge.ts` (`ask($, event, payload, timeoutMs)`: `$.process.run`
  of `$DATA/bin/adage hook claude <event>` with stdin, fail-open), `types/index.d.ts`
  (`PluginState` for `adage`: `session`, `fired`, `rewrites`, `rows`), `.claude-plugin/plugin.json`
  (`types`, `userConfig` rows `band`, `pane`, `notices`, `noticed`, `skills_invoke`,
  `hold_dead_ends`, `routine_model`), `hooks/bridge.test.ts` (stubbed `$.process.run`: exit 0
  with JSON, exit 1, timeout, not JSON, missing binary → `undefined` in all but the first);
  `scripts/check-plugin.mjs` (allow `modules`, require each module file to exist, require `types`
  to exist, keep every other rule); `.github/workflows/ci.yml` `plugin` job (add `claude plugin
  test plugin/claude-code` and the exec plugin); the same files mirrored into
  `plugin/claude-code-exec` with the binary name `adage-exec`.
- **Where the binary is.** The shim links `${CLAUDE_PLUGIN_DATA}/bin/adage`; the mod reads
  `$.env.get('CLAUDE_PLUGIN_DATA')` (to verify: the engine's process environment carries it, or
  the mod derives it from `$.plugin.root` as the shim's `session-start` does) and falls back to
  `adage` on `PATH` through `$.process.run(['adage','version'])`.
- **Acceptance.** `claude plugin validate --strict plugin/claude-code` lists exactly the hooks and
  calls the module makes (`hooks:` and `calls:` lines reviewed and committed as
  `plugin/claude-code/VALIDATE.txt` so a drift shows in review); `claude plugin test` passes; a
  session with the mod loaded shows `1 mod active · adage` under `/plugin`; with the binary
  removed every hook returns `next(e)` and the session is unchanged (`-p` diff identical).
- **Estimate.** 2 days. **Depends on.** S9 Q1, Q6.

#### B2 · Observe the command hooks; the rows and the band

- **Files.** `hooks/decide.ts` (`classic.UserPromptSubmit`, `classic.PostToolBatch`,
  `classic.PostToolUseFailure`, `classic.Stop`, `classic.SessionStart`: `await next(e)`, parse
  `<adage-reflex …>` attributes (`track`, `route`, `strength`, `kind`, `origin`) and the
  `noticed`/`predicted` fields of the record once A6/C1 land, update `$.state`); `hooks/draw/
  rows.tsx` (`ui.render` on `UserMessage`, `ToolUse`, `ToolGroup`, `Spinner`, `TurnDuration`,
  `InfoNotice`: the lines of design §4, each a `Box` with the engine's reference then one dim
  `Text`); `hooks/draw/band.tsx` (`AbovePrompt`: one line, `next(e)` when nothing to say);
  `hooks/draw/*.test.tsx` (`ui.mount` per site over `['terminal','desktop']`, fixture state).
- **Rules enforced in code.** No `ui.render` hook registered when `session.start`'s
  `isInteractive` is false; nothing drawn per event when the state's arm is unknown or the
  record is empty; theme keys from `tokens.ts`, never a literal.
- **Acceptance.** `claude plugin test` green on both surfaces for every site; the `-p` run of
  `demo/run.py one W-2041 adage` with `--plugin-dir` produces a transcript identical to the one
  without the mod (hooks observe, nothing changes); an interactive session on the acme-billing
  sandbox shows the prompt line, the tool-row marker and the band as in design §1 (screenshot
  recorded in `spikes/S9-claude-mods/results/`).
- **Estimate.** 4 days. **Depends on.** B1, S9 Q3.

#### B3 · The pane and `/adage …`

- **Files.** `hooks/feed.ts` (`$.process.spawn(['<bin>','watch','--follow','--json','--repo',
  cwd])` from `session.start`, rows into `$.state`, restart with `--since` on exit while the
  module lives), `hooks/draw/pane.tsx` (tabs Live, Reflexes, Health per design §4.1; rows as
  `plain` Buttons; the inspect pane; `Copy command` via `$.ui.copy`; a `Link` to the map),
  `hooks/commands.ts` (`/adage`, `/adage watch|reflexes|health|why|off|on|pause`; `off`/`on`
  through `adage` writing `.adage/disabled`), `hooks/draw/card.tsx` (`CommandOutput` for
  `/adage` and `/adage why` from `adage --json` and the spool's last decision).
- **Acceptance.** Q3b's criteria: no dropped `seq` at 2 Hz; the child is gone after `/clear`
  and exit (`ps` in the test harness); every row of the design's `adage watch` mapping table
  measured Direct, Close or Out and recorded; `adage review` is never run by the mod (grep the
  validate output: no `process.run` argv containing `review`, `delegate`, `install`, `config
  set` in `VALIDATE.txt`).
- **Estimate.** 5 days. **Depends on.** A3, A5, B2.

### Workstream C · Act in the model's place (Layers 2 and 4)

#### C1 · `TurnStep` in the binary

- **Delivers.** The decision "predict this call now, or nothing", on the CPU, for a directive
  Adage already delivered.
- **Files.** `crates/adage-runtime/src/lib.rs:81` (`Event::TurnStep { turn_id, index, model,
  agent }`, `name() = "turn_step"`), `:137` (`SessionState.last_step_predicted`,
  `predictions`), `:250-304` (`Prediction`, `Decision.predicted`, `Decision.would_predict`,
  `Judgement::Predicted`), `:559` (the `decide` arm: for each `pending` whose `expect_key` is
  unobserved and each armed unmet obligation, in delivery order, the first with a current
  `predict` grant and a fully bound command; bounds checked; control arm records
  `would_predict`), `fast.rs:40` (the `TurnStep` arm), `rebuild.rs` (replays `predicted`);
  `crates/adage-hook/src/harness/mod.rs:35` (`Output::Predict { tool, input, route, track, kind
  }`), `harness/claude.rs:74-123` (`translate` reads `turn_id`, `index`, `model`, `agent_id`;
  `render` emits `{"predict": …}`), `codex.rs:72`, `opencode.rs:71` (`Predict → None`);
  `crates/adage-hook/src/lib.rs:946` (`respond` passes the grant lookup through the extension:
  `Extension::predict_grant(repo, route) -> Option<Grant>`; without adage-exec the answer is
  `None` and nothing is predicted). `Call.by` and the `adage_` prefix on `PreToolUse`,
  `PostToolBatch`, `PostToolUseFailure` in `claude.rs:42-53`.
- **Contract.** stdin: `{ "session_id", "cwd", "hook_event_name": "TurnStep", "turn_id",
  "index", "model", "agent_id"? }`. stdout: nothing, or `{ "predict": { "tool": "Bash", "input":
  { "command": "make gen" }, "route", "track", "kind": "obligation" | "run" | "sequence" |
  "skill" } }`. Never two predictions in a row for one `turn_id`; at most `runtime.predict_cap`
  per session; nothing in the control arm; nothing without a grant.
- **Tests.** Unit (`adage-runtime/src/tests.rs`): a pending directive with a grant predicts
  once; the next `TurnStep` without an intervening batch predicts nothing; a batch that observes
  the key clears it; the cap; the control arm's `would_predict`; a `Skill` step predicts the
  Skill tool. Integration (`hooks.rs`): the golden gains `TurnStep` steps (one predicting, one
  silent); `hook_latency` gains `turn_step_nothing_pending` (must skip) and
  `turn_step_predicts`. Replay gate: `fast_path_changes_no_decision` over `TurnStep`.
- **Acceptance.** `turn_step_nothing_pending` p95 ≤ the `batch_no_match` p95 + 0.3 ms;
  `turn_step_predicts` p95 ≤ 10 ms; goldens re-blessed with only the added steps; the control
  arm emits nothing in the sim (C4).
- **Estimate.** 4 days. **Depends on.** A1 (shared `tool_use_id` handling).

#### C2 · The `predict` grant in adage-exec and the CLI

- **Files.** `crates/adage-exec/src/approvals.rs:22` (`mode: GrantMode`, default `Execute`;
  `for_request` sets it from the request; `diff` names it), `request.rs` (`ExecRequest.mode`),
  `route.rs:264 plan` (a `Predict` request plans no action spec and no sandbox profile:
  `profile_id = "harness"`), `:594-681` (`Lapse::what` wording for `mode`; `reapprove_command`
  appends `--predict`), `hook.rs` (`impl Extension::predict_grant`: the current approval for
  `(repo, route)` with `mode == Predict`, current key, policy permits), `policy.rs:250`
  (`permits` requires `max_level >= E2` for predict); `crates/adage-cli/src/cli.rs:251-266`
  (`--predict` on `DelegateRoute`, exclusive with `--effect`), `exec.rs:620-871` (the branch:
  no preview, `basis` from evidence, `terms.effect = route.effect`), `exec.rs:277` (`delegate
  predict on|off`: writes or removes `exec/predict-off`; `on` requires a person), `exec.rs:325`
  (`status --json` shows `mode`); `docs/execution.md` (a "Predicted calls" section beside
  "Approvals" and "Policy").
- **Tests.** Unit: a predict key never equals an execute key for the same route; an old
  `approvals.json` without `mode` loads as `Execute`; lapse on a changed template names the field;
  `permits` refuses predict at `max_level e1`. Integration (`crates/adage-cli/tests/exec.rs`):
  `review delegate … --predict` refuses inside an agent session (`CLAUDECODE` set), succeeds at a
  terminal, shows in `delegate status --json` with `mode: predict`, and `predict off` stops C1
  from predicting (the hook returns nothing).
- **Acceptance.** `adage review delegate <track> <route> --predict` round-trips through
  `delegate status`, `revoke`, and a lapse notice; `adage-exec hook` parity with `adage hook`
  holds with no grants.
- **Estimate.** 3 days. **Depends on.** C1.

#### C3 · Receipt as the result, rewrites checked by the engine

- **Files.** `crates/adage-hook/src/lib.rs:190-194` (`pretool_context` returns `PreNote`),
  `adage-exec/src/hook.rs:751` (`covering_receipt` fills `fresh` and `result`), `harness/
  claude.rs:117` (`Output::Rewrite` and `Context` carry `receipt: { fresh, result }` in a
  sibling JSON field the command hook renders too, so the mod reads one shape); the mod's
  `hooks/decide.ts` (`tool.call { tool: 'Bash' }`: ask `PreToolUse`; if `receipt.fresh` return
  `{ result: { stdout: receipt.result, stderr: '', interrupted: false }, context: [note] }`; else
  if a rewrite, `$.tool.check` then `next({ ...e, command })` with `$.ui.notice`; else
  `next(e)`); `hooks/draw/rows.tsx` (`ToolResult` receipt card).
- **Tests.** Rust unit: `fresh` is true only for an applied run on identical inputs covering the
  call; false after any file in the snapshot changes. Mod tests: the three branches with stubbed
  `ask` and `$.tool.check` answers (`allow`, `ask`, `deny`); the `deny` branch never rewrites.
  S9 Q4's four settings fixtures recorded.
- **Acceptance.** In the acme-billing demo with E2 on, a second `make test` after a passing
  Adage run returns the receipt as the result and runs no process (verified by the sandbox's
  command log); the no-mod path still gets the note as context (golden unchanged apart from the
  new field); `$.tool.check` agrees with `pretool::keeps_permissions` on every settings fixture.
- **Estimate.** 3 days. **Depends on.** A1, B1, C1 (for `by: Adage`), S9 Q4.

#### C4 · The mod's `turn.step` and the simulator

- **Files.** `hooks/decide.ts` (`turn.step` generator per design §3.2: main loop only, ask
  `TurnStep`, yield `tool`/`input`/`stop`, return the result; a `Skill` prediction yields the
  `Skill` tool with `{ skill, args? }` and checks `$.command.list()` lists the skill first);
  `hooks/draw/rows.tsx` (the "ran this step on the CPU" marker keyed by the `adage_` id);
  `crates/adage-sim/src/claude.rs` (`turn_step_hook(turn, sid, cwd)` payloads between batches;
  `pretool_hook` per Bash call with `tool_use_id`), `scenario.rs:417 Replay` (`predict` and
  `pretool` expectations per turn), `crates/adage-cli/tests/harness_sim.rs:192 replay_claude`
  (asserts a prediction at the registry-edit turn when the grant is present, nothing in control,
  nothing without the grant; `check_graph` counts `predicted`); `cargo run -p adage-sim`
  regenerates fixtures.
- **Acceptance.** S9 Q9's criteria in a live session (10 of 10 runs); the sim passes in both
  arms; `claude plugin test` covers the generator with a stubbed `ask` (yields exactly three
  chunks, returns `stopReason: 'tool_use'`, `usage: null`); with the binary missing the step is
  forwarded unchanged.
- **Estimate.** 4 days. **Depends on.** C1, C2, B1, S9 Q9.

#### C5 · Cached delivery, compaction, routing

- **Files.** `hooks/decide.ts` (`prompt.compose` appends `adage:procedures` from the
  `SessionStart` answer's standing text, a new field `standing` the binary adds beside the
  contract: the applicable procedure cards' titles and the instincts' one-liners, fixed for the
  session; `tool.describe { tool: 'Bash' }` appends the environment note when U11 is bound;
  `session.compact` drops Adage's rows and adds the instruction; `turn.step` routes with
  `next({ ...e, model, effort })` when the answer says `routine` and `routine_model` is set);
  `crates/adage-hook/src/lib.rs:750 session_start` (`standing` in the output JSON);
  `crates/adage-hook/src/lib.rs` (`TurnStep` answers `routine: true` when a fire reflex is
  pending without a grant).
- **Tests.** Mod tests for each hook with fixtures; a Rust unit for `standing`'s size bound
  (≤ 2,000 characters) and stability (identical across two `SessionStart`s in one session).
- **Acceptance.** The system prompt's `adage:procedures` section is byte-identical across every
  request of a session (read with `$.prompt.compose()` in a test); the per-event
  `additionalContext` for standing text is no longer emitted when the mod reports the section is
  in place (the binary reads a `has_standing: true` flag the mod passes on `SessionStart`'s
  stdin, absent for command hooks); Q11 reported.
- **Estimate.** 3 days. **Depends on.** B2, C1.

### Workstream D · Noticed and skills (binary first, mod to draw)

#### D1 · Candidates in the learner and the index

- **Files.** `crates/adage-graph/src/mine/mod.rs:262-273` (the alphabet floor becomes
  `s_min − border`), `mine/mod.rs:505-524` (`mine()` returns border patterns beside the frequent
  ones, flagged), `select.rs:603-688` (border patterns scored and carried as candidates, never
  selected), `compile/shapes.rs:1094` (compiled like a reflex, `Compiled.candidate: bool`);
  `crates/adage-tracks/src/model.rs` (`Candidate`), `io.rs` (read/write `candidates.json`),
  `index.rs:19-33, 199-210` (`candidates`, `candidate_contexts`, `Triggers::of` over candidate
  items), `triggers.rs:102`, `context.rs:51` (`CtxNet::of_candidates`); `crates/adage-learn/
  src/engine.rs:745-767, 965-976` (keep as candidate instead of `Err` when `Compiled.candidate`
  and the trigger qualifies), `lib.rs:292-296` (write `candidates.json` under `tracks.lock`,
  compile into the index; nascent routes → `review: true` candidates), `diff.rs:27-45`
  (`ChangeKind::Candidate`, and the tipping session ids on `New`); `docs/live-learning.md`,
  `docs/how-it-works.md` ("From history to a reflex": a candidate is counted, never delivered),
  `docs/tracks/track-g-graph-engine.md` §9 (the floor).
- **Contract.** `candidates.json`: `{ "schema": "adage-candidates/1", "repo", "s_min",
  "candidates": [{ "id", "trigger", "step", "support", "days", "last_seen", "review" }] }`,
  ids `candidate-<stable(context ‖ step)>` so a candidate that crosses `s_min` becomes the
  reflex with the matching stable id (`compile/ids.rs`).
- **Tests.** `adage-learn` unit: a pattern with support `s_min − 1` over qualifying triggers
  yields a candidate, not a route; `s_min − 3` yields nothing; a one-event after-edit pattern
  yields nothing; the next learn with one more supporting session turns it into a route with the
  same stable id. `adage-tracks` unit: `Triggers::of` includes candidate words; `CtxNet::walk`
  returns `cand:` ids; the index round-trips. Determinism: `index_is_deterministic` with
  candidates.
- **Acceptance.** `adage condition --demo` on the sim's twelve sessions writes at least one
  candidate on the acme-billing fixture (the registry sequence appears in three of twelve, below
  `s_min` under default thresholds and above the border) and the index's `candidates` lists it;
  `hook_latency` `batch_no_match` p95 within +0.3 ms of before with candidates compiled; learn
  time on the sim's 36 sessions within 1.3× of before and `condition --auto` with one changed
  transcript under 150 ms; `cargo test -p adage-graph --test border` (incremental equals full)
  still passes with the lower floor; every recorded index under `evals/datasets/` still loads.
- **Estimate.** 5 days. **Depends on.** Nothing.

#### D2 · `noticed` at hook time

- **Files.** `crates/adage-runtime/src/lib.rs:137` (`SessionState.cand_ctx`, `noticed`),
  `:559-567` (a second `Symbols` over `ctx.index.candidate_contexts(&repo)`), `:960-986,
  1025-1033, 673-678` (push the same symbols into `cand_ctx`), after `:1179 fire_contexts` (the
  candidate walk into `d.noticed`, once per session), `:287 Decision.noticed`, `Decision.cand_ctx`;
  `rebuild.rs` (replays both); `crates/adage-cli/src/ui/feed.rs:16 Act::Noticed { candidate,
  support, s_min, review }`, `graph.rs:774 event()` includes it; `savings.rs:336` ignores it;
  `adage_eval::replay` gate covers it (a skipped event's `noticed` must be empty too).
- **Contract.** In the spool record's `decision`: `"noticed": [{ "candidate", "support", "s_min",
  "review" }]`; nothing in `additionalContext`; both arms.
- **Tests.** Runtime unit: a candidate's context fires `noticed` exactly once per session and
  never a directive; the same events with the route live fire the directive and no `noticed`.
  Integration: the sim's held-out session notices the registry candidate at the `make gen` turn
  in both arms (`replay_claude`), and `adage watch --once` shows the row.
- **Acceptance.** Replay gate green (`fast_path_changes_no_decision` over every dataset with
  candidates compiled); the sim passes; p95 as D1.
- **Estimate.** 2 days. **Depends on.** D1.

#### D3 · The `skill` response kind

- **Files.** `crates/adage-tracks/src/model.rs:186-209, 507-522` (`Step::Skill`, shape, `SHAPES`
  to eight), `io.rs:242-371, 406-414` (validation; `Skill` allowed under `Prompt`, `Intent`,
  `AfterEdit`, `Context`), `interlock.rs:203`; `crates/adage-runtime/src/render.rs:935-1049`
  (`route_text`: the template), `:1058-1107` (`named` reads the template back), `lib.rs:1196-1203`
  (`fire_contexts` admits a `Skill` step), and the follow judge on `control Skill {skill}`;
  `crates/adage-learn/src/bridge.rs:69`, `crates/adage-graph/src/input.rs:26-33`, `events.rs:27-34`,
  `mine/mod.rs:467-475`, `compile/shapes.rs:1094, 224`, `compile/ids.rs:50`, `compile/mod.rs:367
  commands()` (a `Skill` step has no command); `crates/adage-learn/src/engine.rs` (judge `skill`
  like `run`), `import.rs:187-199` (`Directed.skills`); the exhaustive `Step` matches in
  `adage-eval` (`score.rs:378, 574`, `savings_replay.rs:456`, `conventions.rs:423`, `lib.rs:225`)
  and `adage-cli` (`graph.rs:153, 175`, `share.rs:177`, `ui/routes.rs:270`, `review.rs:191, 629,
  1155`); `adage-eval` fixtures with a Skill tool use; `docs/training.md` ("What does not compile
  yet" loses the "command with no stimulus" item for the skill case), `docs/how-it-works.md` (a
  fifth behaviour row).
- **Packs, deferred.** `adage-pack`'s `Shape`, `TriggerV2` and `StepV2` have no skill (and no
  context trigger), every struct is `deny_unknown_fields`, and `convert.rs:198, 255-306` refuses
  what it does not know. A `skill` route is learned and delivered locally in this chunk;
  sharing it in a pack is a format revision (`adage-pack/2`) scheduled after D4 and not needed
  for the mod.
- **Tests.** Unit: validation accepts `release-checklist`, refuses `../x` and a 65-character
  name; `shape()` is `skill`; the directive renders the fixed template and `named` reads the
  skill back; a session that calls `Skill { skill }` within the window is `Followed`; the
  bridge maps a `Skill` control action and nothing else. Learner (`adage-learn/tests/golden.rs`
  with `Tx::call("Skill", …)`): six synthetic sessions whose prompts share a family and whose
  agent invoked the skill after the version bump compile to a `skill` route at prime; four do
  not; a session with the skill directive is steered by that skill alone, not `all`.
- **Acceptance.** `cargo test -p adage-tracks -p adage-graph -p adage-learn -p adage-runtime
  -p adage-eval -p adage-cli` green; the `claude-code` golden gains a prompt step delivering a
  `skill` directive (`kind="skill"`) and is re-blessed; the eval gates unchanged (the
  `replay_with_the_learned_index_gives_every_recorded_directive` whitelist lists the new id).
- **Estimate.** 5 days. **Depends on.** Nothing.

#### D4 · `verified.json` and `SkillPrompt`

- **Files.** `crates/adage-train/src/skill.rs:297 stage` (write `verified.json`),
  `crates/adage-train/src/compile.rs:38-62` (expose `Verified` per step with the step id);
  `crates/adage-hook/src/lib.rs:358` (a `SkillPrompt` branch like `PreToolUse`: Claude only;
  find `train/skills/*/verified.json` whose `skill.name == skill` and whose `repo` matches the
  session's; append lines; spool `kind: "skill_prompt"`), `harness/claude.rs` (`translate` and
  `render` for `{ "text": … }`); the mod's `hooks/decide.ts` (`skill.prompt` → ask `SkillPrompt`
  → `{ text }`); `docs/training.md` (a "Verified in place" subsection).
- **Contract.** stdin `{ "session_id", "cwd", "hook_event_name": "SkillPrompt", "skill", "text"
  }`; stdout `{ "text": "<original>\n\n<fixed heading>\n<one line per step>" }` or nothing. The
  heading is a fixed template: "Adage verified these steps in this repository on <date>:".
  Lines name the step's command and `works`/`stale (<signature>)`. Plain and factual; at most
  2,400 bytes added.
- **Tests.** Train: `verified.json` is written with one entry per candidate and is not part of
  the pack digest (`adage-pack` validation unchanged). Hook: a matching skill gets the lines, a
  non-matching name gets nothing, the control arm gets nothing and spools `would_annotate`.
  Mod: `skill.prompt` with a stubbed answer rewrites `text`; without an answer `next(e)`.
- **Acceptance.** `adage train evals/lab/skills/skills/acme-work-item/SKILL.md --repo <sandbox>`
  writes `verified.json`; `echo '{…"skill":"acme-work-item"…}' | adage hook claude SkillPrompt`
  prints the annotated text; S9 Q13's three entry points verified in a live session.
- **Estimate.** 3 days. **Depends on.** D3 (for the pack's `Step::Skill`, optional), B1.

#### D5 · Drawing noticed, learning and skills

- **Files.** `hooks/draw/rows.tsx` (the `ToolGroup`/last-`ToolUse` noticed line; the `Skill`
  tool-row markers), `hooks/draw/band.tsx` (the learner countdown from the feed's `learner`
  records; `learning…` while running), `hooks/decide.ts` (toasts from `learn.log` lines:
  strength changes, a candidate that formed, naming this session when its id is in the line's
  `sessions`), `feed.ts` (passes `learn` records).
- **Acceptance.** S9 Q12's read of the recording; `claude plugin test` for each row; no line for
  a single call (unit over fixture records with one-event candidates filtered by D1).
- **Estimate.** 2 days. **Depends on.** D2, D4, B3.

### Workstream E · Evaluation

#### E1 · Requests as a metric in the demo runner

- **Files.** `demo/run.py:140-149` (add `--debug` and capture the debug log path; count
  `[API REQUEST]` lines and distinct assistant `message.id`s as `heldout_analysis.py:233-249`
  does; `metrics.requests`, `metrics.requests_verified`), `:105-113` (a `mod` arm:
  `settings_file('adage')` plus `--plugin-dir plugin/claude-code`), `demo/report.py:128-142
  METRICS` (`requests`), `demo/SANDBOX.md` (the scorer's real keys: `passed, ci_passed,
  hidden_passed, hidden_tests_run, stale_generated, ci_tail, hidden_tail`).
- **Acceptance.** `python3 demo/run.py one W-2041 mod` records `requests` equal to the
  distinct-id count within the API-retry slack; `compare(runs, a="mod", b="control")` reports
  the task-stratified permutation p and the bootstrap interval for `requests`.
- **Estimate.** 2 days. **Depends on.** B1.

#### E2 · Development gate for Layer 4

- **Files.** `evals/reflex-proof/harness/dev_arms.py:43-50` (a `MOD` arm: hooks as `BEST`,
  `--plugin-dir`, the `predict` grant issued as the human step), `dev_v9.py:57-65, 515` (the
  verdict loop over D1–D4 for `MOD` against `A0`), the findings file under
  `harness/results/<date>-mod-dev/`.
- **Acceptance.** D1–D4 from `docs/launch-gate-protocol.md:247-252`: requests ≤ 0.85 × A0 and
  fewer in at least 4 of 6 blocks; no more hidden failures than A0; zero necessary verification
  skipped; covered repeats in at most 2 of 6 positives. Reported as development, "not a launch
  statistic".
- **Estimate.** 3 days plus model spend (≤ $0.40 per session, 36 sessions). **Depends on.** C4,
  E1.

#### E3 · Registered confirmation

- **Scope.** Only after E2 passes twice in development: a pre-registered held-out run per
  `docs/launch-gate-protocol.md` on new evaluation groups, three arms in paired blocks, with the
  quality rule (`QUALITY_MARGIN = 1`, G5) and the one-sided 95% bound ≤ 0.90. Outcome recorded in
  `evals/reflex-proof/LAUNCH-STATUS.md`.
- **Acceptance.** The protocol's `pass`; otherwise the finding is recorded and Layer 4 stays
  behind its grant as experimental.
- **Depends on.** E2.

## 4. Sequencing

```text
A1 ─┬─▶ C1 ─▶ C2 ─▶ C4 ─▶ E2 ─▶ E3
    │     └──▶ C3 ─┘
A2, A3, A4, A5 ──▶ B3
S9 (Q1,Q2,Q3,Q6) ─▶ B1 ─▶ B2 ─▶ B3 ─▶ D5
D1 ─▶ D2 ─────────────────────▶ D5
D3 ─▶ D4 ─────────────────────▶ D5
B1 ─▶ E1 ─▶ E2
C1, B2 ─▶ C5
```

Workstream A (7 days) and D1–D4 (15 days) need no Claude Code and can run in parallel with the
spike. B (11 days) follows the spike's first four questions. C (17 days) follows Q9 and A1. E
(5 days plus spend) closes each layer. About eight engineer-weeks of code before the registered
run, which is the design's own estimate order of magnitude; the critical path is S9 → B1 → C4 →
E2.

## 5. Testing strategy

| Level | What | Where | Runs |
| --- | --- | --- | --- |
| Unit, Rust | Every new type, bound and template: `Candidate`, `Step::Skill`, `Event::TurnStep`, `GrantMode`, the dedup ring, `PreNote.fresh`, `verified.json` | each crate's `#[cfg(test)]`, `adage-runtime/src/tests.rs` | `cargo test --workspace --locked`, PR CI |
| Golden and parity | The exact bytes `adage hook claude <event>` prints for the scenario, now with `PreToolUse`, `TurnStep` and a `skill` directive; `adage-exec hook` identical with no grants | `crates/adage-cli/tests/golden/claude-code.txt`, `hooks.rs::claude_scenario`, `adage-exec/tests/parity.rs` | PR CI; re-blessed with `ADAGE_BLESS=1`, diff reviewed |
| Fail-open | Fault injection (`ADAGE_TEST_FAULT=panic`), a missing binary, a malformed `TurnStep`, a 9 MiB payload: exit 0, nothing printed, one `fail-open.log` line | `hooks.rs` hostile-payload tests | PR CI |
| Latency | `hook_latency` scenarios: `pretool_no_match`, `turn_step_nothing_pending`, `turn_step_predicts`, `batch_no_match` with candidates compiled | `crates/adage-cli/examples/hook_latency.rs` | Scheduled CI (release build), and before each A/C/D merge on the developer's machine, numbers in the PR |
| Replay gate | `fast_path_changes_no_decision` over every dataset, `TurnStep` and `noticed` included; the route-fidelity gates unchanged | `crates/adage-eval/tests/gates.rs` | `evals.yml` weekly, and `cargo test -p adage-eval --test gates` before merging D1, D2, C1 |
| Simulator | Twelve seeded Claude sessions, held-out replay in both arms with `PreToolUse`, `TurnStep`, `SkillPrompt`; control silent; `graph --json` counts `predicted` and `noticed` | `crates/adage-sim`, `crates/adage-cli/tests/harness_sim.rs` | PR CI; `cargo run -p adage-sim` when fixtures change |
| Mod, unit | Every hook with stubbed `ask`, `$.tool.check`, `$.process.spawn`; every render site mounted on `terminal` and `desktop`; the `turn.step` generator's three chunks; nothing registered when `isInteractive` is false | `plugin/claude-code/hooks/*.test.ts(x)` | `claude plugin test` in the `plugin` CI job, pinned CLI version |
| Mod, static | `claude plugin validate --strict` output committed as `VALIDATE.txt`; `check-plugin.mjs` rules for modules and types | `scripts/check-plugin.mjs`, CI `plugin` job | PR CI |
| Mod, live | S9's probes: coexistence, surfaces, silence in `-p` and control, hosts, the skipped request, skills | `spikes/S9-claude-mods/` | Once per question, results committed |
| End to end, headless | The demo runner's `mod` arm against `control` and `adage` on the six held-out items: `requests`, `cost_usd`, `passed`, `stale_generated`; identical transcripts with the mod observing only (B2) | `demo/run.py`, `demo/report.py` | Before merging B2, C3, C4; numbers in the PR |
| Development gate | D1–D4 for the `MOD` arm against `A0` on the reflex-proof harness | `evals/reflex-proof/harness/dev_v9.py` | Before E3 |
| Registered | The launch gate protocol on new groups | `evals/reflex-proof/` | Once |

Three conventions hold throughout: every number in a findings file comes from a script in the
same folder (the spikes' rule); a changed golden is a behaviour change and is called out in the
PR; and `NO_COLOR`, `ADAGE_ARM`, `ADAGE_AUTO_LEARN=0` and a fresh `ADAGE_HOME` are set in every
integration test as `first_run.rs`'s `World` does.

## 6. Acceptance criteria, rolled up

| Milestone | Chunks | Done when |
| --- | --- | --- |
| **M0 · Spike** | S9 | Findings for Q1–Q13; Q1, Q2, Q3, Q6 pass before B starts; Q9 before C4 |
| **M1 · Foundation** | A1–A5 | One decision per `tool_use_id`; `duration_us` on every record and the doctor check; `watch --follow --json` streams; `ledger note` prices from the harness; `tokens.ts` generated and checked; all goldens unchanged except the added `PreToolUse` step |
| **M2 · See it** | B1–B3 | `1 mod active · adage`; `-p` transcripts identical with the mod observing; every site drawn on both surfaces in `plugin test`; the pane fed by the binary with no dropped rows; `VALIDATE.txt` names no person-only command |
| **M3 · Trust it** | C1–C3 | `TurnStep` predicts only a delivered directive with a `predict` grant, never twice in a row, never in control, nothing without adage-exec; the grant round-trips through delegate, status, revoke and lapse; a fresh receipt answers the repeated test as its result in the demo with no process run |
| **M4 · Skip it** | C4, C5 | S9 Q9 10 of 10; the sim predicts at the registry turn in treated and nothing in control; `adage:procedures` byte-identical across a session; the demo's `mod` arm runs end to end |
| **M5 · Notice and learn in view** | D1, D2, D5 | A candidate from the sim's history in the index; `noticed` once per session in both arms; replay gate green; the line drawn under the batch; p95 within +0.3 ms |
| **M6 · Skills** | D3, D4, D5 | A `skill` route learned from six synthetic sessions and delivered at prime; the Skill tool predicted at `turn.step` under a `predict` grant; `verified.json` written by `adage train` and read back by `SkillPrompt`; Q13's three entry points |
| **M7 · Evidence** | E1–E3 | `requests` in the demo report; D1–D4 pass twice in development; the registered run's outcome recorded, whatever it is |

## 7. Risks, and what the plan does about them

- **The API moves.** Mods are early access; the public declaration file is in the Claude Code
  repository. CI pins the CLI version for `validate` and `test`; S9 Q8 measures churn; the
  command hooks remain the floor so a broken mod degrades to today.
- **A fabricated assistant turn hurts quality.** Layer 4 ships only behind a `predict` grant,
  with the two bounds, and only after E2's D1–D4 and E3's quality rule; the control arm records
  `would_predict` so the saving is measured against a baseline from day one.
- **The learner relearns Adage's own calls.** The `adage_` id prefix and `Call.by` mark them
  (C1), the importer excludes them (§2.1.3), and S9 Q5 may add the transcript's `origin`.
- **Candidates cost the fast path, and the learner.** Their words join `Triggers`; D1's
  acceptance is a measured +0.3 ms p95 bound, the same bound G4 held itself to. The lower
  alphabet floor widens mining; D1 bounds learn time at 1.3× and keeps the incremental-equals-
  full property test. A candidate net of its own keeps a relearn from resetting the routes'
  context state mid-session.
- **Old indexes and recorded datasets.** Every new `Index` field is `serde(default)`; the
  replay gate loads a recorded index from before any of this, and about twenty recorded indexes
  under `evals/datasets/` must keep parsing. D1 and C1 run `cargo test -p adage-eval --test
  gates` before merging.
- **Noise.** `noticed` is sequences only, once per candidate per session, off with one
  `userConfig` row; Q12 reads the recording with a person.
- **Person-only commands from a pane.** The mod never runs `review`, `delegate`, `install` or
  guarded `config set`; `VALIDATE.txt` is the proof in review, and `AGENT_ENV` refuses them at
  run time regardless.
- **Two plugins, one module.** `plugin/claude-code-exec` ships the identical module with the
  other binary name; `check-plugin.mjs` diffs the two module trees and fails on drift.
