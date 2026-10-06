# Harnesses: what each one supports, and what Adage does with it

Research dates: **2026-09-22**, and **2026-09-23** for the on-disk formats and the source
re-check (pinned below). Sources are the harnesses' official documentation and source
repositories; neither OpenCode nor Codex was installed or run. Every claim is marked
**documented** (stated in official docs or the published SDK/plugin types), **source** (read in the
harness's own source code at the pinned tag, so true of that release but not a promised
interface), or **inferred** (our reading, not verified by running the harness). Anything inferred
is the first thing to check against a real installation. What the harness simulator verifies, and
what still needs a live install, is in [Verification](#verification).

Pinned source trees (read 2026-09-23):

- **Codex**: [openai/codex `rust-v0.156.1`](https://github.com/openai/codex/tree/rust-v0.156.1)
  (commit `b412ff32`, 2026-09-23), the latest release; older shapes checked at
  [`rust-v0.40.0`](https://github.com/openai/codex/tree/rust-v0.40.0).
- **OpenCode**: [anomalyco/opencode `v1.18.32`](https://github.com/anomalyco/opencode/tree/v1.18.32)
  (commit `545f51d2`, 2026-09-21), the latest release (`sst/opencode` redirects there).

Design context: Design Spec §13.1 and decisions O8 and O14; Architecture §4.2, §4.11, §6.2, §15.1 F3.

## Summary

| | Claude Code | OpenCode | Codex CLI |
| --- | --- | --- | --- |
| Registration | Plugin `hooks/hooks.json`, or settings `hooks` | JS plugin in `.opencode/plugins/` or `~/.config/opencode/plugins/` | `hooks.json` in `~/.codex/` or `<repo>/.codex/` |
| Transport | Command hook, payload on stdin | In-process plugin; Adage's plugin spawns `adage hook opencode <event>` | Command hook (shell string), payload on stdin |
| Prompt directives | `additionalContext` | Context-only synthetic message (`session.prompt`, `noReply: true`) | `additionalContext` (developer context) |
| After-tool directives | `PostToolBatch` `additionalContext` | Appended to the tool result (`tool.execute.after`) | `PostToolUse` `additionalContext` |
| Dead ends learned from history | Announced at the first prompt | Announced at the first prompt | Announced at the first prompt |
| After-failure reflexes (authored or imported) | `PostToolUseFailure` | Shell `metadata.exit` ≠ 0, or a tool part in state `error` | **Not possible today**: no failure signal |
| Finish line at stop | Stop `decision: block` | `session.idle` → a synthetic follow-up prompt | Stop `decision: block` (becomes a user prompt) |
| Batches | Yes (`PostToolBatch`) | No (per call) | No (per call) |
| Learning from history | `~/.claude/projects/*.jsonl` | `opencode export <id>` JSON files | `~/.codex/sessions` rollouts (`--codex-sessions`; format from source) |
| Install | `adage install claude-code` / plugin | `adage install opencode` | `adage install codex` |

## Claude Code

Unchanged by this slice; recorded for comparison. Hooks, payloads, and `additionalContext` are
documented at <https://code.claude.com/docs/en/hooks>. Adage's adapter is
`crates/adage-hook/src/harness/claude.rs`; its output is pinned byte for byte by
`crates/adage-cli/tests/golden/claude-code.txt`.

### Capabilities, by caller

Behaviour that differs between harnesses reads the harness's `Capabilities`
(`adage_trace::harness`), never a name. Claude Code's command hooks have four; its mod (the plugin's
module, `plugin/claude-code`) adds seven. The adapter reports the mod's set for a payload the mod
sent: one carrying `via: "mod"`, one of the events only the mod sends (`TurnStep`, `Standing`,
`SkillPrompt`), or a `PreToolUse` carrying `dry_run` or `verdict`. OpenCode and Codex have no mod.

| Capability | What it lets Adage do | Claude Code, command hooks | Claude Code, mod | OpenCode | Codex |
| --- | --- | --- | --- | --- | --- |
| `pre_tool_rewrite` | Rewrite a Bash call's input before it runs | yes | yes | no | no |
| `session_contract` | A context block for the model at session start | yes | yes | no | yes |
| `actions` | Run approved actions inside a hook | yes | yes | no | yes |
| `person_notices` | One line the person sees and the model does not | yes | yes | no | no |
| `predict_step` | Make the directed call at a step boundary (`TurnStep`) | no | yes | no | no |
| `answer_call` | Answer a call with a fresh receipt instead of running it | no | yes | no | no |
| `substitute_call` | Run the learned command in a dead end's place | no | yes | no | no |
| `tool_description` | Put the environment note in the Bash tool's description | no | yes | no | no |
| `cached_section` | Put the standing reflexes in a cached system-prompt section | no | yes | no | no |
| `subagent_prompt` | Tell a subagent the conventions before it starts | no | yes | no | no |
| `compaction` | Tell compaction what to keep | no | yes | no | no |

## OpenCode

### Findings

| Topic | Finding | Status | Source |
| --- | --- | --- | --- |
| Plugin locations | `.opencode/plugins/` (project) and `~/.config/opencode/plugins/` (global); npm packages via the `plugin` key in `opencode.json` | documented | <https://opencode.ai/docs/plugins/> |
| Config files | `~/.config/opencode/opencode.json` (global), `opencode.json` in the project root, `.opencode/` directories; merged, later wins | documented | <https://opencode.ai/docs/config/> |
| `XDG_CONFIG_HOME` | Honoured for the global config directory | inferred | — |
| Plugin shape | `export const P = async ({ project, client, $, directory, worktree }) => ({ …hooks })` | documented | <https://opencode.ai/docs/plugins/> |
| Events | `session.created`, `session.idle`, `session.deleted`, `message.part.updated`, `tool.execute.before`/`after`, `file.edited`, … (full list on the page) | documented | <https://opencode.ai/docs/plugins/> |
| Hook signatures | `"chat.message"(input: { sessionID, … }, output: { message, parts })`; `"tool.execute.after"(input: { tool, sessionID, callID, args }, output: { title, output, metadata })`; `event({ event })`; `experimental.chat.system.transform`, `experimental.session.compacting` | source (published `@opencode-ai/plugin` types) | <https://github.com/sst/opencode/blob/dev/packages/plugin/src/index.ts> |
| Injecting context | `client.session.prompt({ path: { id }, body: { noReply: true, parts: [{ type: "text", text }] } })` — "Inject context without triggering AI response (useful for plugins)" | documented | <https://opencode.ai/docs/sdk/> |
| `noReply` does not lock the session | `prompt` returns right after creating the user message when `noReply` is true, before the agent loop | source | <https://github.com/sst/opencode/blob/dev/packages/opencode/src/session/prompt.ts> |
| Plugins cannot inject AI-visible messages from a hook directly | Feature request closed as not planned | documented (issue) | <https://github.com/anomalyco/opencode/issues/17412> |
| No "stop" hook | `session.idle` fires after the loop ends; a `session.stopping` hook is proposed, open; the workaround is prompting the session again at idle | documented (issue) | <https://github.com/anomalyco/opencode/issues/16626> |
| Tool result mutation | `tool.execute.after` receives the result object the model will read; mutating `output.output` changes it | source (hook types) + inferred (that the mutation reaches the model) | plugin types above |
| `tool.execute.after` on thrown errors | Not called when a tool throws | source | `session/prompt.ts` above |
| Shell exit codes | The shell tool returns normally on a non-zero exit, with `metadata.exit` | source | <https://github.com/sst/opencode/blob/dev/packages/opencode/src/tool/shell.ts> |
| Built-in tools | `bash`, `edit`, `write`, `read`, `grep`, `glob`, `lsp`, `apply_patch`, `skill`, `todowrite`, `webfetch`, `websearch`, `question` | documented | <https://opencode.ai/docs/tools/> |
| Tool arguments | `filePath` (read/edit/write), `patchText` (apply_patch, Codex patch format), `command` (bash) | source | `packages/opencode/src/tool/*.ts` |
| Message and part types | `Session`, `UserMessage`, `AssistantMessage` (`modelID`, `path.cwd`), `TextPart` (`synthetic`, `ignored`), `ToolPart` (`tool`, `state.status` = `completed`/`error`, `state.input`, `state.output`/`state.error`) | documented (SDK types) | <https://opencode.ai/docs/sdk/>, <https://github.com/sst/opencode/blob/dev/packages/sdk/js/src/gen/types.gen.ts> |
| Session storage | "`~/.local/share/opencode/` … `project/` - Project-specific data like session and message data"; no format given | documented (location only) | <https://opencode.ai/docs/troubleshooting/> |
| Storage format | SQLite at `$XDG_DATA_HOME/opencode/opencode.db` (`opencode-<channel>.db` off the release channels; `OPENCODE_DB` overrides). Tables `session` (a row per session: `directory`, `parent_id`, …), `message` and `part` (a JSON `data` column holding the SessionV1 message or part minus its ids), plus event-sourced `session_message` / `session_input`. 38 migrations between 2026-01 and 2026-06, one named "reset_v2_session_state" | source | [`core/src/session/sql.ts`](https://github.com/anomalyco/opencode/blob/v1.18.32/packages/core/src/session/sql.ts), [`core/src/database/database.ts`](https://github.com/anomalyco/opencode/blob/v1.18.32/packages/core/src/database/database.ts), [`core/src/global.ts`](https://github.com/anomalyco/opencode/blob/v1.18.32/packages/core/src/global.ts), `core/src/database/migration/` |
| Database access | `opencode db [query] --format json` (or `tsv`) and `opencode db path` | documented (commands; the schema is not) | <https://opencode.ai/docs/cli/> |
| Export | `opencode export [sessionID]` — "Export session data as JSON". Writes `JSON.stringify({ info, messages }, null, 2)`: `info` is `Session.Info`, `messages` are `{ info, parts }` read from the `message` and `part` tables | documented (command) + source (shape) | <https://opencode.ai/docs/cli/>, [`cli/cmd/export.ts`](https://github.com/anomalyco/opencode/blob/v1.18.32/packages/opencode/src/cli/cmd/export.ts), [`schema/src/v1/session.ts`](https://github.com/anomalyco/opencode/blob/v1.18.32/packages/schema/src/v1/session.ts) |
| Sanitised export | `opencode export --sanitize` replaces text, tool output, titles, and the session directory with `[redacted:…]`; such exports cannot be learned from | documented (flag) + source (what it redacts) | same |
| Listing sessions | `opencode session list --format json` → `[{ id, title, updated, created, projectId, directory }]`, root sessions only | documented (command) + source (fields) | <https://opencode.ai/docs/cli/>, [`cli/cmd/session.ts`](https://github.com/anomalyco/opencode/blob/v1.18.32/packages/opencode/src/cli/cmd/session.ts) |
| Shell tool | Tool id stays `bash` for every shell kind, "for compatibility … Rename with opencode 2.0"; returns normally with `metadata.exit` (the exit code, or `null` on abort or timeout) | source | [`tool/shell/id.ts`](https://github.com/anomalyco/opencode/blob/v1.18.32/packages/opencode/src/tool/shell/id.ts), [`tool/shell.ts`](https://github.com/anomalyco/opencode/blob/v1.18.32/packages/opencode/src/tool/shell.ts) |
| `apply_patch` output | `Success. Updated the following files:` then `A`/`M`/`D <path>` lines | source | [`tool/apply_patch.ts`](https://github.com/anomalyco/opencode/blob/v1.18.32/packages/opencode/src/tool/apply_patch.ts) |
| Session ids | `ses_` + 12 hex digits of the bitwise-inverted (time × 4096 + counter) + 14 base62 characters, so sessions created minutes apart share their first 8 characters or more | source | [`schema/src/identifier.ts`](https://github.com/anomalyco/opencode/blob/v1.18.32/packages/schema/src/identifier.ts), `schema/src/session-id.ts` |
| Plugin hook signatures | Re-checked: as in the rows above | source | [`plugin/src/index.ts`](https://github.com/anomalyco/opencode/blob/v1.18.32/packages/plugin/src/index.ts) |

### What Adage does

**Adapter** — `adage hook opencode <event>` (`crates/adage-hook/src/harness/opencode.rs`) reads a
small JSON payload that Adage's own plugin builds; the table of events and fields is in that file.
The response is `{"context": text}` or `{"continue": reason}`.

**Plugin** — `plugin/opencode/adage.js`, about 130 lines, no dependencies. It:

- spawns the adage binary directly (no shell), writes the payload, and waits at most **1 s**; a
  missing binary, spawn error, timeout, oversized or non-JSON reply, or a non-string field means
  nothing happens. Every hook body is wrapped in `try`.
- at `chat.message`, sends only the user's own text parts (never `synthetic` ones) and delivers
  directives as a separate **context-only, synthetic** message (`noReply: true`), never by
  editing what the user wrote (Architecture §15.1: prompt edits persist as user input).
- at `tool.execute.after`, appends directives to the tool result the model reads next.
- on `message.part.updated` with a tool part in state `error`, asks for failure reflexes and holds
  the directive for the next tool result (there is no mutable output on an event).
- at `session.idle`, asks the finish-line question with `stop_hook_active` set if Adage already
  asked this session to continue since the user last spoke, and if Adage asks to continue, sends
  the fixed-template reason as a synthetic prompt (fire and forget). The runtime bounds these
  requests (once per obligation, twice per session).
- never evaluates, executes, or shells out with text it receives: directive text only ever goes
  into `output.output` or a message part.

**Learning** — `adage condition --transcripts <export.json>…` accepts `opencode export` files
(recognised by content: `info` is the first key) alongside Claude Code transcripts
(`crates/adage-learn/src/opencode.rs`). Steering is detected from synthetic user parts and
directives appended to tool output, both of which the export records. Sub-agent sessions
(`parentID`) are skipped. To export a whole history with documented commands only:

```bash
mkdir -p exports
opencode session list --format json | jq -r '.[].id' | while read -r id; do
  opencode export "$id" > "exports/$id.json"
done
adage condition --transcripts exports/*.json
```

**Install** — `adage install opencode [--write] [--scope project|user]` prints (default) or writes
the plugin with the binary's absolute path embedded, to `.opencode/plugins/adage.js` or
`$XDG_CONFIG_HOME/opencode/plugins/adage.js`. It replaces that file only if it is Adage's, and
`--remove` removes it (keeping `adage.js.adage-bak`).

### Not possible, or not done

- **Reading OpenCode's storage directly.** The schema is now known from source (above), and its
  `message`/`part` rows hold exactly what `opencode export` prints, but it is undocumented and
  migrates often, and reading it would add a SQLite dependency. Adage reads `opencode export`
  output instead (the loop above exports a whole history with documented commands).
- **A true stop hook.** OpenCode has none (#16626 open). Adage's finish-line reminder arrives as
  a new synthetic user message after the session went idle; in `opencode run` it may race with
  process exit (per #16626) — inferred.
- **Hidden context.** The context message and the appended tool-result text are visible in the
  transcript and TUI (inferred). They are wrapped in `<adage-reflex>` and say they are from Adage.
- **Batches.** `tool.execute.after` fires per call, so follow/ignore windows count calls, not
  turns, as with Claude Code's `PostToolUse`.
- **Unverified at runtime** (inferred): that `synthetic` is accepted on `session.prompt` parts; that
  a `noReply` message created during `chat.message` lands before the user's message; that the
  `tool.execute.after` mutation reaches the model. Check these on a real installation first.

## Codex CLI

### Findings

| Topic | Finding | Status | Source |
| --- | --- | --- | --- |
| Hook locations | `~/.codex/hooks.json`, `~/.codex/config.toml` (inline `[hooks]`), `<repo>/.codex/hooks.json`, `<repo>/.codex/config.toml`; all sources load, none replaces another | documented | <https://developers.openai.com/codex/hooks> (redirects to <https://learn.chatgpt.com/docs/hooks>) |
| Format | `{ "hooks": { "<Event>": [{ "matcher": "<regex>", "hooks": [{ "type": "command", "command": "<shell string>", "timeout": 600 }] }] } }` | documented | same |
| Command form | A shell string run with the session `cwd`; no exec-form `args` | documented | same |
| Events | `SessionStart`, `SessionEnd`, `SubagentStart`, `PreToolUse`, `PermissionRequest`, `PostToolUse`, `PreCompact`, `PostCompact`, `UserPromptSubmit`, `SubagentStop`, `Stop`, `Interrupt` | documented | same |
| Payload | Common: `session_id`, `transcript_path`, `cwd`, `hook_event_name`, `model`, `turn_id`, `permission_mode`. `UserPromptSubmit`: `prompt`. `PostToolUse`: `tool_name`, `tool_input`, `tool_response`, `tool_use_id`. `Stop`: `stop_hook_active`, `last_assistant_message` | documented | same |
| Tool names | `Bash` (`tool_input.command`); `apply_patch` (patch text in `tool_input.command`; matchers may use `Edit`/`Write`); MCP tools as `mcp__server__tool`; no hooks for hosted tools such as web search | documented | same |
| Output | `hookSpecificOutput.additionalContext` → developer context (SessionStart, UserPromptSubmit, PostToolUse); `systemMessage`; `continue: false` | documented | same |
| Stop `block` | "creates a new continuation prompt that acts as a new user prompt, using your `reason` as that prompt text" | documented | same |
| Trust | New or changed hooks need review in `/hooks` (hash-based trust); project hooks load only in trusted projects | documented | same |
| Timeouts | Default 600 s; `SessionEnd` 1 s default, at most 3 s | documented | same |
| Failure signal | `tool_response` is a plain string; no exit code; `PostToolUseFailure` never fires (0.144.x) | documented (issue, open) | <https://github.com/openai/codex/issues/34289> |
| Failure signal, at rust-v0.156.1 | Still none. For a finished shell command `tool_response` is the (truncated) output alone, with `tool_name` `Bash` and `tool_input` `{ command }`; a command still running when `exec_command` yields reports the model-facing text, whose header has no exit code yet. `PostToolUse` fires only for calls that did not error, and runs *before* the result is recorded (a hook may replace it), so reading `transcript_path` at that moment cannot find the exit code either | source | [`core/src/tools/context.rs`](https://github.com/openai/codex/blob/rust-v0.156.1/codex-rs/core/src/tools/context.rs) (`ExecCommandToolOutput::post_tool_use_response`), [`core/src/tools/registry.rs`](https://github.com/openai/codex/blob/rust-v0.156.1/codex-rs/core/src/tools/registry.rs) |
| Hook payloads, at rust-v0.156.1 | As documented: `PostToolUse` `{ session_id, turn_id, transcript_path, cwd, hook_event_name, model, permission_mode, tool_name, tool_input, tool_response, tool_use_id }` (plus `agent_id`/`agent_type` in sub-agents); `UserPromptSubmit` adds `prompt`; `Stop` adds `stop_hook_active`, `last_assistant_message`; `SessionStart` adds `source` and has no `turn_id`. `apply_patch` hooks carry `{ command: <patch> }` | source | [`hooks/src/schema.rs`](https://github.com/openai/codex/blob/rust-v0.156.1/codex-rs/hooks/src/schema.rs), [`core/src/tools/handlers/apply_patch.rs`](https://github.com/openai/codex/blob/rust-v0.156.1/codex-rs/core/src/tools/handlers/apply_patch.rs), [`core/src/tools/hook_names.rs`](https://github.com/openai/codex/blob/rust-v0.156.1/codex-rs/core/src/tools/hook_names.rs) |
| Where hook context goes | `additionalContext` is recorded in history as a `developer` message whose text is the context, unwrapped | source | [`core/src/hook_runtime.rs`](https://github.com/openai/codex/blob/rust-v0.156.1/codex-rs/core/src/hook_runtime.rs), `core/src/context/hook_additional_context.rs` |
| Context visibility | `additionalContext` is shown as a developer message in the TUI | documented (issue, open) | <https://github.com/openai/codex/issues/16933> |
| Sessions on disk | `$CODEX_HOME/sessions` (default `~/.codex/sessions`), archived in `archived_sessions` | documented (location only) | <https://learn.chatgpt.com/docs/reference/troubleshooting> |
| Rollout format | `rollout-*.jsonl` lines of `{ timestamp, type, payload }` with `session_meta`, `response_item`, `event_msg`, … | not documented (community reverse-engineering) | <https://github.com/openai/codex/discussions/3827> |
| Rollout file | `sessions/YYYY/MM/DD/rollout-YYYY-MM-DDThh-mm-ss-<thread id>.jsonl` (`…-<thread>_<rollout>.jsonl` after a revert); thread ids are UUIDv7 (time-ordered) | source | [`rollout/src/rollout_file_name.rs`](https://github.com/openai/codex/blob/rust-v0.156.1/codex-rs/rollout/src/rollout_file_name.rs) |
| Rollout line | `RolloutLine { timestamp, ordinal?, type, payload }`, `type` one of `session_meta`, `response_item`, `event_msg`, `turn_context`, `compacted`, `token_usage_record`, `world_state`, `retained_context`, `security_risk_score`, `inter_agent_communication(_metadata)`, `realtime_item` | source | [`history/src/lib.rs`](https://github.com/openai/codex/blob/rust-v0.156.1/codex-rs/history/src/lib.rs), [`history/src/rollout_payload.rs`](https://github.com/openai/codex/blob/rust-v0.156.1/codex-rs/history/src/rollout_payload.rs) |
| `session_meta` | `{ session_id, id, timestamp, cwd, originator, cli_version, source, model_provider, base_instructions, history_mode, parent_thread_id?, forked_from_id?, git? }`; `source` is `cli`/`vscode`/`exec`/`mcp`/…, or `{ subagent: … }` for sub-agent threads; older rollouts lack `session_id` | source | [`protocol/src/protocol.rs`](https://github.com/openai/codex/blob/rust-v0.156.1/codex-rs/protocol/src/protocol.rs) (`SessionMeta`, `SessionMetaLine`, `SessionSource`) |
| `turn_context` | Per user turn: `{ turn_id, cwd, approval_policy, sandbox_policy, model, … }` | source | same (`TurnContextItem`) |
| `response_item` | `message` (`role` user/assistant/developer; `content` of `input_text`/`output_text`), `reasoning`, `function_call` (`name`, `arguments` as a JSON *string*, `call_id`), `function_call_output` (`call_id`, `output`: a string or content items), `custom_tool_call` (`name`, `input` string) and its output, `local_shell_call` (`action.command` argv), `web_search_call`, … | source | [`protocol/src/models.rs`](https://github.com/openai/codex/blob/rust-v0.156.1/codex-rs/protocol/src/models.rs) (`ResponseItem`, `FunctionCallOutputPayload`) |
| What is persisted | All of the above `response_item`s; of `event_msg`, `user_message`, `agent_message`, `task_started`, `task_complete`, `token_count`, … in the default *legacy* history mode. `exec_command_end` (which has `exit_code`) is **not** persisted. A *paginated* mode (feature `background_paginated_rollout_migration`, off by default) stores `item_completed` turn items instead of the legacy events | source | [`rollout/src/policy.rs`](https://github.com/openai/codex/blob/rust-v0.156.1/codex-rs/rollout/src/policy.rs), [`features/src/lib.rs`](https://github.com/openai/codex/blob/rust-v0.156.1/codex-rs/features/src/lib.rs) |
| Shell calls | `function_call` `exec_command` with arguments `{ cmd, workdir, yield_time_ms, … }`; the output is `Chunk ID` / `Wall time` / `Process exited with code N` / `Original token count` / `Output:` then the output. Older CLIs: `shell` with `{ command: ["bash", "-lc", cmd], workdir }` and a JSON output `{ output, metadata: { exit_code, duration_seconds } }` (rust-v0.40.0), or an `Exit code: N` header | source | [`core/src/tools/handlers/shell_spec.rs`](https://github.com/openai/codex/blob/rust-v0.156.1/codex-rs/core/src/tools/handlers/shell_spec.rs), `core/src/tools/context.rs`, `core/src/tools/mod.rs`; [`core/src/codex.rs` at rust-v0.40.0](https://github.com/openai/codex/blob/rust-v0.40.0/codex-rs/core/src/codex.rs) (`format_exec_output`) |
| `apply_patch` | A freeform tool: `custom_tool_call` `{ name: "apply_patch", input: <patch> }`; output `Success. Updated the following files:` and `A`/`M`/`D <path>` lines | source | [`core/src/tools/handlers/apply_patch_spec.rs`](https://github.com/openai/codex/blob/rust-v0.156.1/codex-rs/core/src/tools/handlers/apply_patch_spec.rs), [`apply-patch/src/lib.rs`](https://github.com/openai/codex/blob/rust-v0.156.1/codex-rs/apply-patch/src/lib.rs) |
| Compression | Rollouts idle for 7 days are rewritten as `.jsonl.zst` by a background worker, only with the feature `local_thread_store_compression` (under development, off by default) | source | [`rollout/src/compression.rs`](https://github.com/openai/codex/blob/rust-v0.156.1/codex-rs/rollout/src/compression.rs), `features/src/lib.rs` |
| AGENTS.md | Read from each directory (`AGENTS.override.md`, `AGENTS.md`, fallbacks), 32 KiB total by default | documented | <https://learn.chatgpt.com/docs/agent-configuration/agents-md> |
| Agent-session environment | `CODEX_SANDBOX` / `CODEX_SANDBOX_NETWORK_DISABLED` set in the agent's shell | inferred | — |

### What Adage does

Codex's hook contract mirrors Claude Code's, so the adapter
(`crates/adage-hook/src/harness/codex.rs`) is Claude Code's with three differences:

- `apply_patch` is split into one `Edit`/`Write` call per file named in the patch, so finish-line
  reflexes trigger on Codex edits (`adage_core::tools::from_codex`, `patch_files`).
- Each `PostToolUse` is a batch of one call. A failure is recognised only from a structured
  `exit_code` in `tool_response` — not supplied today (inferred to be how a fix would look).
  Dead ends *learned from history* do not need it: they are announced once, at the first prompt,
  in every harness.
- Stop `block` is used, as Architecture F3 allows, because the reason is a fixed template that
  says plainly it is from Adage, not the user. When that reason comes back as a
  `UserPromptSubmit` prompt, the adapter ignores it rather than matching prompt reflexes on it.

**Learning** — `adage condition --codex-sessions ~/.codex/sessions` reads every rollout under the
`YYYY/MM/DD` layout; `--transcripts` also accepts rollout files, recognised by their first line
(`crates/adage-learn/src/codex.rs`). The importer takes the prompt from the `user_message` event
(or, in paginated history, the first user message that is not Codex's own context such as
`<environment_context>` or AGENTS.md), the model from `turn_context`, commands from
`exec_command`, `shell_command`, `shell` argv, and `local_shell_call`, edits from `apply_patch`
(one per file), and each call's exit code from its output. Rollouts record exit codes, so **dead
ends are learned from Codex history**, and delivered in Claude Code, OpenCode, and Codex alike.
Steering is detected from `developer` messages (hook context) and from Adage's finish-line prompt.
Sub-agent threads are skipped; compressed `.jsonl.zst` rollouts are counted and skipped
(decompress them with `zstd -d` to include them).

`adage install codex [--write] [--scope project|user]` prints or merges a `hooks.json`
registering `SessionStart`, `UserPromptSubmit`, `PostToolUse`, `Stop`, and `SessionEnd`, with the
binary path shell-quoted. Existing hooks are kept as written; Adage's own entries are replaced, so
reinstalling is idempotent, and `--remove` takes them out again. Codex runs both a user-scope and a
repository `hooks.json`, so Adage registers in one of them only. Codex then asks you to review the
hooks in `/hooks`.

### Not possible, or not done

- **After-failure reflexes on Codex.** Learned dead ends work (announced at the first prompt), but a
  reflex that reacts to a failed command — authored or imported with an `after_failure` trigger —
  cannot fire: Codex reports no failures to hooks (#34289, and still so at rust-v0.156.1). It
  starts working if Codex adds an exit code or fires `PostToolUseFailure`, both of which the
  adapter already accepts. A possible workaround, not built: the *next* hook call could read the
  previous call's exit code from `transcript_path`, one call late; whether the rollout writer has
  flushed it by then only a live install can show.
- **Hidden context.** Directives appear as developer messages in the TUI (#16933).
- **A documented rollout format.** The importer follows source at rust-v0.156.1 (and the older
  shapes at rust-v0.40.0); a Codex release may change it without notice. Unknown record types
  are ignored, so a change degrades to fewer learned steps, not errors.
- **AGENTS.md export.** Not needed: Codex hooks can inject context at the right moment, which a
  static file cannot.

## Verification

**The harness simulator** (`crates/adage-sim`) renders one scenario — the acme-billing sandbox:
work items only via `scripts/wi-lookup`, `make test` where `unittest`/`pytest` fail, `make gen`
after schema edits — as each harness records it on disk (12 seeded sessions per harness in
`sim/fixtures/`, including noise) and as each harness's hooks deliver it (native payloads for a
held-out session). `crates/adage-cli/tests/harness_sim.rs` then drives the real binary, per
harness: learn from that harness's history → replay the held-out session through
`adage hook <harness> <event>` in the treated and the control arm → check every response and
`adage graph --json`. `cargo run -p adage-sim` rewrites the fixtures; a test fails if they drift.

The mock data is written from the formats as recorded above, so the simulator proves Adage
against *our reading* of each harness, not the harness itself:

| Claim | Claude Code | OpenCode | Codex |
| --- | --- | --- | --- |
| The importer reads that harness's history and learns the scenario's reflexes (lookup, linked files, dead end, finish line) | simulated (and real, in the demo) | simulated (export shape from source) | simulated (rollout shapes from source, current and older) |
| Each directive arrives at its moment, in the harness's output shape: lookup and dead end at the prompt, linked files after the lookup, finish line after the schema edit, continue at the first stop, nothing at the second | simulated (and real, in the demo) | simulated | simulated |
| The control arm is silent throughout; `adage graph` counts both sessions, split by arm | simulated | simulated | simulated |
| Adage's own continuation, coming back as a prompt, matches no prompt reflex | simulated | simulated | simulated |
| The plugin places text as designed (context message, appended tool output, idle continuation) | — | simulated in node with a fake client, against the real binary | — |
| Reflexes learned in one harness deliver in another | simulated (all three histories at once) | simulated (from Codex history) | simulated (all three) |
| The harness really sends these payloads, in this order | real (demo) | **needs a live install** | **needs a live install** |
| The model really sees the text (OpenCode: `synthetic` accepted on `session.prompt` parts, the `noReply` message lands before the user's, the `tool.execute.after` mutation reaches the model; Codex: context as a developer message) | real (demo) | **needs a live install** | **needs a live install** |
| Real history files parse (a real `opencode export`; rollouts from the installed Codex version) | real (demo) | **needs a real export** | **needs real rollouts** |
| Install and trust (`.opencode/plugins` loads the plugin; Codex `/hooks` review) | real | **needs a live install** | **needs a live install** |

**Found by the simulator:** `adage graph` keyed sessions by the first 8 characters of their id.
OpenCode session ids and Codex UUIDv7 thread ids begin with the creation time, so two sessions
started together were counted as one; graph now keys by the whole id (the dashboard shortens it
for display).

**First checks on a live install.** OpenCode: export one real session and `adage condition` it; run a
work-item prompt with the plugin installed and confirm the context message, the appended tool
text, and the idle continuation reach the model (in the TUI and in `opencode run`). Codex: learn
from the installed version's `~/.codex/sessions`; add a logging hook next to Adage's and diff the
payloads it records against `adage_sim::codex::turn_hooks`; confirm a Stop `block` comes back as a
`UserPromptSubmit` carrying the reason.

## Normalised events

All three adapters translate into `adage_runtime::Event`; the decision core
(`crates/adage-hook/src/lib.rs`) never sees a native payload.

| Normalised | Claude Code | OpenCode (plugin → adage) | Codex |
| --- | --- | --- | --- |
| `SessionStart` | `SessionStart` | `session.created` | `SessionStart` |
| `Prompt` | `UserPromptSubmit` | `chat.message` | `UserPromptSubmit` |
| `Batch` | `PostToolBatch`, `PostToolUse` | `tool.execute.after` | `PostToolUse` |
| `Failure` | `PostToolUseFailure` | `tool.execute.after` with exit ≠ 0, `tool.error` | `PostToolUse` with an exit code (none today) |
| `Stop` | `Stop` | `session.idle` | `Stop` |
| `SessionEnd` | `SessionEnd` | `session.deleted` | `SessionEnd` |
