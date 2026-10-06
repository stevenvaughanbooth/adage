// Reading the binary's `--json` objects (architecture §2: what the mod draws it reads from `adage`
// itself): the status card, `graph --json`'s reflexes, `doctor --json`'s checks, `delegate status
// --json`'s lapsed grants, `status --json`'s packs awaiting trust and candidates, and the map's
// address. Each keeps only what a drawing reads (never a field that would name the arm, ux-design
// §1.7); anything malformed reads as nothing. refresh.ts makes the reads.
import type { Candidate, Check, Lapse, Pack, Reflex } from './plugin'
import { isRecord, records, str, strings } from './shape'

/** The status card as `adage --json --for-session` gives it (A5). */
export type Card = {
  header: string
  off: boolean
  rows: { key?: string; text: string }[]
  next: { command: string; why: string }[]
}

/** Reads the card JSON; undefined when it is not one. */
export function cardOf(v: unknown): Card | undefined {
  if (!isRecord(v) || typeof v.header !== 'string') return undefined
  const rows = records(v.rows).map(r => ({ ...(typeof r.key === 'string' ? { key: r.key } : {}), text: typeof r.text === 'string' ? r.text : '' }))
  const next = records(v.next).map(n => ({ command: typeof n.command === 'string' ? n.command : '', why: typeof n.why === 'string' ? n.why : '' })).filter(n => n.command !== '')
  return { header: v.header, off: v.off === true, rows, next }
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

/**
 * The reflexes of `adage graph --json`, keeping only what the pane and the lines draw: never the
 * `arms`, `live` or control counts a route also carries (ux-design §1.7: the arm is never drawn).
 */
export function reflexesOf(graph: Record<string, unknown>): Reflex[] {
  return records(graph.routes).map(r => {
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
