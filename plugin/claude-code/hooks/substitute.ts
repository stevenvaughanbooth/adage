// The dead end that never runs (docs/claude-mod/architecture.md §3.4; plan C6): before a Bash call
// runs, ask the binary whether it is a dead end this repository learned. If the plan names a
// replacement, the person's rules must decide both commands alike (`$.tool.check`) before the
// binary is asked for its final word, and only that word replaces the command. Everything else,
// a failure included, runs the call as the model wrote it (§6 invariants 1 and 2).
//
// No `$` crosses into this file (`claude plugin validate` follows `$` only within one file):
// register.ts hands it closures over its `$`, and a test hands it stubs.
import type { ToolCallResult } from 'claude-code'

import type { Asked } from './bridge'

/** What the binary would run instead, and the one line said about it. */
export type Substitute = { command: string; route: string; track: string; context: string }

/** The binary's `PreToolUse` payload for the mod's two phases (C6), exactly. */
export type PreToolPayload = {
  session_id: string
  cwd: string
  hook_event_name: 'PreToolUse'
  tool_name: 'Bash'
  tool_input: { command: string }
  tool_use_id: string
  dry_run?: true
  verdict?: { engine: 'same'; substitute: string }
  /** Observe mode: the binary records what it would substitute, as the control arm does. */
  observe?: true
}

export type SubstituteDeps = {
  /** False while the mod is inert: every call runs unread. */
  isActive: boolean
  /** Observe mode (ADAGE_MOD_OBSERVE): the binary is asked at every call; nothing is replaced. */
  isObserving: boolean
  ask: (payload: PreToolPayload) => Promise<Asked>
  context: () => Promise<{ sessionId: string; cwd: string }>
  /** The person's rules on a Bash command now (`$.tool.check`): `allow`, `ask` or `deny`. */
  check: (command: string) => Promise<string>
  /** One line under the call's permission dialog, if one opens (`$.ui.notice`). */
  notice: (toolUseId: string, text: string) => void
  /** Keeps the replacement for the marker under the call's row (`tool.substituted`); a failure is ignored. */
  made?: (toolUseId: string, command: string) => Promise<void>
}

/** Both phases run before the call does, so each has the budget a command hook would. */
export const PRE_TOOL_TIMEOUT_MS = 2000

const isRecord = (v: unknown): v is Record<string, unknown> =>
  v !== null && typeof v === 'object' && !Array.isArray(v)

/** `{ command, route, track, context }` with a non-empty command, route and context, or undefined. */
export function substituteOf(v: unknown): Substitute | undefined {
  if (!isRecord(v)) return undefined
  const { command, route, track, context } = v
  if (typeof command !== 'string' || command.trim() === '') return undefined
  if (typeof route !== 'string' || route === '' || typeof context !== 'string' || context === '') return undefined
  return { command, route, track: typeof track === 'string' ? track : '', context }
}

type BashCall = { command: string; tool_use_id: string }

/** The hook's body: `on('tool.call', { tool: 'Bash' }, ($, e, next) => substituteCall(e, next, deps))`. */
export async function substituteCall<E extends BashCall>(
  e: E,
  next: (e: E) => Promise<ToolCallResult>,
  deps: SubstituteDeps,
): Promise<ToolCallResult> {
  if (!deps.isActive || typeof e.command !== 'string') return next(e)
  let ctx: { sessionId: string; cwd: string }
  try {
    ctx = await deps.context()
  } catch {
    return next(e)
  }
  const base: PreToolPayload = {
    session_id: ctx.sessionId,
    cwd: ctx.cwd,
    hook_event_name: 'PreToolUse',
    tool_name: 'Bash',
    tool_input: { command: e.command },
    tool_use_id: e.tool_use_id,
    ...(deps.isObserving ? { observe: true as const } : {}),
  }
  const ask = (p: PreToolPayload) => deps.ask(p).catch((): Asked => ({ ok: false }))

  const planned = await ask({ ...base, dry_run: true })
  const plan = planned.ok && isRecord(planned.answer) ? planned.answer.plan : undefined
  const sub = isRecord(plan) ? substituteOf(plan.substitute) : undefined
  if (sub === undefined) return next(e)

  // Permissions are Claude Code's (§6 invariant 3): a replacement the rules would treat otherwise
  // than the call as written is never made, and the binary is not asked again.
  let same: boolean
  try {
    const [a, b] = await Promise.all([deps.check(e.command), deps.check(sub.command)])
    same = a === b
  } catch {
    return next(e)
  }
  if (!same) return next(e)

  const final = await ask({ ...base, verdict: { engine: 'same', substitute: sub.route } })
  const made = final.ok && isRecord(final.answer) ? substituteOf(final.answer.substitute) : undefined
  if (made === undefined || deps.isObserving) return next(e)

  await deps.made?.(e.tool_use_id, made.command).catch(() => undefined)
  try {
    deps.notice(e.tool_use_id, made.context)
  } catch {
    // No dialog is open for the call: the model's line below still says it.
  }
  const ran = await next({ ...e, command: made.command })
  if (ran.deny !== undefined) return ran
  return { ...ran, context: [...(ran.context ?? []), made.context] }
}
