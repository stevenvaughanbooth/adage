// The hints the command hook delivered this session, read off its answer as the reflexes are
// (reflex.ts): one `<adage-world kind="hint">` or `kind="disambiguation"` block per prompt at
// most, its first line the reference it named. Pure; observe.ts keeps what this reads.
import type { WorldHint } from '../plugin'

const OPEN = /<adage-world kind="(hint|disambiguation)">\n([^\n]*)/g

/** Every hint block in the context the hook answered, in order. */
export function hintsOf(context: readonly (string | undefined)[], ts: number, prompt: number): WorldHint[] {
  const out: WorldHint[] = []
  for (const text of context) {
    if (typeof text !== 'string') continue
    for (const m of text.matchAll(OPEN)) {
      const cls = m[1] === 'disambiguation' ? 'disambiguation' : 'hint'
      const line = (m[2] ?? '').replace(/^Likely reference: /, '').replace(/^The prompt may refer to one of these:$/, 'one of several').trim()
      out.push({ ts, prompt, cls, text: line })
    }
  }
  return out
}
