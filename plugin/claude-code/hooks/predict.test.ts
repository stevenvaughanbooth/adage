import { describe, expect, test } from 'claude-code/testing'

import { callChunks, predictedResult, predictionOf, toolUseId } from './predict'

const make = (kind: string, calls: unknown[], extra: Record<string, unknown> = {}) => ({
  predict: { kind, route: 'codegen-after-schema', track: 'acme/billing@1', calls, ...extra },
})
const bash = (command: string) => ({ tool: 'Bash', input: { command } })

describe('predictionOf: one branch per kind', () => {
  test('run, obligation and sequence take exactly one call', () => {
    for (const kind of ['run', 'obligation', 'sequence']) {
      expect(predictionOf(make(kind, [bash('make gen')]))).toEqual({
        kind,
        route: 'codegen-after-schema',
        track: 'acme/billing@1',
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
      { predict: null },
      make('teleport', [bash('x')]),
      make('run', [bash('x')], { route: '' }),
      make('run', [bash('x')], { route: 7 }),
      make('run', [{ tool: '', input: {} }]),
      make('run', [{ tool: 'Bash', input: 'make gen' }]),
      make('run', [{ tool: 'Bash' }]),
      { predict: { kind: 'run', route: 'r', calls: 'x' } },
      { predict: { kind: 'toString', route: 'r', calls: [bash('x')] } },
    ]) {
      expect(predictionOf(answer), JSON.stringify(answer)).toBeUndefined()
    }
  })

  test('a missing track reads as empty', () => {
    expect(predictionOf({ predict: { kind: 'run', route: 'r', calls: [bash('x')] } })?.track).toBe('')
  })
})

describe('chunks and ids', () => {
  test('ids carry the provenance and only [A-Za-z0-9_-]', () => {
    expect(toolUseId('codegen-after-schema', 'turn-1', 3, 0)).toBe('adage_codegen-after-schema_turn-1_3_0')
    expect(toolUseId('a/b_c', 'x:y z', 2, 1)).toBe('adage_a-b-c_x-y-z_2_1')
  })

  test('one tool and one whole input chunk per call, block indexes from the first given', () => {
    const p = predictionOf(make('chain', [bash('a'), { tool: 'Read', input: { file_path: '/f' } }]))!
    expect(callChunks(p, 't', 2, 1)).toEqual([
      { kind: 'tool', index: 1, id: 'adage_codegen-after-schema_t_2_0', name: 'Bash' },
      { kind: 'input', index: 1, json: '{"command":"a"}' },
      { kind: 'tool', index: 2, id: 'adage_codegen-after-schema_t_2_1', name: 'Read' },
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
      stopReason: 'tool_use',
      usage: null,
    })
  })
})
