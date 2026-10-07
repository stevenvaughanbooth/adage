// The mod end to end, as the engine loads it (test-plan §6, the cross-cutting cases): every hook
// fails open (T-X-02), a headless session draws and asks nothing (T-X-03), the argv recorder over
// every hook, command and press (T-X-04), and the control arm drawn as a treated session with
// nothing fired (T-X-06). register.ts holds only the registrations; the other test files test the
// module each registration delegates to.
import { describe, expect, test } from 'claude-code/testing'
import type { Engine } from 'claude-code/testing'

import { ALLOWED_VERBS, headOf } from './bridge'
import { BAND, CARD, GRAPH, GRAPH_OFFER, LINE, PACKS, PREDICT, SESSION, STEP, SURFACES, TOOL, TURN_END, USER, begin, block, drain, mountPane, presses, start, stdin, turnEnd, world } from './fixtures/kit'
import type { World, WorldOptions } from './fixtures/kit'

const FEED = [LINE.learnerIdle, LINE.healthy, LINE.start, LINE.firedGen, LINE.firedLookup]
const PRESS_FEED = [LINE.learnerIdle, LINE.healthy, LINE.start, LINE.pressIssued]

describe('T-X-02 · fail open', () => {
  test('a binary that fails every ask: each hook answers what next answered, the call runs as written, one warning', async ($, on) => {
    const w = world(on, { feed: FEED, hook: { TurnStep: '{', PreToolUse: '{', SkillPrompt: '{', Press: '{' }, classic: { UserPromptSubmit: { additionalContext: ['ctx'] } } })
    await start($, w)
    await $.classic.SessionStart({ source: 'startup' } as never)
    expect(await $.classic.UserPromptSubmit({ prompt: 'x', source: 'user' } as never)).toEqual({ additionalContext: ['ctx'] })
    for (let k = 0; k < 3; k++) await drain($.turn.step(STEP))
    expect(w.requests).toBe(3)
    expect((await $.tool.call({ tool: 'Bash', command: 'make test', tool_use_id: 't9' } as never)) as unknown).toMatchObject({ result: { stdout: 'ok' } })
    expect(await $.skill.prompt({ skill: 'x', text: 'the text' })).toEqual({ text: 'the text' })
    expect(await $.turn.complete(TURN_END as never)).toEqual({ text: 'Done.' })
    await w.clock.settle()
    expect(w.statuses).toEqual(['hooks are not answering; run adage doctor'])
  })

  test('reads that fail keep what was drawn: no reflexes, no checks, the band still says what it knows', async ($, on) => {
    const w = world(on, { feed: [LINE.learnerIdle, LINE.check], failing: ['graph', 'doctor', 'card', 'delegate', 'status'] })
    await start($, w)
    const ui = await $.ui.mount({ plugin: 'adage', surface: 'terminal', component: 'AbovePrompt', props: BAND() })
    expect((await ui.findAll({ type: 'Text' })).map(t => t.text).join('')).toBe('◠ adage  ● check: hooks: claude-code')
    expect((await $.command.run({ command: 'adage', args: '' } as never)).text).toBe('adage is not answering here; run adage doctor in a terminal.')
  })
})

describe('T-X-03 · headless', () => {
  test('no feed, no reads, nothing drawn, nothing asked; the observers still run', async ($, on) => {
    const w = world(on, { feed: FEED, classic: { UserPromptSubmit: { additionalContext: [block('codegen-after-schema')] } } })
    await start($, w, null, false)
    expect(w.spawned).toEqual([])
    // The probe and the session's Standing (attached headless too, plan C5); nothing read to draw.
    expect(w.ran.map(r => r.argv.slice(1, 4).join(' '))).toEqual(['version', 'hook claude Standing'])
    expect(await $.classic.UserPromptSubmit({ prompt: 'x', source: 'sdk' } as never)).toEqual({ additionalContext: [block('codegen-after-schema')] })
    await $.turn.complete(TURN_END as never)
    await w.clock.settle()
    expect(w.asked).toEqual([])
    for (const surface of SURFACES) {
      const sites = [
        await $.ui.mount({ plugin: 'adage', surface, component: 'AbovePrompt', props: BAND() }),
        await $.ui.mount({ plugin: 'adage', surface, component: 'UserMessage', props: USER('x') }),
        await $.ui.mount({ plugin: 'adage', surface, component: 'ToolUse', props: TOOL('t1') }),
      ]
      for (const s of sites) expect((await s.findAll({ type: 'Text' })).every(t => t.text.startsWith('ENGINE'))).toBe(true)
    }
  })
})

/** Runs every hook, command and press the module has over one session. */
async function everything($: Engine, w: World) {
  await $.classic.SessionStart({ source: 'startup', session_id: 'sess-1', cwd: '/w' } as never)
  await $.classic.UserPromptSubmit({ prompt: 'fix W-2042', source: 'user' } as never)
  await $.tool.call({ tool: 'Bash', command: 'pytest -q', tool_use_id: 't1' } as never)
  await drain($.turn.step(STEP))
  await $.classic.PostToolBatch({ tool_calls: [{ tool_name: 'Bash', tool_input: { command: 'pytest -q' }, tool_use_id: 't1' }] } as never)
  await $.classic.PostToolUseFailure({ tool_name: 'Bash', tool_input: {}, tool_use_id: 't1', error: 'x' } as never)
  await $.classic.Stop({ stop_hook_active: false } as never)
  await $.turn.complete(TURN_END as never)
  await w.clock.settle()
  for (const args of ['', 'status', 'watch', 'reflexes', 'review', 'health', 'doctor', 'why', 'off', 'on', 'pause', 'pause', 'savings', 'install claude-code', 'config set x']) {
    await $.command.run({ command: 'adage', args } as never)
  }
  await w.clock.settle()
  const ui = await mountPane($, 'terminal')
  for (const tab of ['tab-reflexes', 'tab-health', 'tab-live']) {
    await ui.press({ key: tab })
    for (const b of await ui.findAll({ type: 'Button' })) {
      if (b.key === undefined || b.key.startsWith('tab-') || b.key === 'pause' || b.key === 'explore') continue
      await ui.press({ key: b.key }).catch(() => undefined)
    }
  }
  // Last, as a serving map keeps work pending for the rest of the test.
  await ui.press({ key: 'explore' })
  await $.command.run({ command: 'adage', args: 'map' } as never)
  await w.clock.settle()
}

describe('T-X-04 · authority', () => {
  test('outside a press, the argv heads are exactly the allowed ones; install and config never; a person-only verb only with the press on stdin', { timeoutMs: 60_000 }, async ($, on) => {
    const w = world(on, { feed: FEED, hook: { TurnStep: PREDICT }, pretool: { updatedInput: { command: '.venv/bin/python -m pytest -q' } }, ask: (q, o) => o[0] })
    await start($, w)
    await everything($, w)
    const pressed = w.ran.filter(r => typeof r.init?.stdin === 'string' && r.init.stdin.includes('"press"'))
    const others = w.ran.filter(r => !pressed.includes(r))
    const heads = new Set([...others.map(r => headOf(r.argv)), ...w.spawned.map(headOf)])
    const allowed = new Set(['hook', 'version', 'watch', 'graph', 'doctor', 'status', 'ledger note', 'off', 'map', 'condition', 'delegate'])
    for (const h of heads) expect(allowed.has(h), h).toBe(true)
    for (const h of ['install', 'config', 'review', 'on', 'add']) expect(heads.has(h)).toBe(false)
    for (const r of pressed) {
      expect(['review', 'on', 'add'].includes(r.argv[1] ?? '')).toBe(true)
      expect(r.argv.some(a => /^[0-9a-f]{64}$/.test(a))).toBe(false)
    }
    expect(pressed.length).toBeGreaterThanOrEqual(2)
    expect(ALLOWED_VERBS.includes('review')).toBe(false)
    for (const r of w.ran) expect(r.init?.env?.CLAUDECODE).toBe('1')
  })
})

describe('T-X-04, T-O9-04 · authority, with presses', () => {
  // A newer Adage too (onboarding plan O9): `/adage update`'s answer is a press like the others.
  const update = { current: '0.1.0', latest: '0.2.0', channel: 'npm', next: '/adage update', text: '0.2.0 is out (you have 0.1.0)', notes: null }
  const opts: WorldOptions = { feed: PRESS_FEED, graph: { ...GRAPH_OFFER, routes: [...GRAPH_OFFER.routes, GRAPH.routes[3]] }, status_json: { tracks: [], candidates: [], packs_awaiting_trust: PACKS }, card: { ...CARD, update }, ask: (q, o) => o[0] }

  test('person-only verbs run only inside press(), with the token on stdin and never on argv', { timeoutMs: 60_000 }, async ($, on) => {
    const w = world(on, opts)
    await begin($, w)
    await turnEnd($, w)
    const grant = await mountPane($, 'terminal', 120, 'adage-allow')
    await grant.press({ key: 'allow' })
    await w.clock.settle()
    await grant.unmount()
    await $.command.run({ command: 'adage', args: 'on' } as never)
    await w.clock.settle()
    await $.command.run({ command: 'adage', args: 'update' } as never)
    await w.clock.settle()
    const ui = await mountPane($)
    await ui.press({ key: 'tab-reflexes' })
    for (const b of await ui.findAll({ type: 'Button' })) {
      if (b.key === undefined || b.key.startsWith('tab-') || b.key === 'explore') continue
      await ui.press({ key: b.key }).catch(() => undefined)
    }
    await w.clock.settle()
    const pressed = presses(w)
    const others = w.ran.filter(r => !pressed.includes(r))
    const allowed = new Set(['hook', 'version', 'watch', 'graph', 'doctor', 'status', 'ledger note', 'off', 'map', 'condition', 'delegate'])
    for (const r of others) expect(allowed.has(headOf(r.argv)), headOf(r.argv)).toBe(true)
    for (const r of others) expect(['review', 'on', 'add', 'update'].includes(r.argv[1] ?? '')).toBe(false)
    expect(pressed.length).toBeGreaterThanOrEqual(4)
    expect(pressed.filter(r => r.argv[1] === 'update').map(r => r.argv.slice(1))).toEqual([['update']])
    for (const r of pressed) {
      expect(['review', 'on', 'add', 'update'].includes(r.argv[1] ?? '')).toBe(true)
      expect(r.argv.some(a => /^[0-9a-f]{64}$/.test(a))).toBe(false)
      expect(r.init?.env?.CLAUDECODE).toBe('1')
      expect(typeof stdin(r)?.press).toBe('string')
      expect(stdin(r)?.session).toBe(SESSION)
    }
    for (const r of w.ran) expect(['install', 'config', 'share', 'train', 'packs', 'init'].includes(r.argv[1] ?? '')).toBe(false)
    expect(ALLOWED_VERBS.includes('review')).toBe(false)
    expect(w.asked.length).toBeGreaterThanOrEqual(2)
  })
})

/** What a session draws, everywhere, as text: the band, the rows, the pane's Live tab, /adage why. */
async function drawings($: Engine): Promise<string> {
  const out: string[] = []
  for (const surface of SURFACES) {
    const sites = [
      await $.ui.mount({ plugin: 'adage', surface, component: 'AbovePrompt', props: BAND() }),
      await $.ui.mount({ plugin: 'adage', surface, component: 'UserMessage', props: USER('fix W-2042') }),
      await $.ui.mount({ plugin: 'adage', surface, component: 'ToolUse', props: TOOL('t1') }),
      await mountPane($, surface),
    ]
    for (const s of sites) {
      out.push(JSON.stringify((await s.findAll({})).map(e => ({ type: e.type, key: e.key, text: e.text }))))
      await s.unmount()
    }
  }
  out.push((await $.command.run({ command: 'adage', args: 'why' } as never)).text ?? '')
  return out.join('\n')
}

describe('T-X-06 · the control arm draws what a treated session with nothing fired draws', () => {
  test('byte-identical: every site, the band, the Live tab and /adage why', async ($, on) => {
    const control = world(on, { feed: [LINE.learnerIdle, LINE.healthy, LINE.start, LINE.controlPrompt, LINE.controlStep, LINE.controlJudged, LINE.controlPretool] })
    await start($, control)
    await $.classic.UserPromptSubmit({ prompt: 'fix W-2042', source: 'user' } as never) // the command hook printed nothing
    const a = await drawings($)
    expect(a).not.toMatch(/withheld|would|unaided|not reached|control/)
    capture.control = a
  })

  test('the treated session with no match', async ($, on) => {
    const treated = world(on, { feed: [LINE.learnerIdle, LINE.healthy, LINE.start, LINE.quietPrompt, LINE.quietStep, LINE.quietTools, LINE.quietPretool] })
    await start($, treated)
    await $.classic.UserPromptSubmit({ prompt: 'fix W-2042', source: 'user' } as never)
    capture.treated = await drawings($)
    expect(capture.control).toBeDefined()
    expect(capture.treated).toBe(capture.control)
  })
})

const capture: { control?: string; treated?: string } = {}
