// Asking the person in the engine's own dialog (plan B4; architecture §3.12; ux-design §4.17, §7,
// §9.12): at a turn's end, when the model is idle, one waiting item (ask.ts picks it and words it),
// at most one per turn end and one per reflex per day; `/adage on`'s one ask; and the answers kept
// while no press was held, put to the person again at the next start in their repository. Each
// answer is a press (token.ts), and so is each press in the pane (`pressHandlers`).
import { showReflex } from './actions'
import { allowYes } from './allow'
import { KEPT_ANSWERS, ON_ANSWERS, ON_QUESTION, answerOf, keptOf, keptQuestion, nextAsk, whatOf } from './ask'
import type { AskItem } from './ask'
import type { PressHandlers } from './draw/pane'
import { keptNow, loadStore, noteAnswer, repoRoot } from './kept'
import type { AnswerAt, EventAt, Mod, NextAt } from './mod'
import { quietly } from './mod'
import type { Kept } from './plugin'
import { KEPT_MAX_AGE_MS } from './plugin'
import { ANSWER, BUTTON, pressArgv } from './press'
import type { Gesture } from './press'
import { recountNoticed } from './notice'
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
      const pressed = await pressCommand(m, ANSWER, k.command, k.say, k.key, whatOf(k.command), undefined, false)
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
