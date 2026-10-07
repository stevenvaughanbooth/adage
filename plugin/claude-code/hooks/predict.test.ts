// A `TurnStep` answer read into the calls it predicts, and those calls as `turn.step` chunks
// (predict.ts; plan C4, C7; T-C4-01, T-C4-03, T-C7-03): one branch per kind, ids with provenance.
// The answers are the wire's (docs/claude-mod/impl/wire.md): its two files read as they are, every
// other kind and every malformed case made from predict.json.
import { describe, expect, test } from 'claude-code/testing'

import { AUTO_MODE_BASH, DEFAULT_MODE_BASH, READ_ALLOWED, wire, wireWith } from './fixtures/kit'
import { callChunks, heldBy, permissionOf, predictedResult, predictionOf, toolUseId } from './predict'

const P = wire('predict').predict as { route: string; track: string }
const make = (kind: string, calls: unknown, extra: Record<string, unknown> = {}) => wireWith('predict', { kind, calls, ...extra })
const bash = (command: string) => ({ tool: 'Bash', input: { command } })

describe('T-C4-01, T-C7-03 · predictionOf reads the wire', () => {
  test('predict.json: an obligation, one Bash call, its route and track', () => {
    const answer = wire('predict')
    const p = answer.predict as Record<string, unknown>
    expect(predictionOf(answer)).toEqual({ kind: 'obligation', route: p.route, track: p.track, calls: p.calls })
    expect(predictionOf(answer)?.calls.map(c => c.tool)).toEqual(['Bash'])
    expect([P.route, P.track].includes('')).toBe(false)
  })

  test('predict-chain.json: a chain, its calls in order', () => {
    const answer = wire('predict-chain')
    const p = answer.predict as Record<string, unknown>
    expect(predictionOf(answer)).toEqual({ kind: 'chain', route: p.route, track: p.track, calls: p.calls })
    expect(predictionOf(answer)?.calls.length).toBeGreaterThan(1)
  })
})

describe('T-C4-01, T-C4-03, T-C7-03 · predictionOf: one branch per kind', () => {
  test('run, obligation and sequence take exactly one call', () => {
    for (const kind of ['run', 'obligation', 'sequence']) {
      expect(predictionOf(make(kind, [bash('make gen')]))).toEqual({
        kind,
        route: P.route,
        track: P.track,
        calls: [bash('make gen')],
      })
      expect(predictionOf(make(kind, [bash('a'), bash('b')]))).toBeUndefined()
      expect(predictionOf(make(kind, []))).toBeUndefined()
    }
  })

  test('skill takes one Skill call naming a skill', () => {
    const call = { tool: 'Skill', input: { skill: 'release-notes' } }
    expect(predictionOf(make('skill', [call]))?.calls).toEqual([call])
    expect(predictionOf(make('skill', [bash('x')]))).toBeUndefined()
    expect(predictionOf(make('skill', [{ tool: 'Skill', input: {} }]))).toBeUndefined()
    expect(predictionOf(make('skill', [call, call]))).toBeUndefined()
  })

  test('chain takes one call or several', () => {
    const reads = [{ tool: 'Read', input: { file_path: '/w/a.ts' } }, { tool: 'Read', input: { file_path: '/w/b.ts' } }]
    expect(predictionOf(make('chain', reads))?.calls).toEqual(reads)
    expect(predictionOf(make('chain', []))).toBeUndefined()
  })

  test('nothing, an unknown kind or a malformed answer predicts nothing', () => {
    for (const answer of [
      undefined,
      {},
      wireWith('predict', null),
      make('teleport', [bash('x')]),
      make('run', [bash('x')], { route: '' }),
      make('run', [bash('x')], { route: 7 }),
      make('run', [{ tool: '', input: {} }]),
      make('run', [{ tool: 'Bash', input: 'make gen' }]),
      make('run', [{ tool: 'Bash' }]),
      make('run', 'x', { route: 'r' }),
      make('toString', [bash('x')], { route: 'r' }),
    ]) {
      expect(predictionOf(answer), JSON.stringify(answer)).toBeUndefined()
    }
  })

  test('a missing track reads as empty', () => {
    expect(predictionOf(make('run', [bash('x')], { track: undefined }))?.track).toBe('')
  })
})

describe('T-C4-01, T-C7-03 · chunks and ids', () => {
  test('ids carry the provenance and only [A-Za-z0-9_-]', () => {
    expect(toolUseId('codegen-after-schema', 'turn-1', 3, 0)).toBe('adage_codegen-after-schema_turn-1_3_0')
    expect(toolUseId('a/b_c', 'x:y z', 2, 1)).toBe('adage_a-b-c_x-y-z_2_1')
  })

  test('one tool and one whole input chunk per call, block indexes from the first given', () => {
    const p = predictionOf(make('chain', [bash('a'), { tool: 'Read', input: { file_path: '/f' } }]))!
    expect(callChunks(p, 't', 2, 1)).toEqual([
      { kind: 'tool', index: 1, id: `adage_${P.route}_t_2_0`, name: 'Bash' },
      { kind: 'input', index: 1, json: '{"command":"a"}' },
      { kind: 'tool', index: 2, id: `adage_${P.route}_t_2_1`, name: 'Read' },
      { kind: 'input', index: 2, json: '{"file_path":"/f"}' },
    ])
    expect(predictedResult(p, 't', 2)).toEqual({
      turnId: 't',
      index: 2,
      answer: '',
      toolUses: [
        { name: 'Bash', input: { command: 'a' } },
        { name: 'Read', input: { file_path: '/f' } },
      ],
      // No request was made: no reason, as TurnStepResult defines it (and the thread, PREDICTED_STOP).
      stopReason: null,
      usage: null,
    })
  })
})

describe('T-C4-07 · the engine\'s verdict on each predicted call', () => {
  test('permissionOf: allow, ask and deny as the engine says them, with its sentence; anything else is no verdict', () => {
    expect(permissionOf(DEFAULT_MODE_BASH)).toEqual({ decision: 'allow', reason: '' })
    expect(permissionOf(AUTO_MODE_BASH)).toEqual({ decision: 'ask', reason: 'This command requires approval' })
    expect(permissionOf({ decision: 'deny', reason: 'no', rule: 'Bash(make:*)' })).toEqual({ decision: 'deny', reason: 'no' })
    for (const v of [undefined, null, 'allow', {}, { decision: 'yes' }]) expect(permissionOf(v)).toEqual({ decision: 'none', reason: '' })
    expect(permissionOf({ decision: 'ask', reason: 7 })).toEqual({ decision: 'ask', reason: '' })
  })

  test('heldBy: undefined only when every call is allowed; else the first call, in order, that is not', async () => {
    const calls = [bash('make gen'), { tool: 'Read', input: { file_path: '/w/a' } }]
    expect(await heldBy(calls, async tool => (tool === 'Read' ? READ_ALLOWED : DEFAULT_MODE_BASH))).toBeUndefined()
    expect(await heldBy(calls, async tool => (tool === 'Read' ? { decision: 'deny', reason: 'r' } : AUTO_MODE_BASH))).toEqual({ tool: 'Bash', decision: 'ask', reason: AUTO_MODE_BASH.reason })
    expect(await heldBy(calls, async tool => (tool === 'Read' ? { decision: 'deny', reason: 'r' } : DEFAULT_MODE_BASH))).toEqual({ tool: 'Read', decision: 'deny', reason: 'r' })
    expect(await heldBy(calls, () => Promise.reject(new Error('x')))).toEqual({ tool: 'Bash', decision: 'none', reason: '' })
    expect(await heldBy([], async () => AUTO_MODE_BASH)).toBeUndefined()
  })
})

