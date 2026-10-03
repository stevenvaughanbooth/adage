// `/adage …` (ux-design §7): one registration, the verb parsed from the arguments. Every verb
// answers with text a headless session can read; the drawn forms are draw/card.tsx's. No verb
// runs a person-only command: `/adage on` says how until the press (plan B4) opens the ask.
//
// No `$` crosses into this file: register.ts hands `runCommand` closures over its `$`.
import type { View } from './plugin'
import { SAVINGS_ROWS, cardText } from './view'
import type { Card } from './view'

/** The verbs `/adage` takes; anything else prints the card (ux-design §7). */
export const VERBS = ['status', 'watch', 'reflexes', 'health', 'doctor', 'review', 'why', 'off', 'on', 'pause', 'map', 'savings'] as const
export type Verb = (typeof VERBS)[number] | 'card'

/** The hint after the name: the two verbs a person needs (ux-design §7). */
export const ARGUMENT_HINT = '[review]'
export const DESCRIPTION = 'What Adage did in this session and this repository; /adage review for what waits for you'

/** The verb in `/adage <args>`: its first word, lowercased; none or an unknown one is the card. */
export function verbOf(args: string): Verb {
  const word = args.trim().split(/\s+/)[0]?.toLowerCase() ?? ''
  return (VERBS as readonly string[]).includes(word) ? (word as Verb) : 'card'
}

/** The pane's tab a verb opens, if it opens one. */
export function tabOf(verb: Verb): View['tab'] | undefined {
  switch (verb) {
    case 'watch':
      return 'live'
    case 'reflexes':
    case 'review':
      return 'reflexes'
    case 'health':
    case 'doctor':
      return 'health'
    case 'card':
      return 'live'
    default:
      return undefined
  }
}

export const OFF_TEXT = 'adage is off for this repository (.adage/disabled). /adage on turns it back on.'
export const ON_TEXT = 'Turning Adage back on here is a decision for a person: run adage on in a terminal (copied).'
export const ON_TEXT_UNCOPIED = 'Turning Adage back on here is a decision for a person: run adage on in a terminal.'
export const PAUSED = 'adage is paused for this session · /adage pause resumes'
export const RESUMED = 'adage resumed'

export type CommandDeps = {
  card: () => Promise<Card | undefined>
  /** Opens the pane on a tab (focused: the person asked). */
  open: (tab: View['tab']) => Promise<void>
  why: () => Promise<string>
  /** Runs `adage off`; the binary's line, or undefined when it could not run. */
  off: () => Promise<string | undefined>
  /** Copies a command for the person; whether it took. */
  copy: (command: string) => Promise<boolean>
  /** Toggles the pause; true when now paused. */
  pause: () => Promise<boolean>
  /** Starts the map if it is not serving; its address, or undefined. */
  map: () => Promise<string | undefined>
}

const NO_CARD = 'adage is not answering here; run adage doctor in a terminal.'

/** The answer to `/adage <args>`: the text for the transcript (empty for a verb that only opens the pane). */
export async function runCommand(args: string, deps: CommandDeps): Promise<{ text?: string }> {
  const verb = verbOf(args)
  switch (verb) {
    case 'card':
    case 'status': {
      const card = await deps.card()
      if (verb === 'card' && args.trim() === '') await deps.open('live')
      return { text: card === undefined ? NO_CARD : cardText(card) }
    }
    case 'savings': {
      const card = await deps.card()
      return { text: card === undefined ? NO_CARD : cardText(card, SAVINGS_ROWS) }
    }
    case 'watch':
    case 'reflexes':
    case 'review':
    case 'health':
    case 'doctor':
      await deps.open(tabOf(verb) ?? 'live')
      return {}
    case 'why':
      return { text: await deps.why() }
    case 'off': {
      const said = await deps.off()
      if (said === undefined) return { text: NO_CARD }
      return { text: said.startsWith('adage is off') ? OFF_TEXT : said }
    }
    case 'on': {
      return { text: (await deps.copy('adage on')) ? ON_TEXT : ON_TEXT_UNCOPIED }
    }
    case 'pause':
      return { text: (await deps.pause()) ? PAUSED : RESUMED }
    case 'map': {
      const url = await deps.map()
      return { text: url === undefined ? 'The map did not start; run adage map in a terminal.' : `Explore in browser: ${url}` }
    }
  }
}
