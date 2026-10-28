// The connector result mapper (world/map.ts; T-W3-01 … T-W3-03): the placeholders in a provider's
// arguments, the pointer walk over a result, a calendar list as artifacts, events and blocks, a
// pull request list as artifacts and events, and what a malformed result yields (nothing).
import { describe, expect, test } from 'claude-code/testing'

import { DESCRIPTORS, T0 } from '../fixtures/kit'
import { artifactId, at, fillArgs, mapped, personOf, resultValue, rows, segments, timeOf } from './map'
import { descriptorsOf } from './sources'

const [CAL, GH] = descriptorsOf(DESCRIPTORS) as [ReturnType<typeof descriptorsOf>[number], ReturnType<typeof descriptorsOf>[number]]
const calProvider = CAL.providers[0]
const ghProvider = GH.providers[0]
if (calProvider === undefined || ghProvider === undefined) throw new Error('fixture')

const iso = (ms: number) => new Date(ms).toISOString()

/** A Google Calendar `list_events` result: a meeting that ended, one under way, one declined. */
const CALENDAR = (now: number) => ({
  items: [
    {
      id: 'ev1',
      summary: 'Marketing sync',
      start: { dateTime: iso(now - 50 * 60_000) },
      end: { dateTime: iso(now - 20 * 60_000) },
      attendees: [{ email: 'ana.perez@acme.com', responseStatus: 'accepted' }, { email: 'bo@acme.com', responseStatus: 'accepted' }],
      attachments: [{ fileUrl: 'https://docs.example/notes' }],
    },
    { id: 'ev2', summary: 'Focus block', start: { dateTime: iso(now - 5 * 60_000) }, end: { dateTime: iso(now + 55 * 60_000) }, transparency: 'transparent' },
    { id: 'ev3', summary: 'All hands', start: { dateTime: iso(now + 2 * 3_600_000) }, end: { dateTime: iso(now + 3 * 3_600_000) }, attendees: [{ email: 'me@acme.com', responseStatus: 'declined' }] },
    { id: 'ev4', start: { dateTime: iso(now) }, end: { dateTime: iso(now + 60_000) } },
  ],
})

describe('T-W3-01 · the arguments and the pointers', () => {
  test('T-W3-01: {{now}}, {{now-lookback}} and {{now+lookahead}} are filled as RFC 3339, in nested arguments; nothing else changes', () => {
    const args = fillArgs({ timeMin: '{{now-lookback}}', timeMax: '{{now+lookahead}}', at: '{{now}}', deep: { list: ['{{now}}', 3, true] }, n: 1 }, CAL, T0)
    expect(args).toEqual({ timeMin: iso(T0 - 2 * 3_600_000), timeMax: iso(T0 + 8 * 3_600_000), at: iso(T0), deep: { list: [iso(T0), 3, true] }, n: 1 })
  })

  test('T-W3-01: the pointer segments, unescaped; a walk fans out at a star, indexes a list, and reads nothing off a scalar', () => {
    expect(segments('/items/*/start/dateTime')).toEqual(['items', '*', 'start', 'dateTime'])
    expect(segments('/a~1b/c~0d')).toEqual(['a/b', 'c~d'])
    const v = { items: [{ id: 1, tags: ['x', 'y'] }, { id: 2, tags: [] }], n: 7 }
    expect(at(v, segments('/items/*/id'))).toEqual([1, 2])
    expect(at(v, segments('/items/*/tags/*'))).toEqual(['x', 'y'])
    expect(at(v, segments('/items/1/id'))).toEqual([2])
    expect(at(v, segments('/n/x'))).toEqual([])
    expect(at(v, [])).toEqual([v])
  })

  test('T-W3-01: rows: one per element of the first starred list, root fields the same in every row; no star, one row', () => {
    const r = rows({ id: '/items/*/id', who: '/items/*/people/*/name', root: '/kind' }, { kind: 'k', items: [{ id: 'a', people: [{ name: 'Ana' }, { name: 'Bo' }] }, { id: 'b' }] })
    expect(r).toEqual([
      { id: ['a'], who: ['Ana', 'Bo'], root: ['k'] },
      { id: ['b'], who: [], root: ['k'] },
    ])
    expect(rows({ id: '/id' }, { id: 'one' })).toEqual([{ id: ['one'] }])
    expect(rows({ id: '/items/*/id' }, { items: 'not a list' })).toEqual([])
  })

  test('T-W3-01: a result is its structured content, else the JSON of its text blocks; anything else is nothing', () => {
    expect(resultValue({ content: [], structuredContent: { a: 1 } })).toEqual({ a: 1 })
    expect(resultValue({ content: [{ type: 'text', text: '{"a":' }, { type: 'text', text: '2}' }] })).toEqual({ a: 2 })
    expect(resultValue({ content: [{ type: 'text', text: 'not json' }] })).toBeUndefined()
    expect(resultValue({ content: [{ type: 'image' }] })).toBeUndefined()
    expect(resultValue('x')).toBeUndefined()
  })

  test('T-W3-01: a time in RFC 3339, seconds or milliseconds; a person from an address', () => {
    expect(timeOf('2026-03-04T10:00:00Z')).toBe(Date.parse('2026-03-04T10:00:00Z'))
    expect(timeOf(1_700_000_000)).toBe(1_700_000_000_000)
    expect(timeOf(1_700_000_000_000)).toBe(1_700_000_000_000)
    expect([timeOf('soon'), timeOf(null)]).toEqual([undefined, undefined])
    expect(personOf('ana.perez@acme.com')).toBe('Ana Perez')
    expect(personOf('bo_lee')).toBe('Bo Lee')
    expect(personOf('')).toBe('')
    expect([artifactId(GH, '418'), artifactId(CAL, 'ev1')]).toEqual(['github:pr:418', 'calendar:ev1'])
  })
})

describe('T-W3-02, T-W3-03 · a result mapped', () => {
  test('T-W3-02: a calendar list: each meeting an artifact, its attachments related, a block with busy and accepted read by the when-lists; the ended one ended, the current one started; a row without a title dropped', () => {
    const b = mapped(CAL, calProvider, { content: [], structuredContent: CALENDAR(T0) }, T0)
    expect(b.source).toBe('calendar')
    expect(b.via).toBe('mod')
    expect(b.server).toBe('claude.ai Google Calendar')
    expect(b.artifacts.map(a => [a.id, a.kind, a.title])).toEqual([
      ['calendar:ev1', 'calendar_event', 'Marketing sync'],
      ['url:https://docs.example/notes', 'doc', 'Marketing sync (attachment)'],
      ['calendar:ev2', 'calendar_event', 'Focus block'],
      ['calendar:ev3', 'calendar_event', 'All hands'],
    ])
    expect(b.artifacts[0]).toMatchObject({ related: ['url:https://docs.example/notes'], people: ['Ana Perez', 'Bo'], locator: 'ev1', first_seen: T0 - 50 * 60_000, last_touched: T0 - 20 * 60_000 })
    expect(b.artifacts[1]).toMatchObject({ locator: 'https://docs.example/notes', related: ['calendar:ev1'] })
    expect(b.events.map(e => [e.artifact, e.action, e.ts])).toEqual([
      ['calendar:ev1', 'ended', T0 - 20 * 60_000],
      ['calendar:ev2', 'started', T0 - 5 * 60_000],
      ['calendar:ev3', 'seen', T0],
    ])
    expect(b.blocks?.map(x => [x.id, x.busy, x.accepted, x.focus, x.attendees])).toEqual([
      ['calendar:ev1', true, true, false, 2],
      ['calendar:ev2', false, true, true, 0],
      ['calendar:ev3', true, false, false, 1],
    ])
  })

  test('T-W3-03: a pull request list: an artifact with its state, locator and people, one seen event each; an empty or malformed result is an empty batch', () => {
    const result = { content: [{ type: 'text', text: JSON.stringify({ items: [{ number: 418, title: 'Fix the lockfile', html_url: 'https://github.com/acme/x/pull/418', state: 'open', updated_at: '2026-03-04T09:00:00Z', user: { login: 'ana' } }, { number: 419 }] }) }] }
    const b = mapped(GH, ghProvider, result, T0)
    expect(b.artifacts).toEqual([{ id: 'github:pr:418', kind: 'pr', title: 'Fix the lockfile', locator: 'https://github.com/acme/x/pull/418', first_seen: Date.parse('2026-03-04T09:00:00Z'), last_touched: Date.parse('2026-03-04T09:00:00Z'), state: 'open', people: ['Ana'] }])
    expect(b.events).toEqual([{ ts: T0, source: 'github', kind: 'pr', artifact: 'github:pr:418', action: 'seen' }])
    expect(b.blocks).toBeUndefined()
    expect(mapped(GH, ghProvider, { content: [{ type: 'text', text: 'oops' }] }, T0)).toEqual({ source: 'github', via: 'mod', server: 'github*', artifacts: [], events: [] })
    expect(mapped(GH, ghProvider, { content: [], structuredContent: { items: 'none' } }, T0).artifacts).toEqual([])
  })
})
