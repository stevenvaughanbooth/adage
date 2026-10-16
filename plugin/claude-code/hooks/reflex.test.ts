// The `<adage-reflex>` blocks a command hook delivered, read off their opening tags (reflex.ts;
// T-B2-02's reader): attributes only, never the text inside.
import { describe, expect, test } from 'claude-code/testing'

import { parseReflexes } from './reflex'

describe('T-B2-02 · the reflexes a hook delivered', () => {
  test('reads each <adage-reflex> opening tag, attributes only', () => {
    const texts = [
      '<adage-reflex track="acme/billing@1" route="codegen-after-schema" strength="fire" kind="obligation">\nRun `make gen`.\n</adage-reflex>',
      'plain context\n<adage-reflex route="case" kind="prime">x</adage-reflex> and <adage-reflex track=\'odd\'>y</adage-reflex>',
      undefined,
      'no blocks here',
    ]
    expect(parseReflexes(texts, 'PostToolBatch')).toEqual([
      { event: 'PostToolBatch', route: 'codegen-after-schema', track: 'acme/billing@1', kind: 'obligation', strength: 'fire' },
      { event: 'PostToolBatch', route: 'case', track: '', kind: 'prime', strength: '' },
      { event: 'PostToolBatch', route: '', track: '', kind: '', strength: '' },
    ])
  })

  test('nothing delivered is nothing fired', () => {
    expect(parseReflexes([], 'Stop')).toEqual([])
    expect(parseReflexes(['</adage-reflex>', '<adage-reflexes route="x">'], 'Stop')).toEqual([])
  })
})
