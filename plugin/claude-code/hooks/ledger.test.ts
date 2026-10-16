// The harness ledger note (ledger.ts; review M6; plan A4, T-A4-*'s mod half): a step's usage, the
// session's steps across turns and a /clear, the note as `adage ledger note` reads it, and the note
// sent at each main-loop turn's end through the engine.
import { describe, expect, test } from 'claude-code/testing'

import { PREDICT, STEP, TURN_END as END, drain, start, wire, world } from './fixtures/kit'
import type { World } from './fixtures/kit'
import { MAX_CALLS, MAX_STEPS, NOTED0, accrue, noteOf, stepOf } from './ledger'

const USAGE = { input_tokens: 12, output_tokens: 340, cache_read_input_tokens: 51000, cache_creation_input_tokens: 900, model: 'model-b' }

describe('T-A4-02 · the ledger note', () => {
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
    expect(cleared).toEqual({ session: 's2', steps: [{ index: 0, ...p }], tool_calls: 0 })
    expect(accrue({ session: 's1', tool_calls: 0, steps: Array.from({ length: MAX_STEPS }, (_, i) => ({ index: i, ...a })) }, 's1', [a]).steps).toHaveLength(MAX_STEPS)
  })

  test('O0 defect 10 · tool calls add up across turns; a /clear starts again; nonsense counts nothing; never past u32', () => {
    const one = accrue(NOTED0, 's1', [], 3)
    expect(one.tool_calls).toBe(3)
    expect(accrue(one, 's1', [], 2).tool_calls).toBe(5)
    expect(accrue(one, 's2', [], 1).tool_calls).toBe(1)
    for (const bad of [-1, 1.5, Number.NaN, Number.POSITIVE_INFINITY]) expect(accrue(one, 's1', [], bad).tool_calls).toBe(3)
    expect(accrue({ ...one, tool_calls: MAX_CALLS }, 's1', [], 4).tool_calls).toBe(MAX_CALLS)
    expect(accrue(one, 's1', []).tool_calls).toBe(3)
  })

  test("the note: the session, its directory, the harness and its version, the times, the steps; the engine's cost only when it keeps one", () => {
    const noted = accrue(NOTED0, 's1', [stepOf('m', null, true) ?? { model: 'm', input: 0, output: 0, cache_read: 0, cache_write: 0, predicted: true }])
    const base = { noted, cwd: '/w', version: '2.1.288', startedAt: 1, endedAt: 2 }
    expect(noteOf({ ...base, costUsd: 0.04 })).toEqual({ session_id: 's1', repo: '/w', harness: 'claude-code', harness_version: '2.1.288', started_at: 1, ended_at: 2, steps: noted.steps, tool_calls: 0, cost_usd: 0.04 })
    expect('cost_usd' in noteOf({ ...base, costUsd: undefined })).toBe(false)
    expect('cost_usd' in noteOf({ ...base, costUsd: Number.NaN })).toBe(false)
  })
})

// M6 · the harness ledger (architecture §3.9, plan A4): every main-loop turn's end hands the
// session's steps, as the engine counted them, to `adage ledger note`.
describe('T-A4-02, M6 · adage ledger note at the turn end', () => {
  const notes = (w: World) => w.ran.filter(r => r.argv[1] === 'ledger').map(r => ({ argv: r.argv, cwd: r.init?.cwd, env: r.init?.env, note: JSON.parse(r.init?.stdin ?? 'null') as Record<string, unknown> }))
  const usage = { startedAt: 1_791_008_000_000, cost: { usd: 0.25 } }

  test("the session's steps, a model's and a predicted one, numbered across turns; one line per turn end", async ($, on) => {
    const w = world(on, { hook: { TurnStep: PREDICT }, model: 'read', usage })
    await start($, w, null, false)
    await drain($.turn.step({ ...STEP, index: 0 }))
    await drain($.turn.step(STEP))
    expect(await $.turn.complete(END as never)).toMatchObject({ text: 'Done.' })
    await w.clock.settle()
    const first = notes(w)
    expect(first).toHaveLength(1)
    expect(first[0]?.argv).toEqual(['/data/bin/adage', 'ledger', 'note'])
    expect(first[0]?.cwd).toBe('/w')
    expect(first[0]?.env).toEqual({ CLAUDECODE: '1' })
    expect(first[0]?.note).toEqual({
      session_id: 'sess-1',
      repo: '/w',
      harness: 'claude-code',
      harness_version: '2.1.288',
      started_at: 1_791_008_000_000,
      ended_at: 1_791_008_381_000,
      steps: [
        { index: 0, model: 'claude-x', input: 10, output: 5, cache_read: 0, cache_write: 0, predicted: false },
        { index: 1, model: 'm', input: 0, output: 0, cache_read: 0, cache_write: 0, predicted: true },
      ],
      tool_calls: 0,
      cost_usd: 0.25,
    })
    await drain($.turn.step({ ...STEP, index: 0, turnId: 'turn-2' }))
    await $.turn.complete({ ...END, turnId: 'turn-2' } as never)
    await w.clock.settle()
    expect(notes(w).map(n => (n.note.steps as { index: number; predicted: boolean }[]).map(x => `${x.index}${x.predicted ? 'p' : ''}`))).toEqual([['0', '1p'], ['0', '1p', '2']])
  })

  test('O0 defect 10 · tool_calls: every main-loop call the engine ran, a Bash plan asked, a predicted one included; not a subagent\'s', async ($, on) => {
    const w = world(on, { hook: { TurnStep: PREDICT, PreToolUse: JSON.stringify(wire('plan')) }, model: 'read', usage })
    await start($, w, null, false)
    await drain($.turn.step({ ...STEP, index: 0 }))
    await $.tool.call({ tool: 'Bash', command: 'make test', tool_use_id: 't1' })
    await $.tool.call({ tool: 'Read', file_path: '/w/a', tool_use_id: 't2' } as never)
    await $.tool.call({ tool: 'Bash', command: 'make gen', tool_use_id: 'adage_regen_turn-1_2_0' })
    await $.tool.call({ tool: 'Bash', command: 'ls', tool_use_id: 't3', agentId: 'agent-7' } as never)
    await $.turn.complete(END as never)
    await w.clock.settle()
    await $.tool.call({ tool: 'Grep', pattern: 'x', tool_use_id: 't4' } as never)
    await drain($.turn.step({ ...STEP, index: 0, turnId: 'turn-2' }))
    await $.turn.complete({ ...END, turnId: 'turn-2' } as never)
    await w.clock.settle()
    expect(notes(w).map(n => n.note.tool_calls)).toEqual([3, 4])
  })

  test("a subagent's turn and steps are not the session's; nothing noted before a step; inert notes nothing", async ($, on) => {
    const w = world(on, { model: 'read', usage })
    await start($, w, null, false)
    await $.turn.complete(END as never)
    await drain($.turn.step({ ...STEP, agentId: 'agent-7' }))
    await $.turn.complete({ ...END, agentId: 'agent-7' } as never)
    await w.clock.settle()
    expect(notes(w)).toEqual([])
  })

  test('a refused note (exit 2) changes nothing: the turn ends as beneath (fail open)', async ($, on) => {
    const w = world(on, { ledgerExit: 2, model: 'read', usage })
    await start($, w, null, false)
    await drain($.turn.step({ ...STEP, index: 0 }))
    expect(await $.turn.complete(END as never)).toMatchObject({ text: 'Done.' })
    await w.clock.settle()
    expect(notes(w)).toHaveLength(1)
  })

  test('inert: no step is kept and no note is sent', async ($, on) => {
    const w = world(on, { version: '', model: 'read', usage })
    await start($, w, null, false)
    await drain($.turn.step({ ...STEP, index: 0 }))
    await $.turn.complete(END as never)
    await w.clock.settle()
    expect(notes(w)).toEqual([])
  })
})
