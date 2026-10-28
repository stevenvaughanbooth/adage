// The connector sources' polling (world/poll.ts; T-W3-05 … T-W3-07): one poll's call, map and
// ingest over the fake; a failed call reported and backed off; the sources started at a session's
// start through the engine, each through `$.mcp.call` with the engine's own connection, their
// batches piped to `adage world ingest`, and every timer ended with the session.
import { describe, expect, test } from 'claude-code/testing'

import { DESCRIPTORS, SESSION, T0, WORLD_OFF_JSON, begin, exited, fake, world } from '../fixtures/kit'
import { WORLD0 } from '../plugin'
import { BACKOFF_S, backoff, onEnd, pollDeps, pollOnce, resolve, startWorld, stopWorld } from './poll'
import type { PollDeps } from './poll'
import { descriptorsOf, resolveSources } from './sources'

const TOOLS = [{ name: 'Bash' }, { name: 'mcp__claude_ai_google_calendar__list_events', mcp: true }, { name: 'mcp__github__search_pull_requests', mcp: true }]
const ON = { ...WORLD_OFF_JSON, enabled: true }
const iso = (ms: number) => new Date(ms).toISOString()
const CALENDAR = { items: [{ id: 'ev1', summary: 'Marketing sync', start: { dateTime: iso(T0 - 50 * 60_000) }, end: { dateTime: iso(T0 - 20 * 60_000) }, attendees: [{ email: 'ana@acme.com', responseStatus: 'accepted' }] }] }
const PRS = { items: [{ number: 418, title: 'Fix the lockfile', html_url: 'https://github.com/acme/x/pull/418', state: 'open', updated_at: iso(T0 - 3_600_000) }] }

/** The binary's reads for the world over the fake. */
const reads = (argv: readonly string[]) => {
  if (argv[1] === 'world' && argv[2] === 'descriptors') return exited(0, JSON.stringify(DESCRIPTORS))
  if (argv[1] === 'world' && argv[2] === 'ingest') return exited(0, JSON.stringify({ source: argv[4], events: 1, artifacts: 1, blocks: 0, skipped: 0, problems: [] }))
  return exited(0, '')
}

describe('T-W3-05 · one poll', () => {
  const [cal, gh] = resolveSources(descriptorsOf(DESCRIPTORS), TOOLS)
  if (cal === undefined || gh === undefined) throw new Error('fixture')

  /** Deps that record what happened. */
  function deps(call: PollDeps['call']) {
    const calls: { server: string; tool: string; args: Record<string, unknown> }[] = []
    const ingested: unknown[] = []
    const failed: string[] = []
    const titled: { id: string; title: string }[][] = []
    const d: PollDeps = {
      call: async (server, tool, args) => {
        calls.push({ server, tool, args })
        return call(server, tool, args)
      },
      ingest: async batch => {
        ingested.push(batch)
        return { ok: true, written: batch.events.length, problems: [] }
      },
      failed: async (source, server, error) => void failed.push(`${source} ${server} ${error}`),
      titled: async named => void titled.push([...named]),
      now: async () => T0,
    }
    return { d, calls, ingested, failed, titled }
  }

  test('T-W3-05: the call with the filled arguments, the batch piped, the titles kept; true', async () => {
    const r = deps(async () => ({ content: [], structuredContent: CALENDAR }))
    expect(await pollOnce(r.d, cal)).toEqual({ ok: true, written: 1, error: '' })
    expect(r.calls).toEqual([{ server: 'claude_ai_google_calendar', tool: 'list_events', args: { timeMin: iso(T0 - 2 * 3_600_000), timeMax: iso(T0 + 8 * 3_600_000), singleEvents: true, orderBy: 'startTime' } }])
    expect(r.ingested).toHaveLength(1)
    expect(r.ingested[0]).toMatchObject({ source: 'calendar', via: 'mod', server: 'claude_ai_google_calendar' })
    expect(r.titled).toEqual([[{ id: 'calendar:ev1', title: 'Marketing sync' }]])
    expect(r.failed).toEqual([])
  })

  test('T-W3-05: a call that throws or a server error is reported to the health, nothing ingested; false', async () => {
    const thrown = deps(async () => {
      throw new Error('401 Unauthorized')
    })
    expect(await pollOnce(thrown.d, gh)).toEqual({ ok: false, written: 0, error: 'call failed: 401 Unauthorized' })
    expect(thrown.failed).toEqual(['github github call failed: 401 Unauthorized'])
    expect(thrown.ingested).toEqual([])
    const errored = deps(async () => ({ content: [{ type: 'text', text: 'rate limited' }], isError: true }))
    expect(await pollOnce(errored.d, gh)).toMatchObject({ ok: false, error: 'the server reported an error' })
    expect(errored.failed).toEqual(['github github the server reported an error'])
  })

  test('T-W3-05: the backoff: 1, 2, 4, 8 minutes, then an hour', () => {
    expect(BACKOFF_S).toEqual([60, 120, 240, 480, 3600])
    expect([backoff(0), backoff(1), backoff(2), backoff(3), backoff(4), backoff(5), backoff(99)]).toEqual([60, 60, 120, 240, 480, 3600, 3600])
  })

  test('T-W3-05: the deps over a Mod: ingest runs `world ingest --source` with the batch on stdin and reads its answer; a failure runs it with --error; the titles land in the state', async () => {
    const f = fake({ run: reads, tools: TOOLS, mcp: () => PRS })
    f.m.rt.cli = '/data/bin/adage'
    const d = pollDeps(f.m)
    expect(await pollOnce(d, gh)).toEqual({ ok: true, written: 1, error: '' })
    const ingest = f.ran.find(r => r.argv[2] === 'ingest')
    expect(ingest?.argv.slice(1)).toEqual(['world', 'ingest', '--source', 'github'])
    expect(JSON.parse(ingest?.init?.stdin ?? '{}')).toMatchObject({ source: 'github', via: 'mod', artifacts: [{ id: 'github:pr:418' }] })
    expect(f.mcpCalls.map(c => `${c.server} ${c.tool}`)).toEqual(['github search_pull_requests'])
    expect(f.state.world.titles).toEqual({ 'github:pr:418': 'Fix the lockfile' })
    await d.failed('github', 'github', 'call failed: 500')
    const err = f.ran.at(-1)
    expect(err?.argv.slice(1)).toEqual(['world', 'ingest', '--source', 'github', '--error', 'call failed: 500'])
    expect(JSON.parse(err?.init?.stdin ?? '{}')).toEqual({ source: 'github', via: 'mod', server: 'github' })
  })
})

describe('T-W3-06 · the sources started', () => {
  test('T-W3-06: resolve reads the descriptors and the tools; without the CLI or a failed read, none', async () => {
    const f = fake({ run: reads, tools: TOOLS })
    expect(await resolve(f.m)).toEqual([])
    f.m.rt.cli = '/data/bin/adage'
    expect((await resolve(f.m)).map(r => r.descriptor.name)).toEqual(['calendar', 'github'])
    const failing = fake({ run: () => exited(1, ''), tools: TOOLS })
    failing.m.rt.cli = '/data/bin/adage'
    expect(await resolve(failing.m)).toEqual([])
  })

  test('T-W3-06: the world off, nothing starts; on, each source polls once now and again on its own timer; the state lists the sources; stop ends every timer', async () => {
    const off = fake({ run: reads, tools: TOOLS, mcp: () => CALENDAR })
    off.m.rt.cli = '/data/bin/adage'
    await startWorld(off.m)
    expect(off.mcpCalls).toEqual([])
    const f = fake({ run: reads, tools: TOOLS, mcp: (server: string) => (server === 'github' ? PRS : CALENDAR), state: { world: { ...WORLD0, enabled: true } } })
    f.m.rt.cli = '/data/bin/adage'
    await startWorld(f.m)
    await new Promise(r => setTimeout(r, 0))
    expect(f.mcpCalls.map(c => c.server).sort()).toEqual(['claude_ai_google_calendar', 'github'])
    expect(f.timers.map(t => [t.kind, t.ms]).sort((a, b) => Number(a[1]) - Number(b[1]))).toEqual([['after', 300_000], ['after', 600_000]])
    expect(f.state.world.sources.map(s => [s.name, s.server, s.ok, s.lastPoll > 0, s.events])).toEqual([['calendar', 'claude_ai_google_calendar', true, true, 1], ['github', 'github', true, true, 1]])
    expect(f.state.world.resolved).toBe(2)
    expect(f.m.rt.worldRuns).toHaveLength(2)
    // The calendar's timer fires: a second poll, and the next timer at the descriptor's interval.
    f.timers.find(t => t.ms === 300_000)?.fire()
    await new Promise(r => setTimeout(r, 0))
    expect(f.mcpCalls.filter(c => c.server === 'claude_ai_google_calendar')).toHaveLength(2)
    expect(f.timers).toHaveLength(3)
    stopWorld(f.m)
    expect(f.timers.every(t => t.cancelled)).toBe(true)
    expect(f.m.rt.worldRuns).toEqual([])
  })

  test('T-W3-06: the session end ends every timer and the follow child, and answers as beneath', async () => {
    const f = fake({ run: reads, tools: TOOLS, mcp: () => CALENDAR, state: { world: { ...WORLD0, enabled: true } }, spawn: () => [] })
    f.m.rt.cli = '/data/bin/adage'
    f.m.rt.interactive = true
    await startWorld(f.m)
    f.m.rt.worldChild = { return: async () => ({ value: undefined, done: true }) } as never
    const gen = f.m.rt.worldFeedGen
    const said = await onEnd(f.m, { reason: 'other', sessionId: SESSION } as never, (async (e: { sessionId: string }) => ({ sessionId: e.sessionId })) as never)
    expect(said).toEqual({ sessionId: SESSION })
    expect(f.timers.every(t => t.cancelled)).toBe(true)
    expect(f.m.rt.worldRuns).toEqual([])
    expect(f.m.rt.worldChild).toBeUndefined()
    expect(f.m.rt.worldFeedGen).toBe(gen + 1)
  })

  test('T-W3-06: a source whose call fails backs off, and its timer is the backoff, not the interval', async () => {
    const f = fake({ run: reads, tools: TOOLS, mcp: () => undefined, state: { world: { ...WORLD0, enabled: true } } })
    f.m.rt.cli = '/data/bin/adage'
    await startWorld(f.m)
    await new Promise(r => setTimeout(r, 0))
    expect(f.timers.map(t => t.ms)).toEqual([60_000, 60_000])
    expect(f.ran.filter(r => r.argv[5] === '--error')).toHaveLength(2)
    expect(f.m.rt.worldRuns.map(r => r.failures)).toEqual([1, 1])
    expect(f.state.world.sources.map(s => [s.ok, s.error])).toEqual([[false, 'the server reported an error'], [false, 'the server reported an error']])
    f.timers[0]?.fire()
    await new Promise(r => setTimeout(r, 0))
    expect(f.timers.at(-1)?.ms).toBe(120_000)
  })
})

describe('T-W3-07 · through the engine', () => {
  test('T-W3-07: the world on: the descriptors read, each connector source called through $.mcp.call and its batch piped to world ingest; the follow child spawned; nothing when off', async ($, on) => {
    const w = world(on, { world_json: ON, tools: TOOLS, mcp: (server: string) => (server === 'github' ? PRS : CALENDAR) })
    await begin($, w)
    expect(w.mcpCalls.map(c => `${c.server} ${c.tool}`).sort()).toEqual(['claude_ai_google_calendar list_events', 'github search_pull_requests'])
    expect(w.ingested.map(i => [i.source, i.error, (i.body.artifacts as unknown[]).length]).sort()).toEqual([['calendar', undefined, 1], ['github', undefined, 1]])
    expect(w.spawned.some(argv => argv[1] === 'world' && argv[2] === 'follow' && argv.includes(SESSION) && argv.includes('--stdin') && argv.includes('ignore'))).toBe(true)
  })

  test('T-W3-07: the world off (the default): no descriptors read, no call, no child', async ($, on) => {
    const w = world(on, { tools: TOOLS, mcp: () => CALENDAR })
    await begin($, w)
    expect(w.mcpCalls).toEqual([])
    expect(w.ran.some(r => r.argv[1] === 'world' && r.argv[2] === 'descriptors')).toBe(false)
    expect(w.spawned.some(argv => argv[1] === 'world')).toBe(false)
  })

  test('T-W3-07: no server connected: the world on but no source resolved; a failing server reported with --error', async ($, on) => {
    const none = world(on, { world_json: ON })
    await begin($, none)
    expect(none.mcpCalls).toEqual([])
    expect(none.ran.some(r => r.argv[1] === 'world' && r.argv[2] === 'descriptors')).toBe(true)
  })
})
