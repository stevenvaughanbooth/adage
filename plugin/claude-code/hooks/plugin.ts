// What differs between the two copies of the module: the hook binary's name here, and the plugin
// name in register.ts's $.state references (validate reads those only as literals in that file).
import type { AdageFired, AdagePredicted } from '../types'

/** The binary the shim links at ${CLAUDE_PLUGIN_DATA}/bin/<BIN> for hook events. */
export const BIN = 'adage'

export type Fired = AdageFired
export type Predicted = AdagePredicted
