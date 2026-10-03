// The receipt as the binary words it (receipt.ts; plan C3; T-C3-04's readers): the dry run's plan,
// the final answer, the card the row draws, and the short id.
import { describe, expect, test } from 'claude-code/testing'

import { ANSWERED, FRESH, RECEIPT_CARD, RECEIPT_NOTE, RECEIPT_RESULT, STALE, VENV } from './fixtures/kit'
import { answeredMark, receiptAnswerOf, receiptPlanOf, shortId } from './receipt'

describe("T-C3-04 · the binary's answers, read", () => {
  test('receiptPlanOf: fresh carries the note and the result; stale only the command and the run', () => {
    expect(receiptPlanOf(FRESH)).toEqual(FRESH)
    expect(receiptPlanOf(STALE)).toEqual({ ...STALE, note: '', result: '' })
    expect(receiptPlanOf({ ...STALE, note: RECEIPT_NOTE, result: RECEIPT_RESULT })).toEqual({ ...STALE, note: '', result: '' })
    expect(receiptPlanOf({ fresh: 'yes' })).toBeUndefined()
    expect(receiptPlanOf(undefined)).toBeUndefined()
    expect(receiptPlanOf({ fresh: true })).toEqual({ fresh: true, covering: '', note: '', result: '', operation: '' })
  })

  test('receiptAnswerOf and the answered mark: the card as the row draws it, and more as a last name', () => {
    expect(receiptAnswerOf(ANSWERED)).toEqual({ result: RECEIPT_RESULT, note: RECEIPT_NOTE, covering: VENV, card: { operation: '3f9c1a2b7d4e', ranAt: '2026-10-03T09:00:00Z', ago: '4 min ago', summary: RECEIPT_CARD.summary, tests: RECEIPT_CARD.tests, more: 0 } })
    expect(receiptAnswerOf({ receipt: { result: ' ' } })).toBeUndefined()
    expect(receiptAnswerOf({})).toBeUndefined()
    expect(receiptAnswerOf({ receipt: { result: 'r' } })).toEqual({ result: 'r', note: '', covering: '', card: undefined })
    const a = receiptAnswerOf({ receipt: { ...ANSWERED.receipt, card: { ...RECEIPT_CARD, more: 3, tests: ['a', 7] } } })
    expect(answeredMark('t1', a!)).toEqual({ id: 't1', kind: 'answered', receipt: '3f9c', summary: RECEIPT_CARD.summary, tests: ['a', 'and 3 more'], ago: '4 min ago', command: VENV })
    expect(answeredMark('t1', receiptAnswerOf({ receipt: { result: 'r' } })!)).toBeUndefined()
    expect(answeredMark('t1', receiptAnswerOf({ receipt: { result: 'r', card: { operation: '' } } })!)).toBeUndefined()
    expect(shortId('3f9c1a2b7d4e')).toBe('3f9c')
  })
})
