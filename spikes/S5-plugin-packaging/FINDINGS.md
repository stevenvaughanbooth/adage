# S5 · Plugin packaging and pack discovery — FINDINGS

## Status

Run 1 (`claude -p`, isolated + real config) failed on a pre-existing auth problem in this sandbox
(`Failed to authenticate: OAuth session expired`). After the coordinator confirmed the CLI was
logged in (`claude auth status` -> `loggedIn: true, claude.ai`), Run 2 succeeded against the
**real** `~/.claude` config with `--plugin-dir` only (the isolated `CLAUDE_CONFIG_DIR` still can't
see that login, as expected — nothing was installed there or in `~/.claude`). One rich `-p` prompt
(`scripts/probe-prompt.txt`) exercised every hook event in one pass; all answers below are now
**measured** from `logs/*.log` (raw JSON-lines, `results/version-bump.txt` for Q2).

**Runs used: 2 of 15** (1 failed on auth, 1 succeeded and answered everything). No retries beyond
that; no usage-limit error was hit.

---

## Q1 — SessionStart vs UserPromptSubmit ordering — **measured**

`SessionStart` (3 hooks: main logger + exec-form arg probe + shell-form arg probe, all across two
plugins) fired and **fully completed** — including each hook's own `claude plugin list --json`
subprocess call — before `UserPromptSubmit` fired one second later (`04:16:29` → `04:16:30`).
SessionStart hooks are synchronous and block the first prompt until they finish; there is no race.

## Q2 — CLAUDE_PLUGIN_ROOT / CLAUDE_PLUGIN_DATA stability across a version bump — **measured**

(Unchanged from the first pass — this didn't need `claude -p`.) Installed `adage-probe@probe-mp`
1.0.0 from a throwaway local-directory marketplace into an isolated config, bumped to 2.0.0,
re-ran `claude plugin update`. `installPath` changed from `.../cache/probe-mp/adage-probe/1.0.0`
to `.../2.0.0` — a real copy (not a symlink), old version retained. Also, in Run 2,
`CLAUDE_PLUGIN_DATA` for the `--plugin-dir`-loaded plugin resolved to
`/Users/stevenbooth/.claude/plugins/data/adage-probe-inline/` — stable, not version-qualified, and
notably **written under the real `~/.claude`** even though the plugin itself was loaded from
`--plugin-dir` (only the plugin file tree is session-local; its persistent data directory is not).

**Doc claim that turned out wrong:** plugin-marketplaces docs say a local-directory marketplace's
relative-path source "loads in place" with "no version-based caching" — measured behavior is a
full versioned copy under `~/.claude/plugins/cache/...`, identical to a git source.

## Q3 — bin/ on PATH: hook processes vs the Bash tool — **measured**

**Different for each.** The Bash tool's own `command -v adage-marker` / `command -v pack-marker`
both resolved, and its `$PATH` had both `.../adage-probe/bin` and `.../pack-probe/bin` appended at
the end. But **every hook subprocess's own `$PATH`** (captured directly, all 15+ firings across
both plugins) **never contains either bin/ directory** — hooks do not inherit the PATH
augmentation that the Bash tool gets. `adage-marker`/`pack-marker` were `NOTFOUND` in 100% of hook
invocations.

## Q4 — Cross-plugin discovery and `claude plugin list --json` from inside a hook — **measured**

`pack-probe`'s `SessionStart` hook ran `claude plugin list --json` (real subprocess, ~0.3s) and got
back **`[]`** — it cannot see either itself or `adage-probe`, because both were loaded via
`--plugin-dir`, which is session-only and never written to persisted config. This confirms the
doc-only inference from the first pass: `claude plugin list --json` only enumerates
marketplace-installed plugins (Q2's mechanism), not `--plugin-dir`/inline ones. For production
(Adage and packs installed from a real marketplace) this command remains a viable, fast, no-auth
discovery path; it is simply untested by the dev-mode harness this spike is required to use.

## Q5 — PostToolBatch payload shape/size; parallel PostToolUse concurrency — **measured**

Exact captured shape:
```json
{"session_id":"...","cwd":"...","hook_event_name":"PostToolBatch",
 "tool_calls":[
   {"tool_name":"Bash","tool_input":{"command":"sleep 2 && echo A_DONE","description":"..."},
    "tool_use_id":"toolu_...","tool_response":"A_DONE"},
   {"tool_name":"Bash","tool_input":{"command":"sleep 2 && echo B_DONE","description":"..."},
    "tool_use_id":"toolu_...","tool_response":"B_DONE"}]}
```
**Doc discrepancy:** the hooks reference documents `result`/`success`/`error` fields; the real
payload uses a single `tool_response` per call (string or, for a standalone `PostToolUse`, a
richer object `{stdout, stderr, interrupted, isImage, noOutputExpected}`) with no boolean
`success` field. Both A_DONE/B_DONE `sleep 2` commands were issued as one parallel batch and their
individual `PostToolUse` hooks both stamped the same second, immediately followed by one
`PostToolBatch` — consistent with genuine parallel tool execution, though sub-second hook-runner
concurrency itself wasn't separately isolated.

## Q6 — async: true semantics — **measured**

4 of 5 `PostToolUse` events produced a matching async sibling (5 s sleep each); their `start_ts`
tracked each triggering `PostToolUse` closely (e.g. both parallel-Bash hooks started their 5 s
sleep at the same second as the sync hook), while the main turn kept progressing through
`PreToolUse:Agent` → `SubagentStart` → `Stop` → `SessionEnd` without waiting — total session wall
time (~22 s) is far less than what 4×5 s of serial async sleeps would need if they blocked (20 s+
just for those, plus the rest of the turn). **Confirms async hooks do not block.** One nuance
found: the 5th async hook (fired from the very last `PostToolUse`, ~3 s before `SessionEnd`) never
produced a log line even after waiting — its 5 s sleep didn't fit before the session process
exited, i.e. **an async hook spawned close to session end can be cut off, not guaranteed to run to
completion.**

## Q7 — SubagentStart payload and PreToolUse/Agent description — **measured**

`SubagentStart`: `{session_id, cwd, scratchpad_dir, prompt_id, agent_id, agent_type,
hook_event_name}` — confirmed **no task/prompt description**, exactly as F5 assumed.
`PreToolUse` matched on `Agent` **does** carry it:
```json
{"tool_name":"Agent","tool_input":{"description":"probe task",
 "prompt":"Reply with the single word OK and stop.","subagent_type":"general-purpose",
 "run_in_background":false}}
```
This confirms F1/F5's design is correct as written: join the delegation description from the
`PreToolUse`/`Agent` hook, not from `SubagentStart`.

## Q8 — Exec-form (args) vs shell-form hooks — **measured**

Both fired from the same `SessionStart` event, each given a literal `$PATH` token and
`${CLAUDE_PLUGIN_ROOT}`:
- **exec-form** (`args` array): received `$PATH` **literally, unexpanded**; `${CLAUDE_PLUGIN_ROOT}`
  substituted to the real path.
- **shell-form** (single `command` string): received `$PATH` **fully expanded** by `sh -c`;
  `${CLAUDE_PLUGIN_ROOT}` also substituted.

Exactly as documented: only the three named placeholders are substituted regardless of form;
everything else is shell-expanded only in shell form.

---

## Final recommended packaging recipe

- **No top-level `bin/`** (confirmed both risk vectors: it's on the agent's Bash PATH, and F4's
  path-instability finding independently confirmed in Q2). Ship the Adage binary via an exec-form
  `scripts/` shim into `${CLAUDE_PLUGIN_DATA}` — confirmed stable and *not* version-qualified (Q2),
  and confirmed to live under the real `~/.claude/plugins/data/...` even for `--plugin-dir`-loaded
  plugins, so it truly persists across path churn.
- **Hooks must exec the binary by absolute `${CLAUDE_PLUGIN_DATA}` path on every invocation** —
  never rely on PATH, since hook subprocesses do **not** get the plugin's `bin/` on PATH (Q3,
  measured) even though the agent's Bash tool does.
- **Pack discovery**: `SessionStart` → `adage pack register "${CLAUDE_PLUGIN_ROOT}"` (F5) plus a
  `claude plugin list --json` cross-check works and is fast (~0.3 s) **only for
  marketplace-installed** packs (Q4, measured) — it returns `[]` for `--plugin-dir` dev mode, so
  Adage's registration RPC, not `plugin list`, must be the source of truth in general.
- **Sync vs async** (confirmed, Q6): observation-only hooks as `async: true` — genuinely
  non-blocking — but do not rely on one to finish if it's fired near `Stop`/`SessionEnd`; flush or
  make decision-relevant hooks (`UserPromptSubmit`, `PostToolBatch`, `SubagentStart`, `Stop`)
  synchronous per F1.
- **First-prompt timing** (Q1, measured): `SessionStart` fully blocks the first `UserPromptSubmit`,
  so pre-warm work placed there is safe to assume complete before the first decision point.
- **Delegation description** (Q7, measured): capture it from `PreToolUse` matched on `Agent`
  (`tool_input.description`/`.prompt`), never from `SubagentStart`.

## Documentation claims that turned out wrong

1. Local-directory marketplace relative-path sources "load in place, no version caching" — false;
   they're copied into a versioned cache dir like any other source (Q2).
2. `PostToolBatch`/`PostToolUse` payloads carry `result`/`success`/`error` fields — actual field is
   `tool_response` (string or an object), with no boolean `success` (Q5).

## Runs used

- `claude -p` (Haiku, `--max-turns 8`): **2 of 15 budgeted runs** — 1 failed on a pre-existing auth
  error (not a usage-limit error), 1 succeeded and produced every measurement above.
- Q2's marketplace/version-bump test used only `claude plugin ...` subcommands (no model call, not
  counted against the budget).

## What could not be done

Nothing outstanding from the original question list. Sub-second-scale concurrency of the two
*parallel* `PostToolUse` hook invocations within a batch (Q5) was inferred from same-second
timestamps rather than isolated with finer-grained instrumentation — a genuinely tighter test would
need higher-resolution timers than this probe's `date -u +%3N`.
