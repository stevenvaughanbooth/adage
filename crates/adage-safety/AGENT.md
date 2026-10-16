# adage-safety

The safety oracle: an independent re-check that every directive, stop request and execution data
block Adage delivers is exactly one of its fixed, allowed forms. It deliberately duplicates
checks made in `adage-tracks` and `adage-runtime`, so that the eval, the property tests and
`adage-exec` can hold those crates to a judge that shares no code with them.

## Where it sits

- Layer 3 (engines) in `xtask/src/layers.rs`, beside `adage-runtime`, which it re-checks.
- Links: `adage-core` (`repo::is_safe_relative`) and `adage-tracks` (`Route`, `Step`, `Trigger`,
  `Contract`, `Strength`, `ceiling`, `interlock`, `card`, `advice`) only.
- Linked by: `adage-eval` (re-exported as `adage_eval::safety`, used in `score.rs` and
  `tests/prop_tracks.rs`), `adage-exec` (`check_data_block` and `DataType` in `hook/prefetch.rs` and
  its tests).
- Must never link `adage-runtime` or anything that renders what it checks (its `Cargo.toml`
  says so; siblings at layer 3 cannot link each other). `adage-runtime` is a dev-dependency
  only: the tests render with the runtime and hold the result to the oracle.

## What's inside

Everything is in `src/lib.rs`:

- `Violation { route, reason }`.
- `check_directive(route, text, root, canaries)`: a directive body is one of the fixed templates
  (`T_LOOKUP`, `T_INTENT`, `T_FINISH`, `T_RUN`, `T_FAILED`, `T_READ`, `T_AVOID`, `T_SEQUENCE`,
  `T_ADVICE`, or a card's own rendering), with a bound command that is the route's template
  under its slot contracts, safe globs, files inside the repository, no canary, no invisible
  character, and a strength within `ceiling`.
- `check_wrapper(route, wrapped, root, canaries)`: the `<adage-reflex …>` wrapper's attributes
  and footer are the runtime's for the route's origin (learned, authored, instinct), then the
  body passes `check_directive`.
- `check_interlock(decisions)`: no directive after or beside an inhibit that avoids its command;
  no two fires directing different commands at one decision point (`interlock::avoids`,
  `interlock::two_fires`).
- `check_stop(route, text, canaries)`: the finish-line stop reason is `T_STOP`.
- `check_data_block(route, block, fields, root, canaries)` and `DataType`: the `<adage-data …>`
  block (`T_DATA`), its byte count, no nested adage tag, typed payloads in Adage's canonical
  JSON with declared fields passing their types, raw payloads free of instruction-shaped text
  (`INSTRUCTION`).
- `is_invisible(c)`: the invisible and direction-changing characters every check refuses.

## How it connects

- `adage-runtime::render` produces the texts; the regexes here are a second, hand-written
  description of the same templates. The unit tests render through `adage_runtime::render`
  (`route_text`, `stop_reason`) and assert the oracle accepts them and rejects tampering.
- `adage-eval` scores every replayed decision with `check_wrapper`, `check_stop` and
  `check_interlock`, and its property tests generate routes and hold the runtime to the oracle.
- `adage-exec` calls `check_data_block` on every E1 block before delivering it; a violation
  withholds the block.

## Changing it

Invariants to keep:

- Independence: never import the runtime's constants or helpers to build a regex here; write
  the expectation out. If the oracle and the renderer agreed by construction it would prove
  nothing.
- Fail closed: an unrecognised text is a violation. New templates in `adage-runtime::render`
  need a new regex here in the same change, plus a test that renders through the runtime and
  passes, and one that tampers and fails.
- Commands are plain argv (`[A-Za-z0-9 _./:=@,+-]`), at most 200 bytes, never a path outside
  the repository; files must resolve inside `root` once symlinks are followed, when a root is
  given.
- `DataType` is the oracle's own copy of the built-in output types; it is duplicated on purpose
  and must not be replaced by `adage-pack::types`.
- Canaries are caller-supplied strings; every check scans for them.

Tests: `cargo test -p adage-safety` (unit tests in `lib.rs`, with `tempfile` for repository
roots). Then `cargo test -p adage-eval -p adage-exec`, whose scoring and E1 delivery depend on
these checks. The crate reads nothing outside itself.

Pitfalls: the `EXIT` and `EXIT_LINE` clauses differ (the exit sentence inline after a space, or
on its own line) and each template uses one of them; a template that moves the clause breaks
the match. The `T_FINISH` basis sentences encode origin (`declares this` for instincts,
`written instructions require it` for authored routes, the rest for learned): `check_directive`
cross-checks them against `Route::is_instinct` and `evidence.origin`. `T_ADVICE` is a fixed
array of advisory sentences with matching template ids; adding a template means growing both.
