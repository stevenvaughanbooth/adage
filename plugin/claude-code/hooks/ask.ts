// Which item Adage asks about at a turn's end, and the dialog's words (plan B4; ux-design §4.17,
// §10; architecture §3.12). view/pane.ts `waiting` computes what waits for the person; dialog.ts
// asks about it in the engine's own dialog at `turn.complete` and runs the answer through the
// press. Nothing here asks, runs or records anything.
import type { AskRecord, Kept } from './plugin'
import { MAX_ASKED } from './plugin'
import { isRecord } from './shape'
import type { Waiting } from './view/pane'

/** One question for `$.ui.ask`: the item it is about, the question, the answers in order. */
export type AskItem = { key: string; kind: Waiting['kind']; question: string; answers: readonly string[]; command: string; item: Waiting }

/** An ask returns at most once a day for one item (ux-design §4.17). */
export const ASK_EVERY_MS = 24 * 60 * 60 * 1000

/** A dismissed dialog returns at most once more: after the second dismissal it waits in the pane only. */
export const MAX_DISMISSALS = 2

/**
 * The answers, by item kind, as the copy deck words them (`ask.enable`, `ask.allow`, `ask.trust`);
 * a prediction offer is one act (architecture §3.12: `allow` / `not now` / `why`).
 */
export const ANSWERS = {
  enable: ['enable', 'not this one', 'why'],
  allow: ['see what it would do', 'not now'],
  predict: ['allow', 'not now', 'why'],
  trust: ['use them', 'not now', 'why'],
} as const

/** `/adage on` (ux-design §7): the one ask a command opens. */
export const ON_QUESTION = 'Adage is off in this repository. Turn it on?'
export const ON_ANSWERS = ['turn it on', 'leave it off'] as const

/**
 * `/adage update` (onboarding plan O9; `ask.update`): the other ask a command opens, worded from
 * the status card's `update`. `update` is the press; `what changes` opens Health on the version
 * row; `not now` records that it was asked.
 */
export const UPDATE_ANSWERS = ['update', 'not now', 'what changes'] as const
export const updateQuestion = (u: { have: string; latest: string }): string =>
  `Adage ${u.latest} is out (you have ${u.have}). Update now? It takes effect in your next session.`
/** What a kept `update` answer says when it is put again (`ask.kept`). */
export const updateSay = (latest: string): string => `update Adage to ${latest}`
/** The asked record's key for one release. */
export const updateKey = (latest: string): string => `update:${latest}`
/** `cmd.update.headless`: no one can be asked here (headless, or the mod inert). */
export const updateHeadless = (latest: string): string => `Adage ${latest} is out; run adage update in a terminal.`
/** `/adage update` with nothing newer known. */
export const upToDate = (version: string): string => (version === '' ? 'adage is the latest.' : `adage ${version} is the latest.`)
/** `toast.update.failed`: the press ran and the update did not. */
export const updateFailed = (error: string): string =>
  `adage could not update (${error === '' ? 'the command did not succeed' : error}) · run adage update in a terminal`

/**
 * `toast.updated`, from the last line a pressed `adage update` printed (reads.ts `updatedOf`): the
 * versions it moved between, the latest it already is, or how this install updates instead.
 */
export function updatedToast(t: { kind: 'updated'; from: string; to: string } | { kind: 'latest'; version: string } | { kind: 'manual'; command: string; say: string }): string {
  switch (t.kind) {
    case 'updated':
      return `adage updated ${t.from} → ${t.to} · takes effect in your next session`
    case 'latest':
      return upToDate(t.version)
    case 'manual':
      return t.say !== '' ? t.say : `run ${t.command} to update`
  }
}

/**
 * The next item to ask about at a turn's end, or undefined: the first waiting item that was not
 * answered, was dismissed fewer than twice, and was not asked in the last day. At most one per
 * turn end.
 */
export function nextAsk(items: readonly Waiting[], asked: readonly AskRecord[], now: number): AskItem | undefined {
  const by = new Map(asked.map(a => [a.key, a]))
  const item = items.find(w => {
    const a = by.get(w.key)
    return a === undefined || (!a.done && a.times < MAX_DISMISSALS && now - a.at >= ASK_EVERY_MS)
  })
  return item === undefined ? undefined : question(item)
}

/**
 * The dialog's words for one item (ux-design §10: `ask.enable`, `ask.enable.quiet`, `ask.allow`,
 * `ask.predict`, `ask.trust`). The reflex's sentence (`after editing src/**, run make test`) is
 * quoted after a colon, whole, so the question reads right whatever form the sentence takes.
 */
export function question(w: Waiting): AskItem {
  const base = { key: w.key, kind: w.kind, command: w.command, item: w }
  switch (w.kind) {
    case 'enable':
      return w.group === 'quiet'
        ? { ...base, question: `Agents here already do this unaided, so Adage stays quiet: ${w.say}. Tell the agent anyway?`, answers: ANSWERS.enable }
        : { ...base, question: `Adage learned: ${w.say}. Tell the agent?`, answers: ANSWERS.enable }
    case 'allow':
      return w.offer === 'predict'
        ? { ...base, question: `Adage can take this step without a model request: ${w.say}. Allow it?`, answers: ANSWERS.predict }
        : { ...base, question: `Adage can do this itself, instead of asking the agent: ${w.say}. See what it would do?`, answers: ANSWERS.allow }
    case 'trust':
      return { ...base, question: `Reflexes from ${w.say} are in this repository. Use them?`, answers: ANSWERS.trust }
  }
}

/** What an answer is: one of the dialog's labels exactly; anything typed under "Other" is not one. */
export function answerOf(item: { answers: readonly string[] }, said: string | undefined): string | undefined {
  return said !== undefined && item.answers.includes(said) ? said : undefined
}

/**
 * The asked records after an answer. `undefined` is a dismissed dialog (`later`): counted, and the
 * ask may return a day later; `why` opens the pane and leaves the decision open; any other answer
 * closes it for good.
 */
export function record(asked: readonly AskRecord[], key: string, answer: string | undefined, now: number): AskRecord[] {
  const was = asked.find(a => a.key === key) ?? { key, at: now, times: 0, done: false }
  const later = answer === undefined
  const next: AskRecord = { key, at: now, times: later ? was.times + 1 : was.times, done: was.done || (!later && answer !== 'why') }
  return [...asked.filter(a => a.key !== key), next].slice(-MAX_ASKED)
}

/** The asked records as `$.store` kept them (`adage.asked`), or none when it holds anything else. */
export function askedOf(v: unknown): AskRecord[] {
  if (!Array.isArray(v)) return []
  return v
    .filter(isRecord)
    .filter(a => typeof a.key === 'string' && typeof a.at === 'number')
    .map(a => ({ key: a.key as string, at: a.at as number, times: typeof a.times === 'number' ? a.times : 0, done: a.done === true }))
    .slice(-MAX_ASKED)
}

/**
 * What a terminal command decides, by its words: a review verb (`approve`, `dismiss`, …), `on`,
 * `trust` (`add … --trust`), `predict`, `stage` or `confirm` (`review delegate …`).
 */
export function whatOf(command: string): string {
  const words = command.trim().split(/\s+/)
  if (words[0] !== 'adage') return ''
  if (words[1] === 'on') return 'on'
  if (words[1] === 'update') return 'update'
  if (words[1] === 'add') return 'trust'
  if (words[1] !== 'review') return ''
  if (words[2] !== 'delegate') return words[2] ?? ''
  if (words.includes('--predict')) return 'predict'
  if (words.includes('--confirm')) return 'confirm'
  return 'stage'
}

/**
 * The toast after a decision landed (`ask.enable.done`, `ask.allow.done`, `cmd.on`), or none: a
 * stage shows a pane, a confirm says its own, and the four more verbs say what they did.
 */
export function doneToast(what: string, say: string, word?: string): string | undefined {
  switch (what) {
    case 'approve':
      return `on: ${say}`
    case 'predict':
      return `allowed: ${say} · from your next session`
    case 'on':
      return 'adage is on for this repository.'
    case 'trust':
      return `using them: ${say}`
    case 'dismiss':
    case 'strengthen':
    case 'weaken':
    case 'extinguish':
    case 'pin':
      return `done: ${word ?? what} · ${say}`
    default:
      return undefined
  }
}

/**
 * The answers kept for the next session start, as `$.store` holds them (`adage.kept`). One with
 * no repository root (kept before answers named one) can be offered nowhere, and is dropped.
 */
export function keptOf(v: unknown): Kept[] {
  if (!Array.isArray(v)) return []
  return v
    .filter(isRecord)
    .filter(k => typeof k.command === 'string' && typeof k.at === 'number' && typeof k.root === 'string' && k.root !== '')
    .map(k => ({
      command: k.command as string,
      say: typeof k.say === 'string' ? k.say : '',
      key: typeof k.key === 'string' ? k.key : '',
      session: typeof k.session === 'string' ? k.session : '',
      root: k.root as string,
      at: k.at as number,
    }))
}

/**
 * `ask.kept` (ux-design §9.12, §10): a kept answer put to the person again at the next start in
 * its repository. The question names the exact command that runs, since the store it was read
 * from is a file the agent's tools can write: what the person agrees to is what they read.
 */
export const KEPT_ANSWERS = ['run it', 'drop it'] as const
export function keptQuestion(k: Pick<Kept, 'command' | 'say'>): string {
  const say = k.say.trim()
  return say === '' || say === 'adage' ? `Kept from your last session: ${k.command}. Run it now?` : `Kept from your last session: ${say} (${k.command}). Run it now?`
}
