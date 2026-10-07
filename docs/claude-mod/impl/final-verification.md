# Final verification

The whole tree checked as CI would check it, at `c06e27d` on `claude/mods-poc-m98u3o` plus the
three small fixes below, on 2026-10-07 (Linux x64, Node 22, the pinned Claude Code CLI 2.1.288).
Dev profile throughout; a release build of `plugin/build.sh` ran in the same tree meanwhile.

## Fixes made during the run

| File | Change | Why |
| --- | --- | --- |
| `plugin/claude-code/hooks/step.test.ts:858` | the `stdins` helper's parameter type `argv: string[]` became `argv: readonly string[]` | strict `tsc` refused four calls (TS2345): the fixture kit's `Ran` declares `argv: readonly string[]`, and the helper only reads it. Runtime behaviour unchanged (430 plugin tests pass before and after) |
| `docs/claude-mod/impl/README.md` (intro, and "What is not on this branch") | "nothing on this branch has run in a live Claude Code session" replaced by: the mod has run live three times, reported in `../evaluation-results.md`; the headless comparison removed from the not-run list | stale: runs 1-3 of the headless comparison are in `docs/claude-mod/evaluation-results.md` |
| `docs/Adage — Architecture.md` (release row) | "shell/PowerShell/Homebrew installers" became "shell and PowerShell installers (Homebrew is deferred until macOS signing exists)" | stale: `dist-workspace.toml` sets `installers = ["shell", "powershell"]`, and `release-check` asserts no Homebrew job |

## Commands and results

| # | Command | Result |
| --- | --- | --- |
| 1 | `cargo fmt --all -- --check` | clean |
| 1 | `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| 2 | `cargo build -p adage-exec -p adage-cli` | ok |
| 2 | `cargo test --workspace --exclude adage-eval --no-fail-fast` | 137 test binaries (19 doc-test runs among them): 1524 passed, 0 failed, 5 ignored |
| 3 | `cargo test -p adage-eval --no-fail-fast --lib --test gates --test predict_gate --test parity --test paired --test experiment --test experiment_sources` | 75 passed, 0 failed: lib 28, experiment 15, experiment_sources 5, gates 15, paired 8, parity 3 (276 s), predict_gate 1. g4's known `contexts_fire_in_replay…` is not in these targets |
| 4 | `cargo test -p xtask` | 24 passed |
| 4 | `cargo xtask test python demo` | 48 run, OK, 6 skipped (the replay through a real binary needs `ADAGE_BIN`); with `ADAGE_BIN=target/debug/adage`: 48 run, OK, 0 skipped |
| 4 | `cargo xtask test python live` | 155 run, OK |
| 4 | `cargo xtask release-check` | ok: versions agree (0.1.0), no Homebrew job; `dist` not installed, so `release.yml` was not regenerated (CI runs `--require-dist`) |
| 5 | `claude plugin validate --strict plugin/claude-code` | passed |
| 5 | `claude plugin validate --strict .` | passed |
| 5 | validate output with `$PWD/` stripped, `diff -u plugin/claude-code/VALIDATE.txt` (as `ci.yml`) | no difference (before and after the `step.test.ts` fix) |
| 5 | `claude plugin test plugin/claude-code` | 430 pass, 0 fail, 45 files |
| 5 | `node scripts/check-plugin.mjs` | `plugin adage 0.1.0: ok` |
| 5 | `node scripts/test-check-plugin.mjs` | ok |
| 5 | `node scripts/test-stage-plugin.mjs` | ok |
| 5 | `bash plugin/test/session-start.test.sh` | `session-start ok` |
| 5 | `node npm/test/launcher.test.mjs` | ok (companions staged: adage-exec, adage-train) |
| 5 | `tsc` strict (`noUncheckedIndexedAccess`) over `plugin/claude-code/hooks` and `types` with 2.1.288's `claude-code.d.ts` (a scratch tsconfig; the repository has no script for it) | 4 errors in `step.test.ts` before the fix, 0 after |
| 6 | `web/map`: `pnpm typecheck` | clean |
| 6 | `web/map`: `pnpm test` (vitest) | 16 files, 71 passed |
| 6 | `web/map`: `pnpm drift` | the embedded map matches `web/map` |
| 6 | `web/map`: `pnpm test:e2e` | not run: no Playwright browsers on this machine (`~/.cache/ms-playwright` absent) |
| 7 | `git diff --stat origin/main -- crates/adage-cli/tests/golden` | 2 files, +43 −1 (below) |
| 8 | the docs sweep (`claude-code-exec`, `both binaries`, `brew install`, `not published yet`, `nothing on this branch has run`, `never run live`) | two stale statements fixed (above); no hit in `README.md`, `docs/usage.md`, `plugin/claude-code/README.md`, `npm/` or `evals/launch/` |

## Goldens against `origin/main`

`origin/main` is `7aaeedd` (#27, this branch's earlier state). Against it:

- `claude-code-newer-index.txt`: +42, 0 removed (a new golden).
- `claude-code.txt`: +1 −1, one line changed: the `### PreToolUse` step's `plan.substitute.context`
  now reads "adage ran make test for you; python3 -m pytest tests was replaced because that command
  fails in this repository (…). The output above is from make test." (`df44e67`). That step is not
  part of the floor: it was itself added by this branch (merge-base `63cbc77` → `origin/main` adds
  its 15 lines), and `adage hook claude` output for every event `main` had before the branch is
  byte-identical. Against the merge-base, the goldens show only added lines (+42 and +15).

## Docs sweep: hits left as they are

Every remaining hit is in a design or plan document describing the design at the time it was
written, with the later decision recorded in the same document or the plan: `architecture.md` §2
(two plugins) and §3.12 (the `claude-code-exec` alias, later deleted by O1),
`implementation-plan.md` (A8 and its "Built" line), `onboarding-plan.md` (O1, O12, the Homebrew
row), `test-plan.md`, `tracks/track-c-compiled-skills.md` (D2, with its 2026-10 revision), `Adage —
Implementation Plan.md` (the original distribution goal), and `release.md:112` (which states the
hygiene rule itself).
