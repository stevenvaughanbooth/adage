// The lines Adage adds to the transcript and its toasts (view/lines.ts; ux-design §4.1–§4.5, §10):
// the opening line, the lines under a prompt, the markers, the receipt's card and line, the
// spinner suffix, the turn line, the learn and lapse toasts.
import { describe, expect, test } from 'claude-code/testing'

import { BANNED, GRAPH, LAPSED, LINE, SESSION, rowsOf as rows } from '../fixtures/kit'
import type { Fired, Mark } from '../plugin'
import { lapsedOf, reflexesOf } from '../reads'
import { lapseToast, learnToast, markText, openNotice, promptLines, receiptBeside, receiptCard, receiptOf, rewriteOf, spinnerSuffix, turnSummary } from './lines'
import { cols } from './text'

const REFLEXES = reflexesOf(GRAPH)
const fired = (route: string, prompt = 1): Fired => ({ event: 'UserPromptSubmit', route, track: 'acme/learned@1', kind: 'run', strength: 'fire', prompt, promptText: 'fix W-2042' })

describe('T-B2-01 · the opening line (open.notice)', () => {
  test("the binary's notice, its prefix stripped, with the glyph; another notice is not Adage's", () => {
    expect(openNotice('adage: 3 reflexes and 4 instincts active in this repo; it keeps learning as you work. Nothing runs without you.', false)).toBe(
      '◠ adage · 3 reflexes and 4 instincts active in this repo; it keeps learning as you work. Nothing runs without you.',
    )
    expect(openNotice('adage-exec: the adage plugin now includes execution', true)).toBe('~ adage - the adage plugin now includes execution')
    expect(openNotice('Using claude-x from settings', false)).toBeUndefined()
    expect(openNotice('adage: ', false)).toBeUndefined()
  })
})

describe('T-B2-02, T-B2-03 · the line under a prompt (prompt.fired)', () => {
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

describe('T-B2-04, T-B2-05 · the markers under a tool row', () => {
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

describe('T-B2-06 · the spinner and the turn line', () => {
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

describe('T-D5-02 · the learn toast', () => {
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

describe('T-B2-08 · the lapse toast', () => {
  test('a lapsed grant names what changed and where it is reviewed, in both glyph sets', () => {
    const [l] = lapsedOf({ lapsed: LAPSED })
    if (l === undefined) throw new Error('lapse')
    expect(lapseToast(l, false)).toBe('adage\nacme/learned/codegen-after-schema is no longer allowed: argv_templates changed · /adage review')
    expect(lapseToast({ ...l, changed: [] }, true)).toBe('adage\nacme/learned/codegen-after-schema is no longer allowed: it changed - /adage review')
  })
})
