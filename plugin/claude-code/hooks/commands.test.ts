// `/adage <verb>` as a pure function of its deps (T-B3-09's text half; the engine half, the
// recorded `$.ui.open` and argv, is in draw.test.ts).
import { describe, expect, test } from 'claude-code/testing'

import { ON_TEXT, ON_TEXT_UNCOPIED, OFF_TEXT, PAUSED, RESUMED, runCommand, tabOf, verbOf } from './commands'
import type { CommandDeps } from './commands'
import { CARD } from './fixtures/kit'
import type { View } from './plugin'
import { cardOf } from './view'

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
    ...over,
  }
  return { d, calls }
}

describe('the verb', () => {
  test('the first word, any case; none or an unknown one is the card', () => {
    expect(verbOf('')).toBe('card')
    expect(verbOf('  Review now')).toBe('review')
    expect(verbOf('frobnicate')).toBe('card')
    expect(['watch', 'reflexes', 'review', 'health', 'doctor', 'why', 'off', 'on', 'pause', 'map', 'status', 'savings'].map(v => verbOf(v))).toEqual(['watch', 'reflexes', 'review', 'health', 'doctor', 'why', 'off', 'on', 'pause', 'map', 'status', 'savings'])
    expect([tabOf('watch'), tabOf('reflexes'), tabOf('review'), tabOf('health'), tabOf('doctor'), tabOf('why')]).toEqual(['live', 'reflexes', 'reflexes', 'health', 'health', undefined])
  })
})

describe('what each verb answers (ux-design §7)', () => {
  test('/adage: the card, and the pane opens on Live', async () => {
    const { d, calls } = deps()
    const r = await runCommand('', d)
    expect(r.text?.split('\n')[0]).toBe(CARD.header)
    expect(calls).toEqual(['open:live'])
  })

  test('an unknown verb prints the card and opens nothing; status prints the card', async () => {
    const { d, calls } = deps()
    expect((await runCommand('frobnicate', d)).text?.split('\n')[0]).toBe(CARD.header)
    expect((await runCommand('status', d)).text?.split('\n')[0]).toBe(CARD.header)
    expect(calls).toEqual([])
  })

  test('watch, reflexes, review, health and doctor open the pane and print nothing', async () => {
    const { d, calls } = deps()
    for (const v of ['watch', 'reflexes', 'review', 'health', 'doctor']) expect(await runCommand(v, d)).toEqual({})
    expect(calls).toEqual(['open:live', 'open:reflexes', 'open:reflexes', 'open:health', 'open:health'])
  })

  test('off runs the switch and says how to turn it back on; outside a repository the binary line', async () => {
    expect((await runCommand('off', deps().d)).text).toBe(OFF_TEXT)
    expect((await runCommand('off', deps({ off: async () => 'not inside a repository: `adage off` turns Adage off for the repository it runs in' }).d)).text).toStartWith('not inside a repository')
    expect((await runCommand('off', deps({ off: async () => undefined }).d)).text).toContain('run adage doctor')
  })

  test('on runs nothing and copies `adage on` (until the press, plan B4)', async () => {
    const { d, calls } = deps()
    expect((await runCommand('on', d)).text).toBe(ON_TEXT)
    expect(calls).toEqual(['copy:adage on'])
    expect((await runCommand('on', deps({ copy: async () => false }).d)).text).toBe(ON_TEXT_UNCOPIED)
  })

  test('pause and resume; why; map; savings', async () => {
    expect((await runCommand('pause', deps().d)).text).toBe(PAUSED)
    expect((await runCommand('pause', deps({ pause: async () => false }).d)).text).toBe(RESUMED)
    expect((await runCommand('why', deps().d)).text).toBe('No reflex has spoken in this session yet.')
    expect((await runCommand('map', deps().d)).text).toBe('Explore in browser: http://localhost:43123/')
    expect((await runCommand('savings', deps().d)).text).toContain('savings     measured')
    expect((await runCommand('savings', deps().d)).text).not.toContain('reflexes    ')
  })

  test('no card (the binary did not answer): one line that says what to do', async () => {
    expect((await runCommand('', deps({ card: async () => undefined }).d)).text).toBe('adage is not answering here; run adage doctor in a terminal.')
  })
})
