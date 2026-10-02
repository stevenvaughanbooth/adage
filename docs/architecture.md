# Architecture: components and boundaries

This is the living map of Adage's components: what each one owns, what it may depend on, and the
ports where it meets the others. The rules here are enforced by tests (`xtask/src/layers.rs`), and
CI uses the same dependency graph to test only what a change can affect
(`xtask/src/affected.rs`). [Adage — Architecture](Adage%20—%20Architecture.md) is the older,
numbered design record; where the two differ about structure, this page is current.

## The components

```text
            ┌──────────────────────── harnesses ────────────────────────┐
            │  Claude Code · Codex · OpenCode: hooks, plugins, installs │
            └───────┬──────────────────────────────────────────┬────────┘
       live events  │                                          │ history on disk
                    ▼                                          ▼
    ┌──────── live interaction ────────┐       ┌──────────── ingestion ────────────┐
    │ adapter → Event → decide → render │       │ TraceSource → Format → Session    │
    │ (adage-hook, adage-runtime)       │       │ (adage-ingest)                    │
    └───────────────┬──────────────────┘       └─────────────────┬─────────────────┘
                    │ reads the index                            │ canonical sessions
                    ▼                                            ▼
            ┌──────────────── store: tracks, index, state ───────────────────┐
            │ (adage-tracks, adage-store)        ◄── writes ── learning engine │
            └────────────────────────────────────────────────── (adage-learn,│
                    ▲                                              adage-graph)
                    │ reads and decides                                       │
            ┌───────┴────────── application / API ───────────────┐
            │ status, reflexes, candidates, promote (adage-cli)   │
            │ CLI · TUI · `adage map` HTTP                         │
            └─────────────────────────────────────────────────────┘
```

All of them speak one **canonical model** (`adage-trace`): a tool [`Call`], a live [`Event`], a
recorded [`Session`]. Each harness is translated into it exactly once, at the edge; nothing past the
edge knows a harness's format.

| Component | Crates | Owns | Input → output |
| --- | --- | --- | --- |
| **Canonical model** | `adage-trace` | `Call`, `Event`, `Session`, `Harness` and its capabilities, how Adage's own text reads in a trace (`wire`) | — (types only, no I/O) |
| **Ingestion** | `adage-ingest` | Trace sources, one decoder per history format, repository identity, the parse cache, batches for a learn run | trace bytes → `Session` |
| **Learning engine** | `adage-learn` (orchestration, `learn_sessions`), `adage-graph` (mining), `adage-instincts`, `adage-intent` | Mining, scoring, selection, compiling routes, versioning tracks | `Session`s → tracks and the index |
| **Live interaction** | `adage-runtime` (decisions), `adage-hook` (the pipeline) | Matching the index against a live session, obligations, rendering directives | `Event` + state + `Policy` → `Decision` |
| **Store** | `adage-tracks` (tracks, index, reviews, proposals, `layout`, and the `matching` primitives learner and runtime share), `adage-store` (state, spools, config, locks) | The materialized output and every file format under `$ADAGE_HOME` | — |
| **Application / API** | `adage-app` (review queue and decisions), `adage-cli` (the rest, and the front ends) | Status, reflex and candidate listings, promotion, the map's HTTP API | requests → typed results |
| **Harnesses** | `adage_trace::Harness`, `adage-hook::harness` (live adapters), `adage-ingest::{claude,codex,opencode}` (history formats), `adage-cli` install and detect, `plugin/` | Native payloads, native histories, installs | native ↔ canonical |
| **Execution** | `adage-exec`, `adage-safety` (the independent oracle every delivered block is re-checked by) | Running approved routes in a sandbox | — |

## Layers

A crate links only crates in strictly lower layers. Tests (dev-dependencies) are free.

| Layer | Crates | Role |
| --- | --- | --- |
| 0 | `adage-core`, `adage-sign`, `adage-term`, `adage-store`, `adage-sim`, `xtask` | Kernel primitives: the shell normaliser and command table, signing, terminal tokens, file primitives |
| 1 | `adage-trace`, `adage-intent` | The canonical trace model; the intent model |
| 2 | `adage-tracks` | The reflex model (tracks, the index) and the matching primitives the learner and the runtime share |
| 3 | `adage-ingest`, `adage-graph`, `adage-runtime`, `adage-instincts`, `adage-safety` | Engines. Siblings never link each other: ingestion knows no learning, the runtime knows no learner |
| 4 | `adage-pack` | Distribution, and the record of how instincts were bound |
| 5 | `adage-learn` | Learning orchestration: ingest → mine → write tracks and the index |
| 6 | `adage-hook`, `adage-eval` | The live pipeline; offline evaluation |
| 7 | `adage-exec` | Execution |
| 8 | `adage-app` | The application layer: the operations front ends call |
| 9 | `adage-cli`, `adage-train` | Binaries: argument parsing, rendering, the TUI, the HTTP server |

Some edges are forbidden even when the layers allow them (`FORBIDDEN` in `xtask/src/layers.rs`).
Ingestion never links tracks or the store, the runtime never links the store, the live hook
never links the learner, and execution never links the learner or the eval harness. Violations
would be listed as `EXCEPTIONS`, each with the work that removes it; today there are none. The test fails on any violation that is not listed, and on a listed one that has been
fixed, so the list can only shrink.

## Ports

Each component meets the outside world through a small trait. Each trait has a local
implementation and can be swapped in tests or for a remote deployment.

### Ingestion (`adage-ingest`)

```text
TraceSource ──bytes──► Format (sniffed) ──► Session
                          │
                          └── Locator: which repository the recorded directory belongs to
```

- **`TraceSource`**: `list()` the traces and `open()` one as bytes. Local sources are `Files`,
  `ClaudeProjects` and `CodexSessions`; `Memory` serves tests and stands in for a fetched remote
  batch. A team trace store (object storage, an HTTP collector) implements the same two methods.
- **`Format`**: one per harness history format. `sniff(head)` recognises it from the first bytes,
  and `decode(reader)` produces the `Session`. The registry (`format::FORMATS`) replaces the old
  content sniffing that was spread across the learner and the savings ledger.
- **`Locator`**: the one place a decoder may need this machine. `LocalCheckout` finds the git
  checkout and its key. `AsRecorded` trusts the trace alone, which is how traces collected on
  other machines decode with no local checkout (tested in `source.rs`). A batch takes its locator
  in `batch::Options`, and the parse cache keeps each locator's parses apart.

Day 0 and live learning use the same path. `adage init` and `adage condition` ingest a whole source
(`ingest`); a session's Stop hook names its trace, and the learner decodes it through the same
`Format` (`ingest_one`, `read_session`).

### Live interaction (`adage-runtime`, `adage-hook`)

- **`Adapter`** (`adage-hook::harness`): `translate` a native payload into an `Event`, and `render`
  an `Output` back into the bytes the harness expects.
- **`decide(ctx, state, event) -> Decision`**: the runtime's single entry point. The hook does
  the I/O around it: reading the index, locking state, appending spools.
- **`Checkout`** (`adage-runtime::checkout`, on `Ctx`): everything a decision reads of the working
  tree, which is a card's orientation and whether a repository-path slot names a file. `Disk` reads this
  machine's checkout; `Unread` reads nothing (a replay, a remote service). Nothing else in the
  runtime touches the filesystem (tested in `tests.rs` with a checkout that exists only behind
  the port).

### Learning engine (`adage-learn`, `adage-graph`)

- `adage_graph::learn(sessions, params) -> Learned` is pure: the same sessions in, the same
  routes out (`tests/determinism.rs`).
- `adage_learn::plan` turns sessions into planned tracks. `learn_sessions_into` materializes
  them through a **`Store`** (`adage_learn::store`): tracks, instinct bindings, follow-judge
  weights, then the index, compiled once after every row. `Files` is `$ADAGE_HOME`'s layout. A
  team service implements the same methods, and a test learns into a store held in memory with
  nothing written to disk (`tests/remote_sessions.rs`). What was learned before (earlier tracks,
  review decisions, proposals) is still read from the tracks directory.

## Where we are

| Gap | Status |
| --- | --- |
| Canonical types split across the runtime and the learner; learner importers linked the live runtime | **Done**: `adage-trace` |
| Trace parsing inside the learner; content sniffing duplicated; repository identity probed the local disk | **Done**: `adage-ingest`, with `TraceSource`, `Format` and `Locator`; traces from another machine decode and learn with no checkout here |
| The learning engine read transcript files itself | **Done**: `learn_sessions` takes canonical sessions; `adage_ingest::batch` reads local history for day 0 and live learning alike |
| No enforced layering | **Done**: `xtask/src/layers.rs`, no exceptions |
| CI tests every crate on every change | **Done**: `cargo xtask affected` |
| `decide()` read hidden thread-local switches; the hook linked the learner; execution linked the eval harness | **Done**: `Policy` on `Ctx`; read-side helpers in `adage_tracks::layout` and `adage_pack::binding`; the oracle in `adage-safety` |
| Promotion rules lived in the CLI binary, and the TUI skipped the tracks lock | **Done**: `adage_app::review`, which takes the lock and needs a `Person` |
| The map API's JSON could drift from what the web map is tested with | **Pinned**: a contract test against the web map's fixtures |
| Harness names compared as strings in about 30 places | **Done**: `Harness` and `Capabilities`; behaviour by capability |
| Status, listings and the map's read model still live in the CLI as untyped JSON, in three shapes for one reflex | Next: move the read model into `adage-app` with typed results. Unifying the shapes changes the CLI's `--json` output and the web map's types, so it needs a decision |
| Install and detection kept separate tables of where each harness's registration lives, and had drifted (`CLAUDE_CONFIG_DIR`) | **Done**: `adage-cli/src/placement.rs`, one table per `Harness` for both |
| The learner wrote files directly | **Done**: every write goes through `adage_learn::store::Store` |
| Learning reads its earlier results from the tracks directory, and reprocesses all history each run (the parse cache makes re-reading cheap, not the mining) | Next: the store's read side (earlier tracks, decisions, proposals) as the same port, then incremental mining over new sessions only. Mining is pure over sessions (`adage-graph`'s determinism test), so incremental mining needs mergeable counts, not a new boundary |
| `decide()` read the checkout directly for card orientation and repository-path contracts | **Done**: the `Checkout` port on `Ctx` |
| Simulator-fixture tests decoded through this machine's checkouts | **Done**: the learner's fixture tests decode `AsRecorded`; the CLI end-to-end tests, which run the binary here, state their one precondition (`adage_sim::assume_no_checkout_at_cwd`) |

## Testing by component

- Each crate's tests must pass on their own: `cargo test -p <crate>`.
- Decoders are tested from bytes (`adage-ingest`), so no harness install or local checkout is
  needed.
- The runtime is tested as a function of index, state and event (`adage-runtime/src/tests.rs`).
- The learner's end-to-end tests read fixtures through ingestion; the graph engine's tests build
  sessions in memory.
- Tests that read files outside their crate declare them in `xtask/src/affected.rs` (`READS`), so
  CI knows to rerun them when those files change. A test there scans every crate's sources and
  fails on an undeclared read.

[`Call`]: ../crates/adage-trace/src/lib.rs
[`Event`]: ../crates/adage-trace/src/lib.rs
[`Session`]: ../crates/adage-trace/src/session.rs
