// The harness ledger note's pieces (review M6; plan A4): a step's usage, the session's steps across
// turns and a /clear, and the note as `adage ledger note` reads it.
import { describe, expect, test } from 'claude-code/testing'

import { MAX_STEPS, NOTED0, accrue, noteOf, stepOf } from './ledger'

const USAGE = { input_tokens: 12, output_tokens: 340, cache_read_input_tokens: 51000, cache_creation_input_tokens: 900, model: 'model-b' }

describe('the ledger note', () => {
  test("a model's step counts its own usage under the model that answered; a predicted one counts nothing; no usage is no step", () => {
    expect(stepOf('model-a', USAGE, false)).toEqual({ model: 'model-b', input: 12, output: 340, cache_read: 51000, cache_write: 900, predicted: false })
    expect(stepOf('model-a', null, true)).toEqual({ model: 'model-a', input: 0, output: 0, cache_read: 0, cache_write: 0, predicted: true })
    expect(stepOf('model-a', null, false)).toBeUndefined()
    expect(stepOf('model-a', { ...USAGE, model: '' }, false)?.model).toBe('model-a')
  })

  test('steps are numbered on across turns; a turn in a new session (a /clear) starts again with its own', () => {
    const a = stepOf('m', USAGE, false)
    const p = stepOf('m', null, true)
    if (a === undefined || p === undefined) throw new Error('no step')
    const one = accrue(NOTED0, 's1', [a, p])
    expect(one.steps.map(s => s.index)).toEqual([0, 1])
    const two = accrue(one, 's1', [a])
    expect(two.steps.map(s => `${s.index}${s.predicted ? 'p' : ''}`)).toEqual(['0', '1p', '2'])
    const cleared = accrue(two, 's2', [p])
    expect(cleared).toEqual({ session: 's2', steps: [{ index: 0, ...p }] })
    expect(accrue({ session: 's1', steps: Array.from({ length: MAX_STEPS }, (_, i) => ({ index: i, ...a })) }, 's1', [a]).steps).toHaveLength(MAX_STEPS)
  })

  test("the note: the session, its directory, the harness and its version, the times, the steps; the engine's cost only when it keeps one", () => {
    const noted = accrue(NOTED0, 's1', [stepOf('m', null, true) ?? { model: 'm', input: 0, output: 0, cache_read: 0, cache_write: 0, predicted: true }])
    const base = { noted, cwd: '/w', version: '2.1.288', startedAt: 1, endedAt: 2 }
    expect(noteOf({ ...base, costUsd: 0.04 })).toEqual({ session_id: 's1', repo: '/w', harness: 'claude-code', harness_version: '2.1.288', started_at: 1, ended_at: 2, steps: noted.steps, cost_usd: 0.04 })
    expect('cost_usd' in noteOf({ ...base, costUsd: undefined })).toBe(false)
    expect('cost_usd' in noteOf({ ...base, costUsd: Number.NaN })).toBe(false)
  })
})
