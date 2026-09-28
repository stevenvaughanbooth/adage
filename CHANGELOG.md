# Changelog

All notable changes to Adage are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/). The release workflow uses the section for a tag's
version as the GitHub Release notes.

Nothing has been released yet, so this file describes the first version as it stands; the path
there is in the git history and in `docs/` (the Implementation Plan, and the track documents
under `docs/tracks/`).

## 0.1.0 - Unreleased

**Agent traces become reflexes.** Adage reads the traces coding agents leave behind, finds the
paths that get work done, and compiles them into reflexes: small, typed, deterministic responses
with their evidence attached. At each decision point the matching reflex tells the agent what to
do next. Adage never runs the commands it directs; execution is a separate, opt-in binary.

### Conditioning (learning from traces)

- `adage condition` imports Claude Code transcripts (sub-agents' traces included), Codex rollouts
  (`--codex-sessions`) and OpenCode exports (`--transcripts`), normalises every tool call into
  canonical actions (tree-sitter-bash plus a command table kept as data, with golden fixtures),
  and hands the traces to **the graph engine**, the one learner (docs/tracks/track-g-graph-engine.md).
- The engine has no list of kinds: it mines what reliably precedes a step and compiles it to a
  delivery shape: a lookup of the prompt's item (its value type learned from the values), a read
  of the files a result lists, a dead end to avoid (announced at session start), a finish line
  after edits (an obligation, with one Stop request if unmet), steps in order, a check scoped to
  the part edited, an environment step after a dependency file changes, and context triggers
  (a short run of events). Housekeeping commands, one-off inline code and edits outside the
  repository never become reflexes.
- Every reflex carries its evidence (distinct traces, Wilson lower bound, why it formed), an
  effect class that caps how strongly it may be directed, and typed slot contracts. A step few
  agents took but the successful ones did forms at prime through outcome lift. Scripts the command
  table does not know are classified observe from what the traces show them doing, with the
  evidence recorded.
- Steering is by influence: what a delivered directive named is no evidence from the point it
  arrived. Only hook output counts as a directive.
- Strengths: **fire** (do this), **prime** (consider this), **inhibit** (a known dead end).
  Lifecycle: **nascent** (waits for review), **forming**, **firm**, **dormant**, **extinguished**.
- **Live conditioning:** the hook starts a debounced, single-flight background learner per
  repository when a session stops or ends; `learn.log` records every change; a parse cache makes
  re-conditioning read only what changed (docs/live-learning.md).
- `adage sense`: a read-only report of what agents do and which reflexes their traces would form.
- **Intent reflexes** (optional): `adage models fetch` adds the pinned potion-base-8M static
  embedding model; prompts that mean the same thing and lead to the same first step form one.
- **Instincts:** reflexes that ship with Adage, bound on day 0 from a repository's own Makefile,
  `package.json`, CI, lockfiles and generators (not its test fixtures), prime at most, and giving
  way to any conditioned reflex (docs/instincts.md). `adage condition --instincts-only` binds them
  before any trace exists.
- **Training:** `adage train` runs your agent headlessly on a skill, runbook or Makefile in a
  sandboxed clone (the separate `adage-train` binary), compiles the sessions into reflexes, and
  reports which of the skill's claims are stale; `adage train plan` estimates the cost and runs
  nothing (docs/training.md).

### Runtime

- Hook clients for Claude Code (plugin with exec-form hooks), OpenCode (bridge plugin) and Codex
  (`hooks.json`); `adage install <harness>` prints, or with `--write` merges, the registration.
  Hooks always fail open, and a no-match fast path skips events that cannot change a decision.
- Directives are rendered from fixed templates inside `<adage-reflex … strength="…">`; slot values
  reach them only through validated contracts and safe, existing repository-relative paths.
- Interlocking: inhibit beats fire, at most one fire per decision point, instincts last, and one
  edit yields one directive (a sequence that begins with a finish line's command carries its
  obligation).
- Daemonless per-session storage: two-slot state files and CRC-framed spools, rebuilt from the
  spool if a state file is damaged.
- A control arm (`runtime.control`, `ADAGE_CONTROL`, default 10%) computes what Adage would have
  said and withholds it, so every effect is measured against a control. Kill switches:
  `ADAGE_DISABLE=1` and `.adage/disabled`.

### Review, packs and sharing

- `adage review` lists reflexes that need a person's decision with their evidence and the exact
  directive text; `approve`, `strengthen`, `weaken`, `extinguish`, `pin` and `dismiss` are
  recorded in `review.yaml` and survive re-conditioning. A pinned reflex is never overwritten:
  changes wait as proposals.
- **Packs** (format `adage-pack/1`) are the only format that travels: closed over everything they
  reference, content-addressed and signed (sshsig, namespace `adage-pack`). `adage share` converts
  a track, redaction-checks every byte and signs; `adage add` verifies against trusted signers
  (trust on first use, pinned by hash), validates in full, binds the pack to the repository and
  installs its reflexes at forming (docs/packs.md).

### Execution (opt-in)

- `adage-exec`, a separate binary, runs only reflexes a person approved on that machine
  (`adage review delegate --level e1|e2`), inside an OS sandbox (Landlock and seccomp on Linux,
  bubblewrap, the macOS sandbox): **E1** prefetches an observe step's typed output beside the
  directive; **E2** (beta, behind policy) carries out a finish line or check in a staged overlay
  and applies only the declared outputs. `adage delegate on|off|status` swaps the hook
  registrations. Policy files, approvals keyed to what runs, automatic revocation; nothing is
  approved automatically unless policy allows the inert-observe class (docs/execution.md).

### Seeing it

- `adage watch` (a live pane), `adage ui` (the full-screen app: live feed, reflexes and review,
  health fixes, settings), the interactive `adage init` wizard, `adage map` (the map in a
  browser, read-only, loopback only), `adage doctor` (every check, with fixes), `adage status`,
  `adage report` and `adage graph`. `adage --help` groups every command; `adage <command> --help`
  shows its options and never runs it.

### Evals

- `adage eval`: offline replay of recorded traces through the real runtime, scored against what
  agents did; `adage eval routes` scores learned reflexes against a sandbox's conventions
  (too-narrow reflexes are reported apart from false ones); `adage eval fidelity`; the L0 bench
  with a committed baseline, sweeps and a scorecard; property tests and a safety oracle; the live
  A/B runner and generated sandboxes (docs/evals.md).

### Platforms and distribution

- macOS (arm64, x64), Linux (x64, arm64; static musl) and Windows (x64; execution is unavailable
  there). CI runs rustfmt, clippy, the whole test suite, cargo-deny, a NOTICE check and plugin
  validation on every platform, and the slow eval gates weekly.
- Releases built by dist for every target: `adage`, `adage-exec` and `adage-train`, each with
  shell, PowerShell and Homebrew installers, Sigstore build-provenance attestations and a
  CycloneDX SBOM. The npm package's launcher runs the binaries from a per-platform package chosen
  by `os`/`cpu` (all three binaries in it); nothing is downloaded at install time.
- A Claude Code plugin marketplace with two plugins (`adage`, and `adage-exec` for execution);
  each uses a bundled, checksum-verified binary, else the one on `PATH`, and never downloads.
- Apache-2.0, with a generated NOTICE of third-party licenses; `SECURITY.md`, `CONTRIBUTING.md`.
