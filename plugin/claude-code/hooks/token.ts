// The session's press token (plan B4; architecture §3.12 "Hold"): issued by the binary once the
// SessionStart command hooks ran (`hook claude Press`, before the agent's first act), held in
// `$.state` only, spent by each press, refused or tampered with. press.ts runs a press; this file
// holds the token around it. With no press held an answer is kept for the next session start
// (kept.ts), and nothing runs.
import { doneToast } from './ask'
import { dataDir } from './binary'
import { ask, exeSuffix, hookBinary } from './bridge'
import { keep, noteAnswer, repoRoot } from './kept'
import type { Mod } from './mod'
import { quietly } from './mod'
import { warnOnce } from './notify'
import type { Press } from './plugin'
import { isToken, press, pressArgv, tampered } from './press'
import type { Gesture, Pressed } from './press'
import { refresh } from './refresh'
import { isRecord } from './shape'
import { VERB_WORDS } from './view/pane'

/** `Press` is asked off the hot path, with a command hook's budget. */
const ISSUE_TIMEOUT_MS = 2000

/** What a session start's command hooks were handed: its id, how it started, its directory. */
export type Started = { session_id?: string; source?: string; cwd?: string }

/** The token in a `Press` answer (`{ press }`, docs/claude-mod/impl/wire.md), or '' for none or a malformed one. */
export function pressOf(answer: unknown): string {
  return isRecord(answer) && isToken(answer.press) ? answer.press : ''
}

/**
 * Asks the binary for the session's press once the command hooks' session start has run (the
 * window is open until the agent's first act), and holds the token in `$.state` only. A start
 * the shim linked during this very hook is asked at the link, before the activation sees it.
 * Resolves whether a press is held now; a compaction asks nothing.
 */
export async function issue(m: Mod, e: Started): Promise<boolean> {
  const source = typeof e.source === 'string' ? e.source : 'startup'
  if (source === 'compact') return false
  const session = typeof e.session_id === 'string' && e.session_id !== '' ? e.session_id : await m.session.id()
  const was = await m.state.press.read()
  const same = was.session === session
  const hook = m.rt.bin ?? hookBinary(await dataDir(m), exeSuffix(await m.env.os()))
  const cwd = typeof e.cwd === 'string' && e.cwd !== '' ? e.cwd : await m.session.cwd()
  const asked = await ask(m.run, hook, 'Press', { session_id: session, cwd, hook_event_name: 'Press', source, via: 'mod' }, ISSUE_TIMEOUT_MS)
  const token = asked.ok ? pressOf(asked.answer) : ''
  // A resume reissues once for the same id: the spool then holds two issues at most. A resume
  // this process got nothing for (a second resume, after the binary's one reissue) counts the
  // same two, so the spool's own two issues are not taken for tampering.
  const issued = token === '' ? (same ? was.issued : source === 'resume' ? 2 : 0) : source === 'resume' ? 2 : (same ? was.issued : 0) + 1
  const next: Press =
    token !== ''
      ? { session, token, state: same && was.state === 'off' ? 'off' : 'held', issued, chain: same ? was.chain : 0 }
      : { session, token: same ? was.token : '', state: same ? was.state : 'none', issued, chain: same ? was.chain : 0 }
  await m.state.press.update(() => next)
  return next.state === 'held'
}

/**
 * The tamper signal (§3.12 "Hold"): more `press_issued` records for this session than tokens the
 * mod was handed means something else claimed the window; presses stop for the session.
 */
export async function checkTamper(m: Mod): Promise<void> {
  const p = await m.state.press.read()
  if (p.state === 'off' || p.session === '') return
  const seen = (await m.state.rows.read()).filter(r => r.mine && r.event === 'press_issued').length
  if (!tampered(seen, p.issued)) return
  await m.state.press.update(cur => ({ ...cur, state: 'off' as const, token: '' }))
  await warnOnce(m, 'press-tampered', "something else claimed this session's press; presses are off for this session · run adage doctor")
}

/** Whether a press is held for this session now. */
export async function held(m: Mod): Promise<boolean> {
  const p = await m.state.press.read()
  return p.state === 'held' && p.token !== '' && p.session === (await m.session.id())
}

/**
 * Runs one person-only command through the press (press.ts): the one path that reaches a verb a
 * person alone may run, from a dialog answer or a pane Button in this session. With no
 * press held the answer is kept for the next session start when it can be (one act), and nothing
 * runs. Resolves to what the press came to, or undefined when nothing ran.
 */
export async function pressCommand(
  m: Mod,
  gesture: Gesture,
  command: string,
  say: string,
  key: string,
  what: string,
  extra?: Readonly<Record<string, unknown>>,
  keepable = true,
): Promise<Pressed | undefined> {
  if (m.rt.cli === undefined) return undefined
  const argv = pressArgv(m.rt.cli, command)
  if (argv === undefined) return undefined
  await checkTamper(m)
  const p = await m.state.press.read()
  const session = await m.session.id()
  if (p.state !== 'held' || p.token === '' || p.session !== session) {
    if (keepable) {
      await keep(m, { command, say, key, session, root: await repoRoot(m), at: await m.clock.now() })
      await noteAnswer(m, key, what)
    }
    return undefined
  }
  const pressed = await press(gesture, m.pressRun, argv, p.token, p.session, await m.session.cwd(), extra)
  // Refused before the spend (no next line: a wrong, spent or stale token, or a verb that had
  // nothing to do, `adage on` where it is on): nothing was spent, and the token stays held for the
  // person's next answer. Otherwise the token went with the verb, and the next one (if the binary
  // printed it) is the one held.
  if (pressed.ok || !pressed.refused) {
    await m.state.press.update(cur => ({
      ...cur,
      token: pressed.next ?? '',
      state: pressed.next !== undefined ? ('held' as const) : ('none' as const),
      chain: cur.chain + 1,
    }))
  }
  if (pressed.ok) {
    if (key !== '') await noteAnswer(m, key, what)
    const toast = doneToast(what, say, VERB_WORDS[what])
    if (toast !== undefined && m.rt.interactive) m.ui.toast(toast)
    if (what !== 'stage' && what !== 'confirm') quietly(refresh(m, true))
  } else if (m.rt.interactive && what !== 'stage' && what !== 'confirm') {
    m.ui.toast(`not recorded: ${pressed.error === '' ? 'the command did not succeed' : pressed.error}`)
  }
  return pressed
}
