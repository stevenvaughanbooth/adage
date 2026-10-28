// The session's life (plan B1–B3; ux-design §4.9, §9.1, §9.6): at session.start the mod probes the
// hook binary the shim linked (adage-exec while execution is on, else adage) and, while none is
// there (the link is scripts/quiet), stays inert: every hook answers what next(e) answered,
// unchanged, and nothing is drawn but the band's one line saying why. The probe runs again once the
// SessionStart command hooks ran (the shim makes the link there), and a new session id there is a
// /clear: the session's counts start again and the follower follows the new id.
import { standing } from './attach'
import { dataDir } from './binary'
import { cliBinary, exeSuffix, hookBinary, joinPath, probe } from './bridge'
import { ARGUMENT_HINT, DESCRIPTION } from './commands'
import { startFeed, stopFeed } from './follower'
import type { AnswerAt, EventAt, Mod, NextAt } from './mod'
import { quietly } from './mod'
import { warnOnce } from './notify'
import type { View } from './plugin'
import { COMMAND, PANE, TESTED, TURN0 } from './plugin'
import { refresh } from './refresh'
import { versionLine } from './view/band'
import { startWorldFeed, stopWorldFeed } from './world/follow'
import { startWorld, stopWorld } from './world/poll'

/** Probes the binary the shim linked; the mod is active only while it answers. */
async function activate(m: Mod): Promise<void> {
  m.rt.observing = ((await m.env.observe()) ?? '') !== ''
  const ascii = (await m.env.ascii()) === '1'
  const reducedMotion = (await m.env.reducedMotion()) === '1'
  m.rt.exe = exeSuffix(await m.env.os())
  const data = await dataDir(m)
  m.rt.bin = await probe(m.run, hookBinary(data, m.rt.exe))
  m.rt.cli = m.rt.bin === undefined ? undefined : cliBinary(data, m.rt.exe)
  let nobinary = ''
  if (m.rt.bin === undefined && data !== undefined) {
    const said = await m.fs.read(joinPath(data, 'status')).catch(() => '')
    nobinary = typeof said === 'string' ? said.trim() : ''
  }
  const was = await m.state.mode.read()
  const active = m.rt.bin !== undefined
  const { interactive, surface } = m.rt
  if (was.active !== active || was.interactive !== interactive || was.surface !== surface || was.nobinary !== nobinary || was.ascii !== ascii || was.reducedMotion !== reducedMotion) {
    await m.state.mode.update(x => ({ ...x, active, interactive, surface, nobinary, ascii, reducedMotion }))
  }
  if (nobinary !== '') await warnOnce(m, 'nobinary', nobinary)
  if (m.rt.bin === undefined) return
  await m.command.register({ name: COMMAND, description: DESCRIPTION, argumentHint: ARGUMENT_HINT, immediate: true }).catch(() => undefined)
  if (!m.rt.interactive) return
  await harnessVersion(m, ascii).catch(() => undefined)
  const view = await m.state.view.read()
  m.rt.paused = view.paused
  const kept = await m.store.tab.get().catch(() => undefined)
  const tab: View['tab'] | undefined = kept === 'graph' || kept === 'live' || kept === 'reflexes' || kept === 'health' || kept === 'world' ? kept : undefined
  if (tab !== undefined && tab !== view.tab) await m.state.view.update(v => ({ ...v, tab }))
}

/** `band.version` (ux-design §9.6): once per new harness version per machine, until the first prompt. */
async function harnessVersion(m: Mod, ascii: boolean): Promise<void> {
  const { version } = await m.session.version()
  const seen = await m.store.harness.get()
  const line = seen === version ? '' : versionLine(version, TESTED, ascii)
  if ((await m.state.mode.read()).versionLine !== line) await m.state.mode.update(x => ({ ...x, versionLine: line }))
}

/** Starts the per-session counts again (a /clear is a new engine session, ux-design §4.9). */
async function newSession(m: Mod): Promise<void> {
  await m.state.fired.update(() => [])
  await m.state.predicted.update(() => [])
  await m.state.marks.update(() => [])
  await m.state.batches.update(() => [])
  await m.state.dag.update(() => [])
  await m.state.turn.update(() => TURN0)
  await m.state.rows.update(rows => rows.map(r => ({ ...r, mine: false })))
  await m.state.world.update(w => ({ ...w, hints: [] }))
  m.rt.failures = 0
  await m.state.mode.update(x => ({ ...x, warned: [] }))
}

/**
 * `session.start`: activation, the standing text asked off the hook's path (so the first request's
 * section and tool description find it), and for a person at the prompt the follower, the reads,
 * and the pane unasked with `pane: auto` (the engine seats an unasked pane from 144 columns, §6).
 */
export async function onStart(m: Mod, e: EventAt<'session.start'>, next: NextAt<'session.start'>): AnswerAt<'session.start'> {
  const started = await next(e)
  m.rt.interactive = e.isInteractive
  m.rt.surface = e.surface
  await activate(m)
  if (m.rt.bin !== undefined) quietly(standing(m))
  if (m.rt.bin !== undefined && m.rt.interactive) {
    const session = await m.session.id()
    await startFeed(m, session, e.cwd)
    quietly(refresh(m, true).then(() => world(m, session, e.cwd)))
    if (m.options.pane === 'auto') quietly(m.ui.open({ id: PANE, title: 'adage' }))
  }
  return started
}

/**
 * The world for a session, once the refresh read whether it is on: its connector sources through
 * this session's MCP servers (world/poll.ts), and its follow child (world/follow.ts).
 */
async function world(m: Mod, session: string, cwd: string): Promise<void> {
  await startWorld(m)
  await startWorldFeed(m, session, cwd)
}

/**
 * `classic.SessionStart`: the link is made by the shim, the SessionStart command hook beneath this
 * observer, so the binary is probed again once it has run: a first session (no link yet at
 * session.start) and a /clear (the shim relinks) see the binary the shim chose. A /clear is a new
 * session: its counts start again and the follower follows the new id.
 */
export async function onShimStart(m: Mod, e: EventAt<'classic.SessionStart'>, next: NextAt<'classic.SessionStart'>): AnswerAt<'classic.SessionStart'> {
  const answered = await next(e)
  await activate(m)
  if (m.rt.bin !== undefined) quietly(standing(m))
  if (m.rt.bin !== undefined && m.rt.interactive) {
    const session = await m.session.id()
    if (session !== m.rt.feedSession) {
      if (m.rt.feedSession !== '') {
        stopFeed(m)
        stopWorld(m)
        stopWorldFeed(m)
        await newSession(m)
      }
      const cwd = await m.session.cwd()
      await startFeed(m, session, cwd)
      quietly(refresh(m, true).then(() => world(m, session, cwd)))
    }
  }
  return answered
}
