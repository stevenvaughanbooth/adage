// Which item is asked about next, the dialog's words, and what an answer records (ux-design §4.17,
// §10; T-B4-01's choice, T-B4-02's `not this one`/`later` rules, T-B4-09's words). Nothing here asks.
import { describe, expect, test } from 'claude-code/testing'

import { ANSWERS, ASK_EVERY_MS, KEPT_ANSWERS, MAX_DISMISSALS, ON_ANSWERS, ON_QUESTION, answerOf, askedOf, doneToast, keptOf, keptQuestion, nextAsk, question, record, whatOf } from './ask'
import { BANNED, GRAPH, GRAPH_OFFER, GRAPH_PREDICT } from './fixtures/kit'
import { reflexesOf, waiting } from './view'

const PACK = [{ name: 'alice@acme.com', command: 'adage add .adage/packs/acme --trust' }]
const items = waiting(reflexesOf(GRAPH), PACK)
const ENABLE = 'acme/learned/test-after-acme-edit'

describe('the next ask', () => {
  test('the first item not asked today, not answered, dismissed fewer than twice', () => {
    expect(nextAsk(items, [], 0)?.key).toBe(ENABLE)
    expect(nextAsk(items, [{ key: ENABLE, at: 0, times: 1, done: false }], 1000)?.key).toBe('pack:alice@acme.com')
    expect(nextAsk(items, [{ key: ENABLE, at: 0, times: 1, done: false }], ASK_EVERY_MS)?.key).toBe(ENABLE)
    expect(nextAsk(items, [{ key: ENABLE, at: 0, times: MAX_DISMISSALS, done: false }], ASK_EVERY_MS)?.key).toBe('pack:alice@acme.com')
    expect(nextAsk(items, [{ key: ENABLE, at: 0, times: 0, done: true }], ASK_EVERY_MS)?.key).toBe('pack:alice@acme.com')
    expect(nextAsk(items, [{ key: ENABLE, at: 0, times: 0, done: true }, { key: 'pack:alice@acme.com', at: 0, times: 0, done: true }], ASK_EVERY_MS)).toBeUndefined()
  })

  test("the dialog's words: two verbs, the reflex by what it tells the agent, never its id", () => {
    const [enable] = items
    if (enable === undefined) throw new Error('item')
    expect(question(enable)).toMatchObject({ question: 'Adage learned: run make test after editing acme/**. Tell the agent?', answers: ['enable', 'not this one', 'why'], command: 'adage review approve acme/learned test-after-acme-edit' })
    expect(enable.no).toBe('adage review dismiss acme/learned test-after-acme-edit')
    const [execute] = waiting(reflexesOf(GRAPH_OFFER), [])
    expect(question(execute as never)).toMatchObject({ question: 'Adage can run make gen after a schema edit, instead of asking the agent to. Allow it?', answers: ['see what it would do', 'not now'] })
    const [predict] = waiting(reflexesOf(GRAPH_PREDICT), [])
    expect(predict?.offer).toBe('predict')
    expect(question(predict as never).answers).toEqual(['allow', 'not now', 'why'])
    expect(question({ key: 'k', kind: 'trust', say: 'alice@acme.com', text: '', command: 'c' }).question).toBe('Reflexes from alice@acme.com are in this repository. Use them?')
    expect(ON_QUESTION).toBe('Adage is off in this repository. Turn it on?')
    expect(ON_ANSWERS).toEqual(['turn it on', 'leave it off'])
    for (const w of [...Object.values(ANSWERS).flat(), ...ON_ANSWERS, ON_QUESTION, ...items.map(i => question(i).question)]) expect(w).not.toMatch(BANNED)
  })

  test('an answer is one of the labels exactly; free text is none', () => {
    const q = { answers: ['enable', 'not this one', 'why'] }
    expect(answerOf(q, 'enable')).toBe('enable')
    expect(answerOf(q, 'Enable')).toBeUndefined()
    expect(answerOf(q, 'later please')).toBeUndefined()
    expect(answerOf(q, undefined)).toBeUndefined()
  })

  test('what an answer records: a dismissal counts, why leaves it open, any other answer closes it', () => {
    const later = record([], ENABLE, undefined, 5)
    expect(later).toEqual([{ key: ENABLE, at: 5, times: 1, done: false }])
    const why = record(later, ENABLE, 'why', 6)
    expect(why[0]).toMatchObject({ times: 1, done: false, at: 6 })
    const done = record(why, ENABLE, 'enable', 7)
    expect(done[0]).toMatchObject({ done: true, at: 7 })
    expect(record(done, ENABLE, undefined, 8)[0]).toMatchObject({ done: true, times: 2 })
    expect(askedOf(JSON.parse(JSON.stringify(done)))).toEqual(done)
    expect(askedOf('x')).toEqual([])
    expect(askedOf([{ key: 'k' }])).toEqual([])
  })

  test('what a command decides, and the toast when it lands', () => {
    expect(whatOf('adage review approve acme/learned r')).toBe('approve')
    expect(whatOf('adage review dismiss acme/learned r')).toBe('dismiss')
    expect(whatOf('adage review delegate acme/learned r --level e2 --stage --json')).toBe('stage')
    expect(whatOf('adage review delegate acme/learned r --level e2 --confirm')).toBe('confirm')
    expect(whatOf('adage review delegate acme/learned r --predict')).toBe('predict')
    expect(whatOf('adage on')).toBe('on')
    expect(whatOf('adage add .adage/packs/x --trust')).toBe('trust')
    expect(whatOf('ls')).toBe('')
    expect(doneToast('approve', 'run make test after editing acme/**')).toBe('on: run make test after editing acme/**')
    expect(doneToast('predict', 'run make gen after a schema edit')).toBe('allowed: run make gen after a schema edit · from your next session')
    expect(doneToast('on', 'adage')).toBe('adage is on for this repository.')
    expect(doneToast('pin', 'x', 'pin it')).toBe('done: pin it · x')
    expect(doneToast('stage', 'x')).toBeUndefined()
    expect(doneToast('confirm', 'x')).toBeUndefined()
  })

  test('H2: a kept answer names the repository it was given in; one without a root is offered nowhere', () => {
    const on = { command: 'adage on', say: 'adage', key: 'on', session: 's', root: '/w', at: 1 }
    expect(keptOf([on, { ...on, root: '' }, { command: 'adage on', at: 1 }, { nope: 1 }, 'x'])).toEqual([on])
  })

  test('H1: a kept answer is asked again in words that name the exact command it runs', () => {
    expect(KEPT_ANSWERS).toEqual(['run it', 'drop it'])
    expect(keptQuestion({ command: 'adage on', say: 'adage' })).toBe('Kept from your last session: adage on. Run it now?')
    expect(keptQuestion({ command: 'adage review approve acme/learned r', say: 'run make test after editing acme/**' })).toBe(
      'Kept from your last session: run make test after editing acme/** (adage review approve acme/learned r). Run it now?',
    )
  })
})
