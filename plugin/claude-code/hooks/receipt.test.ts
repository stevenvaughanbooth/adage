// The receipt as the binary words it (receipt.ts; plan C3; T-C3-04's readers): the dry run's plan,
// the final answer, the card the row draws, and the short id. Every answer is the wire's
// (docs/claude-mod/impl/wire.md), a case the files do not hold made from one that they do.
import { describe, expect, test } from 'claude-code/testing'

import { wire, wireWith } from './fixtures/kit'
import { answeredMark, receiptAnswerOf, receiptPlanOf, shortId } from './receipt'

/** The plan's receipt in a wire file: fresh in plan.json, not fresh in plan-advice.json. */
const planned = (name: 'plan' | 'plan-advice'): Record<string, unknown> => {
  const r = (wire(name).plan as Record<string, unknown> | undefined)?.receipt
  if (typeof r !== 'object' || r === null) throw new Error(`${name}.json holds no receipt`)
  return r as Record<string, unknown>
}

describe("T-C3-04 · the binary's answers, read", () => {
  test('receiptPlanOf: fresh carries the note and the result; stale only the command and the run', () => {
    const fresh = planned('plan')
    const stale = planned('plan-advice')
    expect(receiptPlanOf(fresh)).toEqual(fresh)
    expect(receiptPlanOf(stale)).toEqual({ ...stale, note: '', result: '' })
    expect(receiptPlanOf({ ...stale, note: fresh.note, result: fresh.result })).toEqual({ ...stale, note: '', result: '' })
    expect(receiptPlanOf({ fresh: 'yes' })).toBeUndefined()
    expect(receiptPlanOf(undefined)).toBeUndefined()
    expect(receiptPlanOf({ fresh: true })).toEqual({ fresh: true, covering: '', note: '', result: '', operation: '' })
  })

  test('receiptAnswerOf reads receipt.json: the result, the note, the command and the card, every field there', () => {
    const answer = wire('receipt')
    const r = answer.receipt as Record<string, unknown>
    const card = r.card as Record<string, unknown>
    const read = receiptAnswerOf(answer)
    expect(read).toEqual({ result: r.result, note: r.note, covering: r.covering, card: { operation: card.operation, ranAt: card.ran_at, ago: card.ago, summary: card.summary, tests: card.tests, more: card.more } })
    // A field the Rust side renames reads as empty (or 0) and fails here.
    for (const v of [read?.result, read?.note, read?.covering, ...Object.values(read?.card ?? {})]) expect(v === undefined || v === '' || v === 0).toBe(false)
    expect(read?.card?.tests.length).toBeGreaterThan(0)
  })

  test('receiptAnswerOf reads receipt-without-card.json: the answer stands, and no card is drawn', () => {
    const answer = wire('receipt-without-card')
    const r = answer.receipt as Record<string, unknown>
    expect(r.card).toBeNull()
    const read = receiptAnswerOf(answer)
    expect(read).toEqual({ result: r.result, note: r.note, covering: r.covering, card: undefined })
    expect(answeredMark('t1', read!)).toBeUndefined()
  })

  test('receiptAnswerOf and the answered mark: the card as the row draws it, and more as a last name', () => {
    const r = receiptAnswerOf(wire('receipt'))!
    expect(receiptAnswerOf(wireWith('receipt', { result: ' ' }))).toBeUndefined()
    expect(receiptAnswerOf({})).toBeUndefined()
    expect(receiptAnswerOf(wireWith('receipt', null))).toBeUndefined()
    expect(receiptAnswerOf(wireWith('receipt-without-card', { note: undefined, covering: undefined, card: undefined }))).toMatchObject({ note: '', covering: '', card: undefined })
    const a = receiptAnswerOf(wireWith('receipt', { card: { more: 3, tests: ['a', 7] } }))
    expect(answeredMark('t1', a!)).toEqual({ id: 't1', kind: 'answered', receipt: shortId(r.card!.operation), summary: r.card!.summary, tests: ['a', 'and 3 more'], ago: r.card!.ago, command: r.covering })
    expect(answeredMark('t1', receiptAnswerOf(wireWith('receipt', { card: { more: 0 } }))!)?.tests).toEqual(r.card!.tests)
    expect(answeredMark('t1', receiptAnswerOf(wireWith('receipt', { card: { operation: '' } }))!)).toBeUndefined()
    expect(shortId('3f9c1a2b7d4e')).toBe('3f9c')
  })
})
