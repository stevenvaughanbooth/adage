// `/adage …` (ux-design §7): one registration, the verb parsed from the arguments. Every verb
// answers with text a headless session can read; the drawn forms are draw/card.tsx's. No verb
// runs a person-only command: `/adage on` and `/adage update` open their asks, whose answers are
// the press (plan B4; onboarding plan O9).
// `runCommand` answers over plain deps; `onCommand` binds them to the mod.
import { copy, explore, openPane } from './actions'
import { read, say } from './bridge'
import { pressHandlers } from './dialog'
import { togglePause } from './follower'
import type { EventAt, Mod } from './mod'
import { quietly } from './mod'
import type { View } from './plugin'
import { cardOf } from './reads'
import type { Card, Offer } from './reads'
import { refresh } from './refresh'
import { SAVINGS_ROWS, cardText, whyText } from './view/card'
import { upToDate, updateHeadless } from './ask'

/** The verbs `/adage` takes; anything else prints the card (ux-design §7). */
const VERBS = ['status', 'graph', 'watch', 'reflexes', 'health', 'doctor', 'review', 'why', 'off', 'on', 'pause', 'map', 'savings', 'update', 'world'] as const
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
    case 'graph':
    case 'card':
      return 'graph'
    case 'watch':
      return 'live'
    case 'reflexes':
    case 'review':
      return 'reflexes'
    case 'health':
    case 'doctor':
      return 'health'
    case 'world':
      return 'world'
    default:
      return undefined
  }
}

export const OFF_TEXT = 'adage is off for this repository (.adage/disabled). /adage on turns it back on.'
/** `/adage on` where no one can be asked (headless, or the mod inert): the terminal is the way. */
export const ON_TEXT = 'Turning Adage back on here is a decision for a person: run adage on in a terminal.'
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
  /** Opens `/adage on`'s ask; undefined where no one can be asked (headless, or the mod inert). */
  askOn: (() => void) | undefined
  /** Opens `/adage update`'s ask; undefined where no one can be asked. */
  askUpdate: ((u: Offer) => void) | undefined
}

const NO_CARD = 'adage is not answering here; run adage doctor in a terminal.'

/** The answer to `/adage <args>`: the text for the transcript (empty for a verb that only opens the pane or an ask). */
export async function runCommand(args: string, deps: CommandDeps): Promise<{ text?: string }> {
  const verb = verbOf(args)
  switch (verb) {
    case 'card':
    case 'status': {
      const card = await deps.card()
      if (verb === 'card' && args.trim() === '') await deps.open('graph')
      return { text: card === undefined ? NO_CARD : cardText(card) }
    }
    case 'savings': {
      const card = await deps.card()
      return { text: card === undefined ? NO_CARD : cardText(card, SAVINGS_ROWS) }
    }
    case 'graph':
    case 'watch':
    case 'reflexes':
    case 'review':
    case 'health':
    case 'doctor':
    case 'world':
      await deps.open(tabOf(verb) ?? 'graph')
      return {}
    case 'why':
      return { text: await deps.why() }
    case 'off': {
      const said = await deps.off()
      if (said === undefined) return { text: NO_CARD }
      return { text: said.startsWith('adage is off') ? OFF_TEXT : said }
    }
    case 'on': {
      // The ask, never the verb: a command handler never runs a person-only verb (ux-design §7).
      if (deps.askOn === undefined) return { text: ON_TEXT }
      deps.askOn()
      return {}
    }
    case 'update': {
      // The ask, never the verb (onboarding plan O9), and only when the binary says a release is
      // newer: the card's `update`, as it words it.
      const card = await deps.card()
      if (card === undefined) return { text: NO_CARD }
      if (card.update === undefined) return { text: upToDate(card.version) }
      if (deps.askUpdate === undefined) return { text: updateHeadless(card.update.latest) }
      deps.askUpdate(card.update)
      return {}
    }
    case 'pause':
      return { text: (await deps.pause()) ? PAUSED : RESUMED }
    case 'map': {
      const url = await deps.map()
      return { text: url === undefined ? 'The map did not start; run adage map in a terminal.' : `Explore in browser: ${url}` }
    }
  }
}

/** `/adage …` (ux-design §7): one registration (made at activation), the verb parsed here. */
export async function onCommand(m: Mod, e: EventAt<'command.run', { command: 'adage' }>): Promise<{ text?: string }> {
  return runCommand(e.args, {
    card: async () => {
      const r = await read(m.run, m.rt.cli, ['--json', '--for-session'], await m.session.cwd())
      return r.ok ? cardOf(r.json) : undefined
    },
    open: tab => (m.rt.interactive ? openPane(m, tab) : Promise.resolve()),
    why: async () => whyText(await m.state.rows.read(), await m.state.reflexes.read(), (await m.state.mode.read()).ascii, m.rt.skill),
    off: async () => {
      const said = await say(m.run, m.rt.cli, ['off'], await m.session.cwd())
      quietly(refresh(m, true))
      return said
    },
    copy: command => copy(m, command),
    pause: () => togglePause(m),
    map: () => explore(m),
    askOn: m.rt.pressBound ? pressHandlers(m).askOn : undefined,
    askUpdate: m.rt.pressBound ? pressHandlers(m).askUpdate : undefined,
  })
}
