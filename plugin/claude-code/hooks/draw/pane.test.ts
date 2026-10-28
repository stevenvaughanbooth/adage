// The pane through the engine (draw/pane.tsx, its sites in draw/sites.ts; ux-design §6; test-plan
// §6 B3): the tabs, the Live rows and the inspect pane, the filter, the Reflexes rows with their
// verbs and the command to copy, Health's one action, each tab empty, no binary. The decisions are
// presses since B4 (dialog.test.ts); T-B3-05 keeps the copy beside them for persona (b).
import { describe, expect, test } from 'claude-code/testing'

import { CARD, CLI, DELEGATE, GRAPH, LAPSED, LINE, PANE_PROPS, SURFACES, T0, WORLD_OFF_JSON, begin, mountPane, rowsOf, shown, start, world } from '../fixtures/kit'
import { ROLES } from '../tokens'
import { GRAPH_EMPTY } from '../view/graph'
import { clock } from '../view/pane'

const FEED = [LINE.learnerIdle, LINE.healthy, LINE.start, LINE.firedGen, LINE.firedLookup]

describe('T-B3-01 … T-B3-07 · the pane', () => {
  test('T-B3-01, T-B3-15: five tabs as lowercase hotkeys (no h), the Graph first and open; /adage opens the pane focused, as a dialog', async ($, on) => {
    const w = world(on, { feed: FEED })
    await start($, w)
    await $.command.run({ command: 'adage', args: '' } as never)
    expect(w.opened).toEqual([{ id: 'adage', title: 'adage', focus: true, closeOnEscape: true, holdToasts: true, rows: 20 }])
    for (const surface of SURFACES) {
      const ui = await mountPane($, surface)
      const tabs = await ui.findAll({ type: 'Button', key: undefined })
      const hot = tabs.filter(b => typeof b.props.hotkey === 'string').map(b => `${b.props.hotkey}:${b.props.label}`)
      expect(hot.slice(0, 5)).toEqual(['g:Graph', 'l:Live', 'r:Reflexes', 't:Health', 'w:World'])
      expect(hot.some(h => h.startsWith('h:'))).toBe(false)
      expect((await ui.findAll({ key: 'tab-graph' }))[0]?.props.dimColor).toBe(false)
      await ui.press({ key: 'tab-reflexes' })
      expect(await shown(ui)).toContain('learned in this repository')
      await ui.press({ key: 'tab-health' })
      expect((await shown(ui)).join('\n')).toContain('index  stale')
      await ui.press({ key: 'tab-graph' })
      await ui.unmount()
    }
  })

  test('T-B3-02: rows from the feed, newest first; a row opens the inspect pane with the directive as the agent read it', async ($, on) => {
    const w = world(on, { feed: FEED })
    await start($, w)
    for (const surface of SURFACES) {
      const ui = await mountPane($, surface)
      await ui.press({ key: 'tab-live' })
      const rows = (await ui.findAll({ type: 'Button' })).filter(b => b.key?.startsWith('row:'))
      // row:1 is the session start, where nothing was decided: a quiet row, hidden until `v`.
      expect(rows.map(b => b.key)).toEqual(['row:3', 'row:2'])
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
    const ui = await mountPane($, 'terminal')
    await ui.press({ key: 'tab-live' })
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
    expect(await rowKeys()).toEqual(['row:3', 'row:2'])
  })

  test('T-B3-05: Reflexes: the review verbs are presses (T-B4-06), the command stays beside them to copy, confirmed by a toast; Selects for strength and lifecycle', async ($, on) => {
    const w = world(on, { feed: FEED })
    await start($, w)
    for (const surface of SURFACES) {
      const ui = await mountPane($, surface)
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
    const ui = await mountPane($, 'terminal')
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
      const ui = await mountPane($, surface)
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

  test('T-O9-07: Health\'s version row: the pair in plain ink, the release\'s first lines, `u: update` (the press) and the command to copy', async ($, on) => {
    const update = { current: '0.1.0', latest: '0.2.0', channel: 'npm', next: '/adage update', text: '0.2.0 is out (you have 0.1.0)', notes: 'fixes hook latency' }
    const w = world(on, { feed: FEED, card: { ...CARD, update } })
    await begin($, w)
    for (const surface of SURFACES) {
      const ui = await mountPane($, surface)
      await ui.press({ key: 'tab-health' })
      const texts = (await ui.findAll({ type: 'Text' })).map(t => t.text)
      expect(texts).toContain('version  adage 0.1.0 · 0.2.0 is out')
      expect(texts).toContain('  fixes hook latency')
      const row = (await ui.findAll({ type: 'Text' })).find(t => t.text === 'version  adage 0.1.0 · 0.2.0 is out')
      expect(row?.props.color).toBeUndefined()
      const buttons = (await ui.findAll({ type: 'Button' })).filter(b => /^(update|copy:update)$/.test(b.key ?? ''))
      expect(buttons.map(b => `${b.key}=${b.text}`)).toEqual(['update=update', 'copy:update=Copy: adage update'])
      await ui.press({ key: 'copy:update' })
      expect(w.copies.at(-1)).toBe('adage update')
      await ui.unmount()
    }
    const ui = await mountPane($, 'terminal')
    await ui.press({ key: 'tab-health' })
    await ui.press({ key: 'update' })
    await w.clock.settle()
    const pressed = w.ran.filter(r => r.argv[1] === 'update')
    expect(pressed.map(r => r.argv)).toEqual([[CLI, 'update']])
    expect(pressed[0]?.init?.stdin).toContain('"press"')
    await ui.unmount()
  })

  test('T-B3-07: each tab empty, as the copy deck says', async ($, on) => {
    const w = world(on, { feed: [LINE.learnerIdle], graph: { routes: [GRAPH.routes[4]] } })
    await start($, w)
    for (const surface of SURFACES) {
      const ui = await mountPane($, surface)
      expect(await shown(ui)).toContain(GRAPH_EMPTY)
      await ui.press({ key: 'tab-live' })
      expect(await shown(ui)).toContain('Nothing yet in this repository. Reflexes appear here as Adage uses them.')
      await ui.press({ key: 'tab-reflexes' })
      expect(await shown(ui)).toContain('No learned reflexes yet · 0 of ~5 sessions needed · 1 instinct active meanwhile')
      await ui.press({ key: 'tab-graph' })
      await ui.unmount()
    }
  })

  test('T-B3-07: no binary: the Desktop says live rows need the terminal, Health says why', async ($, on) => {
    const w = world(on, { version: '', status: 'no binary for darwin-arm64' })
    await start($, w, 'desktop')
    const ui = await mountPane($, 'desktop')
    expect(await shown(ui)).toContain('the graph needs the terminal · adage watch')
    await ui.press({ key: 'tab-live' })
    expect(await shown(ui)).toContain('live rows need the terminal · adage watch')
    await ui.press({ key: 'tab-health' })
    expect(await shown(ui)).toContain('no binary for darwin-arm64')
  })
})

describe('T-B3-11 · the Reflexes flow through the engine', () => {
  test('the terminal draws the boxes, the reflex box the row\'s Button; a reflex fired in this session lit in the fire role with its time; narrow, one line', async ($, on) => {
    const w = world(on, { feed: [...FEED, LINE.followedGen] })
    await start($, w)
    const at = clock(rowsOf(LINE.firedGen)[0]?.ts ?? 0)
    const ui = await mountPane($, 'terminal', 120)
    await ui.press({ key: 'tab-reflexes' })
    const texts = await ui.findAll({ type: 'Text' })
    expect(texts.find(t => t.text.startsWith('◆ 2 reflexes fired in this session'))?.props.color).toBe(ROLES.fire)
    expect(texts.find(t => t.text === 'AFTER EDIT')).toBeDefined()
    expect(texts.find(t => t.text === `◆ fired ${at}`)?.props.color).toBe(ROLES.fire)
    expect(texts.find(t => t.text === '✓ followed')?.props.color).toBe(ROLES.fire)
    const row = (await ui.findAll({ type: 'Button' })).find(b => b.key === 'reflex:acme/learned/codegen-after-schema')
    expect(row?.text).toBe('◠ firm · 13/17 · lb 0.53')
    // The row still expands to its directive and facts.
    await ui.press({ key: 'reflex:acme/learned/codegen-after-schema' })
    expect((await shown(ui)).some(t => t.startsWith('codegen-after-schema · when an edit touches schema/*.json'))).toBe(true)
    await ui.press({ key: 'reflex:acme/learned/codegen-after-schema' })
    await ui.press({ key: 'tab-live' })
    await ui.unmount()

    const narrow = await mountPane($, 'terminal', 60)
    await narrow.press({ key: 'tab-reflexes' })
    const line = (await narrow.findAll({ type: 'Button' })).find(b => b.key === 'reflex:acme/learned/codegen-after-schema')
    expect(line?.text).toBe('after editing schema/*.json ──▶ ◠ ──▶ make gen')
    expect((await narrow.findAll({ type: 'Text' })).find(t => t.text === `◆ ${at.slice(0, 5)} `)?.props.color).toBe(ROLES.fire)
    await narrow.press({ key: 'tab-live' })
    await narrow.unmount()
  })

  test('the desktop draws the one-line flow at any width: its native Button would break a box\'s borders', async ($, on) => {
    const w = world(on, { feed: FEED })
    await start($, w)
    const ui = await mountPane($, 'desktop', 160)
    await ui.press({ key: 'tab-reflexes' })
    const line = (await ui.findAll({ type: 'Button' })).find(b => b.key === 'reflex:acme/learned/codegen-after-schema')
    expect(line?.text).toBe('after editing schema/*.json ──▶ ◠ ──▶ make gen')
    expect((await ui.findAll({ type: 'Text' })).some(t => t.text.includes('╭'))).toBe(false)
    await ui.press({ key: 'tab-live' })
    await ui.unmount()
  })

  test('Live: each directive\'s flow in small beneath its act, the arrows in its lamp\'s role; a narrow pane leaves it out', async ($, on) => {
    const w = world(on, { feed: FEED })
    await start($, w)
    for (const surface of SURFACES) {
      const ui = await mountPane($, surface, 120)
      await ui.press({ key: 'tab-live' })
      const texts = await ui.findAll({ type: 'Text' })
      expect(texts.find(t => t.text === 'after editing schema/*.json')?.props.color).toBe(ROLES.ink3)
      expect(texts.find(t => t.text === 'when a prompt names a work item')).toBeDefined()
      expect(texts.find(t => t.text === 'scripts/wi-lookup W-2042 --json')?.props.color).toBe(ROLES.command)
      expect(texts.filter(t => t.text === ' ──▶ ').map(t => t.props.color)).toEqual([ROLES.fire, ROLES.fire, ROLES.fire, ROLES.fire])
      await ui.unmount()
      const narrow = await mountPane($, surface, 60)
      expect((await narrow.findAll({ type: 'Text' })).some(t => t.text === ' ──▶ ')).toBe(false)
      await narrow.unmount()
    }
  })
})

describe('T-W3-13 · the World tab', () => {
  const iso = (ms: number) => new Date(ms).toISOString()
  const TOOLS = [{ name: 'mcp__claude_ai_google_calendar__list_events', mcp: true }]
  const CALENDAR = { items: [{ id: 'ev1', summary: 'Marketing sync', start: { dateTime: iso(T0 - 50 * 60_000) }, end: { dateTime: iso(T0 - 20 * 60_000) }, attendees: [{ email: 'ana@acme.com', responseStatus: 'accepted' }] }, { id: 'ev2', summary: 'Design review', start: { dateTime: iso(T0 + 30 * 60_000) }, end: { dateTime: iso(T0 + 90 * 60_000) } }] }
  const BLOCKS = [
    { id: 'calendar:ev1', start: T0 - 50 * 60_000, end: T0 - 20 * 60_000, title: 'Marketing sync', busy: true, accepted: true, focus: false, attendees: 1 },
    { id: 'calendar:ev2', start: T0 + 30 * 60_000, end: T0 + 90 * 60_000, title: 'Design review', busy: true, accepted: true, focus: false, attendees: 0 },
  ]

  test('T-W3-13: off, the tab says what adage world on would do and draws nothing else', async ($, on) => {
    const off = world(on, { feed: FEED })
    await start($, off)
    const ui0 = await mountPane($)
    await ui0.press({ key: 'tab-world' })
    const t0 = (await shown(ui0)).join('\n')
    expect(t0).toContain('world off')
    expect(t0).toContain('adage world on')
    expect((await ui0.findAll({ key: 'day' }))).toHaveLength(0)
    await ui0.unmount()
  })

  test('T-W3-13: on, the header, the day strip with its labels, the next block, the sources after the first poll', async ($, on) => {
    const w = world(on, { feed: FEED, world_json: { ...WORLD_OFF_JSON, enabled: true, blocks: BLOCKS, artifacts_total: 3 }, tools: TOOLS, mcp: () => CALENDAR })
    await begin($, w)
    for (const surface of SURFACES) {
      const ui = await mountPane($, surface)
      await ui.press({ key: 'tab-world' })
      const text = (await shown(ui)).join('\n')
      expect(text).toContain('world on')
      expect(text).toContain('1 source live')
      expect(text).toContain('3 things it knows')
      expect(text).toContain('07      09      11      13      15      17      19')
      expect(text).toMatch(/next: \d\d:\d\d Design review \(in 30 min\)/)
      expect(text).toContain('calendar  claude_ai_google_calendar')
      // The strip: one cell per quarter hour, the blocks filled.
      const cells = (await shown(ui)).filter(t => t === '█' || t === '╌' || t === '◆')
      expect(cells).toHaveLength(52)
      expect(cells.filter(c => c === '█').length).toBeGreaterThan(0)
      await ui.unmount()
    }
  })

  test('T-W3-13: /adage world opens the pane on the World tab, remembered across sessions', async ($, on) => {
    const w = world(on, { feed: FEED })
    await start($, w)
    await $.command.run({ command: 'adage', args: 'world' } as never)
    const ui = await mountPane($)
    expect((await ui.findAll({ key: 'tab-world' }))[0]?.props.dimColor).toBe(false)
    expect((await ui.findAll({ key: 'tab-graph' }))[0]?.props.dimColor).toBe(true)
    await ui.unmount()
  })
})
