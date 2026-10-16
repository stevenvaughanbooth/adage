// The session graph (docs/claude-mod/ux-design.md §6.0): this session as a DAG that grows while the
// agent works, newest at the bottom. Each prompt is a box, the root of its turn; the agent's tool
// batches hang off a trunk beneath it, one line each (`Bash ×3 · Edit`); each reflex Adage delivered
// branches off the step it followed as the map's picture in small: a `◆ adage` box in its strength's
// role, an arrow, and the response, lit in the fire role once the agent followed it. What else Adage
// did at a step (chose it without a model request, pointed a run at the project's interpreter, ran
// a command instead, headed off a dead end, answered from a receipt) hangs off it as one line that
// says `adage`. A long quiet stretch folds into one line; the finish line and the turn's end close
// the trunk. Wide (60 columns and more, on the terminal) the boxes; narrower, or where a box cannot
// be drawn, one line per node. Pure: dag.ts keeps the nodes, draw/graph.tsx lays the lines out.
import { PULSE_MS } from '../dag'
import type { GraphNode, Mark, Predicted, Reflex, Row } from '../plugin'
import type { Role } from '../tokens'
import { clock, responseOf, strengthRole } from './pane'
import type { Seg } from './pane'
import { cols, cut, g, plural, reflexOf, sayOf } from './text'

/** The edges' glyphs and their ASCII twins (one column each). */
const GLYPH = {
  h: ['─', '-'],
  v: ['│', '|'],
  tee: ['├', '+'],
  end: ['╰', '`'],
  down: ['┬', '+'],
  tl: ['╭', '+'],
  tr: ['╮', '+'],
  bl: ['╰', '+'],
  br: ['╯', '+'],
  head: ['▶', '>'],
  fold: ['┊', ':'],
  up: ['↑', '^'],
  times: ['×', 'x'],
} as const
const gl = (name: keyof typeof GLYPH, ascii: boolean): string => GLYPH[name][ascii ? 1 : 0]

/** What the graph is drawn from: the nodes, and what the rest of `$.state` says about them. */
export type GraphInput = {
  nodes: readonly GraphNode[]
  /** The steps Adage took in the model's place: a batch holding one of their calls was Adage's. */
  predicted: readonly Predicted[]
  /** The markers under tool rows (rewrites, substitutions, dead ends headed off, receipts). */
  marks: readonly Mark[]
  /** The feed's rows: this session's `followed` and `met` light a response; `directed` names it. */
  rows: readonly Row[]
  reflexes: readonly Reflex[]
  now: number
  /** Whether the newest node glows for a moment (false under reduced motion: it is only bold). */
  motion: boolean
}

/** Where it is drawn: the pane's columns, ASCII twins, boxes allowed, the most lines (0: all). */
export type GraphFrame = { columns: number; ascii: boolean; boxes: boolean; height: number }

/** The drawn graph: its summary line, its lines (newest last), and what scrolled off the top. */
export type GraphDrawn = { head: Seg[]; lines: Seg[][]; earlier: { prompts: number; steps: number } }

/** The widest the graph draws (a wider pane leaves the rest empty), and the box tier's narrowest. */
const MAX_W = 120
export const WIDE = 60
/** The trunk's column, the branches' rail, where a branch's box begins, the arrow between two boxes. */
const RAIL_LEAD = 7
const ARROW_W = 6
const MAX_BOX = 44
/** A quiet stretch this long or longer folds into one line. */
export const QUIET_RUN = 3
/** The clocks' skew between the hooks (`at`) and the feed's rows (`ts`), in ms. */
const SKEW = 2000

type NodeOf<K extends GraphNode['kind']> = Extract<GraphNode, { kind: K }>

/** A reflex as drawn: its node, whether (and where) the agent followed it, what it named. */
type Fire = { node: NodeOf<'reflex'>; index: number; followed: Row | undefined; detail: string }
/** Something else Adage did at a step, one line: its glyph, then the words after `adage`. */
type Did = { glyph: string; text: string }
type Step =
  | { kind: 'tools'; node: NodeOf<'tools'>; index: number; fires: Fire[]; did: Did[]; failed: number; followed: boolean }
  | { kind: 'stop'; node: NodeOf<'stop'>; index: number; fires: Fire[] }
type Turn = { prompt: NodeOf<'prompt'> | undefined; index: number; fires: Fire[]; steps: Step[]; closed: boolean }

const STRENGTH = (s: string): string => s.toUpperCase()

/** A tool's name as the line says it: an MCP tool by its own name (`notion-search`). */
const toolName = (t: string): string => (t.startsWith('mcp__') ? (t.split('__').pop() ?? t) : t)

/**
 * The feed's row that says the agent followed a reflex fired at `at` (this session's `followed` or
 * `met` for its route), before the same reflex fired again (`until`).
 */
function followedRow(rows: readonly Row[], route: string, at: number, until: number): Row | undefined {
  return rows.find(r => r.mine && r.ts >= at - SKEW && r.ts < until - SKEW && r.acts.some(a => (a.act === 'followed' || a.act === 'met') && a.route === route))
}

/** What the reflex told the agent to run, as this session's feed named it nearest its firing. */
function detailNear(rows: readonly Row[], route: string, at: number): string {
  let best = ''
  let far = Infinity
  for (const r of rows) {
    if (!r.mine) continue
    const a = r.acts.find(x => x.act === 'directed' && x.route === route && (x.detail ?? '') !== '')
    if (a !== undefined && Math.abs(r.ts - at) < far) [best, far] = [a.detail ?? '', Math.abs(r.ts - at)]
  }
  return best
}

/** What else Adage did at a batch's calls (each once), in the words the transcript's markers use. */
function didAt(ids: readonly string[], i: GraphInput, ascii: boolean): Did[] {
  const out: Did[] = []
  const sep = g('sep', ascii)
  if (i.predicted.some(p => p.ids.some(id => ids.includes(id)))) out.push({ glyph: g('active', ascii), text: `chose this step ${sep} no model request` })
  const seen = new Set<string>()
  for (const x of i.marks) {
    if (!ids.includes(x.id) || seen.has(x.kind)) continue
    seen.add(x.kind)
    if (x.kind === 'rewrite') out.push({ glyph: g('rewrite', ascii), text: `pointed this ${x.what === 'test' ? 'test run' : 'Python run'} at ${x.interpreter}` })
    else if (x.kind === 'substituted') out.push({ glyph: g('rewrite', ascii), text: `ran ${x.command} instead` })
    else if (x.kind === 'avoided') out.push({ glyph: g('ok', ascii), text: 'headed off a dead end' })
    else if (x.kind === 'answered') out.push({ glyph: g('ok', ascii), text: `answered from receipt ${x.receipt} ${sep} it did not run` })
  }
  return out
}

/** The turns, their steps, and each reflex hung off the step it followed (by its calls, else the step before it). */
function build(i: GraphInput, ascii: boolean): Turn[] {
  const turns: Turn[] = []
  const failed = new Set<string>()
  const reflexes: { node: NodeOf<'reflex'>; index: number }[] = []
  const open = (index: number): Turn => {
    const last = turns.at(-1)
    if (last !== undefined && !last.closed) return last
    const t: Turn = { prompt: undefined, index, fires: [], steps: [], closed: false }
    turns.push(t)
    return t
  }
  i.nodes.forEach((n, index) => {
    if (n.kind === 'prompt') turns.push({ prompt: n, index, fires: [], steps: [], closed: false })
    else if (n.kind === 'tools') open(index).steps.push({ kind: 'tools', node: n, index, fires: [], did: [], failed: 0, followed: false })
    else if (n.kind === 'stop') {
      const t = open(index)
      t.steps.push({ kind: 'stop', node: n, index, fires: [] })
      if (!n.gate) t.closed = true
    } else if (n.kind === 'failed') failed.add(n.id)
    else reflexes.push({ node: n, index })
  })
  // Where a reflex hangs: the batch holding a call it followed, else the last node before it.
  const anchors: { index: number; fires: Fire[] }[] = turns.flatMap(t => [{ index: t.index, fires: t.fires }, ...t.steps]).sort((a, b) => a.index - b.index)
  const tools = turns.flatMap(t => t.steps).filter((s): s is Extract<Step, { kind: 'tools' }> => s.kind === 'tools')
  for (const [k, r] of reflexes.entries()) {
    const again = reflexes.slice(k + 1).find(x => x.node.route === r.node.route)
    const fire: Fire = {
      node: r.node,
      index: r.index,
      followed: followedRow(i.rows, r.node.route, r.node.at, again?.node.at ?? Infinity),
      detail: detailNear(i.rows, r.node.route, r.node.at),
    }
    const byCall = r.node.ids.length === 0 ? undefined : tools.find(s => s.node.ids.some(id => r.node.ids.includes(id)))
    const before = [...anchors].reverse().find(a => a.index < r.index)
    const anchor = byCall ?? before ?? anchors[0] ?? open(r.index)
    anchor.fires.push(fire)
    // The step the agent followed it in: the batch the feed's row names, else the one it was judged at.
    const row = fire.followed
    if (row === undefined) continue
    const later = tools.filter(s => s.index > r.index)
    const at = later.find(s => row.ids !== undefined && s.node.ids.some(id => row.ids?.includes(id))) ?? later.find(s => row.ts >= s.node.from - 50 && row.ts <= s.node.at + SKEW)
    if (at !== undefined) at.followed = true
  }
  for (const s of tools) {
    s.failed = s.node.ids.filter(id => failed.has(id)).length
    s.did = didAt(s.node.ids, i, ascii)
  }
  return turns
}

// ------------------------------------------------------------------------------ the pieces

const sp = (n: number): Seg => ({ text: ' '.repeat(Math.max(0, n)) })
const dim = (text: string): Seg => ({ text, role: 'ink3' })
const width = (segs: readonly Seg[]): number => segs.reduce((n, s) => n + cols(s.text), 0)

/** Segments cut to `room` columns (the last kept one with `…`), the rest dropped. */
function fitSegs(segs: readonly Seg[], room: number, ascii: boolean): Seg[] {
  const out: Seg[] = []
  let used = 0
  for (const s of segs) {
    if (used >= room) break
    const t = cut(s.text, room - used, ascii)
    if (t === '') break
    out.push({ ...s, text: t })
    used += cols(t)
  }
  return out
}

/** `left` cut to leave room for `right`, which is set flush against column `end`. */
function spread(left: readonly Seg[], right: readonly Seg[], end: number, ascii: boolean): Seg[] {
  const rw = width(right)
  if (rw === 0) return fitSegs(left, end, ascii)
  const kept = fitSegs(left, Math.max(0, end - rw - 2), ascii)
  return [...kept, sp(end - rw - width(kept)), ...right]
}

/** What the drawing needs to know besides the graph: the frame, the newest node, whether it glows. */
type Ctx = {
  i: GraphInput
  ascii: boolean
  /** The graph's width, the box tier's box width (0: the list), and whether times show. */
  w: number
  box: number
  times: boolean
  newest: number
  fresh: boolean
}

/** A node's time: `now` while the newest glows. */
const when = (c: Ctx, index: number, at: number): string => (index === c.newest && c.fresh ? 'now' : clock(at))
/** The time's piece, in the accent while it is the newest's glow. */
const timeSeg = (c: Ctx, index: number, at: number): Seg[] => (c.times ? [{ text: when(c, index, at), role: index === c.newest && c.fresh ? 'accent' : 'ink3' }] : [])

/** A batch's words: each tool once with its count (`Bash ×3 · Edit`), and a lone call's word. */
export function toolsLabel(n: NodeOf<'tools'>, ascii: boolean): Seg[] {
  const counts = new Map<string, number>()
  for (const t of n.tools) counts.set(toolName(t), (counts.get(toolName(t)) ?? 0) + 1)
  const out: Seg[] = []
  for (const [name, k] of counts) {
    if (out.length > 0) out.push(dim(` ${g('sep', ascii)} `))
    out.push({ text: name })
    if (k > 1) out.push(dim(` ${gl('times', ascii)}${k}`))
  }
  if (n.detail !== '') out.push(sp(2), { text: n.detail, role: n.tools[0] === 'Bash' ? 'command' : 'ink2' })
  return out
}

/** Where the reflex fired, as its box's caption says it. */
function where(event: string): string {
  switch (event) {
    case 'UserPromptSubmit':
      return 'at your prompt'
    case 'PostToolBatch':
      return 'after this step'
    case 'PostToolUseFailure':
      return 'after a failure'
    case 'Stop':
      return 'at the finish line'
    default:
      return 'fired'
  }
}

/** A fired reflex's look: its role, what it tells the agent, its response, followed or not. */
function look(c: Ctx, f: Fire) {
  const r = reflexOf(c.i.reflexes, f.node.route, f.node.track)
  const strength = f.node.strength || r?.strength || 'prime'
  const rs = r === undefined ? undefined : responseOf(r)
  const text = f.detail || rs?.text || ''
  return {
    role: strengthRole(strength),
    strength,
    say: sayOf(c.i.reflexes, f.node.route, f.node.track),
    title: rs?.title ?? (f.detail !== '' ? 'RUN' : 'RESPONSE'),
    response: text,
    command: rs?.command ?? f.detail !== '',
    followed: f.followed !== undefined,
    // A dead end the agent kept away from was headed off; anything else was followed.
    done: strength === 'inhibit' ? 'headed off' : 'followed',
  }
}

/** One rounded box, `w` wide, three lines: the title in the top border, one line inside, the caption in the bottom. */
function box(w: number, title: readonly Seg[], body: Seg, caption: readonly Seg[], border: Role, ascii: boolean, bold: boolean): [Seg[], Seg[], Seg[]] {
  const edge = (text: string): Seg => (bold ? { text, role: border, bold: true } : { text, role: border })
  const h = gl('h', ascii)
  const set = (left: string, right: string, inner: readonly Seg[]): Seg[] => {
    const kept = fitSegs(inner, w - 5, ascii)
    if (kept.length === 0) return [edge(`${left}${h.repeat(w - 2)}${right}`)]
    return [edge(`${left}${h} `), ...kept, edge(` ${h.repeat(Math.max(0, w - 5 - width(kept)))}${right}`)]
  }
  const text = cut(body.text, w - 4, ascii)
  return [
    set(gl('tl', ascii), gl('tr', ascii), title),
    [edge(`${gl('v', ascii)} `), { ...body, text }, sp(w - 4 - cols(text)), edge(` ${gl('v', ascii)}`)],
    set(gl('bl', ascii), gl('br', ascii), caption),
  ]
}

// ------------------------------------------------------------------------------ the box tier

/** A block of lines that scroll off together, and the prompts and steps it stands for. */
type Block = { lines: Seg[][]; prompts: number; steps: number }

/** A reflex off the rail: the `◆ adage` box in its strength's role, the arrow, the response. */
function fireRows(c: Ctx, f: Fire, last: boolean, trunk: string): Seg[][] {
  const { ascii } = c
  const l = look(c, f)
  const hot = f.index === c.newest
  const sep = g('sep', ascii)
  // A narrow box keeps the strength word whole and the time: the separator and the event's words go first.
  const room = c.box - 5
  const mark: Seg[] = [{ text: `${g('lamp', ascii)} `, role: l.role }, { text: STRENGTH(l.strength), role: l.role, bold: true }]
  const adage: Seg = { text: `${g('active', ascii)} adage`, role: l.role, bold: true }
  const title: Seg[] = width([adage, dim(` ${sep} `), ...mark]) <= room ? [adage, dim(` ${sep} `), ...mark] : [adage, sp(1), ...mark]
  const stamp = when(c, f.index, f.node.at)
  const said = `${where(f.node.event)} ${sep} ${stamp}`
  const caption: Seg[] = [{ text: cols(said) <= room ? said : stamp, role: hot && c.fresh ? 'accent' : 'ink3', ...(hot ? { bold: true } : {}) }]
  const [aTop, aMid, aBottom] = box(c.box, title, { text: l.say }, caption, l.role, ascii, hot)
  const done: Seg[] = l.followed ? [{ text: `${g('ok', ascii)} ${l.done}`, role: 'fire', bold: true }] : []
  const [bTop, bMid, bBottom] = box(c.box, [{ text: l.title, role: 'ink3', bold: true }], l.command ? { text: l.response, role: 'command' } : { text: l.response }, done, l.followed ? 'fire' : 'ink3', ascii, false)
  const h = gl('h', ascii)
  const arrow: Seg = { text: ` ${h.repeat(3)}${gl('head', ascii)} `, role: l.followed ? 'fire' : 'ink3', ...(l.followed ? { bold: true } : {}) }
  const lead = [sp(2), dim(trunk), sp(1)]
  return [
    [...lead, { text: gl('v', ascii), role: l.role }, sp(2), ...aTop, sp(ARROW_W), ...bTop],
    [...lead, { text: `${gl(last ? 'end' : 'tee', ascii)}${h}${gl('head', ascii)}`, role: l.role }, ...aMid, arrow, ...bMid],
    [...lead, last ? sp(1) : dim(gl('v', ascii)), sp(2), ...aBottom, sp(ARROW_W), ...bBottom],
  ]
}

/** Something else Adage did at a step, off the rail: `├─ ◆ adage chose this step · no model request`. */
function didRow(c: Ctx, d: Did, last: boolean, trunk: string): Seg[] {
  const lead = [sp(2), dim(trunk), sp(1), dim(`${gl(last ? 'end' : 'tee', c.ascii)}${gl('h', c.ascii)} `)]
  return fitSegs([...lead, { text: `${d.glyph} adage`, role: 'accent', bold: true }, { text: ` ${d.text}` }], c.w, c.ascii)
}

/** What hangs off a node's rail, in order: what Adage did, then the reflexes it delivered. */
function railRows(c: Ctx, did: readonly Did[], fires: readonly Fire[], trunk: string): Seg[][] {
  const n = did.length + fires.length
  const rows: Seg[][] = []
  did.forEach((d, k) => rows.push(didRow(c, d, k === n - 1, trunk)))
  fires.forEach((f, k) => rows.push(...fireRows(c, f, did.length + k === n - 1, trunk)))
  return rows
}

/** The prompt's box: `PROMPT n` and the time in its top border, the prompt inside, the trunk (and rail) beneath. */
function promptRows(c: Ctx, t: Turn): Seg[][] {
  const { ascii } = c
  const h = gl('h', ascii)
  const p = t.prompt
  if (p === undefined) return [[sp(2), dim(gl('down', ascii)), dim(` earlier in this session`)]]
  const hot = t.index === c.newest
  const role: Role = hot && c.fresh ? 'accent' : 'ink3'
  const edge = (text: string): Seg => (hot ? { text, role, bold: true } : { text, role })
  const title: Seg[] = [{ text: `PROMPT ${p.n}`, role: 'ink3', bold: true }]
  const time = timeSeg(c, t.index, p.at)
  const room = c.w - 8 - width(title) - width(time)
  const top: Seg[] =
    time.length > 0 && room >= 1
      ? [edge(`${gl('tl', ascii)}${h} `), ...title, edge(` ${h.repeat(room)} `), ...time, edge(` ${h}${gl('tr', ascii)}`)]
      : [edge(`${gl('tl', ascii)}${h} `), ...fitSegs(title, c.w - 5, ascii), edge(` ${h.repeat(Math.max(0, c.w - 5 - width(title)))}${gl('tr', ascii)}`)]
  const said = cut(p.text.replace(/\s+/g, ' ').trim(), c.w - 4, ascii)
  const mid: Seg[] = [edge(`${gl('v', ascii)} `), { text: said, bold: hot }, sp(c.w - 4 - cols(said)), edge(` ${gl('v', ascii)}`)]
  const bottom = Array.from({ length: c.w }, (_, x) => (x === 0 ? gl('bl', ascii) : x === c.w - 1 ? gl('br', ascii) : x === 2 || (x === 4 && t.fires.length > 0) ? gl('down', ascii) : h)).join('')
  return [top, mid, [edge(bottom)]]
}

/** A step's line on the trunk: `├─┬ Bash ×2 · Edit` (`┬` when something hangs off it), its tags, its time. */
function stepRow(c: Ctx, s: Step, branch: boolean, last: boolean): Seg[] {
  const { ascii } = c
  const h = gl('h', ascii)
  const lead: Seg[] = [sp(2), dim(`${gl(last ? 'end' : 'tee', ascii)}${h}${branch ? gl('down', ascii) : h} `)]
  const hot = s.index === c.newest
  const sep = g('sep', ascii)
  let label: Seg[]
  const tags: Seg[] = []
  if (s.kind === 'tools') {
    label = toolsLabel(s.node, ascii)
    if (s.failed > 0) tags.push({ text: `${g('fail', ascii)} ${s.failed} failed`, role: 'inhibit' })
    if (s.followed) tags.push({ text: `${g('ok', ascii)} followed adage`, role: 'fire' })
  } else if (s.node.gate) {
    label = [{ text: `${g('active', ascii)} adage`, role: 'accent', bold: true }, { text: ` finish line ${sep} the agent continues once` }]
  } else {
    label = [dim(`${g('ok', ascii)} done`)]
  }
  if (hot) label = label.map(x => ({ ...x, bold: true, ...(c.fresh && x.role === undefined ? { role: 'accent' as const } : {}) }))
  const right = [...tags.flatMap((t, k) => (k === 0 ? [t] : [sp(2), t])), ...(tags.length > 0 && c.times ? [sp(2)] : []), ...timeSeg(c, s.index, s.node.at)]
  return spread([...lead, ...label], right, c.w - 3, ascii)
}

/** A quiet stretch folded: `┊   6 steps, nothing fired  Bash ×4 · Read ×2`. */
function foldRow(c: Ctx, run: readonly Extract<Step, { kind: 'tools' }>[], lead: number): Seg[] {
  const all: NodeOf<'tools'> = { kind: 'tools', at: 0, from: 0, ids: [], detail: '', tools: run.flatMap(s => s.node.tools) }
  const tools = toolsLabel(all, c.ascii).map(x => ({ ...x, role: 'ink3' as const }))
  return fitSegs([sp(lead), dim(gl('fold', c.ascii)), sp(lead === 2 ? 3 : 1), dim(`${run.length} steps, nothing fired`), sp(2), ...tools], c.w, c.ascii)
}

/** Whether a step is quiet: Adage did nothing at it and nothing about it calls for a look. */
const quiet = (c: Ctx, s: Step): s is Extract<Step, { kind: 'tools' }> => s.kind === 'tools' && s.fires.length === 0 && s.did.length === 0 && s.failed === 0 && !s.followed && s.index !== c.newest

/** A turn's steps, each run of `QUIET_RUN` or more quiet ones folded: the pieces to draw. */
function pieces(c: Ctx, t: Turn): ({ step: Step } | { fold: Extract<Step, { kind: 'tools' }>[] })[] {
  const out: ({ step: Step } | { fold: Extract<Step, { kind: 'tools' }>[] })[] = []
  let run: Extract<Step, { kind: 'tools' }>[] = []
  const flush = () => {
    if (run.length >= QUIET_RUN) out.push({ fold: run })
    else for (const s of run) out.push({ step: s })
    run = []
  }
  for (const s of t.steps) {
    if (quiet(c, s)) run.push(s)
    else {
      flush()
      out.push({ step: s })
    }
  }
  flush()
  return out
}

/** The box tier's blocks, a turn at a time. */
function wideBlocks(c: Ctx, turns: readonly Turn[]): Block[] {
  const blocks: Block[] = []
  const v = gl('v', c.ascii)
  turns.forEach((t, k) => {
    const head = promptRows(c, t)
    blocks.push({ lines: [...(k === 0 ? [] : [[]]), ...head, ...railRows(c, [], t.fires, v)], prompts: t.prompt === undefined ? 0 : 1, steps: 0 })
    const ps = pieces(c, t)
    ps.forEach((p, j) => {
      if ('fold' in p) {
        blocks.push({ lines: [foldRow(c, p.fold, 2)], prompts: 0, steps: p.fold.length })
        return
      }
      const s = p.step
      const last = s.kind === 'stop' && !s.node.gate && j === ps.length - 1
      const did = s.kind === 'tools' ? s.did : []
      // The trunk runs on beside what hangs here while the turn has more, or is still going.
      const trunk = j < ps.length - 1 || !t.closed ? v : ' '
      blocks.push({ lines: [stepRow(c, s, did.length + s.fires.length > 0, last), ...railRows(c, did, s.fires, trunk)], prompts: 0, steps: s.kind === 'tools' ? 1 : 0 })
    })
  })
  return blocks
}

// ------------------------------------------------------------------------------ the list tier

/** One line per node for a narrow pane (or where boxes cannot be drawn): the trunk in column 1. */
function listBlocks(c: Ctx, turns: readonly Turn[]): Block[] {
  const { ascii } = c
  const sep = g('sep', ascii)
  const v = gl('v', ascii)
  const blocks: Block[] = []
  const line = (segs: readonly Seg[]): Seg[] => fitSegs(segs, c.w, ascii)
  const fireLines = (f: Fire): Seg[][] => {
    const l = look(c, f)
    const hot = f.index === c.newest
    return [
      line([sp(1), dim(`${v} `), { text: `${g('active', ascii)} adage`, role: l.role, bold: true }, sp(1), { text: `${g('lamp', ascii)} ${STRENGTH(l.strength)}`, role: l.role, ...(hot ? { bold: true } : {}) }, dim(` ${sep} `), { text: l.say }]),
      line([sp(1), dim(`${v} `), sp(2), { text: `${gl('h', ascii).repeat(2)}${gl('head', ascii)} `, role: l.followed ? 'fire' : 'ink3' }, l.command ? { text: l.response, role: 'command' } : { text: l.response }, ...(l.followed ? [sp(2), { text: `${g('ok', ascii)} ${l.done}`, role: 'fire' as const, bold: true }] : [])]),
    ]
  }
  const didLine = (d: Did): Seg[] => line([sp(1), dim(`${v} `), { text: `${d.glyph} adage`, role: 'accent', bold: true }, { text: ` ${d.text}` }])
  turns.forEach((t, k) => {
    const p = t.prompt
    const hot = t.index === c.newest
    const head: Seg[] =
      p === undefined
        ? line([sp(1), dim(`${gl('down', ascii)} earlier in this session`)])
        : line([sp(1), { text: `${g('pointer', ascii)} `, role: 'accent' }, { text: `PROMPT ${p.n}`, role: 'ink3', bold: true }, sp(2), { text: p.text.replace(/\s+/g, ' ').trim(), ...(hot ? { bold: true } : {}) }])
    blocks.push({ lines: [...(k === 0 ? [] : [[]]), head, ...t.fires.flatMap(fireLines)], prompts: p === undefined ? 0 : 1, steps: 0 })
    const ps = pieces(c, t)
    ps.forEach((piece, j) => {
      if ('fold' in piece) {
        blocks.push({ lines: [foldRow(c, piece.fold, 1)], prompts: 0, steps: piece.fold.length })
        return
      }
      const s = piece.step
      const end = s.kind === 'stop' && !s.node.gate && j === ps.length - 1
      const mark = dim(`${gl(end ? 'end' : 'tee', ascii)} `)
      const sHot = s.index === c.newest
      const bold = (segs: Seg[]): Seg[] => (sHot ? segs.map(x => ({ ...x, bold: true })) : segs)
      let first: Seg[]
      if (s.kind === 'tools') {
        const tags: Seg[] = [...(s.failed > 0 ? [sp(2), { text: `${g('fail', ascii)} ${s.failed} failed`, role: 'inhibit' as const }] : []), ...(s.followed ? [sp(2), { text: `${g('ok', ascii)} followed adage`, role: 'fire' as const }] : [])]
        first = line([sp(1), mark, ...bold(toolsLabel(s.node, ascii)), ...tags])
      } else if (s.node.gate) first = line([sp(1), mark, { text: `${g('active', ascii)} adage`, role: 'accent', bold: true }, { text: ` finish line ${sep} the agent continues once` }])
      else first = line([sp(1), mark, ...bold([dim(`${g('ok', ascii)} done`)])])
      const did = s.kind === 'tools' ? s.did : []
      blocks.push({ lines: [first, ...did.map(didLine), ...s.fires.flatMap(fireLines)], prompts: 0, steps: s.kind === 'tools' ? 1 : 0 })
    })
  })
  return blocks
}

// ------------------------------------------------------------------------------ the whole

/** The summary above the graph: prompts and steps, then what Adage did, dropped from the right to fit. */
function headLine(i: GraphInput, turns: readonly Turn[], c: Ctx): Seg[] {
  const { ascii } = c
  const sep = g('sep', ascii)
  const steps = turns.flatMap(t => t.steps).filter((s): s is Extract<Step, { kind: 'tools' }> => s.kind === 'tools')
  const fires = turns.flatMap(t => [...t.fires, ...t.steps.flatMap(s => s.fires)])
  const followed = fires.filter(f => f.followed !== undefined).length
  const chose = steps.filter(s => i.predicted.some(p => p.ids.some(id => s.node.ids.includes(id)))).length
  // What Adage did first, so a narrow pane keeps it; the session's own counts go first when room runs out.
  const parts: Seg[][] = [
    [{ text: `${g('active', ascii)} `, role: 'accent' }, { text: 'this session', bold: true }],
    fires.length === 0 ? [dim('adage has fired no reflex yet')] : [{ text: `adage fired ${plural(fires.length, 'reflex', 'reflexes')}`, role: 'fire' }],
    ...(followed > 0 ? [[{ text: `${followed} followed`, role: 'fire' as const }]] : []),
    ...(chose > 0 ? [[{ text: `${plural(chose, 'step', 'steps')} without a model request`, role: 'fire' as const }]] : []),
    [dim(plural(turns.filter(t => t.prompt !== undefined).length, 'prompt', 'prompts'))],
    [dim(plural(steps.length, 'step', 'steps'))],
  ]
  const join = (ps: Seg[][]): Seg[] => ps.flatMap((p, k) => (k === 0 ? p : [dim(` ${sep} `), ...p]))
  let kept = parts
  while (kept.length > 1 && width(join(kept)) > c.w) kept = kept.slice(0, -1)
  return fitSegs(join(kept), c.w, ascii)
}

/** The newest node drawn (a failed call is not drawn: its batch is). */
function newestOf(nodes: readonly GraphNode[]): number {
  for (let k = nodes.length - 1; k >= 0; k--) if (nodes[k]?.kind !== 'failed') return k
  return -1
}

/**
 * The graph as lines of styled pieces, newest last: the box tier from `WIDE` columns where boxes
 * can be drawn, else one line per node. With a `height`, only the newest blocks that fit are drawn
 * (a line above says what scrolled off); every line is at most `columns` wide.
 */
export function graphLines(i: GraphInput, f: GraphFrame): GraphDrawn {
  const w = Math.max(1, Math.min(f.columns, MAX_W))
  const bw = Math.min(MAX_BOX, Math.floor((w - RAIL_LEAD - ARROW_W) / 2))
  const newest = newestOf(i.nodes)
  const at = i.nodes[newest]?.at ?? -Infinity
  const c: Ctx = { i, ascii: f.ascii, w, box: f.boxes && f.columns >= WIDE ? bw : 0, times: f.columns >= 80, newest, fresh: i.motion && i.now - at < PULSE_MS }
  const turns = build(i, f.ascii)
  const head = headLine(i, turns, c)
  if (newest < 0) return { head, lines: [], earlier: { prompts: 0, steps: 0 } }
  const blocks = c.box > 0 ? wideBlocks(c, turns) : listBlocks(c, turns)
  const total = blocks.reduce((n, b) => n + b.lines.length, 0)
  if (f.height <= 0 || total <= f.height) return { head, lines: blocks.flatMap(b => b.lines), earlier: { prompts: 0, steps: 0 } }
  // Follow the newest: drop whole blocks from the top until the rest and one line about them fit.
  let start = 0
  let used = total
  while (start < blocks.length - 1 && used > f.height - 1) used -= blocks[start++]?.lines.length ?? 0
  const gone = blocks.slice(0, start)
  const earlier = { prompts: gone.reduce((n, b) => n + b.prompts, 0), steps: gone.reduce((n, b) => n + b.steps, 0) }
  const kept = blocks.slice(start).flatMap(b => b.lines)
  if (kept[0]?.length === 0) kept.shift()
  const said = [earlier.prompts > 0 ? plural(earlier.prompts, 'prompt', 'prompts') : '', earlier.steps > 0 ? plural(earlier.steps, 'step', 'steps') : ''].filter(x => x !== '').join(` ${g('sep', f.ascii)} `)
  const note = fitSegs([sp(c.box > 0 ? 2 : 1), dim(`${gl('fold', f.ascii)} ${gl('up', f.ascii)} earlier: ${said || 'more'}`)], w, f.ascii)
  return { head, lines: [note, ...kept], earlier }
}

/** `pane.graph.empty`: the graph has nothing yet, and says what will grow here. */
export const GRAPH_EMPTY = 'Nothing yet. This graph grows as the agent works: each prompt, each step, and every reflex Adage fires.'
