// Pure: a `TurnStep` answer to the calls it predicts, and those calls to `turn.step` chunks. The
// binary decides what to predict (§6 invariant 1); this file only checks the answer's shape, so a
// malformed answer is "nothing" and the request goes out.
import type { TurnStepChunk, TurnStepResult } from 'claude-code'

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

/** The step's result for a response made wholly of the predicted calls (no request was made). */
export function predictedResult(p: Prediction, turnId: string, index: number): TurnStepResult {
  return {
    turnId,
    index,
    answer: '',
    toolUses: p.calls.map(c => ({ name: c.tool, input: c.input })),
    stopReason: 'tool_use',
    usage: null,
  }
}
