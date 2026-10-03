# adage-exec

The one component of Adage that may run anything: an approved route, in an OS sandbox, with the
output returned to the agent as a typed, fenced data block. It is a separate binary (`adage-exec`)
that `adage delegate on` makes the hooks call in place of `adage hook`. The `adage` binary itself
never spawns a process (`tests/no_spawn.rs` scans the crates behind it). Design: `../../docs/execution.md`;
layering: `../../docs/architecture.md`.

## Where it sits

- Layer 7 in `xtask/src/layers.rs` ("execution").
- Links: `adage-core`, `adage-store`, `adage-tracks`, `adage-runtime`, `adage-pack`, `adage-hook`,
  `adage-safety`. On Linux also `landlock` and `seccompiler`; on Unix `libc`.
- Dev-dependency only: `adage-learn` (`hook::act`'s unit tests learn tracks into a fixture home).
- Depended on by `adage-app` (approvals in the review queue), `adage-cli` (`adage delegate`,
  `adage review delegate|revoke`, the TUI; the library only) and `adage-train` (replay in the sandbox).
- Forbidden edges (`FORBIDDEN`): `adage-exec -> adage-eval` and `adage-exec -> adage-learn` as
  normal dependencies. Keep the learner a dev-dependency.

## What's inside

- `main.rs`: the binary. `hook <harness> <event>` is dispatched before anything else; then `run`,
  `approve`, `revoke`, `status`, `doctor`, `journal`, `recover`. Exit 0 delivered, 2 usage, 3 refused.
- `request`: `ExecRequest`, `Level` (E1/E2), `OutputMode`, `FieldSpec`, `ActionSpec`, slot binding (`bind`).
- `classify`: `effect` and `inert` from the command table; `policy`: `Policy`, `load`, `merge`
  (personal `$ADAGE_HOME/policy.toml`, managed file wins).
- `approvals`: `ApprovalKey`, `Approval`, `Store` (`$ADAGE_HOME/approvals.json`), `auto_eligible`.
- `run`: `check` (every gate short of running, what `--dry-run` prints), `execute`, `spawn_sandboxed`, `Context`.
- `sandbox`: `Profile`, `Capability`, `detect`; backends `linux` (Landlock + seccomp), `bwrap`, `macos`.
- `limits`, `env` (environment built from nothing), `git` (`HARDENING`, `hazards`).
- `project`, `scan`, `render`: typed or raw projection, secret and injection scan, the one
  `<adage-data>` template (`data_block_checked`).
- `route`: a compiled route as a request (`plan`, `plan_grants`, `lapses`), shared by approval and the hook.
- `hook`: `Prefetch`, the `adage_hook::Extension` that adds E1 prefetch and E2 actions.
- `stage` and `journal`: E2 staging (`stage`, `judge`, `apply`), the apply journal (`Receipt`, `finish`, `acknowledge`).
- `replay`: `Replay`, a sandboxed disposable checkout for `adage train`.
- `person`: `require_person`; `lib.rs`: `last_withheld`, `canonical_json`, `sha256_hex`.

## How it connects

- Implements `adage_hook::Extension` (`hook::Prefetch`) and calls `adage_hook::main` with it, so
  `adage-exec hook` is the same pipeline as `adage hook` plus delivery of data blocks. With no
  grants its output must equal `adage hook`'s byte for byte (`tests/parity.rs`).
- Reads the index and tracks (`adage-tracks`), pack bindings (`adage-pack`), state and spools
  (`adage-store`); re-checks every delivered block with `adage_safety::check_data_block`.
- `adage-app::review` and `adage-cli::exec` write approvals through `approvals::Store`;
  `adage-train::verify` and `cards` run `replay::Replay`.
- Spool records gain an `exec` entry per attempted route; `adage-exec doctor` and `adage review`
  read it back through `last_withheld`.

## Changing it

- Invariants: fail closed (no sandbox, nothing runs); an approval matches on every field of
  `ApprovalKey`; no shell, ever (argv goes to `execve`); output is checked, never cleaned; E2
  applies nothing if any touched path changed since the pre-state.
- Tests: `cargo test -p adage-exec`. `cli`, `e1`, `stage`, `escape` are `#![cfg(unix)]`;
  `e1_private` is Linux only; `replay` and `stage` skip (and assert nothing ran) without a sandbox
  or staging backend. `escape` requires zero escapes; `stage` runs 500 hostile regenerates.
  `--nocapture` prints the tally. `tests/no_spawn.rs` always runs in CI.
- `tests/parity.rs` compiles `crates/adage-cli/tests/hooks.rs` against this binary, so changing
  the hook goldens there changes this crate's tests too.
- Dependents to run: `cargo test -p adage-app -p adage-train`, and `adage-cli` (`exec.rs`,
  `actions.rs`).
- Fixtures read (`READS`): `evals/sandboxes/` (acme-billing, copied into a temp repo).
- Pitfall: `main.rs` dispatches `hook` before parsing anything, so the no-match path costs what
  `adage hook` costs; keep startup work out of that path.
