# S5 · Plugin packaging and pack discovery — FINDINGS

## Blocker (read first)

The `claude` CLI in this sandbox (`~/.local/bin/claude`, 2.1.221) has **no credentials at all**
(`claude auth status` -> `loggedIn: false, authMethod: none`; `claude -p ...` -> `Failed to
authenticate: OAuth session expired and could not be refreshed`). This is true both for the
isolated `CLAUDE_CONFIG_DIR` built for this spike and for the real user config (fallback per the
brief). One `claude -p` attempt was made (see `results/run1-auth-failure.txt`), confirmed via
`claude doctor` / `claude auth status` that no token exists; per instructions not to dig for
workarounds, the keychain was not probed and the run was not retried in a loop. **1 of the 15
budgeted `claude -p` runs was used; 0 succeeded.**

Consequence: every question that requires watching a *live* hook fire during an agentic turn
(ordering, PATH inside a hook process, batch concurrency, async blocking, subagent payload, exec-
vs shell-form substitution) could **not** be measured directly. Two things rescued partial, real
evidence anyway:

- `claude plugin <subcommand>` (install/update/list/details/validate) needs **no model auth** —
  used extensively against the isolated config dir (`cfg/`, never touching real `~/.claude`) to
  answer Q2 and part of Q4 empirically.
- The exact command a hook would run (`claude plugin list --json`) is itself just a subprocess
  call, so it was run directly to characterize its output and latency, which is valid regardless of
  who spawns the subprocess.

Everything else below is answered from `docs/en/plugins-reference` and `docs/en/hooks`
(fetched 2026-09-22) and is marked **[doc-only, unverified live]**.

The probe plugins (`plugins/adage-probe`, `plugins/pack-probe`) are complete, pass
`claude plugin validate`, and `claude plugin details adage-probe` confirms all 8 requested hooks
registered correctly — ready to run the moment a working `claude -p` credential exists. See "How
to finish this spike" at the end.

---

## Q1 — SessionStart vs UserPromptSubmit ordering

**[doc-only, unverified live].** The hooks reference states the lifecycle as:

```
[Optional Setup] -> SessionStart -> [MCP unavailable at launch]
     |
[First user enters prompt] -> UserPromptSubmit -> UserPromptExpansion -> agentic loop -> Stop
```

SessionStart is documented to fire "before any user input," and MCP-tool hooks specifically are
unavailable "at initial launch" — implying `command`/`http`/`prompt`/`agent`-type SessionStart hooks
*do* run before the first `UserPromptSubmit`. Nothing in the docs states whether Claude Code
*waits* for a synchronous SessionStart hook to exit before accepting the first prompt, versus
firing it and racing ahead. Not resolved without a live run.

## Q2 — CLAUDE_PLUGIN_ROOT / CLAUDE_PLUGIN_DATA stability across a version bump

**Measured** (`scripts/version-bump-test.sh`, output in `results/version-bump.txt`).

Built a throwaway local-directory marketplace (`marketplace/probe-mp/marketplace.json`, source
`"./adage-probe-src"`) inside this spike folder, installed `adage-probe@probe-mp` at version 1.0.0
into an isolated `CLAUDE_CONFIG_DIR`, bumped the version in the source to 2.0.0, ran
`claude plugin marketplace update` + `claude plugin update`, and diffed `claude plugin list --json`:

| | before | after |
|---|---|---|
| version | 1.0.0 | 2.0.0 |
| installPath (approx. CLAUDE_PLUGIN_ROOT) | `.../cache/probe-mp/adage-probe/1.0.0` | `.../cache/probe-mp/adage-probe/2.0.0` |

Confirmed the 1.0.0 directory is a real copy (not a symlink — `file` reports "directory") and a
file added to the *source* after install did not appear in the cached copy — it is a genuine
independent snapshot. Both version directories are kept (grace-period retention matches docs).

**CLAUDE_PLUGIN_DATA** was not independently observed (the probe plugin never wrote to it), but per
docs it resolves to `~/.claude/plugins/data/{id}/`, where `{id}` is derived from plugin+marketplace
name only, never the version — so it should be the one stable, cross-version location, exactly as
S4.10/F4 assume. This part is doc-sourced, not measured.

**Documentation claim that turned out wrong:** the plugin-marketplaces docs state a relative-path
source in a *local-directory* marketplace "loads in place" and gets "no version-based caching."
Measured behavior contradicts this for the ordinary `claude plugin install` flow: it is copied into
a versioned cache directory (`~/.claude/plugins/cache/<marketplace>/<plugin>/<version>/`) exactly
like a git/URL/archive source, and a version bump produces a brand-new directory. (The "loads in
place" behavior the docs describe may apply only to referencing the marketplace entry directly
without ever running `install` — not exercised here.) This reinforces, rather than weakens,
F4/S15.1's "CLAUDE_PLUGIN_ROOT changes on every plugin update" — it is not a workaround you get by
using a local/private marketplace instead of git.

## Q3 — Is a plugin's bin/ on the PATH of hook processes and of the agent's Bash tool?

**[doc-only, unverified live].** Docs confirm bin/ is added to the **Bash tool's** PATH while the
plugin is enabled (the F4 fact already accepted in S15.1). The docs are **silent** on whether hook
*subprocesses* also inherit that PATH-modified environment. `plugins/adage-probe` ships
`bin/adage-marker` and `plugins/pack-probe` ships `bin/pack-marker` specifically to answer this
with `command -v adage-marker` inside a hook and inside a Bash-tool call — both scripts are in
place and untested. Open until a live run is possible.

## Q4 — Cross-plugin discovery and claude plugin list --json from inside a hook

**Partially measured.** `claude plugin list --json`:
- needs no model auth (confirmed — it is a config-file read), runs in **~0.35s** as a bare
  subprocess (`results/version-bump.txt`, timing run) — relevant to hook latency budgets (F2);
- lists **every marketplace-installed plugin regardless of enabled state**, with an explicit
  `enabled: true|false` field (confirmed by disabling `adage-probe@probe-mp` and re-listing) and an
  `installPath` — so a hook can filter to enabled plugins and get real filesystem paths;
- reflects only **persisted config** (`enabledPlugins` in settings / `installed_plugins.json`), not
  live per-process state. It therefore **cannot see plugins loaded only via --plugin-dir** (the
  "@inline" source used for this whole spike per the brief's rules) — those are session-scoped and
  never written to disk. It also cannot know about a different, concurrently running session's
  plugin set.

Net for F5: for the real deployment shape (Adage and packs installed from an actual marketplace,
not dev-mode --plugin-dir), a pack's SessionStart hook shelling out to `claude plugin list --json`
**is** a viable, fast, documented way to enumerate sibling plugins and their paths — this validates
F5's premise. It does **not** help in the --plugin-dir dev/test mode this spike was constrained to
use, so the actual "can pack-probe find adage-probe" cross-check could not be exercised end-to-end
live.

## Q5 — PostToolBatch payload shape/size; parallel PostToolUse concurrency

**[doc-only, unverified live].** Documented shape:
```json
{"hook_event_name":"PostToolBatch","tool_calls":[
  {"tool_name":"Bash","tool_input":{},"tool_use_id":"...","success":true,"output":"..."}
]}
```
Docs state "all matching hooks run in parallel within a single event," which speaks to multiple
*hook registrations* for one event, not to whether the underlying tool calls in a batch overlap in
wall-clock time. `plugins/adage-probe`'s PostToolUse hook (`log-event.sh`) and its async sibling
(`log-event-slow.sh`, 5s sleep) are wired up to make this directly observable via timestamps once a
live run is possible — not yet run.

## Q6 — async: true semantics

**[doc-only, unverified live].** Per docs/en/hooks: does not block the turn, output/exit code are
ignored, and the `timeout` field is *not* enforced on async hooks. A distinct `asyncRewake: true`
variant runs in the background but wakes Claude on exit code 2, surfacing stderr as a system
reminder. The probe's async PostToolUse hook sleeps 5s specifically to make "did the turn wait 5s"
observable by wall-clock — ready, not yet run.

## Q7 — SubagentStart payload and PreToolUse/Agent description

**[doc-only, unverified live].** Documented SubagentStart payload is
`{hook_event_name, agent_type, agent_id, cwd}` — **no task/prompt description**, confirming the
S15.1 fact that drove F5's requirement to join the delegation description from the Agent tool call
instead. PreToolUse for `tool_name: "Agent"` should carry the description inside `tool_input` (per
the general PreToolUse shape `{tool_name, tool_input, tool_use_id}`), but this was not observed
directly.

## Q8 — Exec-form (args) vs shell-form hooks

**[doc-only, unverified live].** Both are documented and supported: `args` present => exec form
(`command` is a literal executable, each `args` element passed verbatim, no shell, safer for
plugin-supplied paths); `args` absent => shell form (`sh -c` on Unix), which expands `$VAR` and
supports pipes/redirects. Docs are explicit that `${CLAUDE_PLUGIN_ROOT}` / `${CLAUDE_PLUGIN_DATA}` /
`${CLAUDE_PROJECT_DIR}` are substituted as plain strings **even in exec form**, but ordinary env
vars are not — `hooks/hooks.json` in adage-probe includes matched exec-form and shell-form hooks
(`scripts/log-args.sh`) both given a literal `$PATH` token, specifically to catch this difference
live. Not yet run.

---

## Recommended packaging recipe (revised in light of the above)

- **Binary placement:** confirmed correct not to use a top-level bin/ (F4) — that directory is
  proven to attach to the agent's Bash PATH, and now also proven to move on every version bump even
  for a "private"/local marketplace, so nothing may hardcode into it. Ship the Adage binary through
  an exec-form scripts/ shim (per F4) that installs a digest-named, signature-verified binary into
  `${CLAUDE_PLUGIN_DATA}` (the one location confirmed by docs — id derived from plugin+marketplace
  name, not version — to survive updates) and execs it from there.
- **Hooks reach the binary** via `${CLAUDE_PLUGIN_DATA}/<digest>/adage`, resolved fresh on every
  invocation (never cached as a path in long-lived state, since CLAUDE_PLUGIN_ROOT itself is proven
  unstable across updates).
- **Pack discovery:** register via SessionStart calling
  `adage pack register "${CLAUDE_PLUGIN_ROOT}"` as F5 proposes, **and** have adaged/the CLI
  independently confirm and enumerate enabled packs by shelling out to `claude plugin list --json`
  (confirmed fast, ~0.35s, no auth needed, gives real paths and enabled state) rather than scanning
  undocumented cache paths — viable for the real (marketplace-installed) deployment shape, not for
  --plugin-dir dev mode.
- **Sync vs async:** per docs (not independently confirmed here), observation-only hooks should be
  `async: true` (or `asyncRewake` where a failure must surface); decision hooks (UserPromptSubmit,
  PostToolBatch, SubagentStart, Stop) must stay synchronous, matching F1's split — this spike could
  not add live timing evidence for that split.
- **First-prompt timing guarantee:** unresolved (Q1) — do not assume UserPromptSubmit waits for
  SessionStart to finish; design pre-warm assuming a race until proven otherwise.

## Documentation claim that turned out wrong

plugin-marketplaces docs' claim that a local-directory marketplace's relative-path source "loads in
place" with "no version-based caching" is false for the ordinary `claude plugin install` flow —
measured behavior is a full versioned copy under `~/.claude/plugins/cache/...`, same as a git
source. See Q2.

## Runs used

- `claude -p` (Haiku, --max-turns 8): **1 attempted, 0 succeeded** — auth failure, not a
  usage-limit error. Did not retry (per instructions). 14 of 15 budgeted runs unused.
- All other measurements used plain `claude plugin ...` subcommands (unlimited, no model call).

## What could not be done

Everything gated on watching a real agentic turn: Q1 ordering/blocking, Q3 hook-process PATH, Q4's
--plugin-dir-mode cross-plugin lookup, Q5 batch shape/concurrency, Q6 async blocking, Q7 payload
contents, Q8 substitution differences. The plugins, hooks, scripts, and log format needed to answer
all of them in one more short session are complete and validated (`claude plugin validate`,
`claude plugin details`) in `plugins/adage-probe` and `plugins/pack-probe`.

## How to finish this spike

With working `claude -p` credentials, from this folder:

```bash
claude -p "$(cat scripts/probe-prompt.txt)" \
  --plugin-dir ./plugins/adage-probe --plugin-dir ./plugins/pack-probe \
  --model claude-haiku-4-5-20251001 --max-turns 8 --permission-mode bypassPermissions
```

then read `logs/adage-probe.log`, `logs/adage-probe-slow.log`, `logs/adage-probe-args.log`, and
`logs/pack-probe.log` (JSON-lines, one record per hook firing) to fill in Q1, Q3, Q5, Q6, Q7, Q8.
