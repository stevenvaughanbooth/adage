// A connector's result mapped to the world's events, artifacts and calendar blocks (the
// implementation plan's "Sources as data"): a descriptor's provider names JSON pointers into the
// tool's result, a star walking a list, and nothing else of the result is kept. Pure: poll.ts hands
// the result in and the batch to `adage world ingest`, which checks every field again.
import type { Descriptor, Provider } from './sources'
import { isRecord, str } from '../shape'

/** One artifact as `adage world ingest` takes it (adage_trace::Artifact, less the embedding). */
export type Artifact = {
  id: string
  kind: string
  title: string
  locator: string
  summary?: string
  first_seen: number
  last_touched: number
  related?: string[]
  state?: string
  people?: string[]
}

export type Event = { ts: number; source: string; kind: string; artifact: string; action: string; detail?: string }

export type Block = { id: string; start: number; end: number; title: string; busy: boolean; accepted: boolean; focus: boolean; attendees: number }

/** What one poll writes. */
export type Batch = { source: string; via: 'mod'; server: string; artifacts: Artifact[]; events: Event[]; blocks?: Block[] }

/** `{{now}}`, `{{now-lookback}}` and `{{now+lookahead}}` in a provider's arguments, as RFC 3339. */
export function fillArgs(args: unknown, d: Descriptor, now: number): unknown {
  const iso = (ms: number) => new Date(ms).toISOString()
  const fill = (s: string) =>
    s
      .replaceAll('{{now-lookback}}', iso(now - d.lookback * 1000))
      .replaceAll('{{now+lookahead}}', iso(now + d.lookahead * 1000))
      .replaceAll('{{now}}', iso(now))
  const walk = (v: unknown): unknown => {
    if (typeof v === 'string') return fill(v)
    if (Array.isArray(v)) return v.map(walk)
    if (isRecord(v)) return Object.fromEntries(Object.entries(v).map(([k, x]) => [k, walk(x)]))
    return v
  }
  return walk(args)
}

/** The value an MCP result carries: its structured content, else the JSON its text blocks hold. */
export function resultValue(result: unknown): unknown {
  if (!isRecord(result)) return undefined
  if (result.structuredContent !== undefined) return result.structuredContent
  const blocks = Array.isArray(result.content) ? result.content : []
  const text = blocks
    .filter((b): b is Record<string, unknown> => isRecord(b) && b.type === 'text' && typeof b.text === 'string')
    .map(b => b.text as string)
    .join('')
  if (text.trim() === '') return undefined
  try {
    return JSON.parse(text)
  } catch {
    return undefined
  }
}

/** A JSON pointer's segments (`items`, `*`, `id` for the pointer over every item's id), `~1` and `~0` unescaped. */
export function segments(pointer: string): string[] {
  return pointer
    .split('/')
    .slice(1)
    .map(s => s.replaceAll('~1', '/').replaceAll('~0', '~'))
}

/** The value(s) at `segs` under `v`: one, or every one a `*` fans out to (flattened). */
export function at(v: unknown, segs: readonly string[]): unknown[] {
  if (segs.length === 0) return v === undefined ? [] : [v]
  const [head, ...rest] = segs as [string, ...string[]]
  if (head === '*') {
    if (!Array.isArray(v)) return []
    return v.flatMap(x => at(x, rest))
  }
  if (Array.isArray(v)) {
    const n = Number(head)
    return Number.isInteger(n) ? at(v[n], rest) : []
  }
  return isRecord(v) ? at(v[head], rest) : []
}

/**
 * The rows a provider's map describes: the list its first `*` walks, one row per element, each
 * field read relative to the element (a field with no `*` is read from the root, the same for
 * every row; a field with a second `*` reads a list).
 */
export function rows(map: Readonly<Record<string, unknown>>, value: unknown): Record<string, unknown[]>[] {
  const pointers = Object.entries(map).filter((e): e is [string, string] => typeof e[1] === 'string')
  const starred = pointers.map(([, p]) => segments(p)).find(s => s.includes('*'))
  if (starred === undefined) {
    const one: Record<string, unknown[]> = {}
    for (const [k, p] of pointers) one[k] = at(value, segments(p))
    return [one]
  }
  const prefix = starred.slice(0, starred.indexOf('*'))
  const list = at(value, prefix)[0]
  if (!Array.isArray(list)) return []
  return list.map(el => {
    const row: Record<string, unknown[]> = {}
    for (const [k, p] of pointers) {
      const segs = segments(p)
      const i = segs.indexOf('*')
      row[k] = i === -1 ? at(value, segs) : prefix.every((s, j) => segs[j] === s) ? at(el, segs.slice(i + 1)) : at(value, segs)
    }
    return row
  })
}

/** A time as a source gives it (RFC 3339, or milliseconds) in milliseconds; undefined when it is neither. */
export function timeOf(v: unknown): number | undefined {
  if (typeof v === 'number' && Number.isFinite(v)) return v > 1e12 ? v : v * 1000
  if (typeof v !== 'string') return undefined
  const t = Date.parse(v)
  return Number.isFinite(t) ? t : undefined
}

/** A person as a title would show them: the local part of an address, dots as spaces, title case. */
export function personOf(v: unknown): string {
  const s = str(v)
  const local = s.includes('@') ? (s.split('@')[0] ?? '') : s
  return local
    .split(/[._\s]+/)
    .filter(x => x !== '')
    .map(w => (w[0]?.toUpperCase() ?? '') + w.slice(1))
    .join(' ')
}

const first = (xs: unknown[] | undefined): unknown => xs?.[0]
/** An id as a source gives it: a string, or a number (a pull request's) spelled out. */
const idOf = (v: unknown): string => (typeof v === 'number' && Number.isFinite(v) ? String(v) : str(v))
const strs = (xs: unknown[] | undefined): string[] => (xs ?? []).map(str).filter(s => s !== '')
const when = (xs: unknown[] | undefined, list: unknown, dflt: boolean): boolean => {
  const allowed = Array.isArray(list) ? list.map(str) : undefined
  if (allowed === undefined) return dflt
  const v = first(xs)
  return allowed.includes(v === undefined ? '' : str(v))
}

/** The artifact id a source gives a thing: `<source>:<kind>:<id>` for the world's own namespaces. */
export function artifactId(d: Descriptor, id: string): string {
  return d.kind === 'pr' ? `${d.name}:pr:${id}` : `${d.name}:${id}`
}

/**
 * A result mapped: the batch for `adage world ingest`, or an empty batch when the map found no
 * rows. `server` is the one connected (the provider's pattern resolved); `now` stamps the events;
 * a row with no id or title is dropped.
 */
export function mapped(d: Descriptor, p: Provider, result: unknown, now: number, server = p.server): Batch {
  const batch: Batch = { source: d.name, via: 'mod', server, artifacts: [], events: [] }
  const value = resultValue(result)
  if (value === undefined) return batch
  const blocks: Block[] = []
  for (const r of rows(p.map, value)) {
    const id = idOf(first(r.id))
    const title = str(first(r.title))
    if (id === '' || title === '') continue
    const aid = artifactId(d, id)
    const locator = str(first(r.locator)) || (d.kind === 'pr' ? `#${id}` : id)
    const people = strs(r.attendees).concat(strs(r.people)).map(personOf).filter(x => x !== '').slice(0, 32)
    const touched = timeOf(first(r.updated)) ?? now
    if (d.kind === 'calendar_event') {
      const start = timeOf(first(r.start))
      const end = timeOf(first(r.end))
      if (start === undefined || end === undefined || end < start) continue
      const links = strs(r.links).slice(0, 8)
      const related = links.map(l => `url:${l}`)
      const busy = when(r.busy, p.map.busy_when, true)
      const accepted = when(r.accepted, p.map.accepted_when, true)
      batch.artifacts.push({ id: aid, kind: 'calendar_event', title, locator: id, first_seen: start, last_touched: Math.min(end, now), related, people })
      for (const l of links) batch.artifacts.push({ id: `url:${l}`, kind: 'doc', title: `${title} (attachment)`, locator: l, first_seen: start, last_touched: Math.min(end, now), related: [aid] })
      const action = end <= now ? 'ended' : start <= now ? 'started' : 'seen'
      batch.events.push({ ts: action === 'ended' ? end : action === 'started' ? start : now, source: d.name, kind: 'calendar_event', artifact: aid, action })
      blocks.push({ id: aid, start, end, title, busy, accepted, focus: /\bfocus\b/i.test(title), attendees: strs(r.attendees).length })
      continue
    }
    const state = str(first(r.state))
    batch.artifacts.push({ id: aid, kind: d.kind, title, locator, first_seen: touched, last_touched: touched, ...(state === '' ? {} : { state }), people })
    batch.events.push({ ts: now, source: d.name, kind: d.kind, artifact: aid, action: 'seen' })
  }
  if (d.kind === 'calendar_event') batch.blocks = blocks
  return batch
}
