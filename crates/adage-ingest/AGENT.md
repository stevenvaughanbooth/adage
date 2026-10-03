# adage-ingest

Ingestion: read traces from a source, recognise each harness's history format from its first
bytes, and decode it into the canonical `adage_trace::Session`. Day 0 (`adage init`,
`adage condition`) and live learning (the trace a Stop hook names) both go through it.

## Where it sits

- Layer 3 in `xtask/src/layers.rs` (engines: ingestion, the graph learner, the live runtime,
  instincts, safety).
- Depends on `adage-core` and `adage-trace` (plus serde, serde_json, regex, once_cell, sha2).
- Linked (`[dependencies]`) by `adage-learn` and `adage-hook`; `adage-graph` uses it as a
  dev-dependency only (`tests/support`).
- Forbidden edges (`FORBIDDEN`): `adage-ingest -> adage-tracks` ("ingestion produces sessions,
  not tracks") and `adage-ingest -> adage-store` ("ingestion reads traces, never Adage's own
  state"). Layer 3 siblings (`adage-graph`, `adage-runtime`, `adage-instincts`, `adage-safety`)
  are not lower, so it cannot link them either.

## What's inside

- `source`: trait `TraceSource { list() -> Vec<TraceRef>; open(&TraceRef) -> Box<dyn Read> }`;
  `TraceRef`; local sources `Files`, `ClaudeProjects`, `CodexSessions`, and `Memory` for tests
  and fetched batches; `ingest` (walk a source) and `ingest_one` (one trace).
- `format`: trait `Format { harness(); sniff(head); decode(reader, &Decode) }`; `Decode<'a>
  { table, locator, repo_override, subagent }` with `Decode::local`; the registry
  `FORMATS: [&dyn Format; 3]` in the order tried (codex `Rollout`, opencode `Export`, claude
  `Transcript`); `detect`, `detect_path`, `decode_trace`, `read_session`, `read_trace`;
  `HEAD_LINE`, `HEAD_MIN`.
- `locate`: trait `Locator { id(); locate(cwd, explicit) -> Located; remember(root, repo) }`;
  `LocalCheckout` (this machine's git checkout) and `AsRecorded` (the trace alone).
- `claude`, `codex`, `opencode`: one decoder each (`decode`, `parse_*`, discovery helpers,
  the `Format` impl type).
- `signals`: what decoders share: `Signals` (directives found in the trace, `steered`,
  `directed`, `anchor`), `call_steps` (a `Call` into `StepObs`), `asks_for_help`, and the
  crate-private `Prompts`.
- `batch`: `Options { repo_override, only_repo, cache, locator }`, `Batch { sessions, read,
  steered, cache_hits, cache_misses }`, `read(traces, &opts)`.
- `cache`: the per-transcript parse cache (`Cache::open/read/save`, `Read::{Session, Nothing,
  Elsewhere}`, `SESSION_FORMAT`). `scope`: picking one repository's local traces before anything
  is parsed (`checkouts`, `claude_slug`, `recorded_cwd`, `claude_transcripts`, `inside`).

## How it connects

- In: bytes from a `TraceSource`. Out: `adage_trace::Session`, with each `StepObs::norm` from
  `adage_core::shell::normalize_shell` or `adage_core::tools::normalize_tool` through
  `adage_core::CommandTable`. Decoders read `adage_trace::wire` to recognise Adage's own text.
- `adage-learn` calls `batch::read` for a learn run and re-exports `cache`, `codex`, `opencode`,
  `scope`, `read_session`, `claude::{decode, discover, discover_project, parse_transcript}` and
  `signals::{asks_for_help, call_steps, Signals}`. `adage-hook` uses `scope::checkouts` and
  `scope::count_claude_sessions`.
- The cache directory arrives as a path in `batch::Options::cache`; the caller computes
  `$ADAGE_HOME/cache/sessions` because this crate never links `adage-store`.
- Ports and the whole flow: [docs/architecture.md](../../docs/architecture.md), "Ingestion".

## Changing it

- All tests are unit tests inside the modules: `cargo test -p adage-ingest`. Then run the readers:
  `cargo test -p adage-learn -p adage-graph -p adage-hook` (or `cargo xtask affected`). It
  reads no fixture outside the crate; tests build traces in memory and use `tempfile` for
  directories (the `Memory` source exists for this).
- Adding a harness history: implement `Format`, add it to `FORMATS`, and keep `Transcript` last
  (its `sniff` accepts anything). `format.rs` tests check each format is recognised from its
  first bytes, that near misses are not, and that the registry matches each decoder;
  `source.rs` checks every format decodes from memory without the filesystem.
- A decoder may reach this machine only through its `Locator`
  (`as_recorded_reads_nothing_but_the_trace`). Traces from other machines decode `AsRecorded`.
- A cache hit must equal a fresh parse: the repository key is re-derived and outcomes applied
  after reading; the manifest is stamped with the build and `SESSION_FORMAT`. Any change to what
  a parse records (a `Session` or `StepObs` field) bumps `SESSION_FORMAT`.
- `scope` must never open another repository's transcript: containment is by path components,
  decided before parsing. Keep the two cheap tests in its module doc in that order.
- Sub-agent traces are skipped by each decoder (Claude Code `subagents/`, Codex
  `source.subagent`, OpenCode `parentID`); `Decode::subagent` is set from the path by `read_trace`.
- The Codex rollout format is read from the openai/codex source (tag noted in `codex.rs`), not a
  documented interface; compressed `.jsonl.zst` rollouts are not read. OpenCode is read from
  `opencode export` output, never its SQLite store.
