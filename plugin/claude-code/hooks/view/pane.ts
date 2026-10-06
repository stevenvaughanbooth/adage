// The pane's words, as plain values (docs/claude-mod/ux-design.md §6, §4.17): the Live rows and
// their filter, the inspect text, the ticker, the health lights, the Reflexes rows and their verbs,
// what waits for the person, the Health checks. Strength and lifecycle words are allowed here and
// nowhere else (§1.3). Pure: draw/pane.tsx lays these out.
import type { Act, Check, Lapse, Pack, Reflex, Row, Tick } from '../plugin'
import type { Role } from '../tokens'
import { cols, g, plural, reflexOf } from './text'

const pad2 = (n: number) => String(n).padStart(2, '0')

/** A row's time as `adage watch` prints it (`04:22:53`). */
export function clock(ts: number): string {
  const d = new Date(ts)
  return `${pad2(d.getHours())}:${pad2(d.getMinutes())}:${pad2(d.getSeconds())}`
}

const HARNESS: Record<string, string> = { claude: 'cc', 'claude-code': 'cc', opencode: 'oc', codex: 'cx' }
const STRENGTH = (s: string | undefined) => (s ?? '').toUpperCase()

/** One act as the Live tab writes it; strength words are allowed here (ux-design §1.3). */
function actText(a: Act, ascii: boolean): string {
  const r = a.route ?? ''
  const reflex = g('reflex', ascii)
  switch (a.act) {
    case 'directed':
      return `${g('arrow', ascii)} ${reflex} ${r.padEnd(22)} ${STRENGTH(a.strength).padEnd(8)} ${a.detail ?? ''}`.trimEnd()
    case 'withheld':
      return `${g('off', ascii)} ${r} withheld`
    case 'followed':
      return `${g('ok', ascii)} followed ${r}`
    case 'ignored':
      return `${g('off', ascii)} ignored ${r}`
    case 'misfired':
      return `${g('fail', ascii)} misfired ${r}`
    case 'met':
      return `${g('ok', ascii)} finish line met ${r}`
    case 'unaided':
      return `${g('off', ascii)} unaided ${r}`
    case 'not_reached':
      return `${g('off', ascii)} not reached ${r}`
    case 'stop_request':
      return `${g('notice', ascii)} finish line: the agent continues once`
    case 'stop_withheld':
      return `${g('off', ascii)} finish line withheld`
    case 'pretool':
      return a.rewrite === true
        ? `${g('rewrite', ascii)} ${a.what ?? 'run'} ${g('arrow', ascii)} ${a.interpreter ?? ''}`.trimEnd()
        : `${g('notice', ascii)} advised: ${a.what ?? ''}`.trimEnd()
    case 'predicted':
      return `${reflex} chose ${r} ${g('sep', ascii)} no model request`
    case 'would_predict':
      return `${g('off', ascii)} would choose ${r}`
    case 'noticed':
      return `${g('off', ascii)} noticed ${a.candidate ?? ''}`.trimEnd()
    default:
      return `${g('sep', ascii)} ${(a.what ?? a.act).replace(/_/g, ' ')} ${r}`.trimEnd()
  }
}

/** A Live row's lines: the head (time, session, harness, event), then one act per line. */
export function rowLines(row: Row, ascii: boolean): string[] {
  const head = `${clock(row.ts)} ${row.session.slice(0, 6).padEnd(6)} ${(HARNESS[row.harness] ?? row.harness.slice(0, 2)).padEnd(2)} ${row.event.padEnd(7)}`
  if (row.acts.length === 0) return [`${head} ${g('sep', ascii)} ${row.observed > 0 ? plural(row.observed, 'call', 'calls') + ' seen' : 'seen'}`]
  return row.acts.map((a, k) => `${k === 0 ? head : ' '.repeat(cols(head))} ${actText(a, ascii)}`)
}

/** A row with nothing decided (`v: quiet rows` hides these). */
const isQuiet = (row: Row): boolean => row.acts.length === 0

/** The Live rows the filter and the quiet switch leave, newest first. */
export function visibleRows(rows: readonly Row[], filter: string, hideQuiet: boolean, ascii: boolean): Row[] {
  const f = filter.trim().toLowerCase()
  return rows
    .filter(r => !(hideQuiet && isQuiet(r)))
    .filter(r => f === '' || rowLines(r, ascii).join('\n').toLowerCase().includes(f) || r.texts.some(t => t.text.toLowerCase().includes(f)))
    .reverse()
}

/** The inspect pane's Markdown: the directives as the agent read them, and their evidence. */
export function inspectText(row: Row, reflexes: readonly Reflex[]): string {
  const parts = [`**${row.event}** at ${clock(row.ts)} ${'·'} session \`${row.session.slice(0, 12)}\``]
  for (const t of row.texts) {
    const r = reflexOf(reflexes, t.route)
    parts.push(`### ${r?.gist || t.route}\n\n${t.text}`)
    if (r !== undefined && r.describe !== '') parts.push(`_${r.describe}_`)
  }
  if (row.texts.length === 0) parts.push(row.acts.map(a => `- ${actText(a, false)}`).join('\n') || '_Nothing was decided at this event._')
  return parts.join('\n\n').slice(0, 10_000)
}

/** The conditioning ticker (`◆ 04:22 …`). */
export const tickText = (t: Tick, ascii: boolean): string => `${g('active', ascii)} ${clock(t.ts).slice(0, 5)} ${t.text}`

/** The health strip's lamp shapes (`theme.rs`: a colour-blind reader loses nothing). */
export function lightText(lamp: string, label: string, ascii: boolean): { glyph: string; role: Role; text: string } {
  switch (lamp) {
    case 'ok':
      return { glyph: g('ok', ascii), role: 'fire', text: label }
    case 'warn':
      return { glyph: g('lamp', ascii), role: 'prime', text: label }
    case 'fail':
      return { glyph: g('fail', ascii), role: 'inhibit', text: label }
    default:
      return { glyph: g('off', ascii), role: 'ink3', text: label }
  }
}

/** A Reflexes row's line: glyph, id, lamp and strength, lifecycle, trigger → step, support. */
export function reflexLine(r: Reflex, ascii: boolean): string {
  const mark = r.instinct ? g('instinct', ascii) : g('reflex', ascii)
  const lb = r.lowerBound === null ? '' : ` ${g('sep', ascii)} lb ${r.lowerBound.toFixed(2)}`
  const support = r.opportunities > 0 ? `${r.support}/${r.opportunities}${lb}` : ''
  return `${mark} ${r.id.padEnd(24)} ${STRENGTH(r.strength).padEnd(8)} ${r.state.padEnd(8)} ${r.trigger} ${g('arrow', ascii)} ${r.step}  ${support}`.trimEnd()
}

/** The Reflexes rows the filters leave (fuzzy on id, trigger, step, track; strength; lifecycle). */
export function visibleReflexes(reflexes: readonly Reflex[], filter: string, strength: string, lifecycle: string): Reflex[] {
  const words = filter.trim().toLowerCase().split(/\s+/).filter(w => w !== '')
  return reflexes.filter(r => {
    if (strength !== 'all' && r.strength !== strength) return false
    if (lifecycle !== 'all' && r.state !== lifecycle) return false
    const hay = `${r.id} ${r.trigger} ${r.step} ${r.track} ${r.gist}`.toLowerCase()
    return words.every(w => hay.includes(w))
  })
}

/**
 * The review verbs worded as the person's two verbs and the lexicon's four (ux-design §6.2, §10
 * `pane.verb.*`): `enable` and `not this one` on the row, the rest behind `details`.
 */
export const VERB_WORDS: Readonly<Record<string, string>> = {
  approve: 'enable',
  dismiss: 'not this one',
  strengthen: 'make it a direct instruction',
  weaken: 'make it a suggestion',
  extinguish: 'retire it',
  pin: 'pin it',
}

/** The two verbs a row shows; the others wait behind `d: details` (`pane.verb.more`). */
export const ROW_VERBS: readonly string[] = ['approve', 'dismiss']

/** The lowercase hotkey of each verb in a focused pane (never `l`, `r`, `t`, `p`, `v`, `n`'s other uses clash: one row is expanded at a time). */
export const VERB_HOTKEYS: Readonly<Record<string, string>> = { approve: 'e', dismiss: 'n', strengthen: 's', weaken: 'w', extinguish: 'x', pin: 'i' }

/** The terminal command for a review verb, as `review_command` forms it. */
export const reviewCommand = (verb: string, r: Reflex): string => `adage review ${verb} ${r.reference} ${r.id}`

/** One thing that waits for the person: a reflex to enable, a step Adage could take, a pack to trust. */
export type Waiting = {
  key: string
  kind: 'enable' | 'allow' | 'trust'
  /** What the reflex tells the agent (the pack's signer for a trust). */
  say: string
  text: string
  /** The terminal command the first answer runs. */
  command: string
  /** For an enable: the command `not this one` records (`review dismiss`). */
  no?: string
  /** For an allow: an action Adage would run (two acts, the facts between) or a step it would predict (one). */
  offer?: 'execute' | 'predict'
  /** The reflex the item is about (`reference/id`), which `why` expands in the pane. */
  reflex?: string
}

/**
 * What waits for the person (ux-design §4.17, the band's `» n need review`): reflexes held back
 * until a person enables them (`approval`, `quiet`), the grants the binary would offer, and team
 * packs awaiting trust. `ask.ts` words the question about each; the pane lists them under
 * **waiting for you** with a press for each answer.
 */
export function waiting(reflexes: readonly Reflex[], packs: readonly Pack[], ascii = false): Waiting[] {
  const out: Waiting[] = []
  const sep = g('sep', ascii)
  for (const r of reflexes) {
    const key = `${r.reference}/${r.id}`
    const say = r.gist || r.summary
    if (r.reasons.includes('approval') || r.reasons.includes('quiet')) {
      out.push({ key, kind: 'enable', say, text: `${say} ${sep} enable it to tell the agent`, command: reviewCommand('approve', r), no: reviewCommand('dismiss', r), reflex: key })
    }
    for (const o of r.offers) {
      out.push({ key: `${key}#${o.kind}`, kind: 'allow', say, text: `${say} ${sep} Adage can do this for you`, command: o.command, offer: o.kind === 'predict' ? 'predict' : 'execute', reflex: key })
    }
  }
  for (const p of packs) out.push({ key: `pack:${p.name}`, kind: 'trust', say: p.name, text: `reflexes from ${p.name} are in this repository`, command: p.command })
  return out
}

/** A Health check's row: the lamp, the label, the detail. */
export function checkLine(c: Check, ascii: boolean): { glyph: string; role: Role; text: string } {
  const l = lightText(c.status === 'pass' ? 'ok' : c.status === 'warn' ? 'warn' : 'fail', c.label, ascii)
  return { ...l, text: `${c.label}  ${c.detail}` }
}

/** What a Health check offers: the one action a session may run (`adage condition`), or a command to copy. */
export function checkAction(c: Check): { kind: 'run'; argv: string[] } | { kind: 'copy'; command: string } | undefined {
  if (c.action?.kind === 'run' && c.action.command === 'adage condition') return { kind: 'run', argv: ['condition'] }
  if (c.action?.kind === 'run') return { kind: 'copy', command: c.action.command }
  const m = /`([^`]+)`/.exec(c.fix)
  return m?.[1] !== undefined ? { kind: 'copy', command: m[1] } : undefined
}

/** `pane.reflexes.empty`, with the status card's counts. */
export function reflexesEmpty(instincts: number, ascii: boolean): string {
  return `No learned reflexes yet ${g('sep', ascii)} 0 of ~5 sessions needed ${g('sep', ascii)} ${plural(instincts, 'instinct', 'instincts')} active meanwhile`
}

/** A lapsed grant's line in the Health tab. */
export const lapseText = (l: Lapse): string => `${l.route}: ${l.changed.join(', ') || 'changed'}`
