// The band above the prompt, as plain values (docs/claude-mod/ux-design.md §5, §9.6, §9.12): its
// cells and their narrow-terminal priority, the lamp, the learner, and the one attention line.
// Pure: draw/sites.ts reads the state the band takes, and draw/band.tsx lays it out.
import type { Fired, Health, Learner, Row } from '../plugin'
import type { Role } from '../tokens'
import { cols, g, plural } from './text'

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
  /**
   * A newer Adage, as the status card words it (`latest`, and `next`, the command that brings it),
   * while the band's update line shows (until the first prompt, once per release per machine);
   * null otherwise.
   */
  update: { latest: string; next: string } | null
  versionLine: string
  columns: number
  ascii: boolean
  /** Answers kept for the next session start (`band.kept`). */
  kept: number
  /** Distinct sequences noticed this session (`1 pattern noticed`). */
  noticed: number
  /** Whether the pane is open: the noticed cell shows only while it is closed. */
  paneOpen: boolean
}

export type Band = { cells: Cell[]; second: string }

/** The lamp cell (ux-design §5): the word always beside the lamp, in the fire, prime or inhibit role. */
function lampCell(h: Health | null, ascii: boolean): Cell {
  const word = h?.word ?? 'healthy'
  const role: Role = word === 'problem' ? 'inhibit' : word === 'check' ? 'prime' : 'fire'
  const text = word === 'healthy' || h?.check == null ? word : `${word}: ${h.check}`
  return { glyph: g('lamp', ascii), lamp: role, text }
}

/** The learner's cell: `learning in 38 s`, `learning…`, `learned` for 10 s after a run, else nothing. */
function learnerCell(l: Learner | null, now: number, ascii: boolean): string {
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
 * one attention item, highest first: a lapsed grant, a problem, an answer kept, review waiting,
 * a newer Adage, the harness version.
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
  const noticedCell = i.noticed > 0 && !i.paneOpen ? `${plural(i.noticed, 'pattern', 'patterns')} noticed` : ''
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
  if (i.kept > 0) return `${n} kept for your next session ${sep} /adage review`
  if (i.review > 0) return `${n} ${i.review} need review ${sep} /adage review`
  // `band.update` (onboarding plan O9): the lowest of Adage's own items, and never a problem.
  if (i.update !== null) return updateLine(i.update, i.ascii)
  return i.versionLine
}

/** `band.update`: `» adage 0.2.0 is out · /adage update`, the command as the card words it. */
export function updateLine(u: { latest: string; next: string }, ascii: boolean): string {
  return `${g('notice', ascii)} adage ${u.latest} is out ${g('sep', ascii)} ${u.next}`
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
