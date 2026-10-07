// The pane through the engine (draw/pane.tsx, its sites in draw/sites.ts; ux-design §6; test-plan
// §6 B3): the tabs, the Live rows and the inspect pane, the filter, the Reflexes rows with their
// verbs and the command to copy, Health's one action, each tab empty, no binary. The decisions are
// presses since B4 (dialog.test.ts); T-B3-05 keeps the copy beside them for persona (b).
import { describe, expect, test } from 'claude-code/testing'

import { CARD, CLI, DELEGATE, GRAPH, LAPSED, LINE, PANE_PROPS, SURFACES, begin, mountPane, shown, start, world } from '../fixtures/kit'

const FEED = [LINE.learnerIdle, LINE.healthy, LINE.start, LINE.firedGen, LINE.firedLookup]

describe('T-B3-01 … T-B3-07 · the pane', () => {
  test('T-B3-01: three tabs as lowercase hotkeys (no h); /adage opens the pane focused, as a dialog', async ($, on) => {
    const w = world(on, { feed: FEED })
    await start($, w)
    await $.command.run({ command: 'adage', args: '' } as never)
    expect(w.opened).toEqual([{ id: 'adage', title: 'adage', focus: true, closeOnEscape: true, holdToasts: true, rows: 20 }])
    for (const surface of SURFACES) {
      const ui = await mountPane($, surface)
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
      const ui = await mountPane($, surface)
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
    const ui = await mountPane($, 'terminal')
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
    const ui = await mountPane($, 'desktop')
    expect(await shown(ui)).toContain('live rows need the terminal · adage watch')
    await ui.press({ key: 'tab-health' })
    expect(await shown(ui)).toContain('no binary for darwin-arm64')
  })
})
