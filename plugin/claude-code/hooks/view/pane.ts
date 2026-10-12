// The pane's words, as plain values (docs/claude-mod/ux-design.md §6, §4.17): the Live rows and
// their filter, the inspect text, the ticker, the health lights, the Reflexes rows and their verbs,
// what waits for the person, the Health checks. Strength and lifecycle words are allowed here and
// nowhere else (§1.3). Pure: draw/pane.tsx lays these out.
import type { Act, Check, Fired, Lapse, Pack, Reflex, Row, Tick } from '../plugin'
import type { Role } from '../tokens'
import { cols, cut, g, plural, reflexOf } from './text'

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

/** A Live row's head: time, session, harness, event. */
const rowHead = (row: Row): string =>
  `${clock(row.ts)} ${row.session.slice(0, 6).padEnd(6)} ${(HARNESS[row.harness] ?? row.harness.slice(0, 2)).padEnd(2)} ${row.event.padEnd(7)}`

/** A Live row's lines: the head (time, session, harness, event), then one act per line. */
export function rowLines(row: Row, ascii: boolean): string[] {
  const head = rowHead(row)
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

/** The ticker's most recent lines the Live tab draws; the rest are in `adage watch` and the map. */
export const TICKS_DRAWN = 5

/**
 * The conditioning ticker (`◆ 04:22 …`): a reflex the tick names by id is named by what it tells
 * the agent (ux-design §1.10), and the index generation's hash (`, gen 21aa617d`) is left out.
 */
export function tickText(t: Tick, ascii: boolean, reflexes: readonly Reflex[] = []): string {
  let text = t.text.replace(/, gen [0-9a-f]{6,}\b/, '')
  for (const r of reflexes) {
    const say = r.gist || r.summary
    // Ids are `[A-Za-z0-9_-]` (the binary's own); anything else is left as the tick said it.
    if (say !== '' && /^[A-Za-z0-9_-]+$/.test(r.id)) text = text.replace(new RegExp(`(^|[\\s:])${r.id}(?=\\s|$)`), `$1${say}`)
  }
  return `${g('active', ascii)} ${clock(t.ts).slice(0, 5)} ${text}`
}

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

/**
 * A Reflexes row's line: glyph, strength, what it tells the agent (ux-design §1.10: by its words,
 * never its id; the step first, `listLabel`), then lifecycle and support, which a narrow pane cuts
 * first. The id, the trigger
 * and the step are in the expanded row (`reflexFacts`).
 */
export function reflexLine(r: Reflex, ascii: boolean): string {
  const mark = r.instinct ? g('instinct', ascii) : g('reflex', ascii)
  const sep = g('sep', ascii)
  const support = r.opportunities > 0 ? ` ${sep} ${r.support}/${r.opportunities} sessions` : ''
  return `${mark} ${STRENGTH(r.strength).padEnd(7)} ${listLabel(sayOf(r), sep)}  ${r.state}${support}`.trimEnd()
}

/** What a reflex tells the agent, in its own words: the gist, else the summary, else the id. */
const sayOf = (r: Reflex): string => r.gist || r.summary || r.id

/**
 * A reflex's sentence as a list row leads with it (`adage ui`'s `words::list_label`): the step,
 * then when (`after editing src/**, run make test` → `make test · after editing src/**`), so rows
 * that share a trigger still differ where a narrow pane cuts. Any other sentence is itself.
 */
export function listLabel(say: string, sep: string): string {
  const at = say.lastIndexOf(', run ')
  if (at <= 0 || at + 6 >= say.length) return say
  return `${say.slice(at + 6)} ${sep} ${say.slice(0, at)}`
}

/** The expanded row's facts line: the id a terminal command needs, the trigger, the step, the bound. */
export function reflexFacts(r: Reflex, ascii: boolean): string {
  const lb = r.lowerBound === null ? '' : ` ${g('sep', ascii)} lb ${r.lowerBound.toFixed(2)}`
  return `${r.id} ${g('sep', ascii)} when ${r.trigger} ${g('arrow', ascii)} ${r.step}${lb}`
}

/**
 * A track's heading in plain words: the repository's own learned reflexes, Adage's built-in
 * instincts, or the track's own name (with the signer for an imported one). The track key
 * (`github-com-acme-billing/learned@15.6.0`) is the binary's, for terminal commands.
 */
export function trackTitle(r: Reflex): string {
  const name = r.reference.split('/').pop() ?? r.reference
  if (r.imported) return r.signer === '' ? `shared: ${name}` : `shared: ${name}  ${r.signer}`
  if (name === 'learned') return 'learned in this repository'
  if (name === 'instincts') return "instincts: Adage's built-in defaults, until it learns better here"
  return name
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

/**
 * Which suggestion an item is, in the order the pane lists them and the asks take them: a team's
 * pack, a reflex held back until a person enables it, a step Adage could run itself, a step it
 * could take without a model request, and a reflex kept quiet because agents already do it.
 */
export type Group = 'trust' | 'review' | 'execute' | 'predict' | 'quiet'
export const GROUPS: readonly Group[] = ['trust', 'review', 'execute', 'predict', 'quiet']

/** Each group's heading in the pane (`pane.waiting.{group}`): what saying yes would change. */
export const GROUP_TITLES: Readonly<Record<Group, string>> = {
  trust: 'Reflexes a teammate shared, not used until you say so',
  review: 'Learned here, not told to the agent until you enable them',
  execute: 'Adage can run these itself, instead of asking the agent to',
  predict: 'Adage can take these steps without a model request',
  quiet: 'Agents here already do these unaided, so Adage stays quiet about them',
}

/** `pane.waiting`: the group's rule. Nothing waits on a suggestion; the session goes on either way. */
export const WAITING_TITLE = 'suggestions · optional; nothing waits on them'

/** One thing that waits for the person: a reflex to enable, a step Adage could take, a pack to trust. */
export type Waiting = {
  key: string
  kind: 'enable' | 'allow' | 'trust'
  group: Group
  /** What the reflex tells the agent (the pack's signer for a trust). */
  say: string
  /** The row's sentence, whole: the group's heading says what the answer changes. */
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
 * What waits for the person (ux-design §4.17, the band's `» Adage has n suggestions`): team packs
 * awaiting trust, reflexes held back until a person enables them (`approval`, `quiet`), and the
 * grants the binary would offer, in `GROUPS` order. `ask.ts` words the question about each; the
 * pane lists them under **suggestions**, one heading per group, with a press for each answer.
 */
export function waiting(reflexes: readonly Reflex[], packs: readonly Pack[]): Waiting[] {
  const out: Waiting[] = []
  for (const p of packs) out.push({ key: `pack:${p.name}`, kind: 'trust', group: 'trust', say: p.name, text: `reflexes from ${p.name} are in this repository`, command: p.command })
  for (const r of reflexes) {
    const key = `${r.reference}/${r.id}`
    const say = r.gist || r.summary
    const quiet = r.reasons.includes('quiet')
    if (quiet || r.reasons.includes('approval')) {
      out.push({ key, kind: 'enable', group: quiet ? 'quiet' : 'review', say, text: say, command: reviewCommand('approve', r), no: reviewCommand('dismiss', r), reflex: key })
    }
    for (const o of r.offers) {
      const offer = o.kind === 'predict' ? 'predict' : 'execute'
      out.push({ key: `${key}#${o.kind}`, kind: 'allow', group: offer, say, text: say, command: o.command, offer, reflex: key })
    }
  }
  return GROUPS.flatMap(gr => out.filter(w => w.group === gr))
}

/**
 * The suggestions the band counts and the turn-end ask may raise: every one but a quiet reflex,
 * which agents already follow unaided and so is worth a look in the pane, never an interruption.
 */
export const worthAsking = (items: readonly Waiting[]): Waiting[] => items.filter(w => w.group !== 'quiet')

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

/** The terminal command that updates Adage (Health's `Copy:` on the version row). */
export const UPDATE_COMMAND = 'adage update'

/** Health's `version` row (onboarding plan O9): the pair, as the status card says it. */
export function updateRowText(u: { have: string; latest: string }, ascii: boolean): string {
  return `version  adage ${u.have} ${g('sep', ascii)} ${u.latest} is out`
}

/** `pane.reflexes.empty`, with the status card's counts. */
export function reflexesEmpty(instincts: number, ascii: boolean): string {
  return `No learned reflexes yet ${g('sep', ascii)} 0 of ~5 sessions needed ${g('sep', ascii)} ${plural(instincts, 'instinct', 'instincts')} active meanwhile`
}

/** A lapsed grant's line in the Health tab. */
export const lapseText = (l: Lapse): string => `${l.route}: ${l.changed.join(', ') || 'changed'}`

// ------------------------------------------------------------------ the flow (ux-design §6.2)
//
// Each Reflexes row draws what the map (`adage ui`'s Map tab, `ui/tabs/map.rs`) draws: the
// stimulus, the reflex, the response, joined by arrows. Three rounded boxes when the pane is wide
// enough, one line when it is not, ASCII twins under `ADAGE_ASCII=1`. A reflex that fired in this
// session glows in the fire role with the time, and its response too once the agent followed it.

/** The flow's box-drawing glyphs and their ASCII twins (the map's; one column each). */
const FLOW = { tl: ['╭', '+'], tr: ['╮', '+'], bl: ['╰', '+'], br: ['╯', '+'], h: ['─', '-'], v: ['│', '|'], head: ['▶', '>'], hook: ['╰', '`'] } as const
const fl = (name: keyof typeof FLOW, ascii: boolean): string => FLOW[name][ascii ? 1 : 0]

/** The arrow between two boxes, and between the parts of the one-line flow. */
export const flowArrow = (ascii: boolean, long = false): string => `${fl('h', ascii).repeat(long ? 3 : 2)}${fl('head', ascii)}`

/** The lamp role of a strength (`theme.rs`): fire green, prime amber, inhibit red. */
export const strengthRole = (strength: string): Role => (strength === 'fire' ? 'fire' : strength === 'inhibit' ? 'inhibit' : 'prime')

/** A flow's stimulus: its box title, its box text and its phrase on the one-line flow. */
export type Stimulus = { title: string; text: string; phrase: string }

const unquote = (s: string): string => s.replace(/`/g, '')

/**
 * The stimulus of a reflex as the map titles it (`map.rs` `stimulus`): `AFTER EDIT` /
 * `crates/**\/*.rs changed`, and the phrase the narrow flow leads with (`after editing crates/**\/*.rs`).
 * A sequence's (`context`) label only counts its events, so its words come from the gist.
 */
export function stimulusOf(r: Reflex): Stimulus {
  const text = unquote(r.trigger)
  const kind = r.triggerKind || (text.startsWith('an edit touches ') ? 'after_edit' : text.startsWith('a prompt means: ') ? 'intent' : text.startsWith('a prompt ') ? 'prompt' : '')
  const strip = (prefix: string) => (text.startsWith(prefix) ? text.slice(prefix.length) : text)
  switch (kind) {
    case 'after_edit': {
      const p = strip('an edit touches ')
      return { title: 'AFTER EDIT', text: `${p} changed`, phrase: `after editing ${p}` }
    }
    case 'prompt': {
      const p = strip('a prompt ')
      return { title: 'PROMPT', text: p, phrase: `when a prompt ${p}` }
    }
    case 'intent': {
      const p = strip('a prompt means: ')
      return { title: 'INTENT', text: p, phrase: `when a prompt means ${p}` }
    }
    case 'after_command':
      return { title: 'AFTER COMMAND', text, phrase: `after ${text}` }
    case 'after_failure':
      return { title: 'AFTER FAILURE', text, phrase: `after ${text}` }
    case 'session_start':
      return { title: 'SESSION START', text, phrase: 'at session start' }
    case 'context': {
      const say = r.gist || r.summary
      const at = say.lastIndexOf(', run ')
      const p = at > 0 ? say.slice(0, at) : text
      return { title: 'SEQUENCE', text: p.replace(/^after /, ''), phrase: p }
    }
    default:
      return { title: 'STIMULUS', text, phrase: text }
  }
}

/** A flow's response: its box title, its text, and whether the text is a command (drawn as one). */
export type Response = { title: string; text: string; command: boolean }

/** The response of a reflex as the map titles it (`map.rs` `response`): `RUN` / `cargo fmt --all`. */
export function responseOf(r: Reflex): Response {
  const text = unquote(r.step).replace(/^In this repository, /, '')
  switch (r.stepKind) {
    case 'run':
      return { title: 'RUN', text, command: true }
    case 'read_batch':
      return { title: 'READ TOGETHER', text, command: false }
    case 'avoid':
      return { title: 'RUN INSTEAD', text, command: true }
    case 'advise':
      return { title: 'ADVICE', text, command: false }
    default:
      return { title: r.stepKind === '' && r.step !== '' ? 'RUN' : 'RESPONSE', text, command: r.stepKind === '' }
  }
}

/** What this session saw a reflex do: when it last fired (null: fired, no feed row says when), whether the agent then followed it. */
export type Pulse = { at: number | null; followed: boolean; latest: boolean }

/**
 * The reflexes that fired in this session, by id: the command hooks' blocks (`$.state.fired`) and
 * this session's feed rows, which carry the time (`directed`, and `predicted` for a step taken
 * without a model request), and whether a `followed` or `met` row came after. `latest` marks the
 * one that fired last, which the pane draws brightest.
 */
export function pulses(fired: readonly Fired[], rows: readonly Row[]): Map<string, Pulse> {
  const out = new Map<string, Pulse>()
  for (const x of fired) if (x.route !== '' && !out.has(x.route)) out.set(x.route, { at: null, followed: false, latest: false })
  for (const row of rows) {
    if (!row.mine) continue
    for (const a of row.acts) {
      if (a.route === undefined || a.route === '') continue
      if (a.act === 'directed' || a.act === 'predicted') {
        out.set(a.route, { at: row.ts, followed: false, latest: false })
      } else if (a.act === 'followed' || a.act === 'met') {
        const p = out.get(a.route)
        if (p !== undefined) p.followed = true
      }
    }
  }
  let best: string | undefined
  let bestAt = -Infinity
  for (const [route, p] of out) if (p.at !== null && p.at >= bestAt) [best, bestAt] = [route, p.at]
  if (best !== undefined) {
    const p = out.get(best)
    if (p !== undefined) p.latest = true
  }
  return out
}

/** The Reflexes tab's live line: how many fired in this session, and the last one's time. */
export function pulseLine(ps: ReadonlyMap<string, Pulse>, ascii: boolean): string {
  if (ps.size === 0) return ''
  const sep = g('sep', ascii)
  const last = [...ps.values()].find(p => p.latest)
  const when = last?.at === undefined || last.at === null ? '' : ` ${sep} last at ${clock(last.at)}`
  return `${g('active', ascii)} ${plural(ps.size, 'reflex', 'reflexes')} fired in this session${when}`
}

/**
 * One piece of a drawn flow line: its text, its role (none: the terminal's own ink), dim or bold,
 * and `press` on the one piece that is the row's Button (it expands the row).
 */
export type Seg = { text: string; role?: Role; dim?: boolean; bold?: boolean; press?: true }

/** The arrow's width between two boxes (` ───▶ `), and the least box width the boxes take. */
const GAP = 6
const MIN_BOX = 20
const MAX_BOX = 36
/** The flow's indent under its track heading. */
export const FLOW_INDENT = 2

/** The box width at a pane width, or 0 when three boxes do not fit (the one-line flow then). */
export function boxWidth(columns: number): number {
  const w = Math.floor((columns - FLOW_INDENT - 2 * GAP) / 3)
  return w < MIN_BOX ? 0 : Math.min(MAX_BOX, w)
}

type Boxed = { top: Seg[]; mid: Seg[]; bottom: Seg[] }

/**
 * One rounded box, `w` columns wide: the title set in the top border (`╭─ RUN ────╮`), the body
 * on the one line inside, a caption set in the bottom border (`╰─ ◆ fired 14:02:11 ─╯`).
 */
function box(w: number, title: Seg[], body: Seg, caption: Seg[], border: Role, ascii: boolean, hot: boolean): Boxed {
  const edge = (text: string): Seg => (hot ? { text, role: border, bold: true } : { text, role: border })
  const h = fl('h', ascii)
  const set = (left: string, right: string, inner: Seg[]): Seg[] => {
    const room = w - 5
    let used = 0
    const kept: Seg[] = []
    for (const s of inner) {
      const t = cut(s.text, Math.max(0, room - used), ascii)
      if (t === '') break
      kept.push({ ...s, text: t })
      used += cols(t)
    }
    if (kept.length === 0) return [edge(`${left}${h.repeat(w - 2)}${right}`)]
    return [edge(`${left}${h} `), ...kept, edge(` ${h.repeat(Math.max(0, room - used))}${right}`)]
  }
  const inner = w - 4
  const text = cut(body.text, inner, ascii)
  return {
    top: set(fl('tl', ascii), fl('tr', ascii), title),
    // The padding is its own piece, so the body (the row's Button, for the reflex box) is its words alone.
    mid: [edge(`${fl('v', ascii)} `), { ...body, text }, { text: ' '.repeat(Math.max(0, inner - cols(text))) }, edge(` ${fl('v', ascii)}`)],
    bottom: set(fl('bl', ascii), fl('br', ascii), caption),
  }
}

/** What the reflex box says inside: its glyph, its lifecycle word, its support and bound, as many as fit. */
function reflexBody(r: Reflex, inner: number, ascii: boolean): string {
  const sep = ` ${g('sep', ascii)} `
  const mark = r.instinct ? g('instinct', ascii) : g('reflex', ascii)
  const parts = [`${mark} ${r.instinct ? 'instinct' : r.state}`]
  if (r.opportunities > 0) parts.push(`${r.support}/${r.opportunities}`)
  if (r.lowerBound !== null) parts.push(`lb ${r.lowerBound.toFixed(2)}`)
  let p = parts
  while (p.length > 1 && cols(p.join(sep)) > inner) p = p.slice(0, -1)
  return p.join(sep)
}

/**
 * A reflex as three boxes joined by arrows, three lines (ux-design §6.2): stimulus → reflex →
 * response, the map's picture in the pane. The reflex box's body is the row's Button (`press`).
 * The borders are dim; the expanded row's are the accent; a reflex that fired in this session
 * lights its stimulus, its first arrow and its own box in the fire role, captioned with the time,
 * and, once the agent followed it, the second arrow and the response too. Undefined when the pane
 * is too narrow for three boxes (`flowLine` then).
 */
export function flowBoxes(r: Reflex, pulse: Pulse | undefined, columns: number, ascii: boolean, open = false): Seg[][] | undefined {
  const w = boxWidth(columns)
  if (w === 0) return undefined
  const st = stimulusOf(r)
  const rs = responseOf(r)
  const fired = pulse !== undefined
  const followed = pulse?.followed === true
  const rest: Role = open ? 'accent' : 'ink3'
  const lit = (on: boolean): Role => (on ? 'fire' : rest)
  const title = (t: string): Seg => ({ text: t, role: 'ink3', bold: true })
  const stamp = pulse?.at === undefined || pulse.at === null ? 'fired' : `fired ${clock(pulse.at)}`
  const hot = pulse?.latest === true
  const a = box(w, [title(st.title)], { text: st.text }, [], lit(fired), ascii, false)
  const b = box(
    w,
    [{ text: g('lamp', ascii), role: strengthRole(r.strength) }, { text: ' ' }, { text: STRENGTH(r.strength), role: strengthRole(r.strength), bold: true }],
    { text: reflexBody(r, w - 4, ascii), press: true },
    fired ? [{ text: `${g('active', ascii)} ${stamp}`, role: 'fire', bold: hot }] : [],
    lit(fired),
    ascii,
    hot,
  )
  const c = box(w, [title(rs.title)], rs.command ? { text: rs.text, role: 'command' } : { text: rs.text }, followed ? [{ text: `${g('ok', ascii)} followed`, role: 'fire' }] : [], lit(followed), ascii, false)
  const pad: Seg = { text: ' '.repeat(FLOW_INDENT) }
  const space: Seg = { text: ' '.repeat(GAP) }
  const arrow = (on: boolean): Seg => ({ text: ` ${flowArrow(ascii, true)} `, role: on ? 'fire' : rest, bold: on })
  return [
    [pad, ...a.top, space, ...b.top, space, ...c.top],
    [pad, ...a.mid, arrow(fired), ...b.mid, arrow(followed), ...c.mid],
    [pad, ...a.bottom, space, ...b.bottom, space, ...c.bottom],
  ]
}

/**
 * A reflex as one line, for a narrow pane (ux-design §6.2): the lamp, the time it fired in this
 * session, then `after editing crates/**\/*.rs ──▶ ◠ ──▶ cargo fmt --all` as the row's Button, then
 * the strength and lifecycle words while they fit. The stimulus is cut first, the response last.
 */
export function flowLine(r: Reflex, pulse: Pulse | undefined, columns: number, ascii: boolean): Seg[] {
  const role = strengthRole(r.strength)
  const out: Seg[] = [{ text: `${g('lamp', ascii)} `, role }]
  if (pulse !== undefined) out.push({ text: `${g('active', ascii)} ${pulse.at === null ? 'fired' : clock(pulse.at).slice(0, 5)} `, role: 'fire', bold: pulse.latest })
  const used = out.reduce((n, s) => n + cols(s.text), 0)
  const room = Math.max(10, columns - used)
  const mark = r.instinct ? g('instinct', ascii) : g('reflex', ascii)
  const mid = ` ${flowArrow(ascii)} ${mark} ${flowArrow(ascii)} `
  const st = stimulusOf(r).phrase
  const rs = responseOf(r).text
  const stRoom = Math.max(8, room - cols(mid) - cols(rs))
  const flow = cut(`${cut(st, stRoom, ascii)}${mid}${rs}`, room, ascii)
  out.push({ text: flow, press: true })
  const tail = `  ${STRENGTH(r.strength)} ${g('sep', ascii)} ${r.instinct ? 'instinct' : r.state}`
  if (cols(flow) + cols(tail) <= room) out.push({ text: tail, role: 'ink3', dim: true })
  return out
}

/** The least room the small flow takes beside a Live row's head; narrower, it is not drawn. */
const MIN_SMALL = 40

/**
 * Two ends sharing `room` columns: both whole when they fit; else the shorter whole when it takes
 * at most half, the longer cut to the rest; else each cut to half.
 */
function share(a: string, b: string, room: number, ascii: boolean): [string, string] {
  const [la, lb] = [cols(a), cols(b)]
  if (la + lb <= room) return [a, b]
  const half = Math.floor(room / 2)
  if (lb <= half) return [cut(a, room - lb, ascii), b]
  if (la <= half) return [a, cut(b, room - la, ascii)]
  return [cut(a, half, ascii), cut(b, room - half, ascii)]
}

/**
 * The flow in small under each directive of a Live row (ux-design §6.1), one per act, undefined
 * for an act that is not a directive or names no reflex the graph holds: under the act, `╰ after
 * editing schema/*.json ──▶ ◠ ──▶ make gen`, the arrows and the reflex in its lamp's role, the
 * response as the directive named it (`detail`), else the step. The stimulus is cut first.
 */
export function actFlows(row: Row, reflexes: readonly Reflex[], columns: number, ascii: boolean): (Seg[] | undefined)[] {
  const indent = ' '.repeat(cols(rowHead(row)) + 1)
  return row.acts.map(a => {
    if (a.act !== 'directed' || a.route === undefined) return undefined
    const r = reflexOf(reflexes, a.route)
    if (r === undefined) return undefined
    const role = strengthRole(a.strength ?? r.strength)
    const lead = `${indent}${fl('hook', ascii)} `
    const arrow = ` ${flowArrow(ascii)} `
    const mark = r.instinct ? g('instinct', ascii) : g('reflex', ascii)
    const room = columns - cols(lead)
    // Too narrow to say both ends: the act line above says enough.
    if (room < MIN_SMALL) return undefined
    const [st, rs] = share(stimulusOf(r).phrase, a.detail || responseOf(r).text, room - 2 * cols(arrow) - cols(mark), ascii)
    return [
      { text: lead, role: 'ink3' },
      { text: st, role: 'ink3' },
      { text: arrow, role },
      { text: mark, role, bold: true },
      { text: arrow, role },
      { text: rs, role: 'command' },
    ]
  })
}
