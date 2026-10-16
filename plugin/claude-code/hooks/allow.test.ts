// The execution grant's two acts (allow.ts; plan B4; ux-design §4.17 `ask.allow.pane`; T-B4-03)
// through the engine: stage, the grant pane on the facts, confirm against the digest; a moved
// repository, a refused term, `not now`, and a prediction offer as one act.
import { describe, expect, test } from 'claude-code/testing'

import { CLI, CONFIRMED, GRAPH_OFFER, GRAPH_PREDICT, LINE, SESSION, STAGE, STAGE_MOVED, THIRD_TOKEN, begin, mountPane, personOnly, stdin, turnEnd, world } from './fixtures/kit'
import { ASK_EVERY_MS } from './ask'
import { FIRST_LINE } from './grant'

const FEED = [LINE.learnerIdle, LINE.healthy, LINE.start, LINE.pressIssued]
const ALLOW_Q = 'Adage can do this itself, instead of asking the agent: run make gen after a schema edit. See what it would do?'
const PREDICT_Q = 'Adage can take this step without a model request: run make gen after a schema edit. Allow it?'
const STAGE_ARGV = [CLI, 'review', 'delegate', 'acme/learned', 'codegen-after-schema', '--level', 'e2', '--stage', '--json']

describe('T-B4-03 · two acts with the facts', () => {
  test('a confirm answering a fresh diff redraws the pane and records nothing; a changed term is refused with its reason', async ($, on) => {
    let confirm: unknown = STAGE_MOVED
    const w = world(on, { feed: FEED, graph: GRAPH_OFFER, ask: () => 'see what it would do', confirm: () => confirm })
    await begin($, w)
    await turnEnd($, w)
    let ui = await mountPane($, 'terminal', 120, 'adage-allow')
    await ui.press({ key: 'allow' })
    await w.clock.settle()
    await ui.unmount()
    ui = await mountPane($, 'terminal', 120, 'adage-allow')
    const t = (await ui.findAll({ type: 'Text' })).map(x => x.text)
    expect(t).toContain('the repository moved meanwhile; this is what it would do now')
    expect(t).toContain('it changes acme/generated/models.py  (+2 −0, shown below)')
    expect(String((await ui.findAll({ type: 'Code' }))[0]?.props.source)).toContain('+    locale: str = "en"')
    expect(w.closed).toEqual([])
    expect(w.toasts.some(x => x.startsWith('allowed'))).toBe(false)
    // Confirmed against the fresh digest.
    confirm = CONFIRMED
    await ui.press({ key: 'allow' })
    await w.clock.settle()
    expect(stdin(personOnly(w)[2])).toEqual({ expect: STAGE_MOVED.digest, press: THIRD_TOKEN, session: SESSION })
    expect(w.closed).toEqual(['adage-allow'])
  })

  test('not now at either act runs nothing; the first-run line only when the fixture says so', async ($, on) => {
    const w = world(on, { feed: FEED, graph: GRAPH_OFFER, ask: () => 'not now', stage: { ...STAGE, first: false } })
    await begin($, w)
    await turnEnd($, w)
    expect(personOnly(w)).toEqual([])
    await w.clock.advance(ASK_EVERY_MS)
    await turnEnd($, w)
    expect(w.asked).toHaveLength(1)
    // From the pane: the stage, then not now.
    const ui = await mountPane($)
    await ui.press({ key: 'tab-reflexes' })
    await ui.press({ key: 'yes:acme/learned/codegen-after-schema#execute' })
    await w.clock.settle()
    expect(personOnly(w).map(r => r.argv)).toEqual([STAGE_ARGV])
    const grant = await mountPane($, 'terminal', 120, 'adage-allow')
    expect((await grant.findAll({ type: 'Text' })).map(x => x.text)).not.toContain(FIRST_LINE)
    await grant.press({ key: 'not-now' })
    await w.clock.settle()
    expect(personOnly(w).map(r => r.argv)).toEqual([STAGE_ARGV])
    expect(w.closed).toEqual(['adage-allow'])
  })

  test('a prediction offer is one act: allow runs --predict; why opens the pane', async ($, on) => {
    const answers = ['why', 'allow']
    let id = SESSION
    const w = world(on, { feed: FEED, graph: GRAPH_PREDICT, ask: () => answers.shift(), session: () => id })
    await begin($, w)
    await turnEnd($, w)
    expect(w.asked[0]).toEqual({ question: PREDICT_Q, options: ['allow', 'not now', 'why'] })
    expect(w.opened.at(-1)).toMatchObject({ id: 'adage', focus: true })
    await w.clock.advance(ASK_EVERY_MS)
    id = 'sess-2'
    await $.classic.SessionStart({ source: 'startup', session_id: 'sess-2', cwd: '/w' } as never)
    await w.clock.settle()
    await turnEnd($, w)
    expect(personOnly(w).map(r => r.argv)).toEqual([[CLI, 'review', 'delegate', 'acme/learned', 'codegen-after-schema', '--predict']])
    expect(w.toasts).toContain('allowed: run make gen after a schema edit · from your next session')
  })
})
