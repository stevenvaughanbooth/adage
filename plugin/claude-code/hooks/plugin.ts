// The plugin's constants, its state's value types and initial values, and the two tables every
// other file is held to (docs/claude-mod/impl/mod-structure.md): `STATE0`, every `$.state` key with
// its initial value, and `STORE_KEYS`, every `$.store` key. One plugin ships both binaries (plan
// A8): the session-start shim links the one that serves hook events at
// ${CLAUDE_PLUGIN_DATA}/bin/hook, and `adage` itself, for the verbs the mod reads and draws from, at
// ${CLAUDE_PLUGIN_DATA}/bin/adage.
import type { PluginState } from 'claude-code'

import type {
  AdageAct,
  AdageAllow,
  AdageAsked,
  AdageBatch,
  AdageCandidate,
  AdageCheck,
  AdageFeed,
  AdageFired,
  AdageHealth,
  AdageLapse,
  AdageLearner,
  AdageMap,
  AdageMark,
  AdageMode,
  AdagePack,
  AdagePredicted,
  AdagePress,
  AdageReflex,
  AdageRow,
  AdageSkill,
  AdageStage,
  AdageTick,
  AdageTurn,
  AdageUpdate,
  AdageView,
} from '../types'

/**
 * The entry the shim makes for hook events: adage-exec while execution is on
 * ($ADAGE_HOME/exec/enabled), else adage; scripts/quiet when no binary can serve.
 */
export const BIN = 'hook'

/** The entry the shim makes for `adage` itself: the CLI verbs exist only there (architecture §2). */
export const CLI = 'adage'

/** The pane's id, the inspect pane's and the grant pane's (`see what it would do`; ux-design §6, §11, §4.17). */
export const PANE = 'adage'
export const INSPECT = 'adage-inspect'
export const ALLOW_PANE = 'adage-allow'

/** The slash command, one registration (ux-design §7). */
export const COMMAND = 'adage'

/** The Claude Code version this plugin was validated and tested with (CI's pin). */
export const TESTED = '2.1.288'

export type Fired = AdageFired
export type Predicted = AdagePredicted
export type Mark = AdageMark
export type Batch = AdageBatch
type Feed = AdageFeed
export type Act = AdageAct
export type Row = AdageRow
export type Tick = AdageTick
export type Health = AdageHealth
export type Learner = AdageLearner
export type Reflex = AdageReflex
export type Check = AdageCheck
export type Lapse = AdageLapse
export type Pack = AdagePack
type Mode = AdageMode
export type View = AdageView
export type MapState = AdageMap
export type Turn = AdageTurn
export type Press = AdagePress
export type AskRecord = AdageAsked
export type Allow = AdageAllow
export type Stage = AdageStage
export type Skill = AdageSkill
export type Candidate = AdageCandidate
export type Update = AdageUpdate

/** Every `$.state` value of the plugin, by key, as types/index.d.ts declares them. */
export type AdageState = PluginState['adage']

const FEED0: Feed = { status: 'off', cursor: '', failures: 0 }

const MODE0: Mode = {
  active: false,
  interactive: false,
  surface: null,
  nobinary: '',
  ascii: false,
  reducedMotion: false,
  spinner: false,
  turnLine: false,
  noticed: false,
  versionLine: '',
  update: null,
  warned: [],
}

const VIEW0: View = {
  tab: 'live',
  filter: '',
  selected: null,
  hideQuiet: false,
  paused: false,
  rfilter: '',
  strength: 'all',
  lifecycle: 'all',
  expanded: '',
}

export const MAP0: MapState = { url: '', starting: false }

export const TURN0: Turn = { submits: 0, fired: 0, predicted: 0, marks: 0 }

/** No press yet: before the first session start, the token is neither held nor refused. */
const PRESS0: Press = { session: '', token: '', state: 'none', issued: 0, chain: 0 }

/**
 * The state table: every `$.state` key the mod keeps, with the value it reads as before anything is
 * written. A key is added here, in types/index.d.ts (the contract) and as one atom and one cell in
 * register.ts; `scripts/check-plugin.mjs` holds the four to the same keys in the same order.
 */
export const STATE0: AdageState = {
  fired: [],
  predicted: [],
  marks: [],
  batches: [],
  feed: FEED0,
  rows: [],
  ticks: [],
  health: null,
  learner: null,
  reflexes: [],
  checks: [],
  lapsed: [],
  packs: [],
  mode: MODE0,
  view: VIEW0,
  map: MAP0,
  turn: TURN0,
  press: PRESS0,
  asked: [],
  allow: null,
  skills: [],
  candidates: [],
}

/**
 * The store table: every `$.store` key the mod keeps across sessions (each `adage.<key>`): the
 * person's last tab, the harness version the version line was last shown for, the asks (one per
 * reflex per day), the answers kept for the next session start, and the newest Adage the band's
 * update line was shown for. A key is added here and as one cell in register.ts;
 * `scripts/check-plugin.mjs` holds the two together.
 */
export const STORE_KEYS = ['tab', 'harness', 'asked', 'kept', 'latest'] as const
export type StoreKey = (typeof STORE_KEYS)[number]

/** The most asks remembered across sessions (`$.store`), and the most skills kept for `/adage why`. */
export const MAX_ASKED = 200
export const MAX_SKILLS = 20

/**
 * An answer kept for the next session start (ux-design §9.12): no press was held when it was
 * given. It is never pressed as it stands: the next start in the same repository asks the person
 * again (`ask.kept`), and only that answer runs it.
 */
export type Kept = {
  /** The terminal command the answer runs (`adage review approve T R`, `adage on`, …). */
  command: string
  /** What the reflex tells the agent (or `adage` for `on`), for the toast when it lands. */
  say: string
  /** The waiting key it answered, so the ask does not return meanwhile. */
  key: string
  /** The session the answer was given in. */
  session: string
  /**
   * The repository root it was given in: `adage on` and `add <dir> --trust` act on the working
   * directory's repository, so it is offered only at a start in that same root.
   */
  root: string
  at: number
}

/** A kept answer older than this is dropped unapplied: the decision was about a reflex a week ago. */
export const KEPT_MAX_AGE_MS = 7 * 24 * 60 * 60 * 1000
