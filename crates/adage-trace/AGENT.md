# adage-trace

The canonical trace model: the tool `Call`, the live `Event` and the recorded `Session` every
harness is translated into exactly once, plus `Harness` with its capabilities and the `wire`
format of Adage's own text inside a trace. Types only; it does no I/O.

## Where it sits

- Layer 1 in `xtask/src/layers.rs` ("the canonical trace model; the intent model").
- Depends on `adage-core` only (plus serde, serde_json). It may link layer 0 crates and nothing
  else; its `Cargo.toml` and `lib.rs` say it stays on `adage-core` alone with no I/O.
- Linked (`[dependencies]`) by `adage-cli`, `adage-graph`, `adage-hook`, `adage-ingest`,
  `adage-learn`, `adage-runtime`, `adage-tracks`.
- No `FORBIDDEN` edge names it, but it must never depend on ingestion, the learner or the
  runtime: they all depend on it.

## What's inside

- `lib.rs`: `Call { tool, input, output, failed }` (serde), and `Event` (SessionStart, Prompt,
  Batch, Failure, Stop, SessionEnd; `Debug + Clone` only, not serde) with `Event::name()`.
- `harness`: `Harness` (ClaudeCode, Codex, OpenCode), `Harness::ALL`, `id()`, `hook_name()`,
  `display()`, `short()`, `capabilities()`, `capabilities_with_mod()`, `parse()`; `Capabilities`
  (the command hooks' four and the seven the Claude Code mod adds); and the one table of native
  hook event names: `Harness::events()` (native name to `EventKind`), `event_kind(native)`,
  `mod_only_events()`, with `EventKind` (`decided()`: the runtime event and record `kind`;
  `mod_only()`). The hook's dispatch, adapters and press, and `adage_runtime::rebuild::kind_of`,
  read it; a native name means the same in every harness that sends it.
- `session`: `Session` (id, root, repo, model, day, prompt, steps, steered, directed, influence,
  outcome, harness, prompts, ending), `StepObs` (turn, key, norm, call, rel_path, prompt,
  project_pytest_root_cwd), `PromptObs`, `Ending`, `Directed`, `MAX_PROMPT_BYTES`, `MAX_PROMPTS`,
  `FINISH_LINE_STOP`, the `CLAUDE_CODE`/`CODEX`/`OPENCODE` id constants, and
  `Session::{proxy_outcome, prompt_count, first_edit, discovery}`.
- `wire`: `FINISH_LINE`, `NOT_FROM_USER`, `finish_line`, `is_finish_line`, `stop_gate_failed`,
  `Named { commands, avoided, files }` and `named(body)`.

## How it connects

- `Session` flows out of `adage-ingest` (the `claude`, `codex` and `opencode` decoders build it;
  `adage_ingest::signals::call_steps` builds each `StepObs`) and into `adage_graph::canonical` and
  `adage-learn`. It derives `Serialize`/`Deserialize` because `adage_ingest::cache` stores parses.
- `Event` is produced by the harness adapters in `adage-hook::harness` and consumed by
  `adage_runtime::decide`. `Call` appears in both an `Event::Batch` and a `StepObs`.
- `wire` is written by `adage_runtime::render` and read back by `adage-ingest`'s decoders and the
  hook, so a session never counts Adage's own direction as the agent's or the person's.
- `StepObs::norm` is an `adage_core::Normalized`; `Session::proxy_outcome` reads
  `adage_core::Effect::Verify` and `CanonicalAction::Exec`.
- Every place that behaves differently per harness matches on `Harness` or reads its
  `Capabilities`, never a name string. See [docs/architecture.md](../../docs/architecture.md).

## Changing it

- Run `cargo test -p adage-trace` (unit tests only, in `harness.rs`). Then run the crates that
  observe the change: `cargo test -p adage-ingest -p adage-runtime -p adage-graph -p adage-learn
  -p adage-tracks -p adage-hook`, or `cargo xtask affected`. It reads no fixtures.
- Adding a harness is adding a `Harness` variant: the compiler then lists every exhaustive
  match to update (`capabilities`, `events`, the adapters, install placement).
- Adding a hook event is a row in the harness's table in `harness.rs` (and an `EventKind`
  variant if it is a new kind): see `docs/claude-mod/impl/structure-hook.md`, "How to add a hook
  event". Keep `id`, `hook_name` and
  `short` distinct (`names_are_distinct` tests it) and `parse` round-tripping.
- Adding or changing a field of `Session`, `StepObs`, `PromptObs`, `Ending` or `Directed`: give it
  `#[serde(default)]` so older cached parses still read, and bump
  `adage_ingest::cache::SESSION_FORMAT` (its doc comment lists what each format added).
- `wire` has a writer (`adage_runtime::render::route_text`, `stop_reason`) and a reader
  (`named`). Change both together and run the runtime's round-trip tests.
- `Session::harness` is a `String` holding `Harness::id()`, not the enum; compare with the
  constants in `session` or `Harness::parse` it. `Session::prompt` is also `prompts[0]`.
- Keep this crate free of I/O and of any harness format knowledge; that belongs at the edge
  (`adage-ingest`, `adage-hook::harness`).
