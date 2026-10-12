// What the person's presses in the pane do (actions.ts; ux-design §6; T-B3-03, T-B3-04, T-B3-05,
// T-B4-07): through the engine, the pause, the quiet switch and the newest row, and `Explore in
// browser`; over the fake, copying a command, opening the pane, and the pane's open state.
import { describe, expect, test } from 'claude-code/testing'

import { copy, explore, onPaneClose, onPaneOpen, openPane, paneActions, showReflex } from './actions'
import { BAND, CLI, LINE, SURFACES, fake, mountPane, start, world } from './fixtures/kit'

const FEED = [LINE.learnerIdle, LINE.healthy, LINE.start, LINE.firedGen, LINE.firedLookup]

describe('T-B3-04, T-B4-07 · the pane\'s actions through the engine', () => {
  test('T-B3-04: p pauses (the band says so), quiet rows hidden until v (said in a line), n scrolls to the newest', async ($, on) => {
    const w = world(on, { feed: FEED })
    await start($, w)
    const ui = await mountPane($, 'terminal')
    await ui.press({ key: 'tab-live' })
    const rowKeys = async () => (await ui.findAll({ type: 'Button' })).filter(b => b.key?.startsWith('row:')).map(b => b.key)
    const footer = (await ui.findAll({ type: 'Button' })).filter(b => ['pause', 'quiet', 'newest'].includes(b.key ?? ''))
    expect(footer.map(b => `${b.props.hotkey}:${b.props.label}`)).toEqual(['p:pause', 'v:show quiet rows', 'n:newest'])
    expect(await rowKeys()).toEqual(['row:3', 'row:2'])
    expect((await ui.findAll({ type: 'Text' })).map(t => t.text)).toContain('1 quiet event hidden · v: show')
    await ui.press({ key: 'quiet' })
    expect(await rowKeys()).toEqual(['row:3', 'row:2', 'row:1'])
    expect((await ui.findAll({ key: 'quiet' }))[0]?.props.label).toBe('hide quiet rows')
    await ui.press({ key: 'quiet' })
    expect(await rowKeys()).toEqual(['row:3', 'row:2'])
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

  // A serving map keeps work pending, which each act waits a moment on.
  test('T-B4-07, Explore in browser: one map, started once, then a link to it', { timeoutMs: 30_000 }, async ($, on) => {
    const w = world(on, { feed: FEED })
    await start($, w)
    const first = await mountPane($, 'terminal')
    expect((await first.findAll({ key: 'explore' }))[0]?.text).toBe('Explore in browser')
    await first.press({ key: 'explore' })
    await first.unmount()
    // A serving map keeps work pending from here, which each act waits a moment on: few acts.
    for (const surface of SURFACES) {
      const ui = await mountPane($, surface)
      const link = (await ui.findAll({ type: 'Link' }))[0]
      expect([link?.props.href, link?.text]).toEqual(['http://localhost:43123/', 'Explore in browser'])
      await ui.unmount()
    }
    expect((await $.command.run({ command: 'adage', args: 'map' } as never)).text).toBe('Explore in browser: http://localhost:43123/')
    expect(w.spawned.filter(a => a[1] === 'map')).toEqual([[CLI, 'map', '--port', '0']])
  })
})

describe('T-B3-05 · the pane\'s actions over the fake', () => {
  test('copy: on the surface the press came from, said in a toast; one that did not happen says where the command is', async () => {
    const f = fake()
    expect(await copy(f.m, 'adage doctor', 'desktop')).toBe(true)
    expect(f.copies).toEqual([{ text: 'adage doctor', surface: 'desktop' }])
    expect(f.toasts).toEqual(['copied: adage doctor'])
    const g = fake({ copies: false })
    expect(await copy(g.m, 'adage doctor', 'elsewhere')).toBe(false)
    expect(g.copies).toEqual([{ text: 'adage doctor' }])
    expect(g.toasts).toEqual(['copy is not available here · the command is in /adage health'])
  })

  test('openPane and showReflex: the tab, focused, as a dialog; `why` lands on the reflex with the filters cleared', async () => {
    const f = fake()
    await openPane(f.m, 'health')
    expect(f.state.view.tab).toBe('health')
    expect(f.opened).toEqual([{ id: 'adage', title: 'adage', focus: true, closeOnEscape: true, holdToasts: true, rows: 20 }])
    f.state.view = { ...f.state.view, rfilter: 'x', strength: 'fire' }
    await showReflex(f.m, 'acme/learned/codegen-after-schema')
    expect(f.state.view).toMatchObject({ tab: 'reflexes', expanded: 'acme/learned/codegen-after-schema', rfilter: '', strength: 'all', lifecycle: 'all' })
  })

  test('the filter is kept as typed and cleared by an empty enter; the tab is remembered across sessions', async () => {
    const f = fake()
    const a = paneActions(f.m)
    a.filter('look', false)
    a.tab('reflexes')
    await f.m.clock.now()
    expect(f.state.view.filter).toBe('look')
    expect(f.store['adage.tab']).toBe('reflexes')
    a.filter('  ', true)
    await f.m.clock.now()
    expect(f.state.view.filter).toBe('')
  })

  test('no map without the CLI: nothing is spawned', async () => {
    const f = fake()
    expect(await explore(f.m)).toBeUndefined()
    expect(f.spawned).toEqual([])
  })

  test('the pane open or closed, for the band\'s noticed cell', async () => {
    const f = fake()
    await onPaneOpen(f.m, {} as never, (async () => ({ isPlaced: true })) as never)
    expect(f.m.rt.paneOpen).toBe(true)
    await onPaneClose(f.m, {} as never, (async () => undefined) as never)
    expect(f.m.rt.paneOpen).toBe(false)
  })
})
