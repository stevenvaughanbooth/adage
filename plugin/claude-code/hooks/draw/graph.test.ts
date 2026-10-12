// The pane's Graph tab through the engine (draw/graph.tsx, its site draw/sites.ts `onPane`;
// ux-design §6.0; T-B3-14, T-B3-15): the pane opens on it, empty it says what will grow there, it
// grows as the command hooks fire, a reflex's box in its strength's role, the whole session behind
// `a`, the one-line list on the desktop, ASCII twins under ADAGE_ASCII=1.
import { describe, expect, test } from 'claude-code/testing'

import { LINE, SURFACES, block, mountPane, prompt, shown, start, world } from '../fixtures/kit'
import { ROLES } from '../tokens'
import { GRAPH_EMPTY } from '../view/graph'

const FEED = [LINE.learnerIdle, LINE.healthy, LINE.start, LINE.firedGen, LINE.firedLookup]
const CLASSIC = {
  UserPromptSubmit: { additionalContext: [block('lookup-work-item')] },
  PostToolBatch: { additionalContext: [block('codegen-after-schema')] },
}
const EDIT = { tool_calls: [{ tool_name: 'Edit', tool_input: { file_path: '/w/schema/invoice.json' }, tool_use_id: 'e1' }] }

describe('T-B3-14, T-B3-15 · the Graph tab through the engine', () => {
  test('T-B3-15: the pane opens on the Graph; empty, it says what will grow there; /adage graph opens it', async ($, on) => {
    const w = world(on, { feed: [LINE.learnerIdle] })
    await start($, w)
    for (const surface of SURFACES) {
      const ui = await mountPane($, surface)
      expect((await ui.findAll({ key: 'tab-graph' }))[0]?.props.dimColor).toBe(false)
      expect(await shown(ui)).toContain(GRAPH_EMPTY)
      await ui.unmount()
    }
    const ui = await mountPane($, 'terminal')
    await ui.press({ key: 'tab-live' })
    await $.command.run({ command: 'adage', args: 'graph' } as never)
    await w.clock.settle()
    expect(w.opened.at(-1)).toMatchObject({ id: 'adage', focus: true })
    expect((await ui.findAll({ key: 'tab-graph' }))[0]?.props.dimColor).toBe(false)
  })

  test('T-B3-14: it grows as the hooks fire: the prompt\'s box, the reflex at it, the step, the reflex after the step, in the fire role', async ($, on) => {
    const w = world(on, { feed: FEED, classic: CLASSIC })
    await start($, w)
    const ui = await mountPane($, 'terminal', 120)
    await prompt($, 'fix W-2042 and add the currency field')
    await w.clock.settle()
    let texts = await ui.findAll({ type: 'Text' })
    expect(texts.some(t => t.text === 'PROMPT 1')).toBe(true)
    expect(texts.some(t => t.text === 'fix W-2042 and add the currency field')).toBe(true)
    expect(texts.filter(t => t.text === '◆ adage').map(t => t.props.color)).toEqual([ROLES.fire])
    // What the reflex tells the agent, cut to its box (the response beside it, as the feed named it).
    expect(texts.some(t => t.text.startsWith('look up the work item with'))).toBe(true)
    expect(texts.some(t => t.text === 'scripts/wi-lookup W-2042 --json')).toBe(true)
    expect(texts.some(t => t.text === 'at your prompt · now')).toBe(true)
    await $.classic.PostToolBatch(EDIT as never)
    await w.clock.settle()
    texts = await ui.findAll({ type: 'Text' })
    expect(texts.some(t => t.text === 'Edit')).toBe(true)
    expect(texts.some(t => t.text === 'schema/invoice.json')).toBe(true)
    expect(texts.filter(t => t.text === '◆ adage').map(t => t.props.color)).toEqual([ROLES.fire, ROLES.fire])
    expect(texts.some(t => t.text === 'run make gen after a schema edit')).toBe(true)
    // The summary above it counts what Adage did.
    expect(texts.find(t => t.text === 'adage fired 2 reflexes')?.props.color).toBe(ROLES.fire)
    for (const t of texts) if (t.props.color !== undefined) expect(Object.values(ROLES) as string[]).toContain(String(t.props.color))
  })

  test('T-B3-14: following the newest within the pane\'s rows; `a` shows the whole session and follows again', async ($, on) => {
    const w = world(on, { feed: FEED, classic: CLASSIC })
    await start($, w)
    await prompt($, 'first')
    for (let k = 0; k < 12; k++) await $.classic.PostToolBatch({ tool_calls: [{ tool_name: 'Bash', tool_input: { command: `make step${k}` }, tool_use_id: `b${k}` }] } as never)
    await w.clock.settle()
    const ui = await mountPane($, 'terminal', 120)
    const all = async () => (await ui.findAll({ type: 'Text' })).map(t => t.text)
    expect((await all()).some(t => t.startsWith('┊ ↑ earlier: '))).toBe(true)
    expect((await all()).includes('make step11')).toBe(true)
    expect((await all()).includes('PROMPT 1')).toBe(false)
    expect((await ui.findAll({ key: 'history' }))[0]?.props).toMatchObject({ hotkey: 'a', label: 'whole session' })
    await ui.press({ key: 'history' })
    expect((await all()).includes('PROMPT 1')).toBe(true)
    expect((await all()).some(t => t.startsWith('┊ ↑ earlier: '))).toBe(false)
    expect((await ui.findAll({ key: 'history' }))[0]?.props.label).toBe('follow the newest')
    await ui.press({ key: 'history' })
    expect((await all()).includes('PROMPT 1')).toBe(false)
  })

  test('T-B3-14: the desktop draws the one-line list at any width (no box, no Button in a line); ADAGE_ASCII=1 draws every twin', async ($, on) => {
    const w = world(on, { feed: FEED, classic: CLASSIC, env: { ADAGE_ASCII: '1' } })
    await start($, w)
    await prompt($, 'fix W-2042')
    await $.classic.PostToolBatch(EDIT as never)
    await w.clock.settle()
    const desk = await mountPane($, 'desktop', 160)
    const texts = await desk.findAll({ type: 'Text' })
    expect(texts.some(t => t.text.includes('+-'))).toBe(false)
    expect(texts.some(t => t.text === 'PROMPT 1')).toBe(true)
    expect((await desk.findAll({ type: 'Button' })).filter(b => !b.key?.startsWith('tab-') && !['history', 'explore'].includes(b.key ?? ''))).toEqual([])
    const term = await mountPane($, 'terminal', 120)
    for (const t of await term.findAll({ type: 'Text' })) expect(t.text).toMatch(/^[\x20-\x7e]*$/)
    expect((await term.findAll({ type: 'Text' })).some(t => t.text.startsWith('+- '))).toBe(true)
  })
})
