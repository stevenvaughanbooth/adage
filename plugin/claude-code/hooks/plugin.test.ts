// The plugin's tables (plugin.ts; architecture §2 rule 3): every `$.state` key with its initial
// value, and every `$.store` key. scripts/check-plugin.mjs holds register.ts and types/index.d.ts
// to the same keys; here, what the initial values say.
import { describe, expect, test } from 'claude-code/testing'

import { fake } from './fixtures/kit'
import { KEPT_MAX_AGE_MS, MAX_ASKED, MAX_SKILLS, STATE0, STORE_KEYS, TESTED } from './plugin'

describe('T-B1-01 · the state and store tables', () => {
  test('before anything is written: inert, on the Live tab, no press held, the feed off, nothing drawn', () => {
    expect(STATE0.mode).toMatchObject({ active: false, interactive: false, nobinary: '', warned: [] })
    expect(STATE0.view).toMatchObject({ tab: 'live', paused: false, selected: null })
    expect(STATE0.press).toEqual({ session: '', token: '', state: 'none', issued: 0, chain: 0 })
    expect(STATE0.feed).toEqual({ status: 'off', cursor: '', failures: 0 })
    expect([STATE0.health, STATE0.learner, STATE0.allow]).toEqual([null, null, null])
    for (const k of ['fired', 'predicted', 'marks', 'batches', 'rows', 'ticks', 'reflexes', 'checks', 'lapsed', 'packs', 'asked', 'skills', 'candidates'] as const) expect(STATE0[k]).toEqual([])
  })

  test('every key a cell: the fake builds one per key of each table, as register.ts does', () => {
    const f = fake()
    expect(Object.keys(f.m.state)).toEqual(Object.keys(STATE0))
    expect(Object.keys(f.m.store)).toEqual([...STORE_KEYS])
    expect(new Set(STORE_KEYS).size).toBe(STORE_KEYS.length)
  })

  test('the limits and the version tested with', () => {
    expect([MAX_ASKED, MAX_SKILLS]).toEqual([200, 20])
    expect(KEPT_MAX_AGE_MS).toBe(7 * 24 * 3600 * 1000)
    expect(TESTED).toMatch(/^\d+\.\d+\.\d+$/)
  })
})
