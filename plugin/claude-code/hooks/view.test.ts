// The copy deck and the band as plain values (ux-design §5, §10): what each line says, how it
// narrows, and the words it never carries (T-B2-03, T-B2-06 … T-B2-11 in their pure parts).
import { describe, expect, test } from 'claude-code/testing'

import { BANNED, CARD, DOCTOR, GRAPH, LAPSED, LINE, SESSION } from './fixtures/kit'
import { parseLine } from './feed'
import type { Fired, Health, Mark, Row } from './plugin'
import {
  band,
  cardOf,
  cardText,
  checkAction,
  checksOf,
  cols,
  firedThisSession,
  inspectText,
  lapsedOf,
  learnToast,
  mapUrl,
  markText,
  newer,
  openNotice,
  packsOf,
  parseCardText,
  promptLines,
  receiptBeside,
  receiptCard,
  receiptOf,
  reflexesOf,
  rewriteOf,
  spinnerSuffix,
  turnSummary,
  versionLine,
  visibleRows,
  waiting,
  whyText,
  SAVINGS_ROWS,
} from './view'
import type { BandInput } from './view'

const REFLEXES = reflexesOf(GRAPH)
const rows = (...lines: string[]): Row[] =>
  lines.map((l, k) => {
    const p = parseLine(l, SESSION)
    if (p?.kind !== 'decision') throw new Error('not a decision')
    return { ...p.row, seq: k + 1 }
  })
const fired = (route: string, prompt = 1): Fired => ({ event: 'UserPromptSubmit', route, track: 'acme/learned@1', kind: 'run', strength: 'fire', prompt, promptText: 'fix W-2042' })
const HEALTHY: Health = { lights: [], word: 'healthy', lamp: 'fire', check: null, off: false }
const BASE: BandInput = { active: true, nobinary: '', paused: false, health: HEALTHY, inRepo: true, reflexes: 0, steps: 0, reused: 0, learner: null, now: 0, lapsed: 0, review: 0, versionLine: '', columns: 160, ascii: false }
const line1 = (i: BandInput) => {
  const b = band(i)
  return b === undefined ? undefined : b.cells.map(c => (c.glyph ? `${c.glyph} ${c.text}` : c.text)).join(' · ').replace('adage · ', 'adage  ')
}

describe('the opening line (open.notice)', () => {
  test("the binary's notice, its prefix stripped, with the glyph; another notice is not Adage's", () => {
    expect(openNotice('adage: 3 reflexes and 4 instincts active in this repo; it keeps learning as you work. Nothing runs without you.', false)).toBe(
      '◠ adage · 3 reflexes and 4 instincts active in this repo; it keeps learning as you work. Nothing runs without you.',
    )
    expect(openNotice('adage-exec: the adage plugin now includes execution', true)).toBe('~ adage - the adage plugin now includes execution')
    expect(openNotice('Using claude-x from settings', false)).toBeUndefined()
    expect(openNotice('adage: ', false)).toBeUndefined()
  })
})

describe('the line under a prompt (prompt.fired)', () => {
  test('three reflexes, three lines in delivery order, worded by what they tell the agent, no strength word', () => {
    const lines = promptLines([fired('lookup-work-item'), fired('codegen-after-schema'), fired('dead-end-unittest')], REFLEXES, rows(LINE.firedLookup), 120, false)
    expect(lines).toEqual([
      '◠ look up the work item with scripts/wi-lookup · scripts/wi-lookup W-2042 --json',
      '◠ run make gen after a schema edit',
      '◠ avoid python3 -m unittest here; use make test',
    ])
    for (const l of lines) expect(l).not.toMatch(BANNED)
  })

  test('at most three, each reflex once', () => {
    const many = ['lookup-work-item', 'lookup-work-item', 'codegen-after-schema', 'dead-end-unittest', 'test-after-acme-edit'].map(r => fired(r))
    expect(promptLines(many, REFLEXES, [], 120, false)).toHaveLength(3)
  })

  test('T-B2-03: at 40 columns the command goes first, then the sentence is cut', () => {
    const [l] = promptLines([fired('lookup-work-item')], REFLEXES, rows(LINE.firedLookup), 40, false)
    expect(l).toBe('◠ look up the work item with scripts/wi…')
    expect(cols(l ?? '')).toBe(40)
  })
})

describe('the markers under a tool row', () => {
  test('rewrite, predicted, avoided, substituted (T-B2-04)', () => {
    expect(markText({ id: 't', kind: 'rewrite', interpreter: '.venv/bin/python', what: 'test' }, false)).toBe('↪ adage: pointed this test run at .venv/bin/python')
    expect(markText({ id: 't', kind: 'rewrite', interpreter: '.venv/bin/python', what: 'python' }, false)).toBe('↪ adage: pointed this Python run at .venv/bin/python')
    expect(markText({ kind: 'predicted' }, false)).toBe('◠ adage chose this step · no model request')
    expect(markText({ id: 't', kind: 'avoided', route: 'r' }, false)).toBe('◠ adage: dead end headed off')
    expect(markText({ id: 't', kind: 'substituted', command: 'make test' }, false)).toBe('↪ adage ran make test instead')
  })

  test('a rewrite is read off the PreToolUse answer: the interpreter, and whether it runs tests', () => {
    expect(rewriteOf('pytest -q', '.venv/bin/python -m pytest -q')).toEqual({ interpreter: '.venv/bin/python', what: 'test' })
    expect(rewriteOf('python x.py', '.venv/bin/python x.py')).toEqual({ interpreter: '.venv/bin/python', what: 'python' })
    expect(rewriteOf('ls', 'ls')).toBeUndefined()
    expect(rewriteOf('ls', undefined)).toBeUndefined()
  })

  test('T-B2-05: the receipt card names the tests, cut at 2,400 bytes, and says the output is not shown', () => {
    const tests = Array.from({ length: 400 }, (_, k) => `test_case_number_${k}`)
    const m: Extract<Mark, { kind: 'answered' }> = { id: 't', kind: 'answered', receipt: 'a1f2', summary: '48 passed in 1.9 s', tests, ago: '4 s ago', command: 'python -m pytest tests -q' }
    const [first, second] = receiptCard(m, false)
    expect(first).toStartWith('✓ adage receipt a1f2 · 48 passed in 1.9 s · the tests added in this session: test_case_number_0, ')
    expect(new TextEncoder().encode(first.split('session: ')[1] ?? '').length).toBeLessThanOrEqual(2400)
    expect(second).toBe("pytest's own output is not shown; adage answered from its run 4 s ago on identical inputs (python -m pytest tests -q)")
    expect(receiptOf(['x', '<adage-receipt operation="a1f2c3d4e5f6">\nAdage ran it.\n</adage-receipt>'])).toBe('a1f2')
    expect(receiptOf(['<adage-reflex route="x">'])).toBeUndefined()
    expect(receiptBeside('a1f2', false)).toBe('◠ adage receipt a1f2 covers this run · /adage why')
  })
})

describe('the band (T-B2-07 … T-B2-09)', () => {
  test('every cell, in order, with the lamp word', () => {
    expect(line1({ ...BASE, reflexes: 2, steps: 1, reused: 1, learner: { state: 'waiting', nextAt: 38_000, ranAt: null } })).toBe(
      '◠ adage  2 reflexes this session · 1 step without a model request · 1 test run reused · ● healthy · learning in 38 s',
    )
  })

  test('narrowing drops from the right by priority; the lamp word is never dropped', () => {
    const i = { ...BASE, reflexes: 2, steps: 1, reused: 1, learner: { state: 'running' as const, nextAt: null, ranAt: null } }
    expect(line1({ ...i, columns: 60 })).toBe('◠ adage  2 reflexes this session · ● healthy')
    expect(line1({ ...i, columns: 40 })).toBe('◠ adage  ● healthy')
    expect(line1({ ...i, columns: 10 })).toBe('◠ adage  ● healthy')
    for (const columns of [60, 40, 20, 10]) expect(line1({ ...i, columns })).toContain('healthy')
  })

  test('quiet: nothing to say and green health leave the band to others', () => {
    expect(band(BASE)).toBeUndefined()
    expect(band({ ...BASE, active: false })).toBeUndefined()
    expect(band({ ...BASE, inRepo: false, reflexes: 3 })).toBeUndefined()
  })

  test('off, paused, no binary: one cell each', () => {
    expect(line1({ ...BASE, health: { ...HEALTHY, off: true } })).toBe('◠ adage ○ off in this repository · /adage on')
    expect(line1({ ...BASE, paused: true, reflexes: 2 })).toBe('◠ adage  paused · /adage pause')
    expect(line1({ ...BASE, active: false, nobinary: 'no binary for darwin-arm64' })).toBe('◠ adage ○ off · run adage doctor')
  })

  test('the second line: one item, lapsed > problem > review > version', () => {
    const problem: Health = { lights: [], word: 'problem', lamp: 'inhibit', check: 'hook latency', off: false }
    const v = versionLine('2.1.300', '2.1.288', false)
    expect(band({ ...BASE, lapsed: 1, health: problem, review: 2, versionLine: v })?.second).toBe('» 1 grant lapsed · /adage health')
    expect(band({ ...BASE, health: problem, review: 2, versionLine: v })?.second).toBe('» problem: hook latency · /adage health')
    expect(band({ ...BASE, review: 2, versionLine: v })?.second).toBe('» 2 need review · /adage review')
    expect(band({ ...BASE, versionLine: v })?.second).toBe('» Claude Code 2.1.300 is newer than this plugin was tested with (2.1.288) · run adage doctor')
  })

  test('lamp roles and words: healthy, check, problem; the off glyph only when off', () => {
    const cell = (h: Health) => band({ ...BASE, reflexes: 1, health: h })?.cells.find(c => c.glyph !== undefined)
    expect(cell(HEALTHY)).toEqual({ glyph: '●', lamp: 'fire', text: 'healthy' })
    expect(cell({ ...HEALTHY, word: 'check', lamp: 'prime', check: 'hooks: claude-code' })).toEqual({ glyph: '●', lamp: 'prime', text: 'check: hooks: claude-code' })
    expect(cell({ ...HEALTHY, word: 'problem', lamp: 'inhibit', check: 'index' })).toEqual({ glyph: '●', lamp: 'inhibit', text: 'problem: index' })
    expect(line1({ ...BASE, reflexes: 1 })).not.toContain('○')
  })

  test('T-B2-11: every glyph its ASCII twin', () => {
    expect(line1({ ...BASE, reflexes: 1, ascii: true, learner: { state: 'running', nextAt: null, ranAt: null } })).toBe('~ adage  1 reflex this session · * healthy · learning...')
    expect(markText({ kind: 'predicted' }, true)).toBe('~ adage chose this step - no model request')
    expect(promptLines([fired('codegen-after-schema')], REFLEXES, [], 80, true)).toEqual(['~ run make gen after a schema edit'])
  })

  test('learning: the countdown, then running, then learned for 10 s', () => {
    const at = (state: 'waiting' | 'running' | 'idle', now: number, ranAt: number | null = null) => line1({ ...BASE, now, learner: { state, nextAt: 38_000, ranAt } })
    expect(at('waiting', 0)).toContain('learning in 38 s')
    expect(at('running', 0)).toContain('learning…')
    expect(at('idle', 5000, 0)).toContain('learned')
    expect(at('idle', 11_000, 0)).toBeUndefined()
  })

  test('reflexes this session: the hooks and this session feed, each once; another session counts nothing', () => {
    expect(firedThisSession([fired('codegen-after-schema')], rows(LINE.firedGen, LINE.firedLookup, LINE.otherControl))).toBe(2)
  })
})

describe('the spinner and the turn line (T-B2-06)', () => {
  test('a suffix only while something fired; counts worded as counts, never a saving', () => {
    expect(spinnerSuffix(0, false)).toBe('')
    expect(spinnerSuffix(2, false)).toBe(' · adage ◠ 2…')
    expect(turnSummary(0, 0, 0, false)).toBe('')
    expect(turnSummary(1, 0, 1, false)).toBe(' · adage: 1 step without a model request · 1 dead end headed off')
    // Over 72 columns the last count goes first (ux-design §10, cap 72).
    expect(turnSummary(1, 2, 1, false)).toBe(' · adage: 1 step without a model request · 2 test runs reused')
    expect(turnSummary(2, 0, 0, false)).toBe(' · adage: 2 steps without a model request')
    expect(turnSummary(2, 1, 0, false)).not.toMatch(BANNED)
  })
})

describe('the learn toast', () => {
  test('from this session only when its id is among the sessions; named by what the reflex tells the agent', () => {
    const say = (r: string) => (r === 'test-after-acme-edit' ? 'run make test after editing acme/**' : undefined)
    const line = (s?: string[]) => (JSON.parse(LINE.learnNew(1, s)) as { line: Record<string, unknown> }).line
    expect(learnToast(line([SESSION]), SESSION, false, say)).toBe('adage learned from this session\n◠ run make test after editing acme/** · new, from this session')
    expect(learnToast(line(['other']), SESSION, false, say)).toBe('adage learned from recent sessions\n◠ run make test after editing acme/** · new, from recent sessions')
    const strength = (JSON.parse(LINE.learnStrength(1)) as { line: Record<string, unknown> }).line
    expect(learnToast(strength, SESSION, false)).toBe('adage learned from recent sessions\n◠ codegen after schema · now a direct instruction')
  })

  test('no change worth one, no toast; a failed learn says so', () => {
    expect(learnToast((JSON.parse(LINE.learnEvidence(1)) as { line: Record<string, unknown> }).line, SESSION, false)).toBeUndefined()
    expect(learnToast({ kind: 'condition', changes: [] }, SESSION, false)).toBeUndefined()
    expect(learnToast((JSON.parse(LINE.learnFailed(1)) as { line: Record<string, unknown> }).line, SESSION, false)).toBe(
      'adage could not learn from this session (sandbox) · run adage doctor',
    )
  })

  test('a first conditioning counts reflexes and instincts', () => {
    const changes = [
      ...['a', 'b', 'c'].map(r => ({ change: 'new', route: r, track: 'acme/learned' })),
      ...['u1', 'u2'].map(r => ({ change: 'new', route: r, track: 'acme/instincts' })),
    ]
    expect(learnToast({ kind: 'condition', sessions_used: 23, changes }, SESSION, false)).toBe('adage learned from 23 sessions\n◠ 3 reflexes and 2 instincts are now active here')
  })
})

describe('/adage why', () => {
  test('with nothing fired, in either arm, the one sentence', () => {
    expect(whyText([], REFLEXES, false)).toBe('No reflex has spoken in this session yet.')
    const control = rows(LINE.controlPrompt, LINE.controlStep, LINE.controlJudged, LINE.otherControl)
    expect(whyText(control, REFLEXES, false)).toBe('No reflex has spoken in this session yet.')
  })

  test('the last directive: what it tells the agent, the text as read, its route and evidence, followed or not', () => {
    expect(whyText(rows(LINE.firedGen, LINE.followedGen), REFLEXES, false)).toBe(
      '◠ run make gen after a schema edit\n\nIn this repository, changes to schema/*.json must be followed by running:\n  make gen\n\nroute acme/learned/codegen-after-schema · 13 of 17 sessions ran it after the edit · followed',
    )
    expect(whyText(rows(LINE.firedGen), REFLEXES, false)).toEndWith('not followed yet')
  })
})

describe('the status card as text', () => {
  test('the header, the key column and next; read back as the tree draws it', () => {
    const card = cardOf(CARD)
    if (card === undefined) throw new Error('card')
    const text = cardText(card)
    expect(text.split('\n')[0]).toBe('adage 0.1.0 · acme@315465905b6b · ● healthy')
    expect(text).toContain('reflexes    4 active: 3 conditioned, 1 instinct')
    expect(text).toContain('next        /adage watch · live rows, reflexes and health')
    const back = parseCardText(text)
    expect(back.header).toBe(card.header)
    expect(back.rows[0]).toEqual({ key: 'reflexes', text: '4 active: 3 conditioned, 1 instinct' })
    expect(back.rows.find(r => r.text.startsWith('over 30'))?.key).toBe('')
  })

  test('/adage savings: what the binary measured, as the card words it', () => {
    const card = cardOf(CARD)
    if (card === undefined) throw new Error('card')
    expect(cardText(card, SAVINGS_ROWS).split('\n')).toEqual([
      card.header,
      'activity    last 14 days: 9 sessions · 12 directives · 10 followed · 1 dead end headed off',
      'savings     measured: 2.1 fewer model requests per session (1.2–3.0)',
      `${' '.repeat(12)}over 30 treated and 10 control sessions`,
    ])
  })
})

describe('reading the binary', () => {
  test('the reflexes keep nothing that would name the arm', () => {
    expect(JSON.stringify(REFLEXES)).not.toMatch(/control|would_have|arms|unaided/)
    expect(REFLEXES[0]).toMatchObject({ id: 'codegen-after-schema', reference: 'acme/learned', support: 13, opportunities: 17, lowerBound: 0.53, verbs: ['strengthen', 'pin'], uplift: 'uplift +0.30' })
  })

  test('the checks and the one action a session may run', () => {
    const checks = checksOf(DOCTOR)
    expect(checks.map(checkAction)).toEqual([undefined, { kind: 'run', argv: ['condition'] }, { kind: 'copy', command: 'adage install claude-code --write' }, undefined])
  })

  test('lapsed grants, packs awaiting trust, the map address', () => {
    expect(lapsedOf({ lapsed: LAPSED })).toEqual([{ route: 'acme/learned/codegen-after-schema', changed: ['argv_templates'], reapprove: 'adage review delegate acme/learned codegen-after-schema --predict' }])
    expect(packsOf({ packs_awaiting_trust: [{ name: 'alice/acme', path: '.adage/packs/acme', command: 'adage add .adage/packs/acme --trust' }] })).toEqual([{ name: 'alice/acme', command: 'adage add .adage/packs/acme --trust' }])
    expect(mapUrl('map  http://127.0.0.1:43123/ · read-only')).toBe('http://localhost:43123/')
    expect(mapUrl('nothing')).toBeUndefined()
  })

  test('what waits for the person: a reflex to enable, a grant offered, a pack to trust', () => {
    const offered = reflexesOf({ routes: [{ ...GRAPH.routes[0], review: { verbs: [], reasons: [], offers: [{ kind: 'predict', command: 'adage review delegate acme/learned codegen-after-schema --predict' }] } }] })
    const w = waiting([...REFLEXES, ...offered], [{ name: 'alice/acme', command: 'adage add x --trust' }])
    expect(w.map(x => x.kind)).toEqual(['enable', 'allow', 'trust'])
    for (const x of w) expect(x.text).not.toMatch(BANNED)
  })
})

describe('the Live rows', () => {
  test('the filter keeps matching rows, newest first; the quiet switch hides rows with nothing decided', () => {
    const r = rows(LINE.start, LINE.firedGen, LINE.firedLookup)
    expect(visibleRows(r, 'lookup', false, false).map(x => x.seq)).toEqual([3])
    expect(visibleRows(r, '', false, false).map(x => x.seq)).toEqual([3, 2, 1])
    expect(visibleRows(r, '', true, false).map(x => x.seq)).toEqual([3, 2])
  })

  test('a noticed sequence (plan D2) reads as noticed, in both arms', () => {
    const [row] = rows(JSON.stringify({ kind: 'decision', cursor: 'c', ts: 0, session: SESSION, harness: 'claude', event: 'tools', acts: [{ act: 'noticed', candidate: 'make test → make lint', support: 3, s_min: 5, review: false }], texts: [] }))
    expect(row?.acts).toEqual([{ act: 'noticed', candidate: 'make test → make lint' }])
  })

  test('the inspect pane: the directive as the agent read it, and its evidence', () => {
    const [row] = rows(LINE.firedGen)
    if (row === undefined) throw new Error('row')
    const text = inspectText(row, REFLEXES)
    expect(text).toContain('### run make gen after a schema edit')
    expect(text).toContain('must be followed by running')
    expect(text).toContain('_13 of 17 sessions ran it after the edit_')
  })

  test('versions: a newer harness, and not', () => {
    expect(newer('2.1.300', '2.1.288')).toBe(true)
    expect(newer('2.1.288', '2.1.288')).toBe(false)
    expect(newer('2.1.288-dev.20260920', '2.1.288')).toBe(false)
    expect(newer('2.2.0', '2.1.288')).toBe(true)
    expect(versionLine('2.1.288', '2.1.288', false)).toBe('')
  })
})
