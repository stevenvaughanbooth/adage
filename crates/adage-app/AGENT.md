# adage-app

The application layer: what Adage does for a person, as typed operations with no rendering. The
CLI, the TUI and the `adage map` HTTP API all call these and print what they return; none of them
holds the rules. See `../../docs/architecture.md` ("Application / API").

## Where it sits

- Layer 8 in `xtask/src/layers.rs`: below the binaries, above everything they orchestrate.
- Links: `adage-core`, `adage-store`, `adage-tracks`, `adage-runtime`, `adage-learn`,
  `adage-exec`, `adage-hook`, `adage-term`.
- Depended on by `adage-cli` only (among others `graph.rs`, `review.rs`, `savings.rs`, `sense.rs`,
  `share.rs`, `install.rs`, `proposal.rs`, `history.rs`, `exec.rs`, `ledger.rs`, `map/`,
  `ui/routes.rs`, `main.rs`).
- No `FORBIDDEN` edge names this crate; the layer rule alone forbids linking `adage-cli` or
  `adage-train` (layer 9).

## What's inside

- `review`: the review queue and decisions. `discover` (installed tracks), `queue` (`QueueItem`s
  with typed reasons), `Verb`, `decide` and `decide_within` (take the tracks lock, need a `Person`),
  `decide_reviewed_within` (the same, refused unless the route and its proposal still hash to the
  `revision` the person was shown: `adage ui`), `review_preview` (`ReviewPreview`: that revision,
  the current directive and a proposal's diff and directive), `proposed_route`, `Outcome`,
  `standings`, `waiting_counts`, `judgements` (current-version outcomes per installed reference
  and route, from `graph`). A decision records how the person was proven (`via`) and the version
  its counts belong to (`basis.runtime_version`).
- `person`: `Person`, the proof that a person and not an agent is deciding (`require`;
  `require_or_press` for the `PersonVerb` press sites; `AGENT_ENV`).
- `proposal`: `current`, `applied`: proposed changes to pinned routes.
- `history`: where a harness's local history is read from (`set_source`, a `Person`'s choice
  written to the configuration) and the bounded OpenCode export (`import_opencode`).
- `harness_ledger`: what a session cost as its harness reported it (`adage ledger note`).
- `reflex`: `Reflex::of`, the one JSON shape of a reflex every surface renders; `Decisions`,
  `Live`, `Control`.
- `graph`: the map's read model. `build` returns the `/api/v1/graph` JSON; `live` (keyed by
  installed reference and route), `event` (the live stream's row), `trace` (one whole recorded
  session, `/api/v1/trace`). `graph/measurements.rs` reads every spool once: it ties each delivery
  to the installed reflex by repository, track, version and route, and an outcome to one
  unambiguous delivery in the same session, so a historical, legacy or duplicated identity never
  counts for a current reflex. The Claude Code mod's fields (`MOD_FIELDS`: `predicted`,
  `substituted`, `noticed`, `answered` and their would-be forms) ride on the same events, and a
  substitution counts in its route's `arms.<arm>.substituted`.
- `savings`: the savings estimate against the control arm (`Unit`, `Estimate`, `Summary`,
  `estimate`, `summarise`, `gather`, `refresh`, `read`; gate constants `NEED_CONTROL`, `NEED_TREATED`).
- `evidence`: how a reflex's evidence reads (`describe`, `plain`, `value_line`, `obligation`, `execution`).
- `words`: the lexicon's words for stored values (`strength`, `lifecycle`, `evidence`, `count`, `prose`).
- `error`: `Hint` and `hint`, errors that say what to do next.

## How it connects

- In: tracks and the index (`adage_tracks::io`, `why`), session spools and state
  (`adage_store`), the learner's ledger and value model (`adage_learn`), approvals
  (`adage_exec::approvals`), the rewrites-off marker (`Paths::rewrite_off`, in `graph`).
- Out: plain structs and `serde_json::Value`s. `graph::build` is what `adage graph --json` and the
  map server print; `reflex::Reflex` is the shape of `adage status --json`, `adage review --json`
  and each reflex in the graph.
- `review::decide` is the only way a decision is recorded: it holds `paths.tracks_lock()`, bumps
  the track version, re-validates, writes the track and recompiles the index.

## Changing it

- Invariants: nothing here prints or formats for a terminal (only `words` builds `adage_term::Line`s);
  every trust-changing operation takes a `Person`; nothing outside `reflex.rs` builds a reflex's JSON.
- Tests: `cargo test -p adage-app` (unit tests only, against a temporary `ADAGE_HOME` via
  `tempfile`). Then `cargo test -p adage-cli`: `review.rs`, `graph.rs`, `serve.rs`, `exec.rs` and
  the UI snapshots render what this crate returns.
- Contract test: `graph::tests::the_served_graph_has_the_fields_the_web_map_is_tested_with`
  includes `web/map/e2e/fixtures/busy.json` (declared in `READS`). Adding, renaming or removing a
  top-level or per-reflex field fails it; change `web/map/e2e/fixtures/make.mjs` and the web map together.
- Pitfall: spools written before the lexicon rename spell `holdout` and `derailed`; `graph` and
  `review::judgements` (through `graph/measurements.rs`) read them as the new words. Keep that
  mapping when touching judgement counts.
