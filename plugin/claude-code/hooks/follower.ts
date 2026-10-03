// The follower (docs/claude-mod/architecture.md §4.1; plan B3; ux-design §9.4): one child, `adage
// watch --follow --json --repo <cwd> --session <id>`, started when an interactive session starts,
// for the session's life. A child that ends is started again `--since` the last cursor, so nothing
// is lost or repeated across a restart, and after three children in a row end early the feed says
// it stopped. What a chunk of lines says (feed.ts reads them) reaches `$.state` here: the rows,
// ticks, health and learner, the toasts the learn lines earn and the avoid markers the judgements
// earn. A pause ends the child; a resume starts one from the cursor.
import type { ProcessSpawnChunk } from 'claude-code'

import { fold, followArgs, lastCursor, parseLine, splitLines, stem } from './feed'
import type { Line } from './feed'
import { addMarks, batchAt } from './marks'
import type { Mod } from './mod'
import { quietly } from './mod'
import { toastOnce } from './notify'
import type { Learner, Mark, Row } from './plugin'
import { refresh } from './refresh'
import { learnToast } from './view/lines'
import { reflexOf } from './view/text'

/** After this many children in a row end early, the feed stops (ux-design §9.4). */
const MAX_FAILURES = 3
/** A child that lived this long ended late, not early: the count starts again. */
export const HEALTHY_MS = 60_000

/** The reconnect rule as a pure step: after a child ended, how the feed stands. */
export function afterEnd(failures: number, livedMs: number): { failures: number; stop: boolean } {
  const next = livedMs >= HEALTHY_MS ? 1 : failures + 1
  return { failures: next, stop: next >= MAX_FAILURES }
}

export type FollowDeps = {
  /** The guarded `$.process.spawn` for one argv. */
  spawn: (argv: string[]) => AsyncGenerator<ProcessSpawnChunk, unknown>
  now: () => Promise<number>
  /** The argv for a child resuming from `since` (`followArgs` bound to the session). */
  argv: (since: string) => string[]
  /** Hands the parsed lines of one chunk over; resolves once they are in `$.state`. */
  apply: (lines: Line[], cursor: string) => Promise<void>
  /** Says how the feed stands now. */
  status: (status: 'following' | 'reconnecting' | 'stopped' | 'paused', failures: number) => Promise<void>
  /** True while the person paused the feed (`/adage pause`, `p` in the pane): the child ends. */
  isPaused: () => boolean
  /** Parses one line (`parseLine` bound to this session's stem). */
  parse: (text: string) => Line | undefined
}

/**
 * The follower's life: start a child, hand over its lines, start it again `--since` the last
 * cursor when it ends, and stop after `MAX_FAILURES` early ends in a row. A pause ends the child;
 * `follow` returns `'paused'` with the cursor, and a resume is a new `follow` from it.
 */
export async function follow(deps: FollowDeps, since: string, failures: number): Promise<{ end: 'paused' | 'stopped'; cursor: string }> {
  let cursor = since
  for (;;) {
    if (deps.isPaused()) {
      await deps.status('paused', failures)
      return { end: 'paused', cursor }
    }
    await deps.status(failures === 0 ? 'following' : 'reconnecting', failures)
    const started = await deps.now()
    let buffer = ''
    try {
      const child = deps.spawn(deps.argv(cursor))
      for await (const chunk of child) {
        if (chunk.stream !== 'stdout') continue
        const split = splitLines(buffer, chunk.text)
        buffer = split.rest
        const lines = split.lines.map(deps.parse).filter((l): l is Line => l !== undefined)
        if (lines.length > 0) {
          cursor = lastCursor(lines, cursor)
          await deps.apply(lines, cursor)
        }
        if (deps.isPaused()) {
          await child.return(undefined)
          break
        }
      }
    } catch {
      // It could not start, or ended badly: one failure, as an early end is.
    }
    if (deps.isPaused()) continue
    const ended = afterEnd(failures, (await deps.now()) - started)
    failures = ended.failures
    if (ended.stop) {
      await deps.status('stopped', failures)
      return { end: 'stopped', cursor }
    }
  }
}

/** Starts (or restarts) the follower for this session, from the last cursor; a person must be at the prompt. */
export async function startFeed(m: Mod, session: string, cwd: string): Promise<void> {
  if (m.rt.cli === undefined || !m.rt.interactive) return
  const gen = ++m.rt.feedGen
  m.rt.feedSession = session
  const exe = m.rt.cli
  const mine = stem(session)
  const live = () => gen === m.rt.feedGen
  const was = await m.state.feed.read()
  const since = (await m.clock.now()) - 5000
  quietly(follow(
    {
      spawn: argv => {
        const stream = m.spawn({ argv, cwd })
        m.rt.child = stream
        return stream
      },
      now: () => m.clock.now(),
      argv: since => followArgs(exe, cwd, session, since),
      parse: text => parseLine(text, mine),
      isPaused: () => m.rt.paused || !live(),
      apply: async (lines, cursor) => {
        if (!live()) return
        await apply(m, lines, mine, since)
        await m.state.feed.update(f => ({ ...f, cursor }))
      },
      status: async (status, failures) => {
        if (live()) await m.state.feed.update(f => ({ ...f, status, failures }))
      },
    },
    was.cursor,
    0,
  ))
}

/**
 * Ends the follower's child (a pause, a new session); the loop then says how it stands. Not
 * awaited: the child ends when the engine ends it, and nothing waits on a child to go.
 */
export function stopFeed(m: Mod): void {
  const c = m.rt.child
  m.rt.child = undefined
  void c?.return(undefined).catch(() => undefined)
}

/** Pauses or resumes the feed and the band (`/adage pause`, `p` in the pane); true when now paused. */
export async function togglePause(m: Mod): Promise<boolean> {
  const view = await m.state.view.update(v => ({ ...v, paused: !v.paused }))
  m.rt.paused = view.paused
  if (m.rt.paused) stopFeed(m)
  else if (m.rt.feedSession !== '') await startFeed(m, m.rt.feedSession, await m.session.cwd())
  return m.rt.paused
}

/** What a chunk of feed lines changes: rows, ticks, health, learner; the toasts and marks they earn. */
async function apply(m: Mod, lines: Line[], mine: string, since: number): Promise<void> {
  const now = await m.clock.now()
  const was = { rows: await m.state.rows.read(), ticks: await m.state.ticks.read(), health: await m.state.health.read(), learner: await m.state.learner.read() }
  const next = fold(was, lines, now)
  if (next.rows !== was.rows) await m.state.rows.update(() => next.rows)
  if (next.ticks !== was.ticks) await m.state.ticks.update(() => next.ticks)
  if (next.health !== was.health) await m.state.health.update(() => next.health)
  if (next.learner !== was.learner) {
    await m.state.learner.update(() => next.learner)
    timers(m, next.learner)
  }
  const ascii = (await m.state.mode.read()).ascii
  const reflexes = await m.state.reflexes.read()
  for (const l of lines) {
    // A learn line from before this follower started is history (the backfill), not news.
    if (l.kind !== 'learn' || l.ts < since) continue
    const toast = learnToast(l.line, mine, ascii, route => reflexOf(reflexes, route)?.gist)
    if (toast === undefined) continue
    if (l.line.kind === 'condition_error') await toastOnce(m, 'learn-failed', toast)
    else if (m.rt.interactive) m.ui.toast(toast)
  }
  if (lines.some(l => l.kind === 'learn')) quietly(refresh(m))
  const decisions = lines.filter(l => l.kind === 'decision').length
  if (decisions > 0) await avoided(m, next.rows.slice(-decisions), next.rows)
}

/**
 * The avoid marker (`tool.avoided`): a dead end this session was told about (an inhibit directive)
 * and then followed, judged at a tool batch; drawn under that batch's last call, matched by time.
 */
async function avoided(m: Mod, added: readonly Row[], rows: readonly Row[]): Promise<void> {
  const inhibit = new Set(rows.filter(r => r.mine).flatMap(r => r.acts.filter(a => a.act === 'directed' && a.strength === 'inhibit').map(a => a.route ?? '')))
  const hits = added.filter(r => r.mine).flatMap(r => r.acts.filter(a => a.act === 'followed' && inhibit.has(a.route ?? '')).map(a => ({ route: a.route ?? '', ts: r.ts })))
  if (hits.length === 0) return
  const batches = await m.state.batches.read()
  const marks: Mark[] = []
  for (const h of hits) {
    const b = batchAt(batches, h.ts)
    const id = b?.ids[b.ids.length - 1]
    if (id !== undefined) marks.push({ id, kind: 'avoided', route: h.route })
  }
  if (marks.length > 0) await addMarks(m, marks)
}

/** The learner's countdown redraws each second while it waits; `learned` fades after 10 s. */
function timers(m: Mod, learner: Learner | null): void {
  if (learner?.state === 'waiting') {
    m.rt.countdown ??= m.clock.every(1000, () => m.ui.invalidate())
  } else {
    m.rt.countdown?.cancel()
    m.rt.countdown = undefined
  }
  if (learner?.ranAt != null) {
    m.rt.fading?.cancel()
    m.rt.fading = m.clock.after(10_500, () => m.ui.invalidate())
  }
}
