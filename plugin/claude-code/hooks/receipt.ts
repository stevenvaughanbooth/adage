// Pure: the receipt as the binary words it (plan C3; docs/claude-mod/architecture.md §3.3). The dry
// run's `receipt` says whether a run Adage already made covers this call on identical inputs; the
// final answer (`verdict: { answered: "receipt" }`) carries the result the call is answered with,
// the `<adage-receipt>` note the model reads after it, and the card the row draws. The binary
// decides whether a receipt answers a call; this file only checks the shapes, so anything
// malformed is "no receipt" and the call runs.
import type { Mark } from './plugin'

/**
 * The dry run's `receipt`: `note` and `result` only when `fresh` (and `result` only where a call
 * can be answered); `operation` (the run's id) fresh or not, for the line beside a run that happens
 * anyway.
 */
export type ReceiptPlan = { fresh: boolean; covering: string; note: string; result: string; operation: string }

/** The receipt card's data, as the final answer's `card` carries it. */
export type ReceiptCard = { operation: string; ranAt: string; ago: string; summary: string; tests: string[]; more: number }

/** The final answer: what the call is answered with. */
export type ReceiptAnswer = { result: string; note: string; covering: string; card: ReceiptCard | undefined }

const isRecord = (v: unknown): v is Record<string, unknown> => v !== null && typeof v === 'object' && !Array.isArray(v)
const str = (v: unknown): string => (typeof v === 'string' ? v : '')

/** The plan's `receipt`, or undefined when there is none (or it is malformed). */
export function receiptPlanOf(v: unknown): ReceiptPlan | undefined {
  if (!isRecord(v) || typeof v.fresh !== 'boolean') return undefined
  return { fresh: v.fresh, covering: str(v.covering), note: v.fresh ? str(v.note) : '', result: v.fresh ? str(v.result) : '', operation: str(v.operation) }
}

function cardOf(v: unknown): ReceiptCard | undefined {
  if (!isRecord(v) || typeof v.operation !== 'string' || v.operation === '') return undefined
  const tests = Array.isArray(v.tests) ? v.tests.filter((t): t is string => typeof t === 'string') : []
  const more = typeof v.more === 'number' && Number.isFinite(v.more) && v.more > 0 ? Math.floor(v.more) : 0
  return { operation: v.operation, ranAt: str(v.ran_at), ago: str(v.ago), summary: str(v.summary), tests, more }
}

/** The final answer's `receipt`, or undefined: no longer fresh, already decided, the control arm. */
export function receiptAnswerOf(answer: unknown): ReceiptAnswer | undefined {
  if (!isRecord(answer) || !isRecord(answer.receipt)) return undefined
  const r = answer.receipt
  if (typeof r.result !== 'string' || r.result.trim() === '') return undefined
  return { result: r.result, note: str(r.note), covering: str(r.covering), card: cardOf(r.card) }
}

/** The receipt's short id as the rows name it (`<adage-receipt operation="…">`'s first four). */
export const shortId = (operation: string): string => operation.slice(0, 4)

/**
 * The `answered` mark the `ToolResult` row draws the receipt card from (`result.receipt`): the
 * operation, the card's summary, the tests it names and how long ago it ran, and the command that
 * ran. None without a card: the row then shows the result as the engine draws any tool output.
 */
export function answeredMark(toolUseId: string, a: ReceiptAnswer): Extract<Mark, { kind: 'answered' }> | undefined {
  if (a.card === undefined) return undefined
  const tests = a.card.more > 0 ? [...a.card.tests, `and ${a.card.more} more`] : a.card.tests
  return { id: toolUseId, kind: 'answered', receipt: shortId(a.card.operation), summary: a.card.summary, tests, ago: a.card.ago, command: a.covering }
}
