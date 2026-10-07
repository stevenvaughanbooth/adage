# Live fixes: the defects the first live run found (binary and mod)

`docs/claude-mod/evaluation-results.md` (run 1 and run 2) found four defects in the binary and the
mod that the runner could only work around. Each is fixed here with a test, and the three that a
session can show were proved live in a fresh `CLAUDE_CONFIG_DIR` and `ADAGE_HOME` under the
session's scratchpad, never `/tmp/adage-live*`, with no `CLAUDE_PLUGIN_DATA` in the environment.

## 1. The mod was inert without `CLAUDE_PLUGIN_DATA`

Claude Code substitutes `${CLAUDE_PLUGIN_DATA}` into command hooks only; the engine's own process
does not carry it. The mod looked for `bin/hook` under that variable or under a data directory
derived from an installed plugin's cache root, so under `--plugin-dir` (run 1, defect 4) and after
`adage init`'s local-marketplace install (the docs agent's `o12-q1-noenv.log`) it never ran its
binary and only the command hooks served.

Contract (`hooks/binary.ts` `dataDir`, `hooks/bridge.ts`):

- `CLAUDE_PLUGIN_DATA` when the engine's process carries it, as given (trailing separators dropped).
- Else the candidates `dataDirCandidates(root, name, registries)`, in order, the first whose
  `bin/hook` (`hook.exe` on Windows) exists (`$.fs.exists`); none: undefined, the mod stays inert
  and is probed again once the SessionStart shim has run (unchanged):
  1. an installed plugin's cache root `<config>/plugins/cache/<marketplace>/<plugin>/<version>` →
     `<config>/plugins/data/<plugin>-<marketplace>`, and then only that (either separator);
  2. under each config directory (`configDirs`: `CLAUDE_CONFIG_DIR` alone when set, else
     `<HOME>/.claude` and `<USERPROFILE>/.claude`), the data directory of each key the engine
     registered the plugin's folder under (`registeredKeys`): a marketplace in
     `plugins/known_marketplaces.json` whose `installLocation` or `source.path` holds the folder (a
     directory marketplace's plugin is read in place: `adage init`'s `adage@adage-local` from
     `$ADAGE_HOME/plugin/adage`, data `adage-adage-local`), or an entry of
     `plugins/installed_plugins.json` whose `installPath` is the folder;
  3. then `<config>/plugins/data/<name>-inline`, the `--plugin-dir` key `<name>@inline`.
  A key's non-`[A-Za-z0-9_-]` characters become `-`, as the engine names data directories. A key
  the folder is not registered under is never a candidate: its link may be another install's.
- Paths keep their own separator (`joinPath`, `sepOf`); a Windows drive path compares
  case-insensitively, either separator (`pathKey`).
- `register.ts` gives the mod `CLAUDE_CONFIG_DIR`, `HOME`, `USERPROFILE`, `$.plugin.name` and
  `$.fs.exists` (a minimal edit in a file the O6/O9 agent owns: four env getters, `name`,
  `exists`); `fixtures/kit.ts` gives `fake()` and `world()` the same (`present`, `name`, the env
  keys; owned by the run-2 fix agent, additive only). `scripts/check-plugin.mjs` checks the
  derivation is there (cache root, both registry files, `@inline`, the `bin/hook` check) and that
  `register.ts` reads the four variables.

The marketplace name is read from the engine's own files rather than "the most recently written
`bin/hook`": the engine says which key a folder is loaded under, the modification time only which
session ran last, and a second install's link (another `ADAGE_HOME`) must never be taken. Neither
`$.plugin` nor the debug log of 2.1.292 gives the mod its own key.

Tests: `binary.test.ts` (T-B1-03: as given; `--plugin-dir` with and without `CLAUDE_CONFIG_DIR`;
Windows `USERPROFILE`, backslashes, `hook.exe`, Git Bash's `HOME` beside it; the local marketplace,
its link before the inline one, none before the shim, never for a checkout; a published
marketplace read from a directory; an installed root alone), `bridge.test.ts` (T-B1-03:
`inlineDataDirs`, `configDirs`, `registeredKeys` over the files `adage init` leaves, Windows and
malformed input; `dataDirCandidates`), `session.test.ts` (activation through the engine for a
`--plugin-dir` plugin, with and without `CLAUDE_CONFIG_DIR`).

Live, both routes (`claude` 2.1.292; `$S` the scratchpad; the environment cleared of the host
session's variables, `ADAGE_*`, `CLAUDE_PLUGIN_DATA` and `CLAUDE_CONFIG_DIR` by `$S/lf-env.sh`, then
`HOME`, `ADAGE_HOME`, `CLAUDE_CONFIG_DIR` set under the run's directory; a fresh `demo/sandbox`
copy; the finish line `acme-billing-…/instincts finish-line-e7e809` granted `predict` as a person;
the prompt "add `po_number` to schema/invoice.json with the Edit tool, then stop"):

| route | data directory the mod used | debug log | spool |
| --- | --- | --- | --- |
| `adage init --yes --hooks user` (`target/debug/adage`, which embeds this tree's mod; `target/release/adage` predates it), `$S/lfinit` | `cfg/plugins/data/adage-adage-local` | `hooks module adage@adage-local …`; `$.process.run (adage): …/adage-adage-local/bin/hook` (probe, `Standing`, `TurnStep` per step); `adage (user) answered turn.step without next()`; `toolUseId=adage_finish-line-e7e809_…_2_0` | `turn_step` `predicted {kind: run, route: finish-line-e7e809, calls: [Bash make gen]}` at index 2 |
| `--plugin-dir` copy of `plugin/claude-code` with its `dist/` (run 1's binaries), `$S/lfdir` | `cfg/plugins/data/adage-inline` | 20 `$.process.run (adage)` lines on `…/adage-inline/bin/hook`; `answered turn.step without next()`; `toolUseId=adage_finish-line-e7e809_…_2_0` | the same record at index 2 |

Logs: `$S/lfinit/debug-init.log`, `stream-init.jsonl`; `$S/lfdir/debug-dir.log`, `stream-dir.jsonl`;
spools under each `adage-home/sessions/` (`$S/spool.py` prints them). Before the fix, the same setup
had no `$.process.run (adage)` line at all (run 1; `o12-q1-noenv.log`).

## 2. `adage-exec status --repo .` showed no grant

`adage-exec`'s verbs keyed a checkout `repo::repo_key` (`acme-billing@<root commit>`), while
`adage review delegate` writes a grant under the found reflex's `repo` and the hooks read it by
`repo::lookup_key` over the index (`acme-billing` when learned with `condition --repo
acme-billing`). One key now: `adage_exec::route::grant_key(root, paths)` is `lookup_key` over the
index's repositories, else the checkout's own cached `repo_key`; `status`, `revoke`, `approve`,
`run`, `completion`, `readiness` and `journal` all use it. `Store::status_text(key)` is what
`status` prints.

Tests: `crates/adage-exec/tests/cli.rs` `status_and_revoke_read_grants_under_the_key_the_index_holds`
(the binary's `status` and `revoke --mode predict` on a grant under `acme-billing` for a checkout
whose own key differs); `crates/adage-cli/tests/exec.rs`
`a_predict_grant_is_read_under_the_key_review_delegate_wrote_it` (through the CLI's write path:
`adage review delegate --predict --repo .` writes under the reflex's key, `status_text(grant_key)`
lists it, and a `TurnStep` under adage-exec's hook predicts with it, which reads it through
`predict_grants`). Live: `adage-exec status --repo .` in `$S/lfinit` prints `Prediction grants for
acme-billing@380101d397e9: 1 … finish-line-e7e809 E2 typed make gen`.

## 3. A schema edit through Bash was not an edit

Every run-1 schema edit was `python3 - <<'E' … E` (the batch saw `exec python3 -c {code}`), and run
2's heredoc edits did not arm the finish line either. `adage_core::shell::normalize_shell_observed`
is `normalize_shell` from the same parse plus, behind a flag in the walk:

- each file written, as `Write` (an append, `>>`/`tee -a`/`open(…, 'a'|'r+')`, as `Edit`), right
  after the action that wrote it: a redirection's target, one nested in a heredoc too
  (`cat <<'EOF' > f`); `tee`'s operands (also in a later pipeline stage); the files an inline
  Python program opens for writing (`python3 - <<'E'`, `python3 -c`): `open(<lit or name>, <writing
  mode>)`, `mode=`, `Path(<lit or name>).write_text|write_bytes`, a name bound to a literal (or
  `Path(lit)`) by an earlier statement, unbound by any other assignment; at most 32 files; never a
  descriptor, `/dev/…`, `-`, or anything with an expansion or a glob;
- the commands written after a heredoc's start (`cat > f <<'EOF' && make gen`), which the grammar
  nests inside the heredoc.

`adage_runtime::observe::batch_keys` uses it for a Bash call in a tool batch (`decide/batch.rs`);
failures, `PreToolUse` and everything learning, the graph and the classifiers read keep the plain
form. So the write arms triggers, contexts and the finish line as an Edit does, `make gen` later in
the same command meets it, and the spool's `observed` carries `write {path}` (a rebuilt state reads
it back).

Fast path: unchanged. A batch whose input names no trigger word is skipped before any parse, as
before; the observed walk runs only on batches already decided in full. Cost of the extra walk,
in-process, debug build, 2,000 alternating runs each (a throwaway test, removed): `ls -la` 12.4 vs
10.3 µs, `make test` 15.7 vs 29.5, `python3 -m pytest -q 2>&1 | tail -5` 48.9 vs 60.5, a heredoc
into a file 37.3 vs 42.9, run 1's `python3 - <<'E' … E && make gen && make ci` 118.6 vs 186.1,
`git status && git diff | head -50` 41.8 vs 41.8. `hook_latency` (debug, this tree, 30 runs, no
model): `batch_no_match` p50 4.0 ms, `batch_context_fires` p50 24.7 ms, `turn_step_nothing_pending`
p50 4.5 ms. A paired two-build `hook_latency` was not run: it needs a second build of the parent
commit, which the rules' one-`target/`, no-second-worktree rule leaves no way to make; the added
work is at most 0.07 ms against a 25 ms decided batch, inside its run-to-run spread (p95 42 ms).

Goldens byte-identical (`cargo test -p adage-cli --test hooks` without `ADAGE_BLESS`, 25 passed);
parity (`crates/adage-exec/tests/parity.rs`, 25 passed); the eval gates and parity
(`fast_path_changes_no_decision`, `fast_path_changes_no_decision_with_the_learned_index`), the L0
bench (`bench_defaults_reproduce_the_committed_baseline`), `predict_gate` and `paired` all pass
unchanged.

Tests: `adage-core` `shell::observed_tests` (each shape, the Python bindings, reads and computed
paths writing nothing, commands after a heredoc, the plain form unchanged); `adage-runtime`
`tests::bash_edits` (each live shape arms the finish line with the fast path deciding the same,
`write {path}` observed and in the history; the edit and `make gen` in one command meet it; a
context's `edit` item; a rebuilt state).

Live (`$S/lfdir`, `dist/` replaced by this tree's debug `adage` and `adage-exec`, `[runtime] control
= 0`): the prompt "run exactly this one Bash command: `python3 - <<'E'` … writes schema/invoice.json …
then stop" → batch `observed: [exec python3 -c {code}, write {path}]`, then `turn_step` `predicted
{kind: run, route: finish-line-e7e809, calls: [Bash make gen]}` at index 1, `answered turn.step
without next()`, `toolUseId=adage_finish-line-e7e809_…_1_0`, and `acme/generated/models.py`
regenerated (`$S/lfdir/debug-bash3.log`). The run before it (`debug-bash2.log`) drew the control
arm: the same arming, `would_predict` recorded, nothing yielded, as the control arm should.

## 4. A predict grant on an instinct's finish line

Decision: **neither refuse it nor arm an obligation under it; it stands, and is used where the
instinct is due.** The brief's premise ("can never be used") does not hold: an instinct's finish
line is advice, so it is never an obligation and never reaches the end-of-turn prediction (C8,
`ending: true`), but while its directive is pending, the step right after the edit, `predict_step`
predicts it (kind `run`). Every tool call is followed by a request, so that step always comes
unless the same batch already ran `make gen`. Run 1's probe and run 2's `mod-observe` (`would_predict
… finish-line-e7e809` at index 4 and 3) are that path; the two live sessions above predicted it.
Refusing the grant would remove a lever that works; arming an obligation under a grant would let a
grant make an instinct block Stop, against the instinct's contract ("each is prime at most, never
blocks the agent from stopping", as `adage init` tells the person). What is left unpredicted is the
lapsed advice (the follow window passed without the call) at the turn's end, which is the same
"advice, not obligation" boundary. Implemented as the comment in `decide/batch.rs` and two tests:

- `adage-runtime` `tests::finish_line::a_granted_instinct_finish_line_is_taken_at_the_step_after_the_edit_and_never_obliges`
  (due: predicted `run`, judged `predicted`, no Stop request; lapsed: no obligation, nothing at the
  turn's end, no Stop request);
- `adage-exec` `hook::grants::tests::a_predict_grant_on_an_instinct_finish_line_is_used_at_the_step_after_the_edit`
  (`predict_grants` names it; the hook predicts it at the step after the edit; nothing is run).

## Files

- `plugin/claude-code/hooks/binary.ts`, `bridge.ts`, `session.ts`, `mod.ts` and their tests
  (`binary.test.ts`, `bridge.test.ts`, `session.test.ts`); `register.ts` and `fixtures/kit.ts`
  (minimal, additive; see 1); `scripts/check-plugin.mjs` (the data-directory checks);
  `plugin/claude-code/VALIDATE.txt` (regenerated: `$.fs.exists` and the three env reads).
- `crates/adage-core/src/shell.rs`; `crates/adage-runtime/src/observe.rs`, `decide/batch.rs`,
  `tests/bash_edits.rs` (new), `tests/finish_line.rs`, `tests/mod.rs`.
- `crates/adage-exec/src/route.rs` (`grant_key`), `approvals.rs` (`status_text`), `main.rs`,
  `hook/grants.rs` (test), `tests/cli.rs`.
- `crates/adage-cli/tests/exec.rs` (the write-path test; in HEAD already, formatting only here).
- `CHANGELOG.md`: one bullet each under "The runtime and hooks", "Execution (opt-in)", "The Claude
  Code mod".

## Commands and results

- `cargo fmt --all -- --check`: clean on these files (the diffs it lists are in `crates/adage-cli`,
  the O6/O9 agent's files).
- `cargo clippy -p adage-runtime -p adage-core -p adage-exec -p adage-hook --all-targets -- -D
  warnings`: clean.
- `cargo test -p adage-runtime -p adage-core -p adage-exec -p adage-hook --no-fail-fast`: all pass
  (core 72+5+1+8, exec 139 + cli 9 + parity 25 + the rest, hook 62+2, runtime 120+1).
  `cargo test -p adage-cli --test hooks`: 25 passed. `cargo test -p adage-cli --test exec
  a_predict_grant`: 3 passed.
- `cargo test -p adage-eval --test gates --test parity --test bench --test predict_gate --test
  paired`: 15, 3, 4, 8, 1 passed.
- `cargo test -p xtask`: 24 passed.
- Pinned CLI 2.1.288: `claude plugin validate --strict plugin/claude-code`: passed. `claude plugin
  test plugin/claude-code`: 380 pass, 0 fail (final run; earlier runs had failures in other agents'
  in-progress `predict`/`step`/`/adage update` tests, since fixed by them). `VALIDATE.txt`
  regenerated with the pinned CLI as `ci.yml` diffs it, right before the report.
- `node scripts/check-plugin.mjs`: the data-directory checks pass; it fails on one problem in
  `hooks/step.test.ts` (the run-2 fix agent's describe names no test-plan id).
