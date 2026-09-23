# Harnesses: what each one supports, and what Adage does with it

Research date: **2026-09-22**. Sources are the harnesses' official documentation and source
repositories, read on that date; neither OpenCode nor Codex was installed or run. Every claim is
marked **documented** (stated in official docs or the published SDK/plugin types), **source**
(read in the harness's own source code, so true today but not a promised interface), or
**inferred** (our reading, not verified by running the harness). Anything inferred is the first
thing to check against a real installation.

Design context: Design Spec §13.1 and decisions O8 and O14; Architecture §4.2, §4.11, §6.2, §15.1 F3.

## Summary

| | Claude Code | OpenCode | Codex CLI |
| --- | --- | --- | --- |
| Registration | Plugin `hooks/hooks.json`, or settings `hooks` | JS plugin in `.opencode/plugins/` or `~/.config/opencode/plugins/` | `hooks.json` in `~/.codex/` or `<repo>/.codex/` |
| Transport | Command hook, payload on stdin | In-process plugin; Adage's plugin spawns `adage hook opencode <event>` | Command hook (shell string), payload on stdin |
| Prompt directives | `additionalContext` | Context-only synthetic message (`session.prompt`, `noReply: true`) | `additionalContext` (developer context) |
| After-tool directives | `PostToolBatch` `additionalContext` | Appended to the tool result (`tool.execute.after`) | `PostToolUse` `additionalContext` |
| Failure routes (dead ends) | `PostToolUseFailure` | Shell `metadata.exit` ≠ 0, or a tool part in state `error` | **Not possible today**: no failure signal |
| Finish line at stop | Stop `decision: block` | `session.idle` → a synthetic follow-up prompt | Stop `decision: block` (becomes a user prompt) |
| Batches | Yes (`PostToolBatch`) | No (per call) | No (per call) |
| Learning from history | `~/.claude/projects/*.jsonl` | `opencode export <id>` JSON files | **Not built**: rollout format undocumented |
| Install | `adage install claude-code` / plugin | `adage install opencode` | `adage install codex` |

## Claude Code

Unchanged by this slice; recorded for comparison. Hooks, payloads, and `additionalContext` are
documented at <https://code.claude.com/docs/en/hooks>. Adage's adapter is
`crates/adage-cli/src/harness/claude.rs`; its output is pinned byte for byte by
`crates/adage-cli/tests/golden/claude-code.txt`.

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
| Storage format | A SQLite database (`opencode.db`) since v1.2, with legacy tables frozen and new event-sourced tables (`session_v2`, `session_message`) | not documented (community reports) | <https://x.com/thdxr/status/2022562561732812841>, <https://github.com/nicosuave/memex/pull/195> |
| Export | `opencode export [sessionID]` — "Export session data as JSON"; writes `{ info, messages }` | documented (command) + source (shape) | <https://opencode.ai/docs/cli/>, `packages/opencode/src/cli/cmd/export.ts` |

### What Adage does

**Adapter** — `adage hook opencode <event>` (`crates/adage-cli/src/harness/opencode.rs`) reads a
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
- on `message.part.updated` with a tool part in state `error`, asks for failure routes and holds
  the directive for the next tool result (there is no mutable output on an event).
- at `session.idle`, asks the finish-line question with `stop_hook_active` set if Adage already
  asked this session to continue since the user last spoke, and if Adage asks to continue, sends
  the fixed-template reason as a synthetic prompt (fire and forget). The runtime bounds these
  requests (once per obligation, twice per session).
- never evaluates, executes, or shells out with text it receives: directive text only ever goes
  into `output.output` or a message part.

**Learning** — `adage learn --transcripts <export.json>…` accepts `opencode export` files
(recognised by content: `info` is the first key) alongside Claude Code transcripts
(`crates/adage-learn/src/opencode.rs`). Steering is detected from synthetic user parts and
directives appended to tool output, both of which the export records. Sub-agent sessions
(`parentID`) are skipped.

**Install** — `adage install opencode [--write] [--scope project|user]` prints (default) or writes
the plugin with the binary's absolute path embedded, to `.opencode/plugins/adage.js` or
`$XDG_CONFIG_HOME/opencode/plugins/adage.js`. It replaces that file only if it is Adage's.

### Not possible, or not done

- **Reading OpenCode's storage directly.** The SQLite schema is undocumented and changing; Adage
  reads `opencode export` output instead, so importing history is one `opencode export` per session.
- **A true stop hook.** OpenCode has none (#16626 open). Adage's finish-line reminder arrives as
  a new synthetic user message after the session went idle; in `opencode run` it may race with
  process exit (per #16626) — inferred.
- **Hidden context.** The context message and the appended tool-result text are visible in the
  transcript and TUI (inferred). They are wrapped in `<adage-signal>` and say they are from Adage.
- **Batches.** `tool.execute.after` fires per call, so follow/derail windows count calls, not
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
| Context visibility | `additionalContext` is shown as a developer message in the TUI | documented (issue, open) | <https://github.com/openai/codex/issues/16933> |
| Sessions on disk | `$CODEX_HOME/sessions` (default `~/.codex/sessions`), archived in `archived_sessions` | documented (location only) | <https://learn.chatgpt.com/docs/reference/troubleshooting> |
| Rollout format | `rollout-*.jsonl` lines of `{ timestamp, type, payload }` with `session_meta`, `response_item`, `event_msg`, … | not documented (community reverse-engineering; older rollouts compressed) | <https://github.com/openai/codex/discussions/3827> |
| AGENTS.md | Read from each directory (`AGENTS.override.md`, `AGENTS.md`, fallbacks), 32 KiB total by default | documented | <https://learn.chatgpt.com/docs/agent-configuration/agents-md> |
| Agent-session environment | `CODEX_SANDBOX` / `CODEX_SANDBOX_NETWORK_DISABLED` set in the agent's shell | inferred | — |

### What Adage does

Codex's hook contract mirrors Claude Code's, so the adapter
(`crates/adage-cli/src/harness/codex.rs`) is Claude Code's with three differences:

- `apply_patch` is split into one `Edit`/`Write` call per file named in the patch, so finish-line
  routes trigger on Codex edits (`adage_core::tools::from_codex`, `patch_files`).
- Each `PostToolUse` is a batch of one call. A failure is recognised only from a structured
  `exit_code` in `tool_response` — not supplied today (inferred to be how a fix would look).
- Stop `block` is used, as Architecture F3 allows, because the reason is a fixed template that
  says plainly it is from Adage, not the user. When that reason comes back as a
  `UserPromptSubmit` prompt, the adapter ignores it rather than matching prompt routes on it.

`adage install codex [--write] [--scope project|user]` prints or merges a `hooks.json`
registering `SessionStart`, `UserPromptSubmit`, `PostToolUse`, `Stop`, and `SessionEnd`, with the
binary path shell-quoted. Existing hooks are kept; Adage's own entries are replaced, so reinstalling
is idempotent. Codex then asks you to review the hooks in `/hooks`.

### Not possible, or not done

- **Dead-end routes on Codex.** They trigger on a failed command, and Codex reports no failures to
  hooks (#34289). They start working if Codex adds an exit code or fires `PostToolUseFailure`,
  both of which the adapter already accepts.
- **Hidden context.** Directives appear as developer messages in the TUI (#16933).
- **Learning from Codex history.** The rollout format is not documented, so there is no importer;
  `transcript_path` in every hook payload points at it if that changes. Codex routes are learned
  from Claude Code and OpenCode sessions in the same repository.
- **AGENTS.md export.** Not needed: Codex hooks can inject context at the right moment, which a
  static file cannot.

## Normalised events

All three adapters translate into `adage_runtime::Event`; the decision core
(`crates/adage-cli/src/hook.rs`) never sees a native payload.

| Normalised | Claude Code | OpenCode (plugin → adage) | Codex |
| --- | --- | --- | --- |
| `SessionStart` | `SessionStart` | `session.created` | `SessionStart` |
| `Prompt` | `UserPromptSubmit` | `chat.message` | `UserPromptSubmit` |
| `Batch` | `PostToolBatch`, `PostToolUse` | `tool.execute.after` | `PostToolUse` |
| `Failure` | `PostToolUseFailure` | `tool.execute.after` with exit ≠ 0, `tool.error` | `PostToolUse` with an exit code (none today) |
| `Stop` | `Stop` | `session.idle` | `Stop` |
| `SessionEnd` | `SessionEnd` | `session.deleted` | `SessionEnd` |
