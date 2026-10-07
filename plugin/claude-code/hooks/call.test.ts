// A Bash call (call.ts; plan C3, C6; T-C3-04, T-C3-05, T-C3-07, T-C6-03, T-X-02): through the
// module as the engine loads it, the binary answered beneath the plugin by the kit (its plan on
// `dry_run`, its final word on `verdict`), the person's rules by its `tool.check` hook and the Bash
// tool by its `tool.call` hook (nothing runs); `bashCall` alone over plain deps; the receipt card
// and the beside line through the mounted `ToolResult` row. The plan, the replacement and the
// receipt are the wire's (docs/claude-mod/impl/wire.md); the final word on a rewrite is the command
// hook's own format, not on that wire.
import type { On, ToolCallResult } from 'claude-code'
import { describe, expect, test } from 'claude-code/testing'

import { bashCall, planOf, rewritePlanOf, verdictAnswerOf } from './call'
import type { CallDeps, PreToolPayload } from './call'
import { FAIL, PYTEST, RESULT, SURFACES, VENV, start, wire, wireWith, world } from './fixtures/kit'
import type { WireAnswer } from './fixtures/kit'
import { madeForAgentLine, predictedRoute, toolUseId } from './predict'
import { receiptAnswerOf, shortId } from './receipt'
import { substituteAnswerOf } from './substitute'

function defined<T>(v: T | undefined, what: string): T {
  if (v === undefined) throw new Error(`the wire's ${what} does not read`)
  return v
}

// The dry run's plan with the parts each case is about (the wire's `plan` holds all three), and the
// final answer to `answered: receipt`.
const RECEIPT_ONLY = wireWith('plan', { substitute: undefined, rewrite: undefined })
const REWRITE_ONLY = wireWith('plan', { substitute: undefined, receipt: undefined })
const RECEIPT_AND_REWRITE = wireWith('plan', { substitute: undefined })
const ANSWERED = wire('receipt')
const ANSWER = defined(receiptAnswerOf(ANSWERED), 'receipt')
const CARD = defined(ANSWER.card, "receipt's card")
const REWRITE = defined(planOf(wire('plan'))?.rewrite, "plan's rewrite")
const WORD = "Adage pointed this test run at the project's environment (.venv/bin/python)."
const SAME = { systemMessage: 'adage: pointed this test run at .venv/bin/python', hookSpecificOutput: { hookEventName: 'PreToolUse', updatedInput: { command: VENV }, additionalContext: WORD } }
const ADVICE = 'Adage: this test run uses the system Python; the project\'s environment is .venv/bin/python (Adage did not rewrite the command: the rules treat the two differently).'
const DIFFERS = { hookSpecificOutput: { hookEventName: 'PreToolUse', additionalContext: ADVICE } }

type Opts = {
  version?: string
  plan?: unknown
  /** The final word, or a function of the verdict the mod sent (`answered`, `same`, `differs`). */
  final?: unknown | ((verdict: Record<string, unknown>) => unknown)
  rules?: Record<string, 'allow' | 'ask' | 'deny'>
  observe?: string
  /** How the binary fails on the dry run: garbage, a non-zero exit, or a run that cannot start. */
  fail?: 'garbage' | 'exit' | 'throw'
  /** The rules cannot be read. */
  checkThrows?: boolean
}

/** The kit's world with the binary answering `PreToolUse`: what it was asked, what ran beneath, the rules read and the notices. */
function callWorld(on: On, o: Opts) {
  const w = world(on, {
    ...(o.version === undefined ? {} : { version: o.version }),
    ...(o.observe === undefined ? {} : { env: { ADAGE_MOD_OBSERVE: o.observe } }),
    ...(o.rules === undefined ? {} : { rules: o.rules }),
    rulesFail: o.checkThrows === true,
    echo: true,
    answer: (event, p) => {
      if (event !== 'PreToolUse') return undefined
      if (p.dry_run === true) return o.fail === 'garbage' ? '{' : o.fail === 'exit' ? FAIL.exit : o.fail === 'throw' ? FAIL.start : o.plan
      return typeof o.final === 'function' ? (o.final as (v: Record<string, unknown>) => unknown)(p.verdict as Record<string, unknown>) : o.final
    },
  })
  return {
    get asked() {
      return w.payloads('PreToolUse')
    },
    ran: w.bash,
    checked: w.checked,
    get notices() {
      return w.notices.map(n => n.text ?? '')
    },
    w,
  }
}

const verdicts = (asked: readonly Record<string, unknown>[]) => asked.map(p => (p.dry_run === true ? 'dry_run' : JSON.stringify(p.verdict)))

describe('T-C3-04 · the receipt as the result, only on allow', () => {
  test('allow: the receipt is the result, the note its context, nothing runs, two asks and one check', async ($, on) => {
    const w = callWorld(on, { plan: RECEIPT_ONLY, final: (v: Record<string, unknown>) => (v.answered === 'receipt' ? ANSWERED : undefined) })
    await start($, w.w, null, false)
    const r = await $.tool.call({ tool: 'Bash', command: PYTEST, tool_use_id: 't1' })
    expect(r).toEqual({ result: { stdout: ANSWER.result, stderr: '', interrupted: false }, context: [ANSWER.note] })
    expect(w.ran).toEqual([])
    expect(verdicts(w.asked)).toEqual(['dry_run', '{"answered":"receipt"}'])
    expect(w.asked[0]).toMatchObject({ session_id: 'sess-1', cwd: '/w', hook_event_name: 'PreToolUse', tool_name: 'Bash', tool_input: { command: PYTEST }, tool_use_id: 't1', via: 'mod' })
    expect(w.asked.every(p => p.observe === undefined && p.agent_id === undefined)).toBe(true)
    expect(w.checked).toEqual([PYTEST])
    expect(w.notices).toEqual([])
  })

  for (const decision of ['ask', 'deny'] as const) {
    test(`${decision}: the call runs as written after one ask; the receipt is never asked for`, async ($, on) => {
      const w = callWorld(on, { plan: RECEIPT_ONLY, final: () => ANSWERED, rules: { [PYTEST]: decision } })
      await start($, w.w, null, false)
      const r = await $.tool.call({ tool: 'Bash', command: PYTEST, tool_use_id: 't1' })
      expect(w.ran).toEqual([PYTEST])
      expect(r.context).toBeUndefined()
      expect(verdicts(w.asked)).toEqual(['dry_run'])
    })
  }

  test('ask, with a rewrite also planned: the rewrite path is taken (a second ask with the engine verdict)', async ($, on) => {
    const w = callWorld(on, { plan: RECEIPT_AND_REWRITE, final: (v: Record<string, unknown>) => (v.engine === 'same' ? SAME : undefined), rules: { [PYTEST]: 'ask', [VENV]: 'ask' } })
    await start($, w.w, null, false)
    await $.tool.call({ tool: 'Bash', command: PYTEST, tool_use_id: 't1' })
    expect(w.ran).toEqual([VENV])
    expect(verdicts(w.asked)).toEqual(['dry_run', '{"engine":"same"}'])
  })

  test('the final word withheld (the control arm, or no longer fresh): the call runs as written', async ($, on) => {
    const w = callWorld(on, { plan: RECEIPT_ONLY })
    await start($, w.w, null, false)
    const r = await $.tool.call({ tool: 'Bash', command: PYTEST, tool_use_id: 't1' })
    expect(w.ran).toEqual([PYTEST])
    expect(r.context).toBeUndefined()
    expect(verdicts(w.asked)).toEqual(['dry_run', '{"answered":"receipt"}'])
  })

  test('a fresh receipt without a result (a host that cannot answer calls) runs the call', async ($, on) => {
    const w = callWorld(on, { plan: wireWith('plan', { substitute: undefined, rewrite: undefined, receipt: { result: undefined } }), final: () => ANSWERED })
    await start($, w.w, null, false)
    await $.tool.call({ tool: 'Bash', command: PYTEST, tool_use_id: 't1' })
    expect(w.ran).toEqual([PYTEST])
    expect(verdicts(w.asked)).toEqual(['dry_run'])
  })

  test('observe mode: the plan is asked with observe, and the receipt never answers', async ($, on) => {
    const w = callWorld(on, { plan: RECEIPT_ONLY, final: () => ANSWERED, observe: '1' })
    await start($, w.w, null, false)
    await $.tool.call({ tool: 'Bash', command: PYTEST, tool_use_id: 't1' })
    expect(w.ran).toEqual([PYTEST])
    expect(w.asked.map(p => [p.dry_run, p.observe])).toEqual([[true, true]])
  })
})

describe('T-C3-05 · the rewrite checked by the engine', () => {
  test("same: the final ask carries engine same, the call runs rewritten with the binary's word, one notice", async ($, on) => {
    const w = callWorld(on, { plan: REWRITE_ONLY, final: (v: Record<string, unknown>) => (v.engine === 'same' ? SAME : undefined) })
    await start($, w.w, null, false)
    const r = await $.tool.call({ tool: 'Bash', command: PYTEST, tool_use_id: 't1' })
    expect(w.ran).toEqual([VENV])
    expect(r).toMatchObject({ result: { stdout: `ran ${VENV}` }, context: [WORD] })
    expect(verdicts(w.asked)).toEqual(['dry_run', '{"engine":"same"}'])
    expect(w.checked.sort()).toEqual([VENV, PYTEST].sort())
    expect(w.notices).toEqual(['↪ adage: pointed this test run at .venv/bin/python'])
  })

  test('differs: the final ask carries engine differs, the call runs as written, the advice is its context, no notice', async ($, on) => {
    const w = callWorld(on, { plan: REWRITE_ONLY, final: (v: Record<string, unknown>) => (v.engine === 'differs' ? DIFFERS : SAME), rules: { [PYTEST]: 'allow', [VENV]: 'ask' } })
    await start($, w.w, null, false)
    const r = await $.tool.call({ tool: 'Bash', command: PYTEST, tool_use_id: 't1' })
    expect(w.ran).toEqual([PYTEST])
    expect(r).toMatchObject({ result: { stdout: `ran ${PYTEST}` }, context: [ADVICE] })
    expect(verdicts(w.asked)).toEqual(['dry_run', '{"engine":"differs"}'])
    expect(w.notices).toEqual([])
  })

  test('advice alone in the plan: the call runs as written after one ask (the command hook delivers it)', async ($, on) => {
    const w = callWorld(on, { plan: wireWith('plan-advice', { receipt: undefined }), final: () => SAME })
    await start($, w.w, null, false)
    const r = await $.tool.call({ tool: 'Bash', command: PYTEST, tool_use_id: 't1' })
    expect(w.ran).toEqual([PYTEST])
    expect(r.context).toBeUndefined()
    expect(verdicts(w.asked)).toEqual(['dry_run'])
    expect(w.checked).toEqual([])
  })

  test('the binary downgraded a same verdict to advice: the call runs as written with the advice', async ($, on) => {
    const w = callWorld(on, { plan: REWRITE_ONLY, final: () => DIFFERS })
    await start($, w.w, null, false)
    const r = await $.tool.call({ tool: 'Bash', command: PYTEST, tool_use_id: 't1' })
    expect(w.ran).toEqual([PYTEST])
    expect(r.context).toEqual([ADVICE])
    expect(w.notices).toEqual([])
  })

  test('observe mode: the rewrite is never made and the engine is not asked for a verdict', async ($, on) => {
    const w = callWorld(on, { plan: REWRITE_ONLY, final: () => SAME, observe: '1' })
    await start($, w.w, null, false)
    await $.tool.call({ tool: 'Bash', command: PYTEST, tool_use_id: 't1' })
    expect(w.ran).toEqual([PYTEST])
    expect(verdicts(w.asked)).toEqual(['dry_run'])
  })
})

describe('T-X-02 · every path fails open', () => {
  for (const fail of ['garbage', 'exit', 'throw'] as const) {
    test(`a dry run that answers ${fail}: the call runs as written, nothing else is asked`, async ($, on) => {
      const w = callWorld(on, { plan: RECEIPT_AND_REWRITE, final: () => ANSWERED, fail })
      await start($, w.w, null, false)
      const r = await $.tool.call({ tool: 'Bash', command: PYTEST, tool_use_id: 't1' })
      expect(w.ran).toEqual([PYTEST])
      expect(r.context).toBeUndefined()
      expect(w.asked).toHaveLength(1)
      expect(w.checked).toEqual([])
    })
  }

  test('the rules cannot be read: no receipt answers and no rewrite is made', async ($, on) => {
    const w = callWorld(on, { plan: RECEIPT_AND_REWRITE, final: () => ANSWERED, checkThrows: true })
    await start($, w.w, null, false)
    await $.tool.call({ tool: 'Bash', command: PYTEST, tool_use_id: 't1' })
    expect(w.ran).toEqual([PYTEST])
    expect(verdicts(w.asked)).toEqual(['dry_run'])
  })

  test('a malformed final answer runs the call as written', async ($, on) => {
    const w = callWorld(on, { plan: RECEIPT_ONLY, final: () => wireWith('receipt', { result: '' }) })
    await start($, w.w, null, false)
    const r = await $.tool.call({ tool: 'Bash', command: PYTEST, tool_use_id: 't1' })
    expect(w.ran).toEqual([PYTEST])
    expect(r.context).toBeUndefined()
  })

  test('inert: the call runs as written and nothing is asked or checked', async ($, on) => {
    const w = callWorld(on, { version: '', plan: RECEIPT_ONLY, final: () => ANSWERED })
    await start($, w.w, null, false)
    await $.tool.call({ tool: 'Bash', command: PYTEST, tool_use_id: 't1' })
    expect(w.ran).toEqual([PYTEST])
    expect(w.asked).toEqual([])
    expect(w.checked).toEqual([])
  })
})

const DEAD_END = 'python3 -m pytest tests'
const SUB = defined(substituteAnswerOf(wire('substitute')), 'substitute')
const CONTEXT = SUB.context

describe('T-C6-03 · the dead end that never runs', () => {
  const PLANNED = wireWith('plan', { receipt: undefined, rewrite: undefined })
  const MADE = wire('substitute')

  test('the same verdict for both commands: the learned command runs, with one line for the model and the person', async ($, on) => {
    const w = callWorld(on, { plan: PLANNED, final: MADE })
    await start($, w.w, null, false)
    const r = await $.tool.call({ tool: 'Bash', command: DEAD_END })
    expect(w.ran).toEqual([SUB.command])
    expect(r.context).toEqual([CONTEXT])
    expect(w.notices).toEqual([CONTEXT])
    expect(w.asked.map(p => [p.hook_event_name, p.dry_run, p.verdict])).toEqual([
      ['PreToolUse', true, undefined],
      ['PreToolUse', undefined, { engine: 'same', substitute: SUB.route }],
    ])
    expect(w.asked[0]).toMatchObject({ session_id: 'sess-1', cwd: '/w', tool_name: 'Bash', tool_input: { command: DEAD_END } })
    expect(w.asked.every(p => p.observe === undefined)).toBe(true)
  })

  test('different verdicts: the call runs as written, and the binary is not asked again', async ($, on) => {
    const w = callWorld(on, { plan: PLANNED, final: MADE, rules: { [DEAD_END]: 'allow', [SUB.command]: 'ask' } })
    await start($, w.w, null, false)
    const r = await $.tool.call({ tool: 'Bash', command: DEAD_END })
    expect(w.ran).toEqual([DEAD_END])
    expect(r.context).toBeUndefined()
    expect(w.notices).toEqual([])
    expect(w.asked).toHaveLength(1)
  })

  test('no plan: the call runs as written, one ask', async ($, on) => {
    const w = callWorld(on, {})
    await start($, w.w, null, false)
    await $.tool.call({ tool: 'Bash', command: DEAD_END })
    expect(w.ran).toEqual([DEAD_END])
    expect(w.asked).toHaveLength(1)
  })

  test('the final word withheld (the control arm): the call runs as written', async ($, on) => {
    const w = callWorld(on, { plan: PLANNED })
    await start($, w.w, null, false)
    await $.tool.call({ tool: 'Bash', command: DEAD_END })
    expect(w.ran).toEqual([DEAD_END])
    expect(w.asked).toHaveLength(2)
    expect(w.notices).toEqual([])
  })

  test('inert: the call runs as written and nothing is asked', async ($, on) => {
    const w = callWorld(on, { version: '', plan: PLANNED, final: MADE })
    await start($, w.w, null, false)
    await $.tool.call({ tool: 'Bash', command: DEAD_END })
    expect(w.ran).toEqual([DEAD_END])
    expect(w.asked).toEqual([])
  })

  test('observe mode: both phases are asked with observe, and the command is never replaced', async ($, on) => {
    const w = callWorld(on, { plan: PLANNED, final: MADE, observe: '1' })
    await start($, w.w, null, false)
    await $.tool.call({ tool: 'Bash', command: DEAD_END })
    expect(w.ran).toEqual([DEAD_END])
    expect(w.asked.map(p => p.observe)).toEqual([true, true])
    expect(w.notices).toEqual([])
  })

  // Review M2 (the mod's half): a substitution the binary withholds leaves the call undecided, so
  // the interpreter rewrite planned for the same call still happens; the binary's half (no claim on
  // a withheld substitution) is crates/adage-hook's.
  const BOTH = wireWith('plan', { receipt: undefined })
  const REWRITTEN = { hookSpecificOutput: { hookEventName: 'PreToolUse', updatedInput: { command: VENV }, additionalContext: WORD } }

  test('the control arm: a withheld substitution goes on to the rewrite verdict, and its answer rewrites the call', async ($, on) => {
    const w = callWorld(on, { plan: BOTH, final: (v: Record<string, unknown>) => (v.substitute === undefined && v.engine === 'same' ? REWRITTEN : undefined) })
    await start($, w.w, null, false)
    await $.tool.call({ tool: 'Bash', command: DEAD_END })
    expect(verdicts(w.asked)).toEqual(['dry_run', JSON.stringify({ engine: 'same', substitute: SUB.route }), '{"engine":"same"}'])
    expect(w.ran).toEqual([VENV])
    // The rewrite's line under the dialog, never the substitution's.
    expect(w.notices).not.toContain(CONTEXT)
  })

  test('observe mode: after the withheld substitution the call goes on as written with no further verdict, for the command hook to rewrite', async ($, on) => {
    const w = callWorld(on, { plan: BOTH, final: (v: Record<string, unknown>) => (v.substitute === undefined ? REWRITTEN : undefined), observe: '1' })
    await start($, w.w, null, false)
    await $.tool.call({ tool: 'Bash', command: DEAD_END })
    expect(w.asked.map(p => [p.dry_run === true ? 'dry_run' : JSON.stringify(p.verdict), p.observe])).toEqual([['dry_run', true], [JSON.stringify({ engine: 'same', substitute: SUB.route }), true]])
    expect(w.ran).toEqual([DEAD_END])
  })

  test('ADAGE_MOD_OBSERVE set but empty is not observe mode', async ($, on) => {
    const w = callWorld(on, { plan: PLANNED, final: MADE, observe: '' })
    await start($, w.w, null, false)
    await $.tool.call({ tool: 'Bash', command: DEAD_END })
    expect(w.ran).toEqual([SUB.command])
  })
})

describe('T-C3-04, T-C3-07 · drawn: the receipt card, and the line beside a run that happened', () => {
  // The kit's binary prints one fixed answer per event, so the dry run and the final ask read the
  // same object: the dry run its `plan`, the verdict its `receipt` (the real binary prints one or
  // the other).
  const both = (plan: WireAnswer, final: WireAnswer = {}) => ({ PreToolUse: JSON.stringify({ ...plan, ...final }) })
  const receipt = shortId(CARD.operation)
  const texts = async ($: { ui: { mount: (i: never) => Promise<{ findAll: (q: { type: string }) => Promise<{ text: string }[]>; }> } }, surface: string, id: string) =>
    (await (await $.ui.mount({ plugin: 'adage', surface, component: 'ToolResult', props: RESULT(id) } as never)).findAll({ type: 'Text' })).map(t => t.text)

  test('answered: the ToolResult row is the card, on both surfaces', async ($, on) => {
    const w = world(on, { hook: both(RECEIPT_ONLY, ANSWERED) })
    await start($, w)
    const r = await $.tool.call({ tool: 'Bash', command: PYTEST, tool_use_id: 't1' })
    expect(r).toMatchObject({ result: { stdout: ANSWER.result } })
    for (const surface of SURFACES) {
      const t = await texts($ as never, surface, 't1')
      expect(t).toHaveLength(2)
      expect(t[0]).toContain(`adage receipt ${receipt}`)
      expect(t[0]).toContain(`the tests added in this session: ${[...CARD.tests, `and ${CARD.more} more`].join(', ')}`)
      expect(t[1]).toBe(`pytest's own output is not shown; adage answered from its run ${CARD.ago} on identical inputs (${ANSWER.covering})`)
      expect(t.join('\n')).not.toContain('ENGINE')
    }
    expect(await texts($ as never, 'terminal', 't2')).toEqual(['ENGINE ToolResult'])
  })

  test('not fresh: the call runs, and one dim line under its output says the receipt covers it; never the card', async ($, on) => {
    const w = world(on, { hook: both(wireWith('plan-advice', { advice: undefined }), ANSWERED) })
    await start($, w)
    const r = await $.tool.call({ tool: 'Bash', command: PYTEST, tool_use_id: 't1' })
    expect(r).toMatchObject({ result: { stdout: 'ok' } })
    for (const surface of SURFACES) {
      expect(await texts($ as never, surface, 't1')).toEqual(['ENGINE ToolResult', `◠ adage receipt ${receipt} covers this run · /adage why`])
    }
  })

  test('fresh but withheld: the run happens and the line beside it names the receipt', async ($, on) => {
    const w = world(on, { hook: both(RECEIPT_ONLY) })
    await start($, w)
    await $.tool.call({ tool: 'Bash', command: PYTEST, tool_use_id: 't1' })
    expect(await texts($ as never, 'terminal', 't1')).toEqual(['ENGINE ToolResult', `◠ adage receipt ${receipt} covers this run · /adage why`])
  })
})

describe('T-C3-04, T-C3-05 · bashCall alone', () => {
  const deps = (over: Partial<CallDeps>): CallDeps & { asked: PreToolPayload[] } => {
    const asked: PreToolPayload[] = []
    return {
      asked,
      isActive: true,
      isObserving: false,
      ask: async p => {
        asked.push(p)
        return { ok: true, answer: undefined }
      },
      context: async () => ({ sessionId: 'sess-1', cwd: '/w' }),
      check: async () => 'allow',
      notice: () => undefined,
      ...over,
    }
  }
  const next = async (e: { command: string }): Promise<ToolCallResult> => ({ result: { stdout: `ran ${e.command}`, stderr: '', interrupted: false } })

  test("a subagent's call carries its loop in the payload", async () => {
    const d = deps({})
    await bashCall({ command: PYTEST, tool_use_id: 't1', agentId: 'agent-7' }, next, d)
    expect(d.asked).toHaveLength(1)
    expect(d.asked[0]?.agent_id).toBe('agent-7')
  })

  test('no session context, or a command that is not a string: the call runs unread', async () => {
    const d = deps({ context: () => Promise.reject(new Error('no session')) })
    expect(await bashCall({ command: PYTEST, tool_use_id: 't1' }, next, d)).toMatchObject({ result: { stdout: `ran ${PYTEST}` } })
    expect(d.asked).toEqual([])
    const e = deps({})
    await bashCall({ command: 7 as unknown as string, tool_use_id: 't1' }, next, e)
    expect(e.asked).toEqual([])
  })

  test('a refused call keeps its refusal: no context is added', async () => {
    const d = deps({ ask: async p => ({ ok: true, answer: p.dry_run ? REWRITE_ONLY : SAME }) })
    const r = await bashCall({ command: PYTEST, tool_use_id: 't1' }, async () => ({ deny: 'no' }), d)
    expect(r).toEqual({ deny: 'no' })
  })

  test('the drawing deps may fail: the decision stands', async () => {
    const boom = () => Promise.reject(new Error('no state'))
    const d = deps({ ask: async p => ({ ok: true, answer: p.dry_run ? RECEIPT_ONLY : ANSWERED }), answered: boom, beside: boom })
    expect(await bashCall({ command: PYTEST, tool_use_id: 't1' }, next, d)).toMatchObject({ result: { stdout: ANSWER.result } })
    const s = deps({ ask: async p => ({ ok: true, answer: p.dry_run ? REWRITE_ONLY : SAME }), rewrote: boom, delivered: boom, notice: () => { throw new Error('no dialog') } })
    expect(await bashCall({ command: PYTEST, tool_use_id: 't1' }, next, s)).toMatchObject({ result: { stdout: `ran ${VENV}` }, context: [WORD] })
  })
})

describe("T-C3-04, T-C3-05, T-C6-03 · the binary's plan, read from the wire", () => {
  test('planOf reads plan.json: the replacement, the rewrite and a fresh receipt, each by its own reader', () => {
    const answer = wire('plan')
    const p = answer.plan as Record<string, Record<string, unknown>>
    const read = planOf(answer)
    expect(read).toEqual({
      substitute: { command: p.substitute?.command, route: p.substitute?.route, track: p.substitute?.track, context: p.substitute?.context },
      rewrite: { command: p.rewrite?.command, kind: p.rewrite?.kind },
      receipt: { fresh: true, covering: p.receipt?.covering, note: p.receipt?.note, result: p.receipt?.result, operation: p.receipt?.operation },
    })
    // Every field the mod acts on is there: a renamed field reads as empty and fails here.
    for (const v of [...Object.values(read?.substitute ?? {}), ...Object.values(read?.rewrite ?? {}), ...Object.values(read?.receipt ?? {})]) expect(v === undefined || v === '').toBe(false)
  })

  test("planOf reads plan-advice.json: a receipt that is not fresh (its run, no note or result); the advice is the command hook's", () => {
    const answer = wire('plan-advice')
    const r = (answer.plan as Record<string, Record<string, unknown>>).receipt
    expect(planOf(answer)).toEqual({ substitute: undefined, rewrite: undefined, receipt: { fresh: false, covering: r?.covering, note: '', result: '', operation: r?.operation } })
    expect(planOf(answer)?.receipt?.operation).not.toBe('')
    expect(planOf({})).toBeUndefined()
    expect(planOf(wireWith('plan', null))).toBeUndefined()
  })
})

describe("T-C3-05 · the binary's rewrite, read", () => {
  test('rewritePlanOf and verdictAnswerOf: the command the call runs as, and what the model reads beside it', () => {
    expect(rewritePlanOf(REWRITE)).toEqual(REWRITE)
    expect(rewritePlanOf({ command: ' ' })).toBeUndefined()
    expect(rewritePlanOf({ command: VENV })).toEqual({ command: VENV, kind: '' })
    expect(verdictAnswerOf(SAME)).toEqual({ command: VENV, context: [WORD] })
    expect(verdictAnswerOf(DIFFERS)).toEqual({ command: undefined, context: [ADVICE] })
    expect(verdictAnswerOf({ hookSpecificOutput: { updatedInput: { command: ' ' } } })).toBeUndefined()
    expect(verdictAnswerOf({ systemMessage: 'x' })).toBeUndefined()
    expect(verdictAnswerOf(undefined)).toBeUndefined()
  })
})

describe('T-C4-01 · a call Adage made in the model\'s place says so in its result', () => {
  // §3.1–3.2: Adage's act is never counted as the agent's. Live, after a predicted `make gen`, the
  // model apologised for having run it: the result now carries one plain line saying whose call it was.
  const GEN = toolUseId('finish-line-e7e809', 'turn-1', 2, 0)
  const READ = toolUseId('read-linked-files', 'turn-1', 1, 0)
  const LINE = (route: string) =>
    `Adage made this call in your place (learned reflex ${route}); the result above is its output. You did not make it, and it does not need repeating.`

  test('a predicted Bash call runs as decided, and the line follows its result', async ($, on) => {
    const w = callWorld(on, {})
    await start($, w.w, null, false)
    const r = await $.tool.call({ tool: 'Bash', command: 'make gen', tool_use_id: GEN })
    expect(w.ran).toEqual(['make gen'])
    expect(r.context).toEqual([LINE('finish-line-e7e809')])
  })

  test('a predicted Read (the chain) carries it too; the model\'s own calls carry nothing', async ($, on) => {
    const w = callWorld(on, {})
    await start($, w.w, null, false)
    expect((await $.tool.call({ tool: 'Read', file_path: '/w/acme/billing.py', tool_use_id: READ })).context).toEqual([LINE('read-linked-files')])
    expect((await $.tool.call({ tool: 'Read', file_path: '/w/acme/billing.py', tool_use_id: 'toolu_01abc' })).context).toBeUndefined()
    expect((await $.tool.call({ tool: 'Bash', command: 'make gen', tool_use_id: 'toolu_01def' })).context).toBeUndefined()
  })

  test('the line is read from the id alone: only toolUseId\'s exact shape names a route', () => {
    expect(predictedRoute(GEN)).toBe('finish-line-e7e809')
    expect(madeForAgentLine(READ)).toBe(LINE('read-linked-files'))
    for (const id of ['toolu_01abc', 'adage_x', 'adage_a_b_c_0', 'xadage_a_b_1_0', `${GEN}_9`]) expect(madeForAgentLine(id)).toBeUndefined()
  })
})
