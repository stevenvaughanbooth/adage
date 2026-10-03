// The plugin's constants and state value types. One plugin ships both binaries (plan A8): the
// session-start shim links the one that serves hook events at ${CLAUDE_PLUGIN_DATA}/bin/hook.
import type { AdageFired, AdagePredicted } from '../types'

/**
 * The entry the shim makes for hook events: adage-exec while execution is on
 * ($ADAGE_HOME/exec/enabled), else adage; scripts/quiet when no binary can serve.
 */
export const BIN = 'hook'

export type Fired = AdageFired
export type Predicted = AdagePredicted
