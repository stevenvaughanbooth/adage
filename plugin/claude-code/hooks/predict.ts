// Pure: a `TurnStep` answer to the calls it predicts, and those calls to `turn.step` chunks. The
// binary decides what to predict (§6 invariant 1); this file only checks the answer's shape, so a
// malformed answer is "nothing" and the request goes out.
import type { TurnStepChunk, TurnStepResult, TurnStepStopChunk } from 'claude-code'

import { isRecord } from './shape'

export type PredictedCall = { tool: string; input: Record<string, unknown> }

export type Prediction = { kind: PredictKind; route: string; track: string; calls: PredictedCall[] }

/**
 * Each kind the binary may answer, and the calls it takes. A new kind is one line here (and its
 * name in PredictKind); an unknown kind predicts nothing.
 */
const KINDS = {
  run: one,
  obligation: one,
  sequence: one,
  skill: skill,
  chain: several,
} satisfies Record<string, (calls: PredictedCall[]) => PredictedCall[] | undefined>

export type PredictKind = keyof typeof KINDS

function one(calls: PredictedCall[]) {
  return calls.length === 1 ? calls : undefined
}

function skill(calls: PredictedCall[]) {
  const [call] = calls
  return calls.length === 1 && call?.tool === 'Skill' && typeof call.input.skill === 'string' && call.input.skill !== ''
    ? calls
    : undefined
}

function several(calls: PredictedCall[]) {
  return calls.length >= 1 ? calls : undefined
}

function callOf(v: unknown): PredictedCall | undefined {
  if (!isRecord(v) || typeof v.tool !== 'string' || v.tool === '' || !isRecord(v.input)) return undefined
  return { tool: v.tool, input: v.input }
}

/** The prediction in a `TurnStep` answer, or undefined for nothing (or anything malformed). */
export function predictionOf(answer: unknown): Prediction | undefined {
  if (!isRecord(answer) || !isRecord(answer.predict)) return undefined
  const p = answer.predict
  if (typeof p.kind !== 'string' || !Object.hasOwn(KINDS, p.kind)) return undefined
  if (typeof p.route !== 'string' || p.route === '' || !Array.isArray(p.calls)) return undefined
  const calls = p.calls.map(callOf)
  if (calls.some(c => c === undefined)) return undefined
  const kind = p.kind as PredictKind
  const taken = KINDS[kind](calls as PredictedCall[])
  if (taken === undefined) return undefined
  return { kind, route: p.route, track: typeof p.track === 'string' ? p.track : '', calls: taken }
}

const idPart = (s: string) => s.replace(/[^A-Za-z0-9-]/g, '-')

/**
 * The tool_use id of call `k` of a predicted step: `adage_<route>_<turnId>_<index>_<k>`, provenance
 * in the id (§6 invariant 7). The API takes ids of `[A-Za-z0-9_-]`; any other character of the
 * route or the turn id, and `_` inside them, is written `-`.
 */
export function toolUseId(route: string, turnId: string, index: number, k: number): string {
  return `adage_${idPart(route)}_${idPart(turnId)}_${index}_${k}`
}

/** The route a predicted call's id names (`toolUseId`'s shape exactly), or undefined for any other id. */
export function predictedRoute(toolUseId: string): string | undefined {
  return /^adage_([A-Za-z0-9-]+)_[A-Za-z0-9-]+_\d+_\d+$/.exec(toolUseId)?.[1]
}

/**
 * The one line the model reads after the result of a call Adage made in its place (§3.1–3.2:
 * Adage's act is never counted as the agent's, and the model is told so plainly). Without it the
 * model reads a call in its own turn it does not remember making: live, after a predicted
 * `make gen`, it apologised for having run it. Undefined for a call the model made.
 */
export function madeForAgentLine(toolUseId: string): string | undefined {
  const route = predictedRoute(toolUseId)
  if (route === undefined) return undefined
  return `Adage made this call in your place (learned reflex ${route}); the result above is its output. You did not make it, and it does not need repeating.`
}

/**
 * The chunks of a predicted response: per call a `tool` chunk then its whole `input`, at content
 * block `firstBlock + k`. The caller yields its own stop.
 */
export function callChunks(p: Prediction, turnId: string, index: number, firstBlock: number): TurnStepChunk[] {
  return p.calls.flatMap((call, k): TurnStepChunk[] => [
    { kind: 'tool', index: firstBlock + k, id: toolUseId(p.route, turnId, index, k), name: call.tool },
    { kind: 'input', index: firstBlock + k, json: JSON.stringify(call.input) },
  ])
}

/**
 * The stop of a response made wholly of predicted calls: no request was made, so no model stopped
 * and nothing was spent, and the stop says so as `TurnStepResult` defines it (a null reason "when
 * no request was made"). It also keeps the conversation's server-side thread whole. The engine
 * records a made-up response under an id it mints (`msg_hook_…`) and continues the thread from the
 * last assistant message whose id starts `msg_` (2.1.292), so a response that stated a reason was
 * taken as the thread's tip and the next request named an id the server never issued: a 404
 * (`thread_not_found`) and a replayed dispatch after every predicted step (live run 2, 12 of 12).
 * A message with no stop reason is incomplete to the engine, which then sends the next request
 * whole, as the replay did, without the failed dispatch (docs/claude-mod/impl/live-fixes-2-notes.md).
 */
export const PREDICTED_STOP: TurnStepChunk = { kind: 'stop', stopReason: null, usage: null }

/**
 * The stop of a real response the finish line appended predicted calls to: the model's own stop
 * chunk with no reason, its usage and the engine's handle on it kept. The server recorded that
 * response under its real id as the model sent it, text and `end_turn`; the transcript holds it
 * with the calls. A stated reason made it the thread's tip, so the next request continued from the
 * server's copy with only the calls' results after it: results of calls the server never saw (the
 * thread diverges, or the request is refused and replayed). With no reason the engine sends that
 * request whole (`anchor_incomplete`), the calls included, and the server's copy is never
 * continued. Nothing else marks it: the message id is the engine's to set, and a hook-made
 * response is anchored by the same rule as this one.
 */
export function appendedStop(stop: TurnStepStopChunk): TurnStepStopChunk {
  return { ...stop, stopReason: null }
}

/** The step's result for a response made wholly of the predicted calls (no request was made). */
export function predictedResult(p: Prediction, turnId: string, index: number): TurnStepResult {
  return {
    turnId,
    index,
    answer: '',
    toolUses: p.calls.map(c => ({ name: c.tool, input: c.input })),
    stopReason: null,
    usage: null,
  }
}
