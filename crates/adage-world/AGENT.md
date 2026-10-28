# adage-world

The situational awareness layer's decisions: given what the person was doing outside the session
(`adage_trace::world`: the activity timeline, the artifacts it names, the calendar), say what a
prompt refers to (`resolve`), what changed since a session last looked (`classify`), and what the
calendar means for the session (`schedule`). Pure logic, like `adage-runtime`: the hook, the
`adage world follow` child and the Claude Code mod do every piece of I/O around it, and `now`, the
embedder and the parameters are inputs on `WorldCtx`, never read here.

## Where it sits

- Layer 3 (engines) in `xtask/src/layers.rs`, beside `adage-runtime`, `adage-ingest`,
  `adage-graph`, `adage-instincts` and `adage-safety`. Siblings never link each other.
- Links: `adage-core` (nothing yet beyond the workspace rule), `adage-trace` (the world types,
  re-exported), `adage-intent` (`cosine`, `dequantize`, `normalize`).
- Linked by: `adage-hook` (the `Prompt` path's hint, the mod's `WorldSnapshot`, `WorldDelta` and
  `WorldSchedule` asks), `adage-cli` (`adage world follow`'s interrupt classification, `adage
  world eval`).
- Forbidden: `adage-world -> adage-store` ("the resolver decides; the hook and the follow child do
  the I/O"). It is in the `no_spawn` scan (`crates/adage-exec/tests/no_spawn.rs`): nothing here
  starts a process.

## What's inside

- `lib.rs`: `WorldCtx` (now, the local offset, artifacts, events, blocks, the embedder, `Params`),
  `resolve(ctx, prompt) -> Decision`, the re-exports.
- `params.rs`: `Params`, every tunable as a field with the design's initial value: the four
  weights, `tau_min`, `threshold`, `margin`, the windows, the token budgets, the scheduler's bounds.
- `gate.rs`: `gate(prompt, artifacts) -> Option<Signal>`, the trigger gate (demonstratives,
  temporal anchors, a kind named with an article, keys, entities named); `names_its_own` (a path,
  a URL, an `@` mention bypasses the resolver); `is_key`; `names(artifact, words)`.
- `window.rs`: `candidates(ctx)`, the candidate window (touched within 4 h, a meeting ended within
  2 h with its attachments, open pull requests and tickets), most recent first, capped; `is_open`.
- `score.rs`: `score(ctx, signal, cands) -> Vec<Scored>`: `sim` (cosine on the embedder, with a
  lexical floor), `recency` (`exp(-minutes / tau)`), `entity` (named, or the one candidate of the
  kind the prompt names), `kind` (what the verb implies); `implied_kind`.
- `decide.rs`: `decide(ctx, signal, scored) -> Decision` (`Attach`, `Disambiguate`, `None`), the
  design's table: threshold, margin, at most three contenders; `Hint`.
- `render.rs`: the `<adage-world kind="…">` texts (hint, disambiguation, snapshot, delta,
  interrupt, autonomy, finalize), each inside its budget through `text::fit`; every one says it is
  from Adage, not from the user, and none carries an artifact's content.
- `classify.rs`: `classify(ctx, prev: &WorldView, focus) -> Changes`: state changes and new open
  items are deltas (coalesced into one block); a blocking change on an artifact in the session's
  focus, or a block starting within 5 minutes, is an interrupt, at most `max_interrupts` per
  session and never inside a focus block.
- `schedule.rs`: `schedule(ctx, live: &Liveness) -> Plan`: the TTL flip across a predicted gap on
  a heavy session (break-even rule), the autonomy and finalize texts inside a busy block, the
  long-block note, a proposed review slot when briefs wait.
- `text.rs`: `words` (keys and `#412` kept whole), `content_words`, `contains_phrase`, `ago`,
  `until`, `hhmm`, `local_day`, `tokens` (characters / 4), `fit` (drop whole lines to a budget).

## How it connects

- The hook builds a `WorldCtx` from the store (`adage_store::Paths::world_*`), the session's
  state (`WorldView` under `world`) and the loaded intent model, calls `resolve` at `Prompt`
  under the session's state lock, appends the hint after the reflex directives, and writes the
  `injection` record. The mod's asks go through `adage-hook`'s `WorldSnapshot`, `WorldDelta` and
  `WorldSchedule` events, which call `render::snapshot`, `classify` and `schedule`.
- Embeddings: artifacts carry `adage_intent::quantize` of the model's vector for `Artifact::text`
  at ingest; the prompt is embedded with the same model at resolve time. A dimension mismatch
  (another model) scores 0 and the lexical floor carries the term.

## Changing it

- Determinism is the contract (`lib.rs` docs): no wall clock, `BTreeMap` only, ties by id; the
  table-driven cases in `tests/cases/resolve/*.json` run through `tests/cases.rs`, which also
  shuffles every input and asserts the same decision. Add a case there for every new rule.
- Budgets: every rendered text goes through `text::fit`; a new template must too.
- The weights, threshold and `tau_min` are tuned by `adage world eval` against the person's
  labelled prompts, never by hand in a test.
- `cargo test -p adage-world`, then the readers: `cargo test -p adage-hook -p adage-cli`.
