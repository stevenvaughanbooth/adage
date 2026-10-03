// The seam for asking (plan B4; ux-design §4.17, architecture §3.12). B3 computes what waits for
// the person (`view.ts` `waiting`) and lists it in the pane with the command to copy; B4 asks about
// it in the engine's own dialog at `turn.complete` and runs the answer through the press. This file
// is the part of that both chunks share: which item to ask about next, and the dialog's words.
// Nothing here asks, runs or records anything.
import type { Waiting } from './view'

/** One question for `$.ui.ask`: the item it is about, the question, the answers in order. */
export type AskItem = { key: string; kind: Waiting['kind']; question: string; answers: readonly string[]; command: string }

/** An ask returns at most once a day for one item (ux-design §4.17). */
export const ASK_EVERY_MS = 24 * 60 * 60 * 1000

/**
 * The next item to ask about at a turn's end, or undefined: the first waiting item not asked in
 * the last day (`asked`: when each key was last asked) and not answered `not this one`
 * (`dismissed`). At most one per turn end; B4 calls this from its `turn.complete` hook.
 */
export function nextAsk(items: readonly Waiting[], asked: Readonly<Record<string, number>>, dismissed: ReadonlySet<string>, now: number): AskItem | undefined {
  const item = items.find(w => !dismissed.has(w.key) && now - (asked[w.key] ?? -Infinity) >= ASK_EVERY_MS)
  return item === undefined ? undefined : question(item)
}

/** The dialog's words for one item (ux-design §10: `ask.enable`, `ask.allow`, `ask.trust`). */
export function question(w: Waiting): AskItem {
  switch (w.kind) {
    case 'enable':
      return { key: w.key, kind: w.kind, question: `Adage learned: ${w.say}. Tell the agent?`, answers: ['enable', 'not this one', 'why'], command: w.command }
    case 'allow':
      return { key: w.key, kind: w.kind, question: `Adage can ${w.say}, instead of asking the agent to. Allow it?`, answers: ['see what it would do', 'not now'], command: w.command }
    case 'trust':
      return { key: w.key, kind: w.kind, question: `Reflexes from ${w.say} are in this repository. Use them?`, answers: ['use them', 'not now', 'why'], command: w.command }
  }
}
