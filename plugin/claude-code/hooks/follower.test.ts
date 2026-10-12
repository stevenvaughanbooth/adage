// The follower (follower.ts; T-B3-08, ux-design §9.4): the reconnect loop with a stubbed child,
// and through the engine the child's life over a session.
import type { ProcessSpawnChunk } from 'claude-code'
import { describe, expect, test } from 'claude-code/testing'

import { followArgs, parseLine } from './feed'
import type { Line } from './feed'
import { CLI, LINE, SESSION, mountPane, shown, start, world } from './fixtures/kit'
import { HEALTHY_MS, afterEnd, follow } from './follower'
import type { FollowDeps } from './follower'

describe('T-B3-08 · reconnecting', () => {
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
    expect(l.argvs.map(a => a.slice(10))).toEqual([[], ['--since', JSON.parse(LINE.firedGen).cursor], ['--since', JSON.parse(LINE.followedGen).cursor]])
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

describe('T-B3-08, §9.4 · the follower through the engine', () => {
  const FEED = [LINE.learnerIdle, LINE.healthy, LINE.start, LINE.firedGen, LINE.firedLookup]

    test('§9.4: the follower ends three times early: feed stopped, run adage doctor', async ($, on) => {
    const w = world(on, { feed: FEED })
    await start($, w)
    expect(w.spawned.filter(a => a[1] === 'watch').map(a => a.slice(10))).toEqual([[], ['--since', JSON.parse(LINE.firedLookup).cursor], ['--since', JSON.parse(LINE.firedLookup).cursor]])
    const ui = await mountPane($, 'terminal')
    await ui.press({ key: 'tab-live' })
    expect(await shown(ui)).toContain('feed stopped · run adage doctor')
  })

  test('the follower runs `adage watch` in the session directory for this session, reading no stdin', async ($, on) => {
    const w = world(on, { feed: [LINE.learnerIdle] })
    await start($, w)
    expect(w.spawned[0]).toEqual([CLI, 'watch', '--follow', '--json', '--stdin', 'ignore', '--repo', '/w', '--session', SESSION])
  })

  test('a pause ends the child and a resume follows again from the last cursor', async ($, on) => {
    const w = world(on, { feed: [LINE.learnerIdle, LINE.start] })
    await start($, w)
    expect(await $.command.run({ command: 'adage', args: 'pause' } as never)).toEqual({ text: 'adage is paused for this session · /adage pause resumes' })
    await w.clock.settle()
    const before = w.spawned.length
    await $.command.run({ command: 'adage', args: 'pause' } as never)
    await w.clock.settle()
    expect(w.spawned.length).toBeGreaterThan(before)
    expect(w.spawned[before]?.slice(10)).toEqual(['--since', JSON.parse(LINE.start).cursor])
  })

  test('headless: nothing is followed', async ($, on) => {
    const w = world(on, { feed: [LINE.learnerIdle, LINE.start] })
    await start($, w, null, false)
    expect(w.spawned).toEqual([])
  })
})
