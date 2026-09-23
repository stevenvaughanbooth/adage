# Changelog

All notable changes to Adage are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/). The release workflow uses the section for a tag's
version as the GitHub Release notes.

## Unreleased

## 0.1.0 - Unreleased

The first version: Adage learns the routes coding agents take and, at each decision point, tells
the agent deterministically what to do next. It never executes anything itself.

### Learning

- `adage learn` imports Claude Code transcripts (and OpenCode exports), normalises every tool call
  into canonical actions (tree-sitter-bash plus a command table kept as data; 315 golden fixtures),
  and mines five route kinds: prompt → lookup, linked files, dead ends, finish lines, and (optional)
  intent families.
- Every route carries its evidence (distinct sessions, Wilson lower bound), an effect class that caps
  how strongly it may be directed, and typed slot contracts learned from real values.
- Intent families (optional): `adage models fetch` / `adage models install <dir>` add the pinned
  potion-base-8M static embedding model (SHA-256 verified); prompts are clustered per repository
  with exact kNN and deterministic Louvain, then split and merged by what the sessions did next.

### Runtime

- Hook client for Claude Code (plugin with exec-form hooks), OpenCode (bridge plugin) and Codex
  (`hooks.json`); `adage install <harness>` prints, or with `--write` merges, the registration.
- Directives rendered from fixed templates; slot values only through validated contracts and safe,
  existing repository-relative paths. Hooks always fail open.
- Daemonless per-session storage: two-slot state files and CRC-framed spools.
- A holdout arm (`ADAGE_HOLDOUT`, default 10%) measures every effect against a control; kill
  switches `ADAGE_DISABLE=1` and `.adage/disabled`.

### Tracks, review and sharing

- The track format with validation (tracks are untrusted input), a canonical digest, and a compiled
  index.
- `adage review` lists routes that need a person's decision; `approve`, `reject`, `demote`,
  `promote` and `pin` are recorded in `review.yaml`.
- `adage share` redaction-checks a track and signs it (native `sshsig`, ssh-agent or key file);
  `adage add` verifies it against trusted signers and installs it at probation.
- `adage status`, `adage report` and `adage graph` (situations → routes → steps → agent responses,
  with live counts).

### Platforms and distribution

- Windows support in the store (positional I/O and `LockFileEx` locking through `std`), the data
  directory (`%APPDATA%\Adage`), home-directory lookup and path handling. Compiles for every release
  target; Windows tests run in CI.
- Release engineering: CI on macOS (arm64, x64), Linux (x64, arm64) and Windows (x64) with rustfmt,
  clippy, tests, cargo-deny, and plugin validation; releases built by dist for macOS, static Linux
  (musl) and Windows, with shell, PowerShell and Homebrew installers, Sigstore build-provenance
  attestations and a CycloneDX SBOM.
- npm packaging: a main package whose launcher runs the binary from a per-platform package chosen
  by `os`/`cpu`; nothing is downloaded at install time. Publishing is off until the package name is
  chosen.
- A Claude Code plugin marketplace (`.claude-plugin/marketplace.json`). The plugin uses its bundled,
  checksum-verified binary, else `adage` on `PATH`, and never downloads at hook time.
- `SECURITY.md`, `CONTRIBUTING.md`, and a cargo-about NOTICE generator.
