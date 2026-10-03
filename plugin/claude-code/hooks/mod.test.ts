// What every hook function is handed (mod.ts): a fresh load's runtime is inert, and work let run on
// without the hook fails quietly.
import { describe, expect, test } from 'claude-code/testing'

import { quietly, runtime } from './mod'
import { NOTED0 } from './ledger'

describe('T-B1-01, T-X-02 · the runtime and quiet work', () => {
  test('a fresh load: no binary, no person, nothing followed, nothing asked, presses unbound', () => {
    const rt = runtime()
    expect([rt.bin, rt.cli, rt.child, rt.standingFor, rt.skill]).toEqual([undefined, undefined, undefined, undefined, undefined])
    expect([rt.interactive, rt.observing, rt.paused, rt.asking, rt.pressBound, rt.settled, rt.paneOpen]).toEqual([false, false, false, false, false, false, false])
    expect([rt.failures, rt.kept, rt.noticed, rt.feedGen]).toEqual([0, 0, 0, 0])
    expect(rt.lastRefresh).toBe(-Infinity)
    expect(rt.noted).toBe(NOTED0)
    expect(runtime()).not.toBe(rt)
  })

  test('quietly: a rejection is swallowed, never thrown at the hook', async () => {
    let caught = false
    const rejected = Promise.reject(new Error('boom'))
    quietly(rejected)
    await rejected.catch(() => void (caught = true))
    expect(caught).toBe(true)
  })
})
