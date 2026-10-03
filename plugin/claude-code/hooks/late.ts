// What the B4 and D5 hooks hand to drawings and commands that register.ts's earlier hooks build
// with fixed arguments (the band's input, the pane's actions, `/adage`'s deps, `/adage why`). The
// hooks at the end of register.ts bind these once they have a `$` (every session start, and each
// of their own events), so a drawing reads the handlers and the few values it needs from here.
// Until they are bound, every drawing and command behaves as B3 built it: a copy, not a press.
//
// Nothing here is a decision: the handlers only ever reach `press()` from a Button's onPress or a
// dialog's answer (press.ts), and the values are copies of `$.state`.
import type { Skill } from './kinds'
import type { Reflex } from './plugin'
import type { Waiting } from './view'

/** What a press in the pane or a command does, bound to `$` by register.ts. */
export type PressHandlers = {
  /** A review verb on a reflex row (`enable`, `not this one`, and the four more). */
  verb: (verb: string, reflex: Reflex) => void
  /** A waiting item's first answer in the pane (`enable`, `see what it would do`, `allow`, `use them`). */
  yes: (item: Waiting) => void
  /** A waiting item's `not this one` / `not now`. */
  no: (item: Waiting) => void
  /** `/adage on`: opens the ask (never presses itself). */
  askOn: () => void
}

export type Late = {
  press: PressHandlers | undefined
  /** Answers kept for the next session start (no press was held): the band says so. */
  kept: number
  /** Distinct sequences noticed this session (the band's `1 pattern noticed`). */
  noticed: number
  /** Whether the pane is open: the noticed cell shows only while it is closed. */
  paneOpen: boolean
  /** The last skill expanded with verified steps (for `/adage why`). */
  skill: Skill | undefined
}

export const late: Late = { press: undefined, kept: 0, noticed: 0, paneOpen: false, skill: undefined }
