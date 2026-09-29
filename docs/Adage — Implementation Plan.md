# Adage — Implementation Plan

**Status:** Phase 3, draft 4 (as built, plus the road to a public release) · **Date:** 2026-09-28 ·
**Builds:** *Adage — Architecture* draft 2 + spike results + slices 1–10 + Tracks C and G + the
lexicon (`docs/lexicon.md`), as merged on the maturity branch

Draft 1 planned slices 1–10. They are built, and so are the tracks that followed: the graph engine
(Track G, the only learner since G5), packs and execution (Track C: `adage-pack`, `adage-exec`,
E1 prefetch and E2 actions), instincts (`adage-instincts`) and training (`adage-train`). This
draft records what was built and how it differs from the plan, lists the known gaps, and plans the
next three phases:

- **Phase 4:** pilot on real history.
- **Phase 5:** packaging and distribution (npm, Homebrew, crates.io, harness marketplaces).
- **Phase 6:** licensing and legal.

Decisions that need you are collected in §9.

---

## 1. What the spikes settled

| Decision | Settled by | Outcome |
| --- | --- | --- |
| A1 Daemon | S3, S1 | **No daemon.** Hooks are short-lived processes over per-session state files and spools |
| A2 Language | S1 | **Rust.** Exec-form spawn + work ≈ 2 ms p95; a Node client is 35 ms p50 and ruled out |
| A3 Transport | S1, S3 | **Command hooks in exec form**; no socket needed without a daemon |
| A5 Storage | S3 | Hooks write per-session **spools + two-slot state files**; `adage condition` is the only index writer |
| A6 Embeddings | S2, S7 | **Static embeddings** (potion-base-8M, in-process, plain Rust). Hook p95 6.4 ms with the model loaded |
| A7 Signing | S6 | **Native Rust sshsig**, through ssh-agent; `ssh-key = 0.6.7` pinned |
| A8 Packaging | S5 | Binary in `${CLAUDE_PLUGIN_DATA}`, hooks call it by absolute path, pre-run at `SessionStart` |
| Normaliser | S8 | tree-sitter-bash + command table as data; 98.2% coverage, 315 fixtures |
| Clustering | S7 | **Exact kNN + deterministic Louvain, per repository**, then behavioural split/merge: precision 0.57 → 0.87 (synthetic labels); byte-identical under reordering; 17 s per 10k-prompt repository |
| Wording | S4 (partial) + demo | Factual, attributed wording was followed in 36/36 directed runs, with no suspicion or refusal language |

## 2. Repository as built

```text
crates/
  adage-core/       canonical actions, shell normaliser, effect classes, spans, repo identity
  adage-store/      spools + two-slot state files (no daemon), data directories, config
  adage-tracks/     track format, validation (tracks are untrusted input), index, interlocking, review.yaml
  adage-graph/      the graph engine (Track G): trace facts, mining, scoring, selection, compile to reflexes
  adage-learn/      transcript import (Claude Code incl. sub-agents, Codex, OpenCode export), the learner's driver
  adage-intent/     prompt normalisation, static embedding model, kNN + Louvain, split/merge
  adage-instincts/  shipped reflexes (instincts), bound from a repository's own files
  adage-runtime/    decisions, fixed-template rendering, control assignment, follow/ignore judgement
  adage-hook/       the hook pipeline shared by adage and adage-exec (adapters, fast path, fail-open)
  adage-pack/       packs (format adage-pack/1): validation, closure, digest, binding; `adage-pack` tool
  adage-sign/       native sshsig signing, allowed_signers verification
  adage-exec/       the executing binary: approvals, policy, OS sandbox, E1 prefetch, E2 actions
  adage-train/      the training binary: a skill → sandboxed agent sessions → reflexes
  adage-eval/       offline evals, the L0 bench, safety oracle, property tests
  adage-sim/        harness simulator (mock histories and native hook payloads)
  adage-cli/        the `adage` binary
plugin/claude-code/       the Adage plugin: exec-form hooks, checksum-verifying install shim
plugin/claude-code-exec/  the adage-exec plugin (execution on)
plugin/opencode/          bridge plugin (fail-open, no shell)
npm/                      the npm launcher and per-platform packages (adage, adage-exec, adage-train)
demo/                     sandbox generator, runner, live dashboard, results
evals/                    datasets, the bench suites and baseline, generated sandboxes, the live runner
spikes/S1–S8/             the experiments behind every decision above
```

The `adage` binary's commands (`adage --help` groups them; `adage <command> --help` for each):

```text
init · sense · install · doctor · condition · train · models · review · share · add · delegate ·
watch · ui · map · status · report · graph · config · eval · hook · hooks-config · version
```

Three binaries ship: `adage`, `adage-exec` (what `adage delegate` turns on) and `adage-train` (what
`adage train` runs). Test state: `cargo test --workspace --locked` runs the whole suite, including
the normaliser's golden fixtures, golden hook outputs per harness, hostile-payload suites, property
tests, the harness simulator's end-to-end replays, the eval gates and parity, the L0 bench against
its baseline, and the execution tests (which skip where no sandbox backend exists). CI runs them on
macOS arm64 and x64, Linux x64 and arm64, and Windows x64, with clippy `-D warnings`, rustfmt,
`cargo deny` and a NOTICE check; `evals.yml` runs the slow gates.

## 3. Slices: plan versus as built

| # | Slice | Status | Notes |
| --- | --- | --- | --- |
| 1 | Walking skeleton | ✅ | Real Claude Code sessions receive directives through the hook |
| 2 | Normaliser crate | ✅ | 315 fixtures in `cargo test` |
| 3 | Learn: import + segment | ✅ | Claude Code JSONL, plus OpenCode `export` files (slice 10b) |
| 4 | Learn | ✅ (replaced) | The five hand-written miners are gone: the graph engine (Track G) is the only learner since G5, and finds lookups, read batches, dead ends, finish lines, sequences, scoped checks and environment steps from the traces' own structure |
| 5 | Tracks + index | ✅ | Deterministic compile; tampered tracks rejected; effects re-derived, never trusted |
| 6 | Runtime decisions | ✅ | Ceilings, strict slot contracts, per-session caps, bounded Stop blocks, control arm |
| 7 | Claude Code plugin | ✅ | `claude plugin validate` passes; fails open with the binary missing |
| 8 | Demo | ✅ | 90 real Haiku 4.5 sessions, 2 learning rounds; results in `demo/results/` |
| 9 | Intent families | ✅ | See §4.3 for limits |
| 10a | Review queue | ✅ | `adage review`; decisions persist in `review.yaml` and survive re-learning |
| 10b | OpenCode + Codex adapters | ✅ built, ⚠️ not run live | Built from docs and source; neither harness installed here |
| 10c | Sharing + signing | ✅ | `adage share` emits a signed pack (redaction check, sshsig, namespace `adage-pack`) / `adage add` (trust on first use, forming). Packs are the only format that travels |
| — | `adage graph` + `adage map` | ✅ (added) | Stimuli → reflexes → steps → agent responses, live; judgements split by arm |
| — | Sense report (Design J1) | ✅ | `adage sense`: read-only report of what agents do, with the reflexes `adage condition` would form as candidates |
| — | Track G: graph engine | ✅ | G0–G5: trace facts, mining, scoring, compile to delivery shapes, context triggers; the only learner (docs/tracks/track-g-graph-engine.md) |
| — | Track C: packs and execution | ✅ (E2 beta) | Packs (C0/C1), `adage-exec` with approvals, policy and an OS sandbox (C2), E1 prefetch (C3), E2 actions behind policy (C5); `adage delegate`, `adage review delegate` (docs/packs.md, docs/execution.md) |
| — | Instincts | ✅ | Shipped reflexes bound from a repository's files on day 0; the core instincts pack (docs/instincts.md) |
| — | Training | ✅ | `adage train`: a skill or runbook → sandboxed agent sessions → staged reflexes, and which claims are stale (docs/training.md) |
| — | Terminal UI | ✅ | `adage watch`, `adage ui`, the interactive `adage init` wizard |
| — | Evals as a product | ⚠️ partial | Offline: `adage eval` (§3a). Live: `evals/live/runner.py` runs the with/without A/B on any sandbox and task set; not yet a product |
| — | First run: `init`, `doctor`, `map` | ✅ | Plan §6.3 |
| — | Release engineering (M1) | ✅ built | CI, `dist` releases, npm packages, marketplace, Windows port; first tagged release waits on secrets and D1–D4 |
| — | Harness simulator + Codex importer | ✅ | `crates/adage-sim`: mock sessions in each harness's native format, end-to-end replays; `adage condition --codex-sessions` |
| — | Team depots, lockfiles (Design §10.7, O12) | ❌ | Direct sharing works; depots do not |
| — | Live learning (Design §10.9, automatic tracks) | ✅ (added) | Hooks start a debounced, single-flight background learner per repository at each stop; `config.toml` / `adage config`; `learn.log` records each route change; parse cache. Architecture §4.7.2, docs/live-learning.md |

## 3a. Evals

`adage eval` learns from a deterministic train split and replays each held-out session through the
real `decide()` in the control arm. It then scores every would-have directive against what the agent
did later. Committed datasets and their gates (docs/evals.md):

| | demo round 2 (real) | synthetic | adversarial |
| --- | ---: | ---: | ---: |
| Precision, all | 0.74 | 0.93 | 0.73 |
| Recall of key steps | 0.78 | 1.00 | 0.57 |
| Turns saved by the lookup (median) | 9 | 1 | 0 |
| Finish-line accuracy | 1.00 | 1.00 | 1.00 |
| Unsafe directives | 0 | 0 | 0 of 84 checked |

- **The real dataset reproduces the live index.** It holds the 90 demo sessions, redacted. Learning
  from round 2's training split reproduces the live index's routes (ids and strengths; byte for
  byte before the dead-end and exit-condition changes), and replay reproduces 36 of 36 live
  decisions.
- **Offline precision understates correctness.** 6 of 18 lookup "misses" are agents that never found
  the tracker at all.
- **Dead ends are keyed by failing signature.** The eval found the dead-end route also warned
  against `python3 -m unittest`, which passes when `ACME_ENV` is set. The miner now names a runner
  only if its failures share the route's signature and it fails in 9 of 10 sessions that ran it;
  dead-end accuracy rose and precision is unchanged (docs/evals.md). Each further signature with
  enough support is a dead end of its own; the most common keeps the working command's id.

The property tests found 8 bugs, all fixed with regression tests:

1. A single quoted error line failed `learn` for the whole repository.
2. Bidirectional-override and zero-width characters passed validation.
3. A 15 KB `&&` chain overflowed the stack.
4. Pathological commands took 0.7 s to parse; parsing now stops after a fixed lexing budget (about 7 ms).
5. A track could direct commands naming paths outside the repository.
6. Unfillable slots were sent as literal `{item}`.
7. A blank command passed validation.
8. A torn state write could resurrect a state two writes old.

### Where the build deviated from the design, deliberately

- **Imports live apart.** `adage add` installs under `tracks/@imported/`, which `adage condition` never
  writes. Where a learned route and an imported one share an id, the index keeps the learned one.
- **Review decisions** live in a per-track `review.yaml`, not on `Route`, so they travel with a
  shared track without changing the route schema. A recipient's `adage add` does not install the
  author's decisions; recipients review for themselves.
- **"Material change"** (what re-opens a decision):
  - trigger, step, slots, effect, exit or strength changed;
  - Wilson lower bound moved ≥ 0.10;
  - support doubled or halved.
- **Nothing overrides the agent-session refusal.** `share`, `add`, review decisions and `install
  --write` refuse whenever `CLAUDECODE` is set. `ADAGE_ALLOW_NONINTERACTIVE=1` lifts only the
  terminal check, for CI.
- **Codex needed no `AGENTS.md` fallback.** Its hooks use Claude Code's contract, including
  `additionalContext`.

## 4. Known gaps

### 4.1 Correctness and product gaps

Closed since draft 2:

- Proposals for pinned routes (`adage review` reason "proposed").
- Review decisions survive re-import.
- `gauge.harness` records the real harness.
- Passphrase prompts at a TTY.
- `adage sense` report.
- Dead ends are keyed by failing signature: a runner is named only if its failures share the
  route's signature and it fails in 9 of 10 sessions that ran it.
- Hooks record caught panics and internal errors in `$ADAGE_HOME/fail-open.log` (class, fixed
  message or panic location, never payload; rotated at 256 KiB), and `adage doctor` reports them.
- Normalisation no longer depends on machine speed: the 50 ms parse timeout is replaced by a
  deterministic budget of bytes the lexer may read (2^20), with a 1 s backstop for unknown shapes.
- One dead end per (working command, failure signature), not per working command. The most
  common signature keeps the id derived from the working command (`dead-end-d41b1e`), so the
  live-index gate holds; round 2 adds `dead-end-21cb2c` (`python -m pytest`). Recall: `hash:0.3`
  0.81 → 0.84, synthetic 0.97 → 1.00; precision unchanged or up. Adversarial recall stays 0.57:
  its pytest failures carry no renderable error line, so no signature (docs/evals.md).
- Repositories without `origin` are keyed `<directory name>@<root commit>`, so unrelated ones
  with the same name no longer share routes. The root commit is read from the object store
  without running git (loose objects, v2 packs with deltas, packed refs, worktrees; new
  dependency `miniz_oxide`), and remembered per path in `$ADAGE_HOME/repo-keys`, which `adage
  condition` fills. Unreadable roots (no commits, SHA-256, shallow, alternates) fall back to a hash
  of the path, which copies do not share. Bare-name keys from before still apply until a
  repository of that name is re-keyed. Hook p50/p95 unchanged within 0.1–0.2 ms; a cold cache
  costs one walk (38 ms for 5,000 packed commits, about 0.45 s for 50,000).
- Linked worktrees share their repository's key: `origin` is read from the common git directory
  (`gitobj::dirs`), not `<root>/.git/config`, which a worktree does not have, so a worktree is no
  longer keyed `<worktree dir>@<root commit>` apart from its main checkout. `adage learn` and
  the end-of-turn hook remember each checkout's key in `$ADAGE_HOME/repo-keys/<hash>.key`
  (lookups write nothing), so the sessions of a removed worktree keep their repository at the
  next learn, even one that runs after the removal. The next learn learns worktree routes again
  under the repository's key; tracks from before under a worktree key stay, unmatched.
  `origin_key` costs 8.6 µs in a main checkout (8.2 before) and
  23.5 µs in a worktree, where it used to fail and fall back to the root commit.
- A state file that can no longer be read (both slots fail their CRC, or the payload no longer
  parses) is rebuilt from the session's spool (at most 4 MiB, 10,000 records): arm, repository,
  counters, emitted routes, history, finish-line obligations and stop requests. Spool records now
  carry `kind` and `repo`; older ones are mapped by event name. The watermark fold S3 also
  proposed is not built: the state stays authoritative while readable.
- The no-match fast path (Design §13.2): the index stores per-repository triggers, and the hook
  skips an event that cannot change a decision before the command table, state lock and spool. A
  no-match batch: 2.82/2.93 ms p50/p95 → 1.87/1.96 ms (a repository without routes: 1.76/1.87).
  The eval replay proves it on every event of every dataset (0 differences; 45–72% of events
  skipped). Skipped events leave no spool record, so `adage graph` lists fewer off-route commands;
  runs of avoided commands, and every event of a session with a pending judgement or obligation,
  are still recorded.
- Payloads over 8 MiB are read to the end and discarded (Design §13.3), so the harness never sees
  a broken pipe; with a kill switch on too. The refusal is recorded in `fail-open.log`.
- Design W8 (spill directives over the harness cap to a file) cannot arise, so it is not built:
  with the longest fields validation accepts, the largest decision (three directives) is 8,287
  bytes, under every harness's 9,500. A test (`the_largest_decision_fits_every_harness_cap`) pins
  the bound and each limit it rests on; raising one fails it. Truncation stays as a backstop.
- `adage condition` appends a one-line JSON summary per run to `learn.log` (rotated at 256 KiB).
- **Live learning** (branch `live-learn`): learning no longer waits for a person to run
  `adage condition`. At Claude Code and Codex `Stop`/`SessionEnd` and OpenCode
  `session.idle`/`session.deleted`, after its decision, the hook writes a request marker
  (`learn-queue/<repo>.json`) and starts a detached `adage condition --auto` for the repository,
  debounced per `learn.min_interval_secs` (a request inside the interval starts a learner that
  waits, so none is lost) and single-flight per repository (`<repo>.lock`); `tracks.lock`
  serialises every writer of tracks and the index. The learner learns only that repository, and
  writes what a full learn writes for it. Stop p95 2.04 → 2.36 ms (learner running), 2.77 ms
  when it starts one (`hook_bench`). Every learn appends per-repository route changes (`new`,
  `nascent`, `extinguished`, `strength`, `evidence`, `proposal`) to `learn.log`; failures are
  `condition_error` lines. `$ADAGE_HOME/config.toml` with `adage config` (`learn.auto`,
  `min_interval_secs`, `thresholds`, sources, `parse_cache`, `runtime.control`; environment
  wins). A parse cache keyed by (path, size, mtime) makes re-learning 207 real transcripts
  60 ms instead of 300–670 ms, byte-identical. `doctor` reports auto-learn state, last runs, stuck
  locks and backlog. `adage_store::spool::read_from` tails spools for live views.
- CI builds the aarch64 musl release target natively on the arm64 runner (`musl-tools`, with
  `CC_aarch64_unknown_linux_musl=musl-gcc`) and checks it is static and runs. Written, and
  type-checked locally with `scripts/cross-check.sh`; its first CI run is the real test.

Open:

| Gap | Impact | Fix |
| --- | --- | --- |
| The command table is parsed, and its 57 regexes compiled, at every decided event (about 1 ms of the 3 ms full path) | Every event the fast path cannot skip pays it | Compile pattern regexes lazily, or once per build |
| Events the no-match fast path skips leave no spool record | `adage graph` and the map list only recorded commands, so fewer off-route ones; route metrics are unaffected | If the off-route view matters, record a light entry for skipped calls (needs the cheaper table above to carry keys) |
| A failing runner whose error output has no renderable line (quotes, shell syntax, invisible characters) has no signature, so no dead end names it | Adversarial dead-end recall 3/30; overall recall 0.57 | Cut an unrenderable line at its first unrenderable character, with a minimum length, if that keeps the safety oracle at 0 |
| Per-session nonce (review finding F12) and arm assignment by root session lineage (F7) were accepted but not built | `/clear` starts a new arm | Build both |
| Automatic learning reads only configured sources; `adage condition` can be given others (`--transcripts`, `--codex-sessions`) | A route learned by hand from an unconfigured source can lose that evidence at the next automatic learn | Set `learn.codex_sessions` / `learn.opencode_exports` for sources you learn from; or remember manual sources |
| The first automatic learn with an empty parse cache parses the whole Claude Code history (every repository), as `adage condition` does | One slow background learn (about 1 s per 4,000 sessions of the simulator's size), after which each learn reads only what changed | Pre-filter by project directory before parsing, if histories get large |
| A learner is detached with a new process group, not a new session (`setsid` needs `libc`) | A terminal hang-up that signals the whole session could stop a learner mid-run. Hooks keep the previous index (it is replaced atomically, last); a track directory left half-written is rewritten by the next learn | Add `setsid` if it shows up |

Fixed from the architecture audit (draft 3):

- **Agent-session refusal:** `share`, `add` and review decisions now refuse inside OpenCode and
  Codex sessions too, not only Claude Code.
- **Track digest:** now follows the S6 spec exactly:
  - no symlinks;
  - hidden files are skipped;
  - a lone `\r` is normalised;
  - line endings are normalised on raw bytes, never through a lossy UTF-8 decode that could map
    different content to the same digest.
- **Linux plugin binary:** the plugin shim finds the released `linux-musl` binaries.

Fixed on branch `gaps-2` (merged):

- **Design O11:** learned environment-effect routes start as nascent reflexes and wait for approval in
  `adage review`.
- **Design W2:** dead-end and linked-file directives state an exit condition.

Fixed in the launch polish (2026-09-28), from dogfooding on the traces of Adage's own development:

- **Sub-agent traces.** Claude Code writes each sub-agent's transcript to
  `<project>/<session>/subagents/agent-*.jsonl`, every record a sidechain; they were never read, so
  multi-agent work was invisible. They are now traces of their own (sidechain records inside a
  main transcript are still skipped).
- **Worktrees.** A linked worktree reads `origin` from its common git directory, and a trace from
  a removed Claude Code agent worktree (`<repo>/.claude/worktrees/<name>`) keeps its repository's
  key, with paths relative to the worktree.
- **Steering.** Only hook output marks a trace steered, never an edited file that quotes the
  wrapper.
- **Noise.** A heredoc program (`python3 - <<'EOF'`) is inline code (`python3 -c {code}`), not a
  bare `python3`; `cargo fmt --check` and `black --check` are checks, not regenerations;
  housekeeping (`wc`, `ls`, `echo`, `cat`, `mkdir`, `sleep`, …) and a command after itself are
  never directed; edits outside the repository never make a glob; instincts skip test fixtures.
- **One edit, one directive.** A sequence that begins with a finish line's command carries its
  obligation (met by the first step, a Stop request if not), and the finish line says nothing
  itself.

### 4.2 Harness limits (from `docs/harnesses.md`)

| Harness | What cannot work today | Why |
| --- | --- | --- |
| Codex | After-failure routes (authored or imported) | Hooks receive no failure signal (openai/codex#34289, still so at rust-v0.156.1); the adapter already accepts one if added. Learned dead ends are unaffected: they are announced at the first prompt, and are learned from Codex rollouts, which record exit codes |
| Codex | ~~Learning from history~~ | Built: `adage condition --codex-sessions` reads rollouts, format read from source (rust-v0.156.1, older shapes at rust-v0.40.0); undocumented, so a release may change it |
| OpenCode | A true finish-line Stop block | No stop hook (#16626); Adage sends a synthetic prompt on `session.idle` |
| OpenCode | Learning from on-disk history | The SQLite schema is known from source but undocumented and migrating; `opencode export` files work, and `opencode session list --format json` plus `export` exports a whole history with documented commands |
| Codex | Hooks on Windows (unverified) | `adage install codex` quotes the binary path for a POSIX shell (`'…'`); which shell Codex uses for hook commands on Windows is not yet checked |
| Both | Invisible directives | Directives are visible to the user in both UIs |
| Both | Verified only in simulation | The harness simulator (`crates/adage-sim`) proves learning and delivery against the formats as read from source; the harnesses themselves have not been run (docs/harnesses.md, Verification) |

### 4.3 Intent families

- **Precision is measured on synthetic data only.** On synthetic behaviour labels, 0.87 of the
  directives given were correct (0.57 before split/merge). Real-history precision is the first
  thing the pilot measures (§5).
- **Label agreement is low.** ARI ≥ 0.8 is unreachable with potion-base-8M: an oracle that knows
  the true families reaches only 0.75. If pilot precision is short, the fix is bge-small (S2: 0.930
  vs 0.852 top-1), at the cost of a larger model and slower hooks.
- **Intent routes bind no slots.** Prompts carrying an ID stay with the ticket route.

### 4.4 Platforms

**Windows is ported and CI-verified** on windows-2022, and Linux on x64 and arm64. Running the
harness simulator there found two Windows-only bugs, both fixed with platform-independent tests:

- Paths under `\\?\` roots failed to relativize.
- `resolve_path` treated `C:\…` paths as relative.

Together they meant finish-line routes never fired on Windows. Claude Code's plugin hook on Windows
(Git Bash) is still unverified.

## 5. Phase 4 — Pilot on real history

**Gate before anything else:** decision O9. You need written IP clearance from your employer
before Adage runs on the work machine or learns from work transcripts. Nothing in this phase
starts without it.

The pilot follows Design §18 (pilot definition):

1. **Sense.** Run `adage condition` over existing Claude Code history, read-only, with no hooks
   installed. Gates:
   - ≥ 95% of calls normalise;
   - the reflexes look right to you in `adage review`;
   - `adage map` shows a plausible map.
2. **Shadow.** Install the plugin at `ADAGE_CONTROL=1.0`. Everything is computed and logged, and
   nothing is delivered. Checks:
   - hook p95 stays under 25 ms;
   - no fail-open incidents;
   - would-have rates per route.
3. **Treated, 50/50.** Run 4–6 weeks, aiming for ≥ 1,500 triggered sessions:
   - Pre-registered primary: time to first useful edit down ≥ 20%.
   - Guardrails: corrections and CI failures non-inferior (+1 point), net cost not worse.
   - Intent-family precision measured from follow/ignore on intent routes.
4. **Read-out.** Publish the graph snapshot and the stats, then go or no-go for a public beta.

## 6. Phase 5 — Packaging and distribution

Goal: `npm i -g <name>`, `brew install`, `cargo install`, and one-command harness setup, with
signed, reproducible, attested binaries and no telemetry.

### 6.1 Release engineering (first)

| Item | Plan |
| --- | --- |
| CI | GitHub Actions: `cargo test`, `clippy -D warnings`, fixture gate, `cargo deny` (licenses + advisories), plugin validate, OpenCode plugin test under node |
| Build matrix | macOS arm64 + x64 · Linux x64 + arm64 (musl, static) · Windows x64 (after the §4.4 port) |
| Release tool | [`dist`](https://github.com/axodotdev/cargo-dist) (cargo-dist). It builds the matrix, SHA-256 sums, a GitHub Release, a shell/PowerShell installer, a Homebrew formula and npm packages from one config |
| Supply chain | Keyless Sigstore signing, attached to releases. `npm publish --provenance` from Actions. SBOM (CycloneDX). `Cargo.lock` committed and `--locked` builds |
| macOS | Developer ID signing + notarisation for downloaded binaries (Apple Developer Program, $99/yr). npm-installed binaries are not quarantined, so this can follow the first beta |
| Versioning | SemVer for the binary. Tracks carry their own format version, with a compatibility policy: a newer binary reads older tracks; an older binary refuses newer tracks and fails open |

### 6.2 Channels

| Channel | How | Name status (checked 2026-09-23) |
| --- | --- | --- |
| **npm** | A thin main package with per-platform `optionalDependencies` (`os`/`cpu` fields), as esbuild and biome do. No postinstall download, so it works with `--ignore-scripts`, offline mirrors and corporate proxies | `adage` is **taken** (an inactive 2022 "Unix fortune" package). `adage-cli` and `adage-ai` are free. The `@adage` scope could not be checked from outside and must be claimed manually |
| **crates.io** | Publish the workspace crates; `cargo install adage-cli` | `adage` and `adage-cli` are **free**. Reserve them early |
| **Homebrew** | Tap `stevenvaughanbooth/homebrew-tap`, generated by `dist` | — |
| **Claude Code** | A plugin marketplace (`.claude-plugin/marketplace.json` in the repo): `/plugin marketplace add stevenvaughanbooth/adage`, then install `adage` (and `adage-exec` to let reflexes act). The shim uses a bundled binary it can verify against the plugin's SHA256SUMS, else `adage` on `PATH` (npm's launcher included); it never downloads anything | — |
| **OpenCode** | Publish `plugin/opencode` as an npm package (e.g. `opencode-adage`), loaded through the `plugin` key in `opencode.json`. `adage install opencode` already writes it | — |
| **Codex** | `adage install codex --write` (project or user `hooks.json`); nothing to publish | — |

The binary stays `adage` whatever the package names are.

### 6.3 Product work before a public beta

- **First run.** `adage init`, one command that:
  - detects the installed harnesses;
  - prints the install snippets;
  - offers `adage models fetch`;
  - runs `adage sense`.
- **Diagnostics.** `adage doctor` checks the binary, the hooks, the index, the model, and fail-open
  status.
- **Sense report (J1).** A readable "what your agents actually do" report, generated from `graph`.
- **map without the demo.** `adage map` serves the live page from the binary, so
  no Python demo server is needed.
- **Architecture draft 3, and user docs:**
  - a quick start per harness;
  - the track format reference;
  - privacy: everything stays local, nothing is uploaded, no telemetry.
- **Close the §4.1 gaps.**

## 7. Phase 6 — Licensing and legal

| Step | What | Who |
| --- | --- | --- |
| L0 | **Employer IP clearance (O9)**, in writing, covering a personal project built on personal equipment and time, including its later use at work. Keep the git history personal-only | You |
| L1 | ✅ **Apache-2.0** (D1). `LICENSE` and a generated `NOTICE` (third-party licenses, `scripts/notice.sh`) are at the repository root and ship in every release archive | Done |
| L2 | **Third-party notices.** All Rust dependencies are permissive: 60× MIT OR Apache-2.0, 27× Apache-2.0 OR MIT, 10× MIT, BSD-2/3, 0BSD, ISC and similar. There is no copyleft; the one LGPL option is an OR choice. `cargo about` generates the NOTICE, and `cargo deny` enforces an allowlist in CI | Me |
| L3 | **Model.** potion-base-8M is MIT. It is fetched, not bundled; attribute it in NOTICE. bge-base is also MIT, if we switch | Me |
| L4 | ✅ **DCO sign-off** (D2): `git commit -s`; CI checks pull-request commits; no CLA | Done |
| L5 | **Trademark.** "Adage" is a common word, and the mark may be narrow (Design §17.5). Run a knockout search (USPTO, EUIPO, classes 9 and 42) before spending on brand. Secure the GitHub org, npm scope, crates names and a domain. "On rails" stays a lowercase phrase; harness names are used descriptively only | You, with a search service if needed |
| L6 | **Project hygiene:** `SECURITY.md` (private reporting; tracks are an attack surface), `CONTRIBUTING.md`, code of conduct, and a privacy statement | Me |
| L7 | **Pack licensing.** Shared packs are data that others copy. Add an SPDX `license` field to the pack manifest; `adage share` asks for it and `adage add` shows it | Me (small format change) |

### License options for D1

| Option | Good for | Trade-off |
| --- | --- | --- |
| **Apache-2.0** (recommended) | Explicit patent grant; enterprise-friendly; matches most of the Rust dependency tree | Anyone may build a competing hosted product |
| **MIT OR Apache-2.0** | The Rust convention; maximum reuse | Same as above; slightly more licensing text to maintain |
| **FSL-1.1-Apache-2.0** (source-available, converting to Apache after 2 years) | Keeps the option to commercialise team features while the code stays readable | Not OSI "open source", so some companies and communities will not adopt or contribute |
| Open core: Apache-2.0 engine + a separate license for team depots and a hosted registry | Separates community adoption from revenue | Two codebases or licenses to keep straight; decide before depots are built |

## 8. Milestones

| Milestone | Contents | Exit |
| --- | --- | --- |
| **M0 — Clearance** | L0, D1–D2 decided, license files, name reservations | Written clearance on file; `LICENSE` merged |
| **M1 — Release engineering** ✅ built | CI, `dist`, build matrix, Windows port, SBOM, signing | A tagged release produces attested binaries for every target (needs `NPM_TOKEN`, `HOMEBREW_TAP_TOKEN`, the tap repo, and D1–D4) |
| **M2 — Pilot** | Phase 4 on real history | Primary metric and guardrails read out |
| **M3 — Channels + first run** | npm, Homebrew, crates.io, Claude Code marketplace, OpenCode npm plugin; `init`, `doctor`, `adage sense` report (first run ✅ built) | A new user goes from `npm i -g` to their first directive in under 5 minutes |
| **M4 — Public beta** | Docs, `SECURITY.md`, live adapter validation on OpenCode and Codex | — |
| **M5 — 1.0** | Team depots + lockfiles (O12), evals as a product | — |

## 9. Decisions

| # | Decision | Recommendation |
| --- | --- | --- |
| **O9** | Employer IP clearance | Get it in writing before M2 or any public release |
| **D1** | License | ✅ **Apache-2.0** (decided 2026-09-23). Open core is still to decide before building depots |
| **D2** | DCO or CLA | ✅ **DCO sign-off** (decided 2026-09-23) |
| **D3** | npm name | ✅ **`@adage/cli`** once the `@adage` scope is claimed (`npm org create adage`); fallback `adage-cli` (decided 2026-09-23). The binary stays `adage` |
| **D4** | Apple Developer ID ($99/yr) | ✅ **Yes** (decided 2026-09-23). Enrol, add the signing secrets, set `macos-sign = true` in `dist-workspace.toml`; required before Homebrew and direct downloads |
| **D5** | Private or public beta first | A private beta with a few teams after the pilot, then public |
