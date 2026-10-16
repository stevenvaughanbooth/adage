// The live feed's lines (docs/claude-mod/architecture.md §4.1; plan B3): what `adage watch --follow
// --json` prints, one JSON object per line with a `cursor`, read into the rows, ticks, health and
// learner the drawings take. Every spool parser, lock probe and word table stays in Rust: this file
// splits lines, reads them and keeps rows. follower.ts runs the child and applies what this reads.
import type { Act, Health, Learner, Row, Tick } from './plugin'
import { isRecord, num, records, str } from './shape'

/** The most rows the pane keeps (ux-design §6.1: rows older than 500 drop). */
export const MAX_ROWS = 500
/** The conditioning ticker's length. */
const MAX_TICKS = 20

/**
 * The spool stem the binary files a session id under (`adage_store::paths::sanitise`): the id
 * itself when it is all `[A-Za-z0-9_-]` (Claude Code's UUIDs are), else the cleaned id and a hash.
 */
export function stem(id: string): string {
  const clean = Array.from(id)
    .map(c => (/^[A-Za-z0-9_-]$/.test(c) ? c : '_'))
    .slice(0, 96)
    .join('')
  if (clean === id) return clean
  let h = 0xcbf29ce484222325n
  for (const b of new TextEncoder().encode(id)) {
    h ^= BigInt(b)
    h = (h * 0x100000001b3n) & 0xffffffffffffffffn
  }
  return `${clean}-${h.toString(16).padStart(16, '0')}`
}

/**
 * The follower's argv: the `--since` form only once a cursor exists (T-B3-08). `--stdin ignore`:
 * the engine closes a spawned child's stdin at the start, which a follower reading it as its
 * parent's pipe would take for the parent's end.
 */
export function followArgs(cli: string, repo: string, session: string, since: string): string[] {
  const argv = [cli, 'watch', '--follow', '--json', '--stdin', 'ignore', '--repo', repo, '--session', session]
  return since === '' ? argv : [...argv, '--since', since]
}

/** Splits a stream into lines: a chunk may end mid-line, which waits for the next. */
export function splitLines(buffer: string, text: string): { lines: string[]; rest: string } {
  const all = (buffer + text).split('\n')
  const rest = all.pop() ?? ''
  return { lines: all.filter(l => l.trim() !== ''), rest }
}

/** One parsed feed line. */
export type Line =
  | { kind: 'decision'; cursor: string; row: Omit<Row, 'seq'> }
  | { kind: 'learn'; cursor: string; ts: number; ticks: Tick[]; line: Record<string, unknown> }
  | { kind: 'health'; cursor: string; health: Health }
  | { kind: 'learner'; cursor: string; state: Learner['state']; nextAt: number | null }
  | { kind: 'other'; cursor: string }

/**
 * The acts that exist only because the arm is control (A3, "The current session"). The binary
 * removes them from the reader's own rows; the mod removes them again, so a binary that missed one
 * still never draws the arm (ux-design §1.7, T-X-06).
 */
const ARM_ACTS = new Set(['withheld', 'unaided', 'not_reached', 'stop_withheld', 'would_predict'])

function actOf(v: unknown): Act | undefined {
  if (!isRecord(v) || typeof v.act !== 'string') return undefined
  const a: Act = { act: v.act }
  for (const k of ['route', 'strength', 'detail', 'what', 'interpreter', 'candidate'] as const) if (typeof v[k] === 'string') a[k] = v[k] as string
  if (typeof v.rewrite === 'boolean') a.rewrite = v.rewrite
  // A noticed sequence (plan D2): how far it is from a reflex, for the line D5 draws.
  for (const k of ['support', 's_min'] as const) if (typeof v[k] === 'number' && Number.isFinite(v[k])) a[k] = v[k] as number
  if (typeof v.review === 'boolean') a.review = v.review
  if (v.act === 'pretool' && v.withheld === true) return { act: 'withheld' } // a withheld pre-tool record names the arm
  return a
}

/**
 * One decision row. For this session's own rows (`mine`) every arm-naming record is dropped; a row
 * left with no act is kept as a quiet row, which is what a treated session with nothing to say
 * spools for the same event (so the two arms' Live tabs read the same, T-X-06). Another session's
 * rows are kept as `adage watch` shows them.
 */
function rowOf(v: Record<string, unknown>, mine: string): Omit<Row, 'seq'> {
  const session = str(v.session)
  const isMine = session !== '' && session === mine
  const raw = (Array.isArray(v.acts) ? v.acts : []).map(actOf).filter((a): a is Act => a !== undefined)
  const acts = isMine ? raw.filter(a => !ARM_ACTS.has(a.act)) : raw
  const texts = records(v.texts)
    .map(t => ({ route: str(t.route), text: str(t.text) }))
    .filter(t => !isMine || acts.some(a => a.act === 'directed' && a.route === t.route))
  // The calls the record names (`tool_use_ids`): a line drawn under a batch finds its group by them.
  const ids = (Array.isArray(v.tool_use_ids) ? v.tool_use_ids : []).filter((x): x is string => typeof x === 'string' && x !== '')
  return {
    ts: num(v.ts),
    session,
    harness: str(v.harness),
    event: str(v.event),
    control: !isMine && v.control === true,
    acts,
    observed: num(v.observed),
    texts,
    mine: isMine,
    ...(ids.length > 0 ? { ids } : {}),
  }
}

const LAMPS = new Set(['ok', 'warn', 'fail', 'off'])

/** Parses one line; an unparseable line, or one of no kind the mod reads, is skipped (undefined). */
export function parseLine(text: string, mine: string): Line | undefined {
  let v: unknown
  try {
    v = JSON.parse(text)
  } catch {
    return undefined
  }
  if (!isRecord(v) || typeof v.kind !== 'string') return undefined
  const cursor = str(v.cursor)
  switch (v.kind) {
    case 'decision':
      return { kind: 'decision', cursor, row: rowOf(v, mine) }
    case 'learn': {
      // The short form (`learned: …`, not the track's whole key): the pane is narrower than a terminal.
      const ticks = records(v.ticks).map(t => ({
        ts: num(v.ts),
        text: str(t.short) || str(t.text),
        tone: str(t.tone),
        strength: typeof t.strength === 'string' ? t.strength : null,
      }))
      return { kind: 'learn', cursor, ts: num(v.ts), ticks, line: isRecord(v.line) ? v.line : {} }
    }
    case 'health': {
      const state = isRecord(v.state) ? v.state : {}
      const word = state.word === 'problem' || state.word === 'check' ? state.word : 'healthy'
      const lamp = state.lamp === 'inhibit' || state.lamp === 'prime' ? state.lamp : 'fire'
      const lights = records(v.lights).map(l => ({
        label: str(l.label),
        lamp: (LAMPS.has(str(l.lamp)) ? l.lamp : 'off') as Health['lights'][number]['lamp'],
        hint: typeof l.hint === 'string' ? l.hint : null,
      }))
      const check = typeof state.check === 'string' && word !== 'healthy' ? state.check : null
      return { kind: 'health', cursor, health: { lights, word, lamp, check, off: v.off === true } }
    }
    case 'learner': {
      const state = v.state === 'waiting' || v.state === 'running' ? v.state : 'idle'
      return { kind: 'learner', cursor, state, nextAt: typeof v.next_at === 'number' ? v.next_at : null }
    }
    default:
      return { kind: 'other', cursor }
  }
}

/** What the feed's lines say, as the drawings read it. */
export type Folded = { rows: Row[]; ticks: Tick[]; health: Health | null; learner: Learner | null }

/**
 * Folds parsed lines into what was there: rows appended (newest last) and capped at `MAX_ROWS`,
 * ticks capped at `MAX_TICKS`, the latest health and learner state. `now` is when the lines came:
 * a learner that goes from running to idle is `learned` for 10 s from then.
 */
export function fold(was: Folded, lines: readonly Line[], now: number): Folded {
  let { rows, ticks, health, learner } = was
  let seq = rows.length > 0 ? (rows[rows.length - 1]?.seq ?? 0) : 0
  const added: Row[] = []
  for (const l of lines) {
    if (l.kind === 'decision') added.push({ ...l.row, seq: ++seq })
    else if (l.kind === 'learn' && l.ticks.length > 0) ticks = [...ticks, ...l.ticks].slice(-MAX_TICKS)
    else if (l.kind === 'health') health = l.health
    else if (l.kind === 'learner') {
      const ranAt = l.state === 'idle' && learner?.state === 'running' ? now : (learner?.ranAt ?? null)
      learner = { state: l.state, nextAt: l.nextAt, ranAt }
    }
  }
  if (added.length > 0) rows = [...rows, ...added].slice(-MAX_ROWS)
  return { rows, ticks, health, learner }
}

/** The last cursor any of the lines carried, or `since` when none did. */
export function lastCursor(lines: readonly Line[], since: string): string {
  for (let i = lines.length - 1; i >= 0; i--) {
    const c = lines[i]?.cursor ?? ''
    if (c !== '') return c
  }
  return since
}
