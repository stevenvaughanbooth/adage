// The pane and `/adage …` through the engine (test-plan §6, B3), and the cross-cutting cases that
// need the whole module: fail open (T-X-02), headless (T-X-03), the argv recorder (T-X-04), the
// control arm drawn as a treated session with nothing fired (T-X-06). The pane's decisions are
// presses since B4 (dialog.test.ts); T-B3-05 keeps the copy beside them for persona (b).
import type { TurnStepChunk, TurnStepResult } from 'claude-code'
import { describe, expect, test } from 'claude-code/testing'
import type { Engine } from 'claude-code/testing'

import { ON_QUESTION } from './ask'
import { ALLOWED_VERBS, headOf } from './bridge'
import { OFF_TEXT, PAUSED, RESUMED } from './commands'
import { BAND, CARD, CLI, DELEGATE, GRAPH, LAPSED, LINE, OUTPUT, PANE_PROPS, PREDICT, STEP, SURFACES, TOOL, TURN_END, USER, block, start, world } from './fixtures/kit'
import type { Surface, World } from './fixtures/kit'

const FEED = [LINE.learnerIdle, LINE.healthy, LINE.start, LINE.firedGen, LINE.firedLookup]

async function pane($: Engine, surface: Surface, columns = 120) {
  return $.ui.mount({ plugin: 'adage', surface, component: 'Pane', requestId: 'adage', props: PANE_PROPS(columns) })
}
type Mounted = Awaited<ReturnType<typeof pane>>
const shown = async (ui: Mounted) => (await ui.findAll({})).filter(e => e.type === 'Text' || e.type === 'Button' || e.type === 'Link' || e.type === 'Markdown').map(e => e.text)
async function drain(stream: AsyncGenerator<TurnStepChunk, TurnStepResult>) {
  for (;;) if ((await stream.next()).done) return
}

describe('B3 · the pane', () => {
  test('T-B3-01: three tabs as lowercase hotkeys (no h); /adage opens the pane focused, as a dialog', async ($, on) => {
    const w = world(on, { feed: FEED })
    await start($, w)
    await $.command.run({ command: 'adage', args: '' } as never)
    expect(w.opened).toEqual([{ id: 'adage', title: 'adage', focus: true, closeOnEscape: true, holdToasts: true, rows: 20 }])
    for (const surface of SURFACES) {
      const ui = await pane($, surface)
      const tabs = await ui.findAll({ type: 'Button', key: undefined })
      const hot = tabs.filter(b => typeof b.props.hotkey === 'string').map(b => `${b.props.hotkey}:${b.props.label}`)
      expect(hot.slice(0, 3)).toEqual(['l:Live', 'r:Reflexes', 't:Health'])
      expect(hot.some(h => h.startsWith('h:'))).toBe(false)
      await ui.press({ key: 'tab-reflexes' })
      expect(await shown(ui)).toContain('acme/learned')
      await ui.press({ key: 'tab-health' })
      expect((await shown(ui)).join('\n')).toContain('index  stale')
      await ui.press({ key: 'tab-live' })
      await ui.unmount()
    }
  })

  test('T-B3-02: rows from the feed, newest first; a row opens the inspect pane with the directive as the agent read it', async ($, on) => {
    const w = world(on, { feed: FEED })
    await start($, w)
    for (const surface of SURFACES) {
      const ui = await pane($, surface)
      const rows = (await ui.findAll({ type: 'Button' })).filter(b => b.key?.startsWith('row:'))
      expect(rows.map(b => b.key)).toEqual(['row:3', 'row:2', 'row:1'])
      expect(rows[0]?.text).toContain('→ ◠ lookup-work-item')
      expect(rows[0]?.text).toContain('FIRE')
      await ui.press({ key: 'row:2' })
      expect(w.opened.at(-1)).toEqual({ id: 'adage-inspect', title: 'adage · inspect', focus: true, closeOnEscape: true, rows: 12 })
      const inspect = await $.ui.mount({ plugin: 'adage', surface, component: 'Pane', requestId: 'adage-inspect', props: PANE_PROPS(80) })
      const md = await inspect.findAll({ type: 'Markdown' })
      expect(md[0]?.text).toContain('must be followed by running')
      expect(md[0]?.text).toContain('### run make gen after a schema edit')
      await inspect.unmount()
      await ui.unmount()
    }
  })

  test('T-B3-03: the filter as typed, kept on enter, cleared by an empty enter; a filtered-out row is not inspected', async ($, on) => {
    const w = world(on, { feed: FEED })
    await start($, w)
    const ui = await pane($, 'terminal')
    const rowKeys = async () => (await ui.findAll({ type: 'Button' })).filter(b => b.key?.startsWith('row:')).map(b => b.key)
    await ui.input({ key: 'filter', text: 'lookup', kind: 'change' })
    expect(await rowKeys()).toEqual(['row:3'])
    await ui.input({ key: 'filter', text: 'lookup' })
    expect(await rowKeys()).toEqual(['row:3'])
    const before = w.opened.length
    // row:2 is not drawn now; the handler a stale drawing holds refuses it too.
    await expect(ui.press({ key: 'row:2' })).rejects.toThrow()
    expect(w.opened.length).toBe(before)
    await ui.input({ key: 'filter', text: '' })
    expect(await rowKeys()).toEqual(['row:3', 'row:2', 'row:1'])
  })

  test('T-B3-04: p pauses (the band says so), v hides quiet rows, n scrolls to the newest', async ($, on) => {
    const w = world(on, { feed: FEED })
    await start($, w)
    const ui = await pane($, 'terminal')
    const footer = (await ui.findAll({ type: 'Button' })).filter(b => ['pause', 'quiet', 'newest'].includes(b.key ?? ''))
    expect(footer.map(b => `${b.props.hotkey}:${b.props.label}`)).toEqual(['p:pause', 'v:quiet rows', 'n:newest'])
    await ui.press({ key: 'quiet' })
    expect((await ui.findAll({ type: 'Button' })).filter(b => b.key?.startsWith('row:')).map(b => b.key)).toEqual(['row:3', 'row:2'])
    // `n` asks the engine to scroll the pane to its newest row ($.ui.scroll); the test engine has no
    // scrolling of its own to answer it, so the press is checked to settle and change nothing else.
    await ui.press({ key: 'newest' })
    expect((await ui.findAll({ type: 'Button' })).filter(b => b.key?.startsWith('row:')).map(b => b.key)).toEqual(['row:3', 'row:2'])
    await ui.press({ key: 'pause' })
    const band = await $.ui.mount({ plugin: 'adage', surface: 'terminal', component: 'AbovePrompt', props: BAND() })
    expect((await band.findAll({ type: 'Text' })).map(t => t.text).join('')).toBe('◠ adage  paused · /adage pause')
    expect((await ui.findAll({ key: 'pause' }))[0]?.props.label).toBe('resume')
    await ui.press({ key: 'pause' })
    expect((await band.findAll({ type: 'Text' })).map(t => t.text).join('')).not.toContain('paused')
  })

  test('T-B3-05: Reflexes: the review verbs are presses (T-B4-06), the command stays beside them to copy, confirmed by a toast; Selects for strength and lifecycle', async ($, on) => {
    const w = world(on, { feed: FEED })
    await start($, w)
    for (const surface of SURFACES) {
      const ui = await pane($, surface)
      await ui.press({ key: 'tab-reflexes' })
      expect((await ui.findAll({ type: 'Select' })).map(s => s.key)).toEqual(['strength', 'lifecycle'])
      const ran = w.ran.length
      await ui.press({ key: 'reflex:acme/learned/codegen-after-schema' })
      await ui.press({ key: 'details:acme/learned/codegen-after-schema' })
      const verbs = (await ui.findAll({ type: 'Button' })).filter(b => b.key?.startsWith('verb:') && b.key.endsWith('acme/learned/codegen-after-schema'))
      expect(verbs.map(b => b.text)).toEqual(['make it a direct instruction', 'pin it'])
      expect(await shown(ui)).toContain('adage review strengthen acme/learned codegen-after-schema')
      await ui.press({ key: 'copy:strengthen:acme/learned/codegen-after-schema' })
      expect(w.copies.at(-1)).toBe('adage review strengthen acme/learned codegen-after-schema')
      expect(w.toasts.at(-1)).toBe('copied: adage review strengthen acme/learned codegen-after-schema')
      expect(w.ran.length).toBe(ran)
      await ui.select({ key: 'strength', value: 'inhibit' })
      expect((await ui.findAll({ type: 'Button' })).filter(b => b.key?.startsWith('reflex:')).map(b => b.key)).toEqual(['reflex:acme/learned/dead-end-unittest'])
      await ui.select({ key: 'strength', value: 'all' })
      await ui.press({ key: 'reflex:acme/learned/codegen-after-schema' }) // collapses it again
      await ui.press({ key: 'tab-live' })
      await ui.unmount()
    }
  })

  test('T-B3-05: a copy that did not happen says where the command is', async ($, on) => {
    const w = world(on, { feed: FEED, copies: false })
    await start($, w)
    const ui = await pane($, 'terminal')
    await ui.press({ key: 'tab-reflexes' })
    await ui.press({ key: 'reflex:acme/learned/codegen-after-schema' })
    await ui.press({ key: 'details:acme/learned/codegen-after-schema' })
    await ui.press({ key: 'copy:strengthen:acme/learned/codegen-after-schema' })
    expect(w.toasts.at(-1)).toBe('copy is not available here · the command is in /adage health')
  })

  test('T-B3-06: Health runs `adage condition` and nothing else; every other fix is a command to copy, the lapsed grants too', async ($, on) => {
    const w = world(on, { feed: FEED, delegate: { ...DELEGATE, lapsed: LAPSED } })
    await start($, w)
    for (const surface of SURFACES) {
      const ui = await pane($, surface)
      await ui.press({ key: 'tab-health' })
      const buttons = (await ui.findAll({ type: 'Button' })).filter(b => /^(run|copy):/.test(b.key ?? ''))
      expect(buttons.map(b => `${b.key}=${b.text}`)).toEqual([
        'run:1=enter: condition this repository now',
        'copy:check:2=Copy command',
        'copy:lapse:acme/learned/codegen-after-schema=Copy command',
      ])
      await ui.press({ key: 'run:1' })
      expect(w.ran.some(r => r.argv.join(' ') === `${CLI} condition`)).toBe(true)
      await ui.press({ key: 'copy:check:2' })
      expect(w.copies.at(-1)).toBe('adage install claude-code --write')
      await ui.press({ key: 'copy:lapse:acme/learned/codegen-after-schema' })
      expect(w.copies.at(-1)).toBe('adage review delegate acme/learned codegen-after-schema --predict')
      await ui.press({ key: 'tab-live' })
      await ui.unmount()
    }
  })

  test('T-B3-07: each tab empty, as the copy deck says', async ($, on) => {
    const w = world(on, { feed: [LINE.learnerIdle], graph: { routes: [GRAPH.routes[4]] } })
    await start($, w)
    for (const surface of SURFACES) {
      const ui = await pane($, surface)
      expect(await shown(ui)).toContain('Nothing yet in this repository. Reflexes appear here as Adage uses them.')
      await ui.press({ key: 'tab-reflexes' })
      expect(await shown(ui)).toContain('No learned reflexes yet · 0 of ~5 sessions needed · 1 instinct active meanwhile')
      await ui.press({ key: 'tab-live' })
      await ui.unmount()
    }
  })

  test('T-B3-07: no binary: the Desktop says live rows need the terminal, Health says why', async ($, on) => {
    const w = world(on, { version: '', status: 'no binary for darwin-arm64' })
    await start($, w, 'desktop')
    const ui = await pane($, 'desktop')
    expect(await shown(ui)).toContain('live rows need the terminal · adage watch')
    await ui.press({ key: 'tab-health' })
    expect(await shown(ui)).toContain('no binary for darwin-arm64')
  })

  test('§9.4: the follower ends three times early: feed stopped, run adage doctor', async ($, on) => {
    const w = world(on, { feed: FEED })
    await start($, w)
    expect(w.spawned.filter(a => a[1] === 'watch').map(a => a.slice(10))).toEqual([[], ['--since', JSON.parse(LINE.firedLookup).cursor], ['--since', JSON.parse(LINE.firedLookup).cursor]])
    expect(await shown(await pane($, 'terminal'))).toContain('feed stopped · run adage doctor')
  })

  // A serving map keeps work pending, which each act waits a moment on.
  test('T-B4-07, Explore in browser: one map, started once, then a link to it', { timeoutMs: 30_000 }, async ($, on) => {
    const w = world(on, { feed: FEED })
    await start($, w)
    const first = await pane($, 'terminal')
    expect((await first.findAll({ key: 'explore' }))[0]?.text).toBe('Explore in browser')
    await first.press({ key: 'explore' })
    await first.unmount()
    // A serving map keeps work pending from here, which each act waits a moment on: few acts.
    for (const surface of SURFACES) {
      const ui = await pane($, surface)
      const link = (await ui.findAll({ type: 'Link' }))[0]
      expect([link?.props.href, link?.text]).toEqual(['http://localhost:43123/', 'Explore in browser'])
      await ui.unmount()
    }
    expect((await $.command.run({ command: 'adage', args: 'map' } as never)).text).toBe('Explore in browser: http://localhost:43123/')
    expect(w.spawned.filter(a => a[1] === 'map')).toEqual([[CLI, 'map', '--port', '0']])
  })
})

describe('B3 · /adage', () => {
  test('T-B3-09, T-B4-08: one registration, immediate, with the hint; each verb answers as ux-design §7 says; /adage on opens the ask', async ($, on) => {
    const w = world(on, { feed: FEED })
    await start($, w)
    expect(w.registered).toEqual([{ name: 'adage', description: expect.any(String), argumentHint: '[review]', immediate: true }])
    const run = async (args: string) => (await $.command.run({ command: 'adage', args } as never)).text
    const ran = w.ran.length
    expect(await run('on')).toBeUndefined()
    await w.clock.settle()
    expect(w.asked).toEqual([{ question: ON_QUESTION, options: ['turn it on', 'leave it off'] }])
    expect(w.ran.length).toBe(ran)
    expect(w.copies).toEqual([])
    expect(await run('off')).toBe(OFF_TEXT)
    expect(w.ran.slice(ran).map(r => r.argv)).toContainEqual([CLI, 'off'])
    expect(w.ran.slice(ran).filter(r => r.argv[1] === 'off')).toHaveLength(1)
    expect(await run('pause')).toBe(PAUSED)
    expect(await run('pause')).toBe(RESUMED)
    expect(await run('why')).toContain('◠ look up the work item with scripts/wi-lookup')
    w.opened.length = 0
    for (const [verb, tab] of [['watch', 'live'], ['reflexes', 'reflexes'], ['review', 'reflexes'], ['health', 'health'], ['doctor', 'health']] as const) {
      expect(await run(verb)).toBeUndefined()
      expect(w.opened.at(-1)).toEqual({ id: 'adage', title: 'adage', focus: true, closeOnEscape: true, holdToasts: true, rows: 20 })
      const ui = await pane($, 'terminal')
      expect((await ui.findAll({ key: `tab-${tab}` }))[0]?.props.dimColor).toBe(false)
      await ui.unmount()
    }
    expect((await run('frobnicate'))?.split('\n')[0]).toBe(CARD.header)
  })

  test('T-B3-09: /adage and /adage why drawn in their row; why in the control arm says nothing spoke', async ($, on) => {
    const w = world(on, { feed: [LINE.learnerIdle, LINE.controlPrompt, LINE.controlStep, LINE.controlJudged] })
    await start($, w)
    const why = (await $.command.run({ command: 'adage', args: 'why' } as never)).text ?? ''
    expect(why).toBe('No reflex has spoken in this session yet.')
    const card = (await $.command.run({ command: 'adage', args: '' } as never)).text ?? ''
    for (const surface of SURFACES) {
      const drawn = await $.ui.mount({ plugin: 'adage', surface, component: 'CommandOutput', props: OUTPUT('', card) })
      const t = (await drawn.findAll({ type: 'Text' })).map(x => x.text)
      expect(t[0]).toBe(CARD.header)
      expect(t).toContain('/adage watch · live rows, reflexes and health')
      const copy = (await drawn.findAll({ type: 'Button' })).map(b => b.key)
      expect(copy.length).toBe(2) // adage map, adage doctor: the terminal's commands, behind Copy
      await drawn.unmount()
      const whyRow = await $.ui.mount({ plugin: 'adage', surface, component: 'CommandOutput', props: OUTPUT('why', why) })
      expect((await whyRow.findAll({ type: 'Text' })).map(x => x.text)).toEqual(['No reflex has spoken in this session yet.'])
      await whyRow.unmount()
    }
  })

  test('T-B3-10: band off registers no band; pane auto opens unasked, manual does not', async ($, on) => {
    const w = world(on, { feed: FEED })
    await start($, w)
    expect(w.opened).toEqual([])
  })

  test('T-B3-10: band: false draws no band at all', { options: { band: false } }, async ($, on) => {
    const w = world(on, { feed: [...FEED, LINE.problem] })
    await start($, w)
    for (const surface of SURFACES) {
      const ui = await $.ui.mount({ plugin: 'adage', surface, component: 'AbovePrompt', props: BAND() })
      expect((await ui.findAll({ type: 'Text' })).map(t => t.text)).toEqual(['ENGINE AbovePrompt'])
    }
  })

  test('T-B3-10: pane: auto opens the pane unasked (the engine seats it from 144 columns)', { options: { pane: 'auto' } }, async ($, on) => {
    const w = world(on, { feed: FEED })
    await start($, w)
    expect(w.opened).toEqual([{ id: 'adage', title: 'adage' }])
  })
})

describe('§4.9 · /clear', () => {
  test('a new session id: the follower follows it, its counts start again, the old rows are no longer its own', async ($, on) => {
    let id = 'sess-1'
    const w = world(on, { feed: FEED, graph: { routes: GRAPH.routes.slice(0, 3) }, session: () => id, classic: { UserPromptSubmit: { additionalContext: [block('codegen-after-schema')] } } })
    await start($, w)
    await $.classic.UserPromptSubmit({ prompt: 'go', source: 'user' } as never)
    const band = async () => (await (await $.ui.mount({ plugin: 'adage', surface: 'terminal', component: 'AbovePrompt', props: BAND(), requestId: `b-${id}` })).findAll({ type: 'Text' })).map(t => t.text).join('')
    expect(await band()).toBe('◠ adage  2 reflexes this session · ● healthy')
    id = 'sess-2'
    await $.classic.SessionStart({ source: 'clear', session_id: 'sess-2', cwd: '/w' } as never)
    await w.clock.settle()
    expect(w.spawned.filter(a => a[1] === 'watch').at(-1)?.slice(8, 10)).toEqual(['--session', 'sess-2'])
    expect(await band()).toBe('ENGINE AbovePrompt')
    expect((await $.command.run({ command: 'adage', args: 'why' } as never)).text).toBe('No reflex has spoken in this session yet.')
    // The new session's press was asked for with its id.
    expect(JSON.parse(w.ran.find(r => r.argv[3] === 'Press')?.init?.stdin ?? '{}')).toMatchObject({ session_id: 'sess-2', source: 'clear' })
  })
})

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
  const ui = await pane($, 'terminal')
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

/** What a session draws, everywhere, as text: the band, the rows, the pane's Live tab, /adage why. */
async function drawings($: Engine): Promise<string> {
  const out: string[] = []
  for (const surface of SURFACES) {
    const sites = [
      await $.ui.mount({ plugin: 'adage', surface, component: 'AbovePrompt', props: BAND() }),
      await $.ui.mount({ plugin: 'adage', surface, component: 'UserMessage', props: USER('fix W-2042') }),
      await $.ui.mount({ plugin: 'adage', surface, component: 'ToolUse', props: TOOL('t1') }),
      await pane($, surface),
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
