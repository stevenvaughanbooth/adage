// The `turn.step` hook (docs/claude-mod/architecture.md §3.2, §3.5; plan C4, C5, C7, C8): before a
// request of the main loop, ask the binary whether it predicts the step; if it does, yield the
// predicted calls as the response (one call, or a chain's several) and send no request. If not,
// the request goes out, on the person's routine model when the binary marks the step routine and
// the person set one; its response is forwarded, and at an end_turn stop the binary is asked once
// more with `ending: true` (the finish line): a prediction then keeps the model's text, appends
// the calls, and ends the response with no stop reason. Either way a prediction is made only under
// the person's permissions: each call is checked first (`$.tool.check`, as the Bash branches do),
// and unless every one is allowed unasked nothing is yielded and the step goes on as if nothing
// were predicted. `turnStep` is the step itself over plain deps; `onStep` binds it to the mod,
// keeps the step's usage for the ledger note, and keeps a step held back for the band's line.
import type { HookStream, TurnStepChunk, TurnStepInput, TurnStepResult } from 'claude-code'

import { askBinary } from './binary'
import type { Asked } from './bridge'
import { keepStep } from './ledger'
import { record } from './marks'
import type { Mod } from './mod'
import { quietly } from './mod'
import type { Predicted } from './plugin'
import { PREDICTED_STOP, appendedStop, callChunks, heldBy, predictedResult, predictionOf } from './predict'
import type { CheckCall, Held, PredictKind } from './predict'
import { settings } from './refresh'
import { isRecord } from './shape'

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
  /** Every payload the mod sends says so: the binary then reads the mod's capabilities (C9). */
  via: 'mod'
  /**
   * The person set `runtime.routine_model`: the binary may mark the step routine (C5). The binary
   * never sees which model; absent, nothing is routed.
   */
  routine_model?: true
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
  /**
   * The person's permissions on one call now (`$.tool.check`: nothing runs, no dialog opens). A
   * prediction is yielded only when every call's verdict is `allow`.
   */
  check: CheckCall
  /** Keeps a prediction the permissions held back (nothing was yielded); a failure is ignored. */
  withheld?: (w: Withheld) => Promise<void>
  /** The person's routine model (`runtime.routine_model`, as the binary reports it); '' when none. */
  routineModel?: string
}

/**
 * A step the binary predicted and the person's permissions held back: which step, the route and
 * kind, and the first call not allowed unasked with the engine's verdict and sentence on it.
 */
export type Withheld = { turnId: string; index: number; route: string; kind: PredictKind; isEnding: boolean } & Held

/**
 * Whether a `TurnStep` answer marks the step routine (`{ routine: true }`, docs/claude-mod/impl/
 * wire.md): only `true` itself does.
 */
export const isRoutine = (answer: unknown): boolean => isRecord(answer) && answer.routine === true

/** The per-step budget for the binary (§2 rule 1): it runs before every model request. */
const TURN_STEP_TIMEOUT_MS = 250

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
  const routine = deps.routineModel ?? ''
  const payload = (ending: boolean): TurnStepPayload => ({
    session_id: ctx.sessionId,
    cwd: ctx.cwd,
    hook_event_name: 'TurnStep',
    turn_id: e.turnId,
    index: e.index,
    model: e.model,
    ending,
    via: 'mod',
    ...(routine !== '' ? { routine_model: true as const } : {}),
    ...(deps.isObserving ? { observe: true as const } : {}),
  })
  let routed = false
  const predicted = async (ending: boolean) => {
    const asked = await deps.ask(payload(ending)).catch((): Asked => ({ ok: false }))
    // A step is predicted or routed, never both; routing only changes a request that goes out.
    routed = !ending && asked.ok && isRoutine(asked.answer)
    const p = asked.ok && !deps.isObserving ? predictionOf(asked.answer) : undefined
    if (p === undefined) return undefined
    if (p.kind === 'skill' && !(await deps.hasSkill(String(p.calls[0]?.input.skill)).catch(() => false))) return undefined
    // §3.2: the call is made only under the person's permissions. A call they would ask about,
    // refuse, or not answer is not yielded: the step goes on as if nothing were predicted.
    const held = await heldBy(p.calls, deps.check)
    if (held === undefined) return p
    const w: Withheld = { turnId: e.turnId, index: e.index, route: p.route, kind: p.kind, isEnding: ending, ...held }
    await Promise.resolve()
      .then(() => deps.withheld?.(w))
      .catch(() => undefined)
    return undefined
  }
  const note = async (p: NonNullable<Awaited<ReturnType<typeof predicted>>>, isEnding: boolean, first: number) => {
    const ids = callChunks(p, e.turnId, e.index, first).flatMap(c => (c.kind === 'tool' ? [c.id] : []))
    await deps
      .record({ turnId: e.turnId, index: e.index, route: p.route, kind: p.kind, ids, isEnding })
      .catch(() => undefined)
  }

  // Before the request: a prediction is the whole response; no request is made, so its stop
  // states no reason (PREDICTED_STOP: the next request then starts the thread afresh).
  const before = await predicted(false)
  if (before !== undefined) {
    yield* callChunks(before, e.turnId, e.index, 0)
    yield PREDICTED_STOP
    await note(before, false, 0)
    return predictedResult(before, e.turnId, e.index)
  }

  // The request goes out, on the routine model at low effort when the binary marked the step
  // routine and the person chose one (§3.5: the setting is the consent). Its chunks pass as they
  // come; the stop is held until it is known whether the response ends the turn (end_turn with no
  // tool call of its own). A finish-line prediction appends its calls to that response, which the
  // server holds without them, so its stop states no reason (appendedStop: the next request then
  // starts the thread afresh, the calls in it, instead of continuing from the server's copy).
  const stream = next(routed && routine !== '' && !deps.isObserving ? { ...e, model: routine, effort: 'low' } : e)
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
          yield appendedStop(chunk)
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
    stopReason: null,
  }
}

/** The most held-back steps one load of the module keeps. */
const MAX_WITHHELD = 20

/**
 * Keeps a step the person's permissions held back (the binary's record cannot say so yet:
 * docs/claude-mod/impl/live-fixes-2-notes.md §7), and raises the band's line once per session: the
 * first held-back step of a session sets it, at the prompt count it was raised at, and the band
 * shows it until the next prompt.
 */
export async function keepWithheld(m: Mod, w: Withheld): Promise<void> {
  m.rt.withheld = [...m.rt.withheld, w].slice(-MAX_WITHHELD)
  const session = await m.session.id()
  if (m.rt.withheldSaid?.session === session) return
  m.rt.withheldSaid = { session, submits: (await m.state.turn.read()).submits }
  if (m.rt.interactive) m.ui.invalidate()
}

/**
 * The hook body (`on('turn.step', …)`): a headless session reads its routine model when it first
 * could use it (off the step's path; it routes from the step after the read lands), the step runs
 * over the mod's binary, session and the engine's permission check, and a main-loop step's usage
 * is kept for the ledger note at the turn's end.
 */
export async function* onStep(m: Mod, e: Readonly<TurnStepInput>, next: StepNext): AsyncGenerator<TurnStepChunk, TurnStepResult> {
  if (m.rt.bin !== undefined && !m.rt.interactive && !m.rt.settled && e.agentId === undefined && e.index > 0) {
    m.rt.settled = true
    quietly(settings(m))
  }
  let predicted = false
  const result = yield* turnStep(e, next, {
    isActive: m.rt.bin !== undefined,
    isObserving: m.rt.observing,
    hasSkill: async skill => (await m.command.list()).some(c => c.name === skill),
    ask: payload => askBinary(m, 'TurnStep', payload, TURN_STEP_TIMEOUT_MS),
    context: async () => ({ sessionId: await m.session.id(), cwd: await m.session.cwd() }),
    record: p => {
      // A step made wholly of predicted calls sent no request; the finish line's did.
      predicted ||= !p.isEnding
      return record(m, p)
    },
    routineModel: m.rt.routineModel,
    check: (tool, input) => m.checkTool(tool, input),
    withheld: w => keepWithheld(m, w),
  })
  if (m.rt.bin !== undefined && e.agentId === undefined) keepStep(m, e.model, result.usage, predicted)
  return result
}
