// The dead end that never runs (C6, T-C6-03), through the module as the engine loads it: the
// binary answered beneath the plugin by the test's `process.run` hook, the person's rules by its
// `tool.check` hook, and the Bash tool by its `tool.call` hook (nothing runs).
import type { On } from 'claude-code'
import { expect, test } from 'claude-code/testing'

import { substituteOf } from './substitute'

const DEAD_END = 'python3 -m pytest tests'
const CONTEXT = 'adage ran make test in place of python3 -m pytest tests: in this repository it fails ("No module named pytest").'
const SUB = { command: 'make test', route: 'dead-end-pytest', track: 'acme/billing@1', context: CONTEXT }
const START = { cwd: '/w', surface: null, isInteractive: false } as const

type Opts = {
  version?: string
  plan?: unknown
  final?: unknown
  rules?: Record<string, 'allow' | 'ask' | 'deny'>
  observe?: string
}

/** The world beneath the plugin; returns what the binary was asked, what ran, and the notices. */
function world(on: On, o: Opts) {
  const env: Record<string, string> = { CLAUDE_PLUGIN_DATA: '/data' }
  if (o.observe !== undefined) env.ADAGE_MOD_OBSERVE = o.observe
  on('env.get', ($, e) => ({ value: env[e.name] }))
  const asked: Record<string, unknown>[] = []
  on('process.run', ($, e) => {
    const out = (stdout: string) => ({ value: { exitCode: 0, stdout, stderr: '', isStdoutTruncated: false, isStderrTruncated: false } })
    if (e.argv[1] === 'version') return out(o.version ?? 'adage 0.1.0\n')
    const p = JSON.parse(e.init?.stdin ?? '{}') as Record<string, unknown>
    asked.push(p)
    const answer = p.dry_run === true ? o.plan : p.verdict !== undefined ? o.final : undefined
    return out(answer === undefined ? '' : JSON.stringify(answer))
  })
  on('session.id', () => ({ value: 'sess-1' }))
  on('session.cwd', () => ({ value: '/w' }))
  on('session.start', ($, e) => ({ cwd: e.cwd }))
  on('tool.check', ($, e) => ({ decision: o.rules?.[(e.input as { command: string }).command] ?? 'allow' }))
  const notices: { tool_use_id: string; text: string | undefined }[] = []
  on('ui.notice', ($, e) => {
    notices.push({ tool_use_id: e.tool_use_id, text: e.text })
    return { value: undefined }
  })
  const ran: string[] = []
  on('tool.call', { tool: 'Bash' }, ($, e) => {
    ran.push(e.command)
    return { result: { stdout: `ran ${e.command}`, stderr: '', interrupted: false } }
  })
  return { asked, ran, notices }
}

const PLANNED = { plan: { substitute: SUB } }
const MADE = { substitute: SUB }

test('the same verdict for both commands: the learned command runs, with one line for the model and the person', async ($, on) => {
  const w = world(on, { plan: PLANNED, final: MADE })
  await $.session.start(START)
  const r = await $.tool.call({ tool: 'Bash', command: DEAD_END })
  expect(w.ran).toEqual(['make test'])
  expect(r.context).toEqual([CONTEXT])
  expect(w.notices.map(n => n.text)).toEqual([CONTEXT])
  expect(w.asked.map(p => [p.hook_event_name, p.dry_run, p.verdict])).toEqual([
    ['PreToolUse', true, undefined],
    ['PreToolUse', undefined, { engine: 'same', substitute: 'dead-end-pytest' }],
  ])
  expect(w.asked[0]).toMatchObject({ session_id: 'sess-1', cwd: '/w', tool_name: 'Bash', tool_input: { command: DEAD_END } })
  expect(w.asked.every(p => p.observe === undefined)).toBe(true)
})

test('different verdicts: the call runs as written, and the binary is not asked again', async ($, on) => {
  const w = world(on, { plan: PLANNED, final: MADE, rules: { [DEAD_END]: 'allow', 'make test': 'ask' } })
  await $.session.start(START)
  const r = await $.tool.call({ tool: 'Bash', command: DEAD_END })
  expect(w.ran).toEqual([DEAD_END])
  expect(r.context).toBeUndefined()
  expect(w.notices).toEqual([])
  expect(w.asked).toHaveLength(1)
})

test('no plan: the call runs as written, one ask', async ($, on) => {
  const w = world(on, {})
  await $.session.start(START)
  await $.tool.call({ tool: 'Bash', command: DEAD_END })
  expect(w.ran).toEqual([DEAD_END])
  expect(w.asked).toHaveLength(1)
})

test('the final word withheld (the control arm): the call runs as written', async ($, on) => {
  const w = world(on, { plan: PLANNED })
  await $.session.start(START)
  await $.tool.call({ tool: 'Bash', command: DEAD_END })
  expect(w.ran).toEqual([DEAD_END])
  expect(w.asked).toHaveLength(2)
  expect(w.notices).toEqual([])
})

test('inert: the call runs as written and nothing is asked', async ($, on) => {
  const w = world(on, { version: '', plan: PLANNED, final: MADE })
  await $.session.start(START)
  await $.tool.call({ tool: 'Bash', command: DEAD_END })
  expect(w.ran).toEqual([DEAD_END])
  expect(w.asked).toEqual([])
})

test('observe mode: both phases are asked with observe, and the command is never replaced', async ($, on) => {
  const w = world(on, { plan: PLANNED, final: MADE, observe: '1' })
  await $.session.start(START)
  await $.tool.call({ tool: 'Bash', command: DEAD_END })
  expect(w.ran).toEqual([DEAD_END])
  expect(w.asked.map(p => p.observe)).toEqual([true, true])
  expect(w.notices).toEqual([])
})

test('ADAGE_MOD_OBSERVE set but empty is not observe mode', async ($, on) => {
  const w = world(on, { plan: PLANNED, final: MADE, observe: '' })
  await $.session.start(START)
  await $.tool.call({ tool: 'Bash', command: DEAD_END })
  expect(w.ran).toEqual(['make test'])
})

test('a malformed substitute is none', () => {
  expect(substituteOf(SUB)).toEqual(SUB)
  expect(substituteOf({ ...SUB, command: ' ' })).toBeUndefined()
  expect(substituteOf({ ...SUB, route: 7 })).toBeUndefined()
  expect(substituteOf({ ...SUB, context: '' })).toBeUndefined()
  expect(substituteOf('make test')).toBeUndefined()
})
