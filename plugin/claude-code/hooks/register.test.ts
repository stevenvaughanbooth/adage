// The module as the engine loads it: the probe at session.start, the classic observers, and the
// turn.step hook end to end, with `$.process.run` answered beneath the plugin by the test's own
// `process.run` hook (the binary is never run).
import type { On, ProcessRunInit, TurnStepChunk, TurnStepResult } from 'claude-code'
import { expect, mock, test } from 'claude-code/testing'

type Ran = { argv: readonly string[]; init?: ProcessRunInit }

const REFLEX = '<adage-reflex track="acme/billing@1" route="codegen-after-schema" strength="fire" kind="obligation">\nRun `make gen`.\n</adage-reflex>'
const USAGE = { input_tokens: 10, output_tokens: 5, cache_creation_input_tokens: 0, cache_read_input_tokens: 0, model: 'claude-x' }
const PREDICT = { predict: { kind: 'obligation', route: 'codegen-after-schema', track: 'acme/billing@1', calls: [{ tool: 'Bash', input: { command: 'make gen' } }] } }

/**
 * The world beneath the plugin: CLAUDE_PLUGIN_DATA (unless `data` is undefined), a binary whose
 * `version` prints `version` and whose `hook claude TurnStep` prints `turnStep`, the session's id
 * and cwd, and a model that streams one text-and-tool response (counted).
 */
function world(on: On, opts: { data?: string; version: string; turnStep: string; isLinked?: () => boolean }) {
  mock.env(on, opts.data === undefined ? {} : { CLAUDE_PLUGIN_DATA: opts.data })
  mock.clock(on, { now: 1_791_008_381_000 })
  on('command.register', () => ({ value: { command: 'adage' } }))
  const ran: Ran[] = []
  on('process.run', ($, e) => {
    ran.push({ argv: e.argv, init: e.init })
    if (opts.isLinked?.() === false) return { deny: 'ENOENT' }
    const stdout = e.argv[1] === 'version' ? opts.version : opts.turnStep
    return { value: { exitCode: 0, stdout, stderr: '', isStdoutTruncated: false, isStderrTruncated: false } }
  })
  on('session.id', () => ({ value: 'sess-1' }))
  on('session.cwd', () => ({ value: '/w' }))
  on('session.start', ($, e) => ({ cwd: e.cwd }))
  const requests: number[] = []
  on('turn.step', async function* ($, e) {
    requests.push(e.index)
    yield { kind: 'text', index: 0, text: 'Reading.' }
    yield { kind: 'tool', index: 1, id: 'toolu_1', name: 'Read' }
    yield { kind: 'input', index: 1, json: '{"file_path":"/w/x"}' }
    yield { kind: 'stop', stopReason: 'tool_use', usage: USAGE }
    return { turnId: e.turnId, index: e.index, answer: 'Reading.', toolUses: [{ name: 'Read', input: { file_path: '/w/x' } }], stopReason: 'tool_use', usage: USAGE }
  })
  const sets: { key: string; value: unknown }[] = []
  on('state.set', async ($, e, next) => {
    // The mode the activation records is the drawings'; these tests look at what the hooks kept.
    if (e.key !== 'mode') sets.push({ key: e.key, value: e.value })
    return next(e)
  })
  return { ran, requests, sets }
}

async function drain(stream: AsyncGenerator<TurnStepChunk, TurnStepResult>) {
  const chunks: TurnStepChunk[] = []
  for (;;) {
    const r = await stream.next()
    if (r.done) return { chunks, result: r.value }
    chunks.push(r.value)
  }
}

const START = { cwd: '/w', surface: null, isInteractive: false } as const
const STEP = { turnId: 'turn-1', index: 2, model: 'claude-x', messageCount: 4 }

test('active: the presence probe runs `<data>/bin/hook version` with CLAUDECODE=1', async ($, on) => {
  const w = world(on, { data: '/data', version: 'adage 0.1.0\n', turnStep: '' })
  await $.session.start(START)
  expect(w.ran).toEqual([{ argv: ['/data/bin/hook', 'version'], init: { timeoutMs: 1000, env: { CLAUDECODE: '1' } } }])
})

test('active: a classic observer returns the command hooks\' answer unchanged and keeps what fired', async ($, on) => {
  const w = world(on, { data: '/data', version: 'adage 0.1.0\n', turnStep: '' })
  on('classic.PostToolBatch', () => ({ additionalContext: [REFLEX] }))
  on('classic.Stop', () => ({ block: 'finish line' }))
  await $.session.start(START)
  expect(await $.classic.PostToolBatch({ tool_calls: [] } as never)).toEqual({ additionalContext: [REFLEX] })
  expect(await $.classic.Stop({ stop_hook_active: false })).toEqual({ block: 'finish line' })
  expect(w.sets).toEqual([
    {
      key: 'fired',
      value: [{ event: 'PostToolBatch', route: 'codegen-after-schema', track: 'acme/billing@1', kind: 'obligation', strength: 'fire' }],
    },
  ])
})

test('active: a TurnStep prediction is yielded in place of the request', async ($, on) => {
  const w = world(on, { data: '/data', version: 'adage 0.1.0\n', turnStep: JSON.stringify(PREDICT) })
  await $.session.start(START)
  const { chunks, result } = await drain($.turn.step(STEP))
  expect(chunks).toEqual([
    { kind: 'tool', index: 0, id: 'adage_codegen-after-schema_turn-1_2_0', name: 'Bash' },
    { kind: 'input', index: 0, json: '{"command":"make gen"}' },
    { kind: 'stop', stopReason: 'tool_use', usage: null },
  ])
  expect(result).toMatchObject({ stopReason: 'tool_use', toolUses: [{ name: 'Bash', input: { command: 'make gen' } }], usage: null })
  expect(w.requests).toEqual([])
  const asked = w.ran.find(r => r.argv[1] === 'hook')
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
  })
  expect(w.sets.map(s => s.key)).toEqual(['predicted'])
})

test('active: nothing predicted, the request goes out and streams unchanged', async ($, on) => {
  const w = world(on, { data: '/data', version: 'adage 0.1.0\n', turnStep: '' })
  await $.session.start(START)
  const { chunks, result } = await drain($.turn.step(STEP))
  expect(w.requests).toEqual([2])
  expect(chunks.map(c => c.kind)).toEqual(['text', 'tool', 'input', 'stop'])
  expect(result).toMatchObject({ stopReason: 'tool_use', answer: 'Reading.' })
})

test('active whichever binary the shim linked: adage-exec answers the same presence probe', async ($, on) => {
  const w = world(on, { data: '/data', version: 'adage-exec 0.1.0\n', turnStep: JSON.stringify(PREDICT) })
  await $.session.start(START)
  const { chunks } = await drain($.turn.step(STEP))
  expect(chunks.map(c => c.kind)).toEqual(['tool', 'input', 'stop'])
  expect(w.requests).toEqual([])
  expect(w.ran.map(r => r.argv[0])).toEqual(['/data/bin/hook', '/data/bin/hook'])
})

test('inert with no binary (the link is scripts/quiet: version prints nothing): every hook forwards, no TurnStep asked', async ($, on) => {
  const w = world(on, { data: '/data', version: '', turnStep: JSON.stringify(PREDICT) })
  on('classic.PostToolBatch', () => ({ additionalContext: [REFLEX] }))
  await $.session.start(START)
  expect(await $.classic.PostToolBatch({ tool_calls: [] } as never)).toEqual({ additionalContext: [REFLEX] })
  const { chunks } = await drain($.turn.step(STEP))
  expect(chunks.map(c => c.kind)).toEqual(['text', 'tool', 'input', 'stop'])
  expect(w.requests).toEqual([2])
  expect(w.ran.map(r => r.argv[1])).toEqual(['version'])
  expect(w.sets).toEqual([])
})

test('no CLAUDE_PLUGIN_DATA and a checkout root: nothing runs, inert, no PATH fallback', async ($, on) => {
  const w = world(on, { version: 'adage 0.1.0\n', turnStep: JSON.stringify(PREDICT) })
  await $.session.start(START)
  const { chunks } = await drain($.turn.step(STEP))
  expect(chunks).toHaveLength(4)
  expect(w.ran).toEqual([])
})

test('a link the SessionStart shim makes after session.start is probed again once the shim ran', async ($, on) => {
  let linked = false
  const w = world(on, { data: '/data', version: 'adage 0.1.0\n', turnStep: JSON.stringify(PREDICT), isLinked: () => linked })
  on('classic.SessionStart', () => {
    linked = true
    return {}
  })
  await $.session.start(START)
  expect((await drain($.turn.step(STEP))).chunks).toHaveLength(4) // inert: forwarded
  await $.classic.SessionStart({ source: 'startup' } as never)
  expect((await drain($.turn.step(STEP))).chunks).toHaveLength(3) // active: predicted
  expect(w.requests).toEqual([2])
})

test('step 0 and a subagent step are forwarded without asking', async ($, on) => {
  const w = world(on, { data: '/data', version: 'adage 0.1.0\n', turnStep: JSON.stringify(PREDICT) })
  await $.session.start(START)
  await drain($.turn.step({ ...STEP, index: 0 }))
  await drain($.turn.step({ ...STEP, agentId: 'agent-7' }))
  expect(w.requests).toEqual([0, 2])
  expect(w.ran.map(r => r.argv[1])).toEqual(['version'])
})

test('a Skill prediction is yielded only for a skill `$.command.list()` lists', async ($, on) => {
  const skill = { predict: { kind: 'skill', route: 'notes', track: 'acme/billing@1', calls: [{ tool: 'Skill', input: { skill: 'release-notes' } }] } }
  const w = world(on, { data: '/data', version: 'adage 0.1.0\n', turnStep: JSON.stringify(skill) })
  let listed = [{ name: 'review', description: '', source: 'plugin' as const }]
  on('command.list', () => ({ value: listed }))
  await $.session.start(START)
  expect((await drain($.turn.step(STEP))).chunks.map(c => c.kind)).toEqual(['text', 'tool', 'input', 'stop'])
  expect(w.requests).toEqual([2])
  expect(w.sets).toEqual([])
  listed = [...listed, { name: 'release-notes', description: '', source: 'plugin' as const }]
  const { chunks } = await drain($.turn.step(STEP))
  expect(chunks[0]).toEqual({ kind: 'tool', index: 0, id: 'adage_notes_turn-1_2_0', name: 'Skill' })
  expect(w.requests).toEqual([2])
})
