// The band's values (view/band.ts; ux-design §5, §9.6, §9.12; T-B2-07 … T-B2-09, T-B2-11): the
// cells and their narrow-terminal priority, the lamp, the learner, the one attention line.
import { describe, expect, test } from 'claude-code/testing'

import { GRAPH, LINE, rowsOf as rows } from '../fixtures/kit'
import type { Fired, Health } from '../plugin'
import { reflexesOf } from '../reads'
import { band, firedThisSession, newer, updateLine, versionLine, withheldLine } from './band'
import type { BandInput } from './band'
import { markText, promptLines } from './lines'

const REFLEXES = reflexesOf(GRAPH)
const fired = (route: string, prompt = 1): Fired => ({ event: 'UserPromptSubmit', route, track: 'acme/learned@1', kind: 'run', strength: 'fire', prompt, promptText: 'fix W-2042' })
const HEALTHY: Health = { lights: [], word: 'healthy', lamp: 'fire', check: null, off: false }
const BASE: BandInput = { active: true, nobinary: '', paused: false, health: HEALTHY, inRepo: true, reflexes: 0, steps: 0, reused: 0, learner: null, now: 0, lapsed: 0, review: 0, update: null, versionLine: '', columns: 160, ascii: false, kept: 0, noticed: 0, paneOpen: false, withheld: false }
const line1 = (i: BandInput) => {
  const b = band(i)
  return b === undefined ? undefined : b.cells.map(c => (c.glyph ? `${c.glyph} ${c.text}` : c.text)).join(' · ').replace('adage · ', 'adage  ')
}

describe('T-B2-07 … T-B2-09 · the band', () => {
  test('every cell, in order, with the lamp word', () => {
    expect(line1({ ...BASE, reflexes: 2, steps: 1, reused: 1, learner: { state: 'waiting', nextAt: 38_000, ranAt: null } })).toBe(
      '◠ adage  2 reflexes this session · 1 step without a model request · 1 test run reused · ● healthy · learning in 38 s',
    )
  })

  test('narrowing drops from the right by priority; the lamp word is never dropped', () => {
    const i = { ...BASE, reflexes: 2, steps: 1, reused: 1, learner: { state: 'running' as const, nextAt: null, ranAt: null } }
    expect(line1({ ...i, columns: 60 })).toBe('◠ adage  2 reflexes this session · ● healthy')
    expect(line1({ ...i, columns: 40 })).toBe('◠ adage  ● healthy')
    expect(line1({ ...i, columns: 10 })).toBe('◠ adage  ● healthy')
    for (const columns of [60, 40, 20, 10]) expect(line1({ ...i, columns })).toContain('healthy')
  })

  test('quiet: nothing to say and green health leave the band to others', () => {
    expect(band(BASE)).toBeUndefined()
    expect(band({ ...BASE, active: false })).toBeUndefined()
    expect(band({ ...BASE, inRepo: false, reflexes: 3 })).toBeUndefined()
  })

  test('off, paused, no binary: one cell each', () => {
    expect(line1({ ...BASE, health: { ...HEALTHY, off: true } })).toBe('◠ adage ○ off in this repository · /adage on')
    expect(line1({ ...BASE, paused: true, reflexes: 2 })).toBe('◠ adage  paused · /adage pause')
    expect(line1({ ...BASE, active: false, nobinary: 'no binary for darwin-arm64' })).toBe('◠ adage ○ off · run adage doctor')
  })

  test('the second line: one item, lapsed > problem > review > version', () => {
    const problem: Health = { lights: [], word: 'problem', lamp: 'inhibit', check: 'hook latency', off: false }
    const v = versionLine('2.1.300', '2.1.288', false)
    expect(band({ ...BASE, lapsed: 1, health: problem, review: 2, versionLine: v })?.second).toBe('» 1 grant lapsed · /adage health')
    expect(band({ ...BASE, health: problem, review: 2, versionLine: v })?.second).toBe('» problem: hook latency · /adage health')
    expect(band({ ...BASE, review: 2, versionLine: v })?.second).toBe('» 2 need review · /adage review')
    expect(band({ ...BASE, versionLine: v })?.second).toBe('» Claude Code 2.1.300 is newer than this plugin was tested with (2.1.288) · run adage doctor')
  })

  test('T-O9-06 · the update line: lapsed > problem > kept > review > update > version, table-driven', () => {
    const problem: Health = { lights: [], word: 'problem', lamp: 'inhibit', check: 'hook latency', off: false }
    const v = versionLine('2.1.300', '2.1.288', false)
    const update = { latest: '0.2.0', next: '/adage update' }
    const all: Partial<BandInput> = { lapsed: 1, health: problem, kept: 1, review: 2, update, versionLine: v }
    const order: [keyof BandInput, string][] = [
      ['lapsed', '» 1 grant lapsed · /adage health'],
      ['health', '» problem: hook latency · /adage health'],
      ['kept', '» kept for your next session · /adage review'],
      ['review', '» 2 need review · /adage review'],
      ['update', '» adage 0.2.0 is out · /adage update'],
      ['versionLine', v],
    ]
    // Each row is what shows once every row above it is gone.
    let i: BandInput = { ...BASE, ...all }
    for (const [key, want] of order) {
      expect(band(i)?.second, key).toBe(want)
      i = { ...i, [key]: BASE[key] }
    }
    expect(band(i)).toBeUndefined()
    // Alone it is enough for a band; never a strength or lifecycle word, and the command as the card says it.
    expect(band({ ...BASE, update })?.second).toBe('» adage 0.2.0 is out · /adage update')
    expect(updateLine({ latest: '0.2.0', next: 'adage update' }, true)).toBe('>> adage 0.2.0 is out - adage update')
    for (const word of ['fire', 'prime', 'inhibit', 'nascent', 'active', 'retired']) expect(updateLine(update, false).includes(word), word).toBe(false)
  })

  test('lamp roles and words: healthy, check, problem; the off glyph only when off', () => {
    const cell = (h: Health) => band({ ...BASE, reflexes: 1, health: h })?.cells.find(c => c.glyph !== undefined)
    expect(cell(HEALTHY)).toEqual({ glyph: '●', lamp: 'fire', text: 'healthy' })
    expect(cell({ ...HEALTHY, word: 'check', lamp: 'prime', check: 'hooks: claude-code' })).toEqual({ glyph: '●', lamp: 'prime', text: 'check: hooks: claude-code' })
    expect(cell({ ...HEALTHY, word: 'problem', lamp: 'inhibit', check: 'index' })).toEqual({ glyph: '●', lamp: 'inhibit', text: 'problem: index' })
    expect(line1({ ...BASE, reflexes: 1 })).not.toContain('○')
  })

  test('T-B2-11: every glyph its ASCII twin', () => {
    expect(line1({ ...BASE, reflexes: 1, ascii: true, learner: { state: 'running', nextAt: null, ranAt: null } })).toBe('~ adage  1 reflex this session · * healthy · learning...')
    expect(markText({ kind: 'predicted' }, true)).toBe('~ adage chose this step - no model request')
    expect(promptLines([fired('codegen-after-schema')], REFLEXES, [], 80, true)).toEqual(['~ run make gen after a schema edit'])
  })

  test('learning: the countdown, then running, then learned for 10 s', () => {
    const at = (state: 'waiting' | 'running' | 'idle', now: number, ranAt: number | null = null) => line1({ ...BASE, now, learner: { state, nextAt: 38_000, ranAt } })
    expect(at('waiting', 0)).toContain('learning in 38 s')
    expect(at('running', 0)).toContain('learning…')
    expect(at('idle', 5000, 0)).toContain('learned')
    expect(at('idle', 11_000, 0)).toBeUndefined()
  })

  test('reflexes this session: the hooks and this session feed, each once; another session counts nothing', () => {
    expect(firedThisSession([fired('codegen-after-schema')], rows(LINE.firedGen, LINE.firedLookup, LINE.otherControl))).toBe(2)
  })

  test('T-B4-01, §9.12: an answer kept for the next start is said before review waiting; a pattern noticed shows while the pane is closed', () => {
    expect(band({ ...BASE, kept: 1, review: 2 })?.second).toBe('» kept for your next session · /adage review')
    expect(line1({ ...BASE, noticed: 2 })).toBe('◠ adage  ● healthy · 2 patterns noticed')
    expect(band({ ...BASE, noticed: 2, paneOpen: true })).toBeUndefined()
  })

  test('T-C4-07 · band.withheld: why a step was left to the agent, after a problem and before what waits; plain words', () => {
    const problem: Health = { ...HEALTHY, word: 'problem', lamp: 'inhibit', check: 'hook latency' }
    const said = '» adage left a step to the agent: your permissions do not allow it unasked'
    expect(band({ ...BASE, withheld: true })?.second).toBe(said)
    expect(band({ ...BASE, withheld: true, kept: 1, review: 2 })?.second).toBe(said)
    expect(band({ ...BASE, withheld: true, health: problem })?.second).toBe('» problem: hook latency · /adage health')
    expect(band({ ...BASE, withheld: true, lapsed: 1 })?.second).toBe('» 1 grant lapsed · /adage health')
    expect(withheldLine(true)).toBe('>> adage left a step to the agent: your permissions do not allow it unasked')
    expect(said).not.toMatch(/\b(predict\w*|grant\w*|fire|prime|inhibit|approval|saved)\b/i)
  })

  test('T-B2-08, §9.6 · versions: a newer harness, and not', () => {
    expect(newer('2.1.300', '2.1.288')).toBe(true)
    expect(newer('2.1.288', '2.1.288')).toBe(false)
    expect(newer('2.1.288-dev.20260920', '2.1.288')).toBe(false)
    expect(newer('2.2.0', '2.1.288')).toBe(true)
    expect(versionLine('2.1.288', '2.1.288', false)).toBe('')
  })
})
