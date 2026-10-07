// The turn.step hook (step.ts; test-plan §6 C4, C5, C7, C8): `turnStep` with a stubbed ask and a
// stubbed stream beneath (it takes both as values): exactly the right chunks per kind, no request
// when predicted, forwarding otherwise, the finish line at an end_turn stop; then `onStep` through
// the engine, the binary answering beneath.
import type { TurnStepChunk, TurnStepInput, TurnStepResult } from 'claude-code'
import { describe, expect, test } from 'claude-code/testing'

import type { Asked } from './bridge'
import { PREDICT, USAGE, drain, engineThread, forwardedResponse, madeUpResponse, start, unseenResults, wire, wireLine, wireWith, world } from './fixtures/kit'
import type { ThreadMessage } from './fixtures/kit'
import type { Predicted } from './plugin'
import { PREDICTED_STOP } from './predict'
import { isRoutine, turnStep } from './step'
import type { StepDeps, StepNext, TurnStepPayload } from './step'

/** The route of the wire's prediction (predict.json), which every predicted id here carries. */
const ROUTE = String((wire('predict').predict as Record<string, unknown>).route)

const STEP: TurnStepInput = { turnId: 'turn-1', index: 2, model: 'claude-x', messageCount: 5 }

/** The model beneath: streams `chunks`, then returns `result`; counts the requests made. */
function model(chunks: TurnStepChunk[], result: TurnStepResult) {
  const requests: TurnStepInput[] = []
  const next: StepNext = e => {
    requests.push(e)
    let settle: (r: TurnStepResult) => void = () => undefined
    const settled = new Promise<TurnStepResult>(r => (settle = r))
    async function* stream(): AsyncGenerator<TurnStepChunk, TurnStepResult> {
      for (const c of chunks) yield c
      settle(result)
      return result
    }
    return Object.assign(stream(), { result: settled })
  }
  return { next, requests }
}

function stub(answer: (p: TurnStepPayload) => Asked | Error, over: Partial<StepDeps> = {}) {
  const asked: TurnStepPayload[] = []
  const recorded: Predicted[] = []
  const deps: StepDeps = {
    isActive: true,
    isObserving: false,
    hasSkill: async () => true,
    ask: async p => {
      asked.push(p)
      const a = answer(p)
      if (a instanceof Error) throw a
      return a
    },
    context: async () => ({ sessionId: 'sess-1', cwd: '/w' }),
    record: async p => {
      recorded.push(p)
    },
    ...over,
  }
  return { deps, asked, recorded }
}

const predict = (kind: string, calls: unknown[]): Asked => ({ ok: true, answer: wireWith('predict', { kind, calls }) })
const MAKE_GEN = { tool: 'Bash', input: { command: 'make gen' } }
const ID = (k: number) => `adage_${ROUTE}_turn-1_2_${k}`

describe('T-C4-01, T-C7-03 · a prediction before the request', () => {
  const cases: [string, unknown[]][] = [
    ['run', [MAKE_GEN]],
    ['obligation', [MAKE_GEN]],
    ['sequence', [MAKE_GEN]],
    ['skill', [{ tool: 'Skill', input: { skill: 'release-notes' } }]],
    [
      'chain',
      [
        { tool: 'Read', input: { file_path: '/w/a.ts' } },
        { tool: 'Read', input: { file_path: '/w/b.ts' } },
      ],
    ],
  ]
  for (const [kind, calls] of cases) {
    test(`${kind}: yields the calls and a stop with no reason, returns the result, sends no request`, async () => {
      const { next, requests } = model([], { turnId: 'turn-1', index: 2, answer: 'unused', toolUses: [], stopReason: 'end_turn', usage: USAGE })
      const { deps, asked, recorded } = stub(() => predict(kind, calls))
      const { chunks, result } = await drain(turnStep(STEP, next, deps))
      const typed = calls as { tool: string; input: unknown }[]
      expect(chunks).toEqual([
        ...typed.flatMap((c, k) => [
          { kind: 'tool', index: k, id: ID(k), name: c.tool },
          { kind: 'input', index: k, json: JSON.stringify(c.input) },
        ]),
        { kind: 'stop', stopReason: null, usage: null },
      ])
      expect(result).toEqual({
        turnId: 'turn-1',
        index: 2,
        answer: '',
        toolUses: typed.map(c => ({ name: c.tool, input: c.input })),
        stopReason: null,
        usage: null,
      })
      expect(requests).toHaveLength(0)
      expect(asked).toEqual([
        { session_id: 'sess-1', cwd: '/w', hook_event_name: 'TurnStep', turn_id: 'turn-1', index: 2, model: 'claude-x', ending: false, via: 'mod' },
      ])
      expect(recorded).toEqual([
        { turnId: 'turn-1', index: 2, route: ROUTE, kind, ids: typed.map((_, k) => ID(k)), isEnding: false },
      ])
    })
  }
})

describe('T-C4-01 · a predicted step keeps the server thread whole (live run 2: a 404 after 12 of 12)', () => {
  // The conversation up to the step: the prompt, the model's own lookup (a real response, made by
  // a request), its result; then what the mod yields in the model's place, and the read's result.
  const before: ThreadMessage[] = [
    { role: 'user' },
    { role: 'assistant', id: 'msg_01lookup', requestId: 'req_01', model: 'claude-x', stopReason: 'tool_use' },
    { role: 'user' },
  ]
  const READ = [{ tool: 'Read', input: { file_path: '/w/acme/billing.py' } }]

  test('the request after it starts the thread afresh instead of naming an id the server never issued', async () => {
    const { next } = model([], { turnId: 'turn-1', index: 2, answer: '', toolUses: [], stopReason: 'end_turn', usage: USAGE })
    const { chunks } = await drain(turnStep(STEP, next, stub(() => predict('chain', READ)).deps))
    expect(chunks.at(-1)).toEqual(PREDICTED_STOP)
    const after: ThreadMessage[] = [...before, madeUpResponse(chunks, STEP.model), { role: 'user' }]
    expect(engineThread(after)).toEqual({ type: 'create', reason: 'anchor_incomplete' })
  })

  test('a stop that states tool_use (the mod before this fix) continued from the made-up id: the 404', () => {
    const chunks: TurnStepChunk[] = [{ kind: 'stop', stopReason: 'tool_use', usage: null }]
    const after: ThreadMessage[] = [...before, madeUpResponse(chunks, STEP.model, 'u1'), { role: 'user' }]
    expect(engineThread(after)).toEqual({ type: 'continue', previousMessageId: 'msg_hook_u1' })
  })

  test('the next real response is a sound anchor again: the request after it continues', () => {
    const after: ThreadMessage[] = [
      ...before,
      madeUpResponse([PREDICTED_STOP], STEP.model),
      { role: 'user' },
      { role: 'assistant', id: 'msg_01edit', requestId: 'req_02', model: 'claude-x', stopReason: 'tool_use' },
      { role: 'user' },
    ]
    expect(engineThread(after)).toEqual({ type: 'continue', previousMessageId: 'msg_01edit' })
  })
})

describe('T-C4-02 · forwarding', () => {
  const TOOL_STEP = () =>
    model(
      [
        { kind: 'text', index: 0, text: 'Reading.' },
        { kind: 'tool', index: 1, id: 'toolu_1', name: 'Read' },
        { kind: 'input', index: 1, json: '{"file_path":"/w/x"}' },
        { kind: 'stop', stopReason: 'tool_use', usage: USAGE },
      ],
      { turnId: 'turn-1', index: 2, answer: 'Reading.', toolUses: [{ name: 'Read', input: { file_path: '/w/x' } }], stopReason: 'tool_use', usage: USAGE },
    )

  test('nothing predicted: the request goes out and its stream passes unchanged', async () => {
    const m = TOOL_STEP()
    const { deps, asked } = stub(() => ({ ok: true, answer: undefined }))
    const { chunks, result } = await drain(turnStep(STEP, m.next, deps))
    expect(m.requests).toEqual([STEP])
    expect(chunks.map(c => c.kind)).toEqual(['text', 'tool', 'input', 'stop'])
    expect(result).toMatchObject({ stopReason: 'tool_use', answer: 'Reading.' })
    expect(asked).toHaveLength(1) // a tool_use stop is not the turn's end: no ending question
  })

  test('a failed or throwing ask, or a malformed answer, forwards', async () => {
    for (const answer of [(): Asked => ({ ok: false }), () => new Error('boom'), (): Asked => ({ ok: true, answer: wireWith('predict', { kind: 'nope' }) })]) {
      const m = TOOL_STEP()
      const { deps } = stub(answer)
      const { chunks } = await drain(turnStep(STEP, m.next, deps))
      expect(m.requests).toHaveLength(1)
      expect(chunks).toHaveLength(4)
    }
  })

  test('inert, a subagent, step 0, or no session context: forwarded without asking', async () => {
    const runs: [TurnStepInput, Partial<StepDeps>][] = [
      [STEP, { isActive: false }],
      [{ ...STEP, agentId: 'agent-7' }, {}],
      [{ ...STEP, index: 0 }, {}],
      [STEP, { context: () => Promise.reject(new Error('no session')) }],
    ]
    for (const [e, over] of runs) {
      const m = TOOL_STEP()
      const { deps, asked } = stub(() => predict('run', [MAKE_GEN]), over)
      const { chunks, result } = await drain(turnStep(e, m.next, deps))
      expect(asked).toHaveLength(0)
      expect(m.requests).toEqual([e])
      expect(chunks).toHaveLength(4)
      expect(result).toMatchObject({ stopReason: 'tool_use' })
    }
  })
})

describe('T-C8-02 · the finish line (ending)', () => {
  test("an end_turn stop asks with ending: true; a prediction keeps the model's text, appends the call, and ends with no stop reason", async () => {
    const m = model(
      [
        { kind: 'text', index: 0, text: 'Done: ' },
        { kind: 'text', index: 0, text: 'schema edited.' },
        { kind: 'stop', stopReason: 'end_turn', usage: USAGE },
      ],
      { turnId: 'turn-1', index: 2, answer: 'Done: schema edited.', toolUses: [], stopReason: 'end_turn', usage: USAGE },
    )
    const { deps, asked, recorded } = stub(p => (p.ending ? predict('obligation', [MAKE_GEN]) : { ok: true, answer: undefined }))
    const { chunks, result } = await drain(turnStep(STEP, m.next, deps))
    expect(m.requests).toHaveLength(1)
    expect(asked.map(p => p.ending)).toEqual([false, true])
    expect(chunks).toEqual([
      { kind: 'text', index: 0, text: 'Done: ' },
      { kind: 'text', index: 0, text: 'schema edited.' },
      { kind: 'tool', index: 1, id: ID(0), name: 'Bash' },
      { kind: 'input', index: 1, json: '{"command":"make gen"}' },
      { kind: 'stop', stopReason: null, usage: USAGE },
    ])
    expect(result).toEqual({
      turnId: 'turn-1',
      index: 2,
      answer: 'Done: schema edited.',
      toolUses: [{ name: 'Bash', input: { command: 'make gen' } }],
      stopReason: null,
      usage: USAGE,
    })
    expect(recorded).toEqual([
      { turnId: 'turn-1', index: 2, route: ROUTE, kind: 'obligation', ids: [ID(0)], isEnding: true },
    ])
  })

  test('an end_turn stop with nothing to predict is forwarded as it came', async () => {
    const m = model(
      [
        { kind: 'text', index: 0, text: 'Done.' },
        { kind: 'stop', stopReason: 'end_turn', usage: USAGE },
      ],
      { turnId: 'turn-1', index: 2, answer: 'Done.', toolUses: [], stopReason: 'end_turn', usage: USAGE },
    )
    const { deps, asked } = stub(() => ({ ok: true, answer: undefined }))
    const { chunks, result } = await drain(turnStep(STEP, m.next, deps))
    expect(asked.map(p => p.ending)).toEqual([false, true])
    expect(chunks).toEqual([
      { kind: 'text', index: 0, text: 'Done.' },
      { kind: 'stop', stopReason: 'end_turn', usage: USAGE },
    ])
    expect(result).toMatchObject({ stopReason: 'end_turn', toolUses: [] })
  })
})

describe('T-C8-02 · the finish line keeps the server thread whole', () => {
  // The conversation up to the last step: the prompt, the model's Edit (a real response) and its
  // result. The model then answers with text and end_turn, which the server records as msg_01done
  // with no call; the mod appends make gen to it, and the call's result follows.
  const before: ThreadMessage[] = [
    { role: 'user' },
    { role: 'assistant', id: 'msg_01edit', requestId: 'req_01', model: 'claude-x', stopReason: 'tool_use', calls: ['toolu_edit'] },
    { role: 'user', results: ['toolu_edit'] },
  ]
  const SENT = { id: 'msg_01done', requestId: 'req_02', model: 'claude-x' }
  const HELD = new Map([['msg_01done', []]])
  const ENDS = () =>
    model(
      [
        { kind: 'text', index: 0, text: 'Done: schema edited.' },
        { kind: 'stop', stopReason: 'end_turn', usage: USAGE },
      ],
      { turnId: 'turn-1', index: 2, answer: 'Done: schema edited.', toolUses: [], stopReason: 'end_turn', usage: USAGE },
    )
  const after = (chunks: TurnStepChunk[]): ThreadMessage[] => [...before, forwardedResponse(chunks, SENT), { role: 'user', results: [ID(0)] }]

  test('the request after it starts the thread afresh and carries the call with its result', async () => {
    const { chunks } = await drain(turnStep(STEP, ENDS().next, stub(p => (p.ending ? predict('obligation', [MAKE_GEN]) : { ok: true, answer: undefined })).deps))
    expect(forwardedResponse(chunks, SENT)).toEqual({ role: 'assistant', ...SENT, stopReason: null, calls: [ID(0)] })
    expect(engineThread(after(chunks))).toEqual({ type: 'create', reason: 'anchor_incomplete' })
    expect(unseenResults(after(chunks), HELD)).toEqual([])
  })

  test("a stop that states tool_use (the mod before this fix) continued from the server's copy: a result of a call it never saw", () => {
    const chunks: TurnStepChunk[] = [
      { kind: 'text', index: 0, text: 'Done: schema edited.' },
      { kind: 'tool', index: 1, id: ID(0), name: 'Bash' },
      { kind: 'input', index: 1, json: '{"command":"make gen"}' },
      { kind: 'stop', stopReason: 'tool_use', usage: USAGE },
    ]
    expect(engineThread(after(chunks))).toEqual({ type: 'continue', previousMessageId: 'msg_01done' })
    expect(unseenResults(after(chunks), HELD)).toEqual([ID(0)])
  })

  test('the next real response is a sound anchor again: the request after it continues', async () => {
    const { chunks } = await drain(turnStep(STEP, ENDS().next, stub(p => (p.ending ? predict('obligation', [MAKE_GEN]) : { ok: true, answer: undefined })).deps))
    const later: ThreadMessage[] = [
      ...after(chunks),
      { role: 'assistant', id: 'msg_01closing', requestId: 'req_03', model: 'claude-x', stopReason: 'end_turn' },
      { role: 'user' },
    ]
    expect(engineThread(later)).toEqual({ type: 'continue', previousMessageId: 'msg_01closing' })
    expect(unseenResults(later, HELD)).toEqual([])
  })
})

describe('T-C5-04 · routing a step (C5)', () => {
  const ROUTINE: Asked = { ok: true, answer: wire('routine') }

  test('isRoutine reads routine.json; only true itself marks a step routine', () => {
    expect(isRoutine(wire('routine'))).toBe(true)
    expect([isRoutine(wireWith('routine', 'yes')), isRoutine(wireWith('routine', undefined)), isRoutine(undefined), isRoutine(wire('predict'))]).toEqual([false, false, false, false])
  })

  const ENDS = () => model([{ kind: 'text', index: 0, text: 'Done.' }, { kind: 'stop', stopReason: 'end_turn', usage: USAGE }], { turnId: 'turn-1', index: 2, answer: 'Done.', toolUses: [], stopReason: 'end_turn', usage: USAGE })

  test('routine, with a model set: the request goes out on it at low effort; the binary is told only that one is set', async () => {
    const m = ENDS()
    const { deps, asked } = stub(p => (p.ending ? { ok: true, answer: undefined } : ROUTINE), { routineModel: 'haiku' })
    const { chunks } = await drain(turnStep(STEP, m.next, deps))
    expect(m.requests).toEqual([{ ...STEP, model: 'haiku', effort: 'low' }])
    expect(chunks.map(c => c.kind)).toEqual(['text', 'stop'])
    expect(asked.map(p => p.routine_model)).toEqual([true, true])
    expect(JSON.stringify(asked)).not.toContain('haiku')
  })

  test('no model set: a routine answer changes nothing, and the payload carries no routine_model', async () => {
    const m = ENDS()
    const { deps, asked } = stub(() => ROUTINE, {})
    await drain(turnStep(STEP, m.next, deps))
    expect(m.requests).toEqual([STEP])
    expect(asked[0]?.routine_model).toBeUndefined()
  })

  test('a step the binary does not mark routine goes to the session\'s model, whatever is set', async () => {
    const m = ENDS()
    const { deps } = stub(() => ({ ok: true, answer: undefined }), { routineModel: 'haiku' })
    await drain(turnStep(STEP, m.next, deps))
    expect(m.requests).toEqual([STEP])
  })

  test('observing: never routed, though the binary is told a model is set', async () => {
    const m = ENDS()
    const { deps, asked } = stub(() => ROUTINE, { routineModel: 'haiku', isObserving: true })
    await drain(turnStep(STEP, m.next, deps))
    expect(m.requests).toEqual([STEP])
    expect(asked[0]).toMatchObject({ routine_model: true, observe: true })
  })

  test('predicted or routed, never both: a prediction beside routine is the response, and no request goes out', async () => {
    const m = ENDS()
    const { deps } = stub(() => ({ ok: true, answer: { ...wireWith('predict', { kind: 'run', calls: [MAKE_GEN] }), ...wire('routine') } }), { routineModel: 'haiku' })
    const { chunks } = await drain(turnStep(STEP, m.next, deps))
    expect(m.requests).toHaveLength(0)
    expect(chunks.map(c => c.kind)).toEqual(['tool', 'input', 'stop'])
  })

  test("routine on the ending ask changes nothing: the request already went out on the session's model", async () => {
    const m = ENDS()
    const { deps } = stub(p => (p.ending ? ROUTINE : { ok: true, answer: undefined }), { routineModel: 'haiku' })
    const { chunks } = await drain(turnStep(STEP, m.next, deps))
    expect(m.requests).toEqual([STEP])
    expect(chunks.map(c => c.kind)).toEqual(['text', 'stop'])
  })
})

describe('T-B1-04, T-C4-03 · observe mode and the skill check', () => {
  const SKILL = { tool: 'Skill', input: { skill: 'release-notes' } }
  const RESULT: TurnStepResult = { turnId: 'turn-1', index: 2, answer: 'Done.', toolUses: [], stopReason: 'end_turn', usage: USAGE }
  const ENDS = () => model([{ kind: 'text', index: 0, text: 'Done.' }, { kind: 'stop', stopReason: 'end_turn', usage: USAGE }], RESULT)

  test('observing: both asks carry observe and nothing is yielded, even when the binary answers', async () => {
    const m = ENDS()
    const { deps, asked, recorded } = stub(() => predict('obligation', [MAKE_GEN]), { isObserving: true })
    const { chunks, result } = await drain(turnStep(STEP, m.next, deps))
    expect(m.requests).toHaveLength(1)
    expect(chunks.map(c => c.kind)).toEqual(['text', 'stop'])
    expect(result).toEqual(RESULT)
    expect(asked.map(p => [p.ending, p.observe])).toEqual([
      [false, true],
      [true, true],
    ])
    expect(recorded).toEqual([])
  })

  test('a skill the session does not list is not yielded: the request goes out, nothing recorded', async () => {
    const m = ENDS()
    const listed: string[] = []
    const { deps, recorded } = stub(() => predict('skill', [SKILL]), {
      hasSkill: async s => {
        listed.push(s)
        return false
      },
    })
    const { chunks } = await drain(turnStep(STEP, m.next, deps))
    expect(m.requests).toHaveLength(1)
    expect(chunks.map(c => c.kind)).toEqual(['text', 'stop'])
    expect(listed).toEqual(['release-notes', 'release-notes'])
    expect(recorded).toEqual([])
  })

  test('a listed skill is yielded', async () => {
    const m = ENDS()
    const { deps } = stub(() => predict('skill', [SKILL]), { hasSkill: async s => s === 'release-notes' })
    const { chunks } = await drain(turnStep(STEP, m.next, deps))
    expect(m.requests).toHaveLength(0)
    expect(chunks[0]).toEqual({ kind: 'tool', index: 0, id: ID(0), name: 'Skill' })
  })
})

describe('T-C4-01 … T-C4-03 · onStep through the engine', () => {
  test('active: a TurnStep prediction is yielded in place of the request', async ($, on) => {
    const w = world(on, { hook: { TurnStep: PREDICT }, model: 'read' })
    await start($, w, null, false)
    const { chunks, result } = await drain($.turn.step(STEP))
    expect(chunks).toEqual([
      { kind: 'tool', index: 0, id: ID(0), name: 'Bash' },
      { kind: 'input', index: 0, json: '{"command":"make gen"}' },
      PREDICTED_STOP,
    ])
    expect(result).toMatchObject({ stopReason: null, toolUses: [{ name: 'Bash', input: { command: 'make gen' } }], usage: null })
    expect(w.models.map(x => x.index)).toEqual([])
    const asked = w.ran.find(r => r.argv[3] === 'TurnStep')
    expect(asked?.argv).toEqual(['/data/bin/hook', 'hook', 'claude', 'TurnStep'])
    expect(asked?.init?.env).toEqual({ CLAUDECODE: '1' })
    expect(asked?.init?.timeoutMs).toBe(250)
    expect(asked?.init?.cwd).toBe('/w')
    expect(JSON.parse(asked?.init?.stdin ?? '')).toEqual({
      session_id: 'sess-1',
      cwd: '/w',
      hook_event_name: 'TurnStep',
      turn_id: 'turn-1',
      index: 2,
      model: 'claude-x',
      ending: false,
      via: 'mod',
    })
    expect(w.sets.map(s => s.key)).toEqual(['predicted'])
  })

  test('active: nothing predicted, the request goes out and streams unchanged', async ($, on) => {
    const w = world(on, { model: 'read' })
    await start($, w, null, false)
    const { chunks, result } = await drain($.turn.step(STEP))
    expect(w.models.map(x => x.index)).toEqual([2])
    expect(chunks.map(c => c.kind)).toEqual(['text', 'tool', 'input', 'stop'])
    expect(result).toMatchObject({ stopReason: 'tool_use', answer: 'Reading.' })
  })

  test('step 0 and a subagent step are forwarded without asking', async ($, on) => {
    const w = world(on, { hook: { TurnStep: PREDICT }, model: 'read' })
    await start($, w, null, false)
    await drain($.turn.step({ ...STEP, index: 0 }))
    await drain($.turn.step({ ...STEP, agentId: 'agent-7' }))
    expect(w.models.map(x => x.index)).toEqual([0, 2])
    expect(w.ran.map(r => r.argv.slice(1).join(' '))).toEqual(['version', 'hook claude Standing'])
  })

  test('T-C5-04 · a headless session reads its routine model from the card at its first step, and routes from the next', async ($, on) => {
    const w = world(on, { hook: { TurnStep: wireLine('routine') }, card: { runtime: { routine_model: 'haiku' }, ui: {} }, model: 'read' })
    await start($, w, null, false)
    await drain($.turn.step(STEP))
    await drain($.turn.step({ ...STEP, index: 3 }))
    expect(w.models.map(x => x.index)).toEqual([2, 3])
    expect(w.models.map(({ model, effort }) => ({ model, effort }))).toEqual([
      { model: 'claude-x', effort: undefined },
      { model: 'haiku', effort: 'low' },
    ])
    expect(w.ran.filter(r => r.argv[1] === '--json').map(r => r.argv)).toEqual([['/data/bin/adage', '--json', '--for-session']])
    const steps = w.ran.filter(r => r.argv[3] === 'TurnStep').map(r => (JSON.parse(r.init?.stdin ?? '{}') as { routine_model?: true; model: string }))
    expect(steps.map(s => s.routine_model)).toEqual([undefined, true])
    expect(JSON.stringify(steps)).not.toContain('haiku')
  })

  test('no routine model on the card: a routine answer changes nothing', async ($, on) => {
    const w = world(on, { hook: { TurnStep: wireLine('routine') }, card: { runtime: { routine_model: '' } }, model: 'read' })
    await start($, w, null, false)
    await drain($.turn.step(STEP))
    await drain($.turn.step({ ...STEP, index: 3 }))
    expect(w.models.map(m => m.model)).toEqual(['claude-x', 'claude-x'])
  })

  test('a Skill prediction is yielded only for a skill `$.command.list()` lists', async ($, on) => {
    const skill = wireWith('predict', { kind: 'skill', route: 'notes', calls: [{ tool: 'Skill', input: { skill: 'release-notes' } }] })
    const w = world(on, { hook: { TurnStep: JSON.stringify(skill) }, model: 'read' })
    let listed = [{ name: 'review', description: '', source: 'plugin' as const }]
    on('command.list', () => ({ value: listed }))
    await start($, w, null, false)
    expect((await drain($.turn.step(STEP))).chunks.map(c => c.kind)).toEqual(['text', 'tool', 'input', 'stop'])
    expect(w.models.map(x => x.index)).toEqual([2])
    expect(w.sets).toEqual([])
    listed = [...listed, { name: 'release-notes', description: '', source: 'plugin' as const }]
    const { chunks } = await drain($.turn.step(STEP))
    expect(chunks[0]).toEqual({ kind: 'tool', index: 0, id: 'adage_notes_turn-1_2_0', name: 'Skill' })
    expect(w.models.map(x => x.index)).toEqual([2])
  })
})
