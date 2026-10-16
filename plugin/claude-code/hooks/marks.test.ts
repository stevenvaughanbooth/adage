// What is kept for drawing beside the transcript's rows (marks.ts; ux-design §4.2–§4.4; T-B2-04):
// one marker per call and kind, the fired reflexes, the predicted steps and the tool batches, each
// capped; and the batch a feed record belongs to by when the hooks ran.
import { describe, expect, test } from 'claude-code/testing'

import { T0, fake } from './fixtures/kit'
import { MAX_BATCHES, MAX_MARKS, addMarks, batchAt, keepBatch, keepFired, mark, record } from './marks'

describe('T-B2-04 · the markers and what they are drawn from', () => {
  test('a second marker of the same kind for the same call replaces the first; another kind is kept beside it', async () => {
    const f = fake()
    await mark(f.m, { id: 't1', kind: 'receipt', receipt: 'aaaa' })
    await mark(f.m, { id: 't1', kind: 'rewrite', interpreter: 'py', what: 'test' })
    await mark(f.m, { id: 't1', kind: 'receipt', receipt: 'bbbb' })
    expect(f.state.marks).toEqual([
      { id: 't1', kind: 'rewrite', interpreter: 'py', what: 'test' },
      { id: 't1', kind: 'receipt', receipt: 'bbbb' },
    ])
  })

  test('each list keeps its latest: marks, fired and predicted at 500, batches at 50', async () => {
    const f = fake()
    await addMarks(f.m, Array.from({ length: MAX_MARKS + 3 }, (_, k) => ({ id: `t${k}`, kind: 'avoided' as const, route: 'r' })))
    expect(f.state.marks).toHaveLength(MAX_MARKS)
    expect(f.state.marks[0]?.id).toBe('t3')
    await keepFired(f.m, Array.from({ length: MAX_MARKS + 1 }, (_, k) => ({ event: 'Stop', route: `r${k}`, track: '', kind: '', strength: '' })))
    expect(f.state.fired).toHaveLength(MAX_MARKS)
    await record(f.m, { turnId: 't', index: 1, route: 'r', kind: 'run', ids: ['x'], isEnding: false })
    expect(f.state.predicted).toHaveLength(1)
    for (let k = 0; k < MAX_BATCHES + 2; k++) await keepBatch(f.m, { ids: [`b${k}`], from: k, to: k })
    expect(f.state.batches).toHaveLength(MAX_BATCHES)
    expect(f.state.batches[0]?.ids).toEqual(['b2'])
  })

  test('T-D5-01 · batchAt: the latest batch whose hooks ran around the record, give or take the clocks', () => {
    const a = { ids: ['a'], from: T0, to: T0 + 400 }
    const b = { ids: ['b'], from: T0 + 300, to: T0 + 900 }
    expect(batchAt([a, b], T0 + 350)?.ids).toEqual(['b'])
    expect(batchAt([a], T0 - 50)?.ids).toEqual(['a'])
    expect(batchAt([a], T0 + 2400)?.ids).toEqual(['a'])
    expect(batchAt([a], T0 + 2401)).toBeUndefined()
    expect(batchAt([], T0)).toBeUndefined()
  })
})
