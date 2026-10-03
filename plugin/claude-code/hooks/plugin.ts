// The plugin's constants and state value types. One plugin ships both binaries (plan A8): the
// session-start shim links the one that serves hook events at ${CLAUDE_PLUGIN_DATA}/bin/hook, and
// `adage` itself, for the verbs the mod reads and draws from, at ${CLAUDE_PLUGIN_DATA}/bin/adage.
import type {
  AdageAct,
  AdageBatch,
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
  AdageReflex,
  AdageRow,
  AdageTick,
  AdageTurn,
  AdageView,
} from '../types'

/**
 * The entry the shim makes for hook events: adage-exec while execution is on
 * ($ADAGE_HOME/exec/enabled), else adage; scripts/quiet when no binary can serve.
 */
export const BIN = 'hook'

/** The entry the shim makes for `adage` itself: the CLI verbs exist only there (architecture §2). */
export const CLI = 'adage'

/** The pane's id, and the inspect pane's (ux-design §6, §11). */
export const PANE = 'adage'
export const INSPECT = 'adage-inspect'

/** The slash command, one registration (ux-design §7). */
export const COMMAND = 'adage'

/** The Claude Code version this plugin was validated and tested with (CI's pin). */
export const TESTED = '2.1.288'

export type Fired = AdageFired
export type Predicted = AdagePredicted
export type Mark = AdageMark
export type Batch = AdageBatch
export type Feed = AdageFeed
export type Act = AdageAct
export type Row = AdageRow
export type Tick = AdageTick
export type Health = AdageHealth
export type Learner = AdageLearner
export type Reflex = AdageReflex
export type Check = AdageCheck
export type Lapse = AdageLapse
export type Pack = AdagePack
export type Mode = AdageMode
export type View = AdageView
export type MapState = AdageMap
export type Turn = AdageTurn

export const FEED0: Feed = { status: 'off', cursor: '', failures: 0 }

export const MODE0: Mode = {
  active: false,
  interactive: false,
  surface: null,
  nobinary: '',
  ascii: false,
  reducedMotion: false,
  spinner: false,
  turnLine: false,
  versionLine: '',
  warned: [],
}

export const VIEW0: View = {
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
