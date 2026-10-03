// The turn.step generator with a stubbed ask and a stubbed stream beneath (step.ts takes both as
// values): exactly the right chunks per kind, no request when predicted, forwarding otherwise, and
// the finish line at an end_turn stop.
import type { TurnStepChunk, TurnStepInput, TurnStepResult } from 'claude-code'
import { describe, expect, test } from 'claude-code/testing'

import type { Asked } from './bridge'
import type { Predicted } from './plugin'
import { turnStep } from './step'
import type { StepDeps, StepNext, TurnStepPayload } from './step'

const STEP: TurnStepInput = { turnId: 'turn-1', index: 2, model: 'claude-x', messageCount: 5 }
const USAGE = { input_tokens: 10, output_tokens: 5, cache_creation_input_tokens: 0, cache_read_input_tokens: 0, model: 'claude-x' }

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

async function drain(gen: AsyncGenerator<TurnStepChunk, TurnStepResult | void>) {
  const chunks: TurnStepChunk[] = []
  for (;;) {
    const r = await gen.next()
    if (r.done) return { chunks, result: r.value }
    chunks.push(r.value)
  }
}

const predict = (kind: string, calls: unknown[]): Asked => ({
  ok: true,
  answer: { predict: { kind, route: 'codegen-after-schema', track: 'acme/billing@1', calls } },
})
const MAKE_GEN = { tool: 'Bash', input: { command: 'make gen' } }
const ID = (k: number) => `adage_codegen-after-schema_turn-1_2_${k}`

describe('a prediction before the request', () => {
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
    test(`${kind}: yields the calls and a tool_use stop, returns the result, sends no request`, async () => {
      const { next, requests } = model([], { turnId: 'turn-1', index: 2, answer: 'unused', toolUses: [], stopReason: 'end_turn', usage: USAGE })
      const { deps, asked, recorded } = stub(() => predict(kind, calls))
      const { chunks, result } = await drain(turnStep(STEP, next, deps))
      const typed = calls as { tool: string; input: unknown }[]
      expect(chunks).toEqual([
        ...typed.flatMap((c, k) => [
          { kind: 'tool', index: k, id: ID(k), name: c.tool },
          { kind: 'input', index: k, json: JSON.stringify(c.input) },
        ]),
        { kind: 'stop', stopReason: 'tool_use', usage: null },
      ])
      expect(result).toEqual({
        turnId: 'turn-1',
        index: 2,
        answer: '',
        toolUses: typed.map(c => ({ name: c.tool, input: c.input })),
        stopReason: 'tool_use',
        usage: null,
      })
      expect(requests).toHaveLength(0)
      expect(asked).toEqual([
        { session_id: 'sess-1', cwd: '/w', hook_event_name: 'TurnStep', turn_id: 'turn-1', index: 2, model: 'claude-x', ending: false },
      ])
      expect(recorded).toEqual([
        { turnId: 'turn-1', index: 2, route: 'codegen-after-schema', kind, ids: typed.map((_, k) => ID(k)), isEnding: false },
      ])
    })
  }
})

describe('forwarding', () => {
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
    for (const answer of [(): Asked => ({ ok: false }), () => new Error('boom'), (): Asked => ({ ok: true, answer: { predict: { kind: 'nope' } } })]) {
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

describe('the finish line (ending)', () => {
  test("an end_turn stop asks with ending: true; a prediction keeps the model's text and replaces the stop", async () => {
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
      { kind: 'stop', stopReason: 'tool_use', usage: USAGE },
    ])
    expect(result).toEqual({
      turnId: 'turn-1',
      index: 2,
      answer: 'Done: schema edited.',
      toolUses: [{ name: 'Bash', input: { command: 'make gen' } }],
      stopReason: 'tool_use',
      usage: USAGE,
    })
    expect(recorded).toEqual([
      { turnId: 'turn-1', index: 2, route: 'codegen-after-schema', kind: 'obligation', ids: [ID(0)], isEnding: true },
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
