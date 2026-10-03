// The execution grant as the pane words it (ux-design §4.17 `ask.allow.*`; T-B4-03's pure half):
// what a stage answers, read into the three plain lines, the diff and the details; what a confirm
// answers.
import { describe, expect, test } from 'claude-code/testing'

import { BANNED, CONFIRMED, STAGE, STAGE_MOVED } from './fixtures/kit'
import { FIRST_LINE, allowDetails, allowLines, confirmCommand, confirmOf, diffCounts, diffText, routeOf, stageOf } from './grant'
import type { Allow } from './plugin'

const staged = stageOf(STAGE)
const allow = (over: Partial<Allow> = {}): Allow => ({ key: 'k', say: 'run make gen after a schema edit', kind: 'execute', track: 'acme/learned', route: 'codegen-after-schema', command: 'adage review delegate acme/learned codegen-after-schema --level e2 --stage --json', stage: staged ?? null, details: false, note: '', busy: false, ...over })

describe('T-B4-03 · the stage', () => {
  test('every fact the pane draws, read off `--stage --json`', () => {
    if (staged === undefined) throw new Error('stage')
    expect(staged).toMatchObject({ ok: true, first: true, digest: STAGE.digest, argv: ['make', 'gen'], sandbox: 'Linux, Landlock ABI 7 + seccomp; network off', profile: 'p9f8e7d6', effect: 'regenerate', cache: ['__pycache__/**'], outputs: ['acme/generated/**'], inputs: ['schema/*.json'] })
    expect(staged.diff).toEqual([{ path: 'acme/generated/models.py', change: 'modify', plus: 1, minus: 0, text: STAGE.facts.staged.diff[0]?.diff }])
    expect(staged.scripts).toEqual([{ path: 'scripts/gen.sh', sha256: 'e3b0c442', text: '#!/bin/sh\npython -m acme.gen\n' }])
    expect(staged.hashes).toEqual(['profile p9f8e7d6', 'route r1a2b3c4', 'scripts/gen.sh e3b0c442'])
    expect(stageOf({})).toBeUndefined()
    expect(stageOf('x')).toBeUndefined()
    expect(diffCounts('--- a\n+++ b\n@@ -1,2 +1,3 @@\n a\n+b\n-c\n+d\n')).toEqual({ plus: 2, minus: 1 })
  })

  test('the three plain lines, the first-run line only when the binary says so, the diff as one text', () => {
    expect(allowLines(allow(), false)).toEqual([
      'run make gen after a schema edit',
      'it runs    make gen',
      'it changes acme/generated/models.py  (+1 −0, shown below)',
      'it never   touches anything else, or the network',
      FIRST_LINE,
    ])
    const later = stageOf({ ...STAGE, first: false })
    expect(allowLines(allow({ stage: later ?? null }), false)).not.toContain(FIRST_LINE)
    expect(diffText(allow())).toContain('+    currency: str = "USD"')
    for (const l of allowLines(allow(), false)) expect(l).not.toMatch(BANNED)
  })

  test('a step that changes nothing says what it reads; a refused stage says what it would also touch', () => {
    const reads = stageOf({ ...STAGE, facts: { ...STAGE.facts, action: { kind: 'lookup', outputs: [], cache: [] }, staged: { ...STAGE.facts.staged, diff: [] } }, terms: { outputs: [], cache: [] } })
    expect(allowLines(allow({ stage: reads ?? null }), false)).toEqual(['run make gen after a schema edit', 'it runs    make gen', 'it reads   schema/*.json', 'it never   touches anything else, or the network', FIRST_LINE])
    const refused = stageOf({ ...STAGE, ok: false, why: 'the staged run changed paths the grant would not cover', facts: { ...STAGE.facts, staged: { ...STAGE.facts.staged, refused: [{ path: 'README.md', why: 'not an output' }] } } })
    expect(allowLines(allow({ stage: refused ?? null }), false).at(-1)).toBe('it would also touch README.md, which the grant would not cover · not allowed as it is')
    expect(allowLines(allow({ stage: null, busy: true }), false)).toEqual(['run make gen after a schema edit', 'staging it…'])
    expect(diffText(allow({ stage: null }))).toBe('')
  })

  test('the details: what the terminal shows a person today', () => {
    const d = allowDetails(allow(), false)
    expect(d[0]).toBe('argv      make gen')
    expect(d).toContain('script    scripts/gen.sh')
    expect(d).toContain('          python -m acme.gen')
    expect(d).toContain('sandbox   Linux, Landlock ABI 7 + seccomp; network off')
    expect(d).toContain('effect    regenerate')
    expect(d).toContain('outputs   acme/generated/**')
    expect(d).toContain('caches    __pycache__/**')
    expect(d).toContain('hash      profile p9f8e7d6')
    expect(d.at(-1)).toBe('takes effect in your next session')
    expect(allowDetails(allow({ stage: null }), false)).toEqual([])
  })
})

describe('T-B4-03 · the confirm', () => {
  test('recorded, or the fresh stage to decide on again', () => {
    expect(confirmOf(CONFIRMED)).toEqual({ recorded: true, wrote: ['policy', 'marker', 'grant'], takesEffect: 'in new sessions' })
    const moved = confirmOf(STAGE_MOVED)
    expect(moved?.recorded).toBe(false)
    expect(moved?.recorded === false ? moved.stage?.diff[0]?.plus : undefined).toBe(2)
    expect(confirmOf(undefined)).toBeUndefined()
  })

  test('the confirm command of a stage command, and the route a command names', () => {
    expect(confirmCommand('adage review delegate acme/learned codegen-after-schema --level e2 --stage --json')).toBe('adage review delegate acme/learned codegen-after-schema --level e2 --confirm')
    expect(routeOf('adage review delegate acme/learned codegen-after-schema --level e2 --stage --json')).toEqual({ track: 'acme/learned', route: 'codegen-after-schema' })
    expect(routeOf('adage on')).toBeUndefined()
  })
})
