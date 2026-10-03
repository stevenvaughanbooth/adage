// The text helpers every drawn word uses (docs/claude-mod/ux-design.md §1.9, §1.10): glyphs and
// their ASCII twins, display columns, cutting and fitting a line, plurals, and a reflex named by
// what it tells the agent. Words follow docs/lexicon.md: no strength or lifecycle word outside the
// pane, no "saved", no "approval", a reflex named by its gist, never by its id.
import type { Reflex } from '../plugin'
import { glyph } from '../tokens'
import type { GlyphName } from '../tokens'

/** A glyph, or its ASCII twin under `ADAGE_ASCII=1` (ux-design §1.9). */
export const g = (name: GlyphName, ascii: boolean): string => glyph(name, ascii)

/** Display columns of a string: every glyph the mod draws is one column (ux-design §1.9). */
export const cols = (s: string): number => Array.from(s).length

/** Cuts a string to `n` columns with `…` (or `...` in ASCII) when it is longer. */
export function cut(s: string, n: number, ascii = false): string {
  const chars = Array.from(s)
  if (chars.length <= n) return s
  const dots = ascii ? '...' : '…'
  if (n <= dots.length) return chars.slice(0, Math.max(0, n)).join('')
  return chars.slice(0, n - dots.length).join('') + dots
}

/**
 * Fits parts joined by ` · ` into `columns`: the parts are dropped from the right while the line
 * is too wide (the first part stays), then the last part left is cut with `…`.
 */
export function fit(parts: readonly string[], columns: number, ascii: boolean, keep = 1): string {
  const sep = ` ${g('sep', ascii)} `
  let p = parts.filter(x => x !== '')
  while (p.length > keep && cols(p.join(sep)) > columns) p = p.slice(0, -1)
  return cut(p.join(sep), columns, ascii)
}

/** `1 step`, `2 steps`. */
export const plural = (n: number, one: string, many: string): string => `${n} ${n === 1 ? one : many}`

/** The reflex a route id names (in this track when the tag says which), from `graph --json`. */
export function reflexOf(reflexes: readonly Reflex[], route: string, track = ''): Reflex | undefined {
  const same = reflexes.filter(r => r.id === route)
  if (track !== '') {
    const name = track.replace(/@[^@]*$/, '')
    const inTrack = same.find(r => r.track === track || r.track === name || r.reference === track)
    if (inTrack !== undefined) return inTrack
  }
  return same[0]
}

/**
 * What a reflex tells the agent (ux-design §1.10, `{summary}`): its gist, else its summary. Before
 * the graph has been read the route id stands in, once, rather than a blank line.
 */
export function sayOf(reflexes: readonly Reflex[], route: string, track = ''): string {
  const r = reflexOf(reflexes, route, track)
  return r?.gist || r?.summary || route.replace(/-/g, ' ')
}
