// The session graph's layout (view/graph.ts; ux-design §6.0; T-B3-12): the prompt boxes, the
// trunk of steps, each reflex a `◆ adage` box off the step it followed with its response lit once
// followed, what else Adage did as a line that says so, the quiet stretches folded, the newest
// followed within the pane's rows, at 100, 80 and 60 columns and the one-line list beneath, with
// ASCII twins.
import { describe, expect, test } from 'claude-code/testing'

import { PULSE_MS } from '../dag'
import { GRAPH, LINE, T0, rowsOf } from '../fixtures/kit'
import type { GraphNode, Mark, Predicted, Row } from '../plugin'
import { reflexesOf } from '../reads'
import type { GraphInput } from './graph'
import { QUIET_RUN, WIDE, graphLines, toolsLabel } from './graph'
import { clock } from './pane'
import type { Seg } from './pane'
import { cols } from './text'

const REFLEXES = reflexesOf(GRAPH)
const t = (n: number) => T0 + n * 1000
const c = (n: number) => clock(t(n))
const text = (l: readonly Seg[]) => l.map(x => x.text).join('')
const ASCII = /^[\x20-\x7e]*$/

/** A session: a prompt a reflex fired at, followed; an edit that fired the codegen reflex, followed; a rewrite; a quiet stretch; a failure a dead end fired after; a step Adage chose; the turn's end; a second prompt. */
const NODES: GraphNode[] = [
  { kind: 'prompt', at: t(0), n: 1, text: 'fix W-2042 and add the currency field' },
  { kind: 'reflex', at: t(0), event: 'UserPromptSubmit', route: 'lookup-work-item', track: 'acme/learned@1', strength: 'fire', ids: [] },
  { kind: 'tools', at: t(2), from: t(2), tools: ['Bash'], ids: ['b1'], detail: 'scripts/wi-lookup W-2042 --json' },
  { kind: 'tools', at: t(3), from: t(3), tools: ['Read', 'Read', 'Grep'], ids: ['r1', 'r2', 'g1'], detail: '' },
  { kind: 'tools', at: t(4), from: t(4), tools: ['Edit'], ids: ['e1'], detail: 'schema/invoice.json' },
  { kind: 'reflex', at: t(4), event: 'PostToolBatch', route: 'codegen-after-schema', track: 'acme/learned@1', strength: 'fire', ids: ['e1'] },
  { kind: 'tools', at: t(6), from: t(6), tools: ['Bash'], ids: ['b2'], detail: 'make gen' },
  { kind: 'tools', at: t(7), from: t(7), tools: ['Read'], ids: ['r3'], detail: 'acme/models.py' },
  { kind: 'tools', at: t(8), from: t(8), tools: ['Edit', 'Edit'], ids: ['e2', 'e3'], detail: '' },
  { kind: 'tools', at: t(9), from: t(9), tools: ['Bash'], ids: ['b3'], detail: 'git diff --stat' },
  { kind: 'failed', at: t(10), id: 'b4' },
  { kind: 'reflex', at: t(10), event: 'PostToolUseFailure', route: 'dead-end-unittest', track: 'acme/learned@1', strength: 'inhibit', ids: ['b4'] },
  { kind: 'tools', at: t(10), from: t(10), tools: ['Bash'], ids: ['b4'], detail: 'python3 -m unittest' },
  { kind: 'tools', at: t(12), from: t(12), tools: ['Bash'], ids: ['adage_x'], detail: 'make test' },
  { kind: 'stop', at: t(14), gate: false },
  { kind: 'prompt', at: t(20), n: 2, text: 'now ship it' },
  { kind: 'tools', at: t(22), from: t(22), tools: ['Bash'], ids: ['b9'], detail: 'git status' },
]
/** This session's feed: the lookup directed and followed at the next step, the codegen followed after its edit. */
const ROWS: Row[] = [
  ...rowsOf(LINE.firedLookup).map(r => ({ ...r, ts: t(0) })),
  { ...(rowsOf(LINE.followedGen)[0] as Row), seq: 2, ts: t(2) + 10, acts: [{ act: 'followed', route: 'lookup-work-item' }] },
  { ...(rowsOf(LINE.followedGen)[0] as Row), seq: 3, ts: t(6) + 10 },
]
const PREDICTED: Predicted[] = [{ turnId: 't', index: 3, route: 'x', kind: 'run', ids: ['adage_x'], isEnding: false }]
const MARKS: Mark[] = [{ id: 'b2', kind: 'rewrite', interpreter: '.venv/bin/python', what: 'test' }]
const INPUT: GraphInput = { nodes: NODES, predicted: PREDICTED, marks: MARKS, rows: ROWS, reflexes: REFLEXES, now: t(22) + 500, motion: true }
const draw = (columns: number, over: Partial<GraphInput> = {}, frame: Partial<{ ascii: boolean; boxes: boolean; height: number }> = {}) =>
  graphLines({ ...INPUT, ...over }, { columns, ascii: false, boxes: true, height: 0, ...frame })

describe('T-B3-12 · the session graph, laid out', () => {
  test('100 columns: the prompt box, the trunk, each reflex a ◆ adage box off the step it followed, its response lit once followed', () => {
    const d = draw(100)
    expect(text(d.head)).toBe('◆ this session · adage fired 3 reflexes · 2 followed · 1 step without a model request · 2 prompts')
    expect(d.lines.map(text)).toEqual([
      `╭─ PROMPT 1 ──────────────────────────────────────────────────────────────────────────── ${c(0)} ─╮`,
      '│ fix W-2042 and add the currency field                                                            │',
      '╰─┬─┬──────────────────────────────────────────────────────────────────────────────────────────────╯',
      '  │ │  ╭─ ◆ adage · ● FIRE ──────────────────────╮      ╭─ RUN ───────────────────────────────────╮',
      '  │ ╰─▶│ look up the work item with scripts/wi-… │ ───▶ │ scripts/wi-lookup W-2042 --json         │',
      `  │    ╰─ at your prompt · ${c(0)} ─────────────╯      ╰─ ✓ followed ────────────────────────────╯`,
      `  ├── Bash  scripts/wi-lookup W-2042 --json                            ✓ followed adage  ${c(2)}`,
      `  ├── Read ×2 · Grep                                                                     ${c(3)}`,
      `  ├─┬ Edit  schema/invoice.json                                                          ${c(4)}`,
      '  │ │  ╭─ ◆ adage · ● FIRE ──────────────────────╮      ╭─ RUN ───────────────────────────────────╮',
      '  │ ╰─▶│ run make gen after a schema edit        │ ───▶ │ make gen                                │',
      `  │    ╰─ after this step · ${c(4)} ────────────╯      ╰─ ✓ followed ────────────────────────────╯`,
      `  ├─┬ Bash  make gen                                                   ✓ followed adage  ${c(6)}`,
      '  │ ╰─ ↪ adage pointed this test run at .venv/bin/python',
      '  ┊   3 steps, nothing fired  Read · Edit ×2 · Bash',
      `  ├─┬ Bash  python3 -m unittest                                              ✗ 1 failed  ${c(10)}`,
      '  │ │  ╭─ ◆ adage · ● INHIBIT ───────────────────╮      ╭─ RUN ───────────────────────────────────╮',
      '  │ ╰─▶│ avoid python3 -m unittest here; use ma… │ ───▶ │ make gen                                │',
      `  │    ╰─ after a failure · ${c(10)} ────────────╯      ╰─────────────────────────────────────────╯`,
      `  ├─┬ Bash  make test                                                                    ${c(12)}`,
      '  │ ╰─ ◆ adage chose this step · no model request',
      `  ╰── ✓ done                                                                             ${c(14)}`,
      '',
      `╭─ PROMPT 2 ──────────────────────────────────────────────────────────────────────────── ${c(20)} ─╮`,
      '│ now ship it                                                                                      │',
      '╰─┬────────────────────────────────────────────────────────────────────────────────────────────────╯',
      '  ├── Bash  git status                                                                        now',
    ])
    // The roles: a reflex's box and edge in its strength's, its response in the fire role once followed.
    const flat = d.lines.flat()
    expect(flat.find(s => s.text === '◆ adage' && s.role === 'inhibit')).toBeDefined()
    expect(flat.filter(s => s.text === '◆ adage' && s.role === 'fire')).toHaveLength(2)
    expect(flat.filter(s => s.text === '✓ followed').map(s => s.role)).toEqual(['fire', 'fire'])
    expect(flat.filter(s => s.text === ' ───▶ ').map(s => s.role)).toEqual(['fire', 'fire', 'ink3'])
    expect(flat.find(s => s.text === 'scripts/wi-lookup W-2042 --json' && s.role === 'command')).toBeDefined()
    expect(flat.find(s => s.text.includes('1 failed'))?.role).toBe('inhibit')
    expect(flat.filter(s => s.text.startsWith('╰─▶') || s.text.startsWith('├─▶')).map(s => s.role)).toEqual(['fire', 'fire', 'inhibit'])
  })

  test('80 columns: the same boxes, narrower, the times kept; never wider than the pane at any width', () => {
    const d = draw(80)
    expect(d.lines.map(text).slice(3, 6)).toEqual([
      '  │ │  ╭─ ◆ adage · ● FIRE ────────────╮      ╭─ RUN ─────────────────────────╮',
      '  │ ╰─▶│ look up the work item with s… │ ───▶ │ scripts/wi-lookup W-2042 --j… │',
      `  │    ╰─ at your prompt · ${c(0)} ───╯      ╰─ ✓ followed ──────────────────╯`,
    ])
    expect(text(d.lines[6] ?? [])).toBe(`  ├── Bash  scripts/wi-lookup W-2042 --json        ✓ followed adage  ${c(2)}`)
    for (const w of [40, 59, 60, 61, 72, 80, 99, 100, 120, 160, 220]) {
      for (const l of [draw(w).head, ...draw(w).lines]) expect(cols(text(l))).toBeLessThanOrEqual(w)
    }
  })

  test('60 columns: still boxes, the strength word kept whole and the time over the event\'s words; no times on the trunk', () => {
    const lines = draw(WIDE).lines.map(text)
    expect(lines.slice(0, 7)).toEqual([
      '╭─ PROMPT 1 ───────────────────────────────────────────────╮',
      '│ fix W-2042 and add the currency field                    │',
      '╰─┬─┬──────────────────────────────────────────────────────╯',
      '  │ │  ╭─ ◆ adage · ● FIRE ──╮      ╭─ RUN ───────────────╮',
      '  │ ╰─▶│ look up the work i… │ ───▶ │ scripts/wi-lookup … │',
      `  │    ╰─ ${c(0)} ──────────╯      ╰─ ✓ followed ────────╯`,
      '  ├── Bash  scripts/wi-lookup W-2042 -…  ✓ followed adage',
    ])
    expect(lines).toContain('  │ │  ╭─ ◆ adage ● INHIBIT ─╮      ╭─ RUN ───────────────╮')
    expect(text(draw(WIDE).head)).toBe('◆ this session · adage fired 3 reflexes · 2 followed')
  })

  test('under 60 columns, and wherever boxes cannot be drawn (the desktop), one line per node down a trunk', () => {
    const expected = [
      ' › PROMPT 1  fix W-2042 and add the currency field',
      ' │ ◆ adage ● FIRE · look up the work item with scripts/wi-…',
      ' │   ──▶ scripts/wi-lookup W-2042 --json  ✓ followed',
      ' ├ Bash  scripts/wi-lookup W-2042 --json  ✓ followed adage',
      ' ├ Read ×2 · Grep',
      ' ├ Edit  schema/invoice.json',
      ' │ ◆ adage ● FIRE · run make gen after a schema edit',
      ' │   ──▶ make gen  ✓ followed',
      ' ├ Bash  make gen  ✓ followed adage',
      ' │ ↪ adage pointed this test run at .venv/bin/python',
      ' ┊ 3 steps, nothing fired  Read · Edit ×2 · Bash',
      ' ├ Bash  python3 -m unittest  ✗ 1 failed',
      ' │ ◆ adage ● INHIBIT · avoid python3 -m unittest here; use…',
      ' │   ──▶ make gen',
      ' ├ Bash  make test',
      ' │ ◆ adage chose this step · no model request',
      ' ╰ ✓ done',
      '',
      ' › PROMPT 2  now ship it',
      ' ├ Bash  git status',
    ]
    expect(draw(59).lines.map(text)).toEqual(expected)
    // The desktop's tree at any width: the same list (a native Button would break a box's borders).
    expect(draw(160, {}, { boxes: false }).lines.map(text).slice(0, 3)).toEqual([
      ' › PROMPT 1  fix W-2042 and add the currency field',
      ' │ ◆ adage ● FIRE · look up the work item with scripts/wi-lookup',
      ' │   ──▶ scripts/wi-lookup W-2042 --json  ✓ followed',
    ])
    for (const l of draw(40).lines) expect(cols(text(l))).toBeLessThanOrEqual(40)
  })

  test('ADAGE_ASCII=1: every glyph its ASCII twin, at every tier', () => {
    for (const w of [100, 80, 60, 50]) for (const l of [draw(w, {}, { ascii: true }).head, ...draw(w, {}, { ascii: true }).lines]) expect(text(l)).toMatch(ASCII)
    const lines = draw(100, {}, { ascii: true }).lines.map(text)
    expect(lines[2]).toMatch(/^\+-\+-\+-+\+$/)
    expect(lines[4]).toBe('  | `->| look up the work item with scripts/w... | ---> | scripts/wi-lookup W-2042 --json         |')
    expect(lines).toContain('  :   3 steps, nothing fired  Read - Edit x2 - Bash')
    expect(lines[21]).toMatch(/^ {2}`-- \+ done +\d\d:\d\d:\d\d$/)
  })

  test('a quiet stretch of three or more folds into one line; the newest step never folds; a failed call is counted on its batch', () => {
    const quiet = (n: number): GraphNode[] => Array.from({ length: n }, (_, k) => ({ kind: 'tools' as const, at: t(30 + k), from: t(30 + k), tools: ['Read'], ids: [`q${k}`], detail: '' }))
    const prompt: GraphNode = { kind: 'prompt', at: t(29), n: 1, text: 'look around' }
    const two = draw(100, { nodes: [prompt, ...quiet(2), { kind: 'stop', at: t(40), gate: false }], rows: [], now: t(41) }).lines.map(text)
    expect(two.filter(l => l.includes('├── Read'))).toHaveLength(QUIET_RUN - 1)
    const many = draw(100, { nodes: [prompt, ...quiet(7)], rows: [], now: t(60) }).lines.map(text)
    expect(many.filter(l => l.includes('6 steps, nothing fired'))).toHaveLength(1)
    // The newest is the live tip: drawn on its own.
    expect(many.at(-1)).toMatch(/^ {2}├── Read +\d\d:\d\d:\d\d$/)
    expect(toolsLabel({ kind: 'tools', at: 0, from: 0, ids: [], detail: '', tools: ['mcp__claude_ai_Notion__notion-search', 'Bash', 'Bash'] }, false).map(s => s.text).join('')).toBe('notion-search · Bash ×2')
  })

  test('following the newest: only the newest blocks that fit, a line saying what scrolled off; height 0 draws them all', () => {
    const d = draw(100, {}, { height: 16 })
    expect(d.lines.length).toBeLessThanOrEqual(16)
    expect(text(d.lines[0] ?? [])).toBe('  ┊ ↑ earlier: 1 prompt · 3 steps')
    expect(d.earlier).toEqual({ prompts: 1, steps: 3 })
    expect(text(d.lines.at(-1) ?? [])).toContain('git status')
    expect(draw(100).lines.length).toBe(27)
    expect(text(draw(50, {}, { height: 6 }).lines[0] ?? [])).toMatch(/^ ┊ ↑ earlier: 1 prompt · \d+ steps$/)
  })

  test('the newest glows for a moment (`now`, the accent) and settles; under reduced motion it is only bold', () => {
    const last = (d: ReturnType<typeof draw>) => d.lines.at(-1) ?? []
    const glowing = last(draw(100))
    expect(text(glowing)).toMatch(/now$/)
    expect(glowing.find(s => s.text === 'now')?.role).toBe('accent')
    expect(glowing.find(s => s.text === 'Bash')).toMatchObject({ bold: true, role: 'accent' })
    const settled = last(draw(100, { now: t(22) + PULSE_MS + 1 }))
    expect(text(settled)).toMatch(new RegExp(`${c(22)}$`))
    expect(settled.find(s => s.text === 'Bash')).toMatchObject({ bold: true })
    expect(settled.find(s => s.text === 'Bash')?.role).toBeUndefined()
    const still = last(draw(100, { motion: false }))
    expect(text(still)).toMatch(new RegExp(`${c(22)}$`))
    // A reflex that is the newest node: its box bold, its caption `now`.
    const fired = draw(100, { nodes: NODES.slice(0, 6), now: t(4) + 100 }).lines.map(text)
    expect(fired.at(-1)).toContain('after this step · now')
  })

  test('everything Adage did says adage; the agent\'s own steps do not; no reflex id is drawn', () => {
    for (const w of [100, 59]) {
      const lines = draw(w).lines.map(text)
      // Each reflex's branch is headed `◆ adage`; each other act, and each step the agent followed it in, says so too.
      for (const l of lines.filter(l => /[◆↪]|INHIBIT|FIRE|chose this step|pointed this/.test(l))) expect(l).toContain('adage')
      expect(lines.filter(l => l.includes('followed adage'))).toHaveLength(2)
      for (const id of ['lookup-work-item', 'codegen-after-schema', 'dead-end-unittest']) expect(lines.join('\n')).not.toContain(id)
    }
    expect(draw(100).lines.map(text).filter(l => l.includes('Read ×2'))[0]).not.toContain('adage')
  })

  test('empty: no lines; the summary says no reflex has fired; the finish line is Adage\'s', () => {
    const d = draw(100, { nodes: [] })
    expect(d.lines).toEqual([])
    expect(text(d.head)).toBe('◆ this session · adage has fired no reflex yet · 0 prompts · 0 steps')
    const gate = draw(100, { nodes: [{ kind: 'prompt', at: t(0), n: 1, text: 'go' }, { kind: 'stop', at: t(1), gate: true }], now: t(9) }).lines.map(text)
    expect(gate.at(-1)).toMatch(/^ {2}├── ◆ adage finish line · the agent continues once +\d\d:\d\d:\d\d$/)
  })
})
