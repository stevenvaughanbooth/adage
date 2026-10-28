// The world's follow child read (world/follow.ts; T-W3-09, T-W3-10): its argv, its lines parsed
// (an event with its title, the health with the sources), what a chunk changes in the state, and
// the child's life over a session through the engine (started on the world's being on, ended by
// the session's end).
import { describe, expect, test } from 'claude-code/testing'

import { CLI, SESSION, T0, WORLD_OFF_JSON, begin, fake, mountPane, shown, world } from '../fixtures/kit'
import { MAX_WORLD_EVENTS, WORLD0 } from '../plugin'
import { MAX_TITLES, applyWorld, lastWorldCursor, parseWorldLine, startWorldFeed, stopWorldFeed, withTitles, worldFollowArgs } from './follow'
import type { WorldLine } from './follow'

const EVENT = (cursor: number, artifact = 'github:pr:418', title = 'Fix the lockfile', ts = T0 - 60_000) =>
  JSON.stringify({ cursor, event: { ts, source: 'github', kind: 'pr', artifact, action: 'changed', detail: 'ci:red' }, title, session: SESSION })
const HEALTH = (cursor: number, ok = true) =>
  JSON.stringify({ cursor, health: { sources: { github: { name: 'github', via: 'mod', server: 'github', last_poll: T0 - 120_000, ok, error: ok ? '' : 'call failed: 401', events: 3 }, meetings: { name: 'meetings', via: 'follow', server: '', last_poll: 0, ok: true, error: '', events: 0 } } }, polling: true, live: true })

describe('T-W3-09 · the lines', () => {
  test('T-W3-09: the argv, with --since only once a cursor exists', () => {
    expect(worldFollowArgs(CLI, SESSION, '')).toEqual([CLI, 'world', 'follow', '--session', SESSION, '--stdin', 'ignore'])
    expect(worldFollowArgs(CLI, SESSION, '42')).toEqual([CLI, 'world', 'follow', '--session', SESSION, '--stdin', 'ignore', '--since', '42'])
  })

  test('T-W3-09: an event line with its title and cursor; a health line with the sources sorted and the lamps; anything else skipped', () => {
    expect(parseWorldLine(EVENT(7))).toEqual({ kind: 'event', cursor: '7', event: { ts: T0 - 60_000, source: 'github', kind: 'pr', artifact: 'github:pr:418', action: 'changed', detail: 'ci:red' }, title: 'Fix the lockfile' })
    const h = parseWorldLine(HEALTH(9, false))
    expect(h).toEqual({
      kind: 'health',
      cursor: '9',
      sources: [
        { name: 'github', server: 'github', lastPoll: T0 - 120_000, ok: false, error: 'call failed: 401', events: 3 },
        { name: 'meetings', server: 'follow', lastPoll: 0, ok: true, error: '', events: 0 },
      ],
      polling: true,
      live: true,
    })
    expect(parseWorldLine('not json')).toBeUndefined()
    expect(parseWorldLine('{"cursor":1}')).toBeUndefined()
    expect(parseWorldLine(JSON.stringify({ cursor: 1, event: { ts: 0, source: 'x', artifact: 'y' } }))).toBeUndefined()
    const lines = [parseWorldLine(EVENT(7)), parseWorldLine(HEALTH(9))].filter((l): l is WorldLine => l !== undefined)
    expect(lastWorldCursor(lines, '1')).toBe('9')
    expect(lastWorldCursor([], '1')).toBe('1')
  })

  test('T-W3-09: the titles kept by id, the newest last, the oldest dropped past the cap; an empty title adds nothing', () => {
    expect(withTitles({ a: 'A' }, [{ id: 'b', title: 'B' }, { id: '', title: 'x' }, { id: 'c', title: '' }])).toEqual({ a: 'A', b: 'B' })
    const many = Array.from({ length: MAX_TITLES + 5 }, (_, k) => ({ id: `t${k}`, title: `T${k}` }))
    const kept = withTitles({}, many)
    expect(Object.keys(kept)).toHaveLength(MAX_TITLES)
    expect(kept.t0).toBeUndefined()
    expect(kept[`t${MAX_TITLES + 4}`]).toBe(`T${MAX_TITLES + 4}`)
  })

  test('T-W3-09: a chunk puts its events first, newest first, capped; its last health line is the sources; the pane is redrawn', async () => {
    const f = fake({ state: { world: { ...WORLD0, enabled: true, events: [{ ts: 1, source: 'old', kind: 'pr', artifact: 'o', action: 'seen', detail: '' }] } } })
    f.m.rt.interactive = true
    const lines = [EVENT(2, 'a', 'A', 10), EVENT(3, 'b', 'B', 20), HEALTH(4, false), HEALTH(5)].map(parseWorldLine).filter((l): l is WorldLine => l !== undefined)
    await applyWorld(f.m, lines)
    expect(f.state.world.events.map(e => e.artifact)).toEqual(['b', 'a', 'o'])
    expect(f.state.world.titles).toEqual({ a: 'A', b: 'B' })
    expect(f.state.world.sources.map(s => s.ok)).toEqual([true, true])
    expect(f.state.world.polling).toBe(true)
    expect(f.invalidated).toBe(1)
    const flood = Array.from({ length: MAX_WORLD_EVENTS + 10 }, (_, k) => parseWorldLine(EVENT(k, `e${k}`, '', k))).filter((l): l is WorldLine => l !== undefined)
    await applyWorld(f.m, flood)
    expect(f.state.world.events).toHaveLength(MAX_WORLD_EVENTS)
    expect(f.state.world.events[0]?.artifact).toBe(`e${MAX_WORLD_EVENTS + 9}`)
    await applyWorld(f.m, [])
    expect(f.invalidated).toBe(2)
  })
})

describe('T-W3-10 · the child over a session', () => {
  test('T-W3-10: nothing without the CLI, a person at the prompt, or the world on; started, the child reads from the last cursor and the status follows it', async () => {
    const off = fake({ state: { world: { ...WORLD0, enabled: false } } })
    off.m.rt.cli = CLI
    off.m.rt.interactive = true
    await startWorldFeed(off.m, SESSION, '/w')
    expect(off.spawned).toEqual([])
    const headless = fake({ state: { world: { ...WORLD0, enabled: true } } })
    headless.m.rt.cli = CLI
    await startWorldFeed(headless.m, SESSION, '/w')
    expect(headless.spawned).toEqual([])
    let children = 0
    const f = fake({ state: { world: { ...WORLD0, enabled: true, cursor: '5' } }, spawn: () => (children++ === 0 ? [{ stream: 'stdout', text: `${EVENT(6)}\n${HEALTH(7)}\n` }] : []) })
    f.m.rt.cli = CLI
    f.m.rt.interactive = true
    await startWorldFeed(f.m, SESSION, '/w')
    await new Promise(r => setTimeout(r, 0))
    expect(f.spawned[0]).toEqual([CLI, 'world', 'follow', '--session', SESSION, '--stdin', 'ignore', '--since', '5'])
    expect(f.state.world.events.map(e => e.artifact)).toEqual(['github:pr:418'])
    expect(f.state.world.cursor).toBe('7')
    stopWorldFeed(f.m)
    expect(f.m.rt.worldChild).toBeUndefined()
  })

  test('T-W3-10: through the engine: the child is one `world follow` for the session; its events and sources draw in the World tab', async ($, on) => {
    const w = world(on, { world_json: { ...WORLD_OFF_JSON, enabled: true }, worldFeed: [EVENT(1), HEALTH(2)] })
    await begin($, w)
    const children = w.spawned.filter(a => a[1] === 'world')
    expect(children).toHaveLength(1)
    expect(children[0]).toEqual([CLI, 'world', 'follow', '--session', SESSION, '--stdin', 'ignore'])
    const ui = await mountPane($)
    await ui.press({ key: 'tab-world' })
    const text = (await shown(ui)).join('\n')
    expect(text).toContain('world on')
    expect(text).toContain('pull request Fix the lockfile changed · ci red')
    expect(text).toContain('github')
    expect(text).toContain('polled 2 min ago')
    expect(text).toContain('3 events')
    expect(text).toContain('meetings')
    expect(text).toContain('not polled yet')
    await ui.unmount()
  })
})
