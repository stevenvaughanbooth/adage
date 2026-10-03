// The session's press token (token.ts; plan B4; architecture §3.12 "Hold"; T-B4-05): issued at the
// command hooks' session start, held, spent once per press, refused without a spend, switched off
// by a `press_issued` record the mod did not cause. Through the engine, and over the fake.
import { describe, expect, test } from 'claude-code/testing'

import { CLI, HOOK, LINE, NEXT_TOKEN, SESSION, T0, TOKEN, bandText, begin, exited, fake, mountPane, personOnly, rowsOf, stdin, turnEnd, world } from './fixtures/kit'
import { ANSWER } from './press'
import { checkTamper, held, issue, pressCommand } from './token'

const FEED = [LINE.learnerIdle, LINE.healthy, LINE.start, LINE.pressIssued]
const ENABLE = 'acme/learned/test-after-acme-edit'
const APPROVE = [CLI, 'review', 'approve', 'acme/learned', 'test-after-acme-edit']

describe('T-B4-05 · the token\'s life', () => {
  test('a press_issued record the mod did not cause switches presses off with one warning; the answer is then kept', async ($, on) => {
    const w = world(on, { feed: [...FEED, LINE.pressIssuedAgain], ask: () => 'enable' })
    await begin($, w)
    await turnEnd($, w)
    expect(w.statuses).toEqual(["something else claimed this session's press; presses are off for this session · run adage doctor"])
    expect(personOnly(w)).toEqual([])
    expect(w.store['adage.kept']).toEqual([{ command: 'adage review approve acme/learned test-after-acme-edit', say: 'run make test after editing acme/**', key: ENABLE, session: SESSION, root: '/w', at: expect.any(Number) }])
    expect(await bandText($)).toBe('◠ adage  ● healthy» kept for your next session · /adage review')
  })

  test('M7: a second resume of the session in a new process gets no press and raises no tamper alarm over the two issues the spool holds', async ($, on) => {
    const w = world(on, { feed: [...FEED, LINE.pressIssuedAgain], hook: { Press: '' }, ask: () => undefined })
    await begin($, w, 'resume')
    await turnEnd($, w)
    expect(w.statuses).toEqual([])
  })

  test('M7: a third issue for the session is more than a start and its one resume: that is tampering', async ($, on) => {
    const t = world(on, { feed: [...FEED, LINE.pressIssuedAgain, LINE.pressIssuedThird], hook: { Press: '' }, ask: () => undefined })
    await begin($, t, 'resume')
    await turnEnd($, t)
    expect(t.statuses).toEqual(["something else claimed this session's press; presses are off for this session · run adage doctor"])
  })

  test('M1: a press the binary refuses records nothing and says so, and spends nothing: the next answer presses with the same token', async ($, on) => {
    let refusing = true
    const w = world(on, { feed: FEED, ask: () => 'enable', refuse: () => refusing })
    await begin($, w)
    await turnEnd($, w)
    expect(personOnly(w).map(r => r.argv)).toEqual([APPROVE])
    expect(w.toasts).toEqual(['not recorded: adage review is a decision for a person: run it at a terminal'])
    refusing = false
    const ui = await mountPane($)
    await ui.press({ key: 'tab-reflexes' })
    await ui.press({ key: `reflex:${ENABLE}` })
    await ui.press({ key: `verb:dismiss:${ENABLE}` })
    await w.clock.settle()
    expect(personOnly(w).map(r => r.argv[2])).toEqual(['approve', 'dismiss'])
    expect(personOnly(w).map(r => stdin(r)?.press)).toEqual([TOKEN, TOKEN])
    expect(w.store['adage.kept']).toBeUndefined()
    expect(await bandText($)).not.toContain('kept')
  })
})

describe('T-B4-05 · the token over the fake', () => {
  const answering = (press: string) => (argv: readonly string[]) =>
    argv[1] === 'hook' ? exited(0, press === '' ? '' : JSON.stringify({ press })) : exited(0, `${JSON.stringify({ next: NEXT_TOKEN })}\nadage is on for this repository.\n`)

  test('issue: the token held for the session the command hooks named; a compaction asks nothing; a resume counts two issues', async () => {
    const f = fake({ run: answering(TOKEN) })
    f.m.rt.bin = HOOK
    expect(await issue(f.m, { source: 'compact' })).toBe(false)
    expect(f.ran).toEqual([])
    expect(await issue(f.m, { source: 'startup', session_id: SESSION, cwd: '/w' })).toBe(true)
    expect(f.state.press).toEqual({ session: SESSION, token: TOKEN, state: 'held', issued: 1, chain: 0 })
    expect(JSON.parse(f.ran[0]?.init?.stdin ?? '{}')).toEqual({ session_id: SESSION, cwd: '/w', hook_event_name: 'Press', source: 'startup', via: 'mod' })
    expect(await issue(f.m, { source: 'resume', session_id: SESSION })).toBe(true)
    expect(f.state.press.issued).toBe(2)
    expect(await held(f.m)).toBe(true)
  })

  test('checkTamper: more press_issued records than tokens handed over switches presses off, with one warning', async () => {
    const f = fake({ state: { press: { session: SESSION, token: TOKEN, state: 'held', issued: 1, chain: 0 }, rows: rowsOf(LINE.pressIssued, LINE.pressIssuedAgain) } })
    f.m.rt.interactive = true
    await checkTamper(f.m)
    expect(f.state.press).toMatchObject({ state: 'off', token: '' })
    expect(f.statuses).toEqual(["something else claimed this session's press; presses are off for this session · run adage doctor"])
    expect(await held(f.m)).toBe(false)
  })

  test('pressCommand: with a press held the verb runs and the next token is held; with none the answer is kept and nothing runs', async () => {
    const f = fake({ run: answering(TOKEN), state: { press: { session: SESSION, token: TOKEN, state: 'held', issued: 1, chain: 0 } } })
    f.m.rt.cli = CLI
    f.m.rt.interactive = true
    const p = await pressCommand(f.m, ANSWER, 'adage on', 'adage', 'on', 'on')
    expect(p).toMatchObject({ ok: true, next: NEXT_TOKEN })
    expect(f.state.press).toMatchObject({ token: NEXT_TOKEN, state: 'held', chain: 1 })
    expect(f.toasts).toEqual(['adage is on for this repository.'])
    const g = fake()
    g.m.rt.cli = CLI
    expect(await pressCommand(g.m, ANSWER, 'adage on', 'adage', 'on', 'on')).toBeUndefined()
    expect(g.ran).toEqual([])
    expect(g.store['adage.kept']).toEqual([{ command: 'adage on', say: 'adage', key: 'on', session: SESSION, root: '/w', at: T0 }])
    expect(await pressCommand(g.m, ANSWER, 'adage install claude-code', 'x', 'k', 'x')).toBeUndefined()
  })
})
