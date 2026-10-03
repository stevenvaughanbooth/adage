// The pane's words (view/pane.ts; ux-design §6, §4.17; T-B3-02 … T-B3-06, T-B4-06): the Live rows
// and their filter, the inspect text, what waits for the person, the Health checks and their one action.
import { describe, expect, test } from 'claude-code/testing'

import { BANNED, DOCTOR, GRAPH, LINE, SESSION, rowsOf as rows } from '../fixtures/kit'
import { checksOf, reflexesOf } from '../reads'
import { VERB_HOTKEYS, VERB_WORDS, checkAction, inspectText, reviewCommand, visibleReflexes, visibleRows, waiting } from './pane'

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
    expect(w.map(x => x.kind)).toEqual(['enable', 'allow', 'trust'])
    for (const x of w) expect(x.text).not.toMatch(BANNED)
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
