// Reading what the binary printed: every answer, feed line and `--json` read arrives as untyped
// JSON, and every reader in the mod checks its shape with these, so a malformed value is "nothing"
// and never a throw (fail open, architecture §6 invariant 2).

/** A JSON object (not null, not an array). */
export const isRecord = (v: unknown): v is Record<string, unknown> => v !== null && typeof v === 'object' && !Array.isArray(v)

/** A string, or '' for anything else. */
export const str = (v: unknown): string => (typeof v === 'string' ? v : '')

/** A finite number, or 0 for anything else. */
export const num = (v: unknown): number => (typeof v === 'number' && Number.isFinite(v) ? v : 0)

/** The strings of a list, or none when it is not one. */
export const strings = (v: unknown): string[] => (Array.isArray(v) ? v.filter((x): x is string => typeof x === 'string') : [])

/** The objects of a list, or none when it is not one. */
export const records = (v: unknown): Record<string, unknown>[] => (Array.isArray(v) ? v.filter(isRecord) : [])
