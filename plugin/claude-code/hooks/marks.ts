// What is kept for drawing beside the transcript's rows (ux-design §4.2–§4.4): the reflexes a
// command hook delivered (`fired`), the steps Adage made in the model's place (`predicted`), one
// marker per tool call and kind (`marks`) and the tool batches the command hooks saw (`batches`),
// each capped so a long session keeps its latest.
import type { Mod } from './mod'
import type { Batch, Fired, Mark, Predicted } from './plugin'

/** The most marks, fired reflexes and predicted steps kept for drawing in one session. */
export const MAX_MARKS = 500
/** The most tool batches kept for matching a feed judgement to its row. */
export const MAX_BATCHES = 50

/** Keeps a marker under a call's row; a second of the same kind for the same call replaces the first. */
export async function mark(m: Mod, x: Mark): Promise<void> {
  await m.state.marks.update(cur => [...cur.filter(y => !(y.id === x.id && y.kind === x.kind)), x].slice(-MAX_MARKS))
}

/** Keeps several markers at once (the avoid markers of one feed chunk), in order. */
export async function addMarks(m: Mod, xs: readonly Mark[]): Promise<void> {
  await m.state.marks.update(cur => [...cur, ...xs].slice(-MAX_MARKS))
}

/** Keeps a step Adage made in the model's place, for its row's marker and the counts. */
export async function record(m: Mod, p: Predicted): Promise<void> {
  await m.state.predicted.update(cur => [...cur, p].slice(-MAX_MARKS))
}

/** Keeps the reflexes a command hook delivered. */
export async function keepFired(m: Mod, fired: readonly Fired[]): Promise<void> {
  await m.state.fired.update(cur => [...cur, ...fired].slice(-MAX_MARKS))
}

/**
 * The tool batch a feed record judged at `ts` belongs to, by when the command hooks ran (a record
 * is spooled within the hooks' run, give or take the clocks' skew): the latest that fits.
 */
export function batchAt(batches: readonly Batch[], ts: number): Batch | undefined {
  return [...batches].reverse().find(x => ts >= x.from - 50 && ts <= x.to + 2000)
}

/** Keeps one tool batch: its calls and when the hooks beneath ran. */
export async function keepBatch(m: Mod, b: Batch): Promise<void> {
  await m.state.batches.update(cur => [...cur, b].slice(-MAX_BATCHES))
}
