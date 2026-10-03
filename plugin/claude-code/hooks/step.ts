// The `turn.step` hook (docs/claude-mod/architecture.md §3.2; plan C4, C7, C8): before a request of
// the main loop, ask the binary whether it predicts the step; if it does, yield the predicted calls
// as the response and send no request. If not, forward the model's response, and at its end_turn
// stop ask once more with `ending: true` (the finish line): a prediction then keeps the model's
// text and replaces the stop with the calls.
//
// No `$` crosses into this file (`claude plugin validate` follows `$` only within one file):
// register.ts hands it closures over its `$`, and a test hands it stubs.
import type { HookStream, TurnStepChunk, TurnStepInput, TurnStepResult } from 'claude-code'

import type { Asked } from './bridge'
import type { Predicted } from './plugin'
import { callChunks, predictedResult, predictionOf } from './predict'

/** The binary's `TurnStep` payload (C1), exactly. */
export type TurnStepPayload = {
  session_id: string
  cwd: string
  hook_event_name: 'TurnStep'
  turn_id: string
  index: number
  model: string
  agent_id?: string
  ending: boolean
  /** Observe mode: the binary records what it would predict, as the control arm does. */
  observe?: true
}

export type StepDeps = {
  /** False while the mod is inert: every step is forwarded unread. */
  isActive: boolean
  /** Observe mode (ADAGE_MOD_OBSERVE): every step is asked about and forwarded; none is yielded. */
  isObserving: boolean
  /** Whether the session lists this skill (`$.command.list()`): a Skill call it cannot run is never yielded. */
  hasSkill: (skill: string) => Promise<boolean>
  ask: (payload: TurnStepPayload) => Promise<Asked>
  /** The session's id and working directory, read at each step (a /clear changes the id). */
  context: () => Promise<{ sessionId: string; cwd: string }>
  /** Records a predicted step for drawing; a failure is ignored. */
  record: (p: Predicted) => Promise<void>
}

/** The per-step budget for the binary (§2 rule 1): it runs before every model request. */
export const TURN_STEP_TIMEOUT_MS = 250

const blockOf = (c: TurnStepChunk): number => ('index' in c && typeof c.index === 'number' ? c.index : -1)

/** The stream beneath, as the hook's `next` opens it. */
export type StepNext = (e: TurnStepInput) => HookStream<TurnStepChunk, TurnStepResult>

/** The hook's body: `on('turn.step', async function* ($, e, next) { return yield* turnStep(e, next, deps) })`. */
export async function* turnStep(
  e: Readonly<TurnStepInput>,
  next: StepNext,
  deps: StepDeps,
): AsyncGenerator<TurnStepChunk, TurnStepResult> {
  if (!deps.isActive || e.agentId !== undefined || e.index === 0) return yield* next(e)

  let ctx: { sessionId: string; cwd: string }
  try {
    ctx = await deps.context()
  } catch {
    return yield* next(e)
  }
  const payload = (ending: boolean): TurnStepPayload => ({
    session_id: ctx.sessionId,
    cwd: ctx.cwd,
    hook_event_name: 'TurnStep',
    turn_id: e.turnId,
    index: e.index,
    model: e.model,
    ending,
    ...(deps.isObserving ? { observe: true as const } : {}),
  })
  const predicted = async (ending: boolean) => {
    const asked = await deps.ask(payload(ending)).catch((): Asked => ({ ok: false }))
    const p = asked.ok && !deps.isObserving ? predictionOf(asked.answer) : undefined
    if (p?.kind !== 'skill') return p
    const skill = String(p.calls[0]?.input.skill)
    return (await deps.hasSkill(skill).catch(() => false)) ? p : undefined
  }
  const note = async (p: NonNullable<Awaited<ReturnType<typeof predicted>>>, isEnding: boolean, first: number) => {
    const ids = callChunks(p, e.turnId, e.index, first).flatMap(c => (c.kind === 'tool' ? [c.id] : []))
    await deps
      .record({ turnId: e.turnId, index: e.index, route: p.route, kind: p.kind, ids, isEnding })
      .catch(() => undefined)
  }

  // Before the request: a prediction is the whole response; no request is made.
  const before = await predicted(false)
  if (before !== undefined) {
    yield* callChunks(before, e.turnId, e.index, 0)
    yield { kind: 'stop', stopReason: 'tool_use', usage: null }
    await note(before, false, 0)
    return predictedResult(before, e.turnId, e.index)
  }

  // The request goes out. Its chunks pass as they come; the stop is held until it is known
  // whether the response ends the turn (end_turn with no tool call of its own).
  const stream = next(e)
  let lastBlock = -1
  let hasToolCall = false
  let ended: Awaited<ReturnType<typeof predicted>>
  let endedAt = 0
  for await (const chunk of stream) {
    if (chunk.kind === 'stop') {
      if (chunk.stopReason === 'end_turn' && !hasToolCall) {
        ended = await predicted(true)
        if (ended !== undefined) {
          endedAt = lastBlock + 1
          yield* callChunks(ended, e.turnId, e.index, endedAt)
          yield { ...chunk, stopReason: 'tool_use' }
          continue
        }
      }
      yield chunk
      continue
    }
    if (chunk.kind === 'tool') hasToolCall = true
    lastBlock = Math.max(lastBlock, blockOf(chunk))
    yield chunk
  }
  const result = await stream.result
  if (ended === undefined) return result
  await note(ended, true, endedAt)
  return {
    ...result,
    toolUses: [...result.toolUses, ...ended.calls.map(c => ({ name: c.tool, input: c.input }))],
    stopReason: 'tool_use',
  }
}
