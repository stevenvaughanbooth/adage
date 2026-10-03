// A Bash call through the module as the engine loads it (plan C3; T-C3-04, T-C3-05, T-C3-07): the
// binary answered beneath the plugin by the test's `process.run` hook (its plan on `dry_run`, its
// final word on `verdict`), the person's rules by its `tool.check` hook, and the Bash tool by its
// `tool.call` hook (nothing runs). The readers of the binary's answers are checked alone, and the
// receipt card and the beside line through the mounted `ToolResult` row.
import type { On, ToolCallResult } from 'claude-code'
import { describe, expect, test } from 'claude-code/testing'

import { bashCall, rewritePlanOf, verdictAnswerOf } from './call'
import type { CallDeps, PreToolPayload } from './call'
import { RESULT, SURFACES, start, world as kit } from './fixtures/kit'
import { answeredMark, receiptAnswerOf, receiptPlanOf, shortId } from './receipt'

const PYTEST = 'python -m pytest tests/test_x.py -q'
const VENV = '.venv/bin/python -m pytest tests/test_x.py -q'
const NOTE = '<adage-receipt operation="3f9c1a2b7d4e">\nAdage\'s run of `' + VENV + '` in this session passed on files identical to the current ones.\n</adage-receipt>'
const RESULT_TEXT = 'Adage answered this test run from its own run of `' + VENV + '` in this session (2026-10-03T09:00:00Z, operation 3f9c1a2b7d4e): it passed on files identical to the current ones.'
const CARD = { operation: '3f9c1a2b7d4e', ran_at: '2026-10-03T09:00:00Z', ago: '4 min ago', summary: 'passed on files identical to the current ones · 2 tests this command selects', tests: ['tests/test_x.py::test_zeta', 'tests/test_x.py::test_alpha'], more: 0 }
const FRESH = { fresh: true, covering: VENV, operation: '3f9c1a2b7d4e', note: NOTE, result: RESULT_TEXT }
const STALE = { fresh: false, covering: VENV, operation: '3f9c1a2b7d4e' }
const ANSWERED = { receipt: { result: RESULT_TEXT, note: NOTE, covering: VENV, card: CARD } }
const REWRITE = { command: VENV, kind: 'test' }
const WORD = "Adage pointed this test run at the project's environment (.venv/bin/python)."
const SAME = { systemMessage: 'adage: pointed this test run at .venv/bin/python', hookSpecificOutput: { hookEventName: 'PreToolUse', updatedInput: { command: VENV }, additionalContext: WORD } }
const ADVICE = 'Adage: this test run uses the system Python; the project\'s environment is .venv/bin/python (Adage did not rewrite the command: the rules treat the two differently).'
const DIFFERS = { hookSpecificOutput: { hookEventName: 'PreToolUse', additionalContext: ADVICE } }
const START = { cwd: '/w', surface: null, isInteractive: false } as const

type Opts = {
  version?: string
  plan?: unknown
  /** The final word, by the verdict the mod sent (`answered`, `same`, `differs`). */
  final?: (verdict: Record<string, unknown>) => unknown
  rules?: Record<string, 'allow' | 'ask' | 'deny'>
  observe?: string
  /** How the binary fails on the dry run: garbage, a non-zero exit, or a run that cannot start. */
  fail?: 'garbage' | 'exit' | 'throw'
  /** The rules cannot be read. */
  checkThrows?: boolean
}

/** The world beneath the plugin; returns what the binary was asked, what ran, the checks and the notices. */
function world(on: On, o: Opts) {
  const env: Record<string, string> = { CLAUDE_PLUGIN_DATA: '/data' }
  if (o.observe !== undefined) env.ADAGE_MOD_OBSERVE = o.observe
  on('env.get', ($, e) => ({ value: env[e.name] }))
  const asked: Record<string, unknown>[] = []
  on('process.run', ($, e) => {
    const out = (stdout: string, exitCode = 0) => ({ value: { exitCode, stdout, stderr: '', isStdoutTruncated: false, isStderrTruncated: false } })
    if (e.argv[1] === 'version') return out(o.version ?? 'adage 0.1.0\n')
    const p = JSON.parse(e.init?.stdin ?? '{}') as Record<string, unknown>
    if (p.hook_event_name !== 'PreToolUse') return out('')
    asked.push(p)
    if (p.dry_run === true) {
      if (o.fail === 'garbage') return out('{')
      if (o.fail === 'exit') return out('', 1)
      if (o.fail === 'throw') return { deny: 'ENOENT' }
      return out(o.plan === undefined ? '' : JSON.stringify(o.plan))
    }
    const answer = o.final?.(p.verdict as Record<string, unknown>)
    return out(answer === undefined ? '' : JSON.stringify(answer))
  })
  on('session.id', () => ({ value: 'sess-1' }))
  on('session.cwd', () => ({ value: '/w' }))
  on('session.start', ($, e) => ({ cwd: e.cwd }))
  const checked: string[] = []
  on('tool.check', ($, e) => {
    const command = (e.input as { command: string }).command
    checked.push(command)
    if (o.checkThrows) throw new Error('no rules')
    return { decision: o.rules?.[command] ?? 'allow' }
  })
  const notices: string[] = []
  on('ui.notice', ($, e) => {
    notices.push(e.text ?? '')
    return { value: undefined }
  })
  const ran: string[] = []
  on('tool.call', { tool: 'Bash' }, ($, e) => {
    ran.push(e.command)
    return { result: { stdout: `ran ${e.command}`, stderr: '', interrupted: false } }
  })
  return { asked, ran, checked, notices }
}

const verdicts = (asked: readonly Record<string, unknown>[]) => asked.map(p => (p.dry_run === true ? 'dry_run' : JSON.stringify(p.verdict)))

describe('T-C3-04 · the receipt as the result, only on allow', () => {
  test('allow: the receipt is the result, the note its context, nothing runs, two asks and one check', async ($, on) => {
    const w = world(on, { plan: { plan: { receipt: FRESH } }, final: v => (v.answered === 'receipt' ? ANSWERED : undefined) })
    await $.session.start(START)
    const r = await $.tool.call({ tool: 'Bash', command: PYTEST, tool_use_id: 't1' })
    expect(r).toEqual({ result: { stdout: RESULT_TEXT, stderr: '', interrupted: false }, context: [NOTE] })
    expect(w.ran).toEqual([])
    expect(verdicts(w.asked)).toEqual(['dry_run', '{"answered":"receipt"}'])
    expect(w.asked[0]).toMatchObject({ session_id: 'sess-1', cwd: '/w', hook_event_name: 'PreToolUse', tool_name: 'Bash', tool_input: { command: PYTEST }, tool_use_id: 't1', via: 'mod' })
    expect(w.asked.every(p => p.observe === undefined && p.agent_id === undefined)).toBe(true)
    expect(w.checked).toEqual([PYTEST])
    expect(w.notices).toEqual([])
  })

  for (const decision of ['ask', 'deny'] as const) {
    test(`${decision}: the call runs as written after one ask; the receipt is never asked for`, async ($, on) => {
      const w = world(on, { plan: { plan: { receipt: FRESH } }, final: () => ANSWERED, rules: { [PYTEST]: decision } })
      await $.session.start(START)
      const r = await $.tool.call({ tool: 'Bash', command: PYTEST, tool_use_id: 't1' })
      expect(w.ran).toEqual([PYTEST])
      expect(r.context).toBeUndefined()
      expect(verdicts(w.asked)).toEqual(['dry_run'])
    })
  }

  test('ask, with a rewrite also planned: the rewrite path is taken (a second ask with the engine verdict)', async ($, on) => {
    const w = world(on, { plan: { plan: { receipt: FRESH, rewrite: REWRITE } }, final: v => (v.engine === 'same' ? SAME : undefined), rules: { [PYTEST]: 'ask', [VENV]: 'ask' } })
    await $.session.start(START)
    await $.tool.call({ tool: 'Bash', command: PYTEST, tool_use_id: 't1' })
    expect(w.ran).toEqual([VENV])
    expect(verdicts(w.asked)).toEqual(['dry_run', '{"engine":"same"}'])
  })

  test('the final word withheld (the control arm, or no longer fresh): the call runs as written', async ($, on) => {
    const w = world(on, { plan: { plan: { receipt: FRESH } } })
    await $.session.start(START)
    const r = await $.tool.call({ tool: 'Bash', command: PYTEST, tool_use_id: 't1' })
    expect(w.ran).toEqual([PYTEST])
    expect(r.context).toBeUndefined()
    expect(verdicts(w.asked)).toEqual(['dry_run', '{"answered":"receipt"}'])
  })

  test('a fresh receipt without a result (a host that cannot answer calls) runs the call', async ($, on) => {
    const w = world(on, { plan: { plan: { receipt: { ...FRESH, result: undefined } } }, final: () => ANSWERED })
    await $.session.start(START)
    await $.tool.call({ tool: 'Bash', command: PYTEST, tool_use_id: 't1' })
    expect(w.ran).toEqual([PYTEST])
    expect(verdicts(w.asked)).toEqual(['dry_run'])
  })

  test('observe mode: the plan is asked with observe, and the receipt never answers', async ($, on) => {
    const w = world(on, { plan: { plan: { receipt: FRESH } }, final: () => ANSWERED, observe: '1' })
    await $.session.start(START)
    await $.tool.call({ tool: 'Bash', command: PYTEST, tool_use_id: 't1' })
    expect(w.ran).toEqual([PYTEST])
    expect(w.asked.map(p => [p.dry_run, p.observe])).toEqual([[true, true]])
  })
})

describe('T-C3-05 · the rewrite checked by the engine', () => {
  test("same: the final ask carries engine same, the call runs rewritten with the binary's word, one notice", async ($, on) => {
    const w = world(on, { plan: { plan: { rewrite: REWRITE } }, final: v => (v.engine === 'same' ? SAME : undefined) })
    await $.session.start(START)
    const r = await $.tool.call({ tool: 'Bash', command: PYTEST, tool_use_id: 't1' })
    expect(w.ran).toEqual([VENV])
    expect(r).toMatchObject({ result: { stdout: `ran ${VENV}` }, context: [WORD] })
    expect(verdicts(w.asked)).toEqual(['dry_run', '{"engine":"same"}'])
    expect(w.checked.sort()).toEqual([VENV, PYTEST].sort())
    expect(w.notices).toEqual(['↪ adage: pointed this test run at .venv/bin/python'])
  })

  test('differs: the final ask carries engine differs, the call runs as written, the advice is its context, no notice', async ($, on) => {
    const w = world(on, { plan: { plan: { rewrite: REWRITE } }, final: v => (v.engine === 'differs' ? DIFFERS : SAME), rules: { [PYTEST]: 'allow', [VENV]: 'ask' } })
    await $.session.start(START)
    const r = await $.tool.call({ tool: 'Bash', command: PYTEST, tool_use_id: 't1' })
    expect(w.ran).toEqual([PYTEST])
    expect(r).toMatchObject({ result: { stdout: `ran ${PYTEST}` }, context: [ADVICE] })
    expect(verdicts(w.asked)).toEqual(['dry_run', '{"engine":"differs"}'])
    expect(w.notices).toEqual([])
  })

  test('advice alone in the plan: the call runs as written after one ask (the command hook delivers it)', async ($, on) => {
    const w = world(on, { plan: { plan: { advice: { kind: 'pip-install', text: 'x' } } }, final: () => SAME })
    await $.session.start(START)
    const r = await $.tool.call({ tool: 'Bash', command: PYTEST, tool_use_id: 't1' })
    expect(w.ran).toEqual([PYTEST])
    expect(r.context).toBeUndefined()
    expect(verdicts(w.asked)).toEqual(['dry_run'])
    expect(w.checked).toEqual([])
  })

  test('the binary downgraded a same verdict to advice: the call runs as written with the advice', async ($, on) => {
    const w = world(on, { plan: { plan: { rewrite: REWRITE } }, final: () => DIFFERS })
    await $.session.start(START)
    const r = await $.tool.call({ tool: 'Bash', command: PYTEST, tool_use_id: 't1' })
    expect(w.ran).toEqual([PYTEST])
    expect(r.context).toEqual([ADVICE])
    expect(w.notices).toEqual([])
  })

  test('observe mode: the rewrite is never made and the engine is not asked for a verdict', async ($, on) => {
    const w = world(on, { plan: { plan: { rewrite: REWRITE } }, final: () => SAME, observe: '1' })
    await $.session.start(START)
    await $.tool.call({ tool: 'Bash', command: PYTEST, tool_use_id: 't1' })
    expect(w.ran).toEqual([PYTEST])
    expect(verdicts(w.asked)).toEqual(['dry_run'])
  })
})

describe('T-X-02 · every path fails open', () => {
  for (const fail of ['garbage', 'exit', 'throw'] as const) {
    test(`a dry run that answers ${fail}: the call runs as written, nothing else is asked`, async ($, on) => {
      const w = world(on, { plan: { plan: { receipt: FRESH, rewrite: REWRITE } }, final: () => ANSWERED, fail })
      await $.session.start(START)
      const r = await $.tool.call({ tool: 'Bash', command: PYTEST, tool_use_id: 't1' })
      expect(w.ran).toEqual([PYTEST])
      expect(r.context).toBeUndefined()
      expect(w.asked).toHaveLength(1)
      expect(w.checked).toEqual([])
    })
  }

  test('the rules cannot be read: no receipt answers and no rewrite is made', async ($, on) => {
    const w = world(on, { plan: { plan: { receipt: FRESH, rewrite: REWRITE } }, final: () => ANSWERED, checkThrows: true })
    await $.session.start(START)
    await $.tool.call({ tool: 'Bash', command: PYTEST, tool_use_id: 't1' })
    expect(w.ran).toEqual([PYTEST])
    expect(verdicts(w.asked)).toEqual(['dry_run'])
  })

  test('a malformed final answer runs the call as written', async ($, on) => {
    const w = world(on, { plan: { plan: { receipt: FRESH } }, final: () => ({ receipt: { result: '' } }) })
    await $.session.start(START)
    const r = await $.tool.call({ tool: 'Bash', command: PYTEST, tool_use_id: 't1' })
    expect(w.ran).toEqual([PYTEST])
    expect(r.context).toBeUndefined()
  })

  test('inert: the call runs as written and nothing is asked or checked', async ($, on) => {
    const w = world(on, { version: '', plan: { plan: { receipt: FRESH } }, final: () => ANSWERED })
    await $.session.start(START)
    await $.tool.call({ tool: 'Bash', command: PYTEST, tool_use_id: 't1' })
    expect(w.ran).toEqual([PYTEST])
    expect(w.asked).toEqual([])
    expect(w.checked).toEqual([])
  })
})

describe('drawn: the receipt card, and the line beside a run that happened (T-C3-04, T-C3-07)', () => {
  // The kit's binary prints one fixed answer per event, so the dry run and the final ask read the
  // same object: the dry run its `plan`, the verdict its `receipt` (the real binary prints one or
  // the other).
  const both = (plan: unknown, final: Record<string, unknown> = {}) => ({ PreToolUse: JSON.stringify({ plan, ...final }) })
  const texts = async ($: { ui: { mount: (i: never) => Promise<{ findAll: (q: { type: string }) => Promise<{ text: string }[]>; }> } }, surface: string, id: string) =>
    (await (await $.ui.mount({ plugin: 'adage', surface, component: 'ToolResult', props: RESULT(id) } as never)).findAll({ type: 'Text' })).map(t => t.text)

  test('answered: the ToolResult row is the card, on both surfaces', async ($, on) => {
    on('tool.check', () => ({ decision: 'allow' }))
    const w = kit(on, { hook: both({ receipt: FRESH }, ANSWERED) })
    await start($, w)
    const r = await $.tool.call({ tool: 'Bash', command: PYTEST, tool_use_id: 't1' })
    expect(r).toMatchObject({ result: { stdout: RESULT_TEXT } })
    for (const surface of SURFACES) {
      const t = await texts($ as never, surface, 't1')
      expect(t).toHaveLength(2)
      expect(t[0]).toContain('adage receipt 3f9c')
      expect(t[0]).toContain('the tests added in this session: tests/test_x.py::test_zeta, tests/test_x.py::test_alpha')
      expect(t[1]).toBe(`pytest's own output is not shown; adage answered from its run 4 min ago on identical inputs (${VENV})`)
      expect(t.join('\n')).not.toContain('ENGINE')
    }
    expect(await texts($ as never, 'terminal', 't2')).toEqual(['ENGINE ToolResult'])
  })

  test('not fresh: the call runs, and one dim line under its output says the receipt covers it; never the card', async ($, on) => {
    on('tool.check', () => ({ decision: 'allow' }))
    const w = kit(on, { hook: both({ receipt: STALE }, ANSWERED) })
    await start($, w)
    const r = await $.tool.call({ tool: 'Bash', command: PYTEST, tool_use_id: 't1' })
    expect(r).toMatchObject({ result: { stdout: 'ok' } })
    for (const surface of SURFACES) {
      expect(await texts($ as never, surface, 't1')).toEqual(['ENGINE ToolResult', '◠ adage receipt 3f9c covers this run · /adage why'])
    }
  })

  test('fresh but withheld: the run happens and the line beside it names the receipt', async ($, on) => {
    on('tool.check', () => ({ decision: 'allow' }))
    const w = kit(on, { hook: both({ receipt: FRESH }) })
    await start($, w)
    await $.tool.call({ tool: 'Bash', command: PYTEST, tool_use_id: 't1' })
    expect(await texts($ as never, 'terminal', 't1')).toEqual(['ENGINE ToolResult', '◠ adage receipt 3f9c covers this run · /adage why'])
  })
})

describe('the hook body alone', () => {
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
    const d = deps({ ask: async p => ({ ok: true, answer: p.dry_run ? { plan: { rewrite: REWRITE } } : SAME }) })
    const r = await bashCall({ command: PYTEST, tool_use_id: 't1' }, async () => ({ deny: 'no' }), d)
    expect(r).toEqual({ deny: 'no' })
  })

  test('the drawing deps may fail: the decision stands', async () => {
    const boom = () => Promise.reject(new Error('no state'))
    const d = deps({ ask: async p => ({ ok: true, answer: p.dry_run ? { plan: { receipt: FRESH } } : ANSWERED }), answered: boom, beside: boom })
    expect(await bashCall({ command: PYTEST, tool_use_id: 't1' }, next, d)).toMatchObject({ result: { stdout: RESULT_TEXT } })
    const s = deps({ ask: async p => ({ ok: true, answer: p.dry_run ? { plan: { rewrite: REWRITE } } : SAME }), rewrote: boom, delivered: boom, notice: () => { throw new Error('no dialog') } })
    expect(await bashCall({ command: PYTEST, tool_use_id: 't1' }, next, s)).toMatchObject({ result: { stdout: `ran ${VENV}` }, context: [WORD] })
  })
})

describe("the binary's answers, read", () => {
  test('receiptPlanOf: fresh carries the note and the result; stale only the command and the run', () => {
    expect(receiptPlanOf(FRESH)).toEqual(FRESH)
    expect(receiptPlanOf(STALE)).toEqual({ ...STALE, note: '', result: '' })
    expect(receiptPlanOf({ ...STALE, note: NOTE, result: RESULT_TEXT })).toEqual({ ...STALE, note: '', result: '' })
    expect(receiptPlanOf({ fresh: 'yes' })).toBeUndefined()
    expect(receiptPlanOf(undefined)).toBeUndefined()
    expect(receiptPlanOf({ fresh: true })).toEqual({ fresh: true, covering: '', note: '', result: '', operation: '' })
  })

  test('receiptAnswerOf and the answered mark: the card as the row draws it, and more as a last name', () => {
    expect(receiptAnswerOf(ANSWERED)).toEqual({ result: RESULT_TEXT, note: NOTE, covering: VENV, card: { operation: '3f9c1a2b7d4e', ranAt: '2026-10-03T09:00:00Z', ago: '4 min ago', summary: CARD.summary, tests: CARD.tests, more: 0 } })
    expect(receiptAnswerOf({ receipt: { result: ' ' } })).toBeUndefined()
    expect(receiptAnswerOf({})).toBeUndefined()
    expect(receiptAnswerOf({ receipt: { result: 'r' } })).toEqual({ result: 'r', note: '', covering: '', card: undefined })
    const a = receiptAnswerOf({ receipt: { ...ANSWERED.receipt, card: { ...CARD, more: 3, tests: ['a', 7] } } })
    expect(answeredMark('t1', a!)).toEqual({ id: 't1', kind: 'answered', receipt: '3f9c', summary: CARD.summary, tests: ['a', 'and 3 more'], ago: '4 min ago', command: VENV })
    expect(answeredMark('t1', receiptAnswerOf({ receipt: { result: 'r' } })!)).toBeUndefined()
    expect(answeredMark('t1', receiptAnswerOf({ receipt: { result: 'r', card: { operation: '' } } })!)).toBeUndefined()
    expect(shortId('3f9c1a2b7d4e')).toBe('3f9c')
  })

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
