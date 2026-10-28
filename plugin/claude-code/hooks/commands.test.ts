// `/adage <verb>` (commands.ts; ux-design §7; T-B3-09, T-B4-04, T-B4-08, T-O9-08): `runCommand` as a pure
// function of its deps, then `onCommand` through the engine (the recorded `$.ui.open`, argv and ask).
import { describe, expect, test } from 'claude-code/testing'

import { ON_TEXT, OFF_TEXT, PAUSED, RESUMED, runCommand, tabOf, verbOf } from './commands'
import type { CommandDeps } from './commands'
import { ON_QUESTION, UPDATE_ANSWERS } from './ask'
import { BAND, CARD, CLI, LINE, SURFACES, begin, mountPane, start, world } from './fixtures/kit'
import type { View } from './plugin'
import { cardOf } from './reads'

const FEED = [LINE.learnerIdle, LINE.healthy, LINE.start, LINE.firedGen, LINE.firedLookup]

function deps(over: Partial<CommandDeps> = {}) {
  const calls: string[] = []
  const d: CommandDeps = {
    card: async () => cardOf(CARD),
    open: async (tab: View['tab']) => void calls.push(`open:${tab}`),
    why: async () => 'No reflex has spoken in this session yet.',
    off: async () => 'adage is off for this repository (.adage/disabled). adage on turns it back on.',
    copy: async command => (calls.push(`copy:${command}`), true),
    pause: async () => true,
    map: async () => 'http://localhost:43123/',
    askOn: undefined,
    askUpdate: undefined,
    ...over,
  }
  return { d, calls }
}

describe('T-B3-09 · the verb', () => {
  test('the first word, any case; none or an unknown one is the card', () => {
    expect(verbOf('')).toBe('card')
    expect(verbOf('  Review now')).toBe('review')
    expect(verbOf('frobnicate')).toBe('card')
    expect(['graph', 'watch', 'reflexes', 'review', 'health', 'doctor', 'why', 'off', 'on', 'pause', 'map', 'status', 'savings'].map(v => verbOf(v))).toEqual(['graph', 'watch', 'reflexes', 'review', 'health', 'doctor', 'why', 'off', 'on', 'pause', 'map', 'status', 'savings'])
    expect([tabOf('card'), tabOf('graph'), tabOf('watch'), tabOf('reflexes'), tabOf('review'), tabOf('health'), tabOf('doctor'), tabOf('world'), tabOf('why')]).toEqual(['graph', 'graph', 'live', 'reflexes', 'reflexes', 'health', 'health', 'world', undefined])
  })
})

describe('T-B3-09, T-B4-04 · what each verb answers (ux-design §7)', () => {
  test('/adage: the card, and the pane opens on the Graph (T-B3-15)', async () => {
    const { d, calls } = deps()
    const r = await runCommand('', d)
    expect(r.text?.split('\n')[0]).toBe(CARD.header)
    expect(calls).toEqual(['open:graph'])
  })

  test('an unknown verb prints the card and opens nothing; status prints the card', async () => {
    const { d, calls } = deps()
    expect((await runCommand('frobnicate', d)).text?.split('\n')[0]).toBe(CARD.header)
    expect((await runCommand('status', d)).text?.split('\n')[0]).toBe(CARD.header)
    expect(calls).toEqual([])
  })

  test('graph, watch, reflexes, review, health and doctor open the pane and print nothing', async () => {
    const { d, calls } = deps()
    for (const v of ['graph', 'watch', 'reflexes', 'review', 'health', 'doctor']) expect(await runCommand(v, d)).toEqual({})
    expect(calls).toEqual(['open:graph', 'open:live', 'open:reflexes', 'open:reflexes', 'open:health', 'open:health'])
  })

  test('off runs the switch and says how to turn it back on; outside a repository the binary line', async () => {
    expect((await runCommand('off', deps().d)).text).toBe(OFF_TEXT)
    expect((await runCommand('off', deps({ off: async () => 'not inside a repository: `adage off` turns Adage off for the repository it runs in' }).d)).text).toStartWith('not inside a repository')
    expect((await runCommand('off', deps({ off: async () => undefined }).d)).text).toContain('run adage doctor')
  })

  test('on opens the ask and runs nothing (T-B4-04); with no one to ask, it says the terminal is the way', async () => {
    const { d, calls } = deps()
    expect((await runCommand('on', d)).text).toBe(ON_TEXT)
    let asked = 0
    expect(await runCommand('on', { ...d, askOn: () => void asked++ })).toEqual({})
    expect(asked).toBe(1)
    expect(calls).toEqual([])
  })

  test('pause and resume; why; map; savings', async () => {
    expect((await runCommand('pause', deps().d)).text).toBe(PAUSED)
    expect((await runCommand('pause', deps({ pause: async () => false }).d)).text).toBe(RESUMED)
    expect((await runCommand('why', deps().d)).text).toBe('No reflex has spoken in this session yet.')
    expect((await runCommand('map', deps().d)).text).toBe('Explore in browser: http://localhost:43123/')
    expect((await runCommand('savings', deps().d)).text).toContain('savings     measured')
    expect((await runCommand('savings', deps().d)).text).not.toContain('reflexes    ')
  })

  test('T-O9-08 · update: the ask when the card says a release is newer; the latest, or the terminal where no one can be asked', async () => {
    const offer = { have: '0.1.0', latest: '0.2.0', next: '/adage update', notes: ['fixes hook latency'] }
    const newer = async () => cardOf({ ...CARD, update: { current: '0.1.0', latest: '0.2.0', next: '/adage update', text: '0.2.0 is out (you have 0.1.0)', notes: 'fixes hook latency', channel: 'plugin' } })
    expect(verbOf('update')).toBe('update')
    expect((await runCommand('update', deps().d)).text).toBe('adage 0.1.0 is the latest.')
    expect((await runCommand('update', deps({ card: newer }).d)).text).toBe('Adage 0.2.0 is out; run adage update in a terminal.')
    const asked: unknown[] = []
    const { d, calls } = deps({ card: newer, askUpdate: u => void asked.push(u) })
    expect(await runCommand('update', d)).toEqual({})
    expect(asked).toEqual([offer])
    expect(calls).toEqual([])
    // A card that says nothing newer never opens it.
    expect((await runCommand('update', { ...deps().d, askUpdate: u => void asked.push(u) })).text).toBe('adage 0.1.0 is the latest.')
    expect(asked).toHaveLength(1)
    expect((await runCommand('update', deps({ card: async () => undefined }).d)).text).toBe('adage is not answering here; run adage doctor in a terminal.')
  })

  test('no card (the binary did not answer): one line that says what to do', async () => {
    expect((await runCommand('', deps({ card: async () => undefined }).d)).text).toBe('adage is not answering here; run adage doctor in a terminal.')
  })
})

describe('T-B3-09, T-B3-10, T-B4-08 · /adage through the engine', () => {
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
    for (const [verb, tab] of [['graph', 'graph'], ['watch', 'live'], ['reflexes', 'reflexes'], ['review', 'reflexes'], ['health', 'health'], ['doctor', 'health']] as const) {
      expect(await run(verb)).toBeUndefined()
      expect(w.opened.at(-1)).toEqual({ id: 'adage', title: 'adage', focus: true, closeOnEscape: true, holdToasts: true, rows: 20 })
      const ui = await mountPane($, 'terminal')
      expect((await ui.findAll({ key: `tab-${tab}` }))[0]?.props.dimColor).toBe(false)
      await ui.unmount()
    }
    expect((await run('frobnicate'))?.split('\n')[0]).toBe(CARD.header)
  })

  test('T-O9-07, T-O9-08: /adage update opens the ask; its `update` answer is the press of exactly [cli, update], token on stdin', async ($, on) => {
    const update = { current: '0.1.0', latest: '0.2.0', next: '/adage update', text: '0.2.0 is out (you have 0.1.0)', notes: null, channel: 'plugin' }
    const w = world(on, { feed: FEED, card: { ...CARD, update }, ask: (q, o) => (q.startsWith('Adage 0.2.0 is out') ? 'update' : o[0]) })
    await begin($, w)
    const ran = w.ran.length
    expect((await $.command.run({ command: 'adage', args: 'update' } as never)).text).toBeUndefined()
    await w.clock.settle()
    expect(w.asked).toEqual([{ question: 'Adage 0.2.0 is out (you have 0.1.0). Update now? It takes effect in your next session.', options: [...UPDATE_ANSWERS] }])
    const pressed = w.ran.slice(ran).filter(r => r.argv[1] === 'update')
    expect(pressed.map(r => r.argv)).toEqual([[CLI, 'update']])
    expect(JSON.parse(pressed[0]?.init?.stdin ?? '{}').press).toMatch(/^[0-9a-f]{64}$/)
    expect(pressed[0]?.init?.env?.CLAUDECODE).toBe('1')
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
