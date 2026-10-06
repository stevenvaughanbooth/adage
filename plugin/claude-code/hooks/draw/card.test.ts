// `/adage` and `/adage why` drawn in their output row (draw/card.tsx and its site; ux-design §7;
// T-B3-09): the status card's header, key column and `next`, the terminal's commands behind Copy,
// and the last directive; the control arm's `why` says nothing spoke.
import { describe, expect, test } from 'claude-code/testing'

import { CARD, LINE, OUTPUT, SURFACES, start, world } from '../fixtures/kit'

describe('T-B3-09 · the card and why, drawn', () => {
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

  test('a Copy beside a terminal command copies it on the surface pressed from; a one-line output and an errored one are left as printed', async ($, on) => {
    const w = world(on, { feed: [LINE.learnerIdle] })
    await start($, w)
    const card = (await $.command.run({ command: 'adage', args: '' } as never)).text ?? ''
    const drawn = await $.ui.mount({ plugin: 'adage', surface: 'desktop', component: 'CommandOutput', props: OUTPUT('', card) })
    const [first] = (await drawn.findAll({ type: 'Button' })).map(b => b.key)
    await drawn.press({ key: first ?? '' })
    expect(w.copies.at(-1)).toBe('adage map')
    const one = await $.ui.mount({ plugin: 'adage', surface: 'terminal', component: 'CommandOutput', props: OUTPUT('off', 'adage is off') })
    expect((await one.findAll({ type: 'Text' })).map(x => x.text)).toEqual(['ENGINE CommandOutput: adage is off'])
    const errored = await $.ui.mount({ plugin: 'adage', surface: 'terminal', component: 'CommandOutput', props: { ...OUTPUT('', card), isErrored: true } })
    expect((await errored.findAll({ type: 'Text' })).map(x => x.text)[0]).toStartWith('ENGINE CommandOutput')
  })
})
