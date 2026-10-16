# S5 · Plugin packaging and pack discovery

**Informs:** A8 (pack discovery), F4 (binary placement), F5 (pack registration), O14 ·
**Wave:** 1 · **Budget:** half a day, ≤ 15 short `claude -p` runs on Haiku or Sonnet

## Question

How do Claude Code plugins actually behave for Adage's needs — hook ordering, paths across updates,
data directories, async hooks, batch payloads, and discovering other enabled plugins — so that Adage
and track packs can ship as plugins safely?

## Why it matters

The review found that `${CLAUDE_PLUGIN_ROOT}` changes on every update, that `bin/` lands on the
agent's Bash PATH, that another plugin's hook can't necessarily find `adage`, and that
`SessionStart` ordering relative to the first prompt is unverified. The packaging design depends on
all of these.

## Method

Use only `--plugin-dir` (and, if needed, a throwaway marketplace directory) plus a **separate
`CLAUDE_CONFIG_DIR`** or per-run `--settings`, so the user's real configuration is never modified.
If an isolated config dir can't authenticate, fall back to `--plugin-dir` with the normal config but
install nothing.

Build two tiny plugins under `plugins/`:

- `adage-probe` — hooks on `SessionStart`, `UserPromptSubmit`, `PreToolUse` (matcher `Agent`),
  `PostToolUse`, `PostToolBatch`, `SubagentStart`, `Stop`, `SessionEnd`; each hook appends a
  timestamped record of its event, its environment (`CLAUDE_PLUGIN_ROOT`, `CLAUDE_PLUGIN_DATA`,
  `CLAUDE_PROJECT_DIR`, PATH), and a digest of its stdin to a log file under this spike folder.
  Include one hook with `async: true`.
- `pack-probe` — a stand-in track pack with a `tracks/` directory and a `SessionStart` hook.

Verify and record:

1. Order and timing of `SessionStart` vs `UserPromptSubmit` for the first prompt — does the prompt
   hook wait for `SessionStart` hooks?
2. Values of `CLAUDE_PLUGIN_ROOT` and `CLAUDE_PLUGIN_DATA`; which is stable across a plugin version
   bump (simulate by changing the version and reinstalling from a local marketplace if feasible).
3. Whether a plugin's `bin/` is on the PATH of **hook** processes and of the agent's Bash tool.
4. Whether `pack-probe`'s hook can locate `adage-probe` (and vice versa), and whether
   `claude plugin list --json` (or another documented command) enumerates enabled plugins with their
   paths from inside a hook.
5. `PostToolBatch` payload shape and size with parallel tool calls; whether parallel `PostToolUse`
   hooks run concurrently.
6. `async: true` hook behaviour: does it delay the agent; is its output ignored?
7. `SubagentStart` payload (confirm no task description) and whether `PreToolUse` on `Agent`
   carries the description.
8. Exec-form hooks (`args`) vs shell form, if supported.

## Pass / fail

A written, evidence-backed recipe for: where the Adage binary lives and how hooks reach it; how pack
discovery works; which hooks are sync vs async; and what timing guarantees the first prompt gets.

## Deliverables

`FINDINGS.md` with each question answered from the probe logs, the recommended packaging recipe, and
any doc claim that turned out wrong.
