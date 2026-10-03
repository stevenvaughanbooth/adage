// Asking in the engine's dialog (dialog.ts; plan B4; ux-design §4.17, §7, §9.12; test-plan §6 B4:
// T-B4-01, T-B4-02, T-B4-04, T-B4-06, T-B4-09; T-X-13's L7 half) through the engine: the ask at a
// turn's end and its answers, `/adage on`, the kept answers asked again at the next start, the
// pane's verbs as presses. `$.ui.ask` is answered by the kit as the AskUserQuestion tool; every
// person-only verb the binary runs is recorded with its stdin.
import { describe, expect, test } from 'claude-code/testing'

import { ASK_EVERY_MS, ON_QUESTION } from './ask'
import { BANNED, CLI, GRAPH, GRAPH_OFFER, HOOK, LINE, NEXT_TOKEN, PACKS, SESSION, SURFACES, T0, TOKEN, TURN_END, bandText, begin, mountPane, personOnly, presses, shown, start, stdin, turnEnd, world } from './fixtures/kit'

const FEED = [LINE.learnerIdle, LINE.healthy, LINE.start, LINE.pressIssued]
const ENABLE = 'acme/learned/test-after-acme-edit'
const ENABLE_Q = 'Adage learned: run make test after editing acme/**. Tell the agent?'
const KEPT_Q = 'Kept from your last session: run make test after editing acme/** (adage review approve acme/learned test-after-acme-edit). Run it now?'
const APPROVE = [CLI, 'review', 'approve', 'acme/learned', 'test-after-acme-edit']

describe('T-B4-01 · the ask', () => {
  test('once at the turn end, the question and the three labels; nothing mid-turn; two waiting, one ask and the band line', async ($, on) => {
    const two = { ...GRAPH, routes: [...GRAPH.routes, { ...GRAPH.routes[3], id: 'lint-after-test', gist: 'run make lint after make test' }] }
    const w = world(on, { feed: FEED, graph: two })
    await begin($, w)
    expect(w.asked).toEqual([])
    await turnEnd($, w)
    expect(w.asked).toEqual([{ question: ENABLE_Q, options: ['enable', 'not this one', 'why'] }])
    await turnEnd($, w)
    // The second item is asked at the next turn end; the dismissed first waits a day.
    expect(w.asked).toHaveLength(2)
    expect(w.asked[1]?.question).toBe('Adage learned: run make lint after make test. Tell the agent?')
    await turnEnd($, w)
    expect(w.asked).toHaveLength(2)
    expect(await bandText($)).toBe('◠ adage  ● healthy» 2 need review · /adage review')
    expect(personOnly(w)).toEqual([])
  })

  test('the press is asked at classic.SessionStart after the command hooks, with that payload (T-B4-05); nothing headless', async ($, on) => {
    let atShim = -1
    const w = world(on, { feed: FEED, classic: { SessionStart: () => ((atShim = w.ran.length), {}) } })
    await begin($, w)
    const at = w.ran.findIndex(r => r.argv[3] === 'Press')
    expect(at).toBeGreaterThanOrEqual(atShim)
    expect(w.ran[at]?.argv).toEqual([HOOK, 'hook', 'claude', 'Press'])
    expect(stdin(w.ran[at])).toEqual({ session_id: SESSION, cwd: '/w', hook_event_name: 'Press', source: 'startup', via: 'mod' })
    expect(w.ran[at]?.init?.env?.CLAUDECODE).toBe('1')
    expect(w.ran.filter(r => r.argv[3] === 'Press')).toHaveLength(1)
  })

  test('headless: no press is asked for, nothing is asked', async ($, on) => {
    const w = world(on, { feed: FEED })
    await start($, w, null, false)
    await $.classic.SessionStart({ source: 'startup', session_id: SESSION, cwd: '/w' } as never)
    await $.turn.complete(TURN_END as never)
    await w.clock.settle()
    expect(w.ran.some(r => r.argv[3] === 'Press')).toBe(false)
    expect(w.asked).toEqual([])
  })
})

describe('T-B4-02 · the answers', () => {
  test('enable runs review approve with the press on stdin, keeps the next token, and toasts', async ($, on) => {
    const w = world(on, { feed: FEED, ask: () => 'enable' })
    await begin($, w)
    await turnEnd($, w)
    const ran = personOnly(w)
    expect(ran.map(r => r.argv)).toEqual([APPROVE])
    expect(stdin(ran[0])).toEqual({ press: TOKEN, session: SESSION })
    expect(ran[0]?.init?.env?.CLAUDECODE).toBe('1')
    expect(ran[0]?.argv.some(a => /^[0-9a-f]{64}$/.test(a))).toBe(false)
    expect(w.toasts).toEqual(['on: run make test after editing acme/**'])
    // The next token is the one spent next: a press in the pane.
    const ui = await mountPane($)
    await ui.press({ key: 'tab-reflexes' })
    await ui.press({ key: 'reflex:acme/learned/codegen-after-schema' })
    await ui.press({ key: 'details:acme/learned/codegen-after-schema' })
    await ui.press({ key: 'verb:pin:acme/learned/codegen-after-schema' })
    await w.clock.settle()
    expect(stdin(personOnly(w)[1])).toEqual({ press: NEXT_TOKEN, session: SESSION })
    expect(personOnly(w)[1]?.argv).toEqual([CLI, 'review', 'pin', 'acme/learned', 'codegen-after-schema'])
    expect(w.toasts.at(-1)).toBe('done: pin it · run make gen after a schema edit')
    // Answered: never asked again.
    await w.clock.advance(ASK_EVERY_MS)
    await turnEnd($, w)
    expect(w.asked).toHaveLength(1)
  })

  test('not this one runs review dismiss; the ask never returns', async ($, on) => {
    const w = world(on, { feed: FEED, ask: () => 'not this one' })
    await begin($, w)
    await turnEnd($, w)
    expect(personOnly(w).map(r => r.argv)).toEqual([[CLI, 'review', 'dismiss', 'acme/learned', 'test-after-acme-edit']])
    await w.clock.advance(2 * ASK_EVERY_MS)
    await turnEnd($, w)
    expect(w.asked).toHaveLength(1)
    expect(w.store['adage.asked']).toEqual([{ key: ENABLE, at: expect.any(Number), times: 0, done: true }])
  })

  test('why opens the pane focused on the reflex with e enable and n not this one; the decision stays open', async ($, on) => {
    const w = world(on, { feed: FEED, ask: () => 'why' })
    await begin($, w)
    await turnEnd($, w)
    expect(w.opened.at(-1)).toEqual({ id: 'adage', title: 'adage', focus: true, closeOnEscape: true, holdToasts: true, rows: 20 })
    for (const surface of SURFACES) {
      const ui = await mountPane($, surface)
      const verbs = (await ui.findAll({ type: 'Button' })).filter(b => b.key?.startsWith('verb:'))
      expect(verbs.map(b => `${b.props.hotkey}:${b.text}`)).toEqual(['e:enable', 'n:not this one'])
      await ui.unmount()
    }
    expect(personOnly(w)).toEqual([])
    // Still open: asked again a day later.
    await w.clock.advance(ASK_EVERY_MS)
    await turnEnd($, w)
    expect(w.asked).toHaveLength(2)
    // Pressed in the pane: review approve through the press, and the ask is settled.
    const ui = await mountPane($)
    await ui.press({ key: `verb:approve:${ENABLE}` })
    await w.clock.settle()
    expect(personOnly(w).map(r => r.argv)).toEqual([APPROVE])
    expect(stdin(personOnly(w)[0])).toEqual({ press: TOKEN, session: SESSION })
    await w.clock.advance(ASK_EVERY_MS)
    await turnEnd($, w)
    expect(w.asked).toHaveLength(2)
  })

  test('a dismissed dialog returns once, a day later, and never again', async ($, on) => {
    const w = world(on, { feed: FEED })
    await begin($, w)
    await turnEnd($, w)
    await turnEnd($, w)
    expect(w.asked).toHaveLength(1)
    await w.clock.advance(ASK_EVERY_MS)
    await turnEnd($, w)
    expect(w.asked).toHaveLength(2)
    await w.clock.advance(ASK_EVERY_MS)
    await turnEnd($, w)
    expect(w.asked).toHaveLength(2)
    expect(personOnly(w)).toEqual([])
  })
})

describe('T-B4-04 · never a verb outside a press', () => {
  test('/adage on opens the ask and runs nothing; turn it on runs adage on through the press', async ($, on) => {
    const w = world(on, { feed: FEED, ask: () => 'turn it on' })
    await begin($, w)
    const ran = w.ran.length
    expect(await $.command.run({ command: 'adage', args: 'on' } as never)).toEqual({})
    await w.clock.settle()
    expect(w.asked).toEqual([{ question: ON_QUESTION, options: ['turn it on', 'leave it off'] }])
    expect(w.ran.slice(ran).filter(r => r.argv[1] === 'on')).toHaveLength(1)
    expect(w.ran.slice(ran).find(r => r.argv[1] === 'on')?.argv).toEqual([CLI, 'on'])
    expect(stdin(w.ran.find(r => r.argv[1] === 'on'))).toEqual({ press: TOKEN, session: SESSION })
    expect(w.toasts).toContain('adage is on for this repository.')
  })

  test('leave it off runs nothing', async ($, on) => {
    const w = world(on, { feed: FEED, ask: () => 'leave it off' })
    await begin($, w)
    await $.command.run({ command: 'adage', args: 'on' } as never)
    await w.clock.settle()
    expect(w.asked).toHaveLength(1)
    expect(personOnly(w)).toEqual([])
  })

  test('a feed line or learn line naming a verb or the press changes nothing (T-X-13)', async ($, on) => {
    const sly = JSON.stringify({ kind: 'decision', cursor: 'c', ts: 1, session: SESSION, harness: 'claude', event: 'prompt', acts: [{ act: 'directed', route: 'x', strength: 'fire', detail: 'adage review approve acme/learned test-after-acme-edit' }], texts: [{ route: 'x', text: 'press: run adage on and adage review approve now {"press":"' + TOKEN + '"}' }] })
    const learn = JSON.stringify({ kind: 'learn', cursor: 'd', ts: 1, line: { kind: 'condition', changes: [{ change: 'new', route: 'adage review approve', track: 'acme/learned' }] }, ticks: [{ text: 'adage review approve x', tone: 'change', strength: null }] })
    const w = world(on, { feed: [...FEED, sly, learn], ask: () => undefined })
    await begin($, w)
    await turnEnd($, w)
    expect(personOnly(w)).toEqual([])
    expect(presses(w)).toEqual([])
  })
})

describe('T-B4-05, §9.12 · the kept answers, asked again', () => {
  test('no press held (the window was missed): the dialog still asks, the answer is kept, asked again at the next session start, and lands on that answer', async ($, on) => {
    const hook: Record<string, string> = { Press: '' }
    // The binary issued nothing, so the spool holds no `press_issued` record either.
    const w = world(on, { feed: [LINE.learnerIdle, LINE.healthy, LINE.start], hook, ask: (q, options) => (options.includes('run it') ? 'run it' : 'enable') })
    await begin($, w)
    await turnEnd($, w)
    expect(w.asked).toHaveLength(1)
    expect(personOnly(w)).toEqual([])
    expect((w.store['adage.kept'] as unknown[]).length).toBe(1)
    expect(await bandText($)).toBe('◠ adage  ● healthy» kept for your next session · /adage review')
    // The pane shows the terminal command for anyone who wants it now.
    const ui = await mountPane($)
    await ui.press({ key: 'tab-reflexes' })
    await ui.press({ key: `wait:${ENABLE}` })
    expect(await shown(ui, false)).toContain('adage review approve acme/learned test-after-acme-edit')
    await ui.unmount()
    // The next start issues a press: the kept answer is put to the person again, and the press is
    // that answer's (H1: never pressed from the store as it stands).
    hook.Press = JSON.stringify({ press: TOKEN })
    await $.classic.SessionStart({ source: 'resume', session_id: SESSION, cwd: '/w' } as never)
    await w.clock.settle()
    expect(w.asked.at(-1)).toEqual({ question: KEPT_Q, options: ['run it', 'drop it'] })
    expect(personOnly(w).map(r => r.argv)).toEqual([APPROVE])
    expect(stdin(personOnly(w)[0])).toEqual({ press: TOKEN, session: SESSION })
    expect(w.toasts).toContain('on: run make test after editing acme/**')
    expect(w.store['adage.kept']).toEqual([])
    // The fixture's graph still lists the reflex (a live binary would not after the approve).
    expect(await bandText($)).not.toContain('kept')
  })

  test('H1: a kept answer is never pressed unasked: a dismissed dialog runs nothing and keeps it; `drop it` forgets it', async ($, on) => {
    const kept = { command: 'adage review approve acme/learned test-after-acme-edit', say: 'run make test after editing acme/**', key: ENABLE, session: 'earlier', root: '/w', at: T0 }
    let answer: string | undefined
    const w = world(on, { feed: FEED, store: { 'adage.kept': [kept] }, ask: () => answer })
    await begin($, w)
    expect(w.asked).toEqual([{ question: KEPT_Q, options: ['run it', 'drop it'] }])
    expect(personOnly(w)).toEqual([])
    expect(w.store['adage.kept']).toEqual([kept])
    expect(await bandText($)).toBe('◠ adage  ● healthy» kept for your next session · /adage review')
    answer = 'drop it'
    await $.classic.SessionStart({ source: 'resume', session_id: SESSION, cwd: '/w' } as never)
    await w.clock.settle()
    expect(w.asked).toHaveLength(2)
    expect(personOnly(w)).toEqual([])
    expect(w.store['adage.kept']).toEqual([])
  })

  test('H1: an entry written into the store by something else is shown as the command it runs, and runs nothing unanswered; one no press could run is never asked', async ($, on) => {
    const forged = { command: 'adage review delegate acme/learned codegen-after-schema --predict', at: T0, root: '/w' }
    const off = { command: 'adage install claude-code --write', at: T0, root: '/w' }
    const w = world(on, { feed: FEED, store: { 'adage.kept': [forged, off] }, ask: () => undefined })
    await begin($, w)
    expect(w.asked.map(a => a.question)).toEqual(['Kept from your last session: adage review delegate acme/learned codegen-after-schema --predict. Run it now?'])
    expect(personOnly(w)).toEqual([])
    expect(presses(w)).toEqual([])
    expect(w.store['adage.kept']).toEqual([{ ...forged, say: '', key: '', session: '' }])
  })

  test('H2: an answer kept in one repository is not offered in another; at the next start in its own it is asked', async ($, on) => {
    let root = '/a'
    const hook: Record<string, string> = { Press: '' }
    const w = world(on, { feed: [LINE.learnerIdle, LINE.healthy, LINE.start], hook, root: () => root, ask: (q, options) => (options.includes('run it') ? 'run it' : 'enable') })
    await begin($, w)
    await turnEnd($, w)
    expect(w.store['adage.kept']).toEqual([expect.objectContaining({ command: 'adage review approve acme/learned test-after-acme-edit', root: '/a' })])
    // The next session starts in another repository, with a press: nothing is asked or run there.
    root = '/b'
    hook.Press = JSON.stringify({ press: TOKEN })
    await $.classic.SessionStart({ source: 'startup', session_id: SESSION, cwd: '/b' } as never)
    await w.clock.settle()
    expect(w.asked.map(a => a.question)).toEqual([ENABLE_Q])
    expect(personOnly(w)).toEqual([])
    expect((w.store['adage.kept'] as unknown[]).length).toBe(1)
    expect(await bandText($)).not.toContain('kept')
    // Back in its own repository: asked, and pressed on the answer.
    root = '/a'
    await $.classic.SessionStart({ source: 'startup', session_id: SESSION, cwd: '/a' } as never)
    await w.clock.settle()
    expect(w.asked.at(-1)?.question).toBe(KEPT_Q)
    expect(personOnly(w).map(r => r.argv)).toEqual([APPROVE])
    expect(w.store['adage.kept']).toEqual([])
  })

  test('M1: a kept answer the binary refuses before the spend stays kept for the next start', async ($, on) => {
    const kept = { command: 'adage on', say: 'adage', key: 'on', session: 'earlier', root: '/w', at: T0 }
    const w = world(on, { feed: FEED, store: { 'adage.kept': [kept] }, ask: () => 'run it', refuse: true })
    await begin($, w)
    expect(w.asked.map(a => a.question)).toEqual(['Kept from your last session: adage on. Run it now?'])
    expect(personOnly(w).map(r => r.argv)).toEqual([[CLI, 'on']])
    expect(w.store['adage.kept']).toEqual([kept])
  })
})

describe('T-B4-06 · the pane\'s verbs', () => {
  test('one Button per review verb, worded per pane.verb.*, lowercase hotkeys; the command dim with Copy; every pending ask under waiting for you', async ($, on) => {
    const w = world(on, { feed: FEED, graph: { ...GRAPH_OFFER, routes: [...GRAPH_OFFER.routes, GRAPH.routes[3]] }, status_json: { tracks: [], candidates: [], packs_awaiting_trust: PACKS } })
    await begin($, w)
    for (const surface of SURFACES) {
      const ui = await mountPane($, surface)
      await ui.press({ key: 'tab-reflexes' })
      const waits = (await ui.findAll({ type: 'Button' })).filter(b => /^(yes|no):/.test(b.key ?? ''))
      expect(waits.map(b => `${b.key}=${b.text}`)).toEqual([
        'yes:acme/learned/codegen-after-schema#execute=see what it would do',
        'no:acme/learned/codegen-after-schema#execute=not now',
        `yes:${ENABLE}=enable`,
        `no:${ENABLE}=not this one`,
        'yes:pack:alice@acme.com=use them',
        'no:pack:alice@acme.com=not now',
      ])
      await ui.press({ key: `reflex:${ENABLE}` })
      const row = (await ui.findAll({ type: 'Button' })).filter(b => b.key?.endsWith(ENABLE) && !/^(yes|no|wait):/.test(b.key))
      expect(row.map(b => `${b.props.hotkey ?? ''}:${b.text}`)).toEqual([`:${(await ui.findAll({ key: `reflex:${ENABLE}` }))[0]?.text}`, 'e:enable', 'n:not this one', 'd:details'])
      await ui.press({ key: `details:${ENABLE}` })
      expect((await ui.findAll({ type: 'Button' })).filter(b => b.key?.startsWith('copy:')).map(b => `${b.props.hotkey}:${b.text}`)).toEqual(['c:Copy'])
      expect(await shown(ui, false)).toContain('adage review approve acme/learned test-after-acme-edit')
      await ui.press({ key: `copy:approve:${ENABLE}` })
      expect(w.copies.at(-1)).toBe('adage review approve acme/learned test-after-acme-edit')
      expect(w.toasts.at(-1)).toBe('copied: adage review approve acme/learned test-after-acme-edit')
      await ui.press({ key: `reflex:${ENABLE}` })
      // The four more verbs, behind details, with their own hotkeys.
      await ui.press({ key: 'reflex:acme/learned/codegen-after-schema' })
      await ui.press({ key: 'details:acme/learned/codegen-after-schema' })
      const more = (await ui.findAll({ type: 'Button' })).filter(b => b.key?.startsWith('verb:'))
      expect(more.map(b => `${b.props.hotkey}:${b.text}`)).toEqual(['s:make it a direct instruction', 'i:pin it'])
      await ui.press({ key: 'reflex:acme/learned/codegen-after-schema' })
      for (const t of await shown(ui, false)) if (!t.includes('FIRE') && !t.includes('INHIBIT') && !t.includes('PRIME')) expect(t).not.toMatch(/\b(approval|saved)\b/)
      await ui.press({ key: 'tab-live' })
      await ui.unmount()
    }
    // Each press reaches the binary through the press, never otherwise.
    const ui = await mountPane($)
    await ui.press({ key: 'tab-reflexes' })
    await ui.press({ key: 'yes:pack:alice@acme.com' })
    await w.clock.settle()
    expect(personOnly(w).at(-1)?.argv).toEqual([CLI, 'add', '.adage/packs/acme', '--trust'])
    expect(stdin(personOnly(w).at(-1))).toEqual({ press: TOKEN, session: SESSION })
    await ui.press({ key: `no:${ENABLE}` })
    await w.clock.settle()
    expect(personOnly(w).at(-1)?.argv).toEqual([CLI, 'review', 'dismiss', 'acme/learned', 'test-after-acme-edit'])
    expect(stdin(personOnly(w).at(-1))).toEqual({ press: NEXT_TOKEN, session: SESSION })
    await ui.press({ key: 'no:acme/learned/codegen-after-schema#execute' })
    await w.clock.settle()
    expect(personOnly(w)).toHaveLength(2)
    for (const r of presses(w)) expect(r.argv.some(a => /^[0-9a-f]{64}$/.test(a))).toBe(false)
  })
})

describe('T-B4-09 · no banned words', () => {
  test('no question, label, pane line or verb label carries a strength or lifecycle word, approval or saved', async ($, on) => {
    const w = world(on, { feed: FEED, graph: { ...GRAPH_OFFER, routes: [...GRAPH_OFFER.routes, GRAPH.routes[3]] }, status_json: { tracks: [], candidates: [], packs_awaiting_trust: PACKS }, ask: () => 'see what it would do' })
    await begin($, w)
    await turnEnd($, w)
    for (const a of w.asked) for (const s of [a.question, ...a.options]) expect(s).not.toMatch(BANNED)
    const grant = await mountPane($, 'terminal', 120, 'adage-allow')
    for (const e of await grant.findAll({})) if (e.type === 'Text' || e.type === 'Button') expect(e.text).not.toMatch(BANNED)
    await grant.unmount()
    const ui = await mountPane($)
    await ui.press({ key: 'tab-reflexes' })
    for (const b of await ui.findAll({ type: 'Button' })) if (/^(yes|no|verb|details|copy):/.test(b.key ?? '')) expect(b.text).not.toMatch(BANNED)
    expect(await bandText($)).toContain('need review')
    expect(await bandText($)).not.toContain('wait')
  })
})
