// The feed's logic as pure functions and the reconnect loop with a stubbed child (T-B3-08), and
// the arm scrub on this session's own rows (T-X-06's binary half, done again in the mod).
import type { ProcessSpawnChunk } from 'claude-code'
import { describe, expect, test } from 'claude-code/testing'

import { HEALTHY_MS, MAX_ROWS, afterEnd, fold, follow, followArgs, lastCursor, parseLine, splitLines, stem } from './feed'
import type { FollowDeps, Folded, Line } from './feed'
import { LINE, SESSION } from './fixtures/kit'

const EMPTY: Folded = { rows: [], ticks: [], health: null, learner: null }

describe('the follower argv and the line buffer', () => {
  test('nextArgs: the --since form only once a cursor exists', () => {
    expect(followArgs('/d/bin/adage', '/w', 's1', '')).toEqual(['/d/bin/adage', 'watch', '--follow', '--json', '--repo', '/w', '--session', 's1'])
    expect(followArgs('/d/bin/adage', '/w', 's1', '1~5~0~s1:0:2')).toEqual(['/d/bin/adage', 'watch', '--follow', '--json', '--repo', '/w', '--session', 's1', '--since', '1~5~0~s1:0:2'])
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

describe('the lines', () => {
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

describe('folding lines into what is drawn', () => {
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

describe('reconnecting', () => {
  test('three early ends in a row stop the feed; a long-lived child starts the count again', () => {
    expect(afterEnd(0, 10)).toEqual({ failures: 1, stop: false })
    expect(afterEnd(1, 10)).toEqual({ failures: 2, stop: false })
    expect(afterEnd(2, 10)).toEqual({ failures: 3, stop: true })
    expect(afterEnd(2, HEALTHY_MS)).toEqual({ failures: 1, stop: false })
  })

  /** A follower whose children print `per[k]` and end; the clock moves `lived[k]` per child. */
  function loop(per: string[][], lived: number[] = [], paused = () => false) {
    let now = 0
    let k = 0
    const argvs: string[][] = []
    const applied: string[] = []
    const statuses: string[] = []
    const deps: FollowDeps = {
      spawn: argv => {
        argvs.push(argv)
        const text = (per[k] ?? []).join('\n')
        const life = lived[k] ?? 10
        k++
        return (async function* (): AsyncGenerator<ProcessSpawnChunk, unknown> {
          if (text !== '') yield { stream: 'stdout', text: text.slice(0, 30) }
          if (text !== '') yield { stream: 'stdout', text: `${text.slice(30)}\n` }
          now += life
          return undefined
        })()
      },
      now: async () => now,
      argv: since => followArgs('/d/bin/adage', '/w', SESSION, since),
      apply: async (lines, cursor) => {
        applied.push(...lines.map(l => (l.kind === 'decision' ? l.row.event : l.kind)), `@${cursor}`)
      },
      status: async (s, f) => {
        statuses.push(`${s}:${f}`)
      },
      isPaused: paused,
      parse: text => parseLine(text, SESSION),
    }
    return { deps, argvs, applied, statuses }
  }

  test('a restart resumes --since the last cursor: nothing lost, nothing repeated; three early ends stop it', async () => {
    const l = loop([[LINE.start, LINE.firedGen], [LINE.followedGen], []])
    const end = await follow(l.deps, '', 0)
    expect(end.end).toBe('stopped')
    expect(l.argvs.map(a => a.slice(8))).toEqual([[], ['--since', JSON.parse(LINE.firedGen).cursor], ['--since', JSON.parse(LINE.followedGen).cursor]])
    expect(l.applied).toEqual(['start', 'tools', `@${JSON.parse(LINE.firedGen).cursor}`, 'tools', `@${JSON.parse(LINE.followedGen).cursor}`])
    expect(l.statuses).toEqual(['following:0', 'reconnecting:1', 'reconnecting:2', 'stopped:3'])
  })

  test('a child that lived a minute is not an early end', async () => {
    const l = loop([[LINE.start], [LINE.start], [LINE.start], [], [], []], [HEALTHY_MS, HEALTHY_MS, 10, 10, 10])
    await follow(l.deps, '', 0)
    // Two long children, then three early ends: the first long end already counted as one.
    expect(l.argvs).toHaveLength(4)
    expect(l.statuses.at(-1)).toBe('stopped:3')
  })

  test('a pause ends the loop with its cursor, for a resume to start from', async () => {
    let paused = false
    const l = loop([[LINE.start]], [], () => paused)
    const deps = { ...l.deps, apply: async (lines: Line[], cursor: string) => { paused = true; await l.deps.apply(lines, cursor) } }
    const end = await follow(deps, '', 0)
    expect(end).toEqual({ end: 'paused', cursor: JSON.parse(LINE.start).cursor })
    expect(l.statuses).toEqual(['following:0', 'paused:0'])
  })
})
