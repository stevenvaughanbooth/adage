// The turn.step hook (step.ts; test-plan §6 C4, C5, C7, C8): `turnStep` with a stubbed ask and a
// stubbed stream beneath (it takes both as values): exactly the right chunks per kind, no request
// when predicted, forwarding otherwise, the finish line at an end_turn stop, nothing yielded unless
// the engine allows every call unasked (T-C4-07); then `onStep` through
// the engine, the binary answering beneath.
import type { TurnStepChunk, TurnStepInput, TurnStepResult } from 'claude-code'
import { describe, expect, test } from 'claude-code/testing'

import type { Asked } from './bridge'
import {
  AUTO_MODE_BASH,
  AUTO_MODE_BASH_UNLISTED,
  DEFAULT_MODE_BASH,
  FAIL,
  LINE,
  PREDICT,
  READ_ALLOWED,
  SKILL_ASKED,
  USAGE,
  bandText,
  drain,
  engineThread,
  forwardedResponse,
  madeUpResponse,
  prompt,
  start,
  unseenResults,
  wire,
  wireLine,
  wireWith,
  phasedStep,
  world,
} from './fixtures/kit'
import type { ThreadMessage } from './fixtures/kit'
import type { Predicted } from './plugin'
import { PREDICTED_STOP } from './predict'
import { heldBy, permissionOf, samePrediction, verdictOf } from './predict'
import { isRoutine, turnStep } from './step'
import type { StepDeps, StepNext, TurnStepPayload, Withheld } from './step'

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
  const withheld: Withheld[] = []
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
    check: async () => ({ decision: 'allow' }),
    withheld: async w => {
      withheld.push(w)
    },
    ...over,
  }
  return { deps, asked, recorded, withheld }
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
      // T-C4-08: the dry run, then the verdict that commits it.
      const base = { session_id: 'sess-1', cwd: '/w', hook_event_name: 'TurnStep', turn_id: 'turn-1', index: 2, model: 'claude-x', ending: false, via: 'mod' }
      expect(asked).toEqual([
        { ...base, dry_run: true },
        { ...base, verdict: { permission: 'allow', route: ROUTE } },
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
    expect(asked.map(p => [p.ending, p.dry_run, p.verdict?.permission])).toEqual([
      [false, true, undefined],
      [true, true, undefined],
      [true, undefined, 'allow'],
    ])
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
    const asks = w.ran.filter(r => r.argv[3] === 'TurnStep')
    const base = { session_id: 'sess-1', cwd: '/w', hook_event_name: 'TurnStep', turn_id: 'turn-1', index: 2, model: 'claude-x', ending: false, via: 'mod' }
    for (const asked of asks) {
      expect(asked.argv).toEqual(['/data/bin/hook', 'hook', 'claude', 'TurnStep'])
      expect(asked.init?.env).toEqual({ CLAUDECODE: '1' })
      expect(asked.init?.timeoutMs).toBe(250)
      expect(asked.init?.cwd).toBe('/w')
    }
    expect(asks.map(a => JSON.parse(a.init?.stdin ?? ''))).toEqual([
      { ...base, dry_run: true },
      { ...base, verdict: { permission: 'allow', route: ROUTE } },
    ])
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

describe('T-C4-07 · a predicted call is made only under the person\'s permissions', () => {
  const RESULT: TurnStepResult = { turnId: 'turn-1', index: 2, answer: 'Done.', toolUses: [], stopReason: 'end_turn', usage: USAGE }
  const ENDS = () => model([{ kind: 'text', index: 0, text: 'Done.' }, { kind: 'stop', stopReason: 'end_turn', usage: USAGE }], RESULT)
  const READS = [
    { tool: 'Read', input: { file_path: '/w/a.ts' } },
    { tool: 'Read', input: { file_path: '/w/b.ts' } },
  ]
  /** A check that answers `verdict` for every call and keeps what it was asked. */
  const checking = (verdict: (tool: string, input: Record<string, unknown>) => unknown) => {
    const checked: { tool: string; input: Record<string, unknown> }[] = []
    const check: StepDeps['check'] = async (tool, input) => {
      checked.push({ tool, input })
      const v = verdict(tool, input)
      if (v instanceof Error) throw v
      return v
    }
    return { check, checked }
  }

  test('allow on every call: yielded, each call checked once as the engine would decide it', async () => {
    const m = ENDS()
    const { check, checked } = checking(tool => (tool === 'Read' ? READ_ALLOWED : DEFAULT_MODE_BASH))
    const { deps, recorded, withheld } = stub(() => predict('chain', READS), { check })
    const { chunks } = await drain(turnStep(STEP, m.next, deps))
    expect(m.requests).toHaveLength(0)
    expect(chunks.map(c => c.kind)).toEqual(['tool', 'input', 'tool', 'input', 'stop'])
    expect(checked).toEqual(READS)
    expect(recorded).toHaveLength(1)
    expect(withheld).toEqual([])
  })

  const held: [string, unknown, Withheld['decision'], string][] = [
    ['ask (a rule or the mode asks)', { decision: 'ask', reason: 'This command requires approval' }, 'ask', 'This command requires approval'],
    ['deny', { decision: 'deny', reason: 'Permission to use Bash with command make gen has been denied.', rule: 'Bash(make:*)' }, 'deny', 'Permission to use Bash with command make gen has been denied.'],
    ['no verdict (the check throws)', new Error('no verdict'), 'none', ''],
    ['no verdict (a malformed answer)', { decision: 'maybe' }, 'none', ''],
    ['no verdict (nothing)', undefined, 'none', ''],
  ]
  for (const [name, verdict, decision, reason] of held) {
    test(`${name}: nothing yielded or recorded, the request goes out and its stream passes unchanged, the reason kept`, async () => {
      const m = ENDS()
      const { check } = checking(() => verdict)
      const { deps, recorded, withheld } = stub(p => (p.ending ? { ok: true, answer: undefined } : predict('obligation', [MAKE_GEN])), { check })
      const { chunks, result } = await drain(turnStep(STEP, m.next, deps))
      expect(m.requests).toEqual([STEP])
      expect(chunks).toEqual([
        { kind: 'text', index: 0, text: 'Done.' },
        { kind: 'stop', stopReason: 'end_turn', usage: USAGE },
      ])
      expect(result).toEqual(RESULT)
      expect(recorded).toEqual([])
      expect(withheld).toEqual([{ turnId: 'turn-1', index: 2, route: ROUTE, kind: 'obligation', isEnding: false, tool: 'Bash', decision, reason }])
    })
  }

  test("the finish line: held back, the model's response and its end_turn stop pass as they came", async () => {
    const m = ENDS()
    const { check } = checking(() => AUTO_MODE_BASH)
    const { deps, asked, recorded, withheld } = stub(p => (p.ending ? predict('obligation', [MAKE_GEN]) : { ok: true, answer: undefined }), { check })
    const { chunks, result } = await drain(turnStep(STEP, m.next, deps))
    expect(asked.map(p => [p.ending, p.dry_run, p.verdict])).toEqual([
      [false, true, undefined],
      [true, true, undefined],
      [true, undefined, { permission: 'ask', route: ROUTE, tool: 'Bash', reason: AUTO_MODE_BASH.reason }],
    ])
    expect(chunks).toEqual([
      { kind: 'text', index: 0, text: 'Done.' },
      { kind: 'stop', stopReason: 'end_turn', usage: USAGE },
    ])
    expect(result).toEqual(RESULT)
    expect(recorded).toEqual([])
    expect(withheld).toEqual([{ turnId: 'turn-1', index: 2, route: ROUTE, kind: 'obligation', isEnding: true, tool: 'Bash', decision: 'ask', reason: AUTO_MODE_BASH.reason }])
  })

  test('one call of a chain not allowed holds the whole step: the first such call, in order, is named', async () => {
    const m = ENDS()
    const { check, checked } = checking((_, input) => (input.file_path === '/w/b.ts' ? { decision: 'deny', reason: 'outside the project' } : READ_ALLOWED))
    const { deps, withheld } = stub(() => predict('chain', READS), { check })
    const { chunks } = await drain(turnStep(STEP, m.next, deps))
    expect(m.requests).toHaveLength(1)
    expect(chunks.map(c => c.kind)).toEqual(['text', 'stop'])
    expect(checked).toHaveLength(4) // both reads, before the request and at the finish line
    expect(withheld.map(w => [w.tool, w.decision, w.reason, w.isEnding])).toEqual([
      ['Read', 'deny', 'outside the project', false],
      ['Read', 'deny', 'outside the project', true],
    ])
  })

  test('a skill the engine asks about is held back even when the session lists it', async () => {
    const m = ENDS()
    const { check } = checking((_, input) => SKILL_ASKED(String(input.skill)))
    const { deps, withheld } = stub(() => predict('skill', [{ tool: 'Skill', input: { skill: 'release-notes' } }]), { check })
    await drain(turnStep(STEP, m.next, deps))
    expect(m.requests).toHaveLength(1)
    expect(withheld[0]).toMatchObject({ kind: 'skill', tool: 'Skill', decision: 'ask', reason: 'Execute skill: release-notes' })
  })

  test('auto mode as the engine reports it: every Bash call is `ask` with no rule, so Bash steps are held; a Read chain stays allowed', async () => {
    for (const verdict of [AUTO_MODE_BASH, AUTO_MODE_BASH_UNLISTED]) {
      expect(permissionOf(verdict)).toEqual({ decision: 'ask', reason: verdict.reason })
      expect(await heldBy([MAKE_GEN], async () => verdict)).toEqual({ tool: 'Bash', decision: 'ask', reason: verdict.reason })
    }
    const auto = async (tool: string) => (tool === 'Bash' ? AUTO_MODE_BASH : READ_ALLOWED)
    expect(await heldBy(READS, auto)).toBeUndefined()
    expect(await heldBy([...READS, MAKE_GEN], auto)).toEqual({ tool: 'Bash', decision: 'ask', reason: AUTO_MODE_BASH.reason })
    expect(await heldBy([MAKE_GEN], async () => DEFAULT_MODE_BASH)).toBeUndefined()
  })

  test('a check that throws before it returns a promise is no verdict', async () => {
    expect(
      await heldBy([MAKE_GEN], () => {
        throw new Error('sync')
      }),
    ).toEqual({ tool: 'Bash', decision: 'none', reason: '' })
  })

  test('a failing keeper of the held-back step changes nothing: the request still goes out', async () => {
    const m = ENDS()
    const { deps } = stub(() => predict('run', [MAKE_GEN]), {
      check: async () => AUTO_MODE_BASH,
      withheld: () => {
        throw new Error('full')
      },
    })
    const { chunks } = await drain(turnStep(STEP, m.next, deps))
    expect(m.requests).toEqual([STEP])
    expect(chunks.map(c => c.kind)).toEqual(['text', 'stop'])
  })
})

describe('T-C4-07 · onStep through the engine, under the session\'s permission mode', () => {
  test('default mode (the Bash rule allows): the prediction is yielded, after one check of its call', async ($, on) => {
    const w = world(on, { hook: { TurnStep: PREDICT }, model: 'read', verdict: tool => (tool === 'Bash' ? DEFAULT_MODE_BASH : undefined) })
    await start($, w, null, false)
    const { chunks } = await drain($.turn.step(STEP))
    expect(chunks.map(c => c.kind)).toEqual(['tool', 'input', 'stop'])
    expect(w.checked).toEqual(['make gen'])
    expect(w.models).toEqual([])
    expect(w.sets.map(s => s.key)).toEqual(['predicted'])
  })

  test('auto mode: nothing yielded or kept as predicted, the request goes out, no call is made', async ($, on) => {
    const w = world(on, { hook: { TurnStep: PREDICT }, model: 'read', verdict: tool => (tool === 'Bash' ? AUTO_MODE_BASH : undefined) })
    await start($, w, null, false)
    const { chunks, result } = await drain($.turn.step(STEP))
    expect(chunks.map(c => c.kind)).toEqual(['text', 'tool', 'input', 'stop'])
    expect(result).toMatchObject({ stopReason: 'tool_use', answer: 'Reading.' })
    expect(w.checked).toEqual(['make gen'])
    expect(w.models.map(x => x.index)).toEqual([2])
    expect(w.sets).toEqual([])
    expect(w.bash).toEqual([])
  })

  test('a check the engine cannot answer is no verdict: forwarded', async ($, on) => {
    const w = world(on, { hook: { TurnStep: PREDICT }, model: 'read', verdict: () => FAIL.exit })
    await start($, w, null, false)
    const { chunks } = await drain($.turn.step(STEP))
    expect(chunks.map(c => c.kind)).toEqual(['text', 'tool', 'input', 'stop'])
    expect(w.sets).toEqual([])
  })

  test('band.withheld: the attention line says why once per session, until the next prompt', async ($, on) => {
    const w = world(on, { feed: [LINE.learnerIdle, LINE.healthy], hook: { TurnStep: PREDICT }, model: 'read', verdict: tool => (tool === 'Bash' ? AUTO_MODE_BASH : undefined) })
    await start($, w)
    const before = await bandText($)
    await drain($.turn.step(STEP))
    const line = '» adage left a step to the agent: your permissions do not allow it unasked'
    expect(await bandText($)).toBe(`◠ adage  ● healthy${line}`)
    expect(await bandText($, 'desktop')).toBe(`◠ adage  ● healthy${line}`)
    await prompt($, 'go on')
    expect(await bandText($)).toBe(before)
    // Held back again in the same session: said once, so not again.
    await drain($.turn.step({ ...STEP, index: 3 }))
    expect(await bandText($)).toBe(before)
  })
})

describe('T-C4-08 · the two phases: a dry run commits nothing, the verdict commits or withholds', () => {
  const RESULT: TurnStepResult = { turnId: 'turn-1', index: 2, answer: 'Done.', toolUses: [], stopReason: 'end_turn', usage: USAGE }
  const ENDS = () => model([{ kind: 'text', index: 0, text: 'Done.' }, { kind: 'stop', stopReason: 'end_turn', usage: USAGE }], RESULT)
  const phases = (asked: TurnStepPayload[]) => asked.map(p => (p.dry_run === true ? 'dry run' : p.verdict !== undefined ? `verdict ${p.verdict.permission}` : 'one phase'))
  /** The binary as it answers each phase: the dry run and an allow predict; anything else, `held`. */
  const binary =
    (p: Asked, held: Asked = { ok: true, answer: undefined }) =>
    (q: TurnStepPayload): Asked =>
      q.verdict !== undefined && q.verdict.permission !== 'allow' ? held : q.ending ? { ok: true, answer: undefined } : p

  test('nothing predicted: the dry run is the only ask, and no verdict follows', async () => {
    const m = ENDS()
    const { deps, asked } = stub(() => ({ ok: true, answer: undefined }))
    await drain(turnStep(STEP, m.next, deps))
    expect(phases(asked)).toEqual(['dry run', 'dry run'])
  })

  test('held: the verdict names the route, the first call held, its decision and sentence; the binary is never asked to commit', async () => {
    const m = ENDS()
    const { deps, asked, recorded } = stub(binary(predict('run', [MAKE_GEN])), { check: async () => AUTO_MODE_BASH })
    await drain(turnStep(STEP, m.next, deps))
    expect(m.requests).toEqual([STEP])
    expect(asked[1]?.verdict).toEqual({ permission: 'ask', route: ROUTE, tool: 'Bash', reason: AUTO_MODE_BASH.reason })
    expect(phases(asked)).toEqual(['dry run', 'verdict ask', 'dry run'])
    expect(recorded).toEqual([])
  })

  test('a held step the binary routes goes to the routine model, as when nothing is predicted', async () => {
    const m = ENDS()
    const { deps } = stub(binary(predict('run', [MAKE_GEN]), { ok: true, answer: wire('routine') }), { check: async () => AUTO_MODE_BASH, routineModel: 'haiku' })
    await drain(turnStep(STEP, m.next, deps))
    expect(m.requests).toEqual([{ ...STEP, model: 'haiku', effort: 'low' }])
  })

  test('allow, but the verdict answers nothing, fails, or another step: nothing is yielded, the request goes out', async () => {
    const other = predict('run', [{ tool: 'Bash', input: { command: 'make other' } }])
    for (const said of [{ ok: true, answer: undefined }, { ok: false }, new Error('timeout'), other] as (Asked | Error)[]) {
      const m = ENDS()
      const { deps, recorded } = stub(q => (q.verdict !== undefined ? said : q.ending ? { ok: true, answer: undefined } : predict('run', [MAKE_GEN])))
      const { chunks } = await drain(turnStep(STEP, m.next, deps))
      expect(m.requests).toEqual([STEP])
      expect(chunks.map(c => c.kind)).toEqual(['text', 'stop'])
      expect(recorded).toEqual([])
    }
  })

  test('a skill the session does not list is held as no verdict, and the band is not told', async () => {
    const m = ENDS()
    const skill = predict('skill', [{ tool: 'Skill', input: { skill: 'release-notes' } }])
    const { deps, asked, withheld } = stub(binary(skill), { hasSkill: async () => false })
    await drain(turnStep(STEP, m.next, deps))
    expect(asked[1]?.verdict).toEqual({ permission: 'none', route: ROUTE, tool: 'Skill', reason: 'This session does not list the skill' })
    expect(withheld).toEqual([])
  })

  test('observing: only the dry run is asked (the binary records it at once), never a verdict', async () => {
    const m = ENDS()
    const { deps, asked } = stub(() => predict('obligation', [MAKE_GEN]), { isObserving: true })
    await drain(turnStep(STEP, m.next, deps))
    expect(phases(asked)).toEqual(['dry run', 'dry run'])
  })

  test('verdictOf and samePrediction', () => {
    const p = { kind: 'run' as const, route: 'r', track: 't', calls: [MAKE_GEN] }
    expect(verdictOf(p, undefined)).toEqual({ permission: 'allow', route: 'r' })
    expect(verdictOf(p, { tool: 'Bash', decision: 'none', reason: '' })).toEqual({ permission: 'none', route: 'r', tool: 'Bash' })
    expect(samePrediction(p, { ...p })).toBe(true)
    expect(samePrediction(p, { ...p, route: 's' })).toBe(false)
    expect(samePrediction(p, { ...p, calls: [{ tool: 'Bash', input: { command: 'make' } }] })).toBe(false)
  })
})

describe('T-C4-08 · onStep through the engine, the binary answering each phase', () => {
  const stdins = (w: { ran: { argv: string[]; init?: { stdin?: string } }[] }) =>
    w.ran.filter(r => r.argv[3] === 'TurnStep').map(r => JSON.parse(r.init?.stdin ?? '{}') as Record<string, unknown>)

  test('default mode: the dry run, then an allow verdict; the prediction is yielded', async ($, on) => {
    const w = world(on, { answer: phasedStep(), model: 'read', verdict: tool => (tool === 'Bash' ? DEFAULT_MODE_BASH : undefined) })
    await start($, w, null, false)
    const { chunks } = await drain($.turn.step(STEP))
    expect(chunks.map(c => c.kind)).toEqual(['tool', 'input', 'stop'])
    expect(stdins(w).map(p => [p.dry_run, p.verdict])).toEqual([
      [true, undefined],
      [undefined, { permission: 'allow', route: ROUTE }],
    ])
  })

  test('auto mode: the verdict says ask with the engine\'s sentence; nothing yielded, the request goes out', async ($, on) => {
    const w = world(on, { answer: phasedStep(), model: 'read', verdict: tool => (tool === 'Bash' ? AUTO_MODE_BASH : undefined) })
    await start($, w, null, false)
    const { chunks } = await drain($.turn.step(STEP))
    expect(chunks.map(c => c.kind)).toEqual(['text', 'tool', 'input', 'stop'])
    expect(w.models.map(x => x.index)).toEqual([2])
    expect(stdins(w).map(p => p.verdict)).toEqual([undefined, { permission: 'ask', route: ROUTE, tool: 'Bash', reason: AUTO_MODE_BASH.reason }])
    expect(w.sets).toEqual([])
    expect(w.bash).toEqual([])
  })
})
