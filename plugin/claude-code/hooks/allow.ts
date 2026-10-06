// The execution grant's two acts (plan B4; architecture §3.12 "Stage, then confirm"; ux-design
// §4.17 `ask.allow.pane`): the first answer stages the action (`--stage --json`, recording nothing)
// and opens the grant pane, focused, on the facts; `allow` there confirms against the digest the
// stage produced. Both are presses. grant.ts reads and words what the binary answered;
// draw/allow.tsx draws the pane.
import { copy } from './actions'
import type { AllowActions } from './draw/allow'
import { confirmCommand, confirmOf, routeOf, stageOf } from './grant'
import { noteAnswer } from './kept'
import type { Mod } from './mod'
import { quietly } from './mod'
import type { Allow } from './plugin'
import { ALLOW_PANE } from './plugin'
import { BUTTON, objectOf } from './press'
import type { Gesture } from './press'
import { refresh } from './refresh'
import { held, pressCommand } from './token'
import type { Waiting } from './view/pane'

/** The note above the grant pane's facts when the repository moved between the two acts. */
const MOVED = 'the repository moved meanwhile; this is what it would do now'
/** The note when no press is held: the stage and the confirm cannot be kept for later. */
const NO_PRESS = 'no press is held in this session; the command below runs it at a terminal'

/**
 * The first act of an execution grant: stage it (`--stage --json`, recording nothing) and open the
 * grant pane, focused, on the facts. With no press held the pane shows the terminal command.
 */
export async function allowYes(m: Mod, item: Waiting, gesture: Gesture): Promise<void> {
  const where = routeOf(item.command)
  const allow: Allow = { key: item.key, say: item.say, kind: 'execute', track: where?.track ?? '', route: where?.route ?? '', command: item.command, stage: null, details: false, note: '', busy: true }
  await m.state.allow.update(() => allow)
  await m.ui.open({ id: ALLOW_PANE, title: 'adage · allow', focus: true, closeOnEscape: true, holdToasts: true, rows: 20 }).catch(() => undefined)
  const same = (cur: Allow | null): cur is Allow => cur !== null && cur.key === item.key
  if (!(await held(m))) {
    await m.state.allow.update(cur => (same(cur) ? { ...cur, busy: false, note: NO_PRESS, details: true } : cur))
    return
  }
  const pressed = await pressCommand(m, gesture, item.command, item.say, item.key, 'stage', undefined, false)
  const stage = pressed?.ok ? stageOf(objectOf(pressed.lines)) : undefined
  const note =
    pressed === undefined ? NO_PRESS
    : !pressed.ok ? `not staged: ${pressed.error === '' ? 'the command did not succeed' : pressed.error}`
    : stage === undefined ? 'the binary answered nothing the pane can show; the command below runs it at a terminal'
    : stage.ok ? ''
    : stage.why
  await m.state.allow.update(cur => (same(cur) ? { ...cur, stage: stage ?? null, busy: false, note, details: stage === undefined } : cur))
}

/**
 * The second act: `--confirm` against the digest the stage produced. Recorded: the toast and the
 * pane closes; the repository moved: the fresh diff, asked again; refused: the binary's reason.
 */
async function confirmAllow(m: Mod): Promise<void> {
  const a = await m.state.allow.read()
  if (a === null || a.stage === null || !a.stage.ok || a.busy) return
  const same = (cur: Allow | null): cur is Allow => cur !== null && cur.key === a.key
  await m.state.allow.update(cur => (same(cur) ? { ...cur, busy: true, note: '' } : cur))
  if (!(await held(m))) {
    await m.state.allow.update(cur => (same(cur) ? { ...cur, busy: false, note: NO_PRESS, details: true } : cur))
    return
  }
  const pressed = await pressCommand(m, BUTTON, confirmCommand(a.command), a.say, a.key, 'confirm', { expect: a.stage.digest }, false)
  const c = pressed?.ok ? confirmOf(objectOf(pressed.lines)) : undefined
  if (c?.recorded === true) {
    await noteAnswer(m, a.key, 'confirm')
    if (m.rt.interactive) m.ui.toast(`allowed: ${a.say} · from your next session`)
    await closeAllow(m)
    quietly(refresh(m, true))
    return
  }
  const note =
    pressed === undefined ? NO_PRESS
    : !pressed.ok ? `not recorded: ${pressed.error === '' ? 'the command did not succeed' : pressed.error}`
    : c?.recorded === false && c.stage !== undefined ? (c.stage.ok ? MOVED : c.stage.why)
    : 'not recorded: the binary answered nothing the pane can show'
  await m.state.allow.update(cur => (same(cur) ? { ...cur, busy: false, note, stage: c?.recorded === false && c.stage !== undefined ? c.stage : cur.stage } : cur))
}

/** `not now`, or the grant recorded: the pane goes; nothing else runs. */
async function closeAllow(m: Mod): Promise<void> {
  await m.state.allow.update(() => null)
  await m.ui.close({ id: ALLOW_PANE }).catch(() => undefined)
}

/** The grant pane's handlers (draw/allow.tsx calls them from its Buttons). */
export function allowActions(m: Mod): AllowActions {
  return {
    allow: () => quietly(confirmAllow(m)),
    notNow: () => quietly(closeAllow(m)),
    details: () => quietly(m.state.allow.update(cur => (cur === null ? cur : { ...cur, details: !cur.details }))),
    copy: surface => {
      quietly((async () => {
        const a = await m.state.allow.read()
        if (a !== null) await copy(m, a.command, surface)
      })())
    },
  }
}
