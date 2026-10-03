// Pure: the dead end that never runs (docs/claude-mod/architecture.md §3.4; plan C6), as the binary
// words it. A dry run's `substitute` names the command a learned dead end is replaced with, and the
// final answer repeats it only once the person's rules were found to treat both commands alike
// (call.ts makes both asks). Anything malformed is "no replacement" and the call runs as written.
import { isRecord } from './shape'

/** What the binary would run instead, and the one line said about it. */
export type Substitute = { command: string; route: string; track: string; context: string }

/** `{ command, route, track, context }` with a non-empty command, route and context, or undefined. */
export function substituteOf(v: unknown): Substitute | undefined {
  if (!isRecord(v)) return undefined
  const { command, route, track, context } = v
  if (typeof command !== 'string' || command.trim() === '') return undefined
  if (typeof route !== 'string' || route === '' || typeof context !== 'string' || context === '') return undefined
  return { command, route, track: typeof track === 'string' ? track : '', context }
}
