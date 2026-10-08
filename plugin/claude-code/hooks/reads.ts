// Reading the binary's `--json` objects (architecture §2: what the mod draws it reads from `adage`
// itself): the status card, `graph --json`'s reflexes, `doctor --json`'s checks, `delegate status
// --json`'s lapsed grants, `status --json`'s packs awaiting trust and candidates, and the map's
// address. Each keeps only what a drawing reads (never a field that would name the arm, ux-design
// §1.7); anything malformed reads as nothing. refresh.ts makes the reads.
import type { Candidate, Check, Lapse, Pack, Reflex, Update } from './plugin'
import { isRecord, records, str, strings } from './shape'

/** The status card as `adage --json --for-session` gives it (A5). */
export type Card = {
  header: string
  /** The binary's version ('' when the card does not say). */
  version: string
  off: boolean
  rows: { key?: string; text: string }[]
  next: { command: string; why: string }[]
  /** A newer Adage, when the binary knows one (onboarding plan O6). */
  update?: Offer
}

/** A newer Adage as the card offers it: the band's line is the mod's to decide (`Update.band`). */
export type Offer = Omit<Update, 'band'>

/** Reads the card JSON; undefined when it is not one. */
export function cardOf(v: unknown): Card | undefined {
  if (!isRecord(v) || typeof v.header !== 'string') return undefined
  const rows = records(v.rows).map(r => ({ ...(typeof r.key === 'string' ? { key: r.key } : {}), text: typeof r.text === 'string' ? r.text : '' }))
  const next = records(v.next).map(n => ({ command: typeof n.command === 'string' ? n.command : '', why: typeof n.why === 'string' ? n.why : '' })).filter(n => n.command !== '')
  const update = updateOf(v)
  return { header: v.header, version: str(v.version), off: v.off === true, rows, next, ...(update === undefined ? {} : { update }) }
}

const VERSION = /^\d+\.\d+\.\d+[0-9A-Za-z.+-]*$/

/**
 * The card's `update` (`{ current, latest, next, notes, … }`, docs/claude-mod/impl/O5-O7-O8-notes.md):
 * the binary decides whether a release is newer, and the mod draws what it says. Undefined when
 * the card says none, or anything malformed (a version that is not one, no command).
 */
export function updateOf(card: unknown): Offer | undefined {
  const u = isRecord(card) ? card.update : undefined
  if (!isRecord(u)) return undefined
  const have = str(u.current)
  const latest = str(u.latest)
  const next = str(u.next)
  if (!VERSION.test(have) || !VERSION.test(latest) || next === '') return undefined
  const notes = str(u.notes).split('\n').map(l => l.trim()).filter(l => l !== '').slice(0, 3)
  return { have, latest, next, notes }
}

/** What `adage update` under a press came to: its last stdout line (docs/claude-mod/impl/O6-O9-notes.md). */
export type Updated =
  | { kind: 'updated'; from: string; to: string }
  | { kind: 'latest'; version: string }
  | { kind: 'manual'; command: string; say: string }

/** The last line of a pressed `adage update` that is one of its answers; undefined for anything else. */
export function updatedOf(lines: readonly string[]): Updated | undefined {
  const last = [...lines].reverse().find(l => l.trim() !== '')
  let v: unknown
  try {
    v = JSON.parse(last ?? '')
  } catch {
    return undefined
  }
  if (!isRecord(v)) return undefined
  if (isRecord(v.updated) && str(v.updated.from) !== '' && str(v.updated.to) !== '') return { kind: 'updated', from: str(v.updated.from), to: str(v.updated.to) }
  if (isRecord(v.latest) && str(v.latest.version) !== '') return { kind: 'latest', version: str(v.latest.version) }
  if (isRecord(v.manual) && str(v.manual.command) !== '') return { kind: 'manual', command: str(v.manual.command), say: str(v.manual.say) }
  return undefined
}

/** The opt-in lines the card reports on (`ui.spinner`, `ui.turn_line`, `ui.noticed`); off otherwise. */
export function optInOf(card: Record<string, unknown>): { spinner: boolean; turnLine: boolean; noticed: boolean } {
  const ui = card.ui
  const on = (k: string) => isRecord(ui) && ui[k] === true
  return { spinner: on('spinner'), turnLine: on('turn_line'), noticed: on('noticed') }
}

/**
 * The person's routine model from the status card (`adage --json`): `runtime.routine_model`, a
 * model name; '' when the binary reports none.
 */
export function routineModelOf(card: unknown): string {
  const runtime = isRecord(card) ? card.runtime : undefined
  const model = isRecord(runtime) ? runtime.routine_model : undefined
  return typeof model === 'string' ? model.trim() : ''
}

const label = (v: unknown): string => (isRecord(v) ? str(v.label) || str(v.kind) : str(v))

/** The repository the status card is about (`repo`, `github.com/acme/billing`); '' when it names none. */
export const repoOf = (card: unknown): string => (isRecord(card) ? str(card.repo) : '')

/**
 * The reflexes of `adage graph --json`, keeping only what the pane and the lines draw: never the
 * `arms`, `live` or control counts a route also carries (ux-design §1.7: the arm is never drawn).
 * The graph lists every repository's reflexes; with `repo` named, only that repository's are kept
 * (a route that names no repository is kept), so a session never shows another repository's.
 */
export function reflexesOf(graph: Record<string, unknown>, repo = ''): Reflex[] {
  return records(graph.routes).filter(r => repo === '' || str(r.repo) === '' || str(r.repo) === repo).map(r => {
    const e = isRecord(r.evidence) ? r.evidence : {}
    const review = isRecord(r.review) ? r.review : {}
    const origin = isRecord(r.origin) ? r.origin : {}
    const uplift = isRecord(e.uplift) ? str(e.uplift.line) : ''
    return {
      id: str(r.id),
      track: str(r.track),
      reference: str(r.reference),
      summary: str(r.summary),
      gist: str(r.gist),
      strength: str(r.strength),
      state: str(r.state),
      instinct: r.instinct === true,
      imported: r.imported === true,
      signer: str(origin.signer) || str(origin.label),
      trigger: label(r.trigger),
      step: label(r.step),
      support: typeof e.support === 'number' ? e.support : 0,
      opportunities: typeof e.opportunities === 'number' ? e.opportunities : 0,
      lowerBound: typeof e.lower_bound === 'number' ? e.lower_bound : null,
      describe: str(e.describe),
      uplift,
      directive: str(r.directive),
      verbs: strings(review.verbs),
      reasons: records(review.reasons).map(x => str(x.kind)).filter(k => k !== ''),
      offers: records(review.offers).map(o => ({ kind: str(o.kind), command: str(o.command) })),
    }
  }).filter(r => r.id !== '')
}

/** The checks of `adage doctor --json`. */
export function checksOf(doctor: Record<string, unknown>): Check[] {
  return records(doctor.checks).map(c => {
    const a = isRecord(c.action) ? c.action : undefined
    const action: Check['action'] = a === undefined ? null : a.kind === 'run' && typeof a.command === 'string' ? { kind: 'run', command: a.command } : { kind: 'manual' }
    const status = c.status === 'fail' || c.status === 'warn' ? c.status : 'pass'
    return { label: str(c.label) || str(c.check), status, detail: str(c.detail), fix: str(c.fix), action }
  })
}

/** The lapsed grants of `adage delegate status --json`. */
export function lapsedOf(status: Record<string, unknown>): Lapse[] {
  return records(status.lapsed).map(l => ({
    route: str(l.route),
    changed: strings(l.changed),
    reapprove: str(l.reapprove),
  })).filter(l => l.route !== '')
}

/** The team packs awaiting trust in `adage status --json`. */
export function packsOf(status: Record<string, unknown>): Pack[] {
  return records(status.packs_awaiting_trust).map(p => ({ name: str(p.name), command: str(p.command) })).filter(p => p.name !== '')
}

/**
 * The candidates of `adage status --json` (plan D1), as the noticed line words them: what the
 * sequence would tell the agent (the gist), never the id; and whether it is a sequence (a context
 * of two or more events), the only kind the line is drawn for.
 */
export function candidatesOf(status: Record<string, unknown>): Candidate[] {
  return records(status.candidates)
    .map(c => {
      const trigger = isRecord(c.trigger) ? c.trigger : {}
      return { id: str(c.id), say: str(c.gist) || str(c.summary), sequence: str(trigger.kind) === 'context' }
    })
    .filter(c => c.id !== '' && c.say !== '')
}

/** The address `adage map` printed (`http://127.0.0.1:<port>/`), as a link the map answers (`localhost`). */
export function mapUrl(text: string): string | undefined {
  const m = /http:\/\/127\.0\.0\.1:(\d{1,5})\//.exec(text)
  return m === null ? undefined : `http://localhost:${m[1]}/`
}
