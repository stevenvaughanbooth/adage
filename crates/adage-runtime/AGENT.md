# adage-runtime

Hook-time decisions. Given the compiled index, a session's state and one canonical event,
`decide()` says which directives to emit, which earlier directives were followed or ignored, and
whether a Stop should be refused. It is pure logic: the hook (`adage-hook`) does every piece of
I/O around it.

## Where it sits

- Layer 3 (engines) in `xtask/src/layers.rs`, beside `adage-ingest`, `adage-graph`,
  `adage-instincts` and `adage-safety`. Siblings never link each other.
- Links: `adage-core` (shell normaliser, keys, `pattern`, `tokens`, `repo`), `adage-trace`
  (`Call`, `Event`, re-exported), `adage-tracks` (`Index`, `Route`, `Trigger`, `Step`,
  `Contract`, `matching`, `params::RuntimeParams`), `adage-intent` (`cosine`, `dequantize`).
- Linked by: `adage-hook`, `adage-eval`, `adage-exec`, `adage-app`, `adage-cli`. Dev-only:
  `adage-learn`, `adage-safety` (their tests drive the runtime; their libraries never link it).
- Forbidden: `adage-runtime -> adage-store` ("the runtime decides; the hook does the I/O"), and
  `adage-learn -> adage-runtime` (learner and runtime are siblings over the canonical model).
  See [`../../docs/architecture.md`](../../docs/architecture.md).

## What's inside

- `lib.rs`: `decide(ctx, st, ev) -> Decision`; `Ctx` (index, repo, root, checkout, table, embed,
  params, policy); `Policy` (`card_orientation`, `card_checks`, `follow_weights`, `exec_only`,
  `exact_keys`; `Default` is what plain `adage hook` uses); `SessionState`, `Pending`,
  `Obligation`, `Directive`, `Judgement`, `Decision`; `Arm`, `assign`, `assign_arm`;
  `command_key`; `span_kind_label`; re-exports `RuntimeParams`, `CtxState`, `Call`, `Event`,
  `extract_json`.
- `checkout.rs`: the `Checkout` port (`text`, `is_file`, `reader`), `Disk` (this machine),
  `Unread` (nothing readable).
- `context.rs`: a re-export of `adage_tracks::matching` (the symboliser the learner shares).
- `fast.rs`: `skip`, the no-match fast path, with a per-event table in its module doc.
- `orient.rs`: `block` (a procedure card's orientation), `readable`, `shown`, `is_test_file`.
- `pretool.rs`: `Env`, `Pre`, `plan`, `keeps_permissions`, `permission_advice`, `rule_allows`,
  `shell_word`, `rewrite_note` (the `PreToolUse` interpreter rewrite for U11).
- `pytest_case.rs`: `facts`, `TestSource`, `CaseFacts` (what a failing test's source says).
- `rebuild.rs`: `rebuild`, `kind_of`, `MAX_SPOOL_BYTES`, `MAX_RECORDS` (state from the spool).
- `render.rs`: `route_text`, `bind`, `wrap_directive`, `footer`, `stop_reason`,
  `oriented_card_text`, `card_checks_line`, `stop_gate_failed`/`_unknown`/`_notice`,
  `join_finish_lines`, and the E1/E2 renderers `adage-exec` uses (`wrap_directive_e1`,
  `action_applied`, `action_predicted`, `action_failed`, `action_not_run`).
- `tests.rs`: `decide` as a function of index, state, event, policy and checkout.

## How it connects

- In: an `Index` compiled by the learner (`adage-tracks`), a `SessionState` the hook persists
  through `adage-store`, an `Event` an `adage-hook::harness` adapter translated.
- Out: a `Decision`. The hook renders `directives` with `render::wrap_directive`, appends the
  whole `Decision` to the session spool, and `rebuild` replays those records when the state file
  cannot be read. `would_have` is recorded in both arms; `directives` is empty in the control arm.
- `Checkout` on `Ctx` is the only path from `decide` to the working tree: `contract_ok` checks a
  `Contract::RepoPath` slot with `ctx.checkout.is_file`, and `card_text` orients a card with
  `orient::block(ctx.checkout, ...)`. The hook passes `checkout::Disk`; replays pass `Unread`.
  `tests.rs::the_checkout_is_read_only_through_the_port` decides against a checkout that exists
  only in memory.
- `Policy` is built in `adage-hook`'s `respond`: `ADAGE_CARD_CHECKS` and
  `ADAGE_CARD_ORIENTATION` knobs, `adage_tracks::layout::read_follow_weights`, and the
  `Extension::exec_only` list. `exact_keys` is only for `adage-eval`'s fidelity replay.
- `Ctx::embed` is an optional closure; the hook supplies one from `adage-intent`'s model. With
  none, intent routes and cards never match (fail open).
- `adage-safety` is the independent oracle over `render`'s templates; `adage-eval` checks that
  `fast::skip` never changes a decision (`fast_path_changes_no_decision`).

## Changing it

Invariants to keep:

- Purity. No environment variables, config, `$ADAGE_HOME` or clock in this crate. The only
  `std::fs` in the library is `checkout::Disk` and `orient::readable`, which `Disk` calls. A new
  side input goes on `Policy` or `Ctx`, set by the hook.
- Determinism: the same `(index, state, event, policy, checkout)` gives the same `Decision`.
  `assign_arm` is a hash of the session id.
- Fixed templates: every directive text comes from `render.rs`; a route contributes structure
  and validated slot values, never prose. Changing a template means changing the matching regex
  in `adage-safety/src/lib.rs` in the same change.
- The fast path must stay exactly equivalent: if `decide` starts reading a new state field or
  trigger kind, update `fast::skip` and the table in its module doc.
- Serialised shapes: `SessionState`, `Decision` and `Directive` live in state files and spools.
  New fields take `#[serde(default)]`, and `rebuild` must still replay old records.
- Caps come from `adage_tracks::params` through `Ctx::params`; do not hard-code new ones.

Tests: `cargo test -p adage-runtime` (`src/tests.rs`, `checkout.rs`, `orient.rs`,
`tests/pretool_replay.rs`, which reads `tests/fixtures/click-0cd28d65-r1-control.json`). Then the
dependents whose tests render or replay decisions: `cargo test -p adage-safety -p adage-hook
-p adage-eval -p adage-exec -p adage-learn`. The crate reads nothing outside itself (no `READS`
entry in `xtask/src/affected.rs`).

Pitfalls: `lib.rs`'s module doc still says "no I/O except checking that bound repository paths
exist"; that check now goes through `Checkout` too. `context.rs` is four lines: the matching
primitives live in `adage-tracks`, so a change to symbolisation belongs there. `render.rs` also
holds `adage-exec`'s E1/E2 text, although execution sits four layers up; run its tests after
editing them.
