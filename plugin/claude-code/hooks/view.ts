// What the mod draws, as plain values (docs/claude-mod/ux-design.md §4–§10): the copy deck's
// templates, the band's cells and their narrow-terminal priority, the lines under a prompt and a
// tool row, the pane's rows, `/adage why` and the status card's text. Every function here is pure:
// the trees in draw/*.tsx lay these values out, and register.ts reads the state they take.
//
// Words follow docs/lexicon.md: no strength or lifecycle word outside the pane, no "saved", no
// "approval", a reflex named by what it tells the agent (its gist), never by its id.
import type { Skill } from './kinds'
import { late } from './late'
import { glyph } from './tokens'
import type { GlyphName, Role } from './tokens'
import type { Act, Check, Fired, Health, Lapse, Learner, Mark, Pack, Reflex, Row, Tick } from './plugin'

/** A glyph, or its ASCII twin under `ADAGE_ASCII=1` (ux-design §1.9). */
export const g = (name: GlyphName, ascii: boolean): string => glyph(name, ascii)

/** Display columns of a string: every glyph the mod draws is one column (ux-design §1.9). */
export const cols = (s: string): number => Array.from(s).length

/** Cuts a string to `n` columns with `…` (or `...` in ASCII) when it is longer. */
export function cut(s: string, n: number, ascii = false): string {
  const chars = Array.from(s)
  if (chars.length <= n) return s
  const dots = ascii ? '...' : '…'
  if (n <= dots.length) return chars.slice(0, Math.max(0, n)).join('')
  return chars.slice(0, n - dots.length).join('') + dots
}

/** The binary's notice text without its `adage: ` prefix (ux-design §10: one helper). */
export function stripPrefix(text: string): string | undefined {
  const m = /^\s*(?:adage|adage-exec):\s+/.exec(text)
  return m === null ? undefined : text.slice(m[0].length)
}

/** `open.notice`: the binary's opening line with the glyph; undefined for a notice not Adage's. */
export function openNotice(text: string, ascii: boolean): string | undefined {
  const rest = stripPrefix(text)
  return rest === undefined || rest.trim() === '' ? undefined : `${g('reflex', ascii)} adage ${g('sep', ascii)} ${rest}`
}

const plural = (n: number, one: string, many: string): string => `${n} ${n === 1 ? one : many}`

/** The reflex a route id names (in this track when the tag says which), from `graph --json`. */
export function reflexOf(reflexes: readonly Reflex[], route: string, track = ''): Reflex | undefined {
  const same = reflexes.filter(r => r.id === route)
  if (track !== '') {
    const name = track.replace(/@[^@]*$/, '')
    const inTrack = same.find(r => r.track === track || r.track === name || r.reference === track)
    if (inTrack !== undefined) return inTrack
  }
  return same[0]
}

/**
 * What a reflex tells the agent (ux-design §1.10, `{summary}`): its gist, else its summary. Before
 * the graph has been read the route id stands in, once, rather than a blank line.
 */
export function sayOf(reflexes: readonly Reflex[], route: string, track = ''): string {
  const r = reflexOf(reflexes, route, track)
  return r?.gist || r?.summary || route.replace(/-/g, ' ')
}

/**
 * Fits parts joined by ` · ` into `columns`: the parts are dropped from the right while the line
 * is too wide (the first part stays), then the last part left is cut with `…`.
 */
export function fit(parts: readonly string[], columns: number, ascii: boolean, keep = 1): string {
  const sep = ` ${g('sep', ascii)} `
  let p = parts.filter(x => x !== '')
  while (p.length > keep && cols(p.join(sep)) > columns) p = p.slice(0, -1)
  return cut(p.join(sep), columns, ascii)
}

/** The receipt block a hook delivered beside a call (`<adage-receipt operation="…">`): its short id. */
export function receiptOf(texts: readonly (string | undefined)[]): string | undefined {
  for (const t of texts) {
    const m = t === undefined ? null : /<adage-receipt\b[^>]*\boperation="([0-9a-f]+)"/.exec(t)
    if (m?.[1] !== undefined) return m[1].slice(0, 4)
  }
  return undefined
}

/** `result.receipt.beside`: one dim line under a run that happened beside a receipt. */
export const receiptBeside = (receipt: string, ascii: boolean): string =>
  `${g('reflex', ascii)} adage receipt ${receipt} covers this run ${g('sep', ascii)} /adage why`

/** The most bytes of test names the receipt card carries (ux-design §12: cut as the receipts are). */
export const RECEIPT_BYTES = 2400

/** `result.receipt`: the two lines that stand in for a test run's output when the receipt was its result. */
export function receiptCard(m: Extract<Mark, { kind: 'answered' }>, ascii: boolean): [string, string] {
  let names = ''
  for (const t of m.tests) {
    const next = names === '' ? t : `${names}, ${t}`
    if (new TextEncoder().encode(next).length > RECEIPT_BYTES) break
    names = next
  }
  const sep = ` ${g('sep', ascii)} `
  const first = `${g('ok', ascii)} adage receipt ${m.receipt}${sep}${m.summary}${names === '' ? '' : `${sep}the tests added in this session: ${names}`}`
  return [first, `pytest's own output is not shown; adage answered from its run ${m.ago} on identical inputs (${m.command})`]
}

/** `toast.lapsed`: once per session per lapse (ux-design §4.5); the command stays in the Health tab. */
export const lapseToast = (l: Lapse, ascii: boolean): string =>
  `adage\n${l.route} is no longer allowed: ${l.changed[0] ?? 'it'} changed ${g('sep', ascii)} /adage review`

/** The most lines under one prompt: the runtime's decision cap (`params.rs`). */
export const MAX_PROMPT_LINES = 3

/** The latest detail this session's feed gave for a route (the command or first sentence it named). */
function detailOf(rows: readonly Row[], route: string): string {
  for (let i = rows.length - 1; i >= 0; i--) {
    const row = rows[i]
    if (row === undefined || !row.mine) continue
    const a = row.acts.find(x => x.act === 'directed' && x.route === route && (x.detail ?? '') !== '')
    if (a !== undefined) return a.detail ?? ''
  }
  return ''
}

/**
 * `prompt.fired`: one line per reflex fired at a prompt, in delivery order, at most three: the
 * glyph and what it tells the agent, then the command or files it names; at a narrow width the
 * command goes first (T-B2-03). No strength word.
 */
export function promptLines(fired: readonly Fired[], reflexes: readonly Reflex[], rows: readonly Row[], columns: number, ascii: boolean): string[] {
  const seen = new Set<string>()
  const lines: string[] = []
  for (const f of fired) {
    if (f.route === '' || seen.has(f.route)) continue
    seen.add(f.route)
    const say = sayOf(reflexes, f.route, f.track)
    const detail = detailOf(rows, f.route)
    const extra = detail !== '' && !say.includes(detail) ? detail : ''
    lines.push(fit([`${g('reflex', ascii)} ${say}`, extra], columns, ascii))
    if (lines.length === MAX_PROMPT_LINES) break
  }
  return lines
}

/** The marks drawn under a tool row (the receipt marks are drawn under its result instead). */
export type RowMark = Extract<Mark, { kind: 'rewrite' | 'substituted' | 'avoided' }> | { kind: 'predicted' }

/** The marker under a tool row (`tool.rewrite`, `tool.predicted`, `tool.avoided`, `tool.substituted`). */
export function markText(mark: RowMark, ascii: boolean): string {
  switch (mark.kind) {
    case 'rewrite':
      return `${g('rewrite', ascii)} adage: pointed this ${mark.what === 'test' ? 'test run' : 'Python run'} at ${mark.interpreter}`
    case 'predicted':
      return `${g('reflex', ascii)} adage chose this step ${g('sep', ascii)} no model request`
    case 'avoided':
      return `${g('reflex', ascii)} adage: dead end headed off`
    case 'substituted':
      return `${g('rewrite', ascii)} adage ran ${mark.command} instead`
  }
}

/**
 * The rewrite the command hook made to a Bash call, read off its `PreToolUse` answer: the
 * interpreter the new command runs, and whether it is a test run. Undefined when nothing changed.
 */
export function rewriteOf(before: unknown, after: unknown): { interpreter: string; what: 'test' | 'python' } | undefined {
  if (typeof before !== 'string' || typeof after !== 'string' || after.trim() === '' || before === after) return undefined
  const interpreter = after.trim().split(/\s+/)[0] ?? ''
  if (interpreter === '') return undefined
  return { interpreter, what: /\bpytest\b|\bunittest\b/.test(after) ? 'test' : 'python' }
}

// ------------------------------------------------------------------------------------- the band

/** A band cell: its text, and the role of its one coloured glyph (the lamp), if any. */
export type Cell = { text: string; lamp?: Role; glyph?: string }

export type BandInput = {
  active: boolean
  /** The shim's reason when no binary could serve. */
  nobinary: string
  paused: boolean
  health: Health | null
  /** Inside a repository (the feed sent a learner line). */
  inRepo: boolean
  reflexes: number
  steps: number
  reused: number
  learner: Learner | null
  now: number
  lapsed: number
  review: number
  versionLine: string
  columns: number
  ascii: boolean
  /** Answers kept for the next session start (`band.kept`); the hooks' own count when absent. */
  kept?: number
  /** Distinct sequences noticed this session (`1 pattern noticed`); the hooks' own count when absent. */
  noticed?: number
  /** Whether the pane is open: the noticed cell shows only while it is closed. */
  paneOpen?: boolean
}

export type Band = { cells: Cell[]; second: string }

/** The lamp cell (ux-design §5): the word always beside the lamp, in the fire, prime or inhibit role. */
export function lampCell(h: Health | null, ascii: boolean): Cell {
  const word = h?.word ?? 'healthy'
  const role: Role = word === 'problem' ? 'inhibit' : word === 'check' ? 'prime' : 'fire'
  const text = word === 'healthy' || h?.check == null ? word : `${word}: ${h.check}`
  return { glyph: g('lamp', ascii), lamp: role, text }
}

/** The learner's cell: `learning in 38 s`, `learning…`, `learned` for 10 s after a run, else nothing. */
export function learnerCell(l: Learner | null, now: number, ascii: boolean): string {
  if (l === null) return ''
  if (l.state === 'waiting' && l.nextAt !== null && l.nextAt > now) return `learning in ${Math.ceil((l.nextAt - now) / 1000)} s`
  if (l.state === 'running' || l.state === 'waiting') return ascii ? 'learning...' : 'learning…'
  if (l.ranAt !== null && now - l.ranAt < 10_000) return 'learned'
  return ''
}

/**
 * The band (ux-design §5), or undefined to leave the band to the engine and other mods. Cells by
 * priority: the name (1), the lamp (2), reflexes (3), steps (4), test runs (5), learning (6);
 * at a narrow width the lowest goes first and the lamp's word never does. The second line shows
 * one attention item, highest first: a lapsed grant, a problem, review waiting, the version.
 */
export function band(i: BandInput): Band | undefined {
  const name = `${g('reflex', i.ascii)} adage`
  if (i.nobinary !== '') return { cells: [{ text: `${name} ${g('off', i.ascii)} off ${g('sep', i.ascii)} run adage doctor` }], second: '' }
  if (!i.active) return undefined
  if (i.paused) return { cells: [{ text: `${name}  paused ${g('sep', i.ascii)} /adage pause` }], second: '' }
  if (i.health?.off === true) return { cells: [{ text: `${name} ${g('off', i.ascii)} off in this repository ${g('sep', i.ascii)} /adage on` }], second: '' }
  if (!i.inRepo) return undefined

  const second = secondLine(i)
  const counts: { cell: string; rank: number }[] = []
  if (i.reflexes > 0) counts.push({ cell: `${i.reflexes} ${i.reflexes === 1 ? 'reflex' : 'reflexes'} this session`, rank: 3 })
  if (i.steps > 0) counts.push({ cell: `${plural(i.steps, 'step', 'steps')} without a model request`, rank: 4 })
  if (i.reused > 0) counts.push({ cell: `${plural(i.reused, 'test run', 'test runs')} reused`, rank: 5 })
  const learning = learnerCell(i.learner, i.now, i.ascii)
  const lamp = lampCell(i.health, i.ascii)
  // `1 pattern noticed` (ux-design §5, rank 7): while the pane, which lists them, is closed.
  const noticed = i.noticed ?? late.noticed
  const noticedCell = noticed > 0 && !(i.paneOpen ?? late.paneOpen) ? `${plural(noticed, 'pattern', 'patterns')} noticed` : ''
  if (counts.length === 0 && learning === '' && noticedCell === '' && lamp.lamp === 'fire' && second === '') return undefined

  // Laid out in reading order, dropped by rank: noticed, then learning, then the counts from the right.
  type Ranked = Cell & { rank: number }
  let cells: Ranked[] = [
    ...counts.map(c => ({ text: c.cell, rank: c.rank })),
    { ...lamp, rank: 2 },
    ...(learning === '' ? [] : [{ text: learning, rank: 6 }]),
    ...(noticedCell === '' ? [] : [{ text: noticedCell, rank: 7 }]),
  ]
  const width = (cs: readonly Ranked[]) => cols(`${name}  `) + cs.reduce((n, c, k) => n + (k > 0 ? 3 : 0) + cols(c.glyph ? `${c.glyph} ${c.text}` : c.text), 0)
  while (width(cells) > i.columns && cells.length > 1) {
    const worst = cells.reduce((w, c) => (c.rank > w.rank ? c : w))
    if (worst.rank === 2) break
    cells = cells.filter(c => c !== worst)
  }
  if (width(cells) > i.columns) {
    // The narrowest band: `◠ adage ● healthy` (ux-design §4.11); the lamp's word is never cut.
    return { cells: [{ text: name }, { ...lamp }], second: second !== '' && cols(second) <= i.columns ? second : '' }
  }
  return { cells: [{ text: name }, ...cells.map(({ rank: _rank, ...c }) => c)], second: second !== '' && cols(second) <= i.columns ? second : '' }
}

function secondLine(i: BandInput): string {
  const n = g('notice', i.ascii)
  const sep = g('sep', i.ascii)
  if (i.lapsed > 0) return `${n} ${plural(i.lapsed, 'grant', 'grants')} lapsed ${sep} /adage health`
  if (i.health?.word === 'problem' && i.health.check !== null) return `${n} problem: ${i.health.check} ${sep} /adage health`
  // `band.kept` (ux-design §9.12): an answer given with no press held waits for the next start;
  // it is said before `need review`, since the review it answers still waits in the graph until
  // the answer runs.
  if ((i.kept ?? late.kept) > 0) return `${n} kept for your next session ${sep} /adage review`
  if (i.review > 0) return `${n} ${i.review} need review ${sep} /adage review`
  return i.versionLine
}

/** `band.version`: a harness newer than the plugin was tested with (ux-design §9.6). */
export function versionLine(version: string, tested: string, ascii: boolean): string {
  return newer(version, tested)
    ? `${g('notice', ascii)} Claude Code ${version} is newer than this plugin was tested with (${tested}) ${g('sep', ascii)} run adage doctor`
    : ''
}

/** Whether dotted version `a` is newer than `b` (a pre-release suffix is ignored). */
export function newer(a: string, b: string): boolean {
  const parts = (v: string) => v.split(/[-+]/)[0]?.split('.').map(x => Number.parseInt(x, 10) || 0) ?? []
  const x = parts(a)
  const y = parts(b)
  for (let k = 0; k < Math.max(x.length, y.length); k++) {
    const d = (x[k] ?? 0) - (y[k] ?? 0)
    if (d !== 0) return d > 0
  }
  return false
}

/** Distinct reflexes fired in this session: the hooks' blocks, and this session's feed rows. */
export function firedThisSession(fired: readonly Fired[], rows: readonly Row[]): number {
  const routes = new Set(fired.map(f => f.route).filter(r => r !== ''))
  for (const row of rows) if (row.mine) for (const a of row.acts) if (a.act === 'directed' && a.route) routes.add(a.route)
  return routes.size
}

/** `spinner.suffix` (`ui.spinner`, off by default): ` · adage ◠ n…` while a reflex was delivered this turn. */
export function spinnerSuffix(n: number, ascii: boolean): string {
  return n > 0 ? cut(` ${g('sep', ascii)} adage ${g('reflex', ascii)} ${n}${ascii ? '...' : '…'}`, 16, ascii) : ''
}

/** `turn.summary` (`ui.turn_line`, off by default): the turn's counts, never a saving; over 72 columns the last counts go first. */
export function turnSummary(steps: number, reused: number, headedOff: number, ascii: boolean): string {
  const sep = ` ${g('sep', ascii)} `
  let parts = [
    steps > 0 ? `${plural(steps, 'step', 'steps')} without a model request` : '',
    reused > 0 ? `${plural(reused, 'test run', 'test runs')} reused` : '',
    headedOff > 0 ? `${plural(headedOff, 'dead end', 'dead ends')} headed off` : '',
  ].filter(p => p !== '')
  if (parts.length === 0) return ''
  const line = () => `${sep}adage: ${parts.join(sep)}`
  while (parts.length > 1 && cols(line()) > 72) parts = parts.slice(0, -1)
  return cut(line(), 72, ascii)
}

// ------------------------------------------------------------------------------------- the pane

const pad2 = (n: number) => String(n).padStart(2, '0')

/** A row's time as `adage watch` prints it (`04:22:53`). */
export function clock(ts: number): string {
  const d = new Date(ts)
  return `${pad2(d.getHours())}:${pad2(d.getMinutes())}:${pad2(d.getSeconds())}`
}

const HARNESS: Record<string, string> = { claude: 'cc', 'claude-code': 'cc', opencode: 'oc', codex: 'cx' }
const STRENGTH = (s: string | undefined) => (s ?? '').toUpperCase()

/** One act as the Live tab writes it; strength words are allowed here (ux-design §1.3). */
export function actText(a: Act, ascii: boolean): string {
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
export const isQuiet = (row: Row): boolean => row.acts.length === 0

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

/** The item of a waiting list by its key. */
export const waitingOf = (items: readonly Waiting[], key: string): Waiting | undefined => items.find(w => w.key === key)

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

// ------------------------------------------------------------------------------------ /adage why

/**
 * `/adage why` after a verified skill (ux-design §4.8, `cmd.why.step`): each step the replay
 * verified here, `s{i} {command} · works | stale: {signature} · {date}`.
 */
export function whySkill(skill: Skill, ascii: boolean): string {
  const sep = ` ${g('sep', ascii)} `
  const works = skill.steps.filter(s => s.result === 'works').length
  const lines = [`${g('skill', ascii)} adage verified ${works} of ${skill.steps.length} steps of the ${skill.skill} skill here`, '']
  skill.steps.forEach((s, k) => {
    const state = s.result === 'works' ? 'works' : `stale${s.signature === '' ? '' : `: ${s.signature}`}`
    lines.push([`s${k + 1} ${s.command}`, state, skill.date].filter(p => p !== '').join(sep))
  })
  return lines.join('\n')
}

/**
 * `/adage why`: the last directive this session, as the model read it, with its route, track,
 * evidence and whether it was followed; after a skill Adage verified in place, its steps; with
 * nothing fired, in either arm, `cmd.why.none`.
 */
export function whyText(rows: readonly Row[], reflexes: readonly Reflex[], ascii: boolean, skill: Skill | undefined = late.skill): string {
  for (let i = rows.length - 1; i >= 0; i--) {
    const row = rows[i]
    if (row === undefined || !row.mine) continue
    const directed = [...row.acts].reverse().find(a => a.act === 'directed' && a.route)
    if (directed?.route === undefined) continue
    if (skill !== undefined && skill.at >= row.ts) return whySkill(skill, ascii)
    const route = directed.route
    const text = row.texts.find(t => t.route === route)?.text ?? ''
    const r = reflexOf(reflexes, route)
    const followed = rows.slice(i).some(x => x.mine && x.acts.some(a => (a.act === 'followed' || a.act === 'met') && a.route === route))
    const sep = ` ${g('sep', ascii)} `
    const lines = [`${g('reflex', ascii)} ${r?.gist || r?.summary || route}`]
    if (text !== '') lines.push('', text.trim())
    lines.push('', [`route ${r !== undefined ? `${r.track}/` : ''}${route}`, r?.describe ?? '', followed ? 'followed' : 'not followed yet'].filter(p => p !== '').join(sep))
    return lines.join('\n')
  }
  if (skill !== undefined) return whySkill(skill, ascii)
  return 'No reflex has spoken in this session yet.'
}

// --------------------------------------------------------------------------- the status card

/** The status card as `adage --json --for-session` gives it (A5). */
export type Card = {
  header: string
  off: boolean
  rows: { key?: string; text: string }[]
  next: { command: string; why: string }[]
}

const isRecord = (v: unknown): v is Record<string, unknown> => v !== null && typeof v === 'object' && !Array.isArray(v)

/** Reads the card JSON; undefined when it is not one. */
export function cardOf(v: unknown): Card | undefined {
  if (!isRecord(v) || typeof v.header !== 'string') return undefined
  const rows = (Array.isArray(v.rows) ? v.rows : []).filter(isRecord).map(r => ({ ...(typeof r.key === 'string' ? { key: r.key } : {}), text: typeof r.text === 'string' ? r.text : '' }))
  const next = (Array.isArray(v.next) ? v.next : []).filter(isRecord).map(n => ({ command: typeof n.command === 'string' ? n.command : '', why: typeof n.why === 'string' ? n.why : '' })).filter(n => n.command !== '')
  return { header: v.header, off: v.off === true, rows, next }
}

const KEY_WIDTH = 12

/**
 * The card as text, the form `/adage` answers everywhere (and the only form a headless session or
 * the VS Code panel shows): the header, the key column, `next`. The verbs `/adage` takes close it.
 */
export function cardText(card: Card, only?: readonly string[]): string {
  const lines = [card.header]
  let keep = only === undefined
  for (const r of card.rows) {
    if (only !== undefined && r.key !== undefined) keep = r.key !== '' ? only.includes(r.key) : keep
    if (!keep) continue
    lines.push(r.key === undefined ? r.text : `${r.key.padEnd(KEY_WIDTH)}${r.text}`)
  }
  if (only === undefined) {
    card.next.forEach((n, k) => lines.push(`${(k === 0 ? 'next' : '').padEnd(KEY_WIDTH)}${n.command} · ${n.why}`))
    lines.push(`${''.padEnd(KEY_WIDTH)}/adage watch · reflexes · health · why · map · pause · off · on`)
  }
  return lines.join('\n')
}

/** The card's text read back into its parts, for the `CommandOutput` tree (the inverse of `cardText`). */
export function parseCardText(text: string): { header: string; rows: { key: string; text: string }[] } {
  const [header = '', ...rest] = text.split('\n')
  return {
    header,
    rows: rest.map(l => (l.length > KEY_WIDTH && /^\S/.test(l) && l.slice(0, KEY_WIDTH).trimEnd() !== '' && !l.slice(0, KEY_WIDTH).trimEnd().includes(' ')
      ? { key: l.slice(0, KEY_WIDTH).trimEnd(), text: l.slice(KEY_WIDTH) }
      : { key: '', text: l.startsWith(' '.repeat(KEY_WIDTH)) ? l.slice(KEY_WIDTH) : l })),
  }
}

/** The card rows `/adage savings` shows: what the binary measured, as the card words it. */
export const SAVINGS_ROWS = ['activity', 'rewrites', 'savings']

// -------------------------------------------------------------------------------- the learn toast

/**
 * `toast.learned.*` and `toast.learn.failed`: the toast a learn line earns, or undefined (no change
 * worth one). `say` names a reflex by what it tells the agent (the graph's gist); a learn line
 * carries only its route and a summary of the change.
 */
export function learnToast(line: Record<string, unknown>, session: string, ascii: boolean, say: (route: string) => string | undefined = () => undefined): string | undefined {
  const rg = g('reflex', ascii)
  const sep = g('sep', ascii)
  if (line.kind === 'condition_error') {
    const cls = typeof line.class === 'string' ? line.class : 'error'
    return `adage could not learn from this session (${cls}) ${sep} run adage doctor`
  }
  const changes = (Array.isArray(line.changes) ? line.changes : []).filter(isRecord)
  if (changes.length === 0) return undefined
  const ours = changes.some(c => Array.isArray(c.sessions) && c.sessions.includes(session))
  const head = `adage learned from ${ours ? 'this session' : 'recent sessions'}`
  const name = (c: Record<string, unknown>) => {
    const route = typeof c.route === 'string' ? c.route : ''
    return say(route) || route.replace(/-/g, ' ')
  }
  const isInstinct = (c: Record<string, unknown>) => typeof c.track === 'string' && /\/instincts$/.test(c.track)
  const fresh = changes.filter(c => c.change === 'new')
  if (fresh.length >= 3) {
    // A first conditioning: many reflexes at once, from the repository's history or its files.
    const reflexes = fresh.filter(c => !isInstinct(c)).length
    const instincts = fresh.length - reflexes
    const sessions = typeof line.sessions_used === 'number' ? line.sessions_used : 0
    return `adage learned from ${plural(sessions, 'session', 'sessions')}\n${rg} ${plural(reflexes, 'reflex', 'reflexes')} and ${plural(instincts, 'instinct', 'instincts')} are now active here`
  }
  const first = fresh[0]
  if (first !== undefined) return `${head}\n${rg} ${name(first)} ${sep} new, from ${ours ? 'this session' : 'recent sessions'}`
  const firmer = changes.find(c => c.change === 'strength' && c.to === 'fire')
  if (firmer !== undefined) return `${head}\n${rg} ${name(firmer)} ${sep} now a direct instruction`
  const softer = changes.find(c => c.change === 'strength' && c.from === 'fire')
  if (softer !== undefined) return `${head}\n${rg} ${name(softer)} ${sep} now a suggestion`
  const review = changes.find(c => c.change === 'nascent')
  if (review !== undefined) return `${head}\n${rg} ${name(review)} ${sep} needs you ${sep} /adage review`
  return undefined
}

// --------------------------------------------------------------------------- reading the binary

const str = (v: unknown): string => (typeof v === 'string' ? v : '')
const label = (v: unknown): string => (isRecord(v) ? str(v.label) || str(v.kind) : str(v))

/**
 * The reflexes of `adage graph --json`, keeping only what the pane and the lines draw: never the
 * `arms`, `live` or control counts a route also carries (ux-design §1.7: the arm is never drawn).
 */
export function reflexesOf(graph: Record<string, unknown>): Reflex[] {
  const routes = Array.isArray(graph.routes) ? graph.routes.filter(isRecord) : []
  return routes.map(r => {
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
      verbs: Array.isArray(review.verbs) ? review.verbs.filter((v): v is string => typeof v === 'string') : [],
      reasons: Array.isArray(review.reasons) ? review.reasons.filter(isRecord).map(x => str(x.kind)).filter(k => k !== '') : [],
      offers: Array.isArray(review.offers) ? review.offers.filter(isRecord).map(o => ({ kind: str(o.kind), command: str(o.command) })) : [],
    }
  }).filter(r => r.id !== '')
}

/** The checks of `adage doctor --json`. */
export function checksOf(doctor: Record<string, unknown>): Check[] {
  const checks = Array.isArray(doctor.checks) ? doctor.checks.filter(isRecord) : []
  return checks.map(c => {
    const a = isRecord(c.action) ? c.action : undefined
    const action: Check['action'] = a === undefined ? null : a.kind === 'run' && typeof a.command === 'string' ? { kind: 'run', command: a.command } : { kind: 'manual' }
    const status = c.status === 'fail' || c.status === 'warn' ? c.status : 'pass'
    return { label: str(c.label) || str(c.check), status, detail: str(c.detail), fix: str(c.fix), action }
  })
}

/** The lapsed grants of `adage delegate status --json`. */
export function lapsedOf(status: Record<string, unknown>): Lapse[] {
  const lapsed = Array.isArray(status.lapsed) ? status.lapsed.filter(isRecord) : []
  return lapsed.map(l => ({
    route: str(l.route),
    changed: Array.isArray(l.changed) ? l.changed.filter((x): x is string => typeof x === 'string') : [],
    reapprove: str(l.reapprove),
  })).filter(l => l.route !== '')
}

/** The team packs awaiting trust in `adage status --json`. */
export function packsOf(status: Record<string, unknown>): Pack[] {
  const packs = Array.isArray(status.packs_awaiting_trust) ? status.packs_awaiting_trust.filter(isRecord) : []
  return packs.map(p => ({ name: str(p.name), command: str(p.command) })).filter(p => p.name !== '')
}

/** The address `adage map` printed (`http://127.0.0.1:<port>/`), as a link the map answers (`localhost`). */
export function mapUrl(text: string): string | undefined {
  const m = /http:\/\/127\.0\.0\.1:(\d{1,5})\//.exec(text)
  return m === null ? undefined : `http://localhost:${m[1]}/`
}
