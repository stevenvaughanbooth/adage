// The Bash call (docs/claude-mod/architecture.md §3.3, §3.4; plan C3, C6): before a Bash call runs,
// the binary is asked for its plan (`dry_run`), which names at most a receipt, a learned
// replacement, and a rewrite or advice. The person's rules are Claude Code's: each branch asks the
// engine what they decide (`$.tool.check`, which runs nothing) before the binary is asked for its
// final word (`verdict`), and only that word changes what happens to the call:
//
// - a fresh receipt answers the call in its place, only when the rules would have let the call as
//   written run unasked (no dialog appears for an answered call);
// - a learned replacement runs instead of the dead end, only when the rules treat both alike (a
//   dead end piped into output filters is replaced as the whole new pipeline the binary plans, the
//   filters as written, and the rules are read on both whole pipelines);
// - a rewrite runs the call on the project's interpreter when the rules treat both alike; when
//   they do not, the binary turns it into advice, which the mod hands the model itself (the
//   command hook that follows for the same call prints nothing once a verdict decided it).
//
// Everything else, a failure included, runs the call as the model wrote it and leaves the command
// hook to decide as it always has (§6 invariants 1 and 2). `bashCall` is the call itself over
// plain deps; `onBashCall` binds it to the mod and keeps the markers each decision earns.
import type { ToolCallResult } from 'claude-code'

import { askBinary } from './binary'
import type { Asked } from './bridge'
import { keepCall } from './ledger'
import { mark } from './marks'
import type { AnswerAt, EventAt, Mod, NextAt } from './mod'
import { madeForAgentLine } from './predict'
import { answeredMark, receiptAnswerOf, receiptPlanOf, shortId } from './receipt'
import type { ReceiptAnswer, ReceiptPlan } from './receipt'
import { isRecord } from './shape'
import { substituteAnswerOf, substituteOf } from './substitute'
import type { Substitute } from './substitute'
import { markText, receiptOf, rewriteOf } from './view/lines'

/** What the mod tells the binary it found out (A1's `verdict`), exactly one decision per call. */
type Verdict = { engine: 'same' | 'differs'; substitute?: string } | { answered: 'receipt' }

/** The binary's `PreToolUse` payload for the mod's two phases (A1, C3, C6), exactly. */
export type PreToolPayload = {
  session_id: string
  cwd: string
  hook_event_name: 'PreToolUse'
  tool_name: 'Bash'
  tool_input: { command: string }
  tool_use_id: string
  /** Every payload the mod sends says so: the binary then reads the mod's capabilities (C9). */
  via: 'mod'
  /** The subagent the call is made in (`tool.call`'s `agentId`); absent on the main loop. */
  agent_id?: string
  dry_run?: true
  verdict?: Verdict
  /** Observe mode: the binary records what it would substitute, as the control arm does. */
  observe?: true
}

export type CallDeps = {
  /** False while the mod is inert: every call runs unread. */
  isActive: boolean
  /**
   * Observe mode (ADAGE_MOD_OBSERVE): the plan is asked for and a replacement's verdict with
   * `observe`, and nothing is replaced or answered.
   */
  isObserving: boolean
  ask: (payload: PreToolPayload) => Promise<Asked>
  context: () => Promise<{ sessionId: string; cwd: string }>
  /** The person's rules on a Bash command now (`$.tool.check`): `allow`, `ask` or `deny`. */
  check: (command: string) => Promise<string>
  /** One line under the call's permission dialog, if one opens (`$.ui.notice`). */
  notice: (toolUseId: string, text: string) => void
  /** Keeps a receipt that answered the call, for the card in its result row; a failure is ignored. */
  answered?: (toolUseId: string, answer: ReceiptAnswer) => Promise<void>
  /**
   * Keeps a covering receipt that did not answer the call (not fresh, the rules asked, or the
   * binary withheld it), for the dim line beside the run that happens (`result.receipt.beside`);
   * a failure is ignored.
   */
  beside?: (toolUseId: string, operation: string) => Promise<void>
  /** Keeps the replacement for the marker under the call's row (`tool.substituted`); a failure is ignored. */
  made?: (toolUseId: string, command: string) => Promise<void>
  /**
   * Keeps a rewrite for the marker under the call's row and says it under the dialog
   * (`tool.rewrite`); a failure is ignored.
   */
  rewrote?: (toolUseId: string, from: string, to: string) => Promise<void>
  /** Keeps what the final answer told the model beside the run (a receipt note); a failure is ignored. */
  delivered?: (toolUseId: string, context: readonly string[]) => Promise<void>
}

/** Both phases run before the call does, so each has the budget a command hook would. */
const PRE_TOOL_TIMEOUT_MS = 2000

/** The parts of a dry run's plan the mod acts on; a missing or malformed part is undefined. */
export type Plan = { receipt: ReceiptPlan | undefined; substitute: Substitute | undefined; rewrite: { command: string; kind: string } | undefined }

/**
 * The dry run's answer (`{ plan }`, docs/claude-mod/impl/wire.md), each part read by its own
 * reader; undefined when there is no plan at all. The plan's `advice` is not read: the command
 * hook that follows the call delivers it.
 */
export function planOf(answer: unknown): Plan | undefined {
  if (!isRecord(answer) || !isRecord(answer.plan)) return undefined
  const p = answer.plan
  return { receipt: receiptPlanOf(p.receipt), substitute: substituteOf(p.substitute), rewrite: rewritePlanOf(p.rewrite) }
}

/** The plan's `rewrite`: the command the call would run as, when U11 bound an environment here. */
export function rewritePlanOf(v: unknown): { command: string; kind: string } | undefined {
  if (!isRecord(v) || typeof v.command !== 'string' || v.command.trim() === '') return undefined
  return { command: v.command, kind: typeof v.kind === 'string' ? v.kind : '' }
}

/**
 * The final answer to an `engine` verdict, as the command hook's own output reads (A1): the
 * command the call runs as (`updatedInput.command`, a rewrite), what the model reads beside the
 * call (`additionalContext`: the rewrite's word, the advice, a receipt note) and the person's line
 * (`systemMessage`). Undefined when it says nothing the mod can use.
 */
export function verdictAnswerOf(answer: unknown): { command: string | undefined; context: string[] } | undefined {
  if (!isRecord(answer) || !isRecord(answer.hookSpecificOutput)) return undefined
  const out = answer.hookSpecificOutput
  const updated = isRecord(out.updatedInput) ? out.updatedInput.command : undefined
  const command = typeof updated === 'string' && updated.trim() !== '' ? updated : undefined
  const context = typeof out.additionalContext === 'string' && out.additionalContext !== '' ? [out.additionalContext] : []
  return command === undefined && context.length === 0 ? undefined : { command, context }
}

/** A result with more for the model to read after it; a refused call is returned as it was. */
function withContext(ran: ToolCallResult, context: readonly string[]): ToolCallResult {
  if (ran.deny !== undefined || context.length === 0) return ran
  return { ...ran, context: [...(ran.context ?? []), ...context] }
}

type BashCall = { command: string; tool_use_id: string; agentId?: string }

/**
 * The hook's body: `on('tool.call', { tool: 'Bash' }, ($, e, next) => bashCall(e, next, deps))`.
 * A call Adage made in the model's place (`adage_` id) is decided like any other, and its result
 * carries the line that says whose call it was (`madeForAgentLine`).
 */
export async function bashCall<E extends BashCall>(e: E, next: (e: E) => Promise<ToolCallResult>, deps: CallDeps): Promise<ToolCallResult> {
  const ran = await decided(e, next, deps)
  const line = deps.isActive ? madeForAgentLine(e.tool_use_id) : undefined
  return line === undefined ? ran : withContext(ran, [line])
}

/**
 * Any other tool's call (`on('tool.call', …)`, every tool but Bash, which `bashCall` answers): a
 * call Adage made in the model's place (the chain's reads, a skill) runs as it is, and its result
 * carries the line that says whose call it was; every other call passes untouched.
 */
export async function onPredictedCall(m: Mod, e: EventAt<'tool.call'>, next: NextAt<'tool.call'>): AnswerAt<'tool.call'> {
  // The ledger counts every main-loop call once: Bash's in its own hook, which a receipt may answer
  // before this one is reached.
  if (e.tool !== 'Bash') keepCall(m, e, next.origin)
  const line = e.tool === 'Bash' ? undefined : madeForAgentLine(e.tool_use_id)
  if (line === undefined) return next(e)
  const ran = await next(e)
  return ran.deny !== undefined ? ran : { ...ran, context: [...(ran.context ?? []), line] }
}

async function decided<E extends BashCall>(e: E, next: (e: E) => Promise<ToolCallResult>, deps: CallDeps): Promise<ToolCallResult> {
  if (!deps.isActive || typeof e.command !== 'string') return next(e)
  let ctx: { sessionId: string; cwd: string }
  try {
    ctx = await deps.context()
  } catch {
    return next(e)
  }
  const id = e.tool_use_id
  const base: PreToolPayload = {
    session_id: ctx.sessionId,
    cwd: ctx.cwd,
    hook_event_name: 'PreToolUse',
    tool_name: 'Bash',
    tool_input: { command: e.command },
    tool_use_id: id,
    via: 'mod',
    ...(typeof e.agentId === 'string' && e.agentId !== '' ? { agent_id: e.agentId } : {}),
    ...(deps.isObserving ? { observe: true as const } : {}),
  }
  const ask = (p: PreToolPayload) => deps.ask(p).catch((): Asked => ({ ok: false }))
  const quiet = (work: Promise<void> | undefined) => (work ?? Promise.resolve()).catch(() => undefined)

  const planned = await ask({ ...base, dry_run: true })
  const plan = planned.ok ? planOf(planned.answer) : undefined
  if (plan === undefined) return next(e)

  // The rules on the call as written, asked at most once whichever branches need it; a check that
  // fails is no answer, and no branch that needs it is taken.
  let written: Promise<string | undefined> | undefined
  const asWritten = () => (written ??= deps.check(e.command).then(d => d, () => undefined))
  const alike = async (other: string): Promise<boolean | undefined> => {
    const [a, b] = await Promise.all([asWritten(), deps.check(other).then(d => d, () => undefined)])
    return a === undefined || b === undefined ? undefined : a === b
  }

  // §3.3 The receipt as the result: no dialog appears for an answered call, so only a call the
  // rules would have let run unasked is answered. A receipt that is not fresh, or a call the rules
  // ask about, runs: the binary says nothing more than it would have.
  const receipt = plan.receipt
  if (receipt?.fresh === true && receipt.result !== '' && !deps.isObserving && (await asWritten()) === 'allow') {
    const final = await ask({ ...base, verdict: { answered: 'receipt' } })
    const answer = final.ok ? receiptAnswerOf(final.answer) : undefined
    if (answer !== undefined) {
      await quiet(deps.answered?.(id, answer))
      return { result: { stdout: answer.result, stderr: '', interrupted: false }, ...(answer.note === '' ? {} : { context: [answer.note] }) }
    }
  }
  // A covering receipt that did not answer the call: the run happens, and the row says a receipt
  // covers it (T-C3-07). The command hook still delivers a fresh one's note beside the result.
  if (receipt !== undefined && receipt.operation !== '' && !deps.isObserving) await quiet(deps.beside?.(id, receipt.operation))

  // §3.4 The dead end that never runs: the learned command, only on the same verdict for the call
  // and for the command the binary planned (the whole pipeline, when the dead end heads one).
  const sub = plan.substitute
  if (sub !== undefined && (await alike(sub.command)) === true) {
    const final = await ask({ ...base, verdict: { engine: 'same', substitute: sub.route } })
    const made = final.ok ? substituteAnswerOf(final.answer) : undefined
    if (made !== undefined && !deps.isObserving) {
      await quiet(deps.made?.(id, made.command))
      try {
        deps.notice(id, made.context)
      } catch {
        // No dialog is open for the call: the model's line below still says it.
      }
      return withContext(await next({ ...e, command: made.command }), [made.context])
    }
    // Withheld (the control arm, or observe mode): the binary records `would_substitute` and leaves
    // the call undecided, so the interpreter rewrite below still decides it: the verdict here in
    // the control arm (recorded withheld there), the command hook in observe mode.
  }

  // §3.4 Rewrites checked by the engine: the engine's comparison replaces the planner's own reading
  // of the settings files. Advice alone, or nothing, is the command hook's to deliver as before.
  const rewrite = plan.rewrite
  if (rewrite === undefined || deps.isObserving) return next(e)
  const same = await alike(rewrite.command)
  if (same === undefined) return next(e)
  const final = await ask({ ...base, verdict: { engine: same ? 'same' : 'differs' } })
  const said = final.ok ? verdictAnswerOf(final.answer) : undefined
  if (said === undefined) return next(e)
  await quiet(deps.delivered?.(id, said.context))
  if (said.command === undefined || said.command === e.command) return withContext(await next(e), said.context)
  await quiet(deps.rewrote?.(id, e.command, said.command))
  return withContext(await next({ ...e, command: said.command }), said.context)
}

/** The rewrite under a call's row (`tool.rewrite`), and the same line under its dialog. */
async function rewrote(m: Mod, id: string, from: string, to: string): Promise<void> {
  const rw = rewriteOf(from, to)
  if (rw === undefined) return
  const x = { id, kind: 'rewrite' as const, ...rw }
  await mark(m, x)
  try {
    m.ui.notice(id, markText(x, (await m.state.mode.read()).ascii))
  } catch {
    // No dialog is open for the call: the row's marker says it.
  }
}

/**
 * The hook body (`on('tool.call', { tool: 'Bash' }, …)`, §3.3, §3.4; C3, C6): a fresh receipt
 * answers the call when the rules would have let it run unasked; a learned replacement or a rewrite
 * runs only on the same verdict for both commands. Each decision keeps its marker for the row.
 */
export async function onBashCall(m: Mod, e: EventAt<'tool.call', { tool: 'Bash' }>, next: NextAt<'tool.call', { tool: 'Bash' }>): AnswerAt<'tool.call', { tool: 'Bash' }> {
  keepCall(m, e, next.origin)
  return bashCall(e, next, {
    isActive: m.rt.bin !== undefined,
    isObserving: m.rt.observing,
    ask: payload => askBinary(m, 'PreToolUse', payload, PRE_TOOL_TIMEOUT_MS),
    context: async () => ({ sessionId: await m.session.id(), cwd: await m.session.cwd() }),
    check: command => m.checkBash(command),
    notice: (id, text) => m.ui.notice(id, text),
    answered: async (id, a) => {
      const x = answeredMark(id, a)
      if (x !== undefined) await mark(m, x)
    },
    beside: (id, operation) => mark(m, { id, kind: 'receipt', receipt: shortId(operation) }),
    made: (id, command) => mark(m, { id, kind: 'substituted', command }),
    rewrote: (id, from, to) => rewrote(m, id, from, to),
    delivered: async (id, context) => {
      const rc = receiptOf(context)
      if (rc !== undefined) await mark(m, { id, kind: 'receipt', receipt: rc })
    },
  })
}
