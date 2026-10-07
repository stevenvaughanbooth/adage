// The `--json` reads the pane and the band draw from (refresh.ts; ux-design §5, §6; T-B2-08,
// T-B3-05, T-X-02, T-C5-04) over the fake: what each read changes, at most once per window unless
// forced, nothing headless, a failed read keeping what was drawn, a lapsed grant toasted once.
import { describe, expect, test } from 'claude-code/testing'

import { CARD, DELEGATE, DOCTOR, GRAPH, LAPSED, exited, fake } from './fixtures/kit'
import { REFRESH_MS, refresh, settings } from './refresh'

/** The binary's reads, by verb; `fail` makes every read exit 1. */
const reads = (card: unknown = CARD, fail = false) => (argv: readonly string[]) => {
  if (fail) return exited(1, '')
  const head = argv[1] === '--json' ? 'card' : `${argv[1]} ${argv[2]}`
  const out: Record<string, unknown> = { 'graph --json': GRAPH, 'doctor --json': DOCTOR, 'delegate status': { ...DELEGATE, lapsed: LAPSED }, 'status --json': { packs_awaiting_trust: [{ name: 'p', command: 'adage add p --trust' }] }, card }
  return exited(0, JSON.stringify(out[head] ?? {}))
}

describe('T-B3-05, T-B2-08 · refresh', () => {
  test('the five reads in the session directory: the reflexes, checks, packs, lapsed grants, opt-in lines and routine model', async () => {
    const f = fake({ run: reads({ ...CARD, ui: { spinner: true }, runtime: { routine_model: 'haiku' } }) })
    f.m.rt.cli = '/data/bin/adage'
    f.m.rt.interactive = true
    await refresh(f.m, true)
    expect(f.ran.map(r => r.argv.slice(1).join(' '))).toEqual(['graph --json', 'doctor --json', 'delegate status --json', 'status --json', '--json --for-session'])
    expect(f.ran.every(r => r.init?.cwd === '/w')).toBe(true)
    expect(f.state.reflexes.map(r => r.id)).toContain('codegen-after-schema')
    expect(f.state.checks).toHaveLength(4)
    expect(f.state.packs).toEqual([{ name: 'p', command: 'adage add p --trust' }])
    expect(f.state.lapsed.map(l => l.route)).toEqual(['acme/learned/codegen-after-schema'])
    expect(f.state.mode).toMatchObject({ spinner: true, turnLine: false, noticed: false })
    expect(f.m.rt.routineModel).toBe('haiku')
    expect(f.toasts).toEqual(['adage\nacme/learned/codegen-after-schema is no longer allowed: argv_templates changed · /adage review'])
  })

  test('at most once per window unless forced; a lapse toasted once per session', async () => {
    const f = fake({ run: reads() })
    f.m.rt.cli = '/data/bin/adage'
    f.m.rt.interactive = true
    await refresh(f.m)
    await refresh(f.m)
    expect(f.ran).toHaveLength(5)
    f.tick(REFRESH_MS)
    await refresh(f.m)
    await refresh(f.m, true)
    expect(f.ran).toHaveLength(15)
    expect(f.toasts).toHaveLength(1)
  })

  test('T-X-02 · a read that fails keeps what was drawn; T-X-03 · headless or inert, nothing is read', async () => {
    const f = fake({ run: reads(CARD, true), state: { packs: [{ name: 'kept', command: 'x' }] } })
    f.m.rt.cli = '/data/bin/adage'
    f.m.rt.interactive = true
    await refresh(f.m, true)
    expect(f.state.packs).toEqual([{ name: 'kept', command: 'x' }])
    const g = fake({ run: reads() })
    g.m.rt.cli = '/data/bin/adage'
    await refresh(g.m, true)
    const h = fake({ run: reads() })
    h.m.rt.interactive = true
    await refresh(h.m, true)
    expect([...g.ran, ...h.ran]).toEqual([])
  })

  test('T-C5-04 · settings: a session that draws nothing reads only the card, for the routine model', async () => {
    const f = fake({ run: reads({ runtime: { routine_model: 'haiku' } }) })
    f.m.rt.cli = '/data/bin/adage'
    await settings(f.m)
    expect(f.ran.map(r => r.argv.slice(1))).toEqual([['--json', '--for-session']])
    expect(f.m.rt.routineModel).toBe('haiku')
  })
})

describe('T-O9-06 · the update line: once per release per machine, until the first prompt', () => {
  const UPDATE = { current: '0.1.0', latest: '0.2.0', channel: 'npm', next: '/adage update', text: '0.2.0 is out (you have 0.1.0)', notes: 'fixes hook latency' }
  const session = (store: Record<string, unknown> = {}) => {
    const f = fake({ run: reads({ ...CARD, update: UPDATE }), store })
    f.m.rt.cli = '/data/bin/adage'
    f.m.rt.interactive = true
    return f
  }

  test("the first session to read it shows the line (and the store says so); a second read keeps it; the next session does not", async () => {
    const f = session()
    await refresh(f.m, true)
    expect(f.state.mode.update).toEqual({ have: '0.1.0', latest: '0.2.0', next: '/adage update', notes: ['fixes hook latency'], band: true })
    expect(f.store['adage.latest']).toBe('0.2.0')
    await refresh(f.m, true)
    expect(f.state.mode.update?.band).toBe(true)
    const next = session({ ...f.store })
    await refresh(next.m, true)
    expect(next.state.mode.update).toEqual({ have: '0.1.0', latest: '0.2.0', next: '/adage update', notes: ['fixes hook latency'], band: false })
  })

  test('after the first prompt the line is gone; a card with nothing newer clears it; a newer release shows again', async () => {
    const f = session()
    await refresh(f.m, true)
    f.state.turn = { ...f.state.turn, submits: 1 }
    await refresh(f.m, true)
    expect(f.state.mode.update?.band).toBe(false)
    const g = session()
    await refresh(g.m, true)
    g.m.run = fake({ run: reads(CARD) }).m.run
    await refresh(g.m, true)
    expect(g.state.mode.update).toBeNull()
    const h = session({ 'adage.latest': '0.1.5' })
    await refresh(h.m, true)
    expect(h.state.mode.update?.band).toBe(true)
  })
})
