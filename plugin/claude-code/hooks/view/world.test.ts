// The World tab's words (view/world.ts; T-W3-11, T-W3-12): the header by state, the day strip's
// cells and labels, the block line, the timeline, source and hint lines, and the empty words.
import { describe, expect, test } from 'claude-code/testing'

import { WORLD0 } from '../plugin'
import type { WorldBlock } from '../plugin'
import { QUARTER_MS, STRIP_FROM, STRIP_TO, WORLD_EMPTY, WORLD_OFF, ago, blockLine, dayStrip, eventLine, hintLine, hm, sourceLine, titleOf, until, worldHeader } from './world'

/** A day's noon in local time, so the strip's hours are the clock's. */
const noon = (() => {
  const d = new Date(2026, 2, 4, 12, 0, 0, 0)
  return d.getTime()
})()
const h = (hours: number) => noon + (hours - 12) * 3_600_000

const BLOCKS: WorldBlock[] = [
  { id: 'c:1', start: h(9), end: h(10), title: 'Marketing sync', busy: true, accepted: true, focus: false, attendees: 4 },
  { id: 'c:2', start: h(11.5), end: h(12.5), title: 'Focus block', busy: true, accepted: true, focus: true, attendees: 0 },
  { id: 'c:3', start: h(14), end: h(15), title: 'All hands', busy: true, accepted: true, focus: false, attendees: 40 },
  { id: 'c:4', start: h(16), end: h(17), title: 'Declined', busy: true, accepted: false, focus: false, attendees: 2 },
  { id: 'c:5', start: h(17), end: h(18), title: 'Free', busy: false, accepted: true, focus: false, attendees: 0 },
]

describe('T-W3-11 · the header, the strip, the block line', () => {
  test('T-W3-11: the header word and role by state; the facts count the live sources, the hints and the things', () => {
    expect(worldHeader(WORLD0, false)).toMatchObject({ word: 'off', role: 'ink3' })
    expect(worldHeader(WORLD0, false).facts[0]).toContain('adage world on')
    const on = { ...WORLD0, enabled: true, sources: [{ name: 'github', server: 'github', lastPoll: 1, ok: true, error: '', events: 2 }, { name: 'calendar', server: 'c', lastPoll: 0, ok: true, error: '', events: 0 }], hints: [{ ts: 1, prompt: 1, cls: 'hint' as const, text: 'x' }], artifacts: 12 }
    expect(worldHeader(on, false)).toEqual({ word: 'on', role: 'fire', facts: ['1 source live of 2', '1 hint this session', '12 things it knows'] })
    expect(worldHeader({ ...on, paused: true }, false)).toMatchObject({ word: 'paused', role: 'prime' })
    expect(worldHeader({ ...on, status: 'stopped' }, false)).toMatchObject({ word: 'feed stopped', role: 'prime' })
  })

  test('T-W3-11: the strip: a cell per quarter hour from 07:00 to 20:00, blocks filled (past quiet, current accent, ahead prime, focus inhibit), free time a rail, the present marked; labels every two hours', () => {
    const now = h(12)
    const { cells, labels } = dayStrip(BLOCKS, now, 200, false)
    expect(cells).toHaveLength((STRIP_TO - STRIP_FROM) * 4)
    const at = (hours: number) => cells[Math.round(((h(hours) - h(STRIP_FROM)) / QUARTER_MS))]
    expect(at(8)).toEqual({ ch: '╌', role: 'ink3', now: false })
    expect(at(9)).toEqual({ ch: '█', role: 'ink3', now: false })
    expect(at(12)).toEqual({ ch: '◆', role: 'accent', now: true })
    expect(at(12.25)).toEqual({ ch: '█', role: 'accent', now: false })
    expect(at(14)).toEqual({ ch: '█', role: 'prime', now: false })
    expect(at(16)).toEqual({ ch: '╌', role: 'ink3', now: false })
    expect(at(17)).toEqual({ ch: '╌', role: 'ink3', now: false })
    expect(labels).toBe('07      09      11      13      15      17      19')
    expect(dayStrip(BLOCKS, now, 20, false).labels).toBe('07      09      11')
    const ascii = dayStrip(BLOCKS, now, 200, true)
    expect(ascii.cells.map(c => c.ch).join('')).toMatch(/^[.#*]+$/)
    expect(dayStrip(BLOCKS, now, 20, false).cells).toHaveLength(20)
    expect(dayStrip(BLOCKS, now, 2, false).cells).toHaveLength(8)
  })

  test('T-W3-11: the block line: the one under way until its end, else the next with how long, else none today', () => {
    expect(blockLine(BLOCKS, h(12))).toBe(`now: Focus block until ${hm(h(12.5))}`)
    expect(blockLine(BLOCKS, h(13))).toBe(`next: ${hm(h(14))} All hands (in 1 h)`)
    expect(blockLine(BLOCKS, h(13.9))).toBe(`next: ${hm(h(14))} All hands (in 6 min)`)
    expect(blockLine(BLOCKS, h(18))).toBe('no more meetings today')
    expect(blockLine([], h(12))).toBe('no meetings on the calendar today')
  })

  test('T-W3-11: the time words', () => {
    expect([ago(60_000, 0), ago(0, 0), ago(3_600_000 * 3, 0), ago(86_400_000 * 3, 0)]).toEqual(['1 min ago', 'just now', '3 h ago', '3 days ago'])
    expect([until(0, 30_000), until(0, 240_000), until(0, 7_200_000)]).toEqual(['now', 'in 4 min', 'in 2 h'])
  })
})

describe('T-W3-12 · the lines', () => {
  test('T-W3-12: a timeline line names the thing by its title, else its id tail, with the kind, the action and the detail', () => {
    const e = { ts: h(9.5), source: 'github', kind: 'pr', artifact: 'github:pr:418', action: 'changed', detail: 'ci:red' }
    expect(titleOf(e, { 'github:pr:418': 'Fix the lockfile' })).toBe('Fix the lockfile')
    expect(titleOf(e, {})).toBe('418')
    expect(titleOf({ ...e, artifact: 'file:/w/src/main.rs' }, {})).toBe('main.rs')
    expect(eventLine(e, { 'github:pr:418': 'Fix the lockfile' }, false)).toBe(`${hm(h(9.5))}  github    pull request Fix the lockfile changed · ci red`)
    expect(eventLine({ ...e, kind: 'calendar_event', action: 'ended', detail: '' }, {}, true)).toBe(`${hm(h(9.5))}  github    meeting 418 ended`)
  })

  test('T-W3-12: a source line and its lamp: live, not polled yet, failed', () => {
    const now = h(12)
    expect(sourceLine({ name: 'calendar', server: 'claude_ai_google_calendar', lastPoll: now - 120_000, ok: true, error: '', events: 14 }, now, false)).toEqual({ glyph: '●', role: 'fire', text: 'calendar  claude_ai_google_calendar · polled 2 min ago · 14 events' })
    expect(sourceLine({ name: 'meetings', server: '', lastPoll: 0, ok: true, error: '', events: 0 }, now, false)).toEqual({ glyph: '○', role: 'ink3', text: 'meetings  not polled yet' })
    expect(sourceLine({ name: 'github', server: 'github', lastPoll: now - 60_000, ok: false, error: 'call failed: 401', events: 0 }, now, true)).toEqual({ glyph: 'x', role: 'inhibit', text: 'github    github - polled 1 min ago - call failed: 401' })
  })

  test('T-W3-12: a hint line, and the empty words', () => {
    expect(hintLine({ ts: h(10.2), prompt: 2, cls: 'hint', text: '"Marketing sync" (meeting)' }, false)).toBe(`${hm(h(10.2))}  ◠ hint  "Marketing sync" (meeting)`)
    expect(hintLine({ ts: h(10.2), prompt: 2, cls: 'disambiguation', text: 'one of several' }, true)).toBe(`${hm(h(10.2))}  ~ asked one of several`)
    expect(WORLD_EMPTY).toContain('/mcp')
    expect(WORLD_OFF).toContain('adage world on')
  })
})
