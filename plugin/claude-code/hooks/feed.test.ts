// The feed's lines (feed.ts; T-B3-08, and T-X-06's binary half done again in the mod): the
// follower's argv, the line buffer, the stem, the lines read, the arm scrub on this session's own
// rows, and folding lines into what is drawn. follower.test.ts runs the child.
import { describe, expect, test } from 'claude-code/testing'

import { MAX_ROWS, fold, followArgs, lastCursor, parseLine, splitLines, stem } from './feed'
import type { Folded, Line } from './feed'
import { LINE, SESSION } from './fixtures/kit'

const EMPTY: Folded = { rows: [], ticks: [], health: null, learner: null }

describe('T-B3-08 · the follower argv and the line buffer', () => {
  test('nextArgs: the --since form only once a cursor exists', () => {
    expect(followArgs('/d/bin/adage', '/w', 's1', '')).toEqual(['/d/bin/adage', 'watch', '--follow', '--json', '--stdin', 'ignore', '--repo', '/w', '--session', 's1'])
    expect(followArgs('/d/bin/adage', '/w', 's1', '1~5~0~s1:0:2')).toEqual(['/d/bin/adage', 'watch', '--follow', '--json', '--stdin', 'ignore', '--repo', '/w', '--session', 's1', '--since', '1~5~0~s1:0:2'])
  })

  test('a chunk ending mid-line is buffered and parsed with the next chunk', () => {
    const a = splitLines('', `${LINE.start}\n${LINE.firedGen.slice(0, 40)}`)
    expect(a.lines).toEqual([LINE.start])
    const b = splitLines(a.rest, `${LINE.firedGen.slice(40)}\n`)
    expect(b.lines).toEqual([LINE.firedGen])
    expect(b.rest).toBe('')
  })

  test('an unparseable line is skipped, never fatal', () => {
    expect(parseLine('{', SESSION)).toBeUndefined()
    expect(parseLine('[1,2]', SESSION)).toBeUndefined()
    expect(parseLine('{"no":"kind"}', SESSION)).toBeUndefined()
    expect(parseLine('{"kind":"future","cursor":"c"}', SESSION)).toEqual({ kind: 'other', cursor: 'c' })
  })

  test('the spool stem: an id the binary files as it is, and one it cleans and hashes', () => {
    expect(stem('0f1e2d3c-aaaa-bbbb-cccc-1234567890ab')).toBe('0f1e2d3c-aaaa-bbbb-cccc-1234567890ab')
    expect(stem('a.b')).toMatch(/^a_b-[0-9a-f]{16}$/)
    expect(stem('a.b')).not.toBe(stem('a_b'))
  })
})

describe('T-B3-08, T-X-06 · the lines', () => {
  test('a decision row, a health line, a learner line', () => {
    const d = parseLine(LINE.firedGen, SESSION)
    expect(d).toMatchObject({ kind: 'decision', row: { session: SESSION, mine: true, event: 'tools', acts: [{ act: 'directed', route: 'codegen-after-schema', strength: 'fire', detail: 'make gen' }] } })
    expect(parseLine(LINE.problem, SESSION)).toMatchObject({ kind: 'health', health: { word: 'problem', lamp: 'inhibit', check: 'hook latency', off: false } })
    expect(parseLine(LINE.healthy, SESSION)).toMatchObject({ kind: 'health', health: { word: 'healthy', check: null } })
    expect(parseLine(LINE.learnerWaiting, SESSION)).toMatchObject({ kind: 'learner', state: 'waiting' })
  })

  test("this session's rows never name the arm: arm-only acts go and the row is left quiet", () => {
    const rows = [LINE.controlPrompt, LINE.controlStep, LINE.controlJudged, LINE.controlPretool].map(l => parseLine(l, SESSION))
    const quiet = [LINE.quietPrompt, LINE.quietStep, LINE.quietTools, LINE.quietPretool].map(l => parseLine(l, SESSION))
    // The cursors differ (they are positions), everything drawn does not.
    const drawn = (l: Line | undefined) => (l?.kind === 'decision' ? { ...l.row } : l)
    expect(rows.map(drawn)).toEqual(quiet.map(drawn))
    for (const r of rows) expect(JSON.stringify(r)).not.toMatch(/withheld|would|unaided|not_reached|"control":true/)
  })

  test("another session's control row is kept as `adage watch` shows it", () => {
    expect(parseLine(LINE.otherControl, SESSION)).toMatchObject({ kind: 'decision', row: { control: true, mine: false, acts: [{ act: 'withheld' }] } })
  })
})

describe('T-B3-08 · folding lines into what is drawn', () => {
  test('rows cap at 500, newest kept; the latest health and learner', () => {
    const lines = Array.from({ length: MAX_ROWS + 20 }, () => parseLine(LINE.start, SESSION)) as Line[]
    const f = fold(EMPTY, [...lines, parseLine(LINE.healthy, SESSION) as Line, parseLine(LINE.learnerRunning, SESSION) as Line], 0)
    expect(f.rows).toHaveLength(MAX_ROWS)
    expect(f.rows[0]?.seq).toBe(21)
    expect(f.rows[MAX_ROWS - 1]?.seq).toBe(520)
    expect(f.health?.word).toBe('healthy')
    expect(f.learner).toEqual({ state: 'running', nextAt: null, ranAt: null })
    const idle = fold(f, [parseLine(LINE.learnerIdle, SESSION) as Line], 77)
    expect(idle.learner?.ranAt).toBe(77)
    expect(idle.rows).toBe(f.rows)
  })

  test('the last cursor a chunk carried', () => {
    const lines = [LINE.start, LINE.firedGen].map(l => parseLine(l, SESSION)) as Line[]
    expect(lastCursor(lines, 'old')).toMatch(/:0:2$/)
    expect(lastCursor([], 'old')).toBe('old')
  })
})
