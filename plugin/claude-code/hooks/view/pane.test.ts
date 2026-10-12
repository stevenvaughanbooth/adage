// The pane's words (view/pane.ts; ux-design §6, §4.17; T-B3-02 … T-B3-06, T-B4-06): the Live rows
// and their filter, the inspect text, what waits for the person, the Health checks and their one action.
import { describe, expect, test } from 'claude-code/testing'

import { BANNED, DOCTOR, GRAPH, LINE, SESSION, rowsOf as rows } from '../fixtures/kit'
import { checksOf, reflexesOf } from '../reads'
import { GROUP_TITLES, VERB_HOTKEYS, VERB_WORDS, WAITING_TITLE, actFlows, boxWidth, rowLines, checkAction, clock, flowBoxes, flowLine, inspectText, listLabel, pulseLine, pulses, reflexFacts, reflexLine, responseOf, reviewCommand, stimulusOf, tickText, trackTitle, visibleReflexes, visibleRows, waiting, worthAsking } from './pane'
import type { Seg } from './pane'
import { cols } from './text'

const REFLEXES = reflexesOf(GRAPH)

describe('T-B3-02 … T-B3-04 · the Live rows', () => {
  test('the filter keeps matching rows, newest first; the quiet switch hides rows with nothing decided', () => {
    const r = rows(LINE.start, LINE.firedGen, LINE.firedLookup)
    expect(visibleRows(r, 'lookup', false, false).map(x => x.seq)).toEqual([3])
    expect(visibleRows(r, '', false, false).map(x => x.seq)).toEqual([3, 2, 1])
    expect(visibleRows(r, '', true, false).map(x => x.seq)).toEqual([3, 2])
  })

  test('a noticed sequence (plan D2) reads as noticed, in both arms', () => {
    const [row] = rows(JSON.stringify({ kind: 'decision', cursor: 'c', ts: 0, session: SESSION, harness: 'claude', event: 'tools', acts: [{ act: 'noticed', candidate: 'make test → make lint', support: 3, s_min: 5, review: false }], texts: [] }))
    expect(row?.acts).toEqual([{ act: 'noticed', candidate: 'make test → make lint', support: 3, s_min: 5, review: false }])
  })

  test('the inspect pane: the directive as the agent read it, and its evidence', () => {
    const [row] = rows(LINE.firedGen)
    if (row === undefined) throw new Error('row')
    const text = inspectText(row, REFLEXES)
    expect(text).toContain('### run make gen after a schema edit')
    expect(text).toContain('must be followed by running')
    expect(text).toContain('_13 of 17 sessions ran it after the edit_')
  })
})

describe('T-B3-05, T-B4-06 · the Reflexes rows and what waits', () => {
  test('what waits for the person: a reflex to enable, a grant offered, a pack to trust', () => {
    const offered = reflexesOf({ routes: [{ ...GRAPH.routes[0], review: { verbs: [], reasons: [], offers: [{ kind: 'predict', command: 'adage review delegate acme/learned codegen-after-schema --predict' }] } }] })
    const w = waiting([...REFLEXES, ...offered], [{ name: 'alice/acme', command: 'adage add x --trust' }])
    expect(w.map(x => x.kind)).toEqual(['trust', 'enable', 'allow'])
    expect(w.map(x => x.group)).toEqual(['trust', 'review', 'predict'])
    for (const x of w) expect(x.text).not.toMatch(BANNED)
    for (const t of [WAITING_TITLE, ...Object.values(GROUP_TITLES)]) expect(t).not.toMatch(BANNED)
  })

  test('a quiet reflex waits last and is never asked about or counted; the groups keep their order', () => {
    const quiet = reflexesOf({ routes: [{ ...GRAPH.routes[0], id: 'quiet-one', review: { verbs: ['approve'], reasons: [{ kind: 'quiet', why: 'x' }] } }] })
    const execute = reflexesOf({ routes: [{ ...GRAPH.routes[1], review: { verbs: [], reasons: [], offers: [{ kind: 'execute', command: 'adage review delegate acme/learned lookup-work-item --level e2 --stage --json' }] } }] })
    const w = waiting([...quiet, ...execute, ...REFLEXES], [])
    expect(w.map(x => x.group)).toEqual(['review', 'execute', 'quiet'])
    expect(worthAsking(w).map(x => x.group)).toEqual(['review', 'execute'])
  })

  test('the ticker names a reflex by what it tells the agent, never its id, and leaves out the index hash', () => {
    const at = { ts: 0, tone: 'change', strength: null }
    expect(tickText({ ...at, text: 'learned: codegen-after-schema evidence (12/16 → 13/17)' }, false, REFLEXES)).toMatch(/learned: run make gen after a schema edit evidence \(12\/16 → 13\/17\)$/)
    expect(tickText({ ...at, text: 'auto-condition: 9/9 traces used, 0.7 s, no reflex changed, gen 21aa617d' }, false, REFLEXES)).toMatch(/no reflex changed$/)
    expect(tickText({ ...at, text: 'learned: sequence-12fccd changed' }, false, REFLEXES)).toMatch(/sequence-12fccd changed$/)
  })

  test('a Reflexes row leads with what the reflex tells the agent; the id is in its facts; tracks in plain words', () => {
    const [r] = REFLEXES
    if (r === undefined) throw new Error('reflex')
    expect(reflexLine(r, false)).toBe('◠ FIRE    run make gen after a schema edit  firm · 13/17 sessions')
    expect(reflexLine({ ...r, gist: 'after editing crates/**/*.rs, run cargo fmt --all' }, false)).toBe('◠ FIRE    cargo fmt --all · after editing crates/**/*.rs  firm · 13/17 sessions')
    expect(listLabel('after editing src/**, then make build, run make test', '-')).toBe('make test - after editing src/**, then make build')
    expect(listLabel("Cargo.lock: let cargo update it; don't edit it by hand", '·')).toBe("Cargo.lock: let cargo update it; don't edit it by hand")
    expect(reflexLine(r, false)).not.toContain(r.id)
    expect(reflexFacts(r, false)).toBe('codegen-after-schema · when an edit touches schema/*.json → make gen · lb 0.53')
    expect(trackTitle(r)).toBe('learned in this repository')
    expect(trackTitle({ ...r, reference: 'acme/instincts' })).toContain('built-in defaults')
    expect(trackTitle({ ...r, reference: 'acme/team', imported: true, signer: 'alice@acme.com' })).toBe('shared: team  alice@acme.com')
  })

  test('the filters: fuzzy words over id, trigger, step and track; strength; lifecycle', () => {
    expect(visibleReflexes(REFLEXES, 'prompt work', 'all', 'all').map(r => r.id)).toEqual(['lookup-work-item'])
    expect(visibleReflexes(REFLEXES, '', 'inhibit', 'all').map(r => r.id)).toEqual(['dead-end-unittest'])
    expect(visibleReflexes(REFLEXES, '', 'all', 'nascent').map(r => r.id)).toEqual(['test-after-acme-edit'])
  })

  test('each review verb is worded and has its own hotkey; the command is the terminal one', () => {
    expect(Object.keys(VERB_WORDS)).toEqual(Object.keys(VERB_HOTKEYS))
    expect(new Set(Object.values(VERB_HOTKEYS)).size).toBe(6)
    const [r] = REFLEXES
    if (r === undefined) throw new Error('reflex')
    expect(reviewCommand('approve', r)).toBe('adage review approve acme/learned codegen-after-schema')
    for (const w of Object.values(VERB_WORDS)) expect(w).not.toMatch(BANNED)
  })
})

describe('T-B3-06 · the Health checks', () => {
  test('the checks and the one action a session may run', () => {
    const checks = checksOf(DOCTOR)
    expect(checks.map(checkAction)).toEqual([undefined, { kind: 'run', argv: ['condition'] }, { kind: 'copy', command: 'adage install claude-code --write' }, undefined])
  })
})

describe('T-B3-11 · the Reflexes flow: stimulus → reflex → response', () => {
  const [gen, lookup] = REFLEXES
  if (gen === undefined || lookup === undefined) throw new Error('fixture')
  const text = (l: readonly Seg[]) => l.map(x => x.text).join('')
  const ASCII = /^[\x20-\x7e]*$/

  test('the boxes are titled as the map titles them; an advice drops its preamble; a sequence speaks its gist', () => {
    expect(stimulusOf(gen)).toEqual({ title: 'AFTER EDIT', text: 'schema/*.json changed', phrase: 'after editing schema/*.json' })
    expect(stimulusOf(lookup)).toEqual({ title: 'PROMPT', text: 'names a work item', phrase: 'when a prompt names a work item' })
    expect(responseOf(gen)).toEqual({ title: 'RUN', text: 'make gen', command: true })
    const [advice, sequence] = reflexesOf({ routes: [
      { ...GRAPH.routes[0], id: 'lock', trigger: { kind: 'after_edit', label: 'an edit touches Cargo.lock' }, step: { kind: 'advise', label: 'In this repository, Cargo.lock is maintained by cargo.' } },
      { ...GRAPH.routes[0], id: 'seq', gist: 'after editing src/*.rs, then cargo build, run cargo test', trigger: { kind: 'context', label: 'a context of 2 events' } },
    ] })
    if (advice === undefined || sequence === undefined) throw new Error('fixture')
    expect(responseOf(advice)).toEqual({ title: 'ADVICE', text: 'Cargo.lock is maintained by cargo.', command: false })
    expect(stimulusOf(sequence)).toMatchObject({ title: 'SEQUENCE', phrase: 'after editing src/*.rs, then cargo build' })
  })

  test('wide: three rounded boxes joined by arrows, never wider than the pane; the reflex box is the Button; ASCII twins', () => {
    const lines = flowBoxes(gen, undefined, 100, false)
    if (lines === undefined) throw new Error('boxes')
    expect(lines.map(text)).toEqual([
      '  ╭─ AFTER EDIT ─────────────╮      ╭─ ● FIRE ─────────────────╮      ╭─ RUN ────────────────────╮',
      '  │ schema/*.json changed    │ ───▶ │ ◠ firm · 13/17 · lb 0.53 │ ───▶ │ make gen                 │',
      '  ╰──────────────────────────╯      ╰──────────────────────────╯      ╰──────────────────────────╯',
    ])
    expect(lines[1]?.filter(x => x.press).map(x => x.text)).toEqual(['◠ firm · 13/17 · lb 0.53'])
    expect(lines[0]?.find(x => x.text === '●')?.role).toBe('fire')
    expect(lines[1]?.find(x => x.text === 'make gen')?.role).toBe('command')
    // At rest no border is lit; the open row's are the accent.
    expect(lines.flat().some(x => x.role === 'fire' && x.text.includes('─'))).toBe(false)
    expect(flowBoxes(gen, undefined, 100, false, true)?.[0]?.[1]?.role).toBe('accent')
    for (const w of [74, 100, 140, 220]) for (const l of flowBoxes(gen, undefined, w, false) ?? []) expect(cols(text(l))).toBeLessThanOrEqual(w)
    for (const l of flowBoxes(gen, undefined, 100, true) ?? []) expect(text(l)).toMatch(ASCII)
    expect(text(flowBoxes(gen, undefined, 100, true)?.[1] ?? [])).toContain('| ---> | ~ firm - 13/17 - lb 0.53 | ---> |')
  })

  test('narrow: one line, the stimulus cut first; under 74 columns the boxes give way to it', () => {
    expect(boxWidth(74)).toBeGreaterThan(0)
    expect(boxWidth(73)).toBe(0)
    expect(flowBoxes(gen, undefined, 60, false)).toBeUndefined()
    expect(text(flowLine(gen, undefined, 64, false))).toBe('● after editing schema/*.json ──▶ ◠ ──▶ make gen  FIRE · firm')
    // The strength and lifecycle words go first when the line is short of room.
    expect(text(flowLine(gen, undefined, 60, false))).toBe('● after editing schema/*.json ──▶ ◠ ──▶ make gen')
    expect(flowLine(gen, undefined, 60, false).filter(x => x.press).map(x => x.text)).toEqual(['after editing schema/*.json ──▶ ◠ ──▶ make gen'])
    const tight = text(flowLine(lookup, undefined, 40, false))
    expect(cols(tight)).toBeLessThanOrEqual(40)
    expect(tight).toMatch(/^● when a … ──▶ ◠ ──▶ scripts\/wi-lookup/)
    expect(text(flowLine(gen, undefined, 64, true))).toBe('* after editing schema/*.json --> ~ --> make gen  FIRE - firm')
  })

  test('fired in this session: the time from this session\'s feed rows, followed once the agent did it, the last one brightest; another session\'s rows never light it', () => {
    const r = rows(LINE.firedGen, LINE.followedGen, LINE.firedLookup, LINE.otherControl)
    const ps = pulses([{ event: 'UserPromptSubmit', route: 'dead-end-unittest', track: 'acme/learned', kind: 'reflex', strength: 'inhibit' }], r)
    expect([...ps.keys()].sort()).toEqual(['codegen-after-schema', 'dead-end-unittest', 'lookup-work-item'])
    expect(ps.get('codegen-after-schema')).toEqual({ at: r[0]?.ts, followed: true, latest: false })
    expect(ps.get('lookup-work-item')).toEqual({ at: r[2]?.ts, followed: false, latest: true })
    expect(ps.get('dead-end-unittest')).toEqual({ at: null, followed: false, latest: false })
    expect(pulseLine(ps, false)).toBe(`◆ 3 reflexes fired in this session · last at ${clock(r[2]?.ts ?? 0)}`)
    expect(pulseLine(new Map(), false)).toBe('')
    expect(pulses([], rows(LINE.otherControl)).size).toBe(0)

    const lit = flowBoxes(gen, ps.get('codegen-after-schema'), 100, false) ?? []
    const at = clock(r[0]?.ts ?? 0)
    expect(text(lit[2] ?? [])).toContain(`╰─ ◆ fired ${at} `)
    expect(text(lit[2] ?? [])).toContain('╰─ ✓ followed ')
    // The signal crosses: both arrows and every border are in the fire role.
    expect(lit[1]?.filter(x => x.text.includes('▶')).map(x => x.role)).toEqual(['fire', 'fire'])
    expect(lit[0]?.filter(x => x.text.startsWith('╭')).map(x => x.role)).toEqual(['fire', 'fire', 'fire'])
    const once = flowBoxes(lookup, { at: 0, followed: false, latest: true }, 100, false) ?? []
    expect(once[1]?.filter(x => x.text.includes('▶')).map(x => x.role)).toEqual(['fire', 'ink3'])
    expect(text(flowLine(gen, ps.get('codegen-after-schema'), 60, false))).toMatch(new RegExp(`^● ◆ ${at.slice(0, 5)} after editing`))
    // A reflex is named by what it does, never by its id (ux-design §1.10).
    for (const l of [...lit, flowLine(gen, ps.get('codegen-after-schema'), 60, false)]) expect(text(l)).not.toContain('codegen-after-schema')
  })

  test('Live: a directive\'s flow in small, under its act, aligned past the head; other acts, unknown reflexes and narrow panes draw none', () => {
    const [gen1] = rows(LINE.firedGen)
    const [followed] = rows(LINE.followedGen)
    if (gen1 === undefined || followed === undefined) throw new Error('rows')
    const [flow] = actFlows(gen1, REFLEXES, 100, false)
    // Where the act begins on the row's first line: the flow hangs beneath it.
    const head = (rowLines(gen1, false)[0] ?? '').indexOf('→')
    expect(head).toBeGreaterThan(20)
    expect(text(flow ?? [])).toBe(`${' '.repeat(head)}╰ after editing schema/*.json ──▶ ◠ ──▶ make gen`)
    expect(flow?.filter(x => x.text.includes('▶')).map(x => x.role)).toEqual(['fire', 'fire'])
    expect(text(actFlows(gen1, REFLEXES, 100, true)[0] ?? [])).toBe(`${' '.repeat(head)}\` after editing schema/*.json --> ~ --> make gen`)
    expect(actFlows(followed, REFLEXES, 100, false)).toEqual([undefined])
    expect(actFlows(gen1, [], 100, false)).toEqual([undefined])
    expect(actFlows(gen1, REFLEXES, 60, false)).toEqual([undefined])
    // Both ends share the room when they do not fit: never wider than the pane.
    for (const w of [70, 80, 90]) expect(cols(text(actFlows(gen1, REFLEXES, w, false)[0] ?? []))).toBeLessThanOrEqual(w)
  })
})

