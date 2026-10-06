// What Adage noticed (plan D5; ux-design §4.7, §5, §10): the noticed line under the tool batch that
// completed a candidate, worded from `status --json`'s candidates, and the band's `1 pattern
// noticed`, counted from this session's feed rows. The words are pure; `recountNoticed` and
// `wordCandidates` keep the count and the words current.
import { read } from './bridge'
import { batchAt } from './marks'
import type { Mod } from './mod'
import type { Act, Batch, Candidate, Row } from './plugin'
import { candidatesOf } from './reads'
import { REFRESH_MS } from './refresh'
import { g } from './view/text'

/** `noticed.forming` / `noticed.review`: plain counts, never a forecast of when a reflex forms. */
export function noticedLine(a: Act, say: string, ascii: boolean): string {
  const sep = g('sep', ascii)
  const head = `${g('off', ascii)} adage noticed ${sep} ${say}`
  if (a.review === true) return `${head} ${sep} waits for your review ${sep} /adage review`
  const n = a.support ?? 0
  return `${head} ${sep} seen in ${n} earlier ${n === 1 ? 'session' : 'sessions'}`
}

/** Whether a feed row's batch is the group drawn: by the calls it names, else by when its hooks ran. */
function belongs(row: Row, ids: readonly string[], batches: readonly Batch[]): boolean {
  if (row.ids !== undefined && row.ids.length > 0) return row.ids.some(id => ids.includes(id))
  const b = batchAt(batches, row.ts)
  return b !== undefined && b.ids.some(id => ids.includes(id))
}

/**
 * The noticed lines under one tool group (`ToolGroup`'s calls): one per candidate first noticed in
 * a batch of this group, worded from `status --json`'s candidates. A candidate already noticed in
 * an earlier batch, one the read does not know yet, or one that is not a sequence draws nothing.
 */
export function noticedLines(rows: readonly Row[], ids: readonly string[], batches: readonly Batch[], candidates: readonly Candidate[], ascii: boolean): string[] {
  const seen = new Set<string>()
  const lines: string[] = []
  for (const row of rows) {
    if (!row.mine) continue
    const here = belongs(row, ids, batches)
    for (const a of row.acts) {
      if (a.act !== 'noticed' || a.candidate === undefined || seen.has(a.candidate)) continue
      seen.add(a.candidate)
      if (!here) continue
      const c = candidates.find(x => x.id === a.candidate)
      if (c === undefined || !c.sequence) continue
      lines.push(noticedLine(a, c.say, ascii))
    }
  }
  return lines
}

/** The candidates noticed in this session's rows: the ids, each once. */
export function noticedIds(rows: readonly Row[]): string[] {
  const ids = new Set<string>()
  for (const row of rows) if (row.mine) for (const a of row.acts) if (a.act === 'noticed' && a.candidate !== undefined) ids.add(a.candidate)
  return [...ids]
}

/** The band's `1 pattern noticed`: distinct candidates noticed in this session's rows; a change redraws. */
export async function recountNoticed(m: Mod): Promise<void> {
  const n = noticedIds(await m.state.rows.read()).length
  if (n === m.rt.noticed) return
  m.rt.noticed = n
  m.ui.invalidate()
}

/** Reads `status --json` for the words of a noticed candidate the drawing does not know yet (at most once per `REFRESH_MS`). */
export async function wordCandidates(m: Mod, rows: readonly Row[]): Promise<void> {
  await recountNoticed(m)
  const known = new Set((await m.state.candidates.read()).map(c => c.id))
  if (!noticedIds(rows).some(id => !known.has(id))) return
  const now = await m.clock.now()
  if (m.rt.cli === undefined || now - m.rt.lastCandidates < REFRESH_MS) return
  m.rt.lastCandidates = now
  const status = await read(m.run, m.rt.cli, ['status', '--json'], await m.session.cwd())
  if (!status.ok) return
  await m.state.candidates.update(() => candidatesOf(status.json))
  m.ui.invalidate()
}
