// Pure: the `<adage-reflex …>` blocks a command hook's answer delivered, read off their opening
// tags (the same tag `crates/adage-ingest/src/signals.rs` reads). Attributes only; the text inside
// is the model's, never parsed here.
import type { Fired } from './plugin'

const TAG = /<adage-reflex\b([^>]*)>/g
const ATTR = /([A-Za-z_][\w-]*)\s*=\s*"([^"]*)"/g

export function parseReflexes(texts: readonly (string | undefined)[], event: string): Fired[] {
  const fired: Fired[] = []
  for (const text of texts) {
    if (text === undefined) continue
    for (const tag of text.matchAll(TAG)) {
      const attrs: Record<string, string> = {}
      for (const a of (tag[1] ?? '').matchAll(ATTR)) attrs[a[1] ?? ''] = a[2] ?? ''
      fired.push({
        event,
        route: attrs.route ?? '',
        track: attrs.track ?? '',
        kind: attrs.kind ?? '',
        strength: attrs.strength ?? '',
      })
    }
  }
  return fired
}
