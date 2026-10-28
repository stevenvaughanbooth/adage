// The connector sources' polling (the implementation plan's W3): at a session's start the
// descriptors are read from the binary and resolved against the connected MCP servers
// (sources.ts); each source then runs on its own timer, calling the server through the engine's
// own connection and credentials (`$.mcp.call`: Adage holds no token), mapping the result
// (map.ts) and piping it to `adage world ingest`, which checks every field. A failed call backs
// off (1, 2, 4, 8 minutes, then once an hour) and is reported to the source's health through
// `ingest --error`. Every timer ends with the session, and the person's `adage world pause` is
// read by the binary, not here.
import { read } from '../bridge'
import type { AnswerAt, EventAt, Mod, NextAt } from '../mod'
import { quietly } from '../mod'
import { stopWorldFeed, withTitles } from './follow'
import { fillArgs, mapped } from './map'
import type { Batch } from './map'
import { descriptorsOf, resolveSources } from './sources'
import type { Resolved } from './sources'

/** The backoff after each failure in a row, in seconds; the last repeats. */
export const BACKOFF_S = [60, 120, 240, 480, 3600] as const

/** After this many failures in a row, how long until the next try. */
export function backoff(failures: number): number {
  return BACKOFF_S[Math.min(Math.max(failures, 1), BACKOFF_S.length) - 1] ?? 3600
}

/** One source's run state. */
export type Running = { resolved: Resolved; failures: number; timer: { cancel: () => void } | undefined; polls: number }

/** `{ "written": … }` or `{ "error": … }` as `ingest` prints it; nothing when it did not run. */
export type Ingested = { ok: boolean; written: number; problems: string[] }

export type PollDeps = {
  /** `$.mcp.call` as the Mod hands it over. */
  call: (server: string, tool: string, args: Record<string, unknown>) => Promise<unknown>
  /** Pipes a batch to `adage world ingest`; resolves what it said. */
  ingest: (batch: Batch) => Promise<Ingested | undefined>
  /** Reports a failed poll to the source's health. */
  failed: (source: string, server: string, error: string) => Promise<void>
  /** Keeps the things' titles a poll ingested, for the pane's timeline. */
  titled: (named: readonly { id: string; title: string }[]) => Promise<void>
  now: () => Promise<number>
}

/** What one poll came to: whether it went through, how many events it wrote, and the error when not. */
export type Polled = { ok: boolean; written: number; error: string }

/** One poll of one source: the call, the map, the ingest. */
export async function pollOnce(deps: PollDeps, r: Resolved): Promise<Polled> {
  const now = await deps.now()
  let result: unknown
  try {
    const args = fillArgs(r.provider.args, r.descriptor, now)
    result = await deps.call(r.server, r.tool, typeof args === 'object' && args !== null && !Array.isArray(args) ? (args as Record<string, unknown>) : {})
  } catch (e) {
    const error = `call failed: ${e instanceof Error ? e.message : String(e)}`.slice(0, 200)
    await deps.failed(r.descriptor.name, r.server, error)
    return { ok: false, written: 0, error }
  }
  const isError = typeof result === 'object' && result !== null && (result as { isError?: unknown }).isError === true
  if (isError) {
    const error = 'the server reported an error'
    await deps.failed(r.descriptor.name, r.server, error)
    return { ok: false, written: 0, error }
  }
  const batch = mapped(r.descriptor, r.provider, result, now, r.server)
  const said = await deps.ingest(batch)
  if (said?.ok !== true) return { ok: false, written: 0, error: said?.problems[0] ?? 'adage world ingest did not answer' }
  await deps.titled(batch.artifacts.map(a => ({ id: a.id, title: a.title })))
  return { ok: true, written: said.written, error: '' }
}

/** The deps over a `Mod`. */
export function pollDeps(m: Mod): PollDeps {
  const run = async (argv: string[], stdin: string): Promise<Ingested | undefined> => {
    if (m.rt.cli === undefined) return undefined
    const cwd = await m.session.cwd()
    try {
      const ran = await m.run([m.rt.cli, ...argv], { cwd, stdin, timeoutMs: 10_000 })
      if (ran.exitCode !== 0) return { ok: false, written: 0, problems: [ran.stderr.trim().slice(0, 200)] }
      const v: unknown = JSON.parse(ran.stdout.trim() || '{}')
      const o = typeof v === 'object' && v !== null ? (v as Record<string, unknown>) : {}
      return { ok: true, written: typeof o.events === 'number' ? o.events : 0, problems: Array.isArray(o.problems) ? o.problems.map(String) : [] }
    } catch {
      return undefined
    }
  }
  return {
    call: (server, tool, args) => m.mcp.call(server, tool, args),
    ingest: batch => run(['world', 'ingest', '--source', batch.source], JSON.stringify(batch)),
    failed: async (source, server, error) => {
      await run(['world', 'ingest', '--source', source, '--error', error], JSON.stringify({ source, via: 'mod', server }))
    },
    titled: async named => {
      if (named.length > 0) await m.state.world.update(w => ({ ...w, titles: withTitles(w.titles, named) }))
    },
    now: () => m.clock.now(),
  }
}

/** Reads the descriptors and resolves them against the connected servers; empty when the binary cannot say. */
export async function resolve(m: Mod): Promise<Resolved[]> {
  if (m.rt.cli === undefined) return []
  const said = await read(m.run, m.rt.cli, ['world', 'descriptors', '--json'], await m.session.cwd())
  if (!said.ok) return []
  const tools = await m.tool.list().catch(() => [])
  return resolveSources(descriptorsOf(said.json), tools)
}

/**
 * Starts every connector source this session can run, each on its own timer from the first poll,
 * with its backoff; nothing while the world is off (the binary says) or the mod inert.
 */
export async function startWorld(m: Mod): Promise<void> {
  stopWorld(m)
  if (m.rt.cli === undefined) return
  const world = await m.state.world.read()
  if (!world.enabled) return
  const resolved = await resolve(m)
  const deps = pollDeps(m)
  const gen = ++m.rt.worldGen
  for (const r of resolved) {
    const run: Running = { resolved: r, failures: 0, timer: undefined, polls: 0 }
    m.rt.worldRuns.push(run)
    const tick = async (): Promise<void> => {
      if (gen !== m.rt.worldGen) return
      run.polls++
      const polled = await pollOnce(deps, r).catch((e: unknown) => ({ ok: false, written: 0, error: e instanceof Error ? e.message : 'failed' }))
      run.failures = polled.ok ? 0 : run.failures + 1
      const next = polled.ok ? r.descriptor.every : backoff(run.failures)
      if (gen !== m.rt.worldGen) return
      run.timer = m.clock.after(next * 1000, () => quietly(tick()))
      // The source's lamp in the pane, until the follow child's health line says the same.
      const now = await m.clock.now()
      await m.state.world.update(w => ({
        ...w,
        sources: w.sources.map(s => (s.name === r.descriptor.name ? { ...s, lastPoll: now, ok: polled.ok, error: polled.error, events: s.events + polled.written } : s)),
      }))
      if (m.rt.interactive) m.ui.invalidate()
    }
    quietly(tick())
  }
  await m.state.world.update(w => ({ ...w, sources: resolved.map(r => ({ name: r.descriptor.name, server: r.server, lastPoll: 0, ok: true, error: '', events: 0 })), resolved: resolved.length }))
}

/** Ends every source's timer (a session end, a pause, a /clear). */
export function stopWorld(m: Mod): void {
  m.rt.worldGen++
  for (const r of m.rt.worldRuns) r.timer?.cancel()
  m.rt.worldRuns = []
}

/** `session.end`: every timer and the follow child end with the session (a /clear restarts them at the next prompt). */
export async function onEnd(m: Mod, e: EventAt<'session.end'>, next: NextAt<'session.end'>): AnswerAt<'session.end'> {
  stopWorld(m)
  stopWorldFeed(m)
  return next(e)
}
