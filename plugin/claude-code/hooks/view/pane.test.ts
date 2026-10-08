// The pane's words (view/pane.ts; ux-design §6, §4.17; T-B3-02 … T-B3-06, T-B4-06): the Live rows
// and their filter, the inspect text, what waits for the person, the Health checks and their one action.
import { describe, expect, test } from 'claude-code/testing'

import { BANNED, DOCTOR, GRAPH, LINE, SESSION, rowsOf as rows } from '../fixtures/kit'
import { checksOf, reflexesOf } from '../reads'
import { GROUP_TITLES, VERB_HOTKEYS, VERB_WORDS, WAITING_TITLE, checkAction, inspectText, listLabel, reflexFacts, reflexLine, reviewCommand, tickText, trackTitle, visibleReflexes, visibleRows, waiting, worthAsking } from './pane'

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
