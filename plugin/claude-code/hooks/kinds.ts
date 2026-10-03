// The state values B4 and D5 add (types/index.d.ts, the section after the B3 values), and their
// initial values: kept apart from plugin.ts, which the B3 and C state owns.
import type { AdageAllow, AdageAsked, AdageCandidate, AdagePress, AdageSkill, AdageStage } from '../types'

export type Press = AdagePress
export type Asked = AdageAsked
export type Allow = AdageAllow
export type Stage = AdageStage
export type Skill = AdageSkill
export type Candidate = AdageCandidate

/** No press yet: before the first session start, the token is neither held nor refused. */
export const PRESS0: Press = { session: '', token: '', state: 'none', issued: 0, chain: 0 }

/** The grant pane's id (`see what it would do`), beside the pane and the inspect pane. */
export const ALLOW_PANE = 'adage-allow'

/** The most asks remembered across sessions (`$.store`), and the most skills kept for `/adage why`. */
export const MAX_ASKED = 200
export const MAX_SKILLS = 20

/** An answer kept for the next session start (ux-design §9.12): no press was held when it was given. */
export type Kept = {
  /** The terminal command the answer runs (`adage review approve T R`, `adage on`, …). */
  command: string
  /** What the reflex tells the agent (or `adage` for `on`), for the toast when it lands. */
  say: string
  /** The waiting key it answered, so the ask does not return meanwhile. */
  key: string
  /** The session the answer was given in. */
  session: string
  at: number
}

/** A kept answer older than this is dropped unapplied: the decision was about a reflex a week ago. */
export const KEPT_MAX_AGE_MS = 7 * 24 * 60 * 60 * 1000
