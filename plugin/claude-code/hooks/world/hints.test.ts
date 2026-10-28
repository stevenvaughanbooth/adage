// The hints the command hook delivered, read off its answer (world/hints.ts; T-W3-08): one per
// `<adage-world>` hint or disambiguation block, its first line, nothing from any other block.
import { describe, expect, test } from 'claude-code/testing'

import { hintsOf } from './hints'

const HINT = '<adage-world kind="hint">\nLikely reference: "Marketing sync" (meeting, ended 12 min ago, 4 attendees)\nThis note is from Adage, not from the user.\n</adage-world>'
const ASK = '<adage-world kind="disambiguation">\nThe prompt may refer to one of these:\n- "PR #418" (pull request)\n- "PR #419" (pull request)\nThis note is from Adage, not from the user.\n</adage-world>'

describe('T-W3-08 · the hints read', () => {
  test('T-W3-08: a hint and a disambiguation, each by its first line, stamped with the clock and the prompt; a reflex block or plain text yields none', () => {
    expect(hintsOf([HINT, undefined, `${ASK}\n<adage-reflex route="r">\nDo it.\n</adage-reflex>`, 'plain'], 1000, 3)).toEqual([
      { ts: 1000, prompt: 3, cls: 'hint', text: '"Marketing sync" (meeting, ended 12 min ago, 4 attendees)' },
      { ts: 1000, prompt: 3, cls: 'disambiguation', text: 'one of several' },
    ])
    expect(hintsOf([], 1, 1)).toEqual([])
    expect(hintsOf(['<adage-world kind="snapshot">\nx\n</adage-world>'], 1, 1)).toEqual([])
  })
})
