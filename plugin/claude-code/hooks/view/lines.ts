// The lines Adage adds to the transcript and its toasts, as plain values (docs/claude-mod/ux-design.md
// §4.1–§4.5, §10): the opening line, the lines under a prompt, the markers under a tool row, the
// receipt's card and its line beside a run, the spinner suffix, the turn line, the learn and lapse
// toasts. Pure: draw/sites.ts reads the state these take, and draw/rows.tsx lays them out.
import type { Fired, Lapse, Mark, Reflex, Row } from '../plugin'
import { isRecord } from '../shape'
import { cols, cut, fit, g, plural, sayOf } from './text'

/** The binary's notice text without its `adage: ` prefix (ux-design §10: one helper). */
function stripPrefix(text: string): string | undefined {
  const m = /^\s*(?:adage|adage-exec):\s+/.exec(text)
  return m === null ? undefined : text.slice(m[0].length)
}

/** `open.notice`: the binary's opening line with the glyph; undefined for a notice not Adage's. */
export function openNotice(text: string, ascii: boolean): string | undefined {
  const rest = stripPrefix(text)
  return rest === undefined || rest.trim() === '' ? undefined : `${g('reflex', ascii)} adage ${g('sep', ascii)} ${rest}`
}

/** The most lines under one prompt: the runtime's decision cap (`params.rs`). */
const MAX_PROMPT_LINES = 3

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
 * `prompt.fired` and `tool.fired`: one line per reflex fired at a prompt or after a tool batch, in
 * delivery order, at most three: the glyph, `adage:` (so the person can tell the agent was told
 * this by Adage, not by them), what it tells the agent, then the command or files it names; at a
 * narrow width the command goes first (T-B2-03). No strength word.
 */
export function firedLines(fired: readonly Fired[], reflexes: readonly Reflex[], rows: readonly Row[], columns: number, ascii: boolean): string[] {
  const seen = new Set<string>()
  const lines: string[] = []
  for (const f of fired) {
    if (f.route === '' || seen.has(f.route)) continue
    seen.add(f.route)
    const say = sayOf(reflexes, f.route, f.track)
    const detail = detailOf(rows, f.route)
    const extra = detail !== '' && !say.includes(detail) ? detail : ''
    lines.push(fit([`${g('reflex', ascii)} adage: ${say}`, extra], columns, ascii))
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
const RECEIPT_BYTES = 2400

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

/** `toast.lapsed`: once per session per lapse (ux-design §4.5); the command stays in the Health tab. */
export const lapseToast = (l: Lapse, ascii: boolean): string =>
  `adage\n${l.route} is no longer allowed: ${l.changed[0] ?? 'it'} changed ${g('sep', ascii)} /adage review`

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
