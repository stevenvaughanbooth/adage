// This session's graph as the observers keep it (dag.ts; ux-design §6.0; T-B3-13): appended in
// order for a person at the prompt only, capped at the newest `MAX_NODES`, the newest's glow
// settled by one redraw (none under reduced motion); a batch's node and a lone call's word.
import { describe, expect, test } from 'claude-code/testing'

import { MAX_NODES, PULSE_MS, detailOf, keepNodes, shortPath, toolsNode } from './dag'
import { T0, fake } from './fixtures/kit'
import type { GraphNode } from './plugin'

const prompt = (n: number): GraphNode => ({ kind: 'prompt', at: T0 + n, n, text: `p${n}` })

describe('T-B3-13 · the graph\'s nodes', () => {
  test('kept in order for a person at the prompt; headless, nothing is kept', async () => {
    const f = fake()
    await keepNodes(f.m, [prompt(1)])
    expect(f.state.dag).toEqual([])
    f.m.rt.interactive = true
    await keepNodes(f.m, [prompt(1)])
    await keepNodes(f.m, [prompt(2), prompt(3)])
    expect(f.state.dag.map(n => (n.kind === 'prompt' ? n.n : 0))).toEqual([1, 2, 3])
  })

  test(`capped at ${MAX_NODES}: the oldest go first`, async () => {
    const f = fake()
    f.m.rt.interactive = true
    await keepNodes(f.m, Array.from({ length: MAX_NODES }, (_, k) => prompt(k)))
    await keepNodes(f.m, [prompt(MAX_NODES), prompt(MAX_NODES + 1)])
    expect(f.state.dag).toHaveLength(MAX_NODES)
    expect(f.state.dag[0]).toMatchObject({ n: 2 })
    expect(f.state.dag.at(-1)).toMatchObject({ n: MAX_NODES + 1 })
  })

  test('the newest settles with one redraw after its glow, a later node moving it on; none under reduced motion', async () => {
    const f = fake()
    f.m.rt.interactive = true
    await keepNodes(f.m, [prompt(1)])
    await keepNodes(f.m, [prompt(2)])
    expect(f.timers.map(x => [x.kind, x.ms, x.cancelled])).toEqual([['after', PULSE_MS + 100, true], ['after', PULSE_MS + 100, false]])
    const still = fake({ state: { mode: { ...fake().state.mode, reducedMotion: true } } })
    still.m.rt.interactive = true
    await keepNodes(still.m, [prompt(1)])
    expect(still.timers).toEqual([])
  })

  test('a batch\'s node: one name per call, the ids, a word on a lone call; none without an id', () => {
    expect(toolsNode([{ tool_name: 'Bash', tool_input: { command: 'make gen\nmake test' }, tool_use_id: 't1' }], 1, 2)).toEqual({ kind: 'tools', at: 2, from: 1, tools: ['Bash'], ids: ['t1'], detail: 'make gen' })
    expect(toolsNode([{ tool_name: 'Read', tool_use_id: 'a' }, { tool_name: 'Read', tool_use_id: 'b' }, { tool_use_id: '' }], 1, 2)).toEqual({ kind: 'tools', at: 2, from: 1, tools: ['Read', 'Read'], ids: ['a', 'b'], detail: '' })
    expect(toolsNode([{ tool_name: 'Bash', tool_use_id: '' }], 1, 2)).toBeUndefined()
    expect(detailOf('Edit', { file_path: '/w/acme/schema/invoice.json' })).toBe('schema/invoice.json')
    expect(detailOf('Grep', { pattern: 'currency' })).toBe('currency')
    expect(detailOf('WebFetch', { url: 'https://docs.example.com/a/b' })).toBe('docs.example.com')
    expect(detailOf('Agent', { description: 'Find the schema' })).toBe('Find the schema')
    expect(detailOf('TodoWrite', { todos: [] })).toBe('')
    expect(detailOf('Bash', 'not an input')).toBe('')
    expect(shortPath('C:\\w\\acme\\x.py')).toBe('acme/x.py')
  })
})
