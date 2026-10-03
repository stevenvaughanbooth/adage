// A Bash call (call.ts; plan C3, C6; T-C3-04, T-C3-05, T-C3-07, T-C6-03, T-X-02): through the
// module as the engine loads it, the binary answered beneath the plugin by the kit (its plan on
// `dry_run`, its final word on `verdict`), the person's rules by its `tool.check` hook and the Bash
// tool by its `tool.call` hook (nothing runs); `bashCall` alone over plain deps; the receipt card
// and the beside line through the mounted `ToolResult` row.
import type { On, ToolCallResult } from 'claude-code'
import { describe, expect, test } from 'claude-code/testing'

import { bashCall, rewritePlanOf, verdictAnswerOf } from './call'
import type { CallDeps, PreToolPayload } from './call'
import { ANSWERED, FAIL, FRESH, PYTEST, RECEIPT_NOTE, RECEIPT_RESULT, RESULT, STALE, SURFACES, VENV, start, world } from './fixtures/kit'

const REWRITE = { command: VENV, kind: 'test' }
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
    const w = callWorld(on, { plan: { plan: { receipt: FRESH } }, final: (v: Record<string, unknown>) => (v.answered === 'receipt' ? ANSWERED : undefined) })
    await start($, w.w, null, false)
    const r = await $.tool.call({ tool: 'Bash', command: PYTEST, tool_use_id: 't1' })
    expect(r).toEqual({ result: { stdout: RECEIPT_RESULT, stderr: '', interrupted: false }, context: [RECEIPT_NOTE] })
    expect(w.ran).toEqual([])
    expect(verdicts(w.asked)).toEqual(['dry_run', '{"answered":"receipt"}'])
    expect(w.asked[0]).toMatchObject({ session_id: 'sess-1', cwd: '/w', hook_event_name: 'PreToolUse', tool_name: 'Bash', tool_input: { command: PYTEST }, tool_use_id: 't1', via: 'mod' })
    expect(w.asked.every(p => p.observe === undefined && p.agent_id === undefined)).toBe(true)
    expect(w.checked).toEqual([PYTEST])
    expect(w.notices).toEqual([])
  })

  for (const decision of ['ask', 'deny'] as const) {
    test(`${decision}: the call runs as written after one ask; the receipt is never asked for`, async ($, on) => {
      const w = callWorld(on, { plan: { plan: { receipt: FRESH } }, final: () => ANSWERED, rules: { [PYTEST]: decision } })
      await start($, w.w, null, false)
      const r = await $.tool.call({ tool: 'Bash', command: PYTEST, tool_use_id: 't1' })
      expect(w.ran).toEqual([PYTEST])
      expect(r.context).toBeUndefined()
      expect(verdicts(w.asked)).toEqual(['dry_run'])
    })
  }

  test('ask, with a rewrite also planned: the rewrite path is taken (a second ask with the engine verdict)', async ($, on) => {
    const w = callWorld(on, { plan: { plan: { receipt: FRESH, rewrite: REWRITE } }, final: (v: Record<string, unknown>) => (v.engine === 'same' ? SAME : undefined), rules: { [PYTEST]: 'ask', [VENV]: 'ask' } })
    await start($, w.w, null, false)
    await $.tool.call({ tool: 'Bash', command: PYTEST, tool_use_id: 't1' })
    expect(w.ran).toEqual([VENV])
    expect(verdicts(w.asked)).toEqual(['dry_run', '{"engine":"same"}'])
  })

  test('the final word withheld (the control arm, or no longer fresh): the call runs as written', async ($, on) => {
    const w = callWorld(on, { plan: { plan: { receipt: FRESH } } })
    await start($, w.w, null, false)
    const r = await $.tool.call({ tool: 'Bash', command: PYTEST, tool_use_id: 't1' })
    expect(w.ran).toEqual([PYTEST])
    expect(r.context).toBeUndefined()
    expect(verdicts(w.asked)).toEqual(['dry_run', '{"answered":"receipt"}'])
  })

  test('a fresh receipt without a result (a host that cannot answer calls) runs the call', async ($, on) => {
    const w = callWorld(on, { plan: { plan: { receipt: { ...FRESH, result: undefined } } }, final: () => ANSWERED })
    await start($, w.w, null, false)
    await $.tool.call({ tool: 'Bash', command: PYTEST, tool_use_id: 't1' })
    expect(w.ran).toEqual([PYTEST])
    expect(verdicts(w.asked)).toEqual(['dry_run'])
  })

  test('observe mode: the plan is asked with observe, and the receipt never answers', async ($, on) => {
    const w = callWorld(on, { plan: { plan: { receipt: FRESH } }, final: () => ANSWERED, observe: '1' })
    await start($, w.w, null, false)
    await $.tool.call({ tool: 'Bash', command: PYTEST, tool_use_id: 't1' })
    expect(w.ran).toEqual([PYTEST])
    expect(w.asked.map(p => [p.dry_run, p.observe])).toEqual([[true, true]])
  })
})

describe('T-C3-05 · the rewrite checked by the engine', () => {
  test("same: the final ask carries engine same, the call runs rewritten with the binary's word, one notice", async ($, on) => {
    const w = callWorld(on, { plan: { plan: { rewrite: REWRITE } }, final: (v: Record<string, unknown>) => (v.engine === 'same' ? SAME : undefined) })
    await start($, w.w, null, false)
    const r = await $.tool.call({ tool: 'Bash', command: PYTEST, tool_use_id: 't1' })
    expect(w.ran).toEqual([VENV])
    expect(r).toMatchObject({ result: { stdout: `ran ${VENV}` }, context: [WORD] })
    expect(verdicts(w.asked)).toEqual(['dry_run', '{"engine":"same"}'])
    expect(w.checked.sort()).toEqual([VENV, PYTEST].sort())
    expect(w.notices).toEqual(['↪ adage: pointed this test run at .venv/bin/python'])
  })

  test('differs: the final ask carries engine differs, the call runs as written, the advice is its context, no notice', async ($, on) => {
    const w = callWorld(on, { plan: { plan: { rewrite: REWRITE } }, final: (v: Record<string, unknown>) => (v.engine === 'differs' ? DIFFERS : SAME), rules: { [PYTEST]: 'allow', [VENV]: 'ask' } })
    await start($, w.w, null, false)
    const r = await $.tool.call({ tool: 'Bash', command: PYTEST, tool_use_id: 't1' })
    expect(w.ran).toEqual([PYTEST])
    expect(r).toMatchObject({ result: { stdout: `ran ${PYTEST}` }, context: [ADVICE] })
    expect(verdicts(w.asked)).toEqual(['dry_run', '{"engine":"differs"}'])
    expect(w.notices).toEqual([])
  })

  test('advice alone in the plan: the call runs as written after one ask (the command hook delivers it)', async ($, on) => {
    const w = callWorld(on, { plan: { plan: { advice: { kind: 'pip-install', text: 'x' } } }, final: () => SAME })
    await start($, w.w, null, false)
    const r = await $.tool.call({ tool: 'Bash', command: PYTEST, tool_use_id: 't1' })
    expect(w.ran).toEqual([PYTEST])
    expect(r.context).toBeUndefined()
    expect(verdicts(w.asked)).toEqual(['dry_run'])
    expect(w.checked).toEqual([])
  })

  test('the binary downgraded a same verdict to advice: the call runs as written with the advice', async ($, on) => {
    const w = callWorld(on, { plan: { plan: { rewrite: REWRITE } }, final: () => DIFFERS })
    await start($, w.w, null, false)
    const r = await $.tool.call({ tool: 'Bash', command: PYTEST, tool_use_id: 't1' })
    expect(w.ran).toEqual([PYTEST])
    expect(r.context).toEqual([ADVICE])
    expect(w.notices).toEqual([])
  })

  test('observe mode: the rewrite is never made and the engine is not asked for a verdict', async ($, on) => {
    const w = callWorld(on, { plan: { plan: { rewrite: REWRITE } }, final: () => SAME, observe: '1' })
    await start($, w.w, null, false)
    await $.tool.call({ tool: 'Bash', command: PYTEST, tool_use_id: 't1' })
    expect(w.ran).toEqual([PYTEST])
    expect(verdicts(w.asked)).toEqual(['dry_run'])
  })
})

describe('T-X-02 · every path fails open', () => {
  for (const fail of ['garbage', 'exit', 'throw'] as const) {
    test(`a dry run that answers ${fail}: the call runs as written, nothing else is asked`, async ($, on) => {
      const w = callWorld(on, { plan: { plan: { receipt: FRESH, rewrite: REWRITE } }, final: () => ANSWERED, fail })
      await start($, w.w, null, false)
      const r = await $.tool.call({ tool: 'Bash', command: PYTEST, tool_use_id: 't1' })
      expect(w.ran).toEqual([PYTEST])
      expect(r.context).toBeUndefined()
      expect(w.asked).toHaveLength(1)
      expect(w.checked).toEqual([])
    })
  }

  test('the rules cannot be read: no receipt answers and no rewrite is made', async ($, on) => {
    const w = callWorld(on, { plan: { plan: { receipt: FRESH, rewrite: REWRITE } }, final: () => ANSWERED, checkThrows: true })
    await start($, w.w, null, false)
    await $.tool.call({ tool: 'Bash', command: PYTEST, tool_use_id: 't1' })
    expect(w.ran).toEqual([PYTEST])
    expect(verdicts(w.asked)).toEqual(['dry_run'])
  })

  test('a malformed final answer runs the call as written', async ($, on) => {
    const w = callWorld(on, { plan: { plan: { receipt: FRESH } }, final: () => ({ receipt: { result: '' } }) })
    await start($, w.w, null, false)
    const r = await $.tool.call({ tool: 'Bash', command: PYTEST, tool_use_id: 't1' })
    expect(w.ran).toEqual([PYTEST])
    expect(r.context).toBeUndefined()
  })

  test('inert: the call runs as written and nothing is asked or checked', async ($, on) => {
    const w = callWorld(on, { version: '', plan: { plan: { receipt: FRESH } }, final: () => ANSWERED })
    await start($, w.w, null, false)
    await $.tool.call({ tool: 'Bash', command: PYTEST, tool_use_id: 't1' })
    expect(w.ran).toEqual([PYTEST])
    expect(w.asked).toEqual([])
    expect(w.checked).toEqual([])
  })
})

const DEAD_END = 'python3 -m pytest tests'
const CONTEXT = 'adage ran make test in place of python3 -m pytest tests: in this repository it fails ("No module named pytest").'
const SUB = { command: 'make test', route: 'dead-end-pytest', track: 'acme/billing@1', context: CONTEXT }

describe('T-C6-03 · the dead end that never runs', () => {
  const PLANNED = { plan: { substitute: SUB } }
  const MADE = { substitute: SUB }

  test('the same verdict for both commands: the learned command runs, with one line for the model and the person', async ($, on) => {
    const w = callWorld(on, { plan: PLANNED, final: MADE })
    await start($, w.w, null, false)
    const r = await $.tool.call({ tool: 'Bash', command: DEAD_END })
    expect(w.ran).toEqual(['make test'])
    expect(r.context).toEqual([CONTEXT])
    expect(w.notices).toEqual([CONTEXT])
    expect(w.asked.map(p => [p.hook_event_name, p.dry_run, p.verdict])).toEqual([
      ['PreToolUse', true, undefined],
      ['PreToolUse', undefined, { engine: 'same', substitute: 'dead-end-pytest' }],
    ])
    expect(w.asked[0]).toMatchObject({ session_id: 'sess-1', cwd: '/w', tool_name: 'Bash', tool_input: { command: DEAD_END } })
    expect(w.asked.every(p => p.observe === undefined)).toBe(true)
  })

  test('different verdicts: the call runs as written, and the binary is not asked again', async ($, on) => {
    const w = callWorld(on, { plan: PLANNED, final: MADE, rules: { [DEAD_END]: 'allow', 'make test': 'ask' } })
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
  const VENV_TESTS = '.venv/bin/python -m pytest tests'
  const BOTH = { plan: { substitute: SUB, rewrite: { command: VENV_TESTS, kind: 'test' } } }
  const REWRITTEN = { hookSpecificOutput: { hookEventName: 'PreToolUse', updatedInput: { command: VENV_TESTS }, additionalContext: 'Adage pointed this test run at the project\'s environment (.venv/bin/python).' } }

  test('the control arm: a withheld substitution goes on to the rewrite verdict, and its answer rewrites the call', async ($, on) => {
    const w = callWorld(on, { plan: BOTH, final: (v: Record<string, unknown>) => (v.substitute === undefined && v.engine === 'same' ? REWRITTEN : undefined) })
    await start($, w.w, null, false)
    await $.tool.call({ tool: 'Bash', command: DEAD_END })
    expect(w.asked.map(p => p.dry_run === true ? 'dry_run' : JSON.stringify(p.verdict))).toEqual(['dry_run', '{"engine":"same","substitute":"dead-end-pytest"}', '{"engine":"same"}'])
    expect(w.ran).toEqual([VENV_TESTS])
    // The rewrite's line under the dialog, never the substitution's.
    expect(w.notices).not.toContain(CONTEXT)
  })

  test('observe mode: after the withheld substitution the call goes on as written with no further verdict, for the command hook to rewrite', async ($, on) => {
    const w = callWorld(on, { plan: BOTH, final: (v: Record<string, unknown>) => (v.substitute === undefined ? REWRITTEN : undefined), observe: '1' })
    await start($, w.w, null, false)
    await $.tool.call({ tool: 'Bash', command: DEAD_END })
    expect(w.asked.map(p => [p.dry_run === true ? 'dry_run' : JSON.stringify(p.verdict), p.observe])).toEqual([['dry_run', true], ['{"engine":"same","substitute":"dead-end-pytest"}', true]])
    expect(w.ran).toEqual([DEAD_END])
  })

  test('ADAGE_MOD_OBSERVE set but empty is not observe mode', async ($, on) => {
    const w = callWorld(on, { plan: PLANNED, final: MADE, observe: '' })
    await start($, w.w, null, false)
    await $.tool.call({ tool: 'Bash', command: DEAD_END })
    expect(w.ran).toEqual(['make test'])
  })
})

describe('T-C3-04, T-C3-07 · drawn: the receipt card, and the line beside a run that happened', () => {
  // The kit's binary prints one fixed answer per event, so the dry run and the final ask read the
  // same object: the dry run its `plan`, the verdict its `receipt` (the real binary prints one or
  // the other).
  const both = (plan: unknown, final: Record<string, unknown> = {}) => ({ PreToolUse: JSON.stringify({ plan, ...final }) })
  const texts = async ($: { ui: { mount: (i: never) => Promise<{ findAll: (q: { type: string }) => Promise<{ text: string }[]>; }> } }, surface: string, id: string) =>
    (await (await $.ui.mount({ plugin: 'adage', surface, component: 'ToolResult', props: RESULT(id) } as never)).findAll({ type: 'Text' })).map(t => t.text)

  test('answered: the ToolResult row is the card, on both surfaces', async ($, on) => {
    const w = world(on, { hook: both({ receipt: FRESH }, ANSWERED) })
    await start($, w)
    const r = await $.tool.call({ tool: 'Bash', command: PYTEST, tool_use_id: 't1' })
    expect(r).toMatchObject({ result: { stdout: RECEIPT_RESULT } })
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
    const w = world(on, { hook: both({ receipt: STALE }, ANSWERED) })
    await start($, w)
    const r = await $.tool.call({ tool: 'Bash', command: PYTEST, tool_use_id: 't1' })
    expect(r).toMatchObject({ result: { stdout: 'ok' } })
    for (const surface of SURFACES) {
      expect(await texts($ as never, surface, 't1')).toEqual(['ENGINE ToolResult', '◠ adage receipt 3f9c covers this run · /adage why'])
    }
  })

  test('fresh but withheld: the run happens and the line beside it names the receipt', async ($, on) => {
    const w = world(on, { hook: both({ receipt: FRESH }) })
    await start($, w)
    await $.tool.call({ tool: 'Bash', command: PYTEST, tool_use_id: 't1' })
    expect(await texts($ as never, 'terminal', 't1')).toEqual(['ENGINE ToolResult', '◠ adage receipt 3f9c covers this run · /adage why'])
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
    const d = deps({ ask: async p => ({ ok: true, answer: p.dry_run ? { plan: { rewrite: REWRITE } } : SAME }) })
    const r = await bashCall({ command: PYTEST, tool_use_id: 't1' }, async () => ({ deny: 'no' }), d)
    expect(r).toEqual({ deny: 'no' })
  })

  test('the drawing deps may fail: the decision stands', async () => {
    const boom = () => Promise.reject(new Error('no state'))
    const d = deps({ ask: async p => ({ ok: true, answer: p.dry_run ? { plan: { receipt: FRESH } } : ANSWERED }), answered: boom, beside: boom })
    expect(await bashCall({ command: PYTEST, tool_use_id: 't1' }, next, d)).toMatchObject({ result: { stdout: RECEIPT_RESULT } })
    const s = deps({ ask: async p => ({ ok: true, answer: p.dry_run ? { plan: { rewrite: REWRITE } } : SAME }), rewrote: boom, delivered: boom, notice: () => { throw new Error('no dialog') } })
    expect(await bashCall({ command: PYTEST, tool_use_id: 't1' }, next, s)).toMatchObject({ result: { stdout: `ran ${VENV}` }, context: [WORD] })
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
