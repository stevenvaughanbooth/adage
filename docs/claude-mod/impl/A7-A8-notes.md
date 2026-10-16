# A7 · The press token · A8 · One plugin, both binaries

Chunks A7 and A8 of [implementation-plan.md](../implementation-plan.md) (workstream A), design in
[architecture.md](../architecture.md) §3.12, cases T-A7-01 … T-A7-09, T-A8-01 … T-A8-05 and T-X-13
(binary side) of [test-plan.md](../test-plan.md). P2's open issue 3 (the TUI routes view and the
review queue show no predict grant) is closed here.

## A7 · What was built

### The token: exact shape and lifecycle

- **Token.** 32 bytes from the operating system (`getrandom`), printed as 64 lowercase hex
  characters. Never stored, never spooled, never on argv.
- **Hash.** SHA-256 of the raw token string (the 64 hex characters as bytes), hex.
  `adage_hook::press::hash`. Compared in constant time (`press::same`).
- **State.** Two files per session id under `$ADAGE_HOME/sessions/` (the id sanitised as for the
  state and spool):
  - `<id>.press`: `adage_hook::press::Record`, in the session state's locked two-slot CRC format
    (`adage_store::state`):

    ```json
    { "hash": "<sha256 hex of the valid token>", "chain": 0, "since": <ms, when it was issued>,
      "opened": <ms, first window>, "closed": <ms, first agent act>, "issued": true,
      "resumed": false, "ended": false, "root": "<canonical repository root>", "warned": false }
    ```

  - `<id>.press-open`: an empty file, present while the window is open.
- **Open** (the command hooks' `SessionStart`, Claude Code only, before the kill switches):
  `source` `startup`, `clear` or absent opens the window when the session id has no record yet;
  `resume` reopens it once per session id (`resumed`), clearing `issued`; `compact` never.
- **Issue** (`adage hook claude Press`, stdin `{ session_id, cwd, source }`, before the kill
  switches, `ADAGE_DISABLE` and `.adage/disabled` included): `{"press":"<64 hex>"}` when the
  window file exists, the record has not `issued` for this start and has not `ended`, and
  `source` is not `compact`; otherwise nothing. Sets `hash`, `chain = 0`, `since`, `issued`.
  Spools `{ ts, event: "Press", kind: "press_issued", source, chain }`.
- **Close** (the first of `UserPromptSubmit`, `PreToolUse`, `PostToolUse`, `PostToolBatch`,
  `PostToolUseFailure`, `Stop`, `TurnStep` for the session, in `handle()` before the kill switch
  and before the fast path, and in `run()` under `ADAGE_DISABLE`): `unlink(<id>.press-open)`; when
  that removed something, `closed` is set. Every later event's cost is one `unlink` that finds
  nothing.
- **Spend** (`adage_hook::press::spend(paths, token, session, root, verb)`): refused unless the
  record exists, its `root` equals the verb's repository root (`press::root_of(cwd)`), the
  session is live (below), it has not `ended`, the hash matches, and `now - since ≤ 24 h`. On
  success `chain += 1`, a new token is minted, its hash replaces the old one, `since = now`, and
  `{ ts, event: "Press", kind: "press_spent", chain, verb }` is spooled. Returns the next token.
- **Live session** (`press::live`): a session the agent has acted in (`closed` set), or one that
  started (`opened`) while no other non-ended session in the same repository had acted (a
  `closed` at or before it, within 24 h). A hand-written `SessionStart` for a new id from a
  running session's shell is therefore refused until something acts in it.
- **End** (`SessionEnd`): `hash` cleared, `ended` set, window file removed.
- **Tripwire** (`PreToolUse`, Bash, Claude Code, no kill switch set): a command naming `hook claude
  Press` (three words) or containing `"press"` (or `\"press\"`) spools `{ kind: "tripwire",
  warned }` every time and, the first time in a session, prints
  `{"systemMessage":"adage: a shell command in this session names Adage's press. A press is a
  person's answer in the session, never a command; the command will not be honoured as one."}`.
  Never a `deny`, never an `updatedInput`; the call then answers nothing else (it names Adage's
  own hook, not a Python run).

### The person check

`adage_app::person` (layer 8, links `adage-hook`):

- `Person { via: Via, next: Option<String> }` with `via()` and `next()`; `Via` is
  `adage_tracks::review::Via { Terminal, Press }` (serde `terminal` / `press`).
- `require(cmd)`: unchanged behaviour, `via: Terminal`.
- `Press { press, session }`, `Stdin { press, expect }`, `read_stdin()` (nothing at a terminal;
  otherwise one JSON object, at most 64 KiB, waited for at most 3 s), `parse_stdin(bytes)`.
- `require_or_press(cmd, press, paths)`: no press → `require(cmd)`. A press with
  `ADAGE_ALLOW_NONINTERACTIVE=1` → refused ("… was given a press and ADAGE_ALLOW_NONINTERACTIVE=1
  together, and refuses both", or `require`'s agent refusal when a marker is set). Otherwise
  `press::spend`; on success `Person { via: Press, next }`, on any refusal exactly `require(cmd)`
  (the person-only message; nothing written, nothing spent).
- CLI wrapper `crate::review::person_or_press(cmd, paths) -> (Person, Stdin)`: reads stdin once,
  checks, and on a press prints `{"next":"<64 hex>"}` as the **first stdout line**, flushed,
  before the verb does anything else (the spent token is refused whatever the verb then does).

### Every press site (and nothing else)

| Verb | Where |
| --- | --- |
| `review approve\|strengthen\|weaken\|extinguish\|pin\|dismiss` | `adage-cli/src/review.rs::decide` (`decide_quietly`, the TUI, uses `require_or_press(cmd, None, …)`: the terminal owns stdin there) |
| `review delegate` (E1, E2, `--predict`, `--stage --json`, `--confirm`) and `review revoke` | `adage-cli/src/exec.rs::review` |
| `delegate on` | `exec.rs::switch`; under a press `switch_pressed`: writes `exec/enabled` only (`{ enabled_at, bin: "plugin", via: "press" }`) and names each settings-file, Codex or OpenCode registration still running `adage` |
| `delegate policy --max-level e2` | `exec.rs::policy` |
| `delegate predict on` | `exec.rs::predict` |
| `on` | `adage-cli/src/switch.rs::on` (the CLI agent's file; a one-line change) |
| `add <pack> --trust` | `adage-cli/src/add.rs::add`, only when the pack directory is inside the repository and every file in it is committed at `HEAD` with the work tree's bytes (`adage_app::trust::committed_pack`, read through `adage_core::gitobj::Repo::committed`, spawning nothing); otherwise `require_person("add")` |

Untouched, so they refuse a press: `install`, a guarded `config set`, `share`, `train accept`,
`packs on`, `init --yes`, `add` without `--trust` (or a pack not committed), and every
`adage-exec` verb (`approve`, `revoke`, `recover`, which use adage-exec's own check and never read
stdin). Turn-downs (`delegate off`, `policy --max-level e1`, `predict off`, `off`) need nothing.

### Provenance

- `Review.via: Option<Via>` (`adage-tracks/src/review.rs`, `review.yaml`), set from the `Person`
  by `adage_app::review::decide`/`decide_within`; `None` for Adage's own automatic extinguish
  (`adage-learn/src/instincts.rs`) and every decision recorded before.
- `Approval.via: Option<Via>` (`adage-exec/src/approvals.rs`, `approvals.json`): from the
  `Person` in `review delegate` (every level, `--predict`, `--confirm`); `Some(Terminal)` for
  `adage-exec approve`; `None` for trial stores, automatic approvals and test fixtures. `--by` is
  unchanged.

### Stage, then confirm (`review delegate … --stage --json` / `--confirm`)

- Clap: `--stage` (requires `--json`, conflicts with `--confirm`, `--predict`), `--confirm`
  (conflicts with `--predict`), `--json`.
- `--stage --json` prints one JSON object (after the `next` line when pressed):

  ```json
  { "route": "<track>/<route>", "repo": "<key>", "level": "e1|e2", "terms": { …Terms… },
    "proposed": true, "first": true, "ok": true, "why": "…", "digest": "<see below>",
    "recorded": false,
    "facts": { "argv": [...], "inputs": [{ "slot", "contract", "from" }],
      "effect": { "approved", "classified" }, "output": { "mode", "fields" }, "exit_when": {},
      "scripts": [{ "path", "sha256", "text" }], "sandbox": "…", "profile_id": "…",
      "route_exec_digest": "…", "evidence": { "support", "opportunities", "days" },
      "attestations": [...], "python": { … }?,
      "action": { "kind", "outputs", "cache" }?,
      "staged": { "backend", "exit", "diff": [{ "path", "change", "before_sha256",
                  "after_sha256", "diff" }], "refused": [{ "path", "why" }], "discarded": 0,
                  "stderr": [...]? }? } }
  ```

- **Proposed terms** (`proposed: true`): an E2 grant with no `--outputs`/`--cache` on a local
  route is first staged with one output glob that matches nothing (`adage-probe-declares-nothing/**`)
  so every path it writes is refused and listed; `adage_exec::route::propose_outputs` turns
  those into outputs (`<dir>/**`, or the file at the top level) and caches (`<dir>/**` under
  `__pycache__`, `.pytest_cache`, `.mypy_cache`, `.ruff_cache`, `.cache`, `.hypothesis`, `.tox`,
  `.nox`); a verify route keeps no outputs (its stage then refuses those paths). E1's proposed
  fields are `route::plan`'s defaults (implicit or lookup outputs).
- **Digest** (`adage_exec::route::grant_digest`): `<sha256(snapshot)>.<16 hex>×11`, the
  snapshot being `{ inputs: Staged.inputs, exit, changes, refused }` (null for E1) and the eleven
  being SHA-256 prefixes of the key's `scripts, argv_templates, effect, level, output_mode,
  profile_id, signer, mode, repo`, the `terms`, and `route_exec_digest`.
  `route::digest_change(expect, now)` → `Same | Snapshot | Terms(fields) | Malformed`
  (`route_exec_digest` is named only alone).
- `--confirm` reads `{ "expect": <digest> }` (and the press) on stdin, plans and stages again:
  `Terms` → error "<label> changed since it was staged (scripts); nothing was recorded";
  `Snapshot` → the fresh stage object with `"changed": "snapshot"`, `recorded: false`, exit 0;
  `Same` with a refused stage → error; `Same` → records, printing the stage object with
  `recorded: true`, `wrote: [...]`, `takes_effect: "in new sessions"`. At E1 policy the stage runs
  under `adage_exec::policy::trial_e2` (personal file raised as `set_max_level` would; `None`
  when the managed file caps at E1 → refused), and the confirm writes `policy` (`set_max_level
  E2`), `marker` (when absent) and `grant`; later confirms write only what is missing.

### Offers and packs awaiting trust

- `adage_app::reflex::Offer { kind: "execute"|"predict", command, confirm? }` and
  `Decisions.offers` (skipped when empty, so every other surface's JSON is unchanged).
  `Offer::of(reference, route, granted, permitted)`: fire, firm or forming, Verify or Regenerate,
  `Step::Run` with no slots, no execution evidence, no grant (any mode) for the label, policy
  permitting (`trial_e2` exists and `effects_deny` does not name the effect); after-edit →
  `execute` (`--level e2 --stage --json`, confirm `--level e2 --confirm`), context → `predict`
  (`--predict`). Filled in `adage_app::graph::build`, so `graph --json` and the map carry it.
- `adage_app::trust::{committed_pack, awaiting}`: `status --json` gains
  `packs_awaiting_trust: [{ name, path, command }]` (only when non-empty) for packs under
  `.adage/packs/*/` whose `pack.toml` names `owner/name` not installed at
  `tracks/@imported/<owner>/<name>` and that are committed.

### P2 open issue 3

The TUI routes view's execution column shows, for a reflex with a predict grant and no execute
grant: `Predicted` (the grant holds now: `adage_exec::hook::predict_grants`), `Paused`
(`exec/predict-off`), `Needs review` (it no longer matches) or `Grant saved` (outside the
checkout). The review queue already lists lapsed grants of every mode (`route::lapses`); a valid
predict grant needs no person, so it waits for nothing; `offers` names the grant a reflex could
get.

## A8 · What was built

### The plugin's final layout

```text
plugin/claude-code/                     the one plugin (adage@adage)
  .claude-plugin/plugin.json            unchanged (types)
  hooks/hooks.json                      modules ["./register.ts"]; SessionStart → scripts/session-start
                                        (10 s); every other hook → ${CLAUDE_PLUGIN_DATA}/bin/hook;
                                        PostToolBatch and Stop 30 s, the rest 5 s
  hooks/*.ts                            the one copy of the mod (bridge, register, step, predict,
                                        reflex, substitute, plugin; tokens.ts from the CLI agent);
                                        BIN = 'hook'
  types/index.d.ts                      unchanged
  scripts/session-start                 the shim (below)
  scripts/quiet                         reads stdin, prints nothing
  VALIDATE.txt                          unchanged (validate output identical)
  dist/<target>/{adage,adage-exec}[.exe], dist/SHA256SUMS   staged by build.sh / the release
plugin/claude-code-exec/                the retired alias (adage-exec@adage), one release
  .claude-plugin/plugin.json            no types; "Retired: …"
  hooks/hooks.json                      SessionStart alone, no modules
  scripts/session-start                 relinks bin/adage-exec → scripts/quiet, removes
                                        bin/adage-exec.exe and bin/adage.exe, prints one
                                        systemMessage: "adage-exec: the adage plugin now includes
                                        execution; uninstall this one (/plugin uninstall
                                        adage-exec@adage)."; runs nothing, exits 0
  scripts/quiet
```

### The shim (`plugin/claude-code/scripts/session-start`)

1. `bin/adage`: `dist/<T>/adage` verified against `SHA256SUMS` (installed as `adage-<sha>`, linked;
   a copy on Windows), else `adage` on `PATH` (npm's launcher answers `ADAGE_NPM_WHICH=1` with
   the platform binary). None → `bin/adage` and `bin/hook` → `scripts/quiet`, `$DATA/status`,
   one systemMessage (the tamper or install hint), exit 0.
2. `bin/adage-exec`: `dist/<T>/adage-exec` verified (as `adage-exec-<sha>`), else `adage-exec` on
   `PATH`, else beside the adage just linked. None → `bin/adage-exec` removed.
3. `bin/hook`: `adage-exec` when `$ADAGE_HOME/exec/enabled` exists and step 2 found one, else
   `adage` (a relative symlink; on Windows a copy, re-copied when it differs). With the marker
   and no adage-exec: adage serves and the shim prints `adage: execution is on, but <no adage-exec
   binary was found for this machine | the bundled adage-exec does not match its checksum
   (reinstall the plugin)>; serving without execution (adage delegate off turns it off).`
4. `exec bin/hook hook claude SessionStart`.

`$ADAGE_HOME` defaults follow `adage_store::Paths` (Windows: `%APPDATA%\Adage`, new).

### Elsewhere

- `crates/adage-hook/src/lib.rs`: `session_start` records (routed and unrouted) carry `bin`, the
  serving binary's file stem (`adage-<sha>`, `adage-exec-<sha>`, `adage`), via `serving_bin()`.
- `exec.rs` `delegate on` plugin line: "Claude Code plugin: no second install; with <marker>
  present its hooks call adage-exec"; module docs updated.
- `plugin/build.sh` stages both binaries into `plugin/claude-code/dist/<host>/`, `SHA256SUMS`
  lists both, removes any stale `plugin/claude-code-exec/dist`.
- `scripts/stage-plugin.mjs`: `plugin/claude-code` bundles `adage` (required) and `adage-exec`
  (required except on Windows); the alias is staged with no binaries.
  `scripts/test-stage-plugin.mjs` asserts both lines and the bare alias.
- `scripts/check-plugin.mjs`: hook commands must run `bin/hook`; PostToolBatch and Stop must be
  30 s; the shim must read the marker and make `bin/hook`; the alias must register SessionStart
  alone, load no module, name no types, ask to be uninstalled and run no binary; no `.ts` in the
  alias; `--staged` expects both binaries in the one plugin. The "same module in both plugins"
  rule is gone (T-X-10 retired).
- `.github/workflows/ci.yml`: `claude plugin test plugin/claude-code` only (the alias has no
  module: `claude plugin test` exits 1 with "no hooks module to load"); the session-start step
  builds `adage-cli` and `adage-exec` first.
- `npm/`: the launcher's comment; `launcher.test.mjs` asserts adage-exec sits beside the binary
  `ADAGE_NPM_WHICH` names.
- `.claude-plugin/marketplace.json`: descriptions (adage: execution included; adage-exec:
  retired).
- `demo/run.py`: `stage_plugin(src, binary, dest, target=None, exec_binary=None)` stages
  `adage-exec` too (default: beside `binary`; `ADAGE_EXEC_BIN`), `plugin_for_runs` passes
  `EXEC_BIN`; docstrings and the ARMS comment. `demo/test_run_arms.py`: both binaries staged,
  the shim links `bin/hook` to adage. `demo/SANDBOX.md` "The plugin's binaries".

## Tests

| Case | Test |
| --- | --- |
| T-A7-01 | `adage-cli/tests/press.rs::issued_once_in_the_window`; `adage-hook press::tests::issued_once_and_closed_by_the_first_act` |
| T-A7-02 | `press.rs::the_window_closes_on_the_first_invocation_not_the_first_record` (no-routes prompt, unmatched prompt, PreToolUse without a Python word, unmatched PostToolBatch); the unit test covers every closer |
| T-A7-03 | `press.rs::sources`; `press::tests::sources` |
| T-A7-04 | `press.rs::a_press_is_a_person_at_the_listed_sites` (six review verbs, `--predict`, revoke, policy e2, predict on, delegate on, on; last character changed, another session's token, none, with the override, 25 h; turn-downs with and without), `a_press_is_refused_everywhere_else` (install, guarded config, share, train accept, packs on, init, add with and without `--trust` outside a committed pack, `adage-exec approve/revoke/recover`; none spent it), `a_committed_pack_can_be_trusted_with_a_press` |
| T-A7-05 | `a_press_is_a_person_at_the_listed_sites` (each spent token refused, the next works, SessionEnd) |
| T-A7-06 | `press.rs::an_e1_stage_answers_the_proposed_grant_and_records_nothing`, `an_action_is_staged_then_confirmed` (live sandbox: proposed `gen/**`, facts, nothing applied or written, snapshot change → fresh diff, policy+marker+grant, script change refused naming `scripts`, grant alone); `adage-exec route::tests::{proposed_outputs_follow_what_a_trial_stage_changed, a_digest_names_what_changed_between_stage_and_confirm}` |
| T-A7-07 | `press::tests::hashing_chaining_and_age` |
| T-A7-08 | `press.rs::the_tripwire_warns_and_never_refuses`; `press::tests::the_tripwire_warns_once` |
| T-A7-09 | `press.rs::the_press_runs_before_the_kill_switches_and_only_for_claude_code`; goldens unchanged (`tests/hooks.rs` 22 pass, no bless), `adage-exec/tests/parity.rs` 22 pass |
| T-X-13 (L2) | `press.rs::the_model_cannot_press`; `press::tests::a_start_written_by_hand_is_not_the_live_session` |
| T-A8-01 | `plugin/test/session-start.test.sh` cases 1, 2, 4–9 (marker off/on/removed, both checksums, tampered adage, tampered adage-exec, PATH, beside, neither, npm, simulated Windows copies re-copied on a flip) |
| T-A8-02 | `session-start.test.sh` case 3 (one `session_start` from `adage-exec-<sha>` with the marker, from `adage-<sha>` without); `zero_config_plugin.rs::exec_marker_picks_the_hook_binary` |
| T-A8-03 | `plugin/claude-code/hooks/register.test.ts`: the presence probe on `bin/hook`, active for `adage-exec` too, inert with `scripts/quiet` (54 pass) |
| T-A8-04 | `session-start.test.sh` case 11 (SessionStart alone, one line, the stale link quiet, no record) |
| T-A8-05 | `hooks.json` 30 s on PostToolBatch and Stop, enforced by `check-plugin.mjs`; `release_plugin_startup` now stages both binaries (`zero_config_plugin.rs::staged_plugin`) but runs only in release CI (see below) |
| Offers, trust, P2 issue 3 | `adage-app reflex::tests::offers_name_the_grant_the_binary_would_ask_for`, `trust::tests::committed_packs_await_trust`, `adage-core` `gitobj::tests::tree_entries_are_found_by_exact_name` and `tests/repo_key.rs::committed_files_match_git`, `adage-cli ui::routes::tests::execution_view_shows_a_predict_grant`, `person::tests::*` |

## Departures from the plan

1. **The press state is two files, not `SessionState.press`.** The plan puts `press: { hash,
   chain, issued, closed, since }` on `adage-runtime`'s `SessionState` (another agent's struct,
   being changed this wave). A separate `sessions/<id>.press` record and a `<id>.press-open`
   window file keep the closing cost to one `unlink` that finds nothing (no state read on the
   fast path) and keep the rebuild rule trivially (a rebuilt state never had a press). The record
   carries `opened`, `resumed`, `ended`, `root` and `warned` beyond the plan's fields.
2. **More closers.** The plan names `UserPromptSubmit`, `PreToolUse` and `PostToolBatch`;
   `PostToolUse`, `PostToolUseFailure`, `Stop` and `TurnStep` close it too (all agent-side).
3. **"The repository's live session"** is defined as above (acted in, or started while no other
   session there had acted). The test plan's forged start is refused; forging a closer for the
   forged session as well defeats it, which the tripwire records and which is within the stated
   non-boundary. Before its first prompt a real session's press is accepted only if no other
   session in the repository had acted before it started.
4. **`next` is the first stdout line**, printed at the moment of the spend (the token is spent
   even if the verb then fails), rather than at the verb's end. For `--stage --json` and
   `--confirm` the result object is the second line.
5. **The tripwire returns early** with its warning on the first occurrence (the call names
   Adage's own hook, so no Python rewrite is lost); later occurrences pass through.
6. **Proposed E2 outputs come from a trial stage**, not the route's evidence: routes record no
   observed writes.
7. **`status --json` packs awaiting trust** reads `.adage/packs/*/pack.toml` names only (the full
   pack is validated by `add`); it is said only when non-empty.
8. **The mod's hook binary is `bin/hook`** (the shim's choice), not `bin/<adage|adage-exec>`
   chosen in TypeScript: the marker is read once, by the shim, at every start, so the module and
   the command hooks can never disagree.
9. **Files outside my brief, minimal edits:** `adage-hook/src/lib.rs` (`pub mod press`, the gate
   in `run()` and `handle()`, `serving_bin()` and `bin` on `session_start` records),
   `adage-hook/Cargo.toml` (`getrandom = "0.2"`, already in the lock through adage-sign),
   `adage-hook/src/press.rs` (new), `adage-tracks/src/review.rs` (`Via`, `Review.via`),
   `adage-learn/src/instincts.rs` and `adage-cli/tests/review.rs` (`via: None` in literals),
   `adage-core/src/gitobj.rs` (`Repo::committed`, `tree_entry`) and `tests/repo_key.rs`,
   `adage-cli/src/cli.rs` (three flags on `DelegateRoute`), `adage-cli/src/switch.rs` (`on`),
   `adage-cli/src/main.rs` (`packs_awaiting_trust`), `adage-cli/src/add.rs`,
   `adage-cli/tests/zero_config_plugin.rs` (`staged_plugin` stages both binaries;
   `exec_marker_without_plugin` became `exec_marker_picks_the_hook_binary`),
   `adage-app/{graph,reflex,review,lib}.rs`, `adage-app/Cargo.toml` (`adage-pack`, `toml`).

## Not verified here (live only)

- Whether Claude Code delivers `UserPromptSubmit` for the mod's own `/adage …` commands (a
  `/adage on` typed first in a session in a repository where another session already acted is
  refused until something acts in this one).
- `release_plugin_startup`'s 1,500 ms first-start budget with two binaries hashed and installed
  (release build only; not built here: no `--release` per the rules).
- The paired `hook_latency` cost of the closing `unlink` on the no-match paths (expected a few
  microseconds; not measured).
- Windows: the shim's copies and `%APPDATA%` default are exercised only through a `uname` stub.

## Notes for other agents

- Spool kinds added: `press_issued`, `press_spent`, `tripwire` (no `arm`, no `decision`).
  `ui/feed.rs::from_record` renders them as empty-act items; B4 counts `press_issued` off the feed.
- B4: run person-only verbs with `{ "press", "session" }` (plus `expect` for `--confirm`) on
  stdin, read line 1 as `{ "next" }`, line 2 (stage/confirm) as the result. `graph --json`
  `routes[].review.offers` names the ask; `status --json` `packs_awaiting_trust` the team packs.
- The bridge's allow-list was not widened (B4's).

## Commands run (in /home/user/adage-poc)

| Command | Result |
| --- | --- |
| `cargo fmt --all` then `cargo fmt --all -- --check` | clean |
| `cargo clippy -p adage-app -p adage-exec -p adage-cli -p adage-hook --all-targets -- -D warnings` | clean (after waits for other agents' in-progress files and a full disk) |
| `cargo test -p adage-exec -p adage-app -p adage-hook --no-fail-fast` | all pass (adage-exec lib 108, cli 7, e1 2, e1_private 3, escape 13, no_spawn 2, parity 22, replay 2, stage 3; adage-app 30; adage-hook lib 43) |
| `cargo test -p adage-cli --no-fail-fast` | all pass except two failures in other agents' areas: `ui::tests::settings_show_rewrites_off_by_the_older_marker_and_notices` (the snapshot gains the skills agent's `skills.invoke` key) and `live_learn::learn_auto_logs_what_changed` (the learner agent's `candidate` change lines). `press` 11, `exec` 12, `zero_config_plugin` 11, `hooks` 22 (goldens unchanged), `review` 7, `actions` 3 |
| `cargo test -p adage-cli --bin adage execution_view` | 2 pass |
| `cargo test -p adage-core --test repo_key committed`, `--lib gitobj` | pass |
| `cargo test -p xtask` | 18 pass |
| `claude plugin validate --strict plugin/claude-code`, `plugin/claude-code-exec`, `.` | all pass; the first's output is identical to `VALIDATE.txt` |
| `claude plugin test plugin/claude-code` | 54 pass |
| `node scripts/check-plugin.mjs` | ok (and fails as intended on a `modules` key added to the alias) |
| `node scripts/test-stage-plugin.mjs` | ok |
| `plugin/test/session-start.test.sh` | `session-start ok` (one earlier run failed two Windows checks with the disk full; rerun clean) |
| `node npm/test/launcher.test.mjs <stripped adage>` | ok (the debug binary could not be copied with the disk full; run on stripped copies) |
| `cargo xtask test python demo` | 36 tests OK (4 skipped: need `ADAGE_BIN`) |

The disk filled repeatedly during this work (five agents, one `target/`); I deleted only older
duplicate test executables in `target/debug/deps` (cargo relinks them) and
`target/debug/incremental`, as the rules allow.

## Review fixes

The binary's half of the branch review's press findings (REVIEW.md numbering), each with its
test. The mod's half (H1, H2, M1, L4, L8) is in B4-D5-notes.md, "Review fixes".

| Finding | What changed | Test |
| --- | --- | --- |
| **M3** The tripwire's warning was swallowed by the mod's own `dry_run` ask (the mod reads no `systemMessage` from it), and recorded twice | `press::gate` runs the tripwire only for a command hook's payload (`!harness::claude::is_mods(event, p)`): the mod's dry run and verdict for the call still close the window and mark the turn, but neither warn nor record. The command hook's `systemMessage`, the path §3.12 names, then warns once | `press::tests::the_tripwire_warns_once` (dry run, verdict and `via: mod` record nothing; the command hook warns); `adage-cli/tests/press.rs::the_tripwire_warns_and_never_refuses` (a dry run first, then the warning, two records for two command-hook calls) |
| **M4** A Bash command could mint a press for a fresh `git init` directory (or, by forging one closer event, in the session's own repository) and run machine-wide verbs | Three changes. (1) `open` opens a window only for a start whose `transcript_path` ends `<session_id>.jsonl`, as every engine start does (the sim's payloads already did; the file need not exist, since Claude Code writes it lazily). (2) "Live" is now `Record.contested`, fixed when the window opens: a session is contested when another session on this machine is mid-turn then. Mid-turn is a marker, `sessions/<id>.press-turn`, created or touched (`set_modified`) by `UserPromptSubmit` and `PreToolUse`, removed by `Stop` and `SessionEnd`, and ignored once untouched for `press::TURN_MS` (1 h, so a session that died mid-turn stops contesting). A Bash call's `PreToolUse` always comes before the command runs, so a start it writes is contested. `spend` refuses a contested session's press with `NotLive`, for every verb, in every repository, for good: acting in it later no longer makes it live (the old `live()`, root-scoped with a 24 h age-out and the `closed` excuse, is gone). (3) `names_press` strips `"`, `'` and `\` before matching, and reads the event word up to its first non-alphanumeric character (`'Press'<p.json`, `Press\|tee`) | `press::tests::a_start_written_by_hand_is_not_the_live_session` (forged start refused, still refused after it acts; a fresh `git init` refused for `delegate on`; after `Stop` a second terminal's press works; a stale mark stops contesting; `SessionEnd` removes it); `a_start_without_its_transcript_opens_nothing`; `the_tripwire_warns_once` (five quoted forms); `press.rs::the_model_cannot_press` (forged start then a prompt in it, `delegate on` in a fresh repository, a start with no transcript) |
| **M7** A second `--resume` of one session raised the tamper alarm | The binary allows one reissue per session id by design, so this is the mod's count: a one-line change in `plugin/claude-code/hooks/register.ts` `issuePress` (unavoidable: the alarm is computed there). A resume this engine process got no token for counts `issued = 2`, the two issues (start and one resume) the spool holds legitimately, where it counted 0. A third issue still alarms | `dialog.test.ts`: `M7: a second resume … raises no tamper alarm …`; `M7: a third issue … is tampering` (fixture line `LINE.pressIssuedThird`) |
| **M8** `committed_pack` accepted a pack the agent wrote and committed in the session | The rule, `adage_app::trust::committed_before(src, root, since)`: committed at `HEAD` with the work tree's bytes (as before), and the commit that brought the pack's content to `HEAD` (`gitobj::Repo::committed_at`, first-parent walk while the pack's tree id is unchanged; its committer second wholly before `since`), and no file or directory of the pack changed on disk since `since` (status-change time on Unix, which no command short of the system clock sets; the modification time elsewhere). `since` is the press session's `Record.opened` (`adage_app::person::press_opened`). `add --trust` reads stdin first, then applies it; without a press (a terminal) the rule is the old one. `status --json`'s `packs_awaiting_trust` still lists committed packs (so the mod may ask), and the press is then refused for a recent one. The commit date alone is the committer's choice (`GIT_COMMITTER_DATE`), which is why the on-disk time is checked too. The ask's wording ("signed by …" from the pack's manifest) is the mod's and unchanged | `trust::tests::a_pack_committed_in_the_session_is_not_trusted_with_its_press` (a pack written in the session and committed with a date a year back: refused; one committed in the session from older files: refused; the earlier pack: trusted); `press.rs::a_committed_pack_can_be_trusted_with_a_press` (the agent's pack refused with the press, the team's taken) |
| **L6** Every review verb waited up to 3 s on stdin whenever it was not a terminal | `adage_app::person::read_press()`: stdin is read only when an agent marker is set (the mod runs every verb with `CLAUDECODE=1`); `review delegate --confirm` still reads its `expect` everywhere. With no marker (a terminal, CI) nothing is read: a press piped beside `ADAGE_ALLOW_NONINTERACTIVE=1` with no marker is now ignored (neither honoured nor spent) where it was refused "both"; with a marker it is refused as before | `press.rs::a_verb_outside_a_session_does_not_wait_on_stdin` (an open, never-closed pipe: done well under 3 s); `a_press_is_refused_everywhere_else` (the no-marker case updated) |

### What the press is now, and the residual (plainly)

What holds: a press is issued only between an engine-shaped start and that session's first act;
a start written while any Claude Code session on the machine is mid-turn (every start a Bash call
writes, since the call's own `PreToolUse` marked its session first) yields a press no verb
honours, in any repository; and the tripwire sees the forms the design names, quoting included.

What does not, and is not claimed: an agent with a shell can still mint a usable press by
forging more of the protocol. It can write a `Stop` (or `SessionEnd`) payload for its own session
id first, which clears the mid-turn mark; it can start a background process that waits until the
turn has ended; it can name the transcript path any start needs; and it can read a press from a
file or build the event name from a variable, which the tripwire does not see. A Codex or OpenCode
session's turn is not marked at all (only Claude Code's hooks run the gate). All of these forge
hook payloads or the session's own state, which mod architecture §3.12 already says the press does
not stand against ("a boundary against an agent that runs as the same user … which can … forge a
hook payload"). The cost on the person's side: a session started while another one is mid-turn
cannot press for its whole life (its answers wait, as when the window was missed).

### Commands run (in /home/user/adage-poc)

See A1-A2-C5-notes.md, "Review fixes", for the whole run; the press-specific results: `cargo test
-p adage-cli --test press` 12 pass; `cargo test -p adage-hook --lib press` 6 pass;
`cargo test -p adage-app --lib trust` 2 pass; `claude plugin test plugin/claude-code` 274 pass, 0
fail (272 before; the two M7 tests added); `claude plugin validate --strict` for
`plugin/claude-code`, `plugin/claude-code-exec` and `.` pass, and the first's output still equals
`VALIDATE.txt` (unchanged); `node scripts/check-plugin.mjs` ok.
