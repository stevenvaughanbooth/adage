// The World tab's words, as plain values (the implementation plan's W3; ux-design §6.5): the
// header, the day strip (the calendar as a bar of the working day, each cell a quarter hour, the
// blocks filled and the present marked), the timeline's lines, the sources' lines and the hints'.
// Pure: draw/pane.tsx lays these out.
import type { WorldBlock, WorldEvent, WorldHint, WorldSource, WorldState } from '../plugin'
import type { Role } from '../tokens'
import { clock } from './pane'
import { g, plural } from './text'

const pad2 = (n: number) => String(n).padStart(2, '0')

/** `HH:MM` in the person's local time. */
export const hm = (ts: number): string => {
  const d = new Date(ts)
  return `${pad2(d.getHours())}:${pad2(d.getMinutes())}`
}

/** `12 min ago`, `2 h ago`, `just now`. */
export function ago(now: number, ts: number): string {
  const s = Math.max(0, Math.floor((now - ts) / 1000))
  if (s < 60) return 'just now'
  if (s < 5400) return `${Math.floor(s / 60)} min ago`
  if (s < 172_800) return `${Math.round(s / 3600)} h ago`
  return `${Math.floor(s / 86_400)} days ago`
}

/** `in 4 min`, `in 2 h`, `now`. */
export function until(now: number, ts: number): string {
  const s = Math.floor((ts - now) / 1000)
  if (s < 60) return 'now'
  if (s < 3600) return `in ${Math.floor(s / 60)} min`
  return `in ${Math.round(s / 3600)} h`
}

/** The header's cells: the state word, the sources live, the hints this session. */
export function worldHeader(w: WorldState, ascii: boolean): { word: string; role: Role; facts: string[] } {
  const word = !w.enabled ? 'off' : w.paused ? 'paused' : w.status === 'stopped' ? 'feed stopped' : 'on'
  const role: Role = !w.enabled ? 'ink3' : w.paused || w.status === 'stopped' ? 'prime' : 'fire'
  const facts: string[] = []
  if (w.enabled) {
    const live = w.sources.filter(s => s.ok && s.lastPoll > 0).length
    facts.push(`${plural(live, 'source', 'sources')} live${w.sources.length > live ? ` of ${w.sources.length}` : ''}`)
    if (w.hints.length > 0) facts.push(`${plural(w.hints.length, 'hint', 'hints')} this session`)
    if (w.artifacts > 0) facts.push(`${plural(w.artifacts, 'thing', 'things')} it knows`)
  } else {
    facts.push(`adage world on ${g('sep', ascii)} record what you do, for the agent`)
  }
  return { word, role, facts }
}

/** The day strip's cells: one per quarter hour from `from` to `to` (local hours). */
export type StripCell = { ch: string; role: Role; now: boolean }

export const STRIP_FROM = 7
export const STRIP_TO = 20
export const QUARTER_MS = 15 * 60_000

/**
 * The day as a bar: each cell a quarter hour of the working day, filled where a busy block lies
 * (the present block in the accent, the past in the quiet role, what is ahead in the prime role),
 * the present quarter marked, free time as a dim rail. Fits `columns` by widening each cell to
 * whole columns where there is room, never narrower than one.
 */
export function dayStrip(blocks: readonly WorldBlock[], now: number, columns: number, ascii: boolean): { cells: StripCell[]; labels: string } {
  const d = new Date(now)
  const dayStart = new Date(d.getFullYear(), d.getMonth(), d.getDate(), STRIP_FROM, 0, 0, 0).getTime()
  const quarters = (STRIP_TO - STRIP_FROM) * 4
  const cells: StripCell[] = []
  for (let q = 0; q < quarters; q++) {
    const t = dayStart + q * QUARTER_MS
    const block = blocks.find(b => b.busy && b.accepted && b.start < t + QUARTER_MS && b.end > t)
    const isNow = now >= t && now < t + QUARTER_MS
    if (block === undefined) cells.push({ ch: isNow ? g('active', ascii) : ascii ? '.' : '╌', role: isNow ? 'accent' : 'ink3', now: isNow })
    else {
      const past = block.end <= now
      const current = block.start <= now && now < block.end
      cells.push({ ch: isNow ? g('active', ascii) : ascii ? '#' : '█', role: current ? 'accent' : past ? 'ink3' : block.focus ? 'inhibit' : 'prime', now: isNow })
    }
  }
  // Hour labels every two hours under their cells (four cells an hour), as wide as the strip.
  const width = Math.max(8, Math.min(columns, cells.length))
  const labels = Array.from({ length: width }, () => ' ')
  for (let h = STRIP_FROM; h < STRIP_TO; h += 2) {
    const col = (h - STRIP_FROM) * 4
    if (col + 1 < width) [labels[col], labels[col + 1]] = [...pad2(h)] as [string, string]
  }
  return { cells: cells.slice(0, width), labels: labels.join('').trimEnd() }
}

/** The block under way or next, in one line: `now: Marketing sync until 10:30` or `next: 13:00 Marketing sync (in 2 h)`. */
export function blockLine(blocks: readonly WorldBlock[], now: number): string {
  const live = blocks.filter(b => b.busy && b.accepted)
  const current = live.find(b => b.start <= now && now < b.end)
  if (current !== undefined) return `now: ${current.title} until ${hm(current.end)}`
  const next = live.filter(b => b.start > now).sort((a, b) => a.start - b.start)[0]
  if (next !== undefined) return `next: ${hm(next.start)} ${next.title} (${until(now, next.start)})`
  return live.length === 0 ? 'no meetings on the calendar today' : 'no more meetings today'
}

const KIND_WORD: Readonly<Record<string, string>> = {
  calendar_event: 'meeting',
  transcript: 'transcript',
  pr: 'pull request',
  ticket: 'ticket',
  doc: 'document',
  file: 'file',
  url: 'page',
}

const ACTION_WORD: Readonly<Record<string, string>> = {
  seen: 'seen',
  opened: 'opened',
  edited: 'edited',
  started: 'started',
  ended: 'ended',
  landed: 'landed',
  changed: 'changed',
}

/** A thing's name for a line: the title the state knows for its id, else the id's tail. */
export function titleOf(e: WorldEvent, titles: Readonly<Record<string, string>>): string {
  const t = titles[e.artifact]
  if (t !== undefined && t !== '') return t
  const tail = e.artifact.split(':').pop() ?? e.artifact
  return tail.split('/').pop() ?? tail
}

/** One timeline line: `09:52  github    pull request #418 changed · ci red`. */
export function eventLine(e: WorldEvent, titles: Readonly<Record<string, string>>, ascii: boolean): string {
  const kind = KIND_WORD[e.kind] ?? e.kind
  const detail = e.detail === '' ? '' : ` ${g('sep', ascii)} ${e.detail.replace(':', ' ')}`
  return `${clock(e.ts).slice(0, 5)}  ${e.source.padEnd(9)} ${kind} ${titleOf(e, titles)} ${ACTION_WORD[e.action] ?? e.action}${detail}`
}

/** A source's line and lamp: `calendar  claude.ai Google Calendar · polled 2 min ago · 14 events`. */
export function sourceLine(s: WorldSource, now: number, ascii: boolean): { glyph: string; role: Role; text: string } {
  const sep = g('sep', ascii)
  const polled = s.lastPoll === 0 ? 'not polled yet' : `polled ${ago(now, s.lastPoll)}`
  const where = s.server === '' ? '' : `${s.server} ${sep} `
  if (!s.ok) return { glyph: g('fail', ascii), role: 'inhibit', text: `${s.name.padEnd(9)} ${where}${polled} ${sep} ${s.error || 'failed'}` }
  if (s.lastPoll === 0) return { glyph: g('off', ascii), role: 'ink3', text: `${s.name.padEnd(9)} ${where}${polled}` }
  return { glyph: g('lamp', ascii), role: 'fire', text: `${s.name.padEnd(9)} ${where}${polled} ${sep} ${plural(s.events, 'event', 'events')}` }
}

/** A hint's line: `10:12  ◠ hint  "Marketing sync" (meeting, ended 12 min ago, 4 attendees)`. */
export function hintLine(h: WorldHint, ascii: boolean): string {
  return `${clock(h.ts).slice(0, 5)}  ${g('reflex', ascii)} ${h.cls === 'hint' ? 'hint ' : 'asked'} ${h.text}`
}

/** The empty words. */
export const WORLD_EMPTY = 'Nothing yet. Connect a calendar or GitHub to Claude Code (/mcp) and set world.transcripts to where your meetings land.'
export const WORLD_OFF = 'The world is off: Adage records nothing of what you do. adage world on turns it on, a person at a terminal.'
