// What outlives the session in `$.store` (kept.ts; ux-design §4.17, §9.12; T-B4-05's H1/H2 halves
// over the fake): the asks and the answers kept for the next start in the same repository.
import { describe, expect, test } from 'claude-code/testing'

import { T0, fake } from './fixtures/kit'
import { keep, keptNow, loadStore, noteAnswer, repoRoot } from './kept'

const K = { command: 'adage on', say: 'adage', key: 'on', session: 's', root: '/w', at: T0 }

describe('T-B4-05, §9.12 · the store across sessions', () => {
  test('the repository is the session\'s root, else its working directory', async () => {
    expect(await repoRoot(fake({ repo: '/repo', cwd: '/repo/sub' }).m)).toBe('/repo')
    expect(await repoRoot(fake({ repo: null, cwd: '/tmp/x' }).m)).toBe('/tmp/x')
  })

  test('keep: one answer per command and repository, the latest; the band counts this repository\'s', async () => {
    const f = fake({ store: { 'adage.kept': [{ ...K, root: '/other' }, { ...K, at: 1 }] } })
    await keep(f.m, K)
    expect(f.store['adage.kept']).toEqual([{ ...K, root: '/other' }, K])
    expect(f.m.rt.kept).toBe(1)
    expect((await keptNow(f.m)).length).toBe(2)
  })

  test('loadStore: the asks kept across sessions, unless this session asked already; the count of answers kept here', async () => {
    const asked = [{ key: 'k', at: T0, times: 1, done: false }]
    const f = fake({ store: { 'adage.asked': asked, 'adage.kept': [K, { ...K, root: '/b' }, { junk: true }] } })
    await loadStore(f.m)
    expect(f.state.asked).toEqual(asked)
    expect(f.m.rt.kept).toBe(1)
    const g = fake({ store: { 'adage.asked': asked }, state: { asked: [{ key: 'mine', at: 1, times: 0, done: true }] } })
    await loadStore(g.m)
    expect(g.state.asked.map(a => a.key)).toEqual(['mine'])
  })

  test('noteAnswer: recorded in the state and the store; a dismissal counts, an answer closes it', async () => {
    const f = fake()
    await noteAnswer(f.m, 'k', undefined)
    expect(f.state.asked).toEqual([{ key: 'k', at: T0, times: 1, done: false }])
    await noteAnswer(f.m, 'k', 'enable')
    expect(f.store['adage.asked']).toEqual([{ key: 'k', at: T0, times: 1, done: true }])
  })
})
