// The world's follow child (the implementation plan's W2 child, W3's reader): one `adage world
// follow --session <id> --stdin ignore [--since <cursor>]` per interactive session, for its life,
// started as the watch feed's follower is (follower.ts `follow`: reconnect from the last cursor,
// stop after three early ends). The child runs the local sources (transcripts, files, the desktop)
// where it holds the world's lock, and prints the activity timeline and the sources' health as
// JSON lines; this module reads them into `$.state.world` for the World tab.
import type { ProcessSpawnChunk } from 'claude-code'

import { follow } from '../follower'
import type { Mod } from '../mod'
import { quietly } from '../mod'
import type { WorldEvent, WorldSource } from '../plugin'
import { MAX_WORLD_EVENTS } from '../plugin'
import { isRecord, num, str } from '../shape'

/** One line of the child. */
export type WorldLine =
  | { kind: 'event'; cursor: string; event: WorldEvent; title: string }
  | { kind: 'health'; cursor: string; sources: WorldSource[]; polling: boolean; live: boolean }

/** The child's argv: `--since` only once a cursor exists. */
export function worldFollowArgs(cli: string, session: string, since: string): string[] {
  const argv = [cli, 'world', 'follow', '--session', session, '--stdin', 'ignore']
  return since === '' ? argv : [...argv, '--since', since]
}

/** Parses one line; anything else is skipped. */
export function parseWorldLine(text: string): WorldLine | undefined {
  let v: unknown
  try {
    v = JSON.parse(text)
  } catch {
    return undefined
  }
  if (!isRecord(v)) return undefined
  const cursor = String(num(v.cursor))
  if (isRecord(v.event)) {
    const e = v.event
    const ts = num(e.ts)
    const source = str(e.source)
    const artifact = str(e.artifact)
    if (ts === 0 || source === '' || artifact === '') return undefined
    return { kind: 'event', cursor, event: { ts, source, kind: str(e.kind), artifact, action: str(e.action), detail: str(e.detail) }, title: str(v.title) }
  }
  if (isRecord(v.health)) {
    const sources = Object.values(isRecord(v.health.sources) ? v.health.sources : {})
      .filter(isRecord)
      .map(s => ({ name: str(s.name), server: str(s.server) || str(s.via), lastPoll: num(s.last_poll), ok: s.ok !== false, error: str(s.error), events: num(s.events) }))
      .filter(s => s.name !== '')
      .sort((a, b) => a.name.localeCompare(b.name))
    return { kind: 'health', cursor, sources, polling: v.polling === true, live: v.live === true }
  }
  return undefined
}

/** The last cursor among the lines, else the one before. */
export const lastWorldCursor = (lines: readonly WorldLine[], before: string): string => lines.at(-1)?.cursor ?? before

/** The most titles the World tab keeps by artifact id. */
export const MAX_TITLES = 500

/** `titles` with these added, the oldest dropped past the cap. */
export function withTitles(titles: Readonly<Record<string, string>>, add: readonly { id: string; title: string }[]): Record<string, string> {
  const named = add.filter(a => a.id !== '' && a.title !== '')
  if (named.length === 0) return { ...titles }
  const out = { ...titles }
  for (const a of named) {
    delete out[a.id]
    out[a.id] = a.title
  }
  const keys = Object.keys(out)
  for (const k of keys.slice(0, Math.max(0, keys.length - MAX_TITLES))) delete out[k]
  return out
}

/** What a chunk of lines changes: the events (newest first, capped), their titles and the sources. */
export async function applyWorld(m: Mod, lines: readonly WorldLine[]): Promise<void> {
  const evs = lines.filter((l): l is Extract<WorldLine, { kind: 'event' }> => l.kind === 'event')
  const events = evs.map(l => l.event)
  const health = lines.filter((l): l is Extract<WorldLine, { kind: 'health' }> => l.kind === 'health').at(-1)
  if (events.length === 0 && health === undefined) return
  await m.state.world.update(w => ({
    ...w,
    events: events.length === 0 ? w.events : [...events.reverse(), ...w.events].slice(0, MAX_WORLD_EVENTS),
    titles: withTitles(w.titles, evs.map(l => ({ id: l.event.artifact, title: l.title }))),
    ...(health === undefined ? {} : { sources: health.sources, polling: health.polling, live: health.live }),
  }))
  if (m.rt.interactive) m.ui.invalidate()
}

/** Starts (or restarts) the world's child for this session, from the last cursor; a person must be at the prompt. */
export async function startWorldFeed(m: Mod, session: string, cwd: string): Promise<void> {
  if (m.rt.cli === undefined || !m.rt.interactive) return
  const world = await m.state.world.read()
  if (!world.enabled) return
  const gen = ++m.rt.worldFeedGen
  const exe = m.rt.cli
  const live = () => gen === m.rt.worldFeedGen
  quietly(follow(
    {
      spawn: argv => {
        const stream = m.spawn({ argv, cwd }) as AsyncGenerator<ProcessSpawnChunk, unknown>
        m.rt.worldChild = stream
        return stream
      },
      now: () => m.clock.now(),
      argv: since => worldFollowArgs(exe, session, since),
      parse: text => parseWorldLine(text) as never,
      isPaused: () => !live(),
      apply: async (lines, cursor) => {
        if (!live()) return
        await applyWorld(m, lines as unknown as WorldLine[])
        await m.state.world.update(w => ({ ...w, cursor }))
      },
      status: async (status, failures) => {
        if (live()) await m.state.world.update(w => ({ ...w, status: status === 'paused' ? 'off' : status, failures }))
      },
    },
    world.cursor,
    0,
  ))
}

/** Ends the world's child (a session end, a /clear). */
export function stopWorldFeed(m: Mod): void {
  m.rt.worldFeedGen++
  const c = m.rt.worldChild
  m.rt.worldChild = undefined
  void c?.return(undefined).catch(() => undefined)
}
