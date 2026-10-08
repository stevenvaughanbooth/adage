// Reading the binary's `--json` objects (reads.ts; architecture §2; ux-design §1.7): the card, the
// reflexes, the checks, the lapsed grants, the packs and candidates, the map's address, the routine
// model, the opt-in lines. Nothing that would name the arm is kept; anything malformed is nothing.
import { describe, expect, test } from 'claude-code/testing'

import { CANDIDATE, CANDIDATE_REVIEW, CANDIDATE_SINGLE, CARD, DOCTOR, GRAPH, LAPSED } from './fixtures/kit'
import { candidatesOf, cardOf, checksOf, lapsedOf, mapUrl, optInOf, packsOf, reflexesOf, repoOf, routineModelOf, updateOf, updatedOf } from './reads'

const REFLEXES = reflexesOf(GRAPH)

describe('T-X-06, T-B3-05 · reading the binary', () => {
  test('the reflexes keep nothing that would name the arm', () => {
    expect(JSON.stringify(REFLEXES)).not.toMatch(/control|would_have|arms|unaided/)
    expect(REFLEXES[0]).toMatchObject({ id: 'codegen-after-schema', reference: 'acme/learned', support: 13, opportunities: 17, lowerBound: 0.53, verbs: ['strengthen', 'pin'], uplift: 'uplift +0.30' })
  })

  test("the reflexes of this repository only: the graph lists every repository's; the card names this one", () => {
    const other = { ...GRAPH.routes[0], id: 'elsewhere', repo: 'github.com/someone/else' }
    const unnamed = { ...GRAPH.routes[0], id: 'unnamed', repo: '' }
    const graph = { routes: [...GRAPH.routes, other, unnamed] }
    expect(repoOf(CARD)).toBe(CARD.repo)
    expect(repoOf('x')).toBe('')
    expect(reflexesOf(graph, repoOf(CARD)).map(r => r.id)).not.toContain('elsewhere')
    expect(reflexesOf(graph, repoOf(CARD)).map(r => r.id)).toContain('unnamed')
    expect(reflexesOf(graph).map(r => r.id)).toContain('elsewhere')
  })

  test('lapsed grants, packs awaiting trust, the map address', () => {
    expect(lapsedOf({ lapsed: LAPSED })).toEqual([{ route: 'acme/learned/codegen-after-schema', changed: ['argv_templates'], reapprove: 'adage review delegate acme/learned codegen-after-schema --predict' }])
    expect(packsOf({ packs_awaiting_trust: [{ name: 'alice/acme', path: '.adage/packs/acme', command: 'adage add .adage/packs/acme --trust' }] })).toEqual([{ name: 'alice/acme', command: 'adage add .adage/packs/acme --trust' }])
    expect(mapUrl('map  http://127.0.0.1:43123/ · read-only')).toBe('http://localhost:43123/')
    expect(mapUrl('nothing')).toBeUndefined()
  })

  test('the checks: label, status, the one action or manual; anything else a pass', () => {
    expect(checksOf(DOCTOR).map(c => `${c.label}:${c.status}:${c.action?.kind ?? 'none'}`)).toEqual(['binary:pass:none', 'index:warn:run', 'hooks: claude-code:warn:run', 'embedding model:fail:manual'])
    expect(checksOf({ checks: [{ check: 'x', status: 'odd' }] })).toEqual([{ label: 'x', status: 'pass', detail: '', fix: '', action: null }])
  })

  test('the card: its header, rows and next; anything that is not one is nothing', () => {
    expect(cardOf(CARD)?.header).toBe(CARD.header)
    expect(cardOf(CARD)?.next.map(n => n.command)).toEqual(['/adage watch', 'adage map', 'adage doctor'])
    expect(cardOf({})).toBeUndefined()
    expect(cardOf('x')).toBeUndefined()
  })

  test('T-C5-04 · the routine model and the opt-in lines the card reports', () => {
    expect(routineModelOf({ runtime: { routine_model: ' haiku ' } })).toBe('haiku')
    expect(routineModelOf({ runtime: {} })).toBe('')
    expect(routineModelOf(undefined)).toBe('')
    expect(optInOf({ ui: { spinner: true, noticed: true } })).toEqual({ spinner: true, turnLine: false, noticed: true })
    expect(optInOf({ ui: 'on' })).toEqual({ spinner: false, turnLine: false, noticed: false })
  })
})

describe('T-D5-01 · the candidates', () => {
  test('candidates as status --json lists them: the gist, never the id; a sequence is a context', () => {
    expect(candidatesOf({ candidates: [CANDIDATE, CANDIDATE_REVIEW, CANDIDATE_SINGLE] })).toEqual([
      { id: 'run-a54d7c', say: 'after make test, run make lint', sequence: true },
      { id: 'lookup-then-read', say: 'scripts/wi-lookup then reading the files it names', sequence: true },
      { id: 'run-single', say: 'run make lint', sequence: false },
    ])
    expect(candidatesOf({})).toEqual([])
  })
})

describe('T-O6-02, T-O9-07 · a newer Adage, as the binary says it', () => {
  const UPDATE = { current: '0.1.0', latest: '0.2.0', channel: 'plugin', next: '/adage update', text: '0.2.0 is out (you have 0.1.0)', published_at: null, url: null, notes: 'fixes hook latency\n\nfaster learning\nthird\nfourth' }

  test("the card's update, drawn as it says it; none when absent or malformed", () => {
    expect(updateOf({ ...CARD, update: UPDATE })).toEqual({ have: '0.1.0', latest: '0.2.0', next: '/adage update', notes: ['fixes hook latency', 'faster learning', 'third'] })
    expect(cardOf({ ...CARD, update: UPDATE })?.update?.latest).toBe('0.2.0')
    expect(cardOf(CARD)?.update).toBeUndefined()
    expect(cardOf(CARD)?.version).toBe('0.1.0')
    for (const bad of [null, 'x', { ...UPDATE, latest: 'soon' }, { ...UPDATE, current: '' }, { ...UPDATE, next: '' }]) expect(updateOf({ ...CARD, update: bad })).toBeUndefined()
    expect(updateOf({ ...CARD, update: { ...UPDATE, notes: null } })?.notes).toEqual([])
  })

  test("a pressed update's last line: updated, latest or manual; anything else nothing", () => {
    expect(updatedOf(['{"next":"x"}', 'adage update · 0.1.0 → 0.2.0', '{"updated":{"from":"0.1.0","to":"0.2.0","effect":"next session"}}', ''])).toEqual({ kind: 'updated', from: '0.1.0', to: '0.2.0' })
    expect(updatedOf(['{"latest":{"version":"0.2.0"}}'])).toEqual({ kind: 'latest', version: '0.2.0' })
    expect(updatedOf(['{"manual":{"command":"adage update","say":"run adage update in a terminal"}}'])).toEqual({ kind: 'manual', command: 'adage update', say: 'run adage update in a terminal' })
    for (const bad of [[], ['done'], ['{"updated":{}}'], ['[1]'], ['{"updated":{"from":"0.1.0","to":"0.2.0"}}', 'trailing prose']]) expect(updatedOf(bad)).toBeUndefined()
  })
})
