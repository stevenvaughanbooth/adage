# The implementation

What was built of `../implementation-plan.md` on the branch `claude/mods-poc-m98u3o` (the name
is historical: the branch began as the proof of concept and now carries the whole plan), against
Claude Code 2.1.288. Each chunk's notes hold the exact contracts, the departures from the plan
and why, what could only be checked live, and the commands run. `../evaluation-plan.md` is the
brief for the live evaluation; nothing on this branch has run in a live Claude Code session,
because the build machine had no key.

Line references in the implementation plan and the notes are at commit fff4fa7 (the plan) and the
chunk's own commit (the notes); the module splits since then are mapped in structure-*.md and
mod-structure.md.

## Chunks

| Chunk | Commit | Notes |
| --- | --- | --- |
| A1 `PreToolUse` identity dedup and the two-phase payload; A2 `duration_us` and the doctor p95; C5 standing text, compaction, routing (binary) | `09f2d1d` | [A1-A2-C5-notes.md](A1-A2-C5-notes.md) |
| A3 `adage watch --follow --json`; A4 `adage ledger note` and the harness ledger; A5 the status card `--json` and `tokens.ts`; A6 `adage off`, `adage on` | `97405bc` | [A3-A6-notes.md](A3-A6-notes.md) |
| A7 the press token and `require_person_or_press`; A8 one plugin, both binaries | `efcb365` | [A7-A8-notes.md](A7-A8-notes.md) |
| B1 the module, the bridge, activation, CI | `4086596` (proof of concept), then A8 | [../poc/P3-notes.md](../poc/P3-notes.md), [A7-A8-notes.md](A7-A8-notes.md) |
| B2 observe the command hooks, the rows and the band; B3 the pane and `/adage …` | `fe70973` | [B2-B3-notes.md](B2-B3-notes.md) |
| B4 asking in the engine's dialog, presses in the pane, `Explore in browser`; D5 drawing noticed, learning and skills | `4fd8e78` | [B4-D5-notes.md](B4-D5-notes.md) |
| C1 `TurnStep` in the binary | `45bc903`, `9a01b14`, hardened in `09f2d1d` | [../poc/P1-notes.md](../poc/P1-notes.md), [A1-A2-C5-notes.md](A1-A2-C5-notes.md) |
| C2 the `predict` grant | `d41d655`, extended in `efcb365` | [../poc/P2-notes.md](../poc/P2-notes.md), [A7-A8-notes.md](A7-A8-notes.md) |
| C3 receipt as the result, rewrites checked by the engine; C7 chains and multi-call steps; C8 the finish line at the last step; C9 attachment (binary side); D2 `noticed` at hook time; E2, E3 offline parts; the simulator | `89d57ff` | [C3-C7-C8-D2-C9-E-notes.md](C3-C7-C8-D2-C9-E-notes.md) |
| C3, C5, C7, C8, C9 (the mod's side) | `af02e95` | [C3-C9-mod-notes.md](C3-C9-mod-notes.md) |
| C4 the mod's `turn.step` and the simulator | `4086596`, `f7b399e` | [../poc/P3-notes.md](../poc/P3-notes.md), [../poc/P4b-notes.md](../poc/P4b-notes.md) |
| C6 the dead end that never runs | `9f6eceb` | [../poc/P5-notes.md](../poc/P5-notes.md) |
| D1 candidates in the learner and the index; D3 the `skill` response kind; D4 `verified.json` and `SkillPrompt` | `ecd4e03` | [D1-notes.md](D1-notes.md), [D3-D4-notes.md](D3-D4-notes.md) |
| E1 arms and the requests metric in the demo runner | `13b2495` | [../poc/P4a-notes.md](../poc/P4a-notes.md) |
| The mod's structure: `register.ts` as registrations only, modules over a capability object, one test file per module | (uncommitted) | [mod-structure.md](mod-structure.md) |

## What is not on this branch

- Anything that needs a live Claude Code session or a model: every "Live-only" section in the
  notes, S9's questions, the paired release-build latency runs, `release_plugin_startup` with two
  binaries, the headless comparison, the development gate (E2's runs) and the registered run (E3).
  `../evaluation-plan.md` collects them in order.
- The usability study (T-U-01).

## Reading order for a reviewer

`../README.md` for the four documents; `../architecture.md` §1b for why; this page for what
exists; each notes file's "Departures" before its code, since the departures are where the plan
met the engine's real rules (the validator's `$` rule, the stop rewritten after `end_turn`, the
checkout taken from the working directory, the follower's stdin).
