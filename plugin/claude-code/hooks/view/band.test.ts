// The band's values (view/band.ts; ux-design §5, §9.6, §9.12; T-B2-07 … T-B2-09, T-B2-11): the
// cells and their narrow-terminal priority, the lamp, the learner, the one attention line.
import { describe, expect, test } from 'claude-code/testing'

import { GRAPH, LINE, rowsOf as rows } from '../fixtures/kit'
import type { Fired, Health } from '../plugin'
import { reflexesOf } from '../reads'
import { band, firedThisSession, newer, versionLine } from './band'
import type { BandInput } from './band'
import { markText, promptLines } from './lines'

const REFLEXES = reflexesOf(GRAPH)
const fired = (route: string, prompt = 1): Fired => ({ event: 'UserPromptSubmit', route, track: 'acme/learned@1', kind: 'run', strength: 'fire', prompt, promptText: 'fix W-2042' })
const HEALTHY: Health = { lights: [], word: 'healthy', lamp: 'fire', check: null, off: false }
const BASE: BandInput = { active: true, nobinary: '', paused: false, health: HEALTHY, inRepo: true, reflexes: 0, steps: 0, reused: 0, learner: null, now: 0, lapsed: 0, review: 0, versionLine: '', columns: 160, ascii: false, kept: 0, noticed: 0, paneOpen: false }
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

  test('T-B2-08, §9.6 · versions: a newer harness, and not', () => {
    expect(newer('2.1.300', '2.1.288')).toBe(true)
    expect(newer('2.1.288', '2.1.288')).toBe(false)
    expect(newer('2.1.288-dev.20260920', '2.1.288')).toBe(false)
    expect(newer('2.2.0', '2.1.288')).toBe(true)
    expect(versionLine('2.1.288', '2.1.288', false)).toBe('')
  })
})
