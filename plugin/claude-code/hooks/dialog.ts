// Asking the person in the engine's own dialog (plan B4; architecture §3.12; ux-design §4.17, §7,
// §9.12): at a turn's end, when the model is idle, one waiting item (ask.ts picks it and words it),
// at most one per turn end and one per reflex per day; `/adage on`'s and `/adage update`'s asks;
// and the answers kept while no press was held, put to the person again at the next start in their
// repository. Each answer is a press (token.ts), and so is each press in the pane (`pressHandlers`).
import { openPane, showReflex } from './actions'
import { allowYes } from './allow'
import {
  KEPT_ANSWERS,
  ON_ANSWERS,
  ON_QUESTION,
  UPDATE_ANSWERS,
  answerOf,
  keptOf,
  keptQuestion,
  nextAsk,
  updateFailed,
  updateKey,
  updateQuestion,
  updateSay,
  updatedToast,
  whatOf,
} from './ask'
import type { AskItem } from './ask'
import type { PressHandlers } from './draw/pane'
import { keep, keptNow, loadStore, noteAnswer, repoRoot } from './kept'
import type { AnswerAt, EventAt, Mod, NextAt } from './mod'
import { quietly } from './mod'
import type { Kept } from './plugin'
import { KEPT_MAX_AGE_MS } from './plugin'
import { ANSWER, BUTTON, press, pressArgv } from './press'
import type { Gesture, Pressed } from './press'
import { recountNoticed } from './notice'
import type { Offer } from './reads'
import { updatedOf } from './reads'
import { refresh } from './refresh'
import { checkTamper, issue, pressCommand } from './token'
import type { Started } from './token'
import { reviewCommand, waiting } from './view/pane'
import type { Waiting } from './view/pane'

/** The pane's presses and `/adage on`, for the drawings and the command to call. */
export function pressHandlers(m: Mod): PressHandlers {
  return {
    verb: (verb, r) => quietly(pressCommand(m, BUTTON, reviewCommand(verb, r), r.gist || r.summary, `${r.reference}/${r.id}`, verb)),
    yes: item => quietly(answerYes(m, item, BUTTON)),
    no: item => quietly(answerNo(m, item, BUTTON)),
    askOn: () => quietly(askOn(m)),
    update: () =>
      quietly(
        (async () => {
          const u = (await m.state.mode.read()).update
          if (u !== null) await pressUpdate(m, BUTTON, u.latest)
        })(),
      ),
    askUpdate: u => quietly(askUpdate(m, u)),
  }
}

/** The one command a press runs to update Adage (press.ts `isPressAllowed`: bare, nothing else). */
const UPDATE = 'adage update'

/**
 * `adage update` through the press (onboarding plan O9): with no press held the answer is kept for
 * the next start in this repository (`keepable`), and nothing runs. The binary updates only a
 * channel whose update is a fetch, writes no settings file, and says what it came to on its last
 * line (reads.ts `updatedOf`), which is the toast; a failure after the spend names the terminal.
 * The token moves as token.ts `pressCommand` moves it: whatever the binary printed next is held.
 */
async function pressUpdate(m: Mod, gesture: Gesture, latest: string, keepable = true): Promise<Pressed | undefined> {
  if (m.rt.cli === undefined) return undefined
  const argv = pressArgv(m.rt.cli, UPDATE)
  if (argv === undefined) return undefined
  await checkTamper(m)
  const p = await m.state.press.read()
  const session = await m.session.id()
  if (p.state !== 'held' || p.token === '' || p.session !== session) {
    if (keepable) {
      await keep(m, { command: UPDATE, say: updateSay(latest), key: updateKey(latest), session, root: await repoRoot(m), at: await m.clock.now() })
      await noteAnswer(m, updateKey(latest), 'update')
    }
    return undefined
  }
  const pressed = await press(gesture, m.pressRun, argv, p.token, p.session, await m.session.cwd())
  if (pressed.ok || !pressed.refused) {
    await m.state.press.update(cur => ({
      ...cur,
      token: pressed.next ?? '',
      state: pressed.next !== undefined ? ('held' as const) : ('none' as const),
      chain: cur.chain + 1,
    }))
  }
  if (pressed.ok) {
    await noteAnswer(m, updateKey(latest), 'update')
    const said = updatedOf(pressed.lines)
    if (m.rt.interactive) m.ui.toast(said === undefined ? updateFailed('') : updatedToast(said))
    quietly(refresh(m, true))
  } else if (m.rt.interactive) {
    m.ui.toast(updateFailed(pressed.error))
  }
  return pressed
}

/**
 * `/adage update` (onboarding plan O9): the ask, opened only by the command (never unasked).
 * `update` is the press; `what changes` opens Health, where the version row holds the release's
 * first lines and the same press; `not now` records that it was asked.
 */
export async function askUpdate(m: Mod, u: Offer): Promise<void> {
  if (m.rt.asking) return
  m.rt.asking = true
  try {
    const said = answerOf({ answers: UPDATE_ANSWERS }, await m.ui.ask(updateQuestion(u), UPDATE_ANSWERS).catch(() => undefined))
    if (said === 'update') await pressUpdate(m, ANSWER, u.latest)
    else if (said === 'what changes') await openPane(m, 'health')
    else if (said === 'not now') await noteAnswer(m, updateKey(u.latest), 'not now')
  } finally {
    m.rt.asking = false
  }
}

/** The first answer to a waiting item: enable it, stage the action, allow the prediction, use the pack. */
async function answerYes(m: Mod, item: Waiting, gesture: Gesture): Promise<void> {
  switch (item.kind) {
    case 'enable':
      await pressCommand(m, gesture, item.command, item.say, item.key, 'approve')
      return
    case 'allow':
      if (item.offer === 'predict') await pressCommand(m, gesture, item.command, item.say, item.key, 'predict')
      else await allowYes(m, item, gesture)
      return
    case 'trust':
      await pressCommand(m, gesture, item.command, item.say, item.key, 'trust')
  }
}

/** `not this one` records a dismiss through the press; `not now` records only that it was asked. */
async function answerNo(m: Mod, item: Waiting, gesture: Gesture): Promise<void> {
  if (item.kind === 'enable' && item.no !== undefined) await pressCommand(m, gesture, item.no, item.say, item.key, 'dismiss')
  else await noteAnswer(m, item.key, 'not now')
}

/** What an answer in the dialog does; a dismissed dialog (`later`) does nothing. */
async function act(m: Mod, q: AskItem, answer: string | undefined): Promise<void> {
  if (answer === undefined) return
  if (answer === 'why') return showReflex(m, q.item.reflex ?? q.key)
  if (answer === 'not now') return
  if (answer === 'not this one') return answerNo(m, q.item, ANSWER)
  return answerYes(m, q.item, ANSWER)
}

/** The ask at a turn's end (ux-design §4.17): one item, the dialog's answer, the press. */
async function turnEnded(m: Mod): Promise<void> {
  await recountNoticed(m)
  if (m.rt.asking) return
  m.rt.asking = true
  try {
    await checkTamper(m)
    await refresh(m)
    const items = waiting(await m.state.reflexes.read(), await m.state.packs.read())
    const q = nextAsk(items, await m.state.asked.read(), await m.clock.now())
    if (q === undefined) return
    const said = await m.ui.ask(q.question, q.answers).catch(() => undefined)
    const answer = answerOf(q, said)
    await noteAnswer(m, q.key, answer)
    await act(m, q, answer)
  } finally {
    m.rt.asking = false
  }
}

/** `/adage on`: the one ask a command opens; the answer is the press (ux-design §7). */
async function askOn(m: Mod): Promise<void> {
  if (m.rt.asking) return
  m.rt.asking = true
  try {
    const said = await m.ui.ask(ON_QUESTION, ON_ANSWERS).catch(() => undefined)
    if (said === ON_ANSWERS[0]) await pressCommand(m, ANSWER, 'adage on', 'adage', 'on', 'on')
  } finally {
    m.rt.asking = false
  }
}

const sameKept = (a: Kept, b: Kept) => a.command === b.command && a.root === b.root && a.at === b.at

/**
 * Puts the answers kept while no press was held to the person again, now that one is (ux-design
 * §9.12, `ask.kept`): never pressed as they stand, since `$.store` is a file the agent's tools can
 * write. Only those given in this repository are asked, one dialog each, and the press is that
 * answer's. `drop it` forgets one; a dismissed dialog, a press the binary refused before the spend
 * or no press held keeps it for the next start; one from another repository waits for a start
 * there; one older than a week is dropped unasked.
 */
async function askKept(m: Mod): Promise<void> {
  const kept = await keptNow(m)
  const root = await repoRoot(m)
  if (kept.length === 0 || m.rt.asking) {
    m.rt.kept = kept.filter(k => k.root === root).length
    return
  }
  m.rt.asking = true
  const now = await m.clock.now()
  const left: Kept[] = []
  try {
    for (const k of kept) {
      if (now - k.at > KEPT_MAX_AGE_MS) continue
      if (k.root !== root || m.rt.cli === undefined) {
        left.push(k)
        continue
      }
      // An answer no press could run (the store holds anything a file can) is never asked.
      if (pressArgv(m.rt.cli, k.command) === undefined) continue
      const said = await m.ui.ask(keptQuestion(k), KEPT_ANSWERS).catch(() => undefined)
      if (said === KEPT_ANSWERS[1]) continue
      if (said !== KEPT_ANSWERS[0]) {
        left.push(k)
        continue
      }
      const what = whatOf(k.command)
      const pressed =
        what === 'update'
          ? await pressUpdate(m, ANSWER, k.key.replace(/^update:/, ''), false)
          : await pressCommand(m, ANSWER, k.command, k.say, k.key, what, undefined, false)
      if (pressed === undefined || (!pressed.ok && pressed.refused)) left.push(k)
    }
  } finally {
    m.rt.asking = false
  }
  // An answer kept while the dialogs waited (a pane press with no press held) stays kept.
  const added = keptOf(await m.store.kept.get().catch(() => undefined)).filter(x => !kept.some(k => sameKept(k, x)))
  const after = [...left, ...added]
  await m.store.kept.set(after).catch(() => undefined)
  m.rt.kept = after.filter(k => k.root === root).length
  quietly(refresh(m, true))
}

/** The press is asked for at the start, and the kept answers put again once one is held. */
async function started(m: Mod, e: Started): Promise<void> {
  m.rt.pressBound = true
  await loadStore(m)
  if (await issue(m, e)) await askKept(m)
}

/**
 * `session.start` (the second registration): the presses are bound once a person is at the prompt
 * (nothing is bound headless), and what the store kept across sessions is read.
 */
export async function onStart(m: Mod, e: EventAt<'session.start', { isInteractive: [true, false] }>, next: NextAt<'session.start', { isInteractive: [true, false] }>): AnswerAt<'session.start', { isInteractive: [true, false] }> {
  const s = await next(e)
  m.rt.pressBound = e.isInteractive
  m.rt.skill = undefined
  m.rt.noticed = 0
  if (e.isInteractive) quietly(loadStore(m))
  return s
}

/**
 * `classic.SessionStart` (the second registration): the press is asked for after the command
 * hooks' session start (the window opens there) with that payload's session id and source;
 * headless sessions issue no token and ask nothing.
 */
export async function onShimStart(m: Mod, e: EventAt<'classic.SessionStart', { hook_event_name: 'SessionStart' }>, next: NextAt<'classic.SessionStart', { hook_event_name: 'SessionStart' }>): AnswerAt<'classic.SessionStart', { hook_event_name: 'SessionStart' }> {
  const answered = await next(e)
  if (m.rt.interactive) quietly(started(m, e))
  return answered
}

/**
 * The ask, at a turn's end, when the model is idle: never inside the hook (the dialog waits for
 * the person), at most one per turn end, one per reflex per day (ask.ts).
 */
export async function onTurnEnd(m: Mod, e: EventAt<'turn.complete', { reason: 'answer' }>, next: NextAt<'turn.complete', { reason: 'answer' }>): AnswerAt<'turn.complete', { reason: 'answer' }> {
  const done = await next(e)
  if (m.rt.bin !== undefined && m.rt.interactive && e.agentId === undefined) quietly(turnEnded(m))
  return done
}
