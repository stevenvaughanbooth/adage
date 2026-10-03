# adage-core

Kernel primitives every other crate builds on: the canonical action vocabulary, the shell
normaliser (tree-sitter-bash plus a data-driven command table), effect classes, typed spans,
tokenisers, command-pattern matching, and repository identity read from `.git` without running git.

## Where it sits

- Layer 0 in `xtask/src/layers.rs` ("kernel primitives, no internal dependencies").
- Depends on no workspace crate. External: serde, serde_json, toml, regex, once_cell, sha2,
  tree-sitter, tree-sitter-bash, miniz_oxide (inflating git objects).
- Linked (`[dependencies]`) by every other crate except `adage-sign`, `adage-sim`, `adage-store`,
  `adage-term` and `xtask`: app, cli, eval, exec, graph, hook, ingest, instincts, intent, learn,
  pack, runtime, safety, trace, tracks, train.
- Must never link another workspace crate: there is no lower layer. No `FORBIDDEN` edge names it.
- Several modules run inside hooks (`shell`, `gitobj`, `repo`, `vtype`), so they are written to
  a time budget and never spawn a process or touch the network.

## What's inside

- `types`: `CanonicalAction` (Read, Search, List, Exec, Edit, Write, Fetch, Unknown, Control,
  Compound), `Effect` (Observe, Verify, Regenerate, Environment, Author, Publish), `Normalized`
  (action, effect, mutating, confidence, note). Re-exported at the crate root.
- `shell`: `normalize_shell(command, cwd, table) -> Vec<Normalized>`; bounds `MAX_COMMAND`,
  `MAX_PIPES`, `MAX_NESTING`, `PARSE_BUDGET`, `PARSE_BACKSTOP_MICROS`; `unlisted_repo_script`.
- `table`: `CommandTable::load()` parses `data/commands.toml` (compiled in with `include_str!`)
  once per process into `CommandSpec`s with flags, subcommands, `inert`, `wraps`, `run_shorthand`.
- `tools`: `normalize_tool(tool, input, cwd, table)` for native tool calls; `from_codex`,
  `from_opencode` translate those harnesses' tool names into the canonical tool vocabulary.
- `keys`, `template`: `template_key`, `template_command`, `canonicalise`, `flatten`, `is_author`.
- `pattern`: the one command-comparison algebra (`Shape`, `CommandPattern`, `meets`, `follows`,
  `same_command`, `is_housekeeping`). Every place that compares commands goes through it.
- `action`: `classify`/`classify_argv` read argv into an `Action` (role, env, runner, targets).
- `repo`: `find_root`, `repo_key`, `repo_key_cached`, `remember_key`, `lookup_key`, `origin_key`,
  `relativize`, `is_safe_relative`. `gitobj`: `Repo` walks HEAD to the root commit (loose objects,
  v2 packs with deltas, packed refs) and gives up with `None` past `MAX_WALK`.
- `gitignore::Ignore`, `secrets` (`has_secret`, `sensitive_path`, `token_rules`), `spans::extract`,
  `tokens::tokens`, `vtype` (learned value types with an O(len) matcher), `judge` (follow judge,
  `THRESHOLD`), `outcome` (reading failure from piped output), `execution_equivalence`,
  `pricing::table()` (from `src/pricing.toml`), `cloud` (macOS offloaded files), `json`, `time`.

## How it connects

- It defines no traits. Its types are embedded in the canonical model: `adage_trace::StepObs`
  carries a `Normalized`, and `adage_trace::Session` is serialised by `adage_ingest::cache`, so a
  change to `Normalized`, `CanonicalAction` or `Effect` changes the cached parse format too.
- Decoders in `adage-ingest` call `shell::normalize_shell` and `tools::normalize_tool` through
  `CommandTable::load()`, and `repo` through `adage_ingest::locate::LocalCheckout`.
- `pattern`, `vtype`, `spans`, `tokens` and `judge` are shared by the learner (`adage-graph`,
  `adage-learn`) and the live runtime (`adage-runtime`), which is why they live here and not in
  either of them. See [docs/architecture.md](../../docs/architecture.md).

## Changing it

- Run `cargo test -p adage-core`. Its integration tests: `tests/normaliser_cases.rs` (table-driven
  cases in `tests/cases/normaliser.json`, asserting op, effect, mutating and a confidence floor,
  not exact scores), `tests/deep.rs` (deep nesting and pathological input on a 1 MiB stack, and
  the parse budget), `tests/repo_key.rs` (hand-built git object stores; the last test compares
  with real git when it is installed).
- It reads no fixture outside the crate (nothing in `READS` of `xtask/src/affected.rs`). Because
  everything links it, CI's `cargo xtask affected` tests the whole workspace for any change here;
  do the same locally (`cargo test --workspace --locked`) before sending one.
- Keep the normaliser bounded: the `deep.rs` regressions came from the eval property tests and
  from hooks aborting. New constructs must not add recursion over input length.
- A command not in `data/commands.toml` is `Effect::Environment`. Add entries to the table, not
  to code; the header of the file documents each field. Never set `inert = true` for anything
  repository files can make run code (make, npm, cargo, pytest, python, node, sh): it gates
  automatic approval in `adage-exec`.
- Do not add a second way to compare commands or to spot secrets: `pattern` and `secrets` exist
  because earlier duplicates disagreed.
- `cloud::never_download` sets a process-wide I/O policy inherited by child processes; nothing that
  spawns an agent or installer on the person's behalf may call it.
- Several module docs still cite the spike and the TypeScript proof of concept
  (`research/poc`); that is history, not a dependency.
