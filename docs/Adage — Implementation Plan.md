# Adage — Implementation Plan

**Status:** Phase 3, draft 3 (as built, plus the road to a public release) · **Date:** 2026-09-23 ·
**Builds:** *Adage — Architecture* draft 2 + spike results + slices 1–10 as merged on `main`

Draft 1 planned slices 1–10. They are built. This draft records what was built and how it differs
from the plan, lists the known gaps, and plans the next three phases:

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
| A5 Storage | S3 | Hooks write per-session **spools + two-slot state files**; `adage learn` is the only index writer |
| A6 Embeddings | S2, S7 | **Static embeddings** (potion-base-8M, in-process, plain Rust). Hook p95 6.4 ms with the model loaded |
| A7 Signing | S6 | **Native Rust sshsig**, through ssh-agent; `ssh-key = 0.6.7` pinned |
| A8 Packaging | S5 | Binary in `${CLAUDE_PLUGIN_DATA}`, hooks call it by absolute path, pre-run at `SessionStart` |
| Normaliser | S8 | tree-sitter-bash + command table as data; 98.2% coverage, 315 fixtures |
| Clustering | S7 | **Exact kNN + deterministic Louvain, per repository**, then behavioural split/merge: precision 0.57 → 0.87 (synthetic labels); byte-identical under reordering; 17 s per 10k-prompt repository |
| Wording | S4 (partial) + demo | Factual, attributed wording was followed in 36/36 directed runs, with no suspicion or refusal language |

## 2. Repository as built

```text
crates/
  adage-core/      canonical actions, shell normaliser, effect classes, spans, repo identity
  adage-store/     spools + two-slot state files (no daemon), data directories
  adage-tracks/    track format, validation (tracks are untrusted input), digest, index, review.yaml
  adage-sign/      native sshsig signing, allowed_signers verification
  adage-intent/    prompt normalisation, static embedding model, kNN + Louvain, split/merge
  adage-learn/     transcript import (Claude Code, OpenCode export), miners, intent families
  adage-runtime/   decisions, fixed-template rendering, holdout assignment, follow/derail judgement
  adage-cli/       the `adage` binary; harness adapters in src/harness/ (claude, opencode, codex)
plugin/claude-code/  plugin manifest, exec-form hooks, checksum-verifying install shim
plugin/opencode/     132-line bridge plugin (fail-open, no shell)
demo/                sandbox generator, runner, live dashboard, signal box (/brain), results
spikes/S1–S8/        the experiments behind every decision above
```

The `adage` binary exposes these commands:

```text
init · survey · learn · status · report · graph [--serve] · review · eval · doctor · share · add · install · models · hooks-config · hook · version
```

Test state: 211 tests pass (`cargo test --workspace`), including 315 normaliser fixtures, golden
hook outputs per harness, hostile-payload suites, property tests, the harness simulator's
end-to-end replays, and the eval gates (§3a). CI runs them on macOS arm64 and x64, Linux x64 and
arm64, and Windows x64, with clippy `-D warnings`, rustfmt, and `cargo deny`.

## 3. Slices: plan versus as built

| # | Slice | Status | Notes |
| --- | --- | --- | --- |
| 1 | Walking skeleton | ✅ | Real Claude Code sessions receive directives through the hook |
| 2 | Normaliser crate | ✅ | 315 fixtures in `cargo test` |
| 3 | Learn: import + segment | ✅ | Claude Code JSONL, plus OpenCode `export` files (slice 10b) |
| 4 | Learn: miners | ✅ | Prompt → lookup, linked-file batches, dead ends (red), finish-line obligations |
| 5 | Tracks + index | ✅ | Deterministic compile; tampered tracks rejected; effects re-derived, never trusted |
| 6 | Runtime decisions | ✅ | Ceilings, strict slot contracts, per-session caps, bounded Stop blocks, holdout arm |
| 7 | Claude Code plugin | ✅ | `claude plugin validate` passes; fails open with the binary missing |
| 8 | Demo | ✅ | 90 real Haiku 4.5 sessions, 2 learning rounds; results in `demo/results/` |
| 9 | Intent families | ✅ | See §4.3 for limits |
| 10a | Review queue | ✅ | `adage review`; decisions persist in `review.yaml` and survive re-learning |
| 10b | OpenCode + Codex adapters | ✅ built, ⚠️ not run live | Built from docs and source; neither harness installed here |
| 10c | Sharing + signing | ✅ | `adage share` (redaction check, sshsig) / `adage add` (trust on first use, probation) |
| — | `adage graph` + signal box | ✅ (added) | Live state as a rail network; judgements split by arm |
| — | Survey report (Design J1) | ✅ | `adage survey`: read-only report of what agents do, with the routes learn would build as candidates |
| — | Proving ground as a product | ⚠️ partial | Offline: `adage eval` (§3a). Live: `evals/live/runner.py` runs the with/without A/B on any sandbox and task set; not yet a product |
| — | First run: `init`, `doctor`, `graph --serve` | ✅ | Plan §6.3 |
| — | Release engineering (M1) | ✅ built | CI, `dist` releases, npm packages, marketplace, Windows port; first tagged release waits on secrets and D1–D4 |
| — | Harness simulator + Codex importer | ✅ | `crates/adage-sim`: mock sessions in each harness's native format, end-to-end replays; `adage learn --codex-sessions` |
| — | Team depots, lockfiles (Design §10.7, O12) | ❌ | Direct sharing works; depots do not |

## 3a. Evals

`adage eval` learns from a deterministic train split and replays each held-out session through the
real `decide()` in the holdout arm. It then scores every would-have directive against what the agent
did later. Committed datasets and their gates (docs/evals.md):

| | demo round 2 (real) | synthetic | adversarial |
| --- | ---: | ---: | ---: |
| Precision, all | 0.73 | 0.91 | 0.73 |
| Recall of key steps | 0.78 | 1.00 | 0.77 |
| Turns saved by the lookup (median) | 9 | 1 | 0 |
| Finish-line accuracy | 1.00 | 1.00 | 1.00 |
| Unsafe directives | 0 | 0 | 0 of 84 checked |

- **The real dataset reproduces the live index.** It holds the 90 demo sessions, redacted. Learning
  from round 2's training split reproduces the live index byte for byte, and replay reproduces
  36 of 36 live decisions.
- **Offline precision understates correctness.** 6 of 18 lookup "misses" are agents that never found
  the tracker at all.
- **One finding still to act on:** the dead-end route also warns against `python3 -m unittest`, which
  passes when `ACME_ENV` is set. The miner should key dead ends on the failing signature.

The property tests found 8 bugs, all fixed with regression tests:

1. A single quoted error line failed `learn` for the whole repository.
2. Bidirectional-override and zero-width characters passed validation.
3. A 15 KB `&&` chain overflowed the stack.
4. Pathological commands took 0.7 s to parse; parsing is now capped at 50 ms.
5. A track could direct commands naming paths outside the repository.
6. Unfillable slots were sent as literal `{item}`.
7. A blank command passed validation.
8. A torn state write could resurrect a state two writes old.

### Where the build deviated from the design, deliberately

- **Imports live apart.** `adage add` installs under `tracks/@imported/`, which `adage learn` never
  writes. Where a learned route and an imported one share an id, the index keeps the learned one.
- **Review decisions** live in a per-track `review.yaml`, not on `Route`, so they travel with a
  shared track without changing the route schema. A recipient's `adage add` does not install the
  author's decisions; recipients review for themselves.
- **"Material change"** (what re-opens a decision):
  - trigger, step, slots, effect, exit or aspect changed;
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
- The survey report.

Open:

| Gap | Impact | Fix |
| --- | --- | --- |
| Dead-end routes are keyed by runner, not by failing signature | Over-broad warnings (eval: `unittest` passes when `ACME_ENV` is set) | Key dead ends on (command, signature); only warn for runners whose failures share it |
| Nothing writes `$ADAGE_HOME/fail-open.log` | `adage doctor` cannot report hook failures | Record caught panics and errors in the hook's fail-open path (bounded, rotating) |
| The parse timeout (50 ms) makes normalisation depend on machine speed for pathological commands | Rare nondeterminism in `learn` for such commands | Replace with a node-count budget |
| Repository key collides for unrelated repos with no `origin` and the same directory name | Routes from one such repo reach the other (directives still validated) | Key no-origin repos by directory name plus the root commit, read without running git in the hook |
| State files are never rebuilt from the spool (S3 recommended a watermark and rebuild on CRC failure) | If both slots are corrupt, a session's state resets | Rebuild from the spool, or document state as a best-effort cache |
| Directive text over 9,500 bytes is truncated, not spilled to a file (Design W8) | Only reachable with many long directives at once | Spill to a file and point to it |
| Payloads over 8 MiB are not drained from stdin (Design §13.3) | The harness may see a broken pipe | Drain and discard |
| No precomputed no-match key set (Design §13.2) | Every event in a repository with routes pays the full decision path (about 3 ms) | Precompute trigger keys in the index |
| Per-session nonce (review finding F12) and arm assignment by root session lineage (F7) were accepted but not built | `/clear` starts a new arm | Build both |
| CI builds only x64 musl; arm64 musl is built only at release | A release-only failure is possible | Add aarch64 musl to CI |
| `learn.log` is defined but never written | — | Write it, or remove the path |

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

Fixed on branch `gaps-2`, and not yet merged:

- **Design O11:** environment-effect routes wait for approval.
- **Design W2:** dead-end and linked-file directives state an exit condition.

### 4.2 Harness limits (from `docs/harnesses.md`)

| Harness | What cannot work today | Why |
| --- | --- | --- |
| Codex | After-failure routes (authored or imported) | Hooks receive no failure signal (openai/codex#34289, still so at rust-v0.156.1); the adapter already accepts one if added. Learned dead ends are unaffected: they are announced at the first prompt, and are learned from Codex rollouts, which record exit codes |
| Codex | ~~Learning from history~~ | Built: `adage learn --codex-sessions` reads rollouts, format read from source (rust-v0.156.1, older shapes at rust-v0.40.0); undocumented, so a release may change it |
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

1. **Survey.** Run `adage learn` over existing Claude Code history, read-only, with no hooks
   installed. Gates:
   - ≥ 95% of calls normalise;
   - the mined routes look right to you in `adage review`;
   - `adage graph` shows a plausible rail network.
2. **Shadow.** Install the plugin at `ADAGE_HOLDOUT=1.0`. Everything is computed and logged, and
   nothing is delivered. Checks:
   - hook p95 stays under 25 ms;
   - no fail-open incidents;
   - would-have rates per route.
3. **Treated, 50/50.** Run 4–6 weeks, aiming for ≥ 1,500 triggered sessions:
   - Pre-registered primary: time to first useful edit down ≥ 20%.
   - Guardrails: corrections and CI failures non-inferior (+1 point), net cost not worse.
   - Intent-family precision measured from follow/derail on intent routes.
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
| **Claude Code** | A plugin marketplace (`.claude-plugin/marketplace.json` in the repo): `/plugin marketplace add stevenvaughanbooth/adage`, then install. The shim looks for `adage` in `${CLAUDE_PLUGIN_DATA}`, then on `PATH`, and otherwise downloads the pinned release and checks it against SHA256SUMS in the plugin | — |
| **OpenCode** | Publish `plugin/opencode` as an npm package (e.g. `opencode-adage`), loaded through the `plugin` key in `opencode.json`. `adage install opencode` already writes it | — |
| **Codex** | `adage install codex --write` (project or user `hooks.json`); nothing to publish | — |

The binary stays `adage` whatever the package names are.

### 6.3 Product work before a public beta

- **First run.** `adage init`, one command that:
  - detects the installed harnesses;
  - prints the install snippets;
  - offers `adage models fetch`;
  - runs a survey `learn`.
- **Diagnostics.** `adage doctor` checks the binary, the hooks, the index, the model, and fail-open
  status.
- **Survey report (J1).** A readable "what your agents actually do" report, generated from `graph`.
- **Signal box without the demo.** `adage graph --serve` serves the live page from the binary, so
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
| L1 | **Choose the license** (decision D1 below) and add `LICENSE` and `NOTICE`. The workspace currently says `license = "UNLICENSED"` | You decide; I implement |
| L2 | **Third-party notices.** All Rust dependencies are permissive: 60× MIT OR Apache-2.0, 27× Apache-2.0 OR MIT, 10× MIT, BSD-2/3, 0BSD, ISC and similar. There is no copyleft; the one LGPL option is an OR choice. `cargo about` generates the NOTICE, and `cargo deny` enforces an allowlist in CI | Me |
| L3 | **Model.** potion-base-8M is MIT. It is fetched, not bundled; attribute it in NOTICE. bge-base is also MIT, if we switch | Me |
| L4 | **Contributions:** a DCO (sign-off) or a CLA (decision D2) | You decide |
| L5 | **Trademark.** "Adage" is a common word, and the mark may be narrow (Design §17.5). Run a knockout search (USPTO, EUIPO, classes 9 and 42) before spending on brand. Secure the GitHub org, npm scope, crates names and a domain. "On rails" stays a lowercase phrase; harness names are used descriptively only | You, with a search service if needed |
| L6 | **Project hygiene:** `SECURITY.md` (private reporting; tracks are an attack surface), `CONTRIBUTING.md`, code of conduct, and a privacy statement | Me |
| L7 | **Track licensing.** Shared tracks are data that others copy. Add an SPDX `license` field to the track manifest; `adage share` asks for it and `adage add` shows it | Me (small format change) |

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
| **M3 — Channels + first run** | npm, Homebrew, crates.io, Claude Code marketplace, OpenCode npm plugin; `init`, `doctor`, the survey report (first run ✅ built) | A new user goes from `npm i -g` to their first directive in under 5 minutes |
| **M4 — Public beta** | Docs, `SECURITY.md`, live adapter validation on OpenCode and Codex | — |
| **M5 — 1.0** | Team depots + lockfiles (O12), proving ground as a product | — |

## 9. Decisions needed from you

| # | Decision | Recommendation |
| --- | --- | --- |
| **O9** | Employer IP clearance | Get it in writing before M2 or any public release |
| **D1** | License | Apache-2.0 for the engine. Decide open core before building depots |
| **D2** | DCO or CLA | DCO, unless you want to keep the option to relicense (then a CLA) |
| **D3** | npm name | Claim the `@adage` scope if it is free, and publish `@adage/cli`; otherwise `adage-cli`. The binary stays `adage` either way |
| **D4** | Apple Developer ID ($99/yr) | Yes, before the Homebrew and direct-download channels |
| **D5** | Private or public beta first | A private beta with a few teams after the pilot, then public |
