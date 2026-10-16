// The harness ledger note (architecture §3.9, plan A4): at each turn's end the mod hands the
// binary the session's usage as the engine counted it, step by step, through `adage ledger note`.
// The binary keeps one line per session, replaced by session id, so every note carries every step
// of the session so far. Steps Adage took in the model's place are counted with the model that
// would have answered them (`predicted`), so the savings estimate can price what was not asked.
// The note also carries the session's main-loop tool calls (`tool_calls`): the binary's count came
// from a transcript pass only, so with automatic learning off every line read 0 (O0 defect 10).
//
// `keepStep` collects the main loop's steps at `turn.step` (step.ts), `keepCall` its tool calls at
// `tool.call` (call.ts); `onTurnEnd` sends the note
// at `turn.complete`, off the hook's path, through bridge.ts `note`; a refusal changes nothing.
import type { TurnUsage } from 'claude-code'

import { note } from './bridge'
import type { AnswerAt, EventAt, Mod, NextAt } from './mod'

/** One main-loop step as `adage ledger note` reads it (A3-A6-notes.md, "stdin contract"). */
export type StepUsage = {
  index: number
  model: string
  input: number
  output: number
  cache_read: number
  cache_write: number
  predicted: boolean
}

/** The steps noted for one session, in order, and its main-loop tool calls so far. */
export type Noted = { session: string; steps: StepUsage[]; tool_calls: number }
export const NOTED0: Noted = { session: '', steps: [], tool_calls: 0 }

/** The binary refuses a note of more steps; a longer session keeps its first ones. */
export const MAX_STEPS = 20_000

/**
 * One step's usage: the request's own counts and the model that answered it; a predicted step (no
 * request was made) counts nothing, under the model the step asked for. A request that reported
 * no usage (an interrupt, an API error) is no step: undefined.
 */
export function stepOf(asked: string, usage: TurnUsage | null | undefined, predicted: boolean): Omit<StepUsage, 'index'> | undefined {
  if (predicted) return { model: asked, input: 0, output: 0, cache_read: 0, cache_write: 0, predicted: true }
  if (usage === null || usage === undefined) return undefined
  return {
    model: usage.model || asked,
    input: usage.input_tokens,
    output: usage.output_tokens,
    cache_read: usage.cache_read_input_tokens,
    cache_write: usage.cache_creation_input_tokens,
    predicted: false,
  }
}

/**
 * The session's steps and tool calls after a turn ended: the turn's steps appended, numbered on,
 * and its calls added. A turn that ends in another session (a /clear started one) starts both
 * counts again with that turn's own.
 */
export function accrue(noted: Noted, session: string, turn: readonly Omit<StepUsage, 'index'>[], calls = 0): Noted {
  const same = noted.session === session
  const was = same ? noted.steps : []
  const room = Math.max(0, MAX_STEPS - was.length)
  const counted = Number.isSafeInteger(calls) && calls > 0 ? calls : 0
  return {
    session,
    steps: [...was, ...turn.slice(0, room).map((s, i) => ({ index: was.length + i, ...s }))],
    // The binary refuses a count past u32; a session never gets near it.
    tool_calls: Math.min(MAX_CALLS, (same ? noted.tool_calls : 0) + counted),
  }
}

/** The most tool calls a note may carry (the binary reads a u32). */
export const MAX_CALLS = 4_294_967_295

/** The note on `adage ledger note`'s stdin. */
export type Note = {
  session_id: string
  repo: string
  harness: 'claude-code'
  harness_version: string
  started_at: number
  ended_at: number
  steps: StepUsage[]
  /** The session's main-loop tool calls so far: the model's, the predicted ones, those a receipt answered. */
  tool_calls: number
  cost_usd?: number
}

/** The note for the session as it stands; the engine's cost only when it keeps one. */
export function noteOf(n: { noted: Noted; cwd: string; version: string; startedAt: number; endedAt: number; costUsd: number | undefined }): Note {
  return {
    session_id: n.noted.session,
    repo: n.cwd,
    harness: 'claude-code',
    harness_version: n.version,
    started_at: n.startedAt,
    ended_at: n.endedAt,
    steps: n.noted.steps,
    tool_calls: n.noted.tool_calls,
    ...(typeof n.costUsd === 'number' && Number.isFinite(n.costUsd) && n.costUsd >= 0 ? { cost_usd: n.costUsd } : {}),
  }
}

/** Keeps one main-loop step's usage for the note at the turn's end (a step with no usage is none). */
export function keepStep(m: Mod, asked: string, usage: TurnUsage | null | undefined, predicted: boolean): void {
  const step = stepOf(asked, usage, predicted)
  if (step !== undefined) m.rt.turnSteps.push(step)
}

/**
 * Counts one main-loop tool call for the note: every call the engine runs on the main loop, the
 * model's own, the predicted ones and those a receipt answered, as a transcript lists them; not a
 * subagent's, and not one a plugin raised (the mod's own `$.ui.ask`).
 */
export function keepCall(m: Mod, e: { agentId?: string }, origin: { plugin: string }): void {
  if (e.agentId === undefined && origin.plugin === 'engine') m.rt.turnCalls += 1
}

/** `adage ledger note` with the session's steps so far (one line per session, replaced each turn). */
async function noteLedger(m: Mod): Promise<void> {
  const session = await m.session.id()
  const calls = m.rt.turnCalls
  m.rt.turnCalls = 0
  m.rt.noted = accrue(m.rt.noted, session, m.rt.turnSteps.splice(0), calls)
  if (m.rt.cli === undefined || m.rt.noted.steps.length === 0) return
  const usage = await m.session.usage().catch(() => undefined)
  const { version } = await m.session.version()
  const cwd = await m.session.cwd()
  const payload = noteOf({ noted: m.rt.noted, cwd, version, startedAt: usage?.startedAt ?? 0, endedAt: await m.clock.now(), costUsd: usage?.cost?.usd })
  await note(m.run, m.rt.cli, payload, cwd)
}

/**
 * The harness ledger (§3.9, plan A4): at each main-loop turn's end, the session's steps as the
 * engine counted them go to `adage ledger note`, off the hook's path (not awaited: the note never
 * holds the turn, and its failure is nothing).
 */
export async function onTurnEnd(m: Mod, e: EventAt<'turn.complete'>, next: NextAt<'turn.complete'>): AnswerAt<'turn.complete'> {
  const done = await next(e)
  if (m.rt.bin !== undefined && e.agentId === undefined) void noteLedger(m).catch(() => undefined)
  return done
}
