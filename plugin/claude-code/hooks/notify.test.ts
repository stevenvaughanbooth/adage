// What the person is told once per session (notify.ts; ux-design §9): a status warning or a toast
// by id, said once until a new session empties the list; nothing to a session no person is at.
import { describe, expect, test } from 'claude-code/testing'

import { fake } from './fixtures/kit'
import { toastOnce, warnOnce } from './notify'

describe('T-B2-08, §9.2 · once per session', () => {
  test('a warning and a toast by id, each once; the ids are kept in the mode', async () => {
    const f = fake()
    f.m.rt.interactive = true
    await warnOnce(f.m, 'nobinary', 'no binary for darwin-arm64')
    await warnOnce(f.m, 'nobinary', 'no binary for darwin-arm64')
    await toastOnce(f.m, 'lapsed:r', 'r is no longer allowed')
    await toastOnce(f.m, 'lapsed:r', 'r is no longer allowed')
    await toastOnce(f.m, 'lapsed:s', 's is no longer allowed')
    expect(f.statuses).toEqual(['no binary for darwin-arm64'])
    expect(f.toasts).toEqual(['r is no longer allowed', 's is no longer allowed'])
    expect(f.state.mode.warned).toEqual(['nobinary', 'lapsed:r', 'lapsed:s'])
  })

  test('T-X-03 · headless: nothing is said and nothing is marked said', async () => {
    const f = fake()
    await warnOnce(f.m, 'x', 'x')
    await toastOnce(f.m, 'y', 'y')
    expect([...f.statuses, ...f.toasts]).toEqual([])
    expect(f.state.mode.warned).toEqual([])
  })
})
