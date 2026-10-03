// The seam B4 builds the asking on: which item is asked about next, and the dialog's words
// (ux-design §4.17, §10). Nothing here asks anything.
import { describe, expect, test } from 'claude-code/testing'

import { ASK_EVERY_MS, nextAsk, question } from './ask'
import { GRAPH } from './fixtures/kit'
import { reflexesOf, waiting } from './view'

const items = waiting(reflexesOf(GRAPH), [{ name: 'alice@acme.com', command: 'adage add .adage/packs/acme --trust' }])

describe('the next ask', () => {
  test('the first item not asked today and not dismissed', () => {
    expect(nextAsk(items, {}, new Set(), 0)?.key).toBe('acme/learned/test-after-acme-edit')
    expect(nextAsk(items, { 'acme/learned/test-after-acme-edit': 0 }, new Set(), 1000)?.key).toBe('pack:alice@acme.com')
    expect(nextAsk(items, { 'acme/learned/test-after-acme-edit': 0 }, new Set(), ASK_EVERY_MS)?.key).toBe('acme/learned/test-after-acme-edit')
    expect(nextAsk(items, {}, new Set(['acme/learned/test-after-acme-edit', 'pack:alice@acme.com']), 0)).toBeUndefined()
  })

  test("the dialog's words: two verbs, the reflex by what it tells the agent", () => {
    const [enable] = items
    if (enable === undefined) throw new Error('item')
    expect(question(enable)).toMatchObject({ question: 'Adage learned: run make test after editing acme/**. Tell the agent?', answers: ['enable', 'not this one', 'why'], command: 'adage review approve acme/learned test-after-acme-edit' })
    expect(question({ key: 'k', kind: 'allow', say: 'run make gen after a schema edit', text: '', command: 'c' })).toMatchObject({ question: 'Adage can run make gen after a schema edit, instead of asking the agent to. Allow it?', answers: ['see what it would do', 'not now'] })
    expect(question({ key: 'k', kind: 'trust', say: 'alice@acme.com', text: '', command: 'c' }).question).toBe('Reflexes from alice@acme.com are in this repository. Use them?')
  })
})
